// Copyright 2026 Nowledge
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Retain completed header, label, property and spill-reference boundaries.
//! Individual inline value encoding uses the shared codec and rolls back its
//! unfinished property only; it does not yet retain a nested value cursor.
use super::*;

#[derive(Default)]
pub(super) struct Progress {
    pub(super) output: Option<CheckpointBytes>,
    phase: u8,
    label: Option<LabelId>,
    property: usize,
    value_bytes: Option<usize>,
    spill_id: Option<u64>,
    finished: bool,
}

pub(super) struct Source<'a> {
    pub(super) header: &'a [u8],
    pub(super) labels: Option<&'a BTreeSet<LabelId>>,
    pub(super) properties: &'a BTreeMap<String, Value>,
}

pub(super) fn encode(
    plan: &mut Planning,
    config: CanonicalSegmentConfig,
    source: Source<'_>,
    mut spills: Option<&mut PropertySpillWriter>,
    dictionary: &PropertyKeyDictionary,
    work: &CheckpointWorkContext,
) -> Result<CheckpointBytes, CanonicalSegmentError> {
    let length = plan.length;
    let Source {
        header,
        labels,
        properties,
    } = source;
    let state = &mut plan.body;
    if state.finished {
        return Err(CanonicalSegmentError::Corrupt(
            "record body already consumed".into(),
        ));
    }
    if state.output.is_none() {
        state.output = Some(allocate(length, config, work)?);
        work.checkpoint()?;
    }
    loop {
        work.checkpoint()?;
        let output = state.output.as_mut().expect("retained record output");
        match state.phase {
            0 => {
                output.append_block_retained(header, work)?;
                state.phase = 1;
            }
            1 => {
                let label = labels.and_then(|labels| match state.label {
                    Some(previous) => labels
                        .range((
                            std::ops::Bound::Excluded(previous),
                            std::ops::Bound::Unbounded,
                        ))
                        .next()
                        .copied(),
                    None => labels.first().copied(),
                });
                if let Some(label) = label {
                    output.append_block_retained(&label.0.to_le_bytes(), work)?;
                    state.label = Some(label);
                } else {
                    state.phase = 2;
                }
            }
            2 => {
                output.append_block_retained(
                    &u32_len(properties.len(), "property map")?.to_le_bytes(),
                    work,
                )?;
                state.phase = 3;
            }
            3 => {
                if state.property == plan.ids.len() {
                    state.phase = 7;
                    continue;
                }
                output.append_block_retained(&plan.ids[state.property].to_le_bytes(), work)?;
                state.phase = 4;
            }
            4 => {
                let checkpoint = dictionary
                    .checkpoint
                    .as_ref()
                    .expect("completed checkpoint dictionary");
                let key = checkpoint
                    .keys
                    .get(plan.ids[state.property] as usize)
                    .ok_or_else(|| {
                        CanonicalSegmentError::Corrupt("completed record key disappeared".into())
                    })?;
                let value = properties.get(key).ok_or_else(|| {
                    CanonicalSegmentError::Corrupt("captured record value disappeared".into())
                })?;
                let value_bytes = match state.value_bytes {
                    Some(bytes) => bytes,
                    None => {
                        let bytes = value_size(value, work)?;
                        state.value_bytes = Some(bytes);
                        bytes
                    }
                };
                if let Some(writer) = spills
                    .as_deref_mut()
                    .filter(|writer| writer.should_spill(value_bytes))
                {
                    let id = writer.next_checkpoint_spill_id();
                    let result = if writer.has_pending_checkpoint_push() {
                        writer.resume_checkpoint_push(work)
                    } else {
                        writer.push_checkpoint(encode_value(value, work)?)
                    };
                    match result {
                        Ok(id) => {
                            state.spill_id = Some(id);
                            state.phase = 5;
                        }
                        Err(error) => {
                            if writer.next_checkpoint_spill_id() > id {
                                state.spill_id = Some(id);
                                state.phase = 5;
                            }
                            return Err(error.into());
                        }
                    }
                } else {
                    let start = output.len();
                    if let Err(error) = value::append(value, output, work) {
                        output.truncate(start);
                        return Err(error);
                    }
                    state.phase = 6;
                }
            }
            5 => {
                let mut reference = [0u8; 9];
                reference[0] = 7;
                reference[1..]
                    .copy_from_slice(&state.spill_id.expect("completed spill id").to_le_bytes());
                output.append_block_retained(&reference, work)?;
                state.phase = 6;
            }
            6 => {
                state.property += 1;
                state.value_bytes = None;
                state.spill_id = None;
                state.phase = 3;
            }
            7 => {
                work.checkpoint()?;
                if output.len() as u64 != length {
                    return Err(CanonicalSegmentError::Corrupt(
                        "retained record wire length changed".into(),
                    ));
                }
                state.finished = true;
                return Ok(state.output.take().expect("complete record output"));
            }
            _ => unreachable!("private record body phase"),
        }
    }
}

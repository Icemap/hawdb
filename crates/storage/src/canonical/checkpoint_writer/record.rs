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

//! Admit the complete canonical record before encoding. Inline values stream
//! directly into that owning buffer; spilled values retain their own inventory.

use super::*;
use crate::background::CheckpointBytes;

#[cfg(test)]
mod tests;

pub(in crate::canonical) fn node_with_preparation(
    node: &NodeRecord,
    spills: Option<&mut PropertySpillWriter>,
    dictionary: &mut PropertyKeyDictionary,
    config: CanonicalSegmentConfig,
    preparation: &mut Option<Planning>,
) -> Result<CheckpointBytes, CanonicalSegmentError> {
    let work = dictionary
        .checkpoint_work()
        .expect("checkpoint dictionary")
        .clone();
    let count = u32_len(node.labels.len(), "node labels")?;
    let prefix = 4u64 + u64::from(count) * 4;
    length(
        prefix,
        &node.properties,
        spills.as_deref(),
        dictionary,
        &work,
        preparation,
    )?;
    let plan = preparation.as_mut().expect("completed record plan");
    body::encode(
        plan,
        config,
        body::Source {
            header: &count.to_le_bytes(),
            labels: Some(&node.labels),
            properties: &node.properties,
        },
        spills,
        dictionary,
        &work,
    )
}

pub(in crate::canonical) fn relationship_with_preparation(
    relationship: &RelRecord,
    spills: Option<&mut PropertySpillWriter>,
    dictionary: &mut PropertyKeyDictionary,
    config: CanonicalSegmentConfig,
    preparation: &mut Option<Planning>,
) -> Result<CheckpointBytes, CanonicalSegmentError> {
    let work = dictionary
        .checkpoint_work()
        .expect("checkpoint dictionary")
        .clone();
    length(
        20,
        &relationship.properties,
        spills.as_deref(),
        dictionary,
        &work,
        preparation,
    )?;
    let mut header = [0u8; 20];
    header[..8].copy_from_slice(&relationship.source.0.to_le_bytes());
    header[8..16].copy_from_slice(&relationship.target.0.to_le_bytes());
    header[16..].copy_from_slice(&relationship.rel_type.0.to_le_bytes());
    let plan = preparation.as_mut().expect("completed record plan");
    body::encode(
        plan,
        config,
        body::Source {
            header: &header,
            labels: None,
            properties: &relationship.properties,
        },
        spills,
        dictionary,
        &work,
    )
}

pub(in crate::canonical) struct Planning {
    source: usize,
    property_count: usize,
    prefix: u64,
    length: u64,
    current_id: Option<u32>,
    previous_property: Option<crate::background::CheckpointText>,
    current_property: Option<crate::background::CheckpointText>,
    ids: Vec<u32>,
    complete: bool,
    body: body::Progress,
    allocation: crate::background::CheckpointDecodeContext,
}

fn length(
    prefix: u64,
    properties: &BTreeMap<String, Value>,
    spills: Option<&PropertySpillWriter>,
    dictionary: &mut PropertyKeyDictionary,
    work: &CheckpointWorkContext,
    preparation: &mut Option<Planning>,
) -> Result<u64, CanonicalSegmentError> {
    u32_len(properties.len(), "property map")?;
    let source = properties as *const BTreeMap<String, Value> as usize;
    let state = preparation.get_or_insert_with(|| Planning {
        source,
        property_count: properties.len(),
        prefix,
        length: prefix + 4,
        current_id: None,
        previous_property: None,
        current_property: None,
        ids: Vec::new(),
        complete: false,
        body: body::Progress::default(),
        allocation: crate::background::CheckpointDecodeContext {
            work: work.clone(),
            memory: RefCell::default(),
        },
    });
    if state.source != source || state.property_count != properties.len() || state.prefix != prefix
    {
        return Err(CanonicalSegmentError::Corrupt(
            "retained record property source changed".into(),
        ));
    }
    work.classify(|scoped| {
        state.allocation.work = scoped.clone();
        if state.ids.capacity() == 0 && !properties.is_empty() {
            let unit = scoped.start_unit()?;
            state.ids = state
                .allocation
                .vector_capacity(properties.len())
                .map_err(source_error)?;
            unit.finish();
        }
        while state.ids.len() < properties.len() {
            scoped.checkpoint()?;
            let selected = match state.previous_property.as_deref() {
                Some(previous) => properties
                    .range::<str, _>((
                        std::ops::Bound::Excluded(previous),
                        std::ops::Bound::Unbounded,
                    ))
                    .next(),
                None => properties.first_key_value(),
            };
            let (key, value) = selected.ok_or_else(|| {
                CanonicalSegmentError::Corrupt("captured record property disappeared".into())
            })?;
            let id = match state.current_id {
                Some(id) => id,
                None => {
                    let id = dictionary.intern(key)?;
                    state.current_id = Some(id);
                    id
                }
            };
            let property = state
                .current_property
                .get_or_insert_with(crate::background::CheckpointText::new);
            while property.len() < key.len() {
                let start = property.len();
                let mut end = (start + 64 * 1024).min(key.len());
                while !key.is_char_boundary(end) {
                    end -= 1;
                }
                property
                    .append(&key[start..end], scoped)
                    .map_err(source_error)?;
            }
            let value_bytes = value_size(value, scoped)?;
            let wire = if spills.is_some_and(|spills| spills.should_spill(value_bytes)) {
                9
            } else {
                value_bytes as u64
            };
            let length = state
                .length
                .checked_add(4)
                .and_then(|length| length.checked_add(wire))
                .ok_or_else(|| {
                    CanonicalSegmentError::Work(scoped.record_failure(
                        CheckpointWorkError::Allocation {
                            bytes: u64::MAX,
                            reason: "canonical record length overflows u64".into(),
                        },
                    ))
                })?;
            let previous = state.ids.len();
            let result = state
                .allocation
                .push(&mut state.ids, id)
                .map_err(source_error);
            // An append can complete before its cancellation is returned. Commit
            // its logical length once, so the next ordered lookup advances.
            if state.ids.len() > previous {
                state.length = length;
                state.current_id = None;
                state.previous_property = state.current_property.take();
            }
            result?;
        }
        scoped.checkpoint()?;
        state.complete = true;
        Ok(state.length)
    })
    .map_err(|error| match error {
        crate::background::CheckpointOperationError::Work(error) => {
            CanonicalSegmentError::Work(error)
        }
        crate::background::CheckpointOperationError::Operation(error) => error,
    })
}

fn source_error(error: hawdb_core::HawDBError) -> CanonicalSegmentError {
    CanonicalSegmentError::Corrupt(error.to_string())
}

fn allocate(
    length: u64,
    config: CanonicalSegmentConfig,
    work: &CheckpointWorkContext,
) -> Result<CheckpointBytes, CanonicalSegmentError> {
    let record_bytes = length.saturating_add(12);
    if record_bytes > config.max_record_bytes.get() {
        return Err(CanonicalSegmentError::RecordTooLarge {
            record_bytes,
            max_bytes: config.max_record_bytes.get(),
        });
    }
    let capacity = usize::try_from(length).map_err(|_| {
        CanonicalSegmentError::Work(work.record_failure(CheckpointWorkError::Allocation {
            bytes: length,
            reason: "canonical record capacity exceeds usize".into(),
        }))
    })?;
    CheckpointBytes::new_retained(capacity, work).map_err(CanonicalSegmentError::Work)
}

fn value_size(value: &Value, work: &CheckpointWorkContext) -> Result<usize, CanonicalSegmentError> {
    let length = encoded_value_len_with_work(value, 1, Some(work))?;
    usize::try_from(length).map_err(|_| {
        CanonicalSegmentError::Work(work.record_failure(CheckpointWorkError::Allocation {
            bytes: length,
            reason: "canonical value capacity exceeds usize".into(),
        }))
    })
}

mod body;

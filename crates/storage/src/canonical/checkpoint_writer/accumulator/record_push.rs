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

//! Retain actual record append and capacity-copy positions across safe denials.
use super::*;
use crate::background::{
    CheckpointAllocationLookup, CheckpointAllocationToken, CheckpointVectorPush,
};

pub(super) struct Progress {
    id: u64,
    payload_len: u32,
    endpoints: Option<(u64, u64)>,
    count: u32,
    required: usize,
    phase: usize,
    offset: usize,
    capacity: Option<Capacity>,
    source: Option<CheckpointVectorPush<u64>>,
    target: Option<CheckpointVectorPush<u64>>,
}

struct Capacity {
    replacement: Option<Vec<u8>>,
    previous: Option<CheckpointAllocationToken>,
    located: bool,
    lookup: Option<CheckpointAllocationLookup>,
}

pub(super) fn push(
    output: &mut Accumulator,
    id: u64,
    payload: &[u8],
    endpoints: Option<(u64, u64)>,
    work: &CheckpointWorkContext,
) -> Result<(), CanonicalSegmentError> {
    work.classify(|scoped| {
        let context = CheckpointDecodeContext {
            work: scoped.clone(),
            memory: RefCell::new(std::mem::take(&mut output.memory)),
        };
        let result = resume(output, id, payload, endpoints, &context);
        output.memory = context.memory.into_inner();
        result
    })
    .map_err(|error| match error {
        CheckpointOperationError::Work(error) => CanonicalSegmentError::Work(error),
        CheckpointOperationError::Operation(error) => error,
    })
}

fn resume(
    output: &mut Accumulator,
    id: u64,
    payload: &[u8],
    endpoints: Option<(u64, u64)>,
    context: &CheckpointDecodeContext,
) -> Result<(), CanonicalSegmentError> {
    let segment = &mut output.segment;
    if let Some(progress) = &output.pending_push {
        if progress.id != id
            || progress.payload_len as usize != payload.len()
            || progress.endpoints != endpoints
        {
            return Err(CanonicalSegmentError::Corrupt(
                "retained canonical record identity changed".into(),
            ));
        }
    } else {
        let unit = context.start_unit()?;
        let record_bytes = 12u64.saturating_add(payload.len() as u64);
        if record_bytes > segment.config.max_record_bytes.get() {
            return Err(CanonicalSegmentError::RecordTooLarge {
                record_bytes,
                max_bytes: segment.config.max_record_bytes.get(),
            });
        }
        let payload_len = u32_len(payload.len(), "canonical record")?;
        if id < segment.max_record_id && segment.record_count > 0 {
            return Err(CanonicalSegmentError::Corrupt(
                "canonical records must be ordered by id".into(),
            ));
        }
        let count = segment.record_count.checked_add(1).ok_or_else(|| {
            CanonicalSegmentError::Corrupt("canonical segment record count overflow".into())
        })?;
        let required = segment
            .records
            .len()
            .checked_add(12)
            .and_then(|bytes| bytes.checked_add(payload.len()))
            .ok_or_else(|| {
                CanonicalSegmentError::Corrupt("canonical segment byte count overflow".into())
            })?;
        output.pending_push = Some(Progress {
            id,
            payload_len,
            endpoints,
            count,
            required,
            phase: 0,
            offset: 0,
            capacity: (required > segment.records.capacity()).then_some(Capacity {
                replacement: None,
                previous: None,
                located: false,
                lookup: None,
            }),
            source: endpoints.map(|(source, _)| CheckpointVectorPush::new(source)),
            target: endpoints.map(|(_, target)| CheckpointVectorPush::new(target)),
        });
        unit.finish();
    }
    let progress = output.pending_push.as_mut().expect("validated record");
    if progress.phase == 0 {
        if let Some(capacity) = &mut progress.capacity {
            if !capacity.located {
                capacity.previous = if segment.records.capacity() == 0 {
                    None
                } else {
                    Some(
                        context
                            .find_retained(segment.records.as_ptr() as usize, &mut capacity.lookup)
                            .map_err(source)?,
                    )
                };
                capacity.located = true;
            }
            if capacity.replacement.is_none() {
                let unit = context.start_unit()?;
                let count = segment
                    .records
                    .capacity()
                    .saturating_mul(2)
                    .max(progress.required);
                capacity.replacement = Some(context.vector_capacity(count).map_err(source)?);
                unit.finish();
            }
            let replacement = capacity.replacement.as_mut().expect("admitted replacement");
            while replacement.len() < segment.records.len() {
                let unit = context.start_unit()?;
                let start = replacement.len();
                let end = start.saturating_add(64 * 1024).min(segment.records.len());
                replacement.extend_from_slice(&segment.records[start..end]);
                unit.finish();
            }
            let replacement = capacity.replacement.take().expect("complete replacement");
            drop(std::mem::replace(&mut segment.records, replacement));
            if let Some(previous) = capacity.previous.take() {
                previous.release_buffer();
            }
        }
        progress.capacity = None;
        progress.phase = 1;
    }
    let id_bytes = id.to_le_bytes();
    let length_bytes = progress.payload_len.to_le_bytes();
    for (phase, bytes) in [(1, &id_bytes[..]), (2, &length_bytes[..]), (3, payload)] {
        if progress.phase != phase {
            continue;
        }
        while progress.offset < bytes.len() {
            let unit = context.start_unit()?;
            let end = progress.offset.saturating_add(64 * 1024).min(bytes.len());
            segment
                .records
                .extend_from_slice(&bytes[progress.offset..end]);
            progress.offset = end;
            unit.finish();
        }
        progress.offset = 0;
        progress.phase += 1;
    }
    if progress.phase == 4 {
        context
            .resume_push(&mut segment.source_endpoint_keys, &mut progress.source)
            .map_err(source)?;
        progress.phase = 5;
    }
    if progress.phase == 5 {
        context
            .resume_push(&mut segment.target_endpoint_keys, &mut progress.target)
            .map_err(source)?;
        progress.phase = 6;
    }
    if progress.phase == 6 {
        let unit = context.start_unit()?;
        segment.min_record_id.get_or_insert(id);
        segment.max_record_id = id;
        segment.record_count = progress.count;
        progress.phase = 7;
        unit.finish();
    }
    context.checkpoint()?;
    output.pending_push = None;
    Ok(())
}

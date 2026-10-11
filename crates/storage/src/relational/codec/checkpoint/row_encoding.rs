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

//! The ordinary row grammar counts and writes one admitted output allocation.

use super::*;

struct Size<'a> {
    bytes: usize,
    work: &'a CheckpointWorkContext,
}

impl schema_encoding::Sink for Size<'_> {
    fn raw(&mut self, bytes: &[u8]) -> Result<(), RelationalError> {
        let unit = self.work.start_unit().map_err(work_error)?;
        self.bytes = self.bytes.checked_add(bytes.len()).ok_or_else(|| {
            work_error(self.work.record_failure(CheckpointWorkError::Allocation {
                bytes: u64::MAX,
                reason: "row payload size overflows usize".into(),
            }))
        })?;
        unit.finish();
        self.work.checkpoint().map_err(work_error)
    }
}

struct Output<'a> {
    bytes: CheckpointBytes,
    offset: usize,
    work: &'a CheckpointWorkContext,
}

impl schema_encoding::Sink for Output<'_> {
    fn raw(&mut self, bytes: &[u8]) -> Result<(), RelationalError> {
        for block in bytes.chunks(64 * 1024) {
            let unit = self.work.start_unit().map_err(work_error)?;
            let end = self.offset.checked_add(block.len()).ok_or_else(|| {
                RelationalError::Corruption("row payload offset overflows usize".into())
            })?;
            let target = self
                .bytes
                .as_mut_slice()
                .get_mut(self.offset..end)
                .ok_or_else(|| {
                    RelationalError::Corruption("row payload exceeds its counted size".into())
                })?;
            target.copy_from_slice(block);
            self.offset = end;
            unit.finish();
        }
        self.work.checkpoint().map_err(work_error)
    }
}

pub(super) fn encode(
    row: &RelationalRow,
    work: &CheckpointWorkContext,
) -> Result<CheckpointBytes, RelationalError> {
    let mut size = Size { bytes: 0, work };
    schema_encoding::row(&mut size, row)?;
    let mut output = Output {
        bytes: CheckpointBytes::zeroed(size.bytes, work).map_err(work_error)?,
        offset: 0,
        work,
    };
    schema_encoding::row(&mut output, row)?;
    if output.offset != size.bytes {
        return Err(RelationalError::Corruption(
            "row payload does not match its counted size".into(),
        ));
    }
    work.checkpoint().map_err(work_error)?;
    Ok(output.bytes)
}

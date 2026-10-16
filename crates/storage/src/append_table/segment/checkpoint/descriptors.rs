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

//! The descriptor array and every copied field retain their allocation owners.

use super::*;
use crate::append_table::checkpoint::memory::{copy_key, copy_string};
use crate::background::CheckpointAllocationOwner;
use hawdb_core::RuntimeMemoryPermit;

#[derive(Debug, Default)]
pub(in crate::append_table) struct CheckpointDescriptors {
    values: Vec<AppendSegmentBlockDescriptor>,
    fields: CheckpointAllocationOwner,
    capacity: Option<Box<dyn RuntimeMemoryPermit>>,
}

impl std::ops::Deref for CheckpointDescriptors {
    type Target = [AppendSegmentBlockDescriptor];

    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl PartialEq<Vec<AppendSegmentBlockDescriptor>> for CheckpointDescriptors {
    fn eq(&self, other: &Vec<AppendSegmentBlockDescriptor>) -> bool {
        self.values == *other
    }
}

impl CheckpointDescriptors {
    pub(super) fn copy_table(
        &mut self,
        value: &str,
        work: &CheckpointWorkContext,
    ) -> Result<String, AppendTableError> {
        copy_string(value, &mut self.fields, work)
    }

    pub(super) fn copy_key(
        &mut self,
        value: &RelationalKey,
        work: &CheckpointWorkContext,
    ) -> Result<RelationalKey, AppendTableError> {
        copy_key(value, &mut self.fields, work)
    }

    pub(super) fn push(
        &mut self,
        value: AppendSegmentBlockDescriptor,
        work: &CheckpointWorkContext,
    ) -> Result<(), AppendTableError> {
        if self.values.len() == self.values.capacity() {
            let capacity = self
                .values
                .len()
                .saturating_add(1)
                .max(self.values.capacity().saturating_mul(2));
            let bytes = capacity
                .checked_mul(std::mem::size_of::<AppendSegmentBlockDescriptor>())
                .ok_or_else(|| {
                    work_error(work.record_failure(
                        crate::background::CheckpointWorkError::Allocation {
                            bytes: u64::MAX,
                            reason: "append descriptor capacity overflows usize".into(),
                        },
                    ))
                })?;
            let memory = work.reserve_memory(bytes).map_err(work_error)?;
            let mut replacement = Vec::new();
            replacement.try_reserve_exact(capacity).map_err(|error| {
                work_error(work.record_failure(
                    crate::background::CheckpointWorkError::Allocation {
                        bytes: bytes as u64,
                        reason: error.to_string(),
                    },
                ))
            })?;
            if replacement.capacity() != capacity {
                return Err(work_error(work.record_failure(
                    crate::background::CheckpointWorkError::Allocation {
                        bytes: bytes as u64,
                        reason: "append descriptor capacity differs from admission".into(),
                    },
                )));
            }
            let mut previous = std::mem::take(&mut self.values).into_iter();
            while !previous.as_slice().is_empty() {
                let unit = work.start_unit().map_err(work_error)?;
                replacement.extend(previous.by_ref().take(1024));
                unit.finish();
            }
            drop(previous);
            self.values = replacement;
            self.capacity = memory;
        }
        let unit = work.start_unit().map_err(work_error)?;
        self.values.push(value);
        unit.finish();
        work.checkpoint().map_err(work_error)
    }
}

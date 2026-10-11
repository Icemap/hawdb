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

//! Temporary row values and overflow arrays keep independent backing owners.

use super::*;
use crate::append_table::checkpoint::memory::allocate_capacity;
use crate::background::{CheckpointAllocationOwner, CheckpointValues, CheckpointWorkError};
use crate::relational::overflow::CheckpointRelationalOverflow;
use std::borrow::Cow;

#[derive(Debug)]
pub(super) struct CheckpointExternalizedRow<'a> {
    // Destroy the shared row and its copied scalar payloads before the leases.
    pub(super) row: Cow<'a, RelationalRow>,
    pub(super) _allocations: CheckpointAllocationOwner,
}

impl std::ops::Deref for CheckpointExternalizedRow<'_> {
    type Target = RelationalRow;

    fn deref(&self) -> &Self::Target {
        &self.row
    }
}

pub(super) fn row_values(
    count: usize,
    allocations: &mut CheckpointAllocationOwner,
    work: &CheckpointWorkContext,
) -> Result<Vec<RelationalValue>, AppendTableError> {
    let bytes = count
        .checked_mul(std::mem::size_of::<RelationalValue>())
        // RelationalRow owns an Arc<Vec<RelationalValue>>. The pinned Arc
        // layout stores two reference counters alongside the Vec header.
        .and_then(|bytes| bytes.checked_add(std::mem::size_of::<Vec<RelationalValue>>()))
        .and_then(|bytes| {
            bytes.checked_add(2 * std::mem::size_of::<std::sync::atomic::AtomicUsize>())
        })
        .ok_or_else(|| {
            work_error(work.record_failure(CheckpointWorkError::Allocation {
                bytes: u64::MAX,
                reason: "externalized row capacity overflows usize".into(),
            }))
        })?;
    let unit = work.start_unit().map_err(work_error)?;
    allocations.reserve(bytes, work).map_err(work_error)?;
    let values = allocate_capacity(count, work)?;
    unit.finish();
    work.checkpoint().map_err(work_error)?;
    Ok(values)
}

#[derive(Debug, Default)]
pub(super) struct CheckpointExternalizedOverflows {
    // Inline-only rows allocate no container and need no extra lease.
    values: Option<CheckpointValues<CheckpointRelationalOverflow>>,
}

impl std::ops::Deref for CheckpointExternalizedOverflows {
    type Target = [CheckpointRelationalOverflow];

    fn deref(&self) -> &Self::Target {
        self.values.as_ref().map_or(&[], CheckpointValues::as_slice)
    }
}

impl CheckpointExternalizedOverflows {
    pub(super) fn new(
        count: usize,
        work: &CheckpointWorkContext,
    ) -> Result<Self, AppendTableError> {
        Ok(Self {
            values: Some(CheckpointValues::new(count, work).map_err(work_error)?),
        })
    }

    pub(super) fn push(
        &mut self,
        value: CheckpointRelationalOverflow,
        work: &CheckpointWorkContext,
    ) -> Result<(), AppendTableError> {
        self.values
            .as_mut()
            .expect("externalized overflow capacity was admitted")
            .push(value, work)
            .map_err(work_error)
    }
}

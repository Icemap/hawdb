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

//! Primitive writes into an allocation-owning checkpoint buffer. Geometric
//! growth admits the simultaneous old and new capacities before copying.

use super::*;
use crate::background::CheckpointBytes;

pub(super) struct CheckpointEncoder<'a> {
    bytes: CheckpointBytes,
    work: &'a CheckpointWorkContext,
}

impl<'a> CheckpointEncoder<'a> {
    pub(super) fn new(work: &'a CheckpointWorkContext) -> Result<Self, AppendTableError> {
        Ok(Self {
            bytes: CheckpointBytes::new(0, work).map_err(work_error)?,
            work,
        })
    }

    pub(super) fn len(&self) -> usize {
        self.bytes.len()
    }

    pub(super) fn u32(&mut self, value: u32) -> Result<(), AppendTableError> {
        self.raw(&value.to_le_bytes())
    }

    pub(super) fn u64(&mut self, value: u64) -> Result<(), AppendTableError> {
        self.raw(&value.to_le_bytes())
    }

    pub(super) fn raw(&mut self, value: &[u8]) -> Result<(), AppendTableError> {
        self.bytes
            .append_growing(value, None, self.work)
            .map_err(work_error)
    }

    pub(super) fn overwrite_u32(&mut self, offset: usize, value: u32) {
        self.bytes.as_mut_slice()[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    pub(super) fn finish(self) -> CheckpointBytes {
        self.bytes
    }
}

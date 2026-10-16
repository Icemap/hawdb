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

use super::*;
use crate::background::{CheckpointAllocationOwner, CheckpointWorkError};
use crate::relational::overflow::CheckpointRelationalOverflow;

#[derive(Debug, Default)]
pub(super) struct CheckpointOverflows {
    // Destroy the nodes and shared envelope handles before their coverage.
    values: BTreeMap<Sha256Digest, CheckpointRelationalOverflow>,
    admitted_nodes: usize,
    allocations: CheckpointAllocationOwner,
}

impl std::ops::Deref for CheckpointOverflows {
    type Target = BTreeMap<Sha256Digest, CheckpointRelationalOverflow>;

    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl CheckpointOverflows {
    pub(super) fn insert(
        &mut self,
        value: CheckpointRelationalOverflow,
        work: &CheckpointWorkContext,
    ) -> Result<Option<CheckpointRelationalOverflow>, AppendTableError> {
        let unit = work.start_unit().map_err(work_error)?;
        if !self.values.contains_key(&value.reference.digest) {
            let keys = self.values.len().checked_add(1).ok_or_else(|| {
                work_error(work.record_failure(CheckpointWorkError::Allocation {
                    bytes: u64::MAX,
                    reason: "append overflow map cardinality overflows usize".into(),
                }))
            })?;
            // Match the existing WAL/predicate map insertion bound. In pinned
            // Rust 1.97.1, completed non-root nodes hold >=5 keys; a splitting
            // node may briefly hold four. Cover the special root and one new
            // empty split node separately. This map never removes entries.
            // The shared COW node bound includes full internal-node capacity
            // and alignment gaps. Envelope bytes retain independent leases.
            let nodes = keys / 4 + 2;
            if nodes > self.admitted_nodes {
                let bytes = (nodes - self.admitted_nodes)
                    .checked_mul(crate::cow::checkpoint_tree_node_bytes::<
                        Sha256Digest,
                        CheckpointRelationalOverflow,
                    >())
                    .ok_or_else(|| {
                        work_error(work.record_failure(CheckpointWorkError::Allocation {
                            bytes: u64::MAX,
                            reason: "append overflow map node capacity overflows usize".into(),
                        }))
                    })?;
                self.allocations.reserve(bytes, work).map_err(work_error)?;
                self.admitted_nodes = nodes;
            }
        }
        let previous = self.values.insert(value.reference.digest, value);
        unit.finish();
        work.checkpoint().map_err(work_error)?;
        Ok(previous)
    }
}

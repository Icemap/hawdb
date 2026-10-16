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

//! Retain decoded allocation leases through moves and COW snapshot ownership.
//! The inventory is persistent: cloning a captured owner does not copy cells.
//! Dropping a long unshared inventory does not recurse or allocate scratch.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Allocation {
    buffer: Mutex<Option<Box<dyn RuntimeMemoryPermit>>>,
    address: AtomicUsize,
    next: Option<Arc<Self>>,
    _metadata: Option<Box<dyn RuntimeMemoryPermit>>,
}

#[derive(Default, Clone)]
pub(crate) struct CheckpointAllocationOwner {
    head: Option<Arc<Allocation>>,
}

impl std::fmt::Debug for CheckpointAllocationOwner {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CheckpointAllocationOwner")
            .field("has_retained_allocations", &self.head.is_some())
            .finish()
    }
}

pub(crate) struct CheckpointAllocationToken(Option<Arc<Allocation>>);

/// Keep the immutable inventory suffix visited by an unfinished buffer lookup.
/// Incoming allocations may extend the owner, but the original backing remains
/// owned and unchanged until its caller completes the capacity replacement.
pub(crate) struct CheckpointAllocationLookup {
    address: usize,
    current: Option<Arc<Allocation>>,
}

impl Drop for CheckpointAllocationLookup {
    fn drop(&mut self) {
        // The owner may close while the lookup retains its last suffix. Reuse
        // the inventory's iterative destruction instead of recursively dropping
        // the linked Arc chain through the cursor's last reference.
        drop(CheckpointAllocationOwner {
            head: self.current.take(),
        });
    }
}

impl CheckpointAllocationToken {
    pub(crate) fn address(&self, address: usize) {
        if let Some(node) = &self.0 {
            node.address.store(address, Ordering::Relaxed);
        }
    }

    /// The caller must destroy the previous backing allocation first.
    /// Its inventory cell stays admitted until the inventory's last owner.
    pub(crate) fn release_buffer(self) {
        if let Some(node) = &self.0 {
            let permit = node
                .buffer
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take();
            drop(permit);
        }
    }
}

impl CheckpointAllocationOwner {
    pub(crate) fn is_empty(&self) -> bool {
        self.head.is_none()
    }

    pub(crate) const METADATA_BYTES: usize = std::mem::size_of::<Allocation>()
        + 2 * std::mem::size_of::<std::sync::atomic::AtomicUsize>();

    /// The pinned Arc layout owns two reference counters. The governor also
    /// charges its concrete permit payload; allocator metadata/rounding and
    /// allocation latency remain platform assumptions.
    pub(crate) fn reserve(
        &mut self,
        bytes: usize,
        work: &CheckpointWorkContext,
    ) -> Result<CheckpointAllocationToken, CheckpointWorkError> {
        if bytes == 0 {
            return Ok(CheckpointAllocationToken(None));
        }
        let metadata = work.reserve_memory(Self::METADATA_BYTES)?;
        let buffer = work.reserve_memory(bytes)?;
        if metadata.is_none() && buffer.is_none() {
            return Ok(CheckpointAllocationToken(None));
        }
        let node = Arc::new(Allocation {
            buffer: Mutex::new(buffer),
            address: AtomicUsize::new(0),
            next: self.head.take(),
            _metadata: metadata,
        });
        self.head = Some(node.clone());
        Ok(CheckpointAllocationToken(Some(node)))
    }

    pub(crate) fn find(
        &self,
        address: usize,
        work: &CheckpointWorkContext,
    ) -> Result<CheckpointAllocationToken, CheckpointWorkError> {
        self.find_retained(address, &mut None, work)
    }

    pub(crate) fn find_retained(
        &self,
        address: usize,
        pending: &mut Option<CheckpointAllocationLookup>,
        work: &CheckpointWorkContext,
    ) -> Result<CheckpointAllocationToken, CheckpointWorkError> {
        let state = pending.get_or_insert_with(|| CheckpointAllocationLookup {
            address,
            current: self.head.clone(),
        });
        if state.address != address {
            return Err(work.record_failure(CheckpointWorkError::Allocation {
                bytes: 0,
                reason: "retained allocation lookup address changed".into(),
            }));
        }
        while let Some(node) = state.current.clone() {
            let unit = work.start_unit()?;
            let found = node.address.load(Ordering::Relaxed) == address;
            if found {
                *pending = None;
                unit.finish();
                return Ok(CheckpointAllocationToken(Some(node)));
            }
            state.current = node.next.clone();
            unit.finish();
        }
        *pending = None;
        Ok(CheckpointAllocationToken(None))
    }

    /// Coalesce one pair of retained, opaque capacity records after a
    /// successful mutation preflight. This is infallible and bounded: it
    /// allocates no memory, admits no work and never visits a shared suffix.
    /// Callers must no longer require individual backing-address lookups.
    pub(crate) fn coalesce_retained_head(&mut self) -> bool {
        let Some(head) = self.head.as_mut().and_then(Arc::get_mut) else {
            return false;
        };
        let Some(next) = head.next.as_mut().and_then(Arc::get_mut) else {
            return false;
        };
        let target = head.buffer.get_mut().unwrap_or_else(|p| p.into_inner());
        let source = next.buffer.get_mut().unwrap_or_else(|p| p.into_inner());
        match (target.as_mut(), source.take()) {
            (_, None) => {}
            (Some(target), Some(source_permit)) => {
                if let Err(source_permit) = target.merge(source_permit) {
                    *source = Some(source_permit);
                    return false;
                }
            }
            (None, Some(source_permit)) => *target = Some(source_permit),
        }
        let suffix = next.next.take();
        let retired = head.next.take();
        head.next = suffix;
        drop(CheckpointAllocationOwner { head: retired });
        true
    }

    /// Transfer buffers whose callers will no longer grow or look them up.
    /// Compatible leases are consolidated only in uniquely owned nodes. The
    /// surviving lease retains every backing byte; metadata is refunded only
    /// when its physical inventory node is destroyed. Partial consolidation
    /// after cancellation still owns all buffers and may be retried safely.
    pub(crate) fn append_retained(
        &mut self,
        incoming: &mut Self,
        work: &CheckpointWorkContext,
    ) -> Result<(), CheckpointWorkError> {
        while let Some(head) = incoming.head.as_mut().and_then(Arc::get_mut) {
            let Some(next) = head.next.as_mut().and_then(Arc::get_mut) else {
                break;
            };
            let unit = work.start_unit()?;
            let target = head.buffer.get_mut().unwrap_or_else(|p| p.into_inner());
            let source = next.buffer.get_mut().unwrap_or_else(|p| p.into_inner());
            let merged = match (target.as_mut(), source.take()) {
                (_, None) => true,
                (Some(target), Some(source_permit)) => match target.merge(source_permit) {
                    Ok(()) => true,
                    Err(source_permit) => {
                        *source = Some(source_permit);
                        false
                    }
                },
                (None, Some(source_permit)) => {
                    *target = Some(source_permit);
                    true
                }
            };
            if !merged {
                unit.finish_without_progress();
                break;
            }
            let suffix = next.next.take();
            // The redundant buffer permit was destroyed by merge. Destroy the
            // node before its metadata permit can refund inventory capacity.
            let retired = head.next.take();
            head.next = suffix;
            drop(CheckpointAllocationOwner { head: retired });
            unit.finish();
        }
        let unique_record = incoming
            .head
            .as_ref()
            .is_some_and(|head| head.next.is_none());
        if unique_record
            && let Some(target) = self.head.as_mut().and_then(Arc::get_mut)
            && let Some(source) = incoming.head.as_mut().and_then(Arc::get_mut)
        {
            let unit = work.start_unit()?;
            let target = target.buffer.get_mut().unwrap_or_else(|p| p.into_inner());
            let source = source.buffer.get_mut().unwrap_or_else(|p| p.into_inner());
            let merged = match (target.as_mut(), source.take()) {
                (_, None) => true,
                (Some(target), Some(source_permit)) => match target.merge(source_permit) {
                    Ok(()) => true,
                    Err(source_permit) => {
                        *source = Some(source_permit);
                        false
                    }
                },
                (None, Some(source_permit)) => {
                    *target = Some(source_permit);
                    true
                }
            };
            if merged {
                drop(CheckpointAllocationOwner {
                    head: incoming.head.take(),
                });
                unit.finish();
                return Ok(());
            }
            unit.finish_without_progress();
        }
        self.append(incoming, work)
    }

    /// Incoming decoded ownership is unique. Existing COW snapshots may share
    /// the target inventory. Change only the incoming tail after all checks;
    /// a cancellation leaves both owners intact.
    pub(crate) fn append(
        &mut self,
        incoming: &mut Self,
        work: &CheckpointWorkContext,
    ) -> Result<(), CheckpointWorkError> {
        let Some(mut current) = incoming.head.as_mut() else {
            return Ok(());
        };
        loop {
            let unit = work.start_unit()?;
            let node = Arc::get_mut(current).ok_or_else(|| {
                work.record_failure(CheckpointWorkError::Allocation {
                    bytes: 0,
                    reason: "decoded allocation transfer requires unique incoming ownership".into(),
                })
            })?;
            let last = node.next.is_none();
            unit.finish();
            if last {
                work.checkpoint()?;
                node.next = self.head.take();
                break;
            }
            current = node
                .next
                .as_mut()
                .expect("checked incoming allocation tail");
        }
        self.head = incoming.head.take();
        Ok(())
    }
}

impl Drop for CheckpointAllocationOwner {
    fn drop(&mut self) {
        let mut current = self.head.take();
        while let Some(node) = current {
            match Arc::try_unwrap(node) {
                Ok(mut node) => {
                    current = node.next.take();
                    drop(node);
                }
                Err(shared) => {
                    drop(shared);
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod retained_tests;

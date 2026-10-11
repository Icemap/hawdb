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

//! Retain canonical property-key admission through its final manifest consumer.
//! The dictionary indexes fixed-width digests and compares collision candidates
//! in bounded byte units. Writer payloads and other arrays are separate resources.

use super::*;
use crate::background::{
    CheckpointAllocationOwner, CheckpointDecodeContext, CheckpointOperationError,
    CheckpointVectorPush,
};
use std::cell::RefCell;

#[derive(Default)]
pub(super) struct Keys {
    pub(super) values: Vec<String>,
    pub(super) memory: CheckpointAllocationOwner,
}

pub(crate) struct CheckpointCanonicalManifest {
    // The complete key table must die before its admitted inventory.
    pub(super) manifest: CanonicalSegmentManifest,
    pub(super) memory: CheckpointAllocationOwner,
}

impl CheckpointCanonicalManifest {
    pub(crate) fn encode_with_work_context(
        &self,
        work: &CheckpointWorkContext,
    ) -> Result<crate::background::CheckpointText, CanonicalSegmentError> {
        self.manifest.encode_with_work_context(work)
    }

    pub(super) fn into_unadmitted(self) -> CanonicalSegmentManifest {
        assert!(
            self.memory.is_empty(),
            "admitted canonical keys cannot detach their inventory"
        );
        self.manifest
    }
}

// The private writer retains the immutable source key until this operation
// completes. Addresses are identity tokens only; no retained pointer is read.
struct PendingKey {
    address: usize,
    length: usize,
    hash: Option<IntegrityHasher>,
    hashed: usize,
    digest: Option<[u8; 32]>,
    located: bool,
    candidate: usize,
    compared: usize,
    lookup_done: bool,
    owned: Option<String>,
    result: Option<u32>,
}

impl PendingKey {
    fn new(key: &str) -> Self {
        Self {
            address: key.as_ptr() as usize,
            length: key.len(),
            hash: Some(IntegrityHasher::new()),
            hashed: 0,
            digest: None,
            located: false,
            candidate: 0,
            compared: 0,
            lookup_done: false,
            owned: None,
            result: None,
        }
    }
}

pub(super) struct Dictionary {
    keys: Vec<String>,
    ids: BTreeMap<[u8; 32], Vec<u32>>,
    key_push: Option<CheckpointVectorPush<String>>,
    index_push: Option<CheckpointVectorPush<u32>>,
    pending_key: Option<PendingKey>,
    key_memory: CheckpointAllocationOwner,
    index_memory: CheckpointAllocationOwner,
    work: CheckpointWorkContext,
    pending_index: Option<([u8; 32], u32)>,
    pending_index_ready: bool,
}

impl Dictionary {
    pub(super) fn work_context(&self) -> &CheckpointWorkContext {
        &self.work
    }

    pub(super) fn new(work: CheckpointWorkContext) -> Self {
        Self {
            keys: Vec::new(),
            ids: BTreeMap::new(),
            key_push: None,
            index_push: None,
            pending_key: None,
            key_memory: CheckpointAllocationOwner::default(),
            index_memory: CheckpointAllocationOwner::default(),
            work,
            pending_index: None,
            pending_index_ready: false,
        }
    }

    pub(super) fn intern(&mut self, key: &str) -> Result<u32, CanonicalSegmentError> {
        let work = self.work.clone();
        work.classify(|work| {
            let pending = self.pending_key.get_or_insert_with(|| PendingKey::new(key));
            if pending.address != key.as_ptr() as usize || pending.length != key.len() {
                return Err(CanonicalSegmentError::Corrupt(
                    "retained canonical key source changed".into(),
                ));
            }
            while pending.hashed < key.len() {
                let unit = work.start_unit()?;
                let end = (pending.hashed + 64 * 1024).min(key.len());
                pending
                    .hash
                    .as_mut()
                    .expect("unfinished hash")
                    .update(&key.as_bytes()[pending.hashed..end]);
                pending.hashed = end;
                unit.finish();
                work.checkpoint()?;
            }
            let digest = *pending.digest.get_or_insert_with(|| {
                *pending
                    .hash
                    .take()
                    .expect("finished hash")
                    .finish()
                    .sha256
                    .as_bytes()
            });
            let keys = CheckpointDecodeContext {
                work: work.clone(),
                memory: RefCell::new(std::mem::take(&mut self.key_memory)),
            };
            let index = CheckpointDecodeContext {
                work: work.clone(),
                memory: RefCell::new(std::mem::take(&mut self.index_memory)),
            };
            let result = self.intern_digest(key, digest, &keys, &index);
            self.key_memory = keys.memory.into_inner();
            self.index_memory = index.memory.into_inner();
            result
        })
        .map_err(|error| match error {
            CheckpointOperationError::Work(error) => CanonicalSegmentError::Work(error),
            CheckpointOperationError::Operation(error) => error,
        })
    }

    fn intern_digest(
        &mut self,
        key: &str,
        digest: [u8; 32],
        keys: &CheckpointDecodeContext,
        index: &CheckpointDecodeContext,
    ) -> Result<u32, CanonicalSegmentError> {
        let pending = self.pending_key.get_or_insert_with(|| PendingKey::new(key));
        if pending.address != key.as_ptr() as usize
            || pending.length != key.len()
            || pending.digest.is_some_and(|previous| previous != digest)
        {
            return Err(CanonicalSegmentError::Corrupt(
                "retained canonical key source changed".into(),
            ));
        }
        pending.digest = Some(digest);
        if !pending.lookup_done {
            if !pending.located {
                let unit = keys.start_unit()?;
                // Keep the completed lookup decision before observing stop.
                pending.located = true;
                unit.finish();
                keys.checkpoint()?;
            }
            if let Some(existing) = self.ids.get(&digest) {
                while pending.candidate < existing.len() {
                    let id = existing[pending.candidate];
                    let previous = &self.keys[id as usize];
                    let unit = keys.start_unit()?;
                    if previous.len() != key.len() {
                        pending.candidate += 1;
                        pending.compared = 0;
                        unit.finish();
                        keys.checkpoint()?;
                        continue;
                    }
                    let end = (pending.compared + 64 * 1024).min(key.len());
                    let same = previous.as_bytes()[pending.compared..end]
                        == key.as_bytes()[pending.compared..end];
                    if !same {
                        pending.candidate += 1;
                        pending.compared = 0;
                    } else {
                        pending.compared = end;
                        if end == key.len() {
                            pending.result = Some(id);
                            pending.lookup_done = true;
                        }
                    }
                    unit.finish();
                    keys.checkpoint()?;
                    if pending.lookup_done {
                        break;
                    }
                }
            }
            pending.lookup_done = true;
        }
        if pending.result.is_none() {
            if pending.owned.is_none() {
                let unit = keys.start_unit()?;
                let token = keys.reserve(key.len()).map_err(source)?;
                let mut owned = String::new();
                owned.try_reserve_exact(key.len()).map_err(|error| {
                    CanonicalSegmentError::Work(keys.record_failure(
                        CheckpointWorkError::Allocation {
                            bytes: key.len() as u64,
                            reason: error.to_string(),
                        },
                    ))
                })?;
                if owned.capacity() != key.len() {
                    return Err(CanonicalSegmentError::Work(keys.record_failure(
                        CheckpointWorkError::Allocation {
                            bytes: key.len() as u64,
                            reason: "canonical property key capacity exceeds its admission".into(),
                        },
                    )));
                }
                token.address(owned.as_ptr() as usize);
                pending.owned = Some(owned);
                unit.finish();
                keys.checkpoint()?;
            }
            let owned = pending.owned.as_mut().expect("admitted key");
            while owned.len() < key.len() {
                let mut end = (owned.len() + 64 * 1024).min(key.len());
                while !key.is_char_boundary(end) {
                    end -= 1;
                }
                let unit = keys.start_unit()?;
                owned.push_str(&key[owned.len()..end]);
                unit.finish();
                keys.checkpoint()?;
            }
            let id = u32_len(self.keys.len(), "canonical property key table")?;
            pending.result = Some(id);
            self.pending_index = Some((digest, id));
            self.pending_index_ready = false;
            self.key_push = Some(CheckpointVectorPush::new(
                pending.owned.take().expect("completed key"),
            ));
        }
        self.finish_pending_index(keys, index)?;
        keys.checkpoint()?;
        let id = self
            .pending_key
            .take()
            .expect("completed key operation")
            .result
            .expect("completed lookup or insertion");
        Ok(id)
    }

    fn finish_pending_index(
        &mut self,
        keys: &CheckpointDecodeContext,
        index: &CheckpointDecodeContext,
    ) -> Result<(), CanonicalSegmentError> {
        let Some((digest, id)) = self.pending_index else {
            return Ok(());
        };
        keys.resume_push(&mut self.keys, &mut self.key_push)
            .map_err(source)?;
        if !self.pending_index_ready {
            let unit = index.start_unit()?;
            if let std::collections::btree_map::Entry::Vacant(entry) = self.ids.entry(digest) {
                // Pinned Rust B-tree nodes have at most eleven key/value pairs
                // and twelve child pointers. Reserve four conservative node bounds
                // per distinct digest before insertion, covering split/root overlap.
                // Allocator rounding/latency and other writer containers are separate.
                let node = 64
                    + 11 * (std::mem::size_of::<[u8; 32]>() + std::mem::size_of::<Vec<u32>>())
                    + 12 * std::mem::size_of::<usize>();
                index.reserve(4 * node).map_err(source)?;
                entry.insert(Vec::new());
            }
            self.pending_index_ready = true;
            unit.finish();
        }
        let ids = self.ids.get_mut(&digest).expect("inserted digest");
        // The push itself can finish before reporting cancellation.
        if self.index_push.is_none() && ids.last() != Some(&id) {
            self.index_push = Some(CheckpointVectorPush::new(id));
        }
        index
            .resume_push(ids, &mut self.index_push)
            .map_err(source)?;
        self.pending_index = None;
        index.checkpoint()?;
        Ok(())
    }

    pub(super) fn into_keys(self) -> Keys {
        let Self {
            keys,
            ids,
            key_push,
            index_push,
            pending_key,
            key_memory,
            index_memory,
            work,
            pending_index: _,
            pending_index_ready: _,
        } = self;
        drop(ids);
        drop(key_push);
        drop(index_push);
        drop(pending_key);
        drop(index_memory);
        drop(work);
        Keys {
            values: keys,
            memory: key_memory,
        }
    }
}

fn source(error: hawdb_core::HawDBError) -> CanonicalSegmentError {
    // The classifier restores recorded work failures without parsing text.
    CanonicalSegmentError::Corrupt(error.to_string())
}

mod value;
pub(super) use value::encode as encode_value;

pub(super) mod accumulator;
pub(super) mod descriptor;
pub(super) mod record;
pub(super) mod state;

pub(super) enum Record {
    Ordinary(Vec<u8>),
    Checkpoint(crate::background::CheckpointBytes),
}

impl std::ops::Deref for Record {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Ordinary(bytes) => bytes,
            Self::Checkpoint(bytes) => bytes,
        }
    }
}

#[cfg(test)]
pub(super) mod tests;

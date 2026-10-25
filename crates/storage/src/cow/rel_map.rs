// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::{CowMapValueMut, CowPageWeight, CowSegment, CowSegmentedMap};
use crate::{RelId, RelRecord};
use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

#[derive(Debug, Clone)]
struct RelOwner(CowSegment<RelRecord>);
impl CowPageWeight for RelOwner {
    fn cow_page_bytes(&self) -> usize {
        self.0.cow_page_bytes()
    }
    fn delta_pressure_bytes(&self) -> u64 {
        self.0.delta_pressure_bytes()
    }
}
impl RelOwner {
    fn into_record(self) -> RelRecord {
        Arc::unwrap_or_clone(self.0 .0).value
    }
}

/// Relationship pages share individual record owners, as node pages do.
/// Detaching a page copies pointers; only an edited record copies properties.
#[doc(hidden)]
#[derive(Debug, Clone, Default)]
pub struct CowRelMap(CowSegmentedMap<RelId, RelOwner>);

impl From<BTreeMap<RelId, RelRecord>> for CowRelMap {
    fn from(records: BTreeMap<RelId, RelRecord>) -> Self {
        Self(CowSegmentedMap::from_ordered_entries(
            records
                .into_iter()
                .map(|(id, record)| (id, RelOwner(CowSegment::from(record)))),
        ))
    }
}
impl CowRelMap {
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn get(&self, id: &RelId) -> Option<&RelRecord> {
        self.0.get(id).map(|row| &*row.0)
    }
    pub fn contains_key(&self, id: &RelId) -> bool {
        self.0.contains_key(id)
    }
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = (&RelId, &RelRecord)> + Clone {
        self.0.iter().map(|(id, row)| (id, &*row.0))
    }
    pub fn values(&self) -> impl DoubleEndedIterator<Item = &RelRecord> + Clone {
        self.iter().map(|(_, row)| row)
    }
    pub fn keys(&self) -> impl DoubleEndedIterator<Item = &RelId> + Clone {
        self.0.keys()
    }
    pub fn iter_after(&self, after: Option<RelId>) -> impl Iterator<Item = (&RelId, &RelRecord)> {
        self.0.iter_after(after).map(|(id, row)| (id, &*row.0))
    }
    pub fn insert(&mut self, id: RelId, record: RelRecord) -> Option<RelRecord> {
        self.0
            .insert(id, RelOwner(CowSegment::from(record)))
            .map(RelOwner::into_record)
    }
    pub fn get_mut(&mut self, id: &RelId) -> Option<RelRecordMut<'_>> {
        self.0.get_mut(id).map(RelRecordMut)
    }
    pub fn remove(&mut self, id: &RelId) -> Option<RelRecord> {
        self.0.remove(id).map(RelOwner::into_record)
    }
    pub fn rebalance_key(&mut self, id: &RelId) {
        self.0.rebalance_key(id);
    }
    pub fn shares_storage_with(&self, other: &Self) -> bool {
        self.0.shares_storage_with(&other.0)
    }
    pub fn segment_count(&self) -> usize {
        self.0.segment_count()
    }
    pub fn shared_segment_count_with(&self, other: &Self) -> usize {
        self.0.shared_segment_count_with(&other.0)
    }
    pub fn delta_pressure_bytes(&self) -> u64 {
        self.0.delta_pressure_bytes()
    }
    pub fn page_directory_capacity_bytes(&self) -> usize {
        self.0.page_directory_capacity_bytes()
    }
    pub fn directory_capacity_bytes(&self) -> usize {
        self.0.directory_capacity_bytes()
    }
    pub(crate) fn first_after(&self, id: Option<&RelId>) -> Option<(&RelId, &RelRecord)> {
        self.0.first_after(id).map(|(id, row)| (id, &*row.0))
    }
}

#[doc(hidden)]
pub struct RelRecordMut<'a>(CowMapValueMut<'a, RelOwner>);
impl Deref for RelRecordMut<'_> {
    type Target = RelRecord;
    fn deref(&self) -> &RelRecord {
        &self.0 .0
    }
}
impl DerefMut for RelRecordMut<'_> {
    fn deref_mut(&mut self) -> &mut RelRecord {
        &mut self.0 .0
    }
}
impl super::checkpoint::CheckpointCopyBytes for RelOwner {
    fn checkpoint_copy_bytes(&self) -> usize {
        0
    }
}
impl CowRelMap {
    pub(crate) fn prepare_checkpoint_insertions_for_key(
        &mut self,
        id: &RelId,
        count: usize,
        work: &crate::background::CheckpointWorkContext,
    ) -> Result<(), crate::background::CheckpointWorkError> {
        self.0
            .prepare_checkpoint_insertions_for_key(id, count, work)
    }
    pub(crate) fn prepare_checkpoint_copy_for_key(
        &mut self,
        id: &RelId,
        work: &crate::background::CheckpointWorkContext,
    ) -> Result<(), crate::background::CheckpointWorkError> {
        let Some(index) = self.0.segment_index(id) else {
            return work.checkpoint();
        };
        let Some(row) = self.0.get(id) else {
            return work.checkpoint();
        };
        let copy_row =
            self.0.segments.is_shared() || self.0.segments[index].is_shared() || row.0.is_shared();
        // Admit row payload before detaching any shared directory or page.
        // A refused indivisible row must preserve the original physical roots.
        let mut memory = crate::background::CheckpointAllocationOwner::default();
        if copy_row {
            let bytes = hawdb_core::ids::relationship_allocation_bytes(&row.0)
                .saturating_add(CowSegment::<RelRecord>::checkpoint_ownership_bytes());
            let unit = work.start_unit()?;
            memory.reserve(bytes, work)?;
            unit.finish();
        }
        self.0.prepare_checkpoint_copy_for_key(id, work)?;
        if copy_row {
            let mut row = self.0.get_mut(id).expect("prepared relationship exists");
            let unit = work.start_unit()?;
            let copied = CowSegment::from_checkpoint_value((*row.0).clone(), memory);
            unit.finish();
            work.checkpoint()?;
            row.0 = copied;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NodeId, Value};
    use hawdb_core::RelTypeId;

    fn record(id: u64) -> RelRecord {
        RelRecord {
            id: RelId(id),
            source: NodeId(1),
            target: NodeId(2),
            rel_type: RelTypeId(1),
            properties: BTreeMap::from([("body".into(), Value::String("x".repeat(8192)))]),
        }
    }

    #[test]
    fn shared_relationship_page_detaches_only_the_changed_payload() {
        let mut map = CowRelMap::from(
            (1..=8)
                .map(|id| (RelId(id), record(id)))
                .collect::<BTreeMap<_, _>>(),
        );
        assert_eq!(map.segment_count(), 1);
        let snapshot = map.clone();
        let original = std::ptr::from_ref(map.get(&RelId(2)).unwrap());
        map.get_mut(&RelId(1)).unwrap().properties.clear();
        assert_eq!(std::ptr::from_ref(map.get(&RelId(2)).unwrap()), original);
        assert_eq!(
            std::ptr::from_ref(snapshot.get(&RelId(2)).unwrap()),
            original
        );
        assert!(!snapshot.get(&RelId(1)).unwrap().properties.is_empty());
        let removed = map.remove(&RelId(2)).unwrap();
        assert_eq!(
            removed.properties,
            snapshot.get(&RelId(2)).unwrap().properties
        );
        assert!(snapshot.get(&RelId(2)).is_some());
        assert_eq!(map.iter_after(Some(RelId(2))).next().unwrap().0, &RelId(3));
        drop(map);
        assert_eq!(
            snapshot.get(&RelId(1)).unwrap().properties["body"],
            Value::String("x".repeat(8192))
        );
    }
}

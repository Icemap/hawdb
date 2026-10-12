// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::retained_capacity::{properties_capacity, tree_capacity};
use super::{CowPageWeight, CowSegmentedMap};
use crate::{NodeId, NodeRecord};
use hawdb_core::LabelId;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Borrow node records independently of their resident ownership layout.
#[doc(hidden)]
pub trait NodeRecordRead {
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn get(&self, id: &NodeId) -> Option<&NodeRecord>;
    fn iter(&self) -> impl Iterator<Item = (&NodeId, &NodeRecord)>;
    fn values(&self) -> impl Iterator<Item = &NodeRecord> {
        self.iter().map(|(_, node)| node)
    }
}

/// Mutation contracts used by snapshot WAL replay.
#[doc(hidden)]
pub trait NodeRecordWrite: NodeRecordRead {
    fn insert(&mut self, id: NodeId, node: NodeRecord) -> Option<NodeRecord>;
    fn get_mut(&mut self, id: &NodeId) -> Option<&mut NodeRecord>;
    fn remove(&mut self, id: &NodeId) -> Option<NodeRecord>;
}

impl NodeRecordRead for CowSegmentedMap<NodeId, NodeRecord> {
    fn len(&self) -> usize {
        self.len()
    }
    fn get(&self, id: &NodeId) -> Option<&NodeRecord> {
        self.get(id)
    }
    fn iter(&self) -> impl Iterator<Item = (&NodeId, &NodeRecord)> {
        self.iter()
    }
}
impl NodeRecordWrite for CowSegmentedMap<NodeId, NodeRecord> {
    fn insert(&mut self, id: NodeId, node: NodeRecord) -> Option<NodeRecord> {
        self.insert(id, node)
    }
    fn get_mut(&mut self, id: &NodeId) -> Option<&mut NodeRecord> {
        self.get_mut(id)
    }
    fn remove(&mut self, id: &NodeId) -> Option<NodeRecord> {
        self.remove(id)
    }
}

#[derive(Debug, Clone)]
struct NodeOwner(Arc<NodeRecord>);
impl CowPageWeight for NodeOwner {
    fn cow_page_bytes(&self) -> usize {
        self.0.cow_page_bytes()
    }
}
impl NodeOwner {
    fn into_record(self) -> NodeRecord {
        Arc::unwrap_or_clone(self.0)
    }
}

/// COW pages contain row owners; detaching a page never clones row payloads.
#[doc(hidden)]
#[derive(Debug, Clone, Default)]
pub struct CowNodeMap(CowSegmentedMap<NodeId, NodeOwner>);

fn node_capacity(node: &NodeRecord) -> Option<usize> {
    std::mem::size_of::<NodeRecord>()
        .checked_add(2 * std::mem::size_of::<usize>())?
        .checked_add(tree_capacity::<LabelId, ()>(node.labels.len())?)?
        .checked_add(properties_capacity(&node.properties, 0)?)
}

impl From<BTreeMap<NodeId, NodeRecord>> for CowNodeMap {
    fn from(nodes: BTreeMap<NodeId, NodeRecord>) -> Self {
        Self(CowSegmentedMap::from_ordered_entries(
            nodes
                .into_iter()
                .map(|(id, node)| (id, NodeOwner(Arc::new(node)))),
        ))
    }
}
impl CowNodeMap {
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn get(&self, id: &NodeId) -> Option<&NodeRecord> {
        self.0.get(id).map(|owner| owner.0.as_ref())
    }
    pub fn contains_key(&self, id: &NodeId) -> bool {
        self.0.contains_key(id)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&NodeId, &NodeRecord)> {
        self.0.iter().map(|(id, owner)| (id, owner.0.as_ref()))
    }
    pub fn values(&self) -> impl Iterator<Item = &NodeRecord> {
        self.iter().map(|(_, node)| node)
    }
    pub fn keys(&self) -> impl Iterator<Item = &NodeId> {
        self.0.keys()
    }
    pub fn iter_after(
        &self,
        after: Option<NodeId>,
    ) -> impl Iterator<Item = (&NodeId, &NodeRecord)> {
        self.0
            .iter_after(after)
            .map(|(id, owner)| (id, owner.0.as_ref()))
    }
    pub fn insert(&mut self, id: NodeId, node: NodeRecord) -> Option<NodeRecord> {
        self.0
            .insert(id, NodeOwner(Arc::new(node)))
            .map(NodeOwner::into_record)
    }
    pub fn get_mut(&mut self, id: &NodeId) -> Option<&mut NodeRecord> {
        // The underlying map invalidates its generation capacity before mutation.
        self.0.get_mut(id).map(|owner| Arc::make_mut(&mut owner.0))
    }
    pub fn remove(&mut self, id: &NodeId) -> Option<NodeRecord> {
        self.0.remove(id).map(NodeOwner::into_record)
    }
    pub fn rebalance_key(&mut self, id: &NodeId) {
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
    pub fn directory_capacity_bytes(&self) -> usize {
        self.0.directory_capacity_bytes()
    }
    pub(crate) fn retained_point_row_preflight(
        &self,
        id: NodeId,
    ) -> Option<(&Arc<NodeRecord>, Option<usize>)> {
        let owner = self.0.get(&id)?;
        Some((&owner.0, node_capacity(&owner.0)))
    }
    pub fn retained_capacity_preflight(&self) -> (Option<usize>, usize) {
        let mut inspected = 0;
        let bound = *self.0.segments.retained_capacity.get_or_init(|| {
            self.0
                .segments
                .iter()
                .try_fold(self.directory_capacity_bytes(), |bytes, page| {
                    let bytes = bytes
                        .checked_add(2 * std::mem::size_of::<usize>())?
                        .checked_add(std::mem::size_of::<BTreeMap<NodeId, NodeOwner>>())?
                        .checked_add(tree_capacity::<NodeId, NodeOwner>(page.len())?)?;
                    page.values().try_fold(bytes, |bytes, owner| {
                        inspected += 1;
                        bytes.checked_add(node_capacity(&owner.0)?)
                    })
                })
        });
        (bound, inspected)
    }
}
impl NodeRecordRead for CowNodeMap {
    fn len(&self) -> usize {
        self.len()
    }
    fn get(&self, id: &NodeId) -> Option<&NodeRecord> {
        self.get(id)
    }
    fn iter(&self) -> impl Iterator<Item = (&NodeId, &NodeRecord)> {
        self.iter()
    }
}
impl NodeRecordWrite for CowNodeMap {
    fn insert(&mut self, id: NodeId, node: NodeRecord) -> Option<NodeRecord> {
        self.insert(id, node)
    }
    fn get_mut(&mut self, id: &NodeId) -> Option<&mut NodeRecord> {
        self.get_mut(id)
    }
    fn remove(&mut self, id: &NodeId) -> Option<NodeRecord> {
        self.remove(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Value;
    use std::collections::BTreeSet;

    fn node(id: u64, capacity: usize) -> NodeRecord {
        let mut text = String::with_capacity(capacity);
        text.push_str("original");
        NodeRecord {
            id: NodeId(id),
            labels: BTreeSet::from([LabelId(1)]),
            properties: BTreeMap::from([("text".into(), Value::String(text))]),
        }
    }

    #[test]
    fn row_source_keeps_only_its_record_and_preserves_buffer_addresses() {
        let mut map = CowNodeMap::from(
            (1..=1025)
                .map(|id| (NodeId(id), node(id, 1024)))
                .collect::<BTreeMap<_, _>>(),
        );
        let id = NodeId(1);
        let (row, bound) = map.retained_point_row_preflight(id).unwrap();
        let bound = bound.unwrap();
        assert!(bound >= 1024);
        assert!(bound < map.retained_capacity_preflight().0.unwrap());
        let address = std::ptr::from_ref(row.as_ref());
        let Value::String(text) = &row.properties["text"] else {
            unreachable!()
        };
        let text_address = text.as_ptr();
        let held = Arc::clone(row);
        let unrelated = Arc::downgrade(map.retained_point_row_preflight(NodeId(2)).unwrap().0);
        map.get_mut(&id).unwrap().properties.clear();
        assert_ne!(std::ptr::from_ref(map.get(&id).unwrap()), address);
        drop(map);
        assert!(unrelated.upgrade().is_none());
        assert_eq!(std::ptr::from_ref(held.as_ref()), address);
        let Value::String(text) = &held.properties["text"] else {
            unreachable!()
        };
        assert_eq!(text, "original");
        assert_eq!(text.as_ptr(), text_address);
        assert_eq!(node_capacity(&held), Some(bound));
        let weak = Arc::downgrade(&held);
        drop(held);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn snapshot_detaches_only_the_changed_row_and_invalidates_capacity() {
        let mut map = CowNodeMap::from(BTreeMap::from([
            (NodeId(1), node(1, 1024)),
            (NodeId(2), node(2, 1024)),
        ]));
        let snapshot = map.clone();
        let (old_bound, inspected) = snapshot.retained_capacity_preflight();
        assert_eq!(inspected, 2);
        assert_eq!(map.retained_capacity_preflight(), (old_bound, 0));
        let untouched = std::ptr::from_ref(map.get(&NodeId(2)).unwrap());
        map.get_mut(&NodeId(1))
            .unwrap()
            .properties
            .insert("text".into(), Value::String("x".repeat(16 * 1024)));
        assert_eq!(std::ptr::from_ref(map.get(&NodeId(2)).unwrap()), untouched);
        assert_eq!(
            std::ptr::from_ref(snapshot.get(&NodeId(2)).unwrap()),
            untouched
        );
        let (new_bound, inspected) = map.retained_capacity_preflight();
        assert_eq!(inspected, 2);
        assert!(new_bound > old_bound);
        assert_eq!(snapshot.retained_capacity_preflight(), (old_bound, 0));
        assert_eq!(map.retained_capacity_preflight(), (new_bound, 0));
        let old_row = snapshot
            .retained_point_row_preflight(NodeId(1))
            .unwrap()
            .1
            .unwrap();
        let new_row = map
            .retained_point_row_preflight(NodeId(1))
            .unwrap()
            .1
            .unwrap();
        assert!(new_row > old_row);
        map.insert(NodeId(3), node(3, 1024));
        assert_eq!(map.retained_capacity_preflight().1, 3);
        map.remove(&NodeId(3));
        assert_eq!(map.retained_capacity_preflight().1, 2);
        assert_eq!(map.retained_capacity_preflight().1, 0);
    }

    #[test]
    fn nested_row_bound_refuses_excessive_depth_before_adoption() {
        let mut nested = Value::Null;
        for _ in 0..64 {
            nested = Value::List(vec![nested]);
        }
        let mut record = node(1, 1024);
        record.properties.insert("nested".into(), nested);
        let map = CowNodeMap::from(BTreeMap::from([(record.id, record)]));
        let (row, bound) = map.retained_point_row_preflight(NodeId(1)).unwrap();
        assert!(bound.is_none());
        assert_eq!(Arc::strong_count(row), 1);
        assert!(map.retained_capacity_preflight().0.is_none());
    }
}

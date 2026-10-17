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

use super::{pending_node_index, properties_contain_all, LabelId, NodeId, PendingNode, Value};
use std::collections::{hash_map::RandomState, BTreeMap, BTreeSet, HashMap};
use std::hash::BuildHasher;
use std::ops::Deref;

#[cfg(test)]
#[path = "pending_nodes/tests.rs"]
mod tests;

type ValuePostings = HashMap<u64, BTreeSet<NodeId>>;

#[derive(Default)]
struct PropertyIndexes {
    hashes: RandomState,
    // Exact property names keep one node's distinct properties independently removable.
    labels: HashMap<LabelId, HashMap<String, ValuePostings>>,
}

impl PropertyIndexes {
    fn ensure_property(&mut self, nodes: &[PendingNode], label: LabelId, property: &str) {
        if self
            .labels
            .get(&label)
            .is_some_and(|properties| properties.contains_key(property))
        {
            return;
        }
        let mut postings = ValuePostings::new();
        for (id, node_label, properties) in nodes {
            if *node_label == label
                && let Some(value) = properties.get(property)
            {
                postings
                    .entry(self.hashes.hash_one(value))
                    .or_default()
                    .insert(*id);
            }
        }
        self.labels
            .entry(label)
            .or_default()
            .insert(property.to_owned(), postings);
    }

    fn insert_node(&mut self, (id, label, properties): &PendingNode) {
        let Some(indexes) = self.labels.get_mut(label) else {
            return;
        };
        for (property, value) in properties {
            if let Some(postings) = indexes.get_mut(property) {
                postings
                    .entry(self.hashes.hash_one(value))
                    .or_default()
                    .insert(*id);
            }
        }
    }

    fn remove_property(&mut self, id: NodeId, label: LabelId, property: &str, value: &Value) {
        let Some(postings) = self
            .labels
            .get_mut(&label)
            .and_then(|properties| properties.get_mut(property))
        else {
            return;
        };
        let hash = self.hashes.hash_one(value);
        if let Some(ids) = postings.get_mut(&hash) {
            ids.remove(&id);
            if ids.is_empty() {
                postings.remove(&hash);
            }
        }
        // Retain the property entry even when empty: it still represents a complete index.
    }

    fn insert_property(&mut self, id: NodeId, label: LabelId, property: &str, value: &Value) {
        if let Some(postings) = self
            .labels
            .get_mut(&label)
            .and_then(|properties| properties.get_mut(property))
        {
            postings
                .entry(self.hashes.hash_one(value))
                .or_default()
                .insert(id);
        }
    }

    fn remove_node(&mut self, (id, label, properties): &PendingNode) {
        for (property, value) in properties {
            self.remove_property(*id, *label, property, value);
        }
    }

    fn candidates(&self, label: LabelId, property: &str, hash: u64) -> Option<&BTreeSet<NodeId>> {
        self.labels.get(&label)?.get(property)?.get(&hash)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PendingMatch {
    Subset,
    Exact,
}

#[derive(Default)]
pub(super) struct PendingNodes {
    // Shared slice access preserves the sorted-ID invariant and prevents untracked writes.
    nodes: Vec<PendingNode>,
    property_indexes: Option<PropertyIndexes>,
}

impl Deref for PendingNodes {
    type Target = [PendingNode];

    fn deref(&self) -> &Self::Target {
        &self.nodes
    }
}

impl PendingNodes {
    pub(super) fn push(&mut self, node: PendingNode) {
        debug_assert!(self.nodes.last().is_none_or(|last| last.0 < node.0));
        if let Some(indexes) = &mut self.property_indexes {
            indexes.insert_node(&node);
        }
        self.nodes.push(node);
    }

    pub(super) fn remove(&mut self, id: NodeId) {
        if let Some(index) = pending_node_index(&self.nodes, id) {
            let node = self.nodes.remove(index);
            if let Some(indexes) = &mut self.property_indexes {
                indexes.remove_node(&node);
            }
        }
    }

    pub(super) fn set_property(&mut self, index: usize, property: String, value: Value) {
        let (id, label, properties) = &mut self.nodes[index];
        if let Some(indexes) = &mut self.property_indexes {
            if let Some(old) = properties.get(&property) {
                indexes.remove_property(*id, *label, &property, old);
            }
            indexes.insert_property(*id, *label, &property, &value);
        }
        properties.insert(property, value);
    }

    pub(super) fn find_by_properties(
        &mut self,
        label: LabelId,
        required: &BTreeMap<String, Value>,
        mode: PendingMatch,
    ) -> Option<NodeId> {
        if required.is_empty() {
            return self
                .nodes
                .iter()
                .find(|(_, node_label, properties)| {
                    *node_label == label && (mode == PendingMatch::Subset || properties.is_empty())
                })
                .map(|(id, _, _)| *id);
        }
        let indexes = self
            .property_indexes
            .get_or_insert_with(PropertyIndexes::default);
        let mut best: Option<(&str, u64, usize)> = None;
        for (property, value) in required {
            indexes.ensure_property(&self.nodes, label, property);
            let hash = indexes.hashes.hash_one(value);
            let candidates = indexes.candidates(label, property, hash)?;
            if best.is_none_or(|(_, _, count)| candidates.len() < count) {
                best = Some((property, hash, candidates.len()));
            }
        }
        let (property, hash, _) = best?;
        // Fingerprints only select candidates. Full equality handles collisions and residuals.
        indexes
            .candidates(label, property, hash)?
            .iter()
            .find_map(|id| {
                let index = pending_node_index(&self.nodes, *id)?;
                let (_, node_label, properties) = &self.nodes[index];
                (*node_label == label
                    && (mode == PendingMatch::Subset || properties.len() == required.len())
                    && properties_contain_all(properties, required))
                .then_some(*id)
            })
    }
}

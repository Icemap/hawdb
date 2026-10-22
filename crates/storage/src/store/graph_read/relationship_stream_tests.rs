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
use std::cell::Cell;
use std::rc::Rc;

struct Allocation {
    bytes: usize,
    live: Rc<Cell<usize>>,
}

#[test]
fn point_work_charges_missing_rejected_and_tombstoned_records_without_payload() {
    use crate::read_view::{
        AdmittedNodeRead, AdmittedRelationshipRead, ControlledGraphReadAllocator,
    };
    let mut catalog = Catalog::default();
    let mut store = GraphStore::in_memory();
    let node = store
        .create_node(&mut catalog, "Item", BTreeMap::new())
        .unwrap();
    let deleted = store
        .create_node(&mut catalog, "Other", BTreeMap::new())
        .unwrap();
    let relationship = store
        .create_relationship(&mut catalog, node, deleted, "LINK", BTreeMap::new())
        .unwrap();
    store.node_tombstones.insert(deleted);
    store.relationship_tombstones.insert(relationship);
    let other_label = catalog.label_id("Other").unwrap();
    let task = hawdb_core::RuntimeTaskContext::default().with_work_limit(5);
    let mut unexpected = |_| panic!("absent records must not admit payload");
    let mut policy = ControlledGraphReadAllocator::new(&mut unexpected, Some(&task));
    for (id, labels) in [
        (NodeId(u64::MAX), None),
        (node, Some([other_label])),
        (deleted, None),
    ] {
        assert!(matches!(
            store
                .node_with_allocation(
                    id,
                    labels.as_ref().map(|labels| labels.as_slice()),
                    &mut policy
                )
                .unwrap(),
            AdmittedNodeRead::Missing
        ));
    }
    for id in [RelId(u64::MAX), relationship] {
        assert!(matches!(
            store.relationship_with_allocation(id, &mut policy).unwrap(),
            AdmittedRelationshipRead::Missing
        ));
    }
    assert_eq!(task.work_units(), 5);
    assert!(
        matches!(store.node_with_allocation(node, None, &mut policy), Err(HawDBError::ResourceBudgetExceeded(ref error)) if error.resource == hawdb_core::ResourceBudgetKind::WorkUnits)
    );
    assert!(
        matches!(store.relationship_with_allocation(relationship, &mut policy), Err(HawDBError::ResourceBudgetExceeded(ref error)) if error.resource == hawdb_core::ResourceBudgetKind::WorkUnits)
    );
    assert!(task.check_work_limit().is_err());
    let retry = hawdb_core::RuntimeTaskContext::default().with_work_limit(1);
    assert!(matches!(
        store
            .relationship_with_allocation(
                relationship,
                &mut ControlledGraphReadAllocator::new(&mut unexpected, Some(&retry))
            )
            .unwrap(),
        AdmittedRelationshipRead::Missing
    ));
    assert_eq!(retry.work_units(), 1);
}
impl crate::read_view::GraphReadAllocation for Allocation {
    fn bytes(&self) -> usize {
        self.bytes
    }
    fn grow(&mut self, bytes: usize) -> Result<()> {
        if self.bytes == 0 && bytes > 0 {
            self.live.set(self.live.get() + 1);
        }
        self.bytes += bytes;
        Ok(())
    }
}
impl Drop for Allocation {
    fn drop(&mut self) {
        if self.bytes > 0 {
            self.live.set(self.live.get() - 1);
        }
    }
}

#[test]
fn admitted_relationship_stream_preserves_overlay_filter_stop_and_inventory() {
    let path = std::env::temp_dir().join(format!(
        "hawdb-relationship-stream-{}",
        hawdb_core::generate_uuidv7().unwrap()
    ));
    let mut catalog = Catalog::default();
    let mut store = GraphStore::open(&path, &mut catalog).unwrap();
    let source = store
        .create_node(&mut catalog, "Item", BTreeMap::new())
        .unwrap();
    let target = store
        .create_node(&mut catalog, "Item", BTreeMap::new())
        .unwrap();
    let mut expected = BTreeMap::new();
    for i in 0..64 {
        let body = Value::String(format!("record-{i}:界🙂\0"));
        let id = store
            .create_relationship(
                &mut catalog,
                source,
                target,
                if i % 2 == 0 { "LINK" } else { "OTHER" },
                BTreeMap::from([("body".into(), body.clone())]),
            )
            .unwrap();
        if i % 2 == 0 {
            expected.insert(id, body);
        }
    }
    let selected = catalog.rel_type_id("LINK").unwrap();
    store.checkpoint(&catalog).unwrap();
    drop(store);
    let mut catalog = Catalog::default();
    let mut store = GraphStore::open_with_durability_and_replay_config(
        &path,
        &mut catalog,
        DurabilityPolicy::default(),
        WalReplayConfig {
            residency_mode: StorageResidencyMode::OutOfCore,
            ..Default::default()
        },
    )
    .unwrap();
    let deleted = *expected.keys().next().unwrap();
    store.relationship_tombstones.insert(deleted);
    expected.remove(&deleted);
    let changed = *expected.keys().next().unwrap();
    let mut record = store
        .canonical_base
        .as_ref()
        .unwrap()
        .get_relationship(changed)
        .unwrap()
        .unwrap();
    record
        .properties
        .insert("body".into(), Value::String("changed".into()));
    store.relationships.insert(changed, record);
    expected.insert(changed, Value::String("changed".into()));
    let added = store
        .create_relationship(
            &mut catalog,
            source,
            target,
            "LINK",
            BTreeMap::from([("body".into(), Value::String("delta".into()))]),
        )
        .unwrap();
    expected.insert(added, Value::String("delta".into()));
    let live = Rc::new(Cell::new(0));
    let calls = Cell::new(0);
    let mut admit = |bytes| {
        if bytes > 0 {
            calls.set(calls.get() + 1);
            live.set(live.get() + 1);
        }
        Ok(Some(Box::new(Allocation {
            bytes,
            live: live.clone(),
        })
            as Box<dyn crate::read_view::GraphReadAllocation>))
    };
    let mut seen = BTreeMap::new();
    assert_eq!(
        store
            .visit_relationships_with_allocation(
                Some(selected),
                &mut crate::read_view::ControlledGraphReadAllocator::new(&mut admit, None),
                &mut |row| {
                    assert_eq!(live.get(), 1);
                    let record = row.relationship();
                    assert_eq!(record.source, source);
                    assert_eq!(record.target, target);
                    assert_eq!(record.rel_type, selected);
                    seen.insert(record.id, record.properties["body"].clone());
                    Ok(GraphScanControl::Continue)
                }
            )
            .unwrap(),
        GraphScanControl::Continue
    );
    assert_eq!(seen, expected);
    assert_eq!(calls.get(), expected.len());
    assert_eq!(live.get(), 0);
    let mut visits = 0;
    assert_eq!(
        store
            .visit_relationships_with_allocation(
                Some(selected),
                &mut crate::read_view::ControlledGraphReadAllocator::new(&mut admit, None),
                &mut |_| {
                    visits += 1;
                    Ok(GraphScanControl::Stop)
                }
            )
            .unwrap(),
        GraphScanControl::Stop
    );
    assert_eq!(visits, 1);
    assert_eq!(live.get(), 0);
    let mut deny = |_| Ok(None);
    assert_eq!(
        store
            .visit_relationships_with_allocation(
                Some(selected),
                &mut crate::read_view::ControlledGraphReadAllocator::new(&mut deny, None),
                &mut |_| panic!("refused row reached consumer")
            )
            .unwrap(),
        GraphScanControl::Stop
    );
    let mut insufficient = |bytes: usize| {
        Ok(Some(Box::new(Allocation {
            bytes: bytes.saturating_sub(1),
            live: {
                if bytes > 1 {
                    live.set(live.get() + 1);
                }
                live.clone()
            },
        })
            as Box<dyn crate::read_view::GraphReadAllocation>))
    };
    assert!(matches!(
        store.visit_relationships_with_allocation(
            Some(selected),
            &mut crate::read_view::ControlledGraphReadAllocator::new(&mut insufficient, None),
            &mut |_| panic!("under-admitted row reached consumer")
        ),
        Err(HawDBError::Execution(_))
    ));
    assert_eq!(live.get(), 0);
    let canonical_id = *expected
        .keys()
        .find(|id| **id != changed && **id != added)
        .unwrap();
    assert!(matches!(
        store
            .relationship_with_allocation(
                canonical_id,
                &mut crate::read_view::ControlledGraphReadAllocator::new(&mut deny, None)
            )
            .unwrap(),
        AdmittedRelationshipRead::Stopped
    ));
    assert!(matches!(
        store.relationship_with_allocation(
            canonical_id,
            &mut crate::read_view::ControlledGraphReadAllocator::new(&mut insufficient, None)
        ),
        Err(HawDBError::Execution(_))
    ));
    assert_eq!(live.get(), 0);
    let mut fail = |_| Err(HawDBError::Execution("query budget exhausted".into()));
    assert!(matches!(
        store.relationship_with_allocation(
            canonical_id,
            &mut crate::read_view::ControlledGraphReadAllocator::new(&mut fail, None)
        ),
        Err(HawDBError::Execution(_))
    ));
    let AdmittedRelationshipRead::Relationship(row) = store
        .relationship_with_allocation(
            canonical_id,
            &mut crate::read_view::ControlledGraphReadAllocator::new(&mut admit, None),
        )
        .unwrap()
    else {
        panic!("query admission failure poisoned the canonical reader");
    };
    assert_eq!(
        row.relationship().properties["body"],
        expected[&canonical_id]
    );
    assert_eq!(live.get(), 1);
    drop(row);
    assert_eq!(live.get(), 0);
    assert!(matches!(
        store
            .relationship_with_allocation(
                deleted,
                &mut crate::read_view::ControlledGraphReadAllocator::new(&mut admit, None)
            )
            .unwrap(),
        AdmittedRelationshipRead::Missing
    ));
    drop(store);
    std::fs::remove_dir_all(path).unwrap();
}

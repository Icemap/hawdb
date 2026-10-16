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
use crate::read_view::GraphReadAllocation;
use std::cell::Cell;
use std::rc::Rc;

struct Permit {
    bytes: usize,
    held: Rc<Cell<usize>>,
}
impl GraphReadAllocation for Permit {
    fn bytes(&self) -> usize {
        self.bytes
    }
    fn grow(&mut self, bytes: usize) -> Result<()> {
        self.bytes += bytes;
        self.held.set(self.held.get() + bytes);
        Ok(())
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.held.set(self.held.get() - self.bytes);
    }
}
fn permit(bytes: usize, held: &Rc<Cell<usize>>) -> Box<dyn GraphReadAllocation> {
    held.set(held.get() + bytes);
    Box::new(Permit {
        bytes,
        held: Rc::clone(held),
    })
}
fn fixture() -> (std::path::PathBuf, GraphStore, Catalog, NodeId) {
    let path = std::env::temp_dir().join(format!(
        "hawdb-relationship-scan-{}",
        hawdb_core::generate_uuidv7().unwrap()
    ));
    let mut catalog = Catalog::default();
    let mut store = GraphStore::open_with_durability_and_replay_config(
        &path,
        &mut catalog,
        DurabilityPolicy::SyncOnCheckpoint,
        WalReplayConfig {
            residency_mode: StorageResidencyMode::OutOfCore,
            ..WalReplayConfig::default()
        },
    )
    .unwrap();
    let source = store
        .create_node(&mut catalog, "Source", BTreeMap::new())
        .unwrap();
    for ordinal in 0..64 {
        let target = store
            .create_node(
                &mut catalog,
                "Target",
                BTreeMap::from([("id".into(), Value::Int(ordinal))]),
            )
            .unwrap();
        store
            .create_relationship(
                &mut catalog,
                source,
                target,
                if ordinal % 2 == 0 { "LINK" } else { "OTHER" },
                BTreeMap::from([("rank".into(), Value::Int(ordinal))]),
            )
            .unwrap();
    }
    store.checkpoint(&catalog).unwrap();
    assert!(store.is_out_of_core());
    assert!(store.relationships.is_empty());
    (path, store, catalog, source)
}
fn collect(store: &GraphStore, rel_type: Option<RelTypeId>) -> Vec<RelRecord> {
    let held = Rc::new(Cell::new(0));
    let mut rows = Vec::new();
    assert_eq!(
        store
            .visit_relationships_with_allocation(
                rel_type,
                &mut |bytes| Ok(Some(permit(bytes, &held))),
                &mut |input| {
                    assert!(
                        held.get()
                            >= hawdb_core::ids::relationship_allocation_bytes(input.relationship())
                    );
                    rows.push(input.relationship().clone());
                    Ok(GraphScanControl::Continue)
                }
            )
            .unwrap(),
        GraphScanControl::Continue
    );
    assert_eq!(held.get(), 0);
    rows
}
#[test]
fn canonical_relationship_scan_does_not_repeat_segment_point_reads() {
    let (path, store, _, _) = fixture();
    let before = store.segment_cache_snapshot().unwrap();
    let rows = collect(&store, None);
    let after = store.segment_cache_snapshot().unwrap();
    assert_eq!(rows.len(), 64);
    let lookups = after.hit_count + after.miss_count - before.hit_count - before.miss_count;
    assert!(
        lookups < rows.len() as u64,
        "a segment scan must not perform per-row point reads: {lookups} lookups for {} rows",
        rows.len()
    );
    drop(store);
    std::fs::remove_dir_all(path).unwrap();
}
#[test]
fn canonical_relationship_scan_merges_updates_tombstones_and_delta_in_order() {
    let (path, mut store, mut catalog, source) = fixture();
    store
        .set_relationship_property(
            &mut catalog,
            RelationshipPropertyUpdate {
                source_label: "Source".into(),
                filter: None,
                rel_type: "LINK".into(),
                target_label: "Target".into(),
                target_filter: None,
                rel_filter: Some(PropertyFilter::Eq {
                    property: "rank".into(),
                    value: Value::Int(2),
                }),
                property: "rank".into(),
                value: Value::Int(200),
            },
        )
        .unwrap();
    store
        .delete_nodes(
            &mut catalog,
            "Target",
            Some(&PropertyFilter::Eq {
                property: "id".into(),
                value: Value::Int(4),
            }),
            true,
        )
        .unwrap();
    let target = store
        .create_node(&mut catalog, "Target", BTreeMap::new())
        .unwrap();
    store
        .create_relationship(
            &mut catalog,
            source,
            target,
            "LINK",
            BTreeMap::from([("rank".into(), Value::Int(64))]),
        )
        .unwrap();
    let expected = store
        .relationship_records_owned()
        .collect::<Result<Vec<_>>>()
        .unwrap();
    for rel_type in [
        None,
        catalog.rel_type_id("LINK"),
        catalog.rel_type_id("OTHER"),
    ] {
        assert_eq!(
            collect(&store, rel_type),
            expected
                .iter()
                .filter(|row| rel_type.is_none_or(|selected| selected == row.rel_type))
                .cloned()
                .collect::<Vec<_>>()
        );
    }
    assert!(expected
        .iter()
        .any(|row| row.properties["rank"] == Value::Int(200)));
    assert!(!expected
        .iter()
        .any(|row| row.properties["rank"] == Value::Int(4)));
    drop(store);
    std::fs::remove_dir_all(path).unwrap();
}
#[test]
fn canonical_relationship_scan_stop_and_caller_errors_release_permits_without_poisoning() {
    let (path, store, _, _) = fixture();
    let held = Rc::new(Cell::new(0));
    let mut visited = 0;
    assert_eq!(
        store
            .visit_relationships_with_allocation(None, &mut |_| Ok(None), &mut |_| {
                visited += 1;
                Ok(GraphScanControl::Continue)
            })
            .unwrap(),
        GraphScanControl::Stop
    );
    assert_eq!(visited, 0);
    for short in [false, true] {
        let error = store
            .visit_relationships_with_allocation(
                None,
                &mut |bytes| Ok(Some(permit(if short { bytes - 1 } else { bytes }, &held))),
                &mut |_| Err(HawDBError::Execution("consumer refused".into())),
            )
            .unwrap_err();
        assert!(matches!(error, HawDBError::Execution(_)));
        assert_eq!(held.get(), 0);
        store.ensure_usable().unwrap();
    }
    let error = store
        .visit_relationships_with_allocation(
            None,
            &mut |_| Err(HawDBError::Execution("allocator refused".into())),
            &mut |_| panic!("refused record must not be consumed"),
        )
        .unwrap_err();
    assert!(matches!(error, HawDBError::Execution(message) if message == "allocator refused"));
    assert_eq!(collect(&store, None).len(), 64);
    drop(store);
    std::fs::remove_dir_all(path).unwrap();
}

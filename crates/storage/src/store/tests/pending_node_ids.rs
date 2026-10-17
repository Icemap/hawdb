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
use crate::mutation::MatchedRelationshipCreate;
use crate::predicate::property_filter_matches;
use crate::store::{pending_node_index, PendingNode};

mod scaling;

fn id_eq(id: u64) -> PropertyFilter {
    PropertyFilter::IdEq {
        value: Value::Int(id as i64),
    }
}

fn id_in(ids: &[u64]) -> PropertyFilter {
    PropertyFilter::IdIn {
        values: ids.iter().map(|id| Value::Int(*id as i64)).collect(),
    }
}

#[test]
fn pending_id_lookup_matches_scan_with_gaps_labels_and_signed_ids() {
    let high = i64::MAX as u64 + 1;
    let pending: Vec<PendingNode> = [0, 2, 5, high]
        .into_iter()
        .map(|id| {
            (
                NodeId(id),
                LabelId(u32::from(id == 2)),
                properties([("key", Value::Int(id as i64))]),
            )
        })
        .collect();
    for id in [0, 1, 2, 3, 4, 5, high, high + 1, u64::MAX] {
        assert_eq!(
            pending_node_index(&pending, NodeId(id)),
            pending
                .iter()
                .position(|(candidate, _, _)| candidate.0 == id)
        );
        assert_eq!(pending_node_index(&[], NodeId(id)), None);
    }
    let mut filters = vec![
        None,
        Some(id_in(&[])),
        Some(id_in(&[high, 5, 2, 0, high, 3, 5])),
        Some(id_in(&[3, 4, u64::MAX])),
        Some(PropertyFilter::IdIn {
            values: vec![Value::Float(0.0), Value::Null, Value::String("0".into())],
        }),
        Some(PropertyFilter::And(vec![id_in(&[5, 2, 0])])),
        Some(PropertyFilter::Eq {
            property: "key".into(),
            value: Value::Int(5),
        }),
    ];
    filters.extend(
        [
            Value::Int(0),
            Value::Int(2),
            Value::Int(3),
            Value::Int(i64::MIN),
            Value::Int(-1),
            Value::Float(0.0),
            Value::Null,
        ]
        .into_iter()
        .map(|value| Some(PropertyFilter::IdEq { value })),
    );
    for label in [None, Some(LabelId(0)), Some(LabelId(1)), Some(LabelId(99))] {
        for filter in &filters {
            let expected: Vec<_> = pending
                .iter()
                .filter(|(id, candidate_label, properties)| {
                    label.is_none_or(|label| label == *candidate_label)
                        && filter
                            .as_ref()
                            .is_none_or(|filter| property_filter_matches(filter, id.0, properties))
                })
                .map(|(id, _, _)| *id)
                .collect();
            assert_eq!(
                GraphStore::pending_node_ids_matching(label, filter.as_ref(), &pending),
                expected,
                "label={label:?} filter={filter:?}"
            );
            for limit in [0, 1, 3, 4] {
                let result = GraphStore::default().matching_node_ids_with_pending_bounded(
                    label,
                    filter.as_ref(),
                    &pending,
                    limit,
                    "test_limit",
                );
                if expected.len() > limit {
                    assert_eq!(
                        result.unwrap_err().to_string(),
                        format!("execution error: mutation would exceed test_limit {limit}")
                    );
                } else {
                    assert_eq!(result.unwrap(), expected);
                }
            }
        }
    }
}

fn pending_batch() -> Vec<GraphMutation> {
    let create = |label: &str, score| GraphMutation::CreateNode {
        label: label.into(),
        properties: properties([("score", Value::Int(score))]),
    };
    vec![
        create("Memory", 0),
        create("Memory", 1),
        create("Other", 2),
        create("Memory", 5),
        GraphMutation::DeleteNode {
            label: "Memory".into(),
            filter: Some(id_eq(2)),
            detach: false,
        },
        GraphMutation::SetNodePropertyAddInt {
            label: "Memory".into(),
            filter: Some(id_in(&[4, 1, 4, 3])),
            property: "score".into(),
            amount: 2,
        },
        GraphMutation::CreateRelationshipsBetweenMatches(MatchedRelationshipCreate {
            source_label: "Memory".into(),
            source_filter: Some(id_in(&[4, 1, 0, 4, 2, 3])),
            target_label: "Memory".into(),
            target_filter: Some(id_in(&[4, 1, 4])),
            rel_type: "LINKS".into(),
            rel_properties: properties([]),
        }),
        GraphMutation::SetRelationshipProperty {
            source_label: "Memory".into(),
            filter: Some(id_eq(4)),
            rel_type: "LINKS".into(),
            target_label: "Memory".into(),
            target_filter: Some(id_eq(1)),
            rel_filter: None,
            property: "weight".into(),
            value: Value::Int(7),
        },
    ]
}

#[test]
fn pending_id_batches_preserve_updates_deletion_order_atomicity_and_recovery() {
    for residency_mode in [
        StorageResidencyMode::Materialized,
        StorageResidencyMode::OutOfCore,
    ] {
        let path = unique_test_dir("pending_id_batch");
        let open = |catalog: &mut Catalog| {
            GraphStore::open_with_durability_and_replay_config(
                &path,
                catalog,
                DurabilityPolicy::default(),
                WalReplayConfig {
                    residency_mode,
                    ..WalReplayConfig::default()
                },
            )
            .unwrap()
        };
        let mut catalog = Catalog::default();
        let mut store = open(&mut catalog);
        let first = store
            .create_node(&mut catalog, "Memory", properties([]))
            .unwrap();
        assert_eq!(first, NodeId(0));
        store.checkpoint(&catalog).unwrap();
        drop(store);
        let mut store = open(&mut catalog);
        let snapshot = store.snapshot();
        let before = (store.commit_epoch(), store.next_node_id, store.next_rel_id);
        let limits = MutationLimits {
            max_affected_rows: NonZeroUsize::new(13).unwrap(),
            ..MutationLimits::default()
        };
        let error = store
            .commit_mutations_with_limits(&mut catalog, pending_batch(), limits)
            .unwrap_err();
        assert!(error.to_string().contains("max_mutation_affected_rows"));
        assert_eq!(
            (store.commit_epoch(), store.next_node_id, store.next_rel_id),
            before
        );
        assert_eq!(store.node_count_for_label(None), 1);
        assert_eq!(store.relationship_records_owned().count(), 0);
        assert!(catalog.label_id("Other").is_none());
        assert!(catalog.rel_type_id("LINKS").is_none());
        drop(store);
        store = open(&mut catalog);
        assert_eq!(
            (store.commit_epoch(), store.next_node_id, store.next_rel_id),
            before
        );
        assert_eq!(store.node_count_for_label(None), 1);
        assert_eq!(store.relationship_records_owned().count(), 0);
        assert!(catalog.label_id("Other").is_none());
        assert!(catalog.rel_type_id("LINKS").is_none());
        let limits = MutationLimits {
            max_affected_rows: NonZeroUsize::new(14).unwrap(),
            max_operations: NonZeroUsize::new(9).unwrap(),
            ..MutationLimits::default()
        };
        let result = store
            .commit_mutations_with_limits(&mut catalog, pending_batch(), limits)
            .unwrap();
        assert_eq!(result.rows.len(), 14);
        let expected = [(0, 1), (0, 4), (1, 1), (1, 4), (4, 1), (4, 4)];
        for (index, (row, (source, target))) in result.rows[7..13].iter().zip(expected).enumerate()
        {
            assert_eq!(row["source_node_id"], Value::Int(source));
            assert_eq!(row["target_node_id"], Value::Int(target));
            assert_eq!(row["rel_id"], Value::Int(index as i64));
        }
        assert_eq!(snapshot.node_count_for_label(None), 1);
        assert!(snapshot.node_owned(NodeId(4)).unwrap().is_none());
        assert_eq!(snapshot.relationship_records_owned().count(), 0);
        drop(snapshot);
        for checkpoint in [false, true] {
            if checkpoint {
                store.checkpoint(&catalog).unwrap();
            }
            drop(store);
            store = open(&mut catalog);
            assert_eq!(store.node_count_for_label(None), 4);
            assert!(store.node_owned(NodeId(2)).unwrap().is_none());
            assert_eq!(
                store.node_owned(NodeId(1)).unwrap().unwrap().properties["score"],
                Value::Int(2)
            );
            assert_eq!(
                store.node_owned(NodeId(3)).unwrap().unwrap().properties["score"],
                Value::Int(2)
            );
            assert_eq!(
                store.node_owned(NodeId(4)).unwrap().unwrap().properties["score"],
                Value::Int(7)
            );
            let records: Vec<_> = store
                .relationship_records_owned()
                .map(Result::unwrap)
                .collect();
            let actual: Vec<_> = records
                .iter()
                .map(|record| (record.source.0 as i64, record.target.0 as i64))
                .collect();
            assert_eq!(actual, expected);
            for (index, record) in records.iter().enumerate() {
                assert_eq!(
                    record.properties.get("weight"),
                    (index == 4).then_some(&Value::Int(7))
                );
            }
        }
        drop(store);
        fs::remove_dir_all(path).unwrap();
    }
}

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

fn key(value: i64) -> BTreeMap<String, Value> {
    properties([("key", Value::Int(value))])
}

fn merge(value: i64) -> GraphMutation {
    GraphMutation::MergeNode {
        label: "Memory".into(),
        match_properties: key(value),
        on_create_properties: properties([]),
        on_match_assignments: Vec::new(),
        post_merge_assignments: Vec::new(),
    }
}

fn connected(source: i64, target: i64) -> GraphMutation {
    GraphMutation::MergeConnectedNodes(ConnectedNodesCreate {
        source_label: "Memory".into(),
        source_properties: key(source),
        target_label: "Memory".into(),
        target_properties: key(target),
        rel_type: "LINKS".into(),
        rel_properties: properties([]),
    })
}

fn property_batch() -> Vec<GraphMutation> {
    let increment = || NodeSetAssignment {
        property: "key".into(),
        value: NodeSetValue::AddInt { amount: 1 },
    };
    vec![
        GraphMutation::CreateNode {
            label: "Memory".into(),
            properties: key(100),
        },
        merge(100),
        GraphMutation::MergeNode {
            label: "Memory".into(),
            match_properties: key(1),
            on_create_properties: properties([
                ("key", Value::Int(2)),
                ("payload", Value::String("body".into())),
            ]),
            on_match_assignments: Vec::new(),
            post_merge_assignments: vec![increment()],
        },
        GraphMutation::MergeNode {
            label: "Memory".into(),
            match_properties: key(3),
            on_create_properties: properties([]),
            on_match_assignments: vec![increment()],
            post_merge_assignments: vec![increment()],
        },
        merge(5),
        GraphMutation::SetNodeProperty {
            label: "Memory".into(),
            filter: Some(id_eq(2)),
            property: "key".into(),
            value: Value::Int(8),
        },
        merge(8),
        GraphMutation::CreateNode {
            label: "Memory".into(),
            properties: key(8),
        },
        merge(8),
        GraphMutation::DeleteNode {
            label: "Memory".into(),
            filter: Some(id_eq(2)),
            detach: false,
        },
        merge(8),
        connected(8, 9),
        connected(8, 9),
        GraphMutation::MergeNode {
            label: "Memory".into(),
            match_properties: key(10),
            on_create_properties: properties([("payload", Value::String("extra".into()))]),
            on_match_assignments: Vec::new(),
            post_merge_assignments: Vec::new(),
        },
        connected(10, 10),
        merge(10),
        connected(10, 10),
        GraphMutation::CreateNode {
            label: "Other".into(),
            properties: key(8),
        },
        GraphMutation::MergeNode {
            label: "Other".into(),
            match_properties: key(8),
            on_create_properties: properties([]),
            on_match_assignments: Vec::new(),
            post_merge_assignments: Vec::new(),
        },
    ]
}

fn assert_property_batch_state(store: &GraphStore, catalog: &Catalog) {
    assert_eq!(store.node_count_for_label(None), 8);
    assert!(store.node_owned(NodeId(2)).unwrap().is_none());
    for (id, value) in [
        (0, 100),
        (1, 100),
        (3, 8),
        (4, 9),
        (5, 10),
        (6, 10),
        (7, 10),
        (8, 8),
    ] {
        let node = store.node_owned(NodeId(id)).unwrap().unwrap();
        let mut expected = key(value);
        if id == 0 || id == 5 {
            expected.insert("payload".into(), Value::String("extra".into()));
        }
        assert_eq!(node.properties, expected);
        assert_eq!(
            node.labels,
            BTreeSet::from([catalog
                .label_id(if id == 8 { "Other" } else { "Memory" })
                .unwrap()])
        );
    }
    let endpoints: Vec<_> = store
        .relationship_records_owned()
        .map(|record| {
            let record = record.unwrap();
            (record.source.0, record.target.0)
        })
        .collect();
    assert_eq!(endpoints, [(3, 4), (6, 7), (6, 6)]);
}

#[test]
fn pending_property_merges_preserve_precedence_assignments_atomicity_and_recovery() {
    for residency_mode in [
        StorageResidencyMode::Materialized,
        StorageResidencyMode::OutOfCore,
    ] {
        let path = unique_test_dir("pending_property_batch");
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
        store
            .create_node(
                &mut catalog,
                "Memory",
                properties([
                    ("key", Value::Int(100)),
                    ("payload", Value::String("extra".into())),
                ]),
            )
            .unwrap();
        store
            .create_property_index(&mut catalog, "Memory", "key")
            .unwrap();
        store.checkpoint(&catalog).unwrap();
        drop(store);
        let mut store = open(&mut catalog);
        let snapshot = store.snapshot();
        let before = (store.commit_epoch(), store.next_node_id, store.next_rel_id);
        let error = store
            .commit_mutations_with_limits(
                &mut catalog,
                property_batch(),
                MutationLimits {
                    max_affected_rows: NonZeroUsize::new(18).unwrap(),
                    ..MutationLimits::default()
                },
            )
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
        let result = store
            .commit_mutations_with_limits(
                &mut catalog,
                property_batch(),
                MutationLimits {
                    max_affected_rows: NonZeroUsize::new(19).unwrap(),
                    ..MutationLimits::default()
                },
            )
            .unwrap();
        assert_eq!(result.rows.len(), 19);
        for (row, id, created) in [
            (1, 0, false),
            (2, 2, true),
            (3, 2, false),
            (4, 2, false),
            (6, 2, false),
            (8, 2, false),
            (10, 3, false),
            (13, 5, true),
            (15, 5, false),
            (18, 8, false),
        ] {
            assert_eq!(result.rows[row]["node_id"], Value::Int(id));
            assert_eq!(result.rows[row]["created"], Value::Bool(created));
        }
        for (row, source, target, rel, created) in [
            (11, 3, 4, 0, true),
            (12, 3, 4, 0, false),
            (14, 6, 7, 1, true),
            (16, 6, 6, 2, true),
        ] {
            assert_eq!(result.rows[row]["source_node_id"], Value::Int(source));
            assert_eq!(result.rows[row]["target_node_id"], Value::Int(target));
            assert_eq!(result.rows[row]["rel_id"], Value::Int(rel));
            assert_eq!(result.rows[row]["created"], Value::Bool(created));
        }
        assert_eq!(snapshot.node_count_for_label(None), 1);
        assert_eq!(snapshot.relationship_records_owned().count(), 0);
        assert!(snapshot.node_owned(NodeId(3)).unwrap().is_none());
        drop(snapshot);
        assert_property_batch_state(&store, &catalog);
        for checkpoint in [false, true] {
            if checkpoint {
                store.checkpoint(&catalog).unwrap();
            }
            drop(store);
            store = open(&mut catalog);
            assert_property_batch_state(&store, &catalog);
        }
        drop(store);
        fs::remove_dir_all(path).unwrap();
    }
}

// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

use super::*;

fn owner_record(store: &GraphStore) -> &CapturedProjectionChange {
    store.search_projection_graph_changes.last().unwrap()
}
fn expected(id: NodeId, label: LabelId, before_id: OwnerIdV1) -> Vec<OwnerEffectV1> {
    vec![OwnerEffectV1 {
        node_id: id.0,
        before: OwnerBeforeV1::Present {
            labels: vec![label.0],
            id: before_id,
        },
    }]
}

#[test]
fn owner_only_thread_delete_survives_wal_checkpoint_and_both_residencies() {
    for mode in [
        StorageResidencyMode::Materialized,
        StorageResidencyMode::OutOfCore,
    ] {
        for checkpoint_deleted in [false, true] {
            let path = super::tests::directory();
            let mut catalog = Catalog::default();
            let mut store = GraphStore::open_with_durability_and_replay_config(
                &path,
                &mut catalog,
                Default::default(),
                WalReplayConfig {
                    residency_mode: mode,
                    ..Default::default()
                },
            )
            .unwrap();
            store.create_node_table(&mut catalog, "Thread").unwrap();
            let id = store
                .create_node(
                    &mut catalog,
                    "Thread",
                    BTreeMap::from([("id".into(), Value::String("old-thread".into()))]),
                )
                .unwrap();
            let label = catalog.label_id("Thread").unwrap();
            store.checkpoint(&catalog).unwrap();
            drop(store);
            store = GraphStore::open_with_durability_and_replay_config(
                &path,
                &mut catalog,
                Default::default(),
                WalReplayConfig {
                    residency_mode: mode,
                    ..Default::default()
                },
            )
            .unwrap();
            let c = store.snapshot();
            let cursor = store.commit_epoch();
            store
                .delete_nodes(&mut catalog, "Thread", None, false)
                .unwrap();
            let epoch = store.commit_epoch();
            let effects = expected(id, label, OwnerIdV1::String("old-thread".into()));
            assert_eq!(owner_record(&store).owners_v1.as_ref().unwrap(), &effects);
            assert_eq!(owner_record(&store).commit_epoch, epoch);
            assert!(owner_record(&store).upsert_node_ids.is_empty());
            assert!(owner_record(&store).delete_document_ids.is_empty());
            assert!(store.search_projection_changes_after(cursor).is_empty());
            assert_eq!(c.commit_epoch(), cursor);
            assert_ne!(owner_record(&c).owners_v1, owner_record(&store).owners_v1);
            if checkpoint_deleted {
                store.checkpoint(&catalog).unwrap();
            }
            let floor = store.search_projection_graph_changes.owner_floor();
            drop(c);
            drop(store);
            let recovered = GraphStore::open_with_durability_and_replay_config(
                &path,
                &mut catalog,
                Default::default(),
                WalReplayConfig {
                    residency_mode: mode,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(recovered.commit_epoch(), epoch);
            assert_eq!(
                recovered.search_projection_graph_changes.owner_floor(),
                floor
            );
            assert_eq!(
                owner_record(&recovered).owners_v1.as_ref().unwrap(),
                &effects
            );
            assert!(recovered.search_projection_changes_after(cursor).is_empty());
            drop(recovered);
            std::fs::remove_dir_all(path).unwrap();
        }
    }
}

#[test]
fn owner_identity_is_transaction_preimage_not_an_intermediate_rename() {
    let mut store = GraphStore::in_memory();
    let mut catalog = Catalog::default();
    let id = store
        .create_node(
            &mut catalog,
            "Thread",
            BTreeMap::from([("id".into(), Value::String("old".into()))]),
        )
        .unwrap();
    let label = catalog.label_id("Thread").unwrap();
    for ops in [
        vec![
            WalOp::SetNodeProperty {
                id,
                property: "id".into(),
                value: Value::String("middle".into()),
            },
            WalOp::DeleteNode { id },
        ],
        vec![
            WalOp::SetNodeProperty {
                id,
                property: "body".into(),
                value: Value::String("body".into()),
            },
            WalOp::SetNodeProperty {
                id,
                property: "id".into(),
                value: Value::String("final".into()),
            },
        ],
    ] {
        let PreparedSearchProjectionChange::Complete(Some(change)) = store
            .prepare_search_projection_changes_for_ops(
                &catalog,
                store.commit_epoch() + 1,
                &ops,
                None,
            )
            .unwrap()
        else {
            panic!("expected complete capture");
        };
        assert_eq!(
            change.owners_v1.as_ref().unwrap(),
            &expected(id, label, OwnerIdV1::String("old".into()))
        );
        assert_eq!(change.upsert_node_ids, [id.0]);
    }
    store
        .set_node_property(
            &mut catalog,
            "Thread",
            None,
            "body",
            Value::String("x".into()),
        )
        .unwrap();
    assert_eq!(
        owner_record(&store).owners_v1.as_ref().unwrap()[0].before,
        OwnerBeforeV1::NotRecorded
    );
}

#[test]
fn owner_created_deleted_and_unsupported_ids_are_explicit() {
    for value in [
        None,
        Some(Value::Int(7)),
        Some(Value::String("x\t\nλ".into())),
    ] {
        let mut store = GraphStore::in_memory();
        let mut catalog = Catalog::default();
        let props = value
            .clone()
            .map(|value| BTreeMap::from([("id".into(), value)]))
            .unwrap_or_default();
        let id = store.create_node(&mut catalog, "Thread", props).unwrap();
        assert_eq!(
            owner_record(&store).owners_v1.as_ref().unwrap()[0].before,
            OwnerBeforeV1::Absent
        );
        store
            .delete_nodes(&mut catalog, "Thread", None, false)
            .unwrap();
        let before_id = match value {
            None => OwnerIdV1::Absent,
            Some(Value::String(id)) => OwnerIdV1::String(id),
            _ => OwnerIdV1::Unsupported,
        };
        assert_eq!(
            owner_record(&store).owners_v1.as_ref().unwrap(),
            &expected(id, catalog.label_id("Thread").unwrap(), before_id)
        );
    }
}

#[test]
fn owner_floors_advance_on_gap_trim_and_do_not_follow_live_mutation() {
    let mut store = GraphStore::in_memory();
    let mut catalog = Catalog::default();
    store
        .create_node(&mut catalog, "Thread", BTreeMap::new())
        .unwrap();
    let snapshot = store.snapshot();
    let c = store.commit_epoch();
    let floor = snapshot.search_projection_graph_changes.owner_floor();
    store.set_max_search_projection_change_log_entries(Some(0));
    assert!(store.search_projection_graph_changes.owner_floor() >= c);
    assert_eq!(
        snapshot.search_projection_graph_changes.owner_floor(),
        floor
    );
    store.install_search_projection_change(PreparedSearchProjectionChange::Gap(c + 1));
    assert_eq!(store.search_projection_graph_changes.owner_floor(), c + 1);
    assert_eq!(
        snapshot.search_projection_graph_changes.owner_floor(),
        floor
    );
}

// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::cow::CowPageWeight;
use crate::test_allocator::{AllocationObservation, AllocationStats};
use crate::version::{VersionIndex, VersionKey, VersionStamp};

fn update(id: u64, value: i64) -> GraphMutation {
    GraphMutation::SetNodeProperty {
        label: "Memory".into(),
        filter: Some(PropertyFilter::IdEq {
            value: Value::Int(i64::try_from(id).unwrap()),
        }),
        property: "value".into(),
        value: Value::Int(value),
    }
}

fn stats(stats: AllocationStats) -> serde_json::Value {
    serde_json::json!({
        "allocation_calls": stats.allocations,
        "allocated_bytes": stats.allocated_bytes,
        "deallocation_calls": stats.deallocations,
        "deallocated_bytes": stats.deallocated_bytes,
    })
}

// Run each profile in its own process when recording RSS. All observed work
// runs on this thread; fixture construction and JSON output are not observed.
fn allocation_profile(rows: usize, age: usize) {
    const BODY_BYTES: usize = 16 * 1024;
    let mut catalog = Catalog::default();
    let mut live = GraphStore::default();
    for id in 0..rows {
        live.create_node(
            &mut catalog,
            "Memory",
            BTreeMap::from([
                ("id".into(), Value::Int(i64::try_from(id).unwrap())),
                ("value".into(), Value::Int(0)),
                ("body".into(), Value::String("x".repeat(BODY_BYTES))),
            ]),
        )
        .unwrap();
    }
    live.checkpoint(&catalog).unwrap();
    let pins = live.version_snapshot_pins.clone();
    let before = AllocationObservation::start_all();
    let reader = live.snapshot_for_read();
    let capture = before.finish_stats();
    assert!(reader.nodes.shares_storage_with(&live.nodes));
    assert!(capture.allocated_bytes < BODY_BYTES);
    let base_epoch = reader.commit_epoch();
    let mut roots = Vec::with_capacity(age);
    let mut shared_segments = Vec::with_capacity(age);
    let before = AllocationObservation::start_all();
    for value in 1..=age {
        live.commit_mutations(&mut catalog, vec![update(0, i64::try_from(value).unwrap())])
            .unwrap();
        // A changed directory/page still shares every untouched row owner.
        assert!(std::ptr::eq(
            live.nodes.get(&NodeId(1)).unwrap(),
            reader.nodes.get(&NodeId(1)).unwrap(),
        ));
        shared_segments.push(reader.nodes.shared_segment_count_with(&live.nodes));
        roots.push(live.snapshot());
    }
    let writes = before.finish_stats();
    let current_payload =
        live.version_index.estimated_bytes() - VersionIndex::default().estimated_bytes();
    let retained_payload = live.version_index.retained_history_bytes();
    assert_eq!(retained_payload, age * current_payload);
    assert!(retained_payload > 0);
    let counter = live.version_index.read_baseline(live.commit_epoch());
    assert_eq!(
        reader.node(NodeId(0)).unwrap().properties["value"],
        Value::Int(0)
    );
    assert_eq!(pins.oldest_epoch(), Some(base_epoch));
    // Closing the producer cannot retire snapshots or their history charges.
    drop(live);
    assert_eq!(counter.retained_history_bytes(), retained_payload);
    assert_eq!(reader.node_count_for_label(None), rows);
    let before = AllocationObservation::start_all();
    drop(roots);
    drop(reader);
    let retired = before.finish_stats();
    assert_eq!(counter.retained_history_bytes(), 0);
    assert_eq!(pins.oldest_epoch(), None);
    assert!(retired.deallocated_bytes >= rows * BODY_BYTES);
    println!(
        "cow_snapshot_retention {}",
        serde_json::json!({
            "rows": rows,
            "body_bytes": BODY_BYTES,
            "reader_age_commits": age,
            "capture": stats(capture),
            "updates_and_precise_snapshots": stats(writes),
            "retirement": stats(retired),
            "base_epoch": base_epoch,
            "current_version_payload_estimate": current_payload,
            "retained_version_payload_estimate": retained_payload,
            "retained_version_payload_after_retirement": counter.retained_history_bytes(),
            "shared_segments_after_each_update": shared_segments,
            "untouched_row_owner_shared": true,
            "scope": "in-memory GraphStore; calling-thread allocator traffic; conflict payload estimates exclude allocator overhead and canonical values; RSS recorded separately",
        }),
    );
}

#[test]
fn cow_snapshot_profile_small_young() {
    allocation_profile(64, 1);
}

#[test]
fn cow_snapshot_profile_small_old() {
    allocation_profile(64, 4);
}

#[test]
fn cow_snapshot_profile_large_young() {
    allocation_profile(512, 1);
}

#[test]
fn cow_snapshot_profile_large_old() {
    allocation_profile(512, 4);
}

#[test]
fn restored_savepoint_preserves_conflicts_after_delete_reinsert_and_pruning() {
    let mut catalog = Catalog::default();
    let mut live = GraphStore::default();
    for id in 0..2 {
        live.create_node(
            &mut catalog,
            "Memory",
            BTreeMap::from([
                ("id".into(), Value::Int(id)),
                ("value".into(), Value::Int(0)),
            ]),
        )
        .unwrap();
    }
    live.checkpoint(&catalog).unwrap();
    let reserve = VersionIndex::default().estimated_bytes();
    let weight =
        VersionKey::GraphNode(NodeId(0)).cow_page_bytes() + std::mem::size_of::<VersionStamp>();
    live.version_index = VersionIndex::with_history_limit(reserve + 3 * weight, 6 * weight);
    let epoch = live.commit_epoch();
    let mut stale = live.begin_mutation_transaction(&catalog);
    for value in 1..=3 {
        stale
            .stage_mutation_with_limits(update(0, value), MutationLimits::default())
            .unwrap();
    }
    let saved = stale.savepoint();
    stale
        .stage_mutation_with_limits(update(1, 99), MutationLimits::default())
        .unwrap();
    stale.restore(saved);
    assert_eq!(stale.store.commit_epoch(), epoch + 3);
    // The restored private epoch is newer than a real conflicting commit.
    // Reclaiming at that private epoch would erase the evidence it still needs.
    live.commit_mutations(
        &mut catalog,
        vec![
            GraphMutation::DeleteNode {
                label: "Memory".into(),
                filter: Some(PropertyFilter::IdEq {
                    value: Value::Int(0),
                }),
                detach: true,
            },
            GraphMutation::CreateNode {
                label: "Memory".into(),
                properties: BTreeMap::from([
                    ("id".into(), Value::Int(0)),
                    ("value".into(), Value::Int(8)),
                ]),
            },
        ],
    )
    .unwrap();
    live.checkpoint(&catalog).unwrap();
    assert_eq!(live.version_snapshot_pins.oldest_epoch(), Some(epoch));
    assert!(live
        .version_index
        .stamp(&VersionKey::GraphNode(NodeId(0)))
        .is_some());
    let before = live.commit_epoch();
    let error = live
        .commit_mutation_transaction_and_relational(
            &mut catalog,
            stale,
            RelationalTransaction::default(),
            MutationLimits::default(),
        )
        .unwrap_err();
    assert!(error.is_retryable_transaction_conflict());
    assert_eq!(live.commit_epoch(), before);
    live.reclaim_version_history();
    assert_eq!(live.version_index.retained_history_bytes(), 0);
    assert_eq!(
        live.node(NodeId(1)).unwrap().properties["value"],
        Value::Int(0)
    );
    assert_eq!(
        live.node(NodeId(2)).unwrap().properties["value"],
        Value::Int(8)
    );
    // After history removal, new disjoint workspaces still commit.
    let mut left = live.begin_mutation_transaction(&catalog);
    let mut right = live.begin_mutation_transaction(&catalog);
    left.stage_mutation_with_limits(update(1, 1), MutationLimits::default())
        .unwrap();
    right
        .stage_mutation_with_limits(update(2, 2), MutationLimits::default())
        .unwrap();
    for tx in [left, right] {
        live.commit_mutation_transaction_and_relational(
            &mut catalog,
            tx,
            RelationalTransaction::default(),
            MutationLimits::default(),
        )
        .unwrap();
    }
    live.checkpoint(&catalog).unwrap();
    assert_eq!(live.version_snapshot_pins.oldest_epoch(), None);
    assert_eq!(live.version_index.retained_history_bytes(), 0);
}

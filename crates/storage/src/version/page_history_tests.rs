// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::NodeId;

fn writes(first: u64, end: u64) -> VersionWriteSet {
    let mut writes = VersionWriteSet::default();
    for id in first..end {
        writes
            .record_live(VersionKey::GraphNode(NodeId(id)))
            .unwrap();
    }
    writes
}

#[test]
fn disjoint_roots_charge_shared_pages_once_and_refund_after_last_page_owner() {
    let weight = version_stamp_bytes(&VersionKey::GraphNode(NodeId(0)));
    let mut first = VersionIndex::from_live_keys_at_epoch(
        (0..1536).map(|id| VersionKey::GraphNode(NodeId(id))),
        1,
    );
    first.max_estimated_bytes = inline_barrier_bytes() + 1536 * weight;
    first.history_budget = HistoryBudget::new(2048 * weight);
    first = first.stage(&VersionWriteSet::default(), 1).unwrap();
    assert_eq!(first.stamps.segment_count(), 3);
    let budget = first.history_budget.clone();
    assert_eq!(budget.used(), 1536 * weight);
    let second = first.stage(&writes(0, 1), 2).unwrap();
    assert_eq!(budget.used(), 2048 * weight);
    assert_eq!(second.stamps.shared_segment_count_with(&first.stamps), 2);
    assert_eq!(
        first
            .stamp(&VersionKey::GraphNode(NodeId(0)))
            .unwrap()
            .commit_epoch,
        1
    );
    assert!(second.stage(&writes(512, 513), 3).is_none());
    drop(first);
    assert_eq!(budget.used(), 1536 * weight);
    let third = second.stage(&writes(512, 513), 3).unwrap();
    assert_eq!(budget.used(), 2048 * weight);
    drop(second);
    assert_eq!(budget.used(), 1536 * weight);
    drop(third);
    assert_eq!(budget.used(), 0);
}

#[test]
fn whole_page_pruning_progresses_at_full_budget_without_detaching_kept_pages() {
    let mut source = VersionIndex::from_live_keys_at_epoch(
        (0..1536).map(|id| VersionKey::GraphNode(NodeId(id))),
        1,
    );
    source.apply(&writes(512, 1024), 5);
    source.apply(&writes(1024, 1536), 10);
    let bytes = source.estimated_bytes() - inline_barrier_bytes();
    source.history_budget = HistoryBudget::new(bytes);
    let mut current = source.stage(&VersionWriteSet::default(), 10).unwrap();
    let budget = current.history_budget.clone();
    assert_eq!(budget.used(), bytes);
    current.prune_before(5);
    assert_eq!(current.stamps.len(), 1024);
    assert_eq!(current.stamps.shared_segment_count_with(&source.stamps), 2);
    assert!(current.stamp(&VersionKey::GraphNode(NodeId(0))).is_none());
    assert_eq!(
        source
            .stamp(&VersionKey::GraphNode(NodeId(0)))
            .unwrap()
            .commit_epoch,
        1
    );
    assert_eq!(budget.used(), bytes);
    // The formerly unmanaged source owns the charged old page too.
    drop(source);
    assert_eq!(budget.used(), bytes * 2 / 3);
    assert!(current.history_reclaimed_after(1));
    drop(current);
    assert_eq!(budget.used(), 0);
}

#[test]
fn changed_page_invalidates_cached_epoch_bounds_before_pruning() {
    let index = VersionIndex::default().stage(&writes(0, 1024), 1).unwrap();
    let _ = index
        .stamps
        .version_retention_plan(&|stamp| stamp.commit_epoch >= 5, Some(5));
    let mut current = index.stage(&writes(0, 1), 10).unwrap();
    current.prune_before(5);
    assert_eq!(current.stamps.len(), 1);
    assert_eq!(
        current
            .stamp(&VersionKey::GraphNode(NodeId(0)))
            .unwrap()
            .commit_epoch,
        10
    );
    assert!(current.stamp(&VersionKey::GraphNode(NodeId(1))).is_none());
    assert_eq!(index.stamps.len(), 1024);
}

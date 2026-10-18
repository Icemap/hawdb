// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn budget(bytes: usize) -> NonZeroUsize {
    NonZeroUsize::new(bytes).unwrap()
}

fn resource(error: HawDBError) -> ResourceBudgetError {
    let HawDBError::ResourceBudgetExceeded(error) = error else {
        panic!("expected resource exhaustion, got {error:?}");
    };
    *error
}

#[test]
fn account_refusal_preserves_text_origin_and_all_counters() {
    let ledger = QueryMemoryLedger::new(budget(16));
    let account = ledger.account_with_budget_option(
        QueryMemoryClass::BlockingState,
        "sort",
        budget(8),
        "ExecutionMemoryConfig.blocking_operator_bytes",
    );
    let lease = account.reserve(6).unwrap();
    let before = ledger.snapshot();
    let error = account.reserve(3).unwrap_err();
    let message =
        "query memory account sort (blocking_state) would use 9 bytes, exceeding its 8-byte budget";
    assert_eq!(error.to_string(), format!("execution error: {message}"));
    assert!(std::error::Error::source(&error)
        .unwrap()
        .is::<ResourceBudgetError>());
    assert_eq!(
        resource(error),
        ResourceBudgetError {
            resource: ResourceBudgetKind::MemoryBytes,
            scope: ResourceBudgetScope::Account,
            owner: "sort".into(),
            memory_class: Some("blocking_state"),
            requested: 3,
            used: 6,
            budget: 8,
            budget_option: "ExecutionMemoryConfig.blocking_operator_bytes",
            message: message.into(),
        }
    );
    assert_eq!(ledger.snapshot(), before);
    assert_eq!(account.peak_bytes(), 6);
    drop(lease);
    assert_eq!(ledger.snapshot().used_bytes, 0);
}

#[test]
fn query_refusal_reports_shared_usage_and_configured_origin() {
    let memory = crate::ExecutionMemoryConfig {
        query_memory_bytes: budget(8),
        ..Default::default()
    };
    let ledger = QueryMemoryLedger::for_query(&memory, None).unwrap();
    let left = ledger.account(QueryMemoryClass::BlockingState, "left", budget(8));
    let right = ledger.account(QueryMemoryClass::PipelineBatch, "right", budget(8));
    let lease = left.reserve(6).unwrap();
    let before = ledger.snapshot();
    let error = resource(right.reserve(3).unwrap_err());
    assert_eq!(error.scope, ResourceBudgetScope::Query);
    assert_eq!((error.requested, error.used, error.budget), (3, 6, 8));
    assert_eq!(error.owner, "right");
    assert_eq!(error.memory_class, Some("pipeline_batch"));
    assert_eq!(
        error.budget_option,
        "ExecutionMemoryConfig.query_memory_bytes"
    );
    assert_eq!(ledger.snapshot(), before);
    assert_eq!(right.peak_bytes(), 0);
    drop(lease);
}

#[test]
fn transfer_refusal_reports_account_charge_or_net_query_charge() {
    for (target_budget, scope, requested, used) in [
        (5, ResourceBudgetScope::Account, 3, 4),
        (16, ResourceBudgetScope::Query, 1, 16),
    ] {
        let ledger = QueryMemoryLedger::new_with_budget_option(budget(16), "query grant");
        let source = ledger.account(QueryMemoryClass::BlockingState, "source", budget(16));
        let target = ledger.account_with_budget_option(
            QueryMemoryClass::PipelineBatch,
            "target",
            budget(target_budget),
            "batch grant",
        );
        let other = ledger.account(QueryMemoryClass::ExternalRead, "other", budget(16));
        let mut left = source.reserve(6).unwrap();
        let mut right = target.reserve(4).unwrap();
        let other_lease = other.reserve(6).unwrap();
        let before = ledger.snapshot();
        let error = resource(left.transfer_to(2, &mut right, 3).unwrap_err());
        assert_eq!(error.scope, scope);
        assert_eq!(
            (error.requested, error.used, error.budget),
            (requested, used, target_budget as u64)
        );
        assert_eq!(
            error.budget_option,
            if scope == ResourceBudgetScope::Account {
                "batch grant"
            } else {
                "query grant"
            }
        );
        assert_eq!(error.owner, "target");
        assert_eq!((left.bytes(), right.bytes()), (6, 4));
        assert_eq!(target.peak_bytes(), 4);
        assert_eq!(ledger.snapshot(), before);
        drop((left, right, other_lease));
        assert_eq!(ledger.snapshot().used_bytes, 0);
    }
}

#[test]
fn overflowing_account_and_query_charges_are_typed_and_atomic() {
    let ledger = QueryMemoryLedger::new(budget(usize::MAX));
    let full = ledger.account(QueryMemoryClass::BlockingState, "full", budget(usize::MAX));
    let empty = ledger.account(QueryMemoryClass::PipelineBatch, "empty", budget(usize::MAX));
    let lease = full.reserve(usize::MAX).unwrap();
    let before = ledger.snapshot();
    for (account, scope) in [
        (&full, ResourceBudgetScope::Account),
        (&empty, ResourceBudgetScope::Query),
    ] {
        let error = resource(account.reserve(1).unwrap_err());
        assert_eq!(error.scope, scope);
        assert_eq!(
            (error.requested, error.used, error.budget),
            (1, usize::MAX as u64, usize::MAX as u64)
        );
        assert!(error.message.contains("byte accounting overflow"));
        assert_eq!(ledger.snapshot(), before);
    }
    drop(lease);
    assert_eq!(ledger.snapshot().used_bytes, 0);
}

#[test]
fn child_refusal_names_its_grant_and_keeps_parent_backing_alive() {
    let ledger = QueryMemoryLedger::new(budget(8));
    let parent = ledger.account(QueryMemoryClass::BlockingState, "merge", budget(8));
    let child = parent.sub_account(budget(8)).unwrap();
    let mut lease = child.reserve(8).unwrap();
    let error = resource(lease.grow(1).unwrap_err());
    assert_eq!(
        error.budget_option,
        "QueryMemoryAccount::sub_account(budget)"
    );
    assert_eq!(error.scope, ResourceBudgetScope::Account);
    assert_eq!((error.requested, error.used, error.budget), (1, 8, 8));
    drop((parent, child));
    assert_eq!(ledger.snapshot().used_bytes, 8);
    drop(lease);
    assert_eq!(ledger.snapshot().used_bytes, 0);
}

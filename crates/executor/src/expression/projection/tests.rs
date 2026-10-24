// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{QueryMemoryClass, QueryMemoryLedger};
use std::cell::Cell;
use std::num::NonZeroUsize;

fn size(bytes: usize) -> NonZeroUsize {
    NonZeroUsize::new(bytes).unwrap()
}

fn items(values: &[Value]) -> Vec<Projection> {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| Projection {
            name: format!("column_{index}"),
            expression: ProjectionExpression::Literal(value.clone()),
        })
        .collect()
}

fn evaluate(expression: &ProjectionExpression) -> Result<ProjectedValue<'_>> {
    let ProjectionExpression::Literal(value) = expression else {
        panic!("literal fixture expected")
    };
    Ok(ProjectedValue::borrowed(value))
}

fn run(items: &[Projection], small: bool, payload: usize) -> Result<Vec<Binding>> {
    let ledger = QueryMemoryLedger::new(size(1024 * 1024));
    let working = ledger.account(QueryMemoryClass::ExternalRead, "projection", size(65536));
    let mut batch = AccountedBindingBatch::with_ledger("projection", 16, size(payload), &ledger);
    let mut rows = Vec::new();
    let mut emit = |bindings: BindingBatch| {
        rows.extend(bindings);
        Ok(BatchControl::Continue)
    };
    let result = if small {
        SmallBorrowedProjection::new(items)
            .unwrap()
            .push(&mut batch, &working, evaluate, None, &mut emit)
    } else {
        push_borrowed_projection(
            &mut batch,
            || prepare_borrowed_projection(items, &working, None, evaluate),
            None,
            &mut emit,
        )
    };
    assert_eq!(ledger.snapshot().used_bytes == 0, result.is_err());
    result?;
    batch.emit(&mut emit)?;
    assert_eq!(ledger.snapshot().used_bytes, 0);
    Ok(rows)
}

#[test]
fn small_projection_matches_general_values_and_refunds() {
    let values = [
        Value::Null,
        Value::Int(-7),
        Value::Float(2.5),
        Value::Bool(true),
        Value::String("Unicode: \u{03bb}".into()),
        Value::List(vec![Value::Int(1), Value::Null]),
        Value::Map(BTreeMap::from([(
            "nested".into(),
            Value::String("value".into()),
        )])),
        Value::String("x".repeat(1024)),
    ];
    for count in 1..=SMALL_PROJECTION_CAPACITY {
        let items = items(&values[..count]);
        assert_eq!(
            SmallBorrowedProjection::new(&items)
                .unwrap()
                .preparation_bytes,
            projection_preparation_bytes(&items, None)
        );
        assert_eq!(
            run(&items, true, 65536).unwrap(),
            run(&items, false, 65536).unwrap()
        );
    }
}

#[test]
fn duplicate_and_wide_projections_keep_general_naming() {
    assert!(SmallBorrowedProjection::new(&[]).is_none());
    let computed = [Projection {
        name: "node".into(),
        expression: ProjectionExpression::Variable {
            variable: "n".into(),
        },
    }];
    assert!(SmallBorrowedProjection::new(&computed).is_none());
    let mut duplicate = items(&[Value::Int(1), Value::Int(2), Value::Int(3)]);
    duplicate[0].name = "value".into();
    duplicate[1].name = "value".into();
    duplicate[2].name = "value#2".into();
    assert!(SmallBorrowedProjection::new(&duplicate).is_none());
    let rows = run(&duplicate, false, 65536).unwrap();
    assert_eq!(
        rows[0].values,
        BTreeMap::from([
            ("value".into(), Value::Int(1)),
            ("value#2".into(), Value::Int(2)),
            ("value#2#2".into(), Value::Int(3))
        ])
    );
    assert!(SmallBorrowedProjection::new(&items(&vec![Value::Null; 9])).is_none());
}

#[test]
fn small_projection_preserves_wide_row_refusal() {
    let items = items(&[Value::String("x".repeat(16384)), Value::Null, Value::Int(3)]);
    let small = run(&items, true, 1024).unwrap_err();
    let general = run(&items, false, 1024).unwrap_err();
    assert_eq!(small.to_string(), general.to_string());
    assert!(small.to_string().contains("exceeding batch_payload_bytes"));
}

#[test]
fn byte_boundary_stop_releases_preparation_without_repeating_it() {
    let items = items(&[Value::String("x".repeat(64))]);
    let row = run(&items, false, 65536).unwrap().pop().unwrap();
    let payload = crate::binding::binding_memory_bytes(&row) * 2 - 1;
    let ledger = QueryMemoryLedger::new(size(65536));
    let working = ledger.account(QueryMemoryClass::ExternalRead, "projection", size(16384));
    let mut batch = AccountedBindingBatch::with_ledger("projection", 16, size(payload), &ledger);
    let projection = SmallBorrowedProjection::new(&items).unwrap();
    let calls = Cell::new(0);
    let mut evaluate = |expression| {
        calls.set(calls.get() + 1);
        evaluate(expression)
    };
    let mut emit = |rows: BindingBatch| {
        assert_eq!(rows.len(), 1);
        assert_eq!(ledger.snapshot().used_bytes, 0);
        Ok(BatchControl::Stop)
    };
    assert_eq!(
        projection
            .push(&mut batch, &working, &mut evaluate, None, &mut emit)
            .unwrap(),
        BatchControl::Continue
    );
    assert_eq!(
        projection
            .push(&mut batch, &working, &mut evaluate, None, &mut emit)
            .unwrap(),
        BatchControl::Stop
    );
    assert_eq!(calls.get(), 2);
    assert!(batch.is_empty());
    assert_eq!(ledger.snapshot().used_bytes, 0);
}

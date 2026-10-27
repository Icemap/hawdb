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
use crate::columnar::{
    BindingSchema, ColumnVector, ColumnarBatch, SlotDescriptor, SlotId, SlotType, Validity,
};
use hawdb_core::{LogicalType, ValueRef};
use std::cell::Cell;
use std::sync::Arc;

fn text_batch(names: &[&str], values: &[&str]) -> ColumnarBatch {
    let schema = BindingSchema::try_new(
        names
            .iter()
            .enumerate()
            .map(|(index, name)| SlotDescriptor {
                id: SlotId(index as u32),
                name: (*name).into(),
                slot_type: SlotType::logical(LogicalType::Text),
            })
            .collect(),
    )
    .unwrap();
    ColumnarBatch::try_new(
        Arc::new(schema),
        names
            .iter()
            .map(|_| {
                Arc::new(ColumnVector::Utf8 {
                    values: values
                        .iter()
                        .map(|value| (*value).into())
                        .collect::<Vec<_>>()
                        .into(),
                    validity: Validity::all(values.len()),
                })
            })
            .collect(),
    )
    .unwrap()
}

#[test]
fn immediate_borrowed_delivery_preserves_native_payload_identity() {
    let batch = text_batch(&["value"], &["first", "second"]);
    let source = batch
        .rows()
        .map(|row| row.get("value").unwrap().as_str().unwrap().as_ptr())
        .collect::<Vec<_>>();
    let ledger = QueryMemoryLedger::new(NonZeroUsize::new(4096).unwrap());
    let mut calls = 0;
    let mut consumer = |row: RowRef<'_>| {
        assert!(matches!(row, RowRef::Columnar(_)));
        assert_eq!(
            row.get("value").unwrap().as_str().unwrap().as_ptr(),
            source[calls]
        );
        calls += 1;
        Ok(())
    };
    let mut output = QueryOutputAccumulator::new_borrowed_with_budget_option(
        OutputLimits::default(),
        NonZeroUsize::MIN,
        &ledger,
        "borrowed-test",
        StreamDelivery::Validated,
        &mut consumer,
    )
    .unwrap();
    for row in batch.rows() {
        output.emit_borrowed(row.into()).unwrap();
    }
    assert_eq!(output.metrics().rows, 2);
    assert_eq!(output.metrics().payload_bytes, 21);
    assert_eq!(ledger.snapshot().peak_bytes, 0);
    output.finish_delivery(None).unwrap();
    drop(output);
    assert_eq!(calls, 2);
}

#[test]
fn bounded_borrowed_delivery_owns_compact_values_across_source_retirement() {
    let memory = ExecutionMemoryConfig::default();
    let ledger = QueryMemoryLedger::new(memory.query_memory_bytes);
    let calls = Cell::new(0);
    let schema = Cell::new(None);
    let mut consumer = |row: RowRef<'_>| {
        assert!(matches!(row, RowRef::Compact(_)));
        assert!(ledger.snapshot().used_bytes > 0);
        let (name, value) = row.column(0).unwrap();
        if let Some(previous) = schema.get() {
            assert_eq!(name.as_ptr(), previous);
        }
        schema.set(Some(name.as_ptr()));
        assert_eq!(name, "value");
        assert_eq!(
            value.as_str(),
            Some(if calls.get() == 0 {
                "moved"
            } else {
                "borrowed"
            })
        );
        calls.set(calls.get() + 1);
        Ok(())
    };
    let mut output = QueryOutputAccumulator::new_borrowed_with_budget_option(
        OutputLimits {
            max_rows: Some(513),
            max_payload_bytes: None,
        },
        memory.query_memory_bytes,
        &ledger,
        "borrowed-test",
        StreamDelivery::Validated,
        &mut consumer,
    )
    .unwrap();
    output.emit(binding("moved")).unwrap();
    {
        let batch = text_batch(&["value"], &["borrowed"]);
        for _ in 0..512 {
            output
                .emit_borrowed(batch.rows().next().unwrap().into())
                .unwrap();
        }
    }
    assert_eq!(calls.get(), 0);
    assert_eq!(output.metrics().rows, 513);
    assert!(output.take_owned_rows().is_none());
    output.finish_delivery(None).unwrap();
    assert_eq!(calls.get(), 513);
    assert_eq!(ledger.snapshot().used_bytes, 0);
    drop(output);
    assert_eq!(ledger.snapshot().used_bytes, 0);
}

#[test]
fn borrowed_output_refusals_never_publish_a_validated_prefix() {
    for limits in [
        OutputLimits {
            max_rows: Some(1),
            max_payload_bytes: None,
        },
        OutputLimits {
            max_rows: None,
            max_payload_bytes: Some(10),
        },
    ] {
        let batch = text_batch(&["value"], &["first", "second"]);
        let ledger = QueryMemoryLedger::new(NonZeroUsize::new(8192).unwrap());
        let calls = Cell::new(0);
        let mut consumer = |_: RowRef<'_>| {
            calls.set(calls.get() + 1);
            Ok(())
        };
        let mut output = QueryOutputAccumulator::new_borrowed_with_budget_option(
            limits,
            NonZeroUsize::new(8192).unwrap(),
            &ledger,
            "borrowed-test",
            StreamDelivery::Validated,
            &mut consumer,
        )
        .unwrap();
        let mut rows = batch.rows();
        output.emit_borrowed(rows.next().unwrap().into()).unwrap();
        assert!(matches!(
            output.emit_borrowed(rows.next().unwrap().into()),
            Err(HawDBError::ReadBudgetExceeded(_))
        ));
        assert_eq!(calls.get(), 0);
        drop(output);
        assert_eq!(ledger.snapshot().used_bytes, 0);
    }
}

#[test]
fn borrowed_delivery_cancellation_error_and_unwind_retire_retained_values() {
    for failure in 0..3 {
        let ledger = QueryMemoryLedger::new(NonZeroUsize::new(8192).unwrap());
        let context = RuntimeTaskContext::default();
        let calls = Cell::new(0);
        let mut consumer = |row: RowRef<'_>| {
            assert_eq!(row.get("value"), Some(ValueRef::String("payload")));
            assert!(ledger.snapshot().used_bytes > 0);
            calls.set(calls.get() + 1);
            match failure {
                0 => {
                    context.cancellation().cancel();
                    Ok(())
                }
                1 => Err(HawDBError::Execution("consumer refused".into())),
                _ => panic!("consumer unwound"),
            }
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut output = QueryOutputAccumulator::new_borrowed_with_budget_option(
                OutputLimits {
                    max_rows: Some(2),
                    max_payload_bytes: None,
                },
                NonZeroUsize::new(8192).unwrap(),
                &ledger,
                "borrowed-test",
                StreamDelivery::Validated,
                &mut consumer,
            )
            .unwrap();
            let batch = text_batch(&["value"], &["payload", "payload"]);
            for row in batch.rows() {
                output.emit_borrowed(row.into()).unwrap();
            }
            output.finish_delivery(Some(&context))
        }));
        match result {
            Ok(result) => assert!(result.is_err()),
            Err(_) => assert_eq!(failure, 2),
        }
        assert_eq!(calls.get(), 1);
        assert_eq!(ledger.snapshot().used_bytes, 0);
    }
}

#[test]
fn compact_borrowed_collection_rejects_duplicate_and_changed_columns() {
    let memory = ExecutionMemoryConfig::default();
    let ledger = QueryMemoryLedger::new(memory.query_memory_bytes);
    let mut output = QueryOutputAccumulator::new_owned(
        OutputLimits::default(),
        memory.query_memory_bytes,
        &ledger,
    )
    .unwrap();
    let duplicated = text_batch(&["value", "value"], &["one"]);
    assert!(output
        .emit_borrowed(duplicated.rows().next().unwrap().into())
        .is_err());
    assert_eq!(output.metrics().rows, 0);
    let first = text_batch(&["value"], &["one"]);
    output
        .emit_borrowed(first.rows().next().unwrap().into())
        .unwrap();
    let changed = text_batch(&["other"], &["two"]);
    assert!(output
        .emit_borrowed(changed.rows().next().unwrap().into())
        .is_err());
    let rows = output.take_owned_rows().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows.get(0, "value"), Some(ValueRef::String("one")));
    drop(rows);
    drop(output);
    assert_eq!(ledger.snapshot().used_bytes, 0);
}

#[test]
fn borrowed_clones_admit_nested_payloads_without_source_spare_capacity() {
    let mut text = String::with_capacity(1024 * 1024);
    text.push_str("payload");
    let source = Row::from([(
        "value".into(),
        Value::List(vec![Value::Map(Row::from([(
            "nested".into(),
            Value::String(text),
        )]))]),
    )]);
    for budget in [512, 4096] {
        let ledger = QueryMemoryLedger::new(NonZeroUsize::new(8192).unwrap());
        let mut output = QueryOutputAccumulator::new_owned(
            OutputLimits::default(),
            NonZeroUsize::new(budget).unwrap(),
            &ledger,
        )
        .unwrap();
        let result = output.emit_borrowed((&source).into());
        if budget == 512 {
            assert!(matches!(result, Err(HawDBError::ResourceBudgetExceeded(_))));
            assert_eq!(output.metrics().rows, 0);
        } else {
            result.unwrap();
            let rows = output.take_owned_rows().unwrap();
            assert_eq!(rows.first().unwrap().to_owned_row(), source);
            assert!(ledger.snapshot().peak_bytes < 4096);
            drop(rows);
        }
        drop(output);
        assert_eq!(ledger.snapshot().used_bytes, 0);
    }
}

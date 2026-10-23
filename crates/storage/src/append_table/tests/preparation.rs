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

fn generated_write(table: &str, partitions: &[&str]) -> AppendWrite {
    AppendWrite::AppendGenerated {
        table: table.into(),
        rows: partitions
            .iter()
            .map(|partition| generated_row(partition, 7))
            .collect(),
    }
}
fn key(value: i64) -> RelationalKey {
    RelationalKey(vec![RelationalValue::BigInt(value)])
}
fn partition(value: &str) -> RelationalKey {
    RelationalKey(vec![RelationalValue::Text(value.into())])
}

#[test]
fn mixed_schema_and_append_batch_preserves_all_sequence_overrides() {
    let state = create_state();
    let mut first = generated_schema();
    first.name = "first".into();
    let mut second = generated_schema();
    second.name = "second".into();
    let transaction = AppendTransaction {
        writes: vec![
            AppendWrite::CreateTable { schema: first },
            AppendWrite::CreateTable { schema: second },
            generated_write("first", &["a", "b"]),
            AppendWrite::Append {
                table: "events".into(),
                rows: vec![row("a", 10), row("b", 5)],
            },
            generated_write("first", &["a"]),
            generated_write("second", &["c"]),
        ],
    };
    let prepared = state
        .prepare_transaction(&transaction, AppendMutationLimits::default())
        .unwrap();
    assert_eq!(state.schemas().len(), 1);
    assert_eq!(state.live_rows(), 0);
    assert_eq!(prepared.state.schemas().len(), 3);
    assert_eq!(prepared.state.live_rows(), 6);
    let outcomes: Vec<_> = prepared
        .mutation_outcomes
        .iter()
        .map(|outcome| (outcome.table.as_str(), outcome.generated_order_keys.clone()))
        .collect();
    assert_eq!(
        outcomes,
        vec![
            ("first", vec![key(1), key(2)]),
            ("events", vec![]),
            ("first", vec![key(3)]),
            ("second", vec![key(1)]),
        ]
    );
    let replayed = state
        .apply_recovered_transaction(
            &prepared.materialized_transaction,
            AppendMutationLimits::default(),
        )
        .unwrap();
    assert_eq!(replayed, prepared.state);
    assert_eq!(replayed.generated_order_watermark("first"), Some(3));
    assert_eq!(replayed.generated_order_watermark("second"), Some(1));
}

#[test]
fn generated_global_sequence_protects_old_and_new_partitions_after_checkpoint() {
    let mut state = create_generated_state();
    for _ in 0..256 {
        state = state
            .prepare_transaction(
                &AppendTransaction {
                    writes: vec![generated_write("events", &["old"])],
                },
                AppendMutationLimits::default(),
            )
            .unwrap()
            .state;
    }
    let old = state.clone();
    let prepared = state
        .prepare_transaction(
            &AppendTransaction {
                writes: vec![generated_write("events", &["fresh", "fresh", "old"])],
            },
            AppendMutationLimits::default(),
        )
        .unwrap();
    assert_eq!(
        prepared.mutation_outcomes[0].generated_order_keys,
        vec![key(257), key(258), key(259)]
    );
    assert_eq!(old.watermark("events", &partition("old")), Some(&key(256)));
    assert_eq!(old.watermark("events", &partition("fresh")), None);
    assert_eq!(
        prepared.state.watermark("events", &partition("fresh")),
        Some(&key(258))
    );
    assert_eq!(
        prepared.state.watermark("events", &partition("old")),
        Some(&key(259))
    );
    let gap = AppendTransaction {
        writes: vec![AppendWrite::Append {
            table: "events".into(),
            rows: vec![row("fresh", 257), row("other", 259)],
        }],
    };
    assert!(matches!(
        state.apply_recovered_transaction(&gap, AppendMutationLimits::default()),
        Err(AppendTableError::Corruption(_))
    ));
    assert_eq!(state.generated_order_watermark("events"), Some(256));
    let checkpoint = AppendState::from_checkpoint_with_generated_order_watermarks(
        prepared.state.schemas().clone(),
        BTreeMap::from([(
            "events".into(),
            BTreeMap::from([(partition("old"), key(259)), (partition("fresh"), key(258))]),
        )]),
        prepared.state.generated_order_watermarks().clone(),
    )
    .unwrap();
    let next = checkpoint
        .prepare_transaction(
            &AppendTransaction {
                writes: vec![generated_write("events", &["old", "new"])],
            },
            AppendMutationLimits::default(),
        )
        .unwrap();
    assert_eq!(
        next.mutation_outcomes[0].generated_order_keys,
        vec![key(260), key(261)]
    );
    assert_eq!(checkpoint.generated_order_watermark("events"), Some(259));
}

#[test]
fn caller_keys_still_reject_late_partition_regression_atomically() {
    let mut state = create_state();
    for sequence in 1..=128 {
        state = state
            .prepare_transaction(
                &AppendTransaction {
                    writes: vec![AppendWrite::Append {
                        table: "events".into(),
                        rows: vec![row("old", sequence)],
                    }],
                },
                AppendMutationLimits::default(),
            )
            .unwrap()
            .state;
    }
    let invalid = AppendTransaction {
        writes: vec![AppendWrite::Append {
            table: "events".into(),
            rows: vec![row("fresh", 1), row("old", 128)],
        }],
    };
    assert!(matches!(
        state.prepare_transaction(&invalid, AppendMutationLimits::default()),
        Err(AppendTableError::Constraint(_))
    ));
    assert_eq!(state.live_rows(), 128);
    assert_eq!(state.watermark("events", &partition("fresh")), None);
    let valid = AppendTransaction {
        writes: vec![AppendWrite::Append {
            table: "events".into(),
            rows: vec![row("fresh", 1), row("old", 129)],
        }],
    };
    let prepared = state
        .prepare_transaction(&valid, AppendMutationLimits::default())
        .unwrap();
    assert_eq!(
        prepared.state.watermark("events", &partition("fresh")),
        Some(&key(1))
    );
    assert_eq!(
        prepared.state.watermark("events", &partition("old")),
        Some(&key(129))
    );
}

#[test]
fn late_failure_does_not_publish_new_schema_or_reserve_generated_values() {
    let state = create_state();
    let mut created = generated_schema();
    created.name = "generated".into();
    let mut transaction = AppendTransaction {
        writes: vec![
            AppendWrite::CreateTable { schema: created },
            generated_write("generated", &["a", "b"]),
            AppendWrite::Append {
                table: "events".into(),
                rows: vec![RelationalRow::new(vec![
                    RelationalValue::Null,
                    RelationalValue::BigInt(1),
                    RelationalValue::Bytea(vec![7]),
                ])],
            },
        ],
    };
    assert!(matches!(
        state.prepare_transaction(&transaction, AppendMutationLimits::default()),
        Err(AppendTableError::Constraint(_))
    ));
    assert!(state.schema("generated").is_none());
    assert_eq!(state.generated_order_watermark("generated"), None);
    transaction.writes.pop();
    let prepared = state
        .prepare_transaction(&transaction, AppendMutationLimits::default())
        .unwrap();
    assert_eq!(
        prepared.mutation_outcomes[0].generated_order_keys,
        vec![key(1), key(2)]
    );
    assert_eq!(state.live_rows(), 0);
}

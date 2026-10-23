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

use hawdb_storage::append_table::{
    AppendGeneratedRow, AppendMutationLimits, AppendOrderMode, AppendState, AppendTableSchema,
    AppendTransaction, AppendWrite,
};
use hawdb_storage::relational::{
    RelationalColumnSchema, RelationalKey, RelationalRow, RelationalScalarType, RelationalValue,
};
use serde_json::json;
use std::collections::BTreeMap;
use std::hint::black_box;
use std::time::Instant;

fn schema(name: String, mode: AppendOrderMode) -> AppendTableSchema {
    AppendTableSchema {
        name,
        columns: vec![
            RelationalColumnSchema {
                name: "stream".into(),
                scalar_type: RelationalScalarType::Text,
                nullable: false,
                default: None,
            },
            RelationalColumnSchema {
                name: "sequence".into(),
                scalar_type: RelationalScalarType::BigInt,
                nullable: false,
                default: None,
            },
            RelationalColumnSchema {
                name: "payload".into(),
                scalar_type: RelationalScalarType::Bytea,
                nullable: false,
                default: None,
            },
        ],
        partition_key: vec!["stream".into()],
        order_key: vec!["sequence".into()],
        order_mode: mode,
    }
}
fn transaction(
    mode: AppendOrderMode,
    partition: &str,
    count: usize,
    start: usize,
) -> AppendTransaction {
    let write = match mode {
        AppendOrderMode::CommitSequence => AppendWrite::AppendGenerated {
            table: "events".into(),
            rows: (0..count)
                .map(|_| {
                    AppendGeneratedRow::new(vec![
                        RelationalValue::Text(partition.into()),
                        RelationalValue::Bytea(vec![7; 128]),
                    ])
                })
                .collect(),
        },
        AppendOrderMode::CallerProvided => AppendWrite::Append {
            table: "events".into(),
            rows: (0..count)
                .map(|i| {
                    RelationalRow::new(vec![
                        RelationalValue::Text(partition.into()),
                        RelationalValue::BigInt((start + i + 1) as i64),
                        RelationalValue::Bytea(vec![7; 128]),
                    ])
                })
                .collect(),
        },
    };
    AppendTransaction {
        writes: vec![write],
    }
}
fn measure(
    mode: AppendOrderMode,
    tables: usize,
    history: usize,
    batch: usize,
) -> serde_json::Value {
    let mut state = AppendState::from_checkpoint(
        BTreeMap::from([("events".into(), schema("events".into(), mode))]),
        BTreeMap::new(),
    )
    .unwrap();
    for i in 0..history {
        state = state
            .prepare_transaction(
                &transaction(mode, "seed", 1, i),
                AppendMutationLimits::default(),
            )
            .unwrap()
            .state;
    }
    let definitions = AppendTransaction {
        writes: (0..tables)
            .map(|i| AppendWrite::CreateTable {
                schema: schema(format!("unrelated-{i}"), mode),
            })
            .collect(),
    };
    state = state
        .stage_transaction(&definitions, AppendMutationLimits::default())
        .unwrap();
    let input = transaction(mode, "fresh", batch, history);
    let samples = if cfg!(debug_assertions) { 17 } else { 257 };
    let mut times = Vec::with_capacity(samples);
    for i in 0..samples + 9 {
        let start = Instant::now();
        let prepared = state
            .prepare_transaction(black_box(&input), AppendMutationLimits::default())
            .unwrap();
        let elapsed = start.elapsed().as_nanos() as u64;
        black_box(&prepared);
        if i >= 9 {
            times.push(elapsed);
        }
        assert_eq!(prepared.state.live_rows(), history + batch);
        let mut observed = Vec::new();
        prepared
            .state
            .visit_live_rows(
                "events",
                &RelationalKey(vec![RelationalValue::Text("fresh".into())]),
                None,
                batch + 1,
                |row| {
                    observed.push(row.order_key.clone());
                    Ok(())
                },
            )
            .unwrap();
        let expected: Vec<_> = (1..=batch)
            .map(|i| RelationalKey(vec![RelationalValue::BigInt((history + i) as i64)]))
            .collect();
        assert_eq!(observed, expected);
        assert_eq!(state.live_rows(), history);
        assert_eq!(prepared.mutation_outcomes[0].appended_rows, batch);
    }
    times.sort_unstable();
    let p = |percent: usize| times[(times.len() * percent).div_ceil(100) - 1];
    json!({"mode": format!("{mode:?}"), "unrelated_tables":tables, "history_batches":history, "batch_rows":batch,
        "samples":samples, "p50_ns":p(50), "p95_ns":p(95), "p99_ns":p(99)})
}
fn main() {
    let mut results = Vec::new();
    for mode in [
        AppendOrderMode::CallerProvided,
        AppendOrderMode::CommitSequence,
    ] {
        for tables in [0, 1024] {
            for history in [0, 1024] {
                for batch in [1, 64] {
                    results.push(measure(mode, tables, history, batch));
                }
            }
        }
    }
    println!(
        "{}",
        json!({"scope":"AppendState preparation only; no WAL, filesystem synchronization or end-to-end commit timing", "results":results})
    );
}

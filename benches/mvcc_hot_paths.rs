// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

//! Manual fixed-work MVCC qualification. Run the identical source on both
//! revisions, in release mode; setup and warmup are outside measured intervals.

use hawdb::schema::Catalog;
use hawdb::store::GraphStore;
use hawdb::{
    ConcurrentDatabase, ConcurrentTransactionOptions, Database, Value, WalGroupCommitConfig,
};
use hawdb_storage::cow::CowSegmentedMap;
use hawdb_storage::mutation::{PropertyFilter, RelationshipPropertyUpdate};
use serde_json::json;
use std::collections::BTreeMap;
use std::hint::black_box;
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::Barrier;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn main() {
    let mut cases = Vec::new();
    for repetition in 0..3 {
        for rows in [64, 4096] {
            for pinned in [false, true] {
                let mut map = CowSegmentedMap::from(
                    (0..rows)
                        .map(|id| (id, Value::Int(0)))
                        .collect::<BTreeMap<_, _>>(),
                );
                let iterations = 10_000;
                let started = Instant::now();
                for iteration in 0..iterations {
                    let snapshot = pinned.then(|| map.clone());
                    map.insert(iteration % rows, Value::Int(iteration as i64));
                    black_box(snapshot);
                }
                cases.push(json!({"kind": "cow_replace", "repetition": repetition,
                    "rows": rows, "pinned_each_write": pinned, "operations": iterations,
                    "elapsed_ns": started.elapsed().as_nanos()}));
                assert_eq!(map.len(), rows as usize);
            }
        }
        for readers in [1, 4, 8] {
            let mut database = Database::new();
            database.query("CREATE (:Item {id: 1, value: 7})").unwrap();
            let database = ConcurrentDatabase::new(database);
            let query = "MATCH (n:Item {id: $id}) RETURN n.value AS value LIMIT 1";
            let parameters = BTreeMap::from([("id".into(), Value::Int(1))]);
            database.query_with_params(query, &parameters).unwrap();
            let iterations = 2000;
            let barrier = Barrier::new(readers);
            let started = Instant::now();
            std::thread::scope(|scope| {
                for _ in 0..readers {
                    let database = &database;
                    let parameters = &parameters;
                    let barrier = &barrier;
                    scope.spawn(move || {
                        barrier.wait();
                        for _ in 0..iterations {
                            let output = database.query_with_params(query, parameters).unwrap();
                            assert_eq!(output.rows[0]["value"], Value::Int(7));
                        }
                    });
                }
            });
            cases.push(json!({"kind": "cypher_point", "repetition": repetition,
                "readers": readers, "operations": readers * iterations,
                "elapsed_ns": started.elapsed().as_nanos()}));
        }
        for body_bytes in [0, 8192] {
            for pinned in [false, true] {
                cases.push(measure_relationships(repetition, body_bytes, pinned));
            }
        }
        for writers in [1, 4, 8] {
            cases.push(measure_commits(repetition, writers));
        }
    }
    println!(
        "{}",
        json!({"schema": "mvcc-hot-paths-v4", "profile": "release", "cases": cases})
    );
}

fn measure_relationships(repetition: usize, body_bytes: usize, pinned: bool) -> serde_json::Value {
    let mut store = GraphStore::in_memory();
    let mut catalog = Catalog::default();
    let target = store
        .create_node(&mut catalog, "Target", BTreeMap::new())
        .unwrap();
    for id in 0..128 {
        let source = store
            .create_node(
                &mut catalog,
                "Source",
                BTreeMap::from([("id".into(), Value::Int(id))]),
            )
            .unwrap();
        store
            .create_relationship(
                &mut catalog,
                source,
                target,
                "LINK",
                BTreeMap::from([("body".into(), Value::String("x".repeat(body_bytes)))]),
            )
            .unwrap();
    }
    let started = Instant::now();
    for iteration in 0..512 {
        let reader = pinned.then(|| store.snapshot());
        let changed = store
            .set_relationship_property(
                &mut catalog,
                RelationshipPropertyUpdate {
                    source_label: "Source".into(),
                    filter: Some(PropertyFilter::Eq {
                        property: "id".into(),
                        value: Value::Int(iteration % 128),
                    }),
                    rel_type: "LINK".into(),
                    target_label: "Target".into(),
                    target_filter: None,
                    rel_filter: None,
                    property: "value".into(),
                    value: Value::Int(iteration),
                },
            )
            .unwrap();
        assert_eq!(changed.len(), 1);
        black_box(reader);
    }
    json!({"kind": "relationship_payload", "storage": "in_memory", "repetition": repetition,
        "rows": 128, "body_bytes": body_bytes, "pinned_each_write": pinned,
        "operations": 512, "elapsed_ns": started.elapsed().as_nanos()})
}

fn measure_commits(repetition: usize, writers: usize) -> serde_json::Value {
    let path = std::env::temp_dir().join(format!(
        "hawdb-mvcc-hot-paths-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut source = Database::open(&path).unwrap();
    {
        let mut transaction = source.begin_transaction().unwrap();
        for id in 0..writers * 8 {
            transaction
                .query_with_params(
                    "CREATE (:Item {id: $id, value: 0})",
                    &BTreeMap::from([("id".into(), Value::Int(id as i64))]),
                )
                .unwrap();
        }
        transaction.commit().unwrap();
    }
    let config = WalGroupCommitConfig::benchmark_candidate(
        NonZeroUsize::new(8).unwrap(),
        NonZeroU64::new(1024 * 1024).unwrap(),
        Duration::from_micros(100),
    )
    .unwrap();
    let database = ConcurrentDatabase::new_with_wal_group_commit(source, config);
    let barrier = Barrier::new(writers);
    let body = "b".repeat(4096);
    let started = Instant::now();
    let mut latencies = std::thread::scope(|scope| {
        let mut tasks = Vec::new();
        for writer in 0..writers {
            let database = &database;
            let barrier = &barrier;
            let body = &body;
            tasks.push(scope.spawn(move || {
                let mut latencies = Vec::new();
                barrier.wait();
                for iteration in 1..=8 {
                    let request_started = Instant::now();
                    let mut transaction = database
                        .begin_transaction(ConcurrentTransactionOptions::optimistic())
                        .unwrap();
                    for row in 0..8 {
                        transaction
                            .query_with_params(
                                "MATCH (n:Item {id: $id}) SET n.value = $value, n.body = $body",
                                &BTreeMap::from([
                                    ("id".into(), Value::Int((writer * 8 + row) as i64)),
                                    ("value".into(), Value::Int(iteration)),
                                    ("body".into(), Value::String(body.clone())),
                                ]),
                            )
                            .unwrap();
                    }
                    transaction.commit().unwrap();
                    latencies.push(request_started.elapsed().as_nanos());
                }
                latencies
            }));
        }
        tasks
            .into_iter()
            .flat_map(|task| task.join().unwrap())
            .collect::<Vec<_>>()
    });
    let elapsed = started.elapsed().as_nanos();
    let groups = database.wal_group_commit_snapshot().unwrap();
    latencies.sort_unstable();
    let percentile = |percent: usize| latencies[(latencies.len() - 1) * percent / 100];
    let report = json!({"kind": "graph_commits", "storage": "persistent", "durability": "SyncOnEveryWrite",
        "group_max_entries": 8, "group_max_bytes": 1048576, "group_delay_us": 100,
        "repetition": repetition, "writers": writers, "operations": writers * 8,
        "rows_per_transaction": 8, "body_bytes": 4096, "elapsed_ns": elapsed,
        "group_count": groups.group_count, "shared_sync_count": groups.shared_sync_count,
        "grouped_wal_entries": groups.grouped_wal_entries,
        "total_fsync_micros": groups.total_fsync_micros,
        "transaction_ns_p50": percentile(50), "transaction_ns_p95": percentile(95), "transaction_ns_p99": percentile(99)});
    drop(database);
    let mut reopened = Database::open(&path).unwrap();
    let rows = reopened
        .query("MATCH (n:Item) RETURN n.value AS value")
        .unwrap()
        .rows;
    assert_eq!(rows.len(), writers * 8);
    assert!(rows.iter().all(|row| row["value"] == Value::Int(8)));
    drop(reopened);
    std::fs::remove_dir_all(path).unwrap();
    report
}

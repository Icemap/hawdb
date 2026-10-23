// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

//! Local fixed-work MVCC qualification. The serialized control holds a host
//! mutex across the same transaction lifecycle; it is not a historical binary.

use hawdb::{
    ConcurrentDatabase, ConcurrentTransactionOptions, Database, Value, WalGroupCommitConfig,
};
use serde_json::{json, Value as JsonValue};
use std::collections::BTreeMap;
use std::num::{NonZeroU64, NonZeroUsize};
use std::path::Path;
use std::sync::{Barrier, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[path = "concurrent_writers/readers.rs"]
mod readers;

const SMOKE: bool = cfg!(debug_assertions);
const COMMITS: usize = if SMOKE { 16 } else { 256 };
const ROUNDS: usize = if SMOKE { 1 } else { 5 };
const UPDATE: &str = "MATCH (n:MvccCounter) WHERE n.id = $id SET n.value = 1";
const READ: &str = "MATCH (n:MvccCounter) RETURN n.id AS id, n.value AS value ORDER BY id";

// The original gate baseline returned a bare epoch from Database. Keep one
// identical harness across that API change; this oracle is outside timing.
trait ReopenedEpoch {
    fn checked(self) -> u64;
}
impl ReopenedEpoch for u64 {
    fn checked(self) -> u64 {
        self
    }
}
impl ReopenedEpoch for hawdb::Result<u64> {
    fn checked(self) -> u64 {
        self.unwrap()
    }
}

fn nanos(start: Instant) -> u64 {
    start.elapsed().as_nanos().try_into().unwrap()
}

fn latency(samples: &[u64]) -> JsonValue {
    assert!(!samples.is_empty());
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    json!({
        "count": sorted.len(),
        "p50_ns": sorted[(sorted.len() * 50).div_ceil(100) - 1],
        "p95_ns": sorted[(sorted.len() * 95).div_ceil(100) - 1],
        "p99_ns": sorted[(sorted.len() * 99).div_ceil(100) - 1],
        "max_ns": sorted.last().unwrap(),
        "samples_ns": samples,
    })
}

fn expected(value: i64) -> Vec<BTreeMap<String, Value>> {
    (0..COMMITS)
        .map(|id| {
            BTreeMap::from([
                ("id".into(), Value::Int(id.try_into().unwrap())),
                ("value".into(), Value::Int(value)),
            ])
        })
        .collect()
}

fn measure(
    path: &Path,
    durable: bool,
    grouped: bool,
    writers: usize,
    serial: bool,
    reopen_before_measure: bool,
    readers: usize,
) -> JsonValue {
    let database = if durable {
        Database::open(path).unwrap()
    } else {
        Database::new()
    };
    let config = if grouped {
        WalGroupCommitConfig::benchmark_candidate(
            NonZeroUsize::new(8).unwrap(),
            NonZeroU64::new(1024 * 1024).unwrap(),
            Duration::from_micros(100),
        )
        .unwrap()
    } else {
        WalGroupCommitConfig::disabled()
    };
    let database = ConcurrentDatabase::new_with_wal_group_commit(database, config);
    let mut seed = database
        .begin_transaction(ConcurrentTransactionOptions::optimistic())
        .unwrap();
    for id in 0..COMMITS {
        seed.query_with_params(
            if readers == 0 {
                "CREATE (:MvccCounter {id: $id, value: 0})"
            } else {
                "CREATE (:MvccCounter {id: $id, value: 0, mirror: 0})"
            },
            &BTreeMap::from([("id".into(), Value::Int(id.try_into().unwrap()))]),
        )
        .unwrap();
    }
    seed.commit().unwrap();
    database.checkpoint().unwrap();
    let reopened_before_measure = durable && reopen_before_measure;
    let database = if reopened_before_measure {
        // Checkpoint rotation clears the captured WAL owner. Reopen before
        // timing to exercise captured append fences on every measured commit.
        drop(database);
        ConcurrentDatabase::new_with_wal_group_commit(Database::open(path).unwrap(), config)
    } else {
        database
    };
    assert_eq!(database.query(READ).unwrap().rows, expected(0));
    let initial_epoch = database.commit_epoch().unwrap();
    let mut pinned = (readers > 0).then(|| database.begin_read_transaction().unwrap());
    if let Some(snapshot) = &mut pinned {
        assert_eq!(snapshot.commit_epoch(), initial_epoch);
        readers::verify(&snapshot.query(readers::READ).unwrap(), 0);
    }
    let before = database.wal_group_commit_snapshot().unwrap();
    let serial_gate = Mutex::new(());
    // Threads and per-request parameters are prepared before the timed start.
    let ready = Barrier::new(writers + readers + 1);
    let go = Barrier::new(writers + readers + 1);
    let origin = Instant::now();
    let (elapsed, workers, reader_measurements) = std::thread::scope(|scope| {
        let handles = (0..writers)
            .map(|worker| {
                let database = &database;
                let serial_gate = &serial_gate;
                let ready = &ready;
                let go = &go;
                scope.spawn(move || {
                    let requests = (worker..COMMITS)
                        .step_by(writers)
                        .map(|id| {
                            BTreeMap::from([("id".into(), Value::Int(id.try_into().unwrap()))])
                        })
                        .collect::<Vec<_>>();
                    let mut transaction_latencies = Vec::with_capacity(requests.len());
                    let mut commit_latencies = Vec::with_capacity(requests.len());
                    let mut intervals = Vec::with_capacity(requests.len());
                    ready.wait();
                    go.wait();
                    let worker_started = Instant::now();
                    for params in requests {
                        let started = Instant::now();
                        let _guard = serial.then(|| serial_gate.lock().unwrap());
                        let mut tx = database
                            .begin_transaction(ConcurrentTransactionOptions::optimistic())
                            .unwrap();
                        tx.query_with_params(
                            if readers == 0 {
                                UPDATE
                            } else {
                                readers::UPDATE
                            },
                            &params,
                        )
                        .unwrap();
                        let commit_started = Instant::now();
                        // Disjoint keys must commit once, without hidden retries.
                        tx.commit().unwrap();
                        commit_latencies.push(nanos(commit_started));
                        transaction_latencies.push(nanos(started));
                        if readers > 0 {
                            intervals.push((
                                started
                                    .duration_since(origin)
                                    .as_nanos()
                                    .try_into()
                                    .unwrap(),
                                nanos(origin),
                            ));
                        }
                    }
                    (
                        worker,
                        nanos(worker_started),
                        transaction_latencies,
                        commit_latencies,
                        intervals,
                    )
                })
            })
            .collect::<Vec<_>>();
        let reader_handles = (0..readers)
            .map(|worker| {
                let database = &database;
                let ready = &ready;
                let go = &go;
                scope.spawn(move || {
                    readers::measure(database, worker, ready, go, origin, initial_epoch)
                })
            })
            .collect::<Vec<_>>();
        ready.wait();
        let started = Instant::now();
        go.wait();
        let workers = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        let reader_measurements = reader_handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        (nanos(started), workers, reader_measurements)
    });
    let after = database.wal_group_commit_snapshot().unwrap();
    if grouped {
        assert_eq!(
            after.submitted_commits - before.submitted_commits,
            COMMITS as u64
        );
        assert_eq!(
            after.completed_commits - before.completed_commits,
            COMMITS as u64
        );
        assert_eq!(
            after.grouped_wal_entries - before.grouped_wal_entries,
            COMMITS as u64
        );
        assert!(
            (1..=COMMITS as u64).contains(&(after.shared_sync_count - before.shared_sync_count))
        );
    }
    assert_eq!(
        database.commit_epoch().unwrap() - initial_epoch,
        COMMITS as u64
    );
    assert_eq!(database.query(READ).unwrap().rows, expected(1));
    if let Some(snapshot) = &mut pinned {
        assert_eq!(snapshot.commit_epoch(), initial_epoch);
        readers::verify(&snapshot.query(readers::READ).unwrap(), 0);
        readers::verify(&database.query(readers::READ).unwrap(), COMMITS as u64);
    }
    drop(pinned);
    let writes = workers
        .iter()
        .flat_map(|w| w.4.iter().copied())
        .collect::<Vec<_>>();
    let read_report = readers::report(&reader_measurements, &writes, elapsed, initial_epoch);
    let transaction_samples = workers
        .iter()
        .flat_map(|w| w.2.iter().copied())
        .collect::<Vec<_>>();
    let commit_samples = workers
        .iter()
        .flat_map(|w| w.3.iter().copied())
        .collect::<Vec<_>>();
    assert_eq!(transaction_samples.len(), COMMITS);
    drop(database);
    if durable {
        let mut reopened = Database::open(path).unwrap();
        assert_eq!(
            reopened.commit_epoch().checked(),
            initial_epoch + COMMITS as u64
        );
        assert_eq!(reopened.query(READ).unwrap().rows, expected(1));
        if readers > 0 {
            readers::verify(&reopened.query(readers::READ).unwrap(), COMMITS as u64);
        }
        drop(reopened);
        std::fs::remove_dir_all(path).unwrap();
    }
    json!({
        "durable": durable, "grouped": grouped, "writers": writers, "serialized_control": serial,
        "readers": readers, "read_workload": read_report,
        "reopened_before_measure": reopened_before_measure,
        "commits": COMMITS, "elapsed_ns": elapsed,
        "commits_per_second": COMMITS as f64 * 1e9 / elapsed as f64,
        "transaction_latency": latency(&transaction_samples),
        "commit_latency": latency(&commit_samples),
        "workers": workers.iter().map(|w| json!({
            "worker": w.0, "elapsed_ns": w.1, "commits": w.2.len(),
            "transaction_intervals_ns": w.4,
        })).collect::<Vec<_>>(),
        "group_commit": {
            "submitted": after.submitted_commits - before.submitted_commits,
            "completed": after.completed_commits - before.completed_commits,
            "shared_syncs": after.shared_sync_count - before.shared_sync_count,
            "grouped_entries": after.grouped_wal_entries - before.grouped_wal_entries,
            "fsync_micros": after.total_fsync_micros - before.total_fsync_micros,
        },
        "verified_rows": COMMITS, "reopen_verified": durable,
    })
}

fn main() {
    let mut selected_round = None;
    let mut selected_writers = None;
    let mut selected_readers = None;
    let mut reopen_before_measure = false;
    let mut fixture_parent = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bench" => {} // Cargo supplies this for harness-free benchmarks.
            "--reopen-before-measure" => {
                assert!(!reopen_before_measure, "reopen option may appear only once");
                reopen_before_measure = true;
            }
            "--fixture-parent" => {
                assert!(
                    fixture_parent.is_none(),
                    "--fixture-parent may appear only once"
                );
                let path = std::path::PathBuf::from(
                    args.next().expect("--fixture-parent requires a directory"),
                );
                assert!(
                    path.is_dir(),
                    "fixture parent must be an existing directory"
                );
                fixture_parent = Some(path);
            }
            "--writers" => {
                assert!(selected_writers.is_none(), "--writers may appear only once");
                let writers = args
                    .next()
                    .expect("--writers requires a count")
                    .parse::<usize>()
                    .expect("writers must be an unsigned integer");
                assert!(
                    (1..=COMMITS).contains(&writers),
                    "each writer requires at least one transaction"
                );
                selected_writers = Some(writers);
            }
            "--readers" => {
                assert!(selected_readers.is_none(), "--readers may appear only once");
                let readers = args
                    .next()
                    .expect("--readers requires a count")
                    .parse::<usize>()
                    .expect("readers must be an unsigned integer");
                selected_readers = Some(readers);
            }
            "--round" => {
                assert!(selected_round.is_none(), "--round may appear only once");
                let round = args
                    .next()
                    .expect("--round requires an index")
                    .parse::<usize>()
                    .expect("round must be an unsigned integer");
                assert!(round < ROUNDS, "round must be below {ROUNDS}");
                selected_round = Some(round);
            }
            _ => panic!("unknown benchmark argument: {arg}"),
        }
    }
    let root = fixture_parent
        .unwrap_or_else(std::env::temp_dir)
        .join(format!(
            "hawdb-concurrent-writers-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir(&root).unwrap();
    let writer_counts = selected_writers.map_or_else(|| vec![1, 4, 8], |count| vec![count]);
    for round in 0..ROUNDS {
        if selected_round.is_some_and(|selected| selected != round) {
            continue;
        }
        // Reverse case order every round to reduce consistent order bias.
        let mut cases = Vec::new();
        for (durable, grouped) in [(false, false), (true, false), (true, true)] {
            for &writers in &writer_counts {
                for serial in [true, false] {
                    cases.push((durable, grouped, writers, serial));
                }
            }
        }
        if round % 2 == 1 {
            cases.reverse();
        }
        for (case, (durable, grouped, writers, serial)) in cases.into_iter().enumerate() {
            let result = measure(
                &root.join(format!("{round}-{case}")),
                durable,
                grouped,
                writers,
                serial,
                reopen_before_measure,
                selected_readers.unwrap_or(0),
            );
            println!(
                "{}",
                json!({"schema": "concurrent-writers-v2", "smoke": SMOKE, "round": round, "result": result})
            );
        }
    }
    std::fs::remove_dir(root).unwrap();
}

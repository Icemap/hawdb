// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::QueryStreamOptions;
use std::sync::mpsc;
use std::thread;

const UPDATE: &str =
    "MATCH (n:Counter) WHERE n.id = $id SET n.score = $score RETURN count(n) AS total";

fn fixture() -> ConcurrentDatabase {
    let mut database = Database::new();
    database
        .query("CREATE (:Counter {id: 1, score: 0})")
        .unwrap();
    database
        .query("CREATE (:Counter {id: 2, score: 0})")
        .unwrap();
    database
        .query("CREATE CONSTRAINT ON :Counter(id) ASSERT UNIQUE")
        .unwrap();
    database.query("CREATE INDEX ON :Counter(id)").unwrap();
    database.into_concurrent()
}

fn params(id: i64, score: i64) -> BTreeMap<String, Value> {
    BTreeMap::from([
        ("id".into(), Value::Int(id)),
        ("score".into(), Value::Int(score)),
    ])
}

fn score(database: &ConcurrentDatabase, id: i64) -> Value {
    database
        .query_with_params(
            "MATCH (n:Counter) WHERE n.id = $id RETURN n.score AS score",
            &params(id, 0),
        )
        .unwrap()
        .rows[0]["score"]
        .clone()
}

#[test]
fn same_label_unique_writers_and_readers_advance_during_private_execution() {
    let database = fixture();
    database.enable_concurrency_metrics();
    let (ready, resume) =
        database.pause_next_autocommit_mutation(AutocommitMutationPhase::BeforeExecution);
    let first_database = database.clone();
    let first = thread::spawn(move || first_database.query_with_params(UPDATE, &params(1, 11)));
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    let second_database = database.clone();
    let (done, completed) = mpsc::channel();
    let second = thread::spawn(move || {
        let result = (|| {
            for value in 1..=8 {
                second_database.query_with_params(UPDATE, &params(2, value))?;
            }
            Ok::<_, HawDBError>((score(&second_database, 1), score(&second_database, 2)))
        })();
        done.send(result).unwrap();
    });
    let progress = completed.recv_timeout(Duration::from_secs(5));
    resume.send(()).unwrap();
    let first = first.join().unwrap();
    second.join().unwrap();
    assert_eq!(progress.unwrap().unwrap(), (Value::Int(0), Value::Int(8)));
    assert_eq!(first.unwrap().rows[0]["total"], Value::Int(1));
    assert_eq!(score(&database, 1), Value::Int(11));
    assert_eq!(database.concurrency_metrics().graph_statement_replays, 0);
}

#[test]
fn commits_during_prepared_lock_wait_use_a_fresh_snapshot_without_replanning() {
    let database = fixture();
    let (ready, resume) =
        database.pause_next_autocommit_mutation(AutocommitMutationPhase::BeforeLocks);
    let first_database = database.clone();
    let first = thread::spawn(move || {
        first_database.query(
            "MATCH (n:Counter) WHERE n.id = 1 SET n.score = n.score + 1 RETURN n.score AS score",
        )
    });
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    for value in 1..=8 {
        database
            .query_with_params(UPDATE, &params(2, value))
            .unwrap();
    }
    database.query_with_params(UPDATE, &params(1, 40)).unwrap();
    let preparations = database
        .inner
        .commits
        .lock()
        .unwrap()
        .plan_cache_stats()
        .unwrap();
    resume.send(()).unwrap();
    assert_eq!(
        first.join().unwrap().unwrap().rows[0]["score"],
        Value::Int(41)
    );
    assert_eq!(
        database
            .inner
            .commits
            .lock()
            .unwrap()
            .plan_cache_stats()
            .unwrap(),
        preparations
    );
}

#[test]
fn refused_output_and_cancelled_staging_leave_no_commit_and_release_locks() {
    let database = fixture();
    let epoch = database.commit_epoch().unwrap();
    assert!(matches!(
        database.query_request(
            QueryRequest::new(UPDATE)
                .with_params(&params(1, 99))
                .with_output_limits(QueryStreamOptions {
                    max_rows: Some(1),
                    max_payload_bytes: Some(0)
                })
        ),
        Err(HawDBError::ReadBudgetExceeded(_))
    ));
    assert_eq!(database.commit_epoch().unwrap(), epoch);
    assert_eq!(score(&database, 1), Value::Int(0));
    let (ready, resume) =
        database.pause_next_autocommit_mutation(AutocommitMutationPhase::BeforeExecution);
    let task = hawdb_core::RuntimeTaskContext::default();
    let other_task = task.clone();
    let other_database = database.clone();
    let writer = thread::spawn(move || {
        other_database.query_request(
            QueryRequest::new(UPDATE)
                .with_params(&params(1, 99))
                .with_task_context(&other_task),
        )
    });
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    task.cancellation().cancel();
    resume.send(()).unwrap();
    assert!(writer.join().unwrap().is_err());
    assert_eq!(database.commit_epoch().unwrap(), epoch);
    database.query_with_params(UPDATE, &params(1, 3)).unwrap();
    assert_eq!(score(&database, 1), Value::Int(3));
}

#[test]
fn schema_change_during_wait_rejects_old_preparation_before_mutation() {
    let database = fixture();
    let (ready, resume) =
        database.pause_next_autocommit_mutation(AutocommitMutationPhase::BeforeLocks);
    let writer_database = database.clone();
    let writer = thread::spawn(move || writer_database.query_with_params(UPDATE, &params(1, 99)));
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    database.query("CREATE NODE TABLE Changed").unwrap();
    let epoch = database.commit_epoch().unwrap();
    resume.send(()).unwrap();
    assert!(writer
        .join()
        .unwrap()
        .unwrap_err()
        .to_string()
        .contains("prepare and admit again"));
    assert_eq!(database.commit_epoch().unwrap(), epoch);
    assert_eq!(score(&database, 1), Value::Int(0));
}

#[test]
fn concurrent_creates_return_committed_ids_and_reopen_complete_transactions() {
    let path = std::env::temp_dir().join(format!(
        "hawdb-autocommit-{}",
        hawdb_core::generate_uuidv7().unwrap()
    ));
    let database = ConcurrentDatabase::open(&path).unwrap();
    database.query("CREATE NODE TABLE Item").unwrap();
    database
        .query("CREATE PROPERTY ON NODE TABLE Item(value) TYPE INT NOT NULL")
        .unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let writers: Vec<_> = (1..=2)
        .map(|value| {
            let database = database.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                database.query_with_params(
                    "CREATE (:Item {value: $value})",
                    &BTreeMap::from([("value".into(), Value::Int(value))]),
                )
            })
        })
        .collect();
    barrier.wait();
    let mut ids = BTreeSet::new();
    for writer in writers {
        ids.insert(writer.join().unwrap().unwrap().rows[0]["node_id"].clone());
    }
    assert_eq!(ids.len(), 2);
    drop(database);
    let mut reopened = Database::open(&path).unwrap();
    let rows = reopened
        .query("MATCH (n:Item) RETURN id(n) AS node_id, n.value AS value ORDER BY value")
        .unwrap();
    assert_eq!(rows.rows.len(), 2);
    assert_eq!(
        rows.rows
            .iter()
            .map(|row| row["node_id"].clone())
            .collect::<BTreeSet<_>>(),
        ids
    );
    assert_eq!(rows.rows[0]["value"], Value::Int(1));
    assert_eq!(rows.rows[1]["value"], Value::Int(2));
    drop(reopened);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn unique_candidate_remains_locked_when_an_additional_predicate_excludes_it() {
    let database = fixture();
    let (ready, resume) =
        database.pause_next_autocommit_mutation(AutocommitMutationPhase::BeforeExecution);
    let writer_database = database.clone();
    let writer = thread::spawn(move || {
        writer_database.query("MATCH (n:Counter) WHERE n.id = 1 AND n.score = 9 SET n.score = 10 RETURN count(n) AS total")
    });
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    // The actual mutation has zero matches, but its unique candidate must still
    // conflict with an explicit writer changing the predicate's other property.
    let mut competing = database
        .begin_transaction(ConcurrentTransactionOptions::pessimistic(
            Duration::from_millis(20),
        ))
        .unwrap();
    let conflict = competing.query_with_params(UPDATE, &params(1, 9));
    drop(competing);
    resume.send(()).unwrap();
    assert!(conflict.is_err());
    assert_eq!(
        writer.join().unwrap().unwrap().rows[0]["total"],
        Value::Int(0)
    );
    assert_eq!(score(&database, 1), Value::Int(0));
}

#[test]
fn diagnostics_and_mutation_summary_limits_never_publish_refused_writes() {
    let database = fixture();
    let epoch = database.commit_epoch().unwrap();
    assert!(database
        .query("EXPLAIN ANALYZE MATCH (n:Counter) SET n.score = 99")
        .is_err());
    assert_eq!(database.commit_epoch().unwrap(), epoch);
    let error = database
        .query_request(
            QueryRequest::new("MATCH (n:Counter) SET n.score = 99").with_output_limits(
                QueryStreamOptions {
                    max_rows: Some(1),
                    max_payload_bytes: None,
                },
            ),
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("max_mutation_result_rows"),
        "{error}"
    );
    assert_eq!(database.commit_epoch().unwrap(), epoch);
    assert_eq!(score(&database, 1), Value::Int(0));
    assert_eq!(score(&database, 2), Value::Int(0));
}

#[test]
fn mutation_preflight_preserves_expression_and_null_predicate_semantics() {
    let database = fixture();
    database.query("CREATE (:Counter {id: 3})").unwrap();
    let returned = database
        .query(
            "MATCH (n:Counter) WHERE NOT (n.score = 9) SET n.score = 10 RETURN count(n) AS total",
        )
        .unwrap();
    assert_eq!(returned.rows[0]["total"], Value::Int(2));
    assert_eq!(score(&database, 3), Value::Null);
    let returned = database
        .query("MATCH (n:Counter) WHERE COALESCE(n.score, 0) >= 0 SET n.score = 7")
        .unwrap();
    assert_eq!(returned.rows.len(), 3);
    assert_eq!(score(&database, 3), Value::Int(7));
}

#[test]
fn out_of_core_unique_candidate_lookup_uses_the_complete_published_source() {
    let path = std::env::temp_dir().join(format!(
        "hawdb-autocommit-canonical-{}",
        hawdb_core::generate_uuidv7().unwrap()
    ));
    let config = DatabaseConfig {
        storage_residency_mode: crate::StorageResidencyMode::OutOfCore,
        ..Default::default()
    };
    let mut database = Database::open_with_config(&path, config.clone()).unwrap();
    database
        .query("CREATE (:Counter {id: 1, score: 0})")
        .unwrap();
    database
        .query("CREATE (:Counter {id: 2, score: 0})")
        .unwrap();
    database
        .query("CREATE CONSTRAINT ON :Counter(id) ASSERT UNIQUE")
        .unwrap();
    database.query("CREATE INDEX ON :Counter(id)").unwrap();
    database.checkpoint().unwrap();
    drop(database);
    let database = Database::open_with_config(&path, config).unwrap();
    assert!(database.storage_residency_report().unwrap().out_of_core);
    let database = database.into_concurrent();
    assert_eq!(
        database
            .query_with_params(UPDATE, &params(1, 5))
            .unwrap()
            .rows[0]["total"],
        Value::Int(1)
    );
    assert_eq!(score(&database, 1), Value::Int(5));
    assert_eq!(score(&database, 2), Value::Int(0));
    drop(database);
    std::fs::remove_dir_all(path).unwrap();
}

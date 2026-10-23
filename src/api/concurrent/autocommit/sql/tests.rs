// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::QueryStreamOptions;
use std::sync::{mpsc, Weak};
use std::thread;

const UPDATE: &str = "UPDATE counters SET counter = $2 WHERE id = $1";

fn fixture() -> ConcurrentDatabase {
    let database = Database::new().into_concurrent();
    database
        .query_sql("CREATE TABLE counters (id BIGINT PRIMARY KEY, counter BIGINT NOT NULL)")
        .unwrap();
    database
        .query_sql("INSERT INTO counters (id, counter) VALUES (1, 10), (2, 20)")
        .unwrap();
    database
}

fn counter(database: &ConcurrentDatabase, id: i64) -> Value {
    database
        .query_sql_with_params(
            "SELECT counter FROM counters WHERE id = $1",
            &[Value::Int(id)],
        )
        .unwrap()
        .rows[0]["counter"]
        .clone()
}

#[test]
fn point_writers_and_readers_progress_during_private_execution() {
    let database = fixture();
    let (ready, resume) =
        database.pause_next_autocommit_mutation(AutocommitMutationPhase::BeforeExecution);
    let first_database = database.clone();
    let first = thread::spawn(move || {
        first_database.query_sql_with_params(UPDATE, &[Value::Int(1), Value::Int(99)])
    });
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    let second_database = database.clone();
    let (done, completed) = mpsc::channel();
    let second = thread::spawn(move || {
        let result = (|| {
            for value in 1..=8 {
                second_database
                    .query_sql_with_params(UPDATE, &[Value::Int(2), Value::Int(value)])?;
            }
            Ok::<_, HawDBError>((counter(&second_database, 1), counter(&second_database, 2)))
        })();
        done.send(result).unwrap();
    });
    let progress = completed.recv_timeout(Duration::from_secs(5));
    resume.send(()).unwrap();
    first.join().unwrap().unwrap();
    second.join().unwrap();
    assert_eq!(progress.unwrap().unwrap(), (Value::Int(10), Value::Int(8)));
    assert_eq!(counter(&database, 1), Value::Int(99));
}

#[test]
fn lock_wait_releases_the_source_and_execution_sees_intervening_commits() {
    let database = fixture();
    let old_view = Arc::downgrade(&database.inner.commits.read_view().unwrap());
    let (ready, resume) =
        database.pause_next_autocommit_mutation(AutocommitMutationPhase::BeforeLocks);
    let writer_database = database.clone();
    let writer = thread::spawn(move || {
        writer_database.query_sql("UPDATE counters SET counter = counter + 1 WHERE id = 1")
    });
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    database
        .query_sql_with_params(UPDATE, &[Value::Int(1), Value::Int(40)])
        .unwrap();
    // No template, lock request or queued writer owns the old published source.
    assert_eq!(Weak::strong_count(&old_view), 0);
    resume.send(()).unwrap();
    writer.join().unwrap().unwrap();
    assert_eq!(counter(&database, 1), Value::Int(41));
}

#[test]
fn a_new_constraint_scope_is_refused_before_mutation() {
    let database = fixture();
    let (ready, resume) =
        database.pause_next_autocommit_mutation(AutocommitMutationPhase::BeforeLocks);
    let writer_database = database.clone();
    let writer = thread::spawn(move || {
        writer_database.query_sql_with_params(UPDATE, &[Value::Int(1), Value::Int(99)])
    });
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    database
        .query_sql("CREATE UNIQUE INDEX counters_value ON counters (counter)")
        .unwrap();
    let epoch = database.commit_epoch().unwrap();
    resume.send(()).unwrap();
    let error = writer.join().unwrap().unwrap_err();
    assert!(
        error.to_string().contains("prepare and admit again"),
        "{error}"
    );
    assert_eq!(database.commit_epoch().unwrap(), epoch);
    assert_eq!(counter(&database, 1), Value::Int(10));
    database
        .query_sql_with_params(UPDATE, &[Value::Int(1), Value::Int(99)])
        .unwrap();
}

#[test]
fn excluded_predicate_candidates_still_conflict_with_explicit_writers() {
    let database = fixture();
    let (ready, resume) =
        database.pause_next_autocommit_mutation(AutocommitMutationPhase::BeforeExecution);
    let writer_database = database.clone();
    let writer = thread::spawn(move || {
        writer_database.query_sql("UPDATE counters SET counter = 99 WHERE id = 1 AND counter = 0")
    });
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut competing = database
        .begin_transaction(ConcurrentTransactionOptions::pessimistic(
            Duration::from_millis(20),
        ))
        .unwrap();
    let conflict = competing.query_sql_with_params(UPDATE, &[Value::Int(1), Value::Int(0)]);
    drop(competing);
    resume.send(()).unwrap();
    assert!(conflict
        .unwrap_err()
        .to_string()
        .contains("lock wait timed out"));
    writer.join().unwrap().unwrap();
    assert_eq!(counter(&database, 1), Value::Int(10));
}

#[test]
fn inserts_on_different_tables_overlap_and_keep_generated_defaults() {
    let database = fixture();
    database
        .query_sql(
            "CREATE TABLE items (id UUID PRIMARY KEY DEFAULT uuidv7(), body TEXT NOT NULL UNIQUE)",
        )
        .unwrap();
    let (ready, resume) =
        database.pause_next_autocommit_mutation(AutocommitMutationPhase::BeforeExecution);
    let writer_database = database.clone();
    let writer = thread::spawn(move || {
        writer_database.query_sql_with_params(
            "INSERT INTO items (body) VALUES ($1) RETURNING id",
            &[Value::String("first".into())],
        )
    });
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    let other_database = database.clone();
    let (done, completed) = mpsc::channel();
    let other = thread::spawn(move || {
        done.send(other_database.query_sql("INSERT INTO counters (id, counter) VALUES (3, 30)"))
            .unwrap();
    });
    let progress = completed.recv_timeout(Duration::from_secs(5));
    resume.send(()).unwrap();
    let returned = writer.join().unwrap().unwrap();
    other.join().unwrap();
    progress.unwrap().unwrap();
    let stored = database.query_sql("SELECT id FROM items").unwrap();
    assert!(matches!(returned.rows[0]["id"], Value::Uuid(_)));
    assert_eq!(returned.rows[0]["id"], stored.rows[0]["id"]);
}

#[test]
fn foreign_key_cascades_protect_child_writes_before_publication() {
    let database = fixture();
    database
        .query_sql("CREATE TABLE children (id BIGINT PRIMARY KEY, parent_id BIGINT NOT NULL REFERENCES counters(id) ON DELETE CASCADE)")
        .unwrap();
    database
        .query_sql("INSERT INTO children (id, parent_id) VALUES (1, 1)")
        .unwrap();
    let (ready, resume) =
        database.pause_next_autocommit_mutation(AutocommitMutationPhase::BeforeExecution);
    let writer_database = database.clone();
    let writer =
        thread::spawn(move || writer_database.query_sql("DELETE FROM counters WHERE id = 1"));
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut child = database
        .begin_transaction(ConcurrentTransactionOptions::pessimistic(
            Duration::from_millis(20),
        ))
        .unwrap();
    let conflict = child.query_sql("INSERT INTO children (id, parent_id) VALUES (2, 1)");
    drop(child);
    resume.send(()).unwrap();
    assert!(conflict
        .unwrap_err()
        .to_string()
        .contains("lock wait timed out"));
    writer.join().unwrap().unwrap();
    assert!(database
        .query_sql("SELECT id FROM children")
        .unwrap()
        .rows
        .is_empty());
}

#[test]
fn returning_refusal_and_cancellation_preserve_the_durable_prefix() {
    let path = std::env::temp_dir().join(format!(
        "hawdb-sql-autocommit-{}",
        hawdb_core::generate_uuidv7().unwrap()
    ));
    let database = Database::open(&path).unwrap().into_concurrent();
    database
        .query_sql("CREATE TABLE items (id BIGINT PRIMARY KEY, body TEXT NOT NULL)")
        .unwrap();
    let epoch = database.commit_epoch().unwrap();
    for limits in [
        QueryStreamOptions {
            max_rows: Some(0),
            max_payload_bytes: Some(4096),
        },
        QueryStreamOptions {
            max_rows: Some(1),
            max_payload_bytes: Some(0),
        },
    ] {
        let result = database.query_sql_request(
            SqlQueryRequest::new(
                "INSERT INTO items (id, body) VALUES (1, 'rejected') RETURNING body",
            )
            .with_output_limits(limits),
        );
        assert!(
            matches!(result, Err(HawDBError::ReadBudgetExceeded(_))),
            "{result:?}"
        );
        assert_eq!(database.commit_epoch().unwrap(), epoch);
    }
    let (ready, resume) = database.pause_next_autocommit_mutation(AutocommitMutationPhase::Staged);
    let task = hawdb_core::RuntimeTaskContext::default();
    let other_task = task.clone();
    let writer_database = database.clone();
    let writer = thread::spawn(move || {
        writer_database.query_sql_request(
            SqlQueryRequest::new(
                "INSERT INTO items (id, body) VALUES (1, 'cancelled') RETURNING id",
            )
            .with_task_context(&other_task),
        )
    });
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    task.cancellation().cancel();
    resume.send(()).unwrap();
    assert!(writer.join().unwrap().is_err());
    assert_eq!(database.commit_epoch().unwrap(), epoch);
    let returned = database
        .query_sql("INSERT INTO items (id, body) VALUES (1, 'durable') RETURNING body")
        .unwrap();
    assert_eq!(returned.rows[0]["body"], Value::String("durable".into()));
    drop(database);
    let mut reopened = Database::open(&path).unwrap();
    let rows = reopened.query_sql("SELECT body FROM items").unwrap();
    assert_eq!(rows.rows.len(), 1);
    assert_eq!(rows.rows[0]["body"], Value::String("durable".into()));
    drop(reopened);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn ddl_and_append_stage_privately_without_reparsing_the_template() {
    let database = Database::new().into_concurrent();
    for sql in [
        "CREATE TABLE events (stream_id TEXT NOT NULL, sequence BIGINT NOT NULL, payload TEXT NOT NULL) WITH (storage_mode = 'strict_append', partition_key = 'stream_id', order_key = 'sequence', generated_order = 'commit_sequence')",
        "INSERT INTO events (stream_id, payload) VALUES ('stream', 'first')",
        "INSERT INTO events (stream_id, payload) VALUES ('stream', 'second')",
    ] {
        let prepared = database.inner.commits.prepare_sql(sql).unwrap();
        let view = database.inner.commits.read_view().unwrap();
        database.query_prepared_sql_mutation(
            SqlQueryRequest::new("/* syntax already prepared */"), prepared, view, Instant::now()
        ).unwrap();
    }
    let rows = database.query_sql("SELECT sequence, payload FROM events WHERE stream_id = 'stream' ORDER BY sequence LIMIT 2").unwrap();
    assert_eq!(rows.rows[0]["sequence"], Value::Int(1));
    assert_eq!(rows.rows[1]["sequence"], Value::Int(2));
    assert!(database
        .query_sql("UPDATE events SET payload = 'invalid'")
        .is_err());
    assert!(fixture()
        .query_sql("SELECT id FROM counters WHERE id = 1 FOR UPDATE")
        .is_err());
}

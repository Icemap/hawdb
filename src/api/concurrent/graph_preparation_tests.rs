// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{QueryRequest, QueryStreamOptions};

const UPDATE: &str = "MATCH (n:Counter) WHERE n.id = $id \
    SET n.score = $score RETURN count(n) AS total";

fn database() -> ConcurrentDatabase {
    let mut database = Database::new();
    database
        .query("CREATE (:Counter {id: 1, score: 0})")
        .unwrap();
    database
        .query("CREATE (:Counter {id: 2, score: 0})")
        .unwrap();
    database.into_concurrent()
}

fn parameters(id: i64, score: i64) -> BTreeMap<String, Value> {
    BTreeMap::from([
        ("id".into(), Value::Int(id)),
        ("score".into(), Value::Int(score)),
    ])
}

#[test]
fn sealed_commit_preparation_allows_other_commits_and_revalidates_before_wal() {
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    for conflicting in [false, true] {
        let path = std::env::temp_dir().join(format!(
            "hawdb-sealed-preparation-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut source = Database::open(&path).unwrap();
        source.query("CREATE (:Counter {id: 1, score: 0})").unwrap();
        source.query("CREATE (:Counter {id: 2, score: 0})").unwrap();
        let database = source.into_concurrent();
        let (ready, reached) = mpsc::channel();
        let (resume, release) = mpsc::channel();
        let writer_database = database.clone();
        let writer = thread::spawn(move || {
            let mut transaction = writer_database
                .begin_transaction(ConcurrentTransactionOptions::optimistic())
                .unwrap();
            transaction
                .query_with_params(UPDATE, &parameters(1, 11))
                .unwrap();
            set_graph_commit_preparation_hook(Box::new(move || {
                ready.send(()).unwrap();
                release.recv_timeout(Duration::from_secs(10)).unwrap();
            }));
            transaction.commit()
        });
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        let (completed, progress) = mpsc::channel();
        let other_database = database.clone();
        let other = thread::spawn(move || {
            let mut transaction = other_database
                .begin_transaction(ConcurrentTransactionOptions::optimistic())
                .unwrap();
            transaction
                .query_with_params(UPDATE, &parameters(if conflicting { 1 } else { 2 }, 22))
                .unwrap();
            transaction.commit().unwrap();
            completed.send(()).unwrap();
        });
        let advanced_while_preparing = progress.recv_timeout(Duration::from_secs(5));
        resume.send(()).unwrap();
        let result = writer.join().unwrap();
        other.join().unwrap();
        assert!(
            advanced_while_preparing.is_ok(),
            "competing commit waited for preparation"
        );
        if conflicting {
            assert!(matches!(
                result,
                Err(HawDBError::TransactionConflict { .. })
            ));
        } else {
            result.unwrap();
        }
        let expected = if conflicting { 22 } else { 11 };
        let query = "MATCH (n:Counter {id: 1}) RETURN n.score AS score";
        assert_eq!(
            database.query(query).unwrap().rows[0]["score"],
            Value::Int(expected)
        );
        drop(database);
        let mut reopened = Database::open(&path).unwrap();
        assert_eq!(
            reopened.query(query).unwrap().rows[0]["score"],
            Value::Int(expected)
        );
        drop(reopened);
        std::fs::remove_dir_all(path).unwrap();
    }
}

fn preparations(transaction: &ConcurrentDatabaseTransaction) -> u64 {
    transaction.runtime.plan_cache.borrow().stats().bypasses
}

#[test]
fn pessimistic_replay_reuses_preparation_and_next_statement_rebinds_parameters() {
    let database = database();
    database.enable_concurrency_metrics();
    let mut transaction = database.begin_transaction(Default::default()).unwrap();
    for (id, score, expected_preparations, expected_replays) in
        [(1, 11, 1, 1), (2, 22, 2, 2), (1, 33, 3, 2)]
    {
        let parameters = parameters(id, score);
        let result = transaction
            .query_request(
                QueryRequest::new(UPDATE)
                    .with_params(&parameters)
                    .with_output_limits(QueryStreamOptions {
                        max_rows: Some(1),
                        max_payload_bytes: Some(13),
                    }),
            )
            .unwrap();
        assert_eq!(result.rows[0]["total"], Value::Int(1));
        assert_eq!(preparations(&transaction), expected_preparations);
        assert_eq!(
            database.concurrency_metrics().graph_statement_replays,
            expected_replays
        );
    }
    assert_eq!(database.concurrency_metrics().graph_statement_reuses, 1);
    let staged = transaction
        .query("MATCH (n:Counter) RETURN n.id AS id, n.score AS score ORDER BY id")
        .unwrap();
    assert_eq!(staged.rows[0]["score"], Value::Int(33));
    assert_eq!(staged.rows[1]["score"], Value::Int(22));
    assert_eq!(preparations(&transaction), 4);
    transaction.commit().unwrap();
    let published = database
        .query("MATCH (n:Counter) RETURN n.id AS id, n.score AS score ORDER BY id")
        .unwrap();
    assert_eq!(published, staged);
}

#[test]
fn refused_prepared_mutation_preserves_prior_statement_and_its_locks() {
    let database = database();
    database.enable_concurrency_metrics();
    let mut transaction = database.begin_transaction(Default::default()).unwrap();
    transaction
        .query_with_params(UPDATE, &parameters(1, 11))
        .unwrap();
    let before = database.commit_epoch().unwrap();
    let refused_parameters = parameters(2, 99);
    let error = transaction
        .query_request(
            QueryRequest::new(UPDATE)
                .with_params(&refused_parameters)
                .with_output_limits(QueryStreamOptions {
                    max_rows: Some(1),
                    max_payload_bytes: Some(0),
                }),
        )
        .unwrap_err();
    assert!(matches!(error, HawDBError::ReadBudgetExceeded(_)));
    assert_eq!(preparations(&transaction), 2);
    assert_eq!(database.concurrency_metrics().graph_statement_replays, 1);
    assert_eq!(database.commit_epoch().unwrap(), before);
    transaction
        .query_with_params(UPDATE, &parameters(1, 33))
        .unwrap();
    assert_eq!(preparations(&transaction), 3);
    assert_eq!(database.concurrency_metrics().graph_statement_reuses, 1);
    transaction.commit().unwrap();
    let result = database
        .query("MATCH (n:Counter) RETURN n.id AS id, n.score AS score ORDER BY id")
        .unwrap();
    assert_eq!(result.rows[0]["score"], Value::Int(33));
    assert_eq!(result.rows[1]["score"], Value::Int(0));
}

#[test]
fn next_statement_prepares_against_the_staged_schema() {
    let database = database();
    database.enable_concurrency_metrics();
    let mut transaction = database.begin_transaction(Default::default()).unwrap();
    for (index, statement) in [
        "CREATE NODE TABLE Planned",
        "CREATE PROPERTY ON NODE TABLE Planned(score) TYPE INT NOT NULL",
        "CREATE (:Planned {score: 7})",
    ]
    .into_iter()
    .enumerate()
    {
        transaction.query(statement).unwrap();
        assert_eq!(preparations(&transaction), index as u64 + 1);
    }
    let staged = transaction
        .query("MATCH (n:Planned) RETURN n.score AS score")
        .unwrap();
    assert_eq!(staged.rows[0]["score"], Value::Int(7));
    assert_eq!(preparations(&transaction), 4);
    assert_eq!(database.concurrency_metrics().graph_statement_replays, 1);
    transaction.commit().unwrap();
    assert_eq!(
        database
            .query("MATCH (n:Planned) RETURN n.score AS score")
            .unwrap(),
        staged
    );
}

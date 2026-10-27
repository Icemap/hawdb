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
use crate::QueryRequest;

const CANDIDATES: &str = "MATCH (j:AugmentationJob) WHERE j.status IN $statuses \
    RETURN j.job_id AS job_id, j.status AS previous_status ORDER BY job_id";
const INTERRUPT: &str = "MATCH (j:AugmentationJob) WHERE j.status IN $statuses \
    SET j.status = $status, j.message = $message, j.error_message = $error, \
    j.completed_at = $completed_at RETURN j.job_id AS job_id";

fn bounded_query<'a>(query: &'a str, parameters: &'a BTreeMap<String, Value>) -> QueryRequest<'a> {
    QueryRequest::new(query)
        .with_params(parameters)
        .with_output_limits(QueryStreamOptions {
            max_rows: Some(4),
            max_payload_bytes: Some(4096),
        })
}

fn parameters(error: &str, completed_at: i64) -> Result<BTreeMap<String, Value>> {
    // Input validation belongs to the host; graph selection belongs to Cypher.
    if error.is_empty() {
        return Err(HawDBError::Semantic(
            "non-empty error message required".into(),
        ));
    }
    Ok(BTreeMap::from([
        (
            "statuses".into(),
            Value::List(vec![
                Value::String("pending".into()),
                Value::String("running".into()),
            ]),
        ),
        ("status".into(), Value::String("failed".into())),
        (
            "message".into(),
            Value::String("Interrupted before completion".into()),
        ),
        ("error".into(), Value::String(error.into())),
        ("completed_at".into(), Value::Int(completed_at)),
    ]))
}

#[test]
fn interrupts_pending_and_running_augmentation_jobs_for_nowledge_orphans() {
    let mut db = Database::new();
    db.query("CREATE (:AugmentationJob {job_id: 'pending_job', status: 'pending'})")
        .unwrap();
    db.query("CREATE (:AugmentationJob {job_id: 'running_job', status: 'running'})")
        .unwrap();
    db.query(
        "CREATE (:AugmentationJob {job_id: 'completed_job', status: 'completed', message: 'done'})",
    )
    .unwrap();
    db.query("CREATE (:AugmentationJob {job_id: 'failed_job', status: 'failed', message: 'old failure'})")
        .unwrap();

    let parameters = parameters("stale owner", 300).unwrap();
    let before = db.commit_epoch().unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    let candidates = transaction
        .query_request(bounded_query(CANDIDATES, &parameters))
        .unwrap();
    assert_eq!(candidates.rows.len(), 2);
    assert_eq!(
        candidates.rows[0]["job_id"],
        Value::String("pending_job".into())
    );
    assert_eq!(
        candidates.rows[0]["previous_status"],
        Value::String("pending".into())
    );
    assert_eq!(
        candidates.rows[1]["job_id"],
        Value::String("running_job".into())
    );
    assert_eq!(
        candidates.rows[1]["previous_status"],
        Value::String("running".into())
    );
    let output = transaction
        .query_request(bounded_query(INTERRUPT, &parameters))
        .unwrap();
    assert_eq!(output.rows.len(), 2);
    transaction.commit().unwrap();
    assert_eq!(before, 4);
    assert_eq!(db.commit_epoch().unwrap(), before + 1);

    let rows = db
        .query_request(bounded_query("MATCH (j:AugmentationJob) RETURN j.job_id AS id, j.status AS status, j.message AS message, j.error_message AS error, j.completed_at AS completed_at ORDER BY id ASC", &BTreeMap::new()))
        .unwrap();
    let completed = rows
        .rows
        .iter()
        .find(|row| row.get("id") == Some(&Value::String("completed_job".to_string())))
        .unwrap();
    assert_eq!(
        completed.get("status"),
        Some(&Value::String("completed".to_string()))
    );
    assert_eq!(
        completed.get("message"),
        Some(&Value::String("done".to_string()))
    );

    for job_id in ["pending_job", "running_job"] {
        let row = rows
            .rows
            .iter()
            .find(|row| row.get("id") == Some(&Value::String(job_id.to_string())))
            .unwrap();
        assert_eq!(
            row.get("status"),
            Some(&Value::String("failed".to_string()))
        );
        assert_eq!(
            row.get("message"),
            Some(&Value::String("Interrupted before completion".to_string()))
        );
        assert_eq!(
            row.get("error"),
            Some(&Value::String("stale owner".to_string()))
        );
        assert_eq!(row.get("completed_at"), Some(&Value::Int(300)));
    }
}

#[test]
fn augmentation_job_interrupt_without_candidates_does_not_write_wal() {
    let path = unique_test_dir("augmentation_job_interrupt_without_candidates");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:AugmentationJob {job_id: 'completed_job', status: 'completed'})")
            .unwrap();
    }
    let wal_before = read_test_wal(&path).unwrap();
    {
        let mut db = Database::open(&path).unwrap();
        let graph_commit_epoch_before = db.runtime.get().unwrap().store.commit_epoch();
        let parameters = parameters("no candidates", 301).unwrap();
        let output = db
            .query_request(bounded_query(INTERRUPT, &parameters))
            .unwrap();
        assert!(output.rows.is_empty());
        assert_eq!(db.commit_epoch().unwrap(), graph_commit_epoch_before);
    }
    let wal_after = read_test_wal(&path).unwrap();
    assert_eq!(wal_after, wal_before);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn augmentation_job_interrupt_rejects_empty_reason_before_wal() {
    let mut db = Database::new();
    db.query("CREATE (:AugmentationJob {job_id: 'pending_job', status: 'pending'})")
        .unwrap();
    let graph_commit_epoch_before = db.runtime.get().unwrap().store.commit_epoch();

    let error = parameters("", 302).unwrap_err();

    assert!(error.to_string().contains("non-empty error message"));
    assert_eq!(
        db.runtime.get().unwrap().store.commit_epoch(),
        graph_commit_epoch_before
    );
}

#[test]
fn augmentation_job_interrupt_queries_persist_as_one_wal_batch_and_replays() {
    let path = unique_test_dir("augmentation_job_interrupt_query_wal_replay");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:AugmentationJob {job_id: 'pending_job', status: 'pending'})")
            .unwrap();
        db.query("CREATE (:AugmentationJob {job_id: 'running_job', status: 'running'})")
            .unwrap();
    }
    let setup_wal = read_test_wal(&path).unwrap();
    let setup_batch_count = setup_wal.matches("\tbatch\t").count();
    {
        let mut db = Database::open(&path).unwrap();
        let parameters = parameters("shutdown", 303).unwrap();
        let output = db
            .query_request(bounded_query(INTERRUPT, &parameters))
            .unwrap();
        assert_eq!(output.rows.len(), 2);
    }
    let wal = read_test_wal(&path).unwrap();
    assert!(wal.contains("set_node_property"));
    assert_eq!(wal.matches("\tbatch\t").count(), setup_batch_count + 1);
    {
        let mut db = Database::open(&path).unwrap();
        let rows = db
            .query_request(bounded_query("MATCH (j:AugmentationJob) RETURN j.job_id AS id, j.status AS status, j.message AS message, j.error_message AS error, j.completed_at AS completed_at ORDER BY id ASC", &BTreeMap::new()))
            .unwrap();
        assert_eq!(rows.rows.len(), 2);
        for row in &rows.rows {
            assert_eq!(
                row.get("status"),
                Some(&Value::String("failed".to_string()))
            );
            assert_eq!(
                row.get("message"),
                Some(&Value::String("Interrupted before completion".to_string()))
            );
            assert_eq!(
                row.get("error"),
                Some(&Value::String("shutdown".to_string()))
            );
            assert_eq!(row.get("completed_at"), Some(&Value::Int(303)));
        }
    }
    std::fs::remove_dir_all(path).unwrap();
}

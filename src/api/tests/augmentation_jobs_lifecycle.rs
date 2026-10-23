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

const CREATE: &str = "MERGE (j:AugmentationJob {job_id: $job_id}) ON CREATE SET \
    j.job_type = $job_type, j.parameters = $parameters, j.created_at = $created_at, \
    j.status = 'pending', j.progress = 0.0, j.message = 'Job created', \
    j.result = '{}', j.error_message = '', j.started_at = NULL, j.completed_at = NULL \
    RETURN j.job_id AS id";
const START: &str = "MATCH (j:AugmentationJob {job_id: $job_id}) WHERE j.status = 'pending' \
    SET j.status = 'running', j.started_at = $started_at, j.message = 'Job started' \
    RETURN j.job_id AS id";
const PROGRESS: &str = "MATCH (j:AugmentationJob {job_id: $job_id}) WHERE j.status = 'running' \
    SET j.progress = $progress, j.message = $message RETURN j.job_id AS id";
const COMPLETE: &str = "MATCH (j:AugmentationJob {job_id: $job_id}) WHERE j.status = 'running' \
    SET j.status = 'completed', j.progress = 100.0, j.message = 'Job completed successfully', \
    j.result = $result, j.completed_at = $completed_at RETURN j.job_id AS id";
const FAIL: &str =
    "MATCH (j:AugmentationJob {job_id: $job_id}) WHERE j.status IN ['pending', 'running'] \
    SET j.status = 'failed', j.message = 'Job failed', j.error_message = $error_message, \
    j.completed_at = $completed_at RETURN j.job_id AS id";

fn bounded_query<'a>(query: &'a str, parameters: &'a BTreeMap<String, Value>) -> QueryRequest<'a> {
    QueryRequest::new(query)
        .with_params(parameters)
        .with_output_limits(QueryStreamOptions {
            max_rows: Some(5),
            max_payload_bytes: Some(4096),
        })
}

fn progress_parameters(progress: f64, message: &str) -> Result<BTreeMap<String, Value>> {
    // Request normalization has no database access or graph semantics.
    if !progress.is_finite() || !(0.0..=100.0).contains(&progress) {
        return Err(HawDBError::Semantic("finite percentage required".into()));
    }
    if message.is_empty() {
        return Err(HawDBError::Semantic("non-empty message required".into()));
    }
    Ok(BTreeMap::from([
        ("progress".into(), Value::Float(progress)),
        ("message".into(), Value::String(message.into())),
    ]))
}

#[test]
fn updates_augmentation_job_lifecycle_batch_for_nowledge_shapes() {
    let mut db = Database::new();
    db.query("CREATE (:AugmentationJob {job_id: 'pending_job', status: 'pending'})")
        .unwrap();
    db.query("CREATE (:AugmentationJob {job_id: 'running_progress', status: 'running', progress: 1.0, message: 'old'})")
        .unwrap();
    db.query(
        "CREATE (:AugmentationJob {job_id: 'running_complete', status: 'running', progress: 50.0})",
    )
    .unwrap();
    db.query("CREATE (:AugmentationJob {job_id: 'completed_job', status: 'completed'})")
        .unwrap();

    let operations = [
        (
            "created_job",
            CREATE,
            BTreeMap::from([
                ("job_type".into(), Value::String("pagerank".into())),
                (
                    "parameters".into(),
                    Value::String("{\"graph\":\"main\"}".into()),
                ),
                ("created_at".into(), Value::Int(100)),
            ]),
        ),
        (
            "pending_job",
            START,
            BTreeMap::from([("started_at".into(), Value::Int(101))]),
        ),
        (
            "running_progress",
            PROGRESS,
            progress_parameters(42.5, "halfway").unwrap(),
        ),
        (
            "running_complete",
            COMPLETE,
            BTreeMap::from([
                ("result".into(), Value::String("{\"ok\":true}".into())),
                ("completed_at".into(), Value::Int(102)),
            ]),
        ),
        (
            "completed_job",
            START,
            BTreeMap::from([("started_at".into(), Value::Int(103))]),
        ),
        (
            "missing_job",
            FAIL,
            BTreeMap::from([
                ("error_message".into(), Value::String("missing".into())),
                ("completed_at".into(), Value::Int(104)),
            ]),
        ),
        (
            "created_job",
            CREATE,
            BTreeMap::from([
                ("job_type".into(), Value::String("duplicate".into())),
                ("parameters".into(), Value::String("{}".into())),
                ("created_at".into(), Value::Int(105)),
            ]),
        ),
    ];
    let before = db.commit_epoch().unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    let mut seen = BTreeSet::new();
    let mut counts = Vec::new();
    let mut duplicates = 0;
    for (job_id, query, mut parameters) in operations {
        // De-duplicate caller input; never enumerate or filter graph rows here.
        if !seen.insert(job_id) {
            duplicates += 1;
            continue;
        }
        parameters.insert("job_id".into(), Value::String(job_id.into()));
        let output = transaction
            .query_request(bounded_query(query, &parameters))
            .unwrap();
        counts.push(output.rows.len());
    }
    assert_eq!(counts, [1, 1, 1, 1, 0, 0]);
    assert_eq!(duplicates, 1);
    transaction.commit().unwrap();
    assert_eq!(before, 4);
    assert_eq!(db.commit_epoch().unwrap(), before + 1);

    let rows = db
        .query_request(bounded_query("MATCH (j:AugmentationJob) RETURN j.job_id AS id, j.status AS status, j.progress AS progress, j.message AS message, j.result AS result, j.error_message AS error, j.started_at AS started_at, j.completed_at AS completed_at, j.created_at AS created_at ORDER BY id ASC", &BTreeMap::new()))
        .unwrap();
    let created = rows
        .rows
        .iter()
        .find(|row| row.get("id") == Some(&Value::String("created_job".to_string())))
        .unwrap();
    assert_eq!(
        created.get("status"),
        Some(&Value::String("pending".to_string()))
    );
    assert_eq!(created.get("progress"), Some(&Value::Float(0.0)));
    assert_eq!(
        created.get("message"),
        Some(&Value::String("Job created".to_string()))
    );
    assert_eq!(created.get("created_at"), Some(&Value::Int(100)));

    let pending = rows
        .rows
        .iter()
        .find(|row| row.get("id") == Some(&Value::String("pending_job".to_string())))
        .unwrap();
    assert_eq!(
        pending.get("status"),
        Some(&Value::String("running".to_string()))
    );
    assert_eq!(pending.get("started_at"), Some(&Value::Int(101)));

    let progress = rows
        .rows
        .iter()
        .find(|row| row.get("id") == Some(&Value::String("running_progress".to_string())))
        .unwrap();
    assert_eq!(progress.get("progress"), Some(&Value::Float(42.5)));
    assert_eq!(
        progress.get("message"),
        Some(&Value::String("halfway".to_string()))
    );

    let completed = rows
        .rows
        .iter()
        .find(|row| row.get("id") == Some(&Value::String("running_complete".to_string())))
        .unwrap();
    assert_eq!(
        completed.get("status"),
        Some(&Value::String("completed".to_string()))
    );
    assert_eq!(completed.get("progress"), Some(&Value::Float(100.0)));
    assert_eq!(
        completed.get("message"),
        Some(&Value::String("Job completed successfully".to_string()))
    );
    assert_eq!(
        completed.get("result"),
        Some(&Value::String("{\"ok\":true}".to_string()))
    );
    assert_eq!(completed.get("completed_at"), Some(&Value::Int(102)));
}

#[test]
fn augmentation_job_progress_rejects_invalid_percentage_before_wal() {
    let mut db = Database::new();
    db.query("CREATE (:AugmentationJob {job_id: 'running_job', status: 'running'})")
        .unwrap();
    let graph_commit_epoch_before = db.runtime.get().unwrap().store.commit_epoch();

    for progress in [101.0, -1.0, f64::NAN, f64::INFINITY] {
        let error = progress_parameters(progress, "bad").unwrap_err();
        assert!(error.to_string().contains("finite percentage"));
    }
    assert!(progress_parameters(50.0, "").is_err());
    assert_eq!(
        db.runtime.get().unwrap().store.commit_epoch(),
        graph_commit_epoch_before
    );
}

#[test]
fn augmentation_job_lifecycle_queries_persist_as_one_wal_batch_and_replays() {
    let path = unique_test_dir("augmentation_job_lifecycle_query_wal_replay");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:AugmentationJob {job_id: 'running_job', status: 'running'})")
            .unwrap();
        let batch_count_before_update = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        let mut transaction = db.begin_transaction().unwrap();
        let parameters = BTreeMap::from([
            ("job_id".into(), Value::String("created_job".into())),
            ("job_type".into(), Value::String("louvain".into())),
            ("parameters".into(), Value::String("{}".into())),
            ("created_at".into(), Value::Int(10)),
        ]);
        assert_eq!(
            transaction
                .query_request(bounded_query(CREATE, &parameters))
                .unwrap()
                .rows
                .len(),
            1
        );
        let parameters = BTreeMap::from([
            ("job_id".into(), Value::String("running_job".into())),
            (
                "error_message".into(),
                Value::String("runtime failed".into()),
            ),
            ("completed_at".into(), Value::Int(20)),
        ]);
        assert_eq!(
            transaction
                .query_request(bounded_query(FAIL, &parameters))
                .unwrap()
                .rows
                .len(),
            1
        );
        transaction.commit().unwrap();
        let batch_count_after_update = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        assert_eq!(batch_count_after_update, batch_count_before_update + 1);
    }
    let wal = read_test_wal(&path).unwrap();
    assert!(wal.contains("create_node"));
    assert!(wal.contains("set_node_property"));
    {
        let mut db = Database::open(&path).unwrap();
        let rows = db
            .query_request(bounded_query("MATCH (j:AugmentationJob) RETURN j.job_id AS id, j.status AS status, j.job_type AS job_type, j.error_message AS error, j.completed_at AS completed_at ORDER BY id ASC", &BTreeMap::new()))
            .unwrap();
        assert_eq!(rows.rows.len(), 2);
        assert_eq!(
            rows.rows[0].get("id"),
            Some(&Value::String("created_job".to_string()))
        );
        assert_eq!(
            rows.rows[0].get("status"),
            Some(&Value::String("pending".to_string()))
        );
        assert_eq!(
            rows.rows[0].get("job_type"),
            Some(&Value::String("louvain".to_string()))
        );
        assert_eq!(
            rows.rows[1].get("id"),
            Some(&Value::String("running_job".to_string()))
        );
        assert_eq!(
            rows.rows[1].get("status"),
            Some(&Value::String("failed".to_string()))
        );
        assert_eq!(
            rows.rows[1].get("error"),
            Some(&Value::String("runtime failed".to_string()))
        );
        assert_eq!(rows.rows[1].get("completed_at"), Some(&Value::Int(20)));
    }
    std::fs::remove_dir_all(path).unwrap();
}

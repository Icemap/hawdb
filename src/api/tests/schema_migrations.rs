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

const APPLY: &str = "MERGE (m:SchemaMigrationLog {id: $id}) \
    ON CREATE SET m.applied_at = $applied_at \
    RETURN id(m) AS node_id, m.applied_at AS applied_at";

fn bounded_query<'a>(
    query: &'a str,
    parameters: &'a BTreeMap<String, Value>,
    rows: usize,
) -> QueryRequest<'a> {
    QueryRequest::new(query)
        .with_params(parameters)
        .with_output_limits(QueryStreamOptions {
            max_rows: Some(rows),
            max_payload_bytes: Some(4096),
        })
}

fn parameters(id: &str, applied_at: i64) -> Result<BTreeMap<String, Value>> {
    if id.is_empty() {
        return Err(HawDBError::Semantic(
            "non-empty migration id required".into(),
        ));
    }
    Ok(BTreeMap::from([
        ("id".into(), Value::String(id.into())),
        ("applied_at".into(), Value::Int(applied_at)),
    ]))
}

const SCHEMA_MIGRATION_COUNT_QUERY: &str = "MATCH (m:SchemaMigrationLog) RETURN count(m) AS total";
const SCHEMA_MIGRATION_ALL_QUERY: &str = "MATCH (m:SchemaMigrationLog) \
     RETURN m.id AS migration_id, id(m) AS node_id, m.applied_at AS applied_at \
     ORDER BY migration_id ASC";
const SCHEMA_MIGRATION_PAGE_QUERY: &str = "MATCH (m:SchemaMigrationLog) \
     RETURN m.id AS migration_id, id(m) AS node_id, m.applied_at AS applied_at \
     ORDER BY migration_id ASC LIMIT $limit";

#[test]
fn applies_schema_migration_log_batch_idempotently() {
    let mut db = Database::new();
    db.query("CREATE (:SchemaMigrationLog {id: 'existing', applied_at: 10})")
        .unwrap();

    let before = db.commit_epoch().unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    let mut node_ids = Vec::new();
    for (id, timestamp, expected) in [
        ("existing", 100, 10),
        ("new_1", 101, 101),
        ("new_1", 102, 101),
        ("new_2", 103, 103),
    ] {
        let parameters = parameters(id, timestamp).unwrap();
        let output = transaction
            .query_request(bounded_query(APPLY, &parameters, 1))
            .unwrap();
        assert_eq!(output.rows.len(), 1);
        assert_eq!(output.rows[0]["applied_at"], Value::Int(expected));
        node_ids.push(output.rows[0]["node_id"].clone());
    }
    assert_eq!(node_ids[1], node_ids[2]);
    assert_ne!(node_ids[0], node_ids[1]);
    assert_ne!(node_ids[1], node_ids[3]);
    transaction.commit().unwrap();
    assert_eq!(before, 1);
    assert_eq!(db.commit_epoch().unwrap(), before + 1);

    let rows = db
        .query_request(bounded_query("MATCH (m:SchemaMigrationLog) RETURN m.id AS id, m.applied_at AS applied_at ORDER BY id ASC", &BTreeMap::new(), 3))
        .unwrap();
    assert_eq!(rows.rows.len(), 3);
    assert_eq!(
        rows.rows[0].get("id"),
        Some(&Value::String("existing".to_string()))
    );
    assert_eq!(rows.rows[0].get("applied_at"), Some(&Value::Int(10)));
    assert_eq!(
        rows.rows[1].get("id"),
        Some(&Value::String("new_1".to_string()))
    );
    assert_eq!(rows.rows[1].get("applied_at"), Some(&Value::Int(101)));
    assert_eq!(
        rows.rows[2].get("id"),
        Some(&Value::String("new_2".to_string()))
    );
    assert_eq!(rows.rows[2].get("applied_at"), Some(&Value::Int(103)));

    let mut read = db.begin_read_transaction().unwrap();
    let count = read
        .query_request(bounded_query(
            SCHEMA_MIGRATION_COUNT_QUERY,
            &BTreeMap::new(),
            1,
        ))
        .unwrap();
    assert_eq!(count.rows[0].get("total"), Some(&Value::Int(3)));
    let applied = read
        .query_request(bounded_query(
            SCHEMA_MIGRATION_ALL_QUERY,
            &BTreeMap::new(),
            3,
        ))
        .unwrap();
    assert_eq!(
        read.commit_epoch(),
        db.runtime.get().unwrap().store.commit_epoch()
    );
    assert_eq!(applied.rows.len(), 3);
    assert_eq!(
        applied
            .rows
            .iter()
            .map(|row| row.get("migration_id").unwrap())
            .collect::<Vec<_>>(),
        vec![
            &Value::String("existing".to_string()),
            &Value::String("new_1".to_string()),
            &Value::String("new_2".to_string())
        ]
    );
    assert_eq!(applied.rows[0].get("applied_at"), Some(&Value::Int(10)));
    assert_eq!(applied.rows[1].get("applied_at"), Some(&Value::Int(101)));
    assert_eq!(applied.rows[2].get("applied_at"), Some(&Value::Int(103)));

    let limited = read
        .query_request(bounded_query(
            SCHEMA_MIGRATION_PAGE_QUERY,
            &BTreeMap::from([("limit".to_string(), Value::Int(2))]),
            2,
        ))
        .unwrap();
    assert_eq!(limited.rows.len(), 2);
    assert_eq!(
        limited
            .rows
            .iter()
            .map(|row| row.get("migration_id").unwrap())
            .collect::<Vec<_>>(),
        vec![
            &Value::String("existing".to_string()),
            &Value::String("new_1".to_string())
        ]
    );
}

#[test]
fn schema_migration_apply_rejects_empty_id_before_wal() {
    let db = Database::new();
    let graph_commit_epoch_before = db.runtime.get().unwrap().store.commit_epoch();

    let error = parameters("", 100).unwrap_err();

    assert!(error.to_string().contains("non-empty migration id"));
    assert_eq!(
        db.runtime.get().unwrap().store.commit_epoch(),
        graph_commit_epoch_before
    );
}

#[test]
fn schema_migration_queries_persist_as_one_wal_batch_and_replays() {
    let path = unique_test_dir("schema_migration_query_wal_replay");
    {
        let mut db = Database::open(&path).unwrap();
        let batch_count_before_update = read_test_wal(&path)
            .ok()
            .map_or(0, |wal| wal.matches("\tbatch\t").count());
        let mut transaction = db.begin_transaction().unwrap();
        for (id, timestamp) in [("migration_1", 100), ("migration_2", 200)] {
            let parameters = parameters(id, timestamp).unwrap();
            let output = transaction
                .query_request(bounded_query(APPLY, &parameters, 1))
                .unwrap();
            assert_eq!(output.rows.len(), 1);
        }
        transaction.commit().unwrap();
        let batch_count_after_update = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        assert_eq!(batch_count_after_update, batch_count_before_update + 1);
    }
    let wal = read_test_wal(&path).unwrap();
    assert!(wal.contains("create_node"));
    {
        let mut db = Database::open(&path).unwrap();
        let rows = db
            .query_request(bounded_query("MATCH (m:SchemaMigrationLog) RETURN m.id AS id, m.applied_at AS applied_at ORDER BY id ASC", &BTreeMap::new(), 3))
            .unwrap();
        assert_eq!(rows.rows.len(), 2);
        assert_eq!(
            rows.rows[0].get("id"),
            Some(&Value::String("migration_1".to_string()))
        );
        assert_eq!(rows.rows[0].get("applied_at"), Some(&Value::Int(100)));
        assert_eq!(
            rows.rows[1].get("id"),
            Some(&Value::String("migration_2".to_string()))
        );
        assert_eq!(rows.rows[1].get("applied_at"), Some(&Value::Int(200)));
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn schema_migration_existing_only_preserves_epoch_and_wal() {
    let path = unique_test_dir("schema_migration_existing_only");
    {
        let mut db = Database::open(&path).unwrap();
        let parameters = parameters("existing", 10).unwrap();
        db.query_request(bounded_query(APPLY, &parameters, 1))
            .unwrap();
    }
    let wal_before = read_test_wal(&path).unwrap();
    {
        let mut db = Database::open(&path).unwrap();
        let before = db.commit_epoch().unwrap();
        let parameters = parameters("existing", 99).unwrap();
        let result = db
            .query_request(bounded_query(APPLY, &parameters, 1))
            .unwrap();
        assert_eq!(result.rows[0]["applied_at"], Value::Int(10));
        assert_eq!(db.commit_epoch().unwrap(), before);
    }
    assert_eq!(read_test_wal(&path).unwrap(), wal_before);
    std::fs::remove_dir_all(path).unwrap();
}

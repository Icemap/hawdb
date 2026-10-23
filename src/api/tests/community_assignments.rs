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

fn bounded_query<'a>(query: &'a str, parameters: &'a BTreeMap<String, Value>) -> QueryRequest<'a> {
    QueryRequest::new(query)
        .with_params(parameters)
        .with_output_limits(QueryStreamOptions {
            max_rows: Some(4),
            max_payload_bytes: Some(4096),
        })
}

#[test]
fn clears_community_assignments_through_scoped_queries() {
    let mut db = Database::new();
    db.query("CREATE (:Memory {id: 'memory_community_1', community_id: 7})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'entity_community_1', community_id: 8})")
        .unwrap();
    db.query("CREATE (:Source {id: 'source_community_1', community_id: 9})")
        .unwrap();
    db.query("CREATE (:Label {id: 'label_without_community'})")
        .unwrap();
    let parameters = BTreeMap::from([("community_id".into(), Value::Null)]);
    let mut transaction = db.begin_transaction().unwrap();
    for (query, id) in [
        ("MATCH (n:Entity) WHERE n.community_id IS NOT NULL SET n.community_id = $community_id RETURN n.id AS id", "entity_community_1"),
        ("MATCH (n:Memory) WHERE n.community_id IS NOT NULL SET n.community_id = $community_id RETURN n.id AS id", "memory_community_1"),
    ] {
        let output = transaction.query_request(bounded_query(query, &parameters)).unwrap();
        assert_eq!(output.rows.len(), 1);
        assert_eq!(output.rows[0]["id"], Value::String(id.into()));
    }
    transaction.commit().unwrap();
    assert_eq!(db.commit_epoch().unwrap(), 5);
    let rows = db
        .query_request(bounded_query(
            "MATCH (n) RETURN n.id AS id, n.community_id AS community_id ORDER BY n.id",
            &parameters,
        ))
        .unwrap()
        .rows;
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0]["community_id"], Value::Null);
    assert_eq!(rows[1]["community_id"], Value::Null);
    assert_eq!(rows[2]["community_id"], Value::Null);
    assert_eq!(rows[3]["community_id"], Value::Int(9));
    let output = db.query_request(bounded_query(
        "MATCH (n) WHERE n.community_id IS NOT NULL SET n.community_id = $community_id RETURN n.id AS id",
        &parameters,
    )).unwrap();
    assert_eq!(output.rows.len(), 1);
    assert_eq!(
        output.rows[0]["id"],
        Value::String("source_community_1".into())
    );
    assert_eq!(
        db.query_request(bounded_query(
            "MATCH (n) WHERE n.community_id IS NOT NULL RETURN count(n) AS total",
            &parameters,
        ))
        .unwrap()
        .rows[0]["total"],
        Value::Int(0)
    );
}

#[test]
fn community_assignment_clear_rejects_invalid_label_before_wal() {
    let path = unique_test_dir("community_assignment_clear_invalid_label");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Memory {id: 'memory_community_1', community_id: 7})")
            .unwrap();
        let epoch = db.commit_epoch().unwrap();
        let wal = read_test_wal(&path).unwrap();
        let parameters = BTreeMap::from([("community_id".into(), Value::Null)]);
        let error = db
            .query_request(bounded_query(
                "MATCH (n:Bad-Label) SET n.community_id = $community_id",
                &parameters,
            ))
            .unwrap_err();
        assert!(matches!(error, crate::HawDBError::Parse(_)), "{error}");
        assert_eq!(db.commit_epoch().unwrap(), epoch);
        assert_eq!(read_test_wal(&path).unwrap(), wal);
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn community_assignment_clear_persists_as_one_wal_batch_and_replays() {
    let path = unique_test_dir("community_assignment_clear_wal_replay");
    let parameters = BTreeMap::from([("community_id".into(), Value::Null)]);
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Memory {id: 'memory_community_1', community_id: 7})")
            .unwrap();
        db.query("CREATE (:Entity {id: 'entity_community_1', community_id: 8})")
            .unwrap();
        let batches = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        let output = db.query_request(bounded_query(
            "MATCH (n) WHERE n.community_id IS NOT NULL SET n.community_id = $community_id RETURN n.id AS id",
            &parameters,
        )).unwrap();
        assert_eq!(output.rows.len(), 2);
        let wal = read_test_wal(&path).unwrap();
        assert_eq!(wal.matches("\tbatch\t").count(), batches + 1);
        assert!(wal.contains("set_node_property"));
    }
    {
        let mut db = Database::open(&path).unwrap();
        let rows = db
            .query_request(bounded_query(
                "MATCH (n) RETURN n.community_id AS community_id ORDER BY n.id",
                &parameters,
            ))
            .unwrap()
            .rows;
        assert_eq!(rows.len(), 2);
        assert!(rows
            .iter()
            .all(|row| row.get("community_id") == Some(&Value::Null)));
    }
    std::fs::remove_dir_all(path).unwrap();
}

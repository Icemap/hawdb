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

fn bounded_query(query: &str) -> QueryRequest<'_> {
    QueryRequest::new(query).with_output_limits(QueryStreamOptions {
        max_rows: Some(4),
        max_payload_bytes: Some(4096),
    })
}

#[test]
fn deletes_communities_through_query_without_affecting_other_labels() {
    let mut db = Database::new();
    db.query("CREATE (:Community {id: 'community_1', name: 'One'})")
        .unwrap();
    db.query("CREATE (:Community {id: 'community_2', name: 'Two'})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'entity_1'})").unwrap();
    let candidates = db
        .query_request(bounded_query(
            "MATCH (c:Community) RETURN c.id AS id ORDER BY c.id",
        ))
        .unwrap();
    assert_eq!(candidates.rows.len(), 2);
    assert_eq!(
        candidates.rows[0]["id"],
        Value::String("community_1".into())
    );
    assert_eq!(
        candidates.rows[1]["id"],
        Value::String("community_2".into())
    );
    assert_eq!(db.commit_epoch().unwrap(), 3);
    let output = db
        .query_request(bounded_query("MATCH (c:Community) DELETE c"))
        .unwrap();
    assert_eq!(output.rows.len(), 2);
    assert_eq!(db.commit_epoch().unwrap(), 4);
    assert_eq!(
        db.query_request(bounded_query(
            "MATCH (c:Community) RETURN count(c) AS total",
        ))
        .unwrap()
        .rows[0]["total"],
        Value::Int(0)
    );
    assert_eq!(
        db.query_request(bounded_query("MATCH (e:Entity) RETURN count(e) AS total"))
            .unwrap()
            .rows[0]["total"],
        Value::Int(1)
    );
}

#[test]
fn detach_deletes_communities_and_incident_edges() {
    let mut db = Database::new();
    db.query("CREATE (:Entity {id: 'entity_1'})-[:BELONGS_TO]->(:Community {id: 'community_1'})")
        .unwrap();
    db.query("CREATE (:Community {id: 'community_2'})").unwrap();
    let output = db
        .query_request(bounded_query("MATCH (c:Community) DETACH DELETE c"))
        .unwrap();
    assert_eq!(output.rows.len(), 2);
    assert_eq!(
        db.query_request(bounded_query(
            "MATCH (c:Community) RETURN count(c) AS total",
        ))
        .unwrap()
        .rows[0]["total"],
        Value::Int(0)
    );
    assert_eq!(
        db.query_request(bounded_query("MATCH (e:Entity) RETURN count(e) AS total"))
            .unwrap()
            .rows[0]["total"],
        Value::Int(1)
    );
    assert_eq!(
        db.query_request(bounded_query(
            "MATCH (:Entity)-[r:BELONGS_TO]->(:Community) RETURN count(r) AS total",
        ))
        .unwrap()
        .rows[0]["total"],
        Value::Int(0)
    );
}

#[test]
fn community_cleanup_without_candidates_does_not_write_wal() {
    let path = unique_test_dir("community_cleanup_without_candidates");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Entity {id: 'entity_1'})").unwrap();
    }
    let wal = read_test_wal(&path).unwrap();
    {
        let mut db = Database::open(&path).unwrap();
        let epoch = db.commit_epoch().unwrap();
        let output = db
            .query_request(bounded_query("MATCH (c:Community) DETACH DELETE c"))
            .unwrap();
        assert!(output.rows.is_empty());
        assert_eq!(db.commit_epoch().unwrap(), epoch);
    }
    assert_eq!(read_test_wal(&path).unwrap(), wal);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn community_cleanup_persists_as_one_wal_batch_and_replays() {
    let path = unique_test_dir("community_cleanup_wal_replay");
    {
        let mut db = Database::open(&path).unwrap();
        db.query(
            "CREATE (:Entity {id: 'entity_1'})-[:BELONGS_TO]->(:Community {id: 'community_1'})",
        )
        .unwrap();
        db.query("CREATE (:Community {id: 'community_2'})").unwrap();
    }
    let batches = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
    {
        let mut db = Database::open(&path).unwrap();
        let output = db
            .query_request(bounded_query("MATCH (c:Community) DETACH DELETE c"))
            .unwrap();
        assert_eq!(output.rows.len(), 2);
    }
    let wal = read_test_wal(&path).unwrap();
    assert!(wal.contains("delete_node"));
    assert!(wal.contains("delete_rel"));
    assert_eq!(wal.matches("\tbatch\t").count(), batches + 1);
    {
        let mut db = Database::open(&path).unwrap();
        assert_eq!(
            db.query_request(bounded_query(
                "MATCH (c:Community) RETURN count(c) AS total",
            ))
            .unwrap()
            .rows[0]["total"],
            Value::Int(0)
        );
        assert_eq!(
            db.query_request(bounded_query("MATCH (e:Entity) RETURN count(e) AS total"))
                .unwrap()
                .rows[0]["total"],
            Value::Int(1)
        );
    }
    std::fs::remove_dir_all(path).unwrap();
}

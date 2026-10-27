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

const CREATE: &str = "MATCH (e:Entity {id: $entity_id}), (c:Community {id: $community_id}) \
    CREATE (e)-[:BELONGS_TO {strength: $strength, created_at: $created_at, properties: $properties}]->(c)";

const READ: &str =
    "MATCH (:Entity {id: $entity_id})-[r:BELONGS_TO]->(:Community {id: $community_id}) \
    RETURN r.strength AS strength, r.created_at AS created_at, r.properties AS properties";

fn parameters(
    entity_id: &str,
    community_id: &str,
    strength: f64,
    created_at: i64,
    properties: &str,
) -> Result<BTreeMap<String, Value>> {
    if !strength.is_finite() {
        return Err(HawDBError::Semantic("strength must be finite".into()));
    }
    if entity_id.is_empty() || community_id.is_empty() {
        return Err(HawDBError::Semantic(
            "non-empty endpoint id required".into(),
        ));
    }
    Ok(BTreeMap::from([
        ("entity_id".into(), Value::String(entity_id.into())),
        ("community_id".into(), Value::String(community_id.into())),
        ("strength".into(), Value::Float(strength)),
        ("created_at".into(), Value::Int(created_at)),
        ("properties".into(), Value::String(properties.into())),
    ]))
}

#[test]
fn creates_community_memberships_for_nowledge_entity_lifecycle() {
    let mut db = Database::new();
    db.query("CREATE (:Entity {id: 'entity_1', name: 'Entity One'})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'entity_2', name: 'Entity Two'})")
        .unwrap();
    db.query("CREATE (:Community {id: 'community_1', name: 'Community One'})")
        .unwrap();

    let before = db.commit_epoch().unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    let first = parameters("entity_1", "community_1", 0.7, 13, "{}").unwrap();
    let missing = parameters(
        "entity_2",
        "missing_community",
        0.9,
        14,
        "{\"source\":\"test\"}",
    )
    .unwrap();
    for (parameters, expected) in [(&first, 1), (&missing, 0)] {
        let before = transaction
            .query_request(bounded_query(READ, parameters))
            .unwrap();
        assert!(before.rows.is_empty());
        transaction
            .query_request(bounded_query(CREATE, parameters))
            .unwrap();
        let output = transaction
            .query_request(bounded_query(READ, parameters))
            .unwrap();
        assert_eq!(output.rows.len(), expected);
    }
    let endpoints = transaction
        .query_request(bounded_query(
            "MATCH (e:Entity {id: $entity_id})-[r:BELONGS_TO]->(c:Community) \
         RETURN id(e) AS entity_node_id, id(c) AS community_node_id",
            &first,
        ))
        .unwrap();
    assert_eq!(endpoints.rows.len(), 1);
    assert_eq!(endpoints.rows[0]["entity_node_id"], Value::Int(0));
    assert_eq!(endpoints.rows[0]["community_node_id"], Value::Int(2));
    transaction.commit().unwrap();
    assert_eq!(before, 3);
    assert_eq!(db.commit_epoch().unwrap(), before + 1);

    let relationships = db
        .query_request(bounded_query("MATCH (e:Entity {id: 'entity_1'})-[r:BELONGS_TO]->(c:Community) RETURN c.id, r.strength, r.created_at, r.properties", &BTreeMap::new()))
        .unwrap();
    assert_eq!(relationships.rows.len(), 1);
    assert_eq!(
        relationships.rows[0].get("c.id"),
        Some(&Value::String("community_1".to_string()))
    );
    assert_eq!(
        relationships.rows[0].get("r.strength"),
        Some(&Value::Float(0.7))
    );
    assert_eq!(
        relationships.rows[0].get("r.created_at"),
        Some(&Value::Int(13))
    );
    assert_eq!(
        relationships.rows[0].get("r.properties"),
        Some(&Value::String("{}".to_string()))
    );
    let nodes = db
        .query_request(bounded_query("MATCH (n) WHERE n.id IN ['entity_1', 'entity_2', 'community_1'] RETURN count(n) AS total", &BTreeMap::new()))
        .unwrap();
    assert_eq!(nodes.rows[0].get("total"), Some(&Value::Int(3)));
}

#[test]
fn community_membership_create_rejects_invalid_input_before_wal() {
    let path = unique_test_dir("community_membership_create_invalid_before_wal");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Entity {id: 'entity_1'})").unwrap();
        db.query("CREATE (:Community {id: 'community_1'})").unwrap();
    }
    let wal_before = read_test_wal(&path).unwrap();
    {
        let db = Database::open(&path).unwrap();
        let graph_commit_epoch_before = db.runtime.get().unwrap().store.commit_epoch();
        let error = parameters("entity_1", "community_1", f64::NAN, 13, "{}").unwrap_err();
        assert!(error.to_string().contains("strength must be finite"));
        assert_eq!(
            db.runtime.get().unwrap().store.commit_epoch(),
            graph_commit_epoch_before
        );
    }
    let wal_after = read_test_wal(&path).unwrap();
    assert_eq!(wal_after, wal_before);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn community_membership_queries_persist_as_one_wal_batch_and_replays() {
    let path = unique_test_dir("community_membership_query_wal_replay");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Entity {id: 'entity_1'})").unwrap();
        db.query("CREATE (:Entity {id: 'entity_2'})").unwrap();
        db.query("CREATE (:Community {id: 'community_1'})").unwrap();
    }
    let setup_wal = read_test_wal(&path).unwrap();
    let setup_batch_count = setup_wal.matches("\tbatch\t").count();
    {
        let mut db = Database::open(&path).unwrap();
        let mut transaction = db.begin_transaction().unwrap();
        for (entity, strength, timestamp) in [("entity_1", 0.6, 21), ("entity_2", 0.8, 22)] {
            let parameters = parameters(entity, "community_1", strength, timestamp, "{}").unwrap();
            transaction
                .query_request(bounded_query(CREATE, &parameters))
                .unwrap();
            let output = transaction
                .query_request(bounded_query(READ, &parameters))
                .unwrap();
            assert_eq!(output.rows.len(), 1);
            assert_eq!(output.rows[0]["strength"], Value::Float(strength));
            assert_eq!(output.rows[0]["created_at"], Value::Int(timestamp));
        }
        transaction.commit().unwrap();
    }
    let wal = read_test_wal(&path).unwrap();
    assert!(wal.contains("create_rel"));
    assert_eq!(wal.matches("\tbatch\t").count(), setup_batch_count + 1);
    {
        let mut db = Database::open(&path).unwrap();
        let relationships = db
            .query_request(bounded_query(
                "MATCH (:Entity)-[r:BELONGS_TO]->(:Community) RETURN count(r) AS total",
                &BTreeMap::new(),
            ))
            .unwrap();
        assert_eq!(relationships.rows[0].get("total"), Some(&Value::Int(2)));
        let nodes = db
            .query_request(bounded_query(
                "MATCH (n) RETURN count(n) AS total",
                &BTreeMap::new(),
            ))
            .unwrap();
        assert_eq!(nodes.rows[0].get("total"), Some(&Value::Int(3)));
    }
    std::fs::remove_dir_all(path).unwrap();
}

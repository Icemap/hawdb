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
use crate::{HawDBError, QueryRequest, QueryStreamOptions, Result};

const PAGERANK_MEMORY_UPDATE: &str = "MATCH (n:Memory) WHERE n.id = $id \
    SET n.pagerank_score = $score RETURN count(n) AS total";
const PAGERANK_ENTITY_UPDATE: &str = "MATCH (n:Entity) WHERE n.id = $id \
    SET n.pagerank_score = $score RETURN count(n) AS total";
const PAGERANK_CLEAR_QUERIES: [&str; 2] = [
    "MATCH (n:Memory) WHERE n.id IS NOT NULL AND n.pagerank_score IS NOT NULL \
     SET n.pagerank_score = null RETURN count(n) AS total",
    "MATCH (n:Entity) WHERE n.id IS NOT NULL AND n.pagerank_score IS NOT NULL \
     SET n.pagerank_score = null RETURN count(n) AS total",
];
const PAGERANK_SCORE_QUERIES: [&str; 2] = [
    "MATCH (n:Memory) WHERE n.id = $id RETURN n.pagerank_score AS score",
    "MATCH (n:Entity) WHERE n.id = $id RETURN n.pagerank_score AS score",
];
const PAGERANK_OUTPUT_LIMITS: QueryStreamOptions = QueryStreamOptions {
    max_rows: Some(1),
    max_payload_bytes: Some(128),
};

// Only host input normalization lives here. Matching, updates and counts remain
// in the ordinary parameterized statements executed by the tests below.
fn pagerank_parameters(
    updates: &[(&str, &str, f64)],
) -> Result<Vec<(&'static str, BTreeMap<String, Value>)>> {
    let mut seen = BTreeSet::new();
    let mut parameters = Vec::new();
    for &(label, id, score) in updates {
        let query = match label {
            "Memory" | "memory" => PAGERANK_MEMORY_UPDATE,
            "Entity" | "entity" => PAGERANK_ENTITY_UPDATE,
            _ => return Err(HawDBError::Semantic("unsupported PageRank label".into())),
        };
        if id.is_empty() {
            return Err(HawDBError::Semantic("empty PageRank external id".into()));
        }
        if !score.is_finite() || score < 0.0 {
            return Err(HawDBError::Semantic(
                "expected finite non-negative score".into(),
            ));
        }
        if seen.insert((query, id)) {
            parameters.push((
                query,
                BTreeMap::from([
                    ("id".into(), Value::String(id.into())),
                    ("score".into(), Value::Float(score)),
                ]),
            ));
        }
    }
    Ok(parameters)
}

#[test]
fn parameterized_pagerank_updates_and_clear_preserve_projected_entities() {
    let mut db = Database::new();
    db.query("CREATE (:Memory {id: 'memory_rank_1', title: 'Rank One'})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'memory_rank_2', title: 'Rank Two'})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'entity_rank_1', name: 'Entity One'})")
        .unwrap();
    db.query("CREATE (:Entity {name: 'Projected Entity', pagerank_score: 0.9})")
        .unwrap();
    let updates = [
        ("Memory", "memory_rank_1", 0.42),
        ("Entity", "entity_rank_1", 0.84),
        ("memory", "memory_rank_1", 0.99),
        ("Entity", "missing", 0.1),
    ];
    let parameters = pagerank_parameters(&updates).unwrap();
    assert_eq!(updates.len() - parameters.len(), 1);
    let before = db.begin_read_transaction().unwrap().commit_epoch();
    let mut tx = db.begin_transaction().unwrap();
    let mut matched = Vec::new();
    for (query, parameters) in &parameters {
        let output = tx
            .query_request(
                QueryRequest::new(query)
                    .with_params(parameters)
                    .with_output_limits(PAGERANK_OUTPUT_LIMITS),
            )
            .unwrap();
        matched.push(output.rows[0]["total"].clone());
    }
    assert_eq!(matched, [Value::Int(1), Value::Int(1), Value::Int(0)]);
    tx.commit().unwrap();
    assert_eq!(
        db.begin_read_transaction().unwrap().commit_epoch(),
        before + 1
    );
    for (query, id, expected) in [
        (
            PAGERANK_SCORE_QUERIES[0],
            "memory_rank_1",
            Value::Float(0.42),
        ),
        (
            PAGERANK_SCORE_QUERIES[1],
            "entity_rank_1",
            Value::Float(0.84),
        ),
        (PAGERANK_SCORE_QUERIES[0], "memory_rank_2", Value::Null),
    ] {
        let parameters = BTreeMap::from([("id".into(), Value::String(id.into()))]);
        let output = db
            .query_request(
                QueryRequest::new(query)
                    .with_params(&parameters)
                    .with_output_limits(PAGERANK_OUTPUT_LIMITS),
            )
            .unwrap();
        assert_eq!(output.rows[0]["score"], expected);
    }
    let mut tx = db.begin_transaction().unwrap();
    for query in PAGERANK_CLEAR_QUERIES {
        let output = tx
            .query_request(QueryRequest::new(query).with_output_limits(PAGERANK_OUTPUT_LIMITS))
            .unwrap();
        assert_eq!(output.rows[0]["total"], Value::Int(1));
    }
    tx.commit().unwrap();
    assert_eq!(
        db.begin_read_transaction().unwrap().commit_epoch(),
        before + 2
    );
    for (query, id) in PAGERANK_SCORE_QUERIES
        .into_iter()
        .zip(["memory_rank_1", "entity_rank_1"])
    {
        let parameters = BTreeMap::from([("id".into(), Value::String(id.into()))]);
        let output = db
            .query_request(
                QueryRequest::new(query)
                    .with_params(&parameters)
                    .with_output_limits(PAGERANK_OUTPUT_LIMITS),
            )
            .unwrap();
        assert_eq!(output.rows[0]["score"], Value::Null);
    }
    let remaining = db
        .query_request(
            QueryRequest::new(
                "MATCH (n:Entity) WHERE n.pagerank_score IS NOT NULL \
         RETURN n.id AS id, n.pagerank_score AS score",
            )
            .with_output_limits(PAGERANK_OUTPUT_LIMITS),
        )
        .unwrap();
    assert_eq!(remaining.rows.len(), 1);
    assert_eq!(remaining.rows[0]["id"], Value::Null);
    assert_eq!(remaining.rows[0]["score"], Value::Float(0.9));
}

#[test]
fn pagerank_input_normalization_rejects_invalid_batches_before_wal() {
    let path = unique_test_dir("pagerank_invalid_parameters");
    let mut db = Database::open(&path).unwrap();
    db.query("CREATE (:Memory {id: 'memory_rank_1'})").unwrap();
    let epoch = db.begin_read_transaction().unwrap().commit_epoch();
    let wal = read_test_wal(&path).unwrap();
    for invalid in [
        ("Memory", "memory_rank_1", f64::NAN),
        ("Memory", "memory_rank_1", f64::INFINITY),
        ("Memory", "memory_rank_1", -0.1),
        ("Memory", "", 0.1),
        ("Unknown", "memory_rank_1", 0.1),
    ] {
        let result = (|| -> Result<()> {
            let parameters = pagerank_parameters(&[("Memory", "memory_rank_1", 0.42), invalid])?;
            let mut tx = db.begin_transaction()?;
            for (query, parameters) in parameters {
                tx.query_request(
                    QueryRequest::new(query)
                        .with_params(&parameters)
                        .with_output_limits(PAGERANK_OUTPUT_LIMITS),
                )?;
            }
            tx.commit()?;
            Ok(())
        })();
        assert!(result.is_err(), "{invalid:?}");
        assert_eq!(db.begin_read_transaction().unwrap().commit_epoch(), epoch);
        assert_eq!(read_test_wal(&path).unwrap(), wal);
    }
    drop(db);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn parameterized_pagerank_batch_persists_as_one_wal_batch_and_replays() {
    let path = unique_test_dir("parameterized_pagerank_batch_wal_replay");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Memory {id: 'memory_rank_1', title: 'Rank One'})")
            .unwrap();
        db.query("CREATE (:Entity {id: 'entity_rank_1', name: 'Entity One'})")
            .unwrap();
        let batch_count_before = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        let parameters = pagerank_parameters(&[
            ("Memory", "memory_rank_1", 0.42),
            ("Entity", "entity_rank_1", 0.84),
        ])
        .unwrap();
        let mut tx = db.begin_transaction().unwrap();
        for (query, parameters) in parameters {
            let output = tx
                .query_request(
                    QueryRequest::new(query)
                        .with_params(&parameters)
                        .with_output_limits(PAGERANK_OUTPUT_LIMITS),
                )
                .unwrap();
            assert_eq!(output.rows[0]["total"], Value::Int(1));
        }
        tx.commit().unwrap();
        let batch_count_after = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        assert_eq!(batch_count_after, batch_count_before + 1);
    }
    let wal = read_test_wal(&path).unwrap();
    assert!(wal.contains("set_node_property"));
    {
        let mut db = Database::open(&path).unwrap();
        for (query, id, score) in [
            (PAGERANK_SCORE_QUERIES[0], "memory_rank_1", 0.42),
            (PAGERANK_SCORE_QUERIES[1], "entity_rank_1", 0.84),
        ] {
            let parameters = BTreeMap::from([("id".into(), Value::String(id.into()))]);
            let output = db
                .query_request(
                    QueryRequest::new(query)
                        .with_params(&parameters)
                        .with_output_limits(PAGERANK_OUTPUT_LIMITS),
                )
                .unwrap();
            assert_eq!(output.rows[0]["score"], Value::Float(score));
        }
    }
    std::fs::remove_dir_all(path).unwrap();
}

const PAGERANK_BASE_COUNT_QUERIES: [&str; 5] = [
    "MATCH (m:Memory) RETURN count(m) AS total",
    "MATCH (e:Entity) RETURN count(e) AS total",
    "MATCH (:Entity)-[r:RELATES_TO]->(:Entity) RETURN count(r) AS total",
    "MATCH (:Memory)-[r:MENTIONS]->(:Entity) RETURN count(r) AS total",
    "MATCH (:Memory)-[r:MEMORY_RELATES_TO]->(:Memory) WHERE r.status = 'active' RETURN count(r) AS total",
];

const PAGERANK_CHANGED_COUNT_QUERIES: [&str; 5] = [
    "MATCH (m:Memory) WHERE m.created_at > $cutoff OR m.updated_at > $cutoff RETURN count(m) AS total",
    "MATCH (e:Entity) WHERE e.created_at > $cutoff OR e.updated_at > $cutoff RETURN count(e) AS total",
    "MATCH (:Memory)-[r:MENTIONS]->(:Entity) WHERE r.created_at > $cutoff OR r.updated_at > $cutoff RETURN count(r) AS total",
    "MATCH (:Entity)-[r:RELATES_TO]->(:Entity) WHERE r.created_at > $cutoff OR r.updated_at > $cutoff RETURN count(r) AS total",
    "MATCH (:Memory)-[r:MEMORY_RELATES_TO]->(:Memory) WHERE r.status = 'active' AND (r.created_at > $cutoff OR r.updated_at > $cutoff) RETURN count(r) AS total",
];

const PAGERANK_ENTITY_MEMBERSHIP_QUERY: &str = "MATCH (e:Entity) WHERE e.id IN $external_ids \
     RETURN id(e) AS node_id, e.id AS external_id ORDER BY external_id";

const PAGERANK_MEMORY_VISIBILITY_QUERY: &str = "MATCH (m:Memory) WHERE m.id IN $memory_ids \
     RETURN id(m) AS node_id, m.id AS memory_id, m.metadata AS metadata, \
     COALESCE(m.is_latest, true) AS is_latest ORDER BY memory_id";

const PAGERANK_CENTRAL_ENTITY_QUERY: &str = "MATCH (e:Entity) WHERE e.id = $entity_id \
     RETURN id(e) AS node_id, e.name AS name LIMIT 1";

fn pagerank_count(
    read: &mut DatabaseReadTransaction,
    query: &str,
    parameters: &BTreeMap<String, Value>,
) -> i64 {
    let output = read
        .query_with_params_bounded(query, parameters, Some(1))
        .unwrap();
    let Some(Value::Int(total)) = output.rows[0].get("total") else {
        panic!("expected integer pagerank count");
    };
    *total
}

fn pagerank_counts(read: &mut DatabaseReadTransaction, cutoff: i64) -> Vec<i64> {
    let empty = BTreeMap::new();
    let changed = BTreeMap::from([("cutoff".to_string(), Value::Int(cutoff))]);
    let mut counts = Vec::with_capacity(10);
    for query in PAGERANK_BASE_COUNT_QUERIES {
        counts.push(pagerank_count(read, query, &empty));
    }
    for query in PAGERANK_CHANGED_COUNT_QUERIES {
        counts.push(pagerank_count(read, query, &changed));
    }
    counts
}

#[test]
fn pagerank_plan_reads_use_parameterized_queries_on_one_snapshot() {
    let mut db = Database::new();
    db.query(
        "CREATE (:Memory {id: 'm1', created_at: 10, updated_at: 20, metadata: '{\"visible\":true}'})",
    )
    .unwrap();
    db.query("CREATE (:Memory {id: 'm2', created_at: 120, updated_at: 130})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'e1', name: 'Entity One', created_at: 15, updated_at: 25})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'e2', name: 'Entity Two', created_at: 140, updated_at: 150})")
        .unwrap();
    db.query("MATCH (m:Memory {id: 'm1'}), (e:Entity {id: 'e1'}) CREATE (m)-[:MENTIONS {created_at: 30}]->(e)")
        .unwrap();
    db.query("MATCH (m:Memory {id: 'm2'}), (e:Entity {id: 'e2'}) CREATE (m)-[:MENTIONS {created_at: 160}]->(e)")
        .unwrap();
    db.query("MATCH (a:Entity {id: 'e1'}), (b:Entity {id: 'e2'}) CREATE (a)-[:RELATES_TO {created_at: 170}]->(b)")
        .unwrap();
    db.query("MATCH (a:Memory {id: 'm1'}), (b:Memory {id: 'm2'}) CREATE (a)-[:MEMORY_RELATES_TO {status: 'active', created_at: 180}]->(b)")
        .unwrap();
    db.query("MATCH (a:Memory {id: 'm2'}), (b:Memory {id: 'm1'}) CREATE (a)-[:MEMORY_RELATES_TO {status: 'inactive', created_at: 190}]->(b)")
        .unwrap();

    let mut read = db.begin_read_transaction().unwrap();
    let snapshot_epoch = read.commit_epoch();
    db.query("CREATE (:Memory {id: 'after-snapshot', created_at: 200})")
        .unwrap();

    assert_eq!(
        pagerank_counts(&mut read, 100),
        vec![2, 2, 1, 2, 1, 1, 1, 1, 1, 1]
    );
    assert_eq!(read.commit_epoch(), snapshot_epoch);
    assert!(db.commit_epoch().unwrap() > snapshot_epoch);
}

#[test]
fn pagerank_plan_queries_use_query_runtime_plan_cache() {
    let mut db = Database::new_with_config(DatabaseConfig {
        max_plan_cache_entries: Some(32),
        statement_summary_capacity: 32,
        ..DatabaseConfig::default()
    });
    db.query("CREATE (:Memory {id: 'pagerank-cache-memory-one', created_at: 10})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'pagerank-cache-memory-two', updated_at: 20})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'pagerank-cache-entity-one', created_at: 30})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'pagerank-cache-entity-two', updated_at: 40})")
        .unwrap();
    db.query("MATCH (m:Memory {id: 'pagerank-cache-memory-one'}), (e:Entity {id: 'pagerank-cache-entity-one'}) CREATE (m)-[:MENTIONS {created_at: 50}]->(e)")
        .unwrap();
    db.query("MATCH (a:Entity {id: 'pagerank-cache-entity-one'}), (b:Entity {id: 'pagerank-cache-entity-two'}) CREATE (a)-[:RELATES_TO {updated_at: 60}]->(b)")
        .unwrap();
    db.query("MATCH (a:Memory {id: 'pagerank-cache-memory-one'}), (b:Memory {id: 'pagerank-cache-memory-two'}) CREATE (a)-[:MEMORY_RELATES_TO {status: 'active', created_at: 70}]->(b)")
        .unwrap();
    let mut read = db.begin_read_transaction().unwrap();
    let first = pagerank_counts(&mut read, 25);
    let second = pagerank_counts(&mut read, 25);
    assert_eq!(first, second);
    assert_eq!(first[0], 2);
    assert_eq!(first[1], 2);
    assert_eq!(first[3], 1);
    assert_eq!(first[4], 1);
    assert_eq!(read_test_plan_cache_metric(&read, "entries"), 10);
    assert_eq!(read_test_plan_cache_metric(&read, "misses"), 10);
    assert_eq!(read_test_plan_cache_metric(&read, "hits"), 10);
}

#[test]
fn pagerank_lookup_business_logic_uses_parameterized_queries() {
    let mut db = Database::new();
    db.query("CREATE (:Memory {id: 'm1', metadata: '{\"space\":\"default\"}', is_latest: false})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'm2'})").unwrap();
    db.query("CREATE (:Entity {id: 'e1', name: 'Central Entity'})")
        .unwrap();
    let graph_commit_epoch = db.commit_epoch().unwrap();
    let mut read = db.begin_read_transaction().unwrap();
    let membership_parameters = BTreeMap::from([(
        "external_ids".to_string(),
        Value::List(vec![
            Value::String("e1".to_string()),
            Value::String("missing".to_string()),
        ]),
    )]);
    let membership = read
        .query_with_params_bounded(
            PAGERANK_ENTITY_MEMBERSHIP_QUERY,
            &membership_parameters,
            Some(2),
        )
        .unwrap();
    assert_eq!(read.commit_epoch(), graph_commit_epoch);
    assert_eq!(membership.rows.len(), 1);
    assert_eq!(
        membership.rows[0].get("external_id"),
        Some(&Value::String("e1".to_string()))
    );

    let visibility_parameters = BTreeMap::from([(
        "memory_ids".to_string(),
        Value::List(vec![
            Value::String("m1".to_string()),
            Value::String("m2".to_string()),
            Value::String("missing".to_string()),
        ]),
    )]);
    let visibility = read
        .query_with_params_bounded(
            PAGERANK_MEMORY_VISIBILITY_QUERY,
            &visibility_parameters,
            Some(3),
        )
        .unwrap();
    assert_eq!(visibility.rows.len(), 2);
    assert_eq!(
        visibility.rows[0].get("metadata"),
        Some(&Value::String("{\"space\":\"default\"}".to_string()))
    );
    assert_eq!(
        visibility.rows[0].get("is_latest"),
        Some(&Value::Bool(false))
    );
    assert_eq!(
        visibility.rows[1].get("is_latest"),
        Some(&Value::Bool(true))
    );

    let central_parameters =
        BTreeMap::from([("entity_id".to_string(), Value::String("e1".to_string()))]);
    let central = read
        .query_with_params_bounded(PAGERANK_CENTRAL_ENTITY_QUERY, &central_parameters, Some(1))
        .unwrap();
    assert_eq!(central.rows.len(), 1);
    assert_eq!(
        central.rows[0].get("name"),
        Some(&Value::String("Central Entity".to_string()))
    );
    assert_eq!(read.commit_epoch(), graph_commit_epoch);
}

#[test]
fn pagerank_lookup_queries_use_query_runtime_plan_cache() {
    let mut db = Database::new_with_config(DatabaseConfig {
        max_plan_cache_entries: Some(8),
        statement_summary_capacity: 8,
        ..DatabaseConfig::default()
    });
    db.query("CREATE (:Memory {id: 'pagerank-cache-memory', metadata: '{\"space\":\"default\"}', is_latest: false})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'pagerank-cache-entity', name: 'Cache Entity'})")
        .unwrap();

    let membership_parameters = BTreeMap::from([(
        "external_ids".to_string(),
        Value::List(vec![
            Value::String("pagerank-cache-entity".to_string()),
            Value::String("missing".to_string()),
        ]),
    )]);
    let visibility_parameters = BTreeMap::from([(
        "memory_ids".to_string(),
        Value::List(vec![
            Value::String("pagerank-cache-memory".to_string()),
            Value::String("missing".to_string()),
        ]),
    )]);
    let central_parameters = BTreeMap::from([(
        "entity_id".to_string(),
        Value::String("pagerank-cache-entity".to_string()),
    )]);
    let mut read = db.begin_read_transaction().unwrap();
    for _ in 0..2 {
        read.query_with_params_bounded(
            PAGERANK_ENTITY_MEMBERSHIP_QUERY,
            &membership_parameters,
            Some(2),
        )
        .unwrap();
        read.query_with_params_bounded(
            PAGERANK_MEMORY_VISIBILITY_QUERY,
            &visibility_parameters,
            Some(2),
        )
        .unwrap();
        read.query_with_params_bounded(PAGERANK_CENTRAL_ENTITY_QUERY, &central_parameters, Some(1))
            .unwrap();
    }

    assert_eq!(read_test_plan_cache_metric(&read, "entries"), 3);
    assert_eq!(read_test_plan_cache_metric(&read, "misses"), 3);
    assert_eq!(read_test_plan_cache_metric(&read, "hits"), 3);
}

#[test]
fn pagerank_read_queries_keep_user_values_in_parameters() {
    let mut db = Database::new();
    db.query("CREATE (:Entity {id: 'e1', name: 'Safe'})")
        .unwrap();
    let mut read = db.begin_read_transaction().unwrap();
    let parameters = BTreeMap::from([(
        "external_ids".to_string(),
        Value::List(vec![Value::String(
            "e1') MATCH (n) RETURN n //".to_string(),
        )]),
    )]);
    let output = read
        .query_with_params_bounded(PAGERANK_ENTITY_MEMBERSHIP_QUERY, &parameters, Some(1))
        .unwrap();
    assert!(output.rows.is_empty());

    let empty = BTreeMap::from([("external_ids".to_string(), Value::List(Vec::new()))]);
    let output = read
        .query_with_params_bounded(PAGERANK_ENTITY_MEMBERSHIP_QUERY, &empty, Some(1))
        .unwrap();
    assert!(output.rows.is_empty());
}

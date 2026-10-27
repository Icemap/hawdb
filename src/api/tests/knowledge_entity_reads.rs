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

const ENTITIES: &str = "MATCH (m:Memory) WHERE m.id IN $ids \
    RETURN m.id AS id, m AS entity ORDER BY m.id";
const PROPERTIES: &str = "MATCH (m:Memory) WHERE m.id IN $ids \
    RETURN m.id AS id, id(m) AS node_id, m.title AS title, \
    m.source_id AS source_id, m.metadata AS metadata ORDER BY m.id";
const SCOPED_ENTITIES: &str = "MATCH (m:Memory) WHERE m.id IN $ids \
    AND m.source_id = $source_id \
    AND (CASE WHEN m.space_id IS NULL OR m.space_id = '' \
         THEN 'default' ELSE m.space_id END) = $space_id \
    RETURN m.id AS id, m AS entity ORDER BY m.id";
const SCOPED_PROPERTIES: &str = "MATCH (m:Memory) WHERE m.id IN $ids \
    AND m.source_id = $source_id \
    AND (CASE WHEN m.space_id IS NULL OR m.space_id = '' \
         THEN 'default' ELSE m.space_id END) = $space_id \
    RETURN m.id AS id, id(m) AS node_id, m.title AS title, \
    m.source_id AS source_id, m.metadata AS metadata ORDER BY m.id";

fn parameters(ids: &[&str]) -> BTreeMap<String, Value> {
    BTreeMap::from([
        (
            "ids".into(),
            Value::List(ids.iter().map(|id| Value::String((*id).into())).collect()),
        ),
        ("source_id".into(), Value::String("thread_1".into())),
        ("space_id".into(), Value::String("default".into())),
    ])
}

// Only align already-selected query results with caller slots. Scope predicates,
// property selection and ordering remain in the statements above.
fn response_slots(ids: &[&str], rows: &crate::executor::QueryRows) -> Vec<Option<usize>> {
    ids.iter()
        .map(|id| {
            rows.iter()
                .position(|row| row.get("id") == Some(&Value::String((*id).into())))
        })
        .collect()
}

#[test]
fn retrieves_knowledge_entity_without_search_projection() {
    let mut db = Database::new();
    db.query("CREATE (:Entity {id: 'entity_1', name: 'HawDB', kind: 'database', score: 7})")
        .unwrap();
    let mut snapshot = db.begin_read_transaction().unwrap();
    let output = snapshot
        .query_request(bounded_batch_query(
            "MATCH (e:Entity {id: $id}) RETURN e AS entity",
            &BTreeMap::from([("id".into(), Value::String("entity_1".into()))]),
        ))
        .unwrap();
    assert_eq!(snapshot.commit_epoch(), 1);
    assert_eq!(output.rows.len(), 1);
    let Value::Map(entity) = &output.rows[0]["entity"] else {
        panic!("expected node map")
    };
    assert_eq!(entity["_id"], Value::Int(0));
    assert_eq!(
        entity["labels"],
        Value::List(vec![Value::String("Entity".into())])
    );
    assert_eq!(entity["id"], Value::String("entity_1".into()));
    assert_eq!(entity["name"], Value::String("HawDB".into()));
    assert_eq!(entity["score"], Value::Int(7));
}

#[test]
fn scoped_knowledge_entity_filters_by_metadata() {
    let mut db = Database::new();
    db.query(
        "CREATE (:Memory {id: 'memory_1', title: 'Scoped', source_id: 'thread_1', space_id: ''})",
    )
    .unwrap();
    let mut snapshot = db.begin_read_transaction().unwrap();
    let mut parameters = parameters(&["memory_1"]);
    let output = snapshot
        .query_request(bounded_batch_query(SCOPED_ENTITIES, &parameters))
        .unwrap();
    assert_eq!(snapshot.commit_epoch(), 1);
    assert_eq!(output.rows.len(), 1);
    assert_eq!(output.rows[0]["id"], Value::String("memory_1".into()));
    parameters.insert("source_id".into(), Value::String("thread_2".into()));
    assert!(snapshot
        .query_request(bounded_batch_query(SCOPED_ENTITIES, &parameters))
        .unwrap()
        .rows
        .is_empty());
}

#[test]
fn retrieves_knowledge_entity_batch_in_request_order() {
    let mut db = Database::new();
    db.query("CREATE (:Memory {id: 'memory_1', title: 'First'})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'memory_2', title: 'Second'})")
        .unwrap();
    let ids = ["memory_2", "missing", "memory_1"];
    let mut snapshot = db.begin_read_transaction().unwrap();
    let output = snapshot
        .query_request(bounded_batch_query(ENTITIES, &parameters(&ids)))
        .unwrap();
    assert_eq!(snapshot.commit_epoch(), 2);
    assert_eq!(output.rows.len(), 2);
    assert_eq!(
        response_slots(&ids, &output.rows),
        vec![Some(1), None, Some(0)]
    );
    assert_eq!(output.rows[0]["id"], Value::String("memory_1".into()));
    assert_eq!(output.rows[1]["id"], Value::String("memory_2".into()));
}

#[test]
fn knowledge_entity_batch_uses_query_runtime_plan_cache() {
    let mut db = Database::new_with_config(DatabaseConfig {
        max_plan_cache_entries: Some(8),
        statement_summary_capacity: 8,
        ..DatabaseConfig::default()
    });
    db.query("CREATE (:Memory {id: 'cache_memory_1', title: 'First'})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'cache_memory_2', title: 'Second'})")
        .unwrap();
    let ids = ["cache_memory_2", "cache_missing", "cache_memory_1"];
    let parameters = parameters(&ids);
    let mut snapshot = db.begin_read_transaction().unwrap();
    let first = snapshot
        .query_request(bounded_batch_query(ENTITIES, &parameters))
        .unwrap();
    let second = snapshot
        .query_request(bounded_batch_query(ENTITIES, &parameters))
        .unwrap();
    assert_eq!(first.rows, second.rows);
    assert_eq!(first.rows.len(), 2);
    assert_eq!(
        response_slots(&ids, &first.rows),
        vec![Some(1), None, Some(0)]
    );
    let stats = snapshot.plan_cache_stats();
    assert_eq!(stats.entries, 1);
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.hits, 1);
}

#[test]
fn scoped_knowledge_entity_batch_reports_filtered_and_missing_items() {
    let mut db = Database::new();
    db.query(
        "CREATE (:Memory {id: 'memory_1', title: 'First', source_id: 'thread_1', space_id: ''})",
    )
    .unwrap();
    db.query("CREATE (:Memory {id: 'memory_2', title: 'Second', source_id: 'thread_2', space_id: 'default'})").unwrap();
    let ids = ["memory_1", "memory_2", "missing"];
    let parameters = parameters(&ids);
    let mut snapshot =
        db.begin_read_transaction()
            .unwrap()
            .into_bounded(crate::ReadSnapshotBudget {
                max_rows: 3,
                max_payload_bytes: 4096,
            });
    let candidates = snapshot
        .query_request(bounded_batch_query(ENTITIES, &parameters))
        .unwrap();
    let visible = snapshot
        .query_request(bounded_batch_query(SCOPED_ENTITIES, &parameters))
        .unwrap();
    assert_eq!(snapshot.commit_epoch(), 2);
    assert_eq!(candidates.rows.len(), 2);
    assert_eq!(visible.rows.len(), 1);
    assert_eq!(
        response_slots(&ids, &candidates.rows),
        vec![Some(0), Some(1), None]
    );
    assert_eq!(
        response_slots(&ids, &visible.rows),
        vec![Some(0), None, None]
    );
    assert_eq!(visible.rows[0]["id"], Value::String("memory_1".into()));
    assert_eq!(snapshot.usage().output_rows, 3);
}

#[test]
fn scoped_knowledge_entity_batch_uses_query_runtime_plan_cache() {
    let mut db = Database::new_with_config(DatabaseConfig {
        max_plan_cache_entries: Some(8),
        statement_summary_capacity: 8,
        ..DatabaseConfig::default()
    });
    db.query("CREATE (:Memory {id: 'scoped_cache_memory_1', source_id: 'thread_1', space_id: ''})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'scoped_cache_memory_2', source_id: 'thread_2', space_id: 'default'})").unwrap();
    let ids = [
        "scoped_cache_memory_1",
        "scoped_cache_memory_2",
        "scoped_cache_missing",
    ];
    let parameters = parameters(&ids);
    let mut snapshot = db.begin_read_transaction().unwrap();
    for query in [ENTITIES, SCOPED_ENTITIES] {
        let first = snapshot
            .query_request(bounded_batch_query(query, &parameters))
            .unwrap();
        let second = snapshot
            .query_request(bounded_batch_query(query, &parameters))
            .unwrap();
        assert_eq!(first.rows, second.rows);
        assert_eq!(
            response_slots(&ids, &first.rows),
            if query == ENTITIES {
                vec![Some(0), Some(1), None]
            } else {
                vec![Some(0), None, None]
            }
        );
    }
    let stats = snapshot.plan_cache_stats();
    assert_eq!(stats.entries, 2);
    assert_eq!(stats.misses, 2);
    assert_eq!(stats.hits, 2);
}

#[test]
fn retrieves_knowledge_property_batch_without_hydrating_full_entities() {
    let mut db = Database::new();
    db.query("CREATE (:Memory {id: 'memory_1', title: 'First', source_id: 'thread_1', metadata: 'large metadata', content: 'unselected body'})").unwrap();
    db.query("CREATE (:Memory {id: 'memory_2', title: 'Second', source_id: 'thread_2'})")
        .unwrap();
    let ids = ["memory_2", "missing", "memory_1"];
    let mut snapshot = db.begin_read_transaction().unwrap();
    let output = snapshot
        .query_request(bounded_batch_query(PROPERTIES, &parameters(&ids)))
        .unwrap();
    assert_eq!(snapshot.commit_epoch(), 2);
    assert_eq!(output.rows.len(), 2);
    assert_eq!(
        response_slots(&ids, &output.rows),
        vec![Some(1), None, Some(0)]
    );
    assert_eq!(output.rows[1]["title"], Value::String("Second".into()));
    assert_eq!(output.rows[1]["metadata"], Value::Null);
    assert_eq!(
        output.rows[0]["metadata"],
        Value::String("large metadata".into())
    );
    for row in &output.rows {
        assert_eq!(row.len(), 5);
        assert!(row.get("entity").is_none());
        assert!(row.get("content").is_none());
    }
}

#[test]
fn knowledge_property_batch_uses_query_runtime_plan_cache() {
    let mut db = Database::new_with_config(DatabaseConfig {
        max_plan_cache_entries: Some(8),
        statement_summary_capacity: 8,
        ..DatabaseConfig::default()
    });
    db.query("CREATE (:Memory {id: 'property_cache_1', title: 'First', source_id: 'thread_1'})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'property_cache_2', title: 'Second', source_id: 'thread_2'})")
        .unwrap();
    let ids = [
        "property_cache_2",
        "property_cache_missing",
        "property_cache_1",
    ];
    let parameters = parameters(&ids);
    let mut snapshot = db.begin_read_transaction().unwrap();
    let first = snapshot
        .query_request(bounded_batch_query(PROPERTIES, &parameters))
        .unwrap();
    let second = snapshot
        .query_request(bounded_batch_query(PROPERTIES, &parameters))
        .unwrap();
    assert_eq!(first.rows, second.rows);
    assert_eq!(first.rows.len(), 2);
    assert_eq!(
        response_slots(&ids, &first.rows),
        vec![Some(1), None, Some(0)]
    );
    assert_eq!(first.rows[1]["title"], Value::String("Second".into()));
    let stats = snapshot.plan_cache_stats();
    assert_eq!(stats.entries, 1);
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.hits, 1);
}

#[test]
fn scoped_knowledge_property_batch_reports_filtered_rows() {
    let mut db = Database::new();
    db.query(
        "CREATE (:Memory {id: 'memory_1', title: 'First', source_id: 'thread_1', space_id: ''})",
    )
    .unwrap();
    db.query("CREATE (:Memory {id: 'memory_2', title: 'Second', source_id: 'thread_2', space_id: 'default'})").unwrap();
    let ids = ["memory_1", "memory_2"];
    let parameters = parameters(&ids);
    let mut snapshot =
        db.begin_read_transaction()
            .unwrap()
            .into_bounded(crate::ReadSnapshotBudget {
                max_rows: 3,
                max_payload_bytes: 4096,
            });
    let candidates = snapshot
        .query_request(bounded_batch_query(PROPERTIES, &parameters))
        .unwrap();
    let visible = snapshot
        .query_request(bounded_batch_query(SCOPED_PROPERTIES, &parameters))
        .unwrap();
    assert_eq!(snapshot.commit_epoch(), 2);
    assert_eq!(candidates.rows.len(), 2);
    assert_eq!(visible.rows.len(), 1);
    assert_eq!(
        response_slots(&ids, &candidates.rows),
        vec![Some(0), Some(1)]
    );
    assert_eq!(response_slots(&ids, &visible.rows), vec![Some(0), None]);
    assert_eq!(visible.rows[0]["title"], Value::String("First".into()));
    assert_eq!(
        visible.rows[0]["source_id"],
        Value::String("thread_1".into())
    );
    assert_eq!(snapshot.usage().output_rows, 3);
}

#[test]
fn scoped_knowledge_property_batch_uses_query_runtime_plan_cache() {
    let mut db = Database::new_with_config(DatabaseConfig {
        max_plan_cache_entries: Some(8),
        statement_summary_capacity: 8,
        ..DatabaseConfig::default()
    });
    db.query("CREATE (:Memory {id: 'scoped_property_cache_1', title: 'First', source_id: 'thread_1', space_id: ''})").unwrap();
    db.query("CREATE (:Memory {id: 'scoped_property_cache_2', title: 'Second', source_id: 'thread_2', space_id: 'default'})").unwrap();
    let ids = [
        "scoped_property_cache_1",
        "scoped_property_cache_2",
        "scoped_property_cache_missing",
    ];
    let parameters = parameters(&ids);
    let mut snapshot = db.begin_read_transaction().unwrap();
    for query in [PROPERTIES, SCOPED_PROPERTIES] {
        let first = snapshot
            .query_request(bounded_batch_query(query, &parameters))
            .unwrap();
        let second = snapshot
            .query_request(bounded_batch_query(query, &parameters))
            .unwrap();
        assert_eq!(first.rows, second.rows);
        assert_eq!(
            response_slots(&ids, &first.rows),
            if query == PROPERTIES {
                vec![Some(0), Some(1), None]
            } else {
                vec![Some(0), None, None]
            }
        );
        assert_eq!(first.rows[0]["title"], Value::String("First".into()));
    }
    let stats = snapshot.plan_cache_stats();
    assert_eq!(stats.entries, 2);
    assert_eq!(stats.misses, 2);
    assert_eq!(stats.hits, 2);
}

#[test]
fn entity_details_reads_entity_and_exact_neighbor_counts() {
    let mut db = Database::new();
    db.query("CREATE (:Memory {id: 'details-memory'})-[:MENTIONS]->(:Entity {id: 'details-entity', name: 'Entity'})").unwrap();
    let parameters = BTreeMap::from([("id".into(), Value::String("details-entity".into()))]);
    let mut snapshot =
        db.begin_read_transaction()
            .unwrap()
            .into_bounded(crate::ReadSnapshotBudget {
                max_rows: 3,
                max_payload_bytes: 4096,
            });
    let entity = snapshot
        .query_request(bounded_batch_query(
            "MATCH (e:Entity {id: $id}) RETURN e.id AS id",
            &parameters,
        ))
        .unwrap();
    assert_eq!(entity.rows.len(), 1);
    assert_eq!(entity.rows[0]["id"], parameters["id"]);
    for query in [
        "MATCH (e:Entity {id: $id}) OPTIONAL MATCH (e)-[r]-(n) RETURN COUNT(DISTINCT n) AS count",
        "MATCH (e:Entity {id: $id}) OPTIONAL MATCH (e)-[r]-() RETURN COUNT(r) AS count",
    ] {
        let output = snapshot
            .query_request(bounded_batch_query(query, &parameters))
            .unwrap();
        assert_eq!(output.rows.len(), 1);
        assert_eq!(output.rows[0]["count"], Value::Int(1));
    }
    assert_eq!(snapshot.usage().output_rows, 3);
}

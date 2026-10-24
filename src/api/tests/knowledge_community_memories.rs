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

const MENTIONED: &str = "MATCH (m:Memory)-[:MENTIONS]->(e:Entity) WHERE e.community_id IN $community_ids AND \
    (m.is_crystal = false OR ($include_null = true AND m.is_crystal IS NULL)) AND ($all_unit_types = true OR \
    m.unit_type IN $unit_types) WITH m, e ORDER BY e.id WITH m, e.community_id AS community_id, \
    count(DISTINCT e) AS mention_breadth, COLLECT(DISTINCT e.id) AS entity_ids RETURN community_id, \
    'mentioned_entities' AS source, \
    m.id AS id, id(m) AS node_id, m.title AS title, COALESCE(m.title, '') AS title_or_empty, \
    COALESCE(m.content, '') AS content_or_empty, m.unit_type AS unit_type, m.metadata AS metadata, \
    COALESCE(m.is_latest, true) AS is_latest, m.lifecycle_state AS lifecycle_state, m.importance AS \
    importance, m.created_at AS created_at, m.is_crystal AS is_crystal, m.pagerank_score AS \
    pagerank_score, mention_breadth, entity_ids ORDER BY community_id, mention_breadth DESC, \
    importance DESC, created_at DESC, id, node_id LIMIT $limit";
const RANKED: &str = "MATCH (m:Memory)-[:MENTIONS]->(e:Entity) WHERE e.community_id IN $community_ids AND \
    (m.is_crystal = false OR ($include_null = true AND m.is_crystal IS NULL)) AND ($all_unit_types = true OR \
    m.unit_type IN $unit_types) WITH m, e ORDER BY e.id WITH m, e.community_id AS community_id, \
    count(DISTINCT e) AS mention_breadth, COLLECT(DISTINCT e.id) AS entity_ids RETURN community_id, \
    'mentioned_entities' AS source, \
    m.id AS id, id(m) AS node_id, m.title AS title, COALESCE(m.title, '') AS title_or_empty, \
    COALESCE(m.content, '') AS content_or_empty, m.unit_type AS unit_type, m.metadata AS metadata, \
    COALESCE(m.is_latest, true) AS is_latest, m.lifecycle_state AS lifecycle_state, m.importance AS \
    importance, m.created_at AS created_at, m.is_crystal AS is_crystal, m.pagerank_score AS \
    pagerank_score, mention_breadth, entity_ids ORDER BY mention_breadth DESC, importance DESC, \
    pagerank_score DESC, id, community_id, node_id LIMIT $limit";

const DIRECT: &str = "MATCH (m:Memory) WHERE m.community_id IN $community_ids \
    AND (m.is_crystal = false OR ($include_null = true AND m.is_crystal IS NULL)) \
    AND ($all_unit_types = true OR m.unit_type IN $unit_types) \
    RETURN m.community_id AS community_id, 'direct_memory_community' AS source, \
    m.id AS id, id(m) AS node_id, \
    COALESCE(m.content, '') AS content_or_empty, COALESCE(m.is_latest, true) AS is_latest, \
    m.importance AS importance, m.created_at AS created_at, 0 AS mention_breadth, [] AS entity_ids \
    ORDER BY community_id, importance DESC, created_at DESC, id, node_id LIMIT $limit";

fn parameters(
    ids: Vec<Value>,
    include_null: bool,
    unit_types: &[&str],
) -> Result<BTreeMap<String, Value>> {
    let mut parameters = community_parameters(ids)?;
    if unit_types.contains(&"") {
        return Err(HawDBError::Semantic("non-empty unit types required".into()));
    }
    parameters.insert("include_null".into(), Value::Bool(include_null));
    parameters.insert("all_unit_types".into(), Value::Bool(unit_types.is_empty()));
    parameters.insert(
        "unit_types".into(),
        Value::List(
            unit_types
                .iter()
                .map(|unit| Value::String((*unit).into()))
                .collect(),
        ),
    );
    Ok(parameters)
}

#[test]
fn reads_community_memories_for_wiki_ranking_shapes() {
    let mut db = Database::new();
    db.query("CREATE (:Entity {id: 'entity_alpha', name: 'Alpha', community_id: 7})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'entity_beta', name: 'Beta', community_id: 7})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'entity_gamma', name: 'Gamma', community_id: 8})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'entity_other', name: 'Other', community_id: 9})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'memory_alpha', title: 'Alpha memory', content: 'Alpha content', unit_type: 'learning', metadata: '{\"rank\":1}', is_latest: true, lifecycle_state: 'active', importance: 0.8, created_at: 30, pagerank_score: 0.2, is_crystal: false})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'memory_beta', title: 'Beta memory', content: 'Beta content', metadata: '{\"rank\":2}', is_latest: false, lifecycle_state: 'archived', importance: 0.9, created_at: 20, pagerank_score: 0.9, is_crystal: false})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'memory_null_crystal', title: 'Gamma memory', content: 'Gamma content', importance: 0.7, created_at: 40})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'memory_crystal', title: 'Crystal memory', importance: 1.0, created_at: 50, is_crystal: true})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'direct_alpha', title: 'Direct Alpha', content: 'Direct content', community_id: 7, metadata: '{\"direct\":true}', is_crystal: false, importance: 0.6, created_at: 50})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'direct_null', title: 'Direct Null', community_id: 8, importance: 0.4, created_at: 10})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'direct_crystal', title: 'Direct Crystal', community_id: 7, is_crystal: true, importance: 1.0, created_at: 60})")
        .unwrap();
    db.query("MATCH (m:Memory {id: 'memory_alpha'}), (e:Entity {id: 'entity_alpha'}) CREATE (m)-[:MENTIONS]->(e)")
        .unwrap();
    db.query("MATCH (m:Memory {id: 'memory_alpha'}), (e:Entity {id: 'entity_beta'}) CREATE (m)-[:MENTIONS]->(e)")
        .unwrap();
    db.query("MATCH (m:Memory {id: 'memory_beta'}), (e:Entity {id: 'entity_alpha'}) CREATE (m)-[:MENTIONS]->(e)")
        .unwrap();
    db.query("MATCH (m:Memory {id: 'memory_null_crystal'}), (e:Entity {id: 'entity_gamma'}) CREATE (m)-[:MENTIONS]->(e)")
        .unwrap();
    db.query("MATCH (m:Memory {id: 'memory_crystal'}), (e:Entity {id: 'entity_alpha'}) CREATE (m)-[:MENTIONS]->(e)")
        .unwrap();
    db.query("MATCH (m:Memory {id: 'memory_alpha'}), (e:Entity {id: 'entity_other'}) CREATE (m)-[:MENTIONS]->(e)")
        .unwrap();

    let epoch = db.commit_epoch().unwrap();
    let mut snapshot =
        db.begin_read_transaction()
            .unwrap()
            .into_bounded(crate::ReadSnapshotBudget {
                max_rows: 5,
                max_payload_bytes: 8192,
            });
    let scope = parameters(vec![Value::Int(7), Value::Int(8)], true, &[]).unwrap();
    let mentioned = snapshot
        .query_request(bounded_batch_query(MENTIONED, &scope))
        .unwrap();
    assert_eq!(mentioned.rows.len(), 3);
    assert_eq!(
        mentioned.rows[0]["source"],
        Value::String("mentioned_entities".into())
    );
    for (index, id) in ["memory_alpha", "memory_beta", "memory_null_crystal"]
        .iter()
        .enumerate()
    {
        assert_eq!(mentioned.rows[index]["id"], Value::String((*id).into()));
    }
    for (key, value) in [
        ("community_id", Value::Int(7)),
        ("mention_breadth", Value::Int(2)),
        (
            "entity_ids",
            Value::List(vec![
                Value::String("entity_alpha".into()),
                Value::String("entity_beta".into()),
            ]),
        ),
        ("title_or_empty", Value::String("Alpha memory".into())),
        ("content_or_empty", Value::String("Alpha content".into())),
        ("unit_type", Value::String("learning".into())),
        ("metadata", Value::String("{\"rank\":1}".into())),
        ("is_latest", Value::Bool(true)),
        ("lifecycle_state", Value::String("active".into())),
        ("importance", Value::Float(0.8)),
        ("created_at", Value::Int(30)),
        ("is_crystal", Value::Bool(false)),
        ("pagerank_score", Value::Float(0.2)),
    ] {
        assert_eq!(mentioned.rows[0][key], value);
    }
    assert_eq!(mentioned.rows[1]["mention_breadth"], Value::Int(1));
    assert_eq!(mentioned.rows[1]["is_latest"], Value::Bool(false));
    assert_eq!(mentioned.rows[2]["community_id"], Value::Int(8));
    assert_eq!(mentioned.rows[2]["is_crystal"], Value::Null);
    let false_only = parameters(vec![Value::Int(8)], false, &[]).unwrap();
    assert!(snapshot
        .query_request(bounded_batch_query(RANKED, &false_only))
        .unwrap()
        .rows
        .is_empty());
    let direct = snapshot
        .query_request(bounded_batch_query(DIRECT, &scope))
        .unwrap();
    assert_eq!(direct.rows.len(), 2);
    assert_eq!(
        direct.rows[0]["source"],
        Value::String("direct_memory_community".into())
    );
    assert_eq!(direct.rows[0]["id"], Value::String("direct_alpha".into()));
    assert_eq!(direct.rows[1]["id"], Value::String("direct_null".into()));
    assert_eq!(direct.rows[0]["mention_breadth"], Value::Int(0));
    assert_eq!(direct.rows[0]["entity_ids"], Value::List(vec![]));
    assert_eq!(
        direct.rows[0]["content_or_empty"],
        Value::String("Direct content".into())
    );
    assert_eq!(direct.rows[1]["community_id"], Value::Int(8));
    assert_eq!(direct.rows[1]["is_latest"], Value::Bool(true));
    assert_eq!(snapshot.commit_epoch(), epoch);
    assert_eq!(db.commit_epoch().unwrap(), epoch);
    assert_eq!(snapshot.usage().output_rows, 5);
}

#[test]
fn reads_community_memories_for_unit_type_filter_shape() {
    let mut db = Database::new();
    db.query("CREATE (:Entity {id: 'type_entity_a', community_id: 17})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'type_entity_b', community_id: 18})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'type_memory_fact', title: 'Fact Memory', unit_type: 'fact', is_crystal: false})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'type_memory_note', title: 'Note Memory', unit_type: 'note', is_crystal: false})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'type_memory_decision', title: 'Decision Memory', unit_type: 'decision', is_crystal: false})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'type_memory_crystal', title: 'Crystal Memory', unit_type: 'fact', is_crystal: true})")
        .unwrap();
    db.query("MATCH (m:Memory {id: 'type_memory_fact'}), (e:Entity {id: 'type_entity_a'}) CREATE (m)-[:MENTIONS]->(e)")
        .unwrap();
    db.query("MATCH (m:Memory {id: 'type_memory_note'}), (e:Entity {id: 'type_entity_a'}) CREATE (m)-[:MENTIONS]->(e)")
        .unwrap();
    db.query("MATCH (m:Memory {id: 'type_memory_decision'}), (e:Entity {id: 'type_entity_a'}) CREATE (m)-[:MENTIONS]->(e)")
        .unwrap();
    db.query("MATCH (m:Memory {id: 'type_memory_crystal'}), (e:Entity {id: 'type_entity_a'}) CREATE (m)-[:MENTIONS]->(e)")
        .unwrap();
    db.query("MATCH (m:Memory {id: 'type_memory_note'}), (e:Entity {id: 'type_entity_b'}) CREATE (m)-[:MENTIONS]->(e)")
        .unwrap();

    let epoch = db.commit_epoch().unwrap();
    let parameters = parameters(vec![Value::Int(17)], false, &["fact", "note"]).unwrap();
    let mut snapshot = db.begin_read_transaction().unwrap();
    let output = snapshot
        .query_request(bounded_batch_query(RANKED, &parameters))
        .unwrap();
    assert_eq!(snapshot.commit_epoch(), epoch);
    assert_eq!(db.commit_epoch().unwrap(), epoch);
    assert_eq!(output.rows.len(), 2);
    for (index, id, title, unit) in [
        (0, "type_memory_fact", "Fact Memory", "fact"),
        (1, "type_memory_note", "Note Memory", "note"),
    ] {
        assert_eq!(output.rows[index]["id"], Value::String(id.into()));
        assert_eq!(output.rows[index]["title"], Value::String(title.into()));
        assert_eq!(output.rows[index]["unit_type"], Value::String(unit.into()));
    }
}

#[test]
fn community_memory_read_rejects_invalid_scope() {
    for (ids, units, message) in [
        (vec![], vec![], "non-empty community ids"),
        (vec![Value::Null], vec![], "non-null community ids"),
        (vec![Value::Int(1)], vec![""], "non-empty unit types"),
    ] {
        assert!(parameters(ids, false, &units)
            .unwrap_err()
            .to_string()
            .contains(message));
    }
}

#[test]
fn community_memory_reads_use_query_runtime_plan_cache() {
    let mut db = Database::new_with_config(DatabaseConfig {
        max_plan_cache_entries: Some(8),
        statement_summary_capacity: 8,
        ..DatabaseConfig::default()
    });
    db.query("CREATE (:Entity {id: 'community-cache-entity', community_id: 21})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'community-cache-mentioned', title: 'Mentioned', unit_type: 'fact', is_crystal: false, importance: 0.7})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'community-cache-direct', title: 'Direct', unit_type: 'fact', community_id: 21, is_crystal: false, importance: 0.6})")
        .unwrap();
    db.query("MATCH (m:Memory {id: 'community-cache-mentioned'}), (e:Entity {id: 'community-cache-entity'}) CREATE (m)-[:MENTIONS]->(e)")
        .unwrap();

    let parameters = parameters(vec![Value::Int(21)], false, &["fact"]).unwrap();
    let mut snapshot = db.begin_read_transaction().unwrap();
    for (query, id) in [
        (MENTIONED, "community-cache-mentioned"),
        (DIRECT, "community-cache-direct"),
    ] {
        let first = snapshot
            .query_request(bounded_batch_query(query, &parameters))
            .unwrap();
        let second = snapshot
            .query_request(bounded_batch_query(query, &parameters))
            .unwrap();
        assert_eq!(first.rows, second.rows);
        assert_eq!(first.rows.len(), 1);
        assert_eq!(first.rows[0]["id"], Value::String(id.into()));
    }
    let stats = snapshot.plan_cache_stats();
    assert_eq!(stats.entries, 2);
    assert_eq!(stats.misses, 2);
    assert_eq!(stats.hits, 2);
}

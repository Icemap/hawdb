// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;

const KEY_COUNT: &str = "MATCH (m:Memory) WHERE m.is_crystal = true \
    AND m.id CONTAINS $key RETURN count(*) AS count";
const KEY_DETAIL: &str = "MATCH (m:Memory) WHERE m.is_crystal = true AND m.id CONTAINS $key \
    WITH m, CASE WHEN m.id = $key THEN 0 WHEN left(m.id, $key_chars) = $key THEN 1 ELSE 2 END AS rank \
    RETURN m.id AS id, m.crystal_title AS crystal_title, m.title AS title, \
    COALESCE(m.crystal_title, m.title, '') AS display_title, m.content AS content, \
    m.importance AS importance, m.unit_type AS unit_type, m.created_at AS created_at \
    ORDER BY rank, m.id, id(m) LIMIT $limit";
const KEY_RANKED: &str = "MATCH (m:Memory) WHERE m.is_crystal = true AND m.id CONTAINS $key \
    WITH m, CASE WHEN m.id = $key THEN 0 WHEN left(m.id, $key_chars) = $key THEN 1 ELSE 2 END AS rank \
    RETURN m.id AS id, m.importance AS importance, m.created_at AS created_at \
    ORDER BY rank, importance DESC, created_at DESC, id, id(m) LIMIT $limit";
const AFTER_COUNT: &str = "MATCH (m:Memory) WHERE m.is_crystal = true AND m.id > $after \
    RETURN count(*) AS count";
const AFTER_PAGE: &str = "MATCH (m:Memory) WHERE m.is_crystal = true AND m.id > $after \
    RETURN m.id AS id, COALESCE(m.crystal_title, m.title, '') AS display_title, \
    m.is_crystal AS is_crystal, m.is_latest AS is_latest, m.metadata AS metadata \
    ORDER BY id, id(m) LIMIT $limit";
const COMMUNITY_ROWS: &str =
    "MATCH (c:Memory)-[:SYNTHESIZED_FROM]->(s:Memory)-[:MENTIONS]->(e:Entity) \
    WHERE c.is_crystal = true AND e.community_id IN $community_ids \
    WITH c.id AS crystal_memory_id, id(c) AS crystal_node_id, \
    c.crystal_title AS crystal_title, c.title AS title, c.content AS content, \
    c.importance AS importance, c.metadata AS metadata, c.is_latest AS is_latest, \
    c.lifecycle_state AS lifecycle_state, e.community_id AS community_id, \
    count(*) AS hit_count, count(DISTINCT s) AS source_memory_count \
    RETURN crystal_memory_id, crystal_node_id, community_id, hit_count, \
    source_memory_count, crystal_title, title, content, importance, metadata, \
    is_latest, lifecycle_state, COALESCE(crystal_title, title, '') AS display_title \
    ORDER BY hit_count DESC, importance DESC, community_id, crystal_memory_id, crystal_node_id \
    LIMIT $limit";
const VISIBILITY_ROWS: &str = "MATCH (c:Memory)-[:SYNTHESIZED_FROM]->(s:Memory)-[:MENTIONS]->(e:Entity) \
    WHERE c.is_crystal = true AND e.community_id IN $community_ids \
    RETURN c.id AS crystal_memory_id, s.id AS source_memory_id, e.id AS entity_id, \
    e.community_id AS community_id, c.crystal_title AS crystal_title, \
    COALESCE(c.crystal_title, c.title, '') AS display_title, c.content AS content, \
    c.importance AS importance, c.metadata AS crystal_metadata, \
    COALESCE(c.is_latest, true) AS crystal_is_latest, c.lifecycle_state AS crystal_lifecycle_state, \
    s.metadata AS source_metadata, COALESCE(s.is_latest, true) AS source_is_latest, \
    s.lifecycle_state AS source_lifecycle_state \
    ORDER BY community_id, crystal_memory_id, source_memory_id, entity_id, id(c), id(s), id(e) \
    LIMIT $limit";

fn filter_parameters(
    key: Option<&str>,
    after: Option<&str>,
    limit: i64,
) -> Result<BTreeMap<String, Value>> {
    if key == Some("") {
        return Err(HawDBError::Semantic("non-empty key match required".into()));
    }
    if after == Some("") {
        return Err(HawDBError::Semantic("non-empty after id required".into()));
    }
    if key.is_some() && after.is_some() {
        return Err(HawDBError::Semantic("choose key_match or after_id".into()));
    }
    let mut parameters = BTreeMap::from([("limit".into(), Value::Int(limit))]);
    if let Some(key) = key {
        parameters.insert("key".into(), Value::String(key.into()));
        parameters.insert(
            "key_chars".into(),
            Value::Int(i64::try_from(key.chars().count()).unwrap()),
        );
    }
    if let Some(after) = after {
        parameters.insert("after".into(), Value::String(after.into()));
    }
    Ok(parameters)
}

#[test]
fn reads_crystals_for_wiki_and_okf_shapes() {
    let mut db = Database::new();
    db.query("CREATE (:Memory {id: 'crystal-alpha', is_crystal: true, crystal_title: 'Alpha Crystal', title: 'Alpha Title', content: 'Alpha content', importance: 0.8, unit_type: 'fact', created_at: 10, updated_at: 20, metadata: '{\"a\":1}', is_latest: true})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'crystal-beta', is_crystal: true, title: 'Beta Title', content: 'Beta content', importance: 0.9, unit_type: 'decision', created_at: 5, updated_at: 25, metadata: '{\"b\":1}', is_latest: false})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'archive-crystal', is_crystal: true, crystal_title: 'Archive Crystal', content: 'Archive content', importance: 0.1, unit_type: 'note', created_at: 30})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'crystal-non', is_crystal: false, crystal_title: 'Non Crystal', importance: 5.0, created_at: 99})")
        .unwrap();

    let epoch = db.commit_epoch().unwrap();
    let mut snapshot =
        db.begin_read_transaction()
            .unwrap()
            .into_bounded(crate::ReadSnapshotBudget {
                max_rows: 4,
                max_payload_bytes: 4096,
            });
    let parameters = filter_parameters(Some("crystal-a"), None, 1).unwrap();
    let count = snapshot
        .query_request(bounded_batch_query(KEY_COUNT, &parameters))
        .unwrap();
    assert_eq!(count.rows[0]["count"], Value::Int(1));
    let detail = snapshot
        .query_request(bounded_batch_query(KEY_DETAIL, &parameters))
        .unwrap();
    assert_eq!(detail.rows.len(), 1);
    for (key, value) in [
        ("id", Value::String("crystal-alpha".into())),
        ("crystal_title", Value::String("Alpha Crystal".into())),
        ("title", Value::String("Alpha Title".into())),
        ("display_title", Value::String("Alpha Crystal".into())),
        ("content", Value::String("Alpha content".into())),
        ("importance", Value::Float(0.8)),
        ("unit_type", Value::String("fact".into())),
        ("created_at", Value::Int(10)),
    ] {
        assert_eq!(detail.rows[0][key], value);
    }
    let parameters = filter_parameters(None, Some("crystal-alpha"), 10).unwrap();
    let count = snapshot
        .query_request(bounded_batch_query(AFTER_COUNT, &parameters))
        .unwrap();
    assert_eq!(count.rows[0]["count"], Value::Int(1));
    let page = snapshot
        .query_request(bounded_batch_query(AFTER_PAGE, &parameters))
        .unwrap();
    assert_eq!(page.rows.len(), 1);
    for (key, value) in [
        ("id", Value::String("crystal-beta".into())),
        ("display_title", Value::String("Beta Title".into())),
        ("is_crystal", Value::Bool(true)),
        ("is_latest", Value::Bool(false)),
        ("metadata", Value::String("{\"b\":1}".into())),
    ] {
        assert_eq!(page.rows[0][key], value);
    }
    assert_eq!(snapshot.commit_epoch(), epoch);
    assert_eq!(db.commit_epoch().unwrap(), epoch);
    assert_eq!(snapshot.usage().output_rows, 4);
}

#[test]
fn crystal_read_rejects_invalid_filters() {
    for (key, after, message) in [
        (Some(""), None, "non-empty key match"),
        (None, Some(""), "non-empty after id"),
        (
            Some("crystal"),
            Some("crystal-alpha"),
            "key_match or after_id",
        ),
    ] {
        assert!(filter_parameters(key, after, 1)
            .unwrap_err()
            .to_string()
            .contains(message));
    }
}

#[test]
fn crystal_reads_use_query_runtime_plan_cache() {
    let mut db = Database::new_with_config(DatabaseConfig {
        max_plan_cache_entries: Some(8),
        statement_summary_capacity: 8,
        ..DatabaseConfig::default()
    });
    db.query("CREATE (:Memory {id: 'crystal-cache-alpha', is_crystal: true, crystal_title: 'Alpha', importance: 0.9, created_at: 20})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'crystal-cache-beta', is_crystal: true, crystal_title: 'Beta', importance: 0.7, created_at: 30})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'crystal-cache-skip', is_crystal: false, crystal_title: 'Skip', importance: 9.0})")
        .unwrap();

    let parameters = filter_parameters(Some("crystal-cache"), None, 1).unwrap();
    let mut snapshot = db.begin_read_transaction().unwrap();
    for query in [KEY_COUNT, KEY_RANKED] {
        let first = snapshot
            .query_request(bounded_batch_query(query, &parameters))
            .unwrap();
        let second = snapshot
            .query_request(bounded_batch_query(query, &parameters))
            .unwrap();
        assert_eq!(first.rows, second.rows);
        assert_eq!(first.rows.len(), 1);
        if query == KEY_COUNT {
            assert_eq!(first.rows[0]["count"], Value::Int(2));
        } else {
            assert_eq!(
                first.rows[0]["id"],
                Value::String("crystal-cache-alpha".into())
            );
        }
    }
    let stats = snapshot.plan_cache_stats();
    assert_eq!(stats.entries, 2);
    assert_eq!(stats.misses, 2);
    assert_eq!(stats.hits, 2);
}

#[test]
fn crystal_key_ranking_preserves_exact_prefix_and_contains_order() {
    let mut db = Database::new();
    for (id, importance, is_crystal) in [
        ("é", 1, true),
        ("é-prefix", 2, true),
        ("a-é", 3, true),
        ("é-plain", 99, false),
    ] {
        let parameters = BTreeMap::from([
            ("id".into(), Value::String(id.into())),
            ("importance".into(), Value::Int(importance)),
            ("is_crystal".into(), Value::Bool(is_crystal)),
        ]);
        db.query_request(bounded_batch_query(
            "CREATE (:Memory {id: $id, importance: $importance, is_crystal: $is_crystal})",
            &parameters,
        ))
        .unwrap();
    }
    let parameters = filter_parameters(Some("é"), None, 3).unwrap();
    let mut snapshot =
        db.begin_read_transaction()
            .unwrap()
            .into_bounded(crate::ReadSnapshotBudget {
                max_rows: 6,
                max_payload_bytes: 4096,
            });
    for query in [KEY_DETAIL, KEY_RANKED] {
        let output = snapshot
            .query_request(bounded_batch_query(query, &parameters))
            .unwrap();
        assert_eq!(output.rows.len(), 3);
        for (row, expected) in output.rows.iter().zip(["é", "é-prefix", "a-é"]) {
            assert_eq!(row["id"], Value::String(expected.into()));
        }
    }
    assert_eq!(snapshot.usage().output_rows, 6);
}

#[test]
fn reads_crystal_communities_for_topic_ranking_and_okf_mapping() {
    let mut db = Database::new();
    db.query("CREATE (:Memory {id: 'crystal-alpha', is_crystal: true, crystal_title: 'Alpha Crystal', title: 'Alpha Title', content: 'Alpha content', importance: 0.8, metadata: '{\"c\":1}', is_latest: true, lifecycle_state: 'active'})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'crystal-beta', is_crystal: true, title: 'Beta Title', content: 'Beta content', importance: 0.9, is_latest: false})")
        .unwrap();
    db.query(
        "CREATE (:Memory {id: 'plain-memory', is_crystal: false, title: 'Plain', importance: 9.0})",
    )
    .unwrap();
    db.query("CREATE (:Memory {id: 'source-one', metadata: '{\"s\":1}', is_latest: true, lifecycle_state: 'ready'})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'source-two', metadata: '{\"s\":2}', is_latest: false, lifecycle_state: 'archived'})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'source-three'})").unwrap();
    db.query("CREATE (:Entity {id: 'entity-one', name: 'One', community_id: 7})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'entity-two', name: 'Two', community_id: 7})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'entity-three', name: 'Three', community_id: 8})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'entity-null', name: 'Null Entity'})")
        .unwrap();
    db.query(
        "MATCH (c:Memory {id: 'crystal-alpha'}), (s:Memory {id: 'source-one'}) CREATE (c)-[:SYNTHESIZED_FROM]->(s)",
    )
    .unwrap();
    db.query(
        "MATCH (c:Memory {id: 'crystal-alpha'}), (s:Memory {id: 'source-two'}) CREATE (c)-[:SYNTHESIZED_FROM]->(s)",
    )
    .unwrap();
    db.query(
        "MATCH (c:Memory {id: 'crystal-beta'}), (s:Memory {id: 'source-three'}) CREATE (c)-[:SYNTHESIZED_FROM]->(s)",
    )
    .unwrap();
    db.query(
        "MATCH (c:Memory {id: 'plain-memory'}), (s:Memory {id: 'source-one'}) CREATE (c)-[:SYNTHESIZED_FROM]->(s)",
    )
    .unwrap();
    db.query(
        "MATCH (s:Memory {id: 'source-one'}), (e:Entity {id: 'entity-one'}) CREATE (s)-[:MENTIONS]->(e)",
    )
    .unwrap();
    db.query(
        "MATCH (s:Memory {id: 'source-one'}), (e:Entity {id: 'entity-two'}) CREATE (s)-[:MENTIONS]->(e)",
    )
    .unwrap();
    db.query(
        "MATCH (s:Memory {id: 'source-two'}), (e:Entity {id: 'entity-one'}) CREATE (s)-[:MENTIONS]->(e)",
    )
    .unwrap();
    db.query(
        "MATCH (s:Memory {id: 'source-two'}), (e:Entity {id: 'entity-three'}) CREATE (s)-[:MENTIONS]->(e)",
    )
    .unwrap();
    db.query(
        "MATCH (s:Memory {id: 'source-two'}), (e:Entity {id: 'entity-null'}) CREATE (s)-[:MENTIONS]->(e)",
    )
    .unwrap();
    db.query(
        "MATCH (s:Memory {id: 'source-three'}), (e:Entity {id: 'entity-three'}) CREATE (s)-[:MENTIONS]->(e)",
    )
    .unwrap();

    let epoch = db.commit_epoch().unwrap();
    let parameters = community_parameters(vec![Value::Int(7)]).unwrap();
    let mut snapshot = db.begin_read_transaction().unwrap();
    let topic = snapshot
        .query_request(bounded_batch_query(COMMUNITY_ROWS, &parameters))
        .unwrap();
    assert_eq!(snapshot.commit_epoch(), epoch);
    assert_eq!(db.commit_epoch().unwrap(), epoch);
    assert_eq!(topic.rows.len(), 1);
    for (key, value) in [
        ("crystal_memory_id", Value::String("crystal-alpha".into())),
        ("community_id", Value::Int(7)),
        ("hit_count", Value::Int(3)),
        ("source_memory_count", Value::Int(2)),
        ("crystal_title", Value::String("Alpha Crystal".into())),
        ("display_title", Value::String("Alpha Crystal".into())),
        ("content", Value::String("Alpha content".into())),
        ("importance", Value::Float(0.8)),
        ("metadata", Value::String("{\"c\":1}".into())),
        ("is_latest", Value::Bool(true)),
        ("lifecycle_state", Value::String("active".into())),
    ] {
        assert_eq!(topic.rows[0][key], value);
    }
}

#[test]
fn crystal_community_read_rejects_invalid_scope() {
    for (ids, message) in [
        (vec![], "non-empty community ids"),
        (vec![Value::Null], "non-null community ids"),
    ] {
        assert!(community_parameters(ids)
            .unwrap_err()
            .to_string()
            .contains(message));
    }
}

#[test]
fn crystal_source_visibility_rejects_invalid_scope() {
    for (ids, message) in [
        (vec![], "non-empty community ids"),
        (vec![Value::Null], "non-null community ids"),
    ] {
        assert!(community_parameters(ids)
            .unwrap_err()
            .to_string()
            .contains(message));
    }
}

#[test]
fn crystal_community_reads_use_query_runtime_plan_cache() {
    let mut db = Database::new_with_config(DatabaseConfig {
        max_plan_cache_entries: Some(8),
        statement_summary_capacity: 8,
        ..DatabaseConfig::default()
    });
    db.query("CREATE (:Memory {id: 'community-cache-crystal', is_crystal: true, importance: 0.8})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'community-cache-source-one'})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'community-cache-source-two'})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'community-cache-entity-one', community_id: 13})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'community-cache-entity-two', community_id: 13})")
        .unwrap();
    db.query("MATCH (c:Memory {id: 'community-cache-crystal'}), (s:Memory {id: 'community-cache-source-one'}) CREATE (c)-[:SYNTHESIZED_FROM]->(s)")
        .unwrap();
    db.query("MATCH (c:Memory {id: 'community-cache-crystal'}), (s:Memory {id: 'community-cache-source-two'}) CREATE (c)-[:SYNTHESIZED_FROM]->(s)")
        .unwrap();
    db.query("MATCH (s:Memory {id: 'community-cache-source-one'}), (e:Entity {id: 'community-cache-entity-one'}) CREATE (s)-[:MENTIONS]->(e)")
        .unwrap();
    db.query("MATCH (s:Memory {id: 'community-cache-source-one'}), (e:Entity {id: 'community-cache-entity-two'}) CREATE (s)-[:MENTIONS]->(e)")
        .unwrap();
    db.query("MATCH (s:Memory {id: 'community-cache-source-two'}), (e:Entity {id: 'community-cache-entity-one'}) CREATE (s)-[:MENTIONS]->(e)")
        .unwrap();

    let parameters = community_parameters(vec![Value::Int(13)]).unwrap();
    let mut snapshot = db.begin_read_transaction().unwrap();
    let first = snapshot
        .query_request(bounded_batch_query(COMMUNITY_ROWS, &parameters))
        .unwrap();
    let second = snapshot
        .query_request(bounded_batch_query(COMMUNITY_ROWS, &parameters))
        .unwrap();
    assert_eq!(first.rows, second.rows);
    assert_eq!(first.rows.len(), 1);
    assert_eq!(first.rows[0]["hit_count"], Value::Int(3));
    assert_eq!(first.rows[0]["source_memory_count"], Value::Int(2));
    let stats = snapshot.plan_cache_stats();
    assert_eq!(stats.entries, 1);
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.hits, 1);
}

#[test]
fn reads_crystal_source_visibility_for_wiki_community_rows() {
    let mut db = Database::new();
    db.query("CREATE (:Memory {id: 'crystal-alpha', is_crystal: true, crystal_title: 'Alpha Crystal', title: 'Alpha Title', content: 'Alpha content', importance: 0.8, metadata: '{\"c\":1}', is_latest: false, lifecycle_state: 'active'})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'crystal-beta', is_crystal: true, title: 'Beta Title', content: 'Beta content', importance: 0.9})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'plain-memory', is_crystal: false, title: 'Plain', importance: 9.0, metadata: '{\"plain\":1}'})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'source-one', metadata: '{\"s\":1}', is_latest: true, lifecycle_state: 'ready'})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'source-two', metadata: '{\"s\":2}', is_latest: false, lifecycle_state: 'archived'})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'source-three'})").unwrap();
    db.query("CREATE (:Entity {id: 'entity-one', name: 'One', community_id: 7})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'entity-two', name: 'Two', community_id: 7})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'entity-three', name: 'Three', community_id: 8})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'entity-null', name: 'Null Entity'})")
        .unwrap();
    db.query(
        "MATCH (c:Memory {id: 'crystal-alpha'}), (s:Memory {id: 'source-one'}) CREATE (c)-[:SYNTHESIZED_FROM]->(s)",
    )
    .unwrap();
    db.query(
        "MATCH (c:Memory {id: 'crystal-alpha'}), (s:Memory {id: 'source-two'}) CREATE (c)-[:SYNTHESIZED_FROM]->(s)",
    )
    .unwrap();
    db.query(
        "MATCH (c:Memory {id: 'crystal-beta'}), (s:Memory {id: 'source-three'}) CREATE (c)-[:SYNTHESIZED_FROM]->(s)",
    )
    .unwrap();
    db.query(
        "MATCH (c:Memory {id: 'plain-memory'}), (s:Memory {id: 'source-one'}) CREATE (c)-[:SYNTHESIZED_FROM]->(s)",
    )
    .unwrap();
    db.query(
        "MATCH (s:Memory {id: 'source-one'}), (e:Entity {id: 'entity-one'}) CREATE (s)-[:MENTIONS]->(e)",
    )
    .unwrap();
    db.query(
        "MATCH (s:Memory {id: 'source-one'}), (e:Entity {id: 'entity-two'}) CREATE (s)-[:MENTIONS]->(e)",
    )
    .unwrap();
    db.query(
        "MATCH (s:Memory {id: 'source-two'}), (e:Entity {id: 'entity-one'}) CREATE (s)-[:MENTIONS]->(e)",
    )
    .unwrap();
    db.query(
        "MATCH (s:Memory {id: 'source-two'}), (e:Entity {id: 'entity-three'}) CREATE (s)-[:MENTIONS]->(e)",
    )
    .unwrap();
    db.query(
        "MATCH (s:Memory {id: 'source-two'}), (e:Entity {id: 'entity-null'}) CREATE (s)-[:MENTIONS]->(e)",
    )
    .unwrap();
    db.query(
        "MATCH (s:Memory {id: 'source-three'}), (e:Entity {id: 'entity-three'}) CREATE (s)-[:MENTIONS]->(e)",
    )
    .unwrap();

    let epoch = db.commit_epoch().unwrap();
    let parameters = community_parameters(vec![Value::Int(7), Value::Int(8)]).unwrap();
    let mut snapshot = db.begin_read_transaction().unwrap();
    let visibility = snapshot
        .query_request(bounded_batch_query(VISIBILITY_ROWS, &parameters))
        .unwrap();
    assert_eq!(snapshot.commit_epoch(), epoch);
    assert_eq!(db.commit_epoch().unwrap(), epoch);
    assert_eq!(visibility.rows.len(), 5);
    for (key, value) in [
        ("crystal_memory_id", Value::String("crystal-alpha".into())),
        ("source_memory_id", Value::String("source-one".into())),
        ("entity_id", Value::String("entity-one".into())),
        ("community_id", Value::Int(7)),
        ("crystal_title", Value::String("Alpha Crystal".into())),
        ("display_title", Value::String("Alpha Crystal".into())),
        ("content", Value::String("Alpha content".into())),
        ("importance", Value::Float(0.8)),
        ("crystal_metadata", Value::String("{\"c\":1}".into())),
        ("crystal_is_latest", Value::Bool(false)),
        ("crystal_lifecycle_state", Value::String("active".into())),
        ("source_metadata", Value::String("{\"s\":1}".into())),
        ("source_is_latest", Value::Bool(true)),
        ("source_lifecycle_state", Value::String("ready".into())),
    ] {
        assert_eq!(visibility.rows[0][key], value);
    }
    assert_eq!(
        visibility.rows[2]["source_memory_id"],
        Value::String("source-two".into())
    );
    assert_eq!(visibility.rows[2]["source_is_latest"], Value::Bool(false));
    assert_eq!(
        visibility.rows[4]["crystal_memory_id"],
        Value::String("crystal-beta".into())
    );
    assert_eq!(
        visibility.rows[4]["display_title"],
        Value::String("Beta Title".into())
    );
    assert_eq!(visibility.rows[4]["crystal_is_latest"], Value::Bool(true));
    assert_eq!(visibility.rows[4]["source_is_latest"], Value::Bool(true));
}

#[test]
fn crystal_source_visibility_uses_query_runtime_plan_cache() {
    let mut db = Database::new_with_config(DatabaseConfig {
        max_plan_cache_entries: Some(8),
        statement_summary_capacity: 8,
        ..DatabaseConfig::default()
    });
    db.query(
        "CREATE (:Memory {id: 'visibility-cache-crystal', is_crystal: true, title: 'Cached'})",
    )
    .unwrap();
    db.query("CREATE (:Memory {id: 'visibility-cache-source'})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'visibility-cache-entity', community_id: 11})")
        .unwrap();
    db.query("MATCH (c:Memory {id: 'visibility-cache-crystal'}), (s:Memory {id: 'visibility-cache-source'}) CREATE (c)-[:SYNTHESIZED_FROM]->(s)")
        .unwrap();
    db.query("MATCH (s:Memory {id: 'visibility-cache-source'}), (e:Entity {id: 'visibility-cache-entity'}) CREATE (s)-[:MENTIONS]->(e)")
        .unwrap();

    let parameters = community_parameters(vec![Value::Int(11)]).unwrap();
    let mut snapshot = db.begin_read_transaction().unwrap();
    let first = snapshot
        .query_request(bounded_batch_query(VISIBILITY_ROWS, &parameters))
        .unwrap();
    let second = snapshot
        .query_request(bounded_batch_query(VISIBILITY_ROWS, &parameters))
        .unwrap();
    assert_eq!(first.rows, second.rows);
    assert_eq!(first.rows.len(), 1);
    assert_eq!(
        first.rows[0]["crystal_memory_id"],
        Value::String("visibility-cache-crystal".into())
    );
    let stats = snapshot.plan_cache_stats();
    assert_eq!(stats.entries, 1);
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.hits, 1);
}

// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;

const MEMORY_LOOKUP: &str = "MATCH (m:Memory) WHERE m.id IN $ids \
    RETURN m.id AS id, id(m) AS node_id ORDER BY id";
const MEMORY_LABELS: &str = "MATCH (m:Memory {id: $id})-[:HAS_LABEL]->(l:Label) \
    RETURN l.id AS label_id, id(l) AS node_id, l.name AS name, \
    l.canonical_name AS canonical_name, l.color AS color, l.description AS description \
    ORDER BY name, label_id, node_id LIMIT $limit";
const MEMORY_LABEL_PROJECTION: &str = "MATCH (m:Memory {id: $id})-[r:HAS_LABEL]->(l:Label) \
    RETURN l.id AS label_id, id(l) AS label_node_id, id(r) AS relationship_id, \
    l.name AS name, l.canonical_name AS canonical_name, l.future_label_field AS future_label_field, \
    r.assigned_by AS assigned_by, r.future_edge_field AS future_edge_field \
    ORDER BY name, label_id, relationship_id, label_node_id LIMIT $limit";
const SOURCE_LABELS: &str = "MATCH (s:Source {id: $id})-[:HAS_LABEL]->(l:Label) \
    RETURN l.id AS label_id, l.name AS name ORDER BY name, label_id LIMIT $limit";
const SOURCE_LABEL_PROJECTION: &str = "MATCH (s:Source {id: $id})-[r:HAS_LABEL]->(l:Label) \
    RETURN l.id AS label_id, l.future_label_field AS future_label_field, \
    r.assigned_by AS assigned_by ORDER BY label_id LIMIT $limit";

fn lookup_parameters(ids: &[&str]) -> Result<BTreeMap<String, Value>> {
    if ids.is_empty() || ids.iter().any(|id| id.is_empty()) {
        return Err(HawDBError::Semantic(
            "non-empty external ids required".into(),
        ));
    }
    Ok(BTreeMap::from([(
        "ids".into(),
        Value::List(ids.iter().map(|id| Value::String((*id).into())).collect()),
    )]))
}

fn page_parameters(id: &str, limit: i64) -> BTreeMap<String, Value> {
    BTreeMap::from([
        ("id".into(), Value::String(id.into())),
        ("limit".into(), Value::Int(limit)),
    ])
}

#[test]
fn projects_entity_labels_for_nowledge_growth() {
    let mut db = Database::new_with_config(DatabaseConfig {
        max_plan_cache_entries: Some(8),
        statement_summary_capacity: 8,
        ..DatabaseConfig::default()
    });
    db.query("CREATE (:Memory {id: 'projected_label_memory_a'})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'projected_label_memory_b'})")
        .unwrap();
    db.query("CREATE (:Source {id: 'projected_label_source_a'})")
        .unwrap();
    db.query("CREATE (:Label {id: 'label_beta', name: 'Beta', canonical_name: 'beta', color: '#00f', future_label_field: 'label-b'})")
        .unwrap();
    db.query("CREATE (:Label {id: 'label_zeta', name: 'Zeta', canonical_name: 'zeta', color: '#0f0', future_label_field: 'label-z'})")
        .unwrap();
    db.query(
        "CREATE (:Label {id: 'label_source', name: 'Source', future_label_field: 'label-source'})",
    )
    .unwrap();
    db.query("MATCH (m:Memory {id: 'projected_label_memory_a'}), (l:Label {id: 'label_beta'}) CREATE (m)-[:HAS_LABEL {assigned_by: 'system', weight: 2, future_edge_field: 'edge-b'}]->(l)")
        .unwrap();
    db.query("MATCH (m:Memory {id: 'projected_label_memory_a'}), (l:Label {id: 'label_zeta'}) CREATE (m)-[:HAS_LABEL {assigned_by: 'manual', future_edge_field: 'edge-z'}]->(l)")
        .unwrap();
    db.query("MATCH (m:Memory {id: 'projected_label_memory_b'}), (l:Label {id: 'label_zeta'}) CREATE (m)-[:HAS_LABEL {assigned_by: 'system', future_edge_field: 'edge-b2'}]->(l)")
        .unwrap();
    db.query("MATCH (s:Source {id: 'projected_label_source_a'}), (l:Label {id: 'label_source'}) CREATE (s)-[:HAS_LABEL {assigned_by: 'source', future_edge_field: 'edge-source'}]->(l)")
        .unwrap();
    db.query("CREATE (:Label {id: 'label_alpha', name: 'Alpha', canonical_name: 'alpha', color: '#f00', future_label_field: 'label-a'})")
        .unwrap();
    db.query("MATCH (m:Memory {id: 'projected_label_memory_a'}), (l:Label {id: 'label_alpha'}) CREATE (m)-[:HAS_LABEL {assigned_by: 'later', weight: 9, future_edge_field: 'edge-a'}]->(l)")
        .unwrap();

    let epoch = db.commit_epoch().unwrap();
    let mut snapshot = db.begin_read_transaction().unwrap();
    let lookup = lookup_parameters(&[
        "projected_label_memory_a",
        "missing_projected_label_memory",
        "projected_label_memory_b",
    ])
    .unwrap();
    let a = page_parameters("projected_label_memory_a", 1);
    let b = page_parameters("projected_label_memory_b", 1);
    let entities = snapshot
        .query_request(bounded_batch_query(MEMORY_LOOKUP, &lookup))
        .unwrap();
    assert_eq!(entities.rows.len(), 2);
    assert_eq!(
        entities.rows[0].get("id"),
        Some(&Value::String("projected_label_memory_a".into()))
    );
    assert_eq!(
        entities.rows[1].get("id"),
        Some(&Value::String("projected_label_memory_b".into()))
    );
    let projected_a = snapshot
        .query_request(bounded_batch_query(MEMORY_LABEL_PROJECTION, &a))
        .unwrap();
    let projected_b = snapshot
        .query_request(bounded_batch_query(MEMORY_LABEL_PROJECTION, &b))
        .unwrap();
    assert_eq!(projected_a.rows.len(), 1);
    assert_eq!(projected_b.rows.len(), 1);
    let row = &projected_a.rows[0];
    for (key, value) in [
        ("label_id", "label_alpha"),
        ("name", "Alpha"),
        ("canonical_name", "alpha"),
        ("future_label_field", "label-a"),
        ("assigned_by", "later"),
        ("future_edge_field", "edge-a"),
    ] {
        assert_eq!(row.get(key), Some(&Value::String(value.into())));
    }
    assert!(row.get("color").is_none());
    assert!(row.get("weight").is_none());
    assert!(matches!(row.get("relationship_id"), Some(Value::Int(_))));
    assert!(matches!(row.get("label_node_id"), Some(Value::Int(_))));
    assert_eq!(
        projected_b.rows[0].get("label_id"),
        Some(&Value::String("label_zeta".into()))
    );

    let stats = snapshot.plan_cache_stats();
    for (query, parameters, expected) in [
        (MEMORY_LOOKUP, &lookup, &entities),
        (MEMORY_LABEL_PROJECTION, &a, &projected_a),
        (MEMORY_LABEL_PROJECTION, &b, &projected_b),
    ] {
        assert_eq!(
            snapshot
                .query_request(bounded_batch_query(query, parameters))
                .unwrap()
                .rows,
            expected.rows
        );
    }
    let repeated = snapshot.plan_cache_stats();
    assert_eq!(repeated.entries, stats.entries);
    assert_eq!(repeated.misses, stats.misses);
    assert_eq!(repeated.hits, stats.hits + 3);

    let source = snapshot
        .query_request(bounded_batch_query(
            SOURCE_LABEL_PROJECTION,
            &page_parameters("projected_label_source_a", 5),
        ))
        .unwrap();
    assert_eq!(source.rows.len(), 1);
    assert_eq!(
        source.rows[0].get("label_id"),
        Some(&Value::String("label_source".into()))
    );
    assert_eq!(
        source.rows[0].get("future_label_field"),
        Some(&Value::String("label-source".into()))
    );
    assert_eq!(
        source.rows[0].get("assigned_by"),
        Some(&Value::String("source".into()))
    );
    assert_eq!(snapshot.commit_epoch(), epoch);
    assert_eq!(db.commit_epoch().unwrap(), epoch);
}

#[test]
fn entity_label_projected_read_rejects_empty_property_names_without_wal() {
    let path = unique_test_dir("entity_label_projected_empty_property_without_wal");
    let mut db = Database::open(&path).unwrap();
    db.query("CREATE (:Memory {id: 'projected_label_wal_memory'})")
        .unwrap();
    db.query("CREATE (:Label {id: 'projected_label_wal_label'})")
        .unwrap();
    db.query("MATCH (m:Memory {id: 'projected_label_wal_memory'}), (l:Label {id: 'projected_label_wal_label'}) CREATE (m)-[:HAS_LABEL]->(l)")
        .unwrap();
    let epoch = db.commit_epoch().unwrap();
    let wal = read_test_wal(&path).unwrap();
    let mut snapshot = db.begin_read_transaction().unwrap();
    let parameters = page_parameters("projected_label_wal_memory", 5);
    for query in [
        "MATCH (m:Memory {id: $id})-[r:HAS_LABEL]->(l:Label) RETURN l.",
        "MATCH (m:Memory {id: $id})-[r:HAS_LABEL]->(l:Label) RETURN r.",
    ] {
        let error = snapshot
            .query_request(bounded_batch_query(query, &parameters))
            .unwrap_err();
        assert!(matches!(error, HawDBError::Parse(_)), "{error}");
    }
    assert_eq!(snapshot.commit_epoch(), epoch);
    assert_eq!(db.commit_epoch().unwrap(), epoch);
    assert_eq!(read_test_wal(&path).unwrap(), wal);
    drop(snapshot);
    drop(db);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn reads_entity_labels_for_nowledge_has_label_shapes() {
    let mut db = Database::new_with_config(DatabaseConfig {
        max_plan_cache_entries: Some(8),
        statement_summary_capacity: 8,
        ..DatabaseConfig::default()
    });
    db.query("CREATE (:Label {id: 'alpha', name: 'Alpha', canonical_name: 'alpha', color: '#fff', description: 'Alpha label'})")
        .unwrap();
    db.query("CREATE (:Label {id: 'beta', name: 'Beta', canonical_name: 'beta'})")
        .unwrap();
    db.query("CREATE (:Label {id: 'gamma', name: 'Gamma', canonical_name: 'gamma'})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'memory_1'})").unwrap();
    db.query("CREATE (:Memory {id: 'memory_2'})").unwrap();
    db.query("CREATE (:Source {id: 'source_1'})").unwrap();
    db.query(
        "MATCH (m:Memory {id: 'memory_1'}), (l:Label {id: 'beta'}) CREATE (m)-[:HAS_LABEL]->(l)",
    )
    .unwrap();
    db.query(
        "MATCH (m:Memory {id: 'memory_1'}), (l:Label {id: 'alpha'}) CREATE (m)-[:HAS_LABEL]->(l)",
    )
    .unwrap();
    db.query(
        "MATCH (s:Source {id: 'source_1'}), (l:Label {id: 'gamma'}) CREATE (s)-[:HAS_LABEL]->(l)",
    )
    .unwrap();
    let epoch = db.commit_epoch().unwrap();
    let mut snapshot = db.begin_read_transaction().unwrap();
    let lookup = lookup_parameters(&["memory_1", "memory_2", "missing"]).unwrap();
    let first = page_parameters("memory_1", 5);
    let second = page_parameters("memory_2", 5);
    let entities = snapshot
        .query_request(bounded_batch_query(MEMORY_LOOKUP, &lookup))
        .unwrap();
    assert_eq!(entities.rows.len(), 2);
    assert_eq!(
        entities.rows[0].get("id"),
        Some(&Value::String("memory_1".into()))
    );
    assert_eq!(
        entities.rows[1].get("id"),
        Some(&Value::String("memory_2".into()))
    );
    for row in &entities.rows {
        assert!(matches!(row.get("node_id"), Some(Value::Int(_))));
    }
    let labels = snapshot
        .query_request(bounded_batch_query(MEMORY_LABELS, &first))
        .unwrap();
    let empty = snapshot
        .query_request(bounded_batch_query(MEMORY_LABELS, &second))
        .unwrap();
    assert_eq!(labels.rows.len(), 2);
    assert!(empty.rows.is_empty());
    for (key, value) in [
        ("label_id", "alpha"),
        ("name", "Alpha"),
        ("canonical_name", "alpha"),
        ("color", "#fff"),
        ("description", "Alpha label"),
    ] {
        assert_eq!(labels.rows[0].get(key), Some(&Value::String(value.into())));
    }
    assert_eq!(
        labels.rows[1].get("label_id"),
        Some(&Value::String("beta".into()))
    );
    assert_eq!(labels.rows[1].get("color"), Some(&Value::Null));
    assert_eq!(labels.rows[1].get("description"), Some(&Value::Null));

    let stats = snapshot.plan_cache_stats();
    for (query, parameters, expected) in [
        (MEMORY_LOOKUP, &lookup, &entities),
        (MEMORY_LABELS, &first, &labels),
        (MEMORY_LABELS, &second, &empty),
    ] {
        assert_eq!(
            snapshot
                .query_request(bounded_batch_query(query, parameters))
                .unwrap()
                .rows,
            expected.rows
        );
    }
    let repeated = snapshot.plan_cache_stats();
    assert_eq!(repeated.entries, stats.entries);
    assert_eq!(repeated.misses, stats.misses);
    assert_eq!(repeated.hits, stats.hits + 3);

    let source = snapshot
        .query_request(bounded_batch_query(
            SOURCE_LABELS,
            &page_parameters("source_1", 1),
        ))
        .unwrap();
    assert_eq!(source.rows.len(), 1);
    assert_eq!(
        source.rows[0].get("label_id"),
        Some(&Value::String("gamma".into()))
    );
    assert_eq!(snapshot.commit_epoch(), epoch);
    assert_eq!(db.commit_epoch().unwrap(), epoch);
}

#[test]
fn entity_label_read_requests_validate_non_empty_filters() {
    let db = Database::new();
    let mut snapshot = db.begin_read_transaction().unwrap();
    let parameters = lookup_parameters(&["memory_1"]).unwrap();
    let error = snapshot
        .query_request(bounded_batch_query(
            "MATCH (m:Bad Label) WHERE m.id IN $ids RETURN m.id AS id",
            &parameters,
        ))
        .unwrap_err();
    assert!(matches!(error, HawDBError::Parse(_)), "{error}");
    for ids in [&[][..], &[""][..]] {
        assert!(lookup_parameters(ids)
            .unwrap_err()
            .to_string()
            .contains("non-empty external ids"));
    }
}

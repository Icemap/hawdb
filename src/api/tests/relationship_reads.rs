// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;

const SEED: &str = "MATCH (m:Memory {id: $id}) RETURN id(m) AS node_id, m.id AS id";
const PAGE: &str = "MATCH (m:Memory {id: $id})-[r:HAS_LABEL]->(l:Label) \
    RETURN m.id AS source, l.id AS target, id(r) AS relationship_id, r.weight AS weight \
    ORDER BY relationship_id LIMIT $limit";
const CANDIDATES: &str = "MATCH (m:Memory) WHERE m.id IN $ids RETURN m.id AS id ORDER BY id";
const SCOPED_SEEDS: &str = "MATCH (m:Memory) WHERE m.id IN $ids AND m.source_id = $source_id \
    AND (CASE WHEN m.space_id IS NULL OR m.space_id = '' \
         THEN 'default' ELSE m.space_id END) = $space_id \
    RETURN m.id AS id ORDER BY id";
const SCOPED_PAGE: &str = "MATCH (m:Memory)-[r:HAS_LABEL]->(l:Label) \
    WHERE m.id IN $ids AND m.source_id = $source_id \
    AND (CASE WHEN m.space_id IS NULL OR m.space_id = '' \
         THEN 'default' ELSE m.space_id END) = $space_id \
    RETURN m.id AS source, l.id AS target, id(r) AS relationship_id \
    ORDER BY source, relationship_id LIMIT $limit";

#[test]
fn knowledge_relationships_fail_soft_for_unknown_relationship_type() {
    let mut db = Database::new();
    db.query("CREATE (:Memory {id: 'memory_1', title: 'First'})")
        .unwrap();

    let parameters = BTreeMap::from([("id".into(), Value::String("memory_1".into()))]);
    let mut snapshot =
        db.begin_read_transaction()
            .unwrap()
            .into_bounded(crate::ReadSnapshotBudget {
                max_rows: 2,
                max_payload_bytes: 4096,
            });
    let seed = snapshot
        .query_request(bounded_batch_query(SEED, &parameters))
        .unwrap();
    assert_eq!(seed.rows.len(), 1);
    assert_eq!(seed.rows[0]["id"], parameters["id"]);
    let rows = snapshot.query_request(bounded_batch_query(
        "MATCH (m:Memory {id: $id})-[r:DOES_NOT_EXIST]-() RETURN id(r) AS relationship_id LIMIT 4",
        &parameters,
    )).unwrap();
    assert!(rows.rows.is_empty());
    assert_eq!(snapshot.commit_epoch(), 1);
    assert_eq!(snapshot.usage().output_rows, 1);
}

#[test]
fn retrieves_knowledge_relationships_grouped_by_seed() {
    let mut db = Database::new_with_config(DatabaseConfig {
        max_plan_cache_entries: Some(8),
        statement_summary_capacity: 8,
        ..DatabaseConfig::default()
    });
    db.query(
        "CREATE (:Memory {id: 'memory_1', title: 'First'})-[:HAS_LABEL {weight: 3}]->(:Label {id: 'label_1', name: 'Database'})",
    )
    .unwrap();
    db.query(
        "CREATE (:Memory {id: 'memory_2', title: 'Second'})-[:HAS_LABEL]->(:Label {id: 'label_2', name: 'Rust'})",
    )
    .unwrap();

    let mut snapshot = db.begin_read_transaction().unwrap();
    let seeds = [
        ("memory_2", Some("label_2")),
        ("missing", None),
        ("memory_1", Some("label_1")),
    ];
    let mut first_outputs = Vec::new();
    for (id, expected_target) in seeds {
        let parameters = BTreeMap::from([
            ("id".into(), Value::String(id.into())),
            ("limit".into(), Value::Int(4)),
        ]);
        let seed = snapshot
            .query_request(bounded_batch_query(SEED, &parameters))
            .unwrap();
        let page = snapshot
            .query_request(bounded_batch_query(PAGE, &parameters))
            .unwrap();
        if let Some(target) = expected_target {
            assert_eq!(seed.rows.len(), 1);
            assert_eq!(seed.rows[0]["id"], Value::String(id.into()));
            assert_eq!(page.rows.len(), 1);
            assert_eq!(page.rows[0]["source"], Value::String(id.into()));
            assert_eq!(page.rows[0]["target"], Value::String(target.into()));
            assert_eq!(
                page.rows[0]["weight"],
                if id == "memory_1" {
                    Value::Int(3)
                } else {
                    Value::Null
                }
            );
        } else {
            assert!(seed.rows.is_empty());
            assert!(page.rows.is_empty());
        }
        first_outputs.push((seed.rows, page.rows));
    }
    assert_eq!(snapshot.commit_epoch(), 2);
    let stats = snapshot.plan_cache_stats();
    for ((id, _), (seed, page)) in seeds.into_iter().zip(first_outputs) {
        let parameters = BTreeMap::from([
            ("id".into(), Value::String(id.into())),
            ("limit".into(), Value::Int(4)),
        ]);
        assert_eq!(
            snapshot
                .query_request(bounded_batch_query(SEED, &parameters))
                .unwrap()
                .rows,
            seed
        );
        assert_eq!(
            snapshot
                .query_request(bounded_batch_query(PAGE, &parameters))
                .unwrap()
                .rows,
            page
        );
    }
    let repeated = snapshot.plan_cache_stats();
    assert_eq!(repeated.entries, stats.entries);
    assert_eq!(repeated.misses, stats.misses);
    assert_eq!(repeated.hits, stats.hits + 6);
}

#[test]
fn scoped_knowledge_relationships_report_filtered_seeds() {
    let mut db = Database::new_with_config(DatabaseConfig {
        max_plan_cache_entries: Some(8),
        statement_summary_capacity: 8,
        ..DatabaseConfig::default()
    });
    db.query(
        "CREATE (:Memory {id: 'memory_1', title: 'First', source_id: 'thread_1', space_id: ''})-[:HAS_LABEL]->(:Label {id: 'label_1', name: 'Database'})",
    )
    .unwrap();
    db.query(
        "CREATE (:Memory {id: 'memory_2', title: 'Second', source_id: 'thread_2', space_id: 'default'})-[:HAS_LABEL]->(:Label {id: 'label_2', name: 'Rust'})",
    )
    .unwrap();

    let mut parameters = batch_id_parameters(&["memory_1", "memory_2"]).unwrap();
    parameters.insert("source_id".into(), Value::String("thread_1".into()));
    parameters.insert("space_id".into(), Value::String("default".into()));
    parameters.insert("limit".into(), Value::Int(4));
    let mut snapshot =
        db.begin_read_transaction()
            .unwrap()
            .into_bounded(crate::ReadSnapshotBudget {
                max_rows: 4,
                max_payload_bytes: 4096,
            });
    let candidates = snapshot
        .query_request(bounded_batch_query(CANDIDATES, &parameters))
        .unwrap();
    assert_eq!(candidates.rows.len(), 2);
    assert_eq!(candidates.rows[0]["id"], Value::String("memory_1".into()));
    assert_eq!(candidates.rows[1]["id"], Value::String("memory_2".into()));
    let scoped = snapshot
        .query_request(bounded_batch_query(SCOPED_SEEDS, &parameters))
        .unwrap();
    assert_eq!(scoped.rows.len(), 1);
    assert_eq!(scoped.rows[0]["id"], Value::String("memory_1".into()));
    let page = snapshot
        .query_request(bounded_batch_query(SCOPED_PAGE, &parameters))
        .unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0]["source"], Value::String("memory_1".into()));
    assert_eq!(page.rows[0]["target"], Value::String("label_1".into()));
    assert_eq!(snapshot.commit_epoch(), 2);
    assert_eq!(snapshot.usage().output_rows, 4);
    let mut cached = db.begin_read_transaction().unwrap();
    assert_eq!(
        cached
            .query_request(bounded_batch_query(SCOPED_PAGE, &parameters))
            .unwrap()
            .rows,
        page.rows
    );
    let stats = cached.plan_cache_stats();
    assert_eq!(
        cached
            .query_request(bounded_batch_query(SCOPED_PAGE, &parameters))
            .unwrap()
            .rows,
        page.rows
    );
    let repeated = cached.plan_cache_stats();
    assert_eq!(repeated.entries, stats.entries);
    assert_eq!(repeated.misses, stats.misses);
    assert_eq!(repeated.hits, stats.hits + 1);
}

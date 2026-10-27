// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;

const IDENTITIES: &str = "MATCH (n) WHERE n.id IN $ids RETURN n.id AS id ORDER BY n.id";
const EDGE_COUNT: &str = "MATCH (s)-[r]->(t) WHERE s.id IN $ids AND t.id IN $ids \
    RETURN count(r) AS count";
const EDGE_PAGE: &str = "MATCH (s)-[r]->(t) WHERE s.id IN $ids AND t.id IN $ids \
    RETURN s.id AS source_id, t.id AS target_id, id(r) AS relationship_id, \
    type(r) AS relationship_type, COALESCE(r.strength, r.confidence, 0.5) AS strength \
    ORDER BY source_id, target_id, relationship_type, relationship_id LIMIT $limit";

fn parameters(ids: &[&str], limit: i64) -> Result<BTreeMap<String, Value>> {
    if ids.is_empty() {
        return Err(HawDBError::Semantic("non-empty id list required".into()));
    }
    let mut parameters = batch_id_parameters(ids)?;
    parameters.insert("limit".into(), Value::Int(limit));
    Ok(parameters)
}

#[test]
fn induced_edge_read_rejects_empty_external_ids() {
    for ids in [&[][..], &[""][..]] {
        assert!(parameters(ids, 5)
            .unwrap_err()
            .to_string()
            .contains("non-empty id"));
    }
}

#[test]
fn reads_induced_edges_for_nowledge_overview_and_subgraph_shapes() {
    let mut db = Database::new_with_config(DatabaseConfig {
        max_plan_cache_entries: Some(8),
        statement_summary_capacity: 8,
        ..DatabaseConfig::default()
    });
    db.query("CREATE (:Memory {id: 'memory_1'})").unwrap();
    db.query("CREATE (:Entity {id: 'entity_1'})").unwrap();
    db.query("CREATE (:Entity {id: 'entity_2'})").unwrap();
    db.query("CREATE (:Source {id: 'source_outside'})").unwrap();
    db.query("MATCH (m:Memory {id: 'memory_1'}), (e:Entity {id: 'entity_1'}) CREATE (m)-[:MENTIONS {confidence: 0.8}]->(e)")
        .unwrap();
    db.query("MATCH (e1:Entity {id: 'entity_1'}), (e2:Entity {id: 'entity_2'}) CREATE (e1)-[:RELATES_TO {strength: 0.9}]->(e2)")
        .unwrap();
    db.query("MATCH (m:Memory {id: 'memory_1'}), (e:Entity {id: 'entity_2'}) CREATE (m)-[:RELATES_TO]->(e)")
        .unwrap();
    db.query("MATCH (m:Memory {id: 'memory_1'}), (s:Source {id: 'source_outside'}) CREATE (m)-[:SOURCED_FROM]->(s)")
        .unwrap();

    let epoch = db.commit_epoch().unwrap();
    let requested = ["memory_1", "entity_1", "entity_2", "missing"];
    let parameters = parameters(&requested, 5).unwrap();
    let mut snapshot =
        db.begin_read_transaction()
            .unwrap()
            .into_bounded(crate::ReadSnapshotBudget {
                max_rows: 7,
                max_payload_bytes: 4096,
            });
    let identities = snapshot
        .query_request(bounded_batch_query(IDENTITIES, &parameters))
        .unwrap();
    assert_eq!(identities.rows.len(), 3);
    for (index, id) in ["entity_1", "entity_2", "memory_1"].iter().enumerate() {
        assert_eq!(identities.rows[index]["id"], Value::String((*id).into()));
    }
    // Missing caller slots are response shaping over the query-selected identities.
    let missing: Vec<_> = requested
        .iter()
        .filter(|id| {
            !identities
                .rows
                .iter()
                .any(|row| row["id"] == Value::String((**id).into()))
        })
        .copied()
        .collect();
    assert_eq!(missing, vec!["missing"]);
    let count = snapshot
        .query_request(bounded_batch_query(EDGE_COUNT, &parameters))
        .unwrap();
    assert_eq!(count.rows.len(), 1);
    assert_eq!(count.rows[0]["count"], Value::Int(3));
    let output = snapshot
        .query_request(bounded_batch_query(EDGE_PAGE, &parameters))
        .unwrap();
    assert_eq!(output.rows.len(), 3);
    for (index, source, target, kind, strength) in [
        (0, "entity_1", "entity_2", "RELATES_TO", 0.9),
        (1, "memory_1", "entity_1", "MENTIONS", 0.8),
        (2, "memory_1", "entity_2", "RELATES_TO", 0.5),
    ] {
        assert_eq!(
            output.rows[index]["source_id"],
            Value::String(source.into())
        );
        assert_eq!(
            output.rows[index]["target_id"],
            Value::String(target.into())
        );
        assert_eq!(
            output.rows[index]["relationship_type"],
            Value::String(kind.into())
        );
        assert_eq!(output.rows[index]["strength"], Value::Float(strength));
    }
    assert_eq!(snapshot.commit_epoch(), epoch);
    assert_eq!(snapshot.usage().output_rows, 7);
    assert_eq!(db.commit_epoch().unwrap(), epoch);
    let mut paging = db.begin_read_transaction().unwrap();
    let mut parameters = parameters;
    parameters.insert("limit".into(), Value::Int(2));
    let count = paging
        .query_request(bounded_batch_query(EDGE_COUNT, &parameters))
        .unwrap();
    assert_eq!(count.rows[0]["count"], Value::Int(3));
    let limited = paging
        .query_request(bounded_batch_query(EDGE_PAGE, &parameters))
        .unwrap();
    assert_eq!(limited.rows.len(), 2);
    for index in 0..2 {
        assert_eq!(limited.rows[index], output.rows[index]);
    }
    let stats = paging.plan_cache_stats();
    let repeated = paging
        .query_request(bounded_batch_query(EDGE_PAGE, &parameters))
        .unwrap();
    assert_eq!(repeated.rows, limited.rows);
    let repeated_stats = paging.plan_cache_stats();
    assert_eq!(repeated_stats.entries, stats.entries);
    assert_eq!(repeated_stats.misses, stats.misses);
    assert_eq!(repeated_stats.hits, stats.hits + 1);
}

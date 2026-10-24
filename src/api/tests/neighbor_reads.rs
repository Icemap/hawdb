// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;

const SEED: &str = "MATCH (m:Memory {id: $id}) RETURN id(m) AS node_id, m.id AS id";
const OUTGOING: &str = "MATCH (m:Memory {id: $id})-[r:LINKS]->(e:Entity) \
    RETURN id(m) AS source_node_id, m.id AS source, id(e) AS target_node_id, e.id AS target, \
    id(r) AS relationship_id, type(r) AS type, r.weight AS weight, 1 AS hop \
    ORDER BY relationship_id LIMIT $limit";
const SECOND_HOP: &str = "MATCH (m:Memory {id: $id})-[:LINKS]->(a:Entity)-[r:LINKS]->(b:Entity) \
    RETURN id(a) AS source_node_id, a.id AS source, id(b) AS target_node_id, b.id AS target, \
    id(r) AS relationship_id, type(r) AS type, r.weight AS weight, 2 AS hop \
    ORDER BY relationship_id LIMIT $limit";
const INCOMING: &str = "MATCH (m:Memory {id: $id})<-[r]-(e:Entity) \
    RETURN e.id AS source, m.id AS target, type(r) AS type, id(r) AS relationship_id \
    ORDER BY relationship_id LIMIT $limit";
const ANY_DIRECTION: &str = "MATCH (m:Memory {id: $id})-[r]-(e:Entity) \
    RETURN id(m) AS seed_node_id, e.id AS neighbor, id(r) AS relationship_id \
    ORDER BY relationship_id LIMIT $limit";
const SCOPED: &str = "MATCH (m:Memory {id: $id})-[r:LINKS]->(e:Entity) \
    WHERE (CASE WHEN m.space_id IS NULL OR m.space_id = '' THEN 'default' ELSE m.space_id END) = $space \
    RETURN id(m) AS seed_node_id, e.id AS target, id(r) AS relationship_id \
    ORDER BY relationship_id LIMIT $limit";

fn parameters(id: &str, limit: i64) -> BTreeMap<String, Value> {
    BTreeMap::from([
        ("id".into(), Value::String(id.into())),
        ("limit".into(), Value::Int(limit)),
    ])
}

#[test]
fn navigation_reads_idless_seed_with_explicit_internal_identity() {
    let mut db = Database::new();
    db.query(
        "CREATE (:Memory {title: 'Anonymous root'})-[:LINKS]->(:Entity {id: 'leaf', name: 'Leaf'})",
    )
    .unwrap();
    let parameters = BTreeMap::from([
        ("node_id".into(), Value::Int(0)),
        ("target".into(), Value::String("leaf".into())),
        (
            "node_ids".into(),
            Value::List(vec![Value::Int(0), Value::Int(1)]),
        ),
    ]);
    let mut snapshot =
        db.begin_read_transaction()
            .unwrap()
            .into_bounded(crate::ReadSnapshotBudget {
                max_rows: 4,
                max_payload_bytes: 4096,
            });
    for query in [
        "MATCH (m:Memory)-[r:LINKS]->(e:Entity) WHERE id(m) = $node_id RETURN id(m) AS source_node_id, m.id AS source, id(e) AS target_node_id, e.id AS target, id(r) AS relationship_id",
        "MATCH (m:Memory)-[r:LINKS]->(e:Entity {id: $target}) WHERE id(m) = $node_id RETURN id(m) AS source_node_id, m.id AS source, id(e) AS target_node_id, e.id AS target, id(r) AS relationship_id",
    ] {
        let rows = snapshot.query_request(bounded_batch_query(query, &parameters)).unwrap();
        assert_eq!(rows.rows.len(), 1);
        assert_eq!(rows.rows[0]["source_node_id"], Value::Int(0));
        assert_eq!(rows.rows[0]["source"], Value::Null);
        assert_eq!(rows.rows[0]["target_node_id"], Value::Int(1));
        assert_eq!(rows.rows[0]["target"], Value::String("leaf".into()));
        assert_eq!(rows.rows[0]["relationship_id"], Value::Int(0));
    }
    let nodes = snapshot.query_request(bounded_batch_query(
        "MATCH (n) WHERE id(n) IN $node_ids RETURN id(n) AS node_id, n.id AS id ORDER BY node_id", &parameters,
    )).unwrap();
    assert_eq!(nodes.rows.len(), 2);
    assert_eq!(nodes.rows[0]["node_id"], Value::Int(0));
    assert_eq!(nodes.rows[0]["id"], Value::Null);
    assert_eq!(nodes.rows[1]["node_id"], Value::Int(1));
    assert_eq!(nodes.rows[1]["id"], Value::String("leaf".into()));
    assert_eq!(snapshot.usage().output_rows, 4);
}

#[test]
fn retrieves_knowledge_neighbors_without_search_projection() {
    let mut db = Database::new();
    db.query(
        "CREATE (:Memory {id: 'root', title: 'Root'})-[:LINKS]->(:Entity {id: 'mid', name: 'Mid'})",
    )
    .unwrap();
    db.query("CREATE (:Entity {id: 'leaf', name: 'Leaf'})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'mention', name: 'Mention'})")
        .unwrap();
    db.query("MATCH (a:Entity {id: 'mid'}), (b:Entity {id: 'leaf'}) CREATE (a)-[:LINKS {weight: 2}]->(b)").unwrap();
    db.query(
        "MATCH (e:Entity {id: 'mention'}), (m:Memory {id: 'root'}) CREATE (e)-[:MENTIONS]->(m)",
    )
    .unwrap();
    let parameters = parameters("root", 8);
    let mut snapshot =
        db.begin_read_transaction()
            .unwrap()
            .into_bounded(crate::ReadSnapshotBudget {
                max_rows: 4,
                max_payload_bytes: 4096,
            });
    let seed = snapshot
        .query_request(bounded_batch_query(SEED, &parameters))
        .unwrap();
    assert_eq!(seed.rows.len(), 1);
    assert_eq!(seed.rows[0]["node_id"], Value::Int(0));
    for (query, source, target, hop, weight) in [
        (OUTGOING, "root", "mid", 1, Value::Null),
        (SECOND_HOP, "mid", "leaf", 2, Value::Int(2)),
    ] {
        let rows = snapshot
            .query_request(bounded_batch_query(query, &parameters))
            .unwrap();
        assert_eq!(rows.rows.len(), 1);
        assert_eq!(rows.rows[0]["source"], Value::String(source.into()));
        assert_eq!(rows.rows[0]["target"], Value::String(target.into()));
        assert_eq!(rows.rows[0]["hop"], Value::Int(hop));
        assert_eq!(rows.rows[0]["type"], Value::String("LINKS".into()));
        assert_eq!(rows.rows[0]["weight"], weight);
        assert_eq!(rows.rows[0]["source_node_id"], Value::Int(hop - 1));
        assert_eq!(rows.rows[0]["target_node_id"], Value::Int(hop));
        assert_eq!(rows.rows[0]["relationship_id"], Value::Int(hop - 1));
    }
    let incoming = snapshot
        .query_request(bounded_batch_query(INCOMING, &parameters))
        .unwrap();
    assert_eq!(incoming.rows.len(), 1);
    assert_eq!(incoming.rows[0]["source"], Value::String("mention".into()));
    assert_eq!(incoming.rows[0]["target"], Value::String("root".into()));
    assert_eq!(incoming.rows[0]["type"], Value::String("MENTIONS".into()));
    for query in [
        "MATCH (m:Memory {id: $id})-[:DOES_NOT_EXIST]-(e) RETURN id(e) AS node_id LIMIT $limit",
        "MATCH (m:Memory {id: $id})-[:DOES_NOT_EXIST]-(a)-[:DOES_NOT_EXIST]-(e) RETURN id(e) AS node_id LIMIT $limit",
    ] {
        assert!(snapshot.query_request(bounded_batch_query(query, &parameters)).unwrap().rows.is_empty());
    }
    assert_eq!(snapshot.commit_epoch(), 5);
    assert_eq!(snapshot.usage().output_rows, 4);
}

#[test]
fn scoped_knowledge_neighbors_filters_seed_by_metadata() {
    let mut db = Database::new_with_config(DatabaseConfig {
        max_plan_cache_entries: Some(8),
        statement_summary_capacity: 8,
        ..DatabaseConfig::default()
    });
    db.query("CREATE (:Memory {id: 'root', title: 'Root', space_id: ''})-[:LINKS]->(:Entity {id: 'leaf', name: 'Leaf'})").unwrap();
    let mut parameters = parameters("root", 4);
    parameters.insert("space".into(), Value::String("default".into()));
    let mut snapshot = db.begin_read_transaction().unwrap();
    let scoped = snapshot
        .query_request(bounded_batch_query(SCOPED, &parameters))
        .unwrap();
    assert_eq!(scoped.rows.len(), 1);
    assert_eq!(scoped.rows[0]["seed_node_id"], Value::Int(0));
    assert_eq!(scoped.rows[0]["target"], Value::String("leaf".into()));
    let stats = snapshot.plan_cache_stats();
    assert_eq!(
        snapshot
            .query_request(bounded_batch_query(SCOPED, &parameters))
            .unwrap()
            .rows,
        scoped.rows
    );
    let repeated = snapshot.plan_cache_stats();
    assert_eq!(repeated.entries, stats.entries);
    assert_eq!(repeated.misses, stats.misses);
    assert_eq!(repeated.hits, stats.hits + 1);
    parameters.insert("space".into(), Value::String("team".into()));
    assert!(snapshot
        .query_request(bounded_batch_query(SCOPED, &parameters))
        .unwrap()
        .rows
        .is_empty());
    let seed = snapshot
        .query_request(bounded_batch_query(SEED, &parameters))
        .unwrap();
    assert_eq!(seed.rows.len(), 1);
    assert_eq!(seed.rows[0]["node_id"], Value::Int(0));
}

#[test]
fn knowledge_neighbors_reports_limit_and_missing_seed() {
    let mut db = Database::new_with_config(DatabaseConfig {
        max_plan_cache_entries: Some(8),
        statement_summary_capacity: 8,
        ..DatabaseConfig::default()
    });
    db.query("CREATE (:Memory {id: 'root', title: 'Root'})-[:LINKS]->(:Entity {id: 'left', name: 'Left'})").unwrap();
    db.query("CREATE (:Entity {id: 'right', name: 'Right'})")
        .unwrap();
    db.query("MATCH (m:Memory {id: 'root'}), (e:Entity {id: 'right'}) CREATE (m)-[:LINKS]->(e)")
        .unwrap();
    let mut parameters = parameters("root", 1);
    let mut snapshot = db.begin_read_transaction().unwrap();
    let count = snapshot
        .query_request(bounded_batch_query(
            "MATCH (m:Memory {id: $id})-[r]-(e:Entity) RETURN count(r) AS count",
            &parameters,
        ))
        .unwrap();
    assert_eq!(count.rows[0]["count"], Value::Int(2));
    let limited = snapshot
        .query_request(bounded_batch_query(ANY_DIRECTION, &parameters))
        .unwrap();
    assert_eq!(limited.rows.len(), 1);
    assert_eq!(limited.rows[0]["seed_node_id"], Value::Int(0));
    assert_eq!(limited.rows[0]["neighbor"], Value::String("left".into()));
    let stats = snapshot.plan_cache_stats();
    assert_eq!(
        snapshot
            .query_request(bounded_batch_query(ANY_DIRECTION, &parameters))
            .unwrap()
            .rows,
        limited.rows
    );
    let repeated = snapshot.plan_cache_stats();
    assert_eq!(repeated.entries, stats.entries);
    assert_eq!(repeated.misses, stats.misses);
    assert_eq!(repeated.hits, stats.hits + 1);
    parameters.insert("limit".into(), Value::Int(0));
    assert!(snapshot
        .query_request(bounded_batch_query(ANY_DIRECTION, &parameters))
        .unwrap()
        .rows
        .is_empty());
    parameters.insert("limit".into(), Value::Int(8));
    parameters.insert("id".into(), Value::String("missing".into()));
    assert!(snapshot
        .query_request(bounded_batch_query(SEED, &parameters))
        .unwrap()
        .rows
        .is_empty());
    assert!(snapshot
        .query_request(bounded_batch_query(ANY_DIRECTION, &parameters))
        .unwrap()
        .rows
        .is_empty());
}

#[test]
fn query_navigation_retains_dense_adjacency_coverage() {
    let mut db = Database::new();
    db.query("CREATE (:Memory {id: 'root', title: 'Root'})")
        .unwrap();
    for index in 0..DENSE_ADJACENCY_DEGREE_THRESHOLD {
        let parameters = BTreeMap::from([
            ("id".into(), Value::String(format!("entity-{index}"))),
            ("name".into(), Value::String(format!("Entity {index}"))),
        ]);
        db.query_request(bounded_batch_query(
            "CREATE (:Entity {id: $id, name: $name})",
            &parameters,
        ))
        .unwrap();
        db.query_request(bounded_batch_query(
            "MATCH (m:Memory {id: 'root'}), (e:Entity {id: $id}) CREATE (m)-[:LINKS]->(e)",
            &parameters,
        ))
        .unwrap();
    }
    {
        let runtime = db.runtime.get().unwrap();
        let kind = runtime.catalog.rel_type_id("LINKS").unwrap();
        let stats = runtime.store.adjacency_group_stats(
            NodeId(0),
            kind,
            crate::store::AdjacencyDirection::Outgoing,
        );
        assert_eq!(stats.layout, crate::store::AdjacencyLayout::Dense);
        assert_eq!(stats.degree, DENSE_ADJACENCY_DEGREE_THRESHOLD);
        assert_eq!(stats.node_id, NodeId(0));
        let groups = runtime
            .store
            .adjacency_group_stats_for_node(NodeId(0), crate::store::AdjacencyDirection::Outgoing);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0], stats);
    }
    let parameters = parameters("root", DENSE_ADJACENCY_DEGREE_THRESHOLD as i64);
    let mut snapshot = db.begin_read_transaction().unwrap();
    for query in [
        "MATCH (m:Memory {id: $id})-[r:LINKS]->(e:Entity) RETURN id(r) AS relationship_id, type(r) AS type, e.id AS target ORDER BY id(e) LIMIT $limit",
        "MATCH (m:Memory {id: $id})-[r]->(e:Entity) RETURN id(r) AS relationship_id, type(r) AS type, e.id AS target ORDER BY id(e) LIMIT $limit",
    ] {
        let output = snapshot.query_request_profiled(crate::QueryRequest::new(query).with_params(&parameters).with_output_limits(QueryStreamOptions {
            max_rows: Some(DENSE_ADJACENCY_DEGREE_THRESHOLD), max_payload_bytes: Some(64 * 1024),
        })).unwrap();
        assert_eq!(output.output.rows.len(), DENSE_ADJACENCY_DEGREE_THRESHOLD);
        assert_eq!(output.execution_profile.max_rows, Some(DENSE_ADJACENCY_DEGREE_THRESHOLD));
        for (index, row) in output.output.rows.iter().enumerate() {
            assert_eq!(row["target"], Value::String(format!("entity-{index}")));
            assert_eq!(row["type"], Value::String("LINKS".into()));
            assert_eq!(row["relationship_id"], Value::Int(index as i64));
        }
    }
    let nodes = snapshot
        .query_request(bounded_batch_query(
            "MATCH (m:Memory {id: $id})-[:LINKS]->(e:Entity) RETURN count(DISTINCT e) AS count",
            &parameters,
        ))
        .unwrap();
    assert_eq!(
        nodes.rows[0]["count"],
        Value::Int(DENSE_ADJACENCY_DEGREE_THRESHOLD as i64)
    );
    let seed = snapshot
        .query_request(bounded_batch_query(SEED, &parameters))
        .unwrap();
    assert_eq!(seed.rows.len(), 1);
    assert_eq!(seed.rows[0]["node_id"], Value::Int(0));
}

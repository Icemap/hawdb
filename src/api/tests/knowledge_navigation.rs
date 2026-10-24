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

#[test]
fn knowledge_entity_returns_no_rows_for_missing_seed() {
    let mut db = Database::new();
    db.query("CREATE (:Entity {id: 'entity_1', name: 'HawDB'})")
        .unwrap();
    let mut snapshot = db.begin_read_transaction().unwrap();
    let output = snapshot
        .query_request(bounded_batch_query(
            "MATCH (e:Entity {id: $id}) RETURN e AS entity",
            &BTreeMap::from([("id".into(), Value::String("missing".into()))]),
        ))
        .unwrap();
    assert_eq!(snapshot.commit_epoch(), 1);
    assert!(output.rows.is_empty());
}

#[test]
fn knowledge_entity_reads_idless_nodes_by_internal_identity() {
    let mut db = Database::new();
    db.query("CREATE (:Entity {name: 'Anonymous entity', kind: 'concept'})")
        .unwrap();
    let mut snapshot = db.begin_read_transaction().unwrap();
    let output = snapshot.query_request(bounded_batch_query(
        "MATCH (e:Entity) WHERE id(e) = $node_id RETURN id(e) AS node_id, e.id AS id, e.name AS name",
        &BTreeMap::from([("node_id".into(), Value::Int(0))]),
    )).unwrap();
    assert_eq!(snapshot.commit_epoch(), 1);
    assert_eq!(output.rows.len(), 1);
    assert_eq!(output.rows[0]["node_id"], Value::Int(0));
    assert_eq!(output.rows[0]["id"], Value::Null);
    assert_eq!(
        output.rows[0]["name"],
        Value::String("Anonymous entity".into())
    );
}

const NAVIGATION_SEED: &str = "MATCH (m:Memory {id: $source}) RETURN id(m) AS node_id, m.id AS id";
const NAVIGATION_TARGET: &str =
    "MATCH (e:Entity {id: $target}) RETURN id(e) AS node_id, e.id AS id";
const ONE_HOP_PATH: &str = "MATCH (m:Memory {id: $source})-[r]->(e:Entity {id: $target}) \
    WHERE $max_hops >= 1 RETURN id(m) AS source_node_id, m.id AS source, \
    id(e) AS target_node_id, e.id AS target, id(r) AS relationship_id, type(r) AS type \
    ORDER BY relationship_id LIMIT $limit";
const TWO_HOP_PATH: &str =
    "MATCH (m:Memory {id: $source})-[a:LINKS]->(mid:Entity)-[b:LINKS]->(e:Entity {id: $target}) \
    WHERE $max_hops >= 2 RETURN id(m) AS source_node_id, m.id AS source, \
    id(mid) AS middle_node_id, mid.id AS middle, id(e) AS target_node_id, e.id AS target, \
    id(a) AS first_relationship_id, type(a) AS first_type, a.weight AS first_weight, \
    id(b) AS second_relationship_id, type(b) AS second_type, b.weight AS second_weight \
    ORDER BY first_relationship_id, second_relationship_id LIMIT $limit";
const SCOPED_PATH: &str = "MATCH (m:Memory {id: $source})-[r:LINKS]->(e:Entity {id: $target}) \
    WHERE m.source_id = $source_id AND \
    (CASE WHEN e.space_id IS NULL OR e.space_id = '' THEN 'default' ELSE e.space_id END) = $space \
    RETURN id(m) AS source_node_id, id(e) AS target_node_id, id(r) AS relationship_id \
    ORDER BY relationship_id LIMIT $limit";
const SUBGRAPH_NODES: &str = "MATCH (m:Memory {id: $source})-[:LINKS*0..2]->(n) \
    RETURN DISTINCT id(n) AS node_id, n.id AS id ORDER BY node_id LIMIT $node_limit";
const SUBGRAPH_EDGES: &str = "MATCH (s)-[r:LINKS]->(t) \
    WHERE id(s) IN $node_ids AND id(t) IN $node_ids \
    RETURN id(s) AS source_node_id, s.id AS source, id(t) AS target_node_id, t.id AS target, \
    id(r) AS relationship_id, type(r) AS type ORDER BY relationship_id LIMIT $relationship_limit";

fn navigation_parameters(source: &str, target: &str) -> BTreeMap<String, Value> {
    BTreeMap::from([
        ("source".into(), Value::String(source.into())),
        ("target".into(), Value::String(target.into())),
        ("max_hops".into(), Value::Int(2)),
        ("limit".into(), Value::Int(4)),
        ("node_limit".into(), Value::Int(8)),
        ("relationship_limit".into(), Value::Int(8)),
    ])
}

#[test]
fn retrieves_bounded_knowledge_paths_without_search_projection() {
    let mut db = Database::new();
    db.query("CREATE (:Memory {id: 'root', title: 'Root'})-[:LINKS {weight: 1}]->(:Entity {id: 'mid', name: 'Mid'})").unwrap();
    db.query("CREATE (:Entity {id: 'leaf', name: 'Leaf'})")
        .unwrap();
    db.query("MATCH (a:Entity {id: 'mid'}), (b:Entity {id: 'leaf'}) CREATE (a)-[:LINKS {weight: 2}]->(b)").unwrap();
    let parameters = navigation_parameters("root", "leaf");
    let mut snapshot =
        db.begin_read_transaction()
            .unwrap()
            .into_bounded(crate::ReadSnapshotBudget {
                max_rows: 3,
                max_payload_bytes: 4096,
            });
    for (query, id) in [(NAVIGATION_SEED, 0), (NAVIGATION_TARGET, 2)] {
        let rows = snapshot
            .query_request(bounded_batch_query(query, &parameters))
            .unwrap();
        assert_eq!(rows.rows.len(), 1);
        assert_eq!(rows.rows[0]["node_id"], Value::Int(id));
    }
    let paths = snapshot
        .query_request(bounded_batch_query(TWO_HOP_PATH, &parameters))
        .unwrap();
    assert_eq!(paths.rows.len(), 1);
    for (column, value) in [
        ("source_node_id", Value::Int(0)),
        ("source", Value::String("root".into())),
        ("middle_node_id", Value::Int(1)),
        ("middle", Value::String("mid".into())),
        ("target_node_id", Value::Int(2)),
        ("target", Value::String("leaf".into())),
        ("first_relationship_id", Value::Int(0)),
        ("first_type", Value::String("LINKS".into())),
        ("first_weight", Value::Int(1)),
        ("second_relationship_id", Value::Int(1)),
        ("second_type", Value::String("LINKS".into())),
        ("second_weight", Value::Int(2)),
    ] {
        assert_eq!(paths.rows[0][column], value);
    }
    assert_eq!(snapshot.commit_epoch(), 3);
    assert_eq!(snapshot.usage().output_rows, 3);
}

#[test]
fn scoped_knowledge_paths_filter_source_and_target_by_metadata() {
    let mut db = Database::new_with_config(DatabaseConfig {
        max_plan_cache_entries: Some(8),
        ..DatabaseConfig::default()
    });
    db.query("CREATE (:Memory {id: 'root', source_id: 'thread_1'})-[:LINKS]->(:Entity {id: 'leaf', space_id: 'default'})").unwrap();
    let mut parameters = navigation_parameters("root", "leaf");
    parameters.insert("source_id".into(), Value::String("thread_1".into()));
    parameters.insert("space".into(), Value::String("default".into()));
    let mut snapshot = db.begin_read_transaction().unwrap();
    let scoped = snapshot
        .query_request(bounded_batch_query(SCOPED_PATH, &parameters))
        .unwrap();
    assert_eq!(scoped.rows.len(), 1);
    assert_eq!(scoped.rows[0]["source_node_id"], Value::Int(0));
    assert_eq!(scoped.rows[0]["target_node_id"], Value::Int(1));
    assert_eq!(scoped.rows[0]["relationship_id"], Value::Int(0));
    let stats = snapshot.plan_cache_stats();
    assert_eq!(
        snapshot
            .query_request(bounded_batch_query(SCOPED_PATH, &parameters))
            .unwrap()
            .rows,
        scoped.rows
    );
    let repeated = snapshot.plan_cache_stats();
    assert_eq!(repeated.entries, stats.entries);
    assert_eq!(repeated.misses, stats.misses);
    assert_eq!(repeated.hits, stats.hits + 1);
    parameters.insert("space".into(), Value::String("archive".into()));
    assert!(snapshot
        .query_request(bounded_batch_query(SCOPED_PATH, &parameters))
        .unwrap()
        .rows
        .is_empty());
    parameters.insert("space".into(), Value::String("default".into()));
    parameters.insert("source_id".into(), Value::String("thread_2".into()));
    assert!(snapshot
        .query_request(bounded_batch_query(SCOPED_PATH, &parameters))
        .unwrap()
        .rows
        .is_empty());
    for (query, id) in [(NAVIGATION_SEED, 0), (NAVIGATION_TARGET, 1)] {
        let rows = snapshot
            .query_request(bounded_batch_query(query, &parameters))
            .unwrap();
        assert_eq!(rows.rows.len(), 1);
        assert_eq!(rows.rows[0]["node_id"], Value::Int(id));
    }
}

#[test]
fn knowledge_paths_respects_direction_type_limit_and_missing_endpoint() {
    let mut db = Database::new_with_config(DatabaseConfig {
        max_plan_cache_entries: Some(8),
        ..DatabaseConfig::default()
    });
    db.query("CREATE (:Memory {id: 'root'})-[:LINKS]->(:Entity {id: 'left'})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'right'})").unwrap();
    db.query("MATCH (m:Memory {id: 'root'}), (e:Entity {id: 'right'}) CREATE (m)-[:LINKS]->(e)")
        .unwrap();
    db.query("MATCH (m:Memory {id: 'root'}), (e:Entity {id: 'right'}) CREATE (m)-[:MENTIONS]->(e)")
        .unwrap();
    let mut parameters = navigation_parameters("root", "right");
    let mut snapshot = db.begin_read_transaction().unwrap();
    for query in [
        "MATCH (m:Memory {id: $source})<-[r:LINKS]-(e:Entity {id: $target}) RETURN id(r) AS relationship_id LIMIT $limit",
        "MATCH (m:Memory {id: $source})-[r:DOES_NOT_EXIST]->(e:Entity {id: $target}) RETURN id(r) AS relationship_id LIMIT $limit",
    ] { assert!(snapshot.query_request(bounded_batch_query(query, &parameters)).unwrap().rows.is_empty()); }
    let count = snapshot
        .query_request(bounded_batch_query(
            "MATCH (m:Memory {id: $source})-[r]->(e:Entity {id: $target}) RETURN count(r) AS count",
            &parameters,
        ))
        .unwrap();
    assert_eq!(count.rows[0]["count"], Value::Int(2));
    parameters.insert("limit".into(), Value::Int(1));
    let limited = snapshot
        .query_request(bounded_batch_query(ONE_HOP_PATH, &parameters))
        .unwrap();
    assert_eq!(limited.rows.len(), 1);
    assert_eq!(limited.rows[0]["relationship_id"], Value::Int(1));
    assert_eq!(limited.rows[0]["source_node_id"], Value::Int(0));
    assert_eq!(limited.rows[0]["target_node_id"], Value::Int(2));
    assert_eq!(limited.rows[0]["type"], Value::String("LINKS".into()));
    let stats = snapshot.plan_cache_stats();
    assert_eq!(
        snapshot
            .query_request(bounded_batch_query(ONE_HOP_PATH, &parameters))
            .unwrap()
            .rows,
        limited.rows
    );
    let repeated = snapshot.plan_cache_stats();
    assert_eq!(repeated.entries, stats.entries);
    assert_eq!(repeated.misses, stats.misses);
    assert_eq!(repeated.hits, stats.hits + 1);
    parameters.insert("limit".into(), Value::Int(4));
    let all = snapshot
        .query_request(bounded_batch_query(ONE_HOP_PATH, &parameters))
        .unwrap();
    assert_eq!(all.rows.len(), 2);
    assert_eq!(all.rows[1]["type"], Value::String("MENTIONS".into()));
    assert_eq!(all.rows[1]["relationship_id"], Value::Int(2));
    parameters.insert("max_hops".into(), Value::Int(0));
    assert!(snapshot
        .query_request(bounded_batch_query(ONE_HOP_PATH, &parameters))
        .unwrap()
        .rows
        .is_empty());
    parameters.insert("max_hops".into(), Value::Int(2));
    parameters.insert("limit".into(), Value::Int(0));
    assert!(snapshot
        .query_request(bounded_batch_query(ONE_HOP_PATH, &parameters))
        .unwrap()
        .rows
        .is_empty());
    for (source, target, source_id, target_id) in [
        ("root", "missing", Some(0), None),
        ("missing", "right", None, Some(2)),
        ("missing-source", "missing-target", None, None),
    ] {
        let parameters = navigation_parameters(source, target);
        for (query, expected) in [(NAVIGATION_SEED, source_id), (NAVIGATION_TARGET, target_id)] {
            let rows = snapshot
                .query_request(bounded_batch_query(query, &parameters))
                .unwrap();
            assert_eq!(rows.rows.len(), usize::from(expected.is_some()));
            if let Some(id) = expected {
                assert_eq!(rows.rows[0]["node_id"], Value::Int(id));
            }
        }
        assert!(snapshot
            .query_request(bounded_batch_query(ONE_HOP_PATH, &parameters))
            .unwrap()
            .rows
            .is_empty());
        assert!(snapshot.query_request(bounded_batch_query(
            "MATCH (m:Memory {id: $source})-[*1..2]-(e:Entity {id: $target}) RETURN id(e) AS node_id LIMIT $limit", &parameters,
        )).unwrap().rows.is_empty());
    }
}

#[test]
fn retrieves_bounded_knowledge_subgraph_without_search_projection() {
    let mut db = Database::new();
    db.query(
        "CREATE (:Memory {id: 'root', title: 'Root'})-[:LINKS]->(:Entity {id: 'mid', name: 'Mid'})",
    )
    .unwrap();
    db.query("CREATE (:Entity {id: 'leaf', name: 'Leaf'})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'mention', name: 'Mention'})")
        .unwrap();
    db.query("MATCH (a:Entity {id: 'mid'}), (b:Entity {id: 'leaf'}) CREATE (a)-[:LINKS]->(b)")
        .unwrap();
    db.query(
        "MATCH (m:Memory {id: 'root'}), (e:Entity {id: 'mention'}) CREATE (m)-[:MENTIONS]->(e)",
    )
    .unwrap();
    let mut parameters = navigation_parameters("root", "leaf");
    let mut snapshot =
        db.begin_read_transaction()
            .unwrap()
            .into_bounded(crate::ReadSnapshotBudget {
                max_rows: 5,
                max_payload_bytes: 4096,
            });
    let nodes = snapshot
        .query_request(bounded_batch_query(SUBGRAPH_NODES, &parameters))
        .unwrap();
    assert_eq!(nodes.rows.len(), 3);
    for (index, (row, id)) in nodes.rows.iter().zip(["root", "mid", "leaf"]).enumerate() {
        assert_eq!(row["node_id"], Value::Int(index as i64));
        assert_eq!(row["id"], Value::String(id.into()));
    }
    parameters.insert(
        "node_ids".into(),
        Value::List(
            nodes
                .rows
                .iter()
                .map(|row| row["node_id"].clone())
                .collect(),
        ),
    );
    let edges = snapshot
        .query_request(bounded_batch_query(SUBGRAPH_EDGES, &parameters))
        .unwrap();
    assert_eq!(edges.rows.len(), 2);
    for (index, (row, (source, target))) in edges
        .rows
        .iter()
        .zip([("root", "mid"), ("mid", "leaf")])
        .enumerate()
    {
        assert_eq!(row["relationship_id"], Value::Int(index as i64));
        assert_eq!(row["source_node_id"], Value::Int(index as i64));
        assert_eq!(row["target_node_id"], Value::Int(index as i64 + 1));
        assert_eq!(row["source"], Value::String(source.into()));
        assert_eq!(row["target"], Value::String(target.into()));
        assert_eq!(row["type"], Value::String("LINKS".into()));
    }
    assert_eq!(snapshot.commit_epoch(), 5);
    assert_eq!(snapshot.usage().output_rows, 5);
}

#[test]
fn scoped_knowledge_subgraph_filters_seed_by_metadata() {
    const NODES: &str =
        "MATCH (m:Memory {id: $source})-[:LINKS*0..1]->(n) WHERE m.source_id = $source_id \
        RETURN DISTINCT id(n) AS node_id, n.id AS id ORDER BY node_id LIMIT $node_limit";
    let mut db = Database::new_with_config(DatabaseConfig {
        max_plan_cache_entries: Some(8),
        ..DatabaseConfig::default()
    });
    db.query(
        "CREATE (:Memory {id: 'root', source_id: 'thread_1'})-[:LINKS]->(:Entity {id: 'leaf'})",
    )
    .unwrap();
    let mut parameters = navigation_parameters("root", "leaf");
    parameters.insert("source_id".into(), Value::String("thread_1".into()));
    let mut snapshot = db.begin_read_transaction().unwrap();
    let scoped = snapshot
        .query_request(bounded_batch_query(NODES, &parameters))
        .unwrap();
    assert_eq!(scoped.rows.len(), 2);
    assert_eq!(scoped.rows[0]["id"], Value::String("root".into()));
    assert_eq!(scoped.rows[1]["id"], Value::String("leaf".into()));
    let stats = snapshot.plan_cache_stats();
    assert_eq!(
        snapshot
            .query_request(bounded_batch_query(NODES, &parameters))
            .unwrap()
            .rows,
        scoped.rows
    );
    let repeated = snapshot.plan_cache_stats();
    assert_eq!(repeated.entries, stats.entries);
    assert_eq!(repeated.misses, stats.misses);
    assert_eq!(repeated.hits, stats.hits + 1);
    parameters.insert(
        "node_ids".into(),
        Value::List(
            scoped
                .rows
                .iter()
                .map(|row| row["node_id"].clone())
                .collect(),
        ),
    );
    let edges = snapshot
        .query_request(bounded_batch_query(SUBGRAPH_EDGES, &parameters))
        .unwrap();
    assert_eq!(edges.rows.len(), 1);
    assert_eq!(edges.rows[0]["relationship_id"], Value::Int(0));
    parameters.insert("source_id".into(), Value::String("thread_2".into()));
    let filtered = snapshot
        .query_request(bounded_batch_query(NODES, &parameters))
        .unwrap();
    assert!(filtered.rows.is_empty());
    parameters.insert("node_ids".into(), Value::List(Vec::new()));
    assert!(snapshot
        .query_request(bounded_batch_query(SUBGRAPH_EDGES, &parameters))
        .unwrap()
        .rows
        .is_empty());
    let seed = snapshot
        .query_request(bounded_batch_query(NAVIGATION_SEED, &parameters))
        .unwrap();
    assert_eq!(seed.rows.len(), 1);
    assert_eq!(seed.rows[0]["node_id"], Value::Int(0));
}

#[test]
fn knowledge_subgraph_reports_limits_and_missing_seed() {
    let mut db = Database::new_with_config(DatabaseConfig {
        max_plan_cache_entries: Some(8),
        ..DatabaseConfig::default()
    });
    db.query("CREATE (:Memory {id: 'root'})-[:LINKS]->(:Entity {id: 'left'})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'right'})").unwrap();
    db.query("MATCH (m:Memory {id: 'root'}), (e:Entity {id: 'right'}) CREATE (m)-[:LINKS]->(e)")
        .unwrap();
    let mut snapshot = db.begin_read_transaction().unwrap();
    let mut parameters = navigation_parameters("root", "right");
    parameters.insert("node_limit".into(), Value::Int(1));
    let nodes = snapshot
        .query_request(bounded_batch_query(SUBGRAPH_NODES, &parameters))
        .unwrap();
    assert_eq!(nodes.rows.len(), 1);
    assert_eq!(nodes.rows[0]["node_id"], Value::Int(0));
    parameters.insert(
        "node_ids".into(),
        Value::List(
            nodes
                .rows
                .iter()
                .map(|row| row["node_id"].clone())
                .collect(),
        ),
    );
    assert!(snapshot
        .query_request(bounded_batch_query(SUBGRAPH_EDGES, &parameters))
        .unwrap()
        .rows
        .is_empty());
    parameters.insert("node_limit".into(), Value::Int(8));
    let nodes = snapshot
        .query_request(bounded_batch_query(SUBGRAPH_NODES, &parameters))
        .unwrap();
    assert_eq!(nodes.rows.len(), 3);
    parameters.insert(
        "node_ids".into(),
        Value::List(
            nodes
                .rows
                .iter()
                .map(|row| row["node_id"].clone())
                .collect(),
        ),
    );
    parameters.insert("relationship_limit".into(), Value::Int(1));
    let edges = snapshot
        .query_request(bounded_batch_query(SUBGRAPH_EDGES, &parameters))
        .unwrap();
    assert_eq!(edges.rows.len(), 1);
    assert_eq!(edges.rows[0]["source_node_id"], Value::Int(0));
    assert_eq!(edges.rows[0]["target_node_id"], Value::Int(1));
    assert_eq!(edges.rows[0]["relationship_id"], Value::Int(0));
    let stats = snapshot.plan_cache_stats();
    assert_eq!(
        snapshot
            .query_request(bounded_batch_query(SUBGRAPH_EDGES, &parameters))
            .unwrap()
            .rows,
        edges.rows
    );
    let repeated = snapshot.plan_cache_stats();
    assert_eq!(repeated.entries, stats.entries);
    assert_eq!(repeated.misses, stats.misses);
    assert_eq!(repeated.hits, stats.hits + 1);
    parameters.insert("relationship_limit".into(), Value::Int(0));
    assert!(snapshot
        .query_request(bounded_batch_query(SUBGRAPH_EDGES, &parameters))
        .unwrap()
        .rows
        .is_empty());
    assert_eq!(
        snapshot
            .query_request(bounded_batch_query(NAVIGATION_SEED, &parameters))
            .unwrap()
            .rows
            .len(),
        1
    );
    parameters.insert("node_limit".into(), Value::Int(0));
    assert!(snapshot
        .query_request(bounded_batch_query(SUBGRAPH_NODES, &parameters))
        .unwrap()
        .rows
        .is_empty());
    parameters.insert("node_ids".into(), Value::List(Vec::new()));
    parameters.insert("relationship_limit".into(), Value::Int(8));
    assert!(snapshot
        .query_request(bounded_batch_query(SUBGRAPH_EDGES, &parameters))
        .unwrap()
        .rows
        .is_empty());
    assert!(snapshot.query_request(bounded_batch_query(
        "MATCH (m:Memory {id: $source})-[r:DOES_NOT_EXIST]-(n) RETURN id(r) AS relationship_id LIMIT $relationship_limit", &parameters,
    )).unwrap().rows.is_empty());
    parameters.insert("source".into(), Value::String("missing".into()));
    parameters.insert("node_limit".into(), Value::Int(8));
    assert!(snapshot
        .query_request(bounded_batch_query(NAVIGATION_SEED, &parameters))
        .unwrap()
        .rows
        .is_empty());
    assert!(snapshot
        .query_request(bounded_batch_query(SUBGRAPH_NODES, &parameters))
        .unwrap()
        .rows
        .is_empty());
}

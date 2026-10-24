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

const ENTITY_COUNT: &str = "MATCH (e:Entity) WHERE e.community_id IN $community_ids \
    RETURN count(e) AS count";
const VISIBILITY: &str = "MATCH (e:Entity) WHERE e.community_id IN $community_ids \
    OPTIONAL MATCH (m:Memory)-[:MENTIONS]->(e) \
    RETURN e.community_id AS community_id, e.id AS entity_id, id(e) AS entity_node_id, \
    e.name AS entity_name, e.entity_type AS entity_type, m.id AS memory_id, \
    id(m) AS memory_node_id, m.metadata AS memory_metadata, \
    COALESCE(m.is_latest, true) AS memory_is_latest, m.lifecycle_state AS memory_lifecycle_state \
    ORDER BY community_id, entity_name, entity_id, memory_id, entity_node_id, memory_node_id \
    LIMIT $limit";

#[test]
fn reads_community_entity_visibility_for_wiki_anchor_shapes() {
    let mut db = Database::new();
    db.query("CREATE (:Entity {id: 'entity_alpha', name: 'Alpha', entity_type: 'concept', community_id: 7})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'entity_beta', name: 'Beta', entity_type: 'person', community_id: 7})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'entity_gamma', name: 'Gamma', community_id: 8})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'entity_other', name: 'Other', community_id: 9})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'memory_alpha_new', metadata: '{\"rank\":1}', is_latest: true, lifecycle_state: 'active'})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'memory_alpha_old', metadata: '{\"rank\":2}', is_latest: false, lifecycle_state: 'archived'})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'memory_gamma', metadata: '{\"rank\":3}'})")
        .unwrap();
    db.query("CREATE (:Source {id: 'source_alpha', metadata: '{\"ignored\":true}'})")
        .unwrap();
    db.query("MATCH (m:Memory {id: 'memory_alpha_new'}), (e:Entity {id: 'entity_alpha'}) CREATE (m)-[:MENTIONS]->(e)")
        .unwrap();
    db.query("MATCH (m:Memory {id: 'memory_alpha_old'}), (e:Entity {id: 'entity_alpha'}) CREATE (m)-[:MENTIONS]->(e)")
        .unwrap();
    db.query("MATCH (m:Memory {id: 'memory_gamma'}), (e:Entity {id: 'entity_gamma'}) CREATE (m)-[:MENTIONS]->(e)")
        .unwrap();
    db.query("MATCH (s:Source {id: 'source_alpha'}), (e:Entity {id: 'entity_alpha'}) CREATE (s)-[:MENTIONS]->(e)")
        .unwrap();

    let epoch = db.commit_epoch().unwrap();
    let parameters = community_parameters(vec![Value::Int(7), Value::Int(8)]).unwrap();
    let mut snapshot = db.begin_read_transaction().unwrap();
    let count = snapshot
        .query_request(bounded_batch_query(ENTITY_COUNT, &parameters))
        .unwrap();
    assert_eq!(count.rows.len(), 1);
    assert_eq!(count.rows[0]["count"], Value::Int(3));
    let visibility = snapshot
        .query_request(bounded_batch_query(VISIBILITY, &parameters))
        .unwrap();
    assert_eq!(snapshot.commit_epoch(), epoch);
    assert_eq!(db.commit_epoch().unwrap(), epoch);
    assert_eq!(visibility.rows.len(), 4);
    for (key, value) in [
        ("community_id", Value::Int(7)),
        ("entity_id", Value::String("entity_alpha".into())),
        ("entity_name", Value::String("Alpha".into())),
        ("entity_type", Value::String("concept".into())),
        ("memory_id", Value::String("memory_alpha_new".into())),
        ("memory_metadata", Value::String("{\"rank\":1}".into())),
        ("memory_is_latest", Value::Bool(true)),
        ("memory_lifecycle_state", Value::String("active".into())),
    ] {
        assert_eq!(visibility.rows[0][key], value);
    }
    assert_eq!(
        visibility.rows[1]["memory_id"],
        Value::String("memory_alpha_old".into())
    );
    assert_eq!(visibility.rows[1]["memory_is_latest"], Value::Bool(false));
    assert_eq!(
        visibility.rows[2]["entity_id"],
        Value::String("entity_beta".into())
    );
    assert_eq!(visibility.rows[2]["memory_id"], Value::Null);
    assert_eq!(visibility.rows[2]["memory_node_id"], Value::Null);
    assert_eq!(visibility.rows[2]["memory_is_latest"], Value::Bool(true));
    assert_eq!(visibility.rows[3]["community_id"], Value::Int(8));
    assert_eq!(
        visibility.rows[3]["entity_id"],
        Value::String("entity_gamma".into())
    );
    assert_eq!(
        visibility.rows[3]["memory_id"],
        Value::String("memory_gamma".into())
    );
    assert_eq!(visibility.rows[3]["memory_is_latest"], Value::Bool(true));
    assert_eq!(
        snapshot
            .query_request(bounded_batch_query(ENTITY_COUNT, &parameters))
            .unwrap()
            .rows,
        count.rows
    );
    assert_eq!(
        snapshot
            .query_request(bounded_batch_query(VISIBILITY, &parameters))
            .unwrap()
            .rows,
        visibility.rows
    );
    let stats = snapshot.plan_cache_stats();
    assert_eq!(stats.misses, 2);
    assert_eq!(stats.hits, 2);
}

#[test]
fn community_entity_visibility_rejects_invalid_scope() {
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

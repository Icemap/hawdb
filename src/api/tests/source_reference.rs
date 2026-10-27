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

const SOURCE_REFERENCE_ENTITIES_QUERY: &str =
    "MATCH (source:Entity)-[r:RELATES_TO]->(target:Entity) \
     WHERE r.source_reference = $source_reference \
     RETURN id(r) AS relationship_id, \
     source.id AS source_entity_id, id(source) AS source_node_id, \
     target.id AS target_entity_id, id(target) AS target_node_id \
     ORDER BY relationship_id ASC";

const SOURCE_REFERENCE_ENTITY_QUERY: &str =
    "MATCH (e:Entity) WHERE e.id = $entity_id RETURN id(e) AS entity_node_id LIMIT 1";

const SOURCE_REFERENCE_INCIDENT_COUNT_QUERY: &str =
    "MATCH (e:Entity {id: $entity_id})-[r:RELATES_TO]-(other:Entity) \
     WHERE r.source_reference IS NULL OR r.source_reference = '' \
        OR r.source_reference <> $excluded_source_reference \
     RETURN count(r) AS relationship_count";

const SOURCE_REFERENCE_INCOMING_COUNT_QUERY: &str =
    "MATCH (other:Entity)-[r:RELATES_TO]->(e:Entity {id: $entity_id}) \
     WHERE r.source_reference IS NULL OR r.source_reference = '' \
        OR r.source_reference <> $excluded_source_reference \
     RETURN count(r) AS relationship_count";

#[test]
fn source_reference_entities_use_parameterized_pinned_reads() {
    let mut db = Database::new_with_config(DatabaseConfig {
        max_plan_cache_entries: Some(8),
        statement_summary_capacity: 8,
        ..DatabaseConfig::default()
    });
    db.query("CREATE (:Entity {id: 'entity_a'})").unwrap();
    db.query("CREATE (:Entity {id: 'entity_b'})").unwrap();
    db.query("CREATE (:Entity {id: 'entity_c'})").unwrap();
    db.query("MATCH (a:Entity {id: 'entity_a'}), (b:Entity {id: 'entity_b'}) CREATE (a)-[:RELATES_TO {source_reference: 'source_1'}]->(b)")
        .unwrap();
    db.query("MATCH (c:Entity {id: 'entity_c'}), (a:Entity {id: 'entity_a'}) CREATE (c)-[:RELATES_TO {source_reference: 'source_1'}]->(a)")
        .unwrap();
    let parameters = BTreeMap::from([(
        "source_reference".to_string(),
        Value::String("source_1".to_string()),
    )]);
    let mut snapshot = db.begin_read_transaction().unwrap();

    let first = snapshot
        .query_with_params_bounded(SOURCE_REFERENCE_ENTITIES_QUERY, &parameters, Some(2))
        .unwrap();
    let second = snapshot
        .query_with_params_bounded(SOURCE_REFERENCE_ENTITIES_QUERY, &parameters, Some(2))
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(first.rows.len(), 2);
    assert_eq!(read_test_plan_cache_metric(&snapshot, "entries"), 1);
    assert_eq!(read_test_plan_cache_metric(&snapshot, "misses"), 1);
    assert_eq!(read_test_plan_cache_metric(&snapshot, "hits"), 1);

    db.query("CREATE (:Entity {id: 'entity_d'})").unwrap();
    db.query("MATCH (d:Entity {id: 'entity_d'}), (a:Entity {id: 'entity_a'}) CREATE (d)-[:RELATES_TO {source_reference: 'source_1'}]->(a)")
        .unwrap();
    let pinned = snapshot
        .query_with_params_bounded(SOURCE_REFERENCE_ENTITIES_QUERY, &parameters, Some(2))
        .unwrap();
    assert_eq!(pinned.rows.len(), 2);

    let mut live = db.begin_read_transaction().unwrap();
    let current = live
        .query_with_params_bounded(SOURCE_REFERENCE_ENTITIES_QUERY, &parameters, Some(3))
        .unwrap();
    assert_eq!(current.rows.len(), 3);
}

#[test]
fn source_reference_delete_guard_uses_named_count_queries() {
    let mut db = Database::new();
    db.query("CREATE (:Entity {id: 'entity'})").unwrap();
    db.query("CREATE (:Entity {id: 'out'})").unwrap();
    db.query("CREATE (:Entity {id: 'in-empty'})").unwrap();
    db.query("MATCH (e:Entity {id: 'entity'}), (out:Entity {id: 'out'}) CREATE (e)-[:RELATES_TO {source_reference: 'other-source'}]->(out)")
        .unwrap();
    db.query("MATCH (incoming:Entity {id: 'in-empty'}), (e:Entity {id: 'entity'}) CREATE (incoming)-[:RELATES_TO {source_reference: ''}]->(e)")
        .unwrap();
    let parameters = BTreeMap::from([
        ("entity_id".to_string(), Value::String("entity".to_string())),
        (
            "excluded_source_reference".to_string(),
            Value::String("excluded-source".to_string()),
        ),
    ]);
    let mut read = db.begin_read_transaction().unwrap();

    let entity = read
        .query_with_params_bounded(SOURCE_REFERENCE_ENTITY_QUERY, &parameters, Some(1))
        .unwrap();
    assert_eq!(entity.rows.len(), 1);
    let incident = read
        .query_with_params_bounded(SOURCE_REFERENCE_INCIDENT_COUNT_QUERY, &parameters, Some(1))
        .unwrap();
    let incoming = read
        .query_with_params_bounded(SOURCE_REFERENCE_INCOMING_COUNT_QUERY, &parameters, Some(1))
        .unwrap();
    assert_eq!(
        incident.rows[0].get("relationship_count"),
        Some(&Value::Int(2))
    );
    assert_eq!(
        incoming.rows[0].get("relationship_count"),
        Some(&Value::Int(1))
    );

    let hostile_parameters = BTreeMap::from([(
        "entity_id".to_string(),
        Value::String("entity'}) MATCH (n) RETURN n //".to_string()),
    )]);
    assert!(read
        .query_with_params_bounded(SOURCE_REFERENCE_ENTITY_QUERY, &hostile_parameters, Some(1),)
        .unwrap()
        .rows
        .is_empty());
}

const SOURCE_REFERENCE_CREATE: &str = "MATCH (s:Entity {id: $source}), (t:Entity {id: $target}) \
    CREATE (s)-[:RELATES_TO {source_reference: $source_reference}]->(t)";
const SOURCE_REFERENCE_DELETE: &str = "MATCH (:Entity)-[r:RELATES_TO]->(:Entity) \
    WHERE r.source_reference = $source_reference DELETE r";

fn reference_parameters(reference: &str) -> Result<BTreeMap<String, Value>> {
    if reference.trim().is_empty() {
        return Err(HawDBError::Semantic(
            "non-empty source_reference required".into(),
        ));
    }
    Ok(BTreeMap::from([(
        "source_reference".into(),
        Value::String(reference.into()),
    )]))
}

#[test]
fn deletes_source_reference_relationships_for_nowledge_cleanup() {
    let mut db = Database::new();
    db.query("CREATE (:Entity {id: 'entity_1'})-[:RELATES_TO {source_reference: 'source_1'}]->(:Entity {id: 'entity_2'})").unwrap();
    db.query("CREATE (:Entity {id: 'entity_3'})").unwrap();
    db.query("CREATE (:Entity {id: 'entity_4'})").unwrap();
    for (target, reference) in [("entity_3", "source_1"), ("entity_4", "source_2")] {
        let mut parameters = reference_parameters(reference).unwrap();
        parameters.insert("source".into(), Value::String("entity_1".into()));
        parameters.insert("target".into(), Value::String(target.into()));
        db.query_request(bounded_batch_query(SOURCE_REFERENCE_CREATE, &parameters))
            .unwrap();
    }
    let epoch = db.commit_epoch().unwrap();
    let parameters = reference_parameters("source_1").unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    let candidates = transaction
        .query_request(bounded_batch_query(
            SOURCE_REFERENCE_ENTITIES_QUERY,
            &parameters,
        ))
        .unwrap();
    assert_eq!(candidates.rows.len(), 2);
    for row in &candidates.rows {
        assert_eq!(
            row.get("source_entity_id"),
            Some(&Value::String("entity_1".into()))
        );
        assert!(matches!(row.get("relationship_id"), Some(Value::Int(_))));
    }
    assert_eq!(
        candidates.rows[0]["target_entity_id"],
        Value::String("entity_2".into())
    );
    assert_eq!(
        candidates.rows[1]["target_entity_id"],
        Value::String("entity_3".into())
    );
    assert!(transaction
        .query_request(bounded_batch_query(SOURCE_REFERENCE_DELETE, &parameters))
        .unwrap()
        .rows
        .is_empty());
    assert!(transaction
        .query_request(bounded_batch_query(
            SOURCE_REFERENCE_ENTITIES_QUERY,
            &parameters
        ))
        .unwrap()
        .rows
        .is_empty());
    assert_eq!(transaction.commit().unwrap().rows.len(), 2);
    assert_eq!(epoch, 5);
    assert_eq!(db.commit_epoch().unwrap(), epoch + 1);
    assert!(db
        .query_request(bounded_batch_query(
            SOURCE_REFERENCE_ENTITIES_QUERY,
            &parameters
        ))
        .unwrap()
        .rows
        .is_empty());
    let kept = db
        .query_request(bounded_batch_query(
            SOURCE_REFERENCE_ENTITIES_QUERY,
            &reference_parameters("source_2").unwrap(),
        ))
        .unwrap();
    assert_eq!(kept.rows.len(), 1);
    assert_eq!(
        kept.rows[0]["source_entity_id"],
        Value::String("entity_1".into())
    );
    assert_eq!(
        kept.rows[0]["target_entity_id"],
        Value::String("entity_4".into())
    );
    let nodes = db
        .query_request(bounded_batch_query(
            "MATCH (e:Entity) RETURN count(e) AS total",
            &BTreeMap::new(),
        ))
        .unwrap();
    assert_eq!(nodes.rows[0]["total"], Value::Int(4));
}

#[test]
fn source_reference_relationship_cleanup_rejects_empty_reference_before_wal() {
    let path = unique_test_dir("source_reference_cleanup_empty_before_wal");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Entity {id: 'entity_1'})-[:RELATES_TO {source_reference: 'source_1'}]->(:Entity {id: 'entity_2'})").unwrap();
    }
    let wal_before = read_test_wal(&path).unwrap();
    {
        let db = Database::open(&path).unwrap();
        let epoch = db.commit_epoch().unwrap();
        for reference in ["", " ", "\t\n"] {
            let error = reference_parameters(reference).unwrap_err();
            assert!(error.to_string().contains("non-empty source_reference"));
        }
        assert_eq!(db.commit_epoch().unwrap(), epoch);
    }
    assert_eq!(read_test_wal(&path).unwrap(), wal_before);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn typed_source_reference_relationship_cleanup_persists_as_one_wal_batch_and_replays() {
    let path = unique_test_dir("typed_source_reference_cleanup_wal_replay");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Entity {id: 'entity_1'})-[:RELATES_TO {source_reference: 'source_1'}]->(:Entity {id: 'entity_2'})").unwrap();
        db.query("CREATE (:Entity {id: 'entity_3'})").unwrap();
        let mut parameters = reference_parameters("source_1").unwrap();
        parameters.insert("source".into(), Value::String("entity_1".into()));
        parameters.insert("target".into(), Value::String("entity_3".into()));
        db.query_request(bounded_batch_query(SOURCE_REFERENCE_CREATE, &parameters))
            .unwrap();
    }
    let before = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
    {
        let mut db = Database::open(&path).unwrap();
        let parameters = reference_parameters("source_1").unwrap();
        let mut transaction = db.begin_transaction().unwrap();
        assert_eq!(
            transaction
                .query_request(bounded_batch_query(
                    SOURCE_REFERENCE_ENTITIES_QUERY,
                    &parameters
                ))
                .unwrap()
                .rows
                .len(),
            2
        );
        transaction
            .query_request(bounded_batch_query(SOURCE_REFERENCE_DELETE, &parameters))
            .unwrap();
        assert_eq!(transaction.commit().unwrap().rows.len(), 2);
    }
    let wal = read_test_wal(&path).unwrap();
    assert!(wal.contains("delete_rel"));
    assert_eq!(wal.matches("\tbatch\t").count(), before + 1);
    {
        let mut db = Database::open(&path).unwrap();
        let relationships = db
            .query_request(bounded_batch_query(
                "MATCH (:Entity)-[r:RELATES_TO]->(:Entity) RETURN count(r) AS total",
                &BTreeMap::new(),
            ))
            .unwrap();
        assert_eq!(relationships.rows[0]["total"], Value::Int(0));
        let nodes = db
            .query_request(bounded_batch_query(
                "MATCH (e:Entity) RETURN count(e) AS total",
                &BTreeMap::new(),
            ))
            .unwrap();
        assert_eq!(nodes.rows[0]["total"], Value::Int(3));
    }
    std::fs::remove_dir_all(path).unwrap();
}

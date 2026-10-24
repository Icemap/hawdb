// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;

const SOURCE_MERGE: &str = "MATCH (s:Memory {id: $id}), (m:Memory {id: $memory_id}) \
    MERGE (s)-[r:SYNTHESIZED_FROM]->(m) \
    ON CREATE SET r.weight = $weight, r.occasion_key = $occasion_key, r.created_at = $created_at";
const SOURCE_READ: &str =
    "MATCH (s:Memory {id: $id})-[r:SYNTHESIZED_FROM]->(m:Memory {id: $memory_id}) \
    RETURN id(s) AS crystal_node_id, id(m) AS source_node_id, id(r) AS relationship_id, \
    r.weight AS weight, r.occasion_key AS occasion_key, r.created_at AS created_at";

fn source_parameters(
    crystal: &str,
    source: &str,
    weight: Value,
    created_at: i64,
) -> Result<BTreeMap<String, Value>> {
    if crystal.is_empty() {
        return Err(HawDBError::Semantic(
            "non-empty crystal memory id required".into(),
        ));
    }
    if source.is_empty() {
        return Err(HawDBError::Semantic(
            "non-empty source memory id required".into(),
        ));
    }
    let valid_weight = match &weight {
        Value::Float(value) => value.is_finite(),
        Value::Int(_) => true,
        _ => false,
    };
    if !valid_weight {
        return Err(HawDBError::Semantic(
            "numeric finite weight required".into(),
        ));
    }
    Ok(BTreeMap::from([
        ("id".into(), Value::String(crystal.into())),
        ("memory_id".into(), Value::String(source.into())),
        ("weight".into(), weight),
        ("occasion_key".into(), Value::String(String::new())),
        ("created_at".into(), Value::Int(created_at)),
    ]))
}

#[test]
fn merges_crystal_source_for_mcp_create_crystal_shape() {
    let path = unique_test_dir("crystal_source_query_replay");
    let parameters = source_parameters(
        "crystal-source-1",
        "source-memory-1",
        Value::Float(0.75),
        100,
    )
    .unwrap();
    let relationship_id;
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Memory {id: 'crystal-source-1', is_crystal: true, crystal_title: 'Crystal Source'})")
            .unwrap();
        db.query(
            "CREATE (:Memory {id: 'source-memory-1', is_crystal: false, title: 'Source Memory'})",
        )
        .unwrap();
        let before = db.commit_epoch().unwrap();
        let batches = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        let mut transaction = db.begin_transaction().unwrap();
        assert!(transaction
            .query_request(bounded_batch_query(SOURCE_READ, &parameters))
            .unwrap()
            .rows
            .is_empty());
        transaction
            .query_request(bounded_batch_query(SOURCE_MERGE, &parameters))
            .unwrap();
        let output = transaction
            .query_request(bounded_batch_query(SOURCE_READ, &parameters))
            .unwrap();
        assert_eq!(output.rows.len(), 1);
        assert_eq!(output.rows[0]["crystal_node_id"], Value::Int(0));
        assert_eq!(output.rows[0]["source_node_id"], Value::Int(1));
        relationship_id = output.rows[0]["relationship_id"].clone();
        assert_eq!(output.rows[0]["weight"], Value::Float(0.75));
        assert_eq!(output.rows[0]["occasion_key"], Value::String(String::new()));
        assert_eq!(output.rows[0]["created_at"], Value::Int(100));
        let committed = transaction.commit().unwrap();
        assert_eq!(committed.rows.len(), 1);
        assert_eq!(committed.rows[0]["created"], Value::Bool(true));
        assert_eq!(committed.rows[0]["rel_id"], relationship_id);
        assert_eq!(db.commit_epoch().unwrap(), before + 1);
        assert_eq!(
            read_test_wal(&path).unwrap().matches("\tbatch\t").count(),
            batches + 1
        );
        let wal = read_test_wal(&path).unwrap();
        let second = source_parameters(
            "crystal-source-1",
            "source-memory-1",
            Value::Float(0.25),
            200,
        )
        .unwrap();
        let mut transaction = db.begin_transaction().unwrap();
        transaction
            .query_request(bounded_batch_query(SOURCE_MERGE, &second))
            .unwrap();
        let unchanged = transaction
            .query_request(bounded_batch_query(SOURCE_READ, &second))
            .unwrap();
        assert_eq!(unchanged.rows, output.rows);
        let committed = transaction.commit().unwrap();
        assert_eq!(committed.rows.len(), 1);
        assert_eq!(committed.rows[0]["created"], Value::Bool(false));
        assert_eq!(committed.rows[0]["rel_id"], relationship_id);
        assert_eq!(db.commit_epoch().unwrap(), before + 1);
        assert_eq!(read_test_wal(&path).unwrap(), wal);
    }
    {
        let mut db = Database::open(&path).unwrap();
        let output = db
            .query_request(bounded_batch_query(SOURCE_READ, &parameters))
            .unwrap();
        assert_eq!(output.rows.len(), 1);
        assert_eq!(output.rows[0]["relationship_id"], relationship_id);
        assert_eq!(output.rows[0]["crystal_node_id"], Value::Int(0));
        assert_eq!(output.rows[0]["source_node_id"], Value::Int(1));
        assert_eq!(output.rows[0]["weight"], Value::Float(0.75));
        assert_eq!(output.rows[0]["occasion_key"], Value::String(String::new()));
        assert_eq!(output.rows[0]["created_at"], Value::Int(100));
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn crystal_source_merge_reports_missing_endpoint_and_rejects_invalid_inputs() {
    let path = unique_test_dir("crystal_source_query_missing_endpoint");
    {
        let mut db = Database::open(&path).unwrap();
        assert!(source_parameters("", "source-memory-1", Value::Int(1), 100)
            .unwrap_err()
            .to_string()
            .contains("non-empty crystal memory id"));
        assert!(
            source_parameters("crystal-source-1", "", Value::Int(1), 100)
                .unwrap_err()
                .to_string()
                .contains("non-empty source memory id")
        );
        db.query("CREATE (:Memory {id: 'crystal-source-1', is_crystal: true, crystal_title: 'Crystal Source'})")
            .unwrap();
        let before = db.commit_epoch().unwrap();
        let wal = read_test_wal(&path).unwrap();
        for weight in [
            Value::String("heavy".into()),
            Value::Float(f64::NAN),
            Value::Float(f64::INFINITY),
        ] {
            assert!(
                source_parameters("crystal-source-1", "source-memory-1", weight, 100)
                    .unwrap_err()
                    .to_string()
                    .contains("numeric finite weight")
            );
        }
        assert_eq!(db.commit_epoch().unwrap(), before);
        assert_eq!(read_test_wal(&path).unwrap(), wal);
        let parameters =
            source_parameters("crystal-source-1", "missing-memory", Value::Int(1), 100).unwrap();
        let mut transaction = db.begin_transaction().unwrap();
        let crystal = transaction
            .query_request(bounded_batch_query(
                "MATCH (m:Memory {id: $id}) RETURN id(m) AS node_id",
                &parameters,
            ))
            .unwrap();
        assert_eq!(crystal.rows.len(), 1);
        assert_eq!(crystal.rows[0]["node_id"], Value::Int(0));
        transaction
            .query_request(bounded_batch_query(SOURCE_MERGE, &parameters))
            .unwrap();
        assert!(transaction
            .query_request(bounded_batch_query(SOURCE_READ, &parameters))
            .unwrap()
            .rows
            .is_empty());
        assert!(transaction.commit().unwrap().rows.is_empty());
        assert_eq!(db.commit_epoch().unwrap(), before);
        assert_eq!(read_test_wal(&path).unwrap(), wal);
    }
    std::fs::remove_dir_all(path).unwrap();
}

// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;

const MERGE: &str = "MATCH (s:Source {id: $source}), (l:Label {id: $label}) \
    MERGE (s)-[r:HAS_LABEL]->(l) ON CREATE SET r.assigned_by = $assigned_by, \
    r.created_at = $created_at, r.properties = $properties";
const DELETE: &str =
    "MATCH (s:Source {id: $source})-[r:HAS_LABEL]->(l:Label {id: $label}) DELETE r";
const PAIR_READ: &str = "MATCH (s:Source {id: $source})-[r:HAS_LABEL]->(l:Label {id: $label}) \
    RETURN id(r) AS relationship_id, r.assigned_by AS assigned_by, r.created_at AS created_at, r.properties AS properties";
const ALL_READ: &str = "MATCH (s:Source)-[r:HAS_LABEL]->(l:Label) \
    RETURN s.id AS source, l.id AS label, r.assigned_by AS assigned_by, \
    r.created_at AS created_at, r.properties AS properties ORDER BY source, label";
const ENDPOINTS: &str =
    "MATCH (s:Source {id: $source}), (l:Label {id: $label}) RETURN s.id AS source, l.id AS label";
const IDLESS: &str = "MATCH (s:Source {original_name: $name}) RETURN id(s) AS node_id, s AS source";

fn label_parameters(source: &str, label: &str) -> Result<BTreeMap<String, Value>> {
    if source.is_empty() {
        return Err(HawDBError::Semantic("non-empty source id required".into()));
    }
    if label.is_empty() {
        return Err(HawDBError::Semantic("non-empty label id required".into()));
    }
    Ok(BTreeMap::from([
        ("source".into(), Value::String(source.into())),
        ("label".into(), Value::String(label.into())),
    ]))
}

fn assignment_parameters(
    source: &str,
    label: &str,
    assigned_by: &str,
    created_at: i64,
    properties: &str,
) -> Result<BTreeMap<String, Value>> {
    let mut parameters = label_parameters(source, label)?;
    if assigned_by.is_empty() {
        return Err(HawDBError::Semantic(
            "non-empty assigned_by required".into(),
        ));
    }
    parameters.extend([
        ("assigned_by".into(), Value::String(assigned_by.into())),
        ("created_at".into(), Value::Int(created_at)),
        ("properties".into(), Value::String(properties.into())),
    ]);
    Ok(parameters)
}

#[test]
fn assigns_source_labels_for_nowledge_merge_shape() {
    let mut db = Database::new();
    db.query("CREATE (:Source {id: 'source_1', original_name: 'First'})")
        .unwrap();
    db.query("CREATE (:Source {id: 'source_2', original_name: 'Second'})")
        .unwrap();
    db.query("CREATE (:Label {id: 'label_1', name: 'First Label'})")
        .unwrap();
    db.query("CREATE (:Label {id: 'label_2', name: 'Second Label'})")
        .unwrap();
    db.query("CREATE (:Source {original_name: 'Idless Source'})")
        .unwrap();
    db.query("MATCH (s:Source {id: 'source_2'}), (l:Label {id: 'label_2'}) CREATE (s)-[:HAS_LABEL {assigned_by: 'existing'}]->(l)").unwrap();
    let idless_parameters =
        BTreeMap::from([("name".into(), Value::String("Idless Source".into()))]);
    let idless = db
        .query_request(bounded_batch_query(IDLESS, &idless_parameters))
        .unwrap();
    let idless_id = match idless.rows[0]["node_id"] {
        Value::Int(id) => id.to_string(),
        ref value => panic!("unexpected node id {value:?}"),
    };
    let existing_parameters = label_parameters("source_2", "label_2").unwrap();
    let existing = db
        .query_request(bounded_batch_query(PAIR_READ, &existing_parameters))
        .unwrap();
    let epoch = db.commit_epoch().unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    for (source, label, assigned_by, timestamp, properties, expected) in [
        (
            "source_1",
            "label_1",
            "system",
            987,
            "{\"scope\":\"source\"}",
            1,
        ),
        (
            "source_2",
            "label_2",
            "system",
            988,
            "{\"scope\":\"existing\"}",
            1,
        ),
        ("missing", "label_1", "system", 989, "{}", 0),
        (idless_id.as_str(), "label_1", "system", 990, "{}", 0),
        (
            "source_1",
            "label_1",
            "duplicate",
            991,
            "{\"duplicate\":true}",
            1,
        ),
    ] {
        let parameters =
            assignment_parameters(source, label, assigned_by, timestamp, properties).unwrap();
        assert_eq!(
            transaction
                .query_request(bounded_batch_query(ENDPOINTS, &parameters))
                .unwrap()
                .rows
                .len(),
            expected
        );
        transaction
            .query_request(bounded_batch_query(MERGE, &parameters))
            .unwrap();
    }
    let committed = transaction.commit().unwrap();
    assert_eq!(committed.rows.len(), 3);
    for (row, created) in committed.rows.iter().zip([true, false, false]) {
        assert_eq!(row.get("created"), Some(&Value::Bool(created)));
    }
    assert_eq!(db.commit_epoch().unwrap(), epoch + 1);
    let rows = db
        .query_request(bounded_batch_query(ALL_READ, &BTreeMap::new()))
        .unwrap();
    assert_eq!(rows.rows.len(), 2);
    assert_eq!(rows.rows[0]["source"], Value::String("source_1".into()));
    assert_eq!(rows.rows[0]["label"], Value::String("label_1".into()));
    assert_eq!(rows.rows[0]["assigned_by"], Value::String("system".into()));
    assert_eq!(rows.rows[0]["created_at"], Value::Int(987));
    assert_eq!(
        rows.rows[0]["properties"],
        Value::String("{\"scope\":\"source\"}".into())
    );
    assert_eq!(
        rows.rows[1]["assigned_by"],
        Value::String("existing".into())
    );
    assert_eq!(rows.rows[1]["properties"], Value::Null);
    assert_eq!(
        db.query_request(bounded_batch_query(PAIR_READ, &existing_parameters))
            .unwrap()
            .rows,
        existing.rows
    );
    assert_eq!(
        db.query_request(bounded_batch_query(IDLESS, &idless_parameters))
            .unwrap()
            .rows,
        idless.rows
    );
}

#[test]
fn source_label_assignment_rejects_empty_fields_before_wal() {
    let path = unique_test_dir("source_label_assignment_invalid_query_inputs");
    let mut db = Database::open(&path).unwrap();
    db.query("CREATE (:Source {id: 'source_1'})").unwrap();
    db.query("CREATE (:Label {id: 'label_1'})").unwrap();
    let epoch = db.commit_epoch().unwrap();
    let wal = read_test_wal(&path).unwrap();
    for (source, label, assigned_by, message) in [
        ("source_1", "label_1", "", "non-empty assigned_by"),
        ("", "label_1", "system", "non-empty source id"),
        ("source_1", "", "system", "non-empty label id"),
    ] {
        let error = assignment_parameters(source, label, assigned_by, 1, "{}").unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
    }
    assert_eq!(db.commit_epoch().unwrap(), epoch);
    assert_eq!(read_test_wal(&path).unwrap(), wal);
    drop(db);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn typed_source_label_assignment_persists_as_one_wal_batch_and_replays() {
    let path = unique_test_dir("typed_source_label_assignment_wal_replay");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Source {id: 'source_1'})").unwrap();
        db.query("CREATE (:Source {id: 'source_2'})").unwrap();
        db.query("CREATE (:Label {id: 'label_1'})").unwrap();
        db.query("CREATE (:Label {id: 'label_2'})").unwrap();
        let before = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        let mut transaction = db.begin_transaction().unwrap();
        for (source, label, timestamp, properties) in [
            ("source_1", "label_1", 1, "{}"),
            ("source_2", "label_2", 2, "{\"scope\":\"source\"}"),
        ] {
            let parameters =
                assignment_parameters(source, label, "system", timestamp, properties).unwrap();
            transaction
                .query_request(bounded_batch_query(MERGE, &parameters))
                .unwrap();
        }
        let committed = transaction.commit().unwrap();
        assert_eq!(committed.rows.len(), 2);
        assert!(committed
            .rows
            .iter()
            .all(|row| row.get("created") == Some(&Value::Bool(true))));
        assert_eq!(
            read_test_wal(&path).unwrap().matches("\tbatch\t").count(),
            before + 1
        );
    }
    assert!(read_test_wal(&path).unwrap().contains("create_rel"));
    {
        let mut db = Database::open(&path).unwrap();
        let rows = db
            .query_request(bounded_batch_query(ALL_READ, &BTreeMap::new()))
            .unwrap();
        assert_eq!(rows.rows.len(), 2);
        for (index, source, label, timestamp, properties) in [
            (0, "source_1", "label_1", 1, "{}"),
            (1, "source_2", "label_2", 2, "{\"scope\":\"source\"}"),
        ] {
            assert_eq!(rows.rows[index]["source"], Value::String(source.into()));
            assert_eq!(rows.rows[index]["label"], Value::String(label.into()));
            assert_eq!(
                rows.rows[index]["assigned_by"],
                Value::String("system".into())
            );
            assert_eq!(rows.rows[index]["created_at"], Value::Int(timestamp));
            assert_eq!(
                rows.rows[index]["properties"],
                Value::String(properties.into())
            );
        }
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn deletes_source_labels_for_nowledge_delete_shape() {
    let mut db = Database::new();
    db.query("CREATE (:Source {id: 'source_1'})").unwrap();
    db.query("CREATE (:Source {id: 'source_2'})").unwrap();
    db.query("CREATE (:Label {id: 'label_1'})").unwrap();
    db.query("CREATE (:Label {id: 'label_2'})").unwrap();
    db.query("CREATE (:Source {original_name: 'Idless Source'})")
        .unwrap();
    for (source, label) in [
        ("source_1", "label_1"),
        ("source_1", "label_2"),
        ("source_2", "label_1"),
    ] {
        let parameters = label_parameters(source, label).unwrap();
        db.query_request(bounded_batch_query(
            "MATCH (s:Source {id: $source}), (l:Label {id: $label}) CREATE (s)-[:HAS_LABEL]->(l)",
            &parameters,
        ))
        .unwrap();
    }
    let idless_parameters =
        BTreeMap::from([("name".into(), Value::String("Idless Source".into()))]);
    let idless = db
        .query_request(bounded_batch_query(IDLESS, &idless_parameters))
        .unwrap();
    let idless_id = match idless.rows[0]["node_id"] {
        Value::Int(id) => id.to_string(),
        ref value => panic!("unexpected node id {value:?}"),
    };
    let epoch = db.commit_epoch().unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    for (source, label, expected) in [
        ("source_1", "label_1", 1),
        ("missing", "label_1", 0),
        (idless_id.as_str(), "label_1", 0),
        ("source_1", "missing", 0),
        ("source_2", "label_1", 1),
    ] {
        let parameters = label_parameters(source, label).unwrap();
        assert_eq!(
            transaction
                .query_request(bounded_batch_query(PAIR_READ, &parameters))
                .unwrap()
                .rows
                .len(),
            expected
        );
        assert!(transaction
            .query_request(bounded_batch_query(DELETE, &parameters))
            .unwrap()
            .rows
            .is_empty());
        assert!(transaction
            .query_request(bounded_batch_query(PAIR_READ, &parameters))
            .unwrap()
            .rows
            .is_empty());
    }
    assert_eq!(transaction.commit().unwrap().rows.len(), 2);
    assert_eq!(db.commit_epoch().unwrap(), epoch + 1);
    let rows = db
        .query_request(bounded_batch_query(ALL_READ, &BTreeMap::new()))
        .unwrap();
    assert_eq!(rows.rows.len(), 1);
    assert_eq!(rows.rows[0]["source"], Value::String("source_1".into()));
    assert_eq!(rows.rows[0]["label"], Value::String("label_2".into()));
    assert_eq!(
        db.query_request(bounded_batch_query(IDLESS, &idless_parameters))
            .unwrap()
            .rows,
        idless.rows
    );
}

#[test]
fn source_label_delete_rejects_empty_fields_before_wal() {
    let path = unique_test_dir("source_label_delete_invalid_query_inputs");
    let mut db = Database::open(&path).unwrap();
    db.query("CREATE (:Source {id: 'source_1'})").unwrap();
    db.query("CREATE (:Label {id: 'label_1'})").unwrap();
    let epoch = db.commit_epoch().unwrap();
    let wal = read_test_wal(&path).unwrap();
    for (source, label, message) in [
        ("", "label_1", "non-empty source id"),
        ("source_1", "", "non-empty label id"),
    ] {
        let error = label_parameters(source, label).unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
    }
    assert_eq!(db.commit_epoch().unwrap(), epoch);
    assert_eq!(read_test_wal(&path).unwrap(), wal);
    drop(db);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn typed_source_label_delete_persists_as_one_wal_batch_and_replays() {
    let path = unique_test_dir("typed_source_label_delete_wal_replay");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Source {id: 'source_1'})").unwrap();
        db.query("CREATE (:Source {id: 'source_2'})").unwrap();
        db.query("CREATE (:Label {id: 'label_1'})").unwrap();
        db.query("CREATE (:Label {id: 'label_2'})").unwrap();
        db.query("MATCH (s:Source {id: 'source_1'}), (l:Label {id: 'label_1'}) CREATE (s)-[:HAS_LABEL]->(l)").unwrap();
        db.query("MATCH (s:Source {id: 'source_2'}), (l:Label {id: 'label_2'}) CREATE (s)-[:HAS_LABEL]->(l)").unwrap();
        let before = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        let mut transaction = db.begin_transaction().unwrap();
        for (source, label) in [("source_1", "label_1"), ("source_2", "label_2")] {
            let parameters = label_parameters(source, label).unwrap();
            transaction
                .query_request(bounded_batch_query(DELETE, &parameters))
                .unwrap();
        }
        assert_eq!(transaction.commit().unwrap().rows.len(), 2);
        assert_eq!(
            read_test_wal(&path).unwrap().matches("\tbatch\t").count(),
            before + 1
        );
    }
    assert!(read_test_wal(&path).unwrap().contains("delete_rel"));
    {
        let mut db = Database::open(&path).unwrap();
        assert!(db
            .query_request(bounded_batch_query(ALL_READ, &BTreeMap::new()))
            .unwrap()
            .rows
            .is_empty());
        for query in [
            "MATCH (s:Source) RETURN count(s) AS total",
            "MATCH (l:Label) RETURN count(l) AS total",
        ] {
            let rows = db
                .query_request(bounded_batch_query(query, &BTreeMap::new()))
                .unwrap();
            assert_eq!(rows.rows[0]["total"], Value::Int(2));
        }
    }
    std::fs::remove_dir_all(path).unwrap();
}

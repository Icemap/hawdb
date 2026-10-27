// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;

const LIFECYCLE_UPDATE: &str = "MATCH (m:Memory {id: $id}) \
    SET m.metadata = $metadata, m.is_latest = $latest, \
    m.lifecycle_state = $state, m.updated_at = $updated_at RETURN id(m) AS node_id";
const LIFECYCLE_READ: &str = "MATCH (m:Memory) RETURN m.id AS id, m.metadata AS metadata, \
    m.is_latest AS latest, m.lifecycle_state AS state, m.updated_at AS updated_at \
    ORDER BY id";
const LATEST_UPDATE: &str =
    "MATCH (m:Memory {id: $id}) SET m.is_latest = $latest RETURN id(m) AS node_id";
const SCOPED_LATEST_UPDATE: &str = "MATCH (m:Memory {id: $id}) WHERE m.space_id = $space \
    SET m.is_latest = $latest RETURN id(m) AS node_id";

fn memory_parameters(id: &str) -> Result<BTreeMap<String, Value>> {
    if id.is_empty() {
        return Err(HawDBError::Semantic("non-empty memory id required".into()));
    }
    Ok(BTreeMap::from([("id".into(), Value::String(id.into()))]))
}

fn lifecycle_parameters(
    id: &str,
    metadata: &str,
    latest: bool,
    state: &str,
    timestamp: &str,
) -> Result<BTreeMap<String, Value>> {
    let mut parameters = memory_parameters(id)?;
    if state.is_empty() {
        return Err(HawDBError::Semantic(
            "non-empty lifecycle state required".into(),
        ));
    }
    parameters.extend([
        ("metadata".into(), Value::String(metadata.into())),
        ("latest".into(), Value::Bool(latest)),
        ("state".into(), Value::String(state.into())),
        ("updated_at".into(), Value::String(timestamp.into())),
    ]);
    Ok(parameters)
}

#[test]
fn updates_memory_lifecycle_batch_for_metadata_state() {
    let mut db = Database::new();
    db.query("CREATE (:Memory {id: 'memory_1', metadata: '{}', is_latest: true, lifecycle_state: 'active', updated_at: 1})").unwrap();
    db.query("CREATE (:Memory {id: 'memory_2', metadata: '{}', is_latest: true, lifecycle_state: 'active', updated_at: 1})").unwrap();
    let before = db.commit_epoch().unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    let mut seen = BTreeSet::new();
    let mut counts = Vec::new();
    let mut duplicates = 0;
    for (id, metadata, latest, state, timestamp) in [
        (
            "memory_1",
            "{\"archived\":true}",
            false,
            "archived",
            "2026-07-19T13:00:00Z",
        ),
        (
            "memory_2",
            "{\"reviewed\":true}",
            true,
            "active",
            "2026-07-19T13:01:00Z",
        ),
        (
            "memory_2",
            "{\"duplicate\":true}",
            false,
            "archived",
            "2026-07-19T13:02:00Z",
        ),
        ("missing", "{}", false, "archived", "2026-07-19T13:03:00Z"),
    ] {
        let parameters = lifecycle_parameters(id, metadata, latest, state, timestamp).unwrap();
        if !seen.insert(id) {
            duplicates += 1;
            continue;
        }
        let output = transaction
            .query_request(bounded_batch_query(LIFECYCLE_UPDATE, &parameters))
            .unwrap();
        counts.push(output.rows.len());
    }
    assert_eq!(counts, [1, 1, 0]);
    assert_eq!(duplicates, 1);
    transaction.commit().unwrap();
    assert_eq!(before, 2);
    assert_eq!(db.commit_epoch().unwrap(), before + 1);
    let rows = db
        .query_request(bounded_batch_query(LIFECYCLE_READ, &BTreeMap::new()))
        .unwrap();
    assert_eq!(rows.rows.len(), 2);
    assert_eq!(rows.rows[0]["id"], Value::String("memory_1".into()));
    assert_eq!(
        rows.rows[0]["metadata"],
        Value::String("{\"archived\":true}".into())
    );
    assert_eq!(rows.rows[0]["latest"], Value::Bool(false));
    assert_eq!(rows.rows[0]["state"], Value::String("archived".into()));
    assert_eq!(
        rows.rows[0]["updated_at"],
        Value::String("2026-07-19T13:00:00Z".into())
    );
    assert_eq!(rows.rows[1]["id"], Value::String("memory_2".into()));
    assert_eq!(
        rows.rows[1]["metadata"],
        Value::String("{\"reviewed\":true}".into())
    );
    assert_eq!(rows.rows[1]["latest"], Value::Bool(true));
    assert_eq!(rows.rows[1]["state"], Value::String("active".into()));
    assert_eq!(
        rows.rows[1]["updated_at"],
        Value::String("2026-07-19T13:01:00Z".into())
    );
}

#[test]
fn memory_lifecycle_batch_rejects_empty_state_before_wal() {
    let path = unique_test_dir("memory_lifecycle_empty_state_query");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Memory {id: 'memory_1', lifecycle_state: 'active'})")
            .unwrap();
        let before = db.commit_epoch().unwrap();
        let wal_before = read_test_wal(&path).unwrap();
        let error =
            lifecycle_parameters("memory_1", "{}", true, "", "2026-07-19T13:00:00Z").unwrap_err();
        assert!(error.to_string().contains("non-empty lifecycle state"));
        assert_eq!(db.commit_epoch().unwrap(), before);
        assert_eq!(read_test_wal(&path).unwrap(), wal_before);
        let rows = db
            .query_request(bounded_batch_query(LIFECYCLE_READ, &BTreeMap::new()))
            .unwrap();
        assert_eq!(rows.rows[0]["state"], Value::String("active".into()));
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn memory_lifecycle_queries_persist_as_one_wal_batch_and_replay() {
    let path = unique_test_dir("memory_lifecycle_query_wal_replay");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Memory {id: 'memory_1', metadata: '{}', is_latest: true, lifecycle_state: 'active'})").unwrap();
        db.query("CREATE (:Memory {id: 'memory_2', metadata: '{}', is_latest: true, lifecycle_state: 'active'})").unwrap();
        let batches_before = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        let mut transaction = db.begin_transaction().unwrap();
        for (id, metadata, latest, state, timestamp) in [
            (
                "memory_1",
                "{\"state\":\"archived\"}",
                false,
                "archived",
                "2026-07-19T13:00:00Z",
            ),
            (
                "memory_2",
                "{\"state\":\"active\"}",
                true,
                "active",
                "2026-07-19T13:01:00Z",
            ),
        ] {
            let parameters = lifecycle_parameters(id, metadata, latest, state, timestamp).unwrap();
            let output = transaction
                .query_request(bounded_batch_query(LIFECYCLE_UPDATE, &parameters))
                .unwrap();
            assert_eq!(output.rows.len(), 1);
        }
        transaction.commit().unwrap();
        assert_eq!(
            read_test_wal(&path).unwrap().matches("\tbatch\t").count(),
            batches_before + 1
        );
    }
    assert!(read_test_wal(&path).unwrap().contains("set_node_property"));
    {
        let mut db = Database::open(&path).unwrap();
        let rows = db
            .query_request(bounded_batch_query(LIFECYCLE_READ, &BTreeMap::new()))
            .unwrap();
        assert_eq!(rows.rows.len(), 2);
        assert_eq!(rows.rows[0]["id"], Value::String("memory_1".into()));
        assert_eq!(
            rows.rows[0]["metadata"],
            Value::String("{\"state\":\"archived\"}".into())
        );
        assert_eq!(rows.rows[0]["latest"], Value::Bool(false));
        assert_eq!(rows.rows[0]["state"], Value::String("archived".into()));
        assert_eq!(
            rows.rows[0]["updated_at"],
            Value::String("2026-07-19T13:00:00Z".into())
        );
        assert_eq!(rows.rows[1]["id"], Value::String("memory_2".into()));
        assert_eq!(
            rows.rows[1]["metadata"],
            Value::String("{\"state\":\"active\"}".into())
        );
        assert_eq!(rows.rows[1]["latest"], Value::Bool(true));
        assert_eq!(rows.rows[1]["state"], Value::String("active".into()));
        assert_eq!(
            rows.rows[1]["updated_at"],
            Value::String("2026-07-19T13:01:00Z".into())
        );
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn updates_memory_latest_batch_for_nowledge_evolution_shapes() {
    let mut db = Database::new();
    db.query("CREATE (:Memory {id: 'older', is_latest: true, space_id: 'space_a', metadata: '{}', lifecycle_state: 'active'})").unwrap();
    db.query("CREATE (:Memory {id: 'newer', is_latest: false, space_id: 'space_a', metadata: '{}', lifecycle_state: 'active'})").unwrap();
    db.query("CREATE (:Memory {id: 'other_space', is_latest: true, space_id: 'space_b'})")
        .unwrap();
    let before = db.commit_epoch().unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    let mut seen = BTreeSet::new();
    let mut counts = Vec::new();
    let mut duplicates = 0;
    for (id, latest, space) in [
        ("older", false, Some("space_a")),
        ("newer", true, None),
        ("other_space", false, Some("space_a")),
        ("older", true, None),
        ("missing", true, None),
    ] {
        let mut parameters = memory_parameters(id).unwrap();
        parameters.insert("latest".into(), Value::Bool(latest));
        if !seen.insert(id) {
            duplicates += 1;
            continue;
        }
        let query = if let Some(space) = space {
            parameters.insert("space".into(), Value::String(space.into()));
            SCOPED_LATEST_UPDATE
        } else {
            LATEST_UPDATE
        };
        let output = transaction
            .query_request(bounded_batch_query(query, &parameters))
            .unwrap();
        counts.push(output.rows.len());
    }
    assert_eq!(counts, [1, 1, 0, 0]);
    assert_eq!(duplicates, 1);
    transaction.commit().unwrap();
    assert_eq!(before, 3);
    assert_eq!(db.commit_epoch().unwrap(), before + 1);
    let rows = db
        .query_request(bounded_batch_query(LIFECYCLE_READ, &BTreeMap::new()))
        .unwrap();
    assert_eq!(rows.rows.len(), 3);
    assert_eq!(rows.rows[0]["id"], Value::String("newer".into()));
    assert_eq!(rows.rows[0]["latest"], Value::Bool(true));
    assert_eq!(rows.rows[1]["id"], Value::String("older".into()));
    assert_eq!(rows.rows[1]["latest"], Value::Bool(false));
    assert_eq!(rows.rows[1]["metadata"], Value::String("{}".into()));
    assert_eq!(rows.rows[1]["state"], Value::String("active".into()));
    assert_eq!(rows.rows[2]["id"], Value::String("other_space".into()));
    assert_eq!(rows.rows[2]["latest"], Value::Bool(true));
}

#[test]
fn memory_latest_batch_rejects_empty_id_before_wal() {
    let path = unique_test_dir("memory_latest_empty_id");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Memory {id: 'memory_1', is_latest: true})")
            .unwrap();
        let before = db.commit_epoch().unwrap();
        let wal_before = read_test_wal(&path).unwrap();
        let error = memory_parameters("").unwrap_err();
        assert!(error.to_string().contains("non-empty memory id"));
        assert_eq!(db.commit_epoch().unwrap(), before);
        assert_eq!(read_test_wal(&path).unwrap(), wal_before);
        let rows = db
            .query_request(bounded_batch_query(LIFECYCLE_READ, &BTreeMap::new()))
            .unwrap();
        assert_eq!(rows.rows[0]["latest"], Value::Bool(true));
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn memory_latest_queries_persist_as_one_wal_batch_and_replay() {
    let path = unique_test_dir("memory_latest_query_wal_replay");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Memory {id: 'older', is_latest: true, space_id: 'space_a'})")
            .unwrap();
        db.query("CREATE (:Memory {id: 'newer', is_latest: false, space_id: 'space_a'})")
            .unwrap();
        let batches_before = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        let mut transaction = db.begin_transaction().unwrap();
        for (id, latest, space) in [("older", false, Some("space_a")), ("newer", true, None)] {
            let mut parameters = memory_parameters(id).unwrap();
            parameters.insert("latest".into(), Value::Bool(latest));
            let query = if let Some(space) = space {
                parameters.insert("space".into(), Value::String(space.into()));
                SCOPED_LATEST_UPDATE
            } else {
                LATEST_UPDATE
            };
            let output = transaction
                .query_request(bounded_batch_query(query, &parameters))
                .unwrap();
            assert_eq!(output.rows.len(), 1);
        }
        transaction.commit().unwrap();
        assert_eq!(
            read_test_wal(&path).unwrap().matches("\tbatch\t").count(),
            batches_before + 1
        );
    }
    assert!(read_test_wal(&path).unwrap().contains("set_node_property"));
    {
        let mut db = Database::open(&path).unwrap();
        let rows = db
            .query_request(bounded_batch_query(LIFECYCLE_READ, &BTreeMap::new()))
            .unwrap();
        assert_eq!(rows.rows.len(), 2);
        assert_eq!(rows.rows[0]["id"], Value::String("newer".into()));
        assert_eq!(rows.rows[0]["latest"], Value::Bool(true));
        assert_eq!(rows.rows[1]["id"], Value::String("older".into()));
        assert_eq!(rows.rows[1]["latest"], Value::Bool(false));
    }
    std::fs::remove_dir_all(path).unwrap();
}

const EVOLVES_CREATE: &str = "MATCH (older:Memory {id: $older}), (newer:Memory {id: $newer}) \
    CREATE (older)-[:EVOLVES {content_relation: $relation, created_at: $created_at}]->(newer)";
const EVOLVES_PROGRESSION_CREATE: &str =
    "MATCH (older:Memory {id: $older}), (newer:Memory {id: $newer}) \
    CREATE (older)-[:EVOLVES {content_relation: $relation, created_at: $created_at, \
    is_progression: $progression, confidence: $confidence, detected_by: $detected_by, \
    reviewed: $reviewed, reason: $reason}]->(newer)";
const EVOLVES_READ: &str = "MATCH (older:Memory {id: $older})-[r:EVOLVES]->(newer:Memory {id: $newer}) \
    RETURN id(older) AS older_node_id, id(newer) AS newer_node_id, \
    r.content_relation AS relation, r.created_at AS created_at, r.is_progression AS progression, \
    r.confidence AS confidence, r.detected_by AS detected_by, r.reviewed AS reviewed, r.reason AS reason";

fn evolves_parameters(
    older: &str,
    newer: &str,
    relation: &str,
    created_at: i64,
) -> BTreeMap<String, Value> {
    BTreeMap::from([
        ("older".into(), Value::String(older.into())),
        ("newer".into(), Value::String(newer.into())),
        ("relation".into(), Value::String(relation.into())),
        ("created_at".into(), Value::Int(created_at)),
    ])
}

fn validate_evolves_parameters(parameters: &BTreeMap<String, Value>) -> Result<()> {
    for (key, name) in [
        ("older", "older memory id"),
        ("newer", "newer memory id"),
        ("relation", "content relation"),
    ] {
        if !matches!(parameters.get(key), Some(Value::String(value)) if !value.is_empty()) {
            return Err(HawDBError::Semantic(format!("non-empty {name} required")));
        }
    }
    if let Some(value) = parameters.get("confidence") {
        let valid = match value {
            Value::Int(_) => true,
            Value::Float(number) => number.is_finite(),
            _ => false,
        };
        if !valid {
            return Err(HawDBError::Semantic(
                "numeric finite confidence required".into(),
            ));
        }
    }
    if matches!(parameters.get("detected_by"), Some(Value::String(value)) if value.is_empty()) {
        return Err(HawDBError::Semantic(
            "non-empty detected_by required".into(),
        ));
    }
    Ok(())
}

#[test]
fn creates_memory_evolves_batch_for_nowledge_shapes() {
    let mut db = Database::new();
    for id in [
        "older_basic",
        "newer_basic",
        "older_progression",
        "newer_progression",
    ] {
        let parameters = memory_parameters(id).unwrap();
        db.query_request(bounded_batch_query(
            "CREATE (:Memory {id: $id})",
            &parameters,
        ))
        .unwrap();
    }
    db.query("CREATE (:Memory {title: 'Idless Memory'})")
        .unwrap();
    let basic = evolves_parameters("older_basic", "newer_basic", "confirms", 100);
    let mut progression =
        evolves_parameters("older_progression", "newer_progression", "replaces", 200);
    progression.extend([
        ("progression".into(), Value::Bool(true)),
        ("confidence".into(), Value::Float(0.87)),
        ("detected_by".into(), Value::String("scheduler".into())),
        ("reviewed".into(), Value::Bool(false)),
        ("reason".into(), Value::String("replacement".into())),
    ]);
    let missing = evolves_parameters("older_basic", "missing", "supersedes", 300);
    let idless = evolves_parameters("4", "newer_basic", "projected", 400);
    let operations = [
        (EVOLVES_CREATE, &basic),
        (EVOLVES_PROGRESSION_CREATE, &progression),
        (EVOLVES_CREATE, &missing),
        (EVOLVES_CREATE, &idless),
    ];
    for (_, parameters) in operations {
        validate_evolves_parameters(parameters).unwrap();
    }
    let before = db.commit_epoch().unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    let mut counts = Vec::new();
    for (query, parameters) in operations {
        assert!(transaction
            .query_request(bounded_batch_query(EVOLVES_READ, parameters))
            .unwrap()
            .rows
            .is_empty());
        transaction
            .query_request(bounded_batch_query(query, parameters))
            .unwrap();
        let after = transaction
            .query_request(bounded_batch_query(EVOLVES_READ, parameters))
            .unwrap();
        counts.push(after.rows.len());
    }
    assert_eq!(counts, [1, 1, 0, 0]);
    assert_eq!(transaction.commit().unwrap().rows.len(), 2);
    assert_eq!(before, 5);
    assert_eq!(db.commit_epoch().unwrap(), before + 1);
    let output = db
        .query_request(bounded_batch_query(EVOLVES_READ, &basic))
        .unwrap();
    assert_eq!(output.rows.len(), 1);
    let row = &output.rows[0];
    assert_eq!(row["older_node_id"], Value::Int(0));
    assert_eq!(row["newer_node_id"], Value::Int(1));
    assert_eq!(row["relation"], Value::String("confirms".into()));
    assert_eq!(row["created_at"], Value::Int(100));
    for key in [
        "progression",
        "confidence",
        "detected_by",
        "reviewed",
        "reason",
    ] {
        assert_eq!(row[key], Value::Null);
    }
    let output = db
        .query_request(bounded_batch_query(EVOLVES_READ, &progression))
        .unwrap();
    assert_eq!(output.rows.len(), 1);
    let row = &output.rows[0];
    assert_eq!(row["older_node_id"], Value::Int(2));
    assert_eq!(row["newer_node_id"], Value::Int(3));
    assert_eq!(row["relation"], Value::String("replaces".into()));
    assert_eq!(row["created_at"], Value::Int(200));
    assert_eq!(row["progression"], Value::Bool(true));
    assert_eq!(row["confidence"], Value::Float(0.87));
    assert_eq!(row["detected_by"], Value::String("scheduler".into()));
    assert_eq!(row["reviewed"], Value::Bool(false));
    assert_eq!(row["reason"], Value::String("replacement".into()));
    let retained = db
        .query_request(bounded_batch_query(
            "MATCH (m:Memory) WHERE m.id IS NULL RETURN id(m) AS node_id, m.title AS title",
            &BTreeMap::new(),
        ))
        .unwrap();
    assert_eq!(retained.rows.len(), 1);
    assert_eq!(retained.rows[0]["node_id"], Value::Int(4));
    assert_eq!(
        retained.rows[0]["title"],
        Value::String("Idless Memory".into())
    );
    let count = db
        .query_request(bounded_batch_query(
            "MATCH (:Memory)-[r:EVOLVES]->(:Memory) RETURN count(r) AS total",
            &BTreeMap::new(),
        ))
        .unwrap();
    assert_eq!(count.rows[0]["total"], Value::Int(2));
}

#[test]
fn memory_evolves_create_rejects_empty_fields_before_wal() {
    let path = unique_test_dir("memory_evolves_empty_fields");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Memory {id: 'older'})").unwrap();
        db.query("CREATE (:Memory {id: 'newer'})").unwrap();
        let before = db.commit_epoch().unwrap();
        let wal_before = read_test_wal(&path).unwrap();
        for (key, name) in [
            ("older", "older memory id"),
            ("newer", "newer memory id"),
            ("relation", "content relation"),
            ("detected_by", "detected_by"),
        ] {
            let mut parameters = evolves_parameters("older", "newer", "replaces", 100);
            parameters.insert(key.into(), Value::String(String::new()));
            let error = validate_evolves_parameters(&parameters).unwrap_err();
            assert!(error.to_string().contains(name), "{error}");
        }
        assert_eq!(db.commit_epoch().unwrap(), before);
        assert_eq!(read_test_wal(&path).unwrap(), wal_before);
        let output = db
            .query_request(bounded_batch_query(
                EVOLVES_READ,
                &evolves_parameters("older", "newer", "replaces", 100),
            ))
            .unwrap();
        assert!(output.rows.is_empty());
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn memory_evolves_create_rejects_non_numeric_confidence_before_wal() {
    let path = unique_test_dir("memory_evolves_non_numeric_confidence");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Memory {id: 'older'})").unwrap();
        db.query("CREATE (:Memory {id: 'newer'})").unwrap();
        let before = db.commit_epoch().unwrap();
        let wal_before = read_test_wal(&path).unwrap();
        for value in [
            Value::String("high".into()),
            Value::Float(f64::NAN),
            Value::Float(f64::INFINITY),
        ] {
            let mut parameters = evolves_parameters("older", "newer", "replaces", 100);
            parameters.insert("confidence".into(), value);
            let error = validate_evolves_parameters(&parameters).unwrap_err();
            assert!(error.to_string().contains("numeric finite confidence"));
        }
        assert_eq!(db.commit_epoch().unwrap(), before);
        assert_eq!(read_test_wal(&path).unwrap(), wal_before);
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn memory_evolves_queries_persist_as_one_wal_batch_and_replay() {
    let path = unique_test_dir("memory_evolves_query_wal_replay");
    {
        let mut db = Database::open(&path).unwrap();
        for id in ["older_1", "newer_1", "older_2", "newer_2"] {
            let parameters = memory_parameters(id).unwrap();
            db.query_request(bounded_batch_query(
                "CREATE (:Memory {id: $id})",
                &parameters,
            ))
            .unwrap();
        }
        let batches_before = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        let basic = evolves_parameters("older_1", "newer_1", "confirms", 100);
        let mut progression = evolves_parameters("older_2", "newer_2", "replaces", 200);
        progression.extend([
            ("progression".into(), Value::Bool(true)),
            ("confidence".into(), Value::Float(0.9)),
            ("detected_by".into(), Value::String("scheduler".into())),
            ("reviewed".into(), Value::Bool(false)),
            ("reason".into(), Value::String("new evidence".into())),
        ]);
        let mut transaction = db.begin_transaction().unwrap();
        for (query, parameters) in [
            (EVOLVES_CREATE, &basic),
            (EVOLVES_PROGRESSION_CREATE, &progression),
        ] {
            validate_evolves_parameters(parameters).unwrap();
            transaction
                .query_request(bounded_batch_query(query, parameters))
                .unwrap();
        }
        assert_eq!(transaction.commit().unwrap().rows.len(), 2);
        assert_eq!(
            read_test_wal(&path).unwrap().matches("\tbatch\t").count(),
            batches_before + 1
        );
    }
    assert!(read_test_wal(&path).unwrap().contains("create_rel"));
    {
        let mut db = Database::open(&path).unwrap();
        let rows = db.query_request(bounded_batch_query(
            "MATCH (older:Memory)-[r:EVOLVES]->(newer:Memory) RETURN older.id AS older, newer.id AS newer, \
             r.content_relation AS relation, r.created_at AS created_at, r.confidence AS confidence \
             ORDER BY older", &BTreeMap::new())).unwrap();
        assert_eq!(rows.rows.len(), 2);
        assert_eq!(rows.rows[0]["older"], Value::String("older_1".into()));
        assert_eq!(rows.rows[0]["newer"], Value::String("newer_1".into()));
        assert_eq!(rows.rows[0]["relation"], Value::String("confirms".into()));
        assert_eq!(rows.rows[0]["created_at"], Value::Int(100));
        assert_eq!(rows.rows[0]["confidence"], Value::Null);
        assert_eq!(rows.rows[1]["older"], Value::String("older_2".into()));
        assert_eq!(rows.rows[1]["newer"], Value::String("newer_2".into()));
        assert_eq!(rows.rows[1]["relation"], Value::String("replaces".into()));
        assert_eq!(rows.rows[1]["created_at"], Value::Int(200));
        assert_eq!(rows.rows[1]["confidence"], Value::Float(0.9));
    }
    std::fs::remove_dir_all(path).unwrap();
}

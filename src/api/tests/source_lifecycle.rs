// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;

const LIFECYCLE: &str = "MATCH (s:Source {id: $id}) \
    WHERE $current IS NULL OR s.lifecycle_state = $current \
    SET s.lifecycle_state = $state, s.updated_at = $updated_at RETURN id(s) AS node_id";
const LIFECYCLE_WITH_CHUNKS: &str = "MATCH (s:Source {id: $id}) \
    WHERE $current IS NULL OR s.lifecycle_state = $current \
    SET s.lifecycle_state = $state, s.chunk_count = $chunks, s.updated_at = $updated_at RETURN id(s) AS node_id";
const METADATA: &str = "MATCH (s:Source {id: $id}) \
    SET s.metadata = $metadata, s.updated_at = $updated_at RETURN id(s) AS node_id";
const SOURCE_READ: &str = "MATCH (s:Source) RETURN s.id AS id, s.lifecycle_state AS state, \
    s.chunk_count AS chunks, s.metadata AS metadata, s.updated_at AS updated_at ORDER BY id";
const IDLESS_READ: &str = "MATCH (s:Source {original_name: $name}) \
    RETURN id(s) AS node_id, s.id AS id, s.metadata AS metadata, s.updated_at AS updated_at";

fn source_parameters(id: &str, fields: &[(&str, Value)]) -> Result<BTreeMap<String, Value>> {
    if id.is_empty() {
        return Err(HawDBError::Semantic("non-empty source id required".into()));
    }
    let mut parameters = BTreeMap::from([("id".into(), Value::String(id.into()))]);
    parameters.extend(
        fields
            .iter()
            .map(|(key, value)| ((*key).into(), value.clone())),
    );
    Ok(parameters)
}

fn lifecycle_parameters(
    id: &str,
    current: Option<&str>,
    state: &str,
    chunks: Option<i64>,
    updated_at: &str,
) -> Result<BTreeMap<String, Value>> {
    if state.is_empty() || current.is_some_and(str::is_empty) {
        return Err(HawDBError::Semantic(
            "non-empty lifecycle state required".into(),
        ));
    }
    if chunks.is_some_and(|value| value < 0) {
        return Err(HawDBError::Semantic(
            "non-negative chunk count required".into(),
        ));
    }
    let mut parameters = source_parameters(
        id,
        &[
            (
                "current",
                current.map_or(Value::Null, |value| Value::String(value.into())),
            ),
            ("state", Value::String(state.into())),
            ("updated_at", Value::String(updated_at.into())),
        ],
    )?;
    if let Some(chunks) = chunks {
        parameters.insert("chunks".into(), Value::Int(chunks));
    }
    Ok(parameters)
}

#[test]
fn updates_source_lifecycle_batch_for_nowledge_shapes() {
    let mut db = Database::new();
    db.query("CREATE (:Source {id: 'source_1', lifecycle_state: 'extracted', updated_at: 1})")
        .unwrap();
    db.query("CREATE (:Source {id: 'source_2', lifecycle_state: 'parsed', chunk_count: 0, updated_at: 1})").unwrap();
    db.query("CREATE (:Source {id: 'source_3', lifecycle_state: 'parsed', updated_at: 1})")
        .unwrap();
    let inputs = [
        (
            LIFECYCLE,
            lifecycle_parameters(
                "source_1",
                Some("extracted"),
                "indexed",
                None,
                "2026-07-19T12:00:00Z",
            )
            .unwrap(),
        ),
        (
            LIFECYCLE_WITH_CHUNKS,
            lifecycle_parameters("source_2", None, "indexed", Some(8), "2026-07-19T12:01:00Z")
                .unwrap(),
        ),
        (
            LIFECYCLE,
            lifecycle_parameters(
                "source_3",
                Some("extracted"),
                "indexed",
                None,
                "2026-07-19T12:02:00Z",
            )
            .unwrap(),
        ),
        (
            LIFECYCLE,
            lifecycle_parameters("source_2", None, "archived", None, "2026-07-19T12:03:00Z")
                .unwrap(),
        ),
        (
            LIFECYCLE,
            lifecycle_parameters("missing", None, "indexed", None, "2026-07-19T12:04:00Z").unwrap(),
        ),
    ];
    let epoch = db.commit_epoch().unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    let mut seen = BTreeSet::new();
    let mut duplicate_count = 0;
    let mut affected = Vec::new();
    for (query, parameters) in &inputs {
        let Value::String(id) = &parameters["id"] else {
            panic!("expected source identifier");
        };
        if !seen.insert(id) {
            duplicate_count += 1;
            continue;
        }
        affected.push(
            transaction
                .query_request(bounded_batch_query(query, parameters))
                .unwrap()
                .rows
                .len(),
        );
    }
    assert_eq!(duplicate_count, 1);
    assert_eq!(affected, [1, 1, 0, 0]);
    // SET RETURN delivers rows at statement execution, without retaining commit rows.
    assert!(transaction.commit().unwrap().rows.is_empty());
    assert_eq!(db.commit_epoch().unwrap(), epoch + 1);
    let rows = db
        .query_request(bounded_batch_query(SOURCE_READ, &BTreeMap::new()))
        .unwrap();
    assert_eq!(rows.rows.len(), 3);
    assert_eq!(rows.rows[0]["id"], Value::String("source_1".into()));
    assert_eq!(rows.rows[0]["state"], Value::String("indexed".into()));
    assert_eq!(rows.rows[0]["chunks"], Value::Null);
    assert_eq!(
        rows.rows[0]["updated_at"],
        Value::String("2026-07-19T12:00:00Z".into())
    );
    assert_eq!(rows.rows[1]["id"], Value::String("source_2".into()));
    assert_eq!(rows.rows[1]["state"], Value::String("indexed".into()));
    assert_eq!(rows.rows[1]["chunks"], Value::Int(8));
    assert_eq!(
        rows.rows[1]["updated_at"],
        Value::String("2026-07-19T12:01:00Z".into())
    );
    assert_eq!(rows.rows[2]["id"], Value::String("source_3".into()));
    assert_eq!(rows.rows[2]["state"], Value::String("parsed".into()));
    assert_eq!(rows.rows[2]["updated_at"], Value::Int(1));
}

#[test]
fn source_lifecycle_batch_rejects_invalid_rows_before_wal() {
    let path = unique_test_dir("source_lifecycle_invalid_query_inputs");
    let mut db = Database::open(&path).unwrap();
    db.query("CREATE (:Source {id: 'source_1', lifecycle_state: 'parsed'})")
        .unwrap();
    let epoch = db.commit_epoch().unwrap();
    let wal = read_test_wal(&path).unwrap();
    for (id, current, state, chunks, message) in [
        (
            "source_1",
            None,
            "indexed",
            Some(-1),
            "non-negative chunk count",
        ),
        ("", None, "indexed", None, "non-empty source id"),
        ("source_1", None, "", None, "non-empty lifecycle state"),
        (
            "source_1",
            Some(""),
            "indexed",
            None,
            "non-empty lifecycle state",
        ),
    ] {
        let error =
            lifecycle_parameters(id, current, state, chunks, "2026-07-19T12:00:00Z").unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
    }
    assert_eq!(db.commit_epoch().unwrap(), epoch);
    assert_eq!(read_test_wal(&path).unwrap(), wal);
    drop(db);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn typed_source_lifecycle_batch_persists_as_one_wal_batch_and_replays() {
    let path = unique_test_dir("typed_source_lifecycle_batch_wal_replay");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Source {id: 'source_1', lifecycle_state: 'extracted', chunk_count: 0})")
            .unwrap();
        db.query("CREATE (:Source {id: 'source_2', lifecycle_state: 'parsed', chunk_count: 0})")
            .unwrap();
        let before = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        let mut transaction = db.begin_transaction().unwrap();
        for (query, parameters) in [
            (
                LIFECYCLE,
                lifecycle_parameters(
                    "source_1",
                    Some("extracted"),
                    "indexed",
                    None,
                    "2026-07-19T12:00:00Z",
                )
                .unwrap(),
            ),
            (
                LIFECYCLE_WITH_CHUNKS,
                lifecycle_parameters("source_2", None, "indexed", Some(3), "2026-07-19T12:01:00Z")
                    .unwrap(),
            ),
        ] {
            assert_eq!(
                transaction
                    .query_request(bounded_batch_query(query, &parameters))
                    .unwrap()
                    .rows
                    .len(),
                1
            );
        }
        assert!(transaction.commit().unwrap().rows.is_empty());
        assert_eq!(
            read_test_wal(&path).unwrap().matches("\tbatch\t").count(),
            before + 1
        );
    }
    assert!(read_test_wal(&path).unwrap().contains("set_node_property"));
    {
        let mut db = Database::open(&path).unwrap();
        let rows = db
            .query_request(bounded_batch_query(SOURCE_READ, &BTreeMap::new()))
            .unwrap();
        assert_eq!(rows.rows.len(), 2);
        for (index, id, chunks, timestamp) in [
            (0, "source_1", 0, "2026-07-19T12:00:00Z"),
            (1, "source_2", 3, "2026-07-19T12:01:00Z"),
        ] {
            assert_eq!(rows.rows[index]["id"], Value::String(id.into()));
            assert_eq!(rows.rows[index]["state"], Value::String("indexed".into()));
            assert_eq!(rows.rows[index]["chunks"], Value::Int(chunks));
            assert_eq!(
                rows.rows[index]["updated_at"],
                Value::String(timestamp.into())
            );
        }
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn updates_source_metadata_batch_for_nowledge_auto_ocr_shape() {
    let mut db = Database::new();
    db.query("CREATE (:Source {id: 'source_1', metadata: '{}', updated_at: 1})")
        .unwrap();
    db.query("CREATE (:Source {id: 'source_2', metadata: '{}', updated_at: 1})")
        .unwrap();
    db.query("CREATE (:Source {original_name: 'Idless Source'})")
        .unwrap();
    let idless_parameters =
        BTreeMap::from([("name".into(), Value::String("Idless Source".into()))]);
    let idless = db
        .query_request(bounded_batch_query(IDLESS_READ, &idless_parameters))
        .unwrap();
    assert_eq!(idless.rows.len(), 1);
    let idless_id = match idless.rows[0]["node_id"] {
        Value::Int(id) => id.to_string(),
        ref other => panic!("expected projected node id, got {other:?}"),
    };
    let inputs = [
        ("source_1", "{\"ocr\":\"ready\"}", "2026-07-19T12:00:00Z"),
        ("missing", "{\"missing\":true}", "2026-07-19T12:01:00Z"),
        (
            idless_id.as_str(),
            "{\"idless\":true}",
            "2026-07-19T12:02:00Z",
        ),
        ("source_2", "{\"ocr\":\"queued\"}", "2026-07-19T12:03:00Z"),
        ("source_1", "{\"duplicate\":true}", "2026-07-19T12:04:00Z"),
    ]
    .map(|(id, metadata, timestamp)| {
        source_parameters(
            id,
            &[
                ("metadata", Value::String(metadata.into())),
                ("updated_at", Value::String(timestamp.into())),
            ],
        )
        .unwrap()
    });
    let epoch = db.commit_epoch().unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    let mut seen = BTreeSet::new();
    let mut duplicate_count = 0;
    let mut affected = Vec::new();
    for parameters in &inputs {
        let Value::String(id) = &parameters["id"] else {
            panic!("expected source identifier");
        };
        if !seen.insert(id) {
            duplicate_count += 1;
            continue;
        }
        affected.push(
            transaction
                .query_request(bounded_batch_query(METADATA, parameters))
                .unwrap()
                .rows
                .len(),
        );
    }
    assert_eq!(duplicate_count, 1);
    assert_eq!(affected, [1, 0, 0, 1]);
    assert!(transaction.commit().unwrap().rows.is_empty());
    assert_eq!(db.commit_epoch().unwrap(), epoch + 1);
    assert_eq!(
        db.query_request(bounded_batch_query(IDLESS_READ, &idless_parameters))
            .unwrap()
            .rows,
        idless.rows
    );
    let parameters = BTreeMap::from([(
        "ids".into(),
        Value::List(vec![
            Value::String("source_1".into()),
            Value::String("source_2".into()),
        ]),
    )]);
    let rows = db.query_request(bounded_batch_query(
        "MATCH (s:Source) WHERE s.id IN $ids RETURN s.id AS id, s.metadata AS metadata, s.updated_at AS updated_at ORDER BY id", &parameters,
    )).unwrap();
    assert_eq!(rows.rows.len(), 2);
    for (index, id, metadata, timestamp) in [
        (0, "source_1", "{\"ocr\":\"ready\"}", "2026-07-19T12:00:00Z"),
        (
            1,
            "source_2",
            "{\"ocr\":\"queued\"}",
            "2026-07-19T12:03:00Z",
        ),
    ] {
        assert_eq!(rows.rows[index]["id"], Value::String(id.into()));
        assert_eq!(rows.rows[index]["metadata"], Value::String(metadata.into()));
        assert_eq!(
            rows.rows[index]["updated_at"],
            Value::String(timestamp.into())
        );
    }
}

#[test]
fn source_metadata_batch_rejects_empty_source_id_before_wal() {
    let path = unique_test_dir("source_metadata_invalid_query_inputs");
    let mut db = Database::open(&path).unwrap();
    db.query("CREATE (:Source {id: 'source_1', metadata: '{}', updated_at: 1})")
        .unwrap();
    let epoch = db.commit_epoch().unwrap();
    let wal = read_test_wal(&path).unwrap();
    let error = source_parameters(
        "",
        &[
            ("metadata", Value::String("{}".into())),
            ("updated_at", Value::String("2026-07-19T12:00:00Z".into())),
        ],
    )
    .unwrap_err();
    assert!(error.to_string().contains("non-empty source id"));
    assert_eq!(db.commit_epoch().unwrap(), epoch);
    assert_eq!(read_test_wal(&path).unwrap(), wal);
    drop(db);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn typed_source_metadata_batch_persists_as_one_wal_batch_and_replays() {
    let path = unique_test_dir("typed_source_metadata_batch_wal_replay");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Source {id: 'source_1', metadata: '{}', updated_at: 1})")
            .unwrap();
        db.query("CREATE (:Source {id: 'source_2', metadata: '{}', updated_at: 1})")
            .unwrap();
        let before = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        let mut transaction = db.begin_transaction().unwrap();
        for (id, metadata, timestamp) in [
            ("source_1", "{\"ocr\":\"ready\"}", "2026-07-19T12:00:00Z"),
            ("source_2", "{\"ocr\":\"queued\"}", "2026-07-19T12:01:00Z"),
        ] {
            let parameters = source_parameters(
                id,
                &[
                    ("metadata", Value::String(metadata.into())),
                    ("updated_at", Value::String(timestamp.into())),
                ],
            )
            .unwrap();
            assert_eq!(
                transaction
                    .query_request(bounded_batch_query(METADATA, &parameters))
                    .unwrap()
                    .rows
                    .len(),
                1
            );
        }
        assert!(transaction.commit().unwrap().rows.is_empty());
        assert_eq!(
            read_test_wal(&path).unwrap().matches("\tbatch\t").count(),
            before + 1
        );
    }
    assert!(read_test_wal(&path).unwrap().contains("set_node_property"));
    {
        let mut db = Database::open(&path).unwrap();
        let rows = db
            .query_request(bounded_batch_query(SOURCE_READ, &BTreeMap::new()))
            .unwrap();
        assert_eq!(rows.rows.len(), 2);
        for (index, id, metadata, timestamp) in [
            (0, "source_1", "{\"ocr\":\"ready\"}", "2026-07-19T12:00:00Z"),
            (
                1,
                "source_2",
                "{\"ocr\":\"queued\"}",
                "2026-07-19T12:01:00Z",
            ),
        ] {
            assert_eq!(rows.rows[index]["id"], Value::String(id.into()));
            assert_eq!(rows.rows[index]["metadata"], Value::String(metadata.into()));
            assert_eq!(
                rows.rows[index]["updated_at"],
                Value::String(timestamp.into())
            );
        }
    }
    std::fs::remove_dir_all(path).unwrap();
}

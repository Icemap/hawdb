// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;

const CREATE: &str = "MERGE (s:Source {id: $id}) ON CREATE SET \
    s.source_type = $source_type, s.original_name = $original_name, s.mime_type = $mime_type, \
    s.file_path = $file_path, s.parsed_path = $parsed_path, s.source_url = $source_url, \
    s.sha256 = $sha256, s.size_bytes = $size_bytes, s.version = $version, s.space_id = $space_id, \
    s.lifecycle_state = 'parsed', s.chunk_count = 0, s.memory_count = 0, \
    s.section_tree = $section_tree, s.summary = $summary, s.error_message = '', \
    s.created_at = $created_at, s.updated_at = $updated_at, s.metadata = $metadata";
const READ: &str = "MATCH (s:Source) RETURN s.id AS id, id(s) AS node_id, \
    s.source_type AS source_type, s.original_name AS original_name, s.mime_type AS mime_type, \
    s.file_path AS file_path, s.parsed_path AS parsed_path, s.source_url AS source_url, \
    s.sha256 AS sha256, s.size_bytes AS size_bytes, s.version AS version, s.space_id AS space_id, \
    s.lifecycle_state AS state, s.chunk_count AS chunk_count, s.memory_count AS memory_count, \
    s.section_tree AS section_tree, s.summary AS summary, s.error_message AS error_message, \
    s.created_at AS created_at, s.updated_at AS updated_at, s.metadata AS metadata ORDER BY id";
const EXISTING: &str = "MATCH (s:Source {id: $id}) RETURN s AS source";

fn source_parameters(id: &str) -> BTreeMap<String, Value> {
    let mut parameters = BTreeMap::from([
        ("size_bytes".into(), Value::Int(1024)),
        ("version".into(), Value::Int(1)),
    ]);
    for (key, value) in [
        ("id", id),
        ("source_type", "file"),
        ("original_name", "Document.md"),
        ("mime_type", "text/markdown"),
        ("file_path", "/tmp/document.md"),
        ("parsed_path", "/tmp/document.parsed"),
        ("source_url", ""),
        ("sha256", "sha-document"),
        ("space_id", "default"),
        ("section_tree", ""),
        ("summary", "Document summary"),
        ("created_at", "2026-07-19T12:00:00Z"),
        ("updated_at", "2026-07-19T12:01:00Z"),
        ("metadata", "{\"import\":\"manual\"}"),
    ] {
        parameters.insert(key.into(), Value::String(value.into()));
    }
    parameters
}

fn validate_parameters(parameters: &BTreeMap<String, Value>) -> Result<()> {
    for field in [
        "id",
        "source_type",
        "original_name",
        "mime_type",
        "parsed_path",
        "sha256",
        "space_id",
    ] {
        if !matches!(parameters.get(field), Some(Value::String(value)) if !value.is_empty()) {
            return Err(HawDBError::Semantic(format!("non-empty {field} required")));
        }
    }
    if !matches!(parameters.get("size_bytes"), Some(Value::Int(value)) if *value >= 0) {
        return Err(HawDBError::Semantic(
            "non-negative size bytes required".into(),
        ));
    }
    if !matches!(parameters.get("version"), Some(Value::Int(value)) if *value > 0) {
        return Err(HawDBError::Semantic("positive version required".into()));
    }
    Ok(())
}

#[test]
fn creates_source_parsed_batch_for_nowledge_ingest_shapes() {
    let mut db = Database::new();
    db.query("CREATE (:Source {id: 'existing', original_name: 'Existing'})")
        .unwrap();
    let existing_parameters = source_parameters("existing");
    let existing = db
        .query_request(bounded_batch_query(EXISTING, &existing_parameters))
        .unwrap();
    let mut markdown = source_parameters("markdown-source");
    markdown.insert("version".into(), Value::Int(3));
    let mut url = source_parameters("url-source");
    for (key, value) in [
        ("source_type", "url"),
        ("original_name", "URL Source"),
        ("mime_type", "text/html"),
        ("file_path", ""),
        ("source_url", "https://example.test/source"),
        ("section_tree", "{\"sections\":[]}"),
        ("sha256", "sha-url"),
    ] {
        url.insert(key.into(), Value::String(value.into()));
    }
    url.insert("size_bytes".into(), Value::Int(2048));
    let mut pdf = source_parameters("pdf-source");
    for (key, value) in [
        ("original_name", "Source.pdf"),
        ("mime_type", "application/pdf"),
        ("file_path", "/tmp/source.pdf"),
        ("source_url", "file:///tmp/source.pdf"),
        ("sha256", "sha-pdf"),
    ] {
        pdf.insert(key.into(), Value::String(value.into()));
    }
    let inputs = [
        markdown,
        url,
        pdf,
        existing_parameters.clone(),
        source_parameters("markdown-source"),
    ];
    for parameters in &inputs {
        validate_parameters(parameters).unwrap();
    }
    let epoch = db.commit_epoch().unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    for parameters in &inputs {
        transaction
            .query_request(bounded_batch_query(CREATE, parameters))
            .unwrap();
    }
    let committed = transaction.commit().unwrap();
    assert_eq!(committed.rows.len(), 5);
    for (row, created) in committed.rows.iter().zip([true, true, true, false, false]) {
        assert_eq!(row.get("created"), Some(&Value::Bool(created)));
    }
    assert_eq!(db.commit_epoch().unwrap(), epoch + 1);
    assert_eq!(
        db.query_request(bounded_batch_query(EXISTING, &existing_parameters))
            .unwrap()
            .rows,
        existing.rows
    );
    let rows = db
        .query_request(bounded_batch_query(READ, &BTreeMap::new()))
        .unwrap();
    assert_eq!(rows.rows.len(), 4);
    for (index, parameters) in [(1, &inputs[0]), (2, &inputs[2]), (3, &inputs[1])] {
        let row = &rows.rows[index];
        for (key, expected) in parameters {
            assert_eq!(row.get(key), Some(expected), "{key}");
        }
        assert!(matches!(row.get("node_id"), Some(Value::Int(_))));
        assert_eq!(row.get("state"), Some(&Value::String("parsed".into())));
        assert_eq!(row.get("chunk_count"), Some(&Value::Int(0)));
        assert_eq!(row.get("memory_count"), Some(&Value::Int(0)));
        assert_eq!(
            row.get("error_message"),
            Some(&Value::String(String::new()))
        );
    }
}

#[test]
fn source_parsed_create_batch_rejects_invalid_rows_before_wal() {
    let path = unique_test_dir("source_parsed_create_invalid_query_inputs");
    let db = Database::open(&path).unwrap();
    let epoch = db.commit_epoch().unwrap();
    let wal = read_test_wal(&path).unwrap_or_default();
    for (field, value, message) in [
        ("version", Value::Int(0), "positive version"),
        ("size_bytes", Value::Int(-1), "non-negative size bytes"),
        ("id", Value::String(String::new()), "non-empty id"),
        (
            "source_type",
            Value::String(String::new()),
            "non-empty source_type",
        ),
        (
            "original_name",
            Value::String(String::new()),
            "non-empty original_name",
        ),
        (
            "mime_type",
            Value::String(String::new()),
            "non-empty mime_type",
        ),
        (
            "parsed_path",
            Value::String(String::new()),
            "non-empty parsed_path",
        ),
        ("sha256", Value::String(String::new()), "non-empty sha256"),
        (
            "space_id",
            Value::String(String::new()),
            "non-empty space_id",
        ),
    ] {
        let mut parameters = source_parameters("invalid-source");
        parameters.insert(field.into(), value);
        let error = validate_parameters(&parameters).unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
    }
    assert_eq!(db.commit_epoch().unwrap(), epoch);
    assert_eq!(read_test_wal(&path).unwrap_or_default(), wal);
    drop(db);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn typed_source_parsed_create_batch_persists_as_one_wal_batch_and_replays() {
    let path = unique_test_dir("typed_source_parsed_create_batch_wal_replay");
    {
        let mut db = Database::open(&path).unwrap();
        let before = read_test_wal(&path)
            .unwrap_or_default()
            .matches("\tbatch\t")
            .count();
        let mut transaction = db.begin_transaction().unwrap();
        for id in ["source_1", "source_2"] {
            let parameters = source_parameters(id);
            validate_parameters(&parameters).unwrap();
            transaction
                .query_request(bounded_batch_query(CREATE, &parameters))
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
    assert!(read_test_wal(&path).unwrap().contains("create_node"));
    {
        let mut db = Database::open(&path).unwrap();
        let rows = db
            .query_request(bounded_batch_query(READ, &BTreeMap::new()))
            .unwrap();
        assert_eq!(rows.rows.len(), 2);
        for (index, id) in ["source_1", "source_2"].into_iter().enumerate() {
            let parameters = source_parameters(id);
            for (key, expected) in &parameters {
                assert_eq!(rows.rows[index].get(key), Some(expected), "{key}");
            }
            assert_eq!(rows.rows[index]["state"], Value::String("parsed".into()));
            assert_eq!(rows.rows[index]["chunk_count"], Value::Int(0));
            assert_eq!(rows.rows[index]["memory_count"], Value::Int(0));
            assert_eq!(
                rows.rows[index]["error_message"],
                Value::String(String::new())
            );
        }
    }
    std::fs::remove_dir_all(path).unwrap();
}

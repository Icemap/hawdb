// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;

const PARSED: &str = "MATCH (s:Source {id: $id}) SET s.lifecycle_state = 'parsed', \
    s.summary = $summary, s.sha256 = $sha256, s.size_bytes = $size_bytes, s.updated_at = $updated_at \
    RETURN id(s) AS node_id";
const PARSED_PATH: &str =
    "MATCH (s:Source {id: $id}) SET s.parsed_path = $parsed_path RETURN id(s) AS node_id";
const PARSED_DETAILS: &str = "MATCH (s:Source {id: $id}) SET s.parsed_path = $parsed_path, \
    s.file_path = $file_path, s.original_name = $original_name, s.mime_type = $mime_type, \
    s.source_url = $source_url, s.metadata = $metadata RETURN id(s) AS node_id";
const READ: &str = "MATCH (s:Source) WHERE s.id IN $ids RETURN s.id AS id, s.lifecycle_state AS state, \
    s.summary AS summary, s.sha256 AS sha256, s.size_bytes AS size_bytes, s.updated_at AS updated_at, \
    s.parsed_path AS parsed_path, s.file_path AS file_path, s.original_name AS original_name, \
    s.mime_type AS mime_type, s.source_url AS source_url, s.metadata AS metadata ORDER BY id";
const IDLESS: &str = "MATCH (s:Source {original_name: $name}) RETURN id(s) AS node_id, s AS source";

fn parsed_parameters(
    id: &str,
    summary: &str,
    sha256: &str,
    size_bytes: i64,
    timestamp: &str,
) -> Result<BTreeMap<String, Value>> {
    if id.is_empty() {
        return Err(HawDBError::Semantic("non-empty source id required".into()));
    }
    if sha256.is_empty() {
        return Err(HawDBError::Semantic("non-empty sha256 required".into()));
    }
    if size_bytes < 0 {
        return Err(HawDBError::Semantic(
            "non-negative size bytes required".into(),
        ));
    }
    Ok(BTreeMap::from([
        ("id".into(), Value::String(id.into())),
        ("summary".into(), Value::String(summary.into())),
        ("sha256".into(), Value::String(sha256.into())),
        ("size_bytes".into(), Value::Int(size_bytes)),
        ("updated_at".into(), Value::String(timestamp.into())),
    ]))
}

fn read_parameters() -> BTreeMap<String, Value> {
    BTreeMap::from([(
        "ids".into(),
        Value::List(vec![
            Value::String("source_1".into()),
            Value::String("source_2".into()),
        ]),
    )])
}

#[test]
fn updates_source_parsed_metadata_batch_for_nowledge_parser_shapes() {
    let mut db = Database::new();
    db.query("CREATE (:Source {id: 'source_1', lifecycle_state: 'ingested', summary: 'old'})")
        .unwrap();
    db.query("CREATE (:Source {id: 'source_2', lifecycle_state: 'ingested'})")
        .unwrap();
    db.query("CREATE (:Source {original_name: 'Idless Source'})")
        .unwrap();
    let idless_parameters =
        BTreeMap::from([("name".into(), Value::String("Idless Source".into()))]);
    let idless = db
        .query_request(bounded_batch_query(IDLESS, &idless_parameters))
        .unwrap();
    assert_eq!(idless.rows.len(), 1);
    let idless_id = match idless.rows[0]["node_id"] {
        Value::Int(id) => id.to_string(),
        ref other => panic!("expected projected node id, got {other:?}"),
    };
    let mut full = parsed_parameters(
        "source_1",
        "HTML summary",
        "sha-html",
        4096,
        "2026-07-19T12:01:00Z",
    )
    .unwrap();
    for (key, value) in [
        ("parsed_path", "/tmp/html.parsed"),
        ("file_path", ""),
        ("original_name", "HTML Source"),
        ("mime_type", "text/html"),
        ("source_url", "https://example.test/source"),
        ("metadata", "{\"parser\":\"html\"}"),
    ] {
        full.insert(key.into(), Value::String(value.into()));
    }
    let inputs = [
        full,
        parsed_parameters(
            "missing",
            "Missing",
            "sha-missing",
            101,
            "2026-07-19T12:00:00Z",
        )
        .unwrap(),
        parsed_parameters(
            &idless_id,
            "Idless",
            "sha-idless",
            101,
            "2026-07-19T12:00:00Z",
        )
        .unwrap(),
        parsed_parameters(
            "source_2",
            "Summary only",
            "sha-summary",
            128,
            "2026-07-19T12:02:00Z",
        )
        .unwrap(),
        parsed_parameters(
            "source_1",
            "Duplicate",
            "sha-duplicate",
            101,
            "2026-07-19T12:00:00Z",
        )
        .unwrap(),
    ];
    let epoch = db.commit_epoch().unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    let mut seen = BTreeSet::new();
    let mut duplicates = 0;
    let mut affected = Vec::new();
    for parameters in &inputs {
        let Value::String(id) = &parameters["id"] else {
            panic!("expected source identifier");
        };
        if !seen.insert(id) {
            duplicates += 1;
            continue;
        }
        let rows = transaction
            .query_request(bounded_batch_query(PARSED, parameters))
            .unwrap()
            .rows
            .len();
        affected.push(rows);
        if parameters.contains_key("metadata") {
            assert_eq!(
                transaction
                    .query_request(bounded_batch_query(PARSED_DETAILS, parameters))
                    .unwrap()
                    .rows
                    .len(),
                rows
            );
        }
    }
    assert_eq!(duplicates, 1);
    assert_eq!(affected, [1, 0, 0, 1]);
    transaction.commit().unwrap();
    assert_eq!(db.commit_epoch().unwrap(), epoch + 1);
    assert_eq!(
        db.query_request(bounded_batch_query(IDLESS, &idless_parameters))
            .unwrap()
            .rows,
        idless.rows
    );
    let rows = db
        .query_request(bounded_batch_query(READ, &read_parameters()))
        .unwrap();
    assert_eq!(rows.rows.len(), 2);
    for (key, value) in [
        ("id", "source_1"),
        ("state", "parsed"),
        ("summary", "HTML summary"),
        ("sha256", "sha-html"),
        ("updated_at", "2026-07-19T12:01:00Z"),
        ("parsed_path", "/tmp/html.parsed"),
        ("file_path", ""),
        ("original_name", "HTML Source"),
        ("mime_type", "text/html"),
        ("source_url", "https://example.test/source"),
        ("metadata", "{\"parser\":\"html\"}"),
    ] {
        assert_eq!(rows.rows[0][key], Value::String(value.into()));
    }
    assert_eq!(rows.rows[0]["size_bytes"], Value::Int(4096));
    for (key, value) in [
        ("id", "source_2"),
        ("state", "parsed"),
        ("summary", "Summary only"),
        ("sha256", "sha-summary"),
        ("updated_at", "2026-07-19T12:02:00Z"),
    ] {
        assert_eq!(rows.rows[1][key], Value::String(value.into()));
    }
    assert_eq!(rows.rows[1]["size_bytes"], Value::Int(128));
    for key in [
        "parsed_path",
        "file_path",
        "original_name",
        "mime_type",
        "source_url",
        "metadata",
    ] {
        assert_eq!(rows.rows[1][key], Value::Null);
    }
}

#[test]
fn source_parsed_metadata_batch_rejects_invalid_rows_before_wal() {
    let path = unique_test_dir("source_parsed_metadata_invalid_query_inputs");
    let mut db = Database::open(&path).unwrap();
    db.query("CREATE (:Source {id: 'source_1', lifecycle_state: 'ingested'})")
        .unwrap();
    let epoch = db.commit_epoch().unwrap();
    let wal = read_test_wal(&path).unwrap();
    for (id, sha, size, message) in [
        ("source_1", "sha-invalid", -1, "non-negative size bytes"),
        ("", "sha-invalid", 1, "non-empty source id"),
        ("source_1", "", 1, "non-empty sha256"),
    ] {
        let error =
            parsed_parameters(id, "Invalid", sha, size, "2026-07-19T12:00:00Z").unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
    }
    assert_eq!(db.commit_epoch().unwrap(), epoch);
    assert_eq!(read_test_wal(&path).unwrap(), wal);
    drop(db);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn typed_source_parsed_metadata_batch_persists_as_one_wal_batch_and_replays() {
    let path = unique_test_dir("typed_source_parsed_metadata_batch_wal_replay");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Source {id: 'source_1', lifecycle_state: 'ingested'})")
            .unwrap();
        db.query("CREATE (:Source {id: 'source_2', lifecycle_state: 'ingested'})")
            .unwrap();
        let before = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        let mut transaction = db.begin_transaction().unwrap();
        for (id, summary, sha, parsed_path) in [
            ("source_1", "First", "sha-first", "/tmp/first.parsed"),
            ("source_2", "Second", "sha-second", "/tmp/second.parsed"),
        ] {
            let mut parameters =
                parsed_parameters(id, summary, sha, 101, "2026-07-19T12:00:00Z").unwrap();
            parameters.insert("parsed_path".into(), Value::String(parsed_path.into()));
            for query in [PARSED, PARSED_PATH] {
                assert_eq!(
                    transaction
                        .query_request(bounded_batch_query(query, &parameters))
                        .unwrap()
                        .rows
                        .len(),
                    1
                );
            }
        }
        transaction.commit().unwrap();
        assert_eq!(
            read_test_wal(&path).unwrap().matches("\tbatch\t").count(),
            before + 1
        );
    }
    assert!(read_test_wal(&path).unwrap().contains("set_node_property"));
    {
        let mut db = Database::open(&path).unwrap();
        let rows = db
            .query_request(bounded_batch_query(READ, &read_parameters()))
            .unwrap();
        assert_eq!(rows.rows.len(), 2);
        for (index, id, summary, sha, parsed_path) in [
            (0, "source_1", "First", "sha-first", "/tmp/first.parsed"),
            (1, "source_2", "Second", "sha-second", "/tmp/second.parsed"),
        ] {
            assert_eq!(rows.rows[index]["id"], Value::String(id.into()));
            assert_eq!(rows.rows[index]["state"], Value::String("parsed".into()));
            assert_eq!(rows.rows[index]["summary"], Value::String(summary.into()));
            assert_eq!(rows.rows[index]["sha256"], Value::String(sha.into()));
            assert_eq!(rows.rows[index]["size_bytes"], Value::Int(101));
            assert_eq!(
                rows.rows[index]["updated_at"],
                Value::String("2026-07-19T12:00:00Z".into())
            );
            assert_eq!(
                rows.rows[index]["parsed_path"],
                Value::String(parsed_path.into())
            );
        }
    }
    std::fs::remove_dir_all(path).unwrap();
}

// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;

const ENDPOINTS: &str = "MATCH (n:Source {id: $newer}), (o:Source {id: $older}) \
    RETURN n.id AS newer, o.id AS older";
const CREATE: &str = "MATCH (n:Source {id: $newer}), (o:Source {id: $older}) \
    CREATE (n)-[:REVISED_AS {diff_summary: '', sections_changed: '[]', revision_type: 'update', \
    detected_by: 'filename_match', created_at: $created_at}]->(o)";
const READ: &str = "MATCH (n:Source)-[r:REVISED_AS]->(o:Source) \
    RETURN n.id AS newer, o.id AS older, r.diff_summary AS diff_summary, \
    r.sections_changed AS sections_changed, r.revision_type AS revision_type, \
    r.detected_by AS detected_by, r.created_at AS created_at ORDER BY newer, older";
const IDLESS: &str = "MATCH (s:Source {original_name: $name}) RETURN id(s) AS node_id, s AS source";

fn revision_parameters(
    newer: &str,
    older: &str,
    created_at: &str,
) -> Result<BTreeMap<String, Value>> {
    if newer.is_empty() {
        return Err(HawDBError::Semantic(
            "non-empty newer source id required".into(),
        ));
    }
    if older.is_empty() {
        return Err(HawDBError::Semantic(
            "non-empty older source id required".into(),
        ));
    }
    Ok(BTreeMap::from([
        ("newer".into(), Value::String(newer.into())),
        ("older".into(), Value::String(older.into())),
        ("created_at".into(), Value::String(created_at.into())),
    ]))
}

#[test]
fn creates_source_revision_batch_for_nowledge_revision_edges() {
    let mut db = Database::new();
    db.query("CREATE (:Source {id: 'newer', version: 2})")
        .unwrap();
    db.query("CREATE (:Source {id: 'older', version: 1})")
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
        ref other => panic!("expected node identifier, got {other:?}"),
    };
    let epoch = db.commit_epoch().unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    for (newer, older, timestamp, expected) in [
        ("newer", "older", "2026-07-19T12:00:00Z", 1),
        ("newer", "missing", "2026-07-19T12:01:00Z", 0),
        (idless_id.as_str(), "older", "2026-07-19T12:02:00Z", 0),
    ] {
        let parameters = revision_parameters(newer, older, timestamp).unwrap();
        assert_eq!(
            transaction
                .query_request(bounded_batch_query(ENDPOINTS, &parameters))
                .unwrap()
                .rows
                .len(),
            expected
        );
        assert!(transaction
            .query_request(bounded_batch_query(CREATE, &parameters))
            .unwrap()
            .rows
            .is_empty());
    }
    let staged = transaction
        .query_request(bounded_batch_query(READ, &BTreeMap::new()))
        .unwrap();
    assert_eq!(staged.rows.len(), 1);
    assert_eq!(transaction.commit().unwrap().rows.len(), 1);
    assert_eq!(db.commit_epoch().unwrap(), epoch + 1);
    let rows = db
        .query_request(bounded_batch_query(READ, &BTreeMap::new()))
        .unwrap();
    assert_eq!(rows.rows, staged.rows);
    for (key, expected) in [
        ("newer", "newer"),
        ("older", "older"),
        ("diff_summary", ""),
        ("sections_changed", "[]"),
        ("revision_type", "update"),
        ("detected_by", "filename_match"),
        ("created_at", "2026-07-19T12:00:00Z"),
    ] {
        assert_eq!(rows.rows[0][key], Value::String(expected.into()));
    }
    assert_eq!(
        db.query_request(bounded_batch_query(IDLESS, &idless_parameters))
            .unwrap()
            .rows,
        idless.rows
    );
}

#[test]
fn source_revision_create_batch_rejects_empty_ids_before_wal() {
    let path = unique_test_dir("source_revision_invalid_query_inputs");
    let mut db = Database::open(&path).unwrap();
    db.query("CREATE (:Source {id: 'newer'})").unwrap();
    let epoch = db.commit_epoch().unwrap();
    let wal = read_test_wal(&path).unwrap();
    for (newer, older, message) in [
        ("newer", "", "older source id"),
        ("", "older", "newer source id"),
    ] {
        let error = revision_parameters(newer, older, "2026-07-19T12:00:00Z").unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
    }
    assert_eq!(db.commit_epoch().unwrap(), epoch);
    assert_eq!(read_test_wal(&path).unwrap(), wal);
    drop(db);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn typed_source_revision_batch_persists_as_one_wal_batch_and_replays() {
    let path = unique_test_dir("typed_source_revision_batch_wal_replay");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Source {id: 'newer-1'})").unwrap();
        db.query("CREATE (:Source {id: 'older-1'})").unwrap();
        db.query("CREATE (:Source {id: 'newer-2'})").unwrap();
        db.query("CREATE (:Source {id: 'older-2'})").unwrap();
        let before = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        let mut transaction = db.begin_transaction().unwrap();
        for (newer, older, timestamp) in [
            ("newer-1", "older-1", "2026-07-19T12:00:00Z"),
            ("newer-2", "older-2", "2026-07-19T12:01:00Z"),
        ] {
            let parameters = revision_parameters(newer, older, timestamp).unwrap();
            assert!(transaction
                .query_request(bounded_batch_query(CREATE, &parameters))
                .unwrap()
                .rows
                .is_empty());
        }
        assert_eq!(transaction.commit().unwrap().rows.len(), 2);
        assert_eq!(
            read_test_wal(&path).unwrap().matches("\tbatch\t").count(),
            before + 1
        );
    }
    assert!(read_test_wal(&path).unwrap().contains("create_rel"));
    {
        let mut db = Database::open(&path).unwrap();
        let rows = db
            .query_request(bounded_batch_query(READ, &BTreeMap::new()))
            .unwrap();
        assert_eq!(rows.rows.len(), 2);
        for (index, newer, older, timestamp) in [
            (0, "newer-1", "older-1", "2026-07-19T12:00:00Z"),
            (1, "newer-2", "older-2", "2026-07-19T12:01:00Z"),
        ] {
            for (key, expected) in [
                ("newer", newer),
                ("older", older),
                ("created_at", timestamp),
                ("diff_summary", ""),
                ("sections_changed", "[]"),
                ("revision_type", "update"),
                ("detected_by", "filename_match"),
            ] {
                assert_eq!(rows.rows[index][key], Value::String(expected.into()));
            }
        }
    }
    std::fs::remove_dir_all(path).unwrap();
}

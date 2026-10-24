// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;

const INCREMENT: &str = "MATCH (s:Source {id: $id}) RETURN \
    CASE WHEN COALESCE(s.memory_count, 0) < 0 THEN $delta \
    WHEN COALESCE(s.memory_count, 0) > $threshold THEN $maximum \
    ELSE COALESCE(s.memory_count, 0) + $delta END AS count";
const DECREMENT: &str = "MATCH (s:Source {id: $id}) RETURN \
    CASE WHEN COALESCE(s.memory_count, 0) <= 0 THEN 0 \
    WHEN COALESCE(s.memory_count, 0) + $delta < 0 THEN 0 \
    ELSE COALESCE(s.memory_count, 0) + $delta END AS count";
const APPLY: &str =
    "MATCH (s:Source {id: $id}) SET s.memory_count = $count RETURN s.memory_count AS count";
const POINT_READ: &str = "MATCH (s:Source {id: $id}) RETURN COALESCE(s.memory_count, 0) AS count";
const READ: &str = "MATCH (s:Source) RETURN s.id AS id, s.memory_count AS count ORDER BY id";

fn count_parameters(id: &str, delta: i64) -> Result<BTreeMap<String, Value>> {
    if id.is_empty() {
        return Err(HawDBError::Semantic("non-empty source id required".into()));
    }
    if delta == 0 {
        return Err(HawDBError::Semantic("non-zero delta required".into()));
    }
    let mut parameters = BTreeMap::from([
        ("id".into(), Value::String(id.into())),
        ("delta".into(), Value::Int(delta)),
    ]);
    if delta > 0 {
        parameters.insert("threshold".into(), Value::Int(i64::MAX - delta));
        parameters.insert("maximum".into(), Value::Int(i64::MAX));
    }
    Ok(parameters)
}

#[test]
fn adjusts_source_memory_count_batch_with_floor_decrements() {
    let mut db = Database::new();
    db.query("CREATE (:Source {id: 'source_1', memory_count: 1})")
        .unwrap();
    db.query("CREATE (:Source {id: 'source_2'})").unwrap();
    db.query("CREATE (:Source {id: 'bad', memory_count: 'many'})")
        .unwrap();
    let epoch = db.commit_epoch().unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    for (id, delta, before, after) in [
        ("source_1", 1, 1, 2),
        ("source_1", -1, 2, 1),
        ("source_1", -1, 1, 0),
        ("source_1", -1, 0, 0),
        ("source_2", -1, 0, 0),
    ] {
        let mut parameters = count_parameters(id, delta).unwrap();
        let old = transaction
            .query_request(bounded_batch_query(POINT_READ, &parameters))
            .unwrap();
        assert_eq!(old.rows.len(), 1);
        assert_eq!(old.rows[0]["count"], Value::Int(before));
        let query = if delta > 0 { INCREMENT } else { DECREMENT };
        let output = transaction
            .query_request(bounded_batch_query(query, &parameters))
            .unwrap();
        assert_eq!(output.rows.len(), 1);
        assert_eq!(output.rows[0]["count"], Value::Int(after));
        parameters.insert("count".into(), output.rows[0]["count"].clone());
        assert_eq!(
            transaction
                .query_request(bounded_batch_query(APPLY, &parameters))
                .unwrap()
                .rows,
            output.rows
        );
    }
    assert!(transaction
        .query_request(bounded_batch_query(
            INCREMENT,
            &count_parameters("missing", 1).unwrap()
        ))
        .unwrap()
        .rows
        .is_empty());
    let error = transaction
        .query_request(bounded_batch_query(
            INCREMENT,
            &count_parameters("bad", 1).unwrap(),
        ))
        .unwrap_err();
    assert!(error.to_string().contains("numeric operands"), "{error}");
    let staged = transaction
        .query_request(bounded_batch_query(READ, &BTreeMap::new()))
        .unwrap();
    assert_eq!(staged.rows.len(), 3);
    assert_eq!(staged.rows[0]["id"], Value::String("bad".into()));
    assert_eq!(staged.rows[0]["count"], Value::String("many".into()));
    assert_eq!(staged.rows[1]["count"], Value::Int(0));
    assert_eq!(staged.rows[2]["count"], Value::Int(0));
    transaction.commit().unwrap();
    assert_eq!(db.commit_epoch().unwrap(), epoch + 1);
    assert_eq!(
        db.query_request(bounded_batch_query(READ, &BTreeMap::new()))
            .unwrap()
            .rows,
        staged.rows
    );

    let mut transaction = db.begin_transaction().unwrap();
    for (current, delta, expected) in [
        (i64::MAX, 1, i64::MAX),
        (1, i64::MAX, i64::MAX),
        (-3, 2, 2),
        (-3, -1, 0),
        (1, i64::MIN, 0),
    ] {
        let mut parameters = count_parameters("source_1", delta).unwrap();
        parameters.insert("count".into(), Value::Int(current));
        transaction
            .query_request(bounded_batch_query(APPLY, &parameters))
            .unwrap();
        let query = if delta > 0 { INCREMENT } else { DECREMENT };
        let output = transaction
            .query_request(bounded_batch_query(query, &parameters))
            .unwrap();
        assert_eq!(output.rows.len(), 1);
        assert_eq!(output.rows[0]["count"], Value::Int(expected));
        parameters.insert("count".into(), output.rows[0]["count"].clone());
        assert_eq!(
            transaction
                .query_request(bounded_batch_query(APPLY, &parameters))
                .unwrap()
                .rows,
            output.rows
        );
    }
    transaction.commit().unwrap();
}

#[test]
fn source_memory_count_batch_rejects_zero_delta_before_wal() {
    let path = unique_test_dir("source_memory_count_invalid_query_inputs");
    let mut db = Database::open(&path).unwrap();
    db.query("CREATE (:Source {id: 'source_1', memory_count: 1})")
        .unwrap();
    let epoch = db.commit_epoch().unwrap();
    let wal = read_test_wal(&path).unwrap();
    for (id, delta, message) in [
        ("source_1", 0, "non-zero delta"),
        ("", 1, "non-empty source id"),
    ] {
        let error = count_parameters(id, delta).unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
    }
    assert_eq!(db.commit_epoch().unwrap(), epoch);
    assert_eq!(read_test_wal(&path).unwrap(), wal);
    drop(db);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn typed_source_memory_count_batch_persists_as_one_wal_batch_and_replays() {
    let path = unique_test_dir("typed_source_memory_count_batch_wal_replay");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Source {id: 'source_1', memory_count: 0})")
            .unwrap();
        db.query("CREATE (:Source {id: 'source_2', memory_count: 2})")
            .unwrap();
        let before = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        let mut transaction = db.begin_transaction().unwrap();
        for (query, id, delta) in [(INCREMENT, "source_1", 1), (DECREMENT, "source_2", -1)] {
            let mut parameters = count_parameters(id, delta).unwrap();
            let output = transaction
                .query_request(bounded_batch_query(query, &parameters))
                .unwrap();
            assert_eq!(output.rows.len(), 1);
            assert_eq!(output.rows[0]["count"], Value::Int(1));
            parameters.insert("count".into(), output.rows[0]["count"].clone());
            assert_eq!(
                transaction
                    .query_request(bounded_batch_query(APPLY, &parameters))
                    .unwrap()
                    .rows,
                output.rows
            );
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
            .query_request(bounded_batch_query(READ, &BTreeMap::new()))
            .unwrap();
        assert_eq!(rows.rows.len(), 2);
        for (index, id) in ["source_1", "source_2"].into_iter().enumerate() {
            assert_eq!(rows.rows[index]["id"], Value::String(id.into()));
            assert_eq!(rows.rows[index]["count"], Value::Int(1));
        }
    }
    std::fs::remove_dir_all(path).unwrap();
}

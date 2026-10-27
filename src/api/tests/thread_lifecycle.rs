// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;

const THREAD_READ: &str = "MATCH (t:Thread) RETURN t.id AS id, t.metadata AS metadata, \
    t.message_count AS message_count, t.updated_at AS updated_at ORDER BY id";
const METADATA_UPDATE: &str =
    "MATCH (t:Thread {id: $id}) SET t.metadata = $metadata RETURN id(t) AS node_id";
const COUNT_UPDATE: &str =
    "MATCH (t:Thread {id: $id}) SET t.message_count = $count RETURN id(t) AS node_id";
const TIMESTAMP_UPDATE: &str =
    "MATCH (t:Thread {id: $id}) SET t.updated_at = $updated_at RETURN id(t) AS node_id";
const ADVANCE_TIMESTAMP: &str = "MATCH (t:Thread {id: $id}) \
    WHERE t.updated_at IS NULL OR t.updated_at <= $updated_at \
    SET t.updated_at = $updated_at RETURN id(t) AS node_id";

fn thread_parameters(id: &str) -> Result<BTreeMap<String, Value>> {
    if id.is_empty() {
        return Err(HawDBError::Semantic("non-empty thread id required".into()));
    }
    Ok(BTreeMap::from([("id".into(), Value::String(id.into()))]))
}

fn metadata_parameters(
    id: &str,
    metadata: &str,
    timestamp: Option<i64>,
) -> Result<BTreeMap<String, Value>> {
    let mut parameters = thread_parameters(id)?;
    parameters.insert("metadata".into(), Value::String(metadata.into()));
    if let Some(timestamp) = timestamp {
        parameters.insert("updated_at".into(), Value::Int(timestamp));
    }
    Ok(parameters)
}

fn count_parameters(
    id: &str,
    count: i64,
    timestamp: Option<i64>,
) -> Result<BTreeMap<String, Value>> {
    let mut parameters = thread_parameters(id)?;
    if count < 0 {
        return Err(HawDBError::Semantic(
            "non-negative message count required".into(),
        ));
    }
    parameters.insert("count".into(), Value::Int(count));
    if let Some(timestamp) = timestamp {
        parameters.insert("updated_at".into(), Value::Int(timestamp));
    }
    Ok(parameters)
}

#[test]
fn updates_thread_metadata_batch_for_nowledge_shapes() {
    let mut db = Database::new();
    db.query("CREATE (:Thread {id: 'thread_1', metadata: '{}', updated_at: 1})")
        .unwrap();
    db.query("CREATE (:Thread {id: 'thread_2', metadata: '{}', updated_at: 1})")
        .unwrap();
    let before = db.commit_epoch().unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    let mut seen = BTreeSet::new();
    let mut counts = Vec::new();
    let mut duplicates = 0;
    for (id, metadata, timestamp) in [
        ("thread_1", "{\"summary\":\"ready\"}", Some(100)),
        ("thread_2", "{\"summary\":\"metadata-only\"}", None),
        ("thread_2", "{\"duplicate\":true}", Some(200)),
        ("missing", "{}", Some(300)),
    ] {
        let parameters = metadata_parameters(id, metadata, timestamp).unwrap();
        if !seen.insert(id) {
            duplicates += 1;
            continue;
        }
        let output = transaction
            .query_request(bounded_batch_query(METADATA_UPDATE, &parameters))
            .unwrap();
        counts.push(output.rows.len());
        if timestamp.is_some() {
            let updated = transaction
                .query_request(bounded_batch_query(TIMESTAMP_UPDATE, &parameters))
                .unwrap();
            assert_eq!(updated.rows.len(), output.rows.len());
        }
    }
    assert_eq!(counts, [1, 1, 0]);
    assert_eq!(duplicates, 1);
    transaction.commit().unwrap();
    assert_eq!(before, 2);
    assert_eq!(db.commit_epoch().unwrap(), before + 1);
    let output = db
        .query_request(bounded_batch_query(THREAD_READ, &BTreeMap::new()))
        .unwrap();
    assert_eq!(output.rows.len(), 2);
    assert_eq!(output.rows[0]["id"], Value::String("thread_1".into()));
    assert_eq!(
        output.rows[0]["metadata"],
        Value::String("{\"summary\":\"ready\"}".into())
    );
    assert_eq!(output.rows[0]["updated_at"], Value::Int(100));
    assert_eq!(output.rows[1]["id"], Value::String("thread_2".into()));
    assert_eq!(
        output.rows[1]["metadata"],
        Value::String("{\"summary\":\"metadata-only\"}".into())
    );
    assert_eq!(output.rows[1]["updated_at"], Value::Int(1));
}

#[test]
fn thread_metadata_batch_rejects_empty_thread_id_before_wal() {
    let path = unique_test_dir("thread_metadata_query_empty_id");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Thread {id: 'thread_1', metadata: '{}', updated_at: 1})")
            .unwrap();
        let before = db.commit_epoch().unwrap();
        let wal = read_test_wal(&path).unwrap();
        assert!(metadata_parameters("", "{}", Some(100))
            .unwrap_err()
            .to_string()
            .contains("non-empty thread id"));
        assert_eq!(db.commit_epoch().unwrap(), before);
        assert_eq!(read_test_wal(&path).unwrap(), wal);
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn thread_metadata_queries_persist_as_one_wal_batch_and_replay() {
    let path = unique_test_dir("thread_metadata_query_replay");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Thread {id: 'thread_1', metadata: '{}', updated_at: 1})")
            .unwrap();
        db.query("CREATE (:Thread {id: 'thread_2', metadata: '{}', updated_at: 1})")
            .unwrap();
        let before = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        let mut transaction = db.begin_transaction().unwrap();
        for (id, metadata, timestamp) in [
            ("thread_1", "{\"summary\":\"ready\"}", Some(100)),
            ("thread_2", "{\"summary\":\"metadata-only\"}", None),
        ] {
            let parameters = metadata_parameters(id, metadata, timestamp).unwrap();
            assert_eq!(
                transaction
                    .query_request(bounded_batch_query(METADATA_UPDATE, &parameters))
                    .unwrap()
                    .rows
                    .len(),
                1
            );
            if timestamp.is_some() {
                assert_eq!(
                    transaction
                        .query_request(bounded_batch_query(TIMESTAMP_UPDATE, &parameters))
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
        let output = db
            .query_request(bounded_batch_query(THREAD_READ, &BTreeMap::new()))
            .unwrap();
        assert_eq!(output.rows.len(), 2);
        assert_eq!(output.rows[0]["id"], Value::String("thread_1".into()));
        assert_eq!(
            output.rows[0]["metadata"],
            Value::String("{\"summary\":\"ready\"}".into())
        );
        assert_eq!(output.rows[0]["updated_at"], Value::Int(100));
        assert_eq!(output.rows[1]["id"], Value::String("thread_2".into()));
        assert_eq!(
            output.rows[1]["metadata"],
            Value::String("{\"summary\":\"metadata-only\"}".into())
        );
        assert_eq!(output.rows[1]["updated_at"], Value::Int(1));
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn updates_thread_message_count_batch_with_preserve_newer_timestamp() {
    let mut db = Database::new();
    db.query("CREATE (:Thread {id: 'thread_1', message_count: 1, updated_at: 200})")
        .unwrap();
    db.query("CREATE (:Thread {id: 'thread_2', message_count: 1, updated_at: 100})")
        .unwrap();
    db.query("CREATE (:Thread {id: 'thread_3', message_count: 1, updated_at: 10})")
        .unwrap();
    let before = db.commit_epoch().unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    let mut seen = BTreeSet::new();
    let mut counts = Vec::new();
    let mut changed_timestamps = Vec::new();
    let mut duplicates = 0;
    for (id, count, timestamp, preserve) in [
        ("thread_1", 7, Some(150), true),
        ("thread_2", 8, Some(150), true),
        ("thread_3", 9, None, false),
        ("thread_2", 10, Some(300), false),
        ("missing", 1, Some(100), false),
    ] {
        let parameters = count_parameters(id, count, timestamp).unwrap();
        if !seen.insert(id) {
            duplicates += 1;
            continue;
        }
        counts.push(
            transaction
                .query_request(bounded_batch_query(COUNT_UPDATE, &parameters))
                .unwrap()
                .rows
                .len(),
        );
        if timestamp.is_some() {
            let query = if preserve {
                ADVANCE_TIMESTAMP
            } else {
                TIMESTAMP_UPDATE
            };
            changed_timestamps.push(
                transaction
                    .query_request(bounded_batch_query(query, &parameters))
                    .unwrap()
                    .rows
                    .len(),
            );
        }
    }
    assert_eq!(counts, [1, 1, 1, 0]);
    assert_eq!(changed_timestamps, [0, 1, 0]);
    assert_eq!(duplicates, 1);
    transaction.commit().unwrap();
    assert_eq!(before, 3);
    assert_eq!(db.commit_epoch().unwrap(), before + 1);
    let output = db
        .query_request(bounded_batch_query(THREAD_READ, &BTreeMap::new()))
        .unwrap();
    assert_eq!(output.rows.len(), 3);
    for (index, (id, count, timestamp)) in [
        ("thread_1", 7, 200),
        ("thread_2", 8, 150),
        ("thread_3", 9, 10),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(output.rows[index]["id"], Value::String(id.into()));
        assert_eq!(output.rows[index]["message_count"], Value::Int(count));
        assert_eq!(output.rows[index]["updated_at"], Value::Int(timestamp));
    }
}

#[test]
fn thread_message_count_batch_rejects_negative_count_before_wal() {
    let path = unique_test_dir("thread_count_query_negative_count");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Thread {id: 'thread_1', message_count: 1})")
            .unwrap();
        let before = db.commit_epoch().unwrap();
        let wal = read_test_wal(&path).unwrap();
        assert!(count_parameters("thread_1", -1, Some(100))
            .unwrap_err()
            .to_string()
            .contains("non-negative message count"));
        assert_eq!(db.commit_epoch().unwrap(), before);
        assert_eq!(read_test_wal(&path).unwrap(), wal);
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn thread_message_count_queries_persist_as_one_wal_batch_and_replay() {
    let path = unique_test_dir("thread_count_query_replay");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Thread {id: 'thread_1', message_count: 1, updated_at: 200})")
            .unwrap();
        db.query("CREATE (:Thread {id: 'thread_2', message_count: 1, updated_at: 100})")
            .unwrap();
        let before = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        let mut transaction = db.begin_transaction().unwrap();
        for (id, count) in [("thread_1", 7), ("thread_2", 8)] {
            let parameters = count_parameters(id, count, Some(150)).unwrap();
            assert_eq!(
                transaction
                    .query_request(bounded_batch_query(COUNT_UPDATE, &parameters))
                    .unwrap()
                    .rows
                    .len(),
                1
            );
            let timestamp = transaction
                .query_request(bounded_batch_query(ADVANCE_TIMESTAMP, &parameters))
                .unwrap();
            assert_eq!(timestamp.rows.len(), usize::from(id == "thread_2"));
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
        let output = db
            .query_request(bounded_batch_query(THREAD_READ, &BTreeMap::new()))
            .unwrap();
        assert_eq!(output.rows.len(), 2);
        for (index, (id, count, timestamp)) in [("thread_1", 7, 200), ("thread_2", 8, 150)]
            .into_iter()
            .enumerate()
        {
            assert_eq!(output.rows[index]["id"], Value::String(id.into()));
            assert_eq!(output.rows[index]["message_count"], Value::Int(count));
            assert_eq!(output.rows[index]["updated_at"], Value::Int(timestamp));
        }
    }
    std::fs::remove_dir_all(path).unwrap();
}

const MESSAGE_DELETE: &str = "MATCH (t:Thread {id: $id})-[:CONTAINS]->(m:Message) DETACH DELETE m";
const MESSAGE_READ: &str =
    "MATCH (t:Thread {id: $id})-[:CONTAINS]->(m:Message) RETURN id(m) AS node_id ORDER BY node_id";
const THREAD_EXISTS: &str = "MATCH (t:Thread {id: $id}) RETURN id(t) AS node_id";

#[test]
fn deletes_thread_messages_for_nowledge_cleanup_shape() {
    let mut db = Database::new();
    db.query("CREATE (:Thread {id: 'thread_1'})-[:CONTAINS {order_index: 1}]->(:Message {id: 'message_1', order_index: 1})").unwrap();
    db.query("MATCH (t:Thread {id: 'thread_1'}), (m:Message {id: 'message_1'}) CREATE (t)-[:CONTAINS {order_index: 2}]->(m)").unwrap();
    db.query("CREATE (:Message {id: 'message_2', order_index: 3})")
        .unwrap();
    db.query("MATCH (t:Thread {id: 'thread_1'}), (m:Message {id: 'message_2'}) CREATE (t)-[:CONTAINS {order_index: 3}]->(m)").unwrap();
    db.query("CREATE (:Thread {id: 'thread_2'})-[:CONTAINS]->(:Message {id: 'message_other'})")
        .unwrap();
    let before = db.commit_epoch().unwrap();
    let parameters = thread_parameters("thread_1").unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    let matched = transaction
        .query_request(bounded_batch_query(MESSAGE_READ, &parameters))
        .unwrap();
    assert_eq!(matched.rows.len(), 3);
    assert_eq!(matched.rows[0]["node_id"], Value::Int(1));
    assert_eq!(matched.rows[1]["node_id"], Value::Int(1));
    assert_eq!(matched.rows[2]["node_id"], Value::Int(2));
    transaction
        .query_request(bounded_batch_query(MESSAGE_DELETE, &parameters))
        .unwrap();
    assert!(transaction
        .query_request(bounded_batch_query(MESSAGE_READ, &parameters))
        .unwrap()
        .rows
        .is_empty());
    assert_eq!(transaction.commit().unwrap().rows.len(), 2);
    assert_eq!(db.commit_epoch().unwrap(), before + 1);
    let retained = db
        .query_request(bounded_batch_query(THREAD_EXISTS, &parameters))
        .unwrap();
    assert_eq!(retained.rows.len(), 1);
    assert_eq!(retained.rows[0]["node_id"], Value::Int(0));
    let remaining = db
        .query_request(bounded_batch_query(
            "MATCH (m:Message) RETURN m.id AS id ORDER BY id",
            &BTreeMap::new(),
        ))
        .unwrap();
    assert_eq!(remaining.rows.len(), 1);
    assert_eq!(
        remaining.rows[0]["id"],
        Value::String("message_other".into())
    );
    for (id, expected_threads) in [("missing", 0), ("thread_1", 1)] {
        let parameters = thread_parameters(id).unwrap();
        let mut transaction = db.begin_transaction().unwrap();
        assert_eq!(
            transaction
                .query_request(bounded_batch_query(THREAD_EXISTS, &parameters))
                .unwrap()
                .rows
                .len(),
            expected_threads
        );
        assert!(transaction
            .query_request(bounded_batch_query(MESSAGE_READ, &parameters))
            .unwrap()
            .rows
            .is_empty());
        transaction
            .query_request(bounded_batch_query(MESSAGE_DELETE, &parameters))
            .unwrap();
        assert!(transaction.commit().unwrap().rows.is_empty());
        assert_eq!(db.commit_epoch().unwrap(), before + 1);
    }
}

#[test]
fn thread_message_delete_rejects_empty_thread_id_before_wal() {
    let path = unique_test_dir("thread_messages_query_empty_id");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Thread {id: 'thread_1'})-[:CONTAINS]->(:Message {id: 'message_1'})")
            .unwrap();
        let before = db.commit_epoch().unwrap();
        let wal = read_test_wal(&path).unwrap();
        assert!(thread_parameters("")
            .unwrap_err()
            .to_string()
            .contains("non-empty thread id"));
        assert_eq!(db.commit_epoch().unwrap(), before);
        assert_eq!(read_test_wal(&path).unwrap(), wal);
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn thread_message_delete_query_persists_as_one_wal_batch_and_replays() {
    let path = unique_test_dir("thread_messages_query_replay");
    let parameters = thread_parameters("thread_1").unwrap();
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Thread {id: 'thread_1'})-[:CONTAINS]->(:Message {id: 'message_1'})")
            .unwrap();
        db.query("CREATE (:Message {id: 'message_2'})").unwrap();
        db.query("MATCH (t:Thread {id: 'thread_1'}), (m:Message {id: 'message_2'}) CREATE (t)-[:CONTAINS]->(m)").unwrap();
        let before = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        let mut transaction = db.begin_transaction().unwrap();
        transaction
            .query_request(bounded_batch_query(MESSAGE_DELETE, &parameters))
            .unwrap();
        assert_eq!(transaction.commit().unwrap().rows.len(), 2);
        assert_eq!(
            read_test_wal(&path).unwrap().matches("\tbatch\t").count(),
            before + 1
        );
    }
    assert!(read_test_wal(&path).unwrap().contains("delete_node"));
    {
        let mut db = Database::open(&path).unwrap();
        assert_eq!(
            db.query_request(bounded_batch_query(THREAD_EXISTS, &parameters))
                .unwrap()
                .rows
                .len(),
            1
        );
        let count = db
            .query_request(bounded_batch_query(
                "MATCH (m:Message) RETURN count(m) AS total",
                &BTreeMap::new(),
            ))
            .unwrap();
        assert_eq!(count.rows[0]["total"], Value::Int(0));
    }
    std::fs::remove_dir_all(path).unwrap();
}

const COMPACTION_CREATE: &str = "MATCH (t:Thread {id: $id}), (m:Memory {id: $memory}) \
    CREATE (t)-[:COMPACTS_TO {compaction_method: $method, created_at: $created_at, properties: $properties}]->(m)";
const COMPACTION_READ: &str =
    "MATCH (t:Thread {id: $id})-[r:COMPACTS_TO]->(m:Memory {id: $memory}) \
    RETURN id(t) AS thread_node_id, id(m) AS memory_node_id, m.id AS memory_id, \
    r.compaction_method AS method, r.created_at AS created_at, r.properties AS properties";

fn compaction_parameters(
    id: &str,
    memory: &str,
    method: &str,
    created_at: i64,
    properties: &str,
) -> Result<BTreeMap<String, Value>> {
    let mut parameters = thread_parameters(id)?;
    if memory.is_empty() {
        return Err(HawDBError::Semantic("non-empty memory id required".into()));
    }
    if method.is_empty() {
        return Err(HawDBError::Semantic(
            "non-empty compaction method required".into(),
        ));
    }
    parameters.extend([
        ("memory".into(), Value::String(memory.into())),
        ("method".into(), Value::String(method.into())),
        ("created_at".into(), Value::Int(created_at)),
        ("properties".into(), Value::String(properties.into())),
    ]);
    Ok(parameters)
}

#[test]
fn creates_thread_compaction_link_for_nowledge_distill_shape() {
    let mut db = Database::new();
    db.query("CREATE (:Thread {id: 'thread_1', thread_id: 'logical_1'})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'memory_1', title: 'Memory One', importance: 1.0})")
        .unwrap();
    db.query("CREATE (:Memory {id: 'memory_2'})").unwrap();
    db.query("CREATE (:Thread {title: 'Idless Thread'})")
        .unwrap();
    let before = db.commit_epoch().unwrap();
    let parameters = compaction_parameters(
        "thread_1",
        "memory_1",
        "manual_distillation",
        1_700_000_070,
        "{\"mode\":\"manual\"}",
    )
    .unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    transaction
        .query_request(bounded_batch_query(COMPACTION_CREATE, &parameters))
        .unwrap();
    let output = transaction
        .query_request(bounded_batch_query(COMPACTION_READ, &parameters))
        .unwrap();
    assert_eq!(output.rows.len(), 1);
    assert_eq!(output.rows[0]["thread_node_id"], Value::Int(0));
    assert_eq!(output.rows[0]["memory_node_id"], Value::Int(1));
    assert_eq!(
        output.rows[0]["memory_id"],
        Value::String("memory_1".into())
    );
    assert_eq!(
        output.rows[0]["method"],
        Value::String("manual_distillation".into())
    );
    assert_eq!(output.rows[0]["created_at"], Value::Int(1_700_000_070));
    assert_eq!(
        output.rows[0]["properties"],
        Value::String("{\"mode\":\"manual\"}".into())
    );
    assert_eq!(transaction.commit().unwrap().rows.len(), 1);
    assert_eq!(db.commit_epoch().unwrap(), before + 1);
    for (id, memory, created_at) in [("thread_1", "missing", 1), ("3", "memory_2", 2)] {
        let parameters =
            compaction_parameters(id, memory, "manual_distillation", created_at, "{}").unwrap();
        let mut transaction = db.begin_transaction().unwrap();
        transaction
            .query_request(bounded_batch_query(COMPACTION_CREATE, &parameters))
            .unwrap();
        assert!(transaction
            .query_request(bounded_batch_query(COMPACTION_READ, &parameters))
            .unwrap()
            .rows
            .is_empty());
        assert!(transaction.commit().unwrap().rows.is_empty());
        assert_eq!(db.commit_epoch().unwrap(), before + 1);
    }
    let retained = db
        .query_request(bounded_batch_query(
            "MATCH (t:Thread) WHERE t.id IS NULL RETURN id(t) AS node_id, t.title AS title",
            &BTreeMap::new(),
        ))
        .unwrap();
    assert_eq!(retained.rows.len(), 1);
    assert_eq!(retained.rows[0]["node_id"], Value::Int(3));
    assert_eq!(
        retained.rows[0]["title"],
        Value::String("Idless Thread".into())
    );
    let count = db
        .query_request(bounded_batch_query(
            "MATCH (:Thread)-[r:COMPACTS_TO]->(:Memory) RETURN count(r) AS total",
            &BTreeMap::new(),
        ))
        .unwrap();
    assert_eq!(count.rows[0]["total"], Value::Int(1));
}

#[test]
fn thread_compaction_link_rejects_empty_fields_before_wal() {
    let path = unique_test_dir("thread_compaction_query_empty_fields");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Thread {id: 'thread_1'})").unwrap();
        db.query("CREATE (:Memory {id: 'memory_1'})").unwrap();
        let before = db.commit_epoch().unwrap();
        let wal = read_test_wal(&path).unwrap();
        for (id, memory, method, message) in [
            ("", "memory_1", "manual_distillation", "non-empty thread id"),
            ("thread_1", "", "manual_distillation", "non-empty memory id"),
            ("thread_1", "memory_1", "", "non-empty compaction method"),
        ] {
            assert!(compaction_parameters(id, memory, method, 1, "{}")
                .unwrap_err()
                .to_string()
                .contains(message));
        }
        assert_eq!(db.commit_epoch().unwrap(), before);
        assert_eq!(read_test_wal(&path).unwrap(), wal);
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn thread_compaction_query_persists_as_one_wal_batch_and_replays() {
    let path = unique_test_dir("thread_compaction_query_replay");
    let parameters = compaction_parameters(
        "thread_1",
        "memory_1",
        "manual_distillation",
        10,
        "{\"mode\":\"manual\"}",
    )
    .unwrap();
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Thread {id: 'thread_1'})").unwrap();
        db.query("CREATE (:Memory {id: 'memory_1'})").unwrap();
        let before = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        let mut transaction = db.begin_transaction().unwrap();
        transaction
            .query_request(bounded_batch_query(COMPACTION_CREATE, &parameters))
            .unwrap();
        assert_eq!(transaction.commit().unwrap().rows.len(), 1);
        assert_eq!(
            read_test_wal(&path).unwrap().matches("\tbatch\t").count(),
            before + 1
        );
    }
    assert!(read_test_wal(&path).unwrap().contains("create_rel"));
    {
        let mut db = Database::open(&path).unwrap();
        let output = db
            .query_request(bounded_batch_query(COMPACTION_READ, &parameters))
            .unwrap();
        assert_eq!(output.rows.len(), 1);
        assert_eq!(output.rows[0]["thread_node_id"], Value::Int(0));
        assert_eq!(output.rows[0]["memory_node_id"], Value::Int(1));
        assert_eq!(
            output.rows[0]["memory_id"],
            Value::String("memory_1".into())
        );
        assert_eq!(
            output.rows[0]["method"],
            Value::String("manual_distillation".into())
        );
        assert_eq!(output.rows[0]["created_at"], Value::Int(10));
        assert_eq!(
            output.rows[0]["properties"],
            Value::String("{\"mode\":\"manual\"}".into())
        );
    }
    std::fs::remove_dir_all(path).unwrap();
}

const IDENTITY_EXACT_DELETE: &str = "MATCH (i:ThreadIdentity {id: $identity}) DETACH DELETE i";
const IDENTITY_EXACT_READ: &str =
    "MATCH (i:ThreadIdentity {id: $identity}) RETURN id(i) AS node_id";
const IDENTITY_CASCADE_DELETE: &str = "MATCH (i:ThreadIdentity) \
    WHERE i.id = $public OR i.id = $input OR i.thread_node_id = $uuid DETACH DELETE i";
const IDENTITY_CASCADE_READ: &str = "MATCH (i:ThreadIdentity) \
    WHERE i.id = $public OR i.id = $input OR i.thread_node_id = $uuid RETURN id(i) AS node_id ORDER BY node_id";

fn identity_parameters(
    identity: Option<&str>,
    cascade: Option<(&str, &str, &str)>,
) -> Result<BTreeMap<String, Value>> {
    match (identity, cascade) {
        (Some(key), None) if !key.is_empty() => Ok(BTreeMap::from([(
            "identity".into(),
            Value::String(key.into()),
        )])),
        (Some(_), None) => Err(HawDBError::Semantic(
            "non-empty identity key required".into(),
        )),
        (None, Some((public, input, uuid)))
            if !public.is_empty() && !input.is_empty() && !uuid.is_empty() =>
        {
            Ok(BTreeMap::from([
                ("public".into(), Value::String(public.into())),
                ("input".into(), Value::String(input.into())),
                ("uuid".into(), Value::String(uuid.into())),
            ]))
        }
        (None, Some(_)) => Err(HawDBError::Semantic(
            "non-empty cascade keys required".into(),
        )),
        _ => Err(HawDBError::Semantic(
            "exactly one delete mode required".into(),
        )),
    }
}

#[test]
fn deletes_thread_identities_for_nowledge_compensation_and_cascade_shapes() {
    let mut db = Database::new();
    db.query("CREATE (:ThreadIdentity {id: 'identity_compensation', thread_node_id: 'thread_a', thread_id: 'logical_a'})").unwrap();
    db.query("CREATE (:ThreadIdentity {id: 'identity_public', thread_node_id: 'thread_other_1', thread_id: 'logical_public'})").unwrap();
    db.query("CREATE (:ThreadIdentity {id: 'identity_input', thread_node_id: 'thread_other_2', thread_id: 'logical_input'})").unwrap();
    db.query("CREATE (:ThreadIdentity {id: 'identity_by_node', thread_node_id: 'thread_uuid', thread_id: 'logical_node'})").unwrap();
    db.query("CREATE (:ThreadIdentity {id: 'identity_duplicate', thread_node_id: 'thread_uuid', thread_id: 'logical_duplicate'})").unwrap();
    db.query("CREATE (:ThreadIdentity {id: 'identity_survivor', thread_node_id: 'survivor_uuid', thread_id: 'logical_survivor'})").unwrap();
    let before = db.commit_epoch().unwrap();
    let exact = identity_parameters(Some("identity_compensation"), None).unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    let matched = transaction
        .query_request(bounded_batch_query(IDENTITY_EXACT_READ, &exact))
        .unwrap();
    assert_eq!(matched.rows.len(), 1);
    assert_eq!(matched.rows[0]["node_id"], Value::Int(0));
    transaction
        .query_request(bounded_batch_query(IDENTITY_EXACT_DELETE, &exact))
        .unwrap();
    assert_eq!(transaction.commit().unwrap().rows.len(), 1);
    assert_eq!(db.commit_epoch().unwrap(), before + 1);
    let cascade = identity_parameters(
        None,
        Some(("identity_public", "identity_input", "thread_uuid")),
    )
    .unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    let matched = transaction
        .query_request(bounded_batch_query(IDENTITY_CASCADE_READ, &cascade))
        .unwrap();
    assert_eq!(matched.rows.len(), 4);
    for (index, row) in matched.rows.iter().enumerate() {
        assert_eq!(row["node_id"], Value::Int(index as i64 + 1));
    }
    transaction
        .query_request(bounded_batch_query(IDENTITY_CASCADE_DELETE, &cascade))
        .unwrap();
    assert_eq!(transaction.commit().unwrap().rows.len(), 4);
    assert_eq!(db.commit_epoch().unwrap(), before + 2);
    let remaining = db.query_request(bounded_batch_query("MATCH (i:ThreadIdentity) RETURN id(i) AS node_id, i.id AS id, i.thread_node_id AS thread_node_id, i.thread_id AS thread_id ORDER BY id", &BTreeMap::new())).unwrap();
    assert_eq!(remaining.rows.len(), 1);
    assert_eq!(remaining.rows[0]["node_id"], Value::Int(5));
    assert_eq!(
        remaining.rows[0]["id"],
        Value::String("identity_survivor".into())
    );
    assert_eq!(
        remaining.rows[0]["thread_node_id"],
        Value::String("survivor_uuid".into())
    );
    assert_eq!(
        remaining.rows[0]["thread_id"],
        Value::String("logical_survivor".into())
    );
    let missing = identity_parameters(Some("missing"), None).unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    assert!(transaction
        .query_request(bounded_batch_query(IDENTITY_EXACT_READ, &missing))
        .unwrap()
        .rows
        .is_empty());
    transaction
        .query_request(bounded_batch_query(IDENTITY_EXACT_DELETE, &missing))
        .unwrap();
    assert!(transaction.commit().unwrap().rows.is_empty());
    assert_eq!(db.commit_epoch().unwrap(), before + 2);
}

#[test]
fn thread_identity_delete_rejects_invalid_modes_before_wal() {
    let path = unique_test_dir("thread_identity_query_invalid_modes");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:ThreadIdentity {id: 'identity_1'})")
            .unwrap();
        let before = db.commit_epoch().unwrap();
        let wal = read_test_wal(&path).unwrap();
        assert!(identity_parameters(Some(""), None)
            .unwrap_err()
            .to_string()
            .contains("non-empty identity key"));
        assert!(identity_parameters(None, Some(("public", "", "thread")))
            .unwrap_err()
            .to_string()
            .contains("non-empty cascade keys"));
        assert!(identity_parameters(None, None)
            .unwrap_err()
            .to_string()
            .contains("exactly one delete mode"));
        assert!(
            identity_parameters(Some("identity_1"), Some(("public", "input", "thread")))
                .unwrap_err()
                .to_string()
                .contains("exactly one delete mode")
        );
        assert_eq!(db.commit_epoch().unwrap(), before);
        assert_eq!(read_test_wal(&path).unwrap(), wal);
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn thread_identity_delete_query_persists_as_one_wal_batch_and_replays() {
    let path = unique_test_dir("thread_identity_query_replay");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:ThreadIdentity {id: 'identity_public', thread_node_id: 'thread_other', thread_id: 'logical_public'})").unwrap();
        db.query("CREATE (:ThreadIdentity {id: 'identity_by_node', thread_node_id: 'thread_uuid', thread_id: 'logical_node'})").unwrap();
        let before = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        let parameters = identity_parameters(
            None,
            Some(("identity_public", "missing_input", "thread_uuid")),
        )
        .unwrap();
        let mut transaction = db.begin_transaction().unwrap();
        assert_eq!(
            transaction
                .query_request(bounded_batch_query(IDENTITY_CASCADE_READ, &parameters))
                .unwrap()
                .rows
                .len(),
            2
        );
        transaction
            .query_request(bounded_batch_query(IDENTITY_CASCADE_DELETE, &parameters))
            .unwrap();
        assert_eq!(transaction.commit().unwrap().rows.len(), 2);
        assert_eq!(
            read_test_wal(&path).unwrap().matches("\tbatch\t").count(),
            before + 1
        );
    }
    assert!(read_test_wal(&path).unwrap().contains("delete_node"));
    {
        let mut db = Database::open(&path).unwrap();
        let output = db
            .query_request(bounded_batch_query(
                "MATCH (i:ThreadIdentity) RETURN count(i) AS total",
                &BTreeMap::new(),
            ))
            .unwrap();
        assert_eq!(output.rows[0]["total"], Value::Int(0));
    }
    std::fs::remove_dir_all(path).unwrap();
}

// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;

const LABEL_METADATA: &str = "MATCH (l:Label {id: $id}) SET l.metadata = $metadata, l.updated_at = $updated_at RETURN id(l) AS node_id";
const LABEL_CANONICAL: &str =
    "MATCH (l:Label {id: $id}) SET l.canonical_name = $canonical_name RETURN id(l) AS node_id";
const LABEL_RENAME: &str = "MATCH (l:Label {id: $id}) SET l.name = $name, l.canonical_name = $canonical_name, l.updated_at = $updated_at RETURN id(l) AS node_id";
const LABEL_READ: &str = "MATCH (l:Label) RETURN l.id AS id, l.name AS name, \
    l.canonical_name AS canonical_name, l.metadata AS metadata, l.updated_at AS updated_at ORDER BY id";

fn label_parameters(id: &str, fields: &[(&str, Value)]) -> Result<BTreeMap<String, Value>> {
    if id.is_empty() {
        return Err(HawDBError::Semantic("non-empty label id required".into()));
    }
    if !fields
        .iter()
        .any(|(key, _)| matches!(*key, "name" | "canonical_name" | "metadata"))
    {
        return Err(HawDBError::Semantic(
            "at least one lifecycle field required".into(),
        ));
    }
    for (key, value) in fields {
        if matches!(*key, "name" | "canonical_name")
            && matches!(value, Value::String(value) if value.is_empty())
        {
            return Err(HawDBError::Semantic(format!(
                "non-empty {} required",
                key.replace('_', " ")
            )));
        }
    }
    let mut parameters = BTreeMap::from([("id".into(), Value::String(id.into()))]);
    parameters.extend(
        fields
            .iter()
            .map(|(key, value)| ((*key).into(), value.clone())),
    );
    Ok(parameters)
}

#[test]
fn updates_label_lifecycle_batch_for_nowledge_shapes() {
    let mut db = Database::new();
    db.query("CREATE (:Label {id: 'label_meta', name: 'Meta', metadata: '{}', updated_at: 1})")
        .unwrap();
    db.query("CREATE (:Label {id: 'label_canonical', name: 'Canonical'})")
        .unwrap();
    db.query(
        "CREATE (:Label {id: 'label_rename', name: 'Old', canonical_name: 'old', updated_at: 1})",
    )
    .unwrap();
    let inputs = [
        (
            LABEL_METADATA,
            label_parameters(
                "label_meta",
                &[
                    ("metadata", Value::String("{\"owner\":\"mem\"}".into())),
                    ("updated_at", Value::Int(100)),
                ],
            )
            .unwrap(),
        ),
        (
            LABEL_CANONICAL,
            label_parameters(
                "label_canonical",
                &[("canonical_name", Value::String("canonical-label".into()))],
            )
            .unwrap(),
        ),
        (
            LABEL_RENAME,
            label_parameters(
                "label_rename",
                &[
                    ("name", Value::String("Renamed".into())),
                    ("canonical_name", Value::String("renamed".into())),
                    ("updated_at", Value::Int(200)),
                ],
            )
            .unwrap(),
        ),
        (
            LABEL_RENAME,
            label_parameters(
                "label_rename",
                &[
                    ("name", Value::String("Duplicate".into())),
                    ("canonical_name", Value::String("duplicate".into())),
                    ("updated_at", Value::Int(300)),
                ],
            )
            .unwrap(),
        ),
        (
            LABEL_CANONICAL,
            label_parameters(
                "missing",
                &[("canonical_name", Value::String("missing".into()))],
            )
            .unwrap(),
        ),
    ];
    let before = db.commit_epoch().unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    let mut seen = BTreeSet::new();
    let mut counts = Vec::new();
    let mut duplicates = 0;
    for (query, parameters) in &inputs {
        let Value::String(id) = &parameters["id"] else {
            unreachable!()
        };
        if !seen.insert(id) {
            duplicates += 1;
            continue;
        }
        counts.push(
            transaction
                .query_request(bounded_batch_query(query, parameters))
                .unwrap()
                .rows
                .len(),
        );
    }
    assert_eq!(counts, [1, 1, 1, 0]);
    assert_eq!(duplicates, 1);
    transaction.commit().unwrap();
    assert_eq!(before, 3);
    assert_eq!(db.commit_epoch().unwrap(), before + 1);
    let output = db
        .query_request(bounded_batch_query(LABEL_READ, &BTreeMap::new()))
        .unwrap();
    assert_eq!(output.rows.len(), 3);
    for (index, input) in [1, 0, 2].into_iter().enumerate() {
        for (key, value) in &inputs[input].1 {
            assert_eq!(&output.rows[index][key.as_str()], value, "{key}");
        }
    }
    assert_eq!(output.rows[0]["name"], Value::String("Canonical".into()));
    assert_eq!(output.rows[0]["metadata"], Value::Null);
    assert_eq!(output.rows[0]["updated_at"], Value::Null);
    assert_eq!(output.rows[1]["name"], Value::String("Meta".into()));
    assert_eq!(output.rows[1]["canonical_name"], Value::Null);
    assert_eq!(output.rows[2]["metadata"], Value::Null);
}

#[test]
fn label_lifecycle_batch_rejects_empty_canonical_name_before_wal() {
    let path = unique_test_dir("label_lifecycle_query_invalid_fields");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Label {id: 'label_1', name: 'Label'})")
            .unwrap();
        let before = db.commit_epoch().unwrap();
        let wal = read_test_wal(&path).unwrap();
        for (key, message) in [
            ("canonical_name", "non-empty canonical name"),
            ("name", "non-empty name"),
        ] {
            assert!(
                label_parameters("label_1", &[(key, Value::String(String::new()))])
                    .unwrap_err()
                    .to_string()
                    .contains(message)
            );
        }
        assert!(
            label_parameters("label_1", &[("updated_at", Value::Int(100))])
                .unwrap_err()
                .to_string()
                .contains("lifecycle field")
        );
        assert_eq!(db.commit_epoch().unwrap(), before);
        assert_eq!(read_test_wal(&path).unwrap(), wal);
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn label_lifecycle_queries_persist_as_one_wal_batch_and_replay() {
    let path = unique_test_dir("label_lifecycle_query_replay");
    let inputs = [
        (
            LABEL_METADATA,
            label_parameters(
                "label_meta",
                &[
                    ("metadata", Value::String("{\"owner\":\"mem\"}".into())),
                    ("updated_at", Value::Int(100)),
                ],
            )
            .unwrap(),
        ),
        (
            LABEL_RENAME,
            label_parameters(
                "label_rename",
                &[
                    ("name", Value::String("Renamed".into())),
                    ("canonical_name", Value::String("renamed".into())),
                    ("updated_at", Value::Int(200)),
                ],
            )
            .unwrap(),
        ),
    ];
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Label {id: 'label_meta', name: 'Meta', metadata: '{}', updated_at: 1})")
            .unwrap();
        db.query("CREATE (:Label {id: 'label_rename', name: 'Old', canonical_name: 'old', updated_at: 1})").unwrap();
        let before = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        let mut transaction = db.begin_transaction().unwrap();
        for (query, parameters) in &inputs {
            assert_eq!(
                transaction
                    .query_request(bounded_batch_query(query, parameters))
                    .unwrap()
                    .rows
                    .len(),
                1
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
        let output = db
            .query_request(bounded_batch_query(LABEL_READ, &BTreeMap::new()))
            .unwrap();
        assert_eq!(output.rows.len(), 2);
        for (index, (_, parameters)) in inputs.iter().enumerate() {
            for (key, value) in parameters {
                assert_eq!(&output.rows[index][key.as_str()], value, "{key}");
            }
        }
        assert_eq!(output.rows[0]["name"], Value::String("Meta".into()));
        assert_eq!(output.rows[1]["metadata"], Value::Null);
    }
    std::fs::remove_dir_all(path).unwrap();
}

const LABEL_DELETE_EXACT: &str =
    "MATCH (:Memory {id: $memory})-[r:HAS_LABEL]->(:Label {id: $label}) DELETE r";
const LABEL_DELETE_ALL: &str = "MATCH (:Memory {id: $memory})-[r:HAS_LABEL]->(:Label) DELETE r";
const LABEL_EDGES: &str = "MATCH (:Memory {id: $memory})-[r:HAS_LABEL]->(l:Label) RETURN id(r) AS relationship_id, l.id AS label_id ORDER BY label_id";

fn delete_parameters(memory: &str, label: Option<&str>) -> Result<BTreeMap<String, Value>> {
    if memory.is_empty() {
        return Err(HawDBError::Semantic("non-empty memory id required".into()));
    }
    let mut parameters = BTreeMap::from([("memory".into(), Value::String(memory.into()))]);
    if let Some(label) = label {
        if label.is_empty() {
            return Err(HawDBError::Semantic("non-empty label id required".into()));
        }
        parameters.insert("label".into(), Value::String(label.into()));
    }
    Ok(parameters)
}

#[test]
fn deletes_memory_labels_for_nowledge_label_cleanup_shapes() {
    let mut db = Database::new();
    db.query("CREATE (:Memory {id: 'memory_1'})").unwrap();
    db.query("CREATE (:Memory {id: 'memory_2'})").unwrap();
    db.query("CREATE (:Memory {title: 'Idless memory'})")
        .unwrap();
    db.query("CREATE (:Label {id: 'alpha'})").unwrap();
    db.query("CREATE (:Label {id: 'beta'})").unwrap();
    db.query("CREATE (:Label {id: 'gamma'})").unwrap();
    for (memory, label) in [
        ("memory_1", "alpha"),
        ("memory_1", "beta"),
        ("memory_1", "gamma"),
        ("memory_2", "alpha"),
    ] {
        let parameters = delete_parameters(memory, Some(label)).unwrap();
        db.query_request(bounded_batch_query(
            "MATCH (m:Memory {id: $memory}), (l:Label {id: $label}) CREATE (m)-[:HAS_LABEL]->(l)",
            &parameters,
        ))
        .unwrap();
    }
    let before = db.commit_epoch().unwrap();
    let parameters = delete_parameters("memory_1", Some("alpha")).unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    assert_eq!(
        transaction
            .query_request(bounded_batch_query(LABEL_EDGES, &parameters))
            .unwrap()
            .rows
            .len(),
        3
    );
    transaction
        .query_request(bounded_batch_query(LABEL_DELETE_EXACT, &parameters))
        .unwrap();
    let remaining = transaction
        .query_request(bounded_batch_query(LABEL_EDGES, &parameters))
        .unwrap();
    assert_eq!(remaining.rows.len(), 2);
    assert_eq!(remaining.rows[0]["label_id"], Value::String("beta".into()));
    assert_eq!(remaining.rows[1]["label_id"], Value::String("gamma".into()));
    assert_eq!(transaction.commit().unwrap().rows.len(), 1);
    assert_eq!(db.commit_epoch().unwrap(), before + 1);
    let parameters = delete_parameters("memory_1", None).unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    transaction
        .query_request(bounded_batch_query(LABEL_DELETE_ALL, &parameters))
        .unwrap();
    assert!(transaction
        .query_request(bounded_batch_query(LABEL_EDGES, &parameters))
        .unwrap()
        .rows
        .is_empty());
    assert_eq!(transaction.commit().unwrap().rows.len(), 2);
    assert_eq!(db.commit_epoch().unwrap(), before + 2);
    for (memory, label, query) in [
        ("memory_2", Some("missing"), LABEL_DELETE_EXACT),
        ("2", None, LABEL_DELETE_ALL),
        ("memory_1", None, LABEL_DELETE_ALL),
    ] {
        let parameters = delete_parameters(memory, label).unwrap();
        let mut transaction = db.begin_transaction().unwrap();
        transaction
            .query_request(bounded_batch_query(query, &parameters))
            .unwrap();
        assert!(transaction.commit().unwrap().rows.is_empty());
        assert_eq!(db.commit_epoch().unwrap(), before + 2);
    }
    let retained = db
        .query_request(bounded_batch_query(
            LABEL_EDGES,
            &delete_parameters("memory_2", None).unwrap(),
        ))
        .unwrap();
    assert_eq!(retained.rows.len(), 1);
    assert_eq!(retained.rows[0]["label_id"], Value::String("alpha".into()));
    let idless = db
        .query_request(bounded_batch_query(
            "MATCH (m:Memory) WHERE m.id IS NULL RETURN id(m) AS node_id, m.title AS title",
            &BTreeMap::new(),
        ))
        .unwrap();
    assert_eq!(idless.rows.len(), 1);
    assert_eq!(idless.rows[0]["node_id"], Value::Int(2));
    assert_eq!(
        idless.rows[0]["title"],
        Value::String("Idless memory".into())
    );
    let nodes = db
        .query_request(bounded_batch_query(
            "MATCH (l:Label) RETURN count(l) AS total",
            &BTreeMap::new(),
        ))
        .unwrap();
    assert_eq!(nodes.rows[0]["total"], Value::Int(3));
}

#[test]
fn memory_label_delete_rejects_empty_ids_before_wal() {
    let path = unique_test_dir("memory_label_query_empty_ids");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Memory {id: 'memory_1'})-[:HAS_LABEL]->(:Label {id: 'label_1'})")
            .unwrap();
        let before = db.commit_epoch().unwrap();
        let wal = read_test_wal(&path).unwrap();
        assert!(delete_parameters("", None)
            .unwrap_err()
            .to_string()
            .contains("non-empty memory id"));
        assert!(delete_parameters("memory_1", Some(""))
            .unwrap_err()
            .to_string()
            .contains("non-empty label id"));
        assert_eq!(db.commit_epoch().unwrap(), before);
        assert_eq!(read_test_wal(&path).unwrap(), wal);
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn memory_label_delete_query_persists_as_one_wal_batch_and_replays() {
    let path = unique_test_dir("memory_label_delete_query_replay");
    let parameters = delete_parameters("memory_1", None).unwrap();
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Memory {id: 'memory_1'})").unwrap();
        db.query("CREATE (:Label {id: 'alpha'})").unwrap();
        db.query("CREATE (:Label {id: 'beta'})").unwrap();
        for label in ["alpha", "beta"] {
            let parameters = delete_parameters("memory_1", Some(label)).unwrap();
            db.query_request(bounded_batch_query("MATCH (m:Memory {id: $memory}), (l:Label {id: $label}) CREATE (m)-[:HAS_LABEL]->(l)", &parameters)).unwrap();
        }
        let before = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        let mut transaction = db.begin_transaction().unwrap();
        transaction
            .query_request(bounded_batch_query(LABEL_DELETE_ALL, &parameters))
            .unwrap();
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
            .query_request(bounded_batch_query(LABEL_EDGES, &parameters))
            .unwrap()
            .rows
            .is_empty());
        let memories = db
            .query_request(bounded_batch_query(
                "MATCH (m:Memory) RETURN m.id AS id",
                &BTreeMap::new(),
            ))
            .unwrap();
        assert_eq!(memories.rows.len(), 1);
        assert_eq!(memories.rows[0]["id"], Value::String("memory_1".into()));
        let labels = db
            .query_request(bounded_batch_query(
                "MATCH (l:Label) RETURN count(l) AS total",
                &BTreeMap::new(),
            ))
            .unwrap();
        assert_eq!(labels.rows[0]["total"], Value::Int(2));
    }
    std::fs::remove_dir_all(path).unwrap();
}

const TRANSFER_MEMORIES: &str = "MATCH (m:Memory)-[:HAS_LABEL]->(:Label {id: $source}) \
    WHERE m.id IS NOT NULL AND m.id <> '' RETURN DISTINCT m.id AS id ORDER BY id";
const TRANSFER_TO_LABEL: &str = "MATCH (m:Memory {id: $memory}), (l:Label {id: $target}) \
    MERGE (m)-[r:HAS_LABEL]->(l) ON CREATE SET r.assigned_by = 'label_merge', r.created_at = $created_at, r.properties = '{}'";
const TARGET_LABEL_READ: &str = "MATCH (m:Memory)-[r:HAS_LABEL]->(:Label {id: $target}) \
    RETURN m.id AS id, id(r) AS relationship_id, r.assigned_by AS assigned_by, r.created_at AS created_at, r.properties AS properties ORDER BY id";

fn label_transfer_parameters(
    source: &str,
    target: &str,
    created_at: i64,
) -> Result<BTreeMap<String, Value>> {
    if source.is_empty() {
        return Err(HawDBError::Semantic(
            "non-empty source label id required".into(),
        ));
    }
    if target.is_empty() {
        return Err(HawDBError::Semantic(
            "non-empty target label id required".into(),
        ));
    }
    Ok(BTreeMap::from([
        ("source".into(), Value::String(source.into())),
        ("target".into(), Value::String(target.into())),
        ("created_at".into(), Value::Int(created_at)),
    ]))
}

#[test]
fn transfers_label_memory_edges_for_nowledge_label_merge_shape() {
    let mut db = Database::new();
    for query in [
        "CREATE (:Label {id: 'source_label'})", "CREATE (:Label {id: 'target_label'})",
        "CREATE (:Memory {id: 'memory_create'})", "CREATE (:Memory {id: 'memory_existing'})", "CREATE (:Memory {title: 'Idless memory'})",
        "MATCH (m:Memory {id: 'memory_create'}), (l:Label {id: 'source_label'}) CREATE (m)-[:HAS_LABEL]->(l)",
        "MATCH (m:Memory {id: 'memory_create'}), (l:Label {id: 'source_label'}) CREATE (m)-[:HAS_LABEL {duplicate: true}]->(l)",
        "MATCH (m:Memory {id: 'memory_existing'}), (l:Label {id: 'source_label'}) CREATE (m)-[:HAS_LABEL]->(l)",
        "MATCH (m:Memory {id: 'memory_existing'}), (l:Label {id: 'target_label'}) CREATE (m)-[:HAS_LABEL {assigned_by: 'existing'}]->(l)",
        "MATCH (m:Memory {title: 'Idless memory'}), (l:Label {id: 'source_label'}) CREATE (m)-[:HAS_LABEL]->(l)",
    ] { db.query(query).unwrap(); }
    let parameters = label_transfer_parameters("source_label", "target_label", 100).unwrap();
    let before = db.commit_epoch().unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    let source = transaction.query_request(bounded_batch_query("MATCH (m:Memory)-[:HAS_LABEL]->(:Label {id: $source}) RETURN DISTINCT id(m) AS node_id ORDER BY node_id", &parameters)).unwrap();
    assert_eq!(source.rows.len(), 3);
    let protected = transaction.query_request(bounded_batch_query("MATCH (m:Memory)-[:HAS_LABEL]->(:Label {id: $source}) WHERE m.id IS NULL RETURN id(m) AS node_id", &parameters)).unwrap();
    assert_eq!(protected.rows.len(), 1);
    assert_eq!(protected.rows[0]["node_id"], Value::Int(4));
    let existing = transaction
        .query_request(bounded_batch_query(TARGET_LABEL_READ, &parameters))
        .unwrap();
    assert_eq!(existing.rows.len(), 1);
    let candidates = transaction
        .query_request(bounded_batch_query(TRANSFER_MEMORIES, &parameters))
        .unwrap();
    assert_eq!(candidates.rows.len(), 2);
    assert_eq!(
        candidates.rows[0]["id"],
        Value::String("memory_create".into())
    );
    assert_eq!(
        candidates.rows[1]["id"],
        Value::String("memory_existing".into())
    );
    for row in &candidates.rows {
        let mut parameters = parameters.clone();
        parameters.insert("memory".into(), row["id"].clone());
        transaction
            .query_request(bounded_batch_query(TRANSFER_TO_LABEL, &parameters))
            .unwrap();
    }
    let committed = transaction.commit().unwrap();
    assert_eq!(committed.rows.len(), 2);
    assert_eq!(committed.rows[0]["created"], Value::Bool(true));
    assert_eq!(committed.rows[1]["created"], Value::Bool(false));
    assert_eq!(
        committed.rows[1]["rel_id"],
        existing.rows[0]["relationship_id"]
    );
    assert_eq!(db.commit_epoch().unwrap(), before + 1);
    let output = db
        .query_request(bounded_batch_query(TARGET_LABEL_READ, &parameters))
        .unwrap();
    assert_eq!(output.rows.len(), 2);
    assert_eq!(output.rows[0]["id"], Value::String("memory_create".into()));
    assert_eq!(
        output.rows[0]["assigned_by"],
        Value::String("label_merge".into())
    );
    assert_eq!(output.rows[0]["created_at"], Value::Int(100));
    assert_eq!(output.rows[0]["properties"], Value::String("{}".into()));
    assert_eq!(output.rows[1], existing.rows[0]);
    let missing = label_transfer_parameters("missing", "target_label", 200).unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    let target = transaction
        .query_request(bounded_batch_query(
            "MATCH (l:Label {id: $target}) RETURN id(l) AS node_id",
            &missing,
        ))
        .unwrap();
    assert_eq!(target.rows.len(), 1);
    assert!(transaction
        .query_request(bounded_batch_query(TRANSFER_MEMORIES, &missing))
        .unwrap()
        .rows
        .is_empty());
    assert!(transaction.commit().unwrap().rows.is_empty());
    assert_eq!(db.commit_epoch().unwrap(), before + 1);
}

#[test]
fn label_memory_transfer_rejects_empty_ids_before_wal() {
    let path = unique_test_dir("label_transfer_query_empty_ids");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Label {id: 'source_label'})").unwrap();
        db.query("CREATE (:Label {id: 'target_label'})").unwrap();
        let before = db.commit_epoch().unwrap();
        let wal = read_test_wal(&path).unwrap();
        assert!(label_transfer_parameters("", "target_label", 1)
            .unwrap_err()
            .to_string()
            .contains("non-empty source label id"));
        assert!(label_transfer_parameters("source_label", "", 1)
            .unwrap_err()
            .to_string()
            .contains("non-empty target label id"));
        assert_eq!(db.commit_epoch().unwrap(), before);
        assert_eq!(read_test_wal(&path).unwrap(), wal);
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn label_memory_transfer_queries_persist_as_one_wal_batch_and_replay() {
    let path = unique_test_dir("label_transfer_query_replay");
    let parameters = label_transfer_parameters("source_label", "target_label", 10).unwrap();
    {
        let mut db = Database::open(&path).unwrap();
        for query in [
            "CREATE (:Label {id: 'source_label'})", "CREATE (:Label {id: 'target_label'})",
            "CREATE (:Memory {id: 'memory_1'})", "CREATE (:Memory {id: 'memory_2'})",
            "MATCH (m:Memory {id: 'memory_1'}), (l:Label {id: 'source_label'}) CREATE (m)-[:HAS_LABEL]->(l)",
            "MATCH (m:Memory {id: 'memory_2'}), (l:Label {id: 'source_label'}) CREATE (m)-[:HAS_LABEL]->(l)",
        ] { db.query(query).unwrap(); }
        let before = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        let mut transaction = db.begin_transaction().unwrap();
        let candidates = transaction
            .query_request(bounded_batch_query(TRANSFER_MEMORIES, &parameters))
            .unwrap();
        assert_eq!(candidates.rows.len(), 2);
        for row in &candidates.rows {
            let mut parameters = parameters.clone();
            parameters.insert("memory".into(), row["id"].clone());
            transaction
                .query_request(bounded_batch_query(TRANSFER_TO_LABEL, &parameters))
                .unwrap();
        }
        let committed = transaction.commit().unwrap();
        assert_eq!(committed.rows.len(), 2);
        for row in &committed.rows {
            assert_eq!(row["created"], Value::Bool(true));
        }
        assert_eq!(
            read_test_wal(&path).unwrap().matches("\tbatch\t").count(),
            before + 1
        );
    }
    assert!(read_test_wal(&path).unwrap().contains("create_rel"));
    {
        let mut db = Database::open(&path).unwrap();
        let output = db
            .query_request(bounded_batch_query(TARGET_LABEL_READ, &parameters))
            .unwrap();
        assert_eq!(output.rows.len(), 2);
        for (row, id) in output.rows.iter().zip(["memory_1", "memory_2"]) {
            assert_eq!(row["id"], Value::String(id.into()));
            assert_eq!(row["assigned_by"], Value::String("label_merge".into()));
            assert_eq!(row["created_at"], Value::Int(10));
            assert_eq!(row["properties"], Value::String("{}".into()));
        }
        let before = db.commit_epoch().unwrap();
        let wal = read_test_wal(&path).unwrap();
        let mut transaction = db.begin_transaction().unwrap();
        let candidates = transaction
            .query_request(bounded_batch_query(TRANSFER_MEMORIES, &parameters))
            .unwrap();
        for row in &candidates.rows {
            let mut parameters = parameters.clone();
            parameters.insert("memory".into(), row["id"].clone());
            transaction
                .query_request(bounded_batch_query(TRANSFER_TO_LABEL, &parameters))
                .unwrap();
        }
        let committed = transaction.commit().unwrap();
        assert_eq!(committed.rows.len(), 2);
        for row in &committed.rows {
            assert_eq!(row["created"], Value::Bool(false));
        }
        assert_eq!(db.commit_epoch().unwrap(), before);
        assert_eq!(read_test_wal(&path).unwrap(), wal);
    }
    std::fs::remove_dir_all(path).unwrap();
}

const TRANSFER_LABELS: &str =
    "MATCH (older:Memory {id: $older, space_id: $space})-[:HAS_LABEL]->(l:Label), \
    (newer:Memory {id: $newer, space_id: $space}) \
    WHERE l.id IS NOT NULL AND l.id <> '' RETURN DISTINCT l.id AS id ORDER BY id";
const TRANSFER_TO_MEMORY: &str = "MATCH (m:Memory {id: $newer, space_id: $space}), (l:Label {id: $label}) \
    MERGE (m)-[r:HAS_LABEL]->(l) ON CREATE SET r.assigned_by = 'system', r.created_at = $created_at, r.properties = '{}'";
const TARGET_MEMORY_READ: &str = "MATCH (:Memory {id: $newer})-[r:HAS_LABEL]->(l:Label) \
    RETURN l.id AS id, id(r) AS relationship_id, r.assigned_by AS assigned_by, r.created_at AS created_at, r.properties AS properties ORDER BY id";

fn memory_transfer_parameters(
    older: &str,
    newer: &str,
    space: &str,
    created_at: i64,
) -> Result<BTreeMap<String, Value>> {
    for (value, name) in [
        (older, "older memory id"),
        (newer, "newer memory id"),
        (space, "space id"),
    ] {
        if value.is_empty() {
            return Err(HawDBError::Semantic(format!("non-empty {name} required")));
        }
    }
    Ok(BTreeMap::from([
        ("older".into(), Value::String(older.into())),
        ("newer".into(), Value::String(newer.into())),
        ("space".into(), Value::String(space.into())),
        ("created_at".into(), Value::Int(created_at)),
    ]))
}

#[test]
fn transfers_memory_label_edges_for_nowledge_memory_label_carry_over_shape() {
    let mut db = Database::new();
    for query in [
        "CREATE (:Memory {id: 'older', space_id: 'default'})", "CREATE (:Memory {id: 'newer', space_id: 'default'})", "CREATE (:Memory {id: 'other_space', space_id: 'other'})",
        "CREATE (:Label {id: 'alpha'})", "CREATE (:Label {id: 'beta'})",
        "MATCH (m:Memory {id: 'older'}), (l:Label {id: 'alpha'}) CREATE (m)-[:HAS_LABEL]->(l)",
        "MATCH (m:Memory {id: 'older'}), (l:Label {id: 'alpha'}) CREATE (m)-[:HAS_LABEL {duplicate: true}]->(l)",
        "MATCH (m:Memory {id: 'older'}), (l:Label {id: 'beta'}) CREATE (m)-[:HAS_LABEL]->(l)",
        "MATCH (m:Memory {id: 'newer'}), (l:Label {id: 'beta'}) CREATE (m)-[:HAS_LABEL {assigned_by: 'existing'}]->(l)",
    ] { db.query(query).unwrap(); }
    let parameters = memory_transfer_parameters("older", "newer", "default", 7100).unwrap();
    let before = db.commit_epoch().unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    let source = transaction
        .query_request(bounded_batch_query(
            "MATCH (:Memory {id: $older})-[:HAS_LABEL]->(l:Label) RETURN l.id AS id ORDER BY id",
            &parameters,
        ))
        .unwrap();
    assert_eq!(source.rows.len(), 3);
    assert_eq!(source.rows[0]["id"], Value::String("alpha".into()));
    assert_eq!(source.rows[1]["id"], Value::String("alpha".into()));
    assert_eq!(source.rows[2]["id"], Value::String("beta".into()));
    let existing = transaction
        .query_request(bounded_batch_query(TARGET_MEMORY_READ, &parameters))
        .unwrap();
    assert_eq!(existing.rows.len(), 1);
    let candidates = transaction
        .query_request(bounded_batch_query(TRANSFER_LABELS, &parameters))
        .unwrap();
    assert_eq!(candidates.rows.len(), 2);
    assert_eq!(candidates.rows[0]["id"], Value::String("alpha".into()));
    assert_eq!(candidates.rows[1]["id"], Value::String("beta".into()));
    for row in &candidates.rows {
        let mut parameters = parameters.clone();
        parameters.insert("label".into(), row["id"].clone());
        transaction
            .query_request(bounded_batch_query(TRANSFER_TO_MEMORY, &parameters))
            .unwrap();
    }
    let committed = transaction.commit().unwrap();
    assert_eq!(committed.rows.len(), 2);
    assert_eq!(committed.rows[0]["created"], Value::Bool(true));
    assert_eq!(committed.rows[1]["created"], Value::Bool(false));
    assert_eq!(
        committed.rows[1]["rel_id"],
        existing.rows[0]["relationship_id"]
    );
    assert_eq!(db.commit_epoch().unwrap(), before + 1);
    let output = db
        .query_request(bounded_batch_query(TARGET_MEMORY_READ, &parameters))
        .unwrap();
    assert_eq!(output.rows.len(), 2);
    assert_eq!(output.rows[0]["id"], Value::String("alpha".into()));
    assert_eq!(
        output.rows[0]["assigned_by"],
        Value::String("system".into())
    );
    assert_eq!(output.rows[0]["created_at"], Value::Int(7100));
    assert_eq!(output.rows[0]["properties"], Value::String("{}".into()));
    assert_eq!(output.rows[1], existing.rows[0]);
    let mismatch = memory_transfer_parameters("older", "other_space", "default", 7200).unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    let spaces = transaction.query_request(bounded_batch_query("MATCH (o:Memory {id: $older}), (n:Memory {id: $newer}) RETURN o.space_id AS older, n.space_id AS newer", &mismatch)).unwrap();
    assert_eq!(spaces.rows.len(), 1);
    assert_eq!(spaces.rows[0]["older"], Value::String("default".into()));
    assert_eq!(spaces.rows[0]["newer"], Value::String("other".into()));
    assert!(transaction
        .query_request(bounded_batch_query(TRANSFER_LABELS, &mismatch))
        .unwrap()
        .rows
        .is_empty());
    assert!(transaction.commit().unwrap().rows.is_empty());
    assert_eq!(db.commit_epoch().unwrap(), before + 1);
}

#[test]
fn memory_label_transfer_rejects_empty_inputs_before_wal() {
    let path = unique_test_dir("memory_transfer_query_empty_inputs");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Memory {id: 'older', space_id: 'default'})")
            .unwrap();
        db.query("CREATE (:Memory {id: 'newer', space_id: 'default'})")
            .unwrap();
        let before = db.commit_epoch().unwrap();
        let wal = read_test_wal(&path).unwrap();
        for (older, newer, space, message) in [
            ("", "newer", "default", "older memory id"),
            ("older", "", "default", "newer memory id"),
            ("older", "newer", "", "space id"),
        ] {
            assert!(memory_transfer_parameters(older, newer, space, 1)
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
fn memory_label_transfer_queries_persist_as_one_wal_batch_and_replay() {
    let path = unique_test_dir("memory_transfer_query_replay");
    let parameters = memory_transfer_parameters("older", "newer", "default", 7300).unwrap();
    {
        let mut db = Database::open(&path).unwrap();
        for query in [
            "CREATE (:Memory {id: 'older', space_id: 'default'})",
            "CREATE (:Memory {id: 'newer', space_id: 'default'})",
            "CREATE (:Label {id: 'alpha'})",
            "CREATE (:Label {id: 'beta'})",
            "MATCH (m:Memory {id: 'older'}), (l:Label {id: 'alpha'}) CREATE (m)-[:HAS_LABEL]->(l)",
            "MATCH (m:Memory {id: 'older'}), (l:Label {id: 'beta'}) CREATE (m)-[:HAS_LABEL]->(l)",
        ] {
            db.query(query).unwrap();
        }
        let before = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        let mut transaction = db.begin_transaction().unwrap();
        let candidates = transaction
            .query_request(bounded_batch_query(TRANSFER_LABELS, &parameters))
            .unwrap();
        assert_eq!(candidates.rows.len(), 2);
        for row in &candidates.rows {
            let mut parameters = parameters.clone();
            parameters.insert("label".into(), row["id"].clone());
            transaction
                .query_request(bounded_batch_query(TRANSFER_TO_MEMORY, &parameters))
                .unwrap();
        }
        let committed = transaction.commit().unwrap();
        assert_eq!(committed.rows.len(), 2);
        for row in &committed.rows {
            assert_eq!(row["created"], Value::Bool(true));
        }
        assert_eq!(
            read_test_wal(&path).unwrap().matches("\tbatch\t").count(),
            before + 1
        );
    }
    assert!(read_test_wal(&path).unwrap().contains("create_rel"));
    {
        let mut db = Database::open(&path).unwrap();
        let output = db
            .query_request(bounded_batch_query(TARGET_MEMORY_READ, &parameters))
            .unwrap();
        assert_eq!(output.rows.len(), 2);
        for (row, id) in output.rows.iter().zip(["alpha", "beta"]) {
            assert_eq!(row["id"], Value::String(id.into()));
            assert_eq!(row["assigned_by"], Value::String("system".into()));
            assert_eq!(row["created_at"], Value::Int(7300));
            assert_eq!(row["properties"], Value::String("{}".into()));
        }
    }
    std::fs::remove_dir_all(path).unwrap();
}

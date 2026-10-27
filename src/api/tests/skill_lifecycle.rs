// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;

const USAGE_UPDATE: &str = "MATCH (s:Skill {id: $id}) SET s.use_count = $use_count, \
    s.last_activity_at = $last_activity_at, s.updated_at = $updated_at, \
    s.metadata = $metadata RETURN id(s) AS node_id";
const USAGE_RATE_UPDATE: &str = "MATCH (s:Skill {id: $id}) \
    SET s.success_rate = $success_rate RETURN id(s) AS node_id";
const METADATA_UPDATE: &str = "MATCH (s:Skill {id: $id}) \
    SET s.metadata = $metadata, s.updated_at = $updated_at RETURN id(s) AS node_id";
const SKILL_READ: &str = "MATCH (s:Skill {id: $id}) RETURN id(s) AS node_id, \
    s.id AS id, s.use_count AS use_count, s.success_rate AS success_rate, \
    s.last_activity_at AS last_activity_at, s.updated_at AS updated_at, \
    s.metadata AS metadata, s.stage AS stage, s.rejected_at AS rejected_at, \
    s.rationale AS rationale, s.version AS version, s.title AS title, s.name AS name, \
    s.description AS description, s.triggers AS triggers, s.tools AS tools, \
    s.bundle_path AS bundle_path, s.content_hash AS content_hash, s.write_origin AS write_origin";

fn skill_parameters(id: &str, values: &[(&str, Value)]) -> Result<BTreeMap<String, Value>> {
    if id.is_empty() {
        return Err(HawDBError::Semantic("non-empty skill id required".into()));
    }
    let mut parameters = BTreeMap::from([("id".into(), Value::String(id.into()))]);
    parameters.extend(
        values
            .iter()
            .map(|(key, value)| ((*key).into(), value.clone())),
    );
    Ok(parameters)
}

fn usage_parameters(
    id: &str,
    count: i64,
    rate: Option<Value>,
    activity: i64,
    updated: i64,
    metadata: &str,
) -> Result<BTreeMap<String, Value>> {
    if count < 0 {
        return Err(HawDBError::Semantic(
            "non-negative use count required".into(),
        ));
    }
    let mut parameters = skill_parameters(
        id,
        &[
            ("use_count", Value::Int(count)),
            ("last_activity_at", Value::Int(activity)),
            ("updated_at", Value::Int(updated)),
            ("metadata", Value::String(metadata.into())),
        ],
    )?;
    if let Some(rate) = rate {
        let valid = match &rate {
            Value::Int(value) => (0..=1).contains(value),
            Value::Float(value) => value.is_finite() && (0.0..=1.0).contains(value),
            _ => false,
        };
        if !valid {
            return Err(HawDBError::Semantic(
                "success rate between 0 and 1 required".into(),
            ));
        }
        parameters.insert("success_rate".into(), rate);
    }
    Ok(parameters)
}

#[test]
fn updates_skill_usage_stats_batch_for_nowledge_shapes() {
    let mut db = Database::new();
    db.query("CREATE (:Skill {id: 'skill_1', use_count: 1, success_rate: 0.5, metadata: '{}', updated_at: 100})").unwrap();
    db.query("CREATE (:Skill {id: 'skill_2', use_count: 2, metadata: '{}', updated_at: 100})")
        .unwrap();
    let inputs = [
        usage_parameters(
            "skill_1",
            8,
            Some(Value::Float(0.75)),
            810,
            820,
            "{\"runs\":8}",
        )
        .unwrap(),
        usage_parameters("skill_2", 5, None, 910, 910, "{\"runs\":5}").unwrap(),
        usage_parameters("skill_2", 6, None, 920, 920, "{\"duplicate\":true}").unwrap(),
        usage_parameters("missing", 1, Some(Value::Float(1.0)), 930, 930, "{}").unwrap(),
    ];
    let before = db.commit_epoch().unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    let mut seen = BTreeSet::new();
    let mut counts = Vec::new();
    let mut duplicates = 0;
    for parameters in &inputs {
        let Value::String(id) = &parameters["id"] else {
            unreachable!()
        };
        if !seen.insert(id) {
            duplicates += 1;
            continue;
        }
        let output = transaction
            .query_request(bounded_batch_query(USAGE_UPDATE, parameters))
            .unwrap();
        counts.push(output.rows.len());
        if parameters.contains_key("success_rate") {
            let rate = transaction
                .query_request(bounded_batch_query(USAGE_RATE_UPDATE, parameters))
                .unwrap();
            assert_eq!(rate.rows.len(), output.rows.len());
        }
    }
    assert_eq!(counts, [1, 1, 0]);
    assert_eq!(duplicates, 1);
    transaction.commit().unwrap();
    assert_eq!(before, 2);
    assert_eq!(db.commit_epoch().unwrap(), before + 1);
    for parameters in &inputs[..2] {
        let rows = db
            .query_request(bounded_batch_query(SKILL_READ, parameters))
            .unwrap();
        assert_eq!(rows.rows.len(), 1);
        for key in [
            "id",
            "use_count",
            "last_activity_at",
            "updated_at",
            "metadata",
        ] {
            assert_eq!(rows.rows[0][key], parameters[key]);
        }
        assert_eq!(
            rows.rows[0]["success_rate"],
            parameters
                .get("success_rate")
                .cloned()
                .unwrap_or(Value::Null)
        );
    }
}

#[test]
fn skill_usage_stats_batch_rejects_negative_use_count_before_wal() {
    let path = unique_test_dir("skill_usage_query_invalid_input");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Skill {id: 'skill_1', use_count: 1})")
            .unwrap();
        let before = db.commit_epoch().unwrap();
        let wal = read_test_wal(&path).unwrap();
        assert!(usage_parameters("skill_1", -1, None, 1, 1, "{}")
            .unwrap_err()
            .to_string()
            .contains("non-negative use count"));
        for rate in [
            Value::Int(2),
            Value::Float(-0.1),
            Value::Float(f64::NAN),
            Value::Float(f64::INFINITY),
            Value::String("high".into()),
        ] {
            assert!(usage_parameters("skill_1", 1, Some(rate), 1, 1, "{}")
                .unwrap_err()
                .to_string()
                .contains("success rate"));
        }
        assert_eq!(db.commit_epoch().unwrap(), before);
        assert_eq!(read_test_wal(&path).unwrap(), wal);
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn skill_usage_queries_persist_as_one_wal_batch_and_replay() {
    let path = unique_test_dir("skill_usage_query_replay");
    let inputs = [
        usage_parameters(
            "skill_1",
            8,
            Some(Value::Float(0.75)),
            810,
            820,
            "{\"runs\":8}",
        )
        .unwrap(),
        usage_parameters("skill_2", 5, None, 910, 910, "{\"runs\":5}").unwrap(),
    ];
    {
        let mut db = Database::open(&path).unwrap();
        db.query(
            "CREATE (:Skill {id: 'skill_1', use_count: 1, success_rate: 0.5, metadata: '{}'})",
        )
        .unwrap();
        db.query("CREATE (:Skill {id: 'skill_2', use_count: 1, metadata: '{}'})")
            .unwrap();
        let before = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        let mut transaction = db.begin_transaction().unwrap();
        for parameters in &inputs {
            let output = transaction
                .query_request(bounded_batch_query(USAGE_UPDATE, parameters))
                .unwrap();
            assert_eq!(output.rows.len(), 1);
            if parameters.contains_key("success_rate") {
                transaction
                    .query_request(bounded_batch_query(USAGE_RATE_UPDATE, parameters))
                    .unwrap();
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
        for parameters in &inputs {
            let output = db
                .query_request(bounded_batch_query(SKILL_READ, parameters))
                .unwrap();
            assert_eq!(output.rows.len(), 1);
            for key in [
                "id",
                "use_count",
                "last_activity_at",
                "updated_at",
                "metadata",
            ] {
                assert_eq!(output.rows[0][key], parameters[key]);
            }
            assert_eq!(
                output.rows[0]["success_rate"],
                parameters
                    .get("success_rate")
                    .cloned()
                    .unwrap_or(Value::Null)
            );
        }
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn updates_skill_metadata_batch_for_nowledge_shape() {
    let mut db = Database::new();
    db.query("CREATE (:Skill {id: 'skill_1', metadata: '{}', updated_at: 1})")
        .unwrap();
    db.query("CREATE (:Skill {id: 'skill_2', metadata: '{}', updated_at: 1})")
        .unwrap();
    db.query("CREATE (:Skill {title: 'Idless Skill', metadata: '{}', updated_at: 1})")
        .unwrap();
    let before = db.commit_epoch().unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    let mut seen = BTreeSet::new();
    let mut counts = Vec::new();
    let mut duplicates = 0;
    for (id, metadata, timestamp) in [
        ("skill_1", "{\"source\":\"rest\"}", 101),
        ("skill_2", "{\"source\":\"mcp\"}", 202),
        ("missing", "{\"missing\":true}", 303),
        ("2", "{\"idless\":true}", 404),
        ("skill_1", "{\"duplicate\":true}", 505),
    ] {
        let parameters = skill_parameters(
            id,
            &[
                ("metadata", Value::String(metadata.into())),
                ("updated_at", Value::Int(timestamp)),
            ],
        )
        .unwrap();
        if !seen.insert(id) {
            duplicates += 1;
            continue;
        }
        counts.push(
            transaction
                .query_request(bounded_batch_query(METADATA_UPDATE, &parameters))
                .unwrap()
                .rows
                .len(),
        );
    }
    assert_eq!(counts, [1, 1, 0, 0]);
    assert_eq!(duplicates, 1);
    transaction.commit().unwrap();
    assert_eq!(before, 3);
    assert_eq!(db.commit_epoch().unwrap(), before + 1);
    for (id, metadata, timestamp) in [
        ("skill_1", "{\"source\":\"rest\"}", 101),
        ("skill_2", "{\"source\":\"mcp\"}", 202),
    ] {
        let parameters = skill_parameters(id, &[]).unwrap();
        let output = db
            .query_request(bounded_batch_query(SKILL_READ, &parameters))
            .unwrap();
        assert_eq!(output.rows.len(), 1);
        assert_eq!(output.rows[0]["metadata"], Value::String(metadata.into()));
        assert_eq!(output.rows[0]["updated_at"], Value::Int(timestamp));
    }
    let retained = db.query_request(bounded_batch_query("MATCH (s:Skill) WHERE s.id IS NULL RETURN id(s) AS node_id, s.metadata AS metadata, s.updated_at AS updated_at", &BTreeMap::new())).unwrap();
    assert_eq!(retained.rows.len(), 1);
    assert_eq!(retained.rows[0]["node_id"], Value::Int(2));
    assert_eq!(retained.rows[0]["metadata"], Value::String("{}".into()));
    assert_eq!(retained.rows[0]["updated_at"], Value::Int(1));
}

#[test]
fn skill_metadata_update_rejects_empty_id_before_wal() {
    let path = unique_test_dir("skill_metadata_query_empty_id");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Skill {id: 'skill_1', metadata: '{}', updated_at: 1})")
            .unwrap();
        let before = db.commit_epoch().unwrap();
        let wal = read_test_wal(&path).unwrap();
        assert!(skill_parameters(
            "",
            &[
                ("metadata", Value::String("{}".into())),
                ("updated_at", Value::Int(2))
            ]
        )
        .unwrap_err()
        .to_string()
        .contains("non-empty skill id"));
        assert_eq!(db.commit_epoch().unwrap(), before);
        assert_eq!(read_test_wal(&path).unwrap(), wal);
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn skill_metadata_queries_persist_as_one_wal_batch_and_replay() {
    let path = unique_test_dir("skill_metadata_query_replay");
    let inputs = [
        ("skill_1", "{\"source\":\"rest\"}", 101),
        ("skill_2", "{\"source\":\"mcp\"}", 202),
    ];
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Skill {id: 'skill_1', metadata: '{}', updated_at: 1})")
            .unwrap();
        db.query("CREATE (:Skill {id: 'skill_2', metadata: '{}', updated_at: 1})")
            .unwrap();
        let before = read_test_wal(&path).unwrap().matches("\tbatch\t").count();
        let mut transaction = db.begin_transaction().unwrap();
        for (id, metadata, timestamp) in inputs {
            let parameters = skill_parameters(
                id,
                &[
                    ("metadata", Value::String(metadata.into())),
                    ("updated_at", Value::Int(timestamp)),
                ],
            )
            .unwrap();
            assert_eq!(
                transaction
                    .query_request(bounded_batch_query(METADATA_UPDATE, &parameters))
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
        for (id, metadata, timestamp) in inputs {
            let parameters = skill_parameters(id, &[]).unwrap();
            let output = db
                .query_request(bounded_batch_query(SKILL_READ, &parameters))
                .unwrap();
            assert_eq!(output.rows.len(), 1);
            assert_eq!(output.rows[0]["metadata"], Value::String(metadata.into()));
            assert_eq!(output.rows[0]["updated_at"], Value::Int(timestamp));
        }
    }
    std::fs::remove_dir_all(path).unwrap();
}

const REJECT_UPDATE: &str = "MATCH (s:Skill {id: $id}) SET s.stage = $stage, \
    s.rejected_at = $rejected_at, s.updated_at = $updated_at RETURN id(s) AS node_id";
const COMPILED_UPDATE: &str = "MATCH (s:Skill {id: $id}) SET s.version = $version, \
    s.title = $title, s.name = $name, s.description = $description, s.content_hash = $content_hash, \
    s.bundle_path = $bundle_path, s.metadata = $metadata, s.updated_at = $updated_at RETURN id(s) AS node_id";
const PROMOTE_UPDATE: &str = "MATCH (s:Skill {id: $id}) SET s.stage = $stage, \
    s.rationale = $rationale, s.updated_at = $updated_at RETURN id(s) AS node_id";
const DRAFT_UPDATE: &str = "MATCH (s:Skill {id: $id}) SET s.stage = $stage, \
    s.name = $name, s.description = $description, s.triggers = $triggers, s.tools = $tools, \
    s.bundle_path = $bundle_path, s.content_hash = $content_hash, \
    s.write_origin = $write_origin, s.updated_at = $updated_at RETURN id(s) AS node_id";
const CONTENT_UPDATE: &str = "MATCH (s:Skill {id: $id}) SET s.content_hash = $content_hash, \
    s.metadata = $metadata, s.updated_at = $updated_at RETURN id(s) AS node_id";
const STAGE_UPDATE: &str = "MATCH (s:Skill {id: $id}) SET s.stage = $stage, \
    s.updated_at = $updated_at RETURN id(s) AS node_id";

fn lifecycle_parameters(
    id: &str,
    updated: i64,
    fields: &[(&str, Value)],
) -> Result<BTreeMap<String, Value>> {
    if fields.is_empty() {
        return Err(HawDBError::Semantic(
            "at least one lifecycle field required".into(),
        ));
    }
    for (key, value) in fields {
        if matches!(*key, "stage" | "write_origin")
            && matches!(value, Value::String(value) if value.is_empty())
        {
            return Err(HawDBError::Semantic(format!("non-empty {key} required")));
        }
    }
    let mut parameters = skill_parameters(id, fields)?;
    parameters.insert("updated_at".into(), Value::Int(updated));
    Ok(parameters)
}

#[test]
fn updates_skill_lifecycle_batch_for_nowledge_shapes() {
    let mut db = Database::new();
    for id in [
        "skill_reject",
        "skill_compiled",
        "skill_promote",
        "skill_draft",
        "skill_content",
    ] {
        let parameters = skill_parameters(id, &[]).unwrap();
        db.query_request(bounded_batch_query(
            "CREATE (:Skill {id: $id, stage: 'candidate', metadata: '{}', updated_at: 1})",
            &parameters,
        ))
        .unwrap();
    }
    let inputs = [
        (
            REJECT_UPDATE,
            lifecycle_parameters(
                "skill_reject",
                202,
                &[
                    ("stage", Value::String("rejected".into())),
                    ("rejected_at", Value::Int(201)),
                ],
            )
            .unwrap(),
        ),
        (
            COMPILED_UPDATE,
            lifecycle_parameters(
                "skill_compiled",
                401,
                &[
                    ("version", Value::Int(2)),
                    ("title", Value::String("compiled-skill".into())),
                    ("name", Value::String("compiled-skill".into())),
                    ("description", Value::String("compiled description".into())),
                    ("content_hash", Value::String("hash-compiled-rest".into())),
                    ("bundle_path", Value::String("/tmp/compiled-rest".into())),
                    ("metadata", Value::String("{\"compiled\":true}".into())),
                ],
            )
            .unwrap(),
        ),
        (
            PROMOTE_UPDATE,
            lifecycle_parameters(
                "skill_promote",
                1_700_000_032,
                &[
                    ("stage", Value::String("promotable".into())),
                    ("rationale", Value::String("ready to promote".into())),
                ],
            )
            .unwrap(),
        ),
        (
            DRAFT_UPDATE,
            lifecycle_parameters(
                "skill_draft",
                1_700_000_033,
                &[
                    ("stage", Value::String("draft".into())),
                    ("name", Value::String("mcp-skill".into())),
                    ("description", Value::String("compiled skill".into())),
                    (
                        "triggers",
                        Value::List(vec![Value::String("compile".into())]),
                    ),
                    ("tools", Value::List(vec![Value::String("shell".into())])),
                    ("bundle_path", Value::String("/tmp/mcp-skill".into())),
                    ("content_hash", Value::String("hash-write".into())),
                    ("write_origin", Value::String("compiler".into())),
                ],
            )
            .unwrap(),
        ),
        (
            CONTENT_UPDATE,
            lifecycle_parameters(
                "skill_content",
                701,
                &[
                    ("content_hash", Value::String("hash-updated".into())),
                    ("metadata", Value::String("{\"updated\":true}".into())),
                ],
            )
            .unwrap(),
        ),
        (
            STAGE_UPDATE,
            lifecycle_parameters(
                "skill_draft",
                800,
                &[("stage", Value::String("active".into()))],
            )
            .unwrap(),
        ),
        (
            STAGE_UPDATE,
            lifecycle_parameters("missing", 900, &[("stage", Value::String("active".into()))])
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
    assert_eq!(counts, [1, 1, 1, 1, 1, 0]);
    assert_eq!(duplicates, 1);
    transaction.commit().unwrap();
    assert_eq!(before, 5);
    assert_eq!(db.commit_epoch().unwrap(), before + 1);
    for (_, parameters) in &inputs[..5] {
        let output = db
            .query_request(bounded_batch_query(SKILL_READ, parameters))
            .unwrap();
        assert_eq!(output.rows.len(), 1);
        for (key, value) in parameters {
            assert_eq!(&output.rows[0][key.as_str()], value, "{key}");
        }
        if !parameters.contains_key("stage") {
            assert_eq!(output.rows[0]["stage"], Value::String("candidate".into()));
        }
        if !parameters.contains_key("metadata") {
            assert_eq!(output.rows[0]["metadata"], Value::String("{}".into()));
        }
        for key in [
            "rejected_at",
            "rationale",
            "version",
            "title",
            "name",
            "description",
            "triggers",
            "tools",
            "bundle_path",
            "content_hash",
            "write_origin",
        ] {
            if !parameters.contains_key(key) {
                assert_eq!(output.rows[0][key], Value::Null, "{key}");
            }
        }
    }
}

#[test]
fn skill_lifecycle_batch_rejects_empty_stage_before_wal() {
    let path = unique_test_dir("skill_lifecycle_query_empty_stage");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Skill {id: 'skill_1', stage: 'candidate'})")
            .unwrap();
        let before = db.commit_epoch().unwrap();
        let wal = read_test_wal(&path).unwrap();
        for key in ["stage", "write_origin"] {
            assert!(
                lifecycle_parameters("skill_1", 100, &[(key, Value::String(String::new()))])
                    .unwrap_err()
                    .to_string()
                    .contains(key)
            );
        }
        assert!(lifecycle_parameters("skill_1", 100, &[])
            .unwrap_err()
            .to_string()
            .contains("lifecycle field"));
        assert_eq!(db.commit_epoch().unwrap(), before);
        assert_eq!(read_test_wal(&path).unwrap(), wal);
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn skill_lifecycle_queries_persist_as_one_wal_batch_and_replay() {
    let path = unique_test_dir("skill_lifecycle_query_replay");
    let inputs = [
        (
            PROMOTE_UPDATE,
            lifecycle_parameters(
                "skill_1",
                100,
                &[
                    ("stage", Value::String("promotable".into())),
                    ("rationale", Value::String("ready".into())),
                ],
            )
            .unwrap(),
        ),
        (
            CONTENT_UPDATE,
            lifecycle_parameters(
                "skill_2",
                200,
                &[
                    ("content_hash", Value::String("hash-updated".into())),
                    ("metadata", Value::String("{\"updated\":true}".into())),
                ],
            )
            .unwrap(),
        ),
    ];
    {
        let mut db = Database::open(&path).unwrap();
        db.query(
            "CREATE (:Skill {id: 'skill_1', stage: 'candidate', metadata: '{}', updated_at: 1})",
        )
        .unwrap();
        db.query("CREATE (:Skill {id: 'skill_2', stage: 'draft', metadata: '{}', updated_at: 1})")
            .unwrap();
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
        for (_, parameters) in &inputs {
            let output = db
                .query_request(bounded_batch_query(SKILL_READ, parameters))
                .unwrap();
            assert_eq!(output.rows.len(), 1);
            for (key, value) in parameters {
                assert_eq!(&output.rows[0][key.as_str()], value, "{key}");
            }
        }
        let retained = db
            .query_request(bounded_batch_query(SKILL_READ, &inputs[1].1))
            .unwrap();
        assert_eq!(retained.rows[0]["stage"], Value::String("draft".into()));
    }
    std::fs::remove_dir_all(path).unwrap();
}

const SOURCE_MERGE: &str = "MATCH (s:Skill {id: $id}), (m:Memory {id: $memory_id}) \
    MERGE (s)-[r:SYNTHESIZED_FROM]->(m) \
    ON CREATE SET r.weight = $weight, r.occasion_key = $occasion_key, r.created_at = $created_at";
const SOURCE_READ: &str =
    "MATCH (s:Skill {id: $id})-[r:SYNTHESIZED_FROM]->(m:Memory {id: $memory_id}) \
    RETURN id(s) AS skill_node_id, id(m) AS memory_node_id, id(r) AS relationship_id, \
    r.weight AS weight, r.occasion_key AS occasion_key, r.created_at AS created_at";

fn source_parameters(
    skill: &str,
    memory: &str,
    occasion: &str,
    created_at: i64,
) -> Result<BTreeMap<String, Value>> {
    if memory.is_empty() {
        return Err(HawDBError::Semantic("non-empty memory id required".into()));
    }
    skill_parameters(
        skill,
        &[
            ("memory_id", Value::String(memory.into())),
            ("weight", Value::Float(1.0)),
            ("occasion_key", Value::String(occasion.into())),
            ("created_at", Value::Int(created_at)),
        ],
    )
}

#[test]
fn merges_skill_source_for_rest_skills_write_shape() {
    let path = unique_test_dir("skill_source_query_replay");
    let parameters = source_parameters("skill-source-1", "memory-source-1", "", 100).unwrap();
    let relationship_id;
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Skill {id: 'skill-source-1', stage: 'active'})")
            .unwrap();
        db.query(
            "CREATE (:Memory {id: 'memory-source-1', title: 'Memory Source', created_at: 10})",
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
        assert_eq!(output.rows[0]["skill_node_id"], Value::Int(0));
        assert_eq!(output.rows[0]["memory_node_id"], Value::Int(1));
        relationship_id = output.rows[0]["relationship_id"].clone();
        assert_eq!(output.rows[0]["weight"], Value::Float(1.0));
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
        let second = source_parameters("skill-source-1", "memory-source-1", "later", 200).unwrap();
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
        assert_eq!(output.rows[0]["skill_node_id"], Value::Int(0));
        assert_eq!(output.rows[0]["memory_node_id"], Value::Int(1));
        assert_eq!(output.rows[0]["weight"], Value::Float(1.0));
        assert_eq!(output.rows[0]["occasion_key"], Value::String(String::new()));
        assert_eq!(output.rows[0]["created_at"], Value::Int(100));
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn skill_source_merge_reports_missing_endpoint_and_rejects_empty_ids() {
    let path = unique_test_dir("skill_source_query_missing_endpoint");
    {
        let mut db = Database::open(&path).unwrap();
        assert!(source_parameters("", "memory-source-1", "", 100)
            .unwrap_err()
            .to_string()
            .contains("non-empty skill id"));
        assert!(source_parameters("skill-source-1", "", "", 100)
            .unwrap_err()
            .to_string()
            .contains("non-empty memory id"));
        db.query("CREATE (:Skill {id: 'skill-source-1', stage: 'active'})")
            .unwrap();
        let before = db.commit_epoch().unwrap();
        let wal = read_test_wal(&path).unwrap();
        let parameters = source_parameters("skill-source-1", "missing-memory", "", 100).unwrap();
        let mut transaction = db.begin_transaction().unwrap();
        let skill = transaction
            .query_request(bounded_batch_query(SKILL_READ, &parameters))
            .unwrap();
        assert_eq!(skill.rows.len(), 1);
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

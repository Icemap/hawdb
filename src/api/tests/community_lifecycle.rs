// Copyright 2026 Nowledge
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use super::*;
use crate::QueryRequest;

fn bounded_query<'a>(query: &'a str, parameters: &'a BTreeMap<String, Value>) -> QueryRequest<'a> {
    QueryRequest::new(query)
        .with_params(parameters)
        .with_output_limits(QueryStreamOptions {
            max_rows: Some(4),
            max_payload_bytes: Some(4096),
        })
}

const CREATE: &str = "MERGE (c:Community {id: $id}) ON CREATE SET \
    c.community_id = $community_id, c.name = $name, c.description = $description, \
    c.ai_summary = $ai_summary, c.member_count = $member_count, c.algorithm = 'louvain', \
    c.resolution = $resolution, c.created_at = $created_at, c.updated_at = $updated_at \
    RETURN id(c) AS node_id, c.name AS name";
const UPDATE: &str = "MATCH (c:Community {id: $id}) \
    SET c.name = $name, c.description = $description, c.ai_summary = $ai_summary, \
    c.updated_at = $updated_at RETURN id(c) AS node_id";

fn resolution(value: f64) -> Result<Value> {
    if !value.is_finite() {
        return Err(HawDBError::Semantic("resolution must be finite".into()));
    }
    Ok(Value::Float(value))
}

#[test]
fn updates_community_lifecycle_for_nowledge_detection_and_summary() {
    let mut db = Database::new();
    db.query("CREATE (:Community {id: 'community_existing', community_id: 7, name: 'Old Name', description: 'old description', ai_summary: 'old summary', member_count: 2})")
        .unwrap();

    let before = db.commit_epoch().unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    for (
        id,
        number,
        name,
        description,
        summary,
        members,
        value,
        timestamp,
        expected_id,
        expected_name,
    ) in [
        (
            "community_new",
            8,
            "New Community",
            "new description",
            Value::Null,
            3,
            1.25,
            100,
            1,
            "New Community",
        ),
        (
            "community_existing",
            7,
            "Existing Community",
            "ignored",
            Value::String("ignored".into()),
            2,
            1.0,
            101,
            0,
            "Old Name",
        ),
    ] {
        let parameters = BTreeMap::from([
            ("id".into(), Value::String(id.into())),
            ("community_id".into(), Value::Int(number)),
            ("name".into(), Value::String(name.into())),
            ("description".into(), Value::String(description.into())),
            ("ai_summary".into(), summary),
            ("member_count".into(), Value::Int(members)),
            ("resolution".into(), resolution(value).unwrap()),
            ("created_at".into(), Value::Int(timestamp)),
            ("updated_at".into(), Value::Int(timestamp)),
        ]);
        let output = transaction
            .query_request(bounded_query(CREATE, &parameters))
            .unwrap();
        assert_eq!(output.rows.len(), 1);
        assert_eq!(output.rows[0]["node_id"], Value::Int(expected_id));
        assert_eq!(output.rows[0]["name"], Value::String(expected_name.into()));
    }
    for (id, name, description, summary, timestamp, expected) in [
        (
            "community_existing",
            "Updated Name",
            "updated description",
            "updated summary",
            200,
            1,
        ),
        ("missing_community", "Missing", "missing", "missing", 201, 0),
    ] {
        let parameters = BTreeMap::from([
            ("id".into(), Value::String(id.into())),
            ("name".into(), Value::String(name.into())),
            ("description".into(), Value::String(description.into())),
            ("ai_summary".into(), Value::String(summary.into())),
            ("updated_at".into(), Value::Int(timestamp)),
        ]);
        let output = transaction
            .query_request(bounded_query(UPDATE, &parameters))
            .unwrap();
        assert_eq!(output.rows.len(), expected);
    }
    transaction.commit().unwrap();
    assert_eq!(before, 1);
    assert_eq!(db.commit_epoch().unwrap(), before + 1);

    let created = db
        .query_request(bounded_query("MATCH (c:Community {id: 'community_new'}) RETURN c.community_id, c.name, c.description, c.ai_summary, c.member_count, c.algorithm, c.resolution, c.created_at, c.updated_at", &BTreeMap::new()))
        .unwrap();
    assert_eq!(created.rows[0].get("c.community_id"), Some(&Value::Int(8)));
    assert_eq!(
        created.rows[0].get("c.name"),
        Some(&Value::String("New Community".to_string()))
    );
    assert_eq!(
        created.rows[0].get("c.description"),
        Some(&Value::String("new description".to_string()))
    );
    assert_eq!(created.rows[0].get("c.ai_summary"), Some(&Value::Null));
    assert_eq!(created.rows[0].get("c.member_count"), Some(&Value::Int(3)));
    assert_eq!(
        created.rows[0].get("c.algorithm"),
        Some(&Value::String("louvain".to_string()))
    );
    assert_eq!(
        created.rows[0].get("c.resolution"),
        Some(&Value::Float(1.25))
    );
    assert_eq!(created.rows[0].get("c.created_at"), Some(&Value::Int(100)));
    assert_eq!(created.rows[0].get("c.updated_at"), Some(&Value::Int(100)));

    let updated = db
        .query_request(bounded_query("MATCH (c:Community {id: 'community_existing'}) RETURN c.name, c.description, c.ai_summary, c.updated_at", &BTreeMap::new()))
        .unwrap();
    assert_eq!(
        updated.rows[0].get("c.name"),
        Some(&Value::String("Updated Name".to_string()))
    );
    assert_eq!(
        updated.rows[0].get("c.description"),
        Some(&Value::String("updated description".to_string()))
    );
    assert_eq!(
        updated.rows[0].get("c.ai_summary"),
        Some(&Value::String("updated summary".to_string()))
    );
    assert_eq!(updated.rows[0].get("c.updated_at"), Some(&Value::Int(200)));
}

#[test]
fn community_lifecycle_rejects_invalid_create_before_wal() {
    let path = unique_test_dir("community_lifecycle_invalid_create_before_wal");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Community {id: 'community_existing'})")
            .unwrap();
    }
    let wal_before = read_test_wal(&path).unwrap();
    {
        let db = Database::open(&path).unwrap();
        let graph_commit_epoch_before = db.runtime.get().unwrap().store.commit_epoch();
        let error = resolution(f64::NAN).unwrap_err();
        assert!(error.to_string().contains("resolution must be finite"));
        assert_eq!(
            db.runtime.get().unwrap().store.commit_epoch(),
            graph_commit_epoch_before
        );
    }
    let wal_after = read_test_wal(&path).unwrap();
    assert_eq!(wal_after, wal_before);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn community_lifecycle_queries_persist_as_one_wal_batch_and_replays() {
    let path = unique_test_dir("community_lifecycle_query_wal_replay");
    {
        let mut db = Database::open(&path).unwrap();
        db.query("CREATE (:Community {id: 'community_existing', name: 'Old'})")
            .unwrap();
    }
    let setup_wal = read_test_wal(&path).unwrap();
    let setup_batch_count = setup_wal.matches("\tbatch\t").count();
    {
        let mut db = Database::open(&path).unwrap();
        let mut transaction = db.begin_transaction().unwrap();
        let create = BTreeMap::from([
            ("id".into(), Value::String("community_new".into())),
            ("community_id".into(), Value::Int(9)),
            ("name".into(), Value::String("New".into())),
            ("description".into(), Value::String("new".into())),
            ("ai_summary".into(), Value::String("summary".into())),
            ("member_count".into(), Value::Int(4)),
            ("resolution".into(), resolution(0.75).unwrap()),
            ("created_at".into(), Value::Int(10)),
            ("updated_at".into(), Value::Int(10)),
        ]);
        assert_eq!(
            transaction
                .query_request(bounded_query(CREATE, &create))
                .unwrap()
                .rows
                .len(),
            1
        );
        let update = BTreeMap::from([
            ("id".into(), Value::String("community_existing".into())),
            ("name".into(), Value::String("Updated".into())),
            ("description".into(), Value::String("updated".into())),
            ("ai_summary".into(), Value::String("updated summary".into())),
            ("updated_at".into(), Value::Int(20)),
        ]);
        assert_eq!(
            transaction
                .query_request(bounded_query(UPDATE, &update))
                .unwrap()
                .rows
                .len(),
            1
        );
        transaction.commit().unwrap();
    }
    let wal = read_test_wal(&path).unwrap();
    assert!(wal.contains("create_node"));
    assert!(wal.contains("set_node_property"));
    assert_eq!(wal.matches("\tbatch\t").count(), setup_batch_count + 1);
    {
        let mut db = Database::open(&path).unwrap();
        let communities = db
            .query_request(bounded_query(
                "MATCH (c:Community) RETURN count(c) AS total",
                &BTreeMap::new(),
            ))
            .unwrap();
        assert_eq!(communities.rows[0].get("total"), Some(&Value::Int(2)));
        let updated = db
            .query_request(bounded_query(
                "MATCH (c:Community {id: 'community_existing'}) RETURN c.name, c.updated_at",
                &BTreeMap::new(),
            ))
            .unwrap();
        assert_eq!(
            updated.rows[0].get("c.name"),
            Some(&Value::String("Updated".to_string()))
        );
        assert_eq!(updated.rows[0].get("c.updated_at"), Some(&Value::Int(20)));
    }
    std::fs::remove_dir_all(path).unwrap();
}

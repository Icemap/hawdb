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

const COUNT_SUMMARIES: &str = "MATCH (c:Community) WHERE c.ai_summary IS NOT NULL AND c.ai_summary <> '' RETURN count(c) AS total";
const SUMMARY_PAGE: &str =
    "MATCH (c:Community) WHERE c.ai_summary IS NOT NULL AND c.ai_summary <> '' \
    RETURN c.id AS id, c.member_count AS member_count, c.description AS description, \
    c.ai_summary AS ai_summary, c.updated_at AS updated_at, true AS has_summary \
    ORDER BY c.member_count DESC, c.name, c.community_id, c.id, id(c) LIMIT $limit";
const PRESENCE_PAGE: &str = "MATCH (c:Community) WHERE c.community_id >= $minimum \
    RETURN c.id AS id, CASE WHEN c.ai_summary IS NOT NULL AND c.ai_summary <> '' THEN true ELSE false END AS has_summary \
    ORDER BY has_summary DESC, c.member_count DESC, c.name, c.community_id, c.id, id(c) LIMIT $limit";
const BY_NUMBER: &str = "MATCH (c:Community {community_id: $community_id}) \
    RETURN c.id AS id, c.community_id AS community_id, c.name AS name, \
    c.description AS description, c.ai_summary AS ai_summary, c.member_count AS member_count, \
    c.updated_at AS updated_at, CASE WHEN c.ai_summary IS NOT NULL AND c.ai_summary <> '' THEN true ELSE false END AS has_summary \
    ORDER BY id(c) LIMIT 1";
const BY_ID: &str = "MATCH (c:Community {id: $id}) \
    RETURN c.community_id AS community_id, CASE WHEN c.ai_summary IS NOT NULL AND c.ai_summary <> '' THEN true ELSE false END AS has_summary \
    ORDER BY id(c) LIMIT 1";

fn bounded_query<'a>(
    query: &'a str,
    parameters: &'a BTreeMap<String, Value>,
    rows: usize,
) -> QueryRequest<'a> {
    QueryRequest::new(query)
        .with_params(parameters)
        .with_output_limits(QueryStreamOptions {
            max_rows: Some(rows),
            max_payload_bytes: Some(4096),
        })
}

fn id_parameters(id: &str) -> Result<BTreeMap<String, Value>> {
    if id.is_empty() {
        return Err(HawDBError::Semantic("non-empty id required".into()));
    }
    Ok(BTreeMap::from([("id".into(), Value::String(id.into()))]))
}

#[test]
fn reads_communities_for_nowledge_summary_list_shapes() {
    let mut db = Database::new_with_config(DatabaseConfig {
        max_plan_cache_entries: Some(8),
        statement_summary_capacity: 8,
        ..DatabaseConfig::default()
    });
    db.query("CREATE (:Community {id: 'community_a', community_id: 1, name: 'Alpha', description: 'alpha description', ai_summary: 'alpha summary', member_count: 5, updated_at: 10})")
        .unwrap();
    db.query("CREATE (:Community {id: 'community_b', community_id: 2, name: 'Beta', description: 'beta description', ai_summary: '', member_count: 10, updated_at: 20})")
        .unwrap();
    db.query("CREATE (:Community {id: 'community_c', community_id: 3, name: 'Gamma', description: 'gamma description', ai_summary: 'gamma summary', member_count: 3, updated_at: 30})")
        .unwrap();
    db.query("CREATE (:Community {id: 'community_negative', community_id: -1, name: 'Negative', ai_summary: 'negative summary', member_count: 100})")
        .unwrap();
    let epoch = db.commit_epoch().unwrap();
    let mut read = db.begin_read_transaction().unwrap();
    let count = read
        .query_request(bounded_query(COUNT_SUMMARIES, &BTreeMap::new(), 1))
        .unwrap();
    assert_eq!(count.rows[0]["total"], Value::Int(3));
    let parameters = BTreeMap::from([("limit".into(), Value::Int(2))]);
    let summary_only = read
        .query_request(bounded_query(SUMMARY_PAGE, &parameters, 2))
        .unwrap();
    assert_eq!(read.commit_epoch(), epoch);
    assert_eq!(summary_only.rows.len(), 2);
    assert_eq!(
        summary_only.rows[0]["id"],
        Value::String("community_negative".into())
    );
    assert_eq!(summary_only.rows[0]["member_count"], Value::Int(100));
    assert_eq!(summary_only.rows[0]["has_summary"], Value::Bool(true));
    assert_eq!(
        summary_only.rows[1]["id"],
        Value::String("community_a".into())
    );
    assert_eq!(
        summary_only.rows[1]["description"],
        Value::String("alpha description".into())
    );
    assert_eq!(
        summary_only.rows[1]["ai_summary"],
        Value::String("alpha summary".into())
    );
    assert_eq!(summary_only.rows[1]["updated_at"], Value::Int(10));

    let parameters = BTreeMap::from([
        ("minimum".into(), Value::Int(0)),
        ("limit".into(), Value::Int(3)),
    ]);
    let count = read
        .query_request(bounded_query(
            "MATCH (c:Community) WHERE c.community_id >= $minimum RETURN count(c) AS total",
            &parameters,
            1,
        ))
        .unwrap();
    assert_eq!(count.rows[0]["total"], Value::Int(3));
    let presence_ranked = read
        .query_request(bounded_query(PRESENCE_PAGE, &parameters, 3))
        .unwrap();
    assert_eq!(presence_ranked.rows.len(), 3);
    for (row, id) in presence_ranked
        .rows
        .iter()
        .zip(["community_a", "community_c", "community_b"])
    {
        assert_eq!(row["id"], Value::String(id.into()));
    }
    assert_eq!(presence_ranked.rows[2]["has_summary"], Value::Bool(false));
    let entries = read_test_plan_cache_metric(&read, "entries");
    let misses = read_test_plan_cache_metric(&read, "misses");
    let hits = read_test_plan_cache_metric(&read, "hits");
    let repeated = read
        .query_request(bounded_query(PRESENCE_PAGE, &parameters, 3))
        .unwrap();
    assert_eq!(repeated, presence_ranked);
    assert_eq!(read_test_plan_cache_metric(&read, "entries"), entries);
    assert_eq!(read_test_plan_cache_metric(&read, "misses"), misses);
    assert_eq!(read_test_plan_cache_metric(&read, "hits"), hits + 1);
}

#[test]
fn community_list_read_returns_empty_without_community_label() {
    let db = Database::new();
    let mut read = db.begin_read_transaction().unwrap();
    let epoch = read.commit_epoch();
    let count = read
        .query_request(bounded_query(COUNT_SUMMARIES, &BTreeMap::new(), 1))
        .unwrap();
    assert_eq!(count.rows[0]["total"], Value::Int(0));
    let parameters = BTreeMap::from([("limit".into(), Value::Int(10))]);
    let output = read
        .query_request(bounded_query(SUMMARY_PAGE, &parameters, 10))
        .unwrap();
    assert!(output.rows.is_empty());
    assert_eq!(read.commit_epoch(), epoch);
}

#[test]
fn reads_community_detail_for_wiki_and_mcp_shapes() {
    let mut db = Database::new_with_config(DatabaseConfig {
        max_plan_cache_entries: Some(8),
        statement_summary_capacity: 8,
        ..DatabaseConfig::default()
    });
    db.query("CREATE (:Community {id: 'community_a', community_id: 7, name: 'Alpha', description: 'alpha description', ai_summary: 'alpha summary', member_count: 5, updated_at: 10})")
        .unwrap();
    db.query("CREATE (:Community {id: 'community_b', community_id: 8, name: 'Beta', description: 'beta description', ai_summary: '', member_count: 3})")
        .unwrap();
    let epoch = db.commit_epoch().unwrap();
    let mut read = db.begin_read_transaction().unwrap();
    let parameters = BTreeMap::from([("community_id".into(), Value::Int(7))]);
    let by_number = read
        .query_request(bounded_query(BY_NUMBER, &parameters, 1))
        .unwrap();
    assert_eq!(read.commit_epoch(), epoch);
    assert_eq!(by_number.rows.len(), 1);
    let row = &by_number.rows[0];
    assert_eq!(row["id"], Value::String("community_a".into()));
    assert_eq!(row["community_id"], Value::Int(7));
    assert_eq!(row["name"], Value::String("Alpha".into()));
    assert_eq!(
        row["description"],
        Value::String("alpha description".into())
    );
    assert_eq!(row["ai_summary"], Value::String("alpha summary".into()));
    assert_eq!(row["member_count"], Value::Int(5));
    assert_eq!(row["updated_at"], Value::Int(10));
    assert_eq!(row["has_summary"], Value::Bool(true));
    let entries = read_test_plan_cache_metric(&read, "entries");
    let misses = read_test_plan_cache_metric(&read, "misses");
    let hits = read_test_plan_cache_metric(&read, "hits");
    let repeated = read
        .query_request(bounded_query(BY_NUMBER, &parameters, 1))
        .unwrap();
    assert_eq!(repeated, by_number);
    assert_eq!(read_test_plan_cache_metric(&read, "entries"), entries);
    assert_eq!(read_test_plan_cache_metric(&read, "misses"), misses);
    assert_eq!(read_test_plan_cache_metric(&read, "hits"), hits + 1);
    let parameters = id_parameters("community_b").unwrap();
    let by_id = read
        .query_request(bounded_query(BY_ID, &parameters, 1))
        .unwrap();
    assert_eq!(by_id.rows.len(), 1);
    assert_eq!(by_id.rows[0]["community_id"], Value::Int(8));
    assert_eq!(by_id.rows[0]["has_summary"], Value::Bool(false));
    let parameters = BTreeMap::from([("community_id".into(), Value::Int(99))]);
    let missing = read
        .query_request(bounded_query(BY_NUMBER, &parameters, 1))
        .unwrap();
    assert!(missing.rows.is_empty());
}

#[test]
fn community_detail_input_rejects_empty_id() {
    assert!(id_parameters("")
        .unwrap_err()
        .to_string()
        .contains("non-empty id"));
}

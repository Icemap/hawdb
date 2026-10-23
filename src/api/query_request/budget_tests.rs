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

use crate::{
    ConcurrentDatabase, Database, QueryRequest, QueryStreamOptions, ReadSnapshotBudget,
    SqlQueryRequest, Value,
};

fn fixture() -> Database {
    let mut database = Database::new();
    database.query("CREATE (:Probe {id: 1})").unwrap();
    database
        .query_sql("CREATE TABLE records (id BIGINT PRIMARY KEY)")
        .unwrap();
    database
        .query_sql("INSERT INTO records (id) VALUES (1), (2)")
        .unwrap();
    database
}

#[test]
fn mixed_statement_budget_keeps_one_snapshot_while_writers_advance() {
    let database = ConcurrentDatabase::new(fixture());
    let mut snapshot =
        database
            .begin_read_transaction()
            .unwrap()
            .into_bounded(ReadSnapshotBudget {
                max_rows: 3,
                max_payload_bytes: 30,
            });
    let epoch = snapshot.commit_epoch();
    database.query("CREATE (:Probe {id: 2})").unwrap();
    database
        .query_sql("INSERT INTO records (id) VALUES (3)")
        .unwrap();
    let output = snapshot
        .query_request(QueryRequest::new("MATCH (p:Probe) RETURN p.id AS id"))
        .unwrap();
    assert_eq!(output.rows.len(), 1);
    assert_eq!(output.rows[0]["id"], Value::Int(1));
    assert_eq!(snapshot.usage().output_payload_bytes, 10);
    let output = snapshot
        .query_sql_request(SqlQueryRequest::new("SELECT id FROM records ORDER BY id"))
        .unwrap();
    assert_eq!(output.rows.len(), 2);
    assert_eq!(snapshot.usage().output_rows, 3);
    assert_eq!(snapshot.usage().output_payload_bytes, 30);
    assert_eq!(snapshot.commit_epoch(), epoch);
    assert_eq!(snapshot.usage().cypher_statement_count, 1);
    assert_eq!(snapshot.usage().sql_statement_count, 1);
    let before = snapshot.usage();
    assert!(snapshot
        .query_request(QueryRequest::new("MATCH (p:Probe) RETURN p.id AS id"))
        .is_err());
    assert!(snapshot
        .query_sql_request(SqlQueryRequest::new("SELECT id FROM records"))
        .is_err());
    assert_eq!(snapshot.usage(), before);
    assert_eq!(
        database
            .query_sql("SELECT id FROM records")
            .unwrap()
            .rows
            .len(),
        3
    );
}

#[test]
fn refused_results_do_not_consume_quota_and_statement_caps_never_widen() {
    let mut snapshot =
        fixture()
            .begin_read_transaction()
            .unwrap()
            .into_bounded(ReadSnapshotBudget {
                max_rows: 10,
                max_payload_bytes: 19,
            });
    // SQL's compact schema appears once, but logical delivery charges it per row.
    assert!(snapshot
        .query_sql_request(SqlQueryRequest::new("SELECT id FROM records"))
        .is_err());
    assert_eq!(snapshot.usage().output_rows, 0);
    let request = QueryRequest::new("MATCH (p:Probe) RETURN p.id AS id");
    assert!(snapshot
        .query_request(request.with_output_limits(QueryStreamOptions {
            max_rows: Some(0),
            max_payload_bytes: Some(19)
        }))
        .is_err());
    snapshot.query_request(request).unwrap();
    assert_eq!(snapshot.usage().output_payload_bytes, 10);
    assert!(snapshot
        .query_sql_request(SqlQueryRequest::new("SELECT id FROM records WHERE id = 1"))
        .is_err());
    assert_eq!(snapshot.usage().output_rows, 1);
    assert_eq!(snapshot.usage().sql_statement_count, 0);
}

#[test]
fn explain_charges_the_delivered_report_instead_of_inner_execution_rows() {
    let mut snapshot =
        fixture()
            .begin_read_transaction()
            .unwrap()
            .into_bounded(ReadSnapshotBudget {
                max_rows: 1,
                max_payload_bytes: 1024 * 1024,
            });
    let output = snapshot
        .query_request(QueryRequest::new(
            "EXPLAIN MATCH (p:Probe) RETURN p.id AS id",
        ))
        .unwrap();
    assert_eq!(snapshot.usage().output_rows, 1);
    assert_eq!(
        snapshot.usage().output_payload_bytes,
        output.rows.payload_bytes()
    );
    assert!(snapshot
        .query_sql_request(SqlQueryRequest::new("SELECT id FROM records WHERE id = 1"))
        .is_err());
}

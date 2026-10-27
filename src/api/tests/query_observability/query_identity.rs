// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::telemetry::{QueryTelemetry, TelemetrySink};
use crate::QueryRequest;
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
struct IdentityTelemetry(Mutex<Vec<(String, bool, usize)>>);

impl TelemetrySink for IdentityTelemetry {
    fn record_query(&self, event: QueryTelemetry<'_>) {
        self.0.lock().unwrap().push((
            event.query_digest.to_owned(),
            event.success,
            event.row_count,
        ));
    }
}

fn fixture(cache_entries: usize) -> (Database, Arc<IdentityTelemetry>) {
    let mut database = Database::new_with_config(DatabaseConfig {
        max_plan_cache_entries: Some(cache_entries),
        slow_query_log_threshold_micros: 0,
        slow_query_log_capacity: 16,
        statement_summary_capacity: 16,
        ..DatabaseConfig::default()
    });
    database.query("CREATE INDEX ON :IdentityItem(id)").unwrap();
    for id in 1..=2 {
        database
            .query_with_params(
                "CREATE (:IdentityItem {id: $id, value: $value})",
                &BTreeMap::from([
                    ("id".to_owned(), Value::Int(id)),
                    ("value".to_owned(), Value::Int(id * 10)),
                ]),
            )
            .unwrap();
    }
    let telemetry = Arc::new(IdentityTelemetry::default());
    database.set_telemetry_sink(Some(telemetry.clone()));
    (database, telemetry)
}

fn exercise(
    mut query: impl FnMut(QueryRequest<'_>) -> Result<QueryOutput>,
    mutation: bool,
    telemetry: &IdentityTelemetry,
) -> [hawdb_query::QueryIdentity; 2] {
    let queries = if mutation {
        [
            "MATCH (n:IdentityItem {id: $id}) SET n.value = $value RETURN n.value AS value",
            " match (n:IdentityItem { id : $id }) set n.value = $value return n.value as value; ",
        ]
    } else {
        [
            "MATCH (n:IdentityItem {id: $id}) RETURN n.value AS value",
            " match (n:IdentityItem { id : $id }) return n.value as value; ",
        ]
    };
    let identities = queries.map(|text| hawdb_query::QueryIdentity::new("cypher", text));
    assert_eq!(identities[0].query_digest(), identities[1].query_digest());
    assert_ne!(
        identities[0].query_text_hash(),
        identities[1].query_text_hash()
    );
    for (index, text) in queries.into_iter().enumerate() {
        let id = index as i64 + 1;
        let parameters = BTreeMap::from([
            ("id".to_owned(), Value::Int(id)),
            ("value".to_owned(), Value::Int(id * 100)),
        ]);
        let output = query(QueryRequest::new(text).with_params(&parameters)).unwrap();
        assert_eq!(
            output.rows,
            vec![BTreeMap::from([(
                "value".to_owned(),
                Value::Int(id * if mutation { 100 } else { 10 }),
            )])]
        );
    }
    let parameters = BTreeMap::from([
        ("id".to_owned(), Value::Int(1)),
        ("value".to_owned(), Value::Int(999)),
    ]);
    // Refuse after a plan exists, then fail during preparation. Neither error
    // belongs in the completed slow-query ring; both belong in the summary.
    query(
        QueryRequest::new(queries[1])
            .with_params(&parameters)
            .with_output_limits(QueryStreamOptions {
                max_rows: Some(0),
                max_payload_bytes: None,
            }),
    )
    .expect_err("output budget must refuse a nonempty result");
    query(QueryRequest::new(queries[0])).expect_err("missing binding must fail preparation");
    let digest = identities[0].query_digest().to_owned();
    assert_eq!(
        *telemetry.0.lock().unwrap(),
        vec![
            (digest.clone(), true, 1),
            (digest.clone(), true, 1),
            (digest.clone(), false, 0),
            (digest, false, 0),
        ]
    );
    identities
}

fn assert_observations(
    mut query_sql: impl FnMut(&str, &[Value]) -> Result<QueryOutput>,
    identities: &[hawdb_query::QueryIdentity; 2],
) {
    let parameters = [Value::String(identities[0].query_digest().to_owned())];
    let slow = query_sql(
        "SELECT query_digest, query_text_hash, row_count FROM system.slow_queries \
         WHERE query_digest = $1 ORDER BY sequence",
        &parameters,
    )
    .unwrap();
    assert_eq!(slow.rows.len(), 2);
    for (row, identity) in slow.rows.iter().zip(identities) {
        assert_eq!(row.get("row_count"), Some(&Value::Int(1)));
        assert_eq!(
            row.get("query_digest"),
            Some(&Value::String(identity.query_digest().to_owned()))
        );
        assert_eq!(
            row.get("query_text_hash"),
            Some(&Value::String(identity.query_text_hash().to_owned()))
        );
    }
    let summary = query_sql(
        "SELECT execution_count, success_count, error_count, sample_query_text_hash \
         FROM system.statement_summary WHERE digest = $1",
        &parameters,
    )
    .unwrap();
    assert_eq!(summary.rows.len(), 1);
    let row = &summary.rows[0];
    assert_eq!(row.get("execution_count"), Some(&Value::Int(4)));
    assert_eq!(row.get("success_count"), Some(&Value::Int(2)));
    assert_eq!(row.get("error_count"), Some(&Value::Int(2)));
    assert_eq!(
        row.get("sample_query_text_hash"),
        Some(&Value::String(identities[0].query_text_hash().to_owned()))
    );
}

#[test]
fn ordinary_query_identity_survives_rebinding_disabled_cache_and_failure() {
    for cache_entries in [0, 8] {
        for mutation in [false, true] {
            let (mut database, telemetry) = fixture(cache_entries);
            let identities = exercise(
                |request| database.query_request(request),
                mutation,
                &telemetry,
            );
            if !mutation && cache_entries > 0 {
                assert!(database.plan_cache_stats().unwrap().hits > 0);
            }
            assert_observations(
                |sql, parameters| database.query_sql_with_params(sql, parameters),
                &identities,
            );
        }
    }
}

#[test]
fn concurrent_query_identity_survives_rebinding_disabled_cache_and_failure() {
    for cache_entries in [0, 8] {
        for mutation in [false, true] {
            let (database, telemetry) = fixture(cache_entries);
            let database = database.into_concurrent();
            let identities = exercise(
                |request| database.query_request(request),
                mutation,
                &telemetry,
            );
            assert_observations(
                |sql, parameters| database.query_sql_with_params(sql, parameters),
                &identities,
            );
        }
    }
}

// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn simple_case_dispatch_rebinds_values_through_scan_and_projection() {
    for column in [false, true] {
        let mut db = Database::new_with_config(DatabaseConfig {
            max_plan_cache_entries: Some(8),
            ..Default::default()
        });
        let mut parameters = BTreeMap::new();
        let mut expression = if column { "CASE key" } else { "CASE n.id" }.to_owned();
        for index in 0..20 {
            db.query(&format!("CREATE (:Item {{id: 'item-{index}'}})"))
                .unwrap();
            parameters.insert(
                format!("id_{index}"),
                Value::String(format!("item-{index}")),
            );
            parameters.insert(format!("score_{index}"), Value::Int(index));
            expression.push_str(&format!(" WHEN $id_{index} THEN $score_{index}"));
        }
        expression.push_str(" ELSE null END AS score");
        let query = if column {
            format!("MATCH (n:Item) WITH n.id AS key RETURN key, {expression} ORDER BY key")
        } else {
            format!("MATCH (n:Item) RETURN n.id AS key, {expression} ORDER BY key")
        };
        let mut read = db.begin_read_transaction().unwrap();
        for value in [0, 77, -3] {
            parameters.insert("score_0".into(), Value::Int(value));
            let mut rows = Vec::new();
            let output = read
                .query_with_params_streaming(
                    &query,
                    &parameters,
                    QueryStreamOptions {
                        max_rows: Some(20),
                        max_payload_bytes: Some(4096),
                    },
                    |row| {
                        rows.push(row);
                        Ok(())
                    },
                )
                .unwrap();
            assert_eq!(rows.len(), 20);
            assert_eq!(rows[0]["score"], Value::Int(value));
            let memory = output.execution_profile.pipeline_memory_report;
            assert_eq!(memory.simple_case_dispatch_tables, 1);
            assert_eq!(memory.simple_case_dispatch_lookups, 20);
            assert_eq!(memory.query_memory_completion_bytes, 0);
        }
        assert_eq!(read_test_plan_cache_metric(&read, "misses"), 1);
        assert_eq!(read_test_plan_cache_metric(&read, "hits"), 2);
    }
}

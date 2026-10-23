// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

//! Release-only query/reference qualification. The reference retains all IDs.
//! Setup, warmup and equality checks are outside the recorded latency samples.

use hawdb::{Database, DatabaseConfig, QueryRequest, Value};
use serde_json::json;
use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::time::Instant;

#[path = "grouped_evidence/reference.rs"]
mod reference;
use reference::{select_groups, FullGroups};

const RAW: &str = "MATCH (r:EvidenceInput) WHERE r.eligible = true RETURN r.group_id AS g, r.document_id AS doc, r.score AS score";
const GROUPED: &str = "MATCH (r:EvidenceInput) WHERE r.eligible = true WITH r.group_id AS g, r.document_id AS doc, r.score AS score WITH g, MAX(score) AS best, collect_top(doc, score, $m) AS evidence RETURN g, best, evidence ORDER BY best DESC, g ASC LIMIT $k";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args()
        .skip(1)
        .map(|s| s.parse::<usize>())
        .collect::<Result<Vec<_>, _>>()?;
    let [n, group_count, hot_percent, eligibility_stride, m, k, id_width, repeats] =
        args.as_slice()
    else {
        return Err(
            "expected rows groups hot_percent eligibility_stride M K ID_width repeats".into(),
        );
    };
    let (n, group_count, hot_percent, eligibility_stride, m, k, id_width, repeats) = (
        *n,
        *group_count,
        *hot_percent,
        *eligibility_stride,
        *m,
        *k,
        *id_width,
        *repeats,
    );
    if n == 0
        || n > 100_000
        || group_count == 0
        || group_count > n
        || hot_percent > 100
        || eligibility_stride == 0
        || m == 0
        || k == 0
        || id_width > 4096
        || repeats == 0
        || repeats > 100
    {
        return Err("invalid qualification dimensions".into());
    }
    let mut config = DatabaseConfig::default();
    config.execution_memory.query_memory_bytes = NonZeroUsize::new(256 * 1024 * 1024).unwrap();
    config.execution_memory.blocking_operator_bytes = NonZeroUsize::new(128 * 1024 * 1024).unwrap();
    let mut database = Database::new_with_config(config);
    let fixture = (0..n)
        .map(|i| {
            let group = if i % 100 < hot_percent {
                0
            } else {
                i % group_count
            };
            // Adjacent pairs share an ID while scores can arrive in either order.
            let id = format!("{:0width$}", i / 2, width = id_width);
            let score = ((i * 7919) % 1009) as f64 / 4.0 - 100.0;
            Value::Map(BTreeMap::from([
                ("g".into(), Value::Int(group as i64)),
                ("id".into(), Value::String(id)),
                ("score".into(), Value::Float(score)),
                ("eligible".into(), Value::Bool(i % eligibility_stride == 0)),
            ]))
        })
        .collect();
    database.query_with_params("UNWIND $rows AS r CREATE (:EvidenceInput {group_id: r.g, document_id: r.id, score: r.score, eligible: r.eligible})", &BTreeMap::from([("rows".into(), Value::List(fixture))]))?;
    let parameters = BTreeMap::from([
        ("m".into(), Value::Int(i64::try_from(m)?)),
        ("k".into(), Value::Int(i64::try_from(k)?)),
    ]);
    let plans = [RAW, GROUPED]
        .into_iter()
        .map(|query| {
            database
                .query_with_params(&format!("EXPLAIN {query}"), &parameters)
                .map(|output| format!("{:?}", output.rows[0]["plan"]))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut snapshot = database.begin_read_transaction()?;
    let mut samples = Vec::new();
    for run in 0..=repeats {
        // Alternate order to expose, rather than hide, order/cache effects.
        let mut expected = Vec::new();
        let mut actual = Vec::new();
        let mut timings = [0u128; 2];
        let mut profile = None;
        let mut reference_profile = None;
        let mut reference_rows = 0usize;
        for path in if run % 2 == 0 { [0, 1] } else { [1, 0] } {
            let started = Instant::now();
            if path == 0 {
                let mut groups = FullGroups::new();
                reference_profile = Some(
                    snapshot
                        .query_request_streaming(QueryRequest::new(RAW), |row| {
                            let (Value::Int(g), Value::String(id), Value::Float(score)) =
                                (&row["g"], &row["doc"], &row["score"])
                            else {
                                panic!("fixture types")
                            };
                            groups
                                .entry(*g)
                                .or_default()
                                .entry(id.clone())
                                .and_modify(|old| {
                                    if score.total_cmp(old).is_gt() {
                                        *old = *score;
                                    }
                                })
                                .or_insert(*score);
                            reference_rows += 1;
                            Ok(())
                        })?
                        .execution_profile,
                );
                expected = select_groups(groups, m, k);
            } else {
                profile = Some(
                    snapshot
                        .query_request_streaming(
                            QueryRequest::new(GROUPED).with_params(&parameters),
                            |row| {
                                actual.push(row);
                                Ok(())
                            },
                        )?
                        .execution_profile,
                );
            }
            timings[path] = started.elapsed().as_nanos();
        }
        assert_eq!(actual, expected, "run {run}");
        let profile = profile.unwrap();
        let reference_profile = reference_profile.unwrap();
        assert_eq!(
            profile.pipeline_memory_report.query_memory_completion_bytes,
            0
        );
        let group = &profile.grouped_evidence_reports[0];
        assert!(group.executed && group.input_exhausted);
        assert_eq!(group.input_rows, reference_rows);
        if run != 0 {
            samples.push(
                json!({"run": run, "reference_ns": timings[0], "grouped_ns": timings[1],
                "reference_intermediate_rows": reference_profile.pipeline_memory_report.intermediate_rows,
                "grouped_intermediate_rows": profile.pipeline_memory_report.intermediate_rows,
                "reference_intermediate_payload_bytes": reference_profile.pipeline_memory_report.intermediate_payload_bytes,
                "grouped_intermediate_payload_bytes": profile.pipeline_memory_report.intermediate_payload_bytes,
                "reference_operator_rows": reference_profile.operator_cardinality_profiles.iter().map(|p| format!("{:?}: {:?}", p.operator, p.actual_rows)).collect::<Vec<_>>(),
                "grouped_operator_rows": profile.operator_cardinality_profiles.iter().map(|p| format!("{:?}: {:?}", p.operator, p.actual_rows)).collect::<Vec<_>>(),
                "borrowed_group_key_rows": group.borrowed_group_key_rows,
                "input_rows": group.input_rows, "groups_considered": group.groups_considered,
                "retained_evidence": group.retained_evidence, "output_groups": group.output_groups,
                "peak_state_bytes": group.peak_state_bytes, "spilled_bytes": group.spilled_bytes,
                "query_memory_peak_bytes": profile.pipeline_memory_report.query_memory_peak_bytes,
                "process_peak_resident_bytes": profile.pipeline_memory_report.peak_resident_bytes}),
            );
        }
    }
    println!(
        "{}",
        json!({"status": "equal", "rows": n, "groups": group_count, "hot_percent": hot_percent,
        "eligibility_stride": eligibility_stride, "M": m, "K": k, "id_width": id_width,
        "plans": plans, "samples": samples, "scope": "In-memory canonical scalar input; no lexical I/O, graph traversal, allocation instrumentation or process-isolated RSS claim. Reference includes streaming full-ID maximum and full rank sort. Both paths include query planning, execution and result construction; one warmup per path."})
    );
    Ok(())
}

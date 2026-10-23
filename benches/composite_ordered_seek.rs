// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

//! Manual read-only comparisons; fixture writes are outside the timed region.
//! A reopened handle is not a device-cold cache. Raw samples retain regressions.

use hawdb::{Database, DatabaseConfig, QueryRequest, StorageResidencyMode, Value};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

fn populate(db: &mut Database, rows: usize) -> hawdb::Result<()> {
    db.query("CREATE NODE TABLE Item")?;
    db.query("CREATE PROPERTY ON NODE TABLE Item(rank) TYPE INT NOT NULL")?;
    db.query("CREATE PROPERTY ON NODE TABLE Item(id) TYPE STRING NOT NULL")?;
    db.query("CREATE CONSTRAINT ON :Item(id) ASSERT UNIQUE")?;
    let fixture = (0..rows)
        .map(|id| {
            Value::Map(BTreeMap::from([
                (
                    "kind".into(),
                    Value::String(if id % 5 == 0 { "archive" } else { "note" }.into()),
                ),
                ("rank".into(), Value::Int((id / 4) as i64)),
                ("id".into(), Value::String(format!("item-{id:08}"))),
                ("body".into(), Value::String("x".repeat(256))),
            ]))
        })
        .collect::<Vec<_>>();
    // Keep fixture transactions below the unchanged default WAL record ceiling.
    for batch in fixture.chunks(4096) {
        db.query_with_params(
            "UNWIND $rows AS r CREATE (:Item {kind: r.kind, rank: r.rank, id: r.id, body: r.body})",
            &BTreeMap::from([("rows".into(), Value::List(batch.to_vec()))]),
        )?;
    }
    db.query("CREATE INDEX ON :Item(kind, rank, id)")?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let rows: usize = args.next().ok_or("missing rows")?.parse()?;
    let path = args.next().ok_or("missing path or '-' for memory")?;
    let repeats: usize = args.next().unwrap_or_else(|| "7".into()).parse()?;
    if !(64..=100_000).contains(&rows) || repeats == 0 || repeats > 100 || args.next().is_some() {
        return Err("expected rows64..100000 path repeats1..100".into());
    }
    let persistent = path != "-";
    if persistent && Path::new(&path).exists() {
        return Err("persistent fixture requires a fresh path".into());
    }
    let config = DatabaseConfig {
        storage_residency_mode: StorageResidencyMode::OutOfCore,
        ..Default::default()
    };
    let started = Instant::now();
    let mut db = if persistent {
        Database::open_with_config(&path, config.clone())?
    } else {
        Database::new()
    };
    populate(&mut db, rows)?;
    if persistent {
        db.checkpoint()?;
        drop(db);
        db = Database::open_with_config(&path, config)?;
    }
    let setup_ns = started.elapsed().as_nanos();
    for descending in [false, true] {
        let (direction, op) = if descending {
            ("DESC", "<")
        } else {
            ("ASC", ">")
        };
        for depth in [0, rows / 2, rows - 2] {
            let params = BTreeMap::from([
                ("kind".into(), Value::String("note".into())),
                ("rank".into(), Value::Int((depth / 4) as i64)),
                ("id".into(), Value::String(format!("item-{depth:08}"))),
            ]);
            let filter = if depth == 0 {
                "n.kind = $kind".into()
            } else {
                format!(
                    "n.kind = $kind AND (n.rank {op} $rank OR (n.rank = $rank AND n.id {op} $id))"
                )
            };
            let query = format!("MATCH (n:Item) WHERE {filter} RETURN n.rank AS rank, n.id AS id ORDER BY rank {direction}, id {direction} LIMIT 32");
            // The same filter, scalar values and Top-K; only ordering inference is disabled.
            let reference = query.replace("ORDER BY rank", "ORDER BY coalesce(rank, 0)");
            let plan = db
                .explain_query_with_params(&query, &params)?
                .physical_plan
                .explain(0);
            assert!(plan.contains("IndexNodeCompositeOrderedSeek"), "{plan}");
            let reference_plan = db
                .explain_query_with_params(&reference, &params)?
                .physical_plan
                .explain(0);
            assert!(
                reference_plan.contains("TopNExec") || reference_plan.contains("SortExec"),
                "{reference_plan}"
            );
            let mut snapshot = db.begin_read_transaction()?;
            for run in 0..=repeats {
                let mut outputs = [Vec::new(), Vec::new()];
                let mut samples = Vec::new();
                for path_index in if run % 2 == 0 { [0, 1] } else { [1, 0] } {
                    let before = db.storage_residency_report()?;
                    let start = Instant::now();
                    let report = snapshot.query_request_streaming(
                        QueryRequest::new([&query, &reference][path_index]).with_params(&params),
                        |row| {
                            outputs[path_index].push(row);
                            Ok(())
                        },
                    )?;
                    let elapsed = start.elapsed().as_nanos();
                    let after = db.storage_residency_report()?;
                    let reads = after
                        .graph_index_reads
                        .delta_since(before.graph_index_reads);
                    let profile = report.execution_profile;
                    assert_eq!(
                        profile.pipeline_memory_report.query_memory_completion_bytes,
                        0
                    );
                    samples.push(json!({"path": if path_index == 0 {"ordered"} else {"reference"}, "ns":elapsed,
                        "index_descriptors":reads.property_descriptor_pages_visited, "index_blocks":reads.property_blocks_read,
                        "index_entries_decoded":reads.property_entries_decoded, "index_bytes":reads.property_bytes_read,
                        "index_descriptor_bytes":reads.property_descriptor_storage_bytes_read,
                        "index_candidates":reads.property_candidates_returned,
                        "cache_capacity_bytes":after.segment_cache_capacity_bytes,
                        "cache_resident_bytes_before":before.segment_cache_resident_bytes,
                        "cache_resident_bytes_after":after.segment_cache_resident_bytes,
                        "cache_pinned_bytes_after":after.segment_cache_pinned_bytes,
                        "cache_evictions":after.segment_cache_eviction_count.saturating_sub(before.segment_cache_eviction_count),
                        "cache_admission_rejections":after.segment_cache_admission_rejection_count.saturating_sub(before.segment_cache_admission_rejection_count),
                        "query_peak_bytes":profile.pipeline_memory_report.query_memory_peak_bytes,
                        "blocking_peak_bytes":profile.blocking_operator_memory_reports.iter().map(|report| report.peak_tracked_bytes).max().unwrap_or(0),
                        "scan_reports":profile.scan_pruning_reports.iter().map(|report| json!({"strategy":format!("{:?}", report.strategy), "candidates_before_filter":report.candidate_count_before_filter, "output_rows":report.output_count, "filtered_rows":report.filtered_out_count})).collect::<Vec<_>>()}));
                }
                assert_eq!(outputs[0], outputs[1]);
                println!(
                    "{}",
                    json!({"rows":rows,"backend":if persistent {"reopened-out-of-core"} else {"resident"},"setup_ns":setup_ns,"descending":descending,"depth":depth,"page_size":32,"run":run,"warmup":run==0,"output_rows":outputs[0].len(),"samples":samples})
                );
            }
        }
    }
    Ok(())
}

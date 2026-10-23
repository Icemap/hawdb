// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

//! End-to-end lexical producer, canonical graph eligibility and grouped evidence.
//! The full-window control covers this synthetic corpus because its size is known.

use hawdb::executor::{
    ExternalReadOperator, TextSeedExecutionOutput, TextSeedExecutionRequest, TextSeedSourceReport,
};
use hawdb::{
    Database, DatabaseConfig, GraphExpansionBudget, KernelWriteBatch, QueryRequest, SearchIndex,
    SearchOutOfCoreGenerationWriter, SearchOutOfCoreReader, SearchProjectionDelta,
    SearchProjectionKind, SearchProjectionRow, Value,
};
use hawdb_storage::mutation::{ConnectedNodesCreate, GraphMutation};
use serde_json::json;
use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::time::Instant;

#[path = "grouped_evidence/reference.rs"]
mod reference;
use reference::{select_groups, FullGroups as Groups};

const PREFIX: &str = "CALL text_search($text, complete := true, maxCandidates := $cap) YIELD id AS document_id, score MATCH (seed:Memory) MATCH (seed)-[:VISIBLE_IN]->(scope:Scope) WHERE scope.visible = true";
const GROUP: &str = " WITH seed.group_id AS g, MAX(score) AS best, collect_top(document_id, score, $m) AS evidence RETURN g, best, evidence ORDER BY best DESC, g ASC LIMIT $k";
const RAW: &str = " RETURN seed.group_id AS g, document_id AS doc, score";

enum Source {
    Resident(Box<SearchIndex>),
    Disk(Box<SearchOutOfCoreReader>),
}

impl Source {
    fn execute(
        &self,
        request: TextSeedExecutionRequest<'_>,
        complete: bool,
    ) -> hawdb::Result<TextSeedExecutionOutput> {
        request.resources.checkpoint()?;
        let mut output = if complete {
            TextSeedExecutionOutput::new_complete(request.result_account, request.resources.result)
        } else {
            TextSeedExecutionOutput::new(request.result_account, request.resources.result)
        }?;
        let default_task = hawdb_core::RuntimeTaskContext::default();
        let task = request.resources.task_context.unwrap_or(&default_task);
        match self {
            Self::Resident(index) => {
                let scored = if complete {
                    index.complete_text_seed_scores_with_context(
                        request.query_text,
                        request.metadata_filters,
                        request.resources.result.max_rows,
                        request.working_account,
                        task,
                    )
                } else {
                    index.text_seed_scores_with_context(
                        request.query_text,
                        request.metadata_filters,
                        request.resources.result.max_rows,
                        request.working_account,
                        task,
                    )
                }?;
                output = output.with_source_report(TextSeedSourceReport {
                    corpus_documents: Some(scored.corpus_document_count),
                    matching_documents: Some(scored.matching_document_count),
                    ..Default::default()
                });
                let append = |id: &str, score: f64, output: &mut TextSeedExecutionOutput| {
                    request.resources.checkpoint()?;
                    let document = index.document(id).expect("score has a document");
                    output.push(
                        id,
                        document.metadata.get("external_id").map(String::as_str),
                        score,
                    )
                };
                if complete {
                    for (id, score) in scored.scores() {
                        append(id, *score, &mut output)?;
                    }
                } else {
                    let _ranked_memory = request.working_account.reserve(
                        scored
                            .scores()
                            .len()
                            .checked_mul(std::mem::size_of::<(&String, &f64)>())
                            .ok_or_else(|| {
                                hawdb::HawDBError::Execution("rank size overflow".into())
                            })?,
                    )?;
                    let mut ranked = scored.scores().iter().collect::<Vec<_>>();
                    ranked.sort_unstable_by(|a, b| b.1.total_cmp(a.1).then(a.0.cmp(b.0)));
                    for (id, score) in ranked {
                        append(id, *score, &mut output)?;
                    }
                }
            }
            Self::Disk(reader) => {
                let scored = if complete {
                    reader.complete_text_seed_scores_with_metadata_filters(
                        request.query_text,
                        request.metadata_filters,
                        request.resources.result.max_rows,
                        request.working_account,
                        task,
                    )
                } else {
                    reader.text_seed_scores_with_metadata_filters(
                        request.query_text,
                        request.metadata_filters,
                        request.resources.result.max_rows,
                        request.working_account,
                        task,
                    )
                }?;
                output = output.with_source_report(TextSeedSourceReport {
                    generation: Some(scored.report.generation),
                    source_commit_epoch: scored.report.source_graph_commit_epoch,
                    corpus_documents: Some(scored.report.corpus_document_count),
                    matching_documents: Some(scored.report.matching_document_count),
                    postings_visited: Some(scored.report.postings_visited),
                    bytes_read: Some(scored.report.bytes_read),
                    document_bytes_read: Some(scored.report.document_bytes_read),
                });
                if complete {
                    for (id, score) in scored.scores() {
                        request.resources.checkpoint()?;
                        output.push(id, scored.external_id(id), *score)?;
                    }
                } else {
                    let _ranked_memory = request.working_account.reserve(
                        scored
                            .scores()
                            .len()
                            .checked_mul(std::mem::size_of::<(&String, &f64)>())
                            .ok_or_else(|| {
                                hawdb::HawDBError::Execution("rank size overflow".into())
                            })?,
                    )?;
                    let mut ranked = scored.scores().iter().collect::<Vec<_>>();
                    ranked.sort_unstable_by(|a, b| b.1.total_cmp(a.1).then(a.0.cmp(b.0)));
                    for (id, score) in ranked {
                        request.resources.checkpoint()?;
                        output.push(id, scored.external_id(id), *score)?;
                    }
                }
            }
        }
        request.resources.checkpoint()?;
        if complete {
            output.finish_complete()
        } else {
            Ok(output)
        }
    }
}

impl ExternalReadOperator for Source {
    fn execute_vector_seed(
        &mut self,
        _request: hawdb::executor::VectorSeedExecutionRequest<'_>,
    ) -> hawdb::Result<hawdb::executor::VectorSeedExecutionOutput> {
        Err(hawdb::HawDBError::Execution(
            "native evidence benchmark requires lexical seeds".into(),
        ))
    }

    fn execute_complete_text_seed(
        &mut self,
        request: TextSeedExecutionRequest<'_>,
    ) -> hawdb::Result<TextSeedExecutionOutput> {
        self.execute(request, true)
    }
    fn execute_text_seed(
        &mut self,
        request: TextSeedExecutionRequest<'_>,
    ) -> hawdb::Result<TextSeedExecutionOutput> {
        self.execute(request, false)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let backend = arguments.next().ok_or("missing backend")?;
    let root = PathBuf::from(arguments.next().ok_or("missing new artifact path")?);
    let dimensions = arguments
        .map(|value| value.parse::<usize>())
        .collect::<Result<Vec<_>, _>>()?;
    let [n, group_count, hot_percent, match_stride, eligibility_stride, m, k, id_width, repeats] =
        dimensions.as_slice()
    else {
        return Err("expected backend new_path rows groups hot_percent match_stride eligibility_stride M K ID_width repeats".into());
    };
    let (n, group_count, hot_percent, match_stride, eligibility_stride, m, k, id_width, repeats) = (
        *n,
        *group_count,
        *hot_percent,
        *match_stride,
        *eligibility_stride,
        *m,
        *k,
        *id_width,
        *repeats,
    );
    if !matches!(backend.as_str(), "raw" | "resident" | "disk")
        || n == 0
        || n > 10_000
        || group_count == 0
        || group_count > n
        || hot_percent > 100
        || match_stride == 0
        || eligibility_stride == 0
        || m == 0
        || k == 0
        || !(8..=4096).contains(&id_width)
        || repeats == 0
        || repeats > 100
        || root.exists()
    {
        return Err("invalid qualification dimensions or artifact path already exists".into());
    }
    let mut config = DatabaseConfig::default();
    config.execution_memory.query_memory_bytes = NonZeroUsize::new(512 * 1024 * 1024).unwrap();
    config.execution_memory.blocking_operator_bytes = NonZeroUsize::new(128 * 1024 * 1024).unwrap();
    config.execution_memory.graph_expansion_budget = Some(GraphExpansionBudget {
        candidate_limit: 64_000,
        payload_byte_limit: 128 * 1024 * 1024,
    });
    let mut database = Database::new_with_config(config);
    let mut fixture = Vec::new();
    let mut documents = Vec::new();
    for i in 0..n {
        let group = if i % 100 < hot_percent {
            0
        } else {
            i % group_count
        };
        let id = format!("document-{i:0width$}", width = id_width);
        let matches = i % match_stride == 0;
        fixture.push(GraphMutation::CreateConnectedNodes(ConnectedNodesCreate {
            source_label: "Memory".into(),
            source_properties: BTreeMap::from([
                ("id".into(), Value::String(id.clone())),
                ("group_id".into(), Value::Int(i64::try_from(group)?)),
            ]),
            rel_type: "VISIBLE_IN".into(),
            rel_properties: BTreeMap::new(),
            target_label: "Scope".into(),
            target_properties: BTreeMap::from([(
                "visible".into(),
                Value::Bool((i / match_stride) % eligibility_stride == 0),
            )]),
        }));
        documents.push(SearchProjectionRow {
            kind: SearchProjectionKind::Memory,
            external_id: id,
            title: if matches {
                "graph ".repeat(1 + i % 5)
            } else {
                "unrelated".into()
            },
            body: if matches {
                "graph".into()
            } else {
                "unrelated".into()
            },
            embedding: None,
            source_id: None,
            metadata: BTreeMap::new(),
        });
    }
    database.commit_kernel_write_batch(KernelWriteBatch {
        graph: fixture,
        ..Default::default()
    })?;
    database.query("CREATE INDEX ON :Memory(id)")?;
    let mut source = if backend == "disk" {
        let half = n / 2;
        let mut writer = SearchOutOfCoreGenerationWriter::create(&root, Default::default())?;
        for row in &documents[..half] {
            writer.push(row.clone().into_document())?;
        }
        writer.push(
            SearchProjectionRow {
                kind: SearchProjectionKind::Memory,
                external_id: "zombie".into(),
                title: "graph".into(),
                body: "graph".into(),
                embedding: None,
                source_id: None,
                metadata: BTreeMap::new(),
            }
            .into_document(),
        )?;
        writer.finish()?;
        let mut reader = SearchOutOfCoreReader::open(&root)?;
        let mut upserts = documents[half..].to_vec();
        if half > 0 {
            upserts.push(documents[0].clone());
        }
        upserts.sort_unstable_by(|a, b| a.external_id.cmp(&b.external_id));
        SearchOutOfCoreGenerationWriter::prepare_delta(
            &reader,
            SearchProjectionDelta {
                upserts,
                deletes: vec!["memory:zombie".into()],
                ..Default::default()
            },
            Default::default(),
        )?
        .finish()?;
        let refresh = reader.refresh()?;
        assert!(refresh.generation_changed);
        assert_eq!(
            refresh.reused_content_segments + refresh.opened_content_segments,
            2
        );
        assert!(refresh.validated_retractions >= 1);
        assert_eq!(reader.document_count(), n);
        Source::Disk(Box::new(reader))
    } else {
        let mut index = if backend == "resident" {
            SearchIndex::open(&root)?
        } else {
            SearchIndex::in_memory()
        };
        for row in documents {
            index.upsert_projection_row(row)?;
        }
        if backend == "resident" {
            index.checkpoint()?;
            drop(index);
            index = SearchIndex::open(&root)?;
        }
        Source::Resident(Box::new(index))
    };
    let parameters = BTreeMap::from([
        ("text".into(), Value::String("graph".into())),
        ("cap".into(), Value::Int(i64::try_from(n)?)),
        ("m".into(), Value::Int(i64::try_from(m)?)),
        ("k".into(), Value::Int(i64::try_from(k)?)),
    ]);
    let reference_query = format!("{PREFIX}{RAW}");
    let grouped_query = format!("{PREFIX}{GROUP}");
    let window_query =
        grouped_query.replace("complete := true, maxCandidates := $cap", "topK := $cap");
    let expected_candidates = (n - 1) / match_stride + 1;
    let expected_eligible = (expected_candidates - 1) / eligibility_stride + 1;
    let mut snapshot = database.begin_read_transaction()?;
    let mut samples = Vec::new();
    for run in 0..=repeats {
        let mut rows = [Vec::new(), Vec::new(), Vec::new()];
        let mut profiles = Vec::new();
        let mut elapsed = [0u128; 3];
        for path in if run % 2 == 0 { [0, 1, 2] } else { [2, 1, 0] } {
            let started = Instant::now();
            let mut groups = Groups::new();
            let mut reference_rows = 0usize;
            let query = [&reference_query, &grouped_query, &window_query][path];
            let report = snapshot.query_request_streaming_with_external(
                QueryRequest::new(query).with_params(&parameters),
                &mut source,
                |row| {
                    if path == 0 {
                        reference_rows += 1;
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
                    } else {
                        rows[path].push(row);
                    }
                    Ok(())
                },
            )?;
            if path == 0 {
                rows[0] = select_groups(groups, m, k);
            }
            elapsed[path] = started.elapsed().as_nanos();
            if path == 0 {
                assert_eq!(reference_rows, expected_eligible);
            }
            let profile = report.execution_profile;
            if path != 0 {
                assert_eq!(profile.grouped_evidence_reports.len(), 1);
                assert_eq!(
                    profile.grouped_evidence_reports[0].input_rows,
                    expected_eligible
                );
            }
            assert_eq!(
                profile.pipeline_memory_report.query_memory_completion_bytes,
                0
            );
            assert_eq!(profile.text_seed_execution_reports.len(), 1);
            let text = &profile.text_seed_execution_reports[0];
            assert_eq!(text.status(), if path == 2 { "window" } else { "complete" });
            assert_eq!(text.candidate_count, expected_candidates);
            let origin = text.source.unwrap();
            assert_eq!(origin.corpus_documents, Some(n));
            assert_eq!(origin.matching_documents, Some(expected_candidates));
            assert!(!profile.graph_expansion_reports.is_empty());
            assert!(profile
                .graph_expansion_reports
                .iter()
                .all(|report| !report.truncated()));
            profiles.push(json!({"path": path, "candidate_count": text.candidate_count,
                "retained_result_bytes": text.retained_result_bytes, "source_generation": origin.generation,
                "postings_visited": origin.postings_visited, "logical_bytes_read": origin.bytes_read,
                "document_bytes_read": origin.document_bytes_read,
                "query_memory_peak_bytes": profile.pipeline_memory_report.query_memory_peak_bytes,
                "process_peak_resident_bytes": profile.pipeline_memory_report.peak_resident_bytes,
                "grouped": profile.grouped_evidence_reports.iter().map(|report| json!({"input_rows": report.input_rows,
                    "groups_considered": report.groups_considered, "retained_evidence": report.retained_evidence,
                    "peak_state_bytes": report.peak_state_bytes, "spilled_bytes": report.spilled_bytes})).collect::<Vec<_>>() }));
        }
        assert_eq!(rows[1], rows[0], "complete grouping run {run}");
        assert_eq!(rows[2], rows[0], "known full-window control run {run}");
        if run != 0 {
            samples.push(json!({"run": run, "reference_ns": elapsed[0], "complete_grouped_ns": elapsed[1], "full_window_grouped_ns": elapsed[2], "profiles": profiles}));
        }
    }
    println!(
        "{}",
        json!({"status": "equal", "backend": backend, "rows": n, "groups": group_count, "hot_percent": hot_percent,
        "match_stride": match_stride, "eligibility_stride": eligibility_stride, "M": m, "K": k,
        "id_width": id_width, "artifact_path": root, "samples": samples,
        "scope": "Public native lexical producers through an admitted embedded external-provider adapter and canonical one-hop eligibility query. Full-reference host map/sort and known-corpus full-window control. No cold device I/O, whole-query heap, or general window completeness claim."})
    );
    Ok(())
}

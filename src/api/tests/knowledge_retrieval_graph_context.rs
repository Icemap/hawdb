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

#[test]
fn knowledge_context_edge_visitor_releases_admission_on_completion_stop_and_error() {
    use crate::api::knowledge_graph_read;
    use hawdb_core::HawDBError;
    use hawdb_executor::store::ScanControl;
    use hawdb_executor::{QueryMemoryClass, QueryMemoryLedger};

    let mut db = Database::new();
    let mut access = db.runtime.get_mut().unwrap();
    let runtime = &mut *access;
    let mut nodes = Vec::new();
    for _ in 0..3 {
        nodes.push(
            runtime
                .store
                .create_node(&mut runtime.catalog, "Entity", BTreeMap::new())
                .unwrap(),
        );
    }
    for (source, target) in [(nodes[0], nodes[1]), (nodes[2], nodes[0])] {
        runtime
            .store
            .create_relationship(
                &mut runtime.catalog,
                source,
                target,
                "MENTIONS",
                BTreeMap::from([("payload".into(), Value::String("x".repeat(4096)))]),
            )
            .unwrap();
    }

    for control in [Some(ScanControl::Continue), Some(ScanControl::Stop), None] {
        let budget = NonZeroUsize::new(64 * 1024).unwrap();
        let ledger = QueryMemoryLedger::new(budget);
        let account = ledger.account(QueryMemoryClass::ExternalRead, "context-edges", budget);
        let mut visited = Vec::new();
        let result = knowledge_graph_read::visit_expansion_edges(
            &runtime.store,
            nodes[0],
            budget.get(),
            &account,
            |edge| {
                assert!(ledger.snapshot().used_bytes >= 4096);
                visited.push(edge.next_node);
                control.ok_or_else(|| HawDBError::Execution("context visitor failure".into()))
            },
        );
        match control {
            Some(control) => assert_eq!(result.unwrap(), control),
            None => assert!(matches!(
                result,
                Err(HawDBError::Execution(message)) if message == "context visitor failure"
            )),
        }
        if control == Some(ScanControl::Continue) {
            assert_eq!(visited, [nodes[1], nodes[2]]);
        } else {
            assert_eq!(visited, [nodes[1]]);
        }
        assert_eq!(ledger.snapshot().used_bytes, 0);
    }

    let budget = NonZeroUsize::new(1024).unwrap();
    let ledger = QueryMemoryLedger::new(budget);
    let account = ledger.account(QueryMemoryClass::ExternalRead, "context-edges", budget);
    let mut visited = 0;
    let result = knowledge_graph_read::visit_expansion_edges(
        &runtime.store,
        nodes[0],
        budget.get(),
        &account,
        |_| {
            visited += 1;
            Ok(ScanControl::Continue)
        },
    );
    assert!(matches!(result, Err(HawDBError::Execution(_))));
    assert_eq!(visited, 0);
    assert_eq!(ledger.snapshot().used_bytes, 0);
}

#[test]
fn knowledge_retrieval_context_filters_before_output_limit() {
    for incoming in [false, true] {
        let mut db = Database::new();
        {
            let mut access = db.runtime.get_mut().unwrap();
            let runtime = &mut *access;
            let root = runtime
                .store
                .create_node(
                    &mut runtime.catalog,
                    "Memory",
                    BTreeMap::from([
                        ("id".into(), Value::String("root".into())),
                        ("content".into(), Value::String("scope needle".into())),
                        ("space_id".into(), Value::String("space-a".into())),
                    ]),
                )
                .unwrap();
            for index in 0..6 {
                let allowed = index >= 4;
                let neighbor = runtime
                    .store
                    .create_node(
                        &mut runtime.catalog,
                        "Entity",
                        BTreeMap::from([
                            ("id".into(), Value::String(format!("neighbor-{index}"))),
                            (
                                "space_id".into(),
                                Value::String(if allowed { "space-a" } else { "space-b" }.into()),
                            ),
                        ]),
                    )
                    .unwrap();
                let (source, target) = if incoming {
                    (neighbor, root)
                } else {
                    (root, neighbor)
                };
                runtime
                    .store
                    .create_relationship(
                        &mut runtime.catalog,
                        source,
                        target,
                        "MENTIONS",
                        BTreeMap::new(),
                    )
                    .unwrap();
            }
        }
        let mut search_index = SearchIndex::in_memory();
        db.rebuild_search_projection(&mut search_index, SearchRebuildOptions::default())
            .unwrap();
        for limit in [1, 2, 3] {
            let output = db
                .try_retrieve_knowledge(
                    &search_index,
                    &KnowledgeRetrievalRequest {
                        query_text: "scope needle".into(),
                        query_embedding: None,
                        mode: SearchMode::Text,
                        limit: 1,
                        offset: 0,
                        rank_window: None,
                        search_fusion_weights: SearchFusionWeights::default(),
                        metadata_filters: BTreeMap::from([("space_id".into(), "space-a".into())]),
                        candidate_limit: None,
                        candidate_scoring: KnowledgeCandidateScoringPolicy::Max,
                        graph_seed_limit: 0,
                        graph_context_limit: limit,
                        graph_context_max_hops: 1,
                    },
                )
                .unwrap();
            let neighbors = output
                .graph_context_paths
                .iter()
                .map(|path| {
                    if incoming {
                        path.source_external_id.as_deref().unwrap()
                    } else {
                        path.target_external_id.as_deref().unwrap()
                    }
                })
                .collect::<Vec<_>>();
            assert_eq!(neighbors, ["neighbor-4", "neighbor-5"][..limit.min(2)]);
            assert_eq!(output.diagnostics.graph_context_truncated, limit < 2);
        }
    }
}

#[test]
fn knowledge_retrieval_context_deduplicates_before_output_limit() {
    let mut db = Database::new();
    db.query("CREATE (:Memory {id: 'root', content: 'dedup needle'})")
        .unwrap();
    db.query("CREATE (:Entity {id: 'middle'})").unwrap();
    db.query("CREATE (:Entity {id: 'last'})").unwrap();
    {
        let mut access = db.runtime.get_mut().unwrap();
        let runtime = &mut *access;
        for source in [NodeId(0), NodeId(0), NodeId(2)] {
            runtime
                .store
                .create_relationship(
                    &mut runtime.catalog,
                    source,
                    NodeId(1),
                    "MENTIONS",
                    BTreeMap::new(),
                )
                .unwrap();
        }
    }
    let mut search_index = SearchIndex::in_memory();
    db.rebuild_search_projection(&mut search_index, SearchRebuildOptions::default())
        .unwrap();
    let output = db
        .try_retrieve_knowledge(
            &search_index,
            &KnowledgeRetrievalRequest {
                query_text: "dedup needle".into(),
                query_embedding: None,
                mode: SearchMode::Text,
                limit: 1,
                offset: 0,
                rank_window: None,
                search_fusion_weights: SearchFusionWeights::default(),
                metadata_filters: BTreeMap::new(),
                candidate_limit: None,
                candidate_scoring: KnowledgeCandidateScoringPolicy::Max,
                graph_seed_limit: 0,
                graph_context_limit: 3,
                graph_context_max_hops: 2,
            },
        )
        .unwrap();
    assert_eq!(output.graph_context_paths.len(), 3);
    assert_eq!(
        output
            .graph_context_paths
            .iter()
            .map(|path| path.hop)
            .collect::<Vec<_>>(),
        [1, 1, 2]
    );
    assert_eq!(
        output.graph_context_paths[2].source_external_id.as_deref(),
        Some("last")
    );
    assert!(!output.diagnostics.graph_context_truncated);
}

#[test]
fn knowledge_retrieval_expands_graph_context_by_ordered_adjacency() {
    let mut db = Database::new();
    db.query("CREATE (:Memory {id: 'root', title: 'Ordered retrieval context'})")
        .unwrap();
    let lower_neighbor_id = {
        let mut branch_runtime_access = db.runtime.get_mut().unwrap();
        let branch_runtime = &mut *branch_runtime_access;
        branch_runtime.store.create_node(
            &mut branch_runtime.catalog,
            "Entity",
            BTreeMap::from([
                (
                    "id".to_string(),
                    Value::String("lower-neighbor".to_string()),
                ),
                (
                    "name".to_string(),
                    Value::String("Lower neighbor".to_string()),
                ),
            ]),
        )
    }
    .unwrap();
    let higher_neighbor_id = {
        let mut branch_runtime_access = db.runtime.get_mut().unwrap();
        let branch_runtime = &mut *branch_runtime_access;
        branch_runtime.store.create_node(
            &mut branch_runtime.catalog,
            "Entity",
            BTreeMap::from([
                (
                    "id".to_string(),
                    Value::String("higher-neighbor".to_string()),
                ),
                (
                    "name".to_string(),
                    Value::String("Higher neighbor".to_string()),
                ),
            ]),
        )
    }
    .unwrap();
    {
        let mut branch_runtime_access = db.runtime.get_mut().unwrap();
        let branch_runtime = &mut *branch_runtime_access;
        branch_runtime.store.create_relationship(
            &mut branch_runtime.catalog,
            NodeId(0),
            higher_neighbor_id,
            "MENTIONS",
            BTreeMap::new(),
        )
    }
    .unwrap();
    {
        let mut branch_runtime_access = db.runtime.get_mut().unwrap();
        let branch_runtime = &mut *branch_runtime_access;
        branch_runtime.store.create_relationship(
            &mut branch_runtime.catalog,
            NodeId(0),
            lower_neighbor_id,
            "RELATES_TO",
            BTreeMap::new(),
        )
    }
    .unwrap();

    let mut search_index = SearchIndex::in_memory();
    db.rebuild_search_projection(&mut search_index, SearchRebuildOptions::default())
        .unwrap();

    let output = db
        .retrieve_knowledge(
            &search_index,
            &KnowledgeRetrievalRequest {
                query_text: "ordered retrieval".to_string(),
                query_embedding: None,
                mode: SearchMode::Text,
                limit: 1,
                offset: 0,
                rank_window: None,
                search_fusion_weights: SearchFusionWeights::default(),
                metadata_filters: BTreeMap::new(),
                candidate_limit: None,
                candidate_scoring: KnowledgeCandidateScoringPolicy::Max,
                graph_seed_limit: 0,
                graph_context_limit: 1,
                graph_context_max_hops: 1,
            },
        )
        .unwrap();

    assert_eq!(output.graph_context_paths.len(), 1);
    assert_eq!(
        output
            .diagnostics
            .graph_context_input_candidate_set
            .cardinality,
        1
    );
    assert_eq!(
        output.diagnostics.graph_context_candidate_set.cardinality,
        1
    );
    assert_eq!(
        output.diagnostics.graph_context_candidate_set.id_space,
        "canonical_graph_relationship_id"
    );
    assert_eq!(
        output.graph_context_paths[0].target_external_id.as_deref(),
        Some("lower-neighbor")
    );
    assert_eq!(
        output.graph_context_paths[0].relationship_type.as_str(),
        "RELATES_TO"
    );
    assert!(output
        .fanout_reasons
        .iter()
        .any(|reason| reason.contains("graph_context_limit 1")));
}

#[test]
fn knowledge_retrieval_reports_dense_graph_context_without_truncation() {
    let mut db = Database::new();
    db.query("CREATE (:Memory {id: 'root', title: 'Dense retrieval root'})")
        .unwrap();
    for index in 0..DENSE_ADJACENCY_DEGREE_THRESHOLD {
        let target = {
            let mut branch_runtime_access = db.runtime.get_mut().unwrap();
            let branch_runtime = &mut *branch_runtime_access;
            branch_runtime.store.create_node(
                &mut branch_runtime.catalog,
                "Entity",
                BTreeMap::from([
                    ("id".to_string(), Value::String(format!("entity-{index}"))),
                    ("name".to_string(), Value::String(format!("Entity {index}"))),
                ]),
            )
        }
        .unwrap();
        {
            let mut branch_runtime_access = db.runtime.get_mut().unwrap();
            let branch_runtime = &mut *branch_runtime_access;
            branch_runtime.store.create_relationship(
                &mut branch_runtime.catalog,
                NodeId(0),
                target,
                "MENTIONS",
                BTreeMap::new(),
            )
        }
        .unwrap();
    }

    let mut search_index = SearchIndex::in_memory();
    db.rebuild_search_projection(&mut search_index, SearchRebuildOptions::default())
        .unwrap();

    let output = db
        .retrieve_knowledge(
            &search_index,
            &KnowledgeRetrievalRequest {
                query_text: "dense retrieval".to_string(),
                query_embedding: None,
                mode: SearchMode::Text,
                limit: 1,
                offset: 0,
                rank_window: None,
                search_fusion_weights: SearchFusionWeights::default(),
                metadata_filters: BTreeMap::new(),
                candidate_limit: None,
                candidate_scoring: KnowledgeCandidateScoringPolicy::Max,
                graph_seed_limit: 0,
                graph_context_limit: DENSE_ADJACENCY_DEGREE_THRESHOLD,
                graph_context_max_hops: 1,
            },
        )
        .unwrap();

    assert_eq!(
        output.graph_context_paths.len(),
        DENSE_ADJACENCY_DEGREE_THRESHOLD
    );
    assert_eq!(output.diagnostics.fanout_reason_count, 1);
    assert!(!output.diagnostics.graph_context_truncated);
    assert!(output
        .diagnostics
        .graph_context_truncation_reasons
        .is_empty());
    assert!(output.fanout_reasons[0]
        .contains("graph_context dense_adjacency MENTIONS outgoing node 0 degree"));
}

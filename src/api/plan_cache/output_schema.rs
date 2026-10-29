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

//! Capture the public projection before optimizations can erase empty inputs.

use super::*;
use hawdb_plan_cypher::{GraphAlgorithmIdentity, GraphAlgorithmKind, SetNodePropertiesReturnMode};

pub(super) fn logical_output_columns(mut plan: &LogicalPlan) -> Result<Option<Vec<String>>> {
    let columns = loop {
        match plan {
            LogicalPlan::Project { items, .. } => {
                break items
                    .iter()
                    .map(|item| item.name.clone())
                    .collect::<Vec<_>>();
            }
            LogicalPlan::Aggregate {
                group_keys, items, ..
            } => {
                break group_keys
                    .iter()
                    .map(|item| item.name.clone())
                    .chain(items.iter().map(|item| item.name.clone()))
                    .collect();
            }
            LogicalPlan::ShortestPath { returns, .. } => {
                break returns.iter().map(|item| item.name.clone()).collect();
            }
            LogicalPlan::OptionalRelationshipCountSum { output, .. } => {
                break vec![output.clone()];
            }
            LogicalPlan::SetNodePropertiesReturn { returns, .. } => {
                break match returns {
                    SetNodePropertiesReturnMode::Project(items) => {
                        items.iter().map(|item| item.name.clone()).collect()
                    }
                    SetNodePropertiesReturnMode::Count { name } => vec![name.clone()],
                };
            }
            LogicalPlan::GraphAlgorithm {
                algorithm,
                score_column,
                return_node_identity,
                ..
            } => {
                break graph_algorithm_columns(
                    *algorithm,
                    GraphAlgorithmIdentity::legacy(*return_node_identity),
                    score_column,
                )
            }
            LogicalPlan::GraphAlgorithmOutput(output) => {
                break graph_algorithm_columns(
                    output.algorithm,
                    output.identity,
                    &output.score_column,
                )
            }
            LogicalPlan::VectorSeed {
                output_external_id: false,
                ..
            }
            | LogicalPlan::TextSeed {
                output_external_id: false,
                ..
            } => {
                break vec!["id".into(), "score".into()];
            }
            LogicalPlan::ThreadRepairStats { .. } => {
                break [
                    "t.id",
                    "t.thread_id",
                    "CASE WHEN t.space_id IS NULL OR t.space_id = '' THEN 'default' ELSE t.space_id END",
                    "COALESCE(t.message_count, 0)",
                    "identity_refs",
                    "legacy_messages",
                    "COUNT(m)",
                ].into_iter().map(str::to_owned).collect();
            }
            LogicalPlan::Filter { input, .. }
            | LogicalPlan::Distinct { input }
            | LogicalPlan::Sort { input, .. }
            | LogicalPlan::Limit { input, .. } => plan = input,
            _ => return Ok(None),
        }
    };
    for (index, name) in columns.iter().enumerate() {
        if columns[..index].contains(name) {
            return Err(HawDBError::Semantic(format!(
                "query result schema contains duplicate column {name}"
            )));
        }
    }
    Ok(Some(columns))
}

fn graph_algorithm_columns(
    algorithm: GraphAlgorithmKind,
    identity: GraphAlgorithmIdentity,
    score_column: &str,
) -> Vec<String> {
    let mut columns = vec!["node".into()];
    columns.extend(identity.columns().map(str::to_owned));
    if algorithm == GraphAlgorithmKind::Louvain {
        columns.push("level".into());
    }
    columns.push(score_column.to_owned());
    columns
}

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

//! Test-only lifecycle mutation request and response models.

//! The root facade re-exports these only from its cfg(test) API surface.

use hawdb_core::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeLabelLifecycleUpdate {
    pub label_id: String,
    pub name: Option<String>,
    pub canonical_name: Option<String>,
    pub metadata: Option<Value>,
    pub updated_at: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeLabelLifecycleBatchRequest {
    pub updates: Vec<KnowledgeLabelLifecycleUpdate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeLabelLifecycleBatchRow {
    pub label_id: String,
    pub node_id: Option<u64>,
    pub matched: bool,
    pub updated: bool,
    pub duplicate: bool,
    pub non_writable: bool,
    pub updated_property_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeLabelLifecycleBatchOutput {
    pub graph_commit_epoch_before: u64,
    pub graph_commit_epoch_after: u64,
    pub rows: Vec<KnowledgeLabelLifecycleBatchRow>,
    pub matched_count: usize,
    pub missing_count: usize,
    pub duplicate_count: usize,
    pub non_writable_count: usize,
    pub updated_count: usize,
    pub updated_property_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeMemoryLabelDeleteRequest {
    pub memory_id: String,
    pub label_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeMemoryLabelDeleteOutput {
    pub graph_commit_epoch_before: u64,
    pub graph_commit_epoch_after: u64,
    pub memory_id: String,
    pub label_id: Option<String>,
    pub memory_node_id: Option<u64>,
    pub label_node_id: Option<u64>,
    pub found_memory: bool,
    pub found_label: bool,
    pub non_writable: bool,
    pub matched_relationship_count: usize,
    pub deleted_relationship_count: usize,
    pub deleted_relationship_ids: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeLabelMemoryTransferRequest {
    pub source_label_id: String,
    pub target_label_id: String,
    pub created_at: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeLabelMemoryTransferRow {
    pub memory_id: Option<String>,
    pub memory_node_id: u64,
    pub relationship_id: Option<u64>,
    pub created: bool,
    pub already_exists: bool,
    pub non_writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeLabelMemoryTransferOutput {
    pub graph_commit_epoch_before: u64,
    pub graph_commit_epoch_after: u64,
    pub source_label_id: String,
    pub target_label_id: String,
    pub source_label_node_id: Option<u64>,
    pub target_label_node_id: Option<u64>,
    pub found_source_label: bool,
    pub found_target_label: bool,
    pub rows: Vec<KnowledgeLabelMemoryTransferRow>,
    pub matched_memory_count: usize,
    pub created_count: usize,
    pub already_exists_count: usize,
    pub non_writable_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeMemoryLabelTransferRequest {
    pub older_memory_id: String,
    pub newer_memory_id: String,
    pub space_id: String,
    pub created_at: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeMemoryLabelTransferRow {
    pub label_id: Option<String>,
    pub label_node_id: u64,
    pub relationship_id: Option<u64>,
    pub created: bool,
    pub already_exists: bool,
    pub non_writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeMemoryLabelTransferOutput {
    pub graph_commit_epoch_before: u64,
    pub graph_commit_epoch_after: u64,
    pub older_memory_id: String,
    pub newer_memory_id: String,
    pub space_id: String,
    pub older_memory_node_id: Option<u64>,
    pub newer_memory_node_id: Option<u64>,
    pub found_older_memory: bool,
    pub found_newer_memory: bool,
    pub older_space_matches: bool,
    pub newer_space_matches: bool,
    pub rows: Vec<KnowledgeMemoryLabelTransferRow>,
    pub matched_label_count: usize,
    pub created_count: usize,
    pub already_exists_count: usize,
    pub non_writable_count: usize,
    pub duplicate_source_edge_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeEntityLabelListRequest {
    pub entity_label: String,
    pub external_ids: Vec<String>,
    pub limit_per_entity: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeEntityLabelRow {
    pub label_id: Option<String>,
    pub node_id: u64,
    pub name: Option<String>,
    pub canonical_name: Option<String>,
    pub color: Option<Value>,
    pub description: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeEntityLabelGroup {
    pub external_id: String,
    pub node_id: Option<u64>,
    pub found: bool,
    pub labels: Vec<KnowledgeEntityLabelRow>,
    pub returned_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeEntityLabelListOutput {
    pub graph_commit_epoch: u64,
    pub groups: Vec<KnowledgeEntityLabelGroup>,
    pub found_entity_count: usize,
    pub missing_entity_count: usize,
    pub label_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeEntityLabelProjectedListRequest {
    pub list: KnowledgeEntityLabelListRequest,
    pub label_property_names: Vec<String>,
    pub relationship_property_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeEntityLabelProjectedRow {
    pub label_id: Option<String>,
    pub label_node_id: u64,
    pub relationship_id: u64,
    pub label_properties: BTreeMap<String, Value>,
    pub relationship_properties: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeEntityLabelProjectedGroup {
    pub external_id: String,
    pub node_id: Option<u64>,
    pub found: bool,
    pub labels: Vec<KnowledgeEntityLabelProjectedRow>,
    pub returned_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeEntityLabelProjectedListOutput {
    pub graph_commit_epoch: u64,
    pub groups: Vec<KnowledgeEntityLabelProjectedGroup>,
    pub found_entity_count: usize,
    pub missing_entity_count: usize,
    pub label_count: usize,
}

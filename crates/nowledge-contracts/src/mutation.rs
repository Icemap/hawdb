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

//! Test-only graph mutation request and response models.

//! The root facade re-exports these only from its cfg(test) API surface.

use crate::test_support::graph_read::KnowledgeEntityRequest;
use hawdb_core::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeEntityCreateRequest {
    pub label: String,
    pub external_id: String,
    pub properties: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeEntityCreateOutput {
    pub graph_commit_epoch_before: u64,
    pub graph_commit_epoch_after: u64,
    pub node_id: Option<u64>,
    pub created: bool,
    pub already_exists: bool,
    pub created_node_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeEntityCreateBatchRequest {
    pub creates: Vec<KnowledgeEntityCreateRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeEntityCreateBatchRow {
    pub label: String,
    pub external_id: String,
    pub node_id: Option<u64>,
    pub created: bool,
    pub already_exists: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeEntityCreateBatchOutput {
    pub graph_commit_epoch_before: u64,
    pub graph_commit_epoch_after: u64,
    pub rows: Vec<KnowledgeEntityCreateBatchRow>,
    pub created_count: usize,
    pub already_exists_count: usize,
    pub created_node_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeEntityUpsertRequest {
    pub label: String,
    pub external_id: String,
    pub create_properties: BTreeMap<String, Value>,
    pub update_properties: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeEntityUpsertOutput {
    pub graph_commit_epoch_before: u64,
    pub graph_commit_epoch_after: u64,
    pub node_id: Option<u64>,
    pub created: bool,
    pub updated: bool,
    pub already_exists: bool,
    pub non_writable: bool,
    pub created_node_count: usize,
    pub updated_property_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeEntityUpsertBatchRequest {
    pub upserts: Vec<KnowledgeEntityUpsertRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeEntityUpsertBatchRow {
    pub label: String,
    pub external_id: String,
    pub node_id: Option<u64>,
    pub created: bool,
    pub updated: bool,
    pub already_exists: bool,
    pub non_writable: bool,
    pub updated_property_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeEntityUpsertBatchOutput {
    pub graph_commit_epoch_before: u64,
    pub graph_commit_epoch_after: u64,
    pub rows: Vec<KnowledgeEntityUpsertBatchRow>,
    pub created_count: usize,
    pub updated_count: usize,
    pub already_exists_count: usize,
    pub non_writable_count: usize,
    pub created_node_count: usize,
    pub updated_property_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgePropertyBatchRequest {
    pub entities: Vec<KnowledgeEntityRequest>,
    pub property_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeScopedPropertyBatchRequest {
    pub projection: KnowledgePropertyBatchRequest,
    pub metadata_filters: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgePropertyRow {
    pub entity: KnowledgeEntityRequest,
    pub node_id: Option<u64>,
    pub filtered_out: bool,
    pub properties: BTreeMap<String, Option<Value>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgePropertyBatchOutput {
    pub graph_commit_epoch: u64,
    pub rows: Vec<KnowledgePropertyRow>,
    pub found_count: usize,
    pub missing_count: usize,
    pub filtered_out_count: usize,
    pub property_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSourceMemoryCountAdjustment {
    pub source_id: String,
    pub delta: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSourceMemoryCountBatchRequest {
    pub adjustments: Vec<KnowledgeSourceMemoryCountAdjustment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSourceMemoryCountBatchRow {
    pub source_id: String,
    pub node_id: Option<u64>,
    pub matched: bool,
    pub adjusted: bool,
    pub non_writable: bool,
    pub invalid_current_count: bool,
    pub old_count: Option<i64>,
    pub new_count: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSourceMemoryCountBatchOutput {
    pub graph_commit_epoch_before: u64,
    pub graph_commit_epoch_after: u64,
    pub rows: Vec<KnowledgeSourceMemoryCountBatchRow>,
    pub matched_count: usize,
    pub missing_count: usize,
    pub non_writable_count: usize,
    pub invalid_current_count_count: usize,
    pub adjusted_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSourceParsedMetadataUpdate {
    pub source_id: String,
    pub parsed_path: Option<String>,
    pub file_path: Option<String>,
    pub original_name: Option<String>,
    pub mime_type: Option<String>,
    pub source_url: Option<String>,
    pub summary: String,
    pub sha256: String,
    pub size_bytes: i64,
    pub updated_at: Value,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSourceParsedMetadataBatchRequest {
    pub updates: Vec<KnowledgeSourceParsedMetadataUpdate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSourceParsedMetadataBatchRow {
    pub source_id: String,
    pub node_id: Option<u64>,
    pub matched: bool,
    pub updated: bool,
    pub duplicate: bool,
    pub non_writable: bool,
    pub updated_property_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSourceParsedMetadataBatchOutput {
    pub graph_commit_epoch_before: u64,
    pub graph_commit_epoch_after: u64,
    pub rows: Vec<KnowledgeSourceParsedMetadataBatchRow>,
    pub matched_count: usize,
    pub missing_count: usize,
    pub duplicate_count: usize,
    pub non_writable_count: usize,
    pub updated_count: usize,
    pub updated_property_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSourceParsedCreate {
    pub source_id: String,
    pub source_type: String,
    pub original_name: String,
    pub mime_type: String,
    pub file_path: String,
    pub parsed_path: String,
    pub source_url: String,
    pub sha256: String,
    pub size_bytes: i64,
    pub version: i64,
    pub space_id: String,
    pub section_tree: String,
    pub summary: String,
    pub created_at: Value,
    pub updated_at: Value,
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSourceParsedCreateBatchRequest {
    pub creates: Vec<KnowledgeSourceParsedCreate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSourceParsedCreateBatchRow {
    pub source_id: String,
    pub node_id: Option<u64>,
    pub created: bool,
    pub already_exists: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSourceParsedCreateBatchOutput {
    pub graph_commit_epoch_before: u64,
    pub graph_commit_epoch_after: u64,
    pub rows: Vec<KnowledgeSourceParsedCreateBatchRow>,
    pub created_count: usize,
    pub already_exists_count: usize,
    pub created_node_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSourceRevisionCreate {
    pub newer_source_id: String,
    pub older_source_id: String,
    pub created_at: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSourceRevisionCreateBatchRequest {
    pub creates: Vec<KnowledgeSourceRevisionCreate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSourceRevisionCreateBatchRow {
    pub newer_source_id: String,
    pub older_source_id: String,
    pub newer_node_id: Option<u64>,
    pub older_node_id: Option<u64>,
    pub matched: bool,
    pub non_writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSourceRevisionCreateBatchOutput {
    pub graph_commit_epoch_before: u64,
    pub graph_commit_epoch_after: u64,
    pub rows: Vec<KnowledgeSourceRevisionCreateBatchRow>,
    pub matched_count: usize,
    pub missing_endpoint_count: usize,
    pub non_writable_count: usize,
    pub created_relationship_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSourceLabelAssignment {
    pub source_id: String,
    pub label_id: String,
    pub assigned_by: String,
    pub created_at: Value,
    pub properties: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSourceLabelAssignmentBatchRequest {
    pub assignments: Vec<KnowledgeSourceLabelAssignment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSourceLabelAssignmentRow {
    pub source_id: String,
    pub label_id: String,
    pub source_node_id: Option<u64>,
    pub label_node_id: Option<u64>,
    pub relationship_id: Option<u64>,
    pub matched: bool,
    pub created: bool,
    pub already_exists: bool,
    pub non_writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSourceLabelAssignmentBatchOutput {
    pub graph_commit_epoch_before: u64,
    pub graph_commit_epoch_after: u64,
    pub rows: Vec<KnowledgeSourceLabelAssignmentRow>,
    pub matched_count: usize,
    pub missing_endpoint_count: usize,
    pub non_writable_count: usize,
    pub created_count: usize,
    pub already_exists_count: usize,
    pub created_relationship_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSourceLabelDelete {
    pub source_id: String,
    pub label_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSourceLabelDeleteBatchRequest {
    pub deletes: Vec<KnowledgeSourceLabelDelete>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSourceLabelDeleteRow {
    pub source_id: String,
    pub label_id: String,
    pub source_node_id: Option<u64>,
    pub label_node_id: Option<u64>,
    pub matched: bool,
    pub non_writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSourceLabelDeleteBatchOutput {
    pub graph_commit_epoch_before: u64,
    pub graph_commit_epoch_after: u64,
    pub rows: Vec<KnowledgeSourceLabelDeleteRow>,
    pub matched_count: usize,
    pub missing_endpoint_count: usize,
    pub non_writable_count: usize,
    pub deleted_relationship_count: usize,
}

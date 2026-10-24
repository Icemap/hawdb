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

//! Test-only Nowledge analytics protocol models.

//! The root facade re-exports these only from its cfg(test) API surface.

use super::graph_read::KnowledgeEntityRequest;
use hawdb_core::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeRelationshipCreateRequest {
    pub source: KnowledgeEntityRequest,
    pub target: KnowledgeEntityRequest,
    pub relationship_type: String,
    pub properties: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeScopedRelationshipCreateRequest {
    pub create: KnowledgeRelationshipCreateRequest,
    pub source_metadata_filters: BTreeMap<String, String>,
    pub target_metadata_filters: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeRelationshipCreateOutput {
    pub graph_commit_epoch_before: u64,
    pub graph_commit_epoch_after: u64,
    pub source_node_id: Option<u64>,
    pub target_node_id: Option<u64>,
    pub matched: bool,
    pub source_filtered_out: bool,
    pub target_filtered_out: bool,
    pub created_relationship_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeRelationshipCreateBatchRequest {
    pub creates: Vec<KnowledgeRelationshipCreateRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeScopedRelationshipCreateBatchRequest {
    pub creates: Vec<KnowledgeRelationshipCreateRequest>,
    pub source_metadata_filters: BTreeMap<String, String>,
    pub target_metadata_filters: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeRelationshipCreateBatchRow {
    pub source: KnowledgeEntityRequest,
    pub target: KnowledgeEntityRequest,
    pub relationship_type: String,
    pub source_node_id: Option<u64>,
    pub target_node_id: Option<u64>,
    pub matched: bool,
    pub source_filtered_out: bool,
    pub target_filtered_out: bool,
    pub non_writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeRelationshipCreateBatchOutput {
    pub graph_commit_epoch_before: u64,
    pub graph_commit_epoch_after: u64,
    pub rows: Vec<KnowledgeRelationshipCreateBatchRow>,
    pub matched_count: usize,
    pub missing_endpoint_count: usize,
    pub source_filtered_out_count: usize,
    pub target_filtered_out_count: usize,
    pub non_writable_count: usize,
    pub created_relationship_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeRelationshipUpsertRequest {
    pub source: KnowledgeEntityRequest,
    pub target: KnowledgeEntityRequest,
    pub relationship_type: String,
    pub create_properties: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeScopedRelationshipUpsertRequest {
    pub upsert: KnowledgeRelationshipUpsertRequest,
    pub source_metadata_filters: BTreeMap<String, String>,
    pub target_metadata_filters: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeRelationshipUpsertOutput {
    pub graph_commit_epoch_before: u64,
    pub graph_commit_epoch_after: u64,
    pub source_node_id: Option<u64>,
    pub target_node_id: Option<u64>,
    pub relationship_id: Option<u64>,
    pub matched: bool,
    pub created: bool,
    pub already_exists: bool,
    pub source_filtered_out: bool,
    pub target_filtered_out: bool,
    pub non_writable: bool,
    pub created_relationship_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeRelationshipUpsertBatchRequest {
    pub upserts: Vec<KnowledgeRelationshipUpsertRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeScopedRelationshipUpsertBatchRequest {
    pub upserts: Vec<KnowledgeRelationshipUpsertRequest>,
    pub source_metadata_filters: BTreeMap<String, String>,
    pub target_metadata_filters: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeRelationshipUpsertBatchRow {
    pub source: KnowledgeEntityRequest,
    pub target: KnowledgeEntityRequest,
    pub relationship_type: String,
    pub source_node_id: Option<u64>,
    pub target_node_id: Option<u64>,
    pub relationship_id: Option<u64>,
    pub matched: bool,
    pub created: bool,
    pub already_exists: bool,
    pub source_filtered_out: bool,
    pub target_filtered_out: bool,
    pub non_writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeRelationshipUpsertBatchOutput {
    pub graph_commit_epoch_before: u64,
    pub graph_commit_epoch_after: u64,
    pub rows: Vec<KnowledgeRelationshipUpsertBatchRow>,
    pub matched_count: usize,
    pub created_count: usize,
    pub already_exists_count: usize,
    pub missing_endpoint_count: usize,
    pub source_filtered_out_count: usize,
    pub target_filtered_out_count: usize,
    pub non_writable_count: usize,
    pub created_relationship_count: usize,
}

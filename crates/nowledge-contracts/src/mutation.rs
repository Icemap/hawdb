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

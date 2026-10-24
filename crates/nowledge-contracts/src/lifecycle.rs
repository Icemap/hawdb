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

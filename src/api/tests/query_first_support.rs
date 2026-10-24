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

use crate::api::*;
use crate::error::Result;

pub(super) trait QueryFirstTestExt {
    fn query_neighbors_via_cypher(
        &self,
        request: &KnowledgeNeighborsRequest,
    ) -> Result<KnowledgeNeighborsOutput>;

    fn query_scoped_neighbors_via_cypher(
        &self,
        request: &KnowledgeScopedNeighborsRequest,
    ) -> Result<KnowledgeNeighborsOutput>;

    fn query_relationships_via_cypher(
        &self,
        request: &KnowledgeRelationshipsRequest,
    ) -> Result<KnowledgeRelationshipsOutput>;

    fn query_scoped_relationships_via_cypher(
        &self,
        request: &KnowledgeScopedRelationshipsRequest,
    ) -> Result<KnowledgeRelationshipsOutput>;

    fn query_paths_via_cypher(&self, request: &KnowledgePathRequest)
        -> Result<KnowledgePathOutput>;

    fn query_scoped_paths_via_cypher(
        &self,
        request: &KnowledgeScopedPathRequest,
    ) -> Result<KnowledgePathOutput>;

    fn query_subgraph_via_cypher(
        &self,
        request: &KnowledgeSubgraphRequest,
    ) -> Result<KnowledgeSubgraphOutput>;

    fn query_scoped_subgraph_via_cypher(
        &self,
        request: &KnowledgeScopedSubgraphRequest,
    ) -> Result<KnowledgeSubgraphOutput>;
}

impl QueryFirstTestExt for Database {
    fn query_neighbors_via_cypher(
        &self,
        request: &KnowledgeNeighborsRequest,
    ) -> Result<KnowledgeNeighborsOutput> {
        super::super::knowledge_neighbors_via_query_runtime(self, request)
    }

    fn query_scoped_neighbors_via_cypher(
        &self,
        request: &KnowledgeScopedNeighborsRequest,
    ) -> Result<KnowledgeNeighborsOutput> {
        super::super::knowledge_scoped_neighbors_via_query_runtime(self, request)
    }

    fn query_relationships_via_cypher(
        &self,
        request: &KnowledgeRelationshipsRequest,
    ) -> Result<KnowledgeRelationshipsOutput> {
        super::super::knowledge_relationships_via_query_runtime(self, request)
    }

    fn query_scoped_relationships_via_cypher(
        &self,
        request: &KnowledgeScopedRelationshipsRequest,
    ) -> Result<KnowledgeRelationshipsOutput> {
        super::super::knowledge_scoped_relationships_via_query_runtime(self, request)
    }

    fn query_paths_via_cypher(
        &self,
        request: &KnowledgePathRequest,
    ) -> Result<KnowledgePathOutput> {
        super::super::knowledge_paths_via_query_runtime(self, request)
    }

    fn query_scoped_paths_via_cypher(
        &self,
        request: &KnowledgeScopedPathRequest,
    ) -> Result<KnowledgePathOutput> {
        super::super::knowledge_scoped_paths_via_query_runtime(self, request)
    }

    fn query_subgraph_via_cypher(
        &self,
        request: &KnowledgeSubgraphRequest,
    ) -> Result<KnowledgeSubgraphOutput> {
        super::super::knowledge_subgraph_via_query_runtime(self, request)
    }

    fn query_scoped_subgraph_via_cypher(
        &self,
        request: &KnowledgeScopedSubgraphRequest,
    ) -> Result<KnowledgeSubgraphOutput> {
        super::super::knowledge_scoped_subgraph_via_query_runtime(self, request)
    }
}

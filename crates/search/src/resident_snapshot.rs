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

//! Read-only resident generations. Writers share pages and native artifacts;
//! readers cannot reach checkpoint, marker or Consumer mutation entrypoints.

use super::*;
use hawdb_executor::QueryMemoryAccount;

#[derive(Debug, Clone)]
pub struct SearchIndexReadSnapshot {
    index: Arc<SearchIndex>,
    freshness: Arc<SearchProjectionFreshness>,
}

impl SearchIndex {
    /// Capture a read generation without copying document bodies or descriptor
    /// entries. The caller must pair this capture with its graph publication.
    pub fn read_snapshot(&self) -> SearchIndexReadSnapshot {
        let (lexical_projection, lexical_delta) = match self.lexical_snapshot() {
            Some((reader, delta)) => (Some(reader), delta),
            None => (None, Arc::default()),
        };
        let index = SearchIndex {
            documents: self.documents.clone(),
            vector_index_descriptor: self.vector_index_descriptor.clone(),
            path: self.path.clone(),
            _project_files: self._project_files.clone(),
            embedding_dimension: self.embedding_dimension,
            embedding_manifest: self.embedding_manifest.clone(),
            import_source_graph_commit_epoch: self.import_source_graph_commit_epoch,
            source_graph_commit_epoch: self.source_graph_commit_epoch,
            analyzer_lexicon: Arc::clone(&self.analyzer_lexicon),
            lexical_projection: Mutex::new(lexical_projection),
            lexical_delta: Mutex::new(lexical_delta),
            lexical_config: self.lexical_config,
            #[cfg(feature = "vector-search")]
            rabitq_projection: Mutex::new(self.rabitq_projection()),
            #[cfg(feature = "vector-search")]
            rabitq_build_options: self.rabitq_build_options,
            segment_descriptor: self.segment_descriptor.clone(),
            segment_payload_reader: self.segment_payload_reader.clone(),
            range_read_config: self.range_read_config,
            runtime_capabilities: self.runtime_capabilities,
            telemetry: self.telemetry.clone(),
            ..Self::default()
        };
        SearchIndexReadSnapshot {
            index: Arc::new(index),
            freshness: Arc::new(self.projection_freshness()),
        }
    }
}

impl SearchIndexReadSnapshot {
    pub fn document(&self, id: &str) -> Option<&SearchDocument> {
        self.index.document(id)
    }

    pub fn document_count(&self) -> usize {
        self.index.document_count()
    }

    pub fn projection_freshness(&self) -> &SearchProjectionFreshness {
        &self.freshness
    }

    /// Conservative capacity bound for document pages and document buffers.
    /// Projection indexes and query scratch are separate owners, not included.
    pub fn document_retained_capacity_bytes(&self) -> usize {
        self.index.documents.retained_capacity_bytes()
    }

    pub fn text_seed_scores_with_context(
        &self,
        query_text: &str,
        metadata_filters: &BTreeMap<String, String>,
        max_candidates: usize,
        memory: &QueryMemoryAccount,
        task: &RuntimeTaskContext,
    ) -> Result<SearchResidentTextSeedOutput> {
        self.index.text_seed_scores_with_context(
            query_text,
            metadata_filters,
            max_candidates,
            memory,
            task,
        )
    }

    pub fn complete_text_seed_scores_with_context(
        &self,
        query_text: &str,
        metadata_filters: &BTreeMap<String, String>,
        max_candidates: usize,
        memory: &QueryMemoryAccount,
        task: &RuntimeTaskContext,
    ) -> Result<SearchResidentTextSeedOutput> {
        self.index.complete_text_seed_scores_with_context(
            query_text,
            metadata_filters,
            max_candidates,
            memory,
            task,
        )
    }

    pub fn try_search_with_options_adaptive_vector_projection_context(
        &self,
        query_text: &str,
        query_embedding: Option<&[f32]>,
        mode: SearchMode,
        options: SearchQueryOptions,
        adaptive_options: AdaptiveVectorSearchOptions,
        vector_execution_options: VectorSearchExecutionOptions<'_>,
    ) -> Result<SearchResultSet> {
        self.index
            .try_search_with_options_adaptive_vector_projection_context(
                query_text,
                query_embedding,
                mode,
                options,
                adaptive_options,
                vector_execution_options,
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(id: &str, content: &str, space: &str) -> SearchDocument {
        SearchDocument {
            id: id.into(),
            title: id.into(),
            content: content.into(),
            embedding: None,
            metadata: BTreeMap::from([("space_id".into(), space.into())]),
        }
    }

    #[test]
    fn captured_documents_and_configuration_survive_writer_progress() {
        let mut index = SearchIndex::in_memory();
        index.upsert(document("a", "original", "old")).unwrap();
        index.source_graph_commit_epoch = Some(1);
        let snapshot = index.read_snapshot();
        assert!(std::ptr::eq(
            index.document("a").unwrap(),
            snapshot.document("a").unwrap()
        ));
        let index = std::thread::spawn(move || {
            for epoch in 2..=32 {
                index
                    .upsert(document("a", &format!("revision {epoch}"), "new"))
                    .unwrap();
                index.source_graph_commit_epoch = Some(epoch);
            }
            index.delete("a");
            index.set_analyzer_lexicon(SearchAnalyzerLexicon::empty());
            index
        })
        .join()
        .unwrap();
        assert_eq!(snapshot.document("a").unwrap().content, "original");
        assert_eq!(
            snapshot.projection_freshness().source_graph_commit_epoch,
            Some(1)
        );
        assert_eq!(
            index.projection_freshness().source_graph_commit_epoch,
            Some(32)
        );
        assert_eq!(index.document_count(), 0);
        assert_eq!(snapshot.document_count(), 1);
        assert!(
            snapshot.document_retained_capacity_bytes()
                >= snapshot.document("a").unwrap().content.capacity()
        );
        assert!(!Arc::ptr_eq(
            &snapshot.index.analyzer_lexicon,
            &index.analyzer_lexicon
        ));
    }

    #[cfg(feature = "full-text-search")]
    #[test]
    fn captured_payload_reads_old_generation_after_checkpoint_replacement() {
        let path = std::env::temp_dir().join(format!(
            "hawdb-resident-snapshot-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let project =
            hawdb_storage::file_descriptors::ProjectFileDescriptors::acquire_with_capacity_limit(
                &path, 64, true,
            )
            .unwrap();
        let mut index = SearchIndex::open(&path).unwrap();
        for (id, space) in [("a", "other"), ("b", "other"), ("c", "selected")] {
            index
                .upsert(document(id, "original payload", space))
                .unwrap();
        }
        index.source_graph_commit_epoch = Some(1);
        index.checkpoint().unwrap();
        drop(index);
        let mut index = SearchIndex::open(&path).unwrap();
        let old = index.read_snapshot();
        let options = SearchQueryOptions {
            limit: 10,
            offset: 0,
            rank_window: None,
            fusion_weights: SearchFusionWeights::default(),
            metadata_filters: BTreeMap::from([("space_id".into(), "selected".into())]),
            policy_epoch: None,
        };
        let search = |snapshot: &SearchIndexReadSnapshot, text: &str| {
            snapshot
                .try_search_with_options_adaptive_vector_projection_context(
                    text,
                    None,
                    SearchMode::Text,
                    options.clone(),
                    AdaptiveVectorSearchOptions::new(CompressedVectorSearchMode::Disabled),
                    VectorSearchExecutionOptions::default(),
                )
                .unwrap()
        };
        index
            .upsert(document("c", "replacement payload", "selected"))
            .unwrap();
        index.source_graph_commit_epoch = Some(2);
        index.checkpoint().unwrap();
        drop(index);
        let current = SearchIndex::open(&path).unwrap();
        let next = current.read_snapshot();
        let old_result = search(&old, "original");
        assert_eq!(
            old_result
                .hits
                .iter()
                .map(|hit| hit.id.as_str())
                .collect::<Vec<_>>(),
            ["c"]
        );
        assert_eq!(
            old_result
                .candidate_set
                .metadata_predicate_pushdown
                .physical_range_read_count,
            1
        );
        assert!(
            old_result
                .candidate_set
                .metadata_predicate_pushdown
                .physical_bytes_read
                > 0
        );
        assert!(search(&next, "original").hits.is_empty());
        assert_eq!(search(&next, "replacement").hits.len(), 1);
        assert_eq!(old.document("c").unwrap().content, "original payload");
        assert_eq!(next.document("c").unwrap().content, "replacement payload");
        assert_eq!(
            old.projection_freshness().durable_source_graph_commit_epoch,
            Some(1)
        );
        assert_eq!(
            next.projection_freshness()
                .durable_source_graph_commit_epoch,
            Some(2)
        );
        drop(current);
        drop(next);
        assert!(project.metrics().open > 0);
        drop(old);
        assert_eq!(project.metrics().open, 0);
        assert_eq!(project.metrics().reserved, 0);
        drop(project);
        std::fs::remove_dir_all(path).unwrap();
    }
}

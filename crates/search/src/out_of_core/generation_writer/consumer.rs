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

//! Governed artifacts for an invisible registered Consumer producer. The caller
//! retains its original publication lease and owns the sole native selection.

use super::*;
use crate::SearchIndex;

impl SearchOutOfCoreGenerationWriter {
    pub(crate) fn write_consumer(
        index: &SearchIndex,
        publication: &super::super::SearchProjectionPublishLease,
        task: RuntimeTaskContext,
        generation_bytes: NonZeroU64,
        permit: std::sync::Arc<hawdb_qos::RuntimePermit>,
    ) -> Result<crate::snapshot_writer::SearchSnapshotWriteReport> {
        let root = index.path.as_deref().ok_or_else(|| {
            HawDBError::StorageIntegrity("Consumer producer has no private root".into())
        })?;
        let mut memory = BuildMemory::new(&task)?;
        memory.host_admission = Some(permit);
        let scope = io_phase::Scope::acquire(&task, &memory)?;
        let task = scope.task();
        if !publication.matches_source_directory(root)? {
            return Err(HawDBError::StorageIntegrity(
                "Consumer producer changed native root".into(),
            ));
        }
        // This entrypoint accepts only a fresh private producer. It never opens
        // another selector or retries a possibly selected public generation.
        if discovery::active_identity(root, &memory, &task)?.is_some() {
            return Err(HawDBError::StorageIntegrity(
                "Consumer producer is not empty".into(),
            ));
        }
        let snapshot = crate::snapshot_writer::write_search_snapshot_with_context(
            &root.join(crate::SEARCH_SNAPSHOT_FILE),
            crate::snapshot_writer::SearchSnapshotMetadata {
                source_graph_commit_epoch: index.source_graph_commit_epoch,
                import_source_graph_commit_epoch: index.import_source_graph_commit_epoch,
                embedding_manifest: index.embedding_manifest.as_ref(),
                embedding_dimension: index.embedding_dimension,
                consumer_binding: index.consumer_binding.as_ref(),
                vector_index_descriptor: index.vector_index_descriptor.as_ref(),
            },
            index.documents.values(),
            &memory,
            &task,
        )?;
        let definition = index.vector_index_descriptor.as_ref().ok_or_else(|| {
            HawDBError::StorageIntegrity("Consumer vector producer has no definition".into())
        })?;
        let hawdb_core::VectorIndexBackend::RaBitQ {
            bit_width,
            segment_rows,
            transform_seed,
        } = definition.backend();
        #[cfg(not(feature = "vector-search"))]
        let _ = bit_width;
        let options_memory = memory
            .retained
            .reserve(definition.retained_bytes() + 4096)?;
        let options = SearchOutOfCoreGenerationBuildOptions {
            max_documents: NonZeroUsize::new(index.documents.len().max(1)).unwrap(),
            max_content_documents: NonZeroUsize::new(index.documents.len().max(1)).unwrap(),
            max_generation_bytes: generation_bytes,
            max_spool_bytes: generation_bytes,
            lexical_max_spill_bytes: generation_bytes,
            source_graph_commit_epoch: index.source_graph_commit_epoch,
            import_source_graph_commit_epoch: index.import_source_graph_commit_epoch,
            embedding_manifest: index.embedding_manifest.clone(),
            vector_index_descriptor: Some(definition.clone()),
            rabitq_segment_rows: NonZeroUsize::new(segment_rows as usize).unwrap(),
            rabitq_transform_seed: transform_seed,
            #[cfg(feature = "vector-search")]
            rabitq_bit_width: match bit_width {
                1 => hawdb_vector_projection::RaBitQBitWidth::One,
                4 => hawdb_vector_projection::RaBitQBitWidth::Four,
                _ => unreachable!("validated vector definition"),
            },
            ..Default::default()
        };
        let options = context_memory::Options::new(options, &memory, &task)?;
        drop(options_memory);
        let mut writer = Self::create_with_memory(root, options, task, memory)?;
        for document in index.documents.values() {
            checkpoint(&writer.task_context)?;
            let lease = writer
                .memory
                .input
                .reserve(crate::build_memory::document_bytes(document)?)?;
            writer.push_inner(crate::build_memory::AdmittedDocument::from_admitted_parts(
                document.clone(),
                lease,
            ))?;
        }
        writer.seal_input_spool()?;
        let source = writer.input_source()?;
        // Schema-bound producers contain only opaque ASCII identities and vectors.
        // Refuse unexpected lexical work rather than creating an unowned workspace.
        if writer.needs_chinese_analyzer {
            return Err(HawDBError::StorageIntegrity(
                "vector producer contains lexical content".into(),
            ));
        }
        let generation = 1;
        let artifacts = writer.build_artifacts(&source, generation)?;
        drop(source);
        writer.held_spool.take();
        writer.resident_snapshot = Some(super::super::ResidentSnapshot {
            object: super::super::ArtifactObject::from_reference(snapshot.encoded_object)?,
            vector_document_count: snapshot.vector_document_count,
        });
        writer.publish_artifacts(generation, true, artifacts, publication)?;
        Ok(snapshot)
    }
}

#[cfg(all(test, feature = "vector-search"))]
mod tests;

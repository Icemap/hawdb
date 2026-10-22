// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

//! Schema-binding metadata sealed with ordinary search projection artifacts.

use crate::{HawDBError, Result, SearchIndex};
use hawdb_core::vector_index::MAX_VECTOR_INDEX_DESCRIPTOR_BYTES;
use hawdb_core::VectorIndexDescriptor;

mod query;
pub use query::{
    VectorIndexHit, VectorIndexQuery, VectorIndexQueryReport, VectorIndexRetainedOutput,
};

pub(crate) fn encode(descriptor: &VectorIndexDescriptor) -> String {
    use std::fmt::Write;
    let bytes = descriptor.encode();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("writing a String cannot fail");
    }
    output
}

pub(crate) fn decode(encoded: &str) -> Result<VectorIndexDescriptor> {
    if encoded.len() > MAX_VECTOR_INDEX_DESCRIPTOR_BYTES * 2 || !encoded.len().is_multiple_of(2) {
        return Err(invalid("invalid encoded descriptor length"));
    }
    let mut bytes = Vec::with_capacity(encoded.len() / 2);
    for pair in encoded.as_bytes().chunks_exact(2) {
        let digit = |byte: u8| match byte {
            b'0'..=b'9' => Ok(byte - b'0'),
            b'a'..=b'f' => Ok(byte - b'a' + 10),
            _ => Err(invalid("descriptor must use canonical lowercase hex")),
        };
        bytes.push(digit(pair[0])? * 16 + digit(pair[1])?);
    }
    VectorIndexDescriptor::decode(&bytes)
        .map_err(|error| invalid(&format!("invalid descriptor: {error}")))
}

fn invalid(message: &str) -> HawDBError {
    HawDBError::StorageIntegrity(format!("vector index binding: {message}"))
}

#[cfg(feature = "vector-search")]
pub(crate) fn validate_projection(
    descriptor: &VectorIndexDescriptor,
    artifact: &hawdb_vector_projection::ProjectionManifest,
) -> Result<()> {
    let hawdb_core::VectorIndexBackend::RaBitQ {
        bit_width,
        segment_rows,
        transform_seed,
    } = descriptor.backend();
    if artifact.dimension != descriptor.dimension().get() as usize
        || artifact.bit_width != bit_width
        || artifact.requested_segment_rows != segment_rows as usize
        || artifact.transform_seed != transform_seed
    {
        return Err(invalid(
            "RaBitQ artifact disagrees with its bound definition",
        ));
    }
    Ok(())
}

impl SearchIndex {
    pub fn vector_index_descriptor(&self) -> Option<&VectorIndexDescriptor> {
        self.vector_index_descriptor.as_ref()
    }

    /// Binds an empty private builder. The facade owns schema resolution and
    /// source capture; a serving Consumer cannot be redefined in place.
    #[doc(hidden)]
    pub fn bind_vector_index(&mut self, descriptor: VectorIndexDescriptor) -> Result<()> {
        self.runtime_capabilities
            .require(hawdb_core::RuntimeCapability::VectorSearch)?;
        self.require_unregistered_publication()?;
        if !self.documents.is_empty() || self.vector_index_descriptor.is_some() {
            return Err(HawDBError::Semantic(
                "a vector index definition requires an empty unbound builder".into(),
            ));
        }
        let dimension = descriptor.dimension().get() as usize;
        if self
            .embedding_dimension
            .is_some_and(|existing| existing != dimension)
        {
            return Err(HawDBError::Semantic(
                "vector index dimension disagrees with the projection".into(),
            ));
        }
        self.restore_vector_index(descriptor);
        self.embedding_dimension = Some(dimension);
        Ok(())
    }

    pub(crate) fn restore_vector_index(&mut self, descriptor: VectorIndexDescriptor) {
        #[cfg(feature = "vector-search")]
        {
            let hawdb_core::VectorIndexBackend::RaBitQ {
                bit_width,
                segment_rows,
                transform_seed,
            } = descriptor.backend();
            self.rabitq_build_options.bit_width = match bit_width {
                1 => hawdb_vector_projection::RaBitQBitWidth::One,
                4 => hawdb_vector_projection::RaBitQBitWidth::Four,
                _ => unreachable!("validated vector index backend"),
            };
            self.rabitq_build_options.segment_rows = segment_rows as usize;
            self.rabitq_build_options.transform_seed = transform_seed;
        }
        self.vector_index_descriptor = Some(descriptor);
    }
}

#[cfg(all(test, feature = "vector-search"))]
mod tests {
    use super::*;
    use crate::{SearchDocument, SearchOutOfCoreReader};
    use hawdb_core::{VectorDimension, VectorDistance, VectorIndexBackend, VectorIndexTarget};
    use std::collections::BTreeMap;

    fn descriptor() -> VectorIndexDescriptor {
        VectorIndexDescriptor::new(
            "memories.embedding".into(),
            VectorIndexTarget::Relational {
                table: "memories".into(),
                column: "embedding".into(),
                column_ordinal: 1,
            },
            VectorDimension::new(2).unwrap(),
            VectorDistance::Cosine,
            VectorIndexBackend::RaBitQ {
                bit_width: 1,
                segment_rows: 64,
                transform_seed: 42,
            },
            [7; 32],
        )
        .unwrap()
    }

    fn document(id: &str, embedding: Option<Vec<f32>>) -> SearchDocument {
        SearchDocument {
            id: id.into(),
            title: String::new(),
            content: String::new(),
            embedding,
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn vector_index_definition_is_sealed_in_snapshot_and_generation() {
        let path = std::env::temp_dir().join(format!(
            "hawdb-vector-index-binding-{}",
            hawdb_core::generate_uuidv7().unwrap()
        ));
        let binding = descriptor();
        {
            let mut index = SearchIndex::open(&path).unwrap();
            index.bind_vector_index(binding.clone()).unwrap();
            index.upsert(document("a", Some(vec![1.0, 0.0]))).unwrap();
            index.upsert(document("b", Some(vec![-1.0, 0.0]))).unwrap();
            index.upsert(document("null", None)).unwrap();
            index.checkpoint().unwrap();
        }
        let index = SearchIndex::open(&path).unwrap();
        assert_eq!(index.vector_index_descriptor(), Some(&binding));
        assert_eq!(index.rabitq_build_options.bit_width.bits(), 1);
        assert_eq!(index.rabitq_build_options.segment_rows, 64);
        assert_eq!(index.rabitq_build_options.transform_seed, 42);
        let reader = SearchOutOfCoreReader::open(&path).unwrap();
        assert_eq!(reader.vector_index_descriptor().unwrap(), Some(binding));
        drop(reader);
        drop(index);
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn vector_index_invalid_vectors_do_not_mutate_documents() {
        let mut index = SearchIndex::in_memory();
        index.bind_vector_index(descriptor()).unwrap();
        for embedding in [vec![0.0, 0.0], vec![1.0], vec![f32::NAN, 1.0]] {
            assert!(index.upsert(document("invalid", Some(embedding))).is_err());
            assert!(index.documents.is_empty());
        }
        assert!(index.bind_vector_index(descriptor()).is_err());
        index
            .upsert(document("opposite", Some(vec![-1.0, 0.0])))
            .unwrap();
        assert_eq!(index.documents.len(), 1);
    }

    #[test]
    fn vector_index_metadata_rejects_noncanonical_and_oversized_inputs() {
        let encoded = encode(&descriptor());
        assert_eq!(decode(&encoded).unwrap(), descriptor());
        for value in [
            "x".into(),
            "gg".into(),
            "00".repeat(MAX_VECTOR_INDEX_DESCRIPTOR_BYTES + 1),
            encoded.to_uppercase(),
        ] {
            assert!(decode(&value).is_err());
        }
    }

    #[test]
    fn vector_index_reopen_refuses_artifact_build_parameter_drift() {
        let path = std::env::temp_dir().join(format!(
            "hawdb-vector-drift-{}",
            hawdb_core::generate_uuidv7().unwrap()
        ));
        let mut index = SearchIndex::open(&path).unwrap();
        index.bind_vector_index(descriptor()).unwrap();
        index.upsert(document("a", Some(vec![1.0, 0.0]))).unwrap();
        index.rabitq_build_options.transform_seed += 1;
        index.checkpoint().unwrap();
        assert!(matches!(
            SearchOutOfCoreReader::open(&path),
            Err(HawDBError::StorageIntegrity(_))
        ));
        drop(index);
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn vector_index_delta_inherits_nondefault_backend_and_definition() {
        let path = std::env::temp_dir().join(format!(
            "hawdb-vector-delta-{}",
            hawdb_core::generate_uuidv7().unwrap()
        ));
        let binding = descriptor();
        let mut index = SearchIndex::open(&path).unwrap();
        index.bind_vector_index(binding.clone()).unwrap();
        index.upsert(document("a", Some(vec![1.0, 0.0]))).unwrap();
        index.checkpoint().unwrap();
        drop(index);
        let previous = SearchOutOfCoreReader::open(&path).unwrap();
        let (update, _, _) = crate::SearchOutOfCoreGenerationWriter::prepare_delta(
            &previous,
            crate::SearchProjectionDelta {
                upserts: vec![crate::SearchProjectionRow {
                    kind: crate::SearchProjectionKind::Memory,
                    external_id: "b".into(),
                    title: String::new(),
                    body: String::new(),
                    embedding: Some(vec![-1.0, 0.0]),
                    source_id: None,
                    metadata: BTreeMap::new(),
                }],
                source_graph_commit_epoch: Some(1),
                ..Default::default()
            },
            Default::default(),
        )
        .unwrap()
        .finish()
        .unwrap();
        assert_eq!(update.after_document_count, 2);
        let next = SearchOutOfCoreReader::open(&path).unwrap();
        assert_eq!(next.vector_index_descriptor().unwrap(), Some(binding));
        assert!(next.generation() > previous.generation());
        assert_eq!(previous.document_count(), 1);
        drop(next);
        drop(previous);
        std::fs::remove_dir_all(path).unwrap();
    }
}

pub(crate) fn validate_embedding(
    descriptor: Option<&VectorIndexDescriptor>,
    embedding: &[f32],
    checkpoint: impl FnMut() -> Result<()>,
) -> Result<()> {
    let Some(descriptor) = descriptor else {
        return Ok(());
    };
    if embedding.len() != descriptor.dimension().get() as usize {
        return Err(invalid(
            "embedding dimension differs from its schema binding",
        ));
    }
    descriptor
        .distance()
        .distance_slices_with_work(embedding, embedding, checkpoint)?;
    Ok(())
}

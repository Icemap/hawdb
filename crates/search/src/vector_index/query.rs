// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

//! Explicit approximate retrieval, with exact distance only inside its window.

use hawdb_core::VectorIndexDescriptor;
use hawdb_executor::QueryMemoryLease;
use hawdb_qos::RuntimeRetainedResult;
use std::num::NonZeroUsize;

/// A resolved definition and an explicit candidate window. The facade must
/// independently bind canonical hydration to this generation's source epoch.
#[derive(Debug, Clone, Copy)]
pub struct VectorIndexQuery<'a> {
    pub descriptor: &'a VectorIndexDescriptor,
    pub embedding: &'a [f32],
    pub candidate_limit: NonZeroUsize,
    pub top_k: NonZeroUsize,
}

#[derive(Debug, PartialEq)]
pub struct VectorIndexHit {
    pub id: String,
    pub distance: f64,
}

/// Candidate generation remains approximate even though raw distances are
/// exact. Local candidates across segments may exceed the retained window.
#[derive(Debug)]
pub struct VectorIndexQueryReport {
    pub index_name: String,
    pub schema_digest: [u8; 32],
    pub source_epoch: u64,
    /// Native Consumer publication identity; artifact-local counters may reset
    /// inside a new immutable directory and cannot identify a successor.
    pub generation: u64,
    pub backend: &'static str,
    pub candidate_limit: usize,
    pub top_k: usize,
    pub generated_candidate_count: usize,
    pub reranked_candidate_count: usize,
}

/// Payload and its admission survive the query, Consumer pin and read permit.
/// Borrowed access cannot detach result bytes from their reservation.
#[derive(Debug)]
pub struct VectorIndexRetainedOutput {
    pub(crate) hits: Vec<VectorIndexHit>,
    pub(crate) report: VectorIndexQueryReport,
    pub(crate) metrics: crate::SearchOutOfCoreMetrics,
    pub(crate) _memory: QueryMemoryLease,
    pub(crate) runtime: RuntimeRetainedResult,
}

impl VectorIndexRetainedOutput {
    pub fn hits(&self) -> &[VectorIndexHit] {
        &self.hits
    }

    pub fn report(&self) -> &VectorIndexQueryReport {
        &self.report
    }

    pub fn metrics(&self) -> &crate::SearchOutOfCoreMetrics {
        &self.metrics
    }

    pub fn retained_capacity_bytes(&self) -> u64 {
        self.runtime.capacity_bytes()
    }
}

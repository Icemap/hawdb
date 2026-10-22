// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

//! One admitted, complete eligible domain shared by relational and graph reads.

use crate::{QueryMemoryAccount, QueryMemoryLease};
use hawdb_core::{HawDBError, Result, RuntimeTaskContext, VectorIndexDescriptor, VectorRef};
use std::collections::BTreeMap;

/// Counts input rows separately from unique non-NULL vector identities. An
/// upstream LIMIT is applied before this boundary, including duplicate rows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VectorEligibilityReport {
    pub input_rows: usize,
    pub work_units: usize,
    pub max_work: usize,
    pub source_strategy: &'static str,
    pub eligible_vectors: usize,
    pub scored_vectors: usize,
    pub complete: bool,
}

#[derive(Debug)]
pub struct VectorEligibleDomain<'a> {
    definition: &'a VectorIndexDescriptor,
    identity: hawdb_core::VectorIndexReadIdentity,
    embedding: VectorRef<'a>,
    distances: BTreeMap<String, f64>,
    report: VectorEligibilityReport,
    task: RuntimeTaskContext,
    // Payloads are destroyed before their original allocation owner.
    memory: QueryMemoryLease,
}

impl<'a> VectorEligibleDomain<'a> {
    pub fn new(
        definition: &'a VectorIndexDescriptor,
        identity: hawdb_core::VectorIndexReadIdentity,
        embedding: VectorRef<'a>,
        max_work: usize,
        account: &QueryMemoryAccount,
        task: Option<&RuntimeTaskContext>,
    ) -> Result<Self> {
        if embedding.dimension() != definition.dimension() {
            return Err(HawDBError::Semantic(
                "vector query dimension differs from the bound index".into(),
            ));
        }
        // Validate even an empty eligible domain, without allocating coordinates.
        definition
            .distance()
            .distance_with_work(embedding, embedding, || {
                crate::pipeline::runtime_checkpoint(task)
            })?;
        let memory = account.reserve(std::mem::size_of::<Self>().saturating_add(256))?;
        Ok(Self {
            definition,
            identity,
            embedding,
            distances: BTreeMap::new(),
            report: VectorEligibilityReport {
                max_work,
                source_strategy: "bounded_canonical",
                ..Default::default()
            },
            task: task.cloned().unwrap_or_default().with_work_limit(max_work),
            memory,
        })
    }

    pub fn observe_input(&mut self) -> Result<()> {
        if self.report.complete {
            return Err(HawDBError::Execution(
                "vector eligible domain is already complete".into(),
            ));
        }
        self.task.charge_work(1)?;
        self.report.input_rows += 1;
        Ok(())
    }

    /// The caller supplies the identity and vector from the same canonical row.
    /// IDs and scores are retained; canonical payloads remain with their reader.
    pub fn insert(&mut self, identity: &str, vector: VectorRef<'_>) -> Result<()> {
        crate::pipeline::runtime_checkpoint(Some(&self.task))?;
        if self.report.complete {
            return Err(HawDBError::Execution(
                "vector eligible domain is already complete".into(),
            ));
        }
        if self.distances.contains_key(identity) {
            return Ok(());
        }
        let distance =
            self.definition
                .distance()
                .distance_with_work(self.embedding, vector, || {
                    crate::pipeline::runtime_checkpoint(Some(&self.task))
                })?;
        // Conservative B-tree node/split bookkeeping is admitted before insertion.
        self.memory.grow(identity.len().saturating_add(256))?;
        self.distances.insert(identity.to_owned(), distance);
        self.report.eligible_vectors += 1;
        self.report.scored_vectors += 1;
        Ok(())
    }

    pub fn finish(&mut self) -> Result<()> {
        crate::pipeline::runtime_checkpoint(Some(&self.task))?;
        self.task.check_work_limit()?;
        self.report.complete = true;
        Ok(())
    }

    pub fn validate(&self, definition: &VectorIndexDescriptor, embedding: &[f32]) -> Result<()> {
        if !self.report.complete
            || self.definition != definition
            || !self.embedding.coordinates().eq(embedding.iter().copied())
        {
            return Err(HawDBError::Execution(
                "vector eligible domain differs from the completed query binding".into(),
            ));
        }
        Ok(())
    }

    pub fn identity(&self) -> hawdb_core::VectorIndexReadIdentity {
        self.identity
    }

    pub fn task(&self) -> &RuntimeTaskContext {
        &self.task
    }

    pub fn report(&self) -> VectorEligibilityReport {
        VectorEligibilityReport {
            work_units: self.task.work_units(),
            ..self.report
        }
    }

    pub fn distances(&self) -> impl Iterator<Item = (&str, f64)> {
        self.distances
            .iter()
            .map(|(id, distance)| (id.as_str(), *distance))
    }
}

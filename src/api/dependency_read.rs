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

//! Bounded, plan-derived source reads for immutable projection generations.

use super::*;
use hawdb_storage::projection_generation::{
    ProjectionGenerationError, ProjectionGenerationPublishReport, SealedProjectionGeneration,
};
use hawdb_storage::store::{GraphReadDependencies, GraphReadDependencyError};
use hawdb_storage::version::VersionKey;
use std::fmt;

/// Limits for one preparation. Output limits apply to each statement; callers
/// must additionally bound cross-statement work and the artifact builder.
#[derive(Debug, Clone, Copy)]
pub struct PreparedReadLimits {
    pub dependency_entries: NonZeroUsize,
    pub dependency_bytes: NonZeroUsize,
    pub statement_rows: NonZeroUsize,
    pub statement_payload_bytes: NonZeroUsize,
}

/// A pinned query context with no untracked mutable read escape hatch.
#[derive(Debug)]
pub struct PreparedRead {
    read: DatabaseReadTransaction,
    dependencies: PreparedReadDependencies,
    failed: bool,
}

/// Source proof inputs retained until the publication decision. This is not
/// authorization to publish outside the database's exclusive ownership.
#[derive(Debug)]
pub struct PreparedReadDependencies {
    source: DatabaseInstanceIdentity,
    graph: GraphReadDependencies,
}

impl PreparedReadDependencies {
    pub fn source_epoch(&self) -> u64 {
        self.graph.source_epoch()
    }

    /// Reports the conservative fallback used for optimizer-folded empty plans.
    /// Qualified label domains instead admit commits outside those domains.
    pub fn requires_unchanged_database_epoch(&self) -> bool {
        self.graph.requires_unchanged_database_epoch()
    }
}

impl PreparedRead {
    pub fn source_epoch(&self) -> u64 {
        self.dependencies.source_epoch()
    }

    /// Executes parameterized Cypher with complete dependencies for qualified
    /// node-only plans. Unsupported operators fail closed before execution.
    /// No returned row set is treated as a complete membership footprint.
    pub fn query_with_params(
        &mut self,
        cypher_text: &str,
        parameters: &BTreeMap<String, Value>,
    ) -> Result<BoundedReadQueryOutput> {
        let result = self.query_inner(cypher_text, parameters);
        self.failed |= result.is_err();
        result
    }

    fn query_inner(
        &mut self,
        cypher_text: &str,
        parameters: &BTreeMap<String, Value>,
    ) -> Result<BoundedReadQueryOutput> {
        let mut prepared = query_runtime::parse_runtime_execution(cypher_text)?;
        if matches!(prepared.statement, cypher::Statement::Explain(_)) {
            return Err(HawDBError::Execution(
                "prepared source reads do not support EXPLAIN".into(),
            ));
        }
        let optimized = self.read.optimized_query_plan_with_access_control(
            cypher_text,
            &prepared.statement,
            parameters,
            None,
        )?;
        collect_node_dependencies(&optimized.physical_plan, &mut self.dependencies.graph)?;
        prepared.optimized = Some(optimized);
        self.read
            .query_with_params_bounded_profile_prepared_internal(
                cypher_text,
                prepared,
                parameters,
                self.read.config.max_read_result_rows,
                None,
                None,
            )
    }

    /// A failed or incomplete statement cannot be silently omitted from a
    /// source proof, even if the caller ignored its error and read again.
    pub fn finish(self) -> Result<PreparedReadDependencies> {
        if self.failed {
            return Err(HawDBError::Execution(
                "prepared source read contains a failed statement".into(),
            ));
        }
        Ok(self.dependencies)
    }
}

fn collect_node_dependencies(
    plan: &PhysicalPlan,
    dependencies: &mut GraphReadDependencies,
) -> Result<()> {
    use PhysicalPlan as P;
    // A node scan's filter can perform adjacency reads without an explicit
    // traversal operator. Do not infer its footprint from the leaf alone.
    let predicate = match plan {
        P::FilterExec { predicate, .. } => Some(predicate),
        P::NodeProjectionScanExec { predicate, .. } => predicate.as_ref(),
        _ => None,
    };
    if predicate.is_some_and(predicate_reads_relationships) {
        return Err(HawDBError::Execution(
            "prepared source read requires an adjacency predicate collector".into(),
        ));
    }
    let key = match plan {
        P::SeqNodeScan { label, .. }
        | P::NodeProjectionScanExec { label, .. }
        | P::IndexNodeSeek { label, .. }
        | P::IndexNodeMultiSeek { label, .. }
        | P::IndexNodeUnionSeek { label, .. }
        | P::IndexNodeCompositeSeek { label, .. }
        | P::IndexNodeCompositeRangeSeek { label, .. }
        | P::IndexNodeRangeSeek { label, .. }
        | P::NodeCountExec { label, .. } => VersionKey::ReadNodeDomain((!label.is_empty()).then(|| label.clone())),
        P::FilterExec { input, .. }
        | P::ProjectExec { input, .. }
        | P::AggregateExec { input, .. }
        | P::DistinctExec { input }
        | P::SortExec { input, .. }
        | P::TopNExec { input, .. }
        | P::LimitExec { input, .. } => return collect_node_dependencies(input, dependencies),
        // An optimizer-folded empty result can depend on absent catalog/data.
        // Retain the conservative epoch dependency instead of losing it.
        P::EmptyExec => VersionKey::Database,
        _ => return Err(HawDBError::Execution("prepared source read has an unqualified operator; its dependency collector is not available".into())),
    };
    dependencies
        .record(key)
        .map_err(|error| HawDBError::Execution(error.to_string()))
}

fn predicate_reads_relationships(predicate: &crate::planner::Predicate) -> bool {
    use crate::planner::Predicate as P;
    match predicate {
        P::And(parts) | P::Or(parts) => parts.iter().any(predicate_reads_relationships),
        P::Not(inner) => predicate_reads_relationships(inner),
        P::RelationshipExists { .. } | P::BoundRelationshipExists { .. } => true,
        P::ConstantBool(_)
        | P::IdEq { .. }
        | P::IdNotEq { .. }
        | P::IdCompare { .. }
        | P::IdIn { .. }
        | P::PropertyEq { .. }
        | P::PropertyNotEq { .. }
        | P::PropertyCompare { .. }
        | P::ExpressionEq { .. }
        | P::ExpressionNotEq { .. }
        | P::ExpressionCompare { .. }
        | P::ExpressionContains { .. }
        | P::PropertyListContains { .. }
        | P::PropertyListContainsLower { .. }
        | P::PropertyContains { .. }
        | P::PropertyStartsWith { .. }
        | P::PropertyEndsWith { .. }
        | P::PropertyRegexMatch { .. }
        | P::PropertyIsNull { .. }
        | P::PropertyIsNotNull { .. }
        | P::PropertyIn { .. } => false,
    }
}

#[derive(Debug)]
pub enum ProjectionPublicationError {
    SourceChanged,
    SourceNotDurable,
    SourceEpochMismatch { captured: u64, staged: u64 },
    Dependency(GraphReadDependencyError),
    Storage(HawDBError),
    Generation(ProjectionGenerationError),
}

impl fmt::Display for ProjectionPublicationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceChanged => f.write_str("projection source database instance changed"),
            Self::SourceNotDurable => {
                f.write_str("projection source requires a completed durability barrier")
            }
            Self::SourceEpochMismatch { captured, staged } => write!(
                f,
                "projection watermark {staged} does not match captured source epoch {captured}"
            ),
            Self::Dependency(error) => error.fmt(f),
            Self::Storage(error) => error.fmt(f),
            Self::Generation(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ProjectionPublicationError {}

impl Database {
    pub fn begin_prepared_read(&self, limits: PreparedReadLimits) -> Result<PreparedRead> {
        let graph = self.runtime.get()?.store.capture_read_dependencies(
            limits.dependency_entries.get(),
            limits.dependency_bytes.get(),
        )?;
        let mut read = self.begin_read_transaction()?;
        read.config.max_read_result_rows = restrictive_query_limit(
            read.config.max_read_result_rows,
            Some(limits.statement_rows.get()),
        );
        read.config.max_read_result_payload_bytes = restrictive_query_limit(
            read.config.max_read_result_payload_bytes,
            Some(limits.statement_payload_bytes.get()),
        );
        Ok(PreparedRead {
            read,
            dependencies: PreparedReadDependencies {
                source: self.instance_identity()?,
                graph,
            },
            failed: false,
        })
    }

    /// Validates and durably publishes a full generation under one exclusive
    /// database borrow. The catalog's expected-head CAS remains authoritative.
    /// Host bindings/cursors need their additional owner fence before calling.
    /// Relaxed source writes are refused until source-barrier integration is
    /// qualified; this API never silently changes the host durability policy.
    pub fn publish_prepared_projection(
        &mut self,
        dependencies: &PreparedReadDependencies,
        sealed: &SealedProjectionGeneration,
    ) -> std::result::Result<ProjectionGenerationPublishReport, ProjectionPublicationError> {
        use ProjectionPublicationError as E;
        self.ensure_writable().map_err(E::Storage)?;
        let catalog = self.projection_generation_store().map_err(E::Storage)?;
        // Resolve an already committed generation before revalidating an old
        // source token. Subsequent source writes/reopen do not undo its receipt.
        if let Some(receipt) = catalog.published_receipt(sealed).map_err(E::Generation)? {
            return Ok(receipt);
        }
        if self.instance_identity().map_err(E::Storage)? != dependencies.source {
            return Err(E::SourceChanged);
        }
        let captured = dependencies.source_epoch();
        let staged = sealed.manifest().begin.source_watermark;
        if captured != staged {
            return Err(E::SourceEpochMismatch { captured, staged });
        }
        let runtime = self.runtime.get().map_err(E::Storage)?;
        runtime
            .store
            .validate_read_dependencies(&dependencies.graph)
            .map_err(E::Dependency)?;
        if self.durability != DurabilityPolicy::SyncOnEveryWrite
            || runtime.store.wal_sync_group_active()
        {
            return Err(E::SourceNotDurable);
        }
        catalog.publish(sealed).map_err(E::Generation)
    }
}

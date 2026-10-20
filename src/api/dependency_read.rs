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
        | P::ExpressionIn { .. }
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

#[cfg(test)]
mod tests {
    use super::*;

    struct Random(u64);

    impl Random {
        fn below(&mut self, bound: u64) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0 % bound
        }
    }

    #[test]
    fn randomized_node_dependency_validation_preserves_query_results() {
        let mut accepted = [0; 4];
        let mut rejected = [0; 4];
        // Enumerate the shape/write/relation matrix; randomize the label, data,
        // selected key and mutation value so every seed exercises every boundary.
        for seed in [1120, 0x51ce, 0xdecaf, 0x1234_5678] {
            let mut random = Random(seed);
            for (shape, (name, suffix, access)) in [
                (
                    "scan",
                    "RETURN n.key AS key, n.value AS value",
                    "SeqNodeScan",
                ),
                (
                    "seek",
                    "WHERE n.key = $key RETURN n.key AS key, n.value AS value",
                    "IndexNodeSeek",
                ),
                ("count", "RETURN count(n) AS total", "NodeCount"),
                (
                    "empty",
                    "WHERE n.key = $key RETURN n.key AS key, n.value AS value",
                    "IndexNodeSeek",
                ),
            ]
            .into_iter()
            .enumerate()
            {
                for mutation in ["insert", "update", "delete"] {
                    for same_label in [false, true] {
                        let mut db = Database::new();
                        let labels = ["Memory", "Document", "Source"];
                        let read_index = random.below(3) as usize;
                        let other_index = (read_index + 1 + random.below(2) as usize) % 3;
                        let read_label = labels[read_index];
                        let other_label = labels[other_index];
                        let write_label = if same_label { read_label } else { other_label };
                        let case = format!(
                            "seed={seed:#x} query={name} mutation={mutation} read={read_label} write={write_label}"
                        );
                        let count = 32 + random.below(16);
                        for label in [read_label, other_label] {
                            for key in 0..count {
                                let value = random.below(10_000);
                                db.query(&format!(
                                    "CREATE (:{label} {{key: {key}, value: {value}}})"
                                ))
                                .unwrap();
                            }
                        }
                        db.query(&format!("CREATE INDEX ON :{read_label}(key)"))
                            .unwrap();
                        let selected = random.below(count);
                        let key = if name == "empty" { count } else { selected };
                        let parameters = BTreeMap::from([("key".into(), Value::Int(key as i64))]);
                        let query = format!("MATCH (n:{read_label}) {suffix}");
                        let plan = db
                            .explain_query_with_params(&query, &parameters)
                            .unwrap()
                            .physical_plan
                            .explain(0);
                        assert!(plan.contains(access), "{case}: {plan}");
                        let mut preparation = db
                            .begin_prepared_read(PreparedReadLimits {
                                dependency_entries: NonZeroUsize::new(8).unwrap(),
                                dependency_bytes: NonZeroUsize::new(4096).unwrap(),
                                statement_rows: NonZeroUsize::new(64).unwrap(),
                                statement_payload_bytes: NonZeroUsize::new(16 * 1024).unwrap(),
                            })
                            .unwrap();
                        let mut before = preparation
                            .query_with_params(&query, &parameters)
                            .unwrap_or_else(|error| panic!("{case}: {error}"))
                            .output
                            .rows
                            .into_rows();
                        before.sort();
                        if name == "empty" {
                            assert!(before.is_empty(), "{case}");
                        }
                        let write = match mutation {
                            "insert" => format!(
                                "CREATE (:{write_label} {{key: {count}, value: {}}})",
                                random.below(10_000)
                            ),
                            "update" => format!(
                                "MATCH (n:{write_label}) WHERE n.key = {selected} SET n.value = {}",
                                10_000 + random.below(10_000)
                            ),
                            "delete" => {
                                format!("MATCH (n:{write_label}) WHERE n.key = {selected} DELETE n")
                            }
                            _ => unreachable!(),
                        };
                        db.query(&write)
                            .unwrap_or_else(|error| panic!("{case}: {error}"));
                        let validation_epoch = db.commit_epoch().unwrap();
                        assert!(validation_epoch > preparation.source_epoch(), "{case}");
                        let mut retained = preparation
                            .query_with_params(&query, &parameters)
                            .unwrap()
                            .output
                            .rows
                            .into_rows();
                        retained.sort();
                        assert_eq!(retained, before, "{case}: pinned source changed");
                        let dependencies = preparation.finish().unwrap();
                        let mut after = db
                            .query_with_params(&query, &parameters)
                            .unwrap()
                            .rows
                            .into_rows();
                        after.sort();
                        // Use the real commit-side stamps and canonical validator,
                        // independently re-running the query outside the collector.
                        let validation = db
                            .runtime
                            .get()
                            .unwrap()
                            .store
                            .validate_read_dependencies(&dependencies.graph);
                        match validation {
                            Ok(()) => {
                                assert_eq!(before, after, "{case}: accepted stale query result");
                                assert!(!same_label, "{case}: missed label-domain invalidation");
                                accepted[shape] += 1;
                            }
                            Err(GraphReadDependencyError::Conflict(_)) => {
                                assert!(
                                    same_label || dependencies.requires_unchanged_database_epoch(),
                                    "{case}: rejected disjoint write"
                                );
                                rejected[shape] += 1;
                            }
                            Err(error) => panic!("{case}: unexpected validation error: {error}"),
                        }
                        assert_eq!(db.commit_epoch().unwrap(), validation_epoch, "{case}");
                    }
                }
            }
        }
        // An always-rejecting validator cannot satisfy the differential property
        // vacuously; an always-accepting validator must also fail this campaign.
        assert_eq!(accepted, [12; 4]);
        assert_eq!(rejected, [12; 4]);
    }
}

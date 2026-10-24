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

use crate::analytics::ProjectedGraph;
use crate::cypher;
use crate::error::{HawDBError, Result};
use crate::executor::{self, Row, RowRef};
use crate::optimizer::{
    CascadesOptimizer, OptimizerConfig, OptimizerContext, OptimizerSearchDirective, OptimizerTrace,
    PhysicalPlan, ResourceHints,
};
use crate::qos::{
    BackgroundWorkHint, BackgroundWorkPlan, LocalQosPolicy, LocalQosScheduler, LocalQosState,
    QosAdmission, WorkClass, WorkRequest,
};
#[cfg(test)]
use crate::schema::GraphStatistics;
use crate::schema::{Catalog, SchemaObjectState};
#[cfg(test)]
use crate::schema::{
    CompositeIndexDescriptor, ConstraintDescriptor, IndexDescriptor, PropertyDescriptor,
    TableDescriptor,
};
use crate::search::{
    projection_row_from_node_with_graph_metadata, search_metadata_predicate_pushdown,
    AdaptiveVectorSearchOptions, CompressedVectorSearchMode, MetadataRepairOptions,
    MetadataRepairSummary, SearchCandidateSetReport, SearchDerivedArtifactReport,
    SearchEmptyReasonCode, SearchFusionWeights, SearchIndex, SearchMatchedSpan,
    SearchPredicatePushdownReport, SearchProjectionDelta, SearchProjectionDeltaReport,
    SearchProjectionFreshness, SearchQueryOptions, SearchRebuildOptions, SearchRebuildSummary,
    SearchResultSet, SearchRetrieverCandidateSetReport,
};
use crate::store::{
    restore_storage_backup, AdjacencyConsistencyReport, AdjacencyConsolidationPlan,
    AdjacencyConsolidationReport, AdjacencyDirection, AdjacencyLayout, AppendSegmentReadOutput,
    AppendTableSchema, AppendTransaction, BasicStatisticsConsistencyReport,
    DegreeStatisticsConsistencyReport, DistinctValueStatisticsConsistencyReport, DurabilityPolicy,
    GraphMutationLockFootprint, GraphMutationSavepoint, GraphMutationTransaction,
    GraphSnapshotNodeImport, GraphSnapshotRelationshipImport, GraphStore, HawDBSnapshotRowsImport,
    KernelWriteBatch, MutationSummary, NodeId, NodeRecord, OptimizerStatisticsRefreshWork,
    PreparedCheckpoint, PropertyIndexConsistencyReport, PropertyIndexProjectionRebuildAction,
    PublishedReadView, RecoveryMode, RelId, RelRecord, SchemaMaintenanceAction,
    SegmentCacheSnapshot, StorageBackupReport, StoragePressureSnapshot,
    StorageReclamationWatermark, StorageRecoveryReport, StorageRestoreReport, StorageScrubReport,
    StoreStableIdMapping, WalReplayConfig,
};
use crate::telemetry::{
    operations_telemetry_readiness, qos_telemetry_sink, runtime_telemetry_sink, KernelTelemetry,
    KernelTelemetryOperation, OperationsTelemetryReadiness, TelemetrySink,
};
use crate::value::Value;
use canonical_snapshot::export_canonical_graph_snapshot_for;
use explain::{empty_read_execution_profile, explain_analyze_output_row, explain_output_row};
use hawdb_optimizer::{
    search_field_is_enum_like, SearchPredicate, SearchPredicateOp, SearchPredicateSet,
};
use plan_cache::{
    optimized_query_plan_for, statement_uses_plan_cache, OptimizedQueryPlan,
    OptimizerEnvironmentKey, OptimizerPlanningCache, PlanCache, PlanCacheContext, PlanCacheMode,
    PlanTraceMode, DEFAULT_PLAN_CACHE_MAX_ENTRIES,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use system_variables::{
    apply_set_system_variable, query_statement_variables_for_statement,
    query_work_request_for_statement, reject_system_variable_parameters,
};

mod access_control;
mod artifact_jobs;
mod automatic_checkpoint;
pub use automatic_checkpoint::AutomaticCheckpointReport;
mod branch_lifecycle;
mod canonical_snapshot;
mod concurrent;
#[cfg(test)]
pub(crate) use concurrent::AutocommitMutationPhase;
pub(crate) use concurrent::{
    ExclusiveDatabaseLease, PublishedConcurrentRead, SharedDatabaseGuard, SharedDatabaseOwner,
};
mod vector_index;
pub use vector_index::{
    VectorIndexBuildError, VectorIndexBuildOptions, VectorIndexField, VectorIndexLanguageBudget,
    VectorIndexLanguageReport, VectorIndexReadError, VectorIndexReadTransaction,
    VectorIndexSqlReport, VectorIndexSqlRequest,
};
mod transaction_read_dependencies;
pub use transaction_read_dependencies::{
    TransactionIsolation, TransactionReadDependencyLimits, TransactionReadDependencyUsage,
};
mod dependency_read;
pub use dependency_read::{
    PreparedProjectionFeed, PreparedProjectionFullSource, PreparedRead, PreparedReadDependencies,
    PreparedReadLimits, ProjectionBaselineComparisonConfig, ProjectionBaselineComparisonSession,
    ProjectionBaselineComparisonStep, ProjectionBaselineMappingContext,
    ProjectionBaselineMappingFunction, ProjectionBaselineMappingLimits, ProjectionCoverageConfig,
    ProjectionCoverageSession, ProjectionCoverageStep, ProjectionCoveredUpdate,
    ProjectionFeedError, ProjectionFeedItem, ProjectionFeedLimits, ProjectionFeedRead,
    ProjectionFeedResource, ProjectionFeedStep, ProjectionFeedUsage, ProjectionFullRebuildConfig,
    ProjectionFullRebuildSession, ProjectionFullRebuildStep, ProjectionFullSourceItem,
    ProjectionFullSourceLimits, ProjectionMappingContext, ProjectionMappingFunction,
    ProjectionMappingInput, ProjectionMappingLimits, ProjectionMappingStatus,
    ProjectionMappingUsage, ProjectionMatchedBaseline, ProjectionOutputVerificationContext,
    ProjectionOutputVerificationFunction, ProjectionOutputVerificationSession,
    ProjectionPreparedFullRebuild, ProjectionPublicationError, ProjectionSourceDurability,
    ProjectionSourceScope, ProjectionStagedInputs, ProjectionStagingSession,
    ProjectionStagingSessionConfig, ProjectionStagingSessionStep, ReadDependencyValidationFailures,
};
mod explain;
mod graph_analytics;
mod knowledge_graph_filters;
mod knowledge_graph_read;
pub use graph_analytics::{
    GraphAnalyticsAlgorithm, GraphAnalyticsFreshness, GraphAnalyticsPublicationStatus,
    GraphAnalyticsRequest, PreparedGraphAnalytics,
};
#[cfg(test)]
mod explain_format_tests;
mod observability;
mod plan_cache;
pub(crate) mod publication;
pub use publication::{DatabasePublicationOwner, DatabasePublicationState};
#[cfg(feature = "qualification")]
pub use publication::{PublicationLockDurationSnapshot, PublicationLockMetricsSnapshot};
#[cfg(feature = "qualification")]
mod query_preparation_metrics;
mod query_request;
mod query_runtime;
#[cfg(feature = "qualification")]
pub use query_preparation_metrics::{query_preparation_metrics, QueryPreparationMetricsSnapshot};
mod sql_runtime;
#[cfg(feature = "tokio-runtime")]
pub(crate) use sql_runtime::PreparedRuntimeSql;
mod retained_query;
pub use retained_query::{
    ArrowArray, ArrowArrayStream, ArrowSchema, RetainedArrowCodeOwner, RetainedArrowExport,
    RetainedArrowSchema, RetainedArrowStream, RetainedColumnRole, RetainedColumnSchema,
    RetainedColumnType, RetainedColumnValues, RetainedNodeCursor, RetainedNodeField,
    RetainedNodeLabels, RetainedNodeSnapshotCursor, RetainedNodeTaskCursor, RetainedNodeView,
    RetainedQueryBatch, RetainedQueryCursor, RetainedQueryError, RetainedQueryOptions,
    RetainedQueryProfile, RetainedQueryStatus,
};
mod resource_profile;
mod runtime_cell;
mod schema_guidance;
mod search_projection_catch_up;
mod search_projection_consumer;
mod statement_shape;
pub use search_projection_consumer::*;
mod source_candidates;
mod system_schema;
mod system_variables;
mod transaction_locks;
mod types;

pub(crate) use hawdb_system_sql as system_sql;

pub use branch_lifecycle::{
    BranchAbandonmentAcknowledgement, BranchAbandonmentLimits, BranchAbandonmentPlan,
    BranchAbandonmentReport, BranchCreateRequest, BranchInfo, BranchLifecycleError,
    BranchLifecycleState, BranchReclamationLimits, BranchReclamationReport, BranchSelector,
};
pub use hawdb_core::graph_rag::{
    MissingScoringFeature, ScoringCombination, ScoringProgram, ScoringProgramError,
    ScoringProgramShape,
};
pub(crate) use hawdb_executor::runtime_admission::runtime_planning_request;
#[cfg(feature = "tokio-runtime")]
pub(crate) use hawdb_executor::runtime_admission::RuntimeAdmissionPlan;
pub use hawdb_storage::branch_create_recovery::{
    BranchCreateRecoveryEntry, BranchCreateRecoveryLimits, BranchCreateRecoveryReport,
    BranchCreateRecoveryStatus,
};
use query_request::BoundScoringRequest;
pub use query_request::{
    BudgetedReadSnapshot, HostScoringRequest, OwnedHostScoringRequest, OwnedQueryRequest,
    OwnedSqlQueryRequest, QueryRequest, ReadSnapshotBudget, ReadSnapshotUsage, ScoringRequest,
    SqlQueryRequest,
};
#[cfg(feature = "tokio-runtime")]
pub(crate) use query_runtime::RuntimePlanningSnapshot;
pub(crate) use query_runtime::{runtime_result_memory_budget, PreparedRuntimeQuery};
pub use types::*;

const DEFAULT_SEARCH_PROJECTION_CHANGE_LOG_MAX_ENTRIES: usize = 4096;
const DEFAULT_SEARCH_PROJECTION_CHANGE_LOG_MAX_BYTES: usize = 64 * 1024 * 1024;

pub use artifact_jobs::{
    DerivedArtifactJob, DerivedArtifactJobReport, DerivedArtifactJobStatus,
    ExternalContentArtifactJobCompletion, ExternalContentArtifactJobSummary,
    ExternalContentArtifactRuntimeManifest,
};
pub use canonical_snapshot::{
    hawdb_lightning_initial_import_advance_checkpoint,
    hawdb_lightning_initial_import_advance_durable_state_streaming,
    hawdb_lightning_initial_import_advance_durable_state_with_search_projection_batch,
    hawdb_lightning_initial_import_checkpoint_readiness,
    hawdb_lightning_initial_import_cutover_catch_up_report,
    hawdb_lightning_initial_import_decode_durable_state,
    hawdb_lightning_initial_import_document_identity_coverage,
    hawdb_lightning_initial_import_durable_state_report,
    hawdb_lightning_initial_import_encode_durable_state, hawdb_lightning_initial_import_plan,
    hawdb_lightning_initial_import_plan_with_document_identities,
    hawdb_lightning_initial_import_readiness, hawdb_lightning_initial_import_recovery_readiness,
    hawdb_lightning_initial_import_resume_action,
    hawdb_lightning_initial_import_search_projection_batch_report,
    hawdb_lightning_initial_import_search_projection_batch_report_with_document_identities,
    hawdb_lightning_initial_import_session_bundle_readiness,
    hawdb_lightning_initial_import_session_report,
    hawdb_lightning_initial_import_source_bundle_readiness,
    hawdb_lightning_initial_import_source_fingerprint,
    hawdb_lightning_initial_import_startup_readiness, parse_hawdb_lightning_graph_stream_export,
    validate_hawdb_lightning_graph_stream, validate_hawdb_lightning_relational_stream,
    CanonicalGraphSnapshotExport, CanonicalGraphSnapshotValidation,
    CanonicalSnapshotEndpointViolation, CanonicalSnapshotIdentityAudit, CanonicalSnapshotNode,
    CanonicalSnapshotRelationship, CanonicalStableIdMapping, HawDBLightningBootstrapExport,
    HawDBLightningBootstrapManifest, HawDBLightningGraphStream,
    HawDBLightningGraphStreamValidation, HawDBLightningInitialImportApplyReport,
    HawDBLightningInitialImportCheckpoint, HawDBLightningInitialImportCheckpointProgress,
    HawDBLightningInitialImportCheckpointProgressReport,
    HawDBLightningInitialImportCheckpointReadiness,
    HawDBLightningInitialImportCutoverCatchUpReport, HawDBLightningInitialImportDocumentIdentity,
    HawDBLightningInitialImportDocumentIdentityCoverage,
    HawDBLightningInitialImportDocumentIdentityKindReport,
    HawDBLightningInitialImportDurableBatchAdvanceReport, HawDBLightningInitialImportDurableState,
    HawDBLightningInitialImportDurableStateCodecReport,
    HawDBLightningInitialImportDurableStateReport, HawDBLightningInitialImportIdempotencyKey,
    HawDBLightningInitialImportPlan, HawDBLightningInitialImportReadiness,
    HawDBLightningInitialImportReadinessInputs, HawDBLightningInitialImportRecoveryReadinessReport,
    HawDBLightningInitialImportResumeAction, HawDBLightningInitialImportResumeActionKind,
    HawDBLightningInitialImportSearchProjectionBatchReport,
    HawDBLightningInitialImportSessionBundleReadiness, HawDBLightningInitialImportSessionReport,
    HawDBLightningInitialImportSourceBundleReadiness, HawDBLightningInitialImportSourceFingerprint,
    HawDBLightningInitialImportStartupReadinessReport,
    HawDBLightningInitialImportStreamingBatchAdvanceReport, HawDBLightningRelationalStream,
    HawDBLightningRelationalStreamValidation, HAWDB_LIGHTNING_BOOTSTRAP_PROTOCOL_VERSION,
    HAWDB_LIGHTNING_GRAPH_STREAM_FORMAT_VERSION,
    HAWDB_LIGHTNING_INITIAL_IMPORT_DURABLE_STATE_PROTOCOL,
    HAWDB_LIGHTNING_RELATIONAL_STREAM_FORMAT_VERSION,
};
pub use concurrent::{
    ConcurrentDatabase, ConcurrentDatabaseMetrics, ConcurrentDatabaseTransaction,
    ConcurrentPhaseMetrics, ConcurrentTransactionMode, ConcurrentTransactionOptions,
    WalGroupCommitActivation, WalGroupCommitAdaptiveColdStartEvidence,
    WalGroupCommitAdaptivePolicyEvidence, WalGroupCommitAdaptiveSteadyStateEvidence,
    WalGroupCommitConfig, WalGroupCommitDelayPolicy, WalGroupCommitEvidence,
    WalGroupCommitSnapshot, WalGroupCommitTailLatencyEvidence, WalGroupCommitWaitDecision,
    DEFAULT_PESSIMISTIC_LOCK_TIMEOUT, DEFAULT_WAL_GROUP_COMMIT_MAX_BYTES,
    DEFAULT_WAL_GROUP_COMMIT_MAX_DELAY, DEFAULT_WAL_GROUP_COMMIT_MAX_ENTRIES,
};
pub use hawdb_core::QueryAccessControlContext;
pub use hawdb_evidence::AccessControlPolicyReadiness;
pub use hawdb_executor::{BoundedReadQueryOutput, QueryStreamOptions, QueryStreamReport};
pub use hawdb_explain::{ExplainAnalyzeOutput, ExplainOutput, NowledgeGraphExplainOutput};
pub use hawdb_system_sql::{
    SlowQueryLogExportOptions, SlowQueryLogRecordSummary, SLOW_QUERY_LOG_EVENT_PROTOCOL,
};
pub use plan_cache::{PlanCacheBypassReason, PlanCacheLookup, PlanCacheStats};
pub use resource_profile::{
    StorageResourceProfileLimits, StorageResourceProfileReport, STORAGE_RESOURCE_PROFILE_PROTOCOL,
};
pub use search_projection_catch_up::{
    ScheduledSearchProjectionCatchUpReport, SearchProjectionCatchUpReport,
    SearchProjectionCatchUpStopReason,
};
pub use source_candidates::{
    KnowledgeSourceCandidateRow, KnowledgeSourceCandidateScanOrigin,
    KnowledgeSourceCandidateScanOutput, KnowledgeSourceCandidateScanRequest,
};
pub use system_schema::{SystemSchemaMigration, SystemSchemaRegistry, SystemSchemaUpgradeReport};
pub use system_variables::QuerySystemVariables;

fn hawdb_lightning_initial_import_source_fingerprint_key(
    manifest: &HawDBLightningBootstrapManifest,
) -> String {
    let fingerprint = hawdb_lightning_initial_import_source_fingerprint(manifest);
    format!(
        "v{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
        fingerprint.protocol_version,
        fingerprint.database_commit_epoch,
        fingerprint.graph_commit_epoch,
        fingerprint.logical_checksum,
        fingerprint.graph_stream_checksum,
        fingerprint.graph_stream_byte_len,
        fingerprint.relational_stream_checksum,
        fingerprint.relational_stream_byte_len,
        fingerprint.schema_checksum,
        fingerprint.node_count,
        fingerprint.relationship_count,
        fingerprint.relational_table_count,
        fingerprint.relational_row_count,
    )
}

/// Equality-only identity of one admitted database runtime in this process.
///
/// Clones retain only a marker, not database resources. Reopening or switching
/// to another branch creates a different identity, even at equal contents/epochs.
/// This token is not a persistent identity or evidence of a retained fence.
#[derive(Debug, Clone)]
pub struct DatabaseInstanceIdentity(Arc<()>);

impl DatabaseInstanceIdentity {
    fn new() -> Self {
        Self(Arc::new(()))
    }
}

impl PartialEq for DatabaseInstanceIdentity {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for DatabaseInstanceIdentity {}

#[derive(Debug)]
pub struct Database {
    runtime: runtime_cell::BranchRuntimeCell,
    project_root_path: Option<PathBuf>,
    branch_create_recovery: Option<BranchCreateRecoveryReport>,
    durability: DurabilityPolicy,
    slow_query_log: Arc<SharedState<system_sql::SlowQueryLog>>,
    statement_summary: Arc<SharedState<system_sql::StatementSummary>>,
    config: DatabaseConfig,
    local_qos_scheduler: LocalQosScheduler,
    system_variables: QuerySystemVariables,
    derived_artifact_jobs: hawdb_artifact::DerivedArtifactJobQueue,
    derived_artifact_root_pins:
        BTreeMap<u64, Option<Arc<hawdb_storage::branch_root_pins::RootPin>>>,
    telemetry: Option<Arc<dyn TelemetrySink>>,
    runtime_governor: Option<hawdb_qos::RuntimeGovernor>,
    retained_runtime: Arc<retained_query::RetainedRuntime>,
}

pub(crate) struct DatabaseCheckpointSource {
    catalog: Catalog,
    store: GraphStore,
    suspension: Option<automatic_checkpoint::Suspension>,
}

pub(crate) struct PreparedDatabaseCheckpoint {
    checkpoint: PreparedCheckpoint,
    _suspension: Option<automatic_checkpoint::Suspension>,
}

impl DatabaseCheckpointSource {
    pub(crate) fn prepare(self) -> Result<Option<PreparedDatabaseCheckpoint>> {
        Ok(self
            .store
            .prepare_checkpoint(&self.catalog)?
            .map(|checkpoint| PreparedDatabaseCheckpoint {
                checkpoint,
                _suspension: self.suspension,
            }))
    }
}

#[derive(Debug)]
pub(super) struct SharedState<T>(Mutex<T>);

impl<T> SharedState<T> {
    fn new(value: T) -> Self {
        Self(Mutex::new(value))
    }

    pub(super) fn borrow(&self) -> MutexGuard<'_, T> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(super) fn borrow_mut(&self) -> MutexGuard<'_, T> {
        self.borrow()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseConfig {
    pub read_only: bool,
    /// Maximum monotonic age of uncheckpointed data in an admitted persistent
    /// writer. The background-maintenance capability controls worker creation.
    pub automatic_checkpoint_max_age: std::time::Duration,
    /// Bounded pending-child recovery during a writable project open.
    pub branch_create_recovery_limits: BranchCreateRecoveryLimits,
    /// Optional host-wide read row ceiling. `Some(0)` and `None` (the default) mean unlimited.
    /// Session settings may lower a positive ceiling, but cannot raise it.
    pub max_read_result_rows: Option<usize>,
    pub max_read_result_payload_bytes: Option<usize>,
    pub execution_memory: executor::ExecutionMemoryConfig,
    pub mutation_limits: hawdb_storage::mutation::MutationLimits,
    /// Maximum memo groups available to graph and relational optimizer search.
    pub max_optimizer_groups: Option<usize>,
    /// Maximum memo expressions available to relational join enumeration.
    pub max_relational_join_expressions: Option<usize>,
    pub recovery_mode: RecoveryMode,
    pub max_wal_replay_entries: Option<usize>,
    pub max_wal_replay_bytes: Option<u64>,
    /// Maximum sealed WAL generations recovered for one branch admission.
    /// Checkpoint compaction resets the chain; default is 256.
    pub max_branch_sealed_wal_intervals: usize,
    /// Shared capacity target for all engine-owned project file descriptors.
    /// Defaults to 1024 without pre-opening files. Native Unix acquisition limits
    /// effective admission to the OS soft limit minus 64 host-owned handles.
    /// Process limits remain under host control; idle immutable files use LRU.
    /// Independent contexts for one canonical project must request the same value.
    pub max_open_files: usize,
    /// Reject foreground descriptor admission when `max_open_files` cannot be
    /// met by evicting idle handles. Defaults to false. Internal control I/O,
    /// recovery and maintenance admission always use a non-strict policy.
    pub strict_capacity_limit: bool,
    pub max_wal_quarantine_bytes: u64,
    pub max_wal_record_bytes: Option<usize>,
    pub max_wal_batch_operations: Option<usize>,
    pub max_checkpoint_encoded_bytes: Option<u64>,
    pub max_checkpoint_decoded_bytes: Option<u64>,
    pub segment_cache_capacity_bytes: u64,
    /// Aggregate encoded graph-manifest bytes allowed during database open.
    /// Payload pages remain governed separately by the segment cache.
    pub max_graph_manifest_open_bytes: u64,
    /// Maximum physical relational-index file bytes fetched by one query.
    /// Cache hits consume the separately derived logical traversal budget only.
    pub max_relational_index_read_bytes: NonZeroUsize,
    pub max_relational_hydration_bytes: NonZeroUsize,
    pub storage_residency_mode: hawdb_storage::config::StorageResidencyMode,
    pub auto_materialize_checkpoint_bytes: u64,
    pub max_out_of_core_delta_bytes: Option<u64>,
    /// Derived columnar shadow double-write: every checkpoint also
    /// publishes a column-group catalog under `column-groups/`, and recovery
    /// validates it. Off by default; with the flag off checkpoints are
    /// byte-for-byte unchanged and no shadow directory exists. Reads are
    /// never served from the shadow.
    pub graph_columnar_shadow_checkpoint: bool,
    /// Persistent relational-index publication and read activation mode.
    pub relational_index_mode: hawdb_storage::config::RelationalIndexMode,
    /// Enables the metadata-only monotonic INSERT fast path for RowPage tables.
    /// Disabled by default until workload qualification explicitly activates it.
    pub relational_monotonic_append_fast_path: bool,
    pub max_search_projection_change_log_entries: Option<usize>,
    pub max_search_projection_change_log_bytes: Option<usize>,
    pub search_projection_relational_change_limits:
        hawdb_storage::relational::RelationalPrimaryKeyChangeCaptureLimits,
    /// Maximum entries per graph physical-plan or relational SQL-template cache.
    pub max_plan_cache_entries: Option<usize>,
    /// Commit-epoch lag tolerated before the inline query path recomputes
    /// optimizer statistics. `None` or `Some(0)` refreshes on every committed
    /// data change (freshest plans). Larger values amortize statistics
    /// recomputation over write-heavy workloads; stale statistics only affect
    /// plan choice, never correctness.
    pub optimizer_statistics_inline_commit_lag: Option<u64>,
    pub slow_query_log_capacity: usize,
    pub slow_query_log_threshold_micros: u128,
    pub statement_summary_capacity: usize,
    /// Background-work policy used for this database's full runtime lifetime
    /// by its shared local QoS scheduler.
    pub local_qos_policy: LocalQosPolicy,
    pub runtime_capabilities: hawdb_core::RuntimeCapabilities,
    pub compressed_vector_search_mode: CompressedVectorSearchMode,
    pub adaptive_vector_backend_policy: hawdb_optimizer::AdaptiveVectorBackendPolicy,
}

/// Default host row-count ceiling: no ceiling. Byte budgets remain active.
pub const DEFAULT_MAX_READ_RESULT_ROWS: Option<usize> = None;
pub const DEFAULT_MAX_READ_RESULT_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;

fn restrictive_query_limit(configured: Option<usize>, requested: Option<usize>) -> Option<usize> {
    match (configured, requested) {
        (Some(configured), Some(requested)) => Some(configured.min(requested)),
        (Some(configured), None) => Some(configured),
        (None, Some(requested)) => Some(requested),
        (None, None) => None,
    }
}

fn relational_query_limits_with_payload(
    config: &DatabaseConfig,
    max_rows: Option<usize>,
    max_payload_bytes: Option<usize>,
) -> crate::relational_sql::RelationalQueryLimits {
    let max_output_rows =
        restrictive_query_limit(config.read_result_row_limit(), max_rows).unwrap_or(usize::MAX);
    let max_output_payload_bytes =
        restrictive_query_limit(config.max_read_result_payload_bytes, max_payload_bytes)
            .unwrap_or(DEFAULT_MAX_READ_RESULT_PAYLOAD_BYTES);
    let max_intermediate_rows = config.read_result_row_limit().unwrap_or(usize::MAX);
    // A blocking relational query first reads the bounded scan projection and
    // then re-reads at most `max_output_rows` selected locators with the final
    // output projection. Budget both phases explicitly so a scan that exactly
    // reaches its intermediate-row limit can still produce an admitted result.
    let max_row_read_rows = max_intermediate_rows.saturating_add(max_output_rows).max(1);
    let max_scan_pages = max_intermediate_rows
        .div_ceil(hawdb_storage::relational::DEFAULT_RELATIONAL_ROW_PAGE_ROWS)
        .max(1);
    let max_row_read_pages = max_scan_pages.saturating_add(max_output_rows).max(1);
    let max_row_read_bytes = max_row_read_pages
        .saturating_mul(hawdb_storage::relational::DEFAULT_RELATIONAL_ROW_PAGE_BYTES)
        .max(1);
    // Logical index traversal includes cache hits and must scale with the
    // statement's admitted intermediate rows. Physical index reads retain a
    // separate fixed I/O ceiling so a warm cache cannot disable query bounds.
    let max_index_read_pages = max_intermediate_rows
        .saturating_mul(
            hawdb_storage::relational::DEFAULT_RELATIONAL_INDEX_READ_TREE_HEIGHT as usize,
        )
        .max(1);
    let max_index_read_bytes = max_index_read_pages
        .saturating_mul(hawdb_storage::index_page::DEFAULT_IMMUTABLE_INDEX_PAGE_BYTES)
        .max(1);
    crate::relational_sql::RelationalQueryLimits {
        max_output_rows,
        max_output_payload_bytes,
        max_intermediate_rows,
        max_candidate_work: max_intermediate_rows,
        hydration: hawdb_storage::relational::RelationalHydrationBudget {
            max_rows: max_row_read_rows,
            max_compressed_bytes: config.max_relational_hydration_bytes.get(),
            max_decompressed_bytes: config.max_relational_hydration_bytes.get(),
            max_memory_bytes: config.max_relational_hydration_bytes.get(),
            ..hawdb_storage::relational::RelationalHydrationBudget::default()
        },
        index_read: hawdb_storage::relational::RelationalIndexReadLimits {
            max_pages: NonZeroUsize::new(max_index_read_pages)
                .expect("relational index query page budget is non-zero"),
            max_rows: NonZeroUsize::new(max_intermediate_rows.max(1))
                .expect("relational index query row budget is non-zero"),
            max_bytes: NonZeroUsize::new(max_index_read_bytes)
                .expect("relational index query byte budget is non-zero"),
            max_file_bytes: config.max_relational_index_read_bytes.get(),
            ..hawdb_storage::relational::RelationalIndexReadLimits::default()
        },
        row_read: hawdb_storage::relational::RelationalRowPageSnapshotReadLimits {
            demand: hawdb_storage::relational::RelationalRowPageDemandReadLimits {
                max_pages: NonZeroUsize::new(max_row_read_pages)
                    .expect("relational row query page budget is non-zero"),
                max_rows: NonZeroUsize::new(max_row_read_rows)
                    .expect("relational row query row budget is non-zero"),
                max_bytes: NonZeroUsize::new(max_row_read_bytes)
                    .expect("relational row query byte budget is non-zero"),
                ..hawdb_storage::relational::RelationalRowPageDemandReadLimits::default()
            },
            ..hawdb_storage::relational::RelationalRowPageSnapshotReadLimits::default()
        },
    }
}

fn relational_join_enumeration_config_from_database_config(
    config: &DatabaseConfig,
) -> hawdb_optimizer::RelationalJoinEnumerationConfig {
    let defaults = hawdb_optimizer::RelationalJoinEnumerationConfig::default();
    hawdb_optimizer::RelationalJoinEnumerationConfig {
        max_groups: optimizer_config_from_database_config(config).max_groups,
        max_expressions: config
            .max_relational_join_expressions
            .unwrap_or(defaults.max_expressions),
    }
}

fn relational_query_resource_context<'a>(
    config: &'a DatabaseConfig,
    max_rows: Option<usize>,
    max_payload_bytes: Option<usize>,
    task_context: Option<&'a hawdb_core::RuntimeTaskContext>,
) -> crate::relational_sql::RelationalQueryResourceContext<'a> {
    crate::relational_sql::RelationalQueryResourceContext::new(
        relational_join_enumeration_config_from_database_config(config),
        relational_query_limits_with_payload(config, max_rows, max_payload_bytes),
        &config.execution_memory,
        task_context,
    )
}

fn relational_plan_template_cache_from_database_config(
    config: &DatabaseConfig,
) -> Arc<crate::relational_sql::RelationalPlanTemplateCache> {
    Arc::new(crate::relational_sql::RelationalPlanTemplateCache::new(
        config.max_plan_cache_entries,
    ))
}

fn relational_index_read_mode<
    'a,
    R: hawdb_relational::index_runtime::RelationalIndexStoreReader,
>(
    config: &DatabaseConfig,
    store: &'a R,
) -> hawdb_relational::index_runtime::RelationalIndexReadMode<'a, R> {
    match config.relational_index_mode {
        hawdb_storage::config::RelationalIndexMode::Materialized => {
            hawdb_relational::index_runtime::RelationalIndexReadMode::Materialized
        }
        hawdb_storage::config::RelationalIndexMode::Shadow => {
            hawdb_relational::index_runtime::RelationalIndexReadMode::Shadow(store)
        }
        hawdb_storage::config::RelationalIndexMode::DemandPaged => {
            hawdb_relational::index_runtime::RelationalIndexReadMode::DemandPaged(store)
        }
        hawdb_storage::config::RelationalIndexMode::Authoritative => {
            hawdb_relational::index_runtime::RelationalIndexReadMode::Authoritative(store)
        }
    }
}

/// Per-session result policy. Independent sessions start with no row-count limit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionConfig {
    /// Maximum result rows; zero means no session row-count limit.
    /// Payload, execution-memory and explicit host ceilings remain enforced.
    /// A positive ceiling at or above usize::MAX imposes no additional bound:
    /// no in-process result can exceed platform row capacity. On wasm32 this
    /// includes values such as 5,000,000,000; positive host ceilings still apply.
    pub max_result_rows: u64,
}

impl SessionConfig {
    fn row_limit(self) -> Option<usize> {
        // No representable result can exceed usize::MAX rows. Avoid asking
        // the execution detector to add one to that effective infinity.
        (self.max_result_rows != 0)
            .then(|| usize::try_from(self.max_result_rows).unwrap_or(usize::MAX))
            .filter(|rows| *rows != usize::MAX)
    }
}

impl DatabaseConfig {
    /// Effective host row-count ceiling. Both zero and `None` disable it.
    pub fn read_result_row_limit(&self) -> Option<usize> {
        self.max_read_result_rows.filter(|rows| *rows != 0)
    }

    fn wal_replay_config(&self) -> WalReplayConfig {
        WalReplayConfig {
            max_open_files: self.max_open_files,
            strict_capacity_limit: self.strict_capacity_limit,
            recovery_mode: self.recovery_mode,
            max_entries: self.max_wal_replay_entries,
            max_bytes: self.max_wal_replay_bytes,
            max_branch_sealed_wal_intervals: self.max_branch_sealed_wal_intervals,
            max_quarantine_bytes: self.max_wal_quarantine_bytes,
            max_record_bytes: self.max_wal_record_bytes,
            max_batch_operations: self.max_wal_batch_operations,
            max_checkpoint_encoded_bytes: self.max_checkpoint_encoded_bytes,
            max_checkpoint_decoded_bytes: self.max_checkpoint_decoded_bytes,
            segment_cache_capacity_bytes: self.segment_cache_capacity_bytes,
            max_graph_manifest_open_bytes: self.max_graph_manifest_open_bytes,
            residency_mode: self.storage_residency_mode,
            auto_materialize_checkpoint_bytes: self.auto_materialize_checkpoint_bytes,
            max_out_of_core_delta_bytes: self.max_out_of_core_delta_bytes,
            graph_columnar_shadow_checkpoint: self.graph_columnar_shadow_checkpoint,
            relational_index_mode: self.relational_index_mode,
        }
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            read_only: false,
            automatic_checkpoint_max_age: std::time::Duration::from_secs(60),
            branch_create_recovery_limits: BranchCreateRecoveryLimits::default(),
            max_read_result_rows: DEFAULT_MAX_READ_RESULT_ROWS,
            max_read_result_payload_bytes: Some(DEFAULT_MAX_READ_RESULT_PAYLOAD_BYTES),
            execution_memory: executor::ExecutionMemoryConfig::default(),
            mutation_limits: hawdb_storage::mutation::MutationLimits::default(),
            max_optimizer_groups: None,
            max_relational_join_expressions: None,
            recovery_mode: RecoveryMode::default(),
            max_wal_replay_entries: Some(hawdb_storage::config::DEFAULT_MAX_WAL_REPLAY_ENTRIES),
            max_wal_replay_bytes: Some(hawdb_storage::config::DEFAULT_MAX_WAL_REPLAY_BYTES),
            max_branch_sealed_wal_intervals:
                hawdb_storage::config::DEFAULT_MAX_BRANCH_SEALED_WAL_INTERVALS,
            max_open_files: hawdb_storage::file_descriptors::DEFAULT_MAX_OPEN_FILES,
            strict_capacity_limit: false,
            max_wal_quarantine_bytes: hawdb_storage::config::DEFAULT_MAX_WAL_QUARANTINE_BYTES,
            max_wal_record_bytes: Some(hawdb_storage::config::DEFAULT_MAX_WAL_RECORD_BYTES),
            max_wal_batch_operations: Some(hawdb_storage::config::DEFAULT_MAX_WAL_BATCH_OPERATIONS),
            max_checkpoint_encoded_bytes: Some(
                hawdb_storage::config::DEFAULT_MAX_CHECKPOINT_ENCODED_BYTES,
            ),
            max_checkpoint_decoded_bytes: Some(
                hawdb_storage::config::DEFAULT_MAX_CHECKPOINT_DECODED_BYTES,
            ),
            segment_cache_capacity_bytes:
                hawdb_storage::config::DEFAULT_SEGMENT_CACHE_CAPACITY_BYTES,
            max_graph_manifest_open_bytes:
                hawdb_storage::config::DEFAULT_MAX_GRAPH_MANIFEST_OPEN_BYTES,
            max_relational_index_read_bytes: NonZeroUsize::new(
                hawdb_storage::relational::DEFAULT_RELATIONAL_INDEX_READ_BYTES,
            )
            .expect("default relational index read byte budget is non-zero"),
            max_relational_hydration_bytes: NonZeroUsize::new(
                hawdb_storage::relational::DEFAULT_MAX_RELATIONAL_HYDRATION_BYTES,
            )
            .expect("default relational hydration byte budget is non-zero"),
            storage_residency_mode: hawdb_storage::config::StorageResidencyMode::Auto,
            auto_materialize_checkpoint_bytes:
                hawdb_storage::config::DEFAULT_AUTO_MATERIALIZE_CHECKPOINT_BYTES,
            max_out_of_core_delta_bytes: Some(
                hawdb_storage::config::DEFAULT_MAX_OUT_OF_CORE_DELTA_BYTES,
            ),
            graph_columnar_shadow_checkpoint: false,
            relational_index_mode: hawdb_storage::config::RelationalIndexMode::default(),
            relational_monotonic_append_fast_path: false,
            max_search_projection_change_log_entries: Some(
                DEFAULT_SEARCH_PROJECTION_CHANGE_LOG_MAX_ENTRIES,
            ),
            max_search_projection_change_log_bytes: Some(
                DEFAULT_SEARCH_PROJECTION_CHANGE_LOG_MAX_BYTES,
            ),
            search_projection_relational_change_limits: Default::default(),
            max_plan_cache_entries: Some(DEFAULT_PLAN_CACHE_MAX_ENTRIES),
            optimizer_statistics_inline_commit_lag: None,
            slow_query_log_capacity: system_sql::DEFAULT_SLOW_QUERY_LOG_CAPACITY,
            slow_query_log_threshold_micros: system_sql::DEFAULT_SLOW_QUERY_LOG_THRESHOLD_MICROS,
            statement_summary_capacity: system_sql::DEFAULT_STATEMENT_SUMMARY_CAPACITY,
            local_qos_policy: LocalQosPolicy::default(),
            runtime_capabilities: crate::compiled_runtime_capabilities(),
            compressed_vector_search_mode: CompressedVectorSearchMode::Disabled,
            adaptive_vector_backend_policy: hawdb_optimizer::AdaptiveVectorBackendPolicy::default(),
        }
    }
}

pub(crate) fn system_runtime_snapshot(
    config: &DatabaseConfig,
) -> system_sql::SystemRuntimeSnapshot {
    system_sql::SystemRuntimeSnapshot::new(
        config.read_only,
        config.read_result_row_limit(),
        config.max_read_result_payload_bytes,
        config.storage_residency_mode,
        config.relational_index_mode,
        config.runtime_capabilities,
    )
}

pub use hawdb_executor::QueryOutput;

/// Deterministic outcome for one relational INSERT, UPDATE, or DELETE statement.
///
/// A staged statement is provisional until its enclosing transaction commits.
/// Conflict no-ops increment `conflict_rows`, never `affected_rows`, and never
/// contribute a `RETURNING` row. In the v1 contract, `rows` contains values for
/// supported `INSERT ... RETURNING` statements and remains empty for count-only
/// UPDATE and DELETE outcomes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationalMutationResult {
    pub affected_rows: usize,
    pub conflict_rows: usize,
    pub rows: executor::QueryRows,
    pub provisional: bool,
}

/// Result of one SQL statement executed inside a database transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlStatementResult {
    pub output: QueryOutput,
    pub mutation: Option<RelationalMutationResult>,
}

/// Confirmed results produced by the durable transaction commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionCommitResult {
    pub output: QueryOutput,
    pub mutations: Vec<RelationalMutationResult>,
    pub append_mutations: Vec<hawdb_storage::append_table::AppendMutationOutcome>,
}

/// Confirmed generated order-key assignments from one durable append commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppendCommitResult {
    pub commit_epoch: u64,
    pub mutations: Vec<hawdb_storage::append_table::AppendMutationOutcome>,
}

/// Declares the relational tables exposed by one owner-scoped projection
/// generation inside a pinned read transaction.
///
/// The binding is generic rather than route-specific. The application owns the
/// PostgreSQL DDL for the named tables and the projection version. HawDB owns
/// active-generation resolution, pin lifetime, row decoding, and query
/// admission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionRelationalReadBinding {
    projection: String,
    owner_key: Vec<u8>,
    projection_version: u64,
    tables: BTreeSet<String>,
}

impl ProjectionRelationalReadBinding {
    pub fn new(
        projection: impl Into<String>,
        owner_key: impl Into<Vec<u8>>,
        projection_version: u64,
        tables: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self> {
        let projection = projection.into();
        let owner_key = owner_key.into();
        let tables = tables.into_iter().map(Into::into).collect::<BTreeSet<_>>();
        if projection.trim().is_empty() {
            return Err(HawDBError::Semantic(
                "projection relational binding requires a non-empty projection name".to_string(),
            ));
        }
        if owner_key.is_empty() {
            return Err(HawDBError::Semantic(
                "projection relational binding requires a non-empty owner key".to_string(),
            ));
        }
        if projection_version == 0 {
            return Err(HawDBError::Semantic(
                "projection relational binding requires a non-zero projection version".to_string(),
            ));
        }
        if tables.is_empty() || tables.iter().any(|table| table.trim().is_empty()) {
            return Err(HawDBError::Semantic(
                "projection relational binding requires non-empty table names".to_string(),
            ));
        }
        Ok(Self {
            projection,
            owner_key,
            projection_version,
            tables,
        })
    }

    pub fn projection(&self) -> &str {
        &self.projection
    }

    pub fn owner_key(&self) -> &[u8] {
        &self.owner_key
    }

    pub const fn projection_version(&self) -> u64 {
        self.projection_version
    }

    pub fn tables(&self) -> &BTreeSet<String> {
        &self.tables
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct StatementExecutionContext<'a> {
    execution_profile: Option<&'a executor::ReadExecutionProfile>,
    retained_profile: Option<&'a RetainedQueryProfile>,
    access_control: Option<&'a QueryAccessControlContext>,
    parse_nanos: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowledgeGraphStatement {
    pub cypher: String,
    pub parameters: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowledgeGraphTransactionOutput {
    pub statement_outputs: Vec<QueryOutput>,
    pub commit_output: QueryOutput,
}

#[derive(Debug)]
pub struct DatabaseTransaction<'a> {
    db: &'a mut Database,
    runtime: DatabaseTransactionRuntime,
    state: DatabaseTransactionState,
    task_context: Option<hawdb_core::RuntimeTaskContext>,
}

#[derive(Debug)]
pub(super) struct DatabaseTransactionRuntime {
    instance_identity: DatabaseInstanceIdentity,
    session_config: SessionConfig,
    optimizer: CascadesOptimizer,
    plan_cache: SharedState<PlanCache>,
    relational_plan_template_cache: Arc<crate::relational_sql::RelationalPlanTemplateCache>,
    optimizer_planning_cache: SharedState<OptimizerPlanningCache>,
    config: DatabaseConfig,
    system_variables: QuerySystemVariables,
    branch_catalog_path: Option<PathBuf>,
    current_branch: Option<branch_lifecycle::CurrentBranch>,
}

#[derive(Debug)]
pub(super) struct DatabaseTransactionState {
    read_dependencies: Option<transaction_read_dependencies::TransactionReadDependencies>,
    graph_transaction: Option<GraphMutationTransaction>,
    relational_transaction: hawdb_storage::relational::RelationalTransaction,
    relational_state: hawdb_storage::relational::RelationalState,
    append_transaction: hawdb_storage::append_table::AppendTransaction,
    append_state: hawdb_storage::append_table::AppendState,
    pending_generated_append_tables: BTreeSet<String>,
    relational_returning: Vec<Option<crate::relational_sql::RelationalReturningProjection>>,
    relational_index:
        std::result::Result<Option<crate::store::RelationalTransactionIndexView>, String>,
    relational_rows:
        std::result::Result<Option<crate::store::RelationalTransactionRowView>, String>,
    // Moves with the private workspace into a queued commit. Checkpoint
    // reclamation must retain its physical generation until the state retires.
    snapshot_pin: Option<ReaderPin>,
}

struct SparseRelationalStatementStage {
    state: hawdb_storage::relational::RelationalState,
    mutation_outcomes: Vec<hawdb_storage::relational::RelationalMutationOutcome>,
    index_capture: hawdb_storage::relational::RelationalIndexChangeCapture,
    row_capture: hawdb_storage::relational::RelationalRowChangeCapture,
}

pub(crate) struct GraphTransactionStatementOutcome {
    pub(crate) output: QueryOutput,
    pub(crate) savepoint: Option<GraphMutationSavepoint>,
    pub(crate) lock_footprint: GraphMutationLockFootprint,
}

#[derive(Debug)]
pub struct DatabaseSession<'a> {
    config: SessionConfig,
    db: &'a mut Database,
    graph_transaction: Option<GraphMutationTransaction>,
    transaction_runtime: Option<DatabaseTransactionRuntime>,
    system_variables: QuerySystemVariables,
}

#[derive(Debug)]
enum ReadPlanCache {
    Private(SharedState<PlanCache>),
    Shared(Arc<SharedState<PlanCache>>),
}

impl std::ops::Deref for ReadPlanCache {
    type Target = SharedState<PlanCache>;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Private(cache) => cache,
            Self::Shared(cache) => cache,
        }
    }
}

#[derive(Debug)]
pub struct DatabaseReadTransaction<S: crate::executor::ExecutionStore = GraphStore> {
    session_config: SessionConfig,
    system_variables: QuerySystemVariables,
    catalog: Catalog,
    store: S,
    published_read_view: PublishedReadView,
    optimizer: CascadesOptimizer,
    plan_cache: ReadPlanCache,
    relational_plan_template_cache: Arc<crate::relational_sql::RelationalPlanTemplateCache>,
    optimizer_planning_cache: Arc<SharedState<OptimizerPlanningCache>>,
    slow_query_snapshot: Vec<system_sql::SlowQueryRecord>,
    statement_summary_snapshot: Vec<system_sql::StatementSummaryRecord>,
    config: DatabaseConfig,
    branch_catalog_path: Option<PathBuf>,
    current_branch: Option<branch_lifecycle::CurrentBranch>,
    projection_relational: Option<ProjectionRelationalReadSnapshot>,
    task_context: Option<hawdb_core::RuntimeTaskContext>,
    _pin: Arc<ReaderPin>,
    retained_runtime: Arc<retained_query::RetainedRuntime>,
}

/// An immutable canonical view that can create independent query contexts.
/// The shared pin survives publication replacement until the last query drops.
#[derive(Debug)]
pub(crate) struct DatabaseReadSnapshot(DatabaseReadTransaction);

impl DatabaseReadSnapshot {
    pub(crate) fn config(&self) -> &DatabaseConfig {
        &self.0.config
    }

    pub(crate) fn ensure_usable(&self) -> Result<()> {
        self.0.store.ensure_usable()
    }

    pub(crate) fn begin_read_transaction(
        &self,
        task_context: &hawdb_core::RuntimeTaskContext,
    ) -> Result<DatabaseReadTransaction> {
        self.fork(Some(task_context.clone()))
    }

    fn fork(
        &self,
        task_context: Option<hawdb_core::RuntimeTaskContext>,
    ) -> Result<DatabaseReadTransaction> {
        self.ensure_usable()?;
        let source = &self.0;
        Ok(DatabaseReadTransaction {
            session_config: source.session_config,
            system_variables: source.system_variables.clone(),
            catalog: source.catalog.clone(),
            store: source.store.snapshot_for_read(),
            published_read_view: source.published_read_view,
            optimizer: source.optimizer.clone(),
            plan_cache: ReadPlanCache::Private(SharedState::new(PlanCache::new(
                source.config.max_plan_cache_entries,
            ))),
            relational_plan_template_cache: Arc::clone(&source.relational_plan_template_cache),
            optimizer_planning_cache: Arc::clone(&source.optimizer_planning_cache),
            slow_query_snapshot: source.slow_query_snapshot.clone(),
            statement_summary_snapshot: source.statement_summary_snapshot.clone(),
            config: source.config.clone(),
            branch_catalog_path: source.branch_catalog_path.clone(),
            current_branch: source.current_branch.clone(),
            projection_relational: None,
            task_context,
            _pin: Arc::clone(&source._pin),
            retained_runtime: Arc::clone(&source.retained_runtime),
        })
    }
}

#[derive(Debug)]
struct ProjectionRelationalReadSnapshot {
    binding: ProjectionRelationalReadBinding,
    reader: hawdb_storage::projection_generation::ProjectionGenerationReader,
}

struct ReadStreamingExecutionContext<'a> {
    task_context: Option<&'a hawdb_core::RuntimeTaskContext>,
    external: Option<&'a mut dyn executor::ExternalReadOperator>,
    delivery: executor::StreamDelivery,
    scoring: Option<BoundScoringRequest<'a>>,
    access_control: Option<&'a QueryAccessControlContext>,
}

#[derive(Debug)]
pub struct NowledgeGraphAdapter<'a> {
    db: &'a mut Database,
}

#[derive(Debug, Default)]
struct ReaderPins {
    next_reader_id: u64,
    active_views: BTreeMap<u64, PublishedReadView>,
}

#[derive(Debug)]
struct ReaderPin {
    id: u64,
    pins: Arc<Mutex<ReaderPins>>,
}

fn configure_search_projection_changefeed(store: &mut GraphStore, config: &DatabaseConfig) {
    store.set_search_projection_primary_key_capture_limits(
        config.search_projection_relational_change_limits,
    );
    store.set_max_search_projection_change_log_entries(
        config.max_search_projection_change_log_entries,
    );
    store.set_max_search_projection_change_log_bytes(config.max_search_projection_change_log_bytes);
}

fn configure_relational_fast_paths(store: &mut GraphStore, config: &DatabaseConfig) {
    store.set_relational_monotonic_append_fast_path_enabled(
        config.relational_monotonic_append_fast_path,
    );
}

impl Default for Database {
    fn default() -> Self {
        let config = effective_database_config(DatabaseConfig::default());
        let local_qos_scheduler = LocalQosScheduler::new(config.local_qos_policy);
        let mut store = GraphStore::default();
        configure_search_projection_changefeed(&mut store, &config);
        configure_relational_fast_paths(&mut store, &config);
        Self {
            runtime: runtime_cell::BranchRuntimeCell::admitted(
                runtime_cell::AdmittedBranchRuntime {
                    catalog: Catalog::default(),
                    branch_selection: None,
                    projection_consumers: search_projection_consumer::ConsumerRegistry::load(
                        store.search_projection_registry_root(),
                        store.search_projection_database_identity(),
                    ),
                    store,
                    optimizer: optimizer_from_database_config(&config),
                    plan_cache: Arc::new(SharedState::new(PlanCache::new(
                        config.max_plan_cache_entries,
                    ))),
                    relational_plan_template_cache:
                        relational_plan_template_cache_from_database_config(&config),
                    optimizer_planning_cache: Arc::new(SharedState::new(
                        OptimizerPlanningCache::default(),
                    )),
                    reader_pins: Arc::new(Mutex::new(ReaderPins::default())),
                },
            ),
            project_root_path: None,

            durability: DurabilityPolicy::default(),

            slow_query_log: Arc::new(SharedState::new(system_sql::SlowQueryLog::new(
                config.slow_query_log_capacity,
            ))),
            statement_summary: Arc::new(SharedState::new(system_sql::StatementSummary::new(
                config.statement_summary_capacity,
            ))),
            config,
            local_qos_scheduler,
            system_variables: QuerySystemVariables::default(),

            derived_artifact_jobs: hawdb_artifact::DerivedArtifactJobQueue::default(),
            derived_artifact_root_pins: BTreeMap::new(),
            telemetry: None,
            runtime_governor: None,
            retained_runtime: Arc::new(retained_query::RetainedRuntime::default()),
            branch_create_recovery: None,
        }
    }
}

impl Database {
    /// Returns the durable, owner-scoped projection generation catalog.
    ///
    /// Candidate construction and publication stay on the embedded library
    /// path. In-memory databases do not expose a durable generation catalog.
    pub fn projection_generation_store(
        &self,
    ) -> Result<hawdb_storage::projection_generation::ProjectionGenerationStore> {
        self.runtime.get()?.store.projection_generation_store()
    }

    /// Encodes a row for a relational projection table using the durable
    /// PostgreSQL schema registered in this database.
    pub fn encode_projection_relational_row(
        &self,
        table: &str,
        row: hawdb_storage::relational::RelationalRow,
    ) -> Result<hawdb_storage::projection_generation::ProjectionGenerationMember> {
        let runtime = self.runtime.get()?;
        let schema = runtime
            .store
            .relational_state()
            .table_schema(table)
            .ok_or_else(|| {
                HawDBError::Semantic(format!(
                    "projection relational table {table} has no durable PostgreSQL schema"
                ))
            })?;
        hawdb_storage::projection_generation::encode_projection_relational_member(schema, row)
            .map_err(|error| match error {
                hawdb_storage::projection_generation::ProjectionGenerationError::Corruption(
                    message,
                ) => HawDBError::StorageIntegrity(message),
                error => HawDBError::Execution(error.to_string()),
            })
    }

    pub fn new() -> Self {
        Self::default()
    }

    /// Identifies the actual admitted selected runtime, independently of epoch.
    pub fn instance_identity(&self) -> Result<DatabaseInstanceIdentity> {
        self.runtime.get()?.store.ensure_usable()?;
        Ok(self.runtime.instance_identity())
    }

    pub fn into_concurrent(self) -> ConcurrentDatabase {
        ConcurrentDatabase::new(self)
    }

    pub fn new_with_config(config: DatabaseConfig) -> Self {
        let config = effective_database_config(config);
        let local_qos_scheduler = LocalQosScheduler::new(config.local_qos_policy);
        let mut store = GraphStore::default();
        configure_search_projection_changefeed(&mut store, &config);
        configure_relational_fast_paths(&mut store, &config);
        let optimizer = optimizer_from_database_config(&config);
        Self {
            runtime: runtime_cell::BranchRuntimeCell::admitted(
                runtime_cell::AdmittedBranchRuntime {
                    catalog: Catalog::default(),
                    branch_selection: None,
                    projection_consumers: search_projection_consumer::ConsumerRegistry::load(
                        store.search_projection_registry_root(),
                        store.search_projection_database_identity(),
                    ),
                    store,
                    optimizer,
                    plan_cache: Arc::new(SharedState::new(PlanCache::new(
                        config.max_plan_cache_entries,
                    ))),
                    relational_plan_template_cache:
                        relational_plan_template_cache_from_database_config(&config),
                    optimizer_planning_cache: Arc::new(SharedState::new(
                        OptimizerPlanningCache::default(),
                    )),
                    reader_pins: Arc::new(Mutex::new(ReaderPins::default())),
                },
            ),
            project_root_path: None,

            durability: DurabilityPolicy::default(),

            slow_query_log: Arc::new(SharedState::new(system_sql::SlowQueryLog::new(
                config.slow_query_log_capacity,
            ))),
            statement_summary: Arc::new(SharedState::new(system_sql::StatementSummary::new(
                config.statement_summary_capacity,
            ))),
            config,
            local_qos_scheduler,
            system_variables: QuerySystemVariables::default(),

            derived_artifact_jobs: hawdb_artifact::DerivedArtifactJobQueue::default(),
            derived_artifact_root_pins: BTreeMap::new(),
            telemetry: None,
            runtime_governor: None,
            retained_runtime: Arc::new(retained_query::RetainedRuntime::default()),
            branch_create_recovery: None,
        }
    }

    /// Returns the selected branch's committed epoch, admitting default main
    /// first when this handle has only opened project metadata.
    pub fn commit_epoch(&self) -> Result<u64> {
        Ok(self.runtime.get()?.store.commit_epoch())
    }

    pub fn published_read_view(&self) -> Result<PublishedReadView> {
        Ok(self.runtime.get()?.store.published_read_view())
    }

    pub(crate) fn begin_wal_sync_group(&mut self) -> Result<bool> {
        self.runtime.get_mut()?.store.begin_wal_sync_group()
    }

    pub(crate) fn finish_wal_sync_group(&mut self) -> Result<crate::store::WalSyncGroupFlush> {
        self.runtime.get_mut()?.store.finish_wal_sync_group()
    }

    pub(crate) fn search_projection_changefeed_status(
        &self,
    ) -> Result<hawdb_storage::projection::SearchProjectionChangefeedStatus> {
        Ok({
            self.runtime
                .get()?
                .store
                .search_projection_changefeed_status()
        })
    }

    pub fn search_projection_changefeed_readiness(
        &self,
        search_index: &SearchIndex,
        require_restart_recoverable: bool,
        max_operations: Option<usize>,
    ) -> Result<hawdb_storage::projection::SearchProjectionChangefeedReadiness> {
        Ok({
            let freshness = search_index.projection_freshness();
            self.runtime
                .get()?
                .store
                .search_projection_changefeed_status()
                .readiness_after(
                    freshness.source_graph_commit_epoch,
                    freshness.durable_source_graph_commit_epoch,
                    require_restart_recoverable,
                    max_operations,
                )
        })
    }

    /// Opens a persistent project with power-loss-safe writes by default.
    ///
    /// Published projects open metadata without taking a branch writer lease.
    /// Data access lazily recovers default main; busy, damaged, or resource-
    /// rejected admission returns an error and can be retried. Catalog SQL and
    /// `USE BRANCH` can run before main is admitted. Successful metadata open
    /// does not certify that the selected branch's data is ready. Writable opens
    /// also attempt bounded pending-child recovery; inspect
    /// `branch_create_recovery_report` for retained, busy or unattempted receipts.
    ///
    /// The first writable open publishes the project's main branch from the
    /// complete legacy state. Read-only legacy opens do not adopt that layout.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_durability(path, DurabilityPolicy::default())
    }

    pub fn open_with_config(path: impl AsRef<Path>, config: DatabaseConfig) -> Result<Self> {
        Self::open_with_durability_and_config(path, DurabilityPolicy::default(), config)
    }

    pub fn open_with_durability(
        path: impl AsRef<Path>,
        durability: DurabilityPolicy,
    ) -> Result<Self> {
        Self::open_with_durability_and_config(path, durability, DatabaseConfig::default())
    }

    pub fn open_with_durability_and_config(
        path: impl AsRef<Path>,
        durability: DurabilityPolicy,
        config: DatabaseConfig,
    ) -> Result<Self> {
        if cfg!(all(target_arch = "wasm32", target_os = "unknown")) {
            return Err(HawDBError::Storage(
                "persistent storage is unavailable in browser WASM; use Database::new()".into(),
            ));
        }
        let config = effective_database_config(config);
        if config.read_only && !hawdb_storage::file_io::try_exists(path.as_ref())? {
            return Err(HawDBError::Storage(format!(
                "read-only database path does not exist: {}",
                path.as_ref().display()
            )));
        }
        let local_qos_scheduler = LocalQosScheduler::new(config.local_qos_policy);
        let mut catalog = Catalog::default();
        let files = if config.read_only {
            hawdb_storage::file_descriptors::ProjectFileDescriptors::acquire_existing_with_capacity_limit(
                path.as_ref(),
                config.max_open_files,
                config.strict_capacity_limit,
            )?
        } else {
            hawdb_storage::file_descriptors::ProjectFileDescriptors::acquire_with_capacity_limit(
                path.as_ref(),
                config.max_open_files,
                config.strict_capacity_limit,
            )?
        };
        let project_root_path = files.root().to_path_buf();
        let _descriptor_operation = files.io_context().internal_operation()?;
        if matches!(
            hawdb_storage::branch_project::inspect_project_manifest(files.root())?,
            hawdb_storage::branch_project::ProjectManifest::Branch(_)
        ) {
            let metadata = hawdb_storage::branch_project::ProjectMetadata::from_files(files)?;
            return Self::from_project_metadata(metadata, durability, config);
        }
        let replay_config = config.wal_replay_config();
        let mut store = if config.read_only {
            GraphStore::open_read_only_with_durability_and_replay_config(
                path,
                &mut catalog,
                durability,
                replay_config,
            )?
        } else {
            GraphStore::open_with_durability_and_replay_config(
                path,
                &mut catalog,
                durability,
                replay_config,
            )?
        };
        let project_root_path = hawdb_storage::file_io::canonicalize(&project_root_path)?;
        configure_search_projection_changefeed(&mut store, &config);
        configure_relational_fast_paths(&mut store, &config);
        let mut database = Self {
            runtime: runtime_cell::BranchRuntimeCell::admitted(
                runtime_cell::AdmittedBranchRuntime {
                    catalog,
                    branch_selection: None,
                    projection_consumers: search_projection_consumer::ConsumerRegistry::load(
                        store.search_projection_registry_root(),
                        store.search_projection_database_identity(),
                    ),
                    store,
                    optimizer: optimizer_from_database_config(&config),
                    plan_cache: Arc::new(SharedState::new(PlanCache::new(
                        config.max_plan_cache_entries,
                    ))),
                    relational_plan_template_cache:
                        relational_plan_template_cache_from_database_config(&config),
                    optimizer_planning_cache: Arc::new(SharedState::new(
                        OptimizerPlanningCache::default(),
                    )),
                    reader_pins: Arc::new(Mutex::new(ReaderPins::default())),
                },
            ),
            project_root_path: Some(project_root_path),

            durability,

            slow_query_log: Arc::new(SharedState::new(system_sql::SlowQueryLog::new(
                config.slow_query_log_capacity,
            ))),
            statement_summary: Arc::new(SharedState::new(system_sql::StatementSummary::new(
                config.statement_summary_capacity,
            ))),
            config,
            local_qos_scheduler,
            system_variables: QuerySystemVariables::default(),

            derived_artifact_jobs: hawdb_artifact::DerivedArtifactJobQueue::default(),
            derived_artifact_root_pins: BTreeMap::new(),
            telemetry: None,
            runtime_governor: None,
            retained_runtime: Arc::new(retained_query::RetainedRuntime::default()),
            branch_create_recovery: None,
        };
        if database.config.read_only {
            database.apply_engine_system_schema()?;
            return Ok(database);
        }
        let proposed = hawdb_storage::branch_project::ProjectSelector::new(
            hawdb_storage::branch_catalog::BranchId::new(hawdb_core::generate_uuidv7()?)
                .map_err(HawDBError::from_storage_error)?,
            hawdb_storage::branch_catalog::BranchId::new(hawdb_core::generate_uuidv7()?)
                .map_err(HawDBError::from_storage_error)?,
        )?;
        let selector = database
            .runtime
            .get()?
            .store
            .reserve_branch_project_identity(proposed)?;
        let head_path = database.branch_head_path(selector.main_branch_id().as_uuid())?;
        // Check uncertain bootstrap publication before an implicit schema
        // upgrade or row checkpoint can advance the retained legacy history.
        if !hawdb_storage::file_io::try_exists(&head_path)? {
            let catalog_path = database.branch_catalog_path()?;
            if hawdb_storage::file_io::try_exists(&catalog_path)? {
                let catalog = hawdb_storage::branch_catalog::read_catalog(&catalog_path)
                    .map_err(HawDBError::from_storage_error)?;
                let main = catalog
                    .branches
                    .iter()
                    .find(|record| record.id == selector.main_branch_id())
                    .ok_or_else(|| {
                        HawDBError::StorageIntegrity("bootstrap main is absent".into())
                    })?;
                if main.base_root_digest.is_some() {
                    return Err(HawDBError::StorageIntegrity(
                        "initialized main branch is missing its head".into(),
                    ));
                }
            }
        }
        database.complete_required_relational_row_checkpoint("writable recovery")?;
        database.apply_engine_system_schema()?;
        if !hawdb_storage::file_io::try_exists(&head_path)? {
            let watermark = database.storage_reclamation_watermark()?;
            // Generation zero has no checkpoint. A clean bootstrap retry must
            // reuse its checkpoint and private WAL generation: advancing it
            // here could bypass a damaged WAL left by interrupted publication.
            if watermark
                .checkpoint_epoch
                .is_none_or(|generation| generation == 0)
                || watermark.checkpoint_commit_epoch != Some(watermark.current_commit_epoch)
                || database.relational_row_schema_checkpoint_required()?
            {
                database.checkpoint()?;
            }
        }
        database.initialize_main_branch(
            selector.project_id().as_uuid(),
            selector.main_branch_id().as_uuid(),
        )?;
        database
            .runtime
            .get()?
            .store
            .publish_branch_project_selector(selector)?;
        // Release the old directory/WAL owner before admitting any data runtime.
        // Its immutable closure and old files remain durable recovery evidence.
        let config = database.config.clone();
        drop(database);
        let metadata = hawdb_storage::branch_project::ProjectMetadata::from_files(files)?;
        Self::from_project_metadata(metadata, durability, config)
    }

    fn from_project_metadata(
        metadata: hawdb_storage::branch_project::ProjectMetadata,
        durability: DurabilityPolicy,
        config: DatabaseConfig,
    ) -> Result<Self> {
        let branch_create_recovery = if config.read_only {
            None
        } else {
            Some(metadata.recover_pending_creates(config.branch_create_recovery_limits)?)
        };
        let files = metadata.file_descriptors().clone();
        let project_root_path = Some(files.root().to_path_buf());
        let project = metadata.into_capture();
        let local_qos_scheduler = LocalQosScheduler::new(config.local_qos_policy);
        Ok(Self {
            runtime: runtime_cell::BranchRuntimeCell::deferred(
                runtime_cell::DeferredBranchAdmission {
                    config: config.clone(),
                    durability,
                    scheduler: local_qos_scheduler.clone(),
                    telemetry: None,
                    governor: None,
                },
                project,
            ),
            project_root_path,
            durability,
            slow_query_log: Arc::new(SharedState::new(system_sql::SlowQueryLog::new(
                config.slow_query_log_capacity,
            ))),
            statement_summary: Arc::new(SharedState::new(system_sql::StatementSummary::new(
                config.statement_summary_capacity,
            ))),
            config,
            local_qos_scheduler,
            system_variables: QuerySystemVariables::default(),
            derived_artifact_jobs: hawdb_artifact::DerivedArtifactJobQueue::default(),
            derived_artifact_root_pins: BTreeMap::new(),
            telemetry: None,
            runtime_governor: None,
            retained_runtime: Arc::new(retained_query::RetainedRuntime::default()),
            branch_create_recovery,
        })
    }

    pub fn config(&self) -> &DatabaseConfig {
        &self.config
    }

    pub fn automatic_checkpoint_report(&self) -> Result<Option<AutomaticCheckpointReport>> {
        self.runtime.automatic_checkpoint_report()
    }

    /// Engine-owned descriptors and reservations shared by this canonical project.
    pub fn file_descriptor_metrics(
        &self,
    ) -> Option<hawdb_storage::file_descriptors::FileDescriptorMetrics> {
        self.runtime.file_descriptor_metrics()
    }

    /// Returns a handle to the database-owned runtime QoS scheduler.
    /// Cloned handles share the same policy, counters, and permit ownership.
    pub fn local_qos_scheduler(&self) -> LocalQosScheduler {
        self.local_qos_scheduler.clone()
    }

    pub(crate) fn runtime_capabilities(&self) -> hawdb_core::RuntimeCapabilities {
        self.config.runtime_capabilities
    }

    pub(super) fn ensure_runtime_capability(
        &self,
        capability: hawdb_core::RuntimeCapability,
    ) -> Result<()> {
        self.config.runtime_capabilities.require(capability)
    }

    pub fn set_telemetry_sink(&mut self, telemetry: Option<Arc<dyn TelemetrySink>>) {
        if let Some(governor) = &self.runtime_governor {
            governor.set_telemetry_sink(
                telemetry
                    .as_ref()
                    .map(|telemetry| runtime_telemetry_sink(telemetry.clone())),
            );
        }
        if let Some(telemetry) = &telemetry
            && let Some(runtime) = self.runtime.peek()
        {
            runtime_cell::record_recovery_telemetry(&runtime.store, telemetry.as_ref());
        }
        if let Some(mut runtime) = self.runtime.peek_mut() {
            crate::store::StoreTelemetry::set_telemetry_sink(&mut runtime.store, telemetry.clone());
        }
        if let Some(pending) = self.runtime.pending_mut() {
            pending.telemetry = telemetry.clone();
        }
        self.local_qos_scheduler.set_telemetry_sink(
            telemetry
                .as_ref()
                .map(|telemetry| qos_telemetry_sink(telemetry.clone())),
        );
        self.telemetry = telemetry;
    }

    pub fn telemetry_sink_configured(&self) -> bool {
        self.telemetry.is_some()
    }

    pub fn operations_telemetry_readiness(
        &self,
        search_index: Option<&SearchIndex>,
    ) -> OperationsTelemetryReadiness {
        operations_telemetry_readiness(
            self.telemetry_sink_configured(),
            search_index
                .map(SearchIndex::telemetry_sink_configured)
                .unwrap_or(false),
        )
    }

    fn local_qos_scheduler_for_work(&self) -> LocalQosScheduler {
        self.local_qos_scheduler()
    }

    pub fn system_variables(&self) -> &QuerySystemVariables {
        &self.system_variables
    }

    #[cfg(test)]
    fn query_read_only_with_params_bounded(
        &self,
        cypher_text: &str,
        parameters: &BTreeMap<String, Value>,
        max_rows: Option<usize>,
    ) -> Result<QueryOutput> {
        let started = hawdb_core::time::Instant::now();
        let statement = cypher::parse(cypher_text)?;
        let body = statement_body(&statement);
        let query_result = (|| {
            if matches!(body, cypher::Statement::Checkpoint) {
                reject_transaction_control_parameters("CHECKPOINT", parameters)?;
                return Err(HawDBError::Execution(
                    "CHECKPOINT is not allowed inside a read-only query runtime".to_string(),
                ));
            }
            if matches!(body, cypher::Statement::SetSystemVariable(_)) {
                reject_system_variable_parameters(parameters)?;
                return Err(HawDBError::Execution(
                    "SET system variable is not allowed inside a read-only query runtime"
                        .to_string(),
                ));
            }
            query_work_request_for_statement(&self.system_variables, &statement)?;
            let optimized = self.optimized_query_plan_for_request(
                cypher_text,
                &statement,
                parameters,
                None,
                PlanTraceMode::Template,
                None,
            )?;
            if executor::is_mutation_plan(&optimized.physical_plan)? {
                return Err(HawDBError::Execution(
                    "read-only query runtime must not execute a mutation".to_string(),
                ));
            }
            let mut catalog = self.runtime.get()?.catalog.clone();
            let mut store = self.runtime.get()?.store.snapshot();
            let parameters = BTreeMap::new();
            let memory = executor::ExecutionMemoryConfig::default();
            let mut external = executor::NoExternalReadOperator;
            let profiled = executor::execute_with_request(
                executor::ExecutionRequest::new(&optimized.physical_plan, &parameters, &memory)
                    .with_output_limits(max_rows, None),
                executor::ExecutionResources::new(&mut catalog, &mut store, &mut external),
            )?;
            Ok(QueryOutput {
                rows: profiled.rows,
            })
        })();
        self.runtime
            .get()?
            .store
            .poison_on_storage_error(&query_result);
        self.record_statement_execution(
            "cypher",
            cypher_text,
            statement_kind(body),
            started,
            query_result.as_ref(),
            StatementExecutionContext::default(),
        );
        query_result
    }

    /// Begins a transaction after recovering the selected branch. Admission
    /// failure leaves the project handle available for retry or SQL selection.
    pub fn begin_transaction(&mut self) -> Result<DatabaseTransaction<'_>> {
        self.begin_transaction_inner(None)
    }

    /// Begins a transaction with explicit isolation and retained-evidence limits.
    pub fn begin_transaction_with_isolation(
        &mut self,
        isolation: TransactionIsolation,
        limits: TransactionReadDependencyLimits,
    ) -> Result<DatabaseTransaction<'_>> {
        self.runtime.get_mut()?;
        let state =
            DatabaseTransactionState::from_database_with_isolation(self, isolation, limits)?;
        let runtime = DatabaseTransactionRuntime::from_database(self)?;
        Ok(DatabaseTransaction {
            db: self,
            runtime,
            state,
            task_context: None,
        })
    }

    /// Begins an explicitly isolated transaction with cooperative host cancellation.
    pub fn begin_transaction_with_isolation_and_context(
        &mut self,
        isolation: TransactionIsolation,
        limits: TransactionReadDependencyLimits,
        task_context: &hawdb_core::RuntimeTaskContext,
    ) -> Result<DatabaseTransaction<'_>> {
        query_runtime::query_runtime_checkpoint(Some(task_context))?;
        let mut transaction = self.begin_transaction_with_isolation(isolation, limits)?;
        transaction.task_context = Some(task_context.clone());
        Ok(transaction)
    }

    pub fn begin_transaction_with_context(
        &mut self,
        task_context: &hawdb_core::RuntimeTaskContext,
    ) -> Result<DatabaseTransaction<'_>> {
        self.begin_transaction_inner(Some(task_context.clone()))
    }

    fn begin_transaction_inner(
        &mut self,
        task_context: Option<hawdb_core::RuntimeTaskContext>,
    ) -> Result<DatabaseTransaction<'_>> {
        let (runtime, state) = self.transaction_parts()?;
        Ok(DatabaseTransaction {
            db: self,
            runtime,
            state,
            task_context,
        })
    }

    /// Builds the owned runtime and statement state for one transaction.
    /// Embedded hosts that must own the transaction object (rather than
    /// borrow `Database`) assemble the same parts and commit through
    /// [`commit_database_transaction_state`].
    pub(super) fn transaction_parts(
        &mut self,
    ) -> Result<(DatabaseTransactionRuntime, DatabaseTransactionState)> {
        self.runtime.get_mut()?;
        Ok((
            DatabaseTransactionRuntime::from_database(self)?,
            DatabaseTransactionState::from_database(self)?,
        ))
    }

    pub fn session(&mut self) -> DatabaseSession<'_> {
        let system_variables = self.system_variables.clone();
        DatabaseSession {
            config: SessionConfig::default(),
            db: self,
            graph_transaction: None,
            transaction_runtime: None,
            system_variables,
        }
    }

    #[cfg(test)]
    pub(crate) fn read_snapshot(&self) -> Result<DatabaseReadSnapshot> {
        Ok(DatabaseReadSnapshot(self.begin_read_transaction()?))
    }

    fn read_snapshot_without_observations(&self) -> Result<DatabaseReadSnapshot> {
        Ok(DatabaseReadSnapshot(
            self.read_transaction_state(None, None, None)?,
        ))
    }

    /// Pins the selected branch's schema and data after successful admission.
    pub fn begin_read_transaction(&self) -> Result<DatabaseReadTransaction> {
        self.begin_read_transaction_inner(None, None, None)
    }

    pub(crate) fn begin_read_transaction_with_shared_plan_cache(
        &self,
    ) -> Result<DatabaseReadTransaction> {
        self.begin_read_transaction_inner(
            None,
            None,
            Some(Arc::clone(&self.runtime.get()?.plan_cache)),
        )
    }

    pub fn begin_read_transaction_with_context(
        &self,
        task_context: &hawdb_core::RuntimeTaskContext,
    ) -> Result<DatabaseReadTransaction> {
        self.begin_read_transaction_inner(None, Some(task_context.clone()), None)
    }

    /// Pins both the database read view and one active projection generation.
    /// PostgreSQL SQL issued through the returned transaction reads the bound
    /// tables from that generation and all other tables from the same database
    /// snapshot.
    pub fn begin_projection_read_transaction(
        &self,
        binding: ProjectionRelationalReadBinding,
    ) -> Result<DatabaseReadTransaction> {
        let reader = self
            .projection_generation_store()?
            .open_active(binding.projection(), binding.owner_key())
            .map_err(|error| HawDBError::Storage(error.to_string()))?;
        if reader.manifest().begin.projection_version != binding.projection_version() {
            return Err(HawDBError::StorageIntegrity(format!(
                "projection {} version {} does not match required version {}",
                binding.projection(),
                reader.manifest().begin.projection_version,
                binding.projection_version()
            )));
        }
        for table in binding.tables() {
            if self
                .runtime
                .get()?
                .store
                .relational_state()
                .table_schema(table)
                .is_none()
            {
                return Err(HawDBError::Semantic(format!(
                    "projection relational table {table} has no durable PostgreSQL schema"
                )));
            }
            let canonical_rows = self
                .runtime
                .get()?
                .store
                .relational_state()
                .row_count(table);
            if canonical_rows != 0 {
                return Err(HawDBError::StorageIntegrity(format!(
                    "projection relational table {table} contains {canonical_rows} canonical rows"
                )));
            }
        }
        self.begin_read_transaction_inner(
            Some(ProjectionRelationalReadSnapshot { binding, reader }),
            None,
            None,
        )
    }

    fn begin_read_transaction_inner(
        &self,
        projection_relational: Option<ProjectionRelationalReadSnapshot>,
        task_context: Option<hawdb_core::RuntimeTaskContext>,
        plan_cache: Option<Arc<SharedState<PlanCache>>>,
    ) -> Result<DatabaseReadTransaction> {
        let mut snapshot =
            self.read_transaction_state(projection_relational, task_context, plan_cache)?;
        snapshot.slow_query_snapshot = self.slow_query_log.borrow().snapshot();
        snapshot.statement_summary_snapshot = self.statement_summary.borrow().snapshot();
        Ok(snapshot)
    }

    fn read_transaction_state(
        &self,
        projection_relational: Option<ProjectionRelationalReadSnapshot>,
        task_context: Option<hawdb_core::RuntimeTaskContext>,
        plan_cache: Option<Arc<SharedState<PlanCache>>>,
    ) -> Result<DatabaseReadTransaction> {
        let runtime = self.runtime.get()?;
        Ok({
            let (published_read_view, pin) = Self::pin_runtime_read_view(&runtime);
            DatabaseReadTransaction {
                session_config: SessionConfig::default(),
                system_variables: self.system_variables.clone(),
                catalog: runtime.catalog.clone(),
                store: runtime.store.snapshot_for_read(),
                published_read_view,
                optimizer: runtime.optimizer.clone(),
                plan_cache: plan_cache.map_or_else(
                    || {
                        ReadPlanCache::Private(SharedState::new(PlanCache::new(
                            self.config.max_plan_cache_entries,
                        )))
                    },
                    ReadPlanCache::Shared,
                ),
                relational_plan_template_cache: Arc::clone(&runtime.relational_plan_template_cache),
                optimizer_planning_cache: Arc::clone(&runtime.optimizer_planning_cache),
                slow_query_snapshot: Vec::new(),
                statement_summary_snapshot: Vec::new(),
                config: self.config.clone(),
                branch_catalog_path: self.branch_catalog_path_from_runtime(&runtime).ok(),
                current_branch: self.current_branch_from_runtime(&runtime),
                projection_relational,
                task_context,
                _pin: Arc::new(pin),
                retained_runtime: Arc::clone(&self.retained_runtime),
            }
        })
    }

    fn pin_runtime_read_view(
        runtime: &runtime_cell::AdmittedBranchRuntime,
    ) -> (PublishedReadView, ReaderPin) {
        let published_read_view = runtime.store.published_read_view();
        let mut pins = runtime
            .reader_pins
            .lock()
            .expect("database reader pins lock should not be poisoned");
        let id = pins.next_reader_id;
        pins.next_reader_id += 1;
        pins.active_views.insert(id, published_read_view);
        (
            published_read_view,
            ReaderPin::new(id, Arc::clone(&runtime.reader_pins)),
        )
    }

    pub fn explain_query(&self, cypher_text: &str) -> Result<ExplainOutput> {
        self.explain_query_with_params(cypher_text, &BTreeMap::new())
    }

    pub fn explain_query_with_params(
        &self,
        cypher_text: &str,
        parameters: &BTreeMap<String, Value>,
    ) -> Result<ExplainOutput> {
        self.explain_query_with_params_access_control_internal(cypher_text, parameters, None)
    }

    pub fn explain_query_with_params_access_control(
        &self,
        cypher_text: &str,
        parameters: &BTreeMap<String, Value>,
        access_control: QueryAccessControlContext,
    ) -> Result<ExplainOutput> {
        self.explain_query_with_params_access_control_internal(
            cypher_text,
            parameters,
            Some(access_control),
        )
    }

    fn explain_query_with_params_access_control_internal(
        &self,
        cypher_text: &str,
        parameters: &BTreeMap<String, Value>,
        access_control: Option<QueryAccessControlContext>,
    ) -> Result<ExplainOutput> {
        self.runtime.get()?.store.ensure_usable()?;
        let statement = cypher::parse(cypher_text)?;
        let work_request = query_work_request_for_statement(&self.system_variables, &statement)?;
        let optimized = self.optimized_explain_query_plan_with_access_control(
            cypher_text,
            &statement,
            parameters,
            access_control.as_ref(),
        )?;
        Ok(ExplainOutput {
            physical_plan: optimized.physical_plan,
            trace: optimized.trace,
            work_request,
            plan_cache_lookup: optimized.plan_cache_lookup,
            statement_kind: statement_kind(statement_body(&statement)),
        })
    }

    pub fn explain_analyze_query(&mut self, cypher_text: &str) -> Result<ExplainAnalyzeOutput> {
        self.explain_analyze_query_with_params(cypher_text, &BTreeMap::new())
    }

    pub fn explain_analyze_query_with_params(
        &mut self,
        cypher_text: &str,
        parameters: &BTreeMap<String, Value>,
    ) -> Result<ExplainAnalyzeOutput> {
        self.explain_analyze_query_with_params_access_control_internal(
            cypher_text,
            parameters,
            None,
        )
    }

    pub fn explain_analyze_query_with_params_access_control(
        &mut self,
        cypher_text: &str,
        parameters: &BTreeMap<String, Value>,
        access_control: QueryAccessControlContext,
    ) -> Result<ExplainAnalyzeOutput> {
        self.explain_analyze_query_with_params_access_control_internal(
            cypher_text,
            parameters,
            Some(access_control),
        )
    }

    fn explain_analyze_query_with_params_access_control_internal(
        &mut self,
        cypher_text: &str,
        parameters: &BTreeMap<String, Value>,
        access_control: Option<QueryAccessControlContext>,
    ) -> Result<ExplainAnalyzeOutput> {
        self.runtime.get()?.store.ensure_usable()?;
        let statement = cypher::parse(cypher_text)?;
        let work_request = query_work_request_for_statement(&self.system_variables, &statement)?;
        let optimized = self.optimized_explain_query_plan_with_access_control(
            cypher_text,
            &statement,
            parameters,
            access_control.as_ref(),
        )?;
        if executor::requires_write_access(&optimized.physical_plan) {
            return Err(HawDBError::Execution(
                "EXPLAIN ANALYZE only supports read queries".to_string(),
            ));
        }
        let mut external = executor::NoExternalReadOperator;
        let profiled = {
            self.runtime.get_read()?.execute(
                executor::ExecutionRequest::new(
                    &optimized.physical_plan,
                    parameters,
                    &self.config.execution_memory,
                )
                .with_output_limits(
                    self.config.read_result_row_limit(),
                    self.config.max_read_result_payload_bytes,
                ),
                &mut external,
            )
        };
        self.runtime.get()?.store.poison_on_storage_error(&profiled);
        let profiled = profiled?;
        Ok(ExplainAnalyzeOutput {
            output: QueryOutput {
                rows: profiled.rows,
            },
            execution_profile: profiled.profile,
            physical_plan: optimized.physical_plan,
            trace: optimized.trace,
            work_request,
            plan_cache_lookup: optimized.plan_cache_lookup,
            statement_kind: statement_kind(statement_body(&statement)),
        })
    }

    pub fn plan_cache_stats(&self) -> Result<PlanCacheStats> {
        Ok(self.runtime.get()?.plan_cache.borrow().stats())
    }

    pub fn relational_plan_template_cache_stats(&self) -> Result<PlanCacheStats> {
        Ok(self.runtime.get()?.relational_plan_template_cache.stats())
    }

    fn optimized_explain_query_plan(
        &self,
        cypher_text: &str,
        statement: &cypher::Statement,
        parameters: &BTreeMap<String, Value>,
    ) -> Result<OptimizedQueryPlan> {
        self.optimized_explain_query_plan_with_access_control(
            cypher_text,
            statement,
            parameters,
            None,
        )
    }

    fn optimized_explain_query_plan_with_access_control(
        &self,
        cypher_text: &str,
        statement: &cypher::Statement,
        parameters: &BTreeMap<String, Value>,
        access_control: Option<&QueryAccessControlContext>,
    ) -> Result<OptimizedQueryPlan> {
        self.optimized_query_plan_with_access_control_and_trace_mode(
            cypher_text,
            statement,
            parameters,
            access_control,
            PlanTraceMode::Bound,
        )
    }

    fn optimized_query_plan_with_access_control_and_trace_mode(
        &self,
        cypher_text: &str,
        statement: &cypher::Statement,
        parameters: &BTreeMap<String, Value>,
        access_control: Option<&QueryAccessControlContext>,
        trace_mode: PlanTraceMode,
    ) -> Result<OptimizedQueryPlan> {
        self.optimized_query_plan_for_request(
            cypher_text,
            statement,
            parameters,
            access_control,
            trace_mode,
            None,
        )
    }

    fn optimized_query_plan_for_request(
        &self,
        cypher_text: &str,
        statement: &cypher::Statement,
        parameters: &BTreeMap<String, Value>,
        access_control: Option<&QueryAccessControlContext>,
        trace_mode: PlanTraceMode,
        scoring: Option<BoundScoringRequest<'_>>,
    ) -> Result<OptimizedQueryPlan> {
        let optimizer_search =
            query_statement_variables_for_statement(&self.system_variables, statement)?
                .optimizer_search;
        let cache_mode = if optimizer_search != OptimizerSearchDirective::Auto {
            PlanCacheMode::Bypass(PlanCacheBypassReason::OptimizerDirective)
        } else if statement_uses_plan_cache(statement) {
            PlanCacheMode::Use
        } else {
            PlanCacheMode::Bypass(PlanCacheBypassReason::StatementNotCacheable)
        };
        optimized_query_plan_for(
            cypher_text,
            statement,
            parameters,
            cache_mode,
            trace_mode,
            PlanCacheContext {
                catalog: &self.runtime.get()?.catalog,
                store: &self.runtime.get()?.store,
                optimizer: &self.runtime.get()?.optimizer,
                config: &self.config,
                cache: &self.runtime.get()?.plan_cache,
                planning_cache: &self.runtime.get()?.optimizer_planning_cache,
                access_control,
                optimizer_search,
                scoring,
            },
        )
    }

    /// Completes a caller-triggered checkpoint of a writable persistent database.
    ///
    /// This synchronous call does not require automatic checkpoint pressure or
    /// background-maintenance admission. It remains available when background
    /// maintenance is disabled, including with `default-features = false`.
    /// Success follows checkpoint publication and its durability barrier; merely
    /// scheduling background work is not success. Read-only handles return an
    /// error. An in-memory database has no persistent checkpoint to publish.
    ///
    /// The call coordinates with the automatic owner and preserves active read
    /// snapshots. Hosts should run this blocking operation on a suitable thread
    /// and provide their own admission boundary for caller-triggered work.
    pub fn checkpoint(&mut self) -> Result<()> {
        self.checkpoint_internal(None)
    }

    pub fn append_transaction(&mut self, transaction: AppendTransaction) -> Result<u64> {
        self.append_transaction_with_result(transaction)
            .map(|result| result.commit_epoch)
    }

    pub fn append_transaction_with_result(
        &mut self,
        transaction: AppendTransaction,
    ) -> Result<AppendCommitResult> {
        self.ensure_writable()?;
        let summary = {
            let mut branch_runtime_access = self.runtime.get_mut()?;
            let branch_runtime = &mut *branch_runtime_access;
            branch_runtime.store.commit_kernel_write_batch(
                &mut branch_runtime.catalog,
                KernelWriteBatch {
                    append: transaction,
                    ..KernelWriteBatch::default()
                },
                self.config.mutation_limits,
            )
        }?;
        Ok(AppendCommitResult {
            commit_epoch: self.runtime.get()?.store.commit_epoch(),
            mutations: summary.append_mutation_outcomes,
        })
    }

    pub fn commit_kernel_write_batch(
        &mut self,
        batch: KernelWriteBatch,
    ) -> Result<MutationSummary> {
        self.ensure_writable()?;
        {
            let mut branch_runtime_access = self.runtime.get_mut()?;
            let branch_runtime = &mut *branch_runtime_access;
            branch_runtime.store.commit_kernel_write_batch(
                &mut branch_runtime.catalog,
                batch,
                self.config.mutation_limits,
            )
        }
    }

    pub fn read_append_partition(
        &self,
        table: &str,
        partition: &hawdb_storage::relational::RelationalKey,
        after: Option<&hawdb_storage::relational::RelationalKey>,
        max_rows: usize,
    ) -> Result<AppendSegmentReadOutput> {
        let max_rows = self
            .config
            .read_result_row_limit()
            .map_or(max_rows, |configured| configured.min(max_rows));
        let max_payload_bytes = self
            .config
            .max_read_result_payload_bytes
            .unwrap_or(DEFAULT_MAX_READ_RESULT_PAYLOAD_BYTES);
        self.read_append_partition_bounded(table, partition, after, max_rows, max_payload_bytes)
    }

    pub fn read_append_partition_bounded(
        &self,
        table: &str,
        partition: &hawdb_storage::relational::RelationalKey,
        after: Option<&hawdb_storage::relational::RelationalKey>,
        max_rows: usize,
        max_payload_bytes: usize,
    ) -> Result<AppendSegmentReadOutput> {
        let max_rows = self
            .config
            .read_result_row_limit()
            .map_or(max_rows, |configured| configured.min(max_rows));
        let max_payload_bytes = self
            .config
            .max_read_result_payload_bytes
            .map_or(max_payload_bytes, |configured| {
                configured.min(max_payload_bytes)
            });
        self.runtime.get()?.store.read_append_partition_bounded(
            table,
            partition,
            after,
            max_rows,
            max_payload_bytes,
        )
    }

    /// Returns an owned schema snapshot, valid across background checkpoint
    /// adoption and after this database handle is closed.
    pub fn append_table_schema(&self, table: &str) -> Result<Option<AppendTableSchema>> {
        Ok(self
            .runtime
            .get()?
            .store
            .append_table_schema(table)
            .cloned())
    }

    pub fn append_storage_residency_report(&self) -> Result<crate::AppendStorageResidencyReport> {
        Ok(self.runtime.get()?.store.append_storage_residency_report())
    }

    /// Rewrites sparse physical row-page generations through the ordinary
    /// manifest-last checkpoint boundary, retaining older pinned readers.
    pub fn compact_relational_row_pages(
        &mut self,
        config: crate::store::RelationalRowPageCompactionConfig,
    ) -> Result<crate::store::RelationalRowPageCompactionReport> {
        self.compact_relational_row_pages_context(
            config,
            &hawdb_core::RuntimeTaskContext::default(),
        )
    }

    pub fn compact_relational_row_pages_context(
        &mut self,
        config: crate::store::RelationalRowPageCompactionConfig,
        task: &hawdb_core::RuntimeTaskContext,
    ) -> Result<crate::store::RelationalRowPageCompactionReport> {
        self.ensure_writable()?;
        let _suspension = self.runtime.suspend_automatic_checkpoint()?;
        let oldest_reader_epoch = self
            .runtime
            .get()?
            .reader_pins
            .lock()
            .expect("database reader pins lock should not be poisoned")
            .oldest_epoch();
        {
            let mut branch_runtime_access = self.runtime.get_mut()?;
            let branch_runtime = &mut *branch_runtime_access;
            branch_runtime.store.compact_relational_row_pages(
                &branch_runtime.catalog,
                oldest_reader_epoch,
                config,
                task,
            )
        }
    }

    /// Runs an explicitly admitted full relational-row closure scan and
    /// publishes an exact overflow root through the ordinary manifest-last
    /// checkpoint boundary. Large overflow payloads are not hydrated.
    pub fn compact_relational_overflow(
        &mut self,
        config: crate::store::RelationalOverflowCompactionConfig,
    ) -> Result<crate::store::RelationalOverflowCompactionReport> {
        self.compact_relational_overflow_context(config, &hawdb_core::RuntimeTaskContext::default())
    }

    pub fn compact_relational_overflow_context(
        &mut self,
        config: crate::store::RelationalOverflowCompactionConfig,
        task: &hawdb_core::RuntimeTaskContext,
    ) -> Result<crate::store::RelationalOverflowCompactionReport> {
        self.ensure_writable()?;
        let _suspension = self.runtime.suspend_automatic_checkpoint()?;
        let oldest_reader_epoch = self
            .runtime
            .get()?
            .reader_pins
            .lock()
            .expect("database reader pins lock should not be poisoned")
            .oldest_epoch();
        {
            let mut branch_runtime_access = self.runtime.get_mut()?;
            let branch_runtime = &mut *branch_runtime_access;
            branch_runtime.store.compact_relational_overflow(
                &branch_runtime.catalog,
                oldest_reader_epoch,
                config,
                task,
            )
        }
    }

    /// Checkpoint entry carrying an explicit pre-admitted columnar-shadow
    /// context, for callers that already hold a governor permit and
    /// extended it by [`Database::columnar_shadow_admission_bytes`]. The
    /// shadow build then draws only against the passed token — it never
    /// touches the governor, so nested admission cannot deadlock a
    /// constrained configuration.
    pub(crate) fn checkpoint_with_shadow_admission(
        &mut self,
        shadow_admission: crate::store::ColumnarShadowAdmission,
    ) -> Result<()> {
        self.checkpoint_internal(Some(shadow_admission))
    }

    /// The builder-lifetime byte reservation one shadow build needs; zero
    /// when `graph_columnar_shadow_checkpoint` is off.
    pub fn columnar_shadow_admission_bytes(&self) -> Result<u64> {
        Ok(self.runtime.get()?.store.columnar_shadow_admission_bytes())
    }

    fn checkpoint_internal(
        &mut self,
        shadow_admission: Option<crate::store::ColumnarShadowAdmission>,
    ) -> Result<()> {
        let _descriptor_operation = self.internal_descriptor_operation()?;
        self.ensure_writable()?;
        let started = hawdb_core::time::Instant::now();
        let durable = self.runtime.get()?.store.storage_recovery_report().durable;
        let prepared = self.checkpoint_source()?.prepare()?;
        let result = match prepared {
            Some(prepared) => {
                let (oldest_reader_epoch, pinned_reader_generations) = {
                    let runtime = self.runtime.get()?;
                    let pins = runtime
                        .reader_pins
                        .lock()
                        .expect("database reader pins lock should not be poisoned");
                    (pins.oldest_epoch(), pins.pinned_physical_generations())
                };
                self.runtime
                    .get_mut()?
                    .store
                    .publish_prepared_checkpoint_with_reader_generations(
                        prepared.checkpoint,
                        oldest_reader_epoch,
                        &pinned_reader_generations,
                        shadow_admission,
                    )
            }
            None => {
                self.runtime.get_mut()?.store.reclaim_version_history();
                Ok(())
            }
        };
        if durable && let Some(telemetry) = &self.telemetry {
            telemetry.record_kernel(KernelTelemetry {
                operation: KernelTelemetryOperation::Checkpoint,
                success: result.is_ok(),
                elapsed_micros: elapsed_micros(started),
                item_count: 1,
                byte_count: 0,
                fsync_micros: 0,
                generation: self.storage_reclamation_watermark()?.checkpoint_epoch,
            });
        }
        result
    }

    fn internal_descriptor_operation(
        &self,
    ) -> Result<Option<hawdb_storage::file_descriptors::DescriptorReservation>> {
        self.runtime
            .file_descriptor_context()
            .map(|context| context.internal_operation())
            .transpose()
            .map(Option::flatten)
            .map_err(Into::into)
    }

    fn complete_required_relational_row_checkpoint(&mut self, context: &str) -> Result<()> {
        if !self.relational_row_schema_checkpoint_required()? {
            return Ok(());
        }
        if self.runtime.get()?.store.wal_sync_group_active() {
            return Ok(());
        }
        let checkpoint = self.checkpoint_internal(None);
        let result = match checkpoint {
            Ok(()) if !self.relational_row_schema_checkpoint_required()? => Ok(()),
            Ok(()) => Err(HawDBError::StorageIntegrity(format!(
                "canonical relational row schema checkpoint remained required after {context}"
            ))),
            Err(error) => Err(HawDBError::StorageIntegrity(format!(
                "canonical relational row schema checkpoint failed after {context}; the durable WAL remains authoritative and writable reopen will retry: {error}"
            ))),
        };
        self.runtime.get()?.store.poison_on_storage_error(&result);
        result
    }

    fn relational_row_schema_checkpoint_required(&self) -> Result<bool> {
        Ok({
            self.runtime
                .get()?
                .store
                .relational_row_schema_checkpoint_required()
        })
    }

    pub(crate) fn checkpoint_source(&mut self) -> Result<DatabaseCheckpointSource> {
        self.ensure_writable()?;
        let suspension = self.runtime.suspend_automatic_checkpoint()?;
        Ok(DatabaseCheckpointSource {
            catalog: self.runtime.get()?.catalog.clone(),
            store: self.runtime.get()?.store.checkpoint_source(),
            suspension,
        })
    }

    pub(crate) fn publish_prepared_checkpoint(
        &mut self,
        prepared: PreparedDatabaseCheckpoint,
    ) -> Result<()> {
        let (oldest_reader_epoch, pinned_reader_generations) = {
            let runtime = self.runtime.get()?;
            let pins = runtime
                .reader_pins
                .lock()
                .expect("database reader pins lock should not be poisoned");
            (pins.oldest_epoch(), pins.pinned_physical_generations())
        };
        self.runtime
            .get_mut()?
            .store
            .publish_prepared_checkpoint_with_reader_generations(
                prepared.checkpoint,
                oldest_reader_epoch,
                &pinned_reader_generations,
                None,
            )
    }

    pub fn backup_to(&mut self, destination: impl AsRef<Path>) -> Result<StorageBackupReport> {
        self.ensure_writable()?;
        let _suspension = self.runtime.suspend_automatic_checkpoint()?;
        {
            let mut branch_runtime_access = self.runtime.get_mut()?;
            let branch_runtime = &mut *branch_runtime_access;
            branch_runtime
                .store
                .backup_to(&branch_runtime.catalog, destination)
        }
    }

    pub fn scrub_storage(&mut self) -> Result<StorageScrubReport> {
        self.runtime.get_mut()?.store.scrub_storage()
    }

    pub fn restore_backup(
        backup: impl AsRef<Path>,
        destination: impl AsRef<Path>,
    ) -> Result<StorageRestoreReport> {
        restore_storage_backup(backup, destination)
    }

    pub fn storage_reclamation_watermark(&self) -> Result<StorageReclamationWatermark> {
        Ok({
            let oldest_reader_epoch = self
                .runtime
                .get()?
                .reader_pins
                .lock()
                .expect("database reader pins lock should not be poisoned")
                .oldest_epoch();
            self.runtime
                .get()?
                .store
                .storage_reclamation_watermark(oldest_reader_epoch)
        })
    }

    pub fn storage_recovery_report(&self) -> Result<StorageRecoveryReport> {
        Ok(self.runtime.get()?.store.storage_recovery_report())
    }

    /// Inventories regular files in the selected branch's durable artifact
    /// directory for storage qualification. The directory handle participates
    /// in the project's FD budget; immutable closure objects live separately.
    #[doc(hidden)]
    pub fn storage_artifact_file_sizes(&self) -> Result<BTreeMap<String, u64>> {
        let runtime = self.runtime.get()?;
        let root = runtime.store.durable_root_path().ok_or_else(|| {
            HawDBError::Execution("storage artifact inventory requires a durable database".into())
        })?;
        let mut files = BTreeMap::new();
        for entry in hawdb_storage::file_io::read_dir(root)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if metadata.is_file() {
                let name = entry.file_name().into_string().map_err(|_| {
                    HawDBError::Storage("storage artifact inventory found a non-UTF-8 name".into())
                })?;
                files.insert(name, metadata.len());
            }
        }
        Ok(files)
    }

    pub(crate) fn poison_on_storage_error<T>(&self, result: &Result<T>) {
        if let Some(runtime) = self.runtime.peek() {
            runtime.store.poison_on_storage_error(result);
        }
    }

    pub fn storage_handle_poisoned(&self) -> Result<bool> {
        Ok(self.runtime.get()?.store.storage_handle_poisoned())
    }

    pub fn storage_residency_report(&self) -> Result<crate::store::StorageResidencyReport> {
        Ok(self.runtime.get()?.store.storage_residency_report())
    }

    /// Threads the engine's runtime governor into the storage layer so
    /// background columnar-shadow work can request admission. Called by the
    /// embedding layers that own the governor (`HawDBEmbedded`,
    /// `NowledgeMemGraph`); a second governor is never constructed here.
    pub fn set_runtime_governor(&mut self, governor: hawdb_qos::RuntimeGovernor) {
        self.retained_runtime.configure(governor.clone());
        if let Err(error) = self.runtime.set_automatic_governor(governor.clone())
            && let Some(runtime) = self.runtime.peek()
        {
            runtime.store.poison_on_storage_error(&Err::<(), _>(error));
        }
        if let Some(telemetry) = &self.telemetry {
            governor.set_telemetry_sink(Some(runtime_telemetry_sink(telemetry.clone())));
        }
        if let Some(mut runtime) = self.runtime.peek_mut() {
            runtime.store.set_runtime_governor(governor.clone());
        }
        if let Some(pending) = self.runtime.pending_mut() {
            pending.governor = Some(governor.clone());
        }
        self.runtime_governor = Some(governor);
    }

    /// Shadow write-amplification evidence of the most recent checkpoint,
    /// `None` while `graph_columnar_shadow_checkpoint` is off or before the first
    /// derived shadow checkpoint.
    pub fn columnar_shadow_checkpoint_report(
        &self,
    ) -> Result<Option<crate::store::ColumnarShadowCheckpointReport>> {
        Ok({
            self.runtime
                .get()?
                .store
                .columnar_shadow_checkpoint_report()
        })
    }

    /// What recovery observed about the columnar shadow catalog.
    pub fn columnar_shadow_recovery_status(
        &self,
    ) -> Result<crate::store::ColumnarShadowRecoveryStatus> {
        Ok(self.runtime.get()?.store.columnar_shadow_recovery_status())
    }

    /// Publication evidence for the derived relational index-page store.
    /// `None` while publication is disabled or before its first checkpoint
    /// attempt. The returned report is an owned snapshot; later background
    /// publication does not change a previously returned report.
    pub fn relational_index_shadow_checkpoint_report(
        &self,
    ) -> Result<Option<crate::store::RelationalIndexShadowCheckpointReport>> {
        Ok({
            self.runtime
                .get()?
                .store
                .relational_index_shadow_checkpoint_report()
                .cloned()
        })
    }

    /// Recovery's bounded manifest-only assessment of the relational index
    /// store, as an owned snapshot. Selected pages remain lazily validated on first SQL access.
    pub fn relational_index_shadow_recovery_status(
        &self,
    ) -> Result<crate::store::RelationalIndexShadowRecoveryStatus> {
        Ok({
            self.runtime
                .get()?
                .store
                .relational_index_shadow_recovery_status()
                .clone()
        })
    }

    /// Resource and publication evidence for WAL index deltas derived during
    /// the most recent open. The owned report can outlive this handle.
    pub fn relational_index_recovery_report(
        &self,
    ) -> Result<Option<hawdb_storage::relational::RelationalIndexRecoveryReport>> {
        Ok(self
            .runtime
            .get()?
            .store
            .relational_index_recovery_report()
            .cloned())
    }

    pub fn storage_pressure_snapshot(&self) -> Result<StoragePressureSnapshot> {
        Ok({
            let oldest_reader_epoch = self
                .runtime
                .get()?
                .reader_pins
                .lock()
                .expect("database reader pins lock should not be poisoned")
                .oldest_epoch();
            self.runtime
                .get()?
                .store
                .storage_pressure_snapshot(oldest_reader_epoch)
        })
    }

    pub fn segment_cache_snapshot(&self) -> Result<Option<SegmentCacheSnapshot>> {
        Ok(self.runtime.get()?.store.segment_cache_snapshot())
    }

    pub fn export_canonical_graph_snapshot(&self) -> Result<CanonicalGraphSnapshotExport> {
        Ok({
            export_canonical_graph_snapshot_for(
                &self.runtime.get()?.catalog,
                &self.runtime.get()?.store,
            )
        })
    }

    pub fn try_export_canonical_graph_snapshot(&self) -> Result<CanonicalGraphSnapshotExport> {
        canonical_snapshot::try_export_canonical_graph_snapshot_for(
            &self.runtime.get()?.catalog,
            &self.runtime.get()?.store,
        )
    }

    pub fn export_canonical_graph_snapshot_with_persisted_stable_ids(
        &mut self,
    ) -> Result<CanonicalGraphSnapshotExport> {
        self.ensure_writable()?;
        let snapshot = self.try_export_canonical_graph_snapshot()?;
        let duplicate_node_stable_ids = snapshot
            .stable_identity
            .duplicate_node_stable_ids
            .iter()
            .collect::<BTreeSet<_>>();
        let duplicate_relationship_stable_ids = snapshot
            .stable_identity
            .duplicate_relationship_stable_ids
            .iter()
            .collect::<BTreeSet<_>>();
        let required_node_ids = snapshot
            .nodes
            .iter()
            .filter(|node| {
                node.stable_id
                    .as_ref()
                    .is_none_or(|stable_id| duplicate_node_stable_ids.contains(stable_id))
            })
            .map(|node| NodeId(node.node_id))
            .collect::<BTreeSet<_>>();
        let required_relationship_ids = snapshot
            .relationships
            .iter()
            .filter(|relationship| {
                relationship
                    .stable_id
                    .as_ref()
                    .is_none_or(|stable_id| duplicate_relationship_stable_ids.contains(stable_id))
            })
            .map(|relationship| RelId(relationship.relationship_id))
            .collect::<BTreeSet<_>>();
        let mapping = CanonicalStableIdMapping::from(
            self.runtime
                .get_mut()?
                .store
                .ensure_stable_id_mapping(&required_node_ids, &required_relationship_ids)?,
        );
        Ok(snapshot.with_stable_id_mapping(&mapping))
    }

    pub fn prepare_hawdb_lightning_bootstrap_export(
        &mut self,
    ) -> Result<HawDBLightningBootstrapExport> {
        let snapshot = self.export_canonical_graph_snapshot_with_persisted_stable_ids()?;
        let relational_state = self.hawdb_lightning_relational_state()?;
        let relational_stream = HawDBLightningRelationalStream::from_state(
            self.runtime.get()?.store.commit_epoch(),
            &relational_state,
        )?;
        let manifest = snapshot.hawdb_lightning_bootstrap_manifest(&relational_stream);
        let graph_stream = snapshot.hawdb_lightning_graph_stream();
        Ok(HawDBLightningBootstrapExport {
            snapshot,
            manifest,
            graph_stream,
            relational_stream,
        })
    }

    pub fn hawdb_lightning_initial_import_readiness(
        &self,
        manifest: &HawDBLightningBootstrapManifest,
        projection_freshness: Option<&SearchProjectionFreshness>,
    ) -> Result<HawDBLightningInitialImportReadiness> {
        Ok({
            hawdb_lightning_initial_import_readiness(
                manifest,
                self.runtime.get()?.store.commit_epoch(),
                projection_freshness,
            )
        })
    }

    pub fn hawdb_lightning_initial_import_cutover_catch_up_report(
        &self,
        session: &HawDBLightningInitialImportSessionReport,
        live_projection_freshness: Option<&SearchProjectionFreshness>,
    ) -> Result<HawDBLightningInitialImportCutoverCatchUpReport> {
        Ok({
            hawdb_lightning_initial_import_cutover_catch_up_report(
                session,
                self.runtime.get()?.store.commit_epoch(),
                live_projection_freshness,
            )
        })
    }

    pub fn hawdb_lightning_initial_import_session_bundle_readiness(
        &self,
        source_bundle: &HawDBLightningInitialImportSourceBundleReadiness,
        session: &HawDBLightningInitialImportSessionReport,
        catch_up: Option<&HawDBLightningInitialImportCutoverCatchUpReport>,
    ) -> HawDBLightningInitialImportSessionBundleReadiness {
        hawdb_lightning_initial_import_session_bundle_readiness(source_bundle, session, catch_up)
    }

    pub fn hawdb_lightning_initial_import_decode_durable_state(
        &self,
        manifest: &HawDBLightningBootstrapManifest,
        raw: &str,
    ) -> Result<HawDBLightningInitialImportDurableStateCodecReport> {
        hawdb_lightning_initial_import_decode_durable_state(manifest, raw)
    }

    pub fn hawdb_lightning_initial_import_startup_readiness(
        &self,
        inputs: HawDBLightningInitialImportReadinessInputs<'_>,
        durable_state: Option<&HawDBLightningInitialImportDurableState>,
    ) -> Result<HawDBLightningInitialImportStartupReadinessReport> {
        Ok({
            hawdb_lightning_initial_import_startup_readiness(
                inputs,
                self.runtime.get()?.store.commit_epoch(),
                durable_state,
            )
        })
    }

    pub fn hawdb_lightning_initial_import_recovery_readiness(
        &self,
        inputs: HawDBLightningInitialImportReadinessInputs<'_>,
        durable_state_payload: Option<&str>,
    ) -> Result<HawDBLightningInitialImportRecoveryReadinessReport> {
        Ok({
            hawdb_lightning_initial_import_recovery_readiness(
                inputs,
                self.runtime.get()?.store.commit_epoch(),
                durable_state_payload,
            )
        })
    }

    pub fn hawdb_lightning_initial_import_plan(
        &self,
        encoded_graph_stream: &str,
        encoded_relational_stream: &[u8],
        manifest: &HawDBLightningBootstrapManifest,
        projection_freshness: Option<&SearchProjectionFreshness>,
        checkpoint: Option<&HawDBLightningInitialImportCheckpoint>,
    ) -> Result<HawDBLightningInitialImportPlan> {
        Ok({
            hawdb_lightning_initial_import_plan(
                encoded_graph_stream,
                encoded_relational_stream,
                manifest,
                self.runtime.get()?.store.commit_epoch(),
                projection_freshness,
                checkpoint,
            )
        })
    }

    pub fn hawdb_lightning_initial_import_plan_with_document_identities(
        &self,
        encoded_graph_stream: &str,
        encoded_relational_stream: &[u8],
        manifest: &HawDBLightningBootstrapManifest,
        projection_freshness: Option<&SearchProjectionFreshness>,
        checkpoint: Option<&HawDBLightningInitialImportCheckpoint>,
        document_identities: &[HawDBLightningInitialImportDocumentIdentity],
    ) -> Result<HawDBLightningInitialImportPlan> {
        Ok({
            hawdb_lightning_initial_import_plan_with_document_identities(
                encoded_graph_stream,
                encoded_relational_stream,
                manifest,
                self.runtime.get()?.store.commit_epoch(),
                projection_freshness,
                checkpoint,
                Some(document_identities),
            )
        })
    }

    pub fn hawdb_lightning_initial_import_apply(
        &mut self,
        encoded_graph_stream: &str,
        encoded_relational_stream: &[u8],
        manifest: &HawDBLightningBootstrapManifest,
        projection_freshness: Option<&SearchProjectionFreshness>,
        checkpoint: Option<&HawDBLightningInitialImportCheckpoint>,
    ) -> Result<HawDBLightningInitialImportApplyReport> {
        self.hawdb_lightning_initial_import_apply_internal(
            encoded_graph_stream,
            encoded_relational_stream,
            manifest,
            projection_freshness,
            checkpoint,
            None,
        )
    }

    pub fn hawdb_lightning_initial_import_apply_with_document_identities(
        &mut self,
        encoded_graph_stream: &str,
        encoded_relational_stream: &[u8],
        manifest: &HawDBLightningBootstrapManifest,
        projection_freshness: Option<&SearchProjectionFreshness>,
        checkpoint: Option<&HawDBLightningInitialImportCheckpoint>,
        document_identities: &[HawDBLightningInitialImportDocumentIdentity],
    ) -> Result<HawDBLightningInitialImportApplyReport> {
        self.hawdb_lightning_initial_import_apply_internal(
            encoded_graph_stream,
            encoded_relational_stream,
            manifest,
            projection_freshness,
            checkpoint,
            Some(document_identities),
        )
    }

    fn hawdb_lightning_initial_import_apply_internal(
        &mut self,
        encoded_graph_stream: &str,
        encoded_relational_stream: &[u8],
        manifest: &HawDBLightningBootstrapManifest,
        projection_freshness: Option<&SearchProjectionFreshness>,
        checkpoint: Option<&HawDBLightningInitialImportCheckpoint>,
        document_identities: Option<&[HawDBLightningInitialImportDocumentIdentity]>,
    ) -> Result<HawDBLightningInitialImportApplyReport> {
        self.ensure_writable()?;
        let mut blocker_codes = BTreeSet::new();
        let plan = hawdb_lightning_initial_import_plan_with_document_identities(
            encoded_graph_stream,
            encoded_relational_stream,
            manifest,
            self.runtime.get()?.store.commit_epoch(),
            projection_freshness,
            checkpoint,
            document_identities,
        );
        if !plan.ready_for_database_import {
            blocker_codes.insert("hawdb_lightning_database_streams_not_import_ready".to_string());
        }
        let source_fingerprint = hawdb_lightning_initial_import_source_fingerprint_key(manifest);
        if let Some(imported_source_fingerprint) = self
            .runtime
            .get()?
            .store
            .initial_import_source_fingerprint()
        {
            if imported_source_fingerprint == source_fingerprint && plan.ready_for_database_import {
                let (relational_table_count, relational_row_count) =
                    relational_state_counts(self.runtime.get()?.store.relational_state());
                return Ok(HawDBLightningInitialImportApplyReport {
                    applied: false,
                    ready_for_cutover: plan.ready_for_cutover,
                    database_commit_epoch: self.runtime.get()?.store.commit_epoch(),
                    node_count: self.runtime.get()?.store.basic_statistics().node_count as usize,
                    relationship_count: self
                        .runtime
                        .get()?
                        .store
                        .basic_statistics()
                        .relationship_count as usize,
                    relational_table_count,
                    relational_row_count,
                    plan,
                    blocker_codes: Vec::new(),
                });
            }
            if imported_source_fingerprint != source_fingerprint {
                blocker_codes.insert(
                    "hawdb_lightning_initial_import_source_fingerprint_mismatch".to_string(),
                );
            }
        }
        let statistics = self.runtime.get()?.store.basic_statistics();
        let target_has_only_engine_bootstrap = self.has_only_engine_system_schema_bootstrap()?;
        let relational_target_empty = self.runtime.get()?.store.relational_state().is_empty()
            || target_has_only_engine_bootstrap;
        let target_empty = statistics.node_count == 0
            && statistics.relationship_count == 0
            && relational_target_empty
            && self.runtime.get_mut()?.catalog.is_empty();
        if !target_empty {
            blocker_codes.insert("hawdb_lightning_initial_import_target_not_empty".to_string());
        }
        let snapshot = if blocker_codes.is_empty() {
            Some(parse_hawdb_lightning_graph_stream_export(
                encoded_graph_stream,
                Some(manifest),
            )?)
        } else {
            None
        };
        let relational_state = if blocker_codes.is_empty() {
            Some(
                hawdb_storage::relational::decode_relational_checkpoint(
                    encoded_relational_stream,
                    hawdb_storage::relational::RelationalDecodeLimits::checkpoint(),
                )
                .map_err(|error| HawDBError::Storage(error.to_string()))?
                .state,
            )
        } else {
            None
        };
        if let Some(snapshot) = &snapshot {
            for node in &snapshot.nodes {
                if node.labels.len() != 1 {
                    blocker_codes.insert(
                        "hawdb_lightning_initial_import_multi_label_node_unsupported".to_string(),
                    );
                    break;
                }
            }
        }
        if !blocker_codes.is_empty() {
            let (relational_table_count, relational_row_count) =
                relational_state_counts(self.runtime.get()?.store.relational_state());
            return Ok(HawDBLightningInitialImportApplyReport {
                applied: false,
                ready_for_cutover: false,
                database_commit_epoch: self.runtime.get()?.store.commit_epoch(),
                node_count: self.runtime.get()?.store.basic_statistics().node_count as usize,
                relationship_count: self
                    .runtime
                    .get()?
                    .store
                    .basic_statistics()
                    .relationship_count as usize,
                relational_table_count,
                relational_row_count,
                plan,
                blocker_codes: blocker_codes.into_iter().collect(),
            });
        }
        let snapshot = snapshot.expect("snapshot should be available without import blockers");
        let relational_state =
            relational_state.expect("relational state should be available without import blockers");
        Self::validate_hawdb_lightning_system_schema(&relational_state)?;
        let stable_id_mapping = StoreStableIdMapping {
            node_stable_ids: snapshot
                .nodes
                .iter()
                .filter_map(|node| {
                    node.stable_id
                        .as_ref()
                        .map(|stable_id| (NodeId(node.node_id), stable_id.clone()))
                })
                .collect(),
            relationship_stable_ids: snapshot
                .relationships
                .iter()
                .filter_map(|relationship| {
                    relationship
                        .stable_id
                        .as_ref()
                        .map(|stable_id| (RelId(relationship.relationship_id), stable_id.clone()))
                })
                .collect(),
        };
        let node_rows = snapshot
            .nodes
            .iter()
            .map(|node| {
                Ok((
                    NodeId(node.node_id),
                    single_import_label(node)?,
                    node.properties.clone(),
                ))
            })
            .collect::<Result<Vec<GraphSnapshotNodeImport>>>()?;
        let relationship_rows = snapshot
            .relationships
            .iter()
            .map(|relationship| {
                Ok((
                    RelId(relationship.relationship_id),
                    NodeId(relationship.source_node_id),
                    NodeId(relationship.target_node_id),
                    relationship.rel_type.clone(),
                    relationship.properties.clone(),
                ))
            })
            .collect::<Result<Vec<GraphSnapshotRelationshipImport>>>()?;
        {
            let mut branch_runtime_access = self.runtime.get_mut()?;
            let branch_runtime = &mut *branch_runtime_access;
            branch_runtime
                .store
                .import_hawdb_snapshot_rows_with_source_fingerprint(
                    &mut branch_runtime.catalog,
                    HawDBSnapshotRowsImport {
                        stable_id_mapping,
                        source_fingerprint,
                        nodes: node_rows,
                        relationships: relationship_rows,
                        relational_state,
                        target_has_only_engine_bootstrap,
                    },
                )
        }?;
        self.complete_required_relational_row_checkpoint("HawDB Lightning initial import")?;
        let updated_plan = hawdb_lightning_initial_import_plan_with_document_identities(
            encoded_graph_stream,
            encoded_relational_stream,
            manifest,
            self.runtime.get()?.store.commit_epoch(),
            projection_freshness,
            checkpoint,
            document_identities,
        );
        let (relational_table_count, relational_row_count) =
            relational_state_counts(self.runtime.get()?.store.relational_state());
        Ok(HawDBLightningInitialImportApplyReport {
            applied: true,
            ready_for_cutover: updated_plan.ready_for_cutover,
            database_commit_epoch: self.runtime.get()?.store.commit_epoch(),
            node_count: self.runtime.get()?.store.basic_statistics().node_count as usize,
            relationship_count: self
                .runtime
                .get()?
                .store
                .basic_statistics()
                .relationship_count as usize,
            relational_table_count,
            relational_row_count,
            plan: updated_plan,
            blocker_codes: Vec::new(),
        })
    }

    pub fn hawdb_lightning_bootstrap_export_background_work_plan(
        &self,
        hint: BackgroundWorkHint,
    ) -> Result<Option<BackgroundWorkPlan>> {
        Ok({
            let estimated_operations =
                self.hawdb_lightning_bootstrap_export_estimated_operations()?;
            if estimated_operations == 0 {
                return Ok(None);
            }
            Some(BackgroundWorkPlan::background(
                WorkClass::Import,
                estimated_operations,
                hint,
            ))
        })
    }

    pub fn prepare_background_hawdb_lightning_bootstrap_export(
        &mut self,
        policy: &LocalQosPolicy,
        state: &LocalQosState,
    ) -> Result<HawDBLightningBootstrapExport> {
        self.ensure_runtime_capability(hawdb_core::RuntimeCapability::BackgroundMaintenance)?;
        let estimated_operations = self.hawdb_lightning_bootstrap_export_estimated_operations()?;
        if estimated_operations == 0 {
            return self.prepare_hawdb_lightning_bootstrap_export();
        }
        let request = WorkRequest::background(WorkClass::Import, estimated_operations);
        match policy.admit(state, &request) {
            QosAdmission::Admit => self.prepare_hawdb_lightning_bootstrap_export(),
            QosAdmission::Defer { reason, .. } => Err(HawDBError::Storage(format!(
                "background HawDB Lightning bootstrap export deferred: {reason}"
            ))),
            QosAdmission::Reject { reason, .. } => Err(HawDBError::Storage(format!(
                "background HawDB Lightning bootstrap export rejected: {reason}"
            ))),
        }
    }

    pub fn prepare_scheduled_background_hawdb_lightning_bootstrap_export(
        &mut self,
    ) -> Result<HawDBLightningBootstrapExport> {
        self.ensure_runtime_capability(hawdb_core::RuntimeCapability::BackgroundMaintenance)?;
        let estimated_operations = self.hawdb_lightning_bootstrap_export_estimated_operations()?;
        if estimated_operations == 0 {
            return self.prepare_hawdb_lightning_bootstrap_export();
        }
        let scheduler = self.local_qos_scheduler_for_work();
        let permit = match scheduler.try_start(WorkRequest::background(
            WorkClass::Import,
            estimated_operations,
        )) {
            Ok(permit) => permit,
            Err(QosAdmission::Defer { reason, .. }) => {
                return Err(HawDBError::Storage(format!(
                    "background HawDB Lightning bootstrap export deferred: {reason}"
                )));
            }
            Err(QosAdmission::Reject { reason, .. }) => {
                return Err(HawDBError::Storage(format!(
                    "background HawDB Lightning bootstrap export rejected: {reason}"
                )));
            }
            Err(QosAdmission::Admit) => unreachable!("admitted work returns a permit"),
        };

        let result = self.prepare_hawdb_lightning_bootstrap_export();
        permit.finish_with_outcome(result.is_ok());
        result
    }

    fn hawdb_lightning_bootstrap_export_estimated_operations(&self) -> Result<usize> {
        Ok({
            let runtime = self.runtime.get()?;
            let statistics = runtime.store.basic_statistics();
            let graph_total = statistics
                .node_count
                .saturating_add(statistics.relationship_count);
            let relational_total = runtime
                .store
                .relational_state()
                .table_schemas()
                .map(|schema| {
                    1usize.saturating_add(runtime.store.relational_state().row_count(&schema.name))
                })
                .fold(0usize, usize::saturating_add)
                .saturating_add(runtime.store.relational_state().overflow_segment_count());
            usize::try_from(graph_total)
                .unwrap_or(usize::MAX)
                .saturating_add(relational_total)
        })
    }

    pub fn storage_version(&self) -> Result<&'static str> {
        Ok(self.runtime.get()?.store.storage_version())
    }

    #[cfg(test)]
    pub(crate) fn statistics(&self) -> Result<GraphStatistics> {
        Ok({
            self.runtime
                .get()?
                .store
                .statistics(&self.runtime.get()?.catalog)
        })
    }

    pub fn refresh_optimizer_statistics_external(
        &mut self,
        options: &crate::store::OptimizerStatisticsRefreshOptions,
    ) -> Result<crate::store::OptimizerStatisticsRefreshReport> {
        self.ensure_writable()?;
        let previous_statistics = self.runtime.get()?.store.checkpoint_statistics_snapshot();
        let previous_dirty_state = self
            .runtime
            .get()?
            .store
            .advanced_statistics_dirty_snapshot();
        let mut report = {
            let mut branch_runtime_access = self.runtime.get_mut()?;
            let branch_runtime = &mut *branch_runtime_access;
            branch_runtime
                .store
                .refresh_optimizer_statistics_external(&branch_runtime.catalog, options)
        }?;
        if let Err(error) = self.checkpoint() {
            self.runtime
                .get_mut()?
                .store
                .restore_checkpoint_statistics(previous_statistics, previous_dirty_state);
            return Err(error);
        }
        report.checkpoint_persisted = true;
        *self.runtime.get()?.plan_cache.borrow_mut() =
            PlanCache::new(self.config.max_plan_cache_entries);
        self.runtime
            .get()?
            .optimizer_planning_cache
            .borrow_mut()
            .invalidate();
        Ok(report)
    }

    fn optimizer_statistics_refresh_work(&self) -> Result<Option<OptimizerStatisticsRefreshWork>> {
        Ok({
            if self.config.read_only {
                return Ok(None);
            }
            self.runtime
                .get()?
                .store
                .optimizer_statistics_refresh_work(&self.runtime.get()?.catalog)
        })
    }

    pub fn optimizer_statistics_refresh_background_work_plan(
        &self,
        mut hint: BackgroundWorkHint,
    ) -> Result<Option<BackgroundWorkPlan>> {
        Ok({
            let Some(work) = self.optimizer_statistics_refresh_work()? else {
                return Ok(None);
            };
            hint.recent_delta_operations = hint
                .recent_delta_operations
                .max(work.recent_delta_operations);
            hint.source_graph_commit_lag = hint.source_graph_commit_lag.max(work.source_commit_lag);
            Some(BackgroundWorkPlan::background(
                WorkClass::Projection,
                work.estimated_operations,
                hint,
            ))
        })
    }

    pub fn refresh_background_optimizer_statistics(
        &mut self,
        policy: &LocalQosPolicy,
        state: &LocalQosState,
        options: &crate::store::OptimizerStatisticsRefreshOptions,
        hint: BackgroundWorkHint,
    ) -> Result<Option<crate::store::OptimizerStatisticsRefreshReport>> {
        self.ensure_runtime_capability(hawdb_core::RuntimeCapability::BackgroundMaintenance)?;
        let Some(plan) = self.optimizer_statistics_refresh_background_work_plan(hint)? else {
            return Ok(None);
        };
        match policy.admit(state, &plan.request) {
            QosAdmission::Admit => self
                .refresh_optimizer_statistics_external(options)
                .map(Some),
            QosAdmission::Defer { reason, .. } => Err(HawDBError::Storage(format!(
                "background optimizer statistics refresh deferred: {reason}"
            ))),
            QosAdmission::Reject { reason, .. } => Err(HawDBError::Storage(format!(
                "background optimizer statistics refresh rejected: {reason}"
            ))),
        }
    }

    pub fn refresh_scheduled_background_optimizer_statistics(
        &mut self,
        options: &crate::store::OptimizerStatisticsRefreshOptions,
        hint: BackgroundWorkHint,
    ) -> Result<Option<crate::store::OptimizerStatisticsRefreshReport>> {
        self.ensure_runtime_capability(hawdb_core::RuntimeCapability::BackgroundMaintenance)?;
        let Some(plan) = self.optimizer_statistics_refresh_background_work_plan(hint)? else {
            return Ok(None);
        };
        let scheduler = self.local_qos_scheduler_for_work();
        let permit = match scheduler.try_start(plan.request) {
            Ok(permit) => permit,
            Err(QosAdmission::Defer { reason, .. }) => {
                return Err(HawDBError::Storage(format!(
                    "background optimizer statistics refresh deferred: {reason}"
                )));
            }
            Err(QosAdmission::Reject { reason, .. }) => {
                return Err(HawDBError::Storage(format!(
                    "background optimizer statistics refresh rejected: {reason}"
                )));
            }
            Err(QosAdmission::Admit) => unreachable!("admitted work returns a permit"),
        };

        let result = self
            .refresh_optimizer_statistics_external(options)
            .map(Some);
        permit.finish_with_outcome(result.is_ok());
        result
    }

    #[cfg(test)]
    pub(crate) fn basic_statistics(&self) -> Result<crate::schema::BasicGraphStatistics> {
        Ok(self.runtime.get()?.store.basic_statistics())
    }

    pub fn basic_statistics_consistency_report(&self) -> Result<BasicStatisticsConsistencyReport> {
        Ok({
            self.runtime
                .get()?
                .store
                .basic_statistics_consistency_report()
        })
    }

    pub fn adjacency_consistency_report(&self) -> Result<AdjacencyConsistencyReport> {
        Ok(self.runtime.get()?.store.adjacency_consistency_report())
    }

    pub fn adjacency_consolidation_plan(&self) -> Result<AdjacencyConsolidationPlan> {
        Ok(self.runtime.get()?.store.adjacency_consolidation_plan())
    }

    pub fn storage_checkpoint_background_work_plan(
        &self,
        mut hint: BackgroundWorkHint,
    ) -> Result<Option<BackgroundWorkPlan>> {
        Ok({
            if self.config.read_only {
                return Ok(None);
            }
            let pressure = self.storage_pressure_snapshot()?;
            if !pressure.recommends_checkpoint() {
                return Ok(None);
            }
            hint.recent_delta_operations = hint
                .recent_delta_operations
                .max(self.runtime.get()?.store.checkpoint_estimated_operations());
            hint.source_graph_commit_lag = hint.source_graph_commit_lag.max(
                pressure
                    .current_commit_epoch
                    .saturating_sub(pressure.checkpoint_commit_epoch),
            );
            hint.staleness_millis = hint.staleness_millis.max(pressure.wal_age_millis);
            Some(BackgroundWorkPlan::background(
                WorkClass::Mutation,
                self.runtime.get()?.store.checkpoint_estimated_operations(),
                hint,
            ))
        })
    }

    /// Attempts pressure-triggered checkpoint work under the supplied background
    /// policy. For an explicit App-triggered checkpoint, use [`Self::checkpoint`].
    pub fn checkpoint_background(
        &mut self,
        policy: &LocalQosPolicy,
        state: &LocalQosState,
        hint: BackgroundWorkHint,
    ) -> Result<()> {
        self.ensure_runtime_capability(hawdb_core::RuntimeCapability::BackgroundMaintenance)?;
        let Some(plan) = self.storage_checkpoint_background_work_plan(hint)? else {
            return Ok(());
        };
        match policy.admit(state, &plan.request) {
            QosAdmission::Admit => self.checkpoint(),
            QosAdmission::Defer { reason, .. } => Err(HawDBError::Storage(format!(
                "background storage checkpoint deferred: {reason}"
            ))),
            QosAdmission::Reject { reason, .. } => Err(HawDBError::Storage(format!(
                "background storage checkpoint rejected: {reason}"
            ))),
        }
    }

    /// Attempts pressure-triggered checkpoint work under the local background
    /// scheduler. For an explicit App-triggered checkpoint, use [`Self::checkpoint`].
    pub fn checkpoint_scheduled_background(&mut self, hint: BackgroundWorkHint) -> Result<()> {
        self.ensure_runtime_capability(hawdb_core::RuntimeCapability::BackgroundMaintenance)?;
        let Some(plan) = self.storage_checkpoint_background_work_plan(hint)? else {
            return Ok(());
        };
        let scheduler = self.local_qos_scheduler_for_work();
        let permit = match scheduler.try_start(plan.request) {
            Ok(permit) => permit,
            Err(QosAdmission::Defer { reason, .. }) => {
                return Err(HawDBError::Storage(format!(
                    "background storage checkpoint deferred: {reason}"
                )));
            }
            Err(QosAdmission::Reject { reason, .. }) => {
                return Err(HawDBError::Storage(format!(
                    "background storage checkpoint rejected: {reason}"
                )));
            }
            Err(QosAdmission::Admit) => unreachable!("admitted work returns a permit"),
        };
        let result = self.checkpoint();
        permit.finish_with_outcome(result.is_ok());
        result
    }

    pub fn adjacency_consolidation_background_work_plan(
        &self,
        max_estimated_entries: usize,
        hint: BackgroundWorkHint,
    ) -> Result<Option<BackgroundWorkPlan>> {
        Ok({
            let estimated_entries = self
                .runtime
                .get()?
                .store
                .bounded_adjacency_consolidation_estimated_entries(max_estimated_entries);
            (estimated_entries > 0).then(|| {
                BackgroundWorkPlan::background(WorkClass::Mutation, estimated_entries, hint)
            })
        })
    }

    pub fn consolidate_bounded_adjacency_deltas(
        &mut self,
        max_estimated_entries: usize,
    ) -> Result<AdjacencyConsolidationReport> {
        Ok({
            self.runtime
                .get_mut()?
                .store
                .consolidate_bounded_adjacency_deltas(max_estimated_entries)
        })
    }

    pub fn consolidate_bounded_background_adjacency_deltas(
        &mut self,
        policy: &LocalQosPolicy,
        state: &LocalQosState,
        max_estimated_entries: usize,
    ) -> Result<AdjacencyConsolidationReport> {
        self.ensure_runtime_capability(hawdb_core::RuntimeCapability::BackgroundMaintenance)?;
        let estimated_entries = self
            .runtime
            .get()?
            .store
            .bounded_adjacency_consolidation_estimated_entries(max_estimated_entries);
        if estimated_entries == 0 {
            return self.consolidate_bounded_adjacency_deltas(max_estimated_entries);
        }
        match policy.admit(
            state,
            &WorkRequest::background(WorkClass::Mutation, estimated_entries),
        ) {
            QosAdmission::Admit => {
                Ok(self.consolidate_bounded_adjacency_deltas(max_estimated_entries)?)
            }
            QosAdmission::Defer { reason, .. } => Err(HawDBError::Storage(format!(
                "background adjacency consolidation deferred: {reason}"
            ))),
            QosAdmission::Reject { reason, .. } => Err(HawDBError::Storage(format!(
                "background adjacency consolidation rejected: {reason}"
            ))),
        }
    }

    pub fn consolidate_bounded_scheduled_background_adjacency_deltas(
        &mut self,
        max_estimated_entries: usize,
    ) -> Result<AdjacencyConsolidationReport> {
        self.ensure_runtime_capability(hawdb_core::RuntimeCapability::BackgroundMaintenance)?;
        let estimated_entries = self
            .runtime
            .get()?
            .store
            .bounded_adjacency_consolidation_estimated_entries(max_estimated_entries);
        if estimated_entries == 0 {
            return self.consolidate_bounded_adjacency_deltas(max_estimated_entries);
        }
        let scheduler = self.local_qos_scheduler_for_work();
        let permit = match scheduler.try_start(WorkRequest::background(
            WorkClass::Mutation,
            estimated_entries,
        )) {
            Ok(permit) => permit,
            Err(QosAdmission::Defer { reason, .. }) => {
                return Err(HawDBError::Storage(format!(
                    "background adjacency consolidation deferred: {reason}"
                )));
            }
            Err(QosAdmission::Reject { reason, .. }) => {
                return Err(HawDBError::Storage(format!(
                    "background adjacency consolidation rejected: {reason}"
                )));
            }
            Err(QosAdmission::Admit) => unreachable!("admitted work returns a permit"),
        };

        let result = Ok(self.consolidate_bounded_adjacency_deltas(max_estimated_entries)?);
        permit.finish_with_outcome(result.is_ok());
        result
    }

    pub fn degree_statistics_consistency_report(
        &self,
    ) -> Result<DegreeStatisticsConsistencyReport> {
        Ok({
            self.runtime
                .get()?
                .store
                .degree_statistics_consistency_report()
        })
    }

    pub fn distinct_value_statistics_consistency_report(
        &self,
    ) -> Result<DistinctValueStatisticsConsistencyReport> {
        Ok({
            self.runtime
                .get()?
                .store
                .distinct_value_statistics_consistency_report(&self.runtime.get()?.catalog)
        })
    }

    pub fn property_index_consistency_report(&self) -> Result<PropertyIndexConsistencyReport> {
        Ok({
            self.runtime
                .get()?
                .store
                .property_index_consistency_report(&self.runtime.get()?.catalog)
        })
    }

    #[cfg(test)]
    pub(crate) fn property_indexes(&self) -> Result<Vec<IndexDescriptor>> {
        Ok({
            self.runtime
                .get()?
                .catalog
                .property_indexes()
                .cloned()
                .collect()
        })
    }

    #[cfg(test)]
    pub(crate) fn composite_property_indexes(&self) -> Result<Vec<CompositeIndexDescriptor>> {
        Ok({
            self.runtime
                .get()?
                .catalog
                .composite_property_indexes()
                .cloned()
                .collect()
        })
    }

    pub fn rebuild_bounded_property_index_projections(
        &mut self,
        max_estimated_operations: usize,
    ) -> Result<QueryOutput> {
        Ok({
            property_index_projection_rebuild_output({
                let mut branch_runtime_access = self.runtime.get_mut()?;
                let branch_runtime = &mut *branch_runtime_access;
                branch_runtime
                    .store
                    .rebuild_bounded_property_index_projections(
                        &branch_runtime.catalog,
                        max_estimated_operations,
                    )
            })
        })
    }

    pub fn property_index_projection_background_work_plan(
        &self,
        hint: BackgroundWorkHint,
    ) -> Result<Option<BackgroundWorkPlan>> {
        Ok({
            let estimated_operations = self
                .runtime
                .get()?
                .store
                .property_index_projection_estimated_operations(&self.runtime.get()?.catalog);
            if estimated_operations == 0 {
                return Ok(None);
            }
            Some(BackgroundWorkPlan::background(
                WorkClass::Projection,
                estimated_operations,
                hint,
            ))
        })
    }

    pub fn rebuild_bounded_background_property_index_projections(
        &mut self,
        policy: &LocalQosPolicy,
        state: &LocalQosState,
        max_estimated_operations: usize,
    ) -> Result<QueryOutput> {
        self.ensure_runtime_capability(hawdb_core::RuntimeCapability::BackgroundMaintenance)?;
        let estimated_operations = {
            let mut branch_runtime_access = self.runtime.get_mut()?;
            let branch_runtime = &mut *branch_runtime_access;
            branch_runtime
                .store
                .bounded_property_index_projection_estimated_operations(
                    &branch_runtime.catalog,
                    max_estimated_operations,
                )
        };
        if estimated_operations == 0 {
            return self.rebuild_bounded_property_index_projections(max_estimated_operations);
        }
        let request = WorkRequest::background(WorkClass::Projection, estimated_operations);
        match policy.admit(state, &request) {
            QosAdmission::Admit => {
                Ok(self.rebuild_bounded_property_index_projections(max_estimated_operations)?)
            }
            QosAdmission::Defer { reason, .. } => Err(HawDBError::Storage(format!(
                "background property index projection rebuild deferred: {reason}"
            ))),
            QosAdmission::Reject { reason, .. } => Err(HawDBError::Storage(format!(
                "background property index projection rebuild rejected: {reason}"
            ))),
        }
    }

    pub fn rebuild_bounded_scheduled_background_property_index_projections(
        &mut self,
        max_estimated_operations: usize,
    ) -> Result<QueryOutput> {
        self.ensure_runtime_capability(hawdb_core::RuntimeCapability::BackgroundMaintenance)?;
        let estimated_operations = {
            let mut branch_runtime_access = self.runtime.get_mut()?;
            let branch_runtime = &mut *branch_runtime_access;
            branch_runtime
                .store
                .bounded_property_index_projection_estimated_operations(
                    &branch_runtime.catalog,
                    max_estimated_operations,
                )
        };
        if estimated_operations == 0 {
            return self.rebuild_bounded_property_index_projections(max_estimated_operations);
        }
        let scheduler = self.local_qos_scheduler_for_work();
        let permit = match scheduler.try_start(WorkRequest::background(
            WorkClass::Projection,
            estimated_operations,
        )) {
            Ok(permit) => permit,
            Err(QosAdmission::Defer { reason, .. }) => {
                return Err(HawDBError::Storage(format!(
                    "background property index projection rebuild deferred: {reason}"
                )));
            }
            Err(QosAdmission::Reject { reason, .. }) => {
                return Err(HawDBError::Storage(format!(
                    "background property index projection rebuild rejected: {reason}"
                )));
            }
            Err(QosAdmission::Admit) => unreachable!("admitted work returns a permit"),
        };

        let result = Ok(self.rebuild_bounded_property_index_projections(max_estimated_operations)?);
        permit.finish_with_outcome(result.is_ok());
        result
    }

    #[cfg(test)]
    pub(crate) fn unique_constraints(&self) -> Result<Vec<ConstraintDescriptor>> {
        Ok({
            self.runtime
                .get()?
                .catalog
                .unique_constraints()
                .cloned()
                .collect()
        })
    }

    #[cfg(test)]
    pub(crate) fn node_property_exists_constraints(&self) -> Result<Vec<ConstraintDescriptor>> {
        Ok({
            self.runtime
                .get()?
                .catalog
                .node_property_exists_constraints()
                .cloned()
                .collect()
        })
    }

    #[cfg(test)]
    pub(crate) fn relationship_property_exists_constraints(
        &self,
    ) -> Result<Vec<ConstraintDescriptor>> {
        Ok({
            self.runtime
                .get()?
                .catalog
                .relationship_property_exists_constraints()
                .cloned()
                .collect()
        })
    }

    #[cfg(test)]
    pub(crate) fn relationship_unique_constraints(&self) -> Result<Vec<ConstraintDescriptor>> {
        Ok({
            self.runtime
                .get()?
                .catalog
                .relationship_unique_constraints()
                .cloned()
                .collect()
        })
    }

    #[cfg(test)]
    pub(crate) fn table_descriptors(&self) -> Result<Vec<TableDescriptor>> {
        Ok({
            self.runtime
                .get()?
                .catalog
                .table_descriptors()
                .cloned()
                .collect()
        })
    }

    #[cfg(test)]
    pub(crate) fn property_descriptors(&self) -> Result<Vec<PropertyDescriptor>> {
        Ok({
            self.runtime
                .get()?
                .catalog
                .property_descriptors()
                .cloned()
                .collect()
        })
    }

    pub fn plan_schema_maintenance(&self) -> Result<QueryOutput> {
        Ok({
            let rows: Vec<Row> = self
                .runtime
                .get()?
                .store
                .plan_schema_maintenance(&self.runtime.get()?.catalog)
                .into_iter()
                .map(|item| {
                    BTreeMap::from([
                        ("object_type".to_string(), Value::String(item.object_type)),
                        ("object".to_string(), Value::String(item.object)),
                        (
                            "from_state".to_string(),
                            schema_state_value(item.from_state),
                        ),
                        (
                            "to_state".to_string(),
                            item.to_state.map(schema_state_value).unwrap_or(Value::Null),
                        ),
                        ("action".to_string(), Value::String(item.action)),
                        (
                            "estimated_operations".to_string(),
                            Value::Int(
                                i64::try_from(item.estimated_operations).unwrap_or(i64::MAX),
                            ),
                        ),
                    ])
                })
                .collect();
            QueryOutput { rows: rows.into() }
        })
    }

    pub fn schema_maintenance_background_work_plan(
        &self,
        hint: BackgroundWorkHint,
    ) -> Result<Option<BackgroundWorkPlan>> {
        Ok({
            let estimated_operations = self.schema_maintenance_estimated_operations()?;
            if estimated_operations == 0 {
                return Ok(None);
            }
            Some(BackgroundWorkPlan::background(
                WorkClass::Mutation,
                estimated_operations,
                hint,
            ))
        })
    }

    pub fn run_schema_maintenance(&mut self) -> Result<QueryOutput> {
        self.ensure_writable()?;
        let actions = {
            let mut branch_runtime_access = self.runtime.get_mut()?;
            let branch_runtime = &mut *branch_runtime_access;
            branch_runtime
                .store
                .run_schema_maintenance(&mut branch_runtime.catalog)
        }?;
        Ok(schema_maintenance_actions_output(actions))
    }

    pub fn run_bounded_schema_maintenance(
        &mut self,
        max_estimated_operations: usize,
    ) -> Result<QueryOutput> {
        self.ensure_writable()?;
        let actions = {
            let mut branch_runtime_access = self.runtime.get_mut()?;
            let branch_runtime = &mut *branch_runtime_access;
            branch_runtime.store.run_bounded_schema_maintenance(
                &mut branch_runtime.catalog,
                max_estimated_operations,
            )
        }?;
        Ok(schema_maintenance_actions_output(actions))
    }

    pub fn run_background_schema_maintenance(
        &mut self,
        policy: &LocalQosPolicy,
        state: &LocalQosState,
        estimated_operations: usize,
    ) -> Result<QueryOutput> {
        self.ensure_runtime_capability(hawdb_core::RuntimeCapability::BackgroundMaintenance)?;
        let request = WorkRequest::background(WorkClass::Mutation, estimated_operations);
        match policy.admit(state, &request) {
            QosAdmission::Admit => self.run_schema_maintenance(),
            QosAdmission::Defer { reason, .. } => Err(HawDBError::Storage(format!(
                "background schema maintenance deferred: {reason}"
            ))),
            QosAdmission::Reject { reason, .. } => Err(HawDBError::Storage(format!(
                "background schema maintenance rejected: {reason}"
            ))),
        }
    }

    pub fn run_bounded_background_schema_maintenance(
        &mut self,
        policy: &LocalQosPolicy,
        state: &LocalQosState,
        max_estimated_operations: usize,
    ) -> Result<QueryOutput> {
        self.ensure_runtime_capability(hawdb_core::RuntimeCapability::BackgroundMaintenance)?;
        let estimated_operations =
            self.bounded_schema_maintenance_estimated_operations(max_estimated_operations)?;
        if estimated_operations == 0 {
            return self.run_bounded_schema_maintenance(max_estimated_operations);
        }
        let request = WorkRequest::background(WorkClass::Mutation, estimated_operations);
        match policy.admit(state, &request) {
            QosAdmission::Admit => self.run_bounded_schema_maintenance(max_estimated_operations),
            QosAdmission::Defer { reason, .. } => Err(HawDBError::Storage(format!(
                "background schema maintenance deferred: {reason}"
            ))),
            QosAdmission::Reject { reason, .. } => Err(HawDBError::Storage(format!(
                "background schema maintenance rejected: {reason}"
            ))),
        }
    }

    pub fn run_planned_background_schema_maintenance(
        &mut self,
        policy: &LocalQosPolicy,
        state: &LocalQosState,
    ) -> Result<QueryOutput> {
        self.run_background_schema_maintenance(
            policy,
            state,
            self.schema_maintenance_estimated_operations()?,
        )
    }

    pub fn run_scheduled_background_schema_maintenance(
        &mut self,
        estimated_operations: usize,
    ) -> Result<QueryOutput> {
        self.ensure_runtime_capability(hawdb_core::RuntimeCapability::BackgroundMaintenance)?;
        let scheduler = self.local_qos_scheduler_for_work();
        let permit = match scheduler.try_start(WorkRequest::background(
            WorkClass::Mutation,
            estimated_operations,
        )) {
            Ok(permit) => permit,
            Err(QosAdmission::Defer { reason, .. }) => {
                return Err(HawDBError::Storage(format!(
                    "background schema maintenance deferred: {reason}"
                )));
            }
            Err(QosAdmission::Reject { reason, .. }) => {
                return Err(HawDBError::Storage(format!(
                    "background schema maintenance rejected: {reason}"
                )));
            }
            Err(QosAdmission::Admit) => unreachable!("admitted work returns a permit"),
        };

        let result = self.run_schema_maintenance();
        permit.finish_with_outcome(result.is_ok());
        result
    }

    pub fn run_bounded_scheduled_background_schema_maintenance(
        &mut self,
        max_estimated_operations: usize,
    ) -> Result<QueryOutput> {
        self.ensure_runtime_capability(hawdb_core::RuntimeCapability::BackgroundMaintenance)?;
        let estimated_operations =
            self.bounded_schema_maintenance_estimated_operations(max_estimated_operations)?;
        if estimated_operations == 0 {
            return self.run_bounded_schema_maintenance(max_estimated_operations);
        }
        let scheduler = self.local_qos_scheduler_for_work();
        let permit = match scheduler.try_start(WorkRequest::background(
            WorkClass::Mutation,
            estimated_operations,
        )) {
            Ok(permit) => permit,
            Err(QosAdmission::Defer { reason, .. }) => {
                return Err(HawDBError::Storage(format!(
                    "background schema maintenance deferred: {reason}"
                )));
            }
            Err(QosAdmission::Reject { reason, .. }) => {
                return Err(HawDBError::Storage(format!(
                    "background schema maintenance rejected: {reason}"
                )));
            }
            Err(QosAdmission::Admit) => unreachable!("admitted work returns a permit"),
        };

        let result = self.run_bounded_schema_maintenance(max_estimated_operations);
        permit.finish_with_outcome(result.is_ok());
        result
    }

    pub fn run_planned_scheduled_background_schema_maintenance(&mut self) -> Result<QueryOutput> {
        self.run_scheduled_background_schema_maintenance(
            self.schema_maintenance_estimated_operations()?,
        )
    }

    fn schema_maintenance_estimated_operations(&self) -> Result<usize> {
        Ok({
            self.runtime
                .get()?
                .store
                .plan_schema_maintenance(&self.runtime.get()?.catalog)
                .into_iter()
                .map(|item| item.estimated_operations)
                .fold(0usize, usize::saturating_add)
        })
    }

    fn bounded_schema_maintenance_estimated_operations(
        &self,
        max_estimated_operations: usize,
    ) -> Result<usize> {
        Ok({
            let mut used_estimated_operations = 0usize;
            for item in self
                .runtime
                .get()?
                .store
                .plan_schema_maintenance(&self.runtime.get()?.catalog)
            {
                let Some(next) = used_estimated_operations.checked_add(item.estimated_operations)
                else {
                    continue;
                };
                if next <= max_estimated_operations {
                    used_estimated_operations = next;
                }
            }
            used_estimated_operations
        })
    }

    #[cfg(test)]
    pub(crate) fn projected_graph_statuses(
        &self,
    ) -> Result<Vec<crate::store::ProjectedGraphStatus>> {
        Ok(self.runtime.get()?.store.projected_graph_statuses())
    }

    pub fn rebuild_projected_graph_artifacts(&mut self) -> Result<()> {
        self.ensure_writable()?;
        let _suspension = self.runtime.suspend_automatic_checkpoint()?;
        // This derived write has no WAL record, so the next candidate must
        // capture the rebuilt runtime rather than reuse the prior source.
        self.runtime.release_suspended_checkpoint_source()?;
        {
            let mut branch_runtime_access = self.runtime.get_mut()?;
            let branch_runtime = &mut *branch_runtime_access;
            branch_runtime
                .store
                .rebuild_projected_graph_artifacts(&branch_runtime.catalog)
        }
    }

    pub fn rebuild_search_projection(
        &self,
        search_index: &mut SearchIndex,
        options: SearchRebuildOptions,
    ) -> Result<SearchRebuildSummary> {
        search_index.rebuild_from_graph(
            &self.runtime.get()?.catalog,
            &self.runtime.get()?.store,
            options,
        )
    }

    pub fn search_projection_rebuild_background_work_plan(
        &self,
        search_index: &SearchIndex,
        hint: BackgroundWorkHint,
    ) -> Result<Option<BackgroundWorkPlan>> {
        Ok(search_index.rebuild_background_work_plan(&self.runtime.get()?.store, hint))
    }

    pub fn rebuild_background_search_projection(
        &self,
        search_index: &mut SearchIndex,
        policy: &LocalQosPolicy,
        state: &LocalQosState,
        options: SearchRebuildOptions,
    ) -> Result<SearchDerivedArtifactReport> {
        self.ensure_runtime_capability(hawdb_core::RuntimeCapability::BackgroundMaintenance)?;
        search_index.rebuild_background_derived_artifacts(
            policy,
            state,
            &self.runtime.get()?.catalog,
            &self.runtime.get()?.store,
            options,
        )
    }

    pub fn rebuild_scheduled_background_search_projection(
        &self,
        search_index: &mut SearchIndex,
        options: SearchRebuildOptions,
    ) -> Result<SearchDerivedArtifactReport> {
        self.ensure_runtime_capability(hawdb_core::RuntimeCapability::BackgroundMaintenance)?;
        let scheduler = self.local_qos_scheduler_for_work();
        search_index.rebuild_scheduled_background_derived_artifacts(
            &scheduler,
            &self.runtime.get()?.catalog,
            &self.runtime.get()?.store,
            options,
        )
    }

    pub fn repair_search_projection_metadata(
        &self,
        search_index: &mut SearchIndex,
        options: MetadataRepairOptions,
    ) -> Result<MetadataRepairSummary> {
        search_index.repair_metadata_from_graph(
            &self.runtime.get()?.catalog,
            &self.runtime.get()?.store,
            options,
        )
    }

    pub fn search_projection_metadata_repair_background_work_plan(
        &self,
        search_index: &SearchIndex,
        hint: BackgroundWorkHint,
    ) -> Result<Option<BackgroundWorkPlan>> {
        Ok(search_index.metadata_repair_background_work_plan(&self.runtime.get()?.store, hint))
    }

    pub fn repair_background_search_projection_metadata(
        &self,
        search_index: &mut SearchIndex,
        policy: &LocalQosPolicy,
        state: &LocalQosState,
        options: MetadataRepairOptions,
        estimated_operations: usize,
    ) -> Result<MetadataRepairSummary> {
        self.ensure_runtime_capability(hawdb_core::RuntimeCapability::BackgroundMaintenance)?;
        search_index.repair_background_metadata_from_graph(
            policy,
            state,
            &self.runtime.get()?.catalog,
            &self.runtime.get()?.store,
            options,
            estimated_operations,
        )
    }

    pub fn repair_scheduled_background_search_projection_metadata(
        &self,
        search_index: &mut SearchIndex,
        options: MetadataRepairOptions,
        estimated_operations: usize,
    ) -> Result<MetadataRepairSummary> {
        self.ensure_runtime_capability(hawdb_core::RuntimeCapability::BackgroundMaintenance)?;
        let scheduler = self.local_qos_scheduler_for_work();
        search_index.repair_scheduled_background_metadata_from_graph(
            &scheduler,
            &self.runtime.get()?.catalog,
            &self.runtime.get()?.store,
            options,
            estimated_operations,
        )
    }

    pub fn search_projection_delta_background_work_plan(
        &self,
        delta: &SearchProjectionDelta,
        hint: BackgroundWorkHint,
    ) -> Option<BackgroundWorkPlan> {
        delta.background_work_plan(hint)
    }

    pub fn search_projection_graph_delta_background_work_plan(
        &self,
        request: &SearchProjectionGraphDeltaRequest,
        hint: BackgroundWorkHint,
    ) -> Option<BackgroundWorkPlan> {
        request.background_work_plan(hint)
    }

    pub fn search_projection_graph_delta_freshness_background_work_plan(
        &self,
        search_index: &SearchIndex,
        request: &SearchProjectionGraphDeltaRequest,
        mut hint: BackgroundWorkHint,
    ) -> Result<Option<BackgroundWorkPlan>> {
        Ok({
            if hint.recent_delta_operations == 0 {
                hint.recent_delta_operations = request.operation_count();
            }
            if hint.source_graph_commit_lag == 0 {
                hint.source_graph_commit_lag = search_projection_commit_lag(
                    search_index,
                    self.runtime
                        .get()?
                        .store
                        .search_projection_changefeed_status()
                        .required_projection_commit_epoch(),
                );
            }
            if request.operation_count() == 0
                && hint.source_graph_commit_lag > 0
                && request.complete_through_graph_commit_epoch.is_some()
            {
                if hint.recent_delta_operations == 0 {
                    hint.recent_delta_operations = 1;
                }
                return Ok(Some(BackgroundWorkPlan::background(
                    WorkClass::Projection,
                    1,
                    hint,
                )));
            }
            request.background_work_plan(hint)
        })
    }

    pub fn search_projection_freshness_lag_background_work_plan(
        &self,
        search_index: &SearchIndex,
        mut hint: BackgroundWorkHint,
    ) -> Result<Option<BackgroundWorkPlan>> {
        Ok({
            let source_graph_commit_lag = search_projection_commit_lag(
                search_index,
                self.runtime
                    .get()?
                    .store
                    .search_projection_changefeed_status()
                    .required_projection_commit_epoch(),
            );
            if source_graph_commit_lag == 0 {
                return Ok(None);
            }
            let operation_count = usize::try_from(source_graph_commit_lag).unwrap_or(usize::MAX);
            if hint.recent_delta_operations == 0 {
                hint.recent_delta_operations = operation_count;
            }
            if hint.source_graph_commit_lag == 0 {
                hint.source_graph_commit_lag = source_graph_commit_lag;
            }
            Some(BackgroundWorkPlan::background(
                WorkClass::Projection,
                operation_count,
                hint,
            ))
        })
    }

    pub fn build_search_projection_graph_delta_request_after(
        &self,
        source_graph_commit_epoch: u64,
        max_operations: Option<usize>,
    ) -> Result<Option<SearchProjectionGraphDeltaRequest>> {
        let Some(batch) = self.build_search_projection_change_batch_after(
            source_graph_commit_epoch,
            max_operations,
        )?
        else {
            return Ok(None);
        };
        if batch.has_relational_changes() {
            return Err(HawDBError::Storage(format!(
                "search projection commits through epoch {} contain relational primary-key changes; use the unified search projection changefeed and publish one combined projection delta",
                batch.complete_through_commit_epoch().unwrap_or(source_graph_commit_epoch)
            )));
        }
        Ok(Some(batch.into_graph_delta()))
    }

    pub fn build_search_projection_change_batch_after(
        &self,
        source_commit_epoch: u64,
        max_operations: Option<usize>,
    ) -> Result<Option<SearchProjectionChangeBatch>> {
        let current_epoch = self.runtime.get()?.store.commit_epoch();
        if source_commit_epoch >= current_epoch {
            return Ok(None);
        }
        let change_log_start_epoch = self
            .runtime
            .get()?
            .store
            .search_projection_change_log_start_epoch();
        if source_commit_epoch < change_log_start_epoch {
            return Err(HawDBError::Storage(format!(
                "search projection change log starts at commit epoch {change_log_start_epoch}; requested source commit epoch {source_commit_epoch}; full search projection rebuild required"
            )));
        }

        let mut upsert_node_ids = BTreeSet::new();
        let mut delete_document_ids = BTreeSet::new();
        let mut relational_primary_keys =
            BTreeMap::<String, BTreeSet<hawdb_storage::relational::RelationalKey>>::new();
        let mut complete_through_commit_epoch = source_commit_epoch;
        let mut truncated_by_budget = false;
        for change in self
            .runtime
            .get()?
            .store
            .search_projection_changes_after(source_commit_epoch)
        {
            let tables = match &change.relational_primary_key_changes {
                hawdb_storage::relational::RelationalPrimaryKeyChangeCapture::Captured {
                    tables,
                    ..
                } => tables,
                hawdb_storage::relational::RelationalPrimaryKeyChangeCapture::RequiresRebuild {
                    reason,
                } => {
                    return Err(HawDBError::Storage(format!(
                        "search projection relational change at commit epoch {} requires a full rebuild: {reason:?}",
                        change.commit_epoch
                    )));
                }
            };
            let additional_operation_count = change
                .upsert_node_ids
                .iter()
                .filter(|node_id| !upsert_node_ids.contains(*node_id))
                .count()
                .saturating_add(
                    change
                        .delete_document_ids
                        .iter()
                        .filter(|document_id| !delete_document_ids.contains(*document_id))
                        .count(),
                )
                .saturating_add(
                    tables
                        .iter()
                        .map(|table| {
                            let existing = relational_primary_keys.get(&table.table);
                            table
                                .primary_keys
                                .iter()
                                .filter(|key| existing.is_none_or(|keys| !keys.contains(*key)))
                                .count()
                        })
                        .sum::<usize>(),
                );
            let next_operation_count = upsert_node_ids
                .len()
                .saturating_add(delete_document_ids.len())
                .saturating_add(
                    relational_primary_keys
                        .values()
                        .map(BTreeSet::len)
                        .sum::<usize>(),
                )
                .saturating_add(additional_operation_count);
            if let Some(limit) = max_operations
                && next_operation_count > limit
            {
                if complete_through_commit_epoch == source_commit_epoch {
                    return Err(HawDBError::Storage(format!(
                        "search projection change at commit epoch {} requires {next_operation_count} operations, exceeding configured per-batch limit {limit}",
                        change.commit_epoch
                    )));
                }
                truncated_by_budget = true;
                break;
            }
            upsert_node_ids.extend(change.upsert_node_ids);
            delete_document_ids.extend(change.delete_document_ids);
            for table in tables {
                relational_primary_keys
                    .entry(table.table.clone())
                    .or_default()
                    .extend(table.primary_keys.iter().cloned());
            }
            complete_through_commit_epoch = change.commit_epoch;
        }
        if !truncated_by_budget {
            complete_through_commit_epoch = current_epoch;
        }

        Ok(Some(SearchProjectionChangeBatch::new(
            SearchProjectionGraphDeltaRequest {
                upsert_node_ids: upsert_node_ids.into_iter().collect(),
                delete_document_ids: delete_document_ids.into_iter().collect(),
                max_operations,
                complete_through_graph_commit_epoch: Some(complete_through_commit_epoch),
            },
            relational_primary_keys
                .into_iter()
                .map(|(table, primary_keys)| {
                    hawdb_storage::relational::RelationalTablePrimaryKeyChanges {
                        table,
                        primary_keys: primary_keys.into_iter().collect(),
                    }
                })
                .collect(),
        )))
    }

    pub fn build_search_projection_graph_delta_request_from_freshness(
        &self,
        search_index: &SearchIndex,
        max_operations: Option<usize>,
    ) -> Result<Option<SearchProjectionGraphDeltaRequest>> {
        self.build_search_projection_graph_delta_request_after(
            search_index
                .projection_freshness()
                .source_graph_commit_epoch
                .unwrap_or(0),
            max_operations,
        )
    }

    pub fn build_search_projection_change_batch_from_freshness(
        &self,
        search_index: &SearchIndex,
        max_operations: Option<usize>,
    ) -> Result<Option<SearchProjectionChangeBatch>> {
        self.build_search_projection_change_batch_after(
            search_index
                .projection_freshness()
                .source_graph_commit_epoch
                .unwrap_or(0),
            max_operations,
        )
    }

    pub fn background_maintenance_candidates(
        &self,
        search_index: Option<&SearchIndex>,
        options: BackgroundMaintenanceOptions,
    ) -> Result<Vec<BackgroundMaintenanceCandidate>> {
        Ok({
            let mut candidates = Vec::new();

            if options.include_storage_checkpoint
                && let Some(plan) =
                    self.storage_checkpoint_background_work_plan(options.hint.clone())?
            {
                candidates.push(BackgroundMaintenanceCandidate::new(
                    BackgroundMaintenanceKind::StorageCheckpoint,
                    plan,
                ));
            }

            if options.include_schema_maintenance
                && let Some(plan) =
                    self.schema_maintenance_background_work_plan(options.hint.clone())?
            {
                candidates.push(BackgroundMaintenanceCandidate::new(
                    BackgroundMaintenanceKind::SchemaMaintenance,
                    plan,
                ));
            }

            if options.include_property_index_projection
                && let Some(plan) =
                    self.property_index_projection_background_work_plan(options.hint.clone())?
            {
                candidates.push(BackgroundMaintenanceCandidate::new(
                    BackgroundMaintenanceKind::PropertyIndexProjection,
                    plan,
                ));
            }

            if options.include_optimizer_statistics_refresh
                && let Some(plan) =
                    self.optimizer_statistics_refresh_background_work_plan(options.hint.clone())?
            {
                candidates.push(BackgroundMaintenanceCandidate::new(
                    BackgroundMaintenanceKind::OptimizerStatisticsRefresh,
                    plan,
                ));
            }

            if let Some(delta_request) = &options.search_projection_graph_delta {
                let plan = match search_index {
                    Some(search_index) => self
                        .search_projection_graph_delta_freshness_background_work_plan(
                            search_index,
                            delta_request,
                            options.hint.clone(),
                        )?,
                    None => self.search_projection_graph_delta_background_work_plan(
                        delta_request,
                        options.hint.clone(),
                    ),
                };
                if let Some(plan) = plan {
                    candidates.push(
                        BackgroundMaintenanceCandidate::new(
                            BackgroundMaintenanceKind::SearchProjectionGraphDelta,
                            plan,
                        )
                        .with_search_projection_graph_delta(delta_request.clone()),
                    );
                }
            } else if options.include_search_projection_graph_delta_freshness
                && let Some(search_index) = search_index
            {
                let executable_request = self
                    .build_search_projection_graph_delta_request_from_freshness(search_index, None)
                    .ok()
                    .flatten();
                if let Some(request) = executable_request {
                    if let Some(plan) = self
                        .search_projection_graph_delta_freshness_background_work_plan(
                            search_index,
                            &request,
                            options.hint.clone(),
                        )?
                    {
                        candidates.push(
                            BackgroundMaintenanceCandidate::new(
                                BackgroundMaintenanceKind::SearchProjectionGraphDelta,
                                plan,
                            )
                            .with_search_projection_graph_delta(request),
                        );
                    }
                } else if let Some(plan) = self
                    .search_projection_freshness_lag_background_work_plan(
                        search_index,
                        options.hint.clone(),
                    )?
                {
                    candidates.push(BackgroundMaintenanceCandidate::new(
                        BackgroundMaintenanceKind::SearchProjectionGraphDelta,
                        plan,
                    ));
                }
            }

            if let Some(search_index) = search_index {
                if options.include_search_projection_rebuild {
                    let mut hint = options.hint.clone();
                    if hint.source_graph_commit_lag == 0 {
                        hint.source_graph_commit_lag = search_projection_commit_lag(
                            search_index,
                            self.runtime
                                .get()?
                                .store
                                .search_projection_changefeed_status()
                                .required_projection_commit_epoch(),
                        );
                    }
                    if let Some(plan) =
                        self.search_projection_rebuild_background_work_plan(search_index, hint)?
                    {
                        candidates.push(BackgroundMaintenanceCandidate::new(
                            BackgroundMaintenanceKind::SearchProjectionRebuild,
                            plan,
                        ));
                    }
                }

                if options.include_search_projection_metadata_repair
                    && let Some(plan) = self
                        .search_projection_metadata_repair_background_work_plan(
                            search_index,
                            options.hint.clone(),
                        )?
                {
                    candidates.push(BackgroundMaintenanceCandidate::new(
                        BackgroundMaintenanceKind::SearchProjectionMetadataRepair,
                        plan,
                    ));
                }
            }

            if options.include_hawdb_lightning_bootstrap_export
                && let Some(plan) = self
                    .hawdb_lightning_bootstrap_export_background_work_plan(options.hint.clone())?
            {
                candidates.push(BackgroundMaintenanceCandidate::new(
                    BackgroundMaintenanceKind::HawDBLightningBootstrapExport,
                    plan,
                ));
            }

            if options.include_external_content_artifact_jobs
                && let Some(plan) = self.external_content_artifact_job_background_work_plan(
                    options.hint,
                    options.external_content_artifact_estimated_operations,
                )
            {
                candidates.push(BackgroundMaintenanceCandidate::new(
                    BackgroundMaintenanceKind::ExternalContentArtifactJob,
                    plan,
                ));
            }

            candidates
        })
    }

    pub fn rank_background_maintenance(
        &self,
        search_index: Option<&SearchIndex>,
        policy: &LocalQosPolicy,
        state: &LocalQosState,
        options: BackgroundMaintenanceOptions,
    ) -> Result<Vec<RankedBackgroundMaintenance>> {
        Ok({
            let candidates = self.background_maintenance_candidates(search_index, options)?;
            let plans = candidates
                .iter()
                .map(|candidate| candidate.plan.clone())
                .collect::<Vec<_>>();
            policy
                .rank_background_work(state, &plans)
                .into_iter()
                .map(|ranked| {
                    let candidate = &candidates[ranked.index];
                    RankedBackgroundMaintenance {
                        kind: candidate.kind,
                        name: candidate.name.clone(),
                        plan: candidate.plan.clone(),
                        decision: ranked.decision,
                        search_projection_graph_delta: candidate
                            .search_projection_graph_delta
                            .clone(),
                    }
                })
                .collect()
        })
    }

    pub fn background_maintenance_summary(
        &self,
        search_index: Option<&SearchIndex>,
        policy: &LocalQosPolicy,
        state: &LocalQosState,
        options: BackgroundMaintenanceOptions,
    ) -> Result<BackgroundMaintenanceSummary> {
        Ok({
            let ranked = self.rank_background_maintenance(search_index, policy, state, options)?;
            BackgroundMaintenanceSummary::from_ranked(ranked, policy, state)
        })
    }

    /// Hydrate graph changes from the current source view. A declared complete-through
    /// watermark must equal that view's commit epoch; a changefeed prefix does not
    /// provide historical row contents. Requests without a watermark do not claim
    /// complete coverage and preserve their existing ad hoc behavior.
    pub fn build_search_projection_graph_delta(
        &self,
        request: &SearchProjectionGraphDeltaRequest,
    ) -> Result<SearchProjectionDelta> {
        let result = search_projection_graph_delta_for(
            &self.runtime.get()?.catalog,
            &self.runtime.get()?.store,
            request,
        );
        self.runtime.get()?.store.poison_on_storage_error(&result);
        result
    }

    pub fn apply_search_projection_delta(
        &self,
        search_index: &mut SearchIndex,
        delta: SearchProjectionDelta,
    ) -> Result<SearchProjectionDeltaReport> {
        search_index.apply_projection_delta(delta)
    }

    pub fn apply_search_projection_graph_delta(
        &self,
        search_index: &mut SearchIndex,
        request: SearchProjectionGraphDeltaRequest,
    ) -> Result<SearchProjectionDeltaReport> {
        let delta = self.build_search_projection_graph_delta(&request)?;
        search_index.apply_projection_delta(delta)
    }

    pub fn apply_search_projection_change_batch(
        &self,
        search_index: &mut SearchIndex,
        batch: SearchProjectionChangeBatch,
        relational: SearchProjectionRelationalDelta,
    ) -> Result<SearchProjectionDeltaReport> {
        let delta = self.build_search_projection_change_delta(batch, relational)?;
        search_index.apply_projection_delta(delta)
    }

    /// Builds the complete graph-and-host delta without publishing its watermark.
    /// Both full-residency and incremental persistence must pass this same
    /// relational coverage and source-epoch proof before applying a batch.
    pub fn build_search_projection_change_delta(
        &self,
        batch: SearchProjectionChangeBatch,
        relational: SearchProjectionRelationalDelta,
    ) -> Result<SearchProjectionDelta> {
        let expected_primary_key_count = batch
            .relational_primary_key_changes()
            .iter()
            .map(|table| table.primary_keys.len())
            .fold(0usize, usize::saturating_add);
        if relational.processed_primary_key_count != expected_primary_key_count {
            return Err(HawDBError::Storage(format!(
                "search projection relational delta processed {} primary keys, expected {expected_primary_key_count}; projection watermark was not published",
                relational.processed_primary_key_count
            )));
        }
        if relational.delta.source_graph_commit_epoch.is_some() {
            return Err(HawDBError::Storage(
                "search projection relational delta must not publish its own source epoch"
                    .to_string(),
            ));
        }
        let complete_through_commit_epoch = batch.complete_through_commit_epoch();
        let max_operations = batch.graph_delta().max_operations;
        let mut graph = self.build_search_projection_graph_delta(batch.graph_delta())?;
        let SearchProjectionDelta {
            upserts,
            deletes,
            max_operations: _,
            source_graph_commit_epoch: _,
        } = relational.delta;
        graph.upserts.extend(upserts);
        graph.deletes.extend(deletes);
        graph.max_operations = max_operations;
        graph.source_graph_commit_epoch = complete_through_commit_epoch;
        Ok(graph)
    }

    pub fn apply_background_search_projection_delta(
        &self,
        search_index: &mut SearchIndex,
        policy: &LocalQosPolicy,
        state: &LocalQosState,
        delta: SearchProjectionDelta,
    ) -> Result<SearchProjectionDeltaReport> {
        self.ensure_runtime_capability(hawdb_core::RuntimeCapability::BackgroundMaintenance)?;
        search_index.apply_background_projection_delta(policy, state, delta)
    }

    pub fn apply_background_search_projection_graph_delta(
        &self,
        search_index: &mut SearchIndex,
        policy: &LocalQosPolicy,
        state: &LocalQosState,
        request: SearchProjectionGraphDeltaRequest,
    ) -> Result<SearchProjectionDeltaReport> {
        self.ensure_runtime_capability(hawdb_core::RuntimeCapability::BackgroundMaintenance)?;
        match policy.admit(state, &request.background_work_request()) {
            QosAdmission::Admit => self.apply_search_projection_graph_delta(search_index, request),
            QosAdmission::Defer { reason, .. } => Err(HawDBError::Storage(format!(
                "background search projection graph delta deferred: {reason}"
            ))),
            QosAdmission::Reject { reason, .. } => Err(HawDBError::Storage(format!(
                "background search projection graph delta rejected: {reason}"
            ))),
        }
    }

    pub fn apply_scheduled_background_search_projection_delta(
        &self,
        search_index: &mut SearchIndex,
        delta: SearchProjectionDelta,
    ) -> Result<SearchProjectionDeltaReport> {
        self.ensure_runtime_capability(hawdb_core::RuntimeCapability::BackgroundMaintenance)?;
        let scheduler = self.local_qos_scheduler_for_work();
        search_index.apply_scheduled_background_projection_delta(&scheduler, delta)
    }

    pub fn apply_scheduled_background_search_projection_graph_delta(
        &self,
        search_index: &mut SearchIndex,
        request: SearchProjectionGraphDeltaRequest,
    ) -> Result<SearchProjectionDeltaReport> {
        self.ensure_runtime_capability(hawdb_core::RuntimeCapability::BackgroundMaintenance)?;
        let scheduler = self.local_qos_scheduler_for_work();
        let permit = match scheduler.try_start(request.background_work_request()) {
            Ok(permit) => permit,
            Err(QosAdmission::Defer { reason, .. }) => {
                return Err(HawDBError::Storage(format!(
                    "background search projection graph delta deferred: {reason}"
                )));
            }
            Err(QosAdmission::Reject { reason, .. }) => {
                return Err(HawDBError::Storage(format!(
                    "background search projection graph delta rejected: {reason}"
                )));
            }
            Err(QosAdmission::Admit) => unreachable!("admitted work returns a permit"),
        };

        let result = self.apply_search_projection_graph_delta(search_index, request);
        permit.finish_with_outcome(result.is_ok());
        result
    }

    /// Retrieves from resident search payloads, propagating search and graph/pipeline errors.
    pub fn retrieve_knowledge(
        &self,
        search_index: &SearchIndex,
        request: &KnowledgeRetrievalRequest,
    ) -> Result<KnowledgeRetrievalOutput> {
        KnowledgeRetrievalGraphContext {
            catalog: &self.runtime.get()?.catalog,
            store: &self.runtime.get()?.store,
            compressed_vector_search_mode: self.config.compressed_vector_search_mode,
            adaptive_vector_backend_policy: self.config.adaptive_vector_backend_policy,
            query_memory_budget: self.config.execution_memory.query_memory_bytes,
            result_payload_budget: self
                .config
                .max_read_result_payload_bytes
                .unwrap_or(DEFAULT_MAX_READ_RESULT_PAYLOAD_BYTES),
        }
        .retrieve_knowledge(search_index, request)
    }

    pub fn try_retrieve_knowledge(
        &self,
        search_index: &SearchIndex,
        request: &KnowledgeRetrievalRequest,
    ) -> Result<KnowledgeRetrievalOutput> {
        KnowledgeRetrievalGraphContext {
            catalog: &self.runtime.get()?.catalog,
            store: &self.runtime.get()?.store,
            compressed_vector_search_mode: self.config.compressed_vector_search_mode,
            adaptive_vector_backend_policy: self.config.adaptive_vector_backend_policy,
            query_memory_budget: self.config.execution_memory.query_memory_bytes,
            result_payload_budget: self
                .config
                .max_read_result_payload_bytes
                .unwrap_or(DEFAULT_MAX_READ_RESULT_PAYLOAD_BYTES),
        }
        .try_retrieve_knowledge(search_index, request)
    }

    pub(crate) fn try_retrieve_knowledge_from_search(
        &self,
        search: SearchResultSet,
        projection_freshness: SearchProjectionFreshness,
        request: &KnowledgeRetrievalRequest,
    ) -> Result<KnowledgeRetrievalOutput> {
        KnowledgeRetrievalGraphContext {
            catalog: &self.runtime.get()?.catalog,
            store: &self.runtime.get()?.store,
            compressed_vector_search_mode: self.config.compressed_vector_search_mode,
            adaptive_vector_backend_policy: self.config.adaptive_vector_backend_policy,
            query_memory_budget: self.config.execution_memory.query_memory_bytes,
            result_payload_budget: self
                .config
                .max_read_result_payload_bytes
                .unwrap_or(DEFAULT_MAX_READ_RESULT_PAYLOAD_BYTES),
        }
        .retrieve_knowledge_from_search(search, projection_freshness, request)
    }
}

impl Database {
    fn ensure_writable(&self) -> Result<()> {
        self.runtime.get()?.store.ensure_usable()?;
        if self.config.read_only {
            return Err(HawDBError::Execution(
                "database is opened in read-only mode".to_string(),
            ));
        }
        Ok(())
    }

    pub fn project_graph(&self, rel_type: Option<&str>) -> Result<ProjectedGraph> {
        Ok({
            let runtime = self.runtime.get()?;
            match rel_type {
                Some(name) => runtime
                    .catalog
                    .rel_type_id(name)
                    .map(|rel_type_id| {
                        ProjectedGraph::from_store(&runtime.store, Some(rel_type_id))
                    })
                    .unwrap_or_else(|| ProjectedGraph::from_store_without_edges(&runtime.store)),
                None => ProjectedGraph::from_store(&runtime.store, None),
            }
        })
    }
}

fn effective_database_config(mut config: DatabaseConfig) -> DatabaseConfig {
    config.runtime_capabilities =
        crate::compiled_capabilities::effective_runtime_capabilities(config.runtime_capabilities);
    config
}

struct KnowledgeRetrievalGraphContext<'a, S = GraphStore> {
    catalog: &'a Catalog,
    store: &'a S,
    compressed_vector_search_mode: CompressedVectorSearchMode,
    adaptive_vector_backend_policy: hawdb_optimizer::AdaptiveVectorBackendPolicy,
    query_memory_budget: NonZeroUsize,
    result_payload_budget: usize,
}

impl<S: crate::executor::ExecutionStore> KnowledgeRetrievalGraphContext<'_, S> {
    fn retrieve_knowledge(
        &self,
        search_index: &SearchIndex,
        request: &KnowledgeRetrievalRequest,
    ) -> Result<KnowledgeRetrievalOutput> {
        self.retrieve_knowledge_internal(search_index, request, false)
    }

    fn try_retrieve_knowledge(
        &self,
        search_index: &SearchIndex,
        request: &KnowledgeRetrievalRequest,
    ) -> Result<KnowledgeRetrievalOutput> {
        self.retrieve_knowledge_internal(search_index, request, true)
    }

    fn retrieve_knowledge_internal(
        &self,
        search_index: &SearchIndex,
        request: &KnowledgeRetrievalRequest,
        use_physical_range_reads: bool,
    ) -> Result<KnowledgeRetrievalOutput> {
        let search_options = SearchQueryOptions {
            limit: request.limit,
            offset: request.offset,
            rank_window: request.rank_window,
            fusion_weights: request.search_fusion_weights,
            metadata_filters: request.metadata_filters.clone(),
            policy_epoch: None,
        };
        let search = if use_physical_range_reads {
            search_index.try_search_with_options_adaptive_vector_projection(
                &request.query_text,
                request.query_embedding.as_deref(),
                request.mode,
                search_options,
                AdaptiveVectorSearchOptions::new(self.compressed_vector_search_mode)
                    .with_backend_policy(self.adaptive_vector_backend_policy),
            )?
        } else {
            search_index.search_with_options_adaptive_vector_projection(
                &request.query_text,
                request.query_embedding.as_deref(),
                request.mode,
                search_options,
                AdaptiveVectorSearchOptions::new(self.compressed_vector_search_mode)
                    .with_backend_policy(self.adaptive_vector_backend_policy),
            )?
        };
        self.retrieve_knowledge_from_search(search, search_index.projection_freshness(), request)
    }

    fn retrieve_knowledge_from_search(
        &self,
        mut search: SearchResultSet,
        projection_freshness: SearchProjectionFreshness,
        request: &KnowledgeRetrievalRequest,
    ) -> Result<KnowledgeRetrievalOutput> {
        let graph_commit_epoch =
            <S as hawdb_storage::graph_engine::GraphReadEngine>::commit_epoch(self.store);
        let mut pipeline =
            hawdb_search::knowledge_retrieval_pipeline::KnowledgeRetrievalPipelineBudget::new_with_budget_option(
                self.query_memory_budget,
                self.result_payload_budget,
                "ExecutionMemoryConfig.query_memory_bytes",
            )?;
        let graph_read_account = pipeline.working_account();
        let graph = KnowledgeRetrievalPipelineContext {
            catalog: self.catalog,
            store: self.store,
            query_memory_budget: self.query_memory_budget,
            account: &graph_read_account,
            owned: knowledge_graph_read::KnowledgeOwnedState::new(&graph_read_account)?,
        };
        pipeline.enter(KnowledgeRetrievalStage::SearchCandidate)?;
        pipeline.enter(KnowledgeRetrievalStage::MetadataFilter)?;
        let canonical_search_nodes =
            graph.canonical_search_nodes(&search, &request.metadata_filters)?;
        let search_hit_count = search.hits.len();
        search
            .hits
            .retain(|hit| canonical_search_nodes.contains_key(&hit.id));
        let canonical_identity_filtered_out_count =
            search_hit_count.saturating_sub(search.hits.len());
        let graph_seed_search = graph.search_knowledge_graph_seeds(
            &request.query_text,
            request.graph_seed_limit,
            &request.metadata_filters,
        )?;
        pipeline.enter(KnowledgeRetrievalStage::AuthorizedGraphExpand)?;
        let graph_context_search = graph.expand_knowledge_context(
            &canonical_search_nodes,
            &graph_seed_search.seeds,
            &request.metadata_filters,
            request.graph_context_limit,
            request.graph_context_max_hops,
        )?;
        let evidence = graph.knowledge_evidence_for_search(
            &search,
            &canonical_search_nodes,
            &graph_context_search.paths,
        );
        pipeline.enter(KnowledgeRetrievalStage::Rerank)?;
        let mut candidates = graph.knowledge_candidates(
            &search,
            &evidence,
            &graph_seed_search.seeds,
            &graph_context_search.paths,
            &request.candidate_scoring,
            scoring_reference_time_millis(),
        )?;
        pipeline.retain_working(knowledge_candidates_memory_bytes(&candidates))?;
        let candidate_total_count = candidates.len();
        pipeline.enter(KnowledgeRetrievalStage::TopK)?;
        let mut candidate_fanout_details = Vec::new();
        if let Some(limit) = request.candidate_limit {
            candidates.truncate(limit);
            if candidate_total_count > limit {
                candidate_fanout_details.push(KnowledgeFanoutReasonDetail::candidate_limit(
                    limit,
                    candidate_total_count,
                ));
            }
        }
        pipeline.enter(KnowledgeRetrievalStage::CanonicalHydration)?;
        let (graph_seeds, graph_context_paths, canonical_hydrated_node_count) = graph
            .hydrate_knowledge_output(
                &mut candidates,
                &graph_seed_search.seeds,
                &graph_context_search.paths,
            )?;
        let required_projection_commit_epoch =
            <S as hawdb_storage::graph_engine::GraphReadEngine>::search_projection_changefeed_status(
                self.store,
            )
            .required_projection_commit_epoch();
        let retrievers = knowledge_retriever_reports(
            &search,
            &evidence,
            &graph_seeds,
            &graph_context_paths,
            &projection_freshness,
            KnowledgeGraphSeedRetrieverInput {
                limit: request.graph_seed_limit,
                input_candidate_count: graph_seed_search.input_candidate_count,
                input_filtered_out_count: graph_seed_search.input_filtered_out_count,
                metadata_filters: request.metadata_filters.clone(),
                candidate_count: graph_seed_search.candidate_count,
                graph_commit_epoch,
            },
        );
        let mut fanout_reason_details = graph_context_search.fanout_reason_details.clone();
        fanout_reason_details.extend(graph_seed_search.fanout_reason_details.clone());
        fanout_reason_details.extend(candidate_fanout_details);
        let fanout_reason_codes = knowledge_fanout_reason_codes(&fanout_reason_details);
        let fanout_reasons = knowledge_fanout_reason_messages(&fanout_reason_details);
        let (result_memory_bytes, result_payload_bytes) =
            knowledge_retrieval_result_resource_bytes(KnowledgeRetrievalResultResources {
                search: &search,
                retrievers: &retrievers,
                candidates: &candidates,
                evidence: &evidence,
                graph_seeds: &graph_seeds,
                graph_context_paths: &graph_context_paths,
                fanout_reason_details: &fanout_reason_details,
                fanout_reasons: &fanout_reasons,
            });
        graph.owned.release_for_result(&graph_read_account)?;
        pipeline.retain_result(result_memory_bytes, result_payload_bytes)?;
        let pipeline_report = pipeline.finish(
            graph_commit_epoch,
            canonical_identity_filtered_out_count,
            canonical_hydrated_node_count,
            candidates
                .iter()
                .filter(|candidate| candidate.entity.is_some())
                .count(),
            true,
        )?;
        let diagnostics = knowledge_retrieval_diagnostics(
            &search,
            request,
            &projection_freshness,
            graph_commit_epoch,
            required_projection_commit_epoch,
            KnowledgeRetrievalDiagnosticsInput {
                graph_seed_input_candidate_set: knowledge_graph_seed_input_candidate_set_report(
                    graph_seed_search.input_candidate_count,
                    graph_commit_epoch,
                    graph_seed_search.input_filtered_out_count,
                    request.metadata_filters.clone(),
                ),
                graph_seed_candidate_set: knowledge_graph_seed_candidate_set_report(
                    graph_seed_search.seeds.len(),
                    graph_commit_epoch,
                ),
                graph_seed_candidate_count: graph_seed_search.candidate_count,
                graph_seed_returned_count: graph_seeds.len(),
                graph_context_input_candidate_set:
                    knowledge_graph_context_input_candidate_set_report(
                        graph_context_search.input_seed_count,
                        graph_commit_epoch,
                    ),
                graph_context_candidate_set: knowledge_graph_context_candidate_set_report(
                    graph_context_search.expanded_relationship_count,
                    graph_commit_epoch,
                ),
                graph_context_path_count: graph_context_paths.len(),
                graph_context_node_count: knowledge_context_path_node_count(&graph_context_paths),
                graph_context_relationship_count: graph_context_paths.len(),
                graph_context_truncation_reasons: graph_context_search.truncation_reasons.clone(),
                fanout_reason_details: fanout_reason_details.clone(),
                candidate_count: candidates.len(),
                candidate_total_count,
                pipeline: pipeline_report,
            },
        );
        Ok(KnowledgeRetrievalOutput {
            graph_commit_epoch,
            projection_freshness,
            search,
            retrievers,
            diagnostics,
            candidates,
            evidence,
            graph_seeds,
            graph_context_paths,
            fanout_reason_codes,
            fanout_reason_details,
            fanout_reasons,
        })
    }
}

/// The graph stages cannot execute without the pipeline's query ledger.
struct KnowledgeRetrievalPipelineContext<'a, S> {
    catalog: &'a Catalog,
    store: &'a S,
    query_memory_budget: NonZeroUsize,
    account: &'a hawdb_executor::QueryMemoryAccount,
    owned: knowledge_graph_read::KnowledgeOwnedState,
}

impl<S: crate::executor::ExecutionStore> KnowledgeRetrievalPipelineContext<'_, S> {
    fn expand_knowledge_context(
        &self,
        search_nodes: &BTreeMap<String, NodeId>,
        graph_seeds: &[KnowledgeGraphSeedCandidate],
        metadata_filters: &BTreeMap<String, String>,
        graph_context_limit: usize,
        graph_context_max_hops: usize,
    ) -> Result<KnowledgeGraphContextSearchOutput> {
        let temporary = knowledge_graph_read::KnowledgeOwnedState::new(self.account)?;
        let mut paths = Vec::new();
        let mut fanout_reasons = Vec::new();
        let mut truncation_reasons = Vec::new();
        let mut seen_relationships = BTreeSet::new();
        let mut seen_frontier_nodes = BTreeSet::new();
        let mut reported_dense_groups = BTreeSet::new();
        let mut frontier = VecDeque::new();

        for (hit_id, seed_node) in search_nodes {
            let _lookup = self.account.reserve(hit_id.len())?;
            let key = (hit_id.clone(), seed_node.0);
            if !seen_frontier_nodes.contains(&key) {
                temporary.reserve(hit_id.len().saturating_mul(2).saturating_add(
                    128 + 11 * std::mem::size_of::<(String, u64)>()
                        + 2 * std::mem::size_of::<(String, NodeId, usize)>(),
                ))?;
                seen_frontier_nodes.insert(key);
                frontier.reserve_exact(1);
                frontier.push_back((hit_id.clone(), *seed_node, 0usize));
            }
        }
        for seed in graph_seeds {
            let seed_node = seed.node_id;
            let _lookup = self.account.reserve(seed.id.len())?;
            let key = (seed.id.clone(), seed_node.0);
            if !seen_frontier_nodes.contains(&key) {
                temporary.reserve(seed.id.len().saturating_mul(2).saturating_add(
                    128 + 11 * std::mem::size_of::<(String, u64)>()
                        + 2 * std::mem::size_of::<(String, NodeId, usize)>(),
                ))?;
                let seed_id = graph_seed_candidate_id_internal(seed);
                seen_frontier_nodes.insert(key);
                frontier.reserve_exact(1);
                frontier.push_back((seed_id, seed_node, 0usize));
            }
        }
        let input_seed_count = seen_frontier_nodes.len();

        while let Some((seed_hit_id, current_node, depth)) = frontier.pop_front() {
            if depth >= graph_context_max_hops {
                continue;
            }
            record_dense_adjacency_diagnostics(
                DenseAdjacencyDiagnosticContext {
                    catalog: self.catalog,
                    store: self.store,
                    operation: "graph_context",
                    relationship_type: None,
                },
                current_node,
                &mut reported_dense_groups,
                &mut fanout_reasons,
            );
            let control = knowledge_graph_read::visit_expansion_edges(
                self.store,
                current_node,
                self.query_memory_budget.get(),
                self.account,
                |edge| {
                    if !self.graph_expansion_node_is_authorized(edge.next_node, metadata_filters)? {
                        return Ok(hawdb_executor::store::ScanControl::Continue);
                    }
                    let _lookup = self.account.reserve(seed_hit_id.len())?;
                    let relationship_key = (seed_hit_id.clone(), edge.relationship.id.0);
                    if seen_relationships.contains(&relationship_key) {
                        return Ok(hawdb_executor::store::ScanControl::Continue);
                    }
                    temporary.reserve(
                        seed_hit_id
                            .len()
                            .saturating_add(128 + 11 * std::mem::size_of::<(String, u64)>()),
                    )?;
                    seen_relationships.insert(relationship_key);
                    if paths.len() >= graph_context_limit {
                        let detail = KnowledgeFanoutReasonDetail::graph_context_limit(
                            graph_context_limit,
                            &seed_hit_id,
                        );
                        truncation_reasons.push(detail.message.clone());
                        fanout_reasons.push(detail);
                        return Ok(hawdb_executor::store::ScanControl::Stop);
                    }
                    self.owned.reserve_vec(&mut paths)?;
                    self.owned.reserve(seed_hit_id.len())?;
                    paths.push(KnowledgeGraphContextCandidate {
                        seed_hit_id: seed_hit_id.clone(),
                        hop: depth + 1,
                        direction: edge.direction,
                        relationship_id: edge.relationship.id,
                        source_node_id: edge.relationship.source,
                        target_node_id: edge.relationship.target,
                    });
                    let _lookup = self.account.reserve(seed_hit_id.len())?;
                    let frontier_key = (seed_hit_id.clone(), edge.next_node.0);
                    if !seen_frontier_nodes.contains(&frontier_key) {
                        temporary.reserve(seed_hit_id.len().saturating_mul(2).saturating_add(
                            128 + 11 * std::mem::size_of::<(String, u64)>()
                                + 2 * std::mem::size_of::<(String, NodeId, usize)>(),
                        ))?;
                        seen_frontier_nodes.insert(frontier_key);
                        frontier.reserve_exact(1);
                        frontier.push_back((seed_hit_id.clone(), edge.next_node, depth + 1));
                    }
                    Ok(hawdb_executor::store::ScanControl::Continue)
                },
            )?;
            if control == hawdb_executor::store::ScanControl::Stop {
                let expanded_relationship_count = paths.len();
                return Ok(KnowledgeGraphContextSearchOutput {
                    paths,
                    input_seed_count,
                    expanded_relationship_count,
                    fanout_reason_details: fanout_reasons,
                    truncation_reasons,
                });
            }
        }

        let expanded_relationship_count = paths.len();
        Ok(KnowledgeGraphContextSearchOutput {
            paths,
            input_seed_count,
            expanded_relationship_count,
            fanout_reason_details: fanout_reasons,
            truncation_reasons,
        })
    }

    fn graph_expansion_node_is_authorized(
        &self,
        node_id: NodeId,
        metadata_filters: &BTreeMap<String, String>,
    ) -> Result<bool> {
        let scope_filters = knowledge_graph_expansion_scope_filters(metadata_filters);
        if scope_filters.is_empty() {
            return Ok(true);
        }
        let Some(node) = knowledge_graph_read::node(self.store, node_id, None, self.account)?
        else {
            return Ok(false);
        };
        knowledge_graph_filters::PreparedKnowledgeFilter::new(&scope_filters, self.account)?
            .matches(
                self.catalog,
                self.store,
                &node.node,
                self.account,
                self.query_memory_budget.get(),
            )
    }

    fn seed_node_for_hit(
        &self,
        kind: Option<&str>,
        external_id: Option<&str>,
        metadata_filters: &BTreeMap<String, String>,
    ) -> Result<Option<knowledge_graph_read::KnowledgeReadNode>> {
        let Some(external_id) = external_id else {
            return Ok(None);
        };
        let Some(label) = kind.and_then(search_kind_to_label) else {
            return Ok(None);
        };
        let Some(node) = knowledge_graph_read::seed_by_external_id(
            self.catalog,
            self.store,
            label,
            external_id,
            self.account,
        )?
        else {
            return Ok(None);
        };
        if !knowledge_graph_filters::PreparedKnowledgeFilter::new(metadata_filters, self.account)?
            .matches(
            self.catalog,
            self.store,
            &node.node,
            self.account,
            self.query_memory_budget.get(),
        )? {
            return Ok(None);
        }
        Ok(Some(node))
    }

    fn canonical_search_nodes(
        &self,
        search: &SearchResultSet,
        metadata_filters: &BTreeMap<String, String>,
    ) -> Result<BTreeMap<String, NodeId>> {
        let scope_filters = knowledge_graph_expansion_scope_filters(metadata_filters);
        let mut nodes = BTreeMap::new();
        for hit in &search.hits {
            if let Some(node) = self.seed_node_for_hit(
                hit.kind.as_deref(),
                hit.external_id.as_deref(),
                &scope_filters,
            )? {
                self.owned.reserve(
                    hit.id
                        .len()
                        .saturating_add(128 + 11 * std::mem::size_of::<(String, NodeId)>()),
                )?;
                nodes.insert(hit.id.clone(), node.node.id);
            }
        }
        Ok(nodes)
    }

    fn knowledge_entity_from_node(&self, node: &NodeRecord) -> Result<KnowledgeEntity> {
        let labels = node
            .labels
            .iter()
            .filter_map(|id| self.catalog.label_name(*id))
            .fold(0usize, |bytes, label| {
                bytes.saturating_add(label.len() + std::mem::size_of::<String>())
            });
        self.owned.reserve(
            hawdb_core::ids::node_allocation_bytes(node)
                .saturating_mul(2)
                .saturating_add(labels)
                .saturating_add(128),
        )?;
        Ok(knowledge_entity_from_node(self.catalog, node))
    }

    fn knowledge_evidence_for_search(
        &self,
        search: &SearchResultSet,
        canonical_search_nodes: &BTreeMap<String, NodeId>,
        graph_context_paths: &[KnowledgeGraphContextCandidate],
    ) -> Vec<KnowledgeEvidence> {
        let mut evidence = Vec::with_capacity(search.hits.len());
        for hit in &search.hits {
            let canonical_node_id = canonical_search_nodes.get(&hit.id).map(|node_id| node_id.0);
            let graph_context_path_count = graph_context_paths
                .iter()
                .filter(|path| path.seed_hit_id == hit.id)
                .count();
            evidence.push(KnowledgeEvidence {
                hit_id: hit.id.clone(),
                kind: hit.kind.clone(),
                external_id: hit.external_id.clone(),
                source_id: hit.source_id.clone(),
                canonical_node_id,
                graph_context_path_count,
                matched_terms: hit.matched_terms.clone(),
                matched_spans: hit.matched_spans.clone(),
                score: hit.score,
                rrf_score: hit.rrf_score,
                vector_rrf_score: hit.vector_rrf_score,
                text_rrf_score: hit.text_rrf_score,
                vector_score: hit.vector_score,
                text_score: hit.text_score,
                vector_rank: hit.vector_rank,
                text_rank: hit.text_rank,
            });
        }
        evidence
    }

    /// Score-breakdown inputs for one rerank candidate. Canonical node records
    /// are only loaded when the policy asks for property features.
    fn knowledge_candidate_score_breakdown_for(
        &self,
        scoring: &KnowledgeCandidateScoringPolicy,
        needs_properties: bool,
        search_score: Option<f64>,
        graph_seed_score: Option<f64>,
        node_id: Option<NodeId>,
        reference_time_millis: u64,
    ) -> Result<KnowledgeCandidateScoreBreakdown> {
        let node = if needs_properties {
            node_id
                .map(|node_id| knowledge_graph_read::node(self.store, node_id, None, self.account))
                .transpose()?
                .flatten()
        } else {
            None
        };
        let source = KnowledgeCandidateFeatureSource {
            search_score,
            graph_seed_score,
            // Every candidate is a retrieval seed or a graph seed, so its
            // distance from a seed is zero; expanded nodes only appear as
            // graph context today.
            hop_distance: Some(0),
            node: node.as_ref().map(|node| &node.node),
        };
        Ok(knowledge_candidate_score_breakdown(
            &source,
            scoring,
            reference_time_millis,
        ))
    }

    fn knowledge_candidates(
        &self,
        search: &SearchResultSet,
        evidence: &[KnowledgeEvidence],
        graph_seeds: &[KnowledgeGraphSeedCandidate],
        graph_context_paths: &[KnowledgeGraphContextCandidate],
        scoring: &KnowledgeCandidateScoringPolicy,
        reference_time_millis: u64,
    ) -> Result<Vec<KnowledgeCandidate>> {
        let needs_properties = matches!(
            scoring,
            KnowledgeCandidateScoringPolicy::Spec(spec)
                if spec.needs_canonical_node_properties()
        );
        let mut candidates = Vec::with_capacity(search.hits.len());
        for (index, (hit, evidence)) in search.hits.iter().zip(evidence.iter()).enumerate() {
            let score_breakdown = self.knowledge_candidate_score_breakdown_for(
                scoring,
                needs_properties,
                Some(hit.score),
                None,
                evidence.canonical_node_id.map(NodeId),
                reference_time_millis,
            )?;
            candidates.push(KnowledgeCandidate {
                id: hit.id.clone(),
                canonical_node_id: evidence.canonical_node_id,
                source: KnowledgeCandidateSource::SearchHit,
                source_rank: index + 1,
                merged_sources: vec![KnowledgeCandidateSource::SearchHit],
                score: score_breakdown.combined_score,
                score_breakdown,
                entity: None,
                evidence: Some(evidence.clone()),
                matched_properties: Vec::new(),
                graph_context_path_count: evidence.graph_context_path_count,
            });
        }

        for (index, seed) in graph_seeds.iter().enumerate() {
            let seed_candidate_id = graph_seed_candidate_id_internal(seed);
            let seed_graph_context_path_count = graph_context_paths
                .iter()
                .filter(|path| path.seed_hit_id == seed_candidate_id)
                .count();
            if let Some(candidate) = candidates
                .iter_mut()
                .find(|candidate| candidate.canonical_node_id == Some(seed.node_id.0))
            {
                candidate.score_breakdown = self.knowledge_candidate_score_breakdown_for(
                    scoring,
                    needs_properties,
                    candidate.score_breakdown.search_score,
                    Some(seed.score),
                    Some(seed.node_id),
                    reference_time_millis,
                )?;
                candidate.score = candidate.score_breakdown.combined_score;
                if !candidate
                    .merged_sources
                    .contains(&KnowledgeCandidateSource::GraphSeed)
                {
                    candidate
                        .merged_sources
                        .push(KnowledgeCandidateSource::GraphSeed);
                }
                for property in &seed.matched_properties {
                    if !candidate.matched_properties.contains(property) {
                        candidate.matched_properties.push(property.clone());
                    }
                }
                candidate.graph_context_path_count += seed_graph_context_path_count;
                continue;
            }
            let score_breakdown = self.knowledge_candidate_score_breakdown_for(
                scoring,
                needs_properties,
                None,
                Some(seed.score),
                Some(seed.node_id),
                reference_time_millis,
            )?;
            candidates.push(KnowledgeCandidate {
                id: seed_candidate_id,
                canonical_node_id: Some(seed.node_id.0),
                source: KnowledgeCandidateSource::GraphSeed,
                source_rank: index + 1,
                merged_sources: vec![KnowledgeCandidateSource::GraphSeed],
                score: score_breakdown.combined_score,
                score_breakdown,
                entity: None,
                evidence: None,
                matched_properties: seed.matched_properties.clone(),
                graph_context_path_count: seed_graph_context_path_count,
            });
        }

        candidates.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.source_rank.cmp(&right.source_rank))
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(candidates)
    }

    fn hydrate_knowledge_output(
        &self,
        candidates: &mut [KnowledgeCandidate],
        graph_seeds: &[KnowledgeGraphSeedCandidate],
        graph_context_paths: &[KnowledgeGraphContextCandidate],
    ) -> Result<(
        Vec<KnowledgeGraphSeed>,
        Vec<KnowledgeGraphContextPath>,
        usize,
    )> {
        let temporary = knowledge_graph_read::KnowledgeOwnedState::new(self.account)?;
        let mut node_ids = BTreeSet::new();
        for id in candidates
            .iter()
            .filter_map(|candidate| candidate.canonical_node_id.map(NodeId))
            .chain(graph_seeds.iter().map(|seed| seed.node_id))
            .chain(
                graph_context_paths
                    .iter()
                    .flat_map(|path| [path.source_node_id, path.target_node_id]),
            )
        {
            if !node_ids.contains(&id) {
                temporary.reserve(128 + 11 * std::mem::size_of::<NodeId>())?;
                node_ids.insert(id);
            }
        }
        let entities = self.hydrate_knowledge_entities(&node_ids)?;

        for candidate in candidates {
            if let Some(entity) = candidate
                .canonical_node_id
                .and_then(|id| entities.get(&NodeId(id)))
            {
                self.owned
                    .reserve(knowledge_entity_allocation_bytes(entity))?;
                candidate.entity = Some(entity.clone());
            }
        }
        let mut hydrated_seeds = Vec::new();
        for seed in graph_seeds {
            let entity = entities.get(&seed.node_id).ok_or_else(|| {
                HawDBError::StorageIntegrity(format!(
                    "knowledge retrieval graph seed references missing canonical node {}",
                    seed.node_id.0,
                ))
            })?;
            self.owned.reserve_vec(&mut hydrated_seeds)?;
            self.owned.reserve(
                knowledge_entity_allocation_bytes(entity)
                    .saturating_add(string_slice_bytes(&seed.matched_properties))
                    .saturating_add(
                        seed.matched_properties
                            .len()
                            .saturating_mul(std::mem::size_of::<String>()),
                    ),
            )?;
            hydrated_seeds.push(KnowledgeGraphSeed {
                entity: entity.clone(),
                score: seed.score,
                matched_properties: seed.matched_properties.clone(),
            });
        }
        let mut hydrated_paths = Vec::new();
        for path in graph_context_paths {
            self.owned.reserve_vec(&mut hydrated_paths)?;
            hydrated_paths.push(self.hydrate_graph_context_path(path, &entities)?);
        }
        Ok((hydrated_seeds, hydrated_paths, entities.len()))
    }

    fn hydrate_knowledge_entities(
        &self,
        node_ids: &BTreeSet<NodeId>,
    ) -> Result<BTreeMap<NodeId, KnowledgeEntity>> {
        let mut entities = BTreeMap::new();
        for node_id in node_ids {
            let Some(node) = knowledge_graph_read::node(self.store, *node_id, None, self.account)?
            else {
                return Err(HawDBError::StorageIntegrity(format!(
                    "knowledge retrieval canonical hydration references missing node {}",
                    node_id.0
                )));
            };
            self.owned
                .reserve(128 + 11 * std::mem::size_of::<(NodeId, KnowledgeEntity)>())?;
            entities.insert(*node_id, self.knowledge_entity_from_node(&node.node)?);
        }
        Ok(entities)
    }

    fn hydrate_graph_context_path(
        &self,
        path: &KnowledgeGraphContextCandidate,
        entities: &BTreeMap<NodeId, KnowledgeEntity>,
    ) -> Result<KnowledgeGraphContextPath> {
        let relationship =
            knowledge_graph_read::relationship(self.store, path.relationship_id, self.account)?
                .ok_or_else(|| {
                    HawDBError::StorageIntegrity(format!(
                        "knowledge retrieval graph context references missing relationship {}",
                        path.relationship_id.0
                    ))
                })?;
        let source = entities.get(&path.source_node_id).ok_or_else(|| {
            HawDBError::StorageIntegrity(format!(
                "knowledge retrieval graph context references missing source node {}",
                path.source_node_id.0
            ))
        })?;
        let target = entities.get(&path.target_node_id).ok_or_else(|| {
            HawDBError::StorageIntegrity(format!(
                "knowledge retrieval graph context references missing target node {}",
                path.target_node_id.0
            ))
        })?;
        let labels =
            string_slice_bytes(&source.labels).saturating_add(string_slice_bytes(&target.labels));
        let label_slots = source
            .labels
            .len()
            .saturating_add(target.labels.len())
            .saturating_mul(std::mem::size_of::<String>());
        self.owned.reserve(
            path.seed_hit_id
                .len()
                .saturating_add(labels)
                .saturating_add(label_slots)
                .saturating_add(option_string_bytes(&source.external_id))
                .saturating_add(option_string_bytes(&target.external_id))
                .saturating_add(
                    self.catalog
                        .rel_type_name(relationship.relationship.rel_type)
                        .unwrap_or("<unknown>")
                        .len(),
                ),
        )?;
        self.owned.retain_source(relationship._allocation)?;
        let relationship = relationship.relationship;
        Ok(KnowledgeGraphContextPath {
            seed_hit_id: path.seed_hit_id.clone(),
            hop: path.hop,
            direction: path.direction,
            relationship_id: path.relationship_id.0,
            relationship_type: self
                .catalog
                .rel_type_name(relationship.rel_type)
                .unwrap_or("<unknown>")
                .to_string(),
            relationship_properties: relationship.properties,
            source_node_id: path.source_node_id.0,
            source_labels: source.labels.clone(),
            source_external_id: source.external_id.clone(),
            target_node_id: path.target_node_id.0,
            target_labels: target.labels.clone(),
            target_external_id: target.external_id.clone(),
        })
    }

    fn search_knowledge_graph_seeds(
        &self,
        query_text: &str,
        limit: usize,
        metadata_filters: &BTreeMap<String, String>,
    ) -> Result<KnowledgeGraphSeedSearchOutput> {
        use hawdb_executor::graph_seed::GraphSeedScorer;
        use hawdb_executor::store::{admit_graph_read, ScanControl};
        use hawdb_storage::read_view::{AdmittedVec, GraphReadAdmission};

        let account = self.account;
        if limit == 0 {
            return Ok(KnowledgeGraphSeedSearchOutput::default());
        }
        let scorer = GraphSeedScorer::new(query_text, account, None)?;
        if scorer.is_empty() {
            return Ok(KnowledgeGraphSeedSearchOutput::default());
        }
        let filter =
            knowledge_graph_filters::PreparedKnowledgeFilter::new(metadata_filters, account)?;
        let mut allocate_slots = |bytes| admit_graph_read(account, None, bytes);
        let mut scored = AdmittedVec::<KnowledgeGraphSeedCandidate>::new(
            &GraphReadAdmission::new(&mut allocate_slots),
        )?;
        let mut input_candidate_count = 0usize;
        let mut input_filtered_out_count = 0usize;
        let mut candidate_count = 0usize;
        self.store.visit_nodes_with_allocation(
            None,
            &mut hawdb_storage::read_view::ControlledGraphReadAllocator::new(
                &mut |bytes| admit_graph_read(account, None, bytes).map(Some),
                None,
            ),
            &mut |input| {
                let (node, _input_allocation) = input.into_parts();
                if !filter.matches(
                    self.catalog,
                    self.store,
                    &node,
                    account,
                    self.query_memory_budget.get(),
                )? {
                    input_filtered_out_count = input_filtered_out_count.saturating_add(1);
                    return Ok(ScanControl::Continue);
                }
                input_candidate_count = input_candidate_count.saturating_add(1);
                let relevance = scorer.score(&node)?;
                if relevance.score <= 0.0 {
                    return Ok(ScanControl::Continue);
                }
                candidate_count = candidate_count.saturating_add(1);
                // Select before allocating owned candidate ids and property names.
                let replace = if scored.as_slice().len() < limit {
                    None
                } else {
                    let (index, worst) = scored
                        .as_slice()
                        .iter()
                        .enumerate()
                        .max_by(|(_, left), (_, right)| knowledge_graph_seed_order(left, right))
                        .expect("nonzero seed window is full");
                    if relevance.score < worst.score
                        || (relevance.score == worst.score && node.id >= worst.node_id)
                    {
                        return Ok(ScanControl::Continue);
                    }
                    Some(index)
                };
                let seed = graph_seed_candidate_from_node(self.catalog, &node, relevance, account)?;
                if let Some(index) = replace {
                    scored.as_mut_slice()[index] = seed;
                } else {
                    scored.try_push(seed)?;
                }
                Ok(ScanControl::Continue)
            },
        )?;
        let (mut seeds, slots) = scored.into_parts();
        seeds.sort_unstable_by(knowledge_graph_seed_order);
        // The one bounded diagnostic contains at most two usize renderings.
        let report_allocation = account.reserve(if candidate_count > limit {
            std::mem::size_of::<KnowledgeFanoutReasonDetail>() + 256
        } else {
            0
        })?;
        let fanout_reason_details = if candidate_count > limit {
            vec![KnowledgeFanoutReasonDetail::graph_seed_limit(
                limit,
                candidate_count,
            )]
        } else {
            Vec::new()
        };
        Ok(KnowledgeGraphSeedSearchOutput {
            seeds,
            input_candidate_count,
            input_filtered_out_count,
            candidate_count,
            fanout_reason_details,
            _slots: Some(slots),
            _report_allocation: Some(report_allocation),
        })
    }
}

#[derive(Debug, Clone, Default)]
struct KnowledgeGraphContextSearchOutput {
    paths: Vec<KnowledgeGraphContextCandidate>,
    input_seed_count: usize,
    expanded_relationship_count: usize,
    fanout_reason_details: Vec<KnowledgeFanoutReasonDetail>,
    truncation_reasons: Vec<String>,
}

#[derive(Default)]
struct KnowledgeGraphSeedSearchOutput {
    seeds: Vec<KnowledgeGraphSeedCandidate>,
    _slots: Option<Box<dyn hawdb_storage::read_view::GraphReadAllocation>>,
    _report_allocation: Option<hawdb_executor::QueryMemoryLease>,
    input_candidate_count: usize,
    input_filtered_out_count: usize,
    candidate_count: usize,
    fanout_reason_details: Vec<KnowledgeFanoutReasonDetail>,
}

#[derive(Debug)]
struct KnowledgeGraphSeedCandidate {
    _allocation: hawdb_executor::QueryMemoryLease,
    id: String,
    node_id: NodeId,
    score: f64,
    matched_properties: Vec<String>,
}

#[derive(Debug, Clone)]
struct KnowledgeGraphContextCandidate {
    seed_hit_id: String,
    hop: usize,
    direction: KnowledgeGraphPathDirection,
    relationship_id: RelId,
    source_node_id: NodeId,
    target_node_id: NodeId,
}

fn knowledge_graph_seed_order(
    left: &KnowledgeGraphSeedCandidate,
    right: &KnowledgeGraphSeedCandidate,
) -> std::cmp::Ordering {
    right
        .score
        .total_cmp(&left.score)
        .then(left.node_id.cmp(&right.node_id))
}

fn knowledge_candidates_memory_bytes(candidates: &[KnowledgeCandidate]) -> usize {
    std::mem::size_of_val(candidates).saturating_add(
        candidates
            .iter()
            .map(knowledge_candidate_resource_bytes)
            .fold(0usize, usize::saturating_add),
    )
}

struct KnowledgeRetrievalResultResources<'a> {
    search: &'a SearchResultSet,
    retrievers: &'a [KnowledgeRetrieverReport],
    candidates: &'a [KnowledgeCandidate],
    evidence: &'a [KnowledgeEvidence],
    graph_seeds: &'a [KnowledgeGraphSeed],
    graph_context_paths: &'a [KnowledgeGraphContextPath],
    fanout_reason_details: &'a [KnowledgeFanoutReasonDetail],
    fanout_reasons: &'a [String],
}

fn knowledge_retrieval_result_resource_bytes(
    resources: KnowledgeRetrievalResultResources<'_>,
) -> (usize, usize) {
    let payload_bytes = resources
        .search
        .hits
        .iter()
        .map(search_hit_payload_bytes)
        .chain(
            resources
                .retrievers
                .iter()
                .map(knowledge_retriever_payload_bytes),
        )
        .chain(
            resources
                .candidates
                .iter()
                .map(knowledge_candidate_payload_bytes),
        )
        .chain(
            resources
                .evidence
                .iter()
                .map(knowledge_evidence_payload_bytes),
        )
        .chain(
            resources
                .graph_seeds
                .iter()
                .map(knowledge_graph_seed_payload_bytes),
        )
        .chain(
            resources
                .graph_context_paths
                .iter()
                .map(knowledge_graph_context_path_payload_bytes),
        )
        .chain(
            resources
                .fanout_reason_details
                .iter()
                .map(knowledge_fanout_detail_payload_bytes),
        )
        .fold(
            string_slice_bytes(resources.fanout_reasons),
            usize::saturating_add,
        );
    let item_count = resources
        .search
        .hits
        .len()
        .saturating_add(resources.retrievers.len())
        .saturating_add(resources.candidates.len())
        .saturating_add(resources.evidence.len())
        .saturating_add(resources.graph_seeds.len())
        .saturating_add(resources.graph_context_paths.len())
        .saturating_add(resources.fanout_reason_details.len())
        .saturating_add(resources.fanout_reasons.len());
    let memory_bytes = payload_bytes
        .saturating_add(std::mem::size_of::<KnowledgeRetrievalOutput>())
        .saturating_add(item_count.saturating_mul(std::mem::size_of::<usize>() * 4));
    (memory_bytes, payload_bytes)
}

fn string_slice_bytes(values: &[String]) -> usize {
    values.iter().fold(0usize, |bytes, value| {
        bytes
            .saturating_add(std::mem::size_of::<String>())
            .saturating_add(value.len())
    })
}

fn option_string_bytes(value: &Option<String>) -> usize {
    value.as_ref().map_or(0, |value| value.len())
}

fn matched_span_payload_bytes(span: &SearchMatchedSpan) -> usize {
    span.field
        .len()
        .saturating_add(span.text.len())
        .saturating_add(span.term.len())
}

fn search_hit_payload_bytes(hit: &crate::search::SearchHit) -> usize {
    hit.id
        .len()
        .saturating_add(option_string_bytes(&hit.kind))
        .saturating_add(option_string_bytes(&hit.external_id))
        .saturating_add(option_string_bytes(&hit.source_id))
        .saturating_add(string_slice_bytes(&hit.matched_terms))
        .saturating_add(
            hit.matched_spans
                .iter()
                .map(matched_span_payload_bytes)
                .fold(0usize, usize::saturating_add),
        )
        .saturating_add(string_slice_bytes(&hit.fallback_reasons))
}

fn knowledge_entity_allocation_bytes(entity: &KnowledgeEntity) -> usize {
    std::mem::size_of::<KnowledgeEntity>()
        .saturating_add(string_slice_bytes(&entity.labels))
        .saturating_add(
            entity
                .labels
                .len()
                .saturating_mul(std::mem::size_of::<String>()),
        )
        .saturating_add(option_string_bytes(&entity.external_id))
        .saturating_add(hawdb_executor::binding::map_memory_bytes(
            &entity.properties,
        ))
}

fn knowledge_entity_payload_bytes(entity: &KnowledgeEntity) -> usize {
    string_slice_bytes(&entity.labels)
        .saturating_add(option_string_bytes(&entity.external_id))
        .saturating_add(hawdb_executor::binding::map_payload_bytes(
            &entity.properties,
        ))
}

fn knowledge_evidence_payload_bytes(evidence: &KnowledgeEvidence) -> usize {
    evidence
        .hit_id
        .len()
        .saturating_add(option_string_bytes(&evidence.kind))
        .saturating_add(option_string_bytes(&evidence.external_id))
        .saturating_add(option_string_bytes(&evidence.source_id))
        .saturating_add(string_slice_bytes(&evidence.matched_terms))
        .saturating_add(
            evidence
                .matched_spans
                .iter()
                .map(matched_span_payload_bytes)
                .fold(0usize, usize::saturating_add),
        )
}

fn knowledge_candidate_payload_bytes(candidate: &KnowledgeCandidate) -> usize {
    candidate
        .id
        .len()
        .saturating_add(string_slice_bytes(&candidate.matched_properties))
        .saturating_add(
            candidate
                .entity
                .as_ref()
                .map_or(0, knowledge_entity_payload_bytes),
        )
        .saturating_add(
            candidate
                .evidence
                .as_ref()
                .map_or(0, knowledge_evidence_payload_bytes),
        )
}

fn knowledge_candidate_resource_bytes(candidate: &KnowledgeCandidate) -> usize {
    std::mem::size_of::<KnowledgeCandidate>()
        .saturating_add(knowledge_candidate_payload_bytes(candidate))
        .saturating_add(
            candidate
                .merged_sources
                .len()
                .saturating_mul(std::mem::size_of::<KnowledgeCandidateSource>()),
        )
}

fn knowledge_graph_seed_payload_bytes(seed: &KnowledgeGraphSeed) -> usize {
    knowledge_entity_payload_bytes(&seed.entity)
        .saturating_add(string_slice_bytes(&seed.matched_properties))
}

fn knowledge_graph_context_path_payload_bytes(path: &KnowledgeGraphContextPath) -> usize {
    path.seed_hit_id
        .len()
        .saturating_add(path.relationship_type.len())
        .saturating_add(hawdb_executor::binding::map_payload_bytes(
            &path.relationship_properties,
        ))
        .saturating_add(string_slice_bytes(&path.source_labels))
        .saturating_add(option_string_bytes(&path.source_external_id))
        .saturating_add(string_slice_bytes(&path.target_labels))
        .saturating_add(option_string_bytes(&path.target_external_id))
}

fn knowledge_retriever_payload_bytes(report: &KnowledgeRetrieverReport) -> usize {
    report
        .name
        .len()
        .saturating_add(report.backend.len())
        .saturating_add(string_slice_bytes(&report.fallback_reasons))
        .saturating_add(string_slice_bytes(&report.truncation_reasons))
        .saturating_add(
            report
                .top_candidates
                .iter()
                .fold(0usize, |bytes, candidate| {
                    bytes
                        .saturating_add(candidate.id.len())
                        .saturating_add(option_string_bytes(&candidate.kind))
                        .saturating_add(option_string_bytes(&candidate.external_id))
                        .saturating_add(option_string_bytes(&candidate.source_id))
                        .saturating_add(
                            candidate
                                .matched_spans
                                .iter()
                                .map(matched_span_payload_bytes)
                                .fold(0usize, usize::saturating_add),
                        )
                }),
        )
}

fn knowledge_fanout_detail_payload_bytes(detail: &KnowledgeFanoutReasonDetail) -> usize {
    detail
        .message
        .len()
        .saturating_add(option_string_bytes(&detail.operation))
        .saturating_add(option_string_bytes(&detail.seed_hit_id))
        .saturating_add(option_string_bytes(&detail.relationship_type))
        .saturating_add(option_string_bytes(&detail.direction))
}

fn knowledge_retriever_reports(
    search: &SearchResultSet,
    evidence: &[KnowledgeEvidence],
    graph_seeds: &[KnowledgeGraphSeed],
    graph_context_paths: &[KnowledgeGraphContextPath],
    projection_freshness: &SearchProjectionFreshness,
    graph_seed_input: KnowledgeGraphSeedRetrieverInput,
) -> Vec<KnowledgeRetrieverReport> {
    let evidence_by_hit = evidence
        .iter()
        .map(|evidence| (evidence.hit_id.as_str(), evidence))
        .collect::<BTreeMap<_, _>>();
    let mut reports = search
        .retrievers
        .iter()
        .map(|report| KnowledgeRetrieverReport {
            name: report.name.clone(),
            backend: report.backend.clone(),
            available: report.available,
            input_candidate_set: report.input_candidate_set.clone(),
            candidate_count: report.candidate_count,
            candidate_set: report.candidate_set.clone(),
            limit: Some(search.limit),
            rank_window: search.rank_window,
            fusion_weight: knowledge_search_retriever_fusion_weight(
                report.name.as_str(),
                search.fusion_weights,
            ),
            fallback_reason_codes: report.fallback_reason_codes.clone(),
            knowledge_fallback_reason_codes: Vec::new(),
            fallback_reasons: report.fallback_reasons.clone(),
            truncated: report.candidate_count > report.top_candidates.len(),
            truncation_reason_codes: knowledge_search_retriever_truncation_reason_codes(
                report.candidate_count,
                report.top_candidates.len(),
                search.limit,
                search.rank_window,
            ),
            truncation_reasons: knowledge_search_retriever_truncation_reasons(
                report.name.as_str(),
                report.candidate_count,
                report.top_candidates.len(),
                search.limit,
                search.rank_window,
            ),
            top_candidates: report
                .top_candidates
                .iter()
                .map(|candidate| {
                    let evidence = evidence_by_hit.get(candidate.id.as_str()).copied();
                    KnowledgeRetrieverCandidate {
                        id: candidate.id.clone(),
                        kind: evidence.and_then(|evidence| evidence.kind.clone()),
                        external_id: evidence.and_then(|evidence| evidence.external_id.clone()),
                        source_id: evidence.and_then(|evidence| evidence.source_id.clone()),
                        canonical_node_id: evidence.and_then(|evidence| evidence.canonical_node_id),
                        rank: candidate.rank,
                        score: candidate.score,
                        matched_spans: evidence
                            .map(|evidence| evidence.matched_spans.clone())
                            .unwrap_or_default(),
                        graph_context_path_count: evidence
                            .map(|evidence| evidence.graph_context_path_count)
                            .unwrap_or_default(),
                        projection_freshness: Some(projection_freshness.clone()),
                    }
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    reports.push(KnowledgeRetrieverReport {
        name: "graph_seed".to_string(),
        backend: "graph_seed_expand".to_string(),
        available: graph_seed_input.limit > 0,
        input_candidate_set: knowledge_graph_seed_input_candidate_set_report(
            graph_seed_input.input_candidate_count,
            graph_seed_input.graph_commit_epoch,
            graph_seed_input.input_filtered_out_count,
            graph_seed_input.metadata_filters,
        ),
        candidate_count: graph_seed_input.candidate_count,
        candidate_set: knowledge_graph_seed_candidate_set_report(
            graph_seeds.len(),
            graph_seed_input.graph_commit_epoch,
        ),
        limit: Some(graph_seed_input.limit),
        rank_window: None,
        fusion_weight: None,
        fallback_reason_codes: Vec::new(),
        knowledge_fallback_reason_codes: knowledge_graph_seed_fallback_reason_codes(
            graph_seed_input.limit,
        ),
        fallback_reasons: knowledge_graph_seed_fallback_reasons(graph_seed_input.limit),
        truncated: graph_seed_input.candidate_count > graph_seeds.len(),
        truncation_reason_codes: knowledge_graph_seed_truncation_reason_codes(
            graph_seed_input.candidate_count,
            graph_seeds.len(),
        ),
        truncation_reasons: knowledge_graph_seed_truncation_reasons(
            graph_seed_input.candidate_count,
            graph_seeds.len(),
            graph_seed_input.limit,
        ),
        top_candidates: graph_seeds
            .iter()
            .enumerate()
            .map(|(index, seed)| {
                let id = graph_seed_candidate_id(seed);
                let graph_context_path_count = graph_context_paths
                    .iter()
                    .filter(|path| path.seed_hit_id == id)
                    .count();
                KnowledgeRetrieverCandidate {
                    id,
                    kind: seed.entity.labels.first().cloned(),
                    external_id: seed.entity.external_id.clone(),
                    source_id: None,
                    canonical_node_id: Some(seed.entity.node_id),
                    rank: index + 1,
                    score: seed.score,
                    matched_spans: Vec::new(),
                    graph_context_path_count,
                    projection_freshness: None,
                }
            })
            .collect(),
    });
    reports
}

#[derive(Debug, Clone)]
struct KnowledgeGraphSeedRetrieverInput {
    limit: usize,
    input_candidate_count: usize,
    input_filtered_out_count: usize,
    metadata_filters: BTreeMap<String, String>,
    candidate_count: usize,
    graph_commit_epoch: u64,
}

fn knowledge_search_retriever_truncation_reasons(
    name: &str,
    candidate_count: usize,
    returned_count: usize,
    search_limit: usize,
    rank_window: Option<usize>,
) -> Vec<String> {
    if candidate_count <= returned_count {
        return Vec::new();
    }
    let mut reasons = Vec::new();
    if let Some(rank_window) = rank_window
        && candidate_count > rank_window
        && returned_count <= rank_window
    {
        reasons.push(format!(
            "{name} rank_window {rank_window} returned from {candidate_count} candidates"
        ));
    }
    if returned_count >= search_limit && candidate_count > search_limit {
        reasons.push(format!(
            "{name} search_limit {search_limit} returned from {candidate_count} candidates"
        ));
    }
    if reasons.is_empty() {
        reasons.push(format!(
            "{name} returned {returned_count} of {candidate_count} candidates"
        ));
    }
    reasons
}

fn knowledge_search_retriever_truncation_reason_codes(
    candidate_count: usize,
    returned_count: usize,
    search_limit: usize,
    rank_window: Option<usize>,
) -> Vec<KnowledgeTruncationReasonCode> {
    if candidate_count <= returned_count {
        return Vec::new();
    }
    let mut codes = Vec::new();
    if let Some(rank_window) = rank_window
        && candidate_count > rank_window
        && returned_count <= rank_window
    {
        codes.push(KnowledgeTruncationReasonCode::RankWindowExceeded);
    }
    if returned_count >= search_limit && candidate_count > search_limit {
        codes.push(KnowledgeTruncationReasonCode::SearchLimitExceeded);
    }
    if codes.is_empty() {
        codes.push(KnowledgeTruncationReasonCode::PartialCandidateReturn);
    }
    codes
}

fn knowledge_search_retriever_fusion_weight(
    name: &str,
    weights: SearchFusionWeights,
) -> Option<f64> {
    match name {
        "vector" => Some(weights.vector_weight),
        "text" => Some(weights.text_weight),
        _ => None,
    }
}

fn knowledge_graph_seed_truncation_reasons(
    candidate_count: usize,
    returned_count: usize,
    graph_seed_limit: usize,
) -> Vec<String> {
    if candidate_count > returned_count {
        vec![format!(
            "graph_seed limit {graph_seed_limit} returned from {candidate_count} candidates"
        )]
    } else {
        Vec::new()
    }
}

fn knowledge_graph_seed_truncation_reason_codes(
    candidate_count: usize,
    returned_count: usize,
) -> Vec<KnowledgeTruncationReasonCode> {
    if candidate_count > returned_count {
        vec![KnowledgeTruncationReasonCode::GraphSeedLimitExceeded]
    } else {
        Vec::new()
    }
}

fn knowledge_graph_seed_fallback_reasons(graph_seed_limit: usize) -> Vec<String> {
    if graph_seed_limit == 0 {
        vec!["graph seed retriever disabled by limit 0".to_string()]
    } else {
        Vec::new()
    }
}

fn knowledge_graph_seed_fallback_reason_codes(
    graph_seed_limit: usize,
) -> Vec<KnowledgeFallbackReasonCode> {
    if graph_seed_limit == 0 {
        vec![KnowledgeFallbackReasonCode::GraphSeedLimitZero]
    } else {
        Vec::new()
    }
}

fn knowledge_graph_seed_candidate_set_report(
    cardinality: usize,
    graph_commit_epoch: u64,
) -> SearchRetrieverCandidateSetReport {
    SearchRetrieverCandidateSetReport {
        id_space: "canonical_graph_node_id".to_string(),
        representation: "ranked_node_ids".to_string(),
        cardinality,
        exact: true,
        snapshot_source_graph_commit_epoch: Some(graph_commit_epoch),
        policy_epoch: None,
    }
}

fn knowledge_graph_seed_input_candidate_set_report(
    cardinality: usize,
    graph_commit_epoch: u64,
    filtered_out_count: usize,
    metadata_filters: BTreeMap<String, String>,
) -> SearchCandidateSetReport {
    let metadata_predicate_pushdown = search_metadata_predicate_pushdown(&metadata_filters).report;
    SearchCandidateSetReport {
        id_space: "canonical_graph_node_id".to_string(),
        representation: "filtered_node_ids".to_string(),
        cardinality,
        exact: true,
        snapshot_source_graph_commit_epoch: Some(graph_commit_epoch),
        policy_epoch: None,
        filtered_out_count,
        metadata_filters,
        metadata_predicate_pushdown,
    }
}

fn knowledge_graph_context_input_candidate_set_report(
    cardinality: usize,
    graph_commit_epoch: u64,
) -> SearchCandidateSetReport {
    SearchCandidateSetReport {
        id_space: "canonical_graph_node_id".to_string(),
        representation: "context_seed_node_ids".to_string(),
        cardinality,
        exact: true,
        snapshot_source_graph_commit_epoch: Some(graph_commit_epoch),
        policy_epoch: None,
        filtered_out_count: 0,
        metadata_filters: BTreeMap::new(),
        metadata_predicate_pushdown: SearchPredicatePushdownReport::default(),
    }
}

fn knowledge_graph_context_candidate_set_report(
    cardinality: usize,
    graph_commit_epoch: u64,
) -> SearchRetrieverCandidateSetReport {
    SearchRetrieverCandidateSetReport {
        id_space: "canonical_graph_relationship_id".to_string(),
        representation: "expanded_relationship_ids".to_string(),
        cardinality,
        exact: true,
        snapshot_source_graph_commit_epoch: Some(graph_commit_epoch),
        policy_epoch: None,
    }
}

fn knowledge_graph_context_fallback_reasons(request: &KnowledgeRetrievalRequest) -> Vec<String> {
    let mut reasons = Vec::new();
    if request.graph_context_limit == 0 {
        reasons.push("graph context expansion disabled by limit 0".to_string());
    }
    if request.graph_context_max_hops == 0 {
        reasons.push("graph context expansion disabled by max_hops 0".to_string());
    }
    reasons
}

fn knowledge_graph_context_fallback_reason_codes(
    request: &KnowledgeRetrievalRequest,
) -> Vec<KnowledgeFallbackReasonCode> {
    let mut codes = Vec::new();
    if request.graph_context_limit == 0 {
        codes.push(KnowledgeFallbackReasonCode::GraphContextLimitZero);
    }
    if request.graph_context_max_hops == 0 {
        codes.push(KnowledgeFallbackReasonCode::GraphContextMaxHopsZero);
    }
    codes
}

#[derive(Debug, Clone)]
struct KnowledgeRetrievalDiagnosticsInput {
    graph_seed_input_candidate_set: SearchCandidateSetReport,
    graph_seed_candidate_set: SearchRetrieverCandidateSetReport,
    graph_seed_candidate_count: usize,
    graph_seed_returned_count: usize,
    graph_context_input_candidate_set: SearchCandidateSetReport,
    graph_context_candidate_set: SearchRetrieverCandidateSetReport,
    graph_context_path_count: usize,
    graph_context_node_count: usize,
    graph_context_relationship_count: usize,
    graph_context_truncation_reasons: Vec<String>,
    fanout_reason_details: Vec<KnowledgeFanoutReasonDetail>,
    candidate_count: usize,
    candidate_total_count: usize,
    pipeline: KnowledgeRetrievalPipelineReport,
}

fn knowledge_retrieval_diagnostics(
    search: &SearchResultSet,
    request: &KnowledgeRetrievalRequest,
    projection_freshness: &SearchProjectionFreshness,
    graph_commit_epoch: u64,
    required_projection_commit_epoch: u64,
    input: KnowledgeRetrievalDiagnosticsInput,
) -> KnowledgeRetrievalDiagnostics {
    let mut empty_reasons = Vec::new();
    let mut empty_reason_codes = Vec::new();
    let candidate_truncation_reasons = knowledge_candidate_truncation_reasons(
        input.candidate_total_count,
        input.candidate_count,
        request.candidate_limit,
    );
    let candidate_truncation_reason_codes = knowledge_candidate_truncation_reason_codes(
        input.candidate_total_count,
        input.candidate_count,
    );
    if input.candidate_count == 0 {
        empty_reason_codes.extend(
            search
                .empty_reason_codes
                .iter()
                .map(knowledge_empty_reason_code_from_search),
        );
        empty_reasons.extend(search.empty_reasons.iter().cloned());
        if input.candidate_total_count == 0 {
            if request.graph_seed_limit == 0 {
                empty_reason_codes.push(KnowledgeRetrievalEmptyReasonCode::GraphSeedLimitZero);
                empty_reasons.push("graph seed retriever disabled by limit 0".to_string());
            } else if input.graph_seed_candidate_count == 0 {
                empty_reason_codes.push(KnowledgeRetrievalEmptyReasonCode::GraphSeedNoCandidates);
                empty_reasons.push("graph seed retriever returned no candidates".to_string());
            }
        }
        if !candidate_truncation_reasons.is_empty() {
            empty_reason_codes
                .push(KnowledgeRetrievalEmptyReasonCode::CandidateLimitExcludedAllCandidates);
        }
        empty_reasons.extend(candidate_truncation_reasons.iter().cloned());
        empty_reason_codes.push(KnowledgeRetrievalEmptyReasonCode::NoCandidates);
        empty_reasons.push("retrieval produced no candidates".to_string());
    }
    let graph_seed_truncation_reasons = knowledge_graph_seed_truncation_reasons(
        input.graph_seed_candidate_count,
        input.graph_seed_returned_count,
        request.graph_seed_limit,
    );
    let graph_seed_truncation_reason_codes = knowledge_graph_seed_truncation_reason_codes(
        input.graph_seed_candidate_count,
        input.graph_seed_returned_count,
    );
    let graph_context_truncation_reason_codes =
        knowledge_graph_context_truncation_reason_codes(&input.graph_context_truncation_reasons);
    KnowledgeRetrievalDiagnostics {
        graph_commit_epoch,
        projection_source_graph_commit_epoch: projection_freshness.source_graph_commit_epoch,
        projection_commit_lag: search_projection_freshness_commit_lag(
            projection_freshness,
            required_projection_commit_epoch,
        ),
        projection_stale: search_projection_is_stale(
            projection_freshness,
            required_projection_commit_epoch,
        ),
        projection_full_reindex_needed: projection_freshness.full_reindex_needed,
        projection_full_reindex_reasons: projection_freshness.full_reindex_reasons.clone(),
        projection_metadata_repair_needed: projection_freshness.metadata_repair_needed,
        projection_metadata_repair_reasons: projection_freshness.metadata_repair_reasons.clone(),
        search_document_count: search.document_count,
        search_filtered_document_count: search.filtered_document_count,
        search_total_hits: search.total_hits,
        search_candidate_set: search.candidate_set.clone(),
        search_candidate_filtered_out_count: search.candidate_set.filtered_out_count,
        search_limit: search.limit,
        search_truncated: search.truncated,
        search_truncation_reason_codes: search.truncation_reason_codes.clone(),
        search_truncation_reasons: search.truncation_reasons.clone(),
        search_fallback_reason_codes: search.fallback_reason_codes.clone(),
        search_fallback_reasons: search.fallback_reasons.clone(),
        rank_window: search.rank_window,
        search_fusion_weights: search.fusion_weights,
        graph_seed_input_candidate_set: input.graph_seed_input_candidate_set,
        graph_seed_candidate_set: input.graph_seed_candidate_set,
        graph_seed_candidate_count: input.graph_seed_candidate_count,
        graph_seed_returned_count: input.graph_seed_returned_count,
        graph_seed_limit: request.graph_seed_limit,
        graph_seed_truncated: !graph_seed_truncation_reasons.is_empty(),
        graph_seed_truncation_reason_codes,
        graph_seed_truncation_reasons,
        graph_context_input_candidate_set: input.graph_context_input_candidate_set,
        graph_context_candidate_set: input.graph_context_candidate_set,
        graph_context_path_count: input.graph_context_path_count,
        graph_context_node_count: input.graph_context_node_count,
        graph_context_relationship_count: input.graph_context_relationship_count,
        graph_context_limit: request.graph_context_limit,
        graph_context_max_hops: request.graph_context_max_hops,
        graph_context_truncated: !input.graph_context_truncation_reasons.is_empty(),
        graph_context_truncation_reason_codes,
        graph_context_truncation_reasons: input.graph_context_truncation_reasons,
        graph_context_fallback_reason_codes: knowledge_graph_context_fallback_reason_codes(request),
        graph_context_fallback_reasons: knowledge_graph_context_fallback_reasons(request),
        fanout_reason_count: input.fanout_reason_details.len(),
        fanout_reason_codes: knowledge_fanout_reason_codes(&input.fanout_reason_details),
        fanout_reasons: knowledge_fanout_reason_messages(&input.fanout_reason_details),
        fanout_reason_details: input.fanout_reason_details,
        candidate_count: input.candidate_count,
        candidate_total_count: input.candidate_total_count,
        candidate_limit: request.candidate_limit,
        candidate_truncated: !candidate_truncation_reasons.is_empty(),
        candidate_truncation_reason_codes,
        candidate_truncation_reasons,
        warnings: knowledge_retrieval_warnings(
            projection_freshness,
            required_projection_commit_epoch,
        ),
        empty_reason_codes,
        empty_reasons,
        pipeline: input.pipeline,
    }
}

fn knowledge_fanout_reason_codes(
    details: &[KnowledgeFanoutReasonDetail],
) -> Vec<KnowledgeFanoutReasonCode> {
    details.iter().map(|detail| detail.code).collect()
}

fn knowledge_fanout_reason_messages(details: &[KnowledgeFanoutReasonDetail]) -> Vec<String> {
    details
        .iter()
        .map(|detail| detail.message.clone())
        .collect()
}

fn knowledge_empty_reason_code_from_search(
    code: &SearchEmptyReasonCode,
) -> KnowledgeRetrievalEmptyReasonCode {
    match code {
        SearchEmptyReasonCode::ProjectionEmpty => {
            KnowledgeRetrievalEmptyReasonCode::SearchProjectionEmpty
        }
        SearchEmptyReasonCode::MetadataFilterEmpty => {
            KnowledgeRetrievalEmptyReasonCode::SearchMetadataFilterEmpty
        }
        SearchEmptyReasonCode::RetrieverNoHits => {
            KnowledgeRetrievalEmptyReasonCode::SearchRetrieverNoHits
        }
        SearchEmptyReasonCode::LimitExcludedAllHits => {
            KnowledgeRetrievalEmptyReasonCode::SearchLimitExcludedAllHits
        }
    }
}

fn knowledge_candidate_truncation_reasons(
    candidate_total_count: usize,
    candidate_count: usize,
    candidate_limit: Option<usize>,
) -> Vec<String> {
    match candidate_limit {
        Some(limit) if candidate_total_count > candidate_count => vec![format!(
            "knowledge_candidate_limit {limit} returned from {candidate_total_count} merged candidates"
        )],
        _ => Vec::new(),
    }
}

fn knowledge_candidate_truncation_reason_codes(
    candidate_total_count: usize,
    candidate_count: usize,
) -> Vec<KnowledgeTruncationReasonCode> {
    if candidate_total_count > candidate_count {
        vec![KnowledgeTruncationReasonCode::CandidateLimitExceeded]
    } else {
        Vec::new()
    }
}

fn knowledge_graph_context_truncation_reason_codes(
    truncation_reasons: &[String],
) -> Vec<KnowledgeTruncationReasonCode> {
    if truncation_reasons.is_empty() {
        Vec::new()
    } else {
        vec![KnowledgeTruncationReasonCode::GraphContextLimitExceeded]
    }
}

fn knowledge_retrieval_warnings(
    projection_freshness: &SearchProjectionFreshness,
    required_projection_commit_epoch: u64,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if search_projection_is_stale(projection_freshness, required_projection_commit_epoch) {
        warnings.push("search projection is older than graph snapshot".to_string());
    }
    if projection_freshness.full_reindex_needed {
        warnings.push("search projection requires full reindex".to_string());
        warnings.extend(
            projection_freshness
                .full_reindex_reasons
                .iter()
                .map(|reason| format!("search projection full reindex reason: {reason}")),
        );
    }
    if projection_freshness.metadata_repair_needed {
        warnings.push("search projection metadata repair is needed".to_string());
        warnings.extend(
            projection_freshness
                .metadata_repair_reasons
                .iter()
                .map(|reason| format!("search projection metadata repair reason: {reason}")),
        );
    }
    warnings
}

fn search_projection_is_stale(
    projection_freshness: &SearchProjectionFreshness,
    required_projection_commit_epoch: u64,
) -> bool {
    projection_freshness
        .source_graph_commit_epoch
        .map(|projection_epoch| projection_epoch < required_projection_commit_epoch)
        .unwrap_or(false)
}

fn search_projection_freshness_commit_lag(
    projection_freshness: &SearchProjectionFreshness,
    required_projection_commit_epoch: u64,
) -> u64 {
    required_projection_commit_epoch
        .saturating_sub(projection_freshness.source_graph_commit_epoch.unwrap_or(0))
}

fn graph_seed_candidate_id(seed: &KnowledgeGraphSeed) -> String {
    let label = seed
        .entity
        .labels
        .first()
        .map(String::as_str)
        .unwrap_or("node");
    match seed.entity.external_id.as_deref() {
        Some(external_id) if !external_id.is_empty() => format!("{label}:{external_id}"),
        _ => format!("node:{}", seed.entity.node_id),
    }
}

fn graph_seed_candidate_id_internal(seed: &KnowledgeGraphSeedCandidate) -> String {
    seed.id.clone()
}

fn graph_seed_candidate_from_node(
    catalog: &Catalog,
    node: &NodeRecord,
    relevance: hawdb_executor::graph_seed::GraphSeedRelevance,
    account: &hawdb_executor::QueryMemoryAccount,
) -> Result<KnowledgeGraphSeedCandidate> {
    use hawdb_executor::graph_seed::property_text;
    use std::borrow::Cow;
    let label = node
        .labels
        .iter()
        .find_map(|label_id| catalog.label_name(*label_id))
        .unwrap_or("node");
    let (external_id, _external_allocation) = if let Some(value) = node.properties.get("id") {
        property_text(value, account, None)?
    } else {
        (Cow::Borrowed(""), None)
    };
    // u64 decimal fallback requires at most twenty bytes.
    let _fallback_allocation = account.reserve(if external_id.is_empty() { 20 } else { 0 })?;
    let external_id = if external_id.is_empty() {
        Cow::Owned(node.id.0.to_string())
    } else {
        external_id
    };
    let property_count = relevance.matched_properties().count();
    let property_bytes = relevance.matched_properties().map(str::len).sum::<usize>();
    let bytes = label
        .len()
        .checked_add(1)
        .and_then(|bytes| bytes.checked_add(external_id.len()))
        .and_then(|bytes| bytes.checked_add(property_count * std::mem::size_of::<String>()))
        .and_then(|bytes| bytes.checked_add(property_bytes))
        .ok_or_else(|| HawDBError::Execution("graph seed candidate size overflow".into()))?;
    let allocation = account.reserve(bytes)?;
    let id = format!("{label}:{external_id}");
    let mut matched_properties = Vec::with_capacity(property_count);
    matched_properties.extend(relevance.matched_properties().map(str::to_string));
    Ok(KnowledgeGraphSeedCandidate {
        id,
        node_id: node.id,
        score: relevance.score,
        matched_properties,
        _allocation: allocation,
    })
}

fn knowledge_graph_expansion_scope_filters(
    metadata_filters: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    const SCOPE_FIELDS: &[&str] = &["space_id", "tenant_id", "workspace_id", "visibility"];
    metadata_filters
        .iter()
        .filter(|(field, _)| SCOPE_FIELDS.contains(&field.as_str()))
        .map(|(field, value)| (field.clone(), value.clone()))
        .collect()
}

fn knowledge_candidate_score_breakdown(
    source: &impl ScoringFeatureSource,
    scoring: &KnowledgeCandidateScoringPolicy,
    reference_time_millis: u64,
) -> KnowledgeCandidateScoreBreakdown {
    let search_score = source.search_score();
    let graph_seed_score = source.graph_seed_score();
    let combined_score = match scoring {
        KnowledgeCandidateScoringPolicy::Max => search_score
            .into_iter()
            .chain(graph_seed_score)
            .fold(0.0, f64::max),
        KnowledgeCandidateScoringPolicy::WeightedSum {
            search_weight,
            graph_seed_weight,
        } => {
            search_score.unwrap_or(0.0) * search_weight
                + graph_seed_score.unwrap_or(0.0) * graph_seed_weight
        }
        KnowledgeCandidateScoringPolicy::Spec(spec) => {
            let evaluation = spec.evaluate(source, reference_time_millis);
            return KnowledgeCandidateScoreBreakdown {
                search_score,
                graph_seed_score,
                combined_score: evaluation.combined_score,
                scoring_spec: Some(evaluation),
            };
        }
    };
    KnowledgeCandidateScoreBreakdown {
        search_score,
        graph_seed_score,
        combined_score,
        scoring_spec: None,
    }
}

/// Canonical feature values for one rerank candidate.
struct KnowledgeCandidateFeatureSource<'a> {
    search_score: Option<f64>,
    graph_seed_score: Option<f64>,
    hop_distance: Option<usize>,
    node: Option<&'a NodeRecord>,
}

impl ScoringFeatureSource for KnowledgeCandidateFeatureSource<'_> {
    fn search_score(&self) -> Option<f64> {
        self.search_score
    }

    fn graph_seed_score(&self) -> Option<f64> {
        self.graph_seed_score
    }

    fn hop_distance(&self) -> Option<usize> {
        self.hop_distance
    }

    fn numeric_property(&self, property: &str) -> Option<f64> {
        knowledge_graph_seed_filter_numeric_value(self.node?, property)
    }

    /// Canonical timestamps are epoch milliseconds; fractional and string
    /// encodings are reported as missing rather than guessed.
    fn timestamp_millis(&self, property: &str) -> Option<u64> {
        match self.node?.properties.get(property)? {
            Value::Int(millis) => u64::try_from(*millis).ok(),
            _ => None,
        }
    }
}

fn scoring_reference_time_millis() -> u64 {
    hawdb_core::time::SystemTime::now()
        .duration_since(hawdb_core::time::UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
fn graph_seed_score(
    node: &NodeRecord,
    query_terms: &BTreeSet<String>,
    normalized_query: &str,
) -> (f64, Vec<String>) {
    const GRAPH_SEED_PROPERTIES: &[&str] =
        &["id", "title", "name", "summary", "content", "body", "text"];
    let mut score = 0.0;
    let mut matched_properties = Vec::new();
    for property in GRAPH_SEED_PROPERTIES {
        let Some(value) = node.properties.get(*property) else {
            continue;
        };
        let text = value_to_external_id(value);
        let normalized_text = text.to_ascii_lowercase();
        let property_terms = knowledge_query_terms(&text);
        let matched_term_count = query_terms
            .iter()
            .filter(|term| property_terms.contains(*term))
            .count();
        let exact_match = !normalized_query.is_empty() && normalized_text == normalized_query;
        let contains_query =
            !normalized_query.is_empty() && normalized_text.contains(normalized_query);
        if matched_term_count > 0 || exact_match || contains_query {
            matched_properties.push((*property).to_string());
            score += matched_term_count as f64;
            if contains_query {
                score += 2.0;
            }
            if *property == "id" && exact_match {
                score += 8.0;
            }
        }
    }
    (score, matched_properties)
}

fn knowledge_graph_seed_filter_numeric_value(node: &NodeRecord, key: &str) -> Option<f64> {
    hawdb_executor::graph_seed::numeric_property(node, key)
}

fn parse_metadata_filter_number(value: &str) -> Option<f64> {
    let number = value.parse::<f64>().ok()?;
    number.is_finite().then_some(number)
}

fn knowledge_graph_seed_filter_value_matches(key: &str, actual: &str, expected: &str) -> bool {
    if key == "kind" {
        let Some(expected_label) = search_kind_to_label(expected) else {
            return false;
        };
        return search_label_to_kind(expected_label)
            .is_some_and(|expected_kind| actual == expected_kind);
    }
    if key != "space_id" && search_field_is_enum_like(key) {
        return actual.trim().eq_ignore_ascii_case(expected.trim());
    }
    // String conversion preserves contextual Unicode rules, including final
    // sigma. Per-character lowercase is not the same normalization contract.
    actual.trim().to_lowercase() == expected.trim().to_lowercase()
}

#[cfg(test)]
fn normalized_node_space_id(node: &NodeRecord) -> String {
    node.properties
        .get("space_id")
        .map(value_to_external_id)
        .filter(|space_id| !space_id.is_empty())
        .unwrap_or_else(|| "default".to_string())
}

#[cfg(test)]
fn node_projection_source_id(node: &NodeRecord) -> Option<String> {
    ["source_id", "thread_id", "source"]
        .into_iter()
        .filter_map(|key| node.properties.get(key).map(value_to_external_id))
        .find(|source_id| !source_id.is_empty())
}

#[cfg(test)]
fn knowledge_query_terms(text: &str) -> BTreeSet<String> {
    text.split(|ch: char| !ch.is_alphanumeric() && ch != '_')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

fn knowledge_context_path_node_count(paths: &[KnowledgeGraphContextPath]) -> usize {
    paths
        .iter()
        .flat_map(|path| [path.source_node_id, path.target_node_id])
        .collect::<BTreeSet<_>>()
        .len()
}

struct DenseAdjacencyDiagnosticContext<'a, S = GraphStore> {
    catalog: &'a Catalog,
    store: &'a S,
    operation: &'a str,
    relationship_type: Option<crate::schema::RelTypeId>,
}

fn record_dense_adjacency_diagnostics<S: crate::executor::ExecutionStore>(
    context: DenseAdjacencyDiagnosticContext<'_, S>,
    node_id: NodeId,
    reported_dense_groups: &mut BTreeSet<String>,
    fanout_reasons: &mut Vec<KnowledgeFanoutReasonDetail>,
) {
    for adjacency_direction in [AdjacencyDirection::Outgoing, AdjacencyDirection::Incoming] {
        let stats = match context.relationship_type {
            Some(rel_type) => {
                vec![context
                    .store
                    .adjacency_group_stats(node_id, rel_type, adjacency_direction)]
            }
            None => context
                .store
                .adjacency_group_stats_for_node(node_id, adjacency_direction),
        };
        for stats in stats {
            if stats.layout != AdjacencyLayout::Dense {
                continue;
            }
            let rel_type_name = context
                .catalog
                .rel_type_name(stats.rel_type)
                .unwrap_or("<unknown>");
            let direction = adjacency_direction_name(adjacency_direction);
            let key = format!(
                "{}:{rel_type_name}:{direction}:{}",
                context.operation, node_id.0
            );
            if reported_dense_groups.insert(key) {
                fanout_reasons.push(KnowledgeFanoutReasonDetail::dense_adjacency(
                    context.operation,
                    rel_type_name,
                    direction,
                    node_id.0,
                    stats.degree,
                ));
            }
        }
    }
}

fn knowledge_path_direction_for_adjacency(
    adjacency_direction: AdjacencyDirection,
) -> KnowledgeGraphPathDirection {
    match adjacency_direction {
        AdjacencyDirection::Outgoing => KnowledgeGraphPathDirection::Outgoing,
        AdjacencyDirection::Incoming => KnowledgeGraphPathDirection::Incoming,
    }
}

fn adjacency_direction_name(adjacency_direction: AdjacencyDirection) -> &'static str {
    match adjacency_direction {
        AdjacencyDirection::Outgoing => "outgoing",
        AdjacencyDirection::Incoming => "incoming",
    }
}

fn require_search_projection_source_epoch(expected: Option<u64>, actual: u64) -> Result<()> {
    if let Some(expected) = expected
        && expected != actual
    {
        if expected > actual {
            return Err(HawDBError::Storage(format!(
                "search projection graph delta complete-through epoch {expected} is ahead of graph commit epoch {actual}"
            )));
        }
        return Err(HawDBError::Execution(format!(
            "search projection complete-through epoch {expected} differs from source snapshot epoch {actual}; prepare the complete source interval before publishing its watermark"
        )));
    }
    Ok(())
}

fn search_projection_graph_delta_for(
    catalog: &Catalog,
    store: &GraphStore,
    request: &SearchProjectionGraphDeltaRequest,
) -> Result<SearchProjectionDelta> {
    let operation_count = request.operation_count();
    if let Some(limit) = request.max_operations
        && operation_count > limit
    {
        return Err(HawDBError::Storage(format!(
                "search projection graph delta operation count {operation_count} exceeded configured limit {limit}"
            )));
    }
    // A changefeed prefix describes IDs, not a historical read view.
    // Hydrating it from a newer head would publish future data under an old watermark.
    require_search_projection_source_epoch(
        request.complete_through_graph_commit_epoch,
        store.commit_epoch(),
    )?;

    let mut upserts = Vec::new();
    for node_id in &request.upsert_node_ids {
        let Some(node) = store.node_owned(NodeId(*node_id))? else {
            continue;
        };
        if let Some(row) = projection_row_from_node_with_graph_metadata(catalog, store, &node)? {
            upserts.push(row);
        }
    }

    Ok(SearchProjectionDelta {
        upserts,
        deletes: request.delete_document_ids.clone(),
        max_operations: request.max_operations,
        source_graph_commit_epoch: request.complete_through_graph_commit_epoch,
    })
}

fn search_projection_commit_lag(
    search_index: &SearchIndex,
    required_projection_commit_epoch: u64,
) -> u64 {
    search_projection_freshness_commit_lag(
        &search_index.projection_freshness(),
        required_projection_commit_epoch,
    )
}

fn knowledge_entity_from_node(catalog: &Catalog, node: &NodeRecord) -> KnowledgeEntity {
    KnowledgeEntity {
        node_id: node.id.0,
        labels: node_label_names(catalog, node),
        external_id: Some(projected_node_external_id(node)),
        properties: node.properties.clone(),
    }
}

impl ReaderPins {
    fn oldest_epoch(&self) -> Option<u64> {
        self.oldest_epoch_excluding(None)
    }

    fn oldest_epoch_excluding(&self, excluded: Option<u64>) -> Option<u64> {
        self.active_views
            .iter()
            .filter_map(|(&id, view)| (Some(id) != excluded).then_some(view.visible_commit_epoch()))
            .min()
    }

    fn pinned_physical_generations(&self) -> BTreeSet<u64> {
        self.active_views
            .values()
            .filter_map(|view| view.physical_generation())
            .map(|generation| generation.0)
            .collect()
    }
}

fn elapsed_micros(started: hawdb_core::time::Instant) -> u64 {
    started.elapsed().as_micros().min(u64::MAX as u128) as u64
}

impl ReaderPin {
    fn new(id: u64, pins: Arc<Mutex<ReaderPins>>) -> Self {
        Self { id, pins }
    }
}

impl Drop for ReaderPin {
    fn drop(&mut self) {
        self.pins
            .lock()
            .expect("database reader pins lock should not be poisoned")
            .active_views
            .remove(&self.id);
    }
}

fn search_kind_to_label(kind: &str) -> Option<&'static str> {
    match kind {
        "Memory" | "memory" => Some("Memory"),
        "Message" | "message" => Some("Message"),
        "Entity" | "entity" => Some("Entity"),
        "Source" | "source" => Some("Source"),
        "SourceChunk" | "source_chunk" | "sourcechunk" | "chunk" => Some("SourceChunk"),
        "Community" | "community" => Some("Community"),
        _ => None,
    }
}

fn search_label_to_kind(label: &str) -> Option<&'static str> {
    match label {
        "Memory" | "memory" => Some("memory"),
        "Message" | "message" => Some("message"),
        "Entity" | "entity" => Some("entity"),
        "Source" | "source" => Some("source"),
        "SourceChunk" | "source_chunk" | "sourcechunk" | "chunk" => Some("source_chunk"),
        "Community" | "community" => Some("community"),
        _ => None,
    }
}

fn node_label_names(catalog: &Catalog, node: &NodeRecord) -> Vec<String> {
    node.labels
        .iter()
        .filter_map(|label_id| catalog.label_name(*label_id))
        .map(str::to_string)
        .collect()
}

fn node_external_id(node: &NodeRecord) -> Option<String> {
    node.properties
        .get("id")
        .map(value_to_external_id)
        .filter(|external_id| !external_id.is_empty())
}

fn projected_node_external_id(node: &NodeRecord) -> String {
    node_external_id(node).unwrap_or_else(|| node.id.0.to_string())
}

fn value_to_external_id(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Binary(value) => format!(
            "\\x{}",
            value
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        ),
        Value::Uuid(value) => value.to_string(),
        Value::Vector(_) => value.to_string(),
        Value::List(values) => values
            .iter()
            .map(value_to_external_id)
            .collect::<Vec<_>>()
            .join(","),
        Value::Map(values) => values
            .iter()
            .map(|(key, value)| format!("{key}:{}", value_to_external_id(value)))
            .collect::<Vec<_>>()
            .join(","),
    }
}

fn schema_state_value(state: SchemaObjectState) -> Value {
    let value = match state {
        SchemaObjectState::DeleteOnly => "delete_only",
        SchemaObjectState::WriteOnly => "write_only",
        SchemaObjectState::Backfill => "backfill",
        SchemaObjectState::Validating => "validating",
        SchemaObjectState::Public => "public",
        SchemaObjectState::Gc => "gc",
    };
    Value::String(value.to_string())
}

fn schema_maintenance_actions_output(actions: Vec<SchemaMaintenanceAction>) -> QueryOutput {
    let rows = actions
        .into_iter()
        .map(|action| {
            BTreeMap::from([
                ("object_type".to_string(), Value::String(action.object_type)),
                ("object".to_string(), Value::String(action.object)),
                (
                    "from_state".to_string(),
                    schema_state_value(action.from_state),
                ),
                (
                    "to_state".to_string(),
                    action
                        .to_state
                        .map(schema_state_value)
                        .unwrap_or(Value::Null),
                ),
                ("action".to_string(), Value::String(action.action)),
            ])
        })
        .collect();
    QueryOutput { rows }
}

fn property_index_projection_rebuild_output(
    actions: Vec<PropertyIndexProjectionRebuildAction>,
) -> QueryOutput {
    let rows = actions
        .into_iter()
        .map(|action| {
            BTreeMap::from([
                ("index_kind".to_string(), Value::String(action.index_kind)),
                ("label".to_string(), Value::String(action.label)),
                (
                    "properties".to_string(),
                    Value::List(action.properties.into_iter().map(Value::String).collect()),
                ),
                (
                    "estimated_operations".to_string(),
                    Value::Int(i64::try_from(action.estimated_operations).unwrap_or(i64::MAX)),
                ),
                (
                    "indexed_entries".to_string(),
                    Value::Int(i64::try_from(action.indexed_entries).unwrap_or(i64::MAX)),
                ),
            ])
        })
        .collect();
    QueryOutput { rows }
}

fn optional_u64_value(value: Option<u64>) -> Value {
    value
        .map(|value| Value::Int(value as i64))
        .unwrap_or(Value::Null)
}

fn optional_usize_value(value: Option<usize>) -> Value {
    value
        .map(|value| Value::Int(value as i64))
        .unwrap_or(Value::Null)
}

fn statement_body(statement: &cypher::Statement) -> &cypher::Statement {
    match statement {
        cypher::Statement::CypherQuery(query) => &query.statement,
        _ => statement,
    }
}

pub(crate) fn statement_kind(statement: &cypher::Statement) -> &'static str {
    match statement {
        cypher::Statement::AlterPropertyState(_) => "alter_property_state",
        cypher::Statement::AlterTableState(_) => "alter_table_state",
        cypher::Statement::BeginTransaction => "begin_transaction",
        cypher::Statement::Checkpoint => "checkpoint",
        cypher::Statement::Commit => "commit",
        cypher::Statement::CreateCompositeIndex(_) => "create_composite_index",
        cypher::Statement::CreateFullTextIndex(_) => "create_full_text_index",
        cypher::Statement::CreateIndex(_) => "create_index",
        cypher::Statement::CreateNode(_) => "create_node",
        cypher::Statement::CreateNodeLabel(_) => "create_node_label",
        cypher::Statement::CreateNodePropertyExistsConstraint(_) => {
            "create_node_property_exists_constraint"
        }
        cypher::Statement::CreateNodeTable(_) => "create_node_table",
        cypher::Statement::CreateProperty(_) => "create_property",
        cypher::Statement::CreateRangeIndex(_) => "create_range_index",
        cypher::Statement::CreateRelationship(_) => "create_relationship",
        cypher::Statement::CreateRelationshipPropertyExistsConstraint(_) => {
            "create_relationship_property_exists_constraint"
        }
        cypher::Statement::CreateRelationshipTable(_) => "create_relationship_table",
        cypher::Statement::CreateRelationshipType(_) => "create_relationship_type",
        cypher::Statement::CreateRelationshipUniqueConstraint(_) => {
            "create_relationship_unique_constraint"
        }
        cypher::Statement::CreateUniqueConstraint(_) => "create_unique_constraint",
        cypher::Statement::CypherQuery(query) => statement_kind(&query.statement),
        cypher::Statement::Explain(explain) => {
            if explain.analyze {
                "explain_analyze"
            } else {
                "explain"
            }
        }
        cypher::Statement::GraphAlgorithm(_) => "graph_algorithm",
        cypher::Statement::VectorSearch(_) => "vector_search",
        cypher::Statement::Pipeline(query) => statement_shape::pipeline_statement_kind(query),
        cypher::Statement::MergeNode(_) => "merge_node",
        cypher::Statement::MergeRelationship(_) => "merge_relationship",
        cypher::Statement::UnwindMutation(_) => "unwind_mutation",
        cypher::Statement::ProjectGraph(_) => "project_graph",
        cypher::Statement::Rollback => "rollback",
        cypher::Statement::SetSystemVariable(_) => "set_system_variable",
    }
}

fn optimizer_config_from_database_config(config: &DatabaseConfig) -> OptimizerConfig {
    let mut optimizer = OptimizerConfig::default();
    if let Some(max_groups) = config.max_optimizer_groups {
        optimizer.max_groups = max_groups;
    }
    optimizer
}

fn optimizer_from_database_config(config: &DatabaseConfig) -> CascadesOptimizer {
    CascadesOptimizer::with_context(
        OptimizerContext::from_config(optimizer_config_from_database_config(config))
            .with_resource_hints(ResourceHints {
                priority: ResourceHints::default().priority,
                max_memory_bytes: Some(
                    u64::try_from(config.execution_memory.blocking_operator_bytes.get())
                        .unwrap_or(u64::MAX),
                ),
                max_parallelism: executor::MAX_MORSEL_PARALLELISM,
            }),
    )
}

impl<'a> NowledgeGraphAdapter<'a> {
    pub fn new(db: &'a mut Database) -> Self {
        Self { db }
    }

    pub fn query(&mut self, statement: &NowledgeGraphStatement) -> Result<QueryOutput> {
        self.db
            .query_with_params(&statement.cypher, &statement.parameters)
    }

    pub fn query_work_request(&self, statement: &NowledgeGraphStatement) -> Result<WorkRequest> {
        self.db.query_work_request_for(&statement.cypher)
    }

    pub fn explain(
        &self,
        statement: &NowledgeGraphStatement,
    ) -> Result<NowledgeGraphExplainOutput> {
        let output = self
            .db
            .explain_query_with_params(&statement.cypher, &statement.parameters)?;
        Ok(NowledgeGraphExplainOutput {
            plan: output.physical_plan.explain(0),
            trace: output.trace,
            work_request: output.work_request,
            plan_cache_lookup: output.plan_cache_lookup,
            statement_kind: output.statement_kind,
        })
    }

    pub fn transaction(
        &mut self,
        statements: &[NowledgeGraphStatement],
    ) -> Result<NowledgeGraphTransactionOutput> {
        let mut tx = self.db.begin_transaction()?;
        let mut statement_outputs = Vec::with_capacity(statements.len());
        for statement in statements {
            statement_outputs.push(tx.query_with_params(&statement.cypher, &statement.parameters)?);
        }
        let commit_output = tx.commit()?;
        Ok(NowledgeGraphTransactionOutput {
            statement_outputs,
            commit_output,
        })
    }

    /// Retrieves from resident search payloads, propagating search and graph/pipeline errors.
    pub fn retrieve_knowledge(
        &self,
        search_index: &SearchIndex,
        request: &KnowledgeRetrievalRequest,
    ) -> Result<KnowledgeRetrievalOutput> {
        self.db.retrieve_knowledge(search_index, request)
    }

    pub fn try_retrieve_knowledge(
        &self,
        search_index: &SearchIndex,
        request: &KnowledgeRetrievalRequest,
    ) -> Result<KnowledgeRetrievalOutput> {
        self.db.try_retrieve_knowledge(search_index, request)
    }
}

impl DatabaseTransactionRuntime {
    fn from_database(db: &Database) -> Result<Self> {
        Self::from_database_with_system_variables(
            db,
            db.system_variables.clone(),
            SessionConfig::default(),
        )
    }

    fn from_database_with_system_variables(
        db: &Database,
        system_variables: QuerySystemVariables,
        session_config: SessionConfig,
    ) -> Result<Self> {
        let runtime = db.runtime.get()?;
        Ok(Self {
            instance_identity: db.instance_identity()?,
            session_config,
            optimizer: runtime.optimizer.clone(),
            plan_cache: SharedState::new(PlanCache::new(db.config.max_plan_cache_entries)),
            relational_plan_template_cache: Arc::clone(&runtime.relational_plan_template_cache),
            optimizer_planning_cache: SharedState::new(
                runtime.optimizer_planning_cache.borrow().clone(),
            ),
            config: db.config.clone(),
            system_variables,
            branch_catalog_path: db.branch_catalog_path_from_runtime(&runtime).ok(),
            current_branch: db.current_branch_from_runtime(&runtime),
        })
    }

    fn read_result_row_limit(&self) -> Option<usize> {
        restrictive_query_limit(
            self.config.read_result_row_limit(),
            self.session_config.row_limit(),
        )
    }

    fn validate_result_rows(&self, rows: usize) -> Result<()> {
        if let Some(limit) = self.read_result_row_limit()
            && rows > limit
        {
            return Err(HawDBError::read_budget_exceeded(
                crate::ReadBudgetResource::Rows,
                limit,
                "session result exceeds the effective host/session row ceiling",
            ));
        }
        Ok(())
    }

    fn validate_result_payload(&self, bytes: usize, ceiling: Option<usize>) -> Result<()> {
        if let Some(limit) = ceiling
            && bytes > limit
        {
            return Err(HawDBError::read_budget_exceeded(
                crate::ReadBudgetResource::PayloadBytes,
                limit,
                "SQL mutation result exceeds the effective host/admitted payload ceiling",
            ));
        }
        Ok(())
    }

    fn ensure_writable(&self, store: &GraphStore) -> Result<()> {
        store.ensure_usable()?;
        if self.config.read_only {
            return Err(HawDBError::Execution(
                "database is opened in read-only mode".to_string(),
            ));
        }
        Ok(())
    }
}

impl DatabaseTransactionState {
    fn from_database(db: &Database) -> Result<Self> {
        let runtime = db.runtime.get()?;
        let (_, pin) = Database::pin_runtime_read_view(&runtime);
        Ok(Self {
            snapshot_pin: Some(pin),
            read_dependencies: None,
            graph_transaction: Some(runtime.store.begin_mutation_transaction(&runtime.catalog)),
            relational_transaction: hawdb_storage::relational::RelationalTransaction::default(),
            relational_state: runtime.store.relational_state().clone(),
            append_transaction: hawdb_storage::append_table::AppendTransaction::default(),
            append_state: runtime.store.append_state().clone(),
            pending_generated_append_tables: BTreeSet::new(),
            relational_returning: Vec::new(),
            relational_index: runtime
                .store
                .begin_authoritative_relational_transaction_index()
                .map_err(|error| error.to_string()),
            relational_rows: runtime
                .store
                .begin_authoritative_relational_transaction_rows()
                .map_err(|error| error.to_string()),
        })
    }

    pub(super) fn rollback(&mut self) {
        self.read_dependencies.take();
        self.graph_transaction.take();
        self.relational_transaction.writes.clear();
        self.append_transaction.writes.clear();
        self.pending_generated_append_tables.clear();
        self.relational_returning.clear();
        self.relational_index = Ok(None);
        self.relational_rows = Ok(None);
        self.snapshot_pin.take();
    }

    pub(crate) fn restore_graph_statement(&mut self, savepoint: GraphMutationSavepoint) {
        self.graph_transaction
            .as_mut()
            .expect("database transaction must own a graph workspace")
            .restore(savepoint);
    }

    fn take_for_commit(&mut self) -> Self {
        Self {
            read_dependencies: self.read_dependencies.take(),
            snapshot_pin: self.snapshot_pin.take(),
            graph_transaction: self.graph_transaction.take(),
            relational_transaction: std::mem::take(&mut self.relational_transaction),
            relational_state: std::mem::take(&mut self.relational_state),
            append_transaction: std::mem::take(&mut self.append_transaction),
            append_state: std::mem::take(&mut self.append_state),
            pending_generated_append_tables: std::mem::take(
                &mut self.pending_generated_append_tables,
            ),
            relational_returning: std::mem::take(&mut self.relational_returning),
            relational_index: std::mem::replace(&mut self.relational_index, Ok(None)),
            relational_rows: std::mem::replace(&mut self.relational_rows, Ok(None)),
        }
    }

    fn authoritative_relational_index(
        &self,
    ) -> Result<&crate::store::RelationalTransactionIndexView> {
        match &self.relational_index {
            Ok(Some(index)) => Ok(index),
            Ok(None) => Err(HawDBError::StorageIntegrity(
                "authoritative transaction index view is unavailable".to_string(),
            )),
            Err(error) => Err(HawDBError::StorageIntegrity(format!(
                "authoritative transaction index view could not be pinned: {error}"
            ))),
        }
    }

    fn stage_sparse_authoritative_relational_statement(
        &self,
        transaction: hawdb_storage::relational::RelationalTransaction,
    ) -> Result<Option<SparseRelationalStatementStage>> {
        let rows = match &self.relational_rows {
            Ok(Some(rows)) => rows,
            Ok(None) => return Ok(None),
            Err(error) => {
                return Err(HawDBError::StorageIntegrity(format!(
                    "authoritative transaction row view could not be pinned: {error}"
                )));
            }
        };
        let index = match &self.relational_index {
            Ok(Some(index)) => index,
            Ok(None) => {
                return Err(HawDBError::StorageIntegrity(
                    "authoritative transaction index view is unavailable".to_string(),
                ));
            }
            Err(error) => {
                return Err(HawDBError::StorageIntegrity(format!(
                    "authoritative transaction index view could not be pinned: {error}"
                )));
            }
        };
        let store = self
            .graph_transaction
            .as_ref()
            .expect("database transaction must own a graph workspace")
            .store();
        let (next, index_capture, row_capture, mutation_outcomes) = store
            .stage_sparse_relational_transaction_statement(
                &self.relational_state,
                rows,
                transaction,
                index,
            )
            .map_err(map_transaction_relational_error)?;
        Ok(Some(SparseRelationalStatementStage {
            state: next,
            mutation_outcomes,
            index_capture,
            row_capture,
        }))
    }
}

struct GraphTransactionStatementOptions<'a> {
    execution: query_runtime::QueryExecutionOptions<'a>,
    read_dependencies: Option<&'a mut transaction_read_dependencies::TransactionReadDependencies>,
}

fn prepare_graph_transaction_plan(
    runtime: &DatabaseTransactionRuntime,
    transaction: &GraphMutationTransaction,
    system_variables: &QuerySystemVariables,
    cypher_text: &str,
    statement: &cypher::Statement,
    parameters: &BTreeMap<String, Value>,
    execution: query_runtime::QueryExecutionOptions<'_>,
) -> Result<OptimizedQueryPlan> {
    let task_context = execution.task_context;
    query_runtime::query_runtime_checkpoint(task_context)?;
    query_work_request_for_statement(system_variables, statement)?;
    let optimizer_search =
        query_statement_variables_for_statement(system_variables, statement)?.optimizer_search;
    optimized_query_plan_for(
        cypher_text,
        statement,
        parameters,
        PlanCacheMode::Bypass(PlanCacheBypassReason::MutationPlanning),
        PlanTraceMode::Template,
        PlanCacheContext {
            catalog: transaction.catalog(),
            store: transaction.store(),
            optimizer: &runtime.optimizer,
            config: &runtime.config,
            cache: &runtime.plan_cache,
            planning_cache: &runtime.optimizer_planning_cache,
            access_control: execution.access_control,
            optimizer_search,
            scoring: execution.scoring,
        },
    )
}

fn execute_graph_transaction_statement(
    runtime: &DatabaseTransactionRuntime,
    transaction: &mut GraphMutationTransaction,
    system_variables: &QuerySystemVariables,
    cypher_text: &str,
    statement: &cypher::Statement,
    parameters: &BTreeMap<String, Value>,
    options: GraphTransactionStatementOptions<'_>,
) -> Result<GraphTransactionStatementOutcome> {
    let optimized = prepare_graph_transaction_plan(
        runtime,
        transaction,
        system_variables,
        cypher_text,
        statement,
        parameters,
        options.execution,
    )?;
    execute_graph_transaction_plan(
        runtime,
        transaction,
        statement,
        &optimized,
        parameters,
        options,
    )
}

fn execute_graph_transaction_plan(
    runtime: &DatabaseTransactionRuntime,
    transaction: &mut GraphMutationTransaction,
    statement: &cypher::Statement,
    optimized: &OptimizedQueryPlan,
    parameters: &BTreeMap<String, Value>,
    options: GraphTransactionStatementOptions<'_>,
) -> Result<GraphTransactionStatementOutcome> {
    let GraphTransactionStatementOptions {
        execution,
        read_dependencies,
    } = options;
    let task_context = execution.task_context;
    query_runtime::query_runtime_checkpoint(task_context)?;

    if let Some(dependencies) = read_dependencies {
        dependencies.collect_cypher(
            statement,
            &optimized.physical_plan,
            transaction.catalog(),
            transaction.store(),
        )?;
    }
    if executor::is_mutation_plan(&optimized.physical_plan)? {
        runtime.ensure_writable(transaction.store())?;
        let is_mutation_return = matches!(
            optimized.physical_plan,
            PhysicalPlan::SetNodePropertiesReturn { .. }
        );
        let mut mutation_limits = match &optimized.physical_plan {
            PhysicalPlan::SetNodePropertiesReturn {
                returns: crate::planner::SetNodePropertiesReturnMode::Count { .. },
                ..
            } => hawdb_storage::mutation::MutationLimits {
                max_result_rows: runtime.config.mutation_limits.max_affected_rows,
                max_result_payload_bytes: std::num::NonZeroUsize::new(usize::MAX)
                    .expect("usize::MAX is non-zero"),
                ..runtime.config.mutation_limits
            },
            _ => runtime.config.mutation_limits,
        };
        if !is_mutation_return
            && let Some(limit) = execution.output_limits.max_rows.and_then(NonZeroUsize::new)
        {
            mutation_limits.max_result_rows = mutation_limits.max_result_rows.min(limit);
        }
        let statement_savepoint = transaction.savepoint();
        let execution = (|| {
            let summary = match &optimized.physical_plan {
                PhysicalPlan::UnwindMutation {
                    rows,
                    variable,
                    operation,
                } => transaction.stage_mutations_with_limits(
                    executor::materialize_unwind_mutations(
                        rows,
                        variable,
                        operation,
                        mutation_limits,
                        task_context,
                    )?,
                    mutation_limits,
                )?,
                _ => {
                    let command = if is_mutation_return {
                        hawdb_executor::mutation::materialize_node_mutation(
                            &optimized.physical_plan,
                            transaction.catalog(),
                            transaction.store(),
                            mutation_limits,
                            task_context,
                        )?
                    } else {
                        match executor::mutation_command(&optimized.physical_plan) {
                            Ok(command) => command,
                            Err(error) => {
                                match hawdb_executor::mutation::materialize_node_mutation(
                                    &optimized.physical_plan,
                                    transaction.catalog(),
                                    transaction.store(),
                                    mutation_limits,
                                    task_context,
                                )? {
                                    Some(command) => Some(command),
                                    None => return Err(error),
                                }
                            }
                        }
                    };
                    let mutation = command.ok_or_else(|| {
                        HawDBError::Execution(
                            "transaction mutation plan cannot be represented as a staged mutation"
                                .to_string(),
                        )
                    })?;
                    if is_mutation_return {
                        transaction.stage_mutation_without_commit_rows(mutation, mutation_limits)?
                    } else {
                        transaction.stage_mutation_with_limits(mutation, mutation_limits)?
                    }
                }
            };
            let returned_rows = executor::project_staged_mutation_return_rows(
                &optimized.physical_plan,
                transaction.catalog(),
                transaction.store(),
                &summary.rows,
                runtime.config.mutation_limits,
            )?;
            let rows = returned_rows.unwrap_or_default();
            let output = QueryOutput { rows: rows.into() };
            hawdb_executor::result_delivery::OutputLimits {
                max_rows: restrictive_query_limit(
                    runtime.read_result_row_limit(),
                    execution.output_limits.max_rows,
                ),
                max_payload_bytes: restrictive_query_limit(
                    runtime.config.max_read_result_payload_bytes,
                    execution.output_limits.max_payload_bytes,
                ),
            }
            .validate(output.rows.len(), output.rows.payload_bytes())?;
            Ok((
                output,
                transaction.lock_footprint_since(&statement_savepoint)?,
            ))
        })();
        return match execution {
            Ok((output, lock_footprint)) => Ok(GraphTransactionStatementOutcome {
                output,
                lock_footprint,
                savepoint: Some(statement_savepoint),
            }),
            Err(error) => {
                transaction.restore(statement_savepoint);
                Err(error)
            }
        };
    }

    let query_result = {
        let (catalog, store) = transaction.catalog_and_store_mut();
        let mut external = executor::NoExternalReadOperator;
        let profiled = executor::execute_with_request(
            executor::ExecutionRequest::new(
                &optimized.physical_plan,
                parameters,
                &runtime.config.execution_memory,
            )
            .with_output_limits(
                restrictive_query_limit(
                    runtime.read_result_row_limit(),
                    execution.output_limits.max_rows,
                ),
                restrictive_query_limit(
                    runtime.config.max_read_result_payload_bytes,
                    execution.output_limits.max_payload_bytes,
                ),
            )
            .with_optional_task_context(task_context)
            .with_optional_host_scorer(
                execution
                    .scoring
                    .and_then(query_request::BoundScoringRequest::host_scorer),
            ),
            executor::ExecutionResources::new(catalog, store, &mut external),
        );
        profiled.map(|profiled| QueryOutput {
            rows: profiled.rows,
        })
    };
    transaction.store().poison_on_storage_error(&query_result);
    query_result.map(|output| GraphTransactionStatementOutcome {
        output,
        savepoint: None,
        lock_footprint: GraphMutationLockFootprint::default(),
    })
}

/// One statement-local preparation. Its borrowed request keeps parameters,
/// authorization, scoring and the combined task context identical during replay.
/// The transaction's admission remains held while locks are acquired.
pub(super) struct PreparedGraphTransactionRequest<'a> {
    request: QueryRequest<'a>,
    statement: cypher::Statement,
    optimized: OptimizedQueryPlan,
    // Outlives the AST/plan, including autocommit lock waiting and staging.
    _planning_memory: Option<hawdb_qos::RuntimeRetainedMemory>,
}

impl<'a> PreparedGraphTransactionRequest<'a> {
    pub(super) fn prepare(
        runtime: &DatabaseTransactionRuntime,
        state: &mut DatabaseTransactionState,
        request: QueryRequest<'a>,
    ) -> Result<Self> {
        query_runtime::query_runtime_checkpoint(request.task_context)?;
        let empty = BTreeMap::new();
        let parameters = request.parameters.unwrap_or(&empty);
        if let Some(dependencies) = &mut state.read_dependencies {
            dependencies.admit_cypher(request.cypher, parameters)?;
        }
        let statement = cypher::parse(request.cypher)?;
        let body = statement_body(&statement);
        if matches!(body, cypher::Statement::SetSystemVariable(_)) {
            reject_system_variable_parameters(parameters)?;
            return Err(HawDBError::Execution(
                "SET system variable is not allowed inside a transaction".to_string(),
            ));
        }
        if matches!(body, cypher::Statement::Explain(_)) {
            return Err(HawDBError::Execution(
                "EXPLAIN is not allowed inside a transaction".to_string(),
            ));
        }
        let transaction = state
            .graph_transaction
            .as_ref()
            .expect("database transaction must own a graph workspace");
        let optimized = prepare_graph_transaction_plan(
            runtime,
            transaction,
            &runtime.system_variables,
            request.cypher,
            &statement,
            parameters,
            query_runtime::QueryExecutionOptions::for_request(request, None),
        )?;
        Ok(Self {
            request,
            statement,
            optimized,
            _planning_memory: None,
        })
    }

    pub(super) fn from_admission(
        runtime: &DatabaseTransactionRuntime,
        state: &DatabaseTransactionState,
        prepared: PreparedRuntimeQuery,
        request: QueryRequest<'a>,
    ) -> Result<Self> {
        let transaction = state.graph_transaction.as_ref().expect("graph workspace");
        if !prepared.matches_request(request)
            || !prepared.is_compatible(
                runtime.current_branch.as_ref().map(|branch| branch.info.id),
                &runtime.config,
                &runtime.system_variables,
                transaction.catalog(),
                transaction.store(),
            )
        {
            return Err(HawDBError::Execution(
                "prepared mutation changed before execution; prepare and admit again".into(),
            ));
        }
        let (_, mut prepared) = prepared.into_execution(transaction.catalog(), transaction.store());
        let optimized = prepared
            .take_optimized(
                request.access_control,
                request.scoring.map(query_request::ScoringAttachment::bind),
            )?
            .ok_or_else(|| {
                HawDBError::Execution("admitted mutation has no compatible plan".into())
            })?;
        Ok(Self {
            request,
            statement: prepared.statement,
            optimized,
            _planning_memory: prepared.planning_memory,
        })
    }

    pub(super) fn returns_statement_rows(&self) -> bool {
        matches!(
            self.optimized.physical_plan,
            PhysicalPlan::SetNodePropertiesReturn { .. }
        )
    }

    pub(super) fn into_trace(self, capture_trace: bool) -> QueryExecutionTrace {
        QueryExecutionTrace {
            statement: self.statement,
            optimizer_trace: capture_trace.then_some(self.optimized.trace),
            plan_cache_lookup: Some(self.optimized.plan_cache_lookup),
            execution_profile: None,
        }
    }

    pub(super) fn execute(
        &self,
        runtime: &DatabaseTransactionRuntime,
        state: &mut DatabaseTransactionState,
    ) -> Result<GraphTransactionStatementOutcome> {
        let transaction = state
            .graph_transaction
            .as_mut()
            .expect("database transaction must own a graph workspace");
        execute_graph_transaction_plan(
            runtime,
            transaction,
            &self.statement,
            &self.optimized,
            self.request.parameters.unwrap_or(&BTreeMap::new()),
            GraphTransactionStatementOptions {
                execution: query_runtime::QueryExecutionOptions::for_request(self.request, None),
                read_dependencies: state.read_dependencies.as_mut(),
            },
        )
    }
}

pub(crate) fn execute_concurrent_graph_transaction_request(
    runtime: &DatabaseTransactionRuntime,
    state: &mut DatabaseTransactionState,
    request: QueryRequest<'_>,
    parent_context: Option<&hawdb_core::RuntimeTaskContext>,
) -> Result<GraphTransactionStatementOutcome> {
    let task = query_request::bind_request_context(parent_context, request.task_context)?;
    let request = QueryRequest {
        task_context: task.as_ref(),
        ..request
    };
    PreparedGraphTransactionRequest::prepare(runtime, state, request)?.execute(runtime, state)
}

pub(crate) fn execute_database_transaction_sql_request(
    runtime: &DatabaseTransactionRuntime,
    state: &mut DatabaseTransactionState,
    request: SqlQueryRequest<'_>,
    parent_context: Option<&hawdb_core::RuntimeTaskContext>,
) -> Result<SqlStatementResult> {
    let result = (|| {
        let task = query_request::bind_request_context(parent_context, request.task_context)?;
        query_runtime::query_runtime_checkpoint(task.as_ref())?;
        if let Some(dependencies) = &mut state.read_dependencies {
            dependencies.admit_sql(request.sql, request.parameters)?;
        }
        let prepared = runtime
            .relational_plan_template_cache
            .prepare(request.sql)?;
        execute_database_transaction_prepared_sql(
            runtime,
            state,
            request.sql,
            prepared,
            request.parameters,
            DatabaseTransactionSqlOptions {
                allow_system_schema_registry_write: false,
                allow_locking_select: false,
                task_context: task.as_ref(),
                output_limits: request.output_limits,
                join_planning: request.join_planning,
            },
        )
    })();
    if result.is_err() {
        state.fail_read_dependencies();
    }
    result
}

pub(super) fn execute_database_transaction_sql(
    runtime: &DatabaseTransactionRuntime,
    state: &mut DatabaseTransactionState,
    sql_text: &str,
    parameters: &[Value],
    allow_system_schema_registry_write: bool,
    allow_locking_select: bool,
    task_context: Option<&hawdb_core::RuntimeTaskContext>,
) -> Result<SqlStatementResult> {
    if let Some(dependencies) = &mut state.read_dependencies {
        query_runtime::query_runtime_checkpoint(task_context)?;
        dependencies.admit_sql(sql_text, parameters)?;
    }
    let prepared = runtime.relational_plan_template_cache.prepare(sql_text)?;
    execute_database_transaction_prepared_sql(
        runtime,
        state,
        sql_text,
        prepared,
        parameters,
        DatabaseTransactionSqlOptions {
            allow_system_schema_registry_write,
            allow_locking_select,
            task_context,
            output_limits: QueryStreamOptions::default(),
            join_planning: RelationalJoinPlanningDirective::Auto,
        },
    )
}

pub(super) struct DatabaseTransactionSqlOptions<'a> {
    allow_system_schema_registry_write: bool,
    allow_locking_select: bool,
    task_context: Option<&'a hawdb_core::RuntimeTaskContext>,
    output_limits: QueryStreamOptions,
    join_planning: RelationalJoinPlanningDirective,
}

pub(super) fn execute_database_transaction_prepared_sql(
    runtime: &DatabaseTransactionRuntime,
    state: &mut DatabaseTransactionState,
    sql_text: &str,
    prepared: crate::relational_sql::PreparedRelationalSql,
    parameters: &[Value],
    options: DatabaseTransactionSqlOptions<'_>,
) -> Result<SqlStatementResult> {
    query_runtime::query_runtime_checkpoint(options.task_context)?;
    let max_rows = restrictive_query_limit(
        runtime.read_result_row_limit(),
        options.output_limits.max_rows,
    );
    let admitted_result_bytes = options
        .task_context
        .and_then(hawdb_core::RuntimeTaskContext::memory_reservation)
        .map(|reservation| usize::try_from(reservation.result_bytes()).unwrap_or(usize::MAX));
    let max_read_result_payload_bytes = match (
        restrictive_query_limit(
            runtime.config.max_read_result_payload_bytes,
            options.output_limits.max_payload_bytes,
        ),
        admitted_result_bytes,
    ) {
        (Some(configured), Some(admitted)) => Some(configured.min(admitted)),
        (configured, admitted) => configured.or(admitted),
    };
    if let Some(dependencies) = &mut state.read_dependencies {
        query_runtime::query_runtime_checkpoint(options.task_context)?;
        dependencies.collect_sql(
            sql_text,
            prepared.statement(),
            parameters,
            &state.relational_state,
        )?;
    }
    reject_locking_select_without_manager(prepared.statement(), options.allow_locking_select)?;
    if let crate::sql::SqlStatement::Branch(statement) = prepared.statement() {
        return observability::execute_branch_sql_at_path(
            runtime.branch_catalog_path.as_deref(),
            runtime.current_branch.as_ref(),
            "explicit transaction",
            statement,
            parameters,
            max_rows,
            max_read_result_payload_bytes,
        )
        .map(sql_query_result);
    }
    if !options.allow_system_schema_registry_write
        && hawdb_relational::system_schema::statement_writes_system_schema_registry(
            prepared.statement(),
        )
    {
        return Err(HawDBError::Semantic(
            "hawdb_schema_migrations is read-only outside system schema upgrade".to_string(),
        ));
    }
    if matches!(
        prepared.statement(),
        crate::sql::SqlStatement::Select(select)
            if system_sql::is_virtual_catalog_select(select)
    ) {
        let graph_transaction = state
            .graph_transaction
            .as_ref()
            .expect("database transaction must own a graph workspace");
        let plan_cache_stats = runtime.plan_cache.borrow().stats();
        return system_sql::query_sql_with_params(
            sql_text,
            parameters,
            max_rows,
            max_read_result_payload_bytes,
            &system_sql::SystemSqlContext {
                catalog: graph_transaction.catalog(),
                store: graph_transaction.store(),
                relational_state: &state.relational_state,
                append_state: &state.append_state,
                runtime: system_runtime_snapshot(&runtime.config),
                plan_cache_stats: &plan_cache_stats,
                slow_queries: &[],
                statement_summaries: &[],
            },
        )
        .map(sql_query_result);
    }
    let pending_generated_read_table = match prepared.statement() {
        crate::sql::SqlStatement::Select(select) => {
            select.from.as_ref().map(|from| from.name.as_str())
        }
        crate::sql::SqlStatement::Explain(explain) => match explain.statement.as_ref() {
            crate::sql::SqlStatement::Select(select) => {
                select.from.as_ref().map(|from| from.name.as_str())
            }
            _ => None,
        },
        _ => None,
    };
    if pending_generated_read_table
        .is_some_and(|table| state.pending_generated_append_tables.contains(table))
    {
        return Err(HawDBError::Execution(format!(
            "strict append table {} has uncommitted generated-order rows; reads are unavailable until commit",
            pending_generated_read_table.expect("pending generated table was checked")
        )));
    }
    if let Some(plan) = crate::relational_sql::compile_prepared_append_select_sql(
        &prepared.template,
        parameters,
        &state.append_state,
        max_rows.unwrap_or(usize::MAX),
    )? {
        let store = state
            .graph_transaction
            .as_ref()
            .expect("database transaction must own a graph workspace")
            .store();
        let output = store.read_append_partition_from_state_bounded(
            &state.append_state,
            &plan.table,
            &plan.partition,
            plan.after.as_ref(),
            plan.max_rows,
            max_read_result_payload_bytes.unwrap_or(usize::MAX),
        )?;
        return Ok(sql_query_result(QueryOutput {
            rows: crate::relational_sql::project_append_rows(&plan, &output.rows)?.into(),
        }));
    }
    if let Some(plan) = crate::relational_sql::compile_prepared_append_explain_sql(
        &prepared.template,
        parameters,
        &state.append_state,
        max_rows.unwrap_or(usize::MAX),
    )? {
        let report = if plan.analyze {
            let store = state
                .graph_transaction
                .as_ref()
                .expect("database transaction must own a graph workspace")
                .store();
            Some(
                store
                    .read_append_partition_from_state_bounded(
                        &state.append_state,
                        &plan.select.table,
                        &plan.select.partition,
                        plan.select.after.as_ref(),
                        plan.select.max_rows,
                        max_read_result_payload_bytes.unwrap_or(usize::MAX),
                    )?
                    .report,
            )
        } else {
            None
        };
        return observability::admit_append_explain(
            crate::relational_sql::format_append_explain(&plan, report.as_ref()),
            &runtime.config,
            QueryStreamOptions {
                max_rows,
                max_payload_bytes: max_read_result_payload_bytes,
            },
            options.task_context,
        )
        .map(sql_query_result);
    }
    if matches!(
        prepared.statement(),
        crate::sql::SqlStatement::Select(_) | crate::sql::SqlStatement::Explain(_)
    ) {
        let index_read_mode = if runtime
            .config
            .relational_index_mode
            .requires_authoritative_indexes()
        {
            crate::relational_sql::RelationalIndexReadMode::AuthoritativeTransaction(
                state.authoritative_relational_index()?,
            )
        } else if runtime
            .config
            .relational_index_mode
            .serves_demand_paged_reads()
        {
            crate::relational_sql::RelationalIndexReadMode::TransactionWorkspace
        } else {
            crate::relational_sql::RelationalIndexReadMode::Materialized
        };
        let row_read_mode = match &state.relational_rows {
            Ok(Some(rows)) => crate::relational_sql::RelationalRowReadMode::Transaction {
                store: state
                    .graph_transaction
                    .as_ref()
                    .expect("database transaction must own a graph workspace")
                    .store(),
                rows,
            },
            Ok(None) => crate::relational_sql::RelationalRowReadMode::CanonicalMemory,
            Err(error) => {
                return Err(HawDBError::StorageIntegrity(format!(
                    "authoritative transaction row view could not be pinned: {error}"
                )));
            }
        };
        let output = crate::relational_sql::execute_prepared_relational_query_with_resources(
            prepared,
            parameters,
            &state.relational_state,
            crate::relational_sql::RelationalQueryReadModes::new(index_read_mode, row_read_mode),
            relational_query_resource_context(
                &runtime.config,
                max_rows,
                max_read_result_payload_bytes,
                options.task_context,
            )
            .with_join_planning(options.join_planning),
        )?;
        return Ok(sql_query_result(QueryOutput { rows: output.rows }));
    }

    runtime.ensure_writable(
        state
            .graph_transaction
            .as_ref()
            .expect("database transaction must own a graph workspace")
            .store(),
    )?;
    if let Some(transaction) = crate::relational_sql::compile_prepared_append_statement_sql(
        &prepared.template,
        parameters,
        &state.append_state,
    )? {
        if let crate::sql::SqlStatement::CreateTable(create) = prepared.statement()
            && state
                .relational_state
                .table_schema(&create.table.name)
                .is_some()
        {
            return Err(HawDBError::Semantic(format!(
                "table {} already exists as a RowPage table",
                create.table.name
            )));
        }
        state.append_state = state
            .append_state
            .stage_provisional_transaction(
                &transaction,
                hawdb_storage::append_table::AppendMutationLimits::default(),
            )
            .map_err(map_transaction_append_error)?;
        state
            .pending_generated_append_tables
            .extend(transaction.writes.iter().filter_map(|write| match write {
                hawdb_storage::append_table::AppendWrite::AppendGenerated { table, rows }
                    if !rows.is_empty() =>
                {
                    Some(table.clone())
                }
                _ => None,
            }));
        state.append_transaction.writes.extend(transaction.writes);
        return Ok(sql_query_result(QueryOutput {
            rows: Vec::new().into(),
        }));
    }
    if let crate::sql::SqlStatement::CreateTable(create) = prepared.statement()
        && state.append_state.schema(&create.table.name).is_some()
    {
        return Err(HawDBError::Semantic(format!(
            "table {} already exists as a strict append table",
            create.table.name
        )));
    }
    let compiled = crate::relational_sql::compile_prepared_relational_statement_with_result(
        &prepared.template,
        parameters,
        &state.relational_state,
    )?;
    let transaction = compiled.transaction;
    let mut pending_index_capture = None;
    let mut pending_row_capture = None;
    let (next_relational_state, mutation_outcomes) = if runtime
        .config
        .relational_index_mode
        .requires_authoritative_indexes()
    {
        if let Some(staged) =
            state.stage_sparse_authoritative_relational_statement(transaction.clone())?
        {
            pending_index_capture = Some(staged.index_capture);
            pending_row_capture = Some(staged.row_capture);
            (staged.state, staged.mutation_outcomes)
        } else {
            let index = match &mut state.relational_index {
                Ok(Some(index)) => index,
                Ok(None) => {
                    return Err(HawDBError::StorageIntegrity(
                        "authoritative transaction index view is unavailable".to_string(),
                    ));
                }
                Err(error) => {
                    return Err(HawDBError::StorageIntegrity(format!(
                        "authoritative transaction index view could not be pinned: {error}"
                    )));
                }
            };
            let (next, capture, outcomes) = state
                .relational_state
                .stage_transaction_with_authoritative_index_and_outcomes(
                    transaction.clone(),
                    hawdb_storage::relational::RelationalMutationLimits::default(),
                    hawdb_storage::relational::RelationalOverflowConfig::default(),
                    index.capture_limits(),
                    index,
                )
                .map_err(map_transaction_relational_error)?;
            pending_index_capture = Some(capture);
            (next, outcomes)
        }
    } else {
        state
            .relational_state
            .stage_transaction_with_outcomes(
                transaction.clone(),
                hawdb_storage::relational::RelationalMutationLimits::default(),
                hawdb_storage::relational::RelationalOverflowConfig::default(),
            )
            .map_err(map_transaction_relational_error)?
    };
    if mutation_outcomes.len() > 1 {
        return Err(HawDBError::StorageIntegrity(
            "one SQL statement produced multiple relational mutation outcomes".to_string(),
        ));
    }
    let mutation = mutation_outcomes
        .first()
        .map(|outcome| {
            project_relational_mutation_outcome(
                outcome,
                compiled.returning.as_ref(),
                &next_relational_state,
                runtime.config.mutation_limits,
                true,
            )
        })
        .transpose()?;
    if let Some(mutation) = &mutation {
        runtime.validate_result_rows(mutation.rows.len())?;
        if let Some(limit) = max_rows
            && mutation.rows.len() > limit
        {
            return Err(HawDBError::read_budget_exceeded(
                crate::ReadBudgetResource::Rows,
                limit,
                "SQL mutation result exceeds the request row ceiling",
            ));
        }
        runtime.validate_result_payload(
            mutation.rows.payload_bytes(),
            max_read_result_payload_bytes,
        )?;
    }
    let next_relational_rows = if let Some(row_capture) = pending_row_capture {
        let rows = match &state.relational_rows {
            Ok(Some(rows)) => rows,
            Ok(None) => {
                return Err(HawDBError::StorageIntegrity(
                    "relational row transaction view is unavailable".to_string(),
                ));
            }
            Err(error) => {
                return Err(HawDBError::StorageIntegrity(format!(
                    "relational transaction row view could not be pinned: {error}"
                )));
            }
        };
        Some(
            rows.stage_advance(row_capture)
                .map_err(map_transaction_relational_error)?,
        )
    } else {
        None
    };
    if let Some(index_capture) = pending_index_capture {
        state
            .relational_index
            .as_mut()
            .map_err(|error| {
                HawDBError::StorageIntegrity(format!(
                    "authoritative transaction index view could not be pinned: {error}"
                ))
            })?
            .as_mut()
            .ok_or_else(|| {
                HawDBError::StorageIntegrity(
                    "authoritative transaction index view is unavailable".to_string(),
                )
            })?
            .append(index_capture)
            .map_err(map_transaction_relational_error)?;
    }
    if let Some(next_relational_rows) = next_relational_rows {
        state.relational_rows = Ok(Some(next_relational_rows));
    }
    state.relational_state = next_relational_state;
    state
        .relational_transaction
        .writes
        .extend(transaction.writes);
    state
        .relational_returning
        .extend(mutation_outcomes.iter().map(|_| compiled.returning.clone()));
    let rows = mutation
        .as_ref()
        .map(|mutation| mutation.rows.clone())
        .unwrap_or_default();
    Ok(SqlStatementResult {
        output: QueryOutput { rows },
        mutation,
    })
}

fn sql_query_result(output: QueryOutput) -> SqlStatementResult {
    SqlStatementResult {
        output,
        mutation: None,
    }
}

fn project_relational_mutation_outcome(
    outcome: &hawdb_storage::relational::RelationalMutationOutcome,
    returning: Option<&crate::relational_sql::RelationalReturningProjection>,
    state: &hawdb_storage::relational::RelationalState,
    limits: hawdb_storage::mutation::MutationLimits,
    provisional: bool,
) -> Result<RelationalMutationResult> {
    if outcome.affected_rows > limits.max_affected_rows.get() {
        return Err(HawDBError::Execution(format!(
            "relational mutation affects {} rows, exceeding max_affected_rows {}",
            outcome.affected_rows, limits.max_affected_rows
        )));
    }
    let rows = if let Some(returning) = returning {
        if returning.table != outcome.table {
            return Err(HawDBError::StorageIntegrity(format!(
                "relational mutation outcome for table {} was paired with RETURNING for table {}",
                outcome.table, returning.table
            )));
        }
        if outcome.rows.len() > limits.max_result_rows.get() {
            return Err(HawDBError::Execution(format!(
                "relational mutation returns {} rows, exceeding max_result_rows {}",
                outcome.rows.len(),
                limits.max_result_rows
            )));
        }
        let schema = state.table_schema(&outcome.table).ok_or_else(|| {
            HawDBError::StorageIntegrity(format!(
                "relational mutation outcome references unknown table {}",
                outcome.table
            ))
        })?;
        let positions = returning
            .columns
            .iter()
            .map(|column| {
                schema.column_position(column).ok_or_else(|| {
                    HawDBError::StorageIntegrity(format!(
                        "relational mutation outcome references unknown column {column}"
                    ))
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let query_schema = executor::QuerySchema::try_new(returning.columns.clone())?;
        let mut builder = executor::QueryRowsBuilder::with_schema(query_schema, outcome.rows.len());
        for row in &outcome.rows {
            builder.push_values(
                positions
                    .iter()
                    .map(|position| relational_value_to_query_value(&row.values()[*position]))
                    .collect::<Result<Vec<_>>>()?,
            )?;
        }
        let rows = builder.finish();
        if rows.payload_bytes() > limits.max_result_payload_bytes.get() {
            return Err(HawDBError::Execution(format!(
                "relational mutation result contains {} payload bytes, exceeding max_result_payload_bytes {}",
                rows.payload_bytes(), limits.max_result_payload_bytes
            )));
        }
        rows
    } else {
        executor::QueryRows::empty()
    };
    Ok(RelationalMutationResult {
        affected_rows: outcome.affected_rows,
        conflict_rows: outcome.conflict_rows,
        rows,
        provisional,
    })
}

fn relational_value_to_query_value(
    value: &hawdb_storage::relational::RelationalValue,
) -> Result<Value> {
    match value {
        hawdb_storage::relational::RelationalValue::Null => Ok(Value::Null),
        hawdb_storage::relational::RelationalValue::Boolean(value) => Ok(Value::Bool(*value)),
        hawdb_storage::relational::RelationalValue::BigInt(value) => Ok(Value::Int(*value)),
        hawdb_storage::relational::RelationalValue::DoublePrecision(value) => {
            Ok(Value::Float(*value))
        }
        hawdb_storage::relational::RelationalValue::Text(value) => Ok(Value::String(value.clone())),
        hawdb_storage::relational::RelationalValue::Bytea(value) => {
            Ok(Value::Binary(value.clone()))
        }
        hawdb_storage::relational::RelationalValue::Uuid(value) => Ok(Value::Uuid(*value)),
        hawdb_storage::relational::RelationalValue::Vector(value) => {
            Ok(Value::Vector(value.clone()))
        }
        hawdb_storage::relational::RelationalValue::Overflow(_) => {
            Err(HawDBError::StorageIntegrity(
                "logical relational mutation outcome contains an overflow reference".to_string(),
            ))
        }
    }
}

fn map_transaction_append_error(
    error: hawdb_storage::append_table::AppendTableError,
) -> HawDBError {
    match error {
        hawdb_storage::append_table::AppendTableError::FileDescriptors(error) => {
            HawDBError::FileDescriptors(error)
        }
        hawdb_storage::append_table::AppendTableError::SequenceExhausted {
            table,
            watermark,
            requested,
        } => HawDBError::AppendSequenceExhausted {
            table,
            watermark,
            requested,
        },
        hawdb_storage::append_table::AppendTableError::Corruption(message)
        | hawdb_storage::append_table::AppendTableError::Durability(message) => {
            HawDBError::StorageIntegrity(message)
        }
        error @ (hawdb_storage::append_table::AppendTableError::Admission(_)
        | hawdb_storage::append_table::AppendTableError::Schema(_)
        | hawdb_storage::append_table::AppendTableError::Constraint(_)) => {
            HawDBError::Execution(error.to_string())
        }
    }
}

fn map_transaction_relational_error(
    error: hawdb_storage::relational::RelationalError,
) -> HawDBError {
    match error {
        hawdb_storage::relational::RelationalError::FileDescriptors(error) => {
            HawDBError::FileDescriptors(error)
        }
        hawdb_storage::relational::RelationalError::Corruption(message)
        | hawdb_storage::relational::RelationalError::Durability(message) => {
            HawDBError::StorageIntegrity(message)
        }
        error @ (hawdb_storage::relational::RelationalError::Admission(_)
        | hawdb_storage::relational::RelationalError::Schema(_)
        | hawdb_storage::relational::RelationalError::Constraint(_)) => {
            HawDBError::Execution(error.to_string())
        }
    }
}

fn reject_locking_select_without_manager(
    statement: &crate::sql::SqlStatement,
    allow_locking_select: bool,
) -> Result<()> {
    let locking_select = match statement {
        crate::sql::SqlStatement::Select(select) => select.lock_strength.is_some(),
        crate::sql::SqlStatement::Explain(explain) => {
            matches!(explain.statement.as_ref(), crate::sql::SqlStatement::Select(select) if select.lock_strength.is_some())
        }
        _ => false,
    };
    if locking_select && !allow_locking_select {
        return Err(HawDBError::Semantic(
            "FOR UPDATE/SHARE requires a pessimistic concurrent transaction".to_string(),
        ));
    }
    Ok(())
}

pub(super) fn commit_database_transaction_state(
    db: &mut Database,
    state: &mut DatabaseTransactionState,
    allow_stale_rebase: bool,
) -> Result<TransactionCommitResult> {
    let result = commit_database_transaction_state_inner(db, state, allow_stale_rebase);
    db.poison_on_storage_error(&result);
    result
}

fn commit_database_transaction_state_inner(
    db: &mut Database,
    state: &mut DatabaseTransactionState,
    allow_stale_rebase: bool,
) -> Result<TransactionCommitResult> {
    db.ensure_writable()?;
    if let Some(dependencies) = &state.read_dependencies {
        dependencies.validate(db)?;
    }
    let returning = std::mem::take(&mut state.relational_returning);
    let graph_transaction = state
        .graph_transaction
        .take()
        .expect("database transaction must own a graph workspace");
    let relational_transaction = std::mem::take(&mut state.relational_transaction);
    let append_transaction = std::mem::take(&mut state.append_transaction);
    let mut branch_runtime_access = db.runtime.get_mut()?;
    let branch_runtime = &mut *branch_runtime_access;
    let summary = if allow_stale_rebase {
        branch_runtime
            .store
            .commit_rebased_mutation_transaction_relational_and_append(
                &mut branch_runtime.catalog,
                graph_transaction,
                relational_transaction,
                append_transaction,
                db.config.mutation_limits,
            )?
    } else {
        branch_runtime
            .store
            .commit_mutation_transaction_relational_and_append(
                &mut branch_runtime.catalog,
                graph_transaction,
                relational_transaction,
                append_transaction,
                db.config.mutation_limits,
            )?
    };
    drop(branch_runtime_access);
    db.complete_required_relational_row_checkpoint("transaction commit")?;
    if returning.len() != summary.relational_mutation_outcomes.len() {
        return Err(HawDBError::StorageIntegrity(format!(
            "transaction recorded {} relational result projections but committed {} outcomes",
            returning.len(),
            summary.relational_mutation_outcomes.len()
        )));
    }
    let branch_runtime = db.runtime.get()?;
    let mutations = summary
        .relational_mutation_outcomes
        .iter()
        .zip(&returning)
        .map(|(outcome, returning)| {
            project_relational_mutation_outcome(
                outcome,
                returning.as_ref(),
                branch_runtime.store.relational_state(),
                db.config.mutation_limits,
                false,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let mut rows: executor::QueryRows = summary.rows.into();
    if rows.is_empty() && mutations.len() == 1 {
        rows = mutations[0].rows.clone();
    }
    Ok(TransactionCommitResult {
        output: QueryOutput { rows },
        mutations,
        append_mutations: summary.append_mutation_outcomes,
    })
}

impl DatabaseTransaction<'_> {
    pub fn read_dependency_usage(&self) -> Option<TransactionReadDependencyUsage> {
        self.state.read_dependency_usage()
    }

    pub(crate) fn apply_session_config(&mut self, config: SessionConfig) {
        self.runtime.session_config = config;
    }

    pub fn query(&mut self, cypher_text: &str) -> Result<QueryOutput> {
        self.query_with_params(cypher_text, &BTreeMap::new())
    }

    pub fn query_with_params(
        &mut self,
        cypher_text: &str,
        parameters: &BTreeMap<String, Value>,
    ) -> Result<QueryOutput> {
        self.query_request(QueryRequest::new(cypher_text).with_params(parameters))
    }

    pub fn query_request(&mut self, request: QueryRequest<'_>) -> Result<QueryOutput> {
        let result = execute_concurrent_graph_transaction_request(
            &self.runtime,
            &mut self.state,
            request,
            self.task_context.as_ref(),
        )
        .map(|outcome| outcome.output);
        if result.is_err() {
            self.state.fail_read_dependencies();
        }
        result
    }

    pub fn query_sql(&mut self, sql_text: &str) -> Result<QueryOutput> {
        self.query_sql_with_params(sql_text, &[])
    }

    pub fn query_sql_with_params(
        &mut self,
        sql_text: &str,
        parameters: &[Value],
    ) -> Result<QueryOutput> {
        self.query_sql_with_result_and_params(sql_text, parameters)
            .map(|result| result.output)
    }

    /// Executes SQL and returns both PostgreSQL rows and a provisional mutation outcome.
    pub fn query_sql_with_result(&mut self, sql_text: &str) -> Result<SqlStatementResult> {
        self.query_sql_with_result_and_params(sql_text, &[])
    }

    /// Parameterized form of [`Self::query_sql_with_result`].
    pub fn query_sql_with_result_and_params(
        &mut self,
        sql_text: &str,
        parameters: &[Value],
    ) -> Result<SqlStatementResult> {
        self.query_sql_request_with_result(SqlQueryRequest::new(sql_text).with_params(parameters))
    }

    pub fn query_sql_request(&mut self, request: SqlQueryRequest<'_>) -> Result<QueryOutput> {
        self.query_sql_request_with_result(request)
            .map(|result| result.output)
    }

    pub fn query_sql_request_with_result(
        &mut self,
        request: SqlQueryRequest<'_>,
    ) -> Result<SqlStatementResult> {
        execute_database_transaction_sql_request(
            &self.runtime,
            &mut self.state,
            request,
            self.task_context.as_ref(),
        )
    }

    pub(super) fn query_system_schema_sql(&mut self, sql_text: &str) -> Result<QueryOutput> {
        self.query_system_schema_sql_with_params(sql_text, &[])
    }

    pub(super) fn query_system_schema_sql_with_params(
        &mut self,
        sql_text: &str,
        parameters: &[Value],
    ) -> Result<QueryOutput> {
        execute_database_transaction_sql(
            &self.runtime,
            &mut self.state,
            sql_text,
            parameters,
            true,
            false,
            self.task_context.as_ref(),
        )
        .map(|result| result.output)
    }

    pub fn commit(self) -> Result<QueryOutput> {
        self.commit_with_result().map(|result| result.output)
    }

    /// Commits and returns outcomes recomputed against the durable commit state.
    pub fn commit_with_result(mut self) -> Result<TransactionCommitResult> {
        if let Some(dependencies) = &self.state.read_dependencies {
            dependencies.ensure_complete()?;
            query_runtime::query_runtime_checkpoint(self.task_context.as_ref())?;
        }
        commit_database_transaction_state(self.db, &mut self.state, false)
    }

    pub fn rollback(mut self) {
        self.state.rollback();
    }
}

impl DatabaseSession<'_> {
    pub fn config(&self) -> &SessionConfig {
        &self.config
    }

    /// Changes this session only. Active transactions retain the settings
    /// captured at BEGIN; the next transaction inherits the new settings.
    pub fn set_max_result_rows(&mut self, max_result_rows: u64) {
        self.config.max_result_rows = max_result_rows;
    }

    fn row_limit(&self) -> Option<usize> {
        restrictive_query_limit(
            self.db.config.read_result_row_limit(),
            self.config.row_limit(),
        )
    }

    pub fn query_sql(&mut self, sql_text: &str) -> Result<QueryOutput> {
        self.query_sql_with_params(sql_text, &[])
    }

    pub fn query_sql_with_params(
        &mut self,
        sql_text: &str,
        parameters: &[Value],
    ) -> Result<QueryOutput> {
        if self.graph_transaction.is_none() {
            let row_limit = self.row_limit();
            return self.db.query_sql_with_params_options_and_system_variables(
                sql_text,
                parameters,
                QueryStreamOptions {
                    max_rows: row_limit,
                    max_payload_bytes: self.db.config.max_read_result_payload_bytes,
                },
                self.system_variables.clone(),
            );
        }
        let prepared = self
            .db
            .runtime
            .get()?
            .relational_plan_template_cache
            .prepare(sql_text)?;
        if let crate::sql::SqlStatement::Branch(statement) = prepared.statement() {
            let runtime = self
                .transaction_runtime
                .as_ref()
                .expect("active session transaction runtime");
            return observability::execute_branch_sql_at_path(
                runtime.branch_catalog_path.as_deref(),
                runtime.current_branch.as_ref(),
                "explicit transaction",
                statement,
                parameters,
                runtime.read_result_row_limit(),
                runtime.config.max_read_result_payload_bytes,
            );
        }
        Err(HawDBError::Execution(
            "SQL data statements are unavailable in an explicit Cypher session transaction".into(),
        ))
    }

    /// Opens a retained numeric cursor using this session's row ceiling.
    /// Arrow streams derived from it inherit the same terminal-error contract.
    pub fn query_with_params_retained(
        &self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        mut options: RetainedQueryOptions,
    ) -> std::result::Result<RetainedQueryCursor, RetainedQueryError> {
        if self.graph_transaction.is_some() {
            return Err(RetainedQueryError::UnsupportedPlan);
        }
        options.max_result_rows =
            restrictive_query_limit(self.row_limit(), options.max_result_rows);
        self.db
            .query_with_params_retained(cypher, parameters, options)
    }

    pub fn system_variables(&self) -> &QuerySystemVariables {
        &self.system_variables
    }

    pub fn query_work_request(&self) -> WorkRequest {
        self.system_variables.query_work_request()
    }

    pub fn query_work_request_for(&self, cypher_text: &str) -> Result<WorkRequest> {
        let statement = cypher::parse(cypher_text)?;
        query_work_request_for_statement(&self.system_variables, &statement)
    }

    pub fn explain_query(&self, cypher_text: &str) -> Result<ExplainOutput> {
        self.explain_query_with_params(cypher_text, &BTreeMap::new())
    }

    pub fn explain_query_with_params(
        &self,
        cypher_text: &str,
        parameters: &BTreeMap<String, Value>,
    ) -> Result<ExplainOutput> {
        if self.graph_transaction.is_some() {
            return Err(HawDBError::Execution(
                "EXPLAIN is not allowed inside an active transaction".to_string(),
            ));
        }
        let statement = cypher::parse(cypher_text)?;
        let work_request = query_work_request_for_statement(&self.system_variables, &statement)?;
        let optimized =
            self.db
                .optimized_explain_query_plan(cypher_text, &statement, parameters)?;
        Ok(ExplainOutput {
            physical_plan: optimized.physical_plan,
            trace: optimized.trace,
            work_request,
            plan_cache_lookup: optimized.plan_cache_lookup,
            statement_kind: statement_kind(statement_body(&statement)),
        })
    }

    pub fn query(&mut self, cypher_text: &str) -> Result<QueryOutput> {
        self.query_with_params(cypher_text, &BTreeMap::new())
    }

    pub fn query_with_params(
        &mut self,
        cypher_text: &str,
        parameters: &BTreeMap<String, Value>,
    ) -> Result<QueryOutput> {
        let statement = cypher::parse(cypher_text)?;
        let body = statement_body(&statement);
        match body {
            cypher::Statement::BeginTransaction => {
                reject_transaction_control_parameters("BEGIN TRANSACTION", parameters)?;
                if self.graph_transaction.is_some() {
                    return Err(HawDBError::Execution(
                        "transaction is already active".to_string(),
                    ));
                }
                self.db.ensure_writable()?;
                self.transaction_runtime = Some(
                    DatabaseTransactionRuntime::from_database_with_system_variables(
                        self.db,
                        self.system_variables.clone(),
                        self.config,
                    )?,
                );
                let branch_runtime = self.db.runtime.get()?;
                self.graph_transaction = Some(
                    branch_runtime
                        .store
                        .begin_mutation_transaction(&branch_runtime.catalog),
                );
                Ok(QueryOutput {
                    rows: Vec::new().into(),
                })
            }
            cypher::Statement::Commit => {
                reject_transaction_control_parameters("COMMIT", parameters)?;
                let Some(transaction) = self.graph_transaction.take() else {
                    return Err(HawDBError::Execution(
                        "COMMIT requires an active transaction".to_string(),
                    ));
                };
                let runtime = self.transaction_runtime.take().ok_or_else(|| {
                    HawDBError::StorageIntegrity("active session transaction has no runtime".into())
                })?;
                let mut mutation_limits = self.db.config.mutation_limits;
                // Graph summaries keep the MutationLimits payload contract;
                // storage's MutationCommitLimitTracker checks it before WAL publication.
                if let Some(limit) = runtime.read_result_row_limit().and_then(NonZeroUsize::new) {
                    mutation_limits.max_result_rows = mutation_limits.max_result_rows.min(limit);
                }
                self.db.ensure_writable()?;
                let mut branch_runtime_access = self.db.runtime.get_mut()?;
                let branch_runtime = &mut *branch_runtime_access;
                let summary = branch_runtime
                    .store
                    .commit_mutation_transaction_and_relational(
                        &mut branch_runtime.catalog,
                        transaction,
                        hawdb_storage::relational::RelationalTransaction::default(),
                        mutation_limits,
                    )?;
                Ok(QueryOutput {
                    rows: summary.rows.into(),
                })
            }
            cypher::Statement::Rollback => {
                reject_transaction_control_parameters("ROLLBACK", parameters)?;
                if self.graph_transaction.take().is_none() {
                    return Err(HawDBError::Execution(
                        "ROLLBACK requires an active transaction".to_string(),
                    ));
                }
                self.transaction_runtime.take();
                Ok(QueryOutput {
                    rows: Vec::new().into(),
                })
            }
            cypher::Statement::Checkpoint if self.graph_transaction.is_some() => {
                Err(HawDBError::Execution(
                    "CHECKPOINT is not allowed inside an active transaction".to_string(),
                ))
            }
            cypher::Statement::SetSystemVariable(_) if self.graph_transaction.is_some() => {
                Err(HawDBError::Execution(
                    "SET system variable is not allowed inside an active transaction".to_string(),
                ))
            }
            cypher::Statement::SetSystemVariable(set) => {
                reject_system_variable_parameters(parameters)?;
                apply_set_system_variable(&mut self.system_variables, set)
            }
            cypher::Statement::Explain(_) if self.graph_transaction.is_some() => {
                Err(HawDBError::Execution(
                    "EXPLAIN is not allowed inside an active transaction".to_string(),
                ))
            }
            cypher::Statement::Explain(explain) => {
                self.execute_explain_statement(cypher_text, explain, parameters)
            }
            statement if self.graph_transaction.is_some() => {
                let transaction = self
                    .graph_transaction
                    .as_mut()
                    .expect("checked active transaction");
                let runtime = self
                    .transaction_runtime
                    .as_ref()
                    .expect("active session transaction must own a query runtime");
                execute_graph_transaction_statement(
                    runtime,
                    transaction,
                    &self.system_variables,
                    cypher_text,
                    statement,
                    parameters,
                    GraphTransactionStatementOptions {
                        execution: query_runtime::QueryExecutionOptions::for_request(
                            QueryRequest::new(cypher_text).with_params(parameters),
                            None,
                        ),
                        read_dependencies: None,
                    },
                )
                .map(|outcome| outcome.output)
            }
            _ => {
                query_work_request_for_statement(&self.system_variables, &statement)?;
                self.db.query_request(
                    QueryRequest::new(cypher_text)
                        .with_params(parameters)
                        .with_output_limits(QueryStreamOptions {
                            max_rows: self.row_limit(),
                            max_payload_bytes: None,
                        }),
                )
            }
        }
    }

    fn execute_explain_statement(
        &mut self,
        cypher_text: &str,
        explain: &cypher::Explain,
        parameters: &BTreeMap<String, Value>,
    ) -> Result<QueryOutput> {
        let work_request =
            query_work_request_for_statement(&self.system_variables, &explain.statement)?;
        let optimized =
            self.db
                .optimized_explain_query_plan(cypher_text, &explain.statement, parameters)?;
        let options =
            query_runtime::QueryExecutionOptions::for_bounded_read(self.row_limit(), None, None);
        let inner_statement_kind = statement_kind(statement_body(&explain.statement));
        if explain.analyze {
            if executor::is_mutation_plan(&optimized.physical_plan)? {
                return Err(HawDBError::Execution(
                    "EXPLAIN ANALYZE only supports read queries".to_string(),
                ));
            }
            let row_limit = self.row_limit();
            let mut external = executor::NoExternalReadOperator;
            let mut branch_runtime_access = self.db.runtime.get_mut()?;
            let branch_runtime = &mut *branch_runtime_access;
            let profiled = executor::execute_with_request(
                executor::ExecutionRequest::new(
                    &optimized.physical_plan,
                    parameters,
                    &self.db.config.execution_memory,
                )
                .with_output_limits(row_limit, self.db.config.max_read_result_payload_bytes),
                executor::ExecutionResources::new(
                    &mut branch_runtime.catalog,
                    &mut branch_runtime.store,
                    &mut external,
                ),
            );
            drop(branch_runtime_access);
            self.db.poison_on_storage_error(&profiled);
            let profiled = profiled?;
            let row = explain_analyze_output_row(
                &optimized,
                work_request,
                inner_statement_kind,
                profiled.rows.len(),
                &profiled.profile,
            );
            drop(profiled.rows);
            return explain::admit_explain_output(row, &self.db.config, options);
        }
        explain::admit_explain_output(
            explain_output_row(&optimized, work_request, inner_statement_kind),
            &self.db.config,
            options,
        )
    }
}

fn reject_transaction_control_parameters(
    statement: &str,
    parameters: &BTreeMap<String, Value>,
) -> Result<()> {
    if parameters.is_empty() {
        Ok(())
    } else {
        Err(HawDBError::Semantic(format!(
            "{statement} does not accept parameters"
        )))
    }
}

fn profiled_relational_sql_output(
    output: crate::relational_sql::RelationalQueryOutput,
) -> ProfiledRelationalSqlQueryOutput {
    let crate::relational_sql::RelationalQueryOutput {
        rows,
        stage_timings,
        join_planning,
        operator_cardinality_profiles,
        intermediate_rows,
        hydration,
        index_execution_evidence,
        row_execution_evidence,
        blocking_operator_memory_reports,
        ..
    } = output;
    let profile = RelationalSqlReadProfile {
        stage_timings,
        join_planning,
        operator_cardinality_profiles,
        intermediate_rows,
        hydrated_rows: hydration.hydrated_rows,
        hydrated_compressed_bytes: hydration.compressed_bytes,
        hydrated_decompressed_bytes: hydration.decompressed_bytes,
        index_reads: index_execution_evidence
            .into_iter()
            .map(|evidence| {
                let runtime_path = evidence.runtime_path().to_string();
                RelationalSqlIndexReadProfile {
                    table: evidence.table,
                    index: evidence.index,
                    lookups: evidence.lookups,
                    metadata_count_lookups: evidence.metadata_count_lookups,
                    runtime_path,
                    logical_pages: evidence.logical_pages,
                    logical_bytes: evidence.logical_bytes,
                    physical_pages: evidence.file_pages,
                    physical_bytes: evidence.file_bytes,
                    cache_hits: evidence.cache_hits,
                    cache_misses: evidence.cache_misses,
                    cache_admission_rejections: evidence.cache_admission_rejections,
                    rows_visited: evidence.rows_visited,
                }
            })
            .collect(),
        row_read: RelationalSqlRowReadProfile {
            runtime_path: row_execution_evidence.runtime_path.to_string(),
            projection_generation: row_execution_evidence.projection_generation,
            projection_source_watermark: row_execution_evidence.projection_source_watermark,
            projection_version: row_execution_evidence.projection_version,
            projection_publication_commit_epoch: row_execution_evidence
                .projection_publication_commit_epoch,
            base_generation: row_execution_evidence.base_generation,
            delta_generation: row_execution_evidence.delta_generation,
            base_commit_epoch: row_execution_evidence.base_commit_epoch,
            visible_commit_epoch: row_execution_evidence.visible_commit_epoch,
            root_set_digest: row_execution_evidence.root_set_digest,
            descriptor_reads: row_execution_evidence.descriptor_reads,
            logical_pages: row_execution_evidence.logical_pages,
            logical_bytes: row_execution_evidence.logical_bytes,
            physical_pages: row_execution_evidence.file_pages,
            physical_bytes: row_execution_evidence.file_bytes,
            cache_hits: row_execution_evidence.cache_hits,
            cache_misses: row_execution_evidence.cache_misses,
            cache_admission_rejections: row_execution_evidence.cache_admission_rejections,
            rows_visited: row_execution_evidence.rows_visited,
            overlay_entries: row_execution_evidence.overlay_entries,
            overlay_resident_bytes: row_execution_evidence.overlay_resident_bytes,
        },
        blocking_operator_memory_reports,
    };
    ProfiledRelationalSqlQueryOutput {
        output: QueryOutput { rows },
        profile,
    }
}

struct DatabaseReadSqlOptions<'a> {
    max_rows: Option<usize>,
    max_payload_bytes: Option<usize>,
    task_context: &'a hawdb_core::RuntimeTaskContext,
    join_planning: RelationalJoinPlanningDirective,
}

impl<S: crate::executor::ExecutionStore> DatabaseReadTransaction<S> {
    fn read_result_row_limit(&self) -> Option<usize> {
        restrictive_query_limit(
            self.config.read_result_row_limit(),
            self.session_config.row_limit(),
        )
    }

    pub(crate) fn apply_session_config(&mut self, config: SessionConfig) {
        self.session_config = config;
    }

    pub fn commit_epoch(&self) -> u64 {
        self.published_read_view.visible_commit_epoch()
    }

    pub const fn published_read_view(&self) -> PublishedReadView {
        self.published_read_view
    }

    pub fn query(&mut self, cypher_text: &str) -> Result<QueryOutput> {
        self.query_with_params(cypher_text, &BTreeMap::new())
    }

    pub fn query_with_params(
        &mut self,
        cypher_text: &str,
        parameters: &BTreeMap<String, Value>,
    ) -> Result<QueryOutput> {
        self.query_with_params_bounded(cypher_text, parameters, self.read_result_row_limit())
    }

    pub fn query_with_params_context(
        &mut self,
        cypher_text: &str,
        parameters: &BTreeMap<String, Value>,
        task_context: &hawdb_core::RuntimeTaskContext,
    ) -> Result<QueryOutput> {
        Ok(self
            .query_with_params_bounded_profile_internal(
                cypher_text,
                parameters,
                self.read_result_row_limit(),
                None,
                Some(task_context),
            )?
            .output)
    }

    pub(crate) fn query_prepared_with_params_profiled_context(
        &mut self,
        prepared: PreparedRuntimeQuery,
        parameters: &BTreeMap<String, Value>,
        options: QueryStreamOptions,
        task_context: &hawdb_core::RuntimeTaskContext,
    ) -> Result<BoundedReadQueryOutput> {
        self.store.ensure_usable()?;
        let (cypher_text, prepared) = prepared.into_execution(&self.catalog, &self.store);
        self.query_with_params_bounded_profile_prepared_output_internal(
            &cypher_text,
            prepared,
            parameters,
            options,
            None,
            Some(task_context),
        )
    }

    pub(crate) fn query_prepared_request(
        &mut self,
        prepared: PreparedRuntimeQuery,
        request: QueryRequest<'_>,
    ) -> Result<BoundedReadQueryOutput> {
        let task =
            query_request::bind_request_context(self.task_context.as_ref(), request.task_context)?;
        let request = QueryRequest {
            task_context: task.as_ref(),
            ..request
        };
        let (cypher_text, prepared) = prepared.into_execution(&self.catalog, &self.store);
        self.query_prepared_request_profiled(
            &cypher_text,
            prepared,
            request.parameters.unwrap_or(&BTreeMap::new()),
            query_runtime::QueryExecutionOptions::for_request(request, None),
            &mut executor::NoExternalReadOperator,
        )
    }

    pub(crate) fn query_prepared_request_traced(
        &mut self,
        prepared: PreparedRuntimeQuery,
        request: QueryRequest<'_>,
        capture_trace: bool,
        external: &mut dyn executor::ExternalReadOperator,
    ) -> Result<(QueryOutput, QueryExecutionTrace)> {
        let task =
            query_request::bind_request_context(self.task_context.as_ref(), request.task_context)?;
        let request = QueryRequest {
            task_context: task.as_ref(),
            ..request
        };
        let mut options = query_runtime::QueryExecutionOptions::for_request(request, None);
        options.capture_trace = capture_trace;
        let (cypher_text, prepared) = prepared.into_execution(&self.catalog, &self.store);
        self.query_prepared_request_traced_internal(
            &cypher_text,
            prepared,
            request.parameters.unwrap_or(&BTreeMap::new()),
            options,
            external,
        )
    }

    pub fn query_with_params_access_control(
        &mut self,
        cypher_text: &str,
        parameters: &BTreeMap<String, Value>,
        access_control: QueryAccessControlContext,
    ) -> Result<QueryOutput> {
        self.query_with_params_bounded_profile_access_control(
            cypher_text,
            parameters,
            self.read_result_row_limit(),
            access_control,
        )
        .map(|output| output.output)
    }

    pub fn query_with_params_bounded(
        &mut self,
        cypher_text: &str,
        parameters: &BTreeMap<String, Value>,
        max_rows: Option<usize>,
    ) -> Result<QueryOutput> {
        Ok(self
            .query_with_params_bounded_profile(cypher_text, parameters, max_rows)?
            .output)
    }

    pub fn query_with_params_bounded_profile(
        &mut self,
        cypher_text: &str,
        parameters: &BTreeMap<String, Value>,
        max_rows: Option<usize>,
    ) -> Result<BoundedReadQueryOutput> {
        let task_context = self.task_context.clone();
        self.query_with_params_bounded_profile_internal(
            cypher_text,
            parameters,
            max_rows,
            None,
            task_context.as_ref(),
        )
    }

    pub fn query_streaming(
        &mut self,
        cypher_text: &str,
        options: QueryStreamOptions,
        consumer: impl FnMut(Row) -> Result<()>,
    ) -> Result<QueryStreamReport> {
        self.query_with_params_streaming(cypher_text, &BTreeMap::new(), options, consumer)
    }

    /// Streams rows from a read plan through a budgeted consumer boundary.
    ///
    /// When either effective output limit is present, the executor invokes the
    /// consumer only after the complete result passes both limit checks. With
    /// both limits disabled, rows cross the consumer boundary as they are
    /// produced and a later non-budget execution error can still be returned.
    pub fn query_with_params_streaming(
        &mut self,
        cypher_text: &str,
        parameters: &BTreeMap<String, Value>,
        options: QueryStreamOptions,
        mut consumer: impl FnMut(Row) -> Result<()>,
    ) -> Result<QueryStreamReport> {
        self.store.ensure_usable()?;
        let task_context = self.task_context.clone();
        self.query_with_params_streaming_prepared_external_internal(
            cypher_text,
            query_runtime::parse_runtime_execution(cypher_text)?,
            parameters,
            options,
            ReadStreamingExecutionContext {
                task_context: task_context.as_ref(),
                external: None,
                delivery: executor::StreamDelivery::Validated,
                scoring: None,
                access_control: None,
            },
            executor::QueryStreamConsumer::Owned(&mut consumer),
        )
    }

    /// Streams borrowed row views to a synchronous host consumer.
    ///
    /// The view cannot outlive the consumer call. Hosts can inspect, serialize,
    /// or load values without an additional clone; hosts that retain a row
    /// must call [`RowRef::to_owned_row`]. Bounded delivery validates the whole
    /// result in compact, schema-aware storage before invoking the consumer.
    /// The scalar producer still materializes a row before handing it to the
    /// accumulator; native producer delivery is qualified separately.
    pub fn query_with_params_streaming_ref(
        &mut self,
        cypher_text: &str,
        parameters: &BTreeMap<String, Value>,
        options: QueryStreamOptions,
        mut consumer: impl for<'row> FnMut(RowRef<'row>) -> Result<()>,
    ) -> Result<QueryStreamReport> {
        self.store.ensure_usable()?;
        let task_context = self.task_context.clone();
        self.query_with_params_streaming_prepared_external_internal(
            cypher_text,
            query_runtime::parse_runtime_execution(cypher_text)?,
            parameters,
            options,
            ReadStreamingExecutionContext {
                task_context: task_context.as_ref(),
                external: None,
                delivery: executor::StreamDelivery::Validated,
                scoring: None,
                access_control: None,
            },
            executor::QueryStreamConsumer::Borrowed(&mut consumer),
        )
    }

    pub fn query_with_params_streaming_context(
        &mut self,
        cypher_text: &str,
        parameters: &BTreeMap<String, Value>,
        options: QueryStreamOptions,
        task_context: &hawdb_core::RuntimeTaskContext,
        mut consumer: impl FnMut(Row) -> Result<()>,
    ) -> Result<QueryStreamReport> {
        self.store.ensure_usable()?;
        self.query_with_params_streaming_prepared_external_internal(
            cypher_text,
            query_runtime::parse_runtime_execution(cypher_text)?,
            parameters,
            options,
            ReadStreamingExecutionContext {
                task_context: Some(task_context),
                external: None,
                delivery: executor::StreamDelivery::Validated,
                scoring: None,
                access_control: None,
            },
            executor::QueryStreamConsumer::Owned(&mut consumer),
        )
    }

    #[cfg_attr(not(feature = "tokio-runtime"), allow(dead_code))]
    pub(crate) fn query_prepared_with_params_streaming_context(
        &mut self,
        prepared: PreparedRuntimeQuery,
        parameters: &BTreeMap<String, Value>,
        options: QueryStreamOptions,
        delivery: executor::StreamDelivery,
        task_context: &hawdb_core::RuntimeTaskContext,
        consumer: impl FnMut(Row) -> Result<()>,
    ) -> Result<QueryStreamReport> {
        self.query_prepared_request_streaming(
            prepared,
            QueryRequest::new("")
                .with_params(parameters)
                .with_task_context(task_context)
                .with_output_limits(options),
            delivery,
            consumer,
        )
    }

    pub(crate) fn query_prepared_request_streaming(
        &mut self,
        prepared: PreparedRuntimeQuery,
        request: QueryRequest<'_>,
        delivery: executor::StreamDelivery,
        mut consumer: impl FnMut(Row) -> Result<()>,
    ) -> Result<QueryStreamReport> {
        let task =
            query_request::bind_request_context(self.task_context.as_ref(), request.task_context)?;
        let request = QueryRequest {
            task_context: task.as_ref(),
            ..request
        };
        let execution = query_runtime::QueryExecutionOptions::for_request(request, None);
        let (cypher_text, prepared) = prepared.into_execution(&self.catalog, &self.store);
        self.query_with_params_streaming_prepared_external_internal(
            &cypher_text,
            prepared,
            request.parameters.unwrap_or(&BTreeMap::new()),
            execution.output_limits,
            ReadStreamingExecutionContext {
                task_context: execution.task_context,
                external: None,
                delivery,
                scoring: execution.scoring,
                access_control: execution.access_control,
            },
            executor::QueryStreamConsumer::Owned(&mut consumer),
        )
    }

    fn query_with_params_streaming_prepared_external_internal(
        &mut self,
        cypher_text: &str,
        mut prepared: query_runtime::PreparedRuntimeExecution,
        parameters: &BTreeMap<String, Value>,
        options: QueryStreamOptions,
        context: ReadStreamingExecutionContext<'_>,
        consumer: executor::QueryStreamConsumer<'_>,
    ) -> Result<QueryStreamReport> {
        let _planning_memory = prepared.planning_memory.take();
        self.store.ensure_usable()?;
        let max_rows = restrictive_query_limit(self.read_result_row_limit(), options.max_rows);
        let max_payload_bytes = restrictive_query_limit(
            self.config.max_read_result_payload_bytes,
            options.max_payload_bytes,
        );
        let prepared_optimized =
            prepared.take_optimized(context.access_control, context.scoring)?;
        let query_runtime::PreparedRuntimeExecution { statement, .. } = prepared;
        let body = statement_body(&statement);
        if matches!(statement, cypher::Statement::Explain(_)) {
            return Err(HawDBError::Execution(
                "streaming query does not support EXPLAIN".to_string(),
            ));
        }
        if matches!(body, cypher::Statement::Checkpoint) {
            return Err(HawDBError::Execution(
                "CHECKPOINT is not allowed inside a read transaction".to_string(),
            ));
        }
        if matches!(body, cypher::Statement::SetSystemVariable(_)) {
            return Err(HawDBError::Execution(
                "SET system variable is not allowed inside a read transaction".to_string(),
            ));
        }
        query_work_request_for_statement(&self.system_variables, &statement)?;
        let optimized = match prepared_optimized {
            Some(optimized) => optimized,
            _ => self.optimized_query_plan_for_request(
                cypher_text,
                &statement,
                parameters,
                context.access_control,
                PlanTraceMode::Template,
                context.scoring,
            )?,
        };
        if executor::is_mutation_plan(&optimized.physical_plan)? {
            return Err(HawDBError::Execution(
                "read transaction query must not be a mutation".to_string(),
            ));
        }
        let streamed = executor::execute_with_request_stream(
            executor::ExecutionRequest::new(
                &optimized.physical_plan,
                parameters,
                &self.config.execution_memory,
            )
            .with_output_limits(max_rows, max_payload_bytes)
            .with_optional_task_context(context.task_context)
            .with_optional_host_scorer(context.scoring.and_then(BoundScoringRequest::host_scorer)),
            executor::ExecutionResources::new(
                &mut self.catalog,
                &mut self.store,
                context
                    .external
                    .unwrap_or(&mut executor::NoExternalReadOperator),
            ),
            context.delivery,
            consumer,
        );
        self.store.poison_on_storage_error(&streamed);
        let streamed = streamed?;
        let pipeline = &streamed.profile.pipeline_memory_report;
        Ok(QueryStreamReport {
            fully_streamed: streamed.fully_streamed,
            output_rows: pipeline.output_rows,
            output_payload_bytes: pipeline.output_payload_bytes,
            execution_profile: streamed.profile,
        })
    }

    pub fn query_with_params_bounded_profile_access_control(
        &mut self,
        cypher_text: &str,
        parameters: &BTreeMap<String, Value>,
        max_rows: Option<usize>,
        access_control: QueryAccessControlContext,
    ) -> Result<BoundedReadQueryOutput> {
        let task_context = self.task_context.clone();
        self.query_with_params_bounded_profile_internal(
            cypher_text,
            parameters,
            max_rows,
            Some(access_control),
            task_context.as_ref(),
        )
    }

    fn query_with_params_bounded_profile_internal(
        &mut self,
        cypher_text: &str,
        parameters: &BTreeMap<String, Value>,
        max_rows: Option<usize>,
        access_control: Option<QueryAccessControlContext>,
        task_context: Option<&hawdb_core::RuntimeTaskContext>,
    ) -> Result<BoundedReadQueryOutput> {
        self.store.ensure_usable()?;
        self.query_with_params_bounded_profile_prepared_internal(
            cypher_text,
            query_runtime::parse_runtime_execution(cypher_text)?,
            parameters,
            max_rows,
            access_control,
            task_context,
        )
    }

    fn query_with_params_bounded_profile_prepared_internal(
        &mut self,
        cypher_text: &str,
        prepared: query_runtime::PreparedRuntimeExecution,
        parameters: &BTreeMap<String, Value>,
        max_rows: Option<usize>,
        access_control: Option<QueryAccessControlContext>,
        task_context: Option<&hawdb_core::RuntimeTaskContext>,
    ) -> Result<BoundedReadQueryOutput> {
        self.query_with_params_bounded_profile_prepared_output_internal(
            cypher_text,
            prepared,
            parameters,
            QueryStreamOptions {
                max_rows,
                max_payload_bytes: None,
            },
            access_control,
            task_context,
        )
    }

    fn query_with_params_bounded_profile_prepared_output_internal(
        &mut self,
        cypher_text: &str,
        prepared: query_runtime::PreparedRuntimeExecution,
        parameters: &BTreeMap<String, Value>,
        options: QueryStreamOptions,
        access_control: Option<QueryAccessControlContext>,
        task_context: Option<&hawdb_core::RuntimeTaskContext>,
    ) -> Result<BoundedReadQueryOutput> {
        let mut execution_options = query_runtime::QueryExecutionOptions::for_bounded_read(
            options.max_rows,
            access_control.as_ref(),
            task_context,
        );
        execution_options.output_limits = options;
        self.query_prepared_request_profiled(
            cypher_text,
            prepared,
            parameters,
            execution_options,
            &mut executor::NoExternalReadOperator,
        )
    }

    fn query_prepared_request_profiled(
        &mut self,
        cypher_text: &str,
        prepared: query_runtime::PreparedRuntimeExecution,
        parameters: &BTreeMap<String, Value>,
        options: query_runtime::QueryExecutionOptions<'_>,
        external: &mut dyn executor::ExternalReadOperator,
    ) -> Result<BoundedReadQueryOutput> {
        let (output, trace) = self.query_prepared_request_traced_internal(
            cypher_text,
            prepared,
            parameters,
            options,
            external,
        )?;
        Ok(BoundedReadQueryOutput {
            output,
            execution_profile: trace
                .execution_profile
                .expect("read execution always supplies a profile"),
        })
    }

    fn query_prepared_request_traced_internal(
        &mut self,
        cypher_text: &str,
        mut prepared: query_runtime::PreparedRuntimeExecution,
        parameters: &BTreeMap<String, Value>,
        options: query_runtime::QueryExecutionOptions<'_>,
        external: &mut dyn executor::ExternalReadOperator,
    ) -> Result<(QueryOutput, QueryExecutionTrace)> {
        let _planning_memory = prepared.planning_memory.take();
        let task_context = options.task_context;
        self.store.ensure_usable()?;
        query_runtime::query_runtime_checkpoint(task_context)?;
        let max_rows =
            restrictive_query_limit(self.read_result_row_limit(), options.output_limits.max_rows);
        let max_payload_bytes = restrictive_query_limit(
            self.config.max_read_result_payload_bytes,
            options.output_limits.max_payload_bytes,
        );
        let prepared_optimized =
            prepared.take_optimized(options.access_control, options.scoring)?;
        let query_runtime::PreparedRuntimeExecution { statement, .. } = prepared;
        let body = statement_body(&statement);
        if let cypher::Statement::Explain(explain) = &statement {
            let result =
                self.execute_explain_request(cypher_text, explain, parameters, options, external)?;
            let mut trace = QueryExecutionTrace::uncached(statement);
            trace.execution_profile = Some(result.execution_profile);
            return Ok((result.output, trace));
        }
        if matches!(body, cypher::Statement::Checkpoint) {
            reject_transaction_control_parameters("CHECKPOINT", parameters)?;
            return Err(HawDBError::Execution(
                "CHECKPOINT is not allowed inside a read transaction".to_string(),
            ));
        }
        if matches!(body, cypher::Statement::SetSystemVariable(_)) {
            reject_system_variable_parameters(parameters)?;
            return Err(HawDBError::Execution(
                "SET system variable is not allowed inside a read transaction".to_string(),
            ));
        }
        query_work_request_for_statement(&self.system_variables, &statement)?;
        let optimized = match prepared_optimized {
            Some(optimized) => optimized,
            _ => self.optimized_query_plan_for_request(
                cypher_text,
                &statement,
                parameters,
                options.access_control,
                PlanTraceMode::Template,
                options.scoring,
            )?,
        };
        if executor::is_mutation_plan(&optimized.physical_plan)? {
            return Err(HawDBError::Execution(
                "read transaction query must not be a mutation".to_string(),
            ));
        }
        let profiled = executor::execute_with_request(
            executor::ExecutionRequest::new(
                &optimized.physical_plan,
                parameters,
                &self.config.execution_memory,
            )
            .with_output_limits(max_rows, max_payload_bytes)
            .with_optional_task_context(task_context)
            .with_optional_host_scorer(options.scoring.and_then(BoundScoringRequest::host_scorer)),
            executor::ExecutionResources::new(&mut self.catalog, &mut self.store, external),
        );
        self.store.poison_on_storage_error(&profiled);
        let profiled = profiled?;
        query_runtime::query_runtime_checkpoint(task_context)?;
        Ok((
            QueryOutput {
                rows: profiled.rows,
            },
            QueryExecutionTrace {
                statement,
                optimizer_trace: options.capture_trace.then_some(optimized.trace),
                plan_cache_lookup: Some(optimized.plan_cache_lookup),
                execution_profile: Some(profiled.profile),
            },
        ))
    }

    fn execute_explain_request(
        &mut self,
        cypher_text: &str,
        explain: &cypher::Explain,
        parameters: &BTreeMap<String, Value>,
        options: query_runtime::QueryExecutionOptions<'_>,
        external: &mut dyn executor::ExternalReadOperator,
    ) -> Result<BoundedReadQueryOutput> {
        let task_context = options.task_context;
        query_runtime::query_runtime_checkpoint(task_context)?;
        let work_request =
            query_work_request_for_statement(&self.system_variables, &explain.statement)?;
        let optimized = self.optimized_query_plan_for_request(
            cypher_text,
            &explain.statement,
            parameters,
            options.access_control,
            PlanTraceMode::Bound,
            options.scoring,
        )?;
        let inner_statement_kind = statement_kind(statement_body(&explain.statement));
        if explain.analyze {
            if executor::requires_write_access(&optimized.physical_plan) {
                return Err(HawDBError::Execution(
                    "EXPLAIN ANALYZE only supports read queries".to_string(),
                ));
            }
            let profiled = executor::execute_with_request(
                executor::ExecutionRequest::new(
                    &optimized.physical_plan,
                    parameters,
                    &self.config.execution_memory,
                )
                .with_output_limits(
                    restrictive_query_limit(
                        self.read_result_row_limit(),
                        options.output_limits.max_rows,
                    ),
                    restrictive_query_limit(
                        self.config.max_read_result_payload_bytes,
                        options.output_limits.max_payload_bytes,
                    ),
                )
                .with_optional_task_context(task_context)
                .with_optional_host_scorer(
                    options.scoring.and_then(BoundScoringRequest::host_scorer),
                ),
                executor::ExecutionResources::new(&mut self.catalog, &mut self.store, external),
            );
            self.store.poison_on_storage_error(&profiled);
            let profiled = profiled?;
            query_runtime::query_runtime_checkpoint(task_context)?;
            let row_count = profiled.rows.len();
            let row = explain_analyze_output_row(
                &optimized,
                work_request,
                inner_statement_kind,
                row_count,
                &profiled.profile,
            );
            drop(profiled.rows);
            return Ok(BoundedReadQueryOutput {
                output: explain::admit_explain_output(row, &self.config, options)?,
                execution_profile: profiled.profile,
            });
        }
        Ok(BoundedReadQueryOutput {
            output: explain::admit_explain_output(
                explain_output_row(&optimized, work_request, inner_statement_kind),
                &self.config,
                options,
            )?,
            execution_profile: empty_read_execution_profile(),
        })
    }
}

impl<S> DatabaseReadTransaction<S>
where
    S: crate::executor::ExecutionStore + hawdb_system_sql::SystemSqlStore,
{
    pub fn query_sql(&self, sql_text: &str) -> Result<QueryOutput> {
        self.query_sql_bounded(sql_text, self.read_result_row_limit())
    }

    pub fn query_sql_with_params(
        &self,
        sql_text: &str,
        parameters: &[Value],
    ) -> Result<QueryOutput> {
        self.query_sql_with_params_bounded(sql_text, parameters, self.read_result_row_limit())
    }

    pub fn query_sql_bounded(
        &self,
        sql_text: &str,
        max_rows: Option<usize>,
    ) -> Result<QueryOutput> {
        self.query_sql_with_params_bounded(sql_text, &[], max_rows)
    }

    pub fn query_sql_with_params_bounded(
        &self,
        sql_text: &str,
        parameters: &[Value],
        max_rows: Option<usize>,
    ) -> Result<QueryOutput> {
        self.query_sql_with_params_options(
            sql_text,
            parameters,
            QueryStreamOptions {
                max_rows,
                max_payload_bytes: self.config.max_read_result_payload_bytes,
            },
        )
    }

    /// Executes PostgreSQL-dialect SQL with per-statement row and payload
    /// admission against this pinned read transaction. Configured database
    /// limits remain hard upper bounds.
    pub fn query_sql_with_params_options(
        &self,
        sql_text: &str,
        parameters: &[Value],
        options: QueryStreamOptions,
    ) -> Result<QueryOutput> {
        self.query_sql_with_params_options_with_join_planning(
            sql_text,
            parameters,
            options,
            RelationalJoinPlanningDirective::Auto,
        )
    }

    /// Executes one bounded relational query under an explicit join planning
    /// directive. `SyntaxOrder` is intended for diagnostics and differential
    /// verification against the default cost-based plan.
    pub fn query_sql_with_params_options_with_join_planning(
        &self,
        sql_text: &str,
        parameters: &[Value],
        options: QueryStreamOptions,
        join_planning: RelationalJoinPlanningDirective,
    ) -> Result<QueryOutput> {
        let default_context = hawdb_core::RuntimeTaskContext::default();
        let task_context = self.task_context.as_ref().unwrap_or(&default_context);
        self.query_sql_with_params_options_context_and_join_planning(
            sql_text,
            parameters,
            options,
            task_context,
            join_planning,
        )
    }

    /// Executes one bounded relational `SELECT` against this pinned snapshot
    /// and returns result rows together with operator cardinality and storage
    /// accounting from that exact execution. Virtual system-catalog queries
    /// and `EXPLAIN` are kept on their dedicated output paths.
    pub fn query_sql_with_params_options_profiled(
        &self,
        sql_text: &str,
        parameters: &[Value],
        options: QueryStreamOptions,
    ) -> Result<ProfiledRelationalSqlQueryOutput> {
        self.query_sql_with_params_options_profiled_with_join_planning(
            sql_text,
            parameters,
            options,
            RelationalJoinPlanningDirective::Auto,
        )
    }

    /// Executes and profiles one relational `SELECT` under an explicit join
    /// planning directive. `SyntaxOrder` is intended for diagnostics and
    /// differential verification against the default cost-based plan.
    pub fn query_sql_with_params_options_profiled_with_join_planning(
        &self,
        sql_text: &str,
        parameters: &[Value],
        options: QueryStreamOptions,
        join_planning: RelationalJoinPlanningDirective,
    ) -> Result<ProfiledRelationalSqlQueryOutput> {
        let default_context = hawdb_core::RuntimeTaskContext::default();
        let task_context = self.task_context.as_ref().unwrap_or(&default_context);
        self.query_sql_with_params_options_profiled_context_and_join_planning(
            sql_text,
            parameters,
            options,
            task_context,
            join_planning,
        )
    }

    /// Profiled relational `SELECT` with host cancellation and deadline
    /// propagation. The returned rows and profile always belong to one
    /// execution of the same pinned read view.
    pub fn query_sql_with_params_options_profiled_context(
        &self,
        sql_text: &str,
        parameters: &[Value],
        options: QueryStreamOptions,
        task_context: &hawdb_core::RuntimeTaskContext,
    ) -> Result<ProfiledRelationalSqlQueryOutput> {
        self.query_sql_with_params_options_profiled_context_and_join_planning(
            sql_text,
            parameters,
            options,
            task_context,
            RelationalJoinPlanningDirective::Auto,
        )
    }

    fn query_sql_with_params_options_profiled_context_and_join_planning(
        &self,
        sql_text: &str,
        parameters: &[Value],
        options: QueryStreamOptions,
        task_context: &hawdb_core::RuntimeTaskContext,
        join_planning: RelationalJoinPlanningDirective,
    ) -> Result<ProfiledRelationalSqlQueryOutput> {
        self.store.ensure_usable()?;
        query_runtime::query_runtime_checkpoint(Some(task_context))?;
        let max_rows = restrictive_query_limit(self.read_result_row_limit(), options.max_rows);
        let max_payload_bytes = restrictive_query_limit(
            self.config.max_read_result_payload_bytes,
            options.max_payload_bytes,
        );
        let prepared = self.relational_plan_template_cache.prepare(sql_text)?;
        reject_locking_select_without_manager(prepared.statement(), false)?;
        let crate::sql::SqlStatement::Select(select) = prepared.statement() else {
            return Err(HawDBError::Semantic(
                "profiled relational SQL requires SELECT".to_string(),
            ));
        };
        if system_sql::is_virtual_catalog_select(select) {
            return Err(HawDBError::Semantic(
                "profiled relational SQL does not support virtual system catalogs".to_string(),
            ));
        }
        self.execute_profiled_relational_sql(
            prepared,
            parameters,
            max_rows,
            max_payload_bytes,
            task_context,
            join_planning,
        )
    }

    /// Executes bounded PostgreSQL-dialect SQL against this pinned read
    /// transaction while propagating host cancellation and deadlines through
    /// planning, index traversal, row hydration, and result construction.
    pub fn query_sql_with_params_options_context(
        &self,
        sql_text: &str,
        parameters: &[Value],
        options: QueryStreamOptions,
        task_context: &hawdb_core::RuntimeTaskContext,
    ) -> Result<QueryOutput> {
        self.query_sql_with_params_options_context_and_join_planning(
            sql_text,
            parameters,
            options,
            task_context,
            RelationalJoinPlanningDirective::Auto,
        )
    }

    fn query_sql_with_params_options_context_and_join_planning(
        &self,
        sql_text: &str,
        parameters: &[Value],
        options: QueryStreamOptions,
        task_context: &hawdb_core::RuntimeTaskContext,
        join_planning: RelationalJoinPlanningDirective,
    ) -> Result<QueryOutput> {
        self.store.ensure_usable()?;
        query_runtime::query_runtime_checkpoint(Some(task_context))?;
        let max_rows = restrictive_query_limit(self.read_result_row_limit(), options.max_rows);
        let max_payload_bytes = restrictive_query_limit(
            self.config.max_read_result_payload_bytes,
            options.max_payload_bytes,
        );
        let prepared = self.relational_plan_template_cache.prepare(sql_text)?;
        self.query_sql_with_prepared_params_context_and_join_planning(
            sql_text,
            parameters,
            prepared,
            DatabaseReadSqlOptions {
                max_rows,
                max_payload_bytes,
                task_context,
                join_planning,
            },
        )
    }

    pub(super) fn query_sql_with_prepared_params(
        &self,
        sql_text: &str,
        parameters: &[Value],
        prepared: crate::relational_sql::PreparedRelationalSql,
    ) -> Result<QueryOutput> {
        self.query_sql_with_prepared_request(
            SqlQueryRequest::new(sql_text).with_params(parameters),
            prepared,
        )
    }

    pub(crate) fn query_sql_with_prepared_request(
        &self,
        request: SqlQueryRequest<'_>,
        prepared: crate::relational_sql::PreparedRelationalSql,
    ) -> Result<QueryOutput> {
        self.store.ensure_usable()?;
        let task =
            query_request::bind_request_context(self.task_context.as_ref(), request.task_context)?;
        let default_context = hawdb_core::RuntimeTaskContext::default();
        let task_context = task.as_ref().unwrap_or(&default_context);
        query_runtime::query_runtime_checkpoint(Some(task_context))?;
        let request = SqlQueryRequest {
            task_context: Some(task_context),
            ..request
        };
        let limits = request.effective_output_limits(&self.config);
        self.query_sql_with_prepared_params_context_and_join_planning(
            request.sql,
            request.parameters,
            prepared,
            DatabaseReadSqlOptions {
                max_rows: limits.max_rows,
                max_payload_bytes: limits.max_payload_bytes,
                task_context,
                join_planning: request.join_planning,
            },
        )
    }

    fn query_sql_with_prepared_params_context_and_join_planning(
        &self,
        sql_text: &str,
        parameters: &[Value],
        prepared: crate::relational_sql::PreparedRelationalSql,
        options: DatabaseReadSqlOptions<'_>,
    ) -> Result<QueryOutput> {
        let DatabaseReadSqlOptions {
            max_rows,
            max_payload_bytes,
            task_context,
            join_planning,
        } = options;
        reject_locking_select_without_manager(prepared.statement(), false)?;
        if let crate::sql::SqlStatement::Branch(statement) = prepared.statement() {
            return observability::execute_branch_sql_at_path(
                self.branch_catalog_path.as_deref(),
                self.current_branch.as_ref(),
                "read transaction",
                statement,
                parameters,
                max_rows,
                max_payload_bytes,
            );
        }
        if matches!(
            prepared.statement(),
            crate::sql::SqlStatement::Select(select)
                if system_sql::is_virtual_catalog_select(select)
        ) {
            let output = system_sql::query_sql_with_params(
                sql_text,
                parameters,
                max_rows,
                max_payload_bytes,
                &system_sql::SystemSqlContext {
                    catalog: &self.catalog,
                    store: &self.store,
                    relational_state: self.store.relational_state(),
                    append_state: self.store.append_state(),
                    runtime: system_runtime_snapshot(&self.config),
                    plan_cache_stats: &self.plan_cache.borrow().stats(),
                    slow_queries: &self.slow_query_snapshot,
                    statement_summaries: &self.statement_summary_snapshot,
                },
            )?;
            query_runtime::query_runtime_checkpoint(Some(task_context))?;
            return Ok(output);
        }

        if let Some(plan) = crate::relational_sql::compile_prepared_append_select_sql(
            &prepared.template,
            parameters,
            self.store.append_state(),
            max_rows.unwrap_or(usize::MAX),
        )? {
            let output = self.store.read_append_partition_bounded(
                &plan.table,
                &plan.partition,
                plan.after.as_ref(),
                plan.max_rows,
                max_payload_bytes.unwrap_or(usize::MAX),
            )?;
            query_runtime::query_runtime_checkpoint(Some(task_context))?;
            return Ok(QueryOutput {
                rows: crate::relational_sql::project_append_rows(&plan, &output.rows)?.into(),
            });
        }
        if let Some(plan) = crate::relational_sql::compile_prepared_append_explain_sql(
            &prepared.template,
            parameters,
            self.store.append_state(),
            max_rows.unwrap_or(usize::MAX),
        )? {
            let report = if plan.analyze {
                Some(
                    self.store
                        .read_append_partition_bounded(
                            &plan.select.table,
                            &plan.select.partition,
                            plan.select.after.as_ref(),
                            plan.select.max_rows,
                            max_payload_bytes.unwrap_or(usize::MAX),
                        )?
                        .report,
                )
            } else {
                None
            };
            query_runtime::query_runtime_checkpoint(Some(task_context))?;
            return observability::admit_append_explain(
                crate::relational_sql::format_append_explain(&plan, report.as_ref()),
                &self.config,
                QueryStreamOptions {
                    max_rows,
                    max_payload_bytes,
                },
                Some(task_context),
            );
        }

        self.execute_profiled_relational_sql(
            prepared,
            parameters,
            max_rows,
            max_payload_bytes,
            task_context,
            join_planning,
        )
        .map(|profiled| profiled.output)
    }

    fn execute_profiled_relational_sql(
        &self,
        prepared: crate::relational_sql::PreparedRelationalSql,
        parameters: &[Value],
        max_rows: Option<usize>,
        max_payload_bytes: Option<usize>,
        task_context: &hawdb_core::RuntimeTaskContext,
        join_planning: RelationalJoinPlanningDirective,
    ) -> Result<ProfiledRelationalSqlQueryOutput> {
        let row_read_mode = match &self.projection_relational {
            Some(projection) => {
                hawdb_relational::row_runtime::RelationalRowReadMode::<S>::ProjectionGeneration {
                    store: &self.store,
                    reader: &projection.reader,
                    tables: projection.binding.tables(),
                }
            }
            None => hawdb_relational::row_runtime::RelationalRowReadMode::<S>::Store(&self.store),
        };
        let query_result = crate::relational_sql::execute_prepared_relational_query_with_resources(
            prepared,
            parameters,
            self.store.relational_state(),
            hawdb_relational::query::RelationalQueryReadModes::<S>::new(
                relational_index_read_mode(&self.config, &self.store),
                row_read_mode,
            ),
            relational_query_resource_context(
                &self.config,
                max_rows,
                max_payload_bytes,
                Some(task_context),
            )
            .with_join_planning(join_planning),
        );
        self.store.poison_on_storage_error(&query_result);
        let output = query_result?;
        query_runtime::query_runtime_checkpoint(Some(task_context))?;
        Ok(profiled_relational_sql_output(output))
    }
}

impl<S: crate::executor::ExecutionStore> DatabaseReadTransaction<S> {
    pub fn explain_query(&self, cypher_text: &str) -> Result<ExplainOutput> {
        self.explain_query_with_params(cypher_text, &BTreeMap::new())
    }

    pub fn explain_query_with_params(
        &self,
        cypher_text: &str,
        parameters: &BTreeMap<String, Value>,
    ) -> Result<ExplainOutput> {
        self.store.ensure_usable()?;
        let statement = cypher::parse(cypher_text)?;
        let work_request = query_work_request_for_statement(&self.system_variables, &statement)?;
        let optimized = self.optimized_explain_query_plan(cypher_text, &statement, parameters)?;
        Ok(ExplainOutput {
            physical_plan: optimized.physical_plan,
            trace: optimized.trace,
            work_request,
            plan_cache_lookup: optimized.plan_cache_lookup,
            statement_kind: statement_kind(statement_body(&statement)),
        })
    }

    pub fn plan_cache_stats(&self) -> PlanCacheStats {
        self.plan_cache.borrow().stats()
    }

    pub fn relational_plan_template_cache_stats(&self) -> PlanCacheStats {
        self.relational_plan_template_cache.stats()
    }

    fn optimized_query_plan_with_access_control(
        &self,
        cypher_text: &str,
        statement: &cypher::Statement,
        parameters: &BTreeMap<String, Value>,
        access_control: Option<&QueryAccessControlContext>,
    ) -> Result<OptimizedQueryPlan> {
        self.optimized_query_plan_with_access_control_and_trace_mode(
            cypher_text,
            statement,
            parameters,
            access_control,
            PlanTraceMode::Template,
        )
    }

    fn optimized_explain_query_plan(
        &self,
        cypher_text: &str,
        statement: &cypher::Statement,
        parameters: &BTreeMap<String, Value>,
    ) -> Result<OptimizedQueryPlan> {
        self.optimized_explain_query_plan_with_access_control(
            cypher_text,
            statement,
            parameters,
            None,
        )
    }

    fn optimized_explain_query_plan_with_access_control(
        &self,
        cypher_text: &str,
        statement: &cypher::Statement,
        parameters: &BTreeMap<String, Value>,
        access_control: Option<&QueryAccessControlContext>,
    ) -> Result<OptimizedQueryPlan> {
        self.optimized_query_plan_with_access_control_and_trace_mode(
            cypher_text,
            statement,
            parameters,
            access_control,
            PlanTraceMode::Bound,
        )
    }

    fn optimized_query_plan_with_access_control_and_trace_mode(
        &self,
        cypher_text: &str,
        statement: &cypher::Statement,
        parameters: &BTreeMap<String, Value>,
        access_control: Option<&QueryAccessControlContext>,
        trace_mode: PlanTraceMode,
    ) -> Result<OptimizedQueryPlan> {
        self.optimized_query_plan_for_request(
            cypher_text,
            statement,
            parameters,
            access_control,
            trace_mode,
            None,
        )
    }

    fn optimized_query_plan_for_request(
        &self,
        cypher_text: &str,
        statement: &cypher::Statement,
        parameters: &BTreeMap<String, Value>,
        access_control: Option<&QueryAccessControlContext>,
        trace_mode: PlanTraceMode,
        scoring: Option<BoundScoringRequest<'_>>,
    ) -> Result<OptimizedQueryPlan> {
        let optimizer_search =
            query_statement_variables_for_statement(&self.system_variables, statement)?
                .optimizer_search;
        let cache_mode = if optimizer_search != OptimizerSearchDirective::Auto {
            PlanCacheMode::Bypass(PlanCacheBypassReason::OptimizerDirective)
        } else if statement_uses_plan_cache(statement) {
            PlanCacheMode::Use
        } else {
            PlanCacheMode::Bypass(PlanCacheBypassReason::StatementNotCacheable)
        };
        optimized_query_plan_for(
            cypher_text,
            statement,
            parameters,
            cache_mode,
            trace_mode,
            PlanCacheContext {
                catalog: &self.catalog,
                store: &self.store,
                optimizer: &self.optimizer,
                config: &self.config,
                cache: &self.plan_cache,
                planning_cache: &self.optimizer_planning_cache,
                access_control,
                optimizer_search,
                scoring,
            },
        )
    }
}

impl<S> DatabaseReadTransaction<S>
where
    S: crate::executor::ExecutionStore + hawdb_analytics::ProjectionSource,
{
    pub fn project_graph(&self, rel_type: Option<&str>) -> ProjectedGraph {
        match rel_type {
            Some(name) => self
                .catalog
                .rel_type_id(name)
                .map(|rel_type_id| ProjectedGraph::from_store(&self.store, Some(rel_type_id)))
                .unwrap_or_else(|| ProjectedGraph::from_store_without_edges(&self.store)),
            None => ProjectedGraph::from_store(&self.store, None),
        }
    }
}

impl<S: crate::executor::ExecutionStore> DatabaseReadTransaction<S> {
    pub fn export_canonical_graph_snapshot(&self) -> CanonicalGraphSnapshotExport {
        export_canonical_graph_snapshot_for(&self.catalog, &self.store)
    }

    pub fn try_export_canonical_graph_snapshot(&self) -> Result<CanonicalGraphSnapshotExport> {
        canonical_snapshot::try_export_canonical_graph_snapshot_for(&self.catalog, &self.store)
    }
}

impl<S> DatabaseReadTransaction<S>
where
    S: crate::executor::ExecutionStore + hawdb_search::SearchProjectionSource,
{
    pub fn rebuild_search_projection(
        &self,
        search_index: &mut SearchIndex,
        options: SearchRebuildOptions,
    ) -> Result<SearchRebuildSummary> {
        search_index.rebuild_from_graph(&self.catalog, &self.store, options)
    }

    pub fn repair_search_projection_metadata(
        &self,
        search_index: &mut SearchIndex,
        options: MetadataRepairOptions,
    ) -> Result<MetadataRepairSummary> {
        search_index.repair_metadata_from_graph(&self.catalog, &self.store, options)
    }
}

impl<S: crate::executor::ExecutionStore> DatabaseReadTransaction<S> {
    /// Retrieves from resident search payloads, propagating search and graph/pipeline errors.
    pub fn retrieve_knowledge(
        &self,
        search_index: &SearchIndex,
        request: &KnowledgeRetrievalRequest,
    ) -> Result<KnowledgeRetrievalOutput> {
        KnowledgeRetrievalGraphContext {
            catalog: &self.catalog,
            store: &self.store,
            compressed_vector_search_mode: self.config.compressed_vector_search_mode,
            adaptive_vector_backend_policy: self.config.adaptive_vector_backend_policy,
            query_memory_budget: self.config.execution_memory.query_memory_bytes,
            result_payload_budget: self
                .config
                .max_read_result_payload_bytes
                .unwrap_or(DEFAULT_MAX_READ_RESULT_PAYLOAD_BYTES),
        }
        .retrieve_knowledge(search_index, request)
    }

    pub fn try_retrieve_knowledge(
        &self,
        search_index: &SearchIndex,
        request: &KnowledgeRetrievalRequest,
    ) -> Result<KnowledgeRetrievalOutput> {
        KnowledgeRetrievalGraphContext {
            catalog: &self.catalog,
            store: &self.store,
            compressed_vector_search_mode: self.config.compressed_vector_search_mode,
            adaptive_vector_backend_policy: self.config.adaptive_vector_backend_policy,
            query_memory_budget: self.config.execution_memory.query_memory_bytes,
            result_payload_budget: self
                .config
                .max_read_result_payload_bytes
                .unwrap_or(DEFAULT_MAX_READ_RESULT_PAYLOAD_BYTES),
        }
        .try_retrieve_knowledge(search_index, request)
    }

    pub(crate) fn try_retrieve_knowledge_from_search(
        &self,
        search: SearchResultSet,
        projection_freshness: SearchProjectionFreshness,
        request: &KnowledgeRetrievalRequest,
    ) -> Result<KnowledgeRetrievalOutput> {
        if let Some(context) = &self.task_context {
            context
                .checkpoint()
                .map_err(|reason| HawDBError::Execution(format!("runtime task {reason}")))?;
        }
        let configured_memory = self.config.execution_memory.query_memory_bytes.get();
        let configured_result = self
            .config
            .max_read_result_payload_bytes
            .unwrap_or(DEFAULT_MAX_READ_RESULT_PAYLOAD_BYTES);
        let (memory, result) = self
            .task_context
            .as_ref()
            .and_then(hawdb_core::RuntimeTaskContext::memory_reservation)
            .map_or((configured_memory, configured_result), |reservation| {
                (
                    configured_memory
                        .min(usize::try_from(reservation.memory_bytes()).unwrap_or(usize::MAX)),
                    configured_result
                        .min(usize::try_from(reservation.result_bytes()).unwrap_or(usize::MAX)),
                )
            });
        let query_memory_budget = NonZeroUsize::new(memory).ok_or_else(|| {
            HawDBError::Execution("knowledge retrieval requires admitted working memory".into())
        })?;
        KnowledgeRetrievalGraphContext {
            catalog: &self.catalog,
            store: &self.store,
            compressed_vector_search_mode: self.config.compressed_vector_search_mode,
            adaptive_vector_backend_policy: self.config.adaptive_vector_backend_policy,
            query_memory_budget,
            result_payload_budget: result,
        }
        .retrieve_knowledge_from_search(search, projection_freshness, request)
    }

    #[cfg(test)]
    pub(crate) fn statistics(&self) -> GraphStatistics {
        <S as hawdb_storage::graph_engine::GraphReadEngine>::statistics(&self.store, &self.catalog)
    }

    #[cfg(test)]
    pub(crate) fn basic_statistics(&self) -> crate::schema::BasicGraphStatistics {
        self.store.basic_statistics()
    }
}

fn single_import_label(node: &CanonicalSnapshotNode) -> Result<String> {
    match node.labels.as_slice() {
        [label] => Ok(label.clone()),
        [] => Err(HawDBError::Storage(
            "HawDB Lightning initial import node has no label".to_string(),
        )),
        _ => Err(HawDBError::Storage(
            "HawDB Lightning initial import multi-label node is unsupported".to_string(),
        )),
    }
}

fn relational_state_counts(state: &hawdb_storage::relational::RelationalState) -> (usize, usize) {
    let table_count = state.table_schemas().count();
    let row_count = state
        .table_schemas()
        .map(|schema| state.row_count(&schema.name))
        .sum();
    (table_count, row_count)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod legacy_graph_seed_budget_tests;

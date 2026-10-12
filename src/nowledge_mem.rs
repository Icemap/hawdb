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

#[cfg(test)]
use crate::api::{
    KnowledgeEntityDeleteBatchOutput, KnowledgeEntityDeleteBatchRequest,
    KnowledgeMemoryEvolvesCreateBatchOutput, KnowledgeMemoryEvolvesCreateBatchRequest,
    KnowledgeMemoryLifecycleBatchOutput, KnowledgeMemoryLifecycleBatchRequest,
};
use crate::search::{
    AdaptiveVectorSearchOptions, CompressedVectorSearchMode, SearchFallbackReasonCode,
    SearchLexicalProductionQualificationReport, SearchOutOfCoreConfig,
    SearchOutOfCoreHydrationOutput, SearchOutOfCoreMetrics, SearchOutOfCoreReader,
    SearchQueryOptions, SearchRangeReadConfig, VectorRecallProductionQualificationReport,
    VectorRecallValidationOptions, VectorRecallValidationReport, VectorSearchExecutionOptions,
    NOWLEDGE_SEARCH_PROJECTION_SCAN_FILTER_FIELDS,
};
use crate::search_projection_evidence::{
    nowledge_search_projection_evidence_json, nowledge_search_projection_shadow_evidence_json,
    NowledgeSearchProjectionEvidenceReport,
};
use crate::{
    cypher, BackgroundMaintenanceKind, BackgroundMaintenanceOptions, BackgroundMaintenanceSummary,
    BackgroundWorkHint, BackgroundWorkPlan, BoundedReadQueryOutput, Database, DatabaseConfig,
    DatabaseReadTransaction, GraphRagGeneratedQuery, GraphRagSchemaContext,
    GraphRagSchemaContextOptions, HawDBError, HawDBLightningBootstrapManifest,
    HawDBLightningInitialImportApplyReport, HawDBLightningInitialImportCheckpoint,
    HawDBLightningInitialImportCutoverCatchUpReport, HawDBLightningInitialImportDocumentIdentity,
    HawDBLightningInitialImportRecoveryReadinessReport, KnowledgeRetrievalOutput,
    KnowledgeRetrievalRequest, LocalQosPolicy, LocalQosState, NowledgeGraphStatement,
    PlanCacheLookup, QueryOutput, QueryStreamOptions, QueryStreamReport, ReadBudgetResource,
    ReadExecutionProfile, Result, ScheduledSearchProjectionCatchUpReport, SearchDocument,
    SearchIndex, SearchProjectionCatchUpReport, SearchProjectionChangeBatch,
    SearchProjectionChangefeedReadiness, SearchProjectionDelta, SearchProjectionDeltaReport,
    SearchProjectionFreshness, SearchProjectionGraphDeltaRequest, SearchProjectionProbeOptions,
    SearchProjectionRelationalDelta, SearchResultSet, StorageResourceProfileLimits,
    StorageResourceProfileReport, TelemetrySink, Value, STORAGE_RESOURCE_PROFILE_PROTOCOL,
};
use crate::{
    graph_route_readiness::NMEM_GRAPH_ROUTE_READINESS_PROTOCOL,
    nowledge_inventory::{
        background_maintenance_evidence_health, background_maintenance_summary_to_json,
        replacement_readiness_family_evidence_health, REQUIRED_NOWLEDGE_REPLACEMENT_QUERY_FAMILIES,
    },
    route_ownership::NowledgeMemRouteOwnershipReadinessReport,
    search_route_ownership::{
        NowledgeMemActiveSearchRouteOwnershipReadinessReport,
        NowledgeMemActiveSearchRouteReadinessReport,
        NowledgeMemSearchRouteOwnershipReadinessReport,
        NOWLEDGE_MEM_ACTIVE_SEARCH_ROUTE_READINESS_PROTOCOL,
        NOWLEDGE_MEM_SEARCH_ROUTE_OWNERSHIP_PROTOCOL, REQUIRED_NOWLEDGE_MEM_ACTIVE_SEARCH_ROUTES,
        REQUIRED_NOWLEDGE_MEM_SEARCH_ROUTES,
    },
    workload_fixtures::{
        NowledgeGraphRouteWorkloadFixtureReport, NOWLEDGE_GRAPH_ROUTE_WORKLOAD_FIXTURE_PROTOCOL,
    },
};
use hawdb_core::RuntimeTaskContext;
mod incremental_search;
use hawdb_optimizer::AdaptiveVectorBackendPolicy;
use hawdb_qos::{
    IoConcurrencyBudget, RuntimeGovernor, RuntimeGovernorConfig, RuntimeGovernorSnapshot,
    RuntimePermit, RuntimeWorkKind, RuntimeWorkPriority, RuntimeWorkRequest, StorageDeviceProfile,
    WorkPriority,
};
pub use hawdb_readiness::bounded_read_evidence::{
    nowledge_mem_bounded_read_evidence_json,
    nowledge_mem_bounded_read_evidence_json_with_route_readiness,
    nowledge_mem_bounded_read_evidence_json_with_routes, NowledgeMemGraphMode,
    NowledgeMemReadReport, NowledgeMemRouteReadinessSummary,
    NOWLEDGE_MEM_BOUNDED_READ_EVIDENCE_PROTOCOL, NOWLEDGE_MEM_READ_REPORT_PROTOCOL,
};
use hawdb_readiness::nowledge_mem_query_report::nowledge_mem_plan_cache_report;
pub use hawdb_readiness::nowledge_mem_query_report::{
    nowledge_mem_fast_path_classification, NowledgeMemFastPathClassification,
    NowledgeMemQueryApiBehavior, NowledgeMemQueryExecutionPath, NowledgeMemQueryOutput,
    NowledgeMemQueryOutputRowShape, NowledgeMemQueryReport, NowledgeMemQueryReportOptions,
    NOWLEDGE_MEM_QUERY_REPORT_PROTOCOL,
};
pub use hawdb_readiness::query_runtime_preflight::NowledgeQueryRuntimePreflightProbe;
use hawdb_readiness::query_runtime_preflight_report::{
    hawdb_error_class, nowledge_query_runtime_route_coverage,
    query_runtime_preflight_blocker_codes, query_runtime_probe_blocker_codes,
};
pub use hawdb_readiness::query_runtime_preflight_report::{
    NowledgeQueryRuntimePreflightProbeReport, NowledgeQueryRuntimePreflightRedactionSummary,
    NowledgeQueryRuntimePreflightReport,
};
pub use hawdb_readiness::slow_query::{
    NowledgeMemSlowQueryRecord, NowledgeMemSlowQueryReport, NOWLEDGE_MEM_SLOW_QUERY_REPORT_PROTOCOL,
};
pub use hawdb_readiness::{NowledgeMemReadinessAreaMap, NowledgeMemReadinessAreaSummary};
pub use incremental_search::NowledgeMemIncrementalSearchMaintenanceOptions;

#[cfg(test)]
mod slow_query_facade_tests {
    use super::*;
    use std::any::TypeId;

    #[test]
    fn facade_reexports_slow_query_contracts_without_conversion() {
        assert_eq!(
            TypeId::of::<NowledgeMemSlowQueryRecord>(),
            TypeId::of::<hawdb_readiness::slow_query::NowledgeMemSlowQueryRecord>(),
        );
        assert_eq!(
            TypeId::of::<NowledgeMemSlowQueryReport>(),
            TypeId::of::<hawdb_readiness::slow_query::NowledgeMemSlowQueryReport>(),
        );
        assert_eq!(
            NOWLEDGE_MEM_SLOW_QUERY_REPORT_PROTOCOL,
            hawdb_readiness::slow_query::NOWLEDGE_MEM_SLOW_QUERY_REPORT_PROTOCOL,
        );
    }
}
use hawdb_search::candidate_evidence::{
    advised_compressed_vector_search_mode, effective_search_candidate_mode,
    nowledge_mem_search_candidate_report, retrieval_projection_advisor_blocker_codes,
    retrieval_projection_advisor_json,
};
pub use hawdb_search::candidate_evidence::{
    nowledge_mem_search_candidate_shadow_evidence_json, NowledgeMemRetrievalProjectionAdvisor,
    NowledgeMemSearchCandidateFieldSummary, NowledgeMemSearchCandidateFilterPushdownEvidence,
    NowledgeMemSearchCandidateOutput, NowledgeMemSearchCandidateReadinessOptions,
    NowledgeMemSearchCandidateReadinessReport, NowledgeMemSearchCandidateReport,
    NowledgeMemSearchCandidateRequest, NowledgeMemSearchCandidateShadowAccumulator,
    NowledgeMemSearchCandidateShadowEvidence, NOWLEDGE_MEM_SEARCH_CANDIDATE_EVIDENCE_ROUTE,
    NOWLEDGE_MEM_SEARCH_CANDIDATE_EVIDENCE_SOURCE, NOWLEDGE_MEM_SEARCH_CANDIDATE_PRIMARY_ENGINE,
    NOWLEDGE_MEM_SEARCH_CANDIDATE_READINESS_PROTOCOL,
    NOWLEDGE_MEM_SEARCH_CANDIDATE_REPORT_PROTOCOL, NOWLEDGE_MEM_SEARCH_CANDIDATE_SHADOW_ENGINE,
    NOWLEDGE_MEM_SEARCH_CANDIDATE_SHADOW_EVIDENCE_PROTOCOL,
    NOWLEDGE_MEM_SEARCH_CANDIDATE_TRACE_EVIDENCE_SOURCE,
    NOWLEDGE_MEM_SEARCH_CANDIDATE_TRACE_PRIMARY_ENGINE,
    NOWLEDGE_MEM_SEARCH_CANDIDATE_TRACE_SHADOW_ENGINE,
};
use std::collections::{BTreeMap, BTreeSet};
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::Instant;

#[cfg(test)]
#[path = "nowledge_mem_app_read_snapshot_tests.rs"]
mod app_read_snapshot_tests;
#[cfg(test)]
mod candidate_facade_tests;

#[cfg(test)]
mod runtime_status_facade_tests {
    use super::{
        NowledgeMemCutoverControls, NowledgeMemCutoverControlsReport, NowledgeMemProductionStatus,
        NowledgeMemReadControl, NowledgeMemRuntimeStatus, NowledgeMemWorkControl,
    };
    use std::any::TypeId;

    #[test]
    fn root_facade_preserves_runtime_status_type_identity() {
        assert_eq!(
            TypeId::of::<NowledgeMemRuntimeStatus>(),
            TypeId::of::<hawdb_readiness::nowledge_mem_runtime_status::NowledgeMemRuntimeStatus>()
        );
        assert_eq!(
            TypeId::of::<NowledgeMemProductionStatus>(),
            TypeId::of::<hawdb_readiness::nowledge_mem_runtime_status::NowledgeMemProductionStatus>(
            )
        );
        assert_eq!(
            TypeId::of::<NowledgeMemReadControl>(),
            TypeId::of::<hawdb_readiness::nowledge_mem_cutover::NowledgeMemReadControl>()
        );
        assert_eq!(
            TypeId::of::<NowledgeMemWorkControl>(),
            TypeId::of::<hawdb_readiness::nowledge_mem_cutover::NowledgeMemWorkControl>()
        );
        assert_eq!(
            TypeId::of::<NowledgeMemCutoverControls>(),
            TypeId::of::<hawdb_readiness::nowledge_mem_cutover::NowledgeMemCutoverControls>()
        );
        assert_eq!(
            TypeId::of::<NowledgeMemCutoverControlsReport>(),
            TypeId::of::<hawdb_readiness::nowledge_mem_cutover::NowledgeMemCutoverControlsReport>()
        );
    }
}

pub use hawdb_readiness::nowledge_mem_serving_path::{
    NowledgeMemServingEntrypoint, NowledgeMemServingPathReadiness,
    NOWLEDGE_MEM_SERVING_PATH_READINESS_PROTOCOL,
};

const TYPED_CONTROL_STATEMENT_MEMORY_BYTES: u64 = 1024 * 1024;
const SEARCH_PROJECTION_CHANGEFEED_OPERATION_BYTES: usize = 1024;

pub fn nowledge_mem_graph_config(mode: NowledgeMemGraphMode) -> DatabaseConfig {
    nowledge_mem_graph_config_with_search_mode(mode, CompressedVectorSearchMode::Disabled)
}

pub fn nowledge_mem_graph_config_with_search_mode(
    mode: NowledgeMemGraphMode,
    compressed_vector_search_mode: CompressedVectorSearchMode,
) -> DatabaseConfig {
    DatabaseConfig {
        read_only: matches!(mode, NowledgeMemGraphMode::ShadowReadOnly),
        compressed_vector_search_mode,
        ..DatabaseConfig::default()
    }
}

fn default_nowledge_mem_runtime_governor(
    path: &Path,
    database_config: &DatabaseConfig,
) -> RuntimeGovernor {
    let storage_device = StorageDeviceProfile::detect(path);
    let mut governor_config = RuntimeGovernorConfig::shared_host();
    if let Some(max_payload_bytes) = database_config.max_read_result_payload_bytes {
        governor_config.result_budget_bytes = u64::try_from(max_payload_bytes).unwrap_or(u64::MAX);
    }
    RuntimeGovernor::detect(
        governor_config,
        IoConcurrencyBudget::shared_host_for_device(storage_device),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowledgeMemOpenOptions {
    pub graph_path: PathBuf,
    pub search_projection_path: Option<PathBuf>,
    pub search_projection_open_mode: NowledgeMemSearchProjectionOpenMode,
    pub mode: NowledgeMemGraphMode,
    pub database_config: Option<DatabaseConfig>,
    pub compressed_vector_search_mode: CompressedVectorSearchMode,
    pub adaptive_vector_backend_policy: AdaptiveVectorBackendPolicy,
    pub retrieval_projection_advisor: NowledgeMemRetrievalProjectionAdvisor,
    pub search_range_read_config: Option<SearchRangeReadConfig>,
    pub system_schema_registries: Vec<crate::SystemSchemaRegistry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowledgeMemQualifiedOutOfCoreSearchOptions {
    pub config: SearchOutOfCoreConfig,
    pub qualification: SearchLexicalProductionQualificationReport,
    pub expected_identity: crate::ProductionQualificationIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NowledgeMemSearchProjectionOpenMode {
    FullResidencyMaintenance,
    IncrementalMaintenance(Box<NowledgeMemIncrementalSearchMaintenanceOptions>),
    QualifiedOutOfCore(Box<NowledgeMemQualifiedOutOfCoreSearchOptions>),
}

impl NowledgeMemSearchProjectionOpenMode {
    fn role(&self) -> NowledgeMemSearchProjectionRole {
        match self {
            Self::FullResidencyMaintenance => {
                NowledgeMemSearchProjectionRole::FullResidencyMaintenance
            }
            Self::IncrementalMaintenance(_) => {
                NowledgeMemSearchProjectionRole::IncrementalMaintenance
            }
            Self::QualifiedOutOfCore(_) => NowledgeMemSearchProjectionRole::QualifiedOutOfCore,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NowledgeMemSearchProjectionRole {
    FullResidencyMaintenance,
    IncrementalMaintenance,
    QualifiedOutOfCore,
}

impl NowledgeMemSearchProjectionRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FullResidencyMaintenance => "full_residency_maintenance",
            Self::IncrementalMaintenance => "incremental_maintenance",
            Self::QualifiedOutOfCore => "qualified_out_of_core",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NowledgeMemOpenDiagnosticOptions {
    pub include_local_paths: bool,
}

impl NowledgeMemOpenOptions {
    pub fn graph_only(graph_path: impl Into<PathBuf>, mode: NowledgeMemGraphMode) -> Self {
        Self {
            graph_path: graph_path.into(),
            search_projection_path: None,
            search_projection_open_mode:
                NowledgeMemSearchProjectionOpenMode::FullResidencyMaintenance,
            mode,
            database_config: None,
            compressed_vector_search_mode: CompressedVectorSearchMode::Disabled,
            adaptive_vector_backend_policy: AdaptiveVectorBackendPolicy::default(),
            retrieval_projection_advisor: NowledgeMemRetrievalProjectionAdvisor::default(),
            search_range_read_config: None,
            system_schema_registries: Vec::new(),
        }
    }

    pub fn with_search_projection(
        graph_path: impl Into<PathBuf>,
        search_projection_path: impl Into<PathBuf>,
        mode: NowledgeMemGraphMode,
    ) -> Self {
        Self {
            graph_path: graph_path.into(),
            search_projection_path: Some(search_projection_path.into()),
            search_projection_open_mode:
                NowledgeMemSearchProjectionOpenMode::FullResidencyMaintenance,
            mode,
            database_config: None,
            compressed_vector_search_mode: CompressedVectorSearchMode::Disabled,
            adaptive_vector_backend_policy: AdaptiveVectorBackendPolicy::default(),
            retrieval_projection_advisor: NowledgeMemRetrievalProjectionAdvisor::default(),
            search_range_read_config: None,
            system_schema_registries: Vec::new(),
        }
    }

    pub fn with_qualified_out_of_core_search_projection(
        graph_path: impl Into<PathBuf>,
        search_projection_path: impl Into<PathBuf>,
        mode: NowledgeMemGraphMode,
        qualified: NowledgeMemQualifiedOutOfCoreSearchOptions,
    ) -> Self {
        Self {
            graph_path: graph_path.into(),
            search_projection_path: Some(search_projection_path.into()),
            search_projection_open_mode: NowledgeMemSearchProjectionOpenMode::QualifiedOutOfCore(
                Box::new(qualified),
            ),
            mode,
            database_config: None,
            compressed_vector_search_mode: CompressedVectorSearchMode::Disabled,
            adaptive_vector_backend_policy: AdaptiveVectorBackendPolicy::default(),
            retrieval_projection_advisor: NowledgeMemRetrievalProjectionAdvisor::default(),
            search_range_read_config: None,
            system_schema_registries: Vec::new(),
        }
    }

    pub fn with_compressed_vector_search_mode(mut self, mode: CompressedVectorSearchMode) -> Self {
        self.compressed_vector_search_mode = mode;
        self
    }

    /// Overrides resource and storage policy while retaining facade-owned mode
    /// and vector-backend settings.
    pub fn with_database_config(mut self, config: DatabaseConfig) -> Self {
        self.database_config = Some(config);
        self
    }

    /// Registers application-owned system schemas that must be current before
    /// the embedded store is returned to its host.
    pub fn with_system_schema_registry(mut self, registry: crate::SystemSchemaRegistry) -> Self {
        self.system_schema_registries.push(registry);
        self
    }

    pub fn with_adaptive_vector_backend_policy(
        mut self,
        policy: AdaptiveVectorBackendPolicy,
    ) -> Self {
        self.adaptive_vector_backend_policy = policy;
        self
    }

    pub fn with_retrieval_projection_advisor(
        mut self,
        advisor: NowledgeMemRetrievalProjectionAdvisor,
    ) -> Self {
        self.retrieval_projection_advisor = advisor;
        self
    }

    pub fn with_search_range_read_config(mut self, config: SearchRangeReadConfig) -> Self {
        self.search_range_read_config = Some(config);
        self
    }

    fn effective_compressed_vector_search_mode(&self) -> CompressedVectorSearchMode {
        advised_compressed_vector_search_mode(
            self.compressed_vector_search_mode,
            &self.retrieval_projection_advisor,
        )
    }

    fn effective_database_config(&self) -> DatabaseConfig {
        let mut config = self.database_config.clone().unwrap_or_default();
        config.read_only = matches!(self.mode, NowledgeMemGraphMode::ShadowReadOnly);
        config.compressed_vector_search_mode = self.effective_compressed_vector_search_mode();
        config.adaptive_vector_backend_policy = self.adaptive_vector_backend_policy;
        config
    }

    fn validate(&self) -> Result<()> {
        match (
            self.search_projection_path.as_ref(),
            &self.search_projection_open_mode,
        ) {
            (None, NowledgeMemSearchProjectionOpenMode::IncrementalMaintenance(_)) => {
                return Err(HawDBError::Semantic(
                    "incremental search maintenance requires search_projection_path".into(),
                ));
            }
            (Some(_), NowledgeMemSearchProjectionOpenMode::IncrementalMaintenance(options)) => {
                options.validate()?;
                if self.search_range_read_config.is_some() {
                    return Err(HawDBError::Semantic(
                        "incremental search maintenance uses its typed reader configuration".into(),
                    ));
                }
            }
            (None, NowledgeMemSearchProjectionOpenMode::QualifiedOutOfCore(_)) => {
                return Err(HawDBError::Semantic(
                    "qualified out-of-core search requires search_projection_path".to_string(),
                ));
            }
            (Some(_), NowledgeMemSearchProjectionOpenMode::QualifiedOutOfCore(qualified)) => {
                qualified.expected_identity.validate()?;
                if self.search_range_read_config.is_some() {
                    return Err(HawDBError::Semantic(
                        "search_range_read_config applies only to the full-residency maintenance projection"
                            .to_string(),
                    ));
                }
            }
            _ => {}
        }
        if self.search_projection_path.is_none() && self.search_range_read_config.is_some() {
            return Err(HawDBError::Semantic(
                "search_range_read_config requires search_projection_path".to_string(),
            ));
        }
        let mut schema_owners = BTreeSet::new();
        for registry in &self.system_schema_registries {
            if !schema_owners.insert(registry.owner()) {
                return Err(HawDBError::Semantic(format!(
                    "system schema owner {} is registered more than once",
                    registry.owner()
                )));
            }
        }
        Ok(())
    }

    pub fn sanitized_report(&self) -> NowledgeMemOpenReport {
        let effective_compressed_vector_search_mode =
            self.effective_compressed_vector_search_mode();
        NowledgeMemOpenReport {
            protocol: NOWLEDGE_MEM_OPEN_REPORT_PROTOCOL.to_string(),
            mode: self.mode,
            graph_configured: true,
            search_projection_configured: self.search_projection_path.is_some(),
            search_projection_role: self
                .search_projection_path
                .as_ref()
                .map(|_| self.search_projection_open_mode.role()),
            compressed_vector_search_mode: effective_compressed_vector_search_mode,
            requested_compressed_vector_search_mode: self.compressed_vector_search_mode,
            adaptive_vector_backend_policy: self.adaptive_vector_backend_policy,
            retrieval_projection_advisor: self.retrieval_projection_advisor.clone(),
            retrieval_projection_advisor_blocker_codes: if self.compressed_vector_search_mode
                == effective_compressed_vector_search_mode
            {
                Vec::new()
            } else {
                retrieval_projection_advisor_blocker_codes(&self.retrieval_projection_advisor)
            },
            graph_opened: false,
            search_projection_opened: false,
            search_production_qualification_bound: false,
            system_schema_upgrades: Vec::new(),
        }
    }

    pub fn diagnostic_report_json(
        &self,
        options: NowledgeMemOpenDiagnosticOptions,
    ) -> serde_json::Value {
        let mut report = self.sanitized_report().json();
        if let Some(object) = report.as_object_mut() {
            object.insert(
                "debug_local_paths_included".to_string(),
                serde_json::Value::Bool(options.include_local_paths),
            );
            object.insert(
                "local_paths_redacted".to_string(),
                serde_json::Value::Bool(!options.include_local_paths),
            );
            if options.include_local_paths {
                object.insert(
                    "graph_path".to_string(),
                    serde_json::Value::String(self.graph_path.to_string_lossy().into_owned()),
                );
                object.insert(
                    "search_projection_path".to_string(),
                    self.search_projection_path
                        .as_ref()
                        .map(|path| serde_json::Value::String(path.to_string_lossy().into_owned()))
                        .unwrap_or(serde_json::Value::Null),
                );
            }
        }
        report
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowledgeMemOpenReport {
    pub protocol: String,
    pub mode: NowledgeMemGraphMode,
    pub graph_configured: bool,
    pub search_projection_configured: bool,
    pub search_projection_role: Option<NowledgeMemSearchProjectionRole>,
    pub compressed_vector_search_mode: CompressedVectorSearchMode,
    pub requested_compressed_vector_search_mode: CompressedVectorSearchMode,
    pub adaptive_vector_backend_policy: AdaptiveVectorBackendPolicy,
    pub retrieval_projection_advisor: NowledgeMemRetrievalProjectionAdvisor,
    pub retrieval_projection_advisor_blocker_codes: Vec<String>,
    pub graph_opened: bool,
    pub search_projection_opened: bool,
    pub search_production_qualification_bound: bool,
    pub system_schema_upgrades: Vec<crate::SystemSchemaUpgradeReport>,
}

impl NowledgeMemOpenReport {
    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "protocol": self.protocol,
            "mode": self.mode.as_str(),
            "graph_configured": self.graph_configured,
            "search_projection_configured": self.search_projection_configured,
            "search_projection_role": self.search_projection_role.map(NowledgeMemSearchProjectionRole::as_str),
            "compressed_vector_search_mode": self.compressed_vector_search_mode.as_str(),
            "requested_compressed_vector_search_mode": self.requested_compressed_vector_search_mode.as_str(),
            "adaptive_vector_backend_policy": {
                "flat_scan_max_documents": self.adaptive_vector_backend_policy.flat_scan_max_documents,
                "high_filter_selectivity_per_million": self.adaptive_vector_backend_policy.high_filter_selectivity_per_million,
                "flat_scan_memory_budget_bytes": self.adaptive_vector_backend_policy.flat_scan_memory_budget_bytes,
            },
            "retrieval_projection_advisor": retrieval_projection_advisor_json(&self.retrieval_projection_advisor),
            "retrieval_projection_advisor_blocker_codes": self.retrieval_projection_advisor_blocker_codes,
            "graph_opened": self.graph_opened,
            "search_projection_opened": self.search_projection_opened,
            "search_production_qualification_bound": self.search_production_qualification_bound,
            "system_schema_upgrades": self.system_schema_upgrades.iter().map(|upgrade| serde_json::json!({
                "owner": upgrade.owner,
                "previous_version": upgrade.previous_version,
                "current_version": upgrade.current_version,
                "applied_versions": upgrade.applied_versions,
                "commit_epoch_before": upgrade.commit_epoch_before,
                "commit_epoch_after": upgrade.commit_epoch_after,
            })).collect::<Vec<_>>(),
        })
    }
}

pub use hawdb_readiness::nowledge_mem_runtime_status::{
    NowledgeMemProductionStatus, NowledgeMemRuntimeStatus, NOWLEDGE_MEM_PRODUCTION_STATUS_PROTOCOL,
    NOWLEDGE_MEM_RUNTIME_STATUS_PROTOCOL,
};

pub use hawdb_readiness::nowledge_mem_cutover::{
    NowledgeMemCutoverControls, NowledgeMemCutoverControlsReport, NowledgeMemReadControl,
    NowledgeMemWorkControl,
};

pub use hawdb_nowledge_contracts::{
    NowledgeMemReadSnapshotBudget, NowledgeMemReadSnapshotReport,
    NowledgeMemStorageLifecycleActionKind, NowledgeMemStorageLifecycleDecision,
    NowledgeMemStorageRecoveryReport, NOWLEDGE_MEM_READ_SNAPSHOT_REPORT_PROTOCOL,
    NOWLEDGE_MEM_STORAGE_LIFECYCLE_DECISION_PROTOCOL,
};

#[cfg(test)]
mod storage_lifecycle_facade_tests {
    use super::*;
    use std::any::TypeId;

    #[test]
    fn facade_reexports_storage_lifecycle_contracts_without_conversion() {
        assert_eq!(
            TypeId::of::<NowledgeMemStorageRecoveryReport>(),
            TypeId::of::<hawdb_nowledge_contracts::NowledgeMemStorageRecoveryReport>(),
        );
        assert_eq!(
            TypeId::of::<NowledgeMemStorageLifecycleDecision>(),
            TypeId::of::<hawdb_nowledge_contracts::NowledgeMemStorageLifecycleDecision>(),
        );
        assert_eq!(
            TypeId::of::<NowledgeMemStorageLifecycleActionKind>(),
            TypeId::of::<hawdb_nowledge_contracts::NowledgeMemStorageLifecycleActionKind>(),
        );
    }

    #[test]
    fn facade_reexports_read_snapshot_contracts_without_conversion() {
        assert_eq!(
            TypeId::of::<NowledgeMemReadSnapshotBudget>(),
            TypeId::of::<hawdb_nowledge_contracts::NowledgeMemReadSnapshotBudget>(),
        );
        assert_eq!(
            TypeId::of::<NowledgeMemReadSnapshotReport>(),
            TypeId::of::<hawdb_nowledge_contracts::NowledgeMemReadSnapshotReport>(),
        );
        assert_eq!(
            NOWLEDGE_MEM_READ_SNAPSHOT_REPORT_PROTOCOL,
            hawdb_nowledge_contracts::NOWLEDGE_MEM_READ_SNAPSHOT_REPORT_PROTOCOL,
        );
    }
}

pub use hawdb_readiness::source_mutation::{
    nowledge_mem_source_mutation_dual_write_evidence_all_ready,
    nowledge_mem_source_mutation_dual_write_readiness,
    nowledge_mem_source_mutation_family_requirements, NowledgeMemSourceMutationDualWriteEvidence,
    NowledgeMemSourceMutationDualWriteReadinessReport, NowledgeMemSourceMutationFamilyRequirement,
    NOWLEDGE_MEM_SOURCE_MUTATION_DUAL_WRITE_READINESS_PROTOCOL,
    NOWLEDGE_MEM_SOURCE_MUTATION_FAMILY_CONTENT_REFRESH_REPARSE,
    NOWLEDGE_MEM_SOURCE_MUTATION_FAMILY_GRAPH_DELETE,
    NOWLEDGE_MEM_SOURCE_MUTATION_FAMILY_INDEXED_TRANSITION,
    NOWLEDGE_MEM_SOURCE_MUTATION_FAMILY_INGEST_CREATE,
    NOWLEDGE_MEM_SOURCE_MUTATION_FAMILY_LIFECYCLE,
    NOWLEDGE_MEM_SOURCE_MUTATION_FAMILY_PATCH_DELETE,
    NOWLEDGE_MEM_SOURCE_MUTATION_FAMILY_REVISION_EDGES,
    NOWLEDGE_MEM_SOURCE_MUTATION_FAMILY_SEARCH_PROJECTION_EFFECTS,
    REQUIRED_NOWLEDGE_MEM_SOURCE_MUTATION_FAMILIES,
};

pub const NOWLEDGE_MEM_OPEN_REPORT_PROTOCOL: &str = "hawdb-nowledge-mem-open-report";
pub use hawdb_readiness::previous_wrapper_preflight::{
    NOWLEDGE_MEM_CUTOVER_CONTROLS_PROTOCOL, NOWLEDGE_MEM_LIBRARY_READINESS_PROTOCOL,
    NOWLEDGE_MEM_OPERATIONS_READINESS_PROTOCOL, NOWLEDGE_QUERY_RUNTIME_PREFLIGHT_PROTOCOL,
};
pub const NOWLEDGE_MEM_RETRIEVAL_REPORT_PROTOCOL: &str = "hawdb-nowledge-mem-retrieval-report";
pub const NOWLEDGE_MEM_READINESS_DASHBOARD_PROTOCOL: &str =
    "hawdb-nowledge-mem-readiness-dashboard-v1";
pub use hawdb_route_ownership::graph::{
    nowledge_mem_graph_read_route_catalog_digest, nowledge_mem_graph_read_route_spec,
    nowledge_mem_graph_read_route_spec_json, nowledge_mem_graph_read_route_specs_json,
    nowledge_mem_required_query_families_for_route, NowledgeMemGraphReadRouteEvidenceKind,
    NowledgeMemGraphReadRouteOwner, NowledgeMemGraphReadRouteSpec,
    NOWLEDGE_MEM_GRAPH_READ_ROUTE_CATALOG_VERSION, NOWLEDGE_MEM_GRAPH_READ_ROUTE_SPECS,
    NOWLEDGE_MEM_SEARCH_ROUTE, REQUIRED_NOWLEDGE_MEM_BOUNDED_READ_ROUTES,
};

pub const DEFAULT_NOWLEDGE_MEM_READ_MAX_ROWS: usize = 512;
pub const DEFAULT_NOWLEDGE_MEM_READ_MAX_ESTIMATED_PAYLOAD_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug)]
pub struct NowledgeMemGraph {
    db: Database,
    mode: NowledgeMemGraphMode,
    runtime_governor: RuntimeGovernor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowledgeMemReadOptions {
    pub max_rows: Option<usize>,
    pub max_estimated_payload_bytes: Option<usize>,
}

impl Default for NowledgeMemReadOptions {
    fn default() -> Self {
        Self {
            max_rows: Some(DEFAULT_NOWLEDGE_MEM_READ_MAX_ROWS),
            max_estimated_payload_bytes: Some(
                DEFAULT_NOWLEDGE_MEM_READ_MAX_ESTIMATED_PAYLOAD_BYTES,
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowledgeMemReadOutput {
    pub output: QueryOutput,
    pub report: NowledgeMemReadReport,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NowledgeMemReadinessOptions {
    pub bounded_read_probe: Option<NowledgeGraphStatement>,
    pub bounded_read_evidence: Option<serde_json::Value>,
    pub covered_routes: Vec<String>,
    pub graph_route_readiness: Option<NowledgeMemRouteReadinessSummary>,
    pub search_route_ownership: Option<NowledgeMemSearchRouteOwnershipReadinessReport>,
    pub active_search_route_ownership: Option<NowledgeMemActiveSearchRouteOwnershipReadinessReport>,
    pub active_search_route_readiness: Option<NowledgeMemActiveSearchRouteReadinessReport>,
    pub replacement_readiness_by_query_family: Option<serde_json::Value>,
    pub read_options: NowledgeMemReadOptions,
    pub search_projection_evidence: Option<serde_json::Value>,
    pub search_projection_probe_options: SearchProjectionProbeOptions,
    pub primary_search_projection_probe: Option<serde_json::Value>,
    pub search_projection_shadow_evidence: Option<serde_json::Value>,
    pub search_candidate_shadow_evidence: Option<serde_json::Value>,
    pub workload_fixture_evidence: Option<NowledgeGraphRouteWorkloadFixtureReport>,
    pub production_resource_profile: Option<StorageResourceProfileReport>,
    pub qos_policy: LocalQosPolicy,
    pub qos_state: LocalQosState,
    pub background_maintenance_options: BackgroundMaintenanceOptions,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NowledgeMemLibraryReadinessReport {
    pub protocol: String,
    pub present: bool,
    pub ready: bool,
    pub mode: NowledgeMemGraphMode,
    pub redaction: NowledgeMemReadinessRedactionSummary,
    pub production_path: NowledgeMemLibraryProductionPathSummary,
    pub blocker_codes: Vec<String>,
    pub readiness_by_area: NowledgeMemReadinessAreaMap,
    pub ready_area_count: usize,
    pub blocked_area_count: usize,
    pub graph_open: bool,
    pub graph_read_only: bool,
    pub graph_route_readiness: serde_json::Value,
    pub search_route_ownership: serde_json::Value,
    pub active_search_route_ownership: serde_json::Value,
    pub active_search_route_readiness: serde_json::Value,
    pub bounded_read_evidence: serde_json::Value,
    pub storage_recovery: serde_json::Value,
    pub background_maintenance: serde_json::Value,
    pub query_family_evidence: serde_json::Value,
    pub search_projection_evidence: serde_json::Value,
    pub search_projection_shadow_evidence: serde_json::Value,
    pub search_candidate_shadow_evidence: serde_json::Value,
    pub workload_fixture_evidence: serde_json::Value,
    pub production_resource_profile: serde_json::Value,
}

impl NowledgeMemLibraryReadinessReport {
    pub fn areas(&self) -> Vec<NowledgeMemReadinessAreaSummary> {
        self.readiness_by_area.areas()
    }

    pub fn json(&self) -> serde_json::Value {
        let areas = self.areas();
        serde_json::json!({
            "protocol": self.protocol,
            "present": self.present,
            "ready": self.ready,
            "mode": self.mode.as_str(),
            "redaction": self.redaction.json(),
            "production_path": self.production_path.json(),
            "blocker_codes": self.blocker_codes,
            "readiness_by_area": self.readiness_by_area.json(),
            "areas": areas.iter().map(NowledgeMemReadinessAreaSummary::json).collect::<Vec<_>>(),
            "ready_area_count": self.ready_area_count,
            "blocked_area_count": self.blocked_area_count,
            "graph": {
                "open": self.graph_open,
                "mode": self.mode.as_str(),
                "read_only": self.graph_read_only,
            },
            "graph_route_readiness": self.graph_route_readiness,
            "search_route_ownership": self.search_route_ownership,
            "active_search_route_ownership": self.active_search_route_ownership,
            "active_search_route_readiness": self.active_search_route_readiness,
            "bounded_read_evidence": self.bounded_read_evidence,
            "storage_recovery": self.storage_recovery,
            "background_maintenance": self.background_maintenance,
            "query_family_evidence": self.query_family_evidence,
            "search_projection_evidence": self.search_projection_evidence,
            "search_projection_shadow_evidence": self.search_projection_shadow_evidence,
            "search_candidate_shadow_evidence": self.search_candidate_shadow_evidence,
            "workload_fixture_evidence": self.workload_fixture_evidence,
            "production_resource_profile": self.production_resource_profile,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NowledgeMemLibraryProductionPathSummary {
    pub in_process: bool,
    pub cli_required: bool,
    pub env_control_plane_required: bool,
    pub spawned_helper_required: bool,
}

impl Default for NowledgeMemLibraryProductionPathSummary {
    fn default() -> Self {
        Self {
            in_process: true,
            cli_required: false,
            env_control_plane_required: false,
            spawned_helper_required: false,
        }
    }
}

impl NowledgeMemLibraryProductionPathSummary {
    pub fn ready(&self) -> bool {
        self.in_process
            && !self.cli_required
            && !self.env_control_plane_required
            && !self.spawned_helper_required
    }

    fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "ready": self.ready(),
            "in_process": self.in_process,
            "cli_required": self.cli_required,
            "env_control_plane_required": self.env_control_plane_required,
            "spawned_helper_required": self.spawned_helper_required,
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NowledgeMemReadinessRedactionSummary {
    pub query_text_copied: bool,
    pub parameters_copied: bool,
    pub local_paths_copied: bool,
}

impl NowledgeMemReadinessRedactionSummary {
    pub fn ready(&self) -> bool {
        !self.query_text_copied && !self.parameters_copied && !self.local_paths_copied
    }

    fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "ready": self.ready(),
            "query_text_copied": self.query_text_copied,
            "parameters_copied": self.parameters_copied,
            "local_paths_copied": self.local_paths_copied,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowledgeMemReadinessDashboard {
    pub protocol: String,
    pub ready: bool,
    pub mode: NowledgeMemGraphMode,
    pub area_count: usize,
    pub ready_area_count: usize,
    pub blocked_area_count: usize,
    pub blocker_codes: Vec<String>,
    pub areas: Vec<NowledgeMemReadinessAreaSummary>,
    pub storage_lifecycle_action: NowledgeMemStorageLifecycleActionKind,
    pub storage_lifecycle_ready: bool,
    pub slow_query_ready: bool,
    pub slow_query_record_count: usize,
}

impl NowledgeMemReadinessDashboard {
    fn from_reports(
        library: &NowledgeMemLibraryReadinessReport,
        storage_lifecycle: &NowledgeMemStorageLifecycleDecision,
        slow_query: &NowledgeMemSlowQueryReport,
    ) -> Self {
        let areas = nowledge_mem_readiness_dashboard_areas(library, slow_query);
        let blocked_area_count = areas.iter().filter(|area| !area.ready).count();
        let ready_area_count = areas.len().saturating_sub(blocked_area_count);

        Self {
            protocol: NOWLEDGE_MEM_READINESS_DASHBOARD_PROTOCOL.to_string(),
            ready: library.ready && slow_query.ready,
            mode: library.mode,
            area_count: areas.len(),
            ready_area_count,
            blocked_area_count,
            blocker_codes: library.blocker_codes.clone(),
            areas,
            storage_lifecycle_action: storage_lifecycle.action,
            storage_lifecycle_ready: storage_lifecycle.ready_for_mem_lifecycle,
            slow_query_ready: slow_query.ready,
            slow_query_record_count: slow_query.record_count,
        }
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "protocol": self.protocol,
            "ready": self.ready,
            "mode": self.mode.as_str(),
            "area_count": self.area_count,
            "ready_area_count": self.ready_area_count,
            "blocked_area_count": self.blocked_area_count,
            "blocker_codes": self.blocker_codes,
            "areas": self.areas.iter().map(NowledgeMemReadinessAreaSummary::json).collect::<Vec<_>>(),
            "storage_lifecycle": {
                "ready": self.storage_lifecycle_ready,
                "action": self.storage_lifecycle_action.as_str(),
            },
            "slow_query": {
                "ready": self.slow_query_ready,
                "record_count": self.slow_query_record_count,
            },
            "redaction": {
                "query_text_copied": false,
                "parameters_copied": false,
                "local_paths_copied": false
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NowledgeMemOperationsReadinessReport {
    pub protocol: String,
    pub present: bool,
    pub ready: bool,
    pub mode: NowledgeMemGraphMode,
    pub graph_open: bool,
    pub graph_read_only: bool,
    pub search_projection_open: bool,
    pub graph_commit_epoch: u64,
    pub projection_commit_lag: u64,
    pub projection_stale: bool,
    pub storage_lifecycle_action: NowledgeMemStorageLifecycleActionKind,
    pub storage_lifecycle_ready: bool,
    pub storage_recovery_ready: bool,
    pub slow_query_ready: bool,
    pub background_maintenance_ready: bool,
    pub blocker_codes: Vec<String>,
    pub runtime_status: NowledgeMemRuntimeStatus,
    pub storage_lifecycle_decision: NowledgeMemStorageLifecycleDecision,
    pub storage_recovery: NowledgeMemStorageRecoveryReport,
    pub slow_query: NowledgeMemSlowQueryReport,
    pub background_maintenance: NowledgeMemBackgroundMaintenanceReport,
}

impl NowledgeMemOperationsReadinessReport {
    fn from_reports(
        mode: NowledgeMemGraphMode,
        graph_read_only: bool,
        runtime_status: NowledgeMemRuntimeStatus,
        storage_recovery: NowledgeMemStorageRecoveryReport,
        slow_query: NowledgeMemSlowQueryReport,
        background_maintenance: NowledgeMemBackgroundMaintenanceReport,
    ) -> Self {
        let search_projection_open = runtime_status.projection_freshness.is_some();
        let projection_commit_lag = runtime_status.projection_commit_lag();
        let projection_stale = search_projection_open && runtime_status.projection_stale();
        let storage_lifecycle_decision =
            NowledgeMemStorageLifecycleDecision::from_storage_recovery(storage_recovery.clone());
        let mut blocker_codes = Vec::new();
        if !storage_lifecycle_decision.ready_for_mem_lifecycle {
            blocker_codes.push("storage_recovery_not_ready".to_string());
            blocker_codes.extend(
                storage_lifecycle_decision
                    .blocker_codes
                    .iter()
                    .map(|code| format!("storage_lifecycle.{code}")),
            );
        }
        if !slow_query.ready {
            blocker_codes.push("slow_query_report_not_ready".to_string());
        }
        if !background_maintenance.ready {
            blocker_codes.push("background_maintenance_not_ready".to_string());
        }
        if search_projection_open && projection_stale {
            blocker_codes.push("search_projection_stale".to_string());
        }

        Self {
            protocol: NOWLEDGE_MEM_OPERATIONS_READINESS_PROTOCOL.to_string(),
            present: true,
            ready: blocker_codes.is_empty(),
            mode,
            graph_open: true,
            graph_read_only,
            search_projection_open,
            graph_commit_epoch: runtime_status.graph_commit_epoch,
            projection_commit_lag,
            projection_stale,
            storage_lifecycle_action: storage_lifecycle_decision.action,
            storage_lifecycle_ready: storage_lifecycle_decision.ready_for_mem_lifecycle,
            storage_recovery_ready: storage_recovery.ready,
            slow_query_ready: slow_query.ready,
            background_maintenance_ready: background_maintenance.ready,
            blocker_codes,
            runtime_status,
            storage_lifecycle_decision,
            storage_recovery,
            slow_query,
            background_maintenance,
        }
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "protocol": self.protocol,
            "present": self.present,
            "ready": self.ready,
            "mode": self.mode.as_str(),
            "graph": {
                "open": self.graph_open,
                "read_only": self.graph_read_only,
                "commit_epoch": self.graph_commit_epoch,
            },
            "search_projection": {
                "open": self.search_projection_open,
                "commit_lag": self.projection_commit_lag,
                "stale": self.projection_stale,
            },
            "storage_lifecycle": {
                "ready": self.storage_lifecycle_ready,
                "action": self.storage_lifecycle_action.as_str(),
            },
            "readiness": {
                "storage_lifecycle_ready": self.storage_lifecycle_ready,
                "storage_recovery_ready": self.storage_recovery_ready,
                "slow_query_ready": self.slow_query_ready,
                "background_maintenance_ready": self.background_maintenance_ready,
            },
            "blocker_codes": self.blocker_codes,
            "runtime_status": self.runtime_status.json(),
            "storage_lifecycle_decision": self.storage_lifecycle_decision.json(),
            "storage_recovery": self.storage_recovery.json(),
            "slow_query": self.slow_query.json(),
            "background_maintenance": self.background_maintenance.json(),
            "redaction": {
                "query_text_copied": false,
                "parameters_copied": false,
                "local_paths_copied": false,
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NowledgeMemBackgroundMaintenanceReport {
    pub protocol: String,
    pub present: bool,
    pub ready: bool,
    pub total_candidates: usize,
    pub admitted_count: usize,
    pub deferred_count: usize,
    pub rejected_count: usize,
    pub total_estimated_operations: usize,
    pub admitted_estimated_operations: usize,
    pub deferred_estimated_operations: usize,
    pub rejected_estimated_operations: usize,
    pub executable_search_projection_graph_delta_count: usize,
    pub admitted_search_projection_graph_delta_count: usize,
    pub deferred_search_projection_graph_delta_count: usize,
    pub rejected_search_projection_graph_delta_count: usize,
    pub executable_search_projection_graph_delta_operations: usize,
    pub admitted_search_projection_graph_delta_operations: usize,
    pub max_search_projection_graph_delta_complete_through_graph_commit_epoch: Option<u64>,
    pub foreground_admission_probe_ready: Option<bool>,
    pub foreground_admission_probe_admission_name: Option<String>,
    pub memory_pressure_ready: Option<bool>,
    pub memory_budget_bytes: Option<u64>,
    pub estimated_memory_bytes: Option<u64>,
    pub slow_query_ready: Option<bool>,
    pub slow_query_record_count: Option<u64>,
    pub slow_query_capacity: Option<u64>,
    pub slow_query_redaction_ready: Option<bool>,
    pub top_admitted_kind: Option<BackgroundMaintenanceKind>,
    pub top_admitted_name: Option<String>,
    pub ranked_count: u64,
    pub foreground_ranked_count: u64,
    pub unknown_admission_count: u64,
    pub blocker_codes: Vec<String>,
    summary: serde_json::Value,
}

impl NowledgeMemBackgroundMaintenanceReport {
    pub fn from_summary(summary: &BackgroundMaintenanceSummary) -> Self {
        Self::from_summary_with_slow_query(summary, None)
    }

    fn from_summary_with_slow_query(
        summary: &BackgroundMaintenanceSummary,
        slow_query: Option<&NowledgeMemSlowQueryReport>,
    ) -> Self {
        let mut json = background_maintenance_summary_to_json(summary);
        if let Some(object) = json.as_object_mut() {
            object.insert(
                "protocol".to_string(),
                serde_json::Value::String("hawdb-background-maintenance-report".to_string()),
            );
            if let Some(slow_query) = slow_query {
                object.insert(
                    "slow_query".to_string(),
                    background_maintenance_slow_query_json(slow_query),
                );
                object.insert(
                    "slow_query_ready".to_string(),
                    serde_json::Value::Bool(slow_query.ready),
                );
                object.insert(
                    "slow_query_record_count".to_string(),
                    serde_json::json!(slow_query.record_count),
                );
                object.insert(
                    "slow_query_capacity".to_string(),
                    serde_json::json!(slow_query.capacity),
                );
            }
        }
        let health = background_maintenance_evidence_health(Some(&json), true);

        Self {
            protocol: "hawdb-background-maintenance-report".to_string(),
            present: true,
            ready: health.ready,
            total_candidates: summary.total_candidates,
            admitted_count: summary.admitted_count,
            deferred_count: summary.deferred_count,
            rejected_count: summary.rejected_count,
            total_estimated_operations: summary.total_estimated_operations,
            admitted_estimated_operations: summary.admitted_estimated_operations,
            deferred_estimated_operations: summary.deferred_estimated_operations,
            rejected_estimated_operations: summary.rejected_estimated_operations,
            executable_search_projection_graph_delta_count: summary
                .executable_search_projection_graph_delta_count,
            admitted_search_projection_graph_delta_count: summary
                .admitted_search_projection_graph_delta_count,
            deferred_search_projection_graph_delta_count: summary
                .deferred_search_projection_graph_delta_count,
            rejected_search_projection_graph_delta_count: summary
                .rejected_search_projection_graph_delta_count,
            executable_search_projection_graph_delta_operations: summary
                .executable_search_projection_graph_delta_operations,
            admitted_search_projection_graph_delta_operations: summary
                .admitted_search_projection_graph_delta_operations,
            max_search_projection_graph_delta_complete_through_graph_commit_epoch: summary
                .max_search_projection_graph_delta_complete_through_graph_commit_epoch,
            foreground_admission_probe_ready: health.foreground_admission_probe_ready,
            foreground_admission_probe_admission_name: health
                .foreground_admission_probe_admission_name,
            memory_pressure_ready: health.memory_pressure_ready,
            memory_budget_bytes: health.memory_budget_bytes,
            estimated_memory_bytes: health.estimated_memory_bytes,
            slow_query_ready: health.slow_query_ready,
            slow_query_record_count: health.slow_query_record_count,
            slow_query_capacity: health.slow_query_capacity,
            slow_query_redaction_ready: health.slow_query_redaction_ready,
            top_admitted_kind: summary.top_admitted_kind,
            top_admitted_name: summary.top_admitted_name.clone(),
            ranked_count: health.ranked_count.unwrap_or_default(),
            foreground_ranked_count: health.foreground_ranked_count,
            unknown_admission_count: health.unknown_admission_count,
            blocker_codes: health.blocker_codes,
            summary: json,
        }
    }

    pub fn json(&self) -> serde_json::Value {
        self.summary.clone()
    }
}

fn background_maintenance_slow_query_json(
    slow_query: &NowledgeMemSlowQueryReport,
) -> serde_json::Value {
    serde_json::json!({
        "protocol": slow_query.protocol,
        "ready": slow_query.ready,
        "capacity": slow_query.capacity,
        "record_count": slow_query.record_count,
        "latest_sequence": slow_query.latest_sequence,
        "max_elapsed_micros": slow_query.max_elapsed_micros,
        "redaction": {
            "query_text_copied": false,
            "parameters_copied": false,
            "local_paths_copied": false,
        },
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowledgeMemRetrievalReport {
    pub protocol: String,
    pub mode: NowledgeMemGraphMode,
    pub compressed_vector_search_mode: CompressedVectorSearchMode,
    pub graph_commit_epoch: u64,
    pub projection_source_graph_commit_epoch: Option<u64>,
    pub projection_commit_lag: u64,
    pub projection_stale: bool,
    pub search_document_count: usize,
    pub search_filtered_document_count: usize,
    pub search_total_hits: usize,
    pub candidate_count: usize,
    pub candidate_total_count: usize,
    pub evidence_count: usize,
    pub graph_seed_count: usize,
    pub graph_context_path_count: usize,
    pub search_backend: Option<String>,
    pub vector_backend: Option<String>,
    pub text_backend: Option<String>,
    pub search_fallback_reason_codes: Vec<String>,
    pub retriever_fallback_reason_codes: Vec<String>,
    pub knowledge_fallback_reason_codes: Vec<String>,
    pub truncation_reason_codes: Vec<String>,
    pub warning_count: usize,
    pub warnings: Vec<String>,
}

impl NowledgeMemRetrievalReport {
    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "protocol": self.protocol,
            "mode": self.mode.as_str(),
            "compressed_vector_search_mode": self.compressed_vector_search_mode.as_str(),
            "graph_commit_epoch": self.graph_commit_epoch,
            "projection_source_graph_commit_epoch": self.projection_source_graph_commit_epoch,
            "projection_commit_lag": self.projection_commit_lag,
            "projection_stale": self.projection_stale,
            "search_document_count": self.search_document_count,
            "search_filtered_document_count": self.search_filtered_document_count,
            "search_total_hits": self.search_total_hits,
            "candidate_count": self.candidate_count,
            "candidate_total_count": self.candidate_total_count,
            "evidence_count": self.evidence_count,
            "graph_seed_count": self.graph_seed_count,
            "graph_context_path_count": self.graph_context_path_count,
            "search_backend": self.search_backend,
            "vector_backend": self.vector_backend,
            "text_backend": self.text_backend,
            "search_fallback_reason_codes": self.search_fallback_reason_codes,
            "retriever_fallback_reason_codes": self.retriever_fallback_reason_codes,
            "knowledge_fallback_reason_codes": self.knowledge_fallback_reason_codes,
            "truncation_reason_codes": self.truncation_reason_codes,
            "warning_count": self.warning_count,
            "warnings": self.warnings,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NowledgeMemRetrievalOutput {
    pub output: KnowledgeRetrievalOutput,
    pub report: NowledgeMemRetrievalReport,
    pub out_of_core_search_metrics: Option<SearchOutOfCoreMetrics>,
}

impl NowledgeMemGraph {
    pub fn open(path: impl AsRef<Path>, mode: NowledgeMemGraphMode) -> Result<Self> {
        let path = path.as_ref();
        Self::open_with_config(path, nowledge_mem_graph_config(mode))
    }

    pub fn open_with_config(path: impl AsRef<Path>, config: DatabaseConfig) -> Result<Self> {
        let path = path.as_ref();
        let runtime_governor = default_nowledge_mem_runtime_governor(path, &config);
        Self::open_with_config_and_runtime_governor(path, config, runtime_governor)
    }

    pub fn open_with_config_and_runtime_governor(
        path: impl AsRef<Path>,
        config: DatabaseConfig,
        runtime_governor: RuntimeGovernor,
    ) -> Result<Self> {
        let mode = if config.read_only {
            NowledgeMemGraphMode::ShadowReadOnly
        } else {
            NowledgeMemGraphMode::WritableCutover
        };
        let mut db = Database::open_with_config(path, config)?;
        db.set_runtime_governor(runtime_governor.clone());
        Ok(Self {
            db,
            mode,
            runtime_governor,
        })
    }

    pub fn from_database(db: Database, mode: NowledgeMemGraphMode) -> Self {
        let runtime_governor = default_nowledge_mem_runtime_governor(Path::new("."), db.config());
        Self::from_database_with_runtime_governor(db, mode, runtime_governor)
    }

    pub fn from_database_with_runtime_governor(
        mut db: Database,
        mode: NowledgeMemGraphMode,
        runtime_governor: RuntimeGovernor,
    ) -> Self {
        db.set_runtime_governor(runtime_governor.clone());
        Self {
            db,
            mode,
            runtime_governor,
        }
    }

    pub fn mode(&self) -> NowledgeMemGraphMode {
        self.mode
    }

    pub fn database(&self) -> &Database {
        &self.db
    }

    pub fn database_mut(&mut self) -> &mut Database {
        &mut self.db
    }

    pub fn runtime_governor(&self) -> &RuntimeGovernor {
        &self.runtime_governor
    }

    pub fn runtime_governor_snapshot(&self) -> RuntimeGovernorSnapshot {
        self.runtime_governor.snapshot()
    }

    pub fn refresh_runtime_resources(&self) -> bool {
        self.runtime_governor.refresh_from_host()
    }

    pub fn into_database(self) -> Database {
        self.db
    }

    pub fn query(&mut self, cypher: &str) -> Result<QueryOutput> {
        self.query_with_params(cypher, &BTreeMap::new())
    }

    pub fn query_with_report(&mut self, cypher: &str) -> Result<NowledgeMemQueryOutput> {
        self.query_with_params_with_report(cypher, &BTreeMap::new())
    }

    pub fn query_with_report_options(
        &mut self,
        cypher: &str,
        options: NowledgeMemQueryReportOptions,
    ) -> Result<NowledgeMemQueryOutput> {
        self.query_with_params_with_report_options(cypher, &BTreeMap::new(), options)
    }

    pub fn query_with_params(
        &mut self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
    ) -> Result<QueryOutput> {
        Ok(self
            .query_with_params_with_report(cypher, parameters)?
            .output)
    }

    pub fn query_with_params_with_report(
        &mut self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
    ) -> Result<NowledgeMemQueryOutput> {
        self.query_with_params_with_report_options(
            cypher,
            parameters,
            NowledgeMemQueryReportOptions::default(),
        )
    }

    pub fn query_with_params_with_report_options(
        &mut self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        options: NowledgeMemQueryReportOptions,
    ) -> Result<NowledgeMemQueryOutput> {
        let mut external = crate::executor::NoExternalReadOperator;
        self.query_with_params_with_report_options_and_external(
            cypher,
            parameters,
            options,
            &mut external,
        )
    }

    pub fn query_with_params_with_report_options_context(
        &mut self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        options: NowledgeMemQueryReportOptions,
        task_context: &RuntimeTaskContext,
    ) -> Result<NowledgeMemQueryOutput> {
        let mut external = crate::executor::NoExternalReadOperator;
        self.query_with_params_with_report_options_and_external_context(
            cypher,
            parameters,
            options,
            &mut external,
            task_context,
        )
    }

    fn query_with_params_with_report_options_and_external(
        &mut self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        options: NowledgeMemQueryReportOptions,
        external: &mut dyn crate::executor::ExternalReadOperator,
    ) -> Result<NowledgeMemQueryOutput> {
        self.query_with_params_with_report_options_and_external_context(
            cypher,
            parameters,
            options,
            external,
            &RuntimeTaskContext::default(),
        )
    }

    fn query_with_params_with_report_options_and_external_context(
        &mut self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        options: NowledgeMemQueryReportOptions,
        external: &mut dyn crate::executor::ExternalReadOperator,
        task_context: &RuntimeTaskContext,
    ) -> Result<NowledgeMemQueryOutput> {
        self.check_runtime_context(task_context)?;
        let (permit, is_mutation) = self.admit_materialized_query(cypher, parameters)?;
        let execution_task_context = permit.bind_task_context(task_context.clone());
        let started = Instant::now();
        let result = self.db.query_with_params_trace_and_external_with_context(
            cypher,
            parameters,
            options.capture_physical_plan,
            external,
            None,
            Some(&execution_task_context),
        );
        self.record_runtime_cancellation(result.as_ref().err(), task_context);
        let (output, execution_trace) = result?;
        if !is_mutation {
            self.check_runtime_context(task_context)?;
        }
        let elapsed_micros = started.elapsed().as_micros();
        let report = nowledge_mem_query_report(NowledgeMemQueryReportInput {
            mode: self.mode,
            statement: &execution_trace.statement,
            trace: execution_trace.optimizer_trace.as_ref(),
            plan_cache_lookup: execution_trace.plan_cache_lookup,
            execution_profile: execution_trace.execution_profile.as_ref(),
            output: &output,
            options,
            elapsed_micros,
        });
        Ok(NowledgeMemQueryOutput { output, report })
    }

    fn admit_materialized_query(
        &self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
    ) -> Result<(RuntimePermit, bool)> {
        let admission = self.db.runtime_admission_plan(cypher, parameters)?;
        let is_mutation = admission.is_mutation;
        let governor_budget = self.runtime_governor.snapshot().limits.result_budget_bytes;
        let result_budget = if admission.is_mutation {
            governor_budget
        } else {
            let configured = self
                .db
                .config()
                .max_read_result_payload_bytes
                .ok_or_else(|| {
                    HawDBError::Execution(
                        "admitted materialized query requires max_read_result_payload_bytes"
                            .to_string(),
                    )
                })?;
            let configured = u64::try_from(configured).unwrap_or(u64::MAX);
            if configured > governor_budget {
                return Err(HawDBError::Execution(format!(
                    "admitted materialized query payload budget {configured} exceeds runtime result budget {governor_budget}"
                )));
            }
            configured
        };
        self.try_admit_runtime(
            admission
                .runtime_work_request_for_snapshot(result_budget, self.runtime_governor.snapshot()),
        )
        .map(|permit| (permit, is_mutation))
    }

    fn admit_streaming_query(
        &self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        result_budget_bytes: usize,
    ) -> Result<RuntimePermit> {
        let admission = self.db.runtime_admission_plan(cypher, parameters)?;
        if admission.is_mutation {
            return Err(HawDBError::Execution(
                "admitted streaming query must be read-only".to_string(),
            ));
        }
        self.try_admit_runtime(admission.runtime_work_request_for_snapshot(
            u64::try_from(result_budget_bytes).unwrap_or(u64::MAX),
            self.runtime_governor.snapshot(),
        ))
    }

    fn try_admit_runtime(&self, request: hawdb_qos::RuntimeWorkRequest) -> Result<RuntimePermit> {
        self.runtime_governor.try_admit(request).map_err(|error| {
            if error.is_retryable() {
                self.runtime_governor
                    .record_admission_wait(request, error.code, 0);
            }
            HawDBError::Execution(error.to_string())
        })
    }

    fn admitted_streaming_result_bytes(&self, options: &NowledgeMemReadOptions) -> Result<usize> {
        let governor_budget =
            usize::try_from(self.runtime_governor.snapshot().limits.result_budget_bytes)
                .unwrap_or(usize::MAX);
        let configured = self
            .db
            .config()
            .max_read_result_payload_bytes
            .unwrap_or(governor_budget);
        let requested = options
            .max_estimated_payload_bytes
            .unwrap_or(governor_budget);
        let admitted = governor_budget.min(configured).min(requested);
        if admitted == 0 {
            return Err(HawDBError::Execution(
                "admitted streaming query requires a non-zero result byte budget".to_string(),
            ));
        }
        Ok(admitted)
    }

    fn check_runtime_context(&self, task_context: &RuntimeTaskContext) -> Result<()> {
        task_context.checkpoint().map_err(|reason| {
            self.runtime_governor.record_cancellation(reason);
            HawDBError::Execution(format!("runtime task {reason}"))
        })
    }

    fn record_runtime_cancellation(
        &self,
        error: Option<&HawDBError>,
        task_context: &RuntimeTaskContext,
    ) {
        if error.is_some()
            && let Err(reason) = task_context.checkpoint()
        {
            self.runtime_governor.record_cancellation(reason);
        }
    }

    pub fn slow_query_report(&self) -> NowledgeMemSlowQueryReport {
        let config = self.db.config();
        NowledgeMemSlowQueryReport::from_summaries(
            self.mode,
            config.slow_query_log_capacity,
            config.slow_query_log_threshold_micros,
            self.db.slow_query_log_snapshot(),
        )
    }

    pub fn slow_query_report_json(&self) -> serde_json::Value {
        self.slow_query_report().json()
    }

    pub fn read_query(&self, cypher: &str) -> Result<NowledgeMemReadOutput> {
        self.read_query_with_params(cypher, &BTreeMap::new(), &NowledgeMemReadOptions::default())
    }

    pub fn read_query_with_options(
        &self,
        cypher: &str,
        options: &NowledgeMemReadOptions,
    ) -> Result<NowledgeMemReadOutput> {
        self.read_query_with_params(cypher, &BTreeMap::new(), options)
    }

    pub fn read_query_with_params(
        &self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        options: &NowledgeMemReadOptions,
    ) -> Result<NowledgeMemReadOutput> {
        let mut output = self
            .read_query_with_params_streaming_collect(cypher, parameters, options)
            .map_err(|error| legacy_nowledge_mem_read_error(error, options))?;
        output.report.streaming = false;
        Ok(output)
    }

    pub fn read_query_with_params_streaming(
        &self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        options: &NowledgeMemReadOptions,
        consumer: impl FnMut(BTreeMap<String, Value>) -> Result<()>,
    ) -> Result<QueryStreamReport> {
        self.read_query_with_params_streaming_context(
            cypher,
            parameters,
            options,
            &RuntimeTaskContext::default(),
            consumer,
        )
    }

    pub fn read_query_with_params_streaming_context(
        &self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        options: &NowledgeMemReadOptions,
        task_context: &RuntimeTaskContext,
        consumer: impl FnMut(BTreeMap<String, Value>) -> Result<()>,
    ) -> Result<QueryStreamReport> {
        self.check_runtime_context(task_context)?;
        let max_payload_bytes = self.admitted_streaming_result_bytes(options)?;
        let permit = self.admit_streaming_query(cypher, parameters, max_payload_bytes)?;
        let execution_task_context = permit.bind_task_context(task_context.clone());
        let result = self
            .db
            .begin_read_transaction()?
            .query_with_params_streaming_context(
                cypher,
                parameters,
                QueryStreamOptions {
                    max_rows: options.max_rows,
                    max_payload_bytes: Some(max_payload_bytes),
                },
                &execution_task_context,
                consumer,
            );
        self.record_runtime_cancellation(result.as_ref().err(), task_context);
        result
    }

    /// Collects host-owned rows through the streaming consumer boundary.
    /// This retains the legacy `NowledgeMemReadOutput` shape while ensuring
    /// bounded results are validated before any row crosses into the host.
    pub fn read_query_with_params_streaming_collect(
        &self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        options: &NowledgeMemReadOptions,
    ) -> Result<NowledgeMemReadOutput> {
        let mut rows = Vec::new();
        let streamed =
            self.read_query_with_params_streaming(cypher, parameters, options, |row| {
                rows.push(row);
                Ok(())
            })?;
        streamed_nowledge_mem_read_output(self.mode, rows, streamed, options)
    }

    pub fn graph_rag_schema_context(
        &self,
        options: GraphRagSchemaContextOptions,
    ) -> Result<GraphRagSchemaContext> {
        self.db.graph_rag_schema_context(options)
    }

    pub fn read_generated_graph_rag(
        &self,
        query: &GraphRagGeneratedQuery,
        parameters: &BTreeMap<String, Value>,
        options: &NowledgeMemReadOptions,
    ) -> Result<NowledgeMemReadOutput> {
        let bounded = self
            .db
            .begin_read_transaction()?
            .query_generated_graph_rag_bounded_profile(query, parameters, options.max_rows)?;
        bounded_nowledge_mem_read_output(self.mode, bounded, options)
    }
}

#[derive(Debug)]
pub struct NowledgeMemSearchProjection {
    index: SearchIndex,
}

#[derive(Debug)]
pub struct NowledgeMemOutOfCoreSearchProjection {
    reader: SearchOutOfCoreReader,
    maintenance: Option<(PathBuf, NowledgeMemIncrementalSearchMaintenanceOptions)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NowledgeMemOutOfCoreSearchCandidateOutput {
    pub result: SearchResultSet,
    pub report: NowledgeMemSearchCandidateReport,
    pub metrics: SearchOutOfCoreMetrics,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NowledgeMemSearchHydrationOutput {
    pub documents: Vec<SearchDocument>,
    pub out_of_core_metrics: Option<SearchOutOfCoreMetrics>,
}

impl From<NowledgeMemOutOfCoreSearchCandidateOutput> for NowledgeMemSearchCandidateOutput {
    fn from(output: NowledgeMemOutOfCoreSearchCandidateOutput) -> Self {
        Self {
            result: output.result,
            report: output.report,
            out_of_core_metrics: Some(output.metrics),
        }
    }
}

impl NowledgeMemOutOfCoreSearchProjection {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            reader: SearchOutOfCoreReader::open(path)?,
            maintenance: None,
        })
    }

    pub fn open_with_config(path: impl AsRef<Path>, config: SearchOutOfCoreConfig) -> Result<Self> {
        Ok(Self {
            reader: SearchOutOfCoreReader::open_with_config(path, config)?,
            maintenance: None,
        })
    }

    pub fn open_production(
        path: impl AsRef<Path>,
        qualification: &SearchLexicalProductionQualificationReport,
        expected_identity: &crate::ProductionQualificationIdentity,
    ) -> Result<Self> {
        let projection = Self::open(path)?;
        projection.validate_lexical_production_qualification(qualification, expected_identity)?;
        Ok(projection)
    }

    pub fn open_production_with_config(
        path: impl AsRef<Path>,
        config: SearchOutOfCoreConfig,
        qualification: &SearchLexicalProductionQualificationReport,
        expected_identity: &crate::ProductionQualificationIdentity,
    ) -> Result<Self> {
        let projection = Self::open_with_config(path, config)?;
        projection.validate_lexical_production_qualification(qualification, expected_identity)?;
        Ok(projection)
    }

    pub fn validate_lexical_production_qualification(
        &self,
        qualification: &SearchLexicalProductionQualificationReport,
        expected_identity: &crate::ProductionQualificationIdentity,
    ) -> Result<()> {
        qualification.validate_for_projection_and_release(
            &self.reader.production_qualification_identity(),
            expected_identity,
        )
    }

    pub fn reader(&self) -> &SearchOutOfCoreReader {
        &self.reader
    }

    pub fn freshness(&self) -> SearchProjectionFreshness {
        self.reader.projection_freshness()
    }

    fn runtime_admission_memory_bytes(
        &self,
        working_memory_bytes: u64,
        result_budget_bytes: u64,
    ) -> u64 {
        let config = self.reader.config();
        working_memory_bytes
            .saturating_add(config.max_vector_search_working_bytes.get() as u64)
            .saturating_add(config.max_uncompressed_segment_bytes.get())
            .saturating_add(config.max_candidate_block_bytes.get())
            .saturating_add(config.max_hydrated_bytes.get().min(result_budget_bytes))
            .saturating_add(config.max_matched_span_bytes.get())
    }

    pub fn hydrate_documents(
        &self,
        document_ids: &[String],
    ) -> Result<SearchOutOfCoreHydrationOutput> {
        self.reader.hydrate_documents(document_ids)
    }

    pub fn search_candidates(
        &self,
        request: &NowledgeMemSearchCandidateRequest,
    ) -> Result<SearchResultSet> {
        Ok(self.search_candidates_with_report(request)?.result)
    }

    pub fn search_candidates_with_report(
        &self,
        request: &NowledgeMemSearchCandidateRequest,
    ) -> Result<NowledgeMemOutOfCoreSearchCandidateOutput> {
        self.search_candidates_with_report_context(request, VectorSearchExecutionOptions::default())
    }

    pub fn search_candidates_with_report_context(
        &self,
        request: &NowledgeMemSearchCandidateRequest,
        vector_execution_options: VectorSearchExecutionOptions<'_>,
    ) -> Result<NowledgeMemOutOfCoreSearchCandidateOutput> {
        let effective_compressed_vector_search_mode = effective_search_candidate_mode(request);
        let output = self
            .reader
            .search_with_options_compressed_vector_projection_execution_options(
                &request.query_text,
                request.query_embedding.as_deref(),
                request.mode,
                SearchQueryOptions {
                    limit: request.limit,
                    offset: request.offset,
                    rank_window: request.rank_window,
                    fusion_weights: request.fusion_weights,
                    metadata_filters: request.metadata_filters.clone(),
                    policy_epoch: None,
                },
                effective_compressed_vector_search_mode,
                vector_execution_options,
            )?;
        let report = nowledge_mem_search_candidate_report(
            request,
            effective_compressed_vector_search_mode,
            &output.result,
        );
        Ok(NowledgeMemOutOfCoreSearchCandidateOutput {
            result: output.result,
            report,
            metrics: output.metrics,
        })
    }
}

impl NowledgeMemSearchProjection {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            index: SearchIndex::open(path)?,
        })
    }

    pub fn from_index(index: SearchIndex) -> Self {
        Self { index }
    }

    pub fn set_range_read_config(&mut self, config: SearchRangeReadConfig) {
        self.index.set_range_read_config(config);
    }

    pub fn index(&self) -> &SearchIndex {
        &self.index
    }

    pub fn index_mut(&mut self) -> &mut SearchIndex {
        &mut self.index
    }

    pub fn set_telemetry_sink(&mut self, telemetry: Option<Arc<dyn TelemetrySink>>) {
        self.index.set_telemetry_sink(telemetry);
    }

    pub fn into_index(self) -> SearchIndex {
        self.index
    }

    pub fn probe_json(&self, options: SearchProjectionProbeOptions) -> serde_json::Value {
        self.index.nowledge_search_projection_probe_json(options)
    }

    pub fn evidence_json(&self, options: SearchProjectionProbeOptions) -> serde_json::Value {
        nowledge_search_projection_evidence_json(&self.probe_json(options))
    }

    pub fn evidence_report(
        &self,
        options: SearchProjectionProbeOptions,
    ) -> NowledgeSearchProjectionEvidenceReport {
        NowledgeSearchProjectionEvidenceReport::from_probe(&self.probe_json(options))
    }

    pub fn validate_sampled_vector_recall(
        &self,
        options: VectorRecallValidationOptions,
    ) -> VectorRecallValidationReport {
        self.index.validate_sampled_vector_recall(options)
    }

    pub fn qualify_sampled_vector_recall_for_production(
        &self,
        options: VectorRecallValidationOptions,
        evidence_binding: crate::ProductionEvidenceBinding,
        expected_identity: crate::ProductionQualificationIdentity,
    ) -> VectorRecallProductionQualificationReport {
        self.index.qualify_sampled_vector_recall_for_production(
            options,
            evidence_binding,
            expected_identity,
        )
    }

    pub fn shadow_evidence_json(
        &self,
        primary_probe: &serde_json::Value,
        options: SearchProjectionProbeOptions,
    ) -> serde_json::Value {
        nowledge_search_projection_shadow_evidence_json(primary_probe, &self.probe_json(options))
    }

    pub fn freshness(&self) -> SearchProjectionFreshness {
        self.index.projection_freshness()
    }

    pub fn search_candidates(
        &self,
        request: &NowledgeMemSearchCandidateRequest,
    ) -> Result<SearchResultSet> {
        self.search_candidates_with_report(request)
            .map(|output| output.result)
    }

    pub fn search_candidates_with_report(
        &self,
        request: &NowledgeMemSearchCandidateRequest,
    ) -> Result<NowledgeMemSearchCandidateOutput> {
        let effective_compressed_vector_search_mode = effective_search_candidate_mode(request);
        let result = self.index.search_with_options_adaptive_vector_projection(
            &request.query_text,
            request.query_embedding.as_deref(),
            request.mode,
            SearchQueryOptions {
                limit: request.limit,
                offset: request.offset,
                rank_window: request.rank_window,
                fusion_weights: request.fusion_weights,
                metadata_filters: request.metadata_filters.clone(),
                policy_epoch: None,
            },
            AdaptiveVectorSearchOptions {
                compression_mode: effective_compressed_vector_search_mode,
                backend_policy: request.adaptive_vector_backend_policy,
                recall_validation_probe: request.recall_validation_probe,
            },
        )?;
        let report = nowledge_mem_search_candidate_report(
            request,
            effective_compressed_vector_search_mode,
            &result,
        );
        Ok(NowledgeMemSearchCandidateOutput {
            result,
            report,
            out_of_core_metrics: None,
        })
    }

    pub fn try_search_candidates_with_report(
        &self,
        request: &NowledgeMemSearchCandidateRequest,
    ) -> Result<NowledgeMemSearchCandidateOutput> {
        self.try_search_candidates_with_report_context(
            request,
            VectorSearchExecutionOptions::default(),
        )
    }

    pub fn try_search_candidates_with_report_context(
        &self,
        request: &NowledgeMemSearchCandidateRequest,
        vector_execution_options: VectorSearchExecutionOptions<'_>,
    ) -> Result<NowledgeMemSearchCandidateOutput> {
        let effective_compressed_vector_search_mode = effective_search_candidate_mode(request);
        let result = self
            .index
            .try_search_with_options_adaptive_vector_projection_context(
                &request.query_text,
                request.query_embedding.as_deref(),
                request.mode,
                SearchQueryOptions {
                    limit: request.limit,
                    offset: request.offset,
                    rank_window: request.rank_window,
                    fusion_weights: request.fusion_weights,
                    metadata_filters: request.metadata_filters.clone(),
                    policy_epoch: None,
                },
                AdaptiveVectorSearchOptions {
                    compression_mode: effective_compressed_vector_search_mode,
                    backend_policy: request.adaptive_vector_backend_policy,
                    recall_validation_probe: request.recall_validation_probe,
                },
                vector_execution_options,
            )?;
        let report = nowledge_mem_search_candidate_report(
            request,
            effective_compressed_vector_search_mode,
            &result,
        );
        Ok(NowledgeMemSearchCandidateOutput {
            result,
            report,
            out_of_core_metrics: None,
        })
    }

    pub fn search_candidate_readiness(
        &self,
        request: &NowledgeMemSearchCandidateRequest,
        options: &NowledgeMemSearchCandidateReadinessOptions,
    ) -> Result<NowledgeMemSearchCandidateReadinessReport> {
        self.search_candidates_with_report(request)
            .map(|output| output.readiness_report(options))
    }

    pub fn search_candidate_shadow_evidence<I, S>(
        &self,
        request: &NowledgeMemSearchCandidateRequest,
        primary_candidate_ids: I,
    ) -> Result<NowledgeMemSearchCandidateShadowEvidence>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let output = self.search_candidates_with_report(request)?;
        let mut accumulator = NowledgeMemSearchCandidateShadowAccumulator::new();
        accumulator.record_search_candidate_output(primary_candidate_ids, &output);
        Ok(accumulator.evidence())
    }

    pub fn search_candidate_shadow_evidence_json<I, S>(
        &self,
        request: &NowledgeMemSearchCandidateRequest,
        primary_candidate_ids: I,
    ) -> Result<serde_json::Value>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.search_candidate_shadow_evidence(request, primary_candidate_ids)
            .map(|evidence| evidence.json())
    }
}

struct SearchProjectionExternalReadOperator<'a> {
    projection: Option<&'a NowledgeMemSearchProjection>,
    out_of_core_projection: Option<&'a NowledgeMemOutOfCoreSearchProjection>,
    vector_seed_execution_count: usize,
}

impl crate::executor::ExternalReadOperator for SearchProjectionExternalReadOperator<'_> {
    fn execute_vector_seed(
        &mut self,
        request: crate::executor::VectorSeedExecutionRequest<'_>,
    ) -> Result<crate::executor::VectorSeedExecutionOutput> {
        request.resources.checkpoint()?;
        let top_k = vector_plan_top_k(request.vector_plan)?.min(request.resources.result.max_rows);
        let search_request =
            NowledgeMemSearchCandidateRequest::vector(request.embedding.to_vec(), top_k)
                .with_metadata_filters(request.metadata_filters.clone());
        let vector_execution_options = VectorSearchExecutionOptions::bounded(
            request.resources.max_parallelism,
            request.resources.max_working_memory_bytes.get(),
            request.resources.task_context,
        );
        let output = match (self.projection, self.out_of_core_projection) {
            (Some(projection), None) => projection.try_search_candidates_with_report_context(
                &search_request,
                vector_execution_options,
            )?,
            (None, Some(projection)) => projection
                .search_candidates_with_report_context(&search_request, vector_execution_options)?
                .into(),
            (None, None) => return Err(missing_search_projection_error()),
            (Some(_), Some(_)) => {
                return Err(HawDBError::Storage(
                    "nowledge mem search projection ownership is ambiguous".to_string(),
                ));
            }
        };
        let retriever = output
            .result
            .retrievers
            .iter()
            .find(|retriever| retriever.name == "vector")
            .ok_or_else(|| {
                HawDBError::Execution(
                    "vector search did not produce a vector retriever report".to_string(),
                )
            })?;
        let candidate_score_source = vector_score_source(&retriever.candidate_score_source)?;
        let execution_output = crate::executor::VectorSeedExecutionOutput {
            rows: output
                .result
                .hits
                .into_iter()
                .map(|hit| crate::executor::VectorSeedExecutionRow {
                    id: hit.id,
                    external_id: hit.external_id,
                    score: hit.vector_score,
                })
                .collect(),
            report: hawdb_executor::VectorExecutionReport {
                backend: vector_execution_backend(candidate_score_source),
                compression_mode: vector_compression_mode(
                    output.report.compressed_vector_search_mode,
                ),
                candidate_source: vector_plan_candidate_source(request.vector_plan)?,
                backend_selection_reason: retriever.backend_selection_reason,
                estimated_raw_vector_bytes: retriever.estimated_raw_vector_bytes,
                filter_selectivity_per_million: retriever.filter_selectivity_per_million,
                candidate_score_source,
                final_score_source: vector_score_source(&retriever.final_score_source)?,
                generated_candidate_count: retriever.generated_candidate_count,
                descriptor_pruned_count: retriever.descriptor_pruned_count,
                scalar_filtered_count: retriever.scalar_filtered_count,
                residual_filtered_count: retriever.residual_filtered_count,
                candidate_scan_rounds: retriever.candidate_scan_rounds,
                reranked_candidate_count: retriever.reranked_candidate_count,
                returned_count: retriever.candidate_count,
                raw_vector_bytes_read: retriever.raw_vector_bytes_read,
                candidate_scan_metrics: retriever.candidate_scan_kernel.as_ref().map(|kernel| {
                    hawdb_executor::VectorCandidateScanMetrics {
                        kernel: kernel.clone(),
                        worker_count: retriever.candidate_scan_worker_count,
                        segment_count: retriever.candidate_scan_segment_count,
                        scanned_segment_count: retriever.candidate_scan_scanned_segment_count,
                        scored_document_count: retriever.candidate_scan_scored_document_count,
                        filtered_document_count: retriever.candidate_scan_filtered_document_count,
                        scanned_block_count: retriever.candidate_scan_scanned_block_count,
                        skipped_block_count: retriever.candidate_scan_skipped_block_count,
                        payload_bytes_read: retriever.candidate_scan_payload_bytes_read,
                        admitted_working_bytes: retriever.candidate_scan_admitted_working_bytes,
                    }
                }),
                index_covered_document_count: Some(retriever.index_covered_document_count),
                index_candidate_document_count: Some(retriever.index_candidate_document_count),
                index_coverage_complete: Some(retriever.index_coverage_complete),
                fallback_reason_codes: retriever
                    .fallback_reason_codes
                    .iter()
                    .filter_map(|code| vector_fallback_reason_code(*code))
                    .collect(),
            },
        };
        execution_output.validate_result_budget(request.resources.result)?;
        request.resources.checkpoint()?;
        self.vector_seed_execution_count = self
            .vector_seed_execution_count
            .checked_add(1)
            .ok_or_else(|| {
                HawDBError::Execution(
                    "bounded external vector seed execution count overflowed".to_string(),
                )
            })?;
        Ok(execution_output)
    }
}

fn vector_plan_top_k(plan: &hawdb_plan_cypher::VectorPhysicalPlan) -> Result<usize> {
    match plan {
        hawdb_plan_cypher::VectorPhysicalPlan::TopK { limit, .. } => Ok(*limit),
        _ => Err(HawDBError::Execution(
            "vector seed physical plan is missing TopK".to_string(),
        )),
    }
}

fn vector_plan_candidate_source(
    plan: &hawdb_plan_cypher::VectorPhysicalPlan,
) -> Result<hawdb_plan_cypher::VectorCandidateSource> {
    match plan {
        hawdb_plan_cypher::VectorPhysicalPlan::VectorCandidateScan { source, .. } => Ok(*source),
        hawdb_plan_cypher::VectorPhysicalPlan::ResidualFilter { input, .. }
        | hawdb_plan_cypher::VectorPhysicalPlan::RawVectorRerank { input, .. }
        | hawdb_plan_cypher::VectorPhysicalPlan::TopK { input, .. } => {
            vector_plan_candidate_source(input)
        }
        hawdb_plan_cypher::VectorPhysicalPlan::Filter { .. } => Err(HawDBError::Execution(
            "vector seed physical plan is missing VectorCandidateScan".to_string(),
        )),
    }
}

fn vector_score_source(value: &str) -> Result<hawdb_executor::VectorScoreSource> {
    match value {
        "unavailable" | "none" => Ok(hawdb_executor::VectorScoreSource::Unavailable),
        "raw_vector" => Ok(hawdb_executor::VectorScoreSource::RawVector),
        "ann_approximate" => Ok(hawdb_executor::VectorScoreSource::AnnApproximate),
        "quantized_approximate" => Ok(hawdb_executor::VectorScoreSource::QuantizedApproximate),
        _ => Err(HawDBError::Execution(
            "vector search returned an unsupported score source".to_string(),
        )),
    }
}

fn vector_execution_backend(
    score_source: hawdb_executor::VectorScoreSource,
) -> hawdb_executor::VectorExecutionBackend {
    match score_source {
        hawdb_executor::VectorScoreSource::Unavailable => {
            hawdb_executor::VectorExecutionBackend::Unavailable
        }
        hawdb_executor::VectorScoreSource::RawVector => {
            hawdb_executor::VectorExecutionBackend::ScalarFlat
        }
        hawdb_executor::VectorScoreSource::AnnApproximate => {
            hawdb_executor::VectorExecutionBackend::AnnProjection
        }
        hawdb_executor::VectorScoreSource::QuantizedApproximate => {
            hawdb_executor::VectorExecutionBackend::QuantizedProjection
        }
    }
}

fn vector_compression_mode(
    mode: CompressedVectorSearchMode,
) -> hawdb_executor::VectorCompressionMode {
    match mode {
        CompressedVectorSearchMode::Disabled => hawdb_executor::VectorCompressionMode::Disabled,
        CompressedVectorSearchMode::Preferred => hawdb_executor::VectorCompressionMode::Preferred,
        CompressedVectorSearchMode::Required => hawdb_executor::VectorCompressionMode::Required,
    }
}

fn vector_fallback_reason_code(
    code: SearchFallbackReasonCode,
) -> Option<hawdb_executor::VectorFallbackReasonCode> {
    match code {
        SearchFallbackReasonCode::VectorDimensionMismatch => {
            Some(hawdb_executor::VectorFallbackReasonCode::VectorDimensionMismatch)
        }
        SearchFallbackReasonCode::VectorIndexEmpty => {
            Some(hawdb_executor::VectorFallbackReasonCode::VectorIndexEmpty)
        }
        SearchFallbackReasonCode::CompressedVectorProjectionUnavailable => {
            Some(hawdb_executor::VectorFallbackReasonCode::CompressedVectorProjectionUnavailable)
        }
        SearchFallbackReasonCode::CompressedVectorBudgetExceeded => {
            Some(hawdb_executor::VectorFallbackReasonCode::CompressedVectorBudgetExceeded)
        }
        SearchFallbackReasonCode::QueryEmbeddingMissing => {
            Some(hawdb_executor::VectorFallbackReasonCode::QueryEmbeddingMissing)
        }
        SearchFallbackReasonCode::TextQueryEmpty => None,
    }
}

#[derive(Debug)]
pub struct NowledgeMemEmbeddedStore {
    graph: NowledgeMemGraph,
    search_projection: Option<NowledgeMemSearchProjection>,
    out_of_core_search_projection: Option<NowledgeMemOutOfCoreSearchProjection>,
    retrieval_projection_advisor: NowledgeMemRetrievalProjectionAdvisor,
}

#[derive(Debug, Clone)]
pub struct NowledgeMemEmbeddedStoreHandle {
    inner: Arc<RwLock<NowledgeMemEmbeddedStore>>,
    published_read: Arc<Mutex<Option<PublishedCanonicalRead>>>,
    // One-way: a failed or panicking write guard sets this and leaves the
    // last-good publication untouched, so readers fail closed instead of
    // silently serving pre-write state. Mirrors CommitGuard's poison flag.
    read_publication_failed: Arc<AtomicBool>,
}

#[derive(Debug)]
struct PublishedCanonicalRead {
    instance_identity: crate::DatabaseInstanceIdentity,
    snapshot: crate::api::DatabaseReadSnapshot,
    governor: RuntimeGovernor,
}

impl PublishedCanonicalRead {
    fn capture(store: &NowledgeMemEmbeddedStore) -> Result<Self> {
        Ok(Self {
            instance_identity: store.graph.database().instance_identity()?,
            snapshot: store.graph.database().read_snapshot()?,
            governor: store.graph.runtime_governor().clone(),
        })
    }
}

struct EmbeddedStoreWriteGuard<'a> {
    store: RwLockWriteGuard<'a, NowledgeMemEmbeddedStore>,
    published: &'a Mutex<Option<PublishedCanonicalRead>>,
    read_publication_failed: &'a AtomicBool,
}

impl std::ops::Deref for EmbeddedStoreWriteGuard<'_> {
    type Target = NowledgeMemEmbeddedStore;

    fn deref(&self) -> &Self::Target {
        &self.store
    }
}

impl std::ops::DerefMut for EmbeddedStoreWriteGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.store
    }
}

impl Drop for EmbeddedStoreWriteGuard<'_> {
    fn drop(&mut self) {
        // A panicking writer or a failed capture must never overwrite the
        // last known-good publication with an empty or stale one. Record the
        // failure so readers fail closed instead of silently serving
        // pre-write state; the committed write itself is not undone.
        if std::thread::panicking() {
            self.read_publication_failed.store(true, Ordering::Release);
            return;
        }
        if self.read_publication_failed.load(Ordering::Acquire) {
            return;
        }
        // Capture before publication and before releasing the writer guard.
        // Refresh even when a checkpoint changes only the physical generation.
        let next = match PublishedCanonicalRead::capture(&self.store) {
            Ok(next) => next,
            Err(_) => {
                self.read_publication_failed.store(true, Ordering::Release);
                return;
            }
        };
        let previous = self
            .published
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .replace(next);
        // Releasing the last generation pin can acquire the pin registry lock.
        // Keep that destructor outside the publication critical section.
        drop(previous);
    }
}

pub struct NowledgeMemReadSnapshot<'a> {
    instance_identity: crate::DatabaseInstanceIdentity,
    transaction: crate::DatabaseReadTransaction,
    external: SearchProjectionExternalReadOperator<'a>,
    search_projection_source_graph_commit_epoch: Option<u64>,
    search_projection_durable_source_graph_commit_epoch: Option<u64>,
    budget: NowledgeMemReadSnapshotBudget,
    cypher_statement_count: usize,
    sql_statement_count: usize,
    output_rows: usize,
    output_payload_bytes: usize,
    task_context: RuntimeTaskContext,
}

impl<'a> NowledgeMemReadSnapshot<'a> {
    fn new(
        transaction: crate::DatabaseReadTransaction,
        instance_identity: crate::DatabaseInstanceIdentity,
        budget: NowledgeMemReadSnapshotBudget,
        projection: Option<&'a NowledgeMemSearchProjection>,
        out_of_core_projection: Option<&'a NowledgeMemOutOfCoreSearchProjection>,
        task_context: RuntimeTaskContext,
    ) -> Self {
        let freshness = match (projection, out_of_core_projection) {
            (Some(projection), None) => Some(projection.freshness()),
            (None, Some(projection)) => Some(projection.freshness()),
            (None, None) | (Some(_), Some(_)) => None,
        };
        Self {
            transaction,
            instance_identity,
            external: SearchProjectionExternalReadOperator {
                projection,
                out_of_core_projection,
                vector_seed_execution_count: 0,
            },
            search_projection_source_graph_commit_epoch: freshness
                .as_ref()
                .and_then(|value| value.source_graph_commit_epoch),
            search_projection_durable_source_graph_commit_epoch: freshness
                .as_ref()
                .and_then(|value| value.durable_source_graph_commit_epoch),
            budget,
            cypher_statement_count: 0,
            sql_statement_count: 0,
            output_rows: 0,
            output_payload_bytes: 0,
            task_context,
        }
    }

    pub fn commit_epoch(&self) -> u64 {
        self.transaction.commit_epoch()
    }

    /// Identity of the database supplying this exact immutable read view.
    pub fn database_instance_identity(&self) -> &crate::DatabaseInstanceIdentity {
        &self.instance_identity
    }

    pub fn query_cypher(
        &mut self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        max_rows: usize,
    ) -> Result<QueryOutput> {
        Ok(self
            .query_cypher_profiled(cypher, parameters, max_rows)?
            .output)
    }

    pub fn query_cypher_profiled(
        &mut self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        max_rows: usize,
    ) -> Result<BoundedReadQueryOutput> {
        self.query_request_profiled(
            crate::QueryRequest::new(cypher).with_params(parameters),
            max_rows,
        )
    }

    /// Typed query attachment under the same pinned snapshot and cumulative
    /// result budgets as ordinary Cypher. Scoring does not widen caller caps.
    pub fn query_request(
        &mut self,
        request: crate::QueryRequest<'_>,
        max_rows: usize,
    ) -> Result<QueryOutput> {
        Ok(self.query_request_profiled(request, max_rows)?.output)
    }

    pub fn query_request_profiled(
        &mut self,
        request: crate::QueryRequest<'_>,
        max_rows: usize,
    ) -> Result<BoundedReadQueryOutput> {
        let completed_statement_count =
            self.cypher_statement_count.checked_add(1).ok_or_else(|| {
                HawDBError::Execution(
                    "bounded read snapshot Cypher statement count overflowed".to_string(),
                )
            })?;
        let max_rows = self.statement_row_budget(max_rows)?;
        let max_payload_bytes = self.remaining_payload_bytes()?;
        let mut rows = Vec::new();
        let report = self.transaction.query_request_streaming_with_external(
            request.with_restrictive_output_limits(QueryStreamOptions {
                max_rows: Some(max_rows),
                max_payload_bytes: Some(max_payload_bytes),
            }),
            &mut self.external,
            |row| {
                rows.push(row);
                Ok(())
            },
        )?;
        self.consume(report.output_rows, report.output_payload_bytes)?;
        self.cypher_statement_count = completed_statement_count;
        Ok(BoundedReadQueryOutput {
            output: QueryOutput { rows: rows.into() },
            execution_profile: report.execution_profile,
        })
    }

    pub fn query_sql(
        &mut self,
        sql: &str,
        parameters: &[Value],
        max_rows: usize,
    ) -> Result<QueryOutput> {
        let completed_statement_count =
            self.sql_statement_count.checked_add(1).ok_or_else(|| {
                HawDBError::Execution(
                    "bounded read snapshot SQL statement count overflowed".to_string(),
                )
            })?;
        let max_rows = self.statement_row_budget(max_rows)?;
        let max_payload_bytes = self.remaining_payload_bytes()?;
        let output = self.transaction.query_sql_with_params_options(
            sql,
            parameters,
            QueryStreamOptions {
                max_rows: Some(max_rows),
                max_payload_bytes: Some(max_payload_bytes),
            },
        )?;
        let output_rows = output.rows.len();
        let output_payload_bytes = estimate_query_output_payload_bytes(&output);
        self.consume(output_rows, output_payload_bytes)?;
        self.sql_statement_count = completed_statement_count;
        Ok(output)
    }

    /// Visits the complete current projection under this snapshot's retained pin.
    /// The visitor runs synchronously under the store read guard. It must not
    /// reenter the store or wait for an async source while retaining this guard.
    ///
    /// Order is unspecified. A visitor prefix is not exhaustion evidence: only
    /// the successful returned report certifies completion. Retained copies are
    /// caller-owned and require independent accounting. Encoded-record scratch
    /// has its own finite cap, distinct from cumulative logical output bytes.
    pub fn visit_search_projection(
        &mut self,
        max_encoded_record_bytes: usize,
        mut visitor: impl FnMut(&SearchDocument) -> Result<()>,
    ) -> Result<crate::search::SearchProjectionScanReport> {
        let budget = crate::search::SearchProjectionScanBudget {
            max_rows: self.statement_row_budget(self.budget.max_rows)?,
            max_payload_bytes: self.remaining_payload_bytes()?,
            max_encoded_record_bytes,
        };
        let report = match (
            self.external.projection,
            self.external.out_of_core_projection,
        ) {
            (Some(projection), None) => projection.index().visit_projection_documents(
                budget,
                &self.task_context,
                &mut visitor,
            )?,
            (None, Some(projection)) => projection.reader().visit_projection_documents(
                budget,
                &self.task_context,
                &mut visitor,
            )?,
            _ => {
                return Err(HawDBError::Execution(
                    "bounded projection scan requires exactly one pinned Search projection".into(),
                ))
            }
        };
        self.consume(report.document_count, report.payload_bytes)?;
        Ok(report)
    }

    pub fn report(&self) -> NowledgeMemReadSnapshotReport {
        NowledgeMemReadSnapshotReport {
            commit_epoch: self.commit_epoch(),
            search_projection_present: self.external.projection.is_some()
                || self.external.out_of_core_projection.is_some(),
            search_projection_source_graph_commit_epoch: self
                .search_projection_source_graph_commit_epoch,
            search_projection_durable_source_graph_commit_epoch: self
                .search_projection_durable_source_graph_commit_epoch,
            max_rows: self.budget.max_rows,
            max_payload_bytes: self.budget.max_payload_bytes,
            cypher_statement_count: self.cypher_statement_count,
            sql_statement_count: self.sql_statement_count,
            vector_seed_execution_count: self.external.vector_seed_execution_count,
            output_rows: self.output_rows,
            output_payload_bytes: self.output_payload_bytes,
            remaining_rows: self.budget.max_rows.saturating_sub(self.output_rows),
            remaining_payload_bytes: self
                .budget
                .max_payload_bytes
                .saturating_sub(self.output_payload_bytes),
        }
    }

    fn statement_row_budget(&self, requested: usize) -> Result<usize> {
        if requested == 0 {
            return Err(HawDBError::Execution(
                "bounded read statement requires max_rows greater than zero".to_string(),
            ));
        }
        let remaining = self.budget.max_rows.saturating_sub(self.output_rows);
        if remaining == 0 {
            return Err(HawDBError::read_budget_exceeded(
                ReadBudgetResource::Rows,
                self.budget.max_rows,
                "bounded read snapshot exhausted max_rows",
            ));
        }
        Ok(requested.min(remaining))
    }

    fn remaining_payload_bytes(&self) -> Result<usize> {
        let remaining = self
            .budget
            .max_payload_bytes
            .saturating_sub(self.output_payload_bytes);
        if remaining == 0 {
            return Err(HawDBError::read_budget_exceeded(
                ReadBudgetResource::PayloadBytes,
                self.budget.max_payload_bytes,
                "bounded read snapshot exhausted max_payload_bytes",
            ));
        }
        Ok(remaining)
    }

    fn consume(&mut self, rows: usize, payload_bytes: usize) -> Result<()> {
        let output_rows = self.output_rows.saturating_add(rows);
        let output_payload_bytes = self.output_payload_bytes.saturating_add(payload_bytes);
        if output_rows > self.budget.max_rows {
            return Err(HawDBError::read_budget_exceeded(
                ReadBudgetResource::Rows,
                self.budget.max_rows,
                format!(
                    "bounded read snapshot produced {output_rows} rows, exceeding max_rows {}",
                    self.budget.max_rows
                ),
            ));
        }
        if output_payload_bytes > self.budget.max_payload_bytes {
            return Err(HawDBError::read_budget_exceeded(ReadBudgetResource::PayloadBytes, self.budget.max_payload_bytes, format!(
                "bounded read snapshot produced {output_payload_bytes} payload bytes, exceeding max_payload_bytes {}",
                self.budget.max_payload_bytes
            )));
        }
        self.output_rows = output_rows;
        self.output_payload_bytes = output_payload_bytes;
        Ok(())
    }
}

impl NowledgeMemEmbeddedStoreHandle {
    pub fn new(store: NowledgeMemEmbeddedStore) -> Result<Self> {
        let published_read = PublishedCanonicalRead::capture(&store)?;
        Ok(Self {
            inner: Arc::new(RwLock::new(store)),
            published_read: Arc::new(Mutex::new(Some(published_read))),
            read_publication_failed: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn open_with_options(
        options: NowledgeMemOpenOptions,
    ) -> Result<(Self, NowledgeMemOpenReport)> {
        let (store, report) = NowledgeMemEmbeddedStore::open_with_options(options)?;
        Ok((Self::new(store)?, report))
    }

    pub fn open_with_options_and_runtime_governor(
        options: NowledgeMemOpenOptions,
        runtime_governor: RuntimeGovernor,
    ) -> Result<(Self, NowledgeMemOpenReport)> {
        let (store, report) = NowledgeMemEmbeddedStore::open_with_options_and_runtime_governor(
            options,
            runtime_governor,
        )?;
        Ok((Self::new(store)?, report))
    }

    pub fn runtime_governor_snapshot(&self) -> Result<RuntimeGovernorSnapshot> {
        Ok(self.read_store()?.runtime_governor_snapshot())
    }

    /// Returns a point-in-time storage residency snapshot through the admitted
    /// embedded facade. Qualification uses this to prove cancellation does not
    /// leak cache pins without reaching through to the storage engine.
    pub fn storage_residency_report(&self) -> Result<crate::StorageResidencyReport> {
        self.read_store()?
            .graph
            .database()
            .storage_residency_report()
    }

    /// Reports whether a fail-closed storage error poisoned this handle.
    pub fn storage_handle_poisoned(&self) -> Result<bool> {
        self.read_store()?
            .graph
            .database()
            .storage_handle_poisoned()
    }

    /// Reports the admitted facade contract. Production hosts must bind this
    /// report to their long-lived runtime identity before using it as evidence.
    pub fn serving_path_readiness(&self) -> NowledgeMemServingPathReadiness {
        NowledgeMemServingPathReadiness::embedded_store_handle()
    }

    pub fn refresh_runtime_resources(&self) -> Result<bool> {
        Ok(self.read_store()?.refresh_runtime_resources())
    }

    /// Persist the current graph generation through the admitted embedded
    /// maintenance path.
    ///
    /// Import and qualification hosts use this typed boundary instead of
    /// issuing a textual `CHECKPOINT` statement or reaching through the
    /// facade to the storage engine. The single maintenance admission's
    /// memory request is extended by the columnar shadow's builder-lifetime
    /// reservation, and that pre-admitted context travels into the
    /// checkpoint so the shadow build never issues a nested admission
    /// against the permit this method already holds.
    pub fn checkpoint(&self) -> Result<()> {
        let shadow_admission_bytes = self
            .read_store()?
            .graph
            .database()
            .columnar_shadow_admission_bytes()?;
        let _permit = self.admit_typed_maintenance(
            usize::try_from(shadow_admission_bytes).unwrap_or(usize::MAX),
            1,
        )?;
        self.write_store()?
            .graph_mut()
            .database_mut()
            .checkpoint_with_shadow_admission(crate::store::ColumnarShadowAdmission::pre_admitted(
                shadow_admission_bytes,
            ))
    }

    pub fn query_with_report(&self, cypher: &str) -> Result<NowledgeMemQueryOutput> {
        self.write_store()?.query_with_report(cypher)
    }

    pub fn set_telemetry_sink(&self, telemetry: Option<Arc<dyn TelemetrySink>>) -> Result<()> {
        self.write_store()?.set_telemetry_sink(telemetry);
        Ok(())
    }

    pub fn query_with_report_options(
        &self,
        cypher: &str,
        options: NowledgeMemQueryReportOptions,
    ) -> Result<NowledgeMemQueryOutput> {
        self.write_store()?
            .query_with_report_options(cypher, options)
    }

    pub fn query_with_params_with_report(
        &self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
    ) -> Result<NowledgeMemQueryOutput> {
        self.write_store()?
            .query_with_params_with_report(cypher, parameters)
    }

    /// Hydrates an explicitly bounded set of search-projection documents.
    ///
    /// Candidate search intentionally returns compact identities. Embedded
    /// hosts use this method for projection-owned payloads, such as source
    /// chunks, that have no authoritative graph node to hydrate from.
    pub fn search_projection_documents(
        &self,
        document_ids: &[String],
        max_documents: usize,
    ) -> Result<Vec<SearchDocument>> {
        Ok(self
            .search_projection_documents_with_report(document_ids, max_documents)?
            .documents)
    }

    pub fn search_projection_documents_with_report(
        &self,
        document_ids: &[String],
        max_documents: usize,
    ) -> Result<NowledgeMemSearchHydrationOutput> {
        if document_ids.len() > max_documents {
            return Err(HawDBError::Execution(format!(
                "search projection document hydration requested {} rows, limit is {max_documents}",
                document_ids.len()
            )));
        }
        let _permit = self.admit_typed_search()?;
        let store = self.read_store()?;
        let max_payload_bytes = store
            .graph
            .database()
            .config()
            .max_read_result_payload_bytes
            .ok_or_else(|| {
                HawDBError::Execution(
                    "admitted search hydration requires max_read_result_payload_bytes".to_string(),
                )
            })?;
        match (
            store.search_projection.as_ref(),
            store.out_of_core_search_projection.as_ref(),
        ) {
            (Some(projection), None) => {
                let mut documents = Vec::with_capacity(document_ids.len());
                let mut payload_bytes = 0usize;
                for id in document_ids {
                    let Some(document) = projection.index().document(id) else {
                        continue;
                    };
                    payload_bytes =
                        payload_bytes.saturating_add(search_document_payload_bytes(document));
                    if payload_bytes > max_payload_bytes {
                        return Err(HawDBError::read_budget_exceeded(
                            ReadBudgetResource::PayloadBytes,
                            max_payload_bytes,
                            format!(
                                "search projection document hydration produced {payload_bytes} payload bytes, limit is {max_payload_bytes}"
                            ),
                        ));
                    }
                    documents.push(document.clone());
                }
                Ok(NowledgeMemSearchHydrationOutput {
                    documents,
                    out_of_core_metrics: None,
                })
            }
            (None, Some(projection)) => {
                let output = projection.hydrate_documents(document_ids)?;
                if output.metrics.hydrated_bytes
                    > u64::try_from(max_payload_bytes).unwrap_or(u64::MAX)
                {
                    return Err(HawDBError::read_budget_exceeded(
                        ReadBudgetResource::PayloadBytes,
                        max_payload_bytes,
                        format!(
                            "search projection document hydration produced {} payload bytes, limit is {max_payload_bytes}",
                            output.metrics.hydrated_bytes
                        ),
                    ));
                }
                Ok(NowledgeMemSearchHydrationOutput {
                    documents: output.documents,
                    out_of_core_metrics: Some(output.metrics),
                })
            }
            (None, None) => Err(missing_search_projection_error()),
            (Some(_), Some(_)) => Err(ambiguous_search_projection_error()),
        }
    }
}

#[cfg(test)]
impl NowledgeMemEmbeddedStoreHandle {
    pub fn create_knowledge_memory_evolves_batch(
        &self,
        request: &KnowledgeMemoryEvolvesCreateBatchRequest,
    ) -> Result<KnowledgeMemoryEvolvesCreateBatchOutput> {
        let _permit = self.admit_typed_mutation()?;
        self.write_store()?
            .graph_mut()
            .database_mut()
            .create_knowledge_memory_evolves_batch(request)
    }

    pub fn update_knowledge_memory_lifecycle_batch(
        &self,
        request: &KnowledgeMemoryLifecycleBatchRequest,
    ) -> Result<KnowledgeMemoryLifecycleBatchOutput> {
        let _permit = self.admit_typed_mutation()?;
        self.write_store()?
            .graph_mut()
            .database_mut()
            .update_knowledge_memory_lifecycle_batch(request)
    }

    pub fn delete_knowledge_entity_batch(
        &self,
        request: &KnowledgeEntityDeleteBatchRequest,
    ) -> Result<KnowledgeEntityDeleteBatchOutput> {
        let _permit = self.admit_typed_mutation()?;
        self.write_store()?
            .graph_mut()
            .database_mut()
            .delete_knowledge_entity_batch(request)
    }
}

impl NowledgeMemEmbeddedStoreHandle {
    pub fn transaction(
        &self,
        statements: &[NowledgeGraphStatement],
    ) -> Result<crate::NowledgeGraphTransactionOutput> {
        let permit = self.admit_transaction(statements)?;
        let task_context = permit.bind_task_context(RuntimeTaskContext::default());
        let _permit = permit;
        let mut store = self.write_store()?;
        let db = store.graph_mut().database_mut();
        let mut transaction = db.begin_transaction_with_context(&task_context)?;
        let mut statement_outputs = Vec::with_capacity(statements.len());
        for statement in statements {
            statement_outputs
                .push(transaction.query_with_params(&statement.cypher, &statement.parameters)?);
        }
        let commit_output = transaction.commit()?;
        Ok(crate::NowledgeGraphTransactionOutput {
            statement_outputs,
            commit_output,
        })
    }

    /// Executes caller-owned Cypher and SQL statements in one canonical
    /// graph/relational transaction.
    ///
    /// The callback receives the ordinary query-first transaction surface;
    /// no route-specific mutation API or second durability boundary is
    /// introduced. Returning an error rolls back every staged statement.
    pub fn with_transaction<T>(
        &self,
        operation: impl FnOnce(&mut crate::DatabaseTransaction<'_>) -> Result<T>,
    ) -> Result<T> {
        let permit = self.admit_typed_transaction()?;
        let task_context = permit.bind_task_context(RuntimeTaskContext::default());
        let _permit = permit;
        let mut store = self.write_store()?;
        let mut transaction = store
            .graph_mut()
            .database_mut()
            .begin_transaction_with_context(&task_context)?;
        let result = match operation(&mut transaction) {
            Ok(output) => transaction.commit().map(|_| output),
            Err(error) => {
                transaction.rollback();
                Err(error)
            }
        };
        store.graph.database().poison_on_storage_error(&result);
        result
    }

    /// Executes a caller-owned group of bounded Cypher reads against one
    /// immutable graph snapshot.
    pub fn with_read_transaction<T>(
        &self,
        max_estimated_payload_bytes: usize,
        operation: impl FnOnce(&mut crate::DatabaseReadTransaction) -> Result<T>,
    ) -> Result<T> {
        let (_permit, mut transaction) =
            self.canonical_read_transaction(max_estimated_payload_bytes, None)?;
        let result = operation(&mut transaction);
        self.finish_canonical_read(result)
    }

    /// Executes App-owned Cypher and PostgreSQL reads against one immutable
    /// graph/relational snapshot while the search projection is pinned.
    /// Budgets are cumulative across every statement in the callback.
    pub fn with_bounded_read_snapshot<T>(
        &self,
        budget: NowledgeMemReadSnapshotBudget,
        operation: impl FnOnce(&mut NowledgeMemReadSnapshot<'_>) -> Result<T>,
    ) -> Result<T> {
        self.with_bounded_read_snapshot_context(budget, &RuntimeTaskContext::default(), operation)
    }

    /// Executes one bounded App read snapshot under the caller's cancellation,
    /// deadline, and resource reservation context.
    pub fn with_bounded_read_snapshot_context<T>(
        &self,
        budget: NowledgeMemReadSnapshotBudget,
        task_context: &RuntimeTaskContext,
        operation: impl FnOnce(&mut NowledgeMemReadSnapshot<'_>) -> Result<T>,
    ) -> Result<T> {
        self.with_bounded_read_snapshot_kind(budget, true, task_context, operation)
    }

    /// Executes bounded graph and relational reads from the published committed
    /// view. Acquisition and callback execution can overlap a staging writer.
    ///
    /// External search projections are deliberately unavailable: vector queries
    /// fail and the report contains no projection epochs. Use
    /// `with_bounded_read_snapshot` when a pinned projection is required.
    pub fn with_bounded_graph_read_snapshot<T>(
        &self,
        budget: NowledgeMemReadSnapshotBudget,
        operation: impl FnOnce(&mut NowledgeMemReadSnapshot<'_>) -> Result<T>,
    ) -> Result<T> {
        self.with_bounded_graph_read_snapshot_context(
            budget,
            &RuntimeTaskContext::default(),
            operation,
        )
    }

    /// Executes one published graph snapshot under the caller's cancellation,
    /// deadline, and resource reservation context.
    pub fn with_bounded_graph_read_snapshot_context<T>(
        &self,
        budget: NowledgeMemReadSnapshotBudget,
        task_context: &RuntimeTaskContext,
        operation: impl FnOnce(&mut NowledgeMemReadSnapshot<'_>) -> Result<T>,
    ) -> Result<T> {
        self.with_bounded_read_snapshot_kind(budget, false, task_context, operation)
    }

    fn with_bounded_read_snapshot_kind<T>(
        &self,
        budget: NowledgeMemReadSnapshotBudget,
        pin_projection: bool,
        task_context: &RuntimeTaskContext,
        operation: impl FnOnce(&mut NowledgeMemReadSnapshot<'_>) -> Result<T>,
    ) -> Result<T> {
        if budget.max_rows == 0 {
            return Err(HawDBError::Execution(
                "bounded read snapshot requires max_rows greater than zero".to_string(),
            ));
        }
        if budget.max_payload_bytes == 0 {
            return Err(HawDBError::Execution(
                "bounded read snapshot requires max_payload_bytes greater than zero".to_string(),
            ));
        }
        if !pin_projection {
            let (_permit, transaction, instance_identity) = self
                .canonical_read_transaction_with_instance(
                    budget.max_payload_bytes,
                    Some(budget.max_rows),
                    task_context,
                )?;
            let mut snapshot = NowledgeMemReadSnapshot::new(
                transaction,
                instance_identity,
                budget,
                None,
                None,
                _permit.bind_task_context(task_context.clone()),
            );
            return self.finish_canonical_read(operation(&mut snapshot));
        }
        let permit = self.admit_typed_read(budget.max_payload_bytes)?;
        let task_context = permit.bind_task_context(task_context.clone());
        let _permit = permit;
        let store = self.read_store()?;
        let configured_rows = store
            .graph
            .database()
            .config()
            .max_read_result_rows
            .ok_or_else(|| {
                HawDBError::Execution(
                    "bounded read snapshot requires max_read_result_rows".to_string(),
                )
            })?;
        if budget.max_rows > configured_rows {
            return Err(HawDBError::Execution(format!(
                "bounded read snapshot row budget {} exceeds configured limit {configured_rows}",
                budget.max_rows
            )));
        }
        let transaction = store
            .graph
            .database()
            .begin_read_transaction_with_context(&task_context)?;
        let mut snapshot = NowledgeMemReadSnapshot::new(
            transaction,
            store.graph.database().instance_identity()?,
            budget,
            store.search_projection.as_ref(),
            store.out_of_core_search_projection.as_ref(),
            task_context,
        );
        operation(&mut snapshot)
    }

    pub fn hawdb_lightning_initial_import_apply_with_document_identities(
        &self,
        encoded_graph_stream: &str,
        encoded_relational_stream: &[u8],
        manifest: &HawDBLightningBootstrapManifest,
        projection_freshness: Option<&SearchProjectionFreshness>,
        checkpoint: Option<&HawDBLightningInitialImportCheckpoint>,
        document_identities: &[HawDBLightningInitialImportDocumentIdentity],
    ) -> Result<HawDBLightningInitialImportApplyReport> {
        let estimated_input_bytes = encoded_graph_stream
            .len()
            .saturating_add(encoded_relational_stream.len())
            .saturating_add(
                document_identities
                    .iter()
                    .map(|identity| identity.document_id.len())
                    .sum::<usize>(),
            );
        let _permit = self.admit_typed_maintenance(estimated_input_bytes, 1)?;
        self.write_store()?
            .graph_mut()
            .database_mut()
            .hawdb_lightning_initial_import_apply_with_document_identities(
                encoded_graph_stream,
                encoded_relational_stream,
                manifest,
                projection_freshness,
                checkpoint,
                document_identities,
            )
    }

    /// Applies an externally materialized projection batch and makes it
    /// durable before returning. This is the library boundary used when Mem
    /// imports a legacy LanceDB projection without rebuilding embeddings.
    pub fn apply_search_projection_delta_and_checkpoint(
        &self,
        delta: SearchProjectionDelta,
    ) -> Result<SearchProjectionDeltaReport> {
        let permit =
            self.admit_search_projection_maintenance(search_projection_delta_bytes(&delta))?;
        let mut store = self.write_store()?;
        if store.incremental_search_enabled() {
            store.apply_incremental_search_delta(
                delta,
                permit.bind_task_context(RuntimeTaskContext::default()),
            )
        } else {
            store.apply_search_projection_delta_and_checkpoint(delta)
        }
    }

    /// Applies one externally materialized import batch, records its immutable
    /// source provenance, and checkpoints both before acknowledging success.
    pub fn apply_initial_import_projection_delta_and_checkpoint(
        &self,
        delta: SearchProjectionDelta,
        import_source_graph_commit_epoch: u64,
    ) -> Result<SearchProjectionDeltaReport> {
        let _permit = self.admit_typed_maintenance(search_projection_delta_bytes(&delta), 1)?;
        self.write_store()?
            .apply_initial_import_projection_delta_and_checkpoint(
                delta,
                import_source_graph_commit_epoch,
            )
    }

    pub fn query_with_params_with_report_options(
        &self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        options: NowledgeMemQueryReportOptions,
    ) -> Result<NowledgeMemQueryOutput> {
        self.write_store()?
            .query_with_params_with_report_options(cypher, parameters, options)
    }

    pub fn query_with_params_with_report_options_context(
        &self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        options: NowledgeMemQueryReportOptions,
        task_context: &RuntimeTaskContext,
    ) -> Result<NowledgeMemQueryOutput> {
        self.write_store()?
            .query_with_params_with_report_options_context(
                cypher,
                parameters,
                options,
                task_context,
            )
    }

    pub fn read_query(
        &self,
        cypher: &str,
        options: &NowledgeMemReadOptions,
    ) -> Result<NowledgeMemReadOutput> {
        self.read_store()?.read_query_with_options(cypher, options)
    }

    pub fn read_query_with_params(
        &self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        options: &NowledgeMemReadOptions,
    ) -> Result<NowledgeMemReadOutput> {
        self.read_store()?
            .read_query_with_params(cypher, parameters, options)
    }

    pub fn read_query_with_params_streaming(
        &self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        options: &NowledgeMemReadOptions,
        consumer: impl FnMut(BTreeMap<String, Value>) -> Result<()>,
    ) -> Result<QueryStreamReport> {
        self.read_store()?
            .read_query_with_params_streaming(cypher, parameters, options, consumer)
    }

    pub fn read_query_with_params_streaming_context(
        &self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        options: &NowledgeMemReadOptions,
        task_context: &RuntimeTaskContext,
        consumer: impl FnMut(BTreeMap<String, Value>) -> Result<()>,
    ) -> Result<QueryStreamReport> {
        self.read_store()?.read_query_with_params_streaming_context(
            cypher,
            parameters,
            options,
            task_context,
            consumer,
        )
    }

    pub fn read_query_with_params_streaming_collect(
        &self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        options: &NowledgeMemReadOptions,
    ) -> Result<NowledgeMemReadOutput> {
        self.read_store()?
            .read_query_with_params_streaming_collect(cypher, parameters, options)
    }

    pub fn graph_rag_schema_context(
        &self,
        options: GraphRagSchemaContextOptions,
    ) -> Result<GraphRagSchemaContext> {
        self.read_store()?.graph_rag_schema_context(options)
    }

    pub fn read_generated_graph_rag(
        &self,
        query: &GraphRagGeneratedQuery,
        parameters: &BTreeMap<String, Value>,
        options: &NowledgeMemReadOptions,
    ) -> Result<NowledgeMemReadOutput> {
        self.read_store()?
            .read_generated_graph_rag(query, parameters, options)
    }
}
impl NowledgeMemEmbeddedStoreHandle {
    pub fn search_candidates(
        &self,
        request: &NowledgeMemSearchCandidateRequest,
    ) -> Result<SearchResultSet> {
        let _permit = self.admit_typed_search()?;
        Ok(self.read_store()?.search_candidates(request)?.result)
    }

    pub fn search_candidates_with_report(
        &self,
        request: &NowledgeMemSearchCandidateRequest,
    ) -> Result<NowledgeMemSearchCandidateOutput> {
        let _permit = self.admit_typed_search()?;
        self.read_store()?.search_candidates(request)
    }

    pub fn validate_sampled_vector_recall(
        &self,
        options: VectorRecallValidationOptions,
    ) -> Result<VectorRecallValidationReport> {
        self.read_store()?.validate_sampled_vector_recall(options)
    }

    pub fn qualify_sampled_vector_recall_for_production(
        &self,
        options: VectorRecallValidationOptions,
        evidence_binding: crate::ProductionEvidenceBinding,
        expected_identity: crate::ProductionQualificationIdentity,
    ) -> Result<VectorRecallProductionQualificationReport> {
        self.read_store()?
            .qualify_sampled_vector_recall_for_production(
                options,
                evidence_binding,
                expected_identity,
            )
    }

    pub fn retrieve_knowledge(
        &self,
        request: &KnowledgeRetrievalRequest,
    ) -> Result<KnowledgeRetrievalOutput> {
        let _permit = self.admit_typed_search()?;
        self.read_store()?.retrieve_knowledge(request)
    }

    pub fn retrieve_knowledge_with_report(
        &self,
        request: &KnowledgeRetrievalRequest,
    ) -> Result<NowledgeMemRetrievalOutput> {
        let _permit = self.admit_typed_search()?;
        self.read_store()?.retrieve_knowledge_with_report(request)
    }

    pub fn search_candidate_readiness(
        &self,
        request: &NowledgeMemSearchCandidateRequest,
        options: &NowledgeMemSearchCandidateReadinessOptions,
    ) -> Result<NowledgeMemSearchCandidateReadinessReport> {
        let _permit = self.admit_typed_search()?;
        self.read_store()?
            .search_candidate_readiness(request, options)
    }

    pub fn search_candidate_shadow_evidence_json<I, S>(
        &self,
        request: &NowledgeMemSearchCandidateRequest,
        primary_candidate_ids: I,
    ) -> Result<serde_json::Value>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let _permit = self.admit_typed_search()?;
        self.read_store()?
            .search_candidate_shadow_evidence_json(request, primary_candidate_ids)
    }

    pub fn slow_query_report(&self) -> Result<NowledgeMemSlowQueryReport> {
        Ok(self.read_store()?.slow_query_report())
    }

    pub fn slow_query_report_json(&self) -> Result<serde_json::Value> {
        Ok(self.read_store()?.slow_query_report_json())
    }

    pub fn runtime_status(&self) -> Result<NowledgeMemRuntimeStatus> {
        self.read_store()?.runtime_status()
    }

    /// Current admitted database instance; does not retain a target fence.
    pub fn database_instance_identity(&self) -> Result<crate::DatabaseInstanceIdentity> {
        self.read_store()?.graph.database().instance_identity()
    }

    pub fn production_resource_profile(
        &self,
        statement: &NowledgeGraphStatement,
        limits: StorageResourceProfileLimits,
        evidence_binding: crate::ProductionEvidenceBinding,
        expected_identity: crate::ProductionQualificationIdentity,
    ) -> Result<StorageResourceProfileReport> {
        let store = self.read_store()?;
        let permit = store.graph.admit_streaming_query(
            &statement.cypher,
            &statement.parameters,
            limits.max_output_payload_bytes,
        )?;
        let task_context = permit.bind_task_context(RuntimeTaskContext::default());
        store
            .graph
            .database()
            .storage_resource_profile_for_production_with_context(
                &statement.cypher,
                &statement.parameters,
                limits,
                evidence_binding,
                expected_identity,
                &task_context,
            )
    }

    pub fn production_status(
        &self,
        route_ownership: Option<&NowledgeMemRouteOwnershipReadinessReport>,
    ) -> Result<NowledgeMemProductionStatus> {
        self.read_store()?.production_status(route_ownership)
    }

    pub fn production_status_json(
        &self,
        route_ownership: Option<&NowledgeMemRouteOwnershipReadinessReport>,
    ) -> Result<serde_json::Value> {
        self.read_store()?.production_status_json(route_ownership)
    }

    pub fn cutover_controls_report(
        &self,
        controls: NowledgeMemCutoverControls,
        route_ownership: Option<&NowledgeMemRouteOwnershipReadinessReport>,
    ) -> Result<NowledgeMemCutoverControlsReport> {
        self.read_store()?
            .cutover_controls_report(controls, route_ownership)
    }

    pub fn cutover_controls_report_with_initial_import_cutover_catch_up(
        &self,
        controls: NowledgeMemCutoverControls,
        route_ownership: Option<&NowledgeMemRouteOwnershipReadinessReport>,
        initial_import_cutover_catch_up: Option<&HawDBLightningInitialImportCutoverCatchUpReport>,
    ) -> Result<NowledgeMemCutoverControlsReport> {
        self.read_store()?
            .cutover_controls_report_with_initial_import_cutover_catch_up(
                controls,
                route_ownership,
                initial_import_cutover_catch_up,
            )
    }

    pub fn cutover_controls_report_with_initial_import_recovery(
        &self,
        controls: NowledgeMemCutoverControls,
        route_ownership: Option<&NowledgeMemRouteOwnershipReadinessReport>,
        initial_import_recovery: Option<&HawDBLightningInitialImportRecoveryReadinessReport>,
    ) -> Result<NowledgeMemCutoverControlsReport> {
        self.read_store()?
            .cutover_controls_report_with_initial_import_recovery(
                controls,
                route_ownership,
                initial_import_recovery,
            )
    }

    pub fn cutover_controls_report_json(
        &self,
        controls: NowledgeMemCutoverControls,
        route_ownership: Option<&NowledgeMemRouteOwnershipReadinessReport>,
    ) -> Result<serde_json::Value> {
        self.read_store()?
            .cutover_controls_report_json(controls, route_ownership)
    }

    pub fn cutover_controls_report_json_with_initial_import_cutover_catch_up(
        &self,
        controls: NowledgeMemCutoverControls,
        route_ownership: Option<&NowledgeMemRouteOwnershipReadinessReport>,
        initial_import_cutover_catch_up: Option<&HawDBLightningInitialImportCutoverCatchUpReport>,
    ) -> Result<serde_json::Value> {
        self.read_store()?
            .cutover_controls_report_json_with_initial_import_cutover_catch_up(
                controls,
                route_ownership,
                initial_import_cutover_catch_up,
            )
    }

    pub fn library_readiness(
        &self,
        options: &NowledgeMemReadinessOptions,
    ) -> Result<NowledgeMemLibraryReadinessReport> {
        self.read_store()?.library_readiness(options)
    }

    pub fn library_readiness_json(
        &self,
        options: &NowledgeMemReadinessOptions,
    ) -> Result<serde_json::Value> {
        self.read_store()?.library_readiness_json(options)
    }

    pub fn readiness_dashboard(
        &self,
        options: &NowledgeMemReadinessOptions,
    ) -> Result<NowledgeMemReadinessDashboard> {
        self.read_store()?.readiness_dashboard(options)
    }

    pub fn readiness_dashboard_json(
        &self,
        options: &NowledgeMemReadinessOptions,
    ) -> Result<serde_json::Value> {
        self.read_store()?.readiness_dashboard_json(options)
    }

    pub fn operations_readiness(
        &self,
        options: &NowledgeMemReadinessOptions,
    ) -> Result<NowledgeMemOperationsReadinessReport> {
        self.read_store()?.operations_readiness(options)
    }

    pub fn operations_readiness_json(
        &self,
        options: &NowledgeMemReadinessOptions,
    ) -> Result<serde_json::Value> {
        self.read_store()?.operations_readiness_json(options)
    }

    pub fn query_runtime_preflight(
        &self,
        probes: &[NowledgeQueryRuntimePreflightProbe],
    ) -> Result<NowledgeQueryRuntimePreflightReport> {
        Ok(self.write_store()?.query_runtime_preflight(probes))
    }

    pub fn query_runtime_preflight_json(
        &self,
        probes: &[NowledgeQueryRuntimePreflightProbe],
    ) -> Result<serde_json::Value> {
        Ok(self.write_store()?.query_runtime_preflight_json(probes))
    }

    pub fn catch_up_search_projection(
        &self,
        max_operations_per_batch: usize,
        max_batches: usize,
    ) -> Result<SearchProjectionCatchUpReport> {
        let estimated_operations = max_operations_per_batch.saturating_mul(max_batches);
        let estimated_input_bytes =
            estimated_operations.saturating_mul(SEARCH_PROJECTION_CHANGEFEED_OPERATION_BYTES);
        let _permit = self.admit_typed_maintenance(estimated_input_bytes, 1)?;
        self.write_store()?
            .catch_up_search_projection(max_operations_per_batch, max_batches)
    }

    /// Runs bounded unified graph-and-relational projection catch-up through
    /// the embedded library handle.
    pub fn catch_up_search_projection_with_relational<F>(
        &self,
        max_operations_per_batch: usize,
        max_batches: usize,
        relational_hydrator: F,
    ) -> Result<SearchProjectionCatchUpReport>
    where
        F: FnMut(
            &mut DatabaseReadTransaction,
            &SearchProjectionChangeBatch,
        ) -> Result<SearchProjectionRelationalDelta>,
    {
        let estimated_operations = max_operations_per_batch.saturating_mul(max_batches);
        let estimated_input_bytes =
            estimated_operations.saturating_mul(SEARCH_PROJECTION_CHANGEFEED_OPERATION_BYTES);
        let _permit = self.admit_typed_maintenance(estimated_input_bytes, 1)?;
        self.write_store()?
            .catch_up_search_projection_with_relational(
                max_operations_per_batch,
                max_batches,
                relational_hydrator,
            )
    }

    /// Runs bounded unified projection catch-up through the embedded library
    /// handle and invokes the host hydrator for every selected batch.
    pub fn catch_up_search_projection_with_batch_hydrator<F>(
        &self,
        max_change_operations_per_batch: usize,
        max_projection_operations_per_batch: usize,
        max_batches: usize,
        batch_hydrator: F,
    ) -> Result<SearchProjectionCatchUpReport>
    where
        F: FnMut(
            &mut DatabaseReadTransaction,
            &SearchProjectionChangeBatch,
        ) -> Result<SearchProjectionRelationalDelta>,
    {
        let estimated_operations = max_change_operations_per_batch
            .max(max_projection_operations_per_batch)
            .saturating_mul(max_batches);
        let estimated_input_bytes =
            estimated_operations.saturating_mul(SEARCH_PROJECTION_CHANGEFEED_OPERATION_BYTES);
        let permit = self.admit_search_projection_maintenance(estimated_input_bytes)?;
        let mut store = self.write_store()?;
        if store.incremental_search_enabled() {
            store.catch_up_incremental_search_with_batch_hydrator(
                max_change_operations_per_batch,
                max_projection_operations_per_batch,
                max_batches,
                batch_hydrator,
                permit.bind_task_context(RuntimeTaskContext::default()),
            )
        } else {
            store.catch_up_search_projection_with_batch_hydrator(
                max_change_operations_per_batch,
                max_projection_operations_per_batch,
                max_batches,
                batch_hydrator,
            )
        }
    }

    pub fn search_projection_changefeed_readiness(
        &self,
        require_restart_recoverable: bool,
        max_operations: Option<usize>,
    ) -> Result<SearchProjectionChangefeedReadiness> {
        self.read_store()?
            .search_projection_changefeed_readiness(require_restart_recoverable, max_operations)
    }

    pub fn catch_up_search_projection_scheduled(
        &self,
        max_operations_per_batch: usize,
        max_batches: usize,
    ) -> Result<ScheduledSearchProjectionCatchUpReport> {
        let estimated_operations = max_operations_per_batch.saturating_mul(max_batches);
        let estimated_input_bytes =
            estimated_operations.saturating_mul(SEARCH_PROJECTION_CHANGEFEED_OPERATION_BYTES);
        let _permit = self.admit_typed_maintenance(estimated_input_bytes, 1)?;
        self.write_store()?
            .catch_up_search_projection_scheduled(max_operations_per_batch, max_batches)
    }

    #[cfg(test)]
    fn admit_typed_mutation(&self) -> Result<RuntimePermit> {
        let store = self.read_store()?;
        let config = store.graph.database().config();
        let estimated_memory_bytes = crate::executor::estimated_mutation_memory_bytes(
            config.mutation_limits,
            config.max_wal_record_bytes,
        );
        store
            .graph
            .try_admit_runtime(RuntimeWorkRequest::foreground_mutation(
                estimated_memory_bytes,
            ))
    }

    fn admit_typed_transaction(&self) -> Result<RuntimePermit> {
        let store = self.read_store()?;
        let config = store.graph.database().config();
        let memory_bytes = crate::executor::estimated_mutation_memory_bytes(
            config.mutation_limits,
            config.max_wal_record_bytes,
        )
        .max(u64::try_from(config.execution_memory.query_memory_bytes.get()).unwrap_or(u64::MAX));
        let result_bytes = u64::try_from(
            config
                .max_read_result_payload_bytes
                .unwrap_or(config.mutation_limits.max_result_payload_bytes.get()),
        )
        .unwrap_or(u64::MAX);
        store.graph.try_admit_runtime(
            RuntimeWorkRequest::new(RuntimeWorkPriority::Foreground, RuntimeWorkKind::Mutation)
                .with_cpu_slots(1)
                .with_memory_bytes(memory_bytes)
                .with_result_bytes(result_bytes)
                .with_blocking(true),
        )
    }

    fn admit_typed_read(&self, max_estimated_payload_bytes: usize) -> Result<RuntimePermit> {
        let store = self.read_store()?;
        let request =
            typed_read_request(store.graph.database().config(), max_estimated_payload_bytes)?;
        store.graph.try_admit_runtime(request)
    }

    fn lock_published_read(&self) -> Result<MutexGuard<'_, Option<PublishedCanonicalRead>>> {
        // A failed or panicking write guard leaves the last-good value in
        // place but poisons future reads, since that value no longer proves
        // it reflects the most recent committed write.
        if self.read_publication_failed.load(Ordering::Acquire) {
            return Err(HawDBError::Execution(
                "nowledge mem embedded store read lock poisoned".to_string(),
            ));
        }
        self.published_read.lock().map_err(|_| {
            HawDBError::Execution("embedded snapshot publication lock poisoned".to_string())
        })
    }

    fn canonical_read_transaction(
        &self,
        max_estimated_payload_bytes: usize,
        max_rows: Option<usize>,
    ) -> Result<(RuntimePermit, crate::DatabaseReadTransaction)> {
        self.canonical_read_transaction_with_context(
            max_estimated_payload_bytes,
            max_rows,
            &RuntimeTaskContext::default(),
        )
    }

    fn canonical_read_transaction_with_context(
        &self,
        max_estimated_payload_bytes: usize,
        max_rows: Option<usize>,
        task_context: &RuntimeTaskContext,
    ) -> Result<(RuntimePermit, crate::DatabaseReadTransaction)> {
        self.canonical_read_transaction_with_instance(
            max_estimated_payload_bytes,
            max_rows,
            task_context,
        )
        .map(|(permit, transaction, _)| (permit, transaction))
    }

    fn canonical_read_transaction_with_instance(
        &self,
        max_estimated_payload_bytes: usize,
        max_rows: Option<usize>,
        task_context: &RuntimeTaskContext,
    ) -> Result<(
        RuntimePermit,
        crate::DatabaseReadTransaction,
        crate::DatabaseInstanceIdentity,
    )> {
        // Admission emits host telemetry. Never invoke it while holding the
        // publication lock: a sink may reenter the embedded handle.
        let (governor, request) = {
            let published = self.lock_published_read()?;
            let published = published.as_ref().ok_or_else(|| {
                HawDBError::Execution("nowledge mem embedded store read lock poisoned".to_string())
            })?;
            published.snapshot.ensure_usable()?;
            (
                published.governor.clone(),
                typed_read_request(published.snapshot.config(), max_estimated_payload_bytes)?,
            )
        };
        let permit = governor.try_admit(request).map_err(|error| {
            if error.is_retryable() {
                governor.record_admission_wait(request, error.code, 0);
            }
            HawDBError::Execution(error.to_string())
        })?;
        // Capture after admission so a write completed during admission is visible.
        // This guard must also drop before the permit on every error path.
        let published = self.lock_published_read()?;
        let published = published.as_ref().ok_or_else(|| {
            HawDBError::Execution("nowledge mem embedded store read lock poisoned".to_string())
        })?;
        published.snapshot.ensure_usable()?;
        let config = published.snapshot.config();
        if let Some(max_rows) = max_rows {
            let configured_rows = config.max_read_result_rows.ok_or_else(|| {
                HawDBError::Execution(
                    "bounded read snapshot requires max_read_result_rows".to_string(),
                )
            })?;
            if max_rows > configured_rows {
                return Err(HawDBError::Execution(format!(
                    "bounded read snapshot row budget {max_rows} exceeds configured limit {configured_rows}"
                )));
            }
        }
        let task_context = permit.bind_task_context(task_context.clone());
        let transaction = published.snapshot.begin_read_transaction(&task_context)?;
        Ok((permit, transaction, published.instance_identity.clone()))
    }

    fn finish_canonical_read<T>(&self, result: Result<T>) -> Result<T> {
        if result.is_ok() {
            let published = self.lock_published_read()?;
            let published = published.as_ref().ok_or_else(|| {
                HawDBError::Execution("nowledge mem embedded store read lock poisoned".to_string())
            })?;
            published.snapshot.ensure_usable()?;
        }
        result
    }

    fn admit_typed_maintenance(
        &self,
        estimated_input_bytes: usize,
        io_slots: usize,
    ) -> Result<RuntimePermit> {
        let store = self.read_store()?;
        let config = store.graph.database().config();
        let working_memory_bytes =
            u64::try_from(config.execution_memory.blocking_operator_bytes.get())
                .unwrap_or(u64::MAX);
        let estimated_input_bytes = u64::try_from(estimated_input_bytes).unwrap_or(u64::MAX);
        store.graph.try_admit_runtime(
            RuntimeWorkRequest::background_maintenance(
                working_memory_bytes.saturating_add(estimated_input_bytes),
            )
            .with_io_slots(io_slots)
            .with_blocking(true),
        )
    }

    fn admit_typed_search(&self) -> Result<RuntimePermit> {
        let store = self.read_store()?;
        let config = store.graph.database().config();
        let result_bytes = config
            .max_read_result_payload_bytes
            .ok_or_else(|| {
                HawDBError::Execution(
                    "admitted typed search requires max_read_result_payload_bytes".to_string(),
                )
            })
            .and_then(|bytes| {
                u64::try_from(bytes).map_err(|_| {
                    HawDBError::Execution(
                        "admitted typed search result budget exceeds u64".to_string(),
                    )
                })
            })?;
        let working_memory_bytes =
            u64::try_from(config.execution_memory.blocking_operator_bytes.get())
                .unwrap_or(u64::MAX);
        let memory_bytes = store.search_memory_bytes(working_memory_bytes, result_bytes)?;
        let io_slots = store.search_io_slots()?;
        store.graph.try_admit_runtime(
            RuntimeWorkRequest::foreground_query(memory_bytes, result_bytes)
                .with_io_slots(io_slots)
                .with_blocking(true),
        )
    }

    fn admit_transaction(&self, statements: &[NowledgeGraphStatement]) -> Result<RuntimePermit> {
        let store = self.read_store()?;
        let db = store.graph.database();
        let config = db.config();
        let mut priority = RuntimeWorkPriority::Background;
        let mut kind = RuntimeWorkKind::Query;
        let mut estimated_memory_bytes = TYPED_CONTROL_STATEMENT_MEMORY_BYTES;
        let mut result_bytes = 0u64;
        let mut io_slots = 0usize;
        for statement in statements {
            let admission = db.runtime_admission_plan(&statement.cypher, &statement.parameters)?;
            if admission.work_request.priority == WorkPriority::Foreground {
                priority = RuntimeWorkPriority::Foreground;
            }
            if admission.is_mutation {
                kind = RuntimeWorkKind::Mutation;
            }
            estimated_memory_bytes = estimated_memory_bytes.max(admission.estimated_memory_bytes);
            io_slots = io_slots.max(admission.required_io_slots);
            let statement_result_bytes = if admission.is_mutation {
                config.mutation_limits.max_result_payload_bytes.get()
            } else {
                config.max_read_result_payload_bytes.ok_or_else(|| {
                    HawDBError::Execution(
                        "admitted transaction requires max_read_result_payload_bytes".to_string(),
                    )
                })?
            };
            result_bytes = result_bytes
                .saturating_add(u64::try_from(statement_result_bytes).unwrap_or(u64::MAX));
        }
        let request = RuntimeWorkRequest::new(priority, kind)
            .with_cpu_slots(1)
            .with_memory_bytes(estimated_memory_bytes)
            .with_io_slots(io_slots)
            .with_result_bytes(result_bytes)
            .with_blocking(true);
        store.graph.try_admit_runtime(request)
    }

    fn read_store(&self) -> Result<RwLockReadGuard<'_, NowledgeMemEmbeddedStore>> {
        self.inner.read().map_err(|_| {
            HawDBError::Execution("nowledge mem embedded store read lock poisoned".to_string())
        })
    }

    fn write_store(&self) -> Result<EmbeddedStoreWriteGuard<'_>> {
        let store = self.inner.write().map_err(|_| {
            HawDBError::Execution("nowledge mem embedded store write lock poisoned".to_string())
        })?;
        Ok(EmbeddedStoreWriteGuard {
            store,
            published: &self.published_read,
            read_publication_failed: &self.read_publication_failed,
        })
    }
}

fn typed_read_request(
    config: &DatabaseConfig,
    max_estimated_payload_bytes: usize,
) -> Result<RuntimeWorkRequest> {
    let configured_result_bytes = config.max_read_result_payload_bytes.ok_or_else(|| {
        HawDBError::Execution(
            "admitted typed read requires max_read_result_payload_bytes".to_string(),
        )
    })?;
    if max_estimated_payload_bytes > configured_result_bytes {
        return Err(HawDBError::Execution(format!(
                "typed read payload budget {max_estimated_payload_bytes} exceeds configured limit {configured_result_bytes}"
            )));
    }
    let result_bytes = u64::try_from(configured_result_bytes).unwrap_or(u64::MAX);
    let working_memory_bytes =
        u64::try_from(config.execution_memory.query_memory_bytes.get()).unwrap_or(u64::MAX);
    Ok(
        RuntimeWorkRequest::foreground_query(working_memory_bytes, result_bytes)
            .with_io_slots(1)
            .with_blocking(true),
    )
}

fn search_projection_delta_bytes(delta: &SearchProjectionDelta) -> usize {
    let upsert_bytes = delta.upserts.iter().fold(0usize, |total, row| {
        let embedding_bytes = row.embedding.as_ref().map_or(0, |embedding| {
            embedding.len().saturating_mul(size_of::<f32>())
        });
        let metadata_bytes = row
            .metadata
            .iter()
            .fold(0usize, |metadata_total, (key, value)| {
                metadata_total
                    .saturating_add(key.len())
                    .saturating_add(value.len())
            });
        total
            .saturating_add(size_of::<crate::SearchProjectionRow>())
            .saturating_add(row.external_id.len())
            .saturating_add(row.title.len())
            .saturating_add(row.body.len())
            .saturating_add(row.source_id.as_ref().map_or(0, String::len))
            .saturating_add(embedding_bytes)
            .saturating_add(metadata_bytes)
    });
    delta.deletes.iter().fold(upsert_bytes, |total, id| {
        total
            .saturating_add(size_of::<String>())
            .saturating_add(id.len())
    })
}

fn search_document_payload_bytes(document: &SearchDocument) -> usize {
    let embedding_bytes = document.embedding.as_ref().map_or(0, |embedding| {
        embedding.len().saturating_mul(size_of::<f32>())
    });
    let metadata_bytes = document
        .metadata
        .iter()
        .fold(0usize, |total, (key, value)| {
            total.saturating_add(key.len()).saturating_add(value.len())
        });
    size_of::<SearchDocument>()
        .saturating_add(document.id.len())
        .saturating_add(document.title.len())
        .saturating_add(document.content.len())
        .saturating_add(embedding_bytes)
        .saturating_add(metadata_bytes)
}

impl NowledgeMemEmbeddedStore {
    pub fn new(
        graph: NowledgeMemGraph,
        mut search_projection: Option<NowledgeMemSearchProjection>,
    ) -> Self {
        if let Some(projection) = search_projection.as_mut() {
            let mut range_read_config = projection.index().range_read_config();
            range_read_config.io_depth = range_read_config
                .io_depth
                .min(graph.runtime_governor_snapshot().limits.foreground_io_depth);
            projection.set_range_read_config(range_read_config);
        }
        Self {
            graph,
            search_projection,
            out_of_core_search_projection: None,
            retrieval_projection_advisor: NowledgeMemRetrievalProjectionAdvisor::default(),
        }
    }

    fn new_with_out_of_core_search(
        graph: NowledgeMemGraph,
        out_of_core_search_projection: NowledgeMemOutOfCoreSearchProjection,
        retrieval_projection_advisor: NowledgeMemRetrievalProjectionAdvisor,
    ) -> Self {
        Self {
            graph,
            search_projection: None,
            out_of_core_search_projection: Some(out_of_core_search_projection),
            retrieval_projection_advisor,
        }
    }

    pub fn open_with_options(
        options: NowledgeMemOpenOptions,
    ) -> Result<(Self, NowledgeMemOpenReport)> {
        let graph_config = options.effective_database_config();
        let runtime_governor =
            default_nowledge_mem_runtime_governor(&options.graph_path, &graph_config);
        Self::open_with_options_and_runtime_governor(options, runtime_governor)
    }

    pub fn open_with_options_and_runtime_governor(
        options: NowledgeMemOpenOptions,
        runtime_governor: RuntimeGovernor,
    ) -> Result<(Self, NowledgeMemOpenReport)> {
        options.validate()?;
        let mut report = options.sanitized_report();
        let graph_config = options.effective_database_config();
        let mut graph = NowledgeMemGraph::open_with_config_and_runtime_governor(
            &options.graph_path,
            graph_config,
            runtime_governor,
        )?;
        for registry in &options.system_schema_registries {
            report.system_schema_upgrades.push(
                graph
                    .database_mut()
                    .apply_system_schema_registry(registry)?,
            );
        }
        report.graph_opened = true;
        let default_search_range_read_config = SearchRangeReadConfig {
            io_depth: graph.runtime_governor_snapshot().limits.foreground_io_depth,
            ..SearchRangeReadConfig::default()
        };
        let Some(path) = options.search_projection_path.as_ref() else {
            return Ok((Self::new(graph, None), report));
        };
        match &options.search_projection_open_mode {
            NowledgeMemSearchProjectionOpenMode::FullResidencyMaintenance => {
                let mut projection = NowledgeMemSearchProjection::open(path)?;
                projection.set_range_read_config(
                    options
                        .search_range_read_config
                        .unwrap_or(default_search_range_read_config),
                );
                report.search_projection_opened = true;
                Ok((Self::new(graph, Some(projection)), report))
            }
            NowledgeMemSearchProjectionOpenMode::IncrementalMaintenance(maintenance) => {
                let projection = NowledgeMemOutOfCoreSearchProjection::open_incremental(
                    path,
                    (**maintenance).clone(),
                )?;
                report.search_projection_opened = true;
                Ok((
                    Self::new_with_out_of_core_search(
                        graph,
                        projection,
                        options.retrieval_projection_advisor.clone(),
                    ),
                    report,
                ))
            }
            NowledgeMemSearchProjectionOpenMode::QualifiedOutOfCore(qualified) => {
                let graph_commit_epoch = graph.database().commit_epoch()?;
                if graph_commit_epoch != qualified.expected_identity.canonical_graph_commit_epoch {
                    return Err(HawDBError::Storage(format!(
                        "qualified out-of-core search expected canonical graph commit epoch {}, opened graph is at {graph_commit_epoch}",
                        qualified.expected_identity.canonical_graph_commit_epoch
                    )));
                }
                let projection = NowledgeMemOutOfCoreSearchProjection::open_production_with_config(
                    path,
                    qualified.config.clone(),
                    &qualified.qualification,
                    &qualified.expected_identity,
                )?;
                report.search_projection_opened = true;
                report.search_production_qualification_bound = true;
                Ok((
                    Self::new_with_out_of_core_search(
                        graph,
                        projection,
                        options.retrieval_projection_advisor.clone(),
                    ),
                    report,
                ))
            }
        }
    }

    pub fn graph(&self) -> &NowledgeMemGraph {
        &self.graph
    }

    pub fn graph_mut(&mut self) -> &mut NowledgeMemGraph {
        &mut self.graph
    }

    pub fn runtime_governor_snapshot(&self) -> RuntimeGovernorSnapshot {
        self.graph.runtime_governor_snapshot()
    }

    pub fn refresh_runtime_resources(&self) -> bool {
        self.graph.refresh_runtime_resources()
    }

    pub fn search_projection(&self) -> Option<&NowledgeMemSearchProjection> {
        self.search_projection.as_ref()
    }

    pub fn search_projection_mut(&mut self) -> Option<&mut NowledgeMemSearchProjection> {
        self.search_projection.as_mut()
    }

    pub fn out_of_core_search_projection(&self) -> Option<&NowledgeMemOutOfCoreSearchProjection> {
        self.out_of_core_search_projection.as_ref()
    }

    /// Applies a projection delta and checkpoints it before acknowledging
    /// success to the host. A checkpoint failure is returned to the caller,
    /// leaving the host free to retry its idempotent batch.
    pub fn apply_search_projection_delta_and_checkpoint(
        &mut self,
        delta: SearchProjectionDelta,
    ) -> Result<SearchProjectionDeltaReport> {
        if self.incremental_search_enabled() {
            let permit =
                self.admit_incremental_search_maintenance(search_projection_delta_bytes(&delta))?;
            return self.apply_incremental_search_delta(
                delta,
                permit.bind_task_context(RuntimeTaskContext::default()),
            );
        }
        let projection = require_search_projection_mut(&mut self.search_projection)?;
        let report = projection.index_mut().apply_projection_delta(delta)?;
        projection.index().checkpoint()?;
        Ok(report)
    }

    pub fn apply_initial_import_projection_delta_and_checkpoint(
        &mut self,
        delta: SearchProjectionDelta,
        import_source_graph_commit_epoch: u64,
    ) -> Result<SearchProjectionDeltaReport> {
        // The frozen legacy source epoch is provenance only. The database import
        // created a new local WAL history, so the projection cursor must be
        // stamped from that local history before it is made durable.
        let local_graph_commit_epoch = self.graph.database().commit_epoch()?;
        let projection = require_search_projection_mut(&mut self.search_projection)?;
        projection
            .index()
            .validate_import_source_graph_commit_epoch(import_source_graph_commit_epoch)?;
        let report = projection
            .index_mut()
            .apply_projection_delta(SearchProjectionDelta {
                source_graph_commit_epoch: Some(local_graph_commit_epoch),
                ..delta
            })?;
        projection
            .index_mut()
            .record_import_source_graph_commit_epoch(import_source_graph_commit_epoch)?;
        projection.index().checkpoint()?;
        Ok(report)
    }

    pub fn set_telemetry_sink(&mut self, telemetry: Option<Arc<dyn TelemetrySink>>) {
        self.graph
            .database_mut()
            .set_telemetry_sink(telemetry.clone());
        if let Some(search_projection) = &mut self.search_projection {
            search_projection.set_telemetry_sink(telemetry);
        }
    }

    pub fn storage_recovery_report(&self) -> Result<NowledgeMemStorageRecoveryReport> {
        Ok({
            NowledgeMemStorageRecoveryReport::from_storage_report(
                &self.graph.database().storage_recovery_report()?,
            )
        })
    }

    pub fn storage_recovery_report_json(&self) -> Result<serde_json::Value> {
        Ok(self.storage_recovery_report()?.json())
    }

    pub fn production_resource_profile(
        &self,
        statement: &NowledgeGraphStatement,
        limits: StorageResourceProfileLimits,
        evidence_binding: crate::ProductionEvidenceBinding,
        expected_identity: crate::ProductionQualificationIdentity,
    ) -> Result<StorageResourceProfileReport> {
        self.graph
            .database()
            .storage_resource_profile_for_production(
                &statement.cypher,
                &statement.parameters,
                limits,
                evidence_binding,
                expected_identity,
            )
    }

    pub fn storage_lifecycle_decision(&self) -> Result<NowledgeMemStorageLifecycleDecision> {
        Ok({
            NowledgeMemStorageLifecycleDecision::from_storage_recovery(
                self.storage_recovery_report()?,
            )
        })
    }

    pub fn storage_lifecycle_decision_json(&self) -> Result<serde_json::Value> {
        Ok(self.storage_lifecycle_decision()?.json())
    }

    pub fn runtime_status(&self) -> Result<NowledgeMemRuntimeStatus> {
        Ok({
            let graph_commit_epoch = self.graph.database().commit_epoch()?;
            NowledgeMemRuntimeStatus {
                protocol: NOWLEDGE_MEM_RUNTIME_STATUS_PROTOCOL.to_string(),
                graph_commit_epoch,
                changefeed: self
                    .graph
                    .database()
                    .search_projection_changefeed_status()?,
                projection_freshness: self.search_projection_freshness(),
                file_descriptors: self.graph.database().file_descriptor_metrics(),
            }
        })
    }

    pub fn production_status(
        &self,
        route_ownership: Option<&NowledgeMemRouteOwnershipReadinessReport>,
    ) -> Result<NowledgeMemProductionStatus> {
        Ok({
            NowledgeMemProductionStatus::from_runtime(
                self.graph.mode(),
                self.graph.database().config().read_only,
                self.runtime_status()?,
                route_ownership.cloned(),
            )
        })
    }

    pub fn production_status_json(
        &self,
        route_ownership: Option<&NowledgeMemRouteOwnershipReadinessReport>,
    ) -> Result<serde_json::Value> {
        Ok(self.production_status(route_ownership)?.json())
    }

    pub fn cutover_controls_report(
        &self,
        controls: NowledgeMemCutoverControls,
        route_ownership: Option<&NowledgeMemRouteOwnershipReadinessReport>,
    ) -> Result<NowledgeMemCutoverControlsReport> {
        Ok({
            self.cutover_controls_report_with_initial_import_cutover_catch_up(
                controls,
                route_ownership,
                None,
            )?
        })
    }

    pub fn cutover_controls_report_with_initial_import_cutover_catch_up(
        &self,
        controls: NowledgeMemCutoverControls,
        route_ownership: Option<&NowledgeMemRouteOwnershipReadinessReport>,
        initial_import_cutover_catch_up: Option<&HawDBLightningInitialImportCutoverCatchUpReport>,
    ) -> Result<NowledgeMemCutoverControlsReport> {
        Ok({
            NowledgeMemCutoverControlsReport::from_production_status(
                controls,
                self.production_status(route_ownership)?,
                initial_import_cutover_catch_up.is_some_and(|report| report.ready),
            )
        })
    }

    pub fn cutover_controls_report_with_initial_import_recovery(
        &self,
        controls: NowledgeMemCutoverControls,
        route_ownership: Option<&NowledgeMemRouteOwnershipReadinessReport>,
        initial_import_recovery: Option<&HawDBLightningInitialImportRecoveryReadinessReport>,
    ) -> Result<NowledgeMemCutoverControlsReport> {
        Ok({
            let recovery_ready = initial_import_recovery.is_some_and(|report| {
                report.ready
                    && report.startup.readiness.ready_for_cutover
                    && report
                        .startup
                        .cutover_catch_up
                        .as_ref()
                        .is_some_and(|catch_up| catch_up.ready)
            });
            let mut report = NowledgeMemCutoverControlsReport::from_production_status(
                controls,
                self.production_status(route_ownership)?,
                recovery_ready,
            );
            if report.initial_import_enabled && !recovery_ready {
                report.ready = false;
                report
                    .blocker_codes
                    .push("initial_import_recovery_not_ready".to_string());
                report.blocker_codes.sort();
                report.blocker_codes.dedup();
            }
            report
        })
    }

    pub fn cutover_controls_report_json(
        &self,
        controls: NowledgeMemCutoverControls,
        route_ownership: Option<&NowledgeMemRouteOwnershipReadinessReport>,
    ) -> Result<serde_json::Value> {
        Ok({
            self.cutover_controls_report(controls, route_ownership)?
                .json()
        })
    }

    pub fn cutover_controls_report_json_with_initial_import_cutover_catch_up(
        &self,
        controls: NowledgeMemCutoverControls,
        route_ownership: Option<&NowledgeMemRouteOwnershipReadinessReport>,
        initial_import_cutover_catch_up: Option<&HawDBLightningInitialImportCutoverCatchUpReport>,
    ) -> Result<serde_json::Value> {
        Ok({
            self.cutover_controls_report_with_initial_import_cutover_catch_up(
                controls,
                route_ownership,
                initial_import_cutover_catch_up,
            )?
            .json()
        })
    }

    pub fn build_search_projection_graph_delta_request_from_freshness(
        &self,
        max_operations: Option<usize>,
    ) -> Result<Option<SearchProjectionGraphDeltaRequest>> {
        let search_projection = self.require_search_projection()?;
        self.graph
            .database()
            .build_search_projection_graph_delta_request_from_freshness(
                search_projection.index(),
                max_operations,
            )
    }

    pub fn search_projection_changefeed_readiness(
        &self,
        require_restart_recoverable: bool,
        max_operations: Option<usize>,
    ) -> Result<SearchProjectionChangefeedReadiness> {
        if self.incremental_search_enabled() {
            let freshness = self.search_projection_freshness().ok_or_else(|| {
                HawDBError::Storage("incremental search reader is detached".into())
            })?;
            return Ok(self
                .graph
                .database()
                .search_projection_changefeed_status()?
                .readiness_after(
                    freshness.source_graph_commit_epoch,
                    freshness.durable_source_graph_commit_epoch,
                    require_restart_recoverable,
                    max_operations,
                ));
        }
        let search_projection = self.require_search_projection()?;
        self.graph
            .database()
            .search_projection_changefeed_readiness(
                search_projection.index(),
                require_restart_recoverable,
                max_operations,
            )
    }

    pub fn search_projection_graph_delta_background_work_plan(
        &self,
        request: &SearchProjectionGraphDeltaRequest,
        hint: BackgroundWorkHint,
    ) -> Result<Option<BackgroundWorkPlan>> {
        let Some(search_projection) = self.search_projection.as_ref() else {
            return Ok(None);
        };
        self.graph
            .database()
            .search_projection_graph_delta_freshness_background_work_plan(
                search_projection.index(),
                request,
                hint,
            )
    }

    pub fn apply_search_projection_graph_delta(
        &mut self,
        request: SearchProjectionGraphDeltaRequest,
    ) -> Result<SearchProjectionDeltaReport> {
        let Self {
            graph,
            search_projection,
            ..
        } = self;
        let search_projection = require_search_projection_mut(search_projection)?;
        graph
            .database()
            .apply_search_projection_graph_delta(search_projection.index_mut(), request)
    }

    pub fn catch_up_search_projection(
        &mut self,
        max_operations_per_batch: usize,
        max_batches: usize,
    ) -> Result<SearchProjectionCatchUpReport> {
        let Self {
            graph,
            search_projection,
            ..
        } = self;
        let search_projection = require_search_projection_mut(search_projection)?;
        graph.database().catch_up_search_projection(
            search_projection.index_mut(),
            max_operations_per_batch,
            max_batches,
        )
    }

    pub fn catch_up_search_projection_with_relational<F>(
        &mut self,
        max_operations_per_batch: usize,
        max_batches: usize,
        relational_hydrator: F,
    ) -> Result<SearchProjectionCatchUpReport>
    where
        F: FnMut(
            &mut DatabaseReadTransaction,
            &SearchProjectionChangeBatch,
        ) -> Result<SearchProjectionRelationalDelta>,
    {
        let Self {
            graph,
            search_projection,
            ..
        } = self;
        let search_projection = require_search_projection_mut(search_projection)?;
        graph.database().catch_up_search_projection_with_relational(
            search_projection.index_mut(),
            max_operations_per_batch,
            max_batches,
            relational_hydrator,
        )
    }

    pub fn catch_up_search_projection_with_batch_hydrator<F>(
        &mut self,
        max_change_operations_per_batch: usize,
        max_projection_operations_per_batch: usize,
        max_batches: usize,
        batch_hydrator: F,
    ) -> Result<SearchProjectionCatchUpReport>
    where
        F: FnMut(
            &mut DatabaseReadTransaction,
            &SearchProjectionChangeBatch,
        ) -> Result<SearchProjectionRelationalDelta>,
    {
        if self.incremental_search_enabled() {
            let operations = max_change_operations_per_batch
                .max(max_projection_operations_per_batch)
                .saturating_mul(max_batches);
            let permit = self.admit_incremental_search_maintenance(
                operations.saturating_mul(SEARCH_PROJECTION_CHANGEFEED_OPERATION_BYTES),
            )?;
            return self.catch_up_incremental_search_with_batch_hydrator(
                max_change_operations_per_batch,
                max_projection_operations_per_batch,
                max_batches,
                batch_hydrator,
                permit.bind_task_context(RuntimeTaskContext::default()),
            );
        }
        let Self {
            graph,
            search_projection,
            ..
        } = self;
        let search_projection = require_search_projection_mut(search_projection)?;
        graph
            .database()
            .catch_up_search_projection_with_batch_hydrator(
                search_projection.index_mut(),
                max_change_operations_per_batch,
                max_projection_operations_per_batch,
                max_batches,
                batch_hydrator,
            )
    }

    pub fn catch_up_search_projection_scheduled(
        &mut self,
        max_operations_per_batch: usize,
        max_batches: usize,
    ) -> Result<ScheduledSearchProjectionCatchUpReport> {
        let Self {
            graph,
            search_projection,
            ..
        } = self;
        let search_projection = require_search_projection_mut(search_projection)?;
        graph.database().catch_up_search_projection_scheduled(
            search_projection.index_mut(),
            max_operations_per_batch,
            max_batches,
        )
    }

    pub fn apply_scheduled_background_search_projection_graph_delta(
        &mut self,
        request: SearchProjectionGraphDeltaRequest,
    ) -> Result<SearchProjectionDeltaReport> {
        let Self {
            graph,
            search_projection,
            ..
        } = self;
        let search_projection = require_search_projection_mut(search_projection)?;
        graph
            .database()
            .apply_scheduled_background_search_projection_graph_delta(
                search_projection.index_mut(),
                request,
            )
    }

    pub fn search_projection_probe_json(
        &self,
        options: SearchProjectionProbeOptions,
    ) -> Result<serde_json::Value> {
        Ok(self.require_search_projection()?.probe_json(options))
    }

    pub fn search_projection_evidence_json(
        &self,
        options: SearchProjectionProbeOptions,
    ) -> Result<serde_json::Value> {
        Ok(self.require_search_projection()?.evidence_json(options))
    }

    pub fn search_projection_evidence_report(
        &self,
        options: SearchProjectionProbeOptions,
    ) -> Result<NowledgeSearchProjectionEvidenceReport> {
        Ok(self.require_search_projection()?.evidence_report(options))
    }

    pub fn validate_sampled_vector_recall(
        &self,
        options: VectorRecallValidationOptions,
    ) -> Result<VectorRecallValidationReport> {
        Ok(self
            .require_search_projection()?
            .validate_sampled_vector_recall(options))
    }

    pub fn qualify_sampled_vector_recall_for_production(
        &self,
        options: VectorRecallValidationOptions,
        evidence_binding: crate::ProductionEvidenceBinding,
        expected_identity: crate::ProductionQualificationIdentity,
    ) -> Result<VectorRecallProductionQualificationReport> {
        Ok(self
            .require_search_projection()?
            .qualify_sampled_vector_recall_for_production(
                options,
                evidence_binding,
                expected_identity,
            ))
    }

    pub fn search_projection_shadow_evidence_json(
        &self,
        primary_probe: &serde_json::Value,
        options: SearchProjectionProbeOptions,
    ) -> Result<serde_json::Value> {
        Ok(self
            .require_search_projection()?
            .shadow_evidence_json(primary_probe, options))
    }

    pub fn search_candidates(
        &self,
        request: &NowledgeMemSearchCandidateRequest,
    ) -> Result<NowledgeMemSearchCandidateOutput> {
        match (
            self.search_projection.as_ref(),
            self.out_of_core_search_projection.as_ref(),
        ) {
            (Some(projection), None) => projection.try_search_candidates_with_report(request),
            (None, Some(projection)) => {
                Ok(projection.search_candidates_with_report(request)?.into())
            }
            (None, None) => Err(missing_search_projection_error()),
            (Some(_), Some(_)) => Err(ambiguous_search_projection_error()),
        }
    }

    pub fn search_candidate_readiness(
        &self,
        request: &NowledgeMemSearchCandidateRequest,
        options: &NowledgeMemSearchCandidateReadinessOptions,
    ) -> Result<NowledgeMemSearchCandidateReadinessReport> {
        Ok(self.search_candidates(request)?.readiness_report(options))
    }

    pub fn search_candidate_shadow_evidence_json<I, S>(
        &self,
        request: &NowledgeMemSearchCandidateRequest,
        primary_candidate_ids: I,
    ) -> Result<serde_json::Value>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.require_search_projection()?
            .search_candidate_shadow_evidence_json(request, primary_candidate_ids)
    }

    pub fn retrieve_knowledge(
        &self,
        request: &KnowledgeRetrievalRequest,
    ) -> Result<KnowledgeRetrievalOutput> {
        Ok(self.retrieve_knowledge_with_report(request)?.output)
    }

    pub fn retrieve_knowledge_with_report(
        &self,
        request: &KnowledgeRetrievalRequest,
    ) -> Result<NowledgeMemRetrievalOutput> {
        let (output, out_of_core_search_metrics) = match (
            self.search_projection.as_ref(),
            self.out_of_core_search_projection.as_ref(),
        ) {
            (Some(search_projection), None) => (
                self.graph
                    .database()
                    .try_retrieve_knowledge(search_projection.index(), request)?,
                None,
            ),
            (None, Some(search_projection)) => {
                let search = search_projection
                    .search_candidates_with_report(&self.search_request_for_retrieval(request))?;
                let output = self.graph.database().try_retrieve_knowledge_from_search(
                    search.result,
                    search_projection.freshness(),
                    request,
                )?;
                (output, Some(search.metrics))
            }
            (None, None) => return Err(missing_search_projection_error()),
            (Some(_), Some(_)) => return Err(ambiguous_search_projection_error()),
        };
        let report = nowledge_mem_retrieval_report(
            self.graph.mode(),
            self.graph.database().config().compressed_vector_search_mode,
            &output,
        );
        Ok(NowledgeMemRetrievalOutput {
            output,
            report,
            out_of_core_search_metrics,
        })
    }

    pub fn read_query(&self, cypher: &str) -> Result<NowledgeMemReadOutput> {
        self.graph.read_query(cypher)
    }

    pub fn query_with_report(&mut self, cypher: &str) -> Result<NowledgeMemQueryOutput> {
        self.query_with_params_with_report_options(
            cypher,
            &BTreeMap::new(),
            NowledgeMemQueryReportOptions::default(),
        )
    }

    pub fn query_with_report_options(
        &mut self,
        cypher: &str,
        options: NowledgeMemQueryReportOptions,
    ) -> Result<NowledgeMemQueryOutput> {
        self.query_with_params_with_report_options(cypher, &BTreeMap::new(), options)
    }

    pub fn query_with_params_with_report(
        &mut self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
    ) -> Result<NowledgeMemQueryOutput> {
        self.query_with_params_with_report_options(
            cypher,
            parameters,
            NowledgeMemQueryReportOptions::default(),
        )
    }

    pub fn query_with_params_with_report_options(
        &mut self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        options: NowledgeMemQueryReportOptions,
    ) -> Result<NowledgeMemQueryOutput> {
        let Self {
            graph,
            search_projection,
            out_of_core_search_projection,
            ..
        } = self;
        let mut external = SearchProjectionExternalReadOperator {
            projection: search_projection.as_ref(),
            out_of_core_projection: out_of_core_search_projection.as_ref(),
            vector_seed_execution_count: 0,
        };
        graph.query_with_params_with_report_options_and_external(
            cypher,
            parameters,
            options,
            &mut external,
        )
    }

    pub fn query_with_params_with_report_options_context(
        &mut self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        options: NowledgeMemQueryReportOptions,
        task_context: &RuntimeTaskContext,
    ) -> Result<NowledgeMemQueryOutput> {
        let Self {
            graph,
            search_projection,
            out_of_core_search_projection,
            ..
        } = self;
        let mut external = SearchProjectionExternalReadOperator {
            projection: search_projection.as_ref(),
            out_of_core_projection: out_of_core_search_projection.as_ref(),
            vector_seed_execution_count: 0,
        };
        graph.query_with_params_with_report_options_and_external_context(
            cypher,
            parameters,
            options,
            &mut external,
            task_context,
        )
    }

    pub fn query_runtime_preflight(
        &mut self,
        probes: &[NowledgeQueryRuntimePreflightProbe],
    ) -> NowledgeQueryRuntimePreflightReport {
        nowledge_query_runtime_preflight_report(self.graph.database_mut(), probes)
    }

    pub fn query_runtime_preflight_json(
        &mut self,
        probes: &[NowledgeQueryRuntimePreflightProbe],
    ) -> serde_json::Value {
        self.query_runtime_preflight(probes).json()
    }

    pub fn slow_query_report(&self) -> NowledgeMemSlowQueryReport {
        self.graph.slow_query_report()
    }

    pub fn slow_query_report_json(&self) -> serde_json::Value {
        self.slow_query_report().json()
    }

    pub fn read_query_with_options(
        &self,
        cypher: &str,
        options: &NowledgeMemReadOptions,
    ) -> Result<NowledgeMemReadOutput> {
        self.graph.read_query_with_options(cypher, options)
    }

    pub fn read_query_with_params(
        &self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        options: &NowledgeMemReadOptions,
    ) -> Result<NowledgeMemReadOutput> {
        self.graph
            .read_query_with_params(cypher, parameters, options)
    }

    pub fn read_query_with_params_streaming(
        &self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        options: &NowledgeMemReadOptions,
        consumer: impl FnMut(BTreeMap<String, Value>) -> Result<()>,
    ) -> Result<QueryStreamReport> {
        self.graph
            .read_query_with_params_streaming(cypher, parameters, options, consumer)
    }

    pub fn read_query_with_params_streaming_context(
        &self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        options: &NowledgeMemReadOptions,
        task_context: &RuntimeTaskContext,
        consumer: impl FnMut(BTreeMap<String, Value>) -> Result<()>,
    ) -> Result<QueryStreamReport> {
        self.graph.read_query_with_params_streaming_context(
            cypher,
            parameters,
            options,
            task_context,
            consumer,
        )
    }

    pub fn read_query_with_params_streaming_collect(
        &self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        options: &NowledgeMemReadOptions,
    ) -> Result<NowledgeMemReadOutput> {
        self.graph
            .read_query_with_params_streaming_collect(cypher, parameters, options)
    }

    pub fn graph_rag_schema_context(
        &self,
        options: GraphRagSchemaContextOptions,
    ) -> Result<GraphRagSchemaContext> {
        self.graph.graph_rag_schema_context(options)
    }

    pub fn read_generated_graph_rag(
        &self,
        query: &GraphRagGeneratedQuery,
        parameters: &BTreeMap<String, Value>,
        options: &NowledgeMemReadOptions,
    ) -> Result<NowledgeMemReadOutput> {
        self.graph
            .read_generated_graph_rag(query, parameters, options)
    }
}
impl NowledgeMemEmbeddedStore {
    pub fn background_maintenance_summary(
        &self,
        policy: &LocalQosPolicy,
        state: &LocalQosState,
        options: BackgroundMaintenanceOptions,
    ) -> Result<BackgroundMaintenanceSummary> {
        Ok({
            self.graph.database().background_maintenance_summary(
                self.search_projection
                    .as_ref()
                    .map(NowledgeMemSearchProjection::index),
                policy,
                state,
                options,
            )?
        })
    }

    pub fn background_maintenance_report(
        &self,
        policy: &LocalQosPolicy,
        state: &LocalQosState,
        options: BackgroundMaintenanceOptions,
    ) -> Result<NowledgeMemBackgroundMaintenanceReport> {
        Ok({
            let summary = self.background_maintenance_summary(policy, state, options)?;
            let slow_query = self.slow_query_report();
            NowledgeMemBackgroundMaintenanceReport::from_summary_with_slow_query(
                &summary,
                Some(&slow_query),
            )
        })
    }

    pub fn background_maintenance_report_json(
        &self,
        policy: &LocalQosPolicy,
        state: &LocalQosState,
        options: BackgroundMaintenanceOptions,
    ) -> Result<serde_json::Value> {
        Ok({
            self.background_maintenance_report(policy, state, options)?
                .json()
        })
    }

    pub fn library_readiness_json(
        &self,
        options: &NowledgeMemReadinessOptions,
    ) -> Result<serde_json::Value> {
        Ok(self.library_readiness(options)?.json())
    }

    pub fn library_readiness(
        &self,
        options: &NowledgeMemReadinessOptions,
    ) -> Result<NowledgeMemLibraryReadinessReport> {
        Ok({
            let bounded_read_evidence = options
                .bounded_read_evidence
                .clone()
                .unwrap_or_else(|| self.bounded_read_probe_evidence_json(options));
            let storage_recovery = self.storage_recovery_report_json()?;
            let background_maintenance = self.background_maintenance_report_json(
                &options.qos_policy,
                &options.qos_state,
                options.background_maintenance_options.clone(),
            )?;
            let query_family_evidence = query_family_replacement_evidence_json(
                options.replacement_readiness_by_query_family.as_ref(),
            );
            let graph_route_readiness =
                nowledge_mem_graph_route_readiness_json(options.graph_route_readiness.as_ref());
            let search_route_ownership = options
                .search_route_ownership
                .as_ref()
                .map(NowledgeMemSearchRouteOwnershipReadinessReport::json)
                .unwrap_or_else(missing_search_route_ownership_json);
            let active_search_route_ownership = options
                .active_search_route_ownership
                .as_ref()
                .map(NowledgeMemActiveSearchRouteOwnershipReadinessReport::json)
                .unwrap_or_else(missing_active_search_route_ownership_json);
            let active_search_route_readiness = options
                .active_search_route_readiness
                .as_ref()
                .map(NowledgeMemActiveSearchRouteReadinessReport::json)
                .unwrap_or_else(missing_active_search_route_readiness_json);
            let search_projection_evidence = options
                .search_projection_evidence
                .clone()
                .unwrap_or_else(|| {
                    self.search_projection_evidence_json(
                        options.search_projection_probe_options.clone(),
                    )
                    .unwrap_or_else(|_| missing_search_projection_evidence_json())
                });
            let search_projection_shadow_evidence = options
                .search_projection_shadow_evidence
                .clone()
                .unwrap_or_else(|| {
                    options
                        .primary_search_projection_probe
                        .as_ref()
                        .map(|primary_probe| {
                            self.search_projection_shadow_evidence_json(
                                primary_probe,
                                options.search_projection_probe_options.clone(),
                            )
                            .unwrap_or_else(|_| missing_search_projection_shadow_evidence_json())
                        })
                        .unwrap_or_else(missing_primary_search_projection_probe_json)
                });
            let search_candidate_shadow_evidence = options
                .search_candidate_shadow_evidence
                .clone()
                .unwrap_or_else(missing_search_candidate_shadow_evidence_json);
            let workload_fixture_evidence = options
                .workload_fixture_evidence
                .as_ref()
                .map(NowledgeGraphRouteWorkloadFixtureReport::json)
                .unwrap_or_else(missing_workload_fixture_evidence_json);
            let production_resource_profile = options
                .production_resource_profile
                .as_ref()
                .map(StorageResourceProfileReport::json)
                .unwrap_or_else(missing_production_resource_profile_json);
            let assessment =
                hawdb_readiness::library_readiness::assess_nowledge_mem_library_readiness(
                    &hawdb_readiness::library_readiness::NowledgeMemLibraryReadinessEvidence {
                        bounded_read_evidence: &bounded_read_evidence,
                        storage_recovery: &storage_recovery,
                        background_maintenance: &background_maintenance,
                        query_family_evidence: &query_family_evidence,
                        graph_route_readiness: &graph_route_readiness,
                        search_route_ownership: &search_route_ownership,
                        active_search_route_ownership: &active_search_route_ownership,
                        active_search_route_readiness: &active_search_route_readiness,
                        search_projection_evidence: &search_projection_evidence,
                        search_projection_shadow_evidence: &search_projection_shadow_evidence,
                        search_candidate_shadow_evidence: &search_candidate_shadow_evidence,
                        workload_fixture_evidence: &workload_fixture_evidence,
                        production_resource_profile: &production_resource_profile,
                    },
                );
            let readiness_by_area = assessment.readiness_by_area;
            let areas = readiness_by_area.areas();
            let ready_area_count = areas.iter().filter(|area| area.ready).count();
            let blocked_area_count = areas.len().saturating_sub(ready_area_count);
            let blocker_codes = assessment.blocker_codes;
            let ready = blocker_codes.is_empty();
            NowledgeMemLibraryReadinessReport {
                protocol: NOWLEDGE_MEM_LIBRARY_READINESS_PROTOCOL.to_string(),
                present: true,
                ready,
                mode: self.graph.mode(),
                redaction: NowledgeMemReadinessRedactionSummary::default(),
                production_path: NowledgeMemLibraryProductionPathSummary::default(),
                blocker_codes,
                readiness_by_area,
                ready_area_count,
                blocked_area_count,
                graph_open: true,
                graph_read_only: self.graph.database().config().read_only,
                graph_route_readiness,
                search_route_ownership,
                active_search_route_ownership,
                active_search_route_readiness,
                bounded_read_evidence,
                storage_recovery,
                background_maintenance,
                query_family_evidence,
                search_projection_evidence,
                search_projection_shadow_evidence,
                search_candidate_shadow_evidence,
                workload_fixture_evidence,
                production_resource_profile,
            }
        })
    }

    pub fn readiness_dashboard(
        &self,
        options: &NowledgeMemReadinessOptions,
    ) -> Result<NowledgeMemReadinessDashboard> {
        Ok({
            let library = self.library_readiness(options)?;
            let storage_lifecycle = self.storage_lifecycle_decision()?;
            let slow_query = self.slow_query_report();
            NowledgeMemReadinessDashboard::from_reports(&library, &storage_lifecycle, &slow_query)
        })
    }

    pub fn readiness_dashboard_json(
        &self,
        options: &NowledgeMemReadinessOptions,
    ) -> Result<serde_json::Value> {
        Ok(self.readiness_dashboard(options)?.json())
    }

    pub fn operations_readiness(
        &self,
        options: &NowledgeMemReadinessOptions,
    ) -> Result<NowledgeMemOperationsReadinessReport> {
        Ok({
            let runtime_status = self.runtime_status()?;
            let storage_recovery = self.storage_recovery_report()?;
            let slow_query = self.slow_query_report();
            let background_maintenance = self.background_maintenance_report(
                &options.qos_policy,
                &options.qos_state,
                options.background_maintenance_options.clone(),
            )?;
            NowledgeMemOperationsReadinessReport::from_reports(
                self.graph.mode(),
                self.graph.database().config().read_only,
                runtime_status,
                storage_recovery,
                slow_query,
                background_maintenance,
            )
        })
    }

    pub fn operations_readiness_json(
        &self,
        options: &NowledgeMemReadinessOptions,
    ) -> Result<serde_json::Value> {
        Ok(self.operations_readiness(options)?.json())
    }

    fn bounded_read_probe_evidence_json(
        &self,
        options: &NowledgeMemReadinessOptions,
    ) -> serde_json::Value {
        let Some(probe) = options.bounded_read_probe.as_ref() else {
            return serde_json::json!({
                "protocol": NOWLEDGE_MEM_BOUNDED_READ_EVIDENCE_PROTOCOL,
                "present": false,
                "ready": false,
                "blocker_codes": ["bounded_read_probe_missing"],
            });
        };
        match self.read_query_with_params(&probe.cypher, &probe.parameters, &options.read_options) {
            Ok(read) => nowledge_mem_bounded_read_evidence_json_with_route_readiness(
                &read.report,
                &options.covered_routes,
                options.graph_route_readiness.as_ref(),
            ),
            Err(_) => serde_json::json!({
                "protocol": NOWLEDGE_MEM_BOUNDED_READ_EVIDENCE_PROTOCOL,
                "present": true,
                "ready": false,
                "blocker_codes": ["bounded_read_probe_failed"],
            }),
        }
    }

    fn search_projection_freshness(&self) -> Option<SearchProjectionFreshness> {
        match (
            self.search_projection.as_ref(),
            self.out_of_core_search_projection.as_ref(),
        ) {
            (Some(projection), None) => Some(projection.freshness()),
            (None, Some(projection)) => Some(projection.freshness()),
            _ => None,
        }
    }

    fn search_io_slots(&self) -> Result<usize> {
        match (
            self.search_projection.as_ref(),
            self.out_of_core_search_projection.as_ref(),
        ) {
            (Some(projection), None) => Ok(projection.index().range_read_config().io_depth.get()),
            (None, Some(_)) => Ok(1),
            (None, None) => Err(missing_search_projection_error()),
            (Some(_), Some(_)) => Err(ambiguous_search_projection_error()),
        }
    }

    fn search_memory_bytes(
        &self,
        working_memory_bytes: u64,
        result_budget_bytes: u64,
    ) -> Result<u64> {
        match (
            self.search_projection.as_ref(),
            self.out_of_core_search_projection.as_ref(),
        ) {
            (Some(_), None) => Ok(working_memory_bytes),
            (None, Some(projection)) => Ok(projection
                .runtime_admission_memory_bytes(working_memory_bytes, result_budget_bytes)),
            (None, None) => Err(missing_search_projection_error()),
            (Some(_), Some(_)) => Err(ambiguous_search_projection_error()),
        }
    }

    fn search_request_for_retrieval(
        &self,
        request: &KnowledgeRetrievalRequest,
    ) -> NowledgeMemSearchCandidateRequest {
        let config = self.graph.database().config();
        NowledgeMemSearchCandidateRequest {
            query_text: request.query_text.clone(),
            query_embedding: request.query_embedding.clone(),
            mode: request.mode,
            limit: request.limit,
            offset: request.offset,
            rank_window: request.rank_window,
            fusion_weights: request.search_fusion_weights,
            metadata_filters: request.metadata_filters.clone(),
            compressed_vector_search_mode: config.compressed_vector_search_mode,
            adaptive_vector_backend_policy: config.adaptive_vector_backend_policy,
            recall_validation_probe: false,
            retrieval_projection_advisor: self.retrieval_projection_advisor.clone(),
        }
    }

    fn require_search_projection(&self) -> Result<&NowledgeMemSearchProjection> {
        self.search_projection
            .as_ref()
            .ok_or_else(missing_search_projection_error)
    }
}

fn missing_search_projection_error() -> HawDBError {
    HawDBError::Storage("nowledge mem search projection is not configured".to_string())
}

fn ambiguous_search_projection_error() -> HawDBError {
    HawDBError::Storage("nowledge mem search projection ownership is ambiguous".to_string())
}

fn require_search_projection_mut(
    search_projection: &mut Option<NowledgeMemSearchProjection>,
) -> Result<&mut NowledgeMemSearchProjection> {
    search_projection
        .as_mut()
        .ok_or_else(missing_search_projection_error)
}

struct NowledgeMemQueryReportInput<'a> {
    mode: NowledgeMemGraphMode,
    statement: &'a cypher::Statement,
    trace: Option<&'a crate::optimizer::OptimizerTrace>,
    plan_cache_lookup: Option<PlanCacheLookup>,
    execution_profile: Option<&'a ReadExecutionProfile>,
    output: &'a QueryOutput,
    options: NowledgeMemQueryReportOptions,
    elapsed_micros: u128,
}

fn nowledge_mem_query_report(input: NowledgeMemQueryReportInput<'_>) -> NowledgeMemQueryReport {
    let statement_kind = crate::api::statement_kind(
        hawdb_cypher::read_route::query_statement_body(input.statement),
    );
    let decision = nowledge_mem_fast_path_classification(input.statement);
    hawdb_readiness::nowledge_mem_query_report::nowledge_mem_query_report(
        hawdb_readiness::nowledge_mem_query_report::NowledgeMemQueryReportInput {
            mode: input.mode,
            statement_kind,
            statement: input.statement,
            execution_path: decision.execution_path,
            fast_path_reason: decision.fast_path_reason,
            physical_plan_captured: input.trace.is_some(),
            physical_operator_counts: input
                .trace
                .map(|trace| trace.selected_plan_operator_counts.clone())
                .unwrap_or_default(),
            optimizer_decision_count: input
                .trace
                .map(|trace| trace.decisions.len())
                .unwrap_or_default(),
            optimizer_rule_event_count: input
                .trace
                .map(|trace| trace.rule_events.len())
                .unwrap_or_default(),
            plan_cache_lookup: input.plan_cache_lookup,
            execution_profile: input.execution_profile,
            output: input.output,
            options: input.options,
            elapsed_micros: input.elapsed_micros,
        },
    )
}

fn nowledge_query_runtime_preflight_report(
    db: &mut Database,
    probes: &[NowledgeQueryRuntimePreflightProbe],
) -> NowledgeQueryRuntimePreflightReport {
    let route_coverage = nowledge_query_runtime_route_coverage(probes);
    let probe_reports = probes
        .iter()
        .map(|probe| nowledge_query_runtime_probe_report(db, probe))
        .collect::<Vec<_>>();
    let passed_probe_count = probe_reports.iter().filter(|probe| probe.ready).count();
    let failed_probe_count = probe_reports.len().saturating_sub(passed_probe_count);
    let blocker_codes =
        query_runtime_preflight_blocker_codes(probes.len(), failed_probe_count, &route_coverage);

    NowledgeQueryRuntimePreflightReport {
        protocol: NOWLEDGE_QUERY_RUNTIME_PREFLIGHT_PROTOCOL.to_string(),
        ready: blocker_codes.is_empty(),
        database_opened: true,
        redaction: NowledgeQueryRuntimePreflightRedactionSummary::default(),
        probe_count: probes.len(),
        passed_probe_count,
        failed_probe_count,
        required_route_count: route_coverage.required_route_count,
        covered_route_count: route_coverage.covered_route_count,
        covered_routes: route_coverage.covered_routes,
        missing_required_routes: route_coverage.missing_required_routes,
        required_routes_covered: route_coverage.required_routes_covered,
        unknown_routes: route_coverage.unknown_routes,
        duplicate_routes: route_coverage.duplicate_routes,
        route_catalog_version: NOWLEDGE_MEM_GRAPH_READ_ROUTE_CATALOG_VERSION.to_string(),
        route_catalog_digest: nowledge_mem_graph_read_route_catalog_digest(),
        route_coverage_ready: route_coverage.ready,
        route_coverage_blocker_codes: route_coverage.blocker_codes,
        blocker_codes,
        probes: probe_reports,
    }
}

fn nowledge_query_runtime_probe_report(
    db: &mut Database,
    probe: &NowledgeQueryRuntimePreflightProbe,
) -> NowledgeQueryRuntimePreflightProbeReport {
    match db.explain_analyze_query_with_params(&probe.cypher, &probe.parameters) {
        Ok(output) => {
            let scan_pruning_report_count = output.execution_profile.scan_pruning_reports.len();
            let pruned_scan_count = output
                .execution_profile
                .scan_pruning_reports
                .iter()
                .filter(|report| report.pruned)
                .count();
            let output_row_count = output.output.rows.len();
            let blocker_codes = query_runtime_probe_blocker_codes(
                probe,
                scan_pruning_report_count,
                pruned_scan_count,
                output_row_count,
            );
            let plan_cache_lookup = output.plan_cache_lookup;
            let plan_cache = nowledge_mem_plan_cache_report(Some(plan_cache_lookup));
            NowledgeQueryRuntimePreflightProbeReport {
                name: probe.name.clone(),
                route: probe.route.clone(),
                query_family: probe.query_family.clone(),
                ready: blocker_codes.is_empty(),
                success: true,
                output_row_count,
                selected_plan_fingerprint: Some(output.trace.selected_plan_fingerprint),
                search_mode: Some(output.trace.search_mode.as_str().to_string()),
                selected_plan_operator_counts: output.trace.selected_plan_operator_counts,
                selected_plan_class_counts: output.trace.selected_plan_class_counts,
                optimizer_decision_count: output.trace.decisions.len(),
                optimizer_rule_event_count: output.trace.rule_events.len(),
                plan_cache_lookup: Some(plan_cache_lookup.as_str().to_string()),
                plan_cache_bypass_reason: plan_cache_lookup
                    .bypass_reason()
                    .map(|reason| reason.as_str().to_string()),
                plan_cache_cacheable: plan_cache.cacheable,
                plan_cache_hit: plan_cache.hit,
                plan_cache_miss: plan_cache.miss,
                plan_cache_bypassed: plan_cache.bypassed,
                work_priority: Some(output.work_request.priority.as_str().to_string()),
                work_class: Some(output.work_request.class.as_str().to_string()),
                estimated_operations: Some(output.work_request.estimated_operations),
                max_rows: output.execution_profile.max_rows,
                detection_row_cap: output.execution_profile.detection_row_cap,
                row_limit_enforced_before_output: output
                    .execution_profile
                    .row_limit_enforced_before_output,
                operator_row_cap_enabled: output.execution_profile.operator_row_cap_enabled,
                blocking_operator_kinds: output.execution_profile.blocking_operator_kinds,
                scan_pruning_reports: output.execution_profile.scan_pruning_reports,
                pruned_scan_count,
                error_class: None,
                blocker_codes,
            }
        }
        Err(error) => NowledgeQueryRuntimePreflightProbeReport {
            name: probe.name.clone(),
            route: probe.route.clone(),
            query_family: probe.query_family.clone(),
            ready: false,
            success: false,
            output_row_count: 0,
            selected_plan_fingerprint: None,
            search_mode: None,
            selected_plan_operator_counts: BTreeMap::new(),
            selected_plan_class_counts: BTreeMap::new(),
            optimizer_decision_count: 0,
            optimizer_rule_event_count: 0,
            plan_cache_lookup: None,
            plan_cache_bypass_reason: None,
            plan_cache_cacheable: false,
            plan_cache_hit: false,
            plan_cache_miss: false,
            plan_cache_bypassed: false,
            work_priority: None,
            work_class: None,
            estimated_operations: None,
            max_rows: None,
            detection_row_cap: None,
            row_limit_enforced_before_output: false,
            operator_row_cap_enabled: false,
            blocking_operator_kinds: Vec::new(),
            scan_pruning_reports: Vec::new(),
            pruned_scan_count: 0,
            error_class: Some(hawdb_error_class(&error).to_string()),
            blocker_codes: vec!["query_runtime_failed".to_string()],
        },
    }
}

fn missing_search_projection_evidence_json() -> serde_json::Value {
    serde_json::json!({
        "protocol": "hawdb-nowledge-search-projection-evidence",
        "present": false,
        "ready": false,
        "blocker_codes": ["search_projection_not_configured"],
    })
}

fn missing_search_projection_shadow_evidence_json() -> serde_json::Value {
    serde_json::json!({
        "protocol": "hawdb-nowledge-search-projection-shadow-evidence",
        "present": false,
        "ready": false,
        "blocker_codes": ["search_projection_not_configured"],
    })
}

fn missing_primary_search_projection_probe_json() -> serde_json::Value {
    serde_json::json!({
        "protocol": "hawdb-nowledge-search-projection-shadow-evidence",
        "present": false,
        "ready": false,
        "blocker_codes": ["primary_search_projection_probe_missing"],
    })
}

fn missing_search_candidate_shadow_evidence_json() -> serde_json::Value {
    serde_json::json!({
        "protocol": NOWLEDGE_MEM_SEARCH_CANDIDATE_SHADOW_EVIDENCE_PROTOCOL,
        "route": NOWLEDGE_MEM_SEARCH_CANDIDATE_EVIDENCE_ROUTE,
        "evidence_source": NOWLEDGE_MEM_SEARCH_CANDIDATE_EVIDENCE_SOURCE,
        "present": false,
        "ready": false,
        "candidate_primary_engine": NOWLEDGE_MEM_SEARCH_CANDIDATE_PRIMARY_ENGINE,
        "request_count": null,
        "primary_candidate_count": null,
        "shadow_candidate_count": null,
        "matched_candidate_count": null,
        "primary_only_candidate_count": null,
        "candidate_identity": {
            "ready": false,
            "parity": false,
        },
        "filter_pushdown_ready": false,
        "filter_pushdown": {
            "ready": false,
            "required_fields": NOWLEDGE_SEARCH_PROJECTION_SCAN_FILTER_FIELDS,
            "missing_required_fields": NOWLEDGE_SEARCH_PROJECTION_SCAN_FILTER_FIELDS,
            "field_summary_count": 0,
            "field_summaries": [],
            "blocker_codes": ["search_candidate_shadow_evidence_missing"],
        },
        "blocker_codes": ["search_candidate_shadow_evidence_missing"],
    })
}

fn missing_workload_fixture_evidence_json() -> serde_json::Value {
    serde_json::json!({
        "protocol": NOWLEDGE_GRAPH_ROUTE_WORKLOAD_FIXTURE_PROTOCOL,
        "present": false,
        "ready": false,
        "route_count": null,
        "query_count": null,
        "failed_query_count": null,
        "bounded_expansion_probe_count": null,
        "failed_bounded_expansion_probe_count": null,
        "search_metadata_probe_count": null,
        "failed_search_metadata_probe_count": null,
        "blocker_codes": ["workload_fixture_evidence_missing"],
    })
}

fn missing_production_resource_profile_json() -> serde_json::Value {
    serde_json::json!({
        "protocol": STORAGE_RESOURCE_PROFILE_PROTOCOL,
        "protocol_version": 2,
        "present": false,
        "resource_ready": false,
        "ready": false,
        "blocker_codes": ["production_resource_profile_missing"],
    })
}

fn query_family_replacement_evidence_json(
    replacement_readiness_by_query_family: Option<&serde_json::Value>,
) -> serde_json::Value {
    let Some(families) =
        query_family_replacement_readiness_array(replacement_readiness_by_query_family)
    else {
        return serde_json::json!({
            "protocol": "hawdb-nowledge-query-family-evidence-v1",
            "present": false,
            "ready": false,
            "required_query_families": REQUIRED_NOWLEDGE_REPLACEMENT_QUERY_FAMILIES,
            "missing_required_query_families": REQUIRED_NOWLEDGE_REPLACEMENT_QUERY_FAMILIES,
            "blocker_codes": ["query_family_evidence_missing"],
        });
    };
    let health = replacement_readiness_family_evidence_health(Some(families));
    let blocker_codes = query_family_replacement_blocker_codes(&health);
    serde_json::json!({
        "protocol": "hawdb-nowledge-query-family-evidence-v1",
        "present": health.present,
        "ready": health.ready,
        "min_replacement_readiness_per_million": health.min_replacement_readiness_per_million,
        "invalid_family_count": health.invalid_family_count,
        "blocked_query_families": health.blocked_query_families,
        "required_query_families": REQUIRED_NOWLEDGE_REPLACEMENT_QUERY_FAMILIES,
        "missing_required_query_families": health.missing_required_query_families,
        "blocker_codes": blocker_codes,
        "blockers": health.blockers,
        "replacement_readiness_by_query_family": families,
    })
}

fn query_family_replacement_readiness_array(
    value: Option<&serde_json::Value>,
) -> Option<&serde_json::Value> {
    let value = value?;
    if value.is_array() {
        return Some(value);
    }
    value
        .get("replacement_readiness_by_query_family")
        .filter(|families| families.is_array())
}

fn query_family_replacement_blocker_codes(
    health: &crate::nowledge_inventory::ReplacementReadinessFamilyEvidenceHealth,
) -> Vec<&'static str> {
    let mut blockers = Vec::new();
    if health.invalid_family_count > 0 {
        blockers.push("invalid_family_entries");
    }
    if !health.blocked_query_families.is_empty() {
        blockers.push("blocked_query_families");
    }
    if !health.missing_required_query_families.is_empty() {
        blockers.push("missing_required_query_families");
    }
    blockers
}

fn nowledge_mem_graph_route_readiness_json(
    route_readiness: Option<&NowledgeMemRouteReadinessSummary>,
) -> serde_json::Value {
    let Some(summary) = route_readiness else {
        return serde_json::json!({
            "protocol": NMEM_GRAPH_ROUTE_READINESS_PROTOCOL,
            "present": false,
            "ready": false,
            "required_route_count": REQUIRED_NOWLEDGE_MEM_BOUNDED_READ_ROUTES.len(),
            "primary_ready_route_count": 0,
            "primary_ready_routes": [],
            "missing_required_routes": REQUIRED_NOWLEDGE_MEM_BOUNDED_READ_ROUTES,
            "route_catalog_version": NOWLEDGE_MEM_GRAPH_READ_ROUTE_CATALOG_VERSION,
            "route_catalog_digest": nowledge_mem_graph_read_route_catalog_digest(),
            "route_primary_ready": null,
            "route_query_plan_evidence_ready": null,
            "route_query_profile_evidence_ready": null,
            "route_query_api_behavior_evidence_ready": null,
            "relationship_property_pruning_required_count": null,
            "relationship_property_pruning_report_count": null,
            "route_relationship_property_pruning_evidence_ready": null,
            "blocker_codes": ["graph_route_readiness_missing"],
        });
    };

    let missing_required_routes =
        hawdb_readiness::bounded_read_evidence::missing_nowledge_mem_bounded_read_routes(
            &summary.primary_ready_routes,
        );
    let relationship_property_pruning_count_matches = summary
        .relationship_property_pruning_required_count
        == summary.relationship_property_pruning_report_count;
    let mut blocker_codes = Vec::new();
    if !summary.route_primary_ready || !missing_required_routes.is_empty() {
        blocker_codes.push("route_primary_not_ready");
    }
    if !summary.route_query_plan_evidence_ready {
        blocker_codes.push("route_query_plan_evidence_not_ready");
    }
    if !summary.route_query_profile_evidence_ready {
        blocker_codes.push("route_query_profile_evidence_not_ready");
    }
    if !summary.route_query_api_behavior_evidence_ready {
        blocker_codes.push("route_query_api_behavior_evidence_not_ready");
    }
    if !summary.route_relationship_property_pruning_evidence_ready
        || !relationship_property_pruning_count_matches
    {
        blocker_codes.push("route_relationship_property_pruning_not_ready");
    }

    serde_json::json!({
        "protocol": NMEM_GRAPH_ROUTE_READINESS_PROTOCOL,
        "present": true,
        "ready": blocker_codes.is_empty(),
        "required_route_count": REQUIRED_NOWLEDGE_MEM_BOUNDED_READ_ROUTES.len(),
        "primary_ready_route_count": summary.primary_ready_routes.len(),
        "primary_ready_routes": summary.primary_ready_routes,
        "missing_required_routes": missing_required_routes,
        "route_catalog_version": NOWLEDGE_MEM_GRAPH_READ_ROUTE_CATALOG_VERSION,
        "route_catalog_digest": nowledge_mem_graph_read_route_catalog_digest(),
        "route_primary_ready": summary.route_primary_ready,
        "route_query_plan_evidence_ready": summary.route_query_plan_evidence_ready,
        "route_query_profile_evidence_ready": summary.route_query_profile_evidence_ready,
        "route_query_api_behavior_evidence_ready": summary.route_query_api_behavior_evidence_ready,
        "relationship_property_pruning_required_count": summary.relationship_property_pruning_required_count,
        "relationship_property_pruning_report_count": summary.relationship_property_pruning_report_count,
        "route_relationship_property_pruning_evidence_ready": summary.route_relationship_property_pruning_evidence_ready,
        "blocker_codes": blocker_codes,
    })
}

fn missing_search_route_ownership_json() -> serde_json::Value {
    serde_json::json!({
        "protocol": NOWLEDGE_MEM_SEARCH_ROUTE_OWNERSHIP_PROTOCOL,
        "ready": false,
        "production_cutover_ready": false,
        "require_all_hawdb": true,
        "required_route_count": REQUIRED_NOWLEDGE_MEM_SEARCH_ROUTES.len(),
        "explicit_route_count": 0,
        "hawdb_route_count": 0,
        "lancedb_route_count": 0,
        "routes": [],
        "hawdb_routes": [],
        "lancedb_routes": [],
        "missing_required_routes": REQUIRED_NOWLEDGE_MEM_SEARCH_ROUTES,
        "unknown_routes": [],
        "duplicate_routes": [],
        "conflicting_routes": [],
        "blocker_codes": ["search_route_ownership_missing"],
    })
}

fn missing_active_search_route_ownership_json() -> serde_json::Value {
    serde_json::json!({
        "protocol": NOWLEDGE_MEM_SEARCH_ROUTE_OWNERSHIP_PROTOCOL,
        "ready": false,
        "production_cutover_ready": false,
        "require_all_hawdb": true,
        "required_route_count": REQUIRED_NOWLEDGE_MEM_ACTIVE_SEARCH_ROUTES.len(),
        "explicit_route_count": 0,
        "hawdb_route_count": 0,
        "lancedb_route_count": 0,
        "routes": [],
        "hawdb_routes": [],
        "lancedb_routes": [],
        "missing_required_routes": REQUIRED_NOWLEDGE_MEM_ACTIVE_SEARCH_ROUTES,
        "unknown_routes": [],
        "duplicate_routes": [],
        "invalid_projection_routes": [],
        "blocker_codes": ["active_search_route_ownership_missing"],
    })
}

fn missing_active_search_route_readiness_json() -> serde_json::Value {
    serde_json::json!({
        "protocol": NOWLEDGE_MEM_ACTIVE_SEARCH_ROUTE_READINESS_PROTOCOL,
        "ready": false,
        "production_cutover_ready": false,
        "require_all_hawdb": true,
        "required_route_count": REQUIRED_NOWLEDGE_MEM_ACTIVE_SEARCH_ROUTES.len(),
        "evidence_route_count": 0,
        "ready_route_count": 0,
        "hawdb_route_count": 0,
        "lancedb_handle_required_route_count": 0,
        "routes": [],
        "ready_routes": [],
        "missing_required_routes": REQUIRED_NOWLEDGE_MEM_ACTIVE_SEARCH_ROUTES,
        "unknown_routes": [],
        "duplicate_routes": [],
        "invalid_projection_routes": [],
        "non_hawdb_routes": [],
        "lancedb_handle_required_routes": [],
        "candidate_not_ready_routes": [],
        "candidate_identity_not_ready_routes": [],
        "metadata_pushdown_not_ready_routes": [],
        "ranking_not_ready_routes": [],
        "fail_soft_not_ready_routes": [],
        "blocker_codes": ["active_search_route_readiness_missing"],
    })
}

fn nowledge_mem_readiness_dashboard_areas(
    library: &NowledgeMemLibraryReadinessReport,
    slow_query: &NowledgeMemSlowQueryReport,
) -> Vec<NowledgeMemReadinessAreaSummary> {
    let mut areas = library.areas();
    areas.push(NowledgeMemReadinessAreaSummary {
        name: "slow_query".to_string(),
        ready: slow_query.ready,
        blocker_codes: if slow_query.ready {
            Vec::new()
        } else {
            vec!["slow_query_report_not_ready".to_string()]
        },
    });
    areas
}

fn nowledge_mem_retrieval_report(
    mode: NowledgeMemGraphMode,
    compressed_vector_search_mode: CompressedVectorSearchMode,
    output: &KnowledgeRetrievalOutput,
) -> NowledgeMemRetrievalReport {
    let vector_backend = output
        .search
        .retrievers
        .iter()
        .find(|retriever| retriever.name == "vector")
        .map(|retriever| retriever.backend.clone());
    let text_backend = output
        .search
        .retrievers
        .iter()
        .find(|retriever| retriever.name == "text")
        .map(|retriever| retriever.backend.clone());
    let search_backend = vector_backend
        .clone()
        .or_else(|| text_backend.clone())
        .or_else(|| {
            output
                .search
                .retrievers
                .first()
                .map(|retriever| retriever.backend.clone())
        });
    let knowledge_fallback_reason_codes = output
        .diagnostics
        .graph_context_fallback_reason_codes
        .iter()
        .map(|code| code.as_str().to_string())
        .collect::<Vec<_>>();
    let retriever_fallback_reason_codes = output
        .retrievers
        .iter()
        .flat_map(|retriever| retriever.fallback_reason_codes.iter())
        .map(|code| code.as_str().to_string())
        .collect::<Vec<_>>();
    let truncation_reason_codes = output
        .diagnostics
        .search_truncation_reason_codes
        .iter()
        .map(|code| code.as_str().to_string())
        .chain(
            output
                .diagnostics
                .graph_seed_truncation_reason_codes
                .iter()
                .map(|code| code.as_str().to_string()),
        )
        .chain(
            output
                .diagnostics
                .graph_context_truncation_reason_codes
                .iter()
                .map(|code| code.as_str().to_string()),
        )
        .chain(
            output
                .diagnostics
                .candidate_truncation_reason_codes
                .iter()
                .map(|code| code.as_str().to_string()),
        )
        .collect::<Vec<_>>();
    NowledgeMemRetrievalReport {
        protocol: NOWLEDGE_MEM_RETRIEVAL_REPORT_PROTOCOL.to_string(),
        mode,
        compressed_vector_search_mode,
        graph_commit_epoch: output.graph_commit_epoch,
        projection_source_graph_commit_epoch: output.projection_freshness.source_graph_commit_epoch,
        projection_commit_lag: output.diagnostics.projection_commit_lag,
        projection_stale: output.diagnostics.projection_stale,
        search_document_count: output.search.document_count,
        search_filtered_document_count: output.search.filtered_document_count,
        search_total_hits: output.search.total_hits,
        candidate_count: output.candidates.len(),
        candidate_total_count: output.diagnostics.candidate_total_count,
        evidence_count: output.evidence.len(),
        graph_seed_count: output.graph_seeds.len(),
        graph_context_path_count: output.graph_context_paths.len(),
        search_backend,
        vector_backend,
        text_backend,
        search_fallback_reason_codes: output
            .diagnostics
            .search_fallback_reason_codes
            .iter()
            .map(|code| code.as_str().to_string())
            .collect(),
        retriever_fallback_reason_codes,
        knowledge_fallback_reason_codes,
        truncation_reason_codes,
        warning_count: output.diagnostics.warnings.len(),
        warnings: output.diagnostics.warnings.clone(),
    }
}

fn nowledge_mem_read_report(
    mode: NowledgeMemGraphMode,
    output: &QueryOutput,
    options: &NowledgeMemReadOptions,
    execution_profile: &ReadExecutionProfile,
) -> NowledgeMemReadReport {
    let estimated_payload_bytes = estimate_query_output_payload_bytes(output);
    NowledgeMemReadReport {
        protocol: NOWLEDGE_MEM_READ_REPORT_PROTOCOL.to_string(),
        mode,
        row_count: output.rows.len(),
        max_rows: options.max_rows,
        execution_row_cap: execution_profile.detection_row_cap,
        estimated_payload_bytes,
        max_estimated_payload_bytes: options.max_estimated_payload_bytes,
        row_budget_exceeded: options
            .max_rows
            .is_some_and(|max_rows| output.rows.len() > max_rows),
        payload_budget_exceeded: options
            .max_estimated_payload_bytes
            .is_some_and(|max_bytes| estimated_payload_bytes > max_bytes),
        row_limit_enforced_before_output: execution_profile.row_limit_enforced_before_output,
        operator_row_cap_enabled: execution_profile.operator_row_cap_enabled,
        blocking_operator_count: execution_profile.blocking_operator_count(),
        blocking_operator_kinds: execution_profile.blocking_operator_kinds.clone(),
        blocking_operator_memory_reports: execution_profile
            .blocking_operator_memory_reports
            .clone(),
        intermediate_rows: execution_profile.pipeline_memory_report.intermediate_rows,
        intermediate_payload_bytes: execution_profile
            .pipeline_memory_report
            .intermediate_payload_bytes,
        output_payload_bytes: execution_profile
            .pipeline_memory_report
            .output_payload_bytes,
        steady_resident_bytes: execution_profile
            .pipeline_memory_report
            .steady_resident_bytes,
        peak_resident_bytes: execution_profile.pipeline_memory_report.peak_resident_bytes,
        total_page_faults: execution_profile.pipeline_memory_report.total_page_faults,
        minor_page_faults: execution_profile.pipeline_memory_report.minor_page_faults,
        major_page_faults: execution_profile.pipeline_memory_report.major_page_faults,
        streaming: false,
    }
}

fn bounded_nowledge_mem_read_output(
    mode: NowledgeMemGraphMode,
    bounded: BoundedReadQueryOutput,
    options: &NowledgeMemReadOptions,
) -> Result<NowledgeMemReadOutput> {
    let report =
        nowledge_mem_read_report(mode, &bounded.output, options, &bounded.execution_profile);
    if report.row_budget_exceeded {
        return Err(HawDBError::read_budget_exceeded(
            ReadBudgetResource::Rows,
            report.max_rows.unwrap_or_default(),
            format!(
                "nowledge mem read query returned {} rows, exceeding max_rows {}",
                report.row_count,
                report.max_rows.unwrap_or_default()
            ),
        ));
    }
    if report.payload_budget_exceeded {
        return Err(HawDBError::read_budget_exceeded(ReadBudgetResource::PayloadBytes, report.max_estimated_payload_bytes.unwrap_or_default(), format!(
            "nowledge mem read query estimated {} payload bytes, exceeding max_estimated_payload_bytes {}",
            report.estimated_payload_bytes,
            report.max_estimated_payload_bytes.unwrap_or_default()
        )));
    }
    Ok(NowledgeMemReadOutput {
        output: bounded.output,
        report,
    })
}

fn streamed_nowledge_mem_read_output(
    mode: NowledgeMemGraphMode,
    rows: Vec<BTreeMap<String, Value>>,
    streamed: QueryStreamReport,
    options: &NowledgeMemReadOptions,
) -> Result<NowledgeMemReadOutput> {
    let output = QueryOutput { rows: rows.into() };
    let mut report = nowledge_mem_read_report(mode, &output, options, &streamed.execution_profile);
    report.streaming = true;
    debug_assert_eq!(report.row_count, streamed.output_rows);
    debug_assert_eq!(
        report.estimated_payload_bytes,
        streamed.output_payload_bytes
    );
    Ok(NowledgeMemReadOutput { output, report })
}

fn legacy_nowledge_mem_read_error(
    error: HawDBError,
    options: &NowledgeMemReadOptions,
) -> HawDBError {
    let Some(max_payload_bytes) = options.max_estimated_payload_bytes else {
        return error;
    };
    if matches!(
        &error,
        HawDBError::ReadBudgetExceeded(cause)
            if cause.resource == ReadBudgetResource::PayloadBytes && cause.limit == max_payload_bytes
    ) {
        return HawDBError::read_budget_exceeded(ReadBudgetResource::PayloadBytes, max_payload_bytes, format!(
            "nowledge mem read query payload exceeding max_estimated_payload_bytes {max_payload_bytes}"
        ));
    }
    error
}

fn estimate_query_output_payload_bytes(output: &QueryOutput) -> usize {
    output
        .rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|(key, value)| key.len() + estimate_value_payload_bytes(value))
                .sum::<usize>()
        })
        .sum()
}

fn estimate_value_payload_bytes(value: &Value) -> usize {
    match value {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::Int(_) | Value::Float(_) => std::mem::size_of::<i64>(),
        Value::String(value) => value.len(),
        Value::Binary(value) => value.len(),
        Value::Uuid(_) => 16,
        Value::List(values) => values.iter().map(estimate_value_payload_bytes).sum(),
        Value::Map(values) => values
            .iter()
            .map(|(key, value)| key.len() + estimate_value_payload_bytes(value))
            .sum(),
    }
}

#[cfg(test)]
mod tests;

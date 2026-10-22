// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

//! Complete-E coverage handed directly to the registered publication protocol.

use super::*;
use crate::{
    PreparedReadDependencies, PreparedReadLimits, ProjectionBaselineMappingFunction,
    ProjectionCoverageConfig, ProjectionCoverageSession, ProjectionCoverageStep,
    ProjectionFeedError, ProjectionFeedLimits, ProjectionMappingFunction, ProjectionMappingLimits,
    ProjectionPublicationError, ProjectionSourceDurability, ProjectionStagingSession,
    ProjectionStagingSessionConfig, ProjectionStagingSessionStep,
};
use hawdb_search::{
    SearchComparedConsumerDocuments, SearchMutationSortStepLimits, SearchUnorderedMutationLimits,
};
use std::fmt;

/// Cumulative private-work limits for one registered C..E catch-up.
/// The facade derives C from the completed receipt and captures E itself.
/// Registered generation/resident limits and checkpoint layout remain the
/// existing Consumer contract; standalone Search build options do not apply.
#[derive(Debug, Clone)]
pub struct ProjectionConsumerCatchUpConfig {
    pub reads: PreparedReadLimits,
    pub feed: ProjectionFeedLimits,
    pub mapping: ProjectionMappingLimits,
    pub sorting: SearchUnorderedMutationLimits,
    /// Cancellation/deadline state without pre-bound resource controllers.
    pub task: RuntimeTaskContext,
}

/// Retains the typed refusal from lifecycle, preparation, or final activation.
#[derive(Debug)]
pub enum ProjectionConsumerCatchUpError {
    Consumer(SearchProjectionConsumerError),
    Feed(ProjectionFeedError),
    Publication(ProjectionPublicationError),
    Stopped(hawdb_core::RuntimeCancellationReason),
    RecoveryRetention {
        failure: Box<ProjectionConsumerCatchUpError>,
        retention: HawDBError,
    },
}
impl fmt::Display for ProjectionConsumerCatchUpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Consumer(error) => error.fmt(formatter),
            Self::Feed(error) => error.fmt(formatter),
            Self::Publication(error) => error.fmt(formatter),
            Self::Stopped(error) => error.fmt(formatter),
            Self::RecoveryRetention { failure, retention } => write!(
                formatter,
                "{failure}; retaining the publication owner also failed: {retention}"
            ),
        }
    }
}
impl std::error::Error for ProjectionConsumerCatchUpError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Consumer(error) => Some(error),
            Self::Feed(error) => Some(error),
            Self::Publication(error) => Some(error),
            Self::Stopped(error) => Some(error),
            Self::RecoveryRetention { failure, .. } => Some(failure.as_ref()),
        }
    }
}
impl From<SearchProjectionConsumerError> for ProjectionConsumerCatchUpError {
    fn from(error: SearchProjectionConsumerError) -> Self {
        Self::Consumer(error)
    }
}
impl From<HawDBError> for ProjectionConsumerCatchUpError {
    fn from(error: HawDBError) -> Self {
        Self::Consumer(error.into())
    }
}
impl From<ProjectionFeedError> for ProjectionConsumerCatchUpError {
    fn from(error: ProjectionFeedError) -> Self {
        Self::Feed(error)
    }
}
impl From<ProjectionPublicationError> for ProjectionConsumerCatchUpError {
    fn from(error: ProjectionPublicationError) -> Self {
        Self::Publication(error)
    }
}
type CatchUpResult<T> = std::result::Result<T, ProjectionConsumerCatchUpError>;

struct Base {
    receipt: CheckpointReceipt,
    _pin: SearchProjectionConsumerReadPin,
    _memory: hawdb_executor::QueryMemoryLease,
    task: RuntimeTaskContext,
}

/// Bounded invisible C..E work over this registration's exact selected base.
/// Source writers may continue between steps; the conservative full-source
/// coverage proof rejects activation after any post-E source commit.
pub struct ProjectionConsumerStagingSession<'consumer> {
    session: ProjectionStagingSession<'consumer>,
    base: Base,
}
impl<'consumer> ProjectionConsumerStagingSession<'consumer> {
    pub fn source_epoch(&self) -> u64 {
        self.session.source_epoch()
    }
    pub fn advance(
        &mut self,
        feed: ProjectionFeedLimits,
        ordering: SearchMutationSortStepLimits,
    ) -> CatchUpResult<ProjectionStagingSessionStep> {
        Ok(self.session.advance(feed, ordering)?)
    }
    /// The fixed full-source mapper is the trusted projection definition. Keep
    /// its version bound to the stable Consumer ID through final publication.
    pub fn verify_complete_source(
        self,
        config: ProjectionCoverageConfig,
        mapper: ProjectionBaselineMappingFunction,
    ) -> CatchUpResult<ProjectionConsumerCoverageSession<'consumer>> {
        let operations = self.session.sorting_report().input_operations;
        Ok(ProjectionConsumerCoverageSession {
            session: self.session.verify_complete_source(config, mapper)?,
            base: self.base,
            operations,
        })
    }
}

/// Independent full-E enumeration and exact comparison against the held base.
pub struct ProjectionConsumerCoverageSession<'consumer> {
    session: ProjectionCoverageSession<'consumer>,
    base: Base,
    operations: usize,
}
impl ProjectionConsumerCoverageSession<'_> {
    pub fn source_epoch(&self) -> u64 {
        self.session.source_epoch()
    }
    pub fn advance(
        &mut self,
        feed: ProjectionFeedLimits,
        ordering: SearchMutationSortStepLimits,
    ) -> CatchUpResult<ProjectionCoverageStep> {
        Ok(self.session.advance(feed, ordering)?)
    }
    /// Consume only a complete comparison. The resulting owned token retains
    /// the compared records and native base pin, and borrows no Consumer handle.
    pub fn finish(self) -> CatchUpResult<ProjectionCoveredConsumerUpdate> {
        let (dependencies, documents) = self.session.finish_consumer()?;
        Ok(ProjectionCoveredConsumerUpdate {
            dependencies,
            documents,
            base: self.base,
            operations: self.operations,
        })
    }
}

/// Opaque complete-E content, exact registration/base and source dependencies.
/// There is no mapper, document replacement, or intermediate watermark escape.
pub struct ProjectionCoveredConsumerUpdate {
    dependencies: PreparedReadDependencies,
    documents: SearchComparedConsumerDocuments,
    base: Base,
    operations: usize,
}
impl ProjectionCoveredConsumerUpdate {
    pub fn source_epoch(&self) -> u64 {
        self.dependencies.source_epoch()
    }
    pub fn base_epoch(&self) -> u64 {
        self.base.receipt.source_epoch
    }
}

impl Database {
    /// Begin source-coherent catch-up for one persistent Consumer. Every step
    /// remains private until complete independent E verification and native
    /// selection succeed. Dropping work keeps the last completed receipt.
    ///
    /// The starting epoch comes from the completed Consumer receipt. `task` carries
    /// no resource controllers; this method supplies the original database
    /// governor's admission. Cancellation/deadline state may be supplied.
    pub fn begin_search_projection_consumer_catch_up<'consumer>(
        &mut self,
        consumer: &'consumer SearchProjectionConsumer,
        config: ProjectionConsumerCatchUpConfig,
        mapper: ProjectionMappingFunction,
    ) -> CatchUpResult<ProjectionConsumerStagingSession<'consumer>> {
        self.consumer_writable_root()?;
        self.reload_consumer_registry_if_unavailable()?;
        self.validate_consumer_receipt(consumer)?;
        let receipt = consumer.projection().receipt();
        if config.task.has_resource_controllers() {
            return Err(
                HawDBError::Semantic("Consumer staging requires an unbound task".into()).into(),
            );
        }
        let mut config = ProjectionStagingSessionConfig {
            after_epoch: receipt.source_epoch,
            reads: config.reads,
            feed: config.feed,
            mapping: config.mapping,
            sorting: config.sorting,
            task: config.task,
            build: Default::default(),
        };
        let source =
            self.begin_prepared_projection_feed(config.after_epoch, config.reads, config.feed)?;
        config.build.source_graph_commit_epoch = Some(source.source_epoch());
        let permit = self.consumer_publication_permit()?;
        let pin = consumer.pin(&permit)?;
        let task = config.task.clone();
        let stage = consumer.projection().prepare_source_coherent_update(
            Arc::clone(&permit),
            std::mem::take(&mut config.build),
            config.sorting,
            task.clone(),
        )?;
        let bytes = std::mem::size_of::<ProjectionConsumerStagingSession<'_>>()
            .max(std::mem::size_of::<ProjectionConsumerCoverageSession<'_>>())
            .max(std::mem::size_of::<ProjectionCoveredConsumerUpdate>())
            + receipt.binding.consumer_id.len()
            + receipt.sha256.len();
        let memory = stage.reserve_session_metadata(bytes)?;
        let session = ProjectionStagingSession::from_source(source, stage, config, mapper)?;
        Ok(ProjectionConsumerStagingSession {
            session,
            base: Base {
                receipt,
                _pin: pin,
                _memory: memory,
                task,
            },
        })
    }

    /// Build and publish the already compared E contents through the stationary
    /// Consumer protocol, then acknowledge its durable native receipt. Final
    /// artifact construction is a blocking full-generation build. A lost reply
    /// uses `resolve_search_projection_consumer_publication` or verified reopen;
    /// it must not be treated as rollback or revalidated against a later E.
    pub fn publish_search_projection_consumer_covered_update(
        &mut self,
        consumer: &mut SearchProjectionConsumer,
        covered: ProjectionCoveredConsumerUpdate,
        durability: ProjectionSourceDurability,
    ) -> CatchUpResult<SearchProjectionConsumerCatchUpReport> {
        self.publish_covered_consumer_with_limits(
            consumer,
            covered,
            durability,
            RegisteredConsumerLimits::default(),
        )
    }

    pub(super) fn publish_covered_consumer_with_limits(
        &mut self,
        consumer: &mut SearchProjectionConsumer,
        covered: ProjectionCoveredConsumerUpdate,
        durability: ProjectionSourceDurability,
        limits: RegisteredConsumerLimits,
    ) -> CatchUpResult<SearchProjectionConsumerCatchUpReport> {
        let root = self.consumer_writable_root()?;
        self.reload_consumer_registry_if_unavailable()?;
        self.validate_consumer_receipt(consumer)?;
        let current = consumer.projection().receipt();
        if current != covered.base.receipt {
            return Err(HawDBError::TransactionConflict {
                read_epoch: covered.base.receipt.source_epoch,
                committed_epoch: current.source_epoch,
                key: "registered Consumer base or binding changed".into(),
            }
            .into());
        }
        let epoch = covered.source_epoch();
        self.validate_prepared_projection_source(&covered.dependencies, epoch, durability)?;
        // Task refusal before birth/intent leaves both selector and registry intact.
        covered
            .base
            .task
            .checkpoint()
            .map_err(ProjectionConsumerCatchUpError::Stopped)?;
        publication_failpoint(PublicationStage::BeforeCheckpoint)?;
        if epoch == current.source_epoch {
            return self.covered_consumer_report(consumer, epoch, epoch, 0, 0);
        }
        // Idle compared records retain no execution slot. Re-admit and settle
        // deterministic resident/reader refusals before reserving a birth.
        let permit = self.consumer_publication_permit()?;
        let documents = covered.documents.admit_publication(
            Arc::clone(&permit),
            covered.base.task.clone(),
            limits,
        )?;
        let recovery_memory = permit
            .reserve_retained_memory(
                hawdb_storage::consumer_allocation::CONSUMER_SELECTION_MEMORY_BOUND,
            )
            .map_err(|error| hawdb_core::ResourceBudgetError {
                resource: hawdb_core::ResourceBudgetKind::MemoryBytes,
                scope: hawdb_core::ResourceBudgetScope::SharedPool,
                owner: "covered Consumer recovery metadata".into(),
                memory_class: None,
                requested: error.requested,
                used: 0,
                budget: error.available,
                budget_option: "RuntimeGovernor.available_retained_memory_bytes",
                message: error.to_string(),
            })
            .map_err(HawDBError::from)?;
        let allocation = self
            .runtime
            .get()?
            .store
            .reserve_search_consumer_update(consumer.projection().stationary_receipt()?)?;
        let birth = consumer.projection().prepare_successor_birth(allocation)?;
        let locator = consumer.projection().registered_locator()?.to_owned();
        let preparation = documents.prepare_registered(
            birth,
            RegisteredConsumerPreparation {
                locator: &locator,
                epoch,
                permit: Arc::clone(&permit),
                task: covered.base.task.clone(),
                limits,
                recovery_memory,
                previous: Some(consumer.projection()),
            },
        );
        let mut prepared = match preparation {
            Ok(prepared) => prepared,
            Err(error) => return Err(retain_failed_owner(consumer, *error)),
        };
        let selection = (|| -> CatchUpResult<_> {
            publication_failpoint(PublicationStage::AfterCheckpoint)?;
            self.validate_prepared_projection_source(&covered.dependencies, epoch, durability)?;
            let runtime = self.runtime.get()?;
            runtime.store.verify_search_consumer_candidate_source(
                prepared.reservation_binding()?,
                epoch,
                prepared.report().metadata_reference,
            )?;
            let mut attempt = prepared.admit_selection(Arc::clone(&permit), covered.base.task)?;
            publication_failpoint(PublicationStage::BeforeSelector)?;
            let pending = attempt.select()?;
            drop(runtime);
            publication_failpoint(PublicationStage::AfterSelector)?;
            let receipt = pending.finish()?;
            publication_failpoint(PublicationStage::AfterSelectorBarrier)?;
            Ok(receipt)
        })();
        let selected = match selection {
            Ok(receipt) => receipt,
            Err(error) => {
                if let Err(retention) = consumer
                    .projection_mut()
                    .retain_pending_publication(prepared)
                {
                    return Err(ProjectionConsumerCatchUpError::RecoveryRetention {
                        failure: Box::new(error),
                        retention,
                    });
                }
                return Err(error);
            }
        };
        match prepared.complete(selected) {
            Ok(projection) => *consumer.projection_mut() = projection,
            Err(error) => return Err(retain_failed_owner(consumer, *error)),
        }
        let receipt = consumer.projection().receipt();
        self.acknowledge_consumer_checkpoint(consumer.id(), &receipt)?;
        self.publish_consumer_registry(&root)?;
        self.runtime
            .get_mut()?
            .projection_consumers
            .verified
            .insert(consumer.id().as_str().into(), State::Active);
        self.covered_consumer_report(
            consumer,
            current.source_epoch,
            receipt.source_epoch,
            1,
            covered.operations,
        )
    }

    fn covered_consumer_report(
        &self,
        consumer: &SearchProjectionConsumer,
        start: u64,
        end: u64,
        publications: usize,
        operations: usize,
    ) -> CatchUpResult<SearchProjectionConsumerCatchUpReport> {
        Ok(SearchProjectionConsumerCatchUpReport {
            catch_up: SearchProjectionCatchUpReport {
                graph_commit_epoch: end,
                start_applied_epoch: Some(start),
                start_durable_epoch: Some(start),
                end_applied_epoch: Some(end),
                end_durable_epoch: Some(end),
                applied_batch_count: publications,
                applied_operation_count: operations,
                complete: true,
            },
            consumer: self.search_projection_consumer_status(consumer.id())?,
        })
    }
}

fn retain_failed_owner(
    consumer: &mut SearchProjectionConsumer,
    error: RegisteredConsumerError,
) -> ProjectionConsumerCatchUpError {
    let (error, recovery) = error.into_parts();
    let failure = ProjectionConsumerCatchUpError::from(error);
    if let Some(recovery) = recovery
        && let Err(retention) = consumer
            .projection_mut()
            .retain_failed_preparation(recovery)
    {
        return ProjectionConsumerCatchUpError::RecoveryRetention {
            failure: Box::new(failure),
            retention,
        };
    }
    failure
}

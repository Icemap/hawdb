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
}
impl fmt::Display for ProjectionConsumerCatchUpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Consumer(error) => error.fmt(formatter),
            Self::Feed(error) => error.fmt(formatter),
            Self::Publication(error) => error.fmt(formatter),
        }
    }
}
impl std::error::Error for ProjectionConsumerCatchUpError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Consumer(error) => Some(error),
            Self::Feed(error) => Some(error),
            Self::Publication(error) => Some(error),
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
            permit,
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
            .documents
            .task_context()
            .checkpoint()
            .map_err(|error| HawDBError::Execution(error.to_string()))?;
        publication_failpoint(PublicationStage::BeforeCheckpoint)?;
        if epoch == current.source_epoch {
            return self.covered_consumer_report(consumer, epoch, epoch, 0, 0);
        }
        let permit = self.consumer_publication_permit()?;
        let allocation = self
            .runtime
            .get()?
            .store
            .reserve_search_consumer_update(consumer.projection().stationary_receipt()?)?;
        let mut birth = consumer.projection().prepare_successor_birth(allocation)?;
        birth.create()?;
        let locator = consumer.projection().registered_locator()?.to_owned();
        let mut prepared = covered.documents.prepare_registered(
            birth,
            RegisteredConsumerPreparation {
                locator: &locator,
                epoch,
                permit: Arc::clone(&permit),
                task: covered.base.task.clone(),
                limits: RegisteredConsumerLimits::default(),
                previous: Some(consumer.projection()),
            },
        )?;
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
                consumer
                    .projection_mut()
                    .retain_pending_publication(prepared)?;
                return Err(error);
            }
        };
        *consumer.projection_mut() = prepared.complete(selected)?;
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

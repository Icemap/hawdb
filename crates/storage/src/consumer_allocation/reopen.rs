// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

//! Reconstruct native read custody from a completed durable selection.
//! Saved locators are descriptions; the caller supplies the original parent.

use super::*;
use hawdb_core::RuntimeTaskContext;

// Covers bounded ledger decode/re-encode, one cloned selection/reservation,
// canonical birth-marker construction and native-owner metadata. The caller
// prepays this envelope on its existing account and retains it through this
// object's destruction. This is not allocator/RSS or serving qualification.
pub const CONSUMER_REOPEN_MEMORY_BOUND: u64 = 16 * MAX_BYTES as u64;

/// Captured source custody only, prepared without decoding the ledger under
/// SourceOwner. Execute after releasing that fence with fresh working/I/O/FD
/// admission. This request alone cannot authorize a read version.
#[doc(hidden)]
#[derive(Debug)]
pub struct ConsumerReopenRequest {
    source: ConsumerAllocationSource,
    database: Uuid,
    consumer: [u8; 128],
    consumer_length: u8,
    source_epoch: u64,
}

/// Exact native generation named by a completed selection. Retains its held
/// marker and directory; it grants no writer, deletion or new selector token.
/// Search must still verify the complete sealed metadata/artifact closure.
#[doc(hidden)]
#[derive(Debug)]
pub struct ReopenedConsumerGeneration {
    marker: File,
    directory: SourceDirectory,
    _parent: SourceDirectory,
    marker_bytes: Vec<u8>,
    receipt: ConsumerSelectionReceipt,
    request: ConsumerReopenRequest,
}

pub(crate) fn prepare(
    source: ConsumerAllocationSource,
    database: Uuid,
    consumer: &str,
    source_epoch: u64,
) -> Result<ConsumerReopenRequest> {
    if consumer.is_empty()
        || consumer.len() > 128
        || !consumer
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(invalid("invalid Consumer reopen identity"));
    }
    let mut identifier = [0; 128];
    identifier[..consumer.len()].copy_from_slice(consumer.as_bytes());
    Ok(ConsumerReopenRequest {
        source,
        database,
        consumer: identifier,
        consumer_length: consumer.len() as u8,
        source_epoch,
    })
}

impl ConsumerReopenRequest {
    fn consumer_id(&self) -> &str {
        std::str::from_utf8(&self.consumer[..usize::from(self.consumer_length)])
            .expect("validated ASCII Consumer identifier")
    }
    /// No saved locator is opened. `original_parent` must be an explicitly
    /// supplied native authority in this source's existing descriptor domain.
    /// The caller prepays CONSUMER_REOPEN_MEMORY_BOUND and owns one I/O wave;
    /// resource acquisition and this method run outside SourceOwner.
    pub fn open(
        self,
        original_parent: SourceDirectory,
        task: &RuntimeTaskContext,
    ) -> Result<ReopenedConsumerGeneration> {
        checkpoint(task)?;
        if !task.has_resource_controllers()
            || task
                .memory_reservation()
                .is_none_or(|r| r.memory_bytes() < CONSUMER_REOPEN_MEMORY_BOUND)
        {
            return Err(HawDBError::Execution(
                "Consumer reopen requires prepaid runtime admission".into(),
            ));
        }
        if !original_parent
            .shares_project_context(&self.source.owner.captured_directory().operation_context())
        {
            return Err(invalid(
                "Consumer reopen parent belongs to another descriptor domain",
            ));
        }
        let _descriptors = self
            .source
            .project
            .file_descriptors()
            .reserve_admission(12)?;
        let (receipt, marker_bytes) = self.with_completed(|_, _, ledger, selected| {
            if selected.seal.source_epoch > self.source_epoch {
                return Err(invalid("selected Consumer is ahead of the captured source"));
            }
            let row = ledger
                .pending
                .iter()
                .find(|row| row.operation == selected.operation)
                .ok_or_else(|| invalid("selected Consumer reservation is missing"))?;
            let marker_bytes = super::native_birth::marker_bytes(
                self.source.project.allocation_identity().project_id(),
                row,
                &selected.generation_name,
                &selected.parent_locator,
            )?;
            Ok((
                ConsumerSelectionReceipt {
                    selected: selected.clone(),
                },
                marker_bytes,
            ))
        })?;
        checkpoint(task)?;
        let context = original_parent.operation_context();
        let directory =
            original_parent.open_directory(&context, &receipt.selected.generation_name)?;
        let marker = directory.open_child(&context, super::native_birth::marker_file_name())?;
        let owner = ReopenedConsumerGeneration {
            marker,
            directory,
            _parent: original_parent,
            marker_bytes,
            receipt,
            request: self,
        };
        owner.verify_marker(task)?;
        owner.require_current(task)?;
        Ok(owner)
    }

    fn with_completed<T>(
        &self,
        action: impl FnOnce(&SourceDirectory, &FileOpenContext, &Ledger, &SelectedRoot) -> Result<T>,
    ) -> Result<T> {
        self.source
            .project
            .with_locked_catalog(|root, context, catalog| {
                check_source_custody(
                    &self.source.project,
                    &self.source.owner,
                    self.source.branch,
                    &catalog,
                )?;
                let mut file = root.open_mutable_child(context, LEDGER_FILE, false)?;
                let ledger = decode_held(&mut file)?;
                if selector(&ledger)? != self.source.project.allocation_identity() {
                    return Err(invalid("Consumer reopen project differs"));
                }
                let branch = self.source.branch.as_uuid().to_string();
                let database = self.database.to_string();
                let matches = |row: &Reservation| {
                    row.branch_id == branch
                        && row.database_uuid == database
                        && row.consumer_id == self.consumer_id()
                };
                if ledger.pending.iter().any(|row| {
                    matches(row)
                        && row
                            .publication_intent
                            .as_ref()
                            .is_some_and(PublicationIntent::pending)
                }) {
                    return Err(HawDBError::ProjectionBusy);
                }
                let selected = ledger
                    .selected
                    .iter()
                    .find(|s| {
                        s.consumer_key() == (branch.as_str(), database.as_str(), self.consumer_id())
                    })
                    .ok_or_else(|| invalid("Consumer has no completed selected generation"))?;
                let row = ledger
                    .pending
                    .iter()
                    .find(|row| row.operation == selected.operation && matches(row))
                    .ok_or_else(|| invalid("selected Consumer reservation is missing"))?;
                if row
                    .publication_intent
                    .as_ref()
                    .is_none_or(|intent| !intent.receipt_complete)
                    || SelectedRoot::from_candidate(row)? != *selected
                {
                    return Err(invalid("Consumer selected receipt is incomplete"));
                }
                // A readable complete image can follow a lost final barrier.
                // Complete barriers on that same held image before granting custody.
                file.sync_all()?;
                root.sync(context)?;
                if !root.matches_child_file(context, LEDGER_FILE, &file)? {
                    return Err(HawDBError::ProjectionBusy);
                }
                let result = action(root, context, &ledger, selected)?;
                if !root.matches_child_file(context, LEDGER_FILE, &file)? {
                    return Err(HawDBError::ProjectionBusy);
                }
                Ok(result)
            })
    }
}

impl ReopenedConsumerGeneration {
    pub fn receipt(&self) -> &ConsumerSelectionReceipt {
        &self.receipt
    }
    pub fn generation_name(&self) -> &str {
        &self.receipt.selected.generation_name
    }

    /// Check native marker identity/content without reopening a saved path.
    /// This does not certify that this historical generation is still selected.
    pub fn directory(&self, task: &RuntimeTaskContext) -> Result<&SourceDirectory> {
        self.verify_marker(task)?;
        Ok(&self.directory)
    }

    /// Require the same completed selector after expensive reader construction.
    /// A subsequent host registry/read-slot fence remains a separate contract.
    pub fn require_current(&self, task: &RuntimeTaskContext) -> Result<()> {
        checkpoint(task)?;
        self.request.with_completed(|_, _, _, selected| {
            if selected != &self.receipt.selected {
                return Err(HawDBError::ProjectionBusy);
            }
            Ok(())
        })?;
        checkpoint(task)
    }

    fn verify_marker(&self, task: &RuntimeTaskContext) -> Result<()> {
        checkpoint(task)?;
        let context = self.directory.operation_context();
        let name = super::native_birth::marker_file_name();
        if !self
            .directory
            .matches_child_file(&context, name, &self.marker)?
        {
            return Err(invalid("reopened Consumer birth marker identity changed"));
        }
        let mut observed = self.directory.open_child(&context, name)?;
        let metadata = observed.metadata()?;
        if !metadata.is_file() || metadata.len() != self.marker_bytes.len() as u64 {
            return Err(invalid("reopened Consumer birth marker length changed"));
        }
        let mut buffer = [0u8; 8192];
        for expected in self.marker_bytes.chunks(buffer.len()) {
            checkpoint(task)?;
            observed.read_exact(&mut buffer[..expected.len()])?;
            if &buffer[..expected.len()] != expected {
                return Err(invalid("reopened Consumer birth marker bytes changed"));
            }
        }
        if observed.read(&mut buffer[..1])? != 0
            || !self
                .directory
                .matches_child_file(&context, name, &observed)?
            || !self
                .directory
                .matches_child_file(&context, name, &self.marker)?
        {
            return Err(invalid("reopened Consumer birth marker changed"));
        }
        Ok(())
    }
}

fn checkpoint(task: &RuntimeTaskContext) -> Result<()> {
    task.checkpoint()
        .map_err(|reason| HawDBError::Execution(format!("Consumer reopen {reason}")))
}

// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug)]
pub(super) struct Authority {
    // The owned preparation keeps this root's actual native publication lease,
    // including process-local directory identity, alive after owner closure.
    _lease: SearchProjectionPublishLease,
    busy: AtomicBool,
}

impl Authority {
    pub(super) fn new(lease: SearchProjectionPublishLease) -> Arc<Self> {
        Arc::new(Self {
            _lease: lease,
            busy: AtomicBool::new(false),
        })
    }

    pub(super) fn acquire(self: &Arc<Self>) -> Result<ConsumerPreparationLease> {
        self.busy
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .map_err(|_| HawDBError::ProjectionBusy)?;
        Ok(ConsumerPreparationLease {
            authority: Arc::clone(self),
        })
    }

    pub(super) fn require_available(&self) -> Result<()> {
        if self.busy.load(Ordering::Acquire) {
            Err(HawDBError::ProjectionBusy)
        } else {
            Ok(())
        }
    }
}

/// Exclusive target custody for preparation outside the source owner.
///
/// Retains the native publication lease even if its consumer handle closes.
/// Drop releases operation exclusion; the durable consumer registration remains.
/// This does not certify a source snapshot, C..E closure, target incarnation,
/// artifact installation, source durability or activation. It cannot bypass the
/// generic writer's registered-root rejection. No per-operation heap allocation
/// is performed: the token retains the consumer's existing authority allocation.
#[derive(Debug)]
#[must_use = "retain preparation custody until work finishes or is cancelled"]
pub struct ConsumerPreparationLease {
    authority: Arc<Authority>,
}

impl Drop for ConsumerPreparationLease {
    fn drop(&mut self) {
        self.authority.busy.store(false, Ordering::Release);
    }
}

impl ConsumerProjection {
    /// Kernel entry for the facade's registered-consumer preparation admission.
    /// A completed receipt must match the current clean resident checkpoint.
    #[doc(hidden)]
    pub fn reserve_preparation(&mut self) -> Result<ConsumerPreparationLease> {
        let operation = self.authority.acquire()?;
        let binding = self
            .index
            .consumer_binding
            .as_ref()
            .ok_or_else(|| invalid("preparation requires a bound consumer checkpoint"))?;
        let receipt = self
            .index
            .consumer_receipt
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let receipt = receipt
            .as_ref()
            .ok_or_else(|| invalid("preparation requires a completed consumer checkpoint"))?;
        let durable_epoch = *self
            .index
            .durable_source_graph_commit_epoch
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !self.index.consumer_binding_valid
            || self.index.path.is_none()
            || &receipt.binding != binding
            || self.pending_mutation
            || self.index.source_graph_commit_epoch != durable_epoch
            || self.index.source_graph_commit_epoch != Some(receipt.source_epoch)
        {
            return Err(invalid(
                "preparation requires the matching clean completed checkpoint",
            ));
        }
        Ok(operation)
    }

    /// Includes same-epoch edits and unsuccessful mutation/checkpoint attempts.
    #[doc(hidden)]
    pub fn has_pending_mutation(&self) -> bool {
        self.pending_mutation
    }

    /// Check before source hydration; actual mutation still acquires the slot.
    #[doc(hidden)]
    pub fn require_operation_available(&self) -> Result<()> {
        self.authority.require_available()
    }
}

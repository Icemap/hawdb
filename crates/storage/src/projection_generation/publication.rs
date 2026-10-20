// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

//! Catalog selection custody. This is not source validation or a Consumer lease.

use super::*;
use crate::file_io::{CapturedControlCandidate, PendingControlPublication};
use crate::native_file_identity::{self, NativeIdentity};
use std::sync::TryLockError;

#[derive(Debug)]
pub enum ProjectionGenerationPreparation {
    AlreadyPublished(ProjectionGenerationPublishReport),
    Prepared(Box<PreparedProjectionPublication>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionGenerationResolution {
    Published(ProjectionGenerationPublishReport),
    NotSelected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Prepared,
    Attempted,
    Uncertain,
}

/// Keeping the original file open prevents native file-ID reuse while a
/// prepared or uncertain operation still compares the old selector identity.
#[derive(Debug)]
enum ExpectedHead {
    Absent,
    Present {
        _file: File,
        identity: NativeIdentity,
    },
}

impl ExpectedHead {
    fn matches(&self, observed: Option<NativeIdentity>) -> bool {
        match self {
            Self::Absent => observed.is_none(),
            Self::Present { identity, .. } => observed == Some(*identity),
        }
    }
}

#[derive(Debug)]
pub(super) struct PublicationSlot {
    nonce: u64,
    generation: String,
    phase: Phase,
    expected: ExpectedHead,
}

#[derive(Debug)]
struct Reservation {
    shared: Arc<Mutex<ProjectionGenerationShared>>,
    owner: String,
    nonce: u64,
    closed: bool,
    revoked: bool,
}

impl Reservation {
    fn close(&mut self) -> Result<(), ProjectionGenerationError> {
        let mut shared = self
            .shared
            .lock()
            .map_err(|_| ProjectionGenerationError::PublicationUncertain)?;
        if !shared
            .publications
            .get(&self.owner)
            .is_some_and(|slot| slot.nonce == self.nonce)
        {
            return Err(ProjectionGenerationError::PublicationUncertain);
        }
        let retired = shared.publications.remove(&self.owner);
        self.closed = true;
        drop(shared);
        drop(retired);
        Ok(())
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        let Ok(mut shared) = self.shared.lock() else {
            return;
        };
        let Some(slot) = shared.publications.get_mut(&self.owner) else {
            return;
        };
        if slot.nonce != self.nonce {
            return;
        }
        let retired = if slot.phase == Phase::Prepared {
            shared.publications.remove(&self.owner)
        } else {
            slot.phase = Phase::Uncertain;
            None
        };
        drop(shared);
        drop(retired);
    }
}

/// Owns pre-encoded, synchronized selector bytes and an exclusive process-local
/// owner reservation. The host must independently retain its real interprocess
/// publication authority, validate the source and exchange its read view under
/// the source fence. This token alone never establishes Consumer readiness.
/// Drop never deletes candidate or selected evidence.
#[derive(Debug)]
#[must_use]
pub struct PreparedProjectionPublication {
    // Close control and reservation before releasing the captured catalog.
    control: CapturedControlCandidate<'static>,
    reservation: Reservation,
    sealed: SealedProjectionGeneration,
    store: ProjectionGenerationStore,
    report: ProjectionGenerationPublishReport,
    head_name: String,
}

/// Borrows the original operation until its barriers finish outside the source
/// fence. Dropping this value does not acknowledge or roll back selection; the
/// prepared owner remains attempted until resolved or dropped into uncertainty.
#[derive(Debug)]
#[must_use]
pub struct PendingProjectionPublication<'a> {
    control: PendingControlPublication<'a, 'static>,
    reservation: &'a mut Reservation,
    store: &'a ProjectionGenerationStore,
    report: &'a ProjectionGenerationPublishReport,
}

impl PreparedProjectionPublication {
    pub fn identity(&self) -> &ProjectionGenerationIdentity {
        &self.sealed.manifest.begin.identity
    }

    pub fn source_watermark(&self) -> u64 {
        self.sealed.manifest.begin.source_watermark
    }

    pub fn matches_catalog(&self, store: &ProjectionGenerationStore) -> bool {
        Arc::ptr_eq(&self.store.shared, &store.shared)
    }

    /// Revalidate the retained old selector identity and perform native atomic
    /// selection. Does not serialize manifests, synchronize files/directories,
    /// wait for the catalog mutex, clone the report, or consume rejected owners.
    /// For an unattempted token, `Busy` preserves preparation: retry `select`
    /// on the same token after contention clears.
    /// Native calls may still block; this is not a hard latency guarantee.
    pub fn select(
        &mut self,
    ) -> Result<PendingProjectionPublication<'_>, ProjectionGenerationError> {
        if self.reservation.closed || self.reservation.revoked {
            return Err(ProjectionGenerationError::Conflict(
                "publication token is no longer selectable".into(),
            ));
        }
        let mut shared = match self.store.shared.try_lock() {
            Ok(shared) => shared,
            Err(TryLockError::WouldBlock) => return Err(ProjectionGenerationError::Busy),
            Err(TryLockError::Poisoned(_)) => {
                return Err(ProjectionGenerationError::PublicationUncertain)
            }
        };
        let slot = shared
            .publications
            .get_mut(&self.reservation.owner)
            .filter(|slot| slot.nonce == self.reservation.nonce)
            .ok_or(ProjectionGenerationError::PublicationUncertain)?;
        if slot.phase != Phase::Prepared {
            return Err(ProjectionGenerationError::PublicationUncertain);
        }
        if !slot
            .expected
            .matches(self.store.head_identity(&self.head_name)?)
        {
            self.reservation.revoked = true;
            return Err(ProjectionGenerationError::Conflict(
                "prepared selector changed before selection".into(),
            ));
        }
        // Revoke before entering native code, including errors and unwinding.
        slot.phase = Phase::Attempted;
        let context = self.store.captured.heads.operation_context();
        let control = self.control.select_head(&context, &self.head_name)?;
        #[cfg(test)]
        if FAIL_SELECTION_BARRIER.with(|failure| failure.replace(false)) {
            return Err(io::Error::other("injected lost selector durability response").into());
        }
        drop(shared);
        Ok(PendingProjectionPublication {
            control,
            reservation: &mut self.reservation,
            store: &self.store,
            report: &self.report,
        })
    }

    /// Resolve this live owner's attempted selection outside the source fence.
    /// A resolved operation cannot be retried: prepare a new owner if necessary.
    /// Reusing the selected generation identity with different content returns
    /// `Conflict`. Retrying unchanged content cannot resolve that conflict;
    /// the reservation still protects both recovery roots from GC.
    pub fn resolve(&mut self) -> Result<ProjectionGenerationResolution, ProjectionGenerationError> {
        if self.reservation.closed {
            return Err(ProjectionGenerationError::Conflict(
                "publication token was already resolved".into(),
            ));
        }
        let result = self
            .store
            .resolve_publication(&self.sealed, Some(self.reservation.nonce))?;
        self.reservation.closed = true;
        Ok(result)
    }
}

impl PendingProjectionPublication<'_> {
    pub fn finish(self) -> Result<ProjectionGenerationPublishReport, ProjectionGenerationError> {
        self.control
            .finish(&self.store.captured.heads.operation_context())?;
        self.reservation.close()?;
        Ok(self.report.clone())
    }
}

impl ProjectionGenerationStore {
    /// Prepare all selector bytes and dependency barriers outside SourceOwner.
    /// Reservation is process-local; a host retains its actual target lease.
    pub fn prepare_publication(
        &self,
        sealed: &SealedProjectionGeneration,
    ) -> Result<ProjectionGenerationPreparation, ProjectionGenerationError> {
        self.ensure_writable()?;
        if !Arc::ptr_eq(&self.shared, &sealed._pin.store.shared) {
            return Err(ProjectionGenerationError::Conflict(
                "sealed generation belongs to a different catalog".into(),
            ));
        }
        let mut shared = self.lock_shared()?;
        let identity = &sealed.manifest.begin.identity;
        let owner = owner_key_hash(&identity.projection, &identity.owner_key);
        if let Some(slot) = shared.publications.get(&owner) {
            return Err(if slot.phase == Phase::Uncertain {
                ProjectionGenerationError::PublicationUncertain
            } else {
                ProjectionGenerationError::Busy
            });
        }
        let manifest = self.read_manifest(&self.manifest_path(identity))?;
        if manifest != sealed.manifest {
            return Err(ProjectionGenerationError::Conflict(
                "sealed generation token does not match durable candidate metadata".into(),
            ));
        }
        let head_path = self.head_path(&identity.projection, &identity.owner_key);
        let (current, expected) = match self.open_file(&head_path) {
            Ok(mut file) => {
                let identity = file.with_native(native_file_identity::of_file)?;
                let head = decode_head(&read_bounded_file(
                    &mut file,
                    &head_path,
                    MAX_ENVELOPE_BYTES,
                )?)?;
                (
                    Some(head),
                    ExpectedHead::Present {
                        _file: file,
                        identity,
                    },
                )
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => (None, ExpectedHead::Absent),
            Err(error) => return Err(error.into()),
        };
        if let Some(current) = &current {
            validate_head_owner(current, identity)?;
            if current.manifest.begin.identity.generation == identity.generation {
                if current.manifest != manifest {
                    return Err(ProjectionGenerationError::Conflict(
                        "published generation identity was reused with different content".into(),
                    ));
                }
                self.stabilize_selected_head(&head_path, current)?;
                return Ok(ProjectionGenerationPreparation::AlreadyPublished(
                    ProjectionGenerationPublishReport {
                        identity: identity.clone(),
                        previous_generation: Some(identity.generation.clone()),
                        publication_commit_epoch: current.publication_commit_epoch,
                        idempotent: true,
                    },
                ));
            }
            if manifest.begin.source_watermark < current.manifest.begin.source_watermark {
                return Err(ProjectionGenerationError::Conflict(
                    "source watermark is older than the active watermark".into(),
                ));
            }
        }
        let observed = current
            .as_ref()
            .map(|head| head.manifest.begin.identity.generation.as_str());
        if observed != manifest.begin.expected_head.as_deref() {
            return Err(ProjectionGenerationError::Conflict(
                "prepared generation expected head does not match".into(),
            ));
        }
        let epoch = current
            .as_ref()
            .map_or(Some(1), |head| head.publication_commit_epoch.checked_add(1))
            .ok_or_else(|| {
                ProjectionGenerationError::Admission("publication epoch exhausted".into())
            })?;
        let nonce = shared.publication_sequence.checked_add(1).ok_or_else(|| {
            ProjectionGenerationError::Admission(
                "publication reservation sequence exhausted".into(),
            )
        })?;
        if let Some(head) = &current {
            self.stabilize_selected_head(&head_path, head)?;
        }
        let head = ProjectionGenerationHead {
            publication_commit_epoch: epoch,
            manifest,
        };
        let bytes = encode_head(&head)?;
        let temporary = temporary_path(&head_path, &mut shared);
        let source = temporary
            .file_name()
            .and_then(|name| name.to_str())
            .expect("generated ASCII name");
        let heads = &self.captured.heads;
        let context = heads.operation_context();
        let mut control = heads.create_owned_control_candidate(&context, source)?;
        control.write_all(&bytes)?;
        control.seal()?;
        let report = ProjectionGenerationPublishReport {
            identity: identity.clone(),
            previous_generation: current.map(|head| head.manifest.begin.identity.generation),
            publication_commit_epoch: epoch,
            idempotent: false,
        };
        let mut prepared = Box::new(PreparedProjectionPublication {
            control,
            reservation: Reservation {
                shared: self.shared.clone(),
                owner: owner.clone(),
                nonce,
                closed: true,
                revoked: false,
            },
            sealed: sealed.clone(),
            store: self.clone(),
            report,
            head_name: format!("{owner}.{HEAD_SUFFIX}"),
        });
        shared.publication_sequence = nonce;
        shared.publications.insert(
            owner,
            PublicationSlot {
                nonce,
                generation: identity.generation.clone(),
                phase: Phase::Prepared,
                expected,
            },
        );
        prepared.reservation.closed = false;
        drop(shared);
        Ok(ProjectionGenerationPreparation::Prepared(prepared))
    }

    fn head_identity(
        &self,
        name: &str,
    ) -> Result<Option<NativeIdentity>, ProjectionGenerationError> {
        let heads = &self.captured.heads;
        match heads.open_child(&heads.operation_context(), name) {
            Ok(file) => Ok(Some(file.with_native(native_file_identity::of_file)?)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Only the matching live nonce or an abandoned uncertain operation can
    /// finish recovery. Another caller cannot retire a live pending operation.
    pub(super) fn resolve_publication(
        &self,
        sealed: &SealedProjectionGeneration,
        nonce: Option<u64>,
    ) -> Result<ProjectionGenerationResolution, ProjectionGenerationError> {
        self.ensure_writable()?;
        let identity = &sealed.manifest.begin.identity;
        let owner = owner_key_hash(&identity.projection, &identity.owner_key);
        let mut shared = self.lock_shared()?;
        let slot = shared
            .publications
            .get(&owner)
            .ok_or(ProjectionGenerationError::PublicationUncertain)?;
        let allowed = match nonce {
            Some(nonce) => slot.nonce == nonce && slot.phase == Phase::Attempted,
            None => slot.phase == Phase::Uncertain,
        };
        if !allowed {
            return Err(ProjectionGenerationError::Busy);
        }
        if slot.generation != identity.generation {
            return Err(ProjectionGenerationError::PublicationUncertain);
        }
        let path = self.head_path(&identity.projection, &identity.owner_key);
        let head = self.read_optional_head(&path)?;
        let result = if let Some(head) =
            head.filter(|head| head.manifest.begin.identity.generation == identity.generation)
        {
            validate_head_owner(&head, identity)?;
            if head.manifest != sealed.manifest {
                // A content conflict is terminal for these bytes, but does not
                // establish a durable outcome or release either recovery root.
                return Err(ProjectionGenerationError::Conflict(
                    "published generation identity was reused with different content".into(),
                ));
            }
            self.stabilize_selected_head(&path, &head)?;
            ProjectionGenerationResolution::Published(ProjectionGenerationPublishReport {
                identity: identity.clone(),
                previous_generation: Some(identity.generation.clone()),
                publication_commit_epoch: head.publication_commit_epoch,
                idempotent: true,
            })
        } else if slot
            .expected
            .matches(self.head_identity(&format!("{owner}.{HEAD_SUFFIX}"))?)
        {
            ProjectionGenerationResolution::NotSelected
        } else {
            return Err(ProjectionGenerationError::PublicationUncertain);
        };
        let retired = shared.publications.remove(&owner);
        drop(shared);
        drop(retired);
        Ok(result)
    }
}

pub(super) fn pending_state(
    shared: &ProjectionGenerationShared,
    identity: &ProjectionGenerationIdentity,
) -> Option<ProjectionGenerationState> {
    shared
        .publications
        .get(&owner_key_hash(&identity.projection, &identity.owner_key))
        .filter(|slot| slot.generation == identity.generation)
        .map(|slot| {
            if slot.phase == Phase::Uncertain {
                ProjectionGenerationState::PublicationUncertain
            } else {
                ProjectionGenerationState::PublicationPending
            }
        })
}

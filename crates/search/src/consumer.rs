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

//! Internal projection ownership used by the embedded database facade.
use super::{SearchIndex, SEARCH_SNAPSHOT_FILE};
use crate::build_memory::BuildMemory;
use crate::error::{HawDBError, Result};
use crate::out_of_core::SearchProjectionPublishLease;
use hawdb_core::{RuntimeTaskContext, Uuid};
use hawdb_integrity::IntegrityHasher;
use hawdb_storage::consumer_allocation::PendingConsumerGeneration;
use hawdb_storage::file_io::{self as fs, File};
use hawdb_storage::immutable_object::{ObjectKind, ObjectReference, ObjectReferenceWriter};
use std::io::{Read, Write};
use std::path::Path;

const GENERATION_METADATA_FILE: &str = "consumer-generation.meta.hawdb";

mod control;
#[doc(hidden)]
pub mod generation_metadata;
mod operation;
pub(crate) use control::require_unregistered_directory;
pub use operation::ConsumerPreparationLease;
pub(crate) use operation::{Authority as MutationOwner, MutationPermit};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumerBinding {
    pub database_uuid: Uuid,
    pub projection_uuid: Uuid,
    pub consumer_id: String,
    pub registration_uuid: Uuid,
    pub checkpoint_uuid: Uuid,
}

fn valid_consumer_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

impl ConsumerBinding {
    pub(super) fn parse(
        database: &str,
        projection: &str,
        id: &str,
        registration: &str,
        checkpoint: &str,
    ) -> Result<Self> {
        if !valid_consumer_id(id) {
            return Err(invalid("invalid consumer ID"));
        }
        Ok(Self {
            database_uuid: canonical_uuid(database)?,
            projection_uuid: canonical_uuid(projection)?,
            consumer_id: id.into(),
            registration_uuid: canonical_uuid(registration)?,
            checkpoint_uuid: canonical_uuid(checkpoint)?,
        })
    }

    pub(super) fn record(&self) -> String {
        format!(
            "projection_consumer_binding\t{}\t{}\t{}\t{}\t{}\n",
            self.database_uuid,
            self.projection_uuid,
            self.consumer_id,
            self.registration_uuid,
            self.checkpoint_uuid
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointReceipt {
    pub binding: ConsumerBinding,
    pub source_epoch: u64,
    pub encoded_len: u64,
    pub sha256: String,
}

/// Native target ownership for a born but unpublished Consumer generation.
/// The birth token stays borrowed so failures cannot silently discard its
/// durable debt while a target publication lease is active. This grants no
/// serving view, source watermark or selector publication authority.
#[doc(hidden)]
#[derive(Debug)]
pub struct ConsumerStationaryTargetLease<'birth> {
    _birth: &'birth mut PendingConsumerGeneration,
    lease: SearchProjectionPublishLease,
    memory: BuildMemory,
    task: RuntimeTaskContext,
    candidate_output_begun: bool,
}

/// One bounded native output operation within a born Consumer generation.
/// Completed files remain private until a separate complete selector protocol
/// publishes them. Failed writes retain their attempted disk charge and the
/// birth token's durable debt; dropping this value never removes a file.
#[doc(hidden)]
#[derive(Debug)]
pub struct ConsumerCandidateOutput<'lease, 'birth> {
    target: &'lease mut ConsumerStationaryTargetLease<'birth>,
    memory: BuildMemory,
    task: RuntimeTaskContext,
    remaining_bytes: u64,
    metadata_seal_attempted: bool,
}

/// Producer evidence for exactly the bytes copied into one native target file.
/// This is neither a complete generation closure nor a serving receipt.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsumerCandidateArtifact {
    pub length: u64,
    pub reference: ObjectReference,
}

/// Producer evidence for a private, bounded generation metadata file. It is
/// not a selector or a serving receipt.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsumerCandidateMetadata {
    pub encoded_len: u64,
    pub reference: ObjectReference,
}

impl<'birth> ConsumerStationaryTargetLease<'birth> {
    /// Acquire against the original native generation, never its saved path.
    /// A failed acquisition leaves the caller's birth token available for
    /// resolution and conservatively retains exposed-birth debt.
    pub fn acquire(
        birth: &'birth mut PendingConsumerGeneration,
        task: &RuntimeTaskContext,
    ) -> Result<Self> {
        let memory = BuildMemory::new(task)?;
        let born = birth.born_directory()?;
        let directory = born.duplicate(&born.operation_context())?;
        let lease = SearchProjectionPublishLease::acquire_captured(directory, &memory, task)?;
        Ok(Self {
            _birth: birth,
            lease,
            memory,
            task: task.clone(),
            candidate_output_begun: false,
        })
    }

    /// Physical target identity, useful for later selector/birth binding.
    pub fn source_identity(&self) -> hawdb_storage::file_io::SourceDirectoryIdentity {
        self.lease.source_identity()
    }

    /// Compare metadata identities with this token's original recorded
    /// reservation. Source epoch and artifact closure need separate proof.
    pub fn validate_generation_identity(
        &self,
        input: &generation_metadata::MetadataInput<'_>,
    ) -> Result<()> {
        let original = self._birth.reservation_binding()?;
        if input.binding.database_uuid != original.database_uuid
            || input.binding.projection_uuid != original.projection_uuid
            || input.binding.consumer_id != original.consumer_id
            || input.binding.registration_uuid != original.registration_uuid
            || input.binding.checkpoint_uuid != original.checkpoint_uuid
            || input.target_incarnation != original.target_incarnation
            || input.generation != original.generation
            || input.operation != original.operation
        {
            return Err(HawDBError::StorageIntegrity(
                "consumer generation metadata differs from native reservation".into(),
            ));
        }
        Ok(())
    }

    /// Reserve a cumulative file-output bound once for this born target before
    /// installing any artifact. Dropping the token cannot reset its byte cap.
    /// The caller still owns the candidate's complete workspace and cleanup
    /// admission; this token only bounds the native output files it creates.
    pub fn begin_candidate_output(
        &mut self,
        max_bytes: u64,
    ) -> Result<ConsumerCandidateOutput<'_, 'birth>> {
        crate::build_control::checkpoint(&self.task)?;
        if max_bytes == 0 {
            return Err(invalid("consumer candidate output budget is empty"));
        }
        if self.candidate_output_begun {
            return Err(invalid("consumer candidate output was already started"));
        }
        self.candidate_output_begun = true;
        Ok(ConsumerCandidateOutput {
            memory: self.memory.clone(),
            task: self.task.clone(),
            target: self,
            remaining_bytes: max_bytes,
            metadata_seal_attempted: false,
        })
    }
}

impl ConsumerCandidateOutput<'_, '_> {
    pub fn remaining_bytes(&self) -> u64 {
        self.remaining_bytes
    }

    /// Copy from an already captured private source in the target's project
    /// descriptor domain. Both opens and the final name check are relative to
    /// original directory handles; no diagnostic path selects either file.
    /// The producer's prior full reference must match the copied bytes. The
    /// target must be a new child. An attempted output keeps its charged bytes
    /// even if a later copy or durability barrier fails.
    pub fn install_artifact(
        &mut self,
        source: &hawdb_storage::file_io::SourceDirectory,
        source_name: &str,
        target_name: &str,
        expected: ObjectReference,
    ) -> Result<ConsumerCandidateArtifact> {
        use crate::build_memory::SPOOL_BUFFER_BYTES;
        use std::num::NonZeroUsize;

        if self.metadata_seal_attempted {
            return Err(invalid(
                "consumer candidate metadata seal was already attempted",
            ));
        }
        crate::build_control::checkpoint(&self.task)?;
        let _scratch = self.memory.spool.reserve(SPOOL_BUFFER_BYTES)?;
        let permit = self
            .task
            .acquire_io_wave(NonZeroUsize::MIN)
            .map_err(|error| HawDBError::Execution(error.to_string()))?;
        if self.task.has_resource_controllers() && permit.is_none() {
            return Err(HawDBError::Execution(
                "consumer candidate output requires I/O admission".into(),
            ));
        }
        if expected.kind != ObjectKind::CheckpointArtifact || expected.format_version != 1 {
            return Err(HawDBError::StorageIntegrity(
                "consumer candidate producer reference has an unsupported kind or version".into(),
            ));
        }
        let source_context = source.operation_context();
        self.target.lease.with_captured_directory(|target| {
            let target_context = target.operation_context();
            if !source.shares_project_context(&target_context) {
                return Err(HawDBError::StorageIntegrity(
                    "consumer candidate source has a foreign descriptor domain".into(),
                ));
            }
            let mut input = source.open_child(&source_context, source_name)?;
            let metadata = input.metadata()?;
            if !metadata.is_file() {
                return Err(HawDBError::StorageIntegrity(
                    "consumer candidate source is not a file".into(),
                ));
            }
            let length = metadata.len();
            if length != expected.byte_length {
                return Err(HawDBError::StorageIntegrity(
                    "consumer candidate source length differs from producer reference".into(),
                ));
            }
            if length > self.remaining_bytes {
                return Err(HawDBError::Storage(format!(
                    "consumer candidate artifact requires {length} bytes, exceeding remaining {}",
                    self.remaining_bytes
                )));
            }
            let mut hasher = ObjectReferenceWriter::new(ObjectKind::CheckpointArtifact, 1, length)
                .map_err(|error| HawDBError::StorageIntegrity(error.to_string()))?;
            // Charge before the first native target effect. A failed attempt
            // cannot reuse capacity while its partial file remains on disk.
            self.remaining_bytes -= length;
            let mut output = target.create_child_new(&target_context, target_name)?;
            let mut copied = 0u64;
            let mut buffer = [0u8; SPOOL_BUFFER_BYTES];
            loop {
                crate::build_control::checkpoint(&self.task)?;
                let read = input.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                copied = copied
                    .checked_add(read as u64)
                    .filter(|copied| *copied <= length)
                    .ok_or_else(|| {
                        HawDBError::StorageIntegrity(
                            "consumer candidate source grew during copy".into(),
                        )
                    })?;
                output.write_all(&buffer[..read])?;
                hasher.write_all(&buffer[..read])?;
            }
            if copied != length {
                return Err(HawDBError::StorageIntegrity(
                    "consumer candidate source shrank during copy".into(),
                ));
            }
            let reference = hasher
                .finish()
                .map_err(|error| HawDBError::StorageIntegrity(error.to_string()))?;
            if reference != expected {
                return Err(HawDBError::StorageIntegrity(
                    "consumer candidate source bytes differ from producer reference".into(),
                ));
            }
            if !target.matches_child_file(&target_context, target_name, &output)? {
                return Err(HawDBError::StorageIntegrity(
                    "consumer candidate target file identity changed".into(),
                ));
            }
            output.sync_all()?;
            target.sync(&target_context)?;
            Ok(ConsumerCandidateArtifact { length, reference })
        })
    }

    /// Verify the complete described private file set against producer
    /// references, then write bounded metadata in this same captured target.
    /// This checks the Search manifest's top-level artifact set and epoch;
    /// complete serving closure and source E are still required before a
    /// selector may be prepared. A seal attempt is never retried
    /// through a new token after a native validation or write uncertainty.
    pub fn seal_generation_metadata(
        &mut self,
        input: &generation_metadata::MetadataInput<'_>,
        limits: generation_metadata::MetadataLimits,
    ) -> Result<ConsumerCandidateMetadata> {
        use crate::build_memory::SPOOL_BUFFER_BYTES;
        use std::num::NonZeroUsize;

        if self.metadata_seal_attempted {
            return Err(invalid(
                "consumer candidate metadata seal was already attempted",
            ));
        }
        self.target.validate_generation_identity(input)?;
        let length = input.encoded_len(limits, &self.task)?;
        let length_u64 = length as u64;
        if length_u64 > self.remaining_bytes {
            return Err(HawDBError::Storage(format!(
                "consumer generation metadata requires {length_u64} bytes, exceeding remaining {}",
                self.remaining_bytes
            )));
        }
        let _frame = self.memory.spool.reserve(length)?;
        let _scratch = self.memory.spool.reserve(SPOOL_BUFFER_BYTES)?;
        let permit = self
            .task
            .acquire_io_wave(NonZeroUsize::MIN)
            .map_err(|error| HawDBError::Execution(error.to_string()))?;
        if self.task.has_resource_controllers() && permit.is_none() {
            return Err(HawDBError::Execution(
                "consumer generation metadata requires I/O admission".into(),
            ));
        }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(length).map_err(|error| {
            HawDBError::Execution(format!("consumer metadata allocation failed: {error}"))
        })?;
        if bytes.capacity() > length {
            return Err(HawDBError::Execution(
                "consumer metadata allocation exceeds admission".into(),
            ));
        }
        if input.encode_to(&mut bytes, limits, &self.task)? != length || bytes.len() != length {
            return Err(HawDBError::StorageIntegrity(
                "consumer generation metadata encoded length changed".into(),
            ));
        }
        let reference = ObjectReference::for_bytes(ObjectKind::CheckpointArtifact, 1, &bytes);
        self.metadata_seal_attempted = true;
        self.target.lease.with_captured_directory(|target| {
            let context = target.operation_context();
            let mut buffer = [0u8; SPOOL_BUFFER_BYTES];
            let manifest_len =
                usize::try_from(input.search_manifest.byte_length).map_err(|_| {
                    HawDBError::StorageIntegrity("consumer Search manifest length overflows".into())
                })?;
            if input.search_manifest.byte_length
                > crate::out_of_core::MAX_OUT_OF_CORE_MANIFEST_BYTES
            {
                return Err(HawDBError::StorageIntegrity(
                    "consumer Search manifest exceeds its read limit".into(),
                ));
            }
            let _manifest_memory = self.memory.spool.reserve(manifest_len)?;
            let mut manifest = Vec::new();
            manifest.try_reserve_exact(manifest_len).map_err(|error| {
                HawDBError::Execution(format!(
                    "consumer Search manifest allocation failed: {error}"
                ))
            })?;
            if manifest.capacity() > manifest_len {
                return Err(HawDBError::Execution(
                    "consumer Search manifest allocation exceeds admission".into(),
                ));
            }
            verify_candidate_child(
                target,
                &context,
                crate::out_of_core::OUT_OF_CORE_MANIFEST_FILE,
                input.search_manifest,
                &self.task,
                &mut buffer,
                Some(&mut manifest),
            )?;
            let _decode_memory = self.memory.spool.reserve(
                crate::out_of_core::consumer_manifest_decode_capacity(&manifest, &self.task)?,
            )?;
            let resident_snapshot_absent = crate::out_of_core::validate_consumer_manifest_artifacts(
                &manifest,
                input.source_epoch,
                input.artifacts,
                &self.task,
            )?;
            if resident_snapshot_absent {
                match target.open_child(&context, crate::SEARCH_SNAPSHOT_FILE) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                    Ok(_) => {
                        return Err(HawDBError::StorageIntegrity(
                            "consumer candidate resident snapshot is present despite manifest absence proof"
                                .into(),
                        ));
                    }
                }
            }
            for artifact in input.artifacts {
                verify_candidate_child(
                    target,
                    &context,
                    artifact.name,
                    artifact.reference,
                    &self.task,
                    &mut buffer,
                    None,
                )?;
            }
            // Charge before the first metadata effect. Any uncertain write
            // retains both this cost and the original born-generation debt.
            self.remaining_bytes -= length_u64;
            let mut output = target.create_child_new(&context, GENERATION_METADATA_FILE)?;
            output.write_all(&bytes)?;
            if !target.matches_child_file(&context, GENERATION_METADATA_FILE, &output)? {
                return Err(HawDBError::StorageIntegrity(
                    "consumer metadata target file identity changed".into(),
                ));
            }
            output.sync_all()?;
            target.sync(&context)?;
            Ok(ConsumerCandidateMetadata {
                encoded_len: length_u64,
                reference,
            })
        })
    }
}

fn verify_candidate_child(
    target: &fs::SourceDirectory,
    context: &hawdb_storage::file_descriptors::FileOpenContext,
    name: &str,
    expected: ObjectReference,
    task: &RuntimeTaskContext,
    buffer: &mut [u8; crate::build_memory::SPOOL_BUFFER_BYTES],
    mut capture: Option<&mut Vec<u8>>,
) -> Result<()> {
    let mut file = target.open_child(context, name)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() != expected.byte_length {
        return Err(HawDBError::StorageIntegrity(
            "consumer generation child length differs from producer reference".into(),
        ));
    }
    let mut digest =
        ObjectReferenceWriter::new(expected.kind, expected.format_version, expected.byte_length)
            .map_err(|error| HawDBError::StorageIntegrity(error.to_string()))?;
    let mut read_bytes = 0u64;
    loop {
        crate::build_control::checkpoint(task)?;
        let read = file.read(buffer)?;
        if read == 0 {
            break;
        }
        read_bytes = read_bytes
            .checked_add(read as u64)
            .filter(|read_bytes| *read_bytes <= expected.byte_length)
            .ok_or_else(|| {
                HawDBError::StorageIntegrity("consumer generation child grew during seal".into())
            })?;
        digest.write_all(&buffer[..read])?;
        if let Some(bytes) = capture.as_mut() {
            bytes.extend_from_slice(&buffer[..read]);
        }
    }
    if read_bytes != expected.byte_length
        || digest
            .finish()
            .map_err(|error| HawDBError::StorageIntegrity(error.to_string()))?
            != expected
        || !target.matches_child_file(context, name, &file)?
    {
        return Err(HawDBError::StorageIntegrity(
            "consumer generation child differs from producer reference".into(),
        ));
    }
    Ok(())
}

#[derive(Debug)]
pub struct ConsumerProjection {
    index: SearchIndex,
    authority: std::sync::Arc<operation::Authority>,
    pending_mutation: bool,
}

impl ConsumerProjection {
    pub fn initialize<F>(
        root: &Path,
        binding: ConsumerBinding,
        source_epoch: u64,
        initialize: F,
    ) -> Result<Self>
    where
        F: FnOnce(&mut SearchIndex) -> Result<()>,
    {
        let project_files =
            hawdb_storage::file_descriptors::ProjectFileDescriptors::acquire_component(
                root, false,
            )?;
        let lease = SearchProjectionPublishLease::acquire_for_consumer(root)?;
        let authority = operation::Authority::new(lease);
        let operation = authority.acquire()?;
        let mut index = SearchIndex {
            path: Some(root.to_path_buf()),
            _project_files: Some(project_files),
            consumer_owner: std::sync::Arc::downgrade(&authority),
            consumer_mutation_generation: Some(operation.open_mutations()),
            ..SearchIndex::default()
        };
        {
            let scope = MutationScope {
                index: &mut index,
                operation,
            };
            initialize(scope.index)?;
        }
        if !authority.owns(&index)
            || index.path.as_deref() != Some(root)
            || index.consumer_binding.is_some()
            || index
                .import_source_graph_commit_epoch
                .is_some_and(|epoch| epoch > source_epoch)
        {
            return Err(invalid("initializer replaced the staging projection or supplied an impossible import epoch"));
        }
        if index.analyzer_lexicon != super::SearchAnalyzerLexicon::default() {
            return Err(invalid(
                "consumer reopen currently requires the default analyzer lexicon",
            ));
        }
        index.source_graph_commit_epoch = Some(source_epoch);
        index.consumer_binding = Some(binding);
        let mut owner = Self {
            index,
            authority,
            pending_mutation: true,
        };
        owner.checkpoint()?;
        Ok(owner)
    }

    pub fn open(root: &Path) -> Result<Self> {
        let _project_files =
            hawdb_storage::file_descriptors::ProjectFileDescriptors::acquire_component(
                root, false,
            )?;
        let lease = SearchProjectionPublishLease::acquire_for_consumer(root)?;
        let mut index = SearchIndex::open_under_lease(root, true, &lease)?;
        let authority = operation::Authority::new(lease);
        index.consumer_owner = std::sync::Arc::downgrade(&authority);
        let owner = Self {
            index,
            authority,
            pending_mutation: false,
        };
        let receipt = owner.read_receipt()?;
        *owner
            .index
            .consumer_receipt
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(receipt);
        Ok(owner)
    }

    pub fn index(&self) -> &SearchIndex {
        &self.index
    }

    pub fn binding(&self) -> &ConsumerBinding {
        self.index
            .consumer_binding
            .as_ref()
            .expect("consumer owns a bound projection")
    }

    pub fn receipt(&self) -> CheckpointReceipt {
        self.index
            .consumer_receipt
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .expect("consumer owns a completed checkpoint")
            .clone()
    }

    pub fn apply<T>(&mut self, apply: impl FnOnce(&mut SearchIndex) -> Result<T>) -> Result<T> {
        let operation = self.authority.acquire()?;
        self.authority.require_index(&self.index)?;
        self.pending_mutation = true;
        let result = {
            self.index.consumer_mutation_generation = Some(operation.open_mutations());
            let scope = MutationScope {
                index: &mut self.index,
                operation,
            };
            apply(scope.index)
        };
        if let Err(error) = self.authority.require_index(&self.index) {
            self.index.consumer_binding_valid = false;
            return Err(error);
        }
        result
    }

    pub fn binding_is_valid(&self) -> bool {
        self.index.consumer_binding_valid && self.authority.owns(&self.index)
    }

    pub fn checkpoint(&mut self) -> Result<CheckpointReceipt> {
        let operation = self.authority.acquire()?;
        self.authority.require_index(&self.index)?;
        if !self.index.consumer_binding_valid {
            return Err(invalid("untracked projection mutation"));
        }
        self.pending_mutation = true;
        self.index
            .consumer_binding
            .as_mut()
            .expect("bound consumer")
            .checkpoint_uuid = hawdb_core::uuidv7::generate_uuidv7()?;
        self.index.consumer_mutation_generation = Some(operation.open_mutations());
        {
            let scope = MutationScope {
                index: &mut self.index,
                operation,
            };
            scope
                .index
                .checkpoint_with_lease(Some(scope.operation.publication()))?;
        }
        self.pending_mutation = false;
        Ok(self.receipt())
    }

    /// Windows refuses before any namespace effect and retains the owner receipt.
    pub fn publish_directory(&mut self, destination: &Path) -> Result<()> {
        let operation = self.authority.acquire()?;
        self.authority.require_index(&self.index)?;
        // A child publication lock prevents moving its directory on Windows.
        // Keep native ownership and the completed receipt intact on refusal.
        if cfg!(windows) {
            return Err(HawDBError::CapabilityUnavailable {
                capability: hawdb_core::RuntimeCapability::SearchProjectionPublication,
            });
        }
        let pending_mutation = self.pending_mutation;
        self.pending_mutation = true;
        // Revoke before the native operation: an error may follow a completed
        // rename. Never remove a destination on the strength of that error.
        self.index.consumer_binding_valid = false;
        let source = self.index.path.take().expect("persistent consumer");
        if !operation.publication().matches_source_directory(&source)? {
            return Err(invalid(
                "consumer directory no longer matches its native owner",
            ));
        }
        let destination_path = destination.to_path_buf();
        let moved = fs::rename_directory_new(&source, destination);
        #[cfg(test)]
        if moved.is_ok() {
            AFTER_DIRECTORY_MOVE.with(|hook| {
                let callback = hook.borrow_mut().take();
                if let Some(callback) = callback {
                    callback();
                }
            });
        }
        #[cfg(test)]
        let moved = if moved.is_ok()
            && FAIL_DIRECTORY_MOVE_RESPONSE.with(|failure| failure.replace(false))
        {
            Err(std::io::Error::other(
                "injected lost directory move response",
            ))
        } else {
            moved
        };
        if let Err(error) = moved {
            self.index.path = if operation
                .publication()
                .matches_source_directory(destination)
                .unwrap_or(false)
            {
                Some(destination_path)
            } else if operation
                .publication()
                .matches_source_directory(&source)
                .unwrap_or(false)
            {
                Some(source)
            } else {
                None
            };
            return Err(error.into());
        }
        self.index.path = Some(destination_path);
        if !operation
            .publication()
            .matches_source_directory(destination)?
        {
            return Err(invalid("moved projection does not match its native owner"));
        }
        // Persist the new link before retiring the old one. A source-parent
        // failure retains the destination, receipt and revoked native custody.
        sync_published_parent(destination)?;
        if source.parent() != destination.parent() {
            sync_published_source_parent(&source)?;
        }
        // Readers hold paths into their generation. Reload those readers while
        // retaining the lease, receipt, and initializer's runtime configuration.
        self.index.validate_registered_artifacts()?;
        #[cfg(feature = "vector-search")]
        self.index.load_registered_rabitq_projection()?;
        self.index.consumer_binding_valid = true;
        self.pending_mutation = pending_mutation;
        Ok(())
    }

    fn read_receipt(&self) -> Result<CheckpointReceipt> {
        let path = self
            .index
            .path
            .as_ref()
            .expect("persistent consumer")
            .join(SEARCH_SNAPSHOT_FILE);
        let mut file = File::open(path)?;
        let mut hasher = IntegrityHasher::new();
        let mut buffer = [0u8; 8192];
        let mut encoded_len = 0u64;
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
            encoded_len = encoded_len
                .checked_add(count as u64)
                .ok_or_else(|| invalid("snapshot length overflow"))?;
        }
        Ok(CheckpointReceipt {
            binding: self.binding().clone(),
            source_epoch: self
                .index
                .source_graph_commit_epoch
                .ok_or_else(|| invalid("missing source epoch"))?,
            encoded_len,
            sha256: hasher.finish().sha256.to_string(),
        })
    }
}

#[cfg(test)]
type DirectoryMoveHook = std::cell::RefCell<Option<Box<dyn FnOnce()>>>;

#[cfg(test)]
thread_local! {
    static AFTER_DIRECTORY_MOVE: DirectoryMoveHook = const { std::cell::RefCell::new(None) };
    static FAIL_DIRECTORY_MOVE_RESPONSE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static FAIL_SOURCE_PARENT_SYNC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static FAIL_PUBLISHED_PARENT_SYNC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
fn sync_published_parent(destination: &Path) -> Result<()> {
    #[cfg(test)]
    if FAIL_PUBLISHED_PARENT_SYNC.with(|failure| failure.replace(false)) {
        return Err(invalid(
            "injected parent synchronization failure after directory rename",
        ));
    }
    hawdb_storage::durability::sync_parent_directory(destination)?;
    Ok(())
}

fn sync_published_source_parent(source: &Path) -> Result<()> {
    #[cfg(test)]
    if FAIL_SOURCE_PARENT_SYNC.with(|failure| failure.replace(false)) {
        return Err(invalid("injected source parent synchronization failure"));
    }
    hawdb_storage::durability::sync_parent_directory(source)?;
    Ok(())
}

struct MutationScope<'a> {
    index: &'a mut SearchIndex,
    operation: ConsumerPreparationLease,
}
impl Drop for MutationScope<'_> {
    fn drop(&mut self) {
        if self.operation.owns(self.index)
            && self.index.consumer_mutation_generation == Some(self.operation.generation())
        {
            self.index.consumer_mutation_generation = None;
        }
        if std::thread::panicking() {
            self.index.consumer_binding_valid = false;
        }
        // Dropping the original operation revokes admission independently of
        // whichever index value now occupies the borrowed slot.
    }
}

pub(super) struct MarkerMutationAuthority {
    _consumer: Option<MutationPermit>,
    _native: Option<SearchProjectionPublishLease>,
}

impl SearchIndex {
    pub(super) fn require_marker_mutation(&self) -> Result<MarkerMutationAuthority> {
        let consumer = self.require_owned_mutation_or_unregistered()?;
        // The initializer holds runtime custody before its durable binding
        // exists. Reuse that custody rather than recursively locking its root.
        let consumer = consumer.or_else(|| self.borrow_consumer_mutation().ok());
        let native = if consumer.is_none() {
            self.path
                .as_deref()
                .map(SearchProjectionPublishLease::acquire)
                .transpose()?
        } else {
            None
        };
        Ok(MarkerMutationAuthority {
            _consumer: consumer,
            _native: native,
        })
    }

    pub(super) fn borrow_consumer_mutation(&self) -> Result<MutationPermit> {
        let generation = self
            .consumer_mutation_generation
            .ok_or_else(|| invalid("consumer mutation scope has ended"))?;
        MutationPermit::acquire(&self.consumer_owner, generation)
    }

    pub(super) fn mark_untracked_consumer_mutation(&mut self) -> Option<MutationPermit> {
        self.consumer_binding.as_ref()?;
        match self.borrow_consumer_mutation() {
            Ok(permit) => Some(permit),
            Err(_) => {
                self.consumer_binding_valid = false;
                None
            }
        }
    }

    pub(super) fn require_owned_mutation_or_unregistered(&self) -> Result<Option<MutationPermit>> {
        if !self.consumer_binding_valid {
            return Err(invalid(
                "registered projection requires its consumer mutation owner",
            ));
        }
        if self.consumer_binding.is_none() {
            return Ok(None);
        }
        self.borrow_consumer_mutation().map(Some)
    }

    #[cfg(feature = "vector-search")]
    pub(super) fn load_registered_rabitq_projection(&self) -> Result<()> {
        if self
            .documents
            .values()
            .all(|document| document.embedding.is_none())
        {
            self.invalidate_rabitq_projection();
            return Ok(());
        }
        let root = self
            .path
            .as_deref()
            .expect("persistent registered projection");
        for (generation, path) in super::rabitq_artifacts_descending(root)? {
            match super::RaBitQCandidateProjection::load_from_path_classified(
                &path,
                &self.documents,
                &self.rabitq_projection_identity(generation),
            ) {
                Ok(projection) => {
                    *self
                        .rabitq_projection
                        .lock()
                        .unwrap_or_else(|error| error.into_inner()) =
                        Some(std::sync::Arc::new(projection));
                    return Ok(());
                }
                Err(super::rabitq_projection::RaBitQCandidateProjectionLoadError::Resource(
                    error,
                )) => return Err(error),
                Err(_) => {}
            }
        }
        Err(HawDBError::StorageIntegrity(
            "missing or inconsistent registered vector projection".into(),
        ))
    }

    pub(super) fn validate_registered_artifacts(&mut self) -> Result<()> {
        let root = self
            .path
            .as_deref()
            .expect("persistent registered projection");
        let descriptor = super::read_search_segment_descriptor(root)?
            .ok_or_else(|| invalid("missing registered descriptor"))?;
        if !descriptor.matches_documents(&self.documents)
            || !descriptor.payload_artifact_is_available(root)?
        {
            return Err(invalid(
                "registered descriptor does not match its snapshot or payload",
            ));
        }
        self.segment_descriptor = Some(descriptor);
        self.load_lexical_projection()?;
        if self
            .lexical_projection
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .is_none()
        {
            return Err(invalid(
                "missing or inconsistent registered lexical projection",
            ));
        }
        let reader = super::out_of_core::SearchOutOfCoreReader::open_with_config_and_analyzer(
            root,
            super::out_of_core::SearchOutOfCoreConfig::default(),
            self.analyzer_lexicon.clone(),
        )?;
        if reader.source_graph_commit_epoch() != self.source_graph_commit_epoch
            || reader.document_count() != self.documents.len()
        {
            return Err(invalid("registered generation does not match its snapshot"));
        }
        Ok(())
    }

    pub(super) fn require_unregistered_publication(&self) -> Result<()> {
        if self.consumer_binding.is_some() {
            return Err(invalid("registered projection requires its consumer owner"));
        }
        Ok(())
    }
}

fn canonical_uuid(raw: &str) -> Result<Uuid> {
    let uuid = raw.parse::<Uuid>().map_err(|_| invalid("invalid UUID"))?;
    if uuid.is_nil() || uuid.to_string() != raw {
        return Err(invalid("noncanonical UUID"));
    }
    Ok(uuid)
}
fn invalid(detail: &str) -> HawDBError {
    HawDBError::Storage(format!("projection consumer: {detail}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn binding() -> ConsumerBinding {
        ConsumerBinding {
            database_uuid: hawdb_core::generate_uuidv7().unwrap(),
            projection_uuid: hawdb_core::generate_uuidv7().unwrap(),
            consumer_id: "main".into(),
            registration_uuid: hawdb_core::generate_uuidv7().unwrap(),
            checkpoint_uuid: hawdb_core::generate_uuidv7().unwrap(),
        }
    }
    fn directory() -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "hawdb-owned-projection-{}",
            hawdb_core::generate_uuidv7().unwrap()
        ));
        fs::create_dir(&root).unwrap();
        root
    }
    #[test]
    fn cleanup_reuses_consumer_authority_only_inside_owned_mutation() {
        let root = directory();
        let mut owner = ConsumerProjection::initialize(&root, binding(), 0, |_| Ok(())).unwrap();
        assert!(!owner.index().projection_cleanup_report().retry_required);
        let artifact = root.join("search_rabitq.777.hawdb");
        fs::write(&artifact, b"unreferenced artifact").unwrap();
        let deferred = owner.index().retry_projection_cleanup(Default::default());
        assert!(deferred.retry_required);
        assert_eq!(deferred.delete_attempts, 0);
        assert!(artifact.exists());
        owner
            .apply(|index| {
                let cleaned = index.retry_projection_cleanup(Default::default());
                assert!(!cleaned.retry_required);
                assert!(!artifact.exists());
                Ok(())
            })
            .unwrap();
        assert!(owner.binding_is_valid());
        drop(owner);
        let owner = ConsumerProjection::open(&root).unwrap();
        assert!(!owner.index().projection_cleanup_report().retry_required);
        drop(owner);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ordinary_mutation_and_infallible_setters_cannot_publish_an_owned_index() {
        let root = directory();
        let mut owner = ConsumerProjection::initialize(&root, binding(), 0, |_| Ok(())).unwrap();
        assert!(owner
            .index
            .apply_projection_delta(super::super::SearchProjectionDelta::default())
            .is_err());
        owner.index.delete("missing");
        assert!(!owner.binding_is_valid());
        assert!(owner.checkpoint().is_err());
        drop(owner);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn duplicate_and_malformed_snapshot_bindings_fail_closed() {
        let root = directory();
        let owner = ConsumerProjection::initialize(&root, binding(), 0, |_| Ok(())).unwrap();
        drop(owner);
        let path = root.join(SEARCH_SNAPSHOT_FILE);
        let original = super::super::read_search_snapshot_text(&path).unwrap();
        let (body, _) = super::super::split_checksum(&original).unwrap();
        let line = body
            .lines()
            .find(|line| line.starts_with("projection_consumer_binding"))
            .unwrap();
        for mutated in [
            format!("{body}{line}\n"),
            body.replace(
                "projection_consumer_binding\t",
                "projection_consumer_binding\tbad-uuid\t",
            ),
            body.replace("source_graph_commit_epoch\t0\n", ""),
        ] {
            let text = format!(
                "{mutated}checksum\t{}\n",
                super::super::checksum_bytes(mutated.as_bytes())
            );
            fs::write(
                &path,
                super::super::encode_search_snapshot_text(&text).unwrap(),
            )
            .unwrap();
            assert!(ConsumerProjection::open(&root).is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn ordinary_snapshot_bytes_and_query_results_remain_unchanged() {
        let root = directory();
        let ordinary = root.join("ordinary");
        let registered = root.join("registered");
        fs::create_dir(&registered).unwrap();
        let seed = |index: &mut SearchIndex| {
            index.upsert(super::super::SearchDocument {
                id: "document".into(),
                title: "searchable".into(),
                content: "searchable payload".repeat(300),
                embedding: None,
                metadata: Default::default(),
            })
        };
        let mut index = SearchIndex::open(&ordinary).unwrap();
        seed(&mut index).unwrap();
        index.checkpoint().unwrap();
        let before = fs::read(ordinary.join(SEARCH_SNAPSHOT_FILE)).unwrap();
        // A long second document record must not be treated as a bounded header.
        let mut index = SearchIndex::open(&ordinary).unwrap();
        index.checkpoint().unwrap();
        assert_eq!(
            before,
            fs::read(ordinary.join(SEARCH_SNAPSHOT_FILE)).unwrap()
        );
        index
            .apply_projection_delta(super::super::SearchProjectionDelta {
                source_graph_commit_epoch: Some(0),
                ..Default::default()
            })
            .unwrap();
        index.checkpoint().unwrap();
        let owner = ConsumerProjection::initialize(&registered, binding(), 0, seed).unwrap();
        #[cfg(feature = "full-text-search")]
        assert_eq!(
            index
                .search("searchable", None, super::super::SearchMode::Text, 10)
                .unwrap(),
            owner
                .index()
                .search("searchable", None, super::super::SearchMode::Text, 10)
                .unwrap()
        );
        #[cfg(not(feature = "full-text-search"))]
        for index in [&index, owner.index()] {
            assert!(matches!(
                index.try_search_with_options(
                    "searchable",
                    None,
                    super::super::SearchMode::Text,
                    super::super::SearchQueryOptions {
                        limit: 10,
                        offset: 0,
                        rank_window: None,
                        fusion_weights: super::super::SearchFusionWeights::default(),
                        metadata_filters: Default::default(),
                        policy_epoch: None,
                    }
                ),
                Err(HawDBError::CapabilityUnavailable {
                    capability: hawdb_core::RuntimeCapability::FullTextSearch
                })
            ));
        }
        drop(owner);
        drop(index);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn initializer_cannot_silently_publish_unrecoverable_analyzer_rules() {
        let root = directory();
        let result = ConsumerProjection::initialize(&root, binding(), 0, |index| {
            index.set_analyzer_lexicon(super::super::SearchAnalyzerLexicon::empty());
            Ok(())
        });
        assert!(result.is_err());
        assert!(!root.join(SEARCH_SNAPSHOT_FILE).exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(feature = "vector-search")]
    #[test]
    fn registered_reopen_requires_its_completed_vector_artifact() {
        let root = directory();
        let owner = ConsumerProjection::initialize(&root, binding(), 0, |index| {
            index.upsert(super::super::SearchDocument {
                id: "vector".into(),
                title: String::new(),
                content: "vector document".into(),
                embedding: Some(vec![1.0, 0.5, 0.25, 0.0]),
                metadata: Default::default(),
            })
        })
        .unwrap();
        drop(owner);
        ConsumerProjection::open(&root).unwrap();
        let artifacts = super::super::rabitq_artifacts_descending(&root).unwrap();
        assert!(!artifacts.is_empty());
        for (_, artifact) in artifacts {
            fs::remove_file(artifact).unwrap();
        }
        assert!(matches!(
            ConsumerProjection::open(&root),
            Err(HawDBError::StorageIntegrity(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn preparation_excludes_mutations_before_any_side_effect() {
        let root = directory();
        let destination = root.with_extension("moved");
        let mut owner = ConsumerProjection::initialize(&root, binding(), 0, |_| Ok(())).unwrap();
        let receipt = owner.receipt();
        let snapshot = fs::read(root.join(SEARCH_SNAPSHOT_FILE)).unwrap();
        let operation = owner.reserve_preparation().unwrap();
        assert!(matches!(
            owner.reserve_preparation(),
            Err(HawDBError::ProjectionBusy)
        ));
        let called = std::cell::Cell::new(false);
        assert!(matches!(
            owner.apply(|_| {
                called.set(true);
                Ok(())
            }),
            Err(HawDBError::ProjectionBusy)
        ));
        assert!(!called.get());
        assert!(matches!(
            owner.checkpoint(),
            Err(HawDBError::ProjectionBusy)
        ));
        assert!(matches!(
            owner.publish_directory(&destination),
            Err(HawDBError::ProjectionBusy)
        ));
        assert!(!destination.exists());
        assert_eq!(owner.binding(), &receipt.binding);
        assert_eq!(owner.receipt(), receipt);
        assert_eq!(fs::read(root.join(SEARCH_SNAPSHOT_FILE)).unwrap(), snapshot);
        assert!(!owner.has_pending_mutation());
        drop(operation);
        owner.apply(|_| Ok(())).unwrap();
        assert!(owner.has_pending_mutation());
        owner.checkpoint().unwrap();
        drop(owner.reserve_preparation().unwrap());
        drop(owner);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn preparation_retains_native_root_exclusion_after_owner_closes() {
        let root = directory();
        let mut owner = ConsumerProjection::initialize(&root, binding(), 0, |_| Ok(())).unwrap();
        let operation = owner.reserve_preparation().unwrap();
        drop(owner);
        assert!(ConsumerProjection::open(&root).is_err());
        let deferred = SearchIndex {
            path: Some(root.clone()),
            ..SearchIndex::default()
        }
        .retry_projection_cleanup(Default::default());
        assert!(deferred.retry_required);
        assert_eq!(deferred.scanned_entries, 0);
        assert_eq!(deferred.delete_attempts, 0);
        std::thread::spawn(move || drop(operation)).join().unwrap();
        let owner = ConsumerProjection::open(&root).unwrap();
        assert_eq!(owner.receipt().source_epoch, 0);
        drop(owner);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn preparation_refuses_same_epoch_mutation_and_failed_callbacks_until_checkpoint() {
        let root = directory();
        let mut owner = ConsumerProjection::initialize(&root, binding(), 0, |_| Ok(())).unwrap();
        owner
            .apply(|index| {
                index.upsert(crate::SearchDocument {
                    id: "same-epoch".into(),
                    title: String::new(),
                    content: "changed".into(),
                    embedding: None,
                    metadata: Default::default(),
                })
            })
            .unwrap();
        // Epoch-only freshness misses this actual resident mutation.
        assert!(
            !owner
                .index()
                .projection_freshness()
                .has_uncheckpointed_changes
        );
        assert!(owner.reserve_preparation().is_err());
        owner.checkpoint().unwrap();
        drop(owner.reserve_preparation().unwrap());
        assert!(owner
            .apply::<()>(|_| Err(HawDBError::Execution("refused callback".into())))
            .is_err());
        assert!(owner.reserve_preparation().is_err());
        owner.checkpoint().unwrap();
        drop(owner.reserve_preparation().unwrap());
        drop(owner);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn preparation_rejection_and_unwind_release_the_operation_slot() {
        let root = directory();
        let mut owner = ConsumerProjection::initialize(&root, binding(), 0, |_| Ok(())).unwrap();
        let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _operation = owner.reserve_preparation().unwrap();
            panic!("cancel preparation");
        }));
        assert!(failed.is_err());
        assert!(owner.binding_is_valid());
        drop(owner.reserve_preparation().unwrap());
        // Force the real snapshot publication to fail after checkpoint_uuid
        // changes, while preserving the previous complete snapshot for repair.
        let snapshot = root.join(SEARCH_SNAPSHOT_FILE);
        let previous = root.join("previous-snapshot");
        fs::rename(&snapshot, &previous).unwrap();
        fs::create_dir(&snapshot).unwrap();
        assert!(owner.checkpoint().is_err());
        assert_eq!(owner.index().source_graph_commit_epoch, Some(0));
        assert!(owner.has_pending_mutation());
        assert!(owner.reserve_preparation().is_err());
        owner.require_operation_available().unwrap();
        fs::remove_dir(&snapshot).unwrap();
        fs::rename(&previous, &snapshot).unwrap();
        // Restoring the old file cannot certify this in-memory checkpoint.
        assert!(owner.reserve_preparation().is_err());
        owner.checkpoint().unwrap();
        drop(owner.reserve_preparation().unwrap());
        drop(owner);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn moved_index_cannot_reuse_an_ended_foreign_mutation_scope() {
        let root_a = directory();
        let root_b = directory();
        let initialize = |id: &str| crate::SearchDocument {
            id: id.into(),
            title: String::new(),
            content: id.into(),
            embedding: None,
            metadata: Default::default(),
        };
        let mut owner_a = ConsumerProjection::initialize(&root_a, binding(), 0, |index| {
            index.upsert(initialize("memory:a"))
        })
        .unwrap();
        let mut owner_b = ConsumerProjection::initialize(&root_b, binding(), 0, |index| {
            index.upsert(initialize("memory:b"))
        })
        .unwrap();
        let reached_cleanup = std::cell::Cell::new(false);
        let result = owner_a.apply(|index_a| {
            let moved = owner_b.apply(|index_b| {
                std::mem::swap(index_a, index_b);
                Ok(())
            });
            assert!(
                moved.is_err(),
                "foreign index must invalidate B's owner association"
            );
            // The B callback ended and its owner closes. The moved index must
            // not carry a live B authority into A's still-running callback.
            drop(owner_b);
            let peer_b = ConsumerProjection::open(&root_b).unwrap();
            let report = index_a.retry_projection_cleanup(Default::default());
            assert_eq!(
                report.scanned_entries, 0,
                "foreign ended scope bypassed B's current native owner: {report:?}"
            );
            assert_eq!(report.delete_attempts, 0);
            assert!(report.retry_required);
            reached_cleanup.set(true);
            drop(peer_b);
            Ok(())
        });
        assert!(
            reached_cleanup.get(),
            "rejection must not hide the cleanup regression"
        );
        assert!(
            result.is_err(),
            "foreign index must invalidate A's owner association"
        );
        assert!(!owner_a.binding_is_valid());
        assert!(owner_a.checkpoint().is_err());
        drop(owner_a);
        fs::remove_dir_all(root_a).unwrap();
        fs::remove_dir_all(root_b).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn registered_snapshot_load_uses_captured_directory_after_path_replacement() {
        let root = directory();
        let moved = root.with_extension("moved");
        let owner = ConsumerProjection::initialize(&root, binding(), 0, |index| {
            index.upsert(crate::SearchDocument {
                id: "memory:original".into(),
                title: String::new(),
                content: "original".into(),
                embedding: None,
                metadata: Default::default(),
            })
        })
        .unwrap();
        drop(owner);

        let lease = SearchProjectionPublishLease::acquire_for_consumer(&root).unwrap();
        fs::rename(&root, &moved).unwrap();
        fs::create_dir(&root).unwrap();
        fs::write(root.join(SEARCH_SNAPSHOT_FILE), b"foreign snapshot").unwrap();

        let mut index = SearchIndex {
            path: Some(root.clone()),
            ..SearchIndex::default()
        };
        index.load_snapshot_from_publication(&lease).unwrap();
        assert_eq!(index.documents["memory:original"].content, "original");
        assert!(index.consumer_binding.is_some());

        drop(lease);
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(moved).unwrap();
    }

    #[test]
    fn pre_registration_handle_cannot_write_registered_root_markers() {
        let root = directory();
        let stale = SearchIndex::open(&root).unwrap();
        stale.checkpoint().unwrap();
        let owner = ConsumerProjection::initialize(&root, binding(), 0, |_| Ok(())).unwrap();
        assert!(stale.mark_full_reindex_needed("foreign marker").is_err());
        assert!(stale.mark_metadata_repair_needed("foreign repair").is_err());
        assert!(!owner.index().full_reindex_needed());
        assert!(!owner.index().metadata_repair_needed());
        drop(owner);
        // Even without a live native owner, the durable binding refuses stale I/O.
        assert!(stale.mark_full_reindex_needed("after owner close").is_err());
        assert!(stale
            .mark_metadata_repair_needed("after owner close")
            .is_err());
        assert!(!root.join(super::super::FULL_REINDEX_MARKER).exists());
        assert!(!root.join(super::super::METADATA_REPAIR_MARKER).exists());
        drop(stale);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn initializer_and_owned_markers_reuse_custody_without_recursive_locking() {
        let root = directory();
        let ordinary = SearchIndex::open(&root).unwrap();
        ordinary.mark_full_reindex_needed("ordinary").unwrap();
        assert!(ordinary.full_reindex_needed());
        drop(ordinary);
        let mut owner = ConsumerProjection::initialize(&root, binding(), 0, |index| {
            index.mark_metadata_repair_needed("initializer")
        })
        .unwrap();
        assert!(owner.index().metadata_repair_needed());
        owner
            .apply(|index| {
                index.clear_marker(super::super::METADATA_REPAIR_MARKER)?;
                index.mark_full_reindex_needed("owned")
            })
            .unwrap();
        assert!(!owner.index().metadata_repair_needed());
        assert!(owner.index().full_reindex_needed());
        drop(owner);
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn held_child_refuses_directory_move_without_losing_original_custody() {
        let parent = directory();
        let source = parent.join("source");
        let destination = parent.join("destination");
        let foreign = parent.join("foreign.hawdb");
        fs::create_dir(&source).unwrap();
        fs::write(&foreign, b"unrelated namespace").unwrap();
        let mut owner = ConsumerProjection::initialize(&source, binding(), 0, |_| Ok(())).unwrap();
        let receipt = owner.receipt();
        let snapshot = fs::read(source.join(SEARCH_SNAPSHOT_FILE)).unwrap();

        // The retained in-directory publication handle prevents a Windows
        // directory rename. The legacy API refuses before touching either root.
        let error = owner.publish_directory(&destination).unwrap_err();
        assert!(matches!(
            error,
            HawDBError::CapabilityUnavailable {
                capability: hawdb_core::RuntimeCapability::SearchProjectionPublication
            }
        ));
        assert!(source.is_dir());
        assert!(!destination.exists());
        assert_eq!(
            fs::read(source.join(SEARCH_SNAPSHOT_FILE)).unwrap(),
            snapshot
        );
        assert_eq!(fs::read(&foreign).unwrap(), b"unrelated namespace");
        assert_eq!(owner.index().path.as_deref(), Some(source.as_path()));
        assert_eq!(owner.receipt(), receipt);
        assert!(owner.binding_is_valid());
        drop(owner.reserve_preparation().unwrap());
        assert!(ConsumerProjection::open(&source).is_err());

        drop(owner);
        let recovered = ConsumerProjection::open(&source).unwrap();
        assert_eq!(recovered.receipt(), receipt);
        drop(recovered);
        fs::remove_dir_all(parent).unwrap();
    }
    fn assert_windows_relocation_refusal(
        mut owner: ConsumerProjection,
        parent: &Path,
        source: &Path,
        destination: &Path,
        receipt: &CheckpointReceipt,
    ) {
        let snapshot = fs::read(source.join(SEARCH_SNAPSHOT_FILE)).unwrap();
        let error = owner.publish_directory(destination).unwrap_err();
        assert!(matches!(
            error,
            HawDBError::CapabilityUnavailable {
                capability: hawdb_core::RuntimeCapability::SearchProjectionPublication
            }
        ));
        assert!(source.is_dir());
        assert!(!destination.exists());
        assert_eq!(
            fs::read(source.join(SEARCH_SNAPSHOT_FILE)).unwrap(),
            snapshot
        );
        assert_eq!(owner.index().path.as_deref(), Some(source));
        assert_eq!(&owner.receipt(), receipt);
        assert!(owner.binding_is_valid());
        drop(owner.reserve_preparation().unwrap());
        assert!(ConsumerProjection::open(source).is_err());
        drop(owner);
        let reopened = ConsumerProjection::open(source).unwrap();
        assert_eq!(&reopened.receipt(), receipt);
        drop(reopened);
        fs::remove_dir_all(parent).unwrap();
    }
    #[test]
    fn directory_sync_failure_cannot_reuse_the_vacated_path_as_native_authority() {
        let parent = directory();
        let source = parent.join("source");
        let destination = parent.join("destination");
        fs::create_dir(&source).unwrap();
        let mut owner = ConsumerProjection::initialize(&source, binding(), 0, |_| Ok(())).unwrap();
        if cfg!(windows) {
            let receipt = owner.receipt();
            assert_windows_relocation_refusal(owner, &parent, &source, &destination, &receipt);
            return;
        }
        FAIL_PUBLISHED_PARENT_SYNC.with(|failure| failure.set(true));
        assert!(owner.publish_directory(&destination).is_err());
        assert!(destination.is_dir());
        assert!(!source.exists());
        fs::create_dir(&source).unwrap();
        let peer = ConsumerProjection::initialize(&source, binding(), 0, |_| Ok(())).unwrap();
        let mut observed = None;
        let attempted = owner.apply(|index| {
            let marker = index.mark_full_reindex_needed("old directory owner");
            let cleanup = index.retry_projection_cleanup(Default::default());
            observed = Some((
                marker.is_ok(),
                cleanup.scanned_entries,
                cleanup.delete_attempts,
            ));
            Ok(())
        });
        eprintln!(
            "old-owner attempt={attempted:?}; observed(marker_written,scans,deletes)={observed:?}"
        );
        assert!(
            attempted.is_err(),
            "uncertain directory publication must revoke mutation: {observed:?}"
        );
        assert_eq!(owner.index().path.as_deref(), Some(destination.as_path()));
        assert!(!owner.binding_is_valid());
        assert!(!peer.index().full_reindex_needed());
        assert!(owner.checkpoint().is_err());
        assert!(owner.reserve_preparation().is_err());
        let retry_destination = parent.join("retry");
        assert!(owner.publish_directory(&retry_destination).is_err());
        assert!(!retry_destination.exists());
        // The failed barrier must neither release native ownership nor delete
        // the complete checkpoint already moved to the destination.
        assert!(ConsumerProjection::open(&destination).is_err());
        let original_receipt = owner.receipt();
        drop(peer);
        drop(owner);
        let recovered = ConsumerProjection::open(&destination).unwrap();
        assert_eq!(recovered.receipt(), original_receipt);
        drop(recovered);
        fs::remove_dir_all(parent).unwrap();
    }
    #[test]
    fn lost_directory_move_response_keeps_native_custody_and_original_receipt() {
        let parent = directory();
        let source = parent.join("source");
        let destination = parent.join("destination");
        fs::create_dir(&source).unwrap();
        let mut owner = ConsumerProjection::initialize(&source, binding(), 0, |_| Ok(())).unwrap();
        let receipt = owner.receipt();
        if cfg!(windows) {
            assert_windows_relocation_refusal(owner, &parent, &source, &destination, &receipt);
            return;
        }
        FAIL_DIRECTORY_MOVE_RESPONSE.with(|failure| failure.set(true));
        assert!(owner
            .publish_directory(&destination)
            .unwrap_err()
            .to_string()
            .contains("lost directory move response"));
        assert!(destination.is_dir());
        assert!(!source.exists());
        assert_eq!(owner.index().path.as_deref(), Some(destination.as_path()));
        assert!(!owner.binding_is_valid());
        assert_eq!(owner.receipt(), receipt);
        assert!(owner.checkpoint().is_err());
        assert!(ConsumerProjection::open(&destination).is_err());
        fs::create_dir(&source).unwrap();
        let peer = ConsumerProjection::initialize(&source, binding(), 0, |_| Ok(())).unwrap();
        assert!(owner
            .apply(|index| index.mark_full_reindex_needed("lost move response"))
            .is_err());
        assert!(!peer.index().full_reindex_needed());
        drop(owner);
        let reopened = ConsumerProjection::open(&destination).unwrap();
        assert_eq!(reopened.receipt(), receipt);
        drop(reopened);
        drop(peer);
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn lost_move_outcome_with_no_known_locator_retains_custody_and_evidence() {
        let parent = directory();
        let source = parent.join("source");
        let destination = parent.join("destination");
        let third = parent.join("third");
        fs::create_dir(&source).unwrap();
        let mut owner = ConsumerProjection::initialize(&source, binding(), 0, |_| Ok(())).unwrap();
        let receipt = owner.receipt();
        if cfg!(windows) {
            assert_windows_relocation_refusal(owner, &parent, &source, &destination, &receipt);
            return;
        }
        let moved = destination.clone();
        let actual = third.clone();
        AFTER_DIRECTORY_MOVE.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                fs::rename(&moved, &actual).unwrap();
            }))
        });
        FAIL_DIRECTORY_MOVE_RESPONSE.with(|failure| failure.set(true));
        assert!(owner.publish_directory(&destination).is_err());
        assert_eq!(owner.index().path, None);
        assert!(!owner.binding_is_valid());
        assert!(third.is_dir());
        assert!(ConsumerProjection::open(&third).is_err());
        assert_eq!(owner.receipt(), receipt);
        assert!(owner.checkpoint().is_err());
        assert!(owner.reserve_preparation().is_err());
        drop(owner);
        let recovered = ConsumerProjection::open(&third).unwrap();
        assert_eq!(recovered.receipt(), receipt);
        drop(recovered);
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn a_replaced_source_is_not_moved_using_an_old_native_owner() {
        let parent = directory();
        let source = parent.join("source");
        let original = parent.join("original");
        let destination = parent.join("destination");
        fs::create_dir(&source).unwrap();
        let mut owner = ConsumerProjection::initialize(&source, binding(), 0, |_| Ok(())).unwrap();
        let receipt = owner.receipt();
        if cfg!(windows) {
            assert_eq!(
                fs::rename(&source, &original).unwrap_err().kind(),
                std::io::ErrorKind::PermissionDenied
            );
            assert_windows_relocation_refusal(owner, &parent, &source, &destination, &receipt);
            return;
        }
        fs::rename(&source, &original).unwrap();
        fs::create_dir(&source).unwrap();
        let peer = ConsumerProjection::initialize(&source, binding(), 0, |_| Ok(())).unwrap();
        let foreign = fs::read(source.join(SEARCH_SNAPSHOT_FILE)).unwrap();
        assert!(owner.publish_directory(&destination).is_err());
        assert!(!destination.exists());
        assert_eq!(
            fs::read(source.join(SEARCH_SNAPSHOT_FILE)).unwrap(),
            foreign
        );
        assert!(!owner.binding_is_valid());
        assert_eq!(owner.index().path, None);
        assert!(ConsumerProjection::open(&original).is_err());
        assert_eq!(owner.receipt(), receipt);
        drop(owner);
        let recovered = ConsumerProjection::open(&original).unwrap();
        assert_eq!(recovered.receipt(), receipt);
        drop(recovered);
        drop(peer);
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn occupied_directory_target_is_retained_and_failed_owner_requires_reopen() {
        for populated in [false, true] {
            let parent = directory();
            let source = parent.join("source");
            let destination = parent.join("destination");
            fs::create_dir(&source).unwrap();
            fs::create_dir(&destination).unwrap();
            if populated {
                fs::write(destination.join("foreign.hawdb"), b"foreign evidence").unwrap();
            }
            let mut owner =
                ConsumerProjection::initialize(&source, binding(), 0, |_| Ok(())).unwrap();
            let receipt = owner.receipt();
            assert!(owner.publish_directory(&destination).is_err());
            assert!(source.is_dir());
            assert!(destination.is_dir());
            if populated {
                assert_eq!(
                    fs::read(destination.join("foreign.hawdb")).unwrap(),
                    b"foreign evidence"
                );
            } else {
                assert!(fs::read_dir(&destination).unwrap().next().is_none());
            }
            if cfg!(windows) {
                assert!(owner.binding_is_valid());
                drop(owner.reserve_preparation().unwrap());
            } else {
                assert!(!owner.binding_is_valid());
                assert!(owner.checkpoint().is_err());
            }
            drop(owner);
            let reopened = ConsumerProjection::open(&source).unwrap();
            assert_eq!(reopened.receipt(), receipt);
            drop(reopened);
            fs::remove_dir_all(parent).unwrap();
        }
    }

    #[test]
    fn source_parent_sync_failure_keeps_the_moved_directory_revoked() {
        let parent = directory();
        let first = parent.join("first");
        let second = parent.join("second");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        let source = first.join("source");
        let destination = second.join("destination");
        fs::create_dir(&source).unwrap();
        let mut owner = ConsumerProjection::initialize(&source, binding(), 0, |_| Ok(())).unwrap();
        let receipt = owner.receipt();
        if cfg!(windows) {
            assert_windows_relocation_refusal(owner, &parent, &source, &destination, &receipt);
            return;
        }
        FAIL_SOURCE_PARENT_SYNC.with(|failure| failure.set(true));
        assert!(owner
            .publish_directory(&destination)
            .unwrap_err()
            .to_string()
            .contains("source parent synchronization failure"));
        assert!(destination.is_dir());
        assert!(!source.exists());
        assert!(!owner.binding_is_valid());
        assert_eq!(owner.index().path.as_deref(), Some(destination.as_path()));
        assert!(ConsumerProjection::open(&destination).is_err());
        drop(owner);
        let reopened = ConsumerProjection::open(&destination).unwrap();
        assert_eq!(reopened.receipt(), receipt);
        drop(reopened);
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn successful_directory_move_keeps_mutations_on_the_moved_native_root() {
        let parent = directory();
        let source = parent.join("source");
        let destination = parent.join("destination");
        fs::create_dir(&source).unwrap();
        let mut owner = ConsumerProjection::initialize(&source, binding(), 0, |_| Ok(())).unwrap();
        let receipt = owner.receipt();
        if cfg!(windows) {
            assert_windows_relocation_refusal(owner, &parent, &source, &destination, &receipt);
            return;
        }
        owner.publish_directory(&destination).unwrap();
        assert!(owner.binding_is_valid());
        assert!(!owner.has_pending_mutation());
        assert_eq!(owner.receipt(), receipt);
        assert!(!source.exists());
        fs::create_dir(&source).unwrap();
        let peer = ConsumerProjection::initialize(&source, binding(), 0, |_| Ok(())).unwrap();
        owner
            .apply(|index| index.mark_full_reindex_needed("moved root"))
            .unwrap();
        assert!(owner.index().full_reindex_needed());
        assert!(!peer.index().full_reindex_needed());
        let completed = owner.checkpoint().unwrap();
        drop(peer);
        drop(owner);
        let reopened = ConsumerProjection::open(&destination).unwrap();
        assert_eq!(reopened.receipt(), completed);
        assert!(reopened.index().full_reindex_needed());
        drop(reopened);
        fs::remove_dir_all(parent).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_publication_refusal_preserves_receipt_and_native_lease() {
        let root = directory();
        let source = root.join("source");
        let destination = root.join("destination");
        fs::create_dir(&source).unwrap();
        let mut owner = ConsumerProjection::initialize(&source, binding(), 7, |_| Ok(())).unwrap();
        let receipt = owner.receipt();
        let snapshot = fs::read(source.join(SEARCH_SNAPSHOT_FILE)).unwrap();

        // Retry both an absent and a foreign occupied destination. Neither
        // refusal grants ownership of the destination or releases the source.
        for occupied in [false, true] {
            if occupied {
                fs::create_dir(&destination).unwrap();
                fs::write(destination.join("foreign"), b"keep").unwrap();
            }
            for _ in 0..2 {
                assert!(matches!(
                    owner.publish_directory(&destination),
                    Err(HawDBError::CapabilityUnavailable {
                        capability: hawdb_core::RuntimeCapability::SearchProjectionPublication
                    })
                ));
                assert_eq!(owner.receipt(), receipt);
                assert_eq!(owner.index.path.as_deref(), Some(source.as_path()));
                assert_eq!(
                    fs::read(source.join(SEARCH_SNAPSHOT_FILE)).unwrap(),
                    snapshot
                );
                assert!(matches!(
                    ConsumerProjection::open(&source),
                    Err(HawDBError::Storage(message))
                        if message == "another search projection publication is active"
                ));
                if occupied {
                    assert_eq!(fs::read(destination.join("foreign")).unwrap(), b"keep");
                } else {
                    assert!(!destination.exists());
                }
            }
        }
        drop(owner);
        let reopened = ConsumerProjection::open(&source).unwrap();
        assert_eq!(reopened.receipt(), receipt);
        drop(reopened);
        fs::remove_dir_all(root).unwrap();
    }
}

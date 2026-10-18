// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

//! One native catalog namespace, including reads, writers, heads and reclamation.
//! Paths in the parent module are labels; no child operation resolves them.

use super::*;
use crate::file_io::{SourceDirectory, SourceDirectoryIdentity};

pub(super) type CatalogIdentity = [SourceDirectoryIdentity; 3];

#[derive(Debug)]
pub(super) struct CapturedCatalog {
    #[cfg(test)]
    pub(super) before_close: Option<CaptureCloseProbe>,
    // Retain all three identities while the shared registry key is live.
    root: SourceDirectory,
    pub(super) generations: SourceDirectory,
    pub(super) heads: Arc<SourceDirectory>,
}

impl CapturedCatalog {
    pub(super) fn capture(root: &Path) -> io::Result<Self> {
        let root = SourceDirectory::capture_for_path(root)?;
        Self::from_root(root)
    }

    pub(super) fn from_root(root: SourceDirectory) -> io::Result<Self> {
        let context = root.operation_context();
        let generations = root.open_directory(&context, "generations")?;
        let heads = Arc::new(root.open_directory(&context, "heads")?);
        Ok(Self {
            #[cfg(test)]
            before_close: None,
            root,
            generations,
            heads,
        })
    }

    pub(super) fn identity(&self) -> CatalogIdentity {
        [
            self.root.identity(),
            self.generations.identity(),
            self.heads.identity(),
        ]
    }
}

#[cfg(test)]
#[derive(Debug)]
pub(super) struct CaptureCloseProbe {
    pub(super) shared: Weak<Mutex<ProjectionGenerationShared>>,
    pub(super) project: crate::file_descriptors::ProjectFileDescriptors,
    pub(super) observed: Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(test)]
impl Drop for CaptureCloseProbe {
    fn drop(&mut self) {
        assert!(
            self.shared.upgrade().is_none(),
            "registry still upgrades while its final identity capture closes"
        );
        assert_eq!(
            self.project.metrics().open,
            3,
            "probe must run before native capture fields close"
        );
        self.observed
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

impl ProjectionGenerationStore {
    // Returns false only when a legitimate GC work/byte bound stops this batch.
    pub(super) fn reclaim_entry(
        &self,
        path: &Path,
        limits: ProjectionGenerationGcLimits,
        shared: &ProjectionGenerationShared,
        report: &mut ProjectionGenerationGcReport,
    ) -> Result<bool, ProjectionGenerationError> {
        if !self.file_exists(path)? {
            return Ok(true);
        }
        if path.extension().and_then(|value| value.to_str()) == Some(DATA_SUFFIX) {
            if self.file_exists(&path.with_extension(MANIFEST_SUFFIX))? {
                return Ok(true);
            }
            let (begin, bytes) = {
                let mut file = self.open_file(path)?;
                let bytes = file.metadata()?.len();
                let (begin, _) = match read_data_header(&mut file) {
                    Ok(header) => header,
                    Err(error) if corrupt_entry(&error) => return Ok(true),
                    Err(error) => return Err(error),
                };
                (begin, bytes)
            };
            if self.data_path(&begin.identity) != path
                || shared.open_writers.contains(&identity_key(&begin.identity))
            {
                return Ok(true);
            }
            if shared.publications.contains_key(&owner_key_hash(
                &begin.identity.projection,
                &begin.identity.owner_key,
            )) {
                report.pinned_generations_skipped += 1;
                return Ok(true);
            }
            let head_path = self.head_path(&begin.identity.projection, &begin.identity.owner_key);
            if let Some(head) = self.read_optional_head(&head_path)? {
                validate_head_owner(&head, &begin.identity)?;
                if head.manifest.begin.identity.generation == begin.identity.generation {
                    return Err(ProjectionGenerationError::Corruption(
                        "active generation has no immutable manifest".into(),
                    ));
                }
                self.stabilize_selected_head(&head_path, &head)?;
            }
            if !admit_reclamation(bytes, limits, report) {
                return Ok(false);
            }
            self.remove_file(path)?;
            report.generations_reclaimed += 1;
            report.bytes_reclaimed += bytes;
            return Ok(true);
        }
        if path.extension().and_then(|value| value.to_str()) != Some(MANIFEST_SUFFIX) {
            return Ok(true);
        }
        let manifest = match self.read_manifest(path) {
            Ok(manifest) => manifest,
            Err(error) if corrupt_entry(&error) => return Ok(true),
            Err(error) => return Err(error),
        };
        if self.manifest_path(&manifest.begin.identity) != path {
            return Ok(true);
        }
        if shared.publications.contains_key(&owner_key_hash(
            &manifest.begin.identity.projection,
            &manifest.begin.identity.owner_key,
        )) {
            report.pinned_generations_skipped += 1;
            return Ok(true);
        }
        let key = identity_key(&manifest.begin.identity);
        let head_path = self.head_path(
            &manifest.begin.identity.projection,
            &manifest.begin.identity.owner_key,
        );
        let head = self.read_optional_head(&head_path)?;
        if let Some(head) = &head {
            validate_head_owner(head, &manifest.begin.identity)?;
        }
        if head.as_ref().is_some_and(|head| {
            head.manifest.begin.identity.generation == manifest.begin.identity.generation
        }) {
            report.active_generations_skipped += 1;
            return Ok(true);
        }
        if shared.pinned.contains_key(&key) || shared.open_writers.contains(&key) {
            report.pinned_generations_skipped += 1;
            return Ok(true);
        }
        // A visible but unsynchronized replacement can roll back on power
        // loss. Persist its complete closure/head before deleting any old root
        // that the durable namespace could otherwise select again.
        if let Some(head) = &head {
            self.stabilize_selected_head(&head_path, head)?;
        }
        let data_path = self.data_path(&manifest.begin.identity);
        let data_bytes = match self.file_metadata(&data_path) {
            Ok(metadata) => metadata.len(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
            Err(error) => return Err(error.into()),
        };
        let bytes = self
            .file_metadata(path)?
            .len()
            .checked_add(data_bytes)
            .ok_or_else(|| {
                ProjectionGenerationError::Corruption(
                    "reclaimed generation byte count overflow".into(),
                )
            })?;
        if !admit_reclamation(bytes, limits, report) {
            return Ok(false);
        }
        if self.file_exists(&data_path)? {
            self.remove_file(&data_path)?;
        }
        self.remove_file(path)?;
        report.generations_reclaimed += 1;
        report.bytes_reclaimed += bytes;
        Ok(true)
    }

    fn directory_for<'name>(
        &self,
        path: &'name Path,
    ) -> io::Result<(&SourceDirectory, &'name str)> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "invalid catalog child label");
        let mut components = path
            .strip_prefix(&self.root)
            .map_err(|_| invalid())?
            .components();
        let directory = match components.next() {
            Some(std::path::Component::Normal(name)) if name == "generations" => {
                &self.captured.generations
            }
            Some(std::path::Component::Normal(name)) if name == "heads" => &self.captured.heads,
            _ => return Err(invalid()),
        };
        let Some(std::path::Component::Normal(name)) = components.next() else {
            return Err(invalid());
        };
        if components.next().is_some() {
            return Err(invalid());
        }
        Ok((directory, name.to_str().ok_or_else(invalid)?))
    }

    pub(super) fn open_file(&self, path: &Path) -> io::Result<File> {
        let (directory, name) = self.directory_for(path)?;
        directory.open_child(&directory.operation_context(), name)
    }

    pub(super) fn open_mutable_file(&self, path: &Path, create_new: bool) -> io::Result<File> {
        let (directory, name) = self.directory_for(path)?;
        directory.open_mutable_child(&directory.operation_context(), name, create_new)
    }

    pub(super) fn file_exists(&self, path: &Path) -> io::Result<bool> {
        match self.open_file(path) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub(super) fn file_metadata(&self, path: &Path) -> io::Result<std::fs::Metadata> {
        self.open_file(path)?.metadata()
    }

    pub(super) fn remove_file(&self, path: &Path) -> io::Result<()> {
        let (directory, name) = self.directory_for(path)?;
        directory.remove_child(&directory.operation_context(), name)
    }

    pub(super) fn sync_parent(&self, path: &Path) -> io::Result<()> {
        let (directory, _) = self.directory_for(path)?;
        directory.sync(&directory.operation_context())
    }

    pub(super) fn sync_file_and_parent(&self, path: &Path) -> io::Result<()> {
        {
            let file = self.open_mutable_file(path, false)?;
            file.sync_all()?;
        }
        self.sync_parent(path)
    }

    pub(super) fn stabilize_selected_head(
        &self,
        path: &Path,
        head: &ProjectionGenerationHead,
    ) -> Result<(), ProjectionGenerationError> {
        self.ensure_writable()?;
        let manifest = self.read_manifest(&self.manifest_path(&head.manifest.begin.identity))?;
        if manifest != head.manifest {
            return Err(ProjectionGenerationError::Corruption(
                "selected head does not match its immutable generation manifest".into(),
            ));
        }
        {
            let mut file =
                self.open_mutable_file(&self.data_path(&manifest.begin.identity), false)?;
            let (begin, _) = read_data_header(&mut file)?;
            if begin != manifest.begin || file.metadata()?.len() != manifest.data_bytes {
                return Err(ProjectionGenerationError::Corruption(
                    "selected generation data does not match its sealed boundary".into(),
                ));
            }
            file.sync_all()?;
        }
        // A previously visible manifest can also have lost its namespace
        // barrier. Complete all recovery dependencies before the head barrier.
        self.sync_file_and_parent(&self.manifest_path(&manifest.begin.identity))?;
        self.sync_file_and_parent(path)?;
        Ok(())
    }

    pub(super) fn write_control(
        &self,
        path: &Path,
        bytes: &[u8],
        shared: &mut ProjectionGenerationShared,
    ) -> Result<(), ProjectionGenerationError> {
        let temporary = temporary_path(path, shared);
        let (directory, target) = self.directory_for(path)?;
        let (_, source) = self.directory_for(&temporary)?;
        let context = directory.operation_context();
        let mut candidate = directory.create_control_candidate(&context, source)?;
        candidate.write_all(bytes)?;
        candidate.seal()?;
        // The catalog mutex excludes this process's head/manifest writers.
        // A host must retain its actual interprocess publication authority.
        // Errors/drop retain uncertain candidate or selected-head evidence.
        let pending = candidate.select_head(&context, target)?;
        #[cfg(test)]
        if FAIL_SELECTION_BARRIER.with(|failure| failure.replace(false)) {
            return Err(io::Error::other("injected lost selector durability response").into());
        }
        pending.finish(&context)?;
        Ok(())
    }

    pub(super) fn read_optional_manifest(
        &self,
        path: &Path,
    ) -> Result<Option<ProjectionGenerationManifest>, ProjectionGenerationError> {
        match self.open_file(path) {
            Ok(mut file) => {
                decode_manifest(&read_bounded_file(&mut file, path, MAX_ENVELOPE_BYTES)?).map(Some)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub(super) fn read_manifest(
        &self,
        path: &Path,
    ) -> Result<ProjectionGenerationManifest, ProjectionGenerationError> {
        self.read_optional_manifest(path)?.ok_or_else(|| {
            ProjectionGenerationError::NotFound(format!(
                "generation manifest {} is missing",
                path.display()
            ))
        })
    }

    pub(super) fn read_optional_head(
        &self,
        path: &Path,
    ) -> Result<Option<ProjectionGenerationHead>, ProjectionGenerationError> {
        match self.open_file(path) {
            Ok(mut file) => {
                decode_head(&read_bounded_file(&mut file, path, MAX_ENVELOPE_BYTES)?).map(Some)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
}

fn corrupt_entry(error: &ProjectionGenerationError) -> bool {
    match error {
        ProjectionGenerationError::Corruption(_) | ProjectionGenerationError::NotFound(_) => true,
        ProjectionGenerationError::Io(error) => matches!(
            error.kind(),
            io::ErrorKind::InvalidData | io::ErrorKind::UnexpectedEof | io::ErrorKind::NotFound
        ),
        _ => false,
    }
}

fn admit_reclamation(
    bytes: u64,
    limits: ProjectionGenerationGcLimits,
    report: &mut ProjectionGenerationGcReport,
) -> bool {
    if report.generations_reclaimed >= limits.max_generations_to_reclaim.get()
        || report
            .bytes_reclaimed
            .checked_add(bytes)
            .is_none_or(|total| {
                total > u64::try_from(limits.max_bytes_to_reclaim.get()).unwrap_or(u64::MAX)
            })
    {
        report.backlog_remaining = true;
        false
    } else {
        true
    }
}

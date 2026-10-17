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

use super::root_registry::PublicationRegistration;
use crate::build_control::checkpoint;
use crate::build_memory::{path::OwnedPath, reserved::native_path, BuildMemory};
use crate::error::{HawDBError, Result};
use hawdb_core::RuntimeTaskContext;
use hawdb_storage::file_io::{File, SourceDirectory, SourceDirectoryLockMode};
use std::fs::TryLockError;
use std::path::Path;

const SEARCH_PROJECTION_PUBLISH_LOCK_FILE: &str = ".search_projection.publish.lock";

#[derive(Debug)]
pub(crate) struct SearchProjectionPublishLease {
    lock_file: File,
    // Close the descriptor before releasing process-local exclusion, including
    // platforms whose native record locks belong to the process.
    root: PublicationRegistration,
}

impl SearchProjectionPublishLease {
    pub(crate) fn acquire(root: &Path) -> Result<Self> {
        let task = RuntimeTaskContext::default();
        let memory = BuildMemory::new(&task)?;
        Self::acquire_with_context(root, &memory, &task)
    }

    pub(crate) fn acquire_for_consumer(root: &Path) -> Result<Self> {
        let task = RuntimeTaskContext::default();
        let memory = BuildMemory::new(&task)?;
        Self::acquire_lock_with_context(root, &memory, &task)
    }

    // Cleanup may inspect registered roots, but must share publication exclusion
    // while discovering the current closure and deleting unreferenced files.
    pub(crate) fn acquire_for_cleanup(root: &Path) -> Result<Self> {
        let task = RuntimeTaskContext::default();
        let memory = BuildMemory::new(&task)?;
        Self::acquire_lock_with_context(root, &memory, &task)
    }

    pub(crate) fn acquire_with_context(
        root: &Path,
        memory: &BuildMemory,
        task: &RuntimeTaskContext,
    ) -> Result<Self> {
        let lease = Self::acquire_lock_with_context(root, memory, task)?;
        // Check the binding under the same exclusion used by consumer owners.
        super::super::consumer::require_unregistered_directory(root, memory, task)?;
        checkpoint(task)?;
        Ok(lease)
    }

    fn acquire_lock_with_context(
        root: &Path,
        memory: &BuildMemory,
        task: &RuntimeTaskContext,
    ) -> Result<Self> {
        checkpoint(task)?;
        let _root_scratch = memory.spool.reserve(native_path::bytes(root)?)?;
        let directory = SourceDirectory::capture_for_path(root)?;
        #[cfg(test)]
        tests::after_capture(root);
        let registered = PublicationRegistration::acquire(directory, memory, task)?;
        let lock_path = OwnedPath::join(
            root,
            Path::new(SEARCH_PROJECTION_PUBLISH_LOCK_FILE),
            memory,
            task,
        )?;
        let _native = memory.spool.reserve(native_path::bytes(&lock_path)?)?;
        checkpoint(task)?;
        let lock_file = registered.with_directory(|directory| {
            directory.open_lock_file(
                &directory.operation_context(),
                SEARCH_PROJECTION_PUBLISH_LOCK_FILE,
                SourceDirectoryLockMode::ReadWriteCreate,
            )
        })?;
        if let Err(error) = lock_file.try_lock() {
            return match error {
                TryLockError::WouldBlock => Err(active_publication()),
                TryLockError::Error(error) => Err(error.into()),
            };
        }
        let lease = Self {
            lock_file,
            root: registered,
        };
        #[cfg(test)]
        tests::after_lock(root);
        // The path may have been replaced between registration and native open,
        // or between opening and locking. Never grant a lease for a different
        // directory/lock file than the authority actually retained.
        lease.require_current_binding(root, &lock_path)?;
        checkpoint(task)?;
        Ok(lease)
    }

    pub(super) fn acquire_for_capture(
        root: &Path,
        memory: &BuildMemory,
        task: &RuntimeTaskContext,
    ) -> Result<Self> {
        Self::acquire_lock_with_context(root, memory, task)
    }

    pub(super) fn source_identity(&self) -> hawdb_storage::file_io::SourceDirectoryIdentity {
        self.root.identity()
    }

    pub(crate) fn protects(&self, key: super::artifact_key::ArtifactKey) -> bool {
        self.root.protects(key)
    }

    pub(super) fn capture_source_root<'a, 'execution>(
        &mut self,
        context: &hawdb_storage::file_descriptors::FileOpenContext,
        admission: &'a crate::build_memory::read::ReadAdmission<'execution>,
    ) -> Result<super::root_registry::PendingCapture<'a, 'execution>> {
        let root = self.admit_source_root(context, admission)?;
        root.begin_capture(self, admission)
    }

    pub(super) fn require_source_root(
        &self,
        root: &Path,
        context: &hawdb_storage::file_descriptors::FileOpenContext,
    ) -> Result<()> {
        self.root.with_directory(|directory| {
            if !directory.shares_project_context(context) {
                return Err(HawDBError::StorageIntegrity(
                    "publication root does not match reader descriptor domain".into(),
                ));
            }
            let named = SourceDirectory::capture(root, context)?;
            if !directory.same_directory(&named) {
                return Err(HawDBError::StorageIntegrity(
                    "publication root does not match reader root".into(),
                ));
            }
            Ok(())
        })
    }

    pub(super) fn admit_source_root(
        &mut self,
        context: &hawdb_storage::file_descriptors::FileOpenContext,
        admission: &crate::build_memory::read::ReadAdmission<'_>,
    ) -> Result<super::root_registry::RootRef> {
        self.root.admit_root(context, admission)
    }

    fn require_current_binding(&self, root: &Path, lock_path: &Path) -> Result<()> {
        self.root.with_directory(|directory| -> Result<()> {
            let named = SourceDirectory::capture(root, &directory.operation_context())?;
            if !directory.same_directory(&named) {
                return Err(HawDBError::StorageIntegrity(
                    "search publication root changed during acquisition".into(),
                ));
            }
            Ok(())
        })?;
        if !self.lock_file.matches_native_path(lock_path)? {
            return Err(HawDBError::StorageIntegrity(
                "search publication lock path changed during acquisition".into(),
            ));
        }
        Ok(())
    }
}

fn active_publication() -> HawDBError {
    HawDBError::Storage("another search projection publication is active".into())
}

impl Drop for SearchProjectionPublishLease {
    fn drop(&mut self) {
        let _ = File::unlock(&self.lock_file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hawdb_storage::file_io as fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    type Hook = fn(&Path);
    thread_local! {
        static AFTER_CAPTURE: std::cell::Cell<Option<Hook>> = const { std::cell::Cell::new(None) };
        static AFTER_LOCK: std::cell::Cell<Option<Hook>> = const { std::cell::Cell::new(None) };
    }

    pub(super) fn after_capture(root: &Path) {
        if let Some(hook) = AFTER_CAPTURE.with(|slot| slot.take()) {
            hook(root);
        }
    }
    pub(super) fn after_lock(root: &Path) {
        if let Some(hook) = AFTER_LOCK.with(|slot| slot.take()) {
            hook(root);
        }
    }

    #[test]
    fn replacement_after_capture_cannot_misbind_the_native_lease() {
        let root = test_dir();
        fs::create_dir_all(&root).unwrap();
        AFTER_CAPTURE.with(|slot| {
            slot.set(Some(|root| {
                fs::rename(root, root.with_extension("replaced")).unwrap();
                fs::create_dir(root).unwrap();
            }))
        });
        let result = SearchProjectionPublishLease::acquire(&root);
        assert!(AFTER_CAPTURE.with(|slot| slot.take()).is_none());
        assert!(matches!(result, Err(HawDBError::StorageIntegrity(_))));
        assert!(!root.join(SEARCH_PROJECTION_PUBLISH_LOCK_FILE).exists());
        assert!(root
            .with_extension("replaced")
            .join(SEARCH_PROJECTION_PUBLISH_LOCK_FILE)
            .is_file());
        drop(SearchProjectionPublishLease::acquire(&root).unwrap());
        fs::remove_dir_all(&root).unwrap();
        fs::remove_dir_all(root.with_extension("replaced")).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn replacement_after_process_and_publication_locks_cannot_grant_foreign_root_authority() {
        let root = test_dir();
        fs::create_dir_all(&root).unwrap();
        AFTER_LOCK.with(|slot| {
            slot.set(Some(|root| {
                fs::rename(root, root.with_extension("replaced")).unwrap();
                fs::create_dir(root).unwrap();
                fs::write(root.join("foreign"), b"preserve foreign root").unwrap();
            }))
        });
        let result = SearchProjectionPublishLease::acquire(&root);
        assert!(matches!(result, Err(HawDBError::StorageIntegrity(_))));
        assert!(AFTER_LOCK.with(|slot| slot.take()).is_none());
        assert_eq!(
            fs::read(root.join("foreign")).unwrap(),
            b"preserve foreign root"
        );
        assert!(!root.join(SEARCH_PROJECTION_PUBLISH_LOCK_FILE).exists());
        assert!(root
            .with_extension("replaced")
            .join(SEARCH_PROJECTION_PUBLISH_LOCK_FILE)
            .is_file());
        drop(SearchProjectionPublishLease::acquire(&root).unwrap());
        fs::remove_dir_all(&root).unwrap();
        fs::remove_dir_all(root.with_extension("replaced")).unwrap();
    }

    #[test]
    fn replacement_after_lock_cannot_turn_old_file_lock_into_new_path_authority() {
        let root = test_dir();
        fs::create_dir_all(&root).unwrap();
        AFTER_LOCK.with(|slot| {
            slot.set(Some(|root| {
                let path = root.join(SEARCH_PROJECTION_PUBLISH_LOCK_FILE);
                fs::rename(&path, root.join("replaced-lock")).unwrap();
                fs::write(path, b"").unwrap();
            }))
        });
        let result = SearchProjectionPublishLease::acquire(&root);
        assert!(AFTER_LOCK.with(|slot| slot.take()).is_none());
        assert!(matches!(result, Err(HawDBError::StorageIntegrity(_))));
        drop(SearchProjectionPublishLease::acquire(&root).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn publication_lease_rejects_concurrent_owner_and_recovers_after_drop() {
        let root = test_dir();
        fs::create_dir_all(&root).unwrap();
        let first = SearchProjectionPublishLease::acquire(&root).unwrap();
        assert!(SearchProjectionPublishLease::acquire(&root)
            .unwrap_err()
            .to_string()
            .contains("another search projection publication is active"));
        drop(first);
        SearchProjectionPublishLease::acquire(&root).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn publication_lease_recovers_after_failed_open_and_cancellation() {
        let root = test_dir();
        let lock = root.join(SEARCH_PROJECTION_PUBLISH_LOCK_FILE);
        fs::create_dir_all(&lock).unwrap();
        let task = RuntimeTaskContext::default();
        let memory = BuildMemory::new(&task).unwrap();
        assert!(SearchProjectionPublishLease::acquire_with_context(&root, &memory, &task).is_err());
        assert_eq!(memory.ledger.snapshot().used_bytes, 0);
        fs::remove_dir(&lock).unwrap();
        task.cancellation().cancel();
        assert!(SearchProjectionPublishLease::acquire_with_context(&root, &memory, &task).is_err());
        assert!(!lock.exists());
        SearchProjectionPublishLease::acquire(&root).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn context_publication_preserves_consumer_exclusion_and_releases_rejected_leases() {
        let root = test_dir();
        fs::create_dir_all(&root).unwrap();
        let snapshot = root.join(crate::SEARCH_SNAPSHOT_FILE);
        let task = RuntimeTaskContext::default();
        let memory = BuildMemory::new(&task).unwrap();
        let header = "HAWDB_SEARCH_PROJECTION_V1\nprojection_consumer_binding\towner\n";
        for contents in [
            header.as_bytes().to_vec(),
            crate::encode_search_snapshot_text(header).unwrap(),
        ] {
            fs::write(&snapshot, contents).unwrap();
            for rejected in [
                SearchProjectionPublishLease::acquire(&root),
                SearchProjectionPublishLease::acquire_with_context(&root, &memory, &task),
            ] {
                assert!(rejected
                    .unwrap_err()
                    .to_string()
                    .contains("registered projection requires its consumer owner"));
            }
            assert_eq!(memory.ledger.snapshot().used_bytes, 0);
            let owner = SearchProjectionPublishLease::acquire_for_consumer(&root).unwrap();
            assert!(
                SearchProjectionPublishLease::acquire_with_context(&root, &memory, &task)
                    .unwrap_err()
                    .to_string()
                    .contains("another search projection publication is active")
            );
            assert_eq!(memory.ledger.snapshot().used_bytes, 0);
            drop(owner);
        }
        fs::remove_file(snapshot).unwrap();
        drop(SearchProjectionPublishLease::acquire_with_context(&root, &memory, &task).unwrap());
        assert_eq!(memory.ledger.snapshot().used_bytes, 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn publication_control_probe_covers_requested_allocations() {
        use crate::test_allocation as allocation;
        let _serial = allocation::serial();
        assert_eq!(allocation::live(), 0);
        let root = test_dir();
        fs::create_dir_all(&root).unwrap();
        let snapshot = root.join(crate::SEARCH_SNAPSHOT_FILE);
        let text = format!(
            "HAWDB_SEARCH_PROJECTION_V1\nsource_graph_commit_epoch\t1\n{}",
            "payload".repeat(2048)
        );
        let variants = [
            ("plain", text.as_bytes().to_vec()),
            (
                "compressed",
                crate::encode_search_snapshot_text(&text).unwrap(),
            ),
        ];
        let mut observations = Vec::with_capacity(variants.len());
        for (name, bytes) in variants {
            fs::write(&snapshot, bytes).unwrap();
            let task = RuntimeTaskContext::default();
            let memory = BuildMemory::new(&task).unwrap();
            drop(memory.input.reserve(1).unwrap());
            drop(memory.spool.reserve(1).unwrap());
            drop(memory.retained.reserve(1).unwrap());
            let (lease, peak) = allocation::measure(|| {
                SearchProjectionPublishLease::acquire_with_context(&root, &memory, &task).unwrap()
            });
            let admitted_peak = memory.ledger.snapshot().peak_bytes;
            drop(lease);
            assert_eq!(allocation::live(), 0);
            assert_eq!(memory.ledger.snapshot().used_bytes, 0);
            observations.push((name, peak, admitted_peak));
        }
        fs::remove_dir_all(root).unwrap();
        for (name, peak, admitted_peak) in &observations {
            eprintln!("control probe {name}: requested_peak={peak}, admitted_peak={admitted_peak}");
        }
        assert!(
            observations
                .iter()
                .all(|(_, peak, admitted)| peak <= admitted),
            "publication control-record buffers must be admitted before allocation"
        );
    }

    #[test]
    fn publication_control_probe_releases_exclusion_after_admission_denial() {
        use hawdb_core::RuntimeMemoryReservation;
        let root = test_dir();
        fs::create_dir_all(&root).unwrap();
        let snapshot = root.join(crate::SEARCH_SNAPSHOT_FILE);
        let contents = b"HAWDB_SEARCH_PROJECTION_V1\nsource_graph_commit_epoch\t1\n";
        for bytes in [
            contents.to_vec(),
            crate::encode_search_snapshot_text(std::str::from_utf8(contents).unwrap()).unwrap(),
        ] {
            fs::write(&snapshot, &bytes).unwrap();
            let task = RuntimeTaskContext::default()
                .with_memory_reservation(RuntimeMemoryReservation::new(1024, 0));
            let memory = BuildMemory::new(&task).unwrap();
            let error = SearchProjectionPublishLease::acquire_with_context(&root, &memory, &task)
                .unwrap_err();
            assert!(error.to_string().contains("memory"));
            assert_eq!(memory.ledger.snapshot().used_bytes, 0);
            assert_eq!(fs::read(&snapshot).unwrap(), bytes);
            drop(SearchProjectionPublishLease::acquire_for_consumer(&root).unwrap());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn publication_lease_has_one_concurrent_thread_owner() {
        let root = test_dir();
        fs::create_dir_all(&root).unwrap();
        let barrier = std::sync::Barrier::new(16);
        let winners = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..16 {
                let root = &root;
                let barrier = &barrier;
                let winners = &winners;
                scope.spawn(move || {
                    barrier.wait();
                    let lease = SearchProjectionPublishLease::acquire(root);
                    if lease.is_ok() {
                        winners.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    barrier.wait();
                    drop(lease);
                });
            }
        });
        assert_eq!(winners.load(std::sync::atomic::Ordering::Relaxed), 1);
        SearchProjectionPublishLease::acquire(&root).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn publication_registry_owns_each_node_until_removal_and_rejects_aliases() {
        use crate::test_allocation as allocation;
        use hawdb_core::RuntimeMemoryReservation;
        let _serial = allocation::serial();
        assert_eq!(allocation::live(), 0);
        let root = test_dir();
        let other = root.join("other");
        let alias = root.join("alias");
        fs::create_dir_all(&other).unwrap();
        std::os::unix::fs::symlink(&other, &alias).unwrap();
        let task = RuntimeTaskContext::default();
        let first_memory = BuildMemory::new(&task).unwrap();
        let second_memory = BuildMemory::new(&task).unwrap();
        drop(first_memory.retained.reserve(1).unwrap());
        let capture = |path: &Path| SourceDirectory::capture_for_path(path).unwrap();
        let directory = capture(&root);
        let (first, peak) = allocation::measure(|| {
            PublicationRegistration::acquire(directory, &first_memory, &task).unwrap()
        });
        let bytes = first_memory.ledger.snapshot().used_bytes;
        assert!(bytes > 0);
        assert_eq!(allocation::live(), bytes);
        assert!(peak <= first_memory.ledger.snapshot().peak_bytes);
        let second =
            PublicationRegistration::acquire(capture(&other), &second_memory, &task).unwrap();
        assert!(PublicationRegistration::acquire(capture(&alias), &second_memory, &task).is_err());
        assert_eq!(second_memory.ledger.snapshot().used_bytes, bytes);
        drop(first);
        assert_eq!(first_memory.ledger.snapshot().used_bytes, 0);
        assert_eq!(allocation::live(), 0);
        assert_eq!(second_memory.ledger.snapshot().used_bytes, bytes);
        drop(second);
        assert_eq!(second_memory.ledger.snapshot().used_bytes, 0);
        for short in [1, 0] {
            let task = RuntimeTaskContext::default()
                .with_memory_reservation(RuntimeMemoryReservation::new((bytes - short) as u64, 0));
            let memory = BuildMemory::new(&task).unwrap();
            let lease = PublicationRegistration::acquire(capture(&root), &memory, &task);
            assert_eq!(lease.is_ok(), short == 0);
            drop(lease);
            assert_eq!(memory.ledger.snapshot().used_bytes, 0);
        }
        fs::remove_dir_all(root).unwrap();
    }

    fn test_dir() -> PathBuf {
        // Parallel tests may observe the same clock tick on macOS. Each fixture
        // must own a distinct directory so its cleanup cannot remove another.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        crate::test_temp_dir().join(format!(
            "hawdb_search_publish_lease_{}_{}_{}",
            std::process::id(),
            nanos,
            sequence
        ))
    }
}

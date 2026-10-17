// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

use super::*;
use crate::build_memory::{read::tests::governor, BuildMemory};
use hawdb_core::RuntimeTaskContext;
use hawdb_qos::{RuntimeGovernor, RuntimePermit, RuntimeWorkRequest};
use hawdb_storage::file_descriptors::ProjectFileDescriptors;
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::{Arc, Barrier};

type Hook = RefCell<Option<Box<dyn FnOnce()>>>;
thread_local! {
    pub(super) static AFTER_INITIALIZING: Hook = const { RefCell::new(None) };
    pub(super) static AFTER_RETIRING: Hook = const { RefCell::new(None) };
    pub(super) static AFTER_NATIVE_CLOSE: Hook = const { RefCell::new(None) };
}
pub(super) fn hook(slot: &'static std::thread::LocalKey<Hook>) {
    let action = slot.with(|slot| slot.borrow_mut().take());
    if let Some(action) = action {
        action();
    }
}

struct Fixture {
    root: PathBuf,
    project: ProjectFileDescriptors,
    governor: RuntimeGovernor,
    permit: RuntimePermit,
    task: RuntimeTaskContext,
    memory: BuildMemory,
}
impl Fixture {
    fn new(limit: usize) -> Self {
        let root = std::env::temp_dir().join(format!(
            "hawdb-root-registry-{}",
            hawdb_core::generate_uuidv7().unwrap()
        ));
        let project = ProjectFileDescriptors::acquire(&root, limit).unwrap();
        let governor = governor();
        let permit = governor
            .try_admit(RuntimeWorkRequest::background_maintenance(16 << 20).with_io_wave_slots(1))
            .unwrap();
        let task = permit.bind_task_context(Default::default());
        let memory = BuildMemory::new(&task).unwrap();
        Self {
            root,
            project,
            governor,
            permit,
            task,
            memory,
        }
    }
    fn admission(&self) -> ReadAdmission<'_> {
        ReadAdmission {
            memory: &self.memory,
            task: &self.task,
            permit: &self.permit,
        }
    }
    fn open(&self) -> Result<RootRef> {
        RootRef::acquire(&self.root, &self.project.io_context(), &self.admission())
    }
    fn is_registered(&self) -> bool {
        let directory = SourceDirectory::capture(&self.root, &self.project.io_context()).unwrap();
        registry()
            .iter()
            .any(|entry| entry.state.identity == directory.identity())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.project.metrics().open, 0);
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn shared_root_clones_reuse_native_handles_and_refund_after_last_drop() {
    let fixture = Fixture::new(8);
    let baseline = fixture.memory.ledger.snapshot().used_bytes;
    let root = fixture.open().unwrap();
    assert_eq!(fixture.project.metrics().open, 2);
    let charged = fixture.memory.ledger.snapshot().used_bytes;
    assert!(charged > baseline);
    let copy = root.clone();
    let reopen = fixture.open().unwrap();
    assert_eq!(fixture.memory.ledger.snapshot().used_bytes, charged);
    assert_eq!(fixture.project.metrics().open, 2);
    assert_eq!(fixture.governor.snapshot().active_background_io_slots, 0);
    drop(root);
    drop(reopen);
    assert!(fixture.is_registered());
    assert_eq!(fixture.project.metrics().open, 2);
    drop(copy);
    assert_eq!(fixture.memory.ledger.snapshot().used_bytes, baseline);
    assert!(!fixture.is_registered());
}

#[test]
fn head_read_follows_captured_directory_after_path_replacement_with_one_io_slot() {
    let fixture = Fixture::new(8);
    std::fs::write(fixture.root.join("head"), b"captured head").unwrap();
    let root = fixture.open().unwrap();
    let moved = fixture.root.with_extension("moved");
    std::fs::rename(&fixture.root, &moved).unwrap();
    std::fs::create_dir(&fixture.root).unwrap();
    std::fs::write(fixture.root.join("head"), b"foreign head").unwrap();
    let baseline = fixture.memory.ledger.snapshot().used_bytes;
    let head = root
        .read_head(
            &fixture.project.io_context(),
            "head",
            &fixture.admission(),
            64,
        )
        .unwrap();
    assert_eq!(&*head, b"captured head");
    assert_eq!(
        std::fs::read(fixture.root.join("head")).unwrap(),
        b"foreign head"
    );
    assert_eq!(fixture.project.metrics().open, 2);
    assert_eq!(fixture.governor.snapshot().active_background_io_slots, 0);
    drop(head);
    assert_eq!(fixture.memory.ledger.snapshot().used_bytes, baseline);
    drop(root);
    std::fs::remove_dir_all(moved).unwrap();
}

#[test]
fn held_head_denial_returns_descriptor_memory_and_io_capacity() {
    let fixture = Fixture::new(8);
    std::fs::write(fixture.root.join("head"), [7_u8; 1024]).unwrap();
    let root = fixture.open().unwrap();
    let baseline = fixture.memory.ledger.snapshot().used_bytes;
    assert!(root
        .read_head(
            &fixture.project.io_context(),
            "head",
            &fixture.admission(),
            8
        )
        .is_err());
    assert_eq!(fixture.project.metrics().open, 2);
    assert_eq!(fixture.memory.ledger.snapshot().used_bytes, baseline);
    assert_eq!(fixture.governor.snapshot().active_background_io_slots, 0);
    // Capture a context without an inventory before reserving all free slots.
    let context = fixture.project.io_context();
    let reserved = fixture.project.reserve(6).unwrap();
    assert!(root
        .read_head(&context, "head", &fixture.admission(), 2048)
        .is_err());
    assert_eq!(fixture.project.metrics().open, 2);
    assert_eq!(fixture.project.metrics().reserved, 6);
    assert_eq!(fixture.memory.ledger.snapshot().used_bytes, baseline);
    assert_eq!(fixture.governor.snapshot().active_background_io_slots, 0);
    drop(reserved);
    drop(root);
}

#[test]
fn two_concurrent_final_references_retire_instead_of_leaving_registry_owner() {
    let fixture = Fixture::new(8);
    let baseline = fixture.memory.ledger.snapshot().used_bytes;
    for _ in 0..32 {
        let root = fixture.open().unwrap();
        let copy = root.clone();
        let start = Arc::new(Barrier::new(2));
        std::thread::scope(|scope| {
            let other = start.clone();
            scope.spawn(move || {
                other.wait();
                drop(root);
            });
            scope.spawn(move || {
                start.wait();
                drop(copy);
            });
        });
        assert_eq!(fixture.project.metrics().open, 0);
        assert_eq!(fixture.memory.ledger.snapshot().used_bytes, baseline);
        assert!(!fixture.is_registered());
    }
}

#[test]
fn retirement_excludes_reacquisition_before_and_after_actual_native_close() {
    use std::sync::mpsc;
    use std::time::Duration;
    let fixture = Fixture::new(8);
    let root = fixture.open().unwrap();
    let (events, observed) = mpsc::channel();
    let (release_before, continue_before) = mpsc::channel();
    let (release_after, continue_after) = mpsc::channel();
    std::thread::scope(|scope| {
        let worker = scope.spawn(move || {
            let first = events.clone();
            AFTER_RETIRING.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move || {
                    first.send("retiring").unwrap();
                    continue_before
                        .recv_timeout(Duration::from_secs(5))
                        .unwrap();
                }))
            });
            AFTER_NATIVE_CLOSE.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move || {
                    events.send("closed").unwrap();
                    continue_after.recv_timeout(Duration::from_secs(5)).unwrap();
                }))
            });
            drop(root);
        });
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(5)).unwrap(),
            "retiring"
        );
        let before_count = fixture.project.metrics().open;
        let before_denied = fixture.open().is_err();
        release_before.send(()).unwrap();
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(5)).unwrap(),
            "closed"
        );
        let after_count = fixture.project.metrics().open;
        let after_denied = fixture.open().is_err();
        release_after.send(()).unwrap();
        worker.join().unwrap();
        assert_eq!(before_count, 2);
        assert!(before_denied);
        assert_eq!(after_count, 0);
        assert!(after_denied);
    });
    assert!(!fixture.is_registered());
    drop(fixture.open().unwrap());
}

#[test]
fn lock_fd_denial_cleans_initializing_entry_and_never_creates_lock() {
    let fixture = Fixture::new(1);
    let baseline = fixture.memory.ledger.snapshot().used_bytes;
    assert!(fixture.open().is_err());
    assert_eq!(fixture.project.metrics().open, 0);
    assert!(!fixture.root.join(PROCESS_LOCK).exists());
    assert!(!fixture.is_registered());
    assert_eq!(fixture.memory.ledger.snapshot().used_bytes, baseline);
}

#[test]
fn a_different_project_domain_cannot_borrow_a_ready_roots_descriptors() {
    let owner = Fixture::new(8);
    let foreign = Fixture::new(8);
    let root = owner.open().unwrap();
    let result = RootRef::acquire(
        &owner.root,
        &foreign.project.io_context(),
        &foreign.admission(),
    );
    assert!(
        matches!(result, Err(HawDBError::Storage(ref message)) if message == "source root belongs to another project descriptor domain")
    );
    assert_eq!(foreign.project.metrics().open, 0);
    assert_eq!(owner.project.metrics().open, 2);
    drop(root);
}

#[test]
fn cancelled_initialization_closes_before_unregistering_and_allows_retry() {
    let fixture = Fixture::new(8);
    let cancel = fixture.task.cancellation().clone();
    AFTER_INITIALIZING.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || {
            cancel.cancel();
        }))
    });
    assert!(fixture.open().is_err());
    assert_eq!(fixture.project.metrics().open, 0);
    assert!(!fixture.is_registered());
    let task = fixture.permit.bind_task_context(Default::default());
    let admission = ReadAdmission {
        memory: &fixture.memory,
        task: &task,
        permit: &fixture.permit,
    };
    drop(RootRef::acquire(&fixture.root, &fixture.project.io_context(), &admission).unwrap());
}

#[test]
fn retained_root_releases_execution_but_keeps_peer_exclusion_until_final_close() {
    const PROBE: &str = "HAWDB_ROOT_REGISTRY_PROBE";
    const BUSY: &str = "HAWDB_ROOT_REGISTRY_PROBE_BUSY";
    const CASE: &str = "out_of_core::root_registry::tests::retained_root_releases_execution_but_keeps_peer_exclusion_until_final_close";
    if let Some(root) = std::env::var_os(PROBE) {
        let project = ProjectFileDescriptors::acquire_existing(Path::new(&root), 8).unwrap();
        let governor = governor();
        let permit = governor
            .try_admit(RuntimeWorkRequest::background_maintenance(16 << 20).with_io_wave_slots(1))
            .unwrap();
        let task = permit.bind_task_context(Default::default());
        let memory = BuildMemory::new(&task).unwrap();
        let admission = ReadAdmission {
            memory: &memory,
            task: &task,
            permit: &permit,
        };
        let result = RootRef::acquire(Path::new(&root), &project.io_context(), &admission);
        if std::env::var_os(BUSY).is_some() {
            assert!(
                matches!(result, Err(HawDBError::Storage(ref message)) if message == "source root is owned by another process")
            );
        } else {
            drop(result.unwrap());
        }
        assert_eq!(project.metrics().open, 0);
        return;
    }
    // The shared test_dir helper retains a default-1024 project. This case
    // owns an explicit eight-FD domain, including in the peer process.
    let root = std::env::temp_dir().join(format!(
        "hawdb-root-registry-retained-peer-{}",
        hawdb_core::generate_uuidv7().unwrap()
    ));
    let project = ProjectFileDescriptors::acquire(&root, 8).unwrap();
    let governor = governor();
    let retained = {
        let permit = governor
            .try_admit(RuntimeWorkRequest::background_maintenance(16 << 20).with_io_wave_slots(1))
            .unwrap();
        let task = permit.bind_task_context(Default::default());
        let memory = BuildMemory::new(&task).unwrap();
        RootRef::acquire(
            &root,
            &project.io_context(),
            &ReadAdmission {
                memory: &memory,
                task: &task,
                permit: &permit,
            },
        )
        .unwrap()
    };
    let snapshot = governor.snapshot();
    assert_eq!(snapshot.active_cpu_slots, 0);
    assert_eq!(snapshot.active_background_tasks, 0);
    assert_eq!(snapshot.active_background_io_slots, 0);
    assert!(snapshot.admitted_memory_bytes > 0);
    assert_eq!(project.metrics().open, 2);
    let probe = |busy| {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", CASE, "--nocapture"])
            .env(PROBE, &root);
        if busy {
            command.env(BUSY, "1");
        } else {
            command.env_remove(BUSY);
        }
        let result = command.output().unwrap();
        let output = String::from_utf8_lossy(&result.stdout);
        assert!(
            result.status.success(),
            "{output}\n{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            output.contains("1 passed; 0 failed"),
            "probe selected no case: {output}"
        );
    };
    probe(true);
    // A new operation can be admitted and reuse custody without opening a
    // second process-lock descriptor or retaining the old execution permit.
    {
        let permit = governor
            .try_admit(RuntimeWorkRequest::background_maintenance(16 << 20).with_io_wave_slots(1))
            .unwrap();
        let task = permit.bind_task_context(Default::default());
        let memory = BuildMemory::new(&task).unwrap();
        let successor = RootRef::acquire(
            &root,
            &project.io_context(),
            &ReadAdmission {
                memory: &memory,
                task: &task,
                permit: &permit,
            },
        )
        .unwrap();
        assert_eq!(project.metrics().open, 2);
        drop(successor);
    }
    probe(true);
    drop(retained);
    assert_eq!(project.metrics().open, 0);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    probe(false);
    assert!(root.exists(), "runtime custody never deletes durable roots");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn cold_registry_gate_has_no_query_lifetime_heap() {
    use crate::test_allocation as allocation;
    let _serial = allocation::serial();
    assert_eq!(allocation::live(), 0);
    let (_, peak) = allocation::measure(|| drop(registry()));
    assert_eq!(peak, 0);
    assert_eq!(allocation::live(), 0);
}

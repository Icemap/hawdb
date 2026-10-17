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

use super::*;
use crate::test_allocation as allocation;
use hawdb_qos::{
    IoConcurrencyBudget, RuntimeGovernor, RuntimeGovernorConfig, RuntimeMemorySnapshot,
    RuntimeResourceBudget, RuntimeResourceSnapshot, RuntimeWorkRequest,
};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) fn governor() -> RuntimeGovernor {
    RuntimeGovernor::new(
        RuntimeGovernorConfig {
            memory_budget_bytes: Some(64 << 20),
            background_task_limit: Some(NonZeroUsize::MIN),
            ..RuntimeGovernorConfig::shared_host()
        },
        RuntimeResourceSnapshot::from_parts(
            RuntimeResourceBudget::from_limits(NonZeroUsize::new(2).unwrap(), None, None),
            RuntimeMemorySnapshot::from_limits(Some(1 << 30), Some(1 << 30), None, None, None),
        ),
        IoConcurrencyBudget::new(2, 1),
    )
}

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "hawdb-reader-input-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn warm(memory: &BuildMemory) {
    for account in [&memory.input, &memory.spool, &memory.retained] {
        drop(account.reserve(1).unwrap());
    }
}

#[test]
fn bounded_input_keeps_the_opened_file_after_path_replacement() {
    let directory = Directory::new();
    let path = directory.0.join("artifact");
    let bytes = b"selected old snapshot";
    std::fs::write(&path, bytes).unwrap();
    let replace = path.clone();
    let old = directory.0.join("held-old");
    AFTER_FILE_OPEN.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || {
            std::fs::rename(&replace, old).unwrap();
            std::fs::write(replace, vec![b'x'; bytes.len()]).unwrap();
        }))
    });
    let governor = governor();
    let permit = governor
        .try_admit(RuntimeWorkRequest::background_maintenance(2 << 20).with_io_wave_slots(1))
        .unwrap();
    let task = permit.bind_task_context(Default::default());
    let memory = BuildMemory::new(&task).unwrap();
    warm(&memory);
    let baseline = memory.ledger.snapshot().used_bytes;
    let input = ReadAdmission {
        memory: &memory,
        task: &task,
        permit: &permit,
    }
    .read_bound(
        &path,
        bytes.len() as u64,
        crate::checksum_bytes(bytes),
        1 << 20,
    )
    .unwrap();
    assert_eq!(&*input, bytes);
    assert_ne!(std::fs::read(path).unwrap(), bytes);
    assert_eq!(governor.snapshot().active_background_io_slots, 0);
    assert!(memory.ledger.snapshot().used_bytes >= baseline + bytes.len());
    drop(input);
    assert_eq!(memory.ledger.snapshot().used_bytes, baseline);
}

#[test]
fn cancellation_after_native_open_precedes_large_buffer_allocation() {
    let _serial = allocation::serial();
    assert_eq!(allocation::live(), 0);
    let directory = Directory::new();
    let project = hawdb_storage::file_descriptors::ProjectFileDescriptors::acquire_component(
        &directory.0,
        false,
    )
    .unwrap();
    let path = directory.0.join("artifact");
    let bytes = vec![b'x'; 3 << 20];
    std::fs::write(&path, &bytes).unwrap();
    let governor = governor();
    let permit = governor
        .try_admit(RuntimeWorkRequest::background_maintenance(16 << 20).with_io_wave_slots(1))
        .unwrap();
    let task = permit.bind_task_context(Default::default());
    let memory = BuildMemory::new(&task).unwrap();
    warm(&memory);
    drop(task.acquire_io_wave(NonZeroUsize::MIN).unwrap());
    let cancel = task.cancellation().clone();
    AFTER_FILE_OPEN.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || {
            cancel.cancel();
        }));
    });
    let baseline = memory.ledger.snapshot().used_bytes;
    let host = governor.snapshot().admitted_memory_bytes;
    let (_, peak) = allocation::measure(|| {
        let result = ReadAdmission {
            memory: &memory,
            task: &task,
            permit: &permit,
        }
        .read_bound(&path, bytes.len() as u64, 0, 4 << 20);
        assert!(matches!(result, Err(HawDBError::Execution(_))));
    });
    assert!(
        peak < 16 << 10,
        "cancelled open allocated an encoded buffer: {peak}"
    );
    assert_eq!(memory.ledger.snapshot().used_bytes, baseline);
    assert_eq!(governor.snapshot().admitted_memory_bytes, host);
    assert_eq!(governor.snapshot().active_background_io_slots, 0);
    // Native task/project control state may initialize during I/O. Its real
    // owners remain covered by working admission until their destruction.
    assert!(host >= allocation::live() as u64);
    drop(memory);
    drop(task);
    drop(project);
    assert_eq!(allocation::live(), 0);
    drop(permit);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
}

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
use crate::background::{CheckpointOperationError, CheckpointWorkError};
use hawdb_core::{RuntimeMemoryError, RuntimeTaskContext};

pub(super) fn governor() -> hawdb_qos::RuntimeGovernor {
    use hawdb_qos::{
        IoConcurrencyBudget, RuntimeGovernor, RuntimeGovernorConfig, RuntimeMemorySnapshot,
        RuntimeResourceBudget, RuntimeResourceSnapshot,
    };
    let governor = RuntimeGovernor::new(
        RuntimeGovernorConfig {
            memory_budget_bytes: Some(4 * 1024 * 1024),
            ..RuntimeGovernorConfig::shared_host()
        },
        RuntimeResourceSnapshot::from_parts(
            RuntimeResourceBudget::from_limits(std::num::NonZeroUsize::MIN, None, None),
            RuntimeMemorySnapshot::from_limits(Some(1 << 30), Some(1 << 30), None, None, None),
        ),
        IoConcurrencyBudget::new(2, 1),
    );
    governor.pin_resources();
    governor
}

fn input() -> Vec<u8> {
    let mut seed = 0x6a09_e667_f3bc_c909u64;
    (0..512 * 1024)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed as u8
        })
        .collect()
}

#[test]
fn append_compression_denies_before_allocating_a_wide_output_or_native_workspace() {
    let input = input();
    let governor = governor();
    let admission = governor
        .try_admit_incremental_maintenance(0, 32 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone());
    let observation = crate::test_allocator::AllocationObservation::start();
    let result = work.classify(|work| compress(&input, 3, work));
    let allocations = observation.finish();
    let denied = matches!(
        result,
        Err(CheckpointOperationError::Work(CheckpointWorkError::Memory(
            RuntimeMemoryError::ReservationExceeded { requested_bytes, available_bytes }
        ))) if requested_bytes > 512 * 1024 && available_bytes == 32 * 1024
    );
    drop(result);
    assert_eq!(admission.memory_report().live_accounted_bytes, 0);
    drop(work);
    drop(admission);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    assert_eq!(
        allocations, 0,
        "compression allocated large unadmitted buffers"
    );
    assert!(denied, "32 KiB cannot admit the 512 KiB compressed output");
}

#[test]
fn append_compression_retains_exact_output_ownership_after_task_close() {
    let input = input();
    let expected = zstd::stream::encode_all(input.as_slice(), 3).unwrap();
    let governor = governor();
    let admission = governor
        .try_admit_incremental_maintenance(0, 4 * 1024 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone());
    let output = work
        .classify(|work| compress(&input, 3, work))
        .unwrap_or_else(|error| {
            panic!(
                "compression failed: {error:?}; ledger: {:?}",
                admission.memory_report()
            )
        });
    assert_eq!(&output[..], expected);
    let report = admission.memory_report();
    let retained = report.live_accounted_bytes;
    assert!(report.peak_accounted_bytes > retained + 1024 * 1024);
    assert!(report.peak_accounted_bytes <= 4 * 1024 * 1024);
    drop(work);
    drop(admission);
    assert_eq!(governor.snapshot().admitted_memory_bytes, retained);
    assert_eq!(&output[..], expected);
    assert!(
        retained >= output.len() as u64,
        "compressed output escaped its admission"
    );
    drop(output);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    println!(
        "compressed output retains {retained} bytes; peak including native workspace: {}",
        report.peak_accounted_bytes
    );
}

#[test]
fn append_compression_cancels_each_cpu_unit_and_refunds_then_retries_identical_bytes() {
    use hawdb_core::RuntimeCancellationReason;
    let input = input();
    let expected = zstd::stream::encode_all(input.as_slice(), 3).unwrap();
    let governor = governor();
    let mut cuts = vec![0];
    let mut index = 0;
    while index < cuts.len() {
        let cut = cuts[index];
        let admission = governor
            .try_admit_incremental_maintenance(0, 4 * 1024 * 1024, 1, RuntimeTaskContext::default())
            .unwrap();
        let task = admission.task_context().unwrap().clone();
        let local = scheduler();
        let probe = Arc::new(CheckpointWorkProbe {
            cancellation: task.cancellation().clone(),
            ..CheckpointWorkProbe::default()
        });
        local.set_telemetry_sink(Some(probe.clone()));
        probe.cancel_after.store(cut, Ordering::SeqCst);
        let work = CheckpointWorkContext::new(task).with_scheduler(local.clone());
        let result = work.classify(|work| compress(&input, 3, work));
        if cut == 0 {
            let output = result.unwrap();
            assert_eq!(&output[..], expected);
            if index == 0 {
                let units = probe.completed.load(Ordering::SeqCst);
                assert!(units > 16);
                cuts.extend(1..=units);
                cuts.push(0);
            }
            drop(output);
        } else {
            assert!(
                matches!(
                    result,
                    Err(CheckpointOperationError::Work(
                        CheckpointWorkError::Stopped(RuntimeCancellationReason::Cancelled)
                            | CheckpointWorkError::Memory(RuntimeMemoryError::Stopped(
                                RuntimeCancellationReason::Cancelled
                            ))
                    ))
                ),
                "compression cut {cut}: {result:?}"
            );
            assert_eq!(probe.completed.load(Ordering::SeqCst), cut);
        }
        assert_eq!(admission.memory_report().live_accounted_bytes, 0);
        probe.assert_released(&local);
        drop(work);
        drop(admission);
        assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
        index += 1;
    }
    println!(
        "verified {} compression CPU cancellation cuts and complete retry",
        cuts.len() - 2
    );
}

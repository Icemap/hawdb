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

fn config() -> AppendSegmentConfig {
    AppendSegmentConfig {
        overflow_threshold_bytes: 1024 * 1024,
        ..AppendSegmentConfig::default()
    }
}

fn row() -> RelationalRow {
    RelationalRow::new(vec![
        RelationalValue::Bytea(vec![37; 512 * 1024]),
        RelationalValue::Text("界x".repeat(256 * 1024)),
    ])
}

#[test]
fn externalized_inline_copy_denies_before_large_unadmitted_allocations() {
    let row = row();
    let governor = output_memory::governor();
    let admission = governor
        .try_admit_incremental_maintenance(0, 32 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone());
    let observation = crate::test_allocator::AllocationObservation::start();
    let result = work.classify(|work| externalize(&row, config(), work));
    let allocations = observation.finish();
    let denied = matches!(
        result,
        Err(CheckpointOperationError::Work(CheckpointWorkError::Memory(
            RuntimeMemoryError::ReservationExceeded { .. }
        )))
    );
    drop(result);
    assert_eq!(admission.memory_report().live_accounted_bytes, 0);
    drop(work);
    drop(admission);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    assert_eq!(allocations, 0, "inline fields copied before admission");
    assert!(denied);
}

#[test]
fn externalized_inline_values_and_array_retain_memory_after_envelopes_drop() {
    let row = row();
    let expected = externalize_row(&row, config()).unwrap();
    let governor = output_memory::governor();
    let admission = governor
        .try_admit_incremental_maintenance(0, 8 * 1024 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone());
    let (externalized, overflows) = externalize(&row, config(), &work).unwrap();
    assert_eq!(externalized.values(), expected.0.values());
    drop(overflows);
    drop(work);
    drop(admission);
    assert_eq!(externalized.values(), expected.0.values());
    let minimum = 512 * 1024
        + std::mem::size_of_val(row.values())
        + std::mem::size_of::<Vec<RelationalValue>>()
        + 2 * std::mem::size_of::<std::sync::atomic::AtomicUsize>();
    assert!(
        governor.snapshot().admitted_memory_bytes >= minimum as u64,
        "externalized row lost ownership of its inline values and array"
    );
    drop(externalized);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
}

#[test]
fn externalized_overflow_array_keeps_its_own_lease_after_row_release() {
    let row = row();
    let governor = output_memory::governor();
    let admission = governor
        .try_admit_incremental_maintenance(0, 8 * 1024 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone());
    let (externalized, overflows) = externalize(&row, config(), &work).unwrap();
    assert_eq!(overflows.len(), 1);
    drop(externalized);
    drop(work);
    drop(admission);
    let minimum = row.values().len()
        * std::mem::size_of::<crate::relational::overflow::CheckpointRelationalOverflow>()
        + overflows[0].bytes.len();
    assert!(governor.snapshot().admitted_memory_bytes >= minimum as u64);
    let bytes = overflows[0].bytes.clone();
    drop(overflows);
    assert!(governor.snapshot().admitted_memory_bytes >= bytes.len() as u64);
    drop(bytes);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
}

#[test]
fn externalized_copies_cancel_each_unit_refund_and_retry_complete_values() {
    use hawdb_core::RuntimeCancellationReason;

    let row = row();
    let expected = externalize_row(&row, config()).unwrap();
    let governor = output_memory::governor();
    let mut cuts = vec![0];
    let mut index = 0;
    while index < cuts.len() {
        let cut = cuts[index];
        let admission = governor
            .try_admit_incremental_maintenance(0, 8 * 1024 * 1024, 1, RuntimeTaskContext::default())
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
        let result = work.classify(|work| externalize(&row, config(), work));
        if cut == 0 {
            let (externalized, overflows) = result.unwrap();
            assert_eq!(externalized.values(), expected.0.values());
            assert_eq!(overflows.len(), expected.1.len());
            for (actual, expected) in overflows.iter().zip(&expected.1) {
                assert_eq!(actual, expected);
            }
            if index == 0 {
                cuts.extend(1..=probe.completed.load(Ordering::SeqCst));
                cuts.push(0);
            }
            drop(externalized);
            drop(overflows);
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
                "externalized row cut {cut}: {result:?}"
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
        "verified {} externalized row CPU cuts and complete retry",
        cuts.len() - 2
    );
}

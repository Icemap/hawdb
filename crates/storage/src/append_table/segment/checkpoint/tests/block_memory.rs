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

fn input() -> Vec<AppendTableRow> {
    (0..128)
        .map(|id| AppendTableRow {
            table: "blocks".into(),
            partition_key: RelationalKey(vec![RelationalValue::BigInt(0)]),
            order_key: RelationalKey(vec![RelationalValue::BigInt(id)]),
            row: RelationalRow::new(vec![RelationalValue::Text("界x".repeat(1024))]),
        })
        .collect()
}

fn config() -> AppendSegmentConfig {
    AppendSegmentConfig {
        target_decoded_block_bytes: 1024 * 1024,
        overflow_threshold_bytes: 8 * 1024,
        ..AppendSegmentConfig::default()
    }
}

#[test]
fn block_growth_denies_before_unadmitted_accumulated_row_allocations() {
    let rows = input();
    let governor = compression_memory::governor();
    let admission = governor
        .try_admit_incremental_maintenance(0, 32 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone());
    let observation = crate::test_allocator::AllocationObservation::start();
    let result = work.classify(|work| encode_block(&rows, 0, rows.len(), config(), work));
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
    assert_eq!(allocations, 0, "block accumulated rows outside admission");
    assert!(
        denied,
        "32 KiB cannot admit a block containing 512 KiB of rows"
    );
}

#[test]
fn raw_block_retains_its_backing_memory_after_execution_closes() {
    let rows = input();
    let expected = encode_next_block(&rows, 0, rows.len(), config()).unwrap();
    let governor = compression_memory::governor();
    let admission = governor
        .try_admit_incremental_maintenance(0, 2 * 1024 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone());
    let (end, output) = encode_block(&rows, 0, rows.len(), config(), &work).unwrap();
    assert_eq!(end, rows.len());
    assert_eq!(end, expected.0);
    assert_eq!(&output[..], expected.1);
    let retained = admission.memory_report().live_accounted_bytes;
    drop(work);
    drop(admission);
    assert_eq!(governor.snapshot().admitted_memory_bytes, retained);
    assert!(
        retained >= output.len() as u64,
        "raw block detached its memory permit"
    );
    drop(output);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
}

fn descriptor() -> AppendSegmentBlockDescriptor {
    AppendSegmentBlockDescriptor {
        table: "界x".repeat(128 * 1024),
        partition_key: RelationalKey(vec![RelationalValue::BigInt(0)]),
        min_order_key: RelationalKey(vec![RelationalValue::BigInt(1)]),
        max_order_key: RelationalKey(vec![RelationalValue::BigInt(2)]),
        row_count: 2,
        payload_offset: 7,
        compressed_bytes: 23,
        decoded_bytes: 37,
        payload_crc32c: 41,
        payload_sha256: integrity_digest(b"directory").sha256,
    }
}

#[test]
fn directory_denies_large_strings_and_retains_admitted_bytes_after_task_close() {
    let descriptors = [descriptor()];
    let config = config();
    let expected = encode_directory(&descriptors, config).unwrap();
    let governor = compression_memory::governor();
    for budget in [32 * 1024, 2 * 1024 * 1024] {
        let admission = governor
            .try_admit_incremental_maintenance(0, budget, 1, RuntimeTaskContext::default())
            .unwrap();
        let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone());
        let observation = crate::test_allocator::AllocationObservation::start();
        let result = work.classify(|work| directory(&descriptors, config, work));
        let allocations = observation.finish();
        if budget == 32 * 1024 {
            assert!(matches!(
                result,
                Err(CheckpointOperationError::Work(CheckpointWorkError::Memory(
                    RuntimeMemoryError::ReservationExceeded { .. }
                )))
            ));
            drop(result);
            assert_eq!(allocations, 0);
            assert_eq!(admission.memory_report().live_accounted_bytes, 0);
            drop(work);
            drop(admission);
        } else {
            let output = result.unwrap();
            assert_eq!(&output[..], expected);
            let retained = admission.memory_report().live_accounted_bytes;
            drop(work);
            drop(admission);
            assert_eq!(governor.snapshot().admitted_memory_bytes, retained);
            assert!(retained >= output.len() as u64);
            drop(output);
        }
        assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    }
}

#[test]
fn block_growth_cancels_each_unit_refunds_and_retries_complete_bytes() {
    use hawdb_core::RuntimeCancellationReason;
    // Multiple growth steps and 64 KiB copies, with the original one-unit
    // scheduler. Discover every concrete unit rather than sampling cuts.
    let rows = input().into_iter().take(33).collect::<Vec<_>>();
    let expected = encode_next_block(&rows, 0, rows.len(), config()).unwrap();
    let governor = compression_memory::governor();
    let mut cuts = vec![0];
    let mut index = 0;
    while index < cuts.len() {
        let cut = cuts[index];
        let admission = governor
            .try_admit_incremental_maintenance(0, 2 * 1024 * 1024, 1, RuntimeTaskContext::default())
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
        let result = work.classify(|work| encode_block(&rows, 0, rows.len(), config(), work));
        if cut == 0 {
            let (end, output) = result.unwrap();
            assert_eq!(end, expected.0);
            assert_eq!(&output[..], expected.1);
            if index == 0 {
                cuts.extend(1..=probe.completed.load(Ordering::SeqCst));
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
                "block cut {cut}: {result:?}"
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
        "verified {} accumulated block CPU cuts and complete retry",
        cuts.len() - 2
    );
}

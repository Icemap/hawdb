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
use hawdb_core::RuntimeTaskContext;
use hawdb_qos::{
    IoConcurrencyBudget, RuntimeGovernor, RuntimeGovernorConfig, RuntimeMemorySnapshot,
    RuntimeResourceBudget, RuntimeResourceSnapshot,
};

pub(super) fn governor() -> RuntimeGovernor {
    let governor = RuntimeGovernor::new(
        RuntimeGovernorConfig {
            memory_budget_bytes: Some(16 * 1024 * 1024),
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

fn config() -> AppendSegmentConfig {
    AppendSegmentConfig {
        target_decoded_block_bytes: 16 * 1024,
        overflow_threshold_bytes: 8 * 1024,
        ..AppendSegmentConfig::default()
    }
}

#[test]
fn final_segment_bytes_keep_their_admission_until_the_last_shared_clone() {
    let mut seed = 0x510e_527f_ade6_82d1u64;
    let rows = (0..128)
        .map(|id| {
            let bytes = (0..4096)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    seed as u8
                })
                .collect();
            AppendTableRow {
                table: "output".into(),
                partition_key: RelationalKey(vec![]),
                order_key: RelationalKey(vec![RelationalValue::BigInt(id)]),
                row: RelationalRow::new(vec![RelationalValue::Bytea(bytes)]),
            }
        })
        .collect::<Vec<_>>();
    let expected = AppendSegmentWriter::encode(4, 19, &rows, config()).unwrap();
    let governor = governor();
    let admission = governor
        .try_admit_incremental_maintenance(0, 8 * 1024 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone());
    let output = encode(4, 19, &rows, config(), &work).unwrap();
    assert_eq!(output, expected);
    let bytes = output.encoded.clone();
    drop(output);
    drop(work);
    drop(admission);
    assert_eq!(&bytes[..], &expected.encoded[..]);
    assert!(
        governor.snapshot().admitted_memory_bytes >= bytes.len() as u64,
        "final segment bytes escaped their allocation admission"
    );
    let last = bytes.clone();
    drop(bytes);
    assert_eq!(&last[..], &expected.encoded[..]);
    assert!(governor.snapshot().admitted_memory_bytes >= last.len() as u64);
    drop(last);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
}

#[test]
fn retained_segment_descriptors_cover_their_strings_keys_and_container() {
    let rows = [AppendTableRow {
        table: "界x".repeat(128 * 1024),
        partition_key: RelationalKey(vec![RelationalValue::Text("p".repeat(512 * 1024))]),
        order_key: RelationalKey(vec![RelationalValue::BigInt(1)]),
        row: RelationalRow::new(vec![RelationalValue::BigInt(1)]),
    }];
    let expected = AppendSegmentWriter::encode(5, 23, &rows, config()).unwrap();
    let governor = governor();
    let admission = governor
        .try_admit_incremental_maintenance(0, 8 * 1024 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone());
    let output = encode(5, 23, &rows, config(), &work).unwrap();
    assert_eq!(output, expected);
    let minimum = output.encoded.len()
        + rows[0].table.len()
        + 512 * 1024
        + std::mem::size_of::<AppendSegmentBlockDescriptor>();
    drop(work);
    drop(admission);
    assert!(
        governor.snapshot().admitted_memory_bytes >= minimum as u64,
        "retained append descriptors escaped their memory inventory"
    );
    assert_eq!(output.descriptors, expected.descriptors);
    drop(output);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
}

#[test]
fn descriptor_fields_deny_wide_copies_before_allocation_and_refund() {
    use crate::background::{CheckpointOperationError, CheckpointWorkError};
    use hawdb_core::RuntimeMemoryError;

    let table = "界x".repeat(128 * 1024);
    let key = RelationalKey(vec![RelationalValue::Text("p".repeat(512 * 1024))]);
    let governor = governor();
    for copy_key in [false, true] {
        let admission = governor
            .try_admit_incremental_maintenance(0, 32 * 1024, 1, RuntimeTaskContext::default())
            .unwrap();
        let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone());
        let mut descriptors = CheckpointDescriptors::default();
        let observation = crate::test_allocator::AllocationObservation::start();
        let result = work.classify(|work| {
            if copy_key {
                descriptors.copy_key(&key, work).map(drop)
            } else {
                descriptors.copy_table(&table, work).map(drop)
            }
        });
        let allocations = observation.finish();
        assert!(matches!(
            result,
            Err(CheckpointOperationError::Work(CheckpointWorkError::Memory(
                RuntimeMemoryError::ReservationExceeded { .. }
            )))
        ));
        assert_eq!(allocations, 0, "wide descriptor copied before admission");
        drop(descriptors);
        assert_eq!(admission.memory_report().live_accounted_bytes, 0);
        drop(work);
        drop(admission);
        assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    }
}

#[test]
fn complete_output_cancels_each_unit_refunds_and_retries_all_bytes() {
    use crate::background::{CheckpointOperationError, CheckpointWorkError};
    use hawdb_core::{RuntimeCancellationReason, RuntimeMemoryError};

    // Five independently compressed partitions exercise descriptor growth,
    // accumulated payload growth and the final copy across a 64 KiB boundary.
    let mut seed = 0x1f83_d9ab_fb41_bd6bu64;
    let rows = (0..5)
        .map(|id| AppendTableRow {
            table: "complete".into(),
            partition_key: RelationalKey(vec![RelationalValue::BigInt(id)]),
            order_key: RelationalKey(vec![RelationalValue::BigInt(1)]),
            row: RelationalRow::new(vec![RelationalValue::Bytea(
                (0..15 * 1024)
                    .map(|_| {
                        seed ^= seed << 13;
                        seed ^= seed >> 7;
                        seed ^= seed << 17;
                        seed as u8
                    })
                    .collect(),
            )]),
        })
        .collect::<Vec<_>>();
    let config = AppendSegmentConfig {
        overflow_threshold_bytes: 32 * 1024,
        ..config()
    };
    let expected = AppendSegmentWriter::encode(7, 31, &rows, config).unwrap();
    assert!(expected.encoded.len() > 64 * 1024);
    assert_eq!(expected.descriptors.len(), rows.len());
    let governor = governor();
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
        let result = work.classify(|work| encode(7, 31, &rows, config, work));
        if cut == 0 {
            let output = result.unwrap();
            assert_eq!(output, expected);
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
                "complete output cut {cut}: {result:?}"
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
        "verified {} complete segment CPU cuts and retry",
        cuts.len() - 2
    );
}

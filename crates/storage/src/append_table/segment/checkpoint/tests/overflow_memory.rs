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

fn random() -> Vec<u8> {
    let mut seed = 0x5be0_cd19_137e_2179u64;
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
fn overflow_denies_large_envelopes_before_unadmitted_allocation() {
    let raw = random();
    let governor = output_memory::governor();
    let admission = governor
        .try_admit_incremental_maintenance(0, 32 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone());
    let observation = crate::test_allocator::AllocationObservation::start();
    let result = work.classify(|work| {
        crate::relational::overflow::encode_overflow_envelope_with_work_context(
            RelationalScalarType::Bytea,
            &raw,
            RelationalOverflowConfig::default(),
            work,
        )
        .map_err(map_relational)
    });
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
    assert_eq!(allocations, 0, "overflow allocated outside admission");
    assert!(denied, "32 KiB cannot admit a 512 KiB raw envelope");
}

#[test]
fn overflow_raw_and_compressed_envelopes_retain_the_last_shared_lease() {
    let governor = output_memory::governor();
    for (scalar, raw) in [
        (RelationalScalarType::Bytea, random()),
        (
            RelationalScalarType::Text,
            "界x".repeat(128 * 1024).into_bytes(),
        ),
    ] {
        let config = RelationalOverflowConfig::default();
        let expected = encode_overflow_envelope(scalar, &raw, config).unwrap();
        let admission = governor
            .try_admit_incremental_maintenance(0, 8 * 1024 * 1024, 1, RuntimeTaskContext::default())
            .unwrap();
        let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone());
        let output = crate::relational::overflow::encode_overflow_envelope_with_work_context(
            scalar, &raw, config, &work,
        )
        .unwrap();
        assert_eq!(output, expected);
        let bytes = output.bytes.clone();
        drop(output);
        drop(work);
        drop(admission);
        assert_eq!(&bytes[..], &expected.bytes[..]);
        assert!(
            governor.snapshot().admitted_memory_bytes >= bytes.len() as u64,
            "overflow envelope detached its allocation lease"
        );
        let last = bytes.clone();
        drop(bytes);
        assert!(governor.snapshot().admitted_memory_bytes >= last.len() as u64);
        drop(last);
        assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    }
}

#[test]
fn admitted_overflow_cancels_every_unit_refunds_and_retries_raw_and_zstd() {
    use hawdb_core::RuntimeCancellationReason;

    let governor = output_memory::governor();
    for (scalar, raw) in [
        (RelationalScalarType::Bytea, random()),
        (
            RelationalScalarType::Text,
            "界x".repeat(128 * 1024).into_bytes(),
        ),
    ] {
        let config = RelationalOverflowConfig::default();
        let expected = encode_overflow_envelope(scalar, &raw, config).unwrap();
        let mut cuts = vec![0];
        let mut index = 0;
        while index < cuts.len() {
            let cut = cuts[index];
            let admission = governor
                .try_admit_incremental_maintenance(
                    0,
                    8 * 1024 * 1024,
                    1,
                    RuntimeTaskContext::default(),
                )
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
            let result = work.classify(|work| {
                crate::relational::overflow::encode_overflow_envelope_with_work_context(
                    scalar, &raw, config, work,
                )
                .map_err(map_relational)
            });
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
                    "overflow {scalar:?} cut {cut}: {result:?}"
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
            "verified {} admitted {scalar:?} overflow CPU cuts and retry",
            cuts.len() - 2
        );
    }
}

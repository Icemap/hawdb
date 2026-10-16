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

#[test]
fn append_key_denies_before_encoding_a_wide_partition() {
    let partition = RelationalKey(vec![RelationalValue::Text("p".repeat(512 * 1024))]);
    let governor = compression_memory::governor();
    let admission = governor
        .try_admit_incremental_maintenance(0, 32 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone());
    let observation = crate::test_allocator::AllocationObservation::start();
    let result = work.classify(|work| {
        key(
            &partition,
            AppendSegmentConfig::default().max_key_bytes,
            work,
        )
    });
    let allocations = observation.finish();
    let denied = matches!(result,
        Err(CheckpointOperationError::Work(CheckpointWorkError::Memory(
            RuntimeMemoryError::ReservationExceeded { requested_bytes, available_bytes }
        ))) if requested_bytes > 512 * 1024 && available_bytes == 32 * 1024);
    drop(result);
    assert_eq!(admission.memory_report().live_accounted_bytes, 0);
    drop(work);
    drop(admission);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    assert_eq!(allocations, 0, "wide key allocated before admission");
    assert!(denied, "32 KiB cannot admit a 512 KiB partition key");
}

#[test]
fn append_key_owns_one_final_buffer_until_output_release() {
    let partition = RelationalKey(vec![RelationalValue::Text("p".repeat(512 * 1024))]);
    let expected =
        encode_append_key(&partition, AppendSegmentConfig::default().max_key_bytes).unwrap();
    let governor = compression_memory::governor();
    let admission = governor
        .try_admit_incremental_maintenance(0, 2 * 1024 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone());
    let observation = crate::test_allocator::AllocationObservation::start();
    let output = key(
        &partition,
        AppendSegmentConfig::default().max_key_bytes,
        &work,
    )
    .unwrap();
    let allocations = observation.finish();
    assert_eq!(&output[..], expected);
    let retained = admission.memory_report().live_accounted_bytes;
    drop(work);
    drop(admission);
    assert_eq!(governor.snapshot().admitted_memory_bytes, retained);
    assert!(
        retained >= output.len() as u64,
        "encoded key detached its memory permit"
    );
    assert_eq!(
        allocations, 1,
        "append tag and ordered key need one final allocation"
    );
    drop(output);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
}

#[test]
fn admitted_keys_match_generated_values_and_escaped_chunk_edges() {
    let governor = compression_memory::governor();
    let compare = |source: RelationalKey| {
        let expected = encode_append_key(&source, 4 * 1024 * 1024).unwrap();
        let admission = governor
            .try_admit_incremental_maintenance(0, 2 * 1024 * 1024, 1, RuntimeTaskContext::default())
            .unwrap();
        let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone());
        let actual = key(&source, 4 * 1024 * 1024, &work).unwrap();
        assert_eq!(&actual[..], expected);
        assert!(admission.memory_report().live_accounted_bytes >= actual.len() as u64);
        drop(actual);
        assert_eq!(admission.memory_report().live_accounted_bytes, 0);
        drop(work);
        drop(admission);
        assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    };
    for seed in 0..64u64 {
        compare(RelationalKey(vec![
            RelationalValue::Null,
            RelationalValue::Boolean(seed.is_multiple_of(2)),
            RelationalValue::BigInt(-(seed as i64)),
            RelationalValue::DoublePrecision(f64::from_bits(seed.rotate_left(47))),
            RelationalValue::Text(format!("界\0🙂{seed}")),
            RelationalValue::Bytea(vec![0, seed as u8, 255, 0]),
            RelationalValue::Uuid(hawdb_core::Uuid::from_u128(seed as u128)),
        ]));
    }
    for length in [0, 1, 65535, 65536, 65537, 131073] {
        compare(RelationalKey(vec![
            RelationalValue::Text("界\0🙂".repeat(length / 8)),
            RelationalValue::Bytea(vec![0; length]),
        ]));
    }
}

#[test]
fn key_encoding_cancels_each_count_allocation_and_write_unit_then_retries() {
    use hawdb_core::RuntimeCancellationReason;
    let source = RelationalKey(vec![RelationalValue::Text("界\0🙂".repeat(65536))]);
    let expected = encode_append_key(&source, 4 * 1024 * 1024).unwrap();
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
        let result = work.classify(|work| key(&source, 4 * 1024 * 1024, work));
        if cut == 0 {
            let output = result.unwrap();
            assert_eq!(&output[..], expected);
            if index == 0 {
                let units = probe.completed.load(Ordering::SeqCst);
                assert!(units > 24);
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
                "key cut {cut}: {result:?}"
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
        "verified {} key count/allocation/write CPU cancellation cuts and complete retry",
        cuts.len() - 2
    );
}

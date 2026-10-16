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
use hawdb_core::{
    RuntimeCancellationReason, RuntimeIoWaveError, RuntimeMemoryError, RuntimeTaskContext, Uuid,
};

fn fixture() -> (
    Directory,
    AppendSegmentArtifactMetadata,
    AppendSegmentConfig,
    Vec<u8>,
) {
    let directory = Directory::new();
    let config = AppendSegmentConfig::default();
    let rows = vec![AppendTableRow {
        table: "events".into(),
        partition_key: RelationalKey(vec![RelationalValue::Text("p".repeat(512 * 1024))]),
        order_key: RelationalKey(vec![RelationalValue::BigInt(1)]),
        row: RelationalRow::new(vec![RelationalValue::BigInt(1)]),
    }];
    let output = AppendSegmentWriter::encode(1, 1, &rows, config).unwrap();
    let bytes = output.encoded.to_vec();
    fs::write(directory.0.join(append_segment_file(1)), &bytes).unwrap();
    (directory, output.artifact, config, bytes)
}

#[test]
fn segment_reader_denies_wide_partition_before_directory_or_key_allocation() {
    let (directory, artifact, config, reference) = fixture();
    let path = directory.0.join(append_segment_file(1));
    let governor = manifest_governor();
    let admission = governor
        .try_admit_incremental_maintenance(0, 32 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let local = scheduler();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone())
        .with_scheduler(local.clone());
    let observation = crate::test_allocator::AllocationObservation::start();
    let result = work.classify(|work| {
        AppendSegmentReader::open_bound_file_with_work_context(&path, artifact, config, work)
    });
    let allocations = observation.finish();
    let diagnostic = format!("{result:?}");
    let denied = matches!(
        result,
        Err(CheckpointOperationError::Work(CheckpointWorkError::Memory(
            RuntimeMemoryError::ReservationExceeded {
                requested_bytes,
                available_bytes,
            }
        ))) if requested_bytes > 512 * 1024 && available_bytes == 32 * 1024
    );
    drop(result);
    assert_eq!(admission.memory_report().live_accounted_bytes, 0);
    drop(work);
    drop(admission);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    assert_eq!(local.state().running_background_operations, 0);
    assert_eq!(fs::read(path).unwrap(), reference);
    assert_eq!(
        allocations, 0,
        "large segment directory/key allocations happened before admission"
    );
    assert!(
        denied,
        "32 KiB cannot admit a 512 KiB partition descriptor: {diagnostic}"
    );
}

#[test]
fn segment_reader_retains_descriptor_inventory_through_last_clone() {
    let (directory, artifact, config, reference) = fixture();
    let path = directory.0.join(append_segment_file(1));
    let governor = manifest_governor();
    let admission = governor
        .try_admit_incremental_maintenance(0, 2 * 1024 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let local = scheduler();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone())
        .with_scheduler(local.clone());
    let reader =
        AppendSegmentReader::open_bound_file_with_work_context(&path, artifact, config, &work)
            .unwrap();
    assert_eq!(reader.descriptors().len(), 1);
    assert_eq!(
        reader.descriptors()[0].partition_key,
        RelationalKey(vec![RelationalValue::Text("p".repeat(512 * 1024))])
    );
    let retained = admission.memory_report().live_accounted_bytes;
    let clone = reader.clone();
    drop(reader);
    drop(work);
    drop(admission);
    assert_eq!(governor.snapshot().admitted_memory_bytes, retained);
    drop(clone);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    assert_eq!(local.state().running_background_operations, 0);
    assert_eq!(fs::read(path).unwrap(), reference);
    assert!(
        retained >= 512 * 1024,
        "retained descriptor allocations must keep their admission"
    );
}

#[test]
fn owned_ordered_keys_match_generated_values_corruption_and_chunk_boundaries() {
    use crate::background::CheckpointDecodeContext;
    use crate::relational::{
        decode_relational_primary_key, decode_relational_primary_key_owned,
        encode_relational_primary_key,
    };
    let governor = manifest_governor();
    let compare = |encoded: &[u8]| {
        let expected = decode_relational_primary_key(encoded);
        let admission = governor
            .try_admit_incremental_maintenance(0, 2 * 1024 * 1024, 1, RuntimeTaskContext::default())
            .unwrap();
        let owned = CheckpointDecodeContext {
            work: CheckpointWorkContext::new(admission.task_context().unwrap().clone()),
            memory: std::cell::RefCell::default(),
        };
        let actual = decode_relational_primary_key_owned(encoded, &owned);
        assert_eq!(actual, expected, "{} encoded bytes", encoded.len());
        drop(actual);
        drop(owned);
        assert_eq!(admission.memory_report().live_accounted_bytes, 0);
        drop(admission);
        assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    };
    for seed in 0..64u64 {
        let key = RelationalKey(vec![
            RelationalValue::Null,
            RelationalValue::Boolean(seed.is_multiple_of(2)),
            RelationalValue::BigInt(-(seed as i64)),
            RelationalValue::DoublePrecision(seed as f64 / 7.0),
            RelationalValue::Text(format!("界\0🙂{seed}")),
            RelationalValue::Bytea(vec![0, seed as u8, 255, 0]),
            RelationalValue::Uuid(Uuid::from_u128(seed as u128)),
        ]);
        let encoded = encode_relational_primary_key(&key).unwrap();
        compare(&encoded);
        for end in 0..encoded.len() {
            compare(&encoded[..end]);
            let mut corrupt = encoded.clone();
            corrupt[end] ^= 0x80;
            compare(&corrupt);
        }
    }
    let wide = encode_relational_primary_key(&RelationalKey(vec![
        RelationalValue::Text("界\0🙂".repeat(70_000)),
        RelationalValue::Bytea(vec![0; 131_073]),
    ]))
    .unwrap();
    compare(&wide);
    for end in [
        0,
        1,
        65535,
        65536,
        65537,
        wide.len() - 3,
        wide.len() - 2,
        wide.len() - 1,
    ] {
        compare(&wide[..end]);
    }
}

#[test]
fn segment_reader_cancels_at_every_actual_unit_and_wave_then_retries_complete_keys() {
    let (directory, artifact, config, reference) = fixture();
    let path = directory.0.join(append_segment_file(1));
    let governor = manifest_governor();
    let mut cuts = vec![(0, 0)];
    let mut total = (0, 0);
    let mut index = 0;
    while index < cuts.len() {
        let (cpu, io) = cuts[index];
        let admission = governor
            .try_admit_incremental_maintenance(0, 2 * 1024 * 1024, 1, RuntimeTaskContext::default())
            .unwrap();
        let local = scheduler();
        let task = admission.task_context().unwrap().clone();
        let probe = Arc::new(CheckpointWorkProbe {
            cancellation: task.cancellation().clone(),
            ..CheckpointWorkProbe::default()
        });
        local.set_telemetry_sink(Some(probe.clone()));
        probe.cancel_after.store(cpu, Ordering::SeqCst);
        probe.cancel_on_io_wave.store(io, Ordering::SeqCst);
        let work = CheckpointWorkContext::new(task.with_io_wave_controller(probe.clone()))
            .with_scheduler(local.clone());
        let result = work.classify(|work| {
            AppendSegmentReader::open_bound_file_with_work_context(&path, artifact, config, work)
        });
        if cpu == 0 && io == 0 {
            let reader = result.unwrap();
            assert_eq!(reader.descriptors().len(), 1);
            assert_eq!(
                reader.descriptors()[0].partition_key,
                RelationalKey(vec![RelationalValue::Text("p".repeat(512 * 1024))])
            );
            if index == 0 {
                total = (
                    probe.completed.load(Ordering::SeqCst),
                    probe.io_waves.load(Ordering::SeqCst),
                );
                assert!(total.0 > 64 && total.1 > 32);
                cuts.extend((1..=total.0).map(|cpu| (cpu, 0)));
                cuts.extend((1..=total.1).map(|io| (0, io)));
                cuts.push((0, 0));
            }
            drop(reader);
        } else {
            assert!(
                matches!(
                    result,
                    Err(CheckpointOperationError::Work(
                        CheckpointWorkError::Stopped(RuntimeCancellationReason::Cancelled)
                            | CheckpointWorkError::Memory(RuntimeMemoryError::Stopped(
                                RuntimeCancellationReason::Cancelled
                            ))
                            | CheckpointWorkError::Io(RuntimeIoWaveError::Stopped(
                                RuntimeCancellationReason::Cancelled
                            ))
                    ))
                ),
                "CPU/IO cut ({cpu},{io}) of {total:?}: {result:?}"
            );
            if cpu > 0 {
                assert_eq!(probe.completed.load(Ordering::SeqCst), cpu);
            }
            if io > 0 {
                assert_eq!(probe.io_waves.load(Ordering::SeqCst), io);
            }
        }
        assert_eq!(admission.memory_report().live_accounted_bytes, 0);
        probe.assert_released(&local);
        drop(work);
        drop(admission);
        assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
        assert_eq!(fs::read(&path).unwrap(), reference);
        index += 1;
    }
    println!(
        "verified {} segment CPU cuts and {} IO cuts, then complete retry",
        total.0, total.1
    );
}

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
    RuntimeCancellationReason, RuntimeIoWaveError, RuntimeMemoryError, RuntimeTaskContext,
};

#[test]
fn decoded_bytea_owns_one_value_allocation_after_input_and_task_close() {
    use crate::relational::RelationalColumnDefault;
    let mut manifest = wide_manifest();
    let column = &mut manifest.schemas.get_mut("events").unwrap().columns[1];
    column.name = "payload".into();
    column.scalar_type = RelationalScalarType::Bytea;
    column.default = Some(RelationalColumnDefault::Literal(RelationalValue::Bytea(
        vec![173; 512 * 1024],
    )));
    let directory = Directory::new();
    let config = AppendPublicationConfig::default();
    let report = AppendPublisher::publish_candidate(
        &directory.0,
        1,
        1,
        None,
        &manifest.schemas,
        &[],
        config,
    )
    .unwrap();
    let governor = manifest_governor();
    let admission = governor
        .try_admit_incremental_maintenance(0, 2 * 1024 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let local = scheduler();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone())
        .with_scheduler(local.clone());
    let observation = crate::test_allocator::AllocationObservation::start();
    let reader = AppendGenerationReader::open_bound_with_work_context(
        &directory.0,
        report.generation_artifacts,
        config,
        &work,
    )
    .unwrap();
    assert_eq!(
        observation.finish(),
        2,
        "one input buffer and one decoded Bytea value"
    );
    assert_eq!(reader.manifest().schemas, manifest.schemas);
    let memory = admission.memory_report();
    assert!(memory.live_accounted_bytes >= 512 * 1024);
    assert!(memory.peak_accounted_bytes > 1024 * 1024);
    assert!(memory.peak_accounted_bytes <= 2 * 1024 * 1024);
    let clone = reader.clone();
    drop(reader);
    drop(work);
    drop(admission);
    assert_eq!(
        governor.snapshot().admitted_memory_bytes,
        memory.live_accounted_bytes
    );
    assert_eq!(clone.manifest().schemas, manifest.schemas);
    drop(clone);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    assert_eq!(local.state().running_background_operations, 0);
}

#[test]
fn every_reader_cpu_and_io_cancellation_refunds_private_schema_and_retries() {
    let manifest = wide_manifest();
    let directory = Directory::new();
    let config = AppendPublicationConfig::default();
    let report = AppendPublisher::publish_candidate(
        &directory.0,
        1,
        1,
        None,
        &manifest.schemas,
        &[],
        config,
    )
    .unwrap();
    let path = directory.0.join(append_generation_manifest_file(1));
    let reference = fs::read(&path).unwrap();
    let governor = manifest_governor();
    let mut total = (0, 0);
    let mut cuts = vec![(0, 0)];
    // The initial success discovers the production path's actual boundaries.
    // Every later execution uses a fresh task/controller with unchanged limits.
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
            AppendGenerationReader::open_bound_with_work_context(
                &directory.0,
                report.generation_artifacts,
                config,
                work,
            )
        });
        if index == 0 {
            let reader = result.unwrap();
            assert_eq!(reader.manifest().schemas, manifest.schemas);
            total = (
                probe.completed.load(Ordering::SeqCst),
                probe.io_waves.load(Ordering::SeqCst),
            );
            assert!(total.0 > 64 && total.1 > 8);
            cuts.extend((1..=total.0).map(|cpu| (cpu, 0)));
            cuts.extend((1..=total.1).map(|io| (0, io)));
            cuts.push((0, 0));
            drop(reader);
        } else if cpu == 0 && io == 0 {
            let reader = result.unwrap();
            assert_eq!(reader.manifest().schemas, manifest.schemas);
            assert_eq!(
                admission.memory_report().live_accounted_bytes,
                governor.snapshot().admitted_memory_bytes
            );
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
        "verified {} reader CPU cuts and {} IO cuts, then complete retry",
        total.0, total.1
    );
}

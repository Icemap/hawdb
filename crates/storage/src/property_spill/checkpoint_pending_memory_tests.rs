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
use crate::background::{CheckpointBytes, CheckpointWorkContext, CheckpointWorkProbe};
use hawdb_core::{RuntimeMemoryError, RuntimeTaskContext};
use hawdb_qos::{
    IoConcurrencyBudget, LocalQosPolicy, LocalQosScheduler, RuntimeGovernor, RuntimeGovernorConfig,
    RuntimeMemorySnapshot, RuntimeResourceBudget, RuntimeResourceSnapshot, RuntimeWorkRequest,
};
use std::num::NonZeroUsize;
use std::sync::atomic::Ordering;

fn governor(ceiling: u64) -> RuntimeGovernor {
    RuntimeGovernor::new(
        RuntimeGovernorConfig {
            memory_budget_bytes: Some(ceiling),
            background_task_limit: Some(NonZeroUsize::MIN),
            ..RuntimeGovernorConfig::shared_host()
        },
        RuntimeResourceSnapshot::from_parts(
            RuntimeResourceBudget::from_limits(NonZeroUsize::MIN, None, None),
            RuntimeMemorySnapshot::from_limits(Some(1 << 30), Some(1 << 30), None, None, None),
        ),
        IoConcurrencyBudget::new(2, 1),
    )
}

fn local() -> LocalQosScheduler {
    LocalQosScheduler::new(LocalQosPolicy {
        max_background_operations: Some(1),
        max_total_background_operations: Some(1),
        ..Default::default()
    })
}

fn used(task: &RuntimeTaskContext, ceiling: u64) -> u64 {
    match task.reserve_working_memory(ceiling) {
        Err(RuntimeMemoryError::ReservationExceeded {
            available_bytes, ..
        }) => ceiling - available_bytes,
        other => panic!("full-ceiling concrete lease probe failed: {other:?}"),
    }
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "hawdb-checkpoint-pending-memory-{}",
            hawdb_core::generate_uuidv7().unwrap()
        ));
        std::fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn writer(&self, name: &str, work: CheckpointWorkContext) -> PropertySpillWriter {
        PropertySpillWriter::create_with_work_context(
            self.0.join(format!("{name}.hawdb")),
            ManifestGeneration(47),
            47,
            PropertySpillConfig::default(),
            PersistentPropertySpillDescriptorTree::new(
                GraphDescriptorTreePaths::new(
                    self.0.join(format!("{name}.pages.hawdb")),
                    self.0.join(format!("{name}.root.hawdb")),
                ),
                GraphDescriptorTreeBuildConfig::default(),
            ),
            work,
        )
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn push(writer: &mut PropertySpillWriter, bytes: &[u8]) -> Result<u64, PropertySpillError> {
    let mut value = CheckpointBytes::new(bytes.len(), &writer.work)?;
    value.append(bytes, &writer.work)?;
    writer.push_checkpoint(value)
}

#[test]
fn checkpoint_units_canonical_accumulator_memory_related_pending_capacity_survives_flush_and_reuse()
{
    let fixture = Fixture::new();
    let ceiling = 1024 * 1024;
    let governor = governor(ceiling);
    let permit = governor
        .try_admit(RuntimeWorkRequest::background_maintenance(ceiling).with_io_wave_slots(1))
        .unwrap();
    let task = permit.bind_task_context(RuntimeTaskContext::default());
    let plain = CheckpointWorkContext::new(task.clone());
    let local = local();
    let probe = Arc::new(CheckpointWorkProbe::default());
    local.set_telemetry_sink(Some(probe.clone()));
    let work = plain.clone().with_scheduler(local.clone());
    // Constructor/descriptor flush still have their existing units. This
    // fixture qualifies pending capacity, using one-unit admission for pushes.
    let mut writer = fixture.writer("admitted", plain.clone());
    let mut ordinary = fixture.writer("ordinary", CheckpointWorkContext::default());
    writer.work = work.clone();
    for id in 0..33 {
        let bytes = [id as u8; 9];
        assert_eq!(push(&mut writer, &bytes).unwrap(), id);
        assert_eq!(ordinary.push(bytes.to_vec()).unwrap(), id);
    }
    let capacity = writer.pending.capacity();
    let live = used(&task, ceiling);
    assert!(live >= (capacity * std::mem::size_of::<(u64, PropertySpillValue)>()) as u64);
    assert_eq!(probe.peak_units.load(Ordering::SeqCst), 1);
    probe.assert_released(&local);
    writer.work = plain.clone();
    writer.flush_block().unwrap();
    ordinary.flush_block().unwrap();
    assert!(writer.pending.is_empty());
    assert_eq!(writer.pending.capacity(), capacity);
    let retained = used(&task, ceiling);
    assert!(retained >= (capacity * std::mem::size_of::<(u64, PropertySpillValue)>()) as u64);
    assert!(retained < live);
    writer.work = work.clone();
    for id in 33..66 {
        let bytes = [id as u8; 9];
        assert_eq!(push(&mut writer, &bytes).unwrap(), id);
        assert_eq!(ordinary.push(bytes.to_vec()).unwrap(), id);
        assert_eq!(writer.pending.capacity(), capacity);
    }
    assert_eq!(used(&task, ceiling), live);
    writer.work = plain.clone();
    let prepared = writer.finish().unwrap();
    let expected = ordinary.finish().unwrap();
    assert_eq!(used(&task, ceiling), 0);
    assert_eq!(
        std::fs::read(&prepared.temporary_artifact_path).unwrap(),
        std::fs::read(&expected.temporary_artifact_path).unwrap()
    );
    assert_eq!(prepared.value_count, 66);
    assert_eq!(prepared.block_count, 2);
    assert_eq!(prepared.artifact_digest, expected.artifact_digest);
    assert_eq!(prepared.artifact_sha256, expected.artifact_sha256);
    drop(prepared);
    drop(expected);
    drop(work);
    drop(plain);
    drop(task);
    drop(permit);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    assert_eq!(governor.snapshot().active_background_io_slots, 0);
}

#[test]
fn checkpoint_units_canonical_accumulator_memory_related_pending_every_cpu_cut_releases_and_retries(
) {
    let fixture = Fixture::new();
    let ceiling = 1024 * 1024;
    let governor = governor(ceiling);
    let permit = governor
        .try_admit(RuntimeWorkRequest::background_maintenance(ceiling).with_io_wave_slots(1))
        .unwrap();
    let task = permit.bind_task_context(RuntimeTaskContext::default());
    let plain = CheckpointWorkContext::new(task.clone());
    let local = local();
    let work = plain.clone().with_scheduler(local.clone());
    let payload = [0x9f; 513];
    let fill = |writer: &mut PropertySpillWriter| -> Result<(), PropertySpillError> {
        for id in 0..33 {
            assert_eq!(push(writer, &payload)?, id);
        }
        Ok(())
    };
    let mut writer = fixture.writer("baseline", plain.clone());
    let baseline = Arc::new(CheckpointWorkProbe::default());
    local.set_telemetry_sink(Some(baseline.clone()));
    writer.work = work.clone();
    fill(&mut writer).unwrap();
    let total = baseline.completed.load(Ordering::SeqCst);
    assert!(total > 64);
    assert_eq!(baseline.peak_units.load(Ordering::SeqCst), 1);
    baseline.assert_released(&local);
    drop(writer);
    assert_eq!(used(&task, ceiling), 0);
    for cut in 1..=total {
        let probe = Arc::new(CheckpointWorkProbe::default());
        probe.cancel_after.store(cut, Ordering::SeqCst);
        let mut writer = fixture.writer(&format!("cut-{cut}"), plain.clone());
        local.set_telemetry_sink(Some(probe.clone()));
        let stopped = CheckpointWorkContext::new(permit.bind_task_context(
            RuntimeTaskContext::without_deadline(probe.cancellation.clone()),
        ))
        .with_scheduler(local.clone());
        assert!(matches!(
            stopped.classify(|scoped| {
                writer.work = scoped.clone();
                fill(&mut writer)
            }),
            Err(crate::background::CheckpointOperationError::Work(
                crate::background::CheckpointWorkError::Stopped(_)
            ))
        ));
        assert_eq!(probe.completed.load(Ordering::SeqCst), cut);
        assert_eq!(probe.peak_units.load(Ordering::SeqCst), 1);
        probe.assert_released(&local);
        drop(writer);
        drop(stopped);
        assert_eq!(used(&task, ceiling), 0);
        local.set_telemetry_sink(None);
        let mut retry = fixture.writer(&format!("retry-{cut}"), plain.clone());
        retry.work = work.clone();
        fill(&mut retry).unwrap();
        assert_eq!(retry.pending.len(), 33);
        for (id, (spill_id, value)) in retry.pending.iter().enumerate() {
            assert_eq!(*spill_id, id as u64);
            assert_eq!(&**value, payload);
        }
        drop(retry);
        assert_eq!(used(&task, ceiling), 0);
    }
    drop(work);
    drop(plain);
    drop(task);
    drop(permit);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    assert_eq!(governor.snapshot().active_background_io_slots, 0);
}

#[test]
fn retained_spill_finish_preserves_written_block_and_tree_with_two_units() {
    let fixture = Fixture::new();
    let bytes = vec![0x97; 512 * 1024];
    let mut ordinary = fixture.writer("ordinary", CheckpointWorkContext::default());
    assert_eq!(push(&mut ordinary, &bytes).unwrap(), 0);
    let expected = ordinary
        .finish()
        .unwrap()
        .publish(&fixture.0.join("ordinary-final.hawdb"))
        .unwrap();
    let ceiling = 4 * 1024 * 1024;
    let governor = governor(ceiling);
    let permit = governor
        .try_admit(RuntimeWorkRequest::background_maintenance(ceiling).with_io_wave_slots(1))
        .unwrap();
    let task = permit.bind_task_context(RuntimeTaskContext::default());
    let local = local();
    let work = CheckpointWorkContext::new(task.clone()).with_scheduler(local.clone());
    let mut writer = fixture.writer("retained", work.clone());
    assert_eq!(push(&mut writer, &bytes).unwrap(), 0);
    let mut completed = None;
    let mut previous_bytes = fs::metadata(writer.path()).unwrap().len();
    let mut attempts = 0;
    for attempt in 1..=128 {
        attempts = attempt;
        let probe = Arc::new(CheckpointWorkProbe::default());
        probe.cancel_after.store(2, Ordering::SeqCst);
        local.set_telemetry_sink(Some(probe.clone()));
        let fresh = CheckpointWorkContext::new(permit.bind_task_context(
            RuntimeTaskContext::without_deadline(probe.cancellation.clone()),
        ))
        .with_scheduler(local.clone());
        let result = writer.finish_with_work_context(&fresh);
        probe.assert_released(&local);
        assert!(probe.completed.load(Ordering::SeqCst) <= 2);
        let current_bytes = fs::metadata(writer.path()).unwrap().len();
        assert!(current_bytes >= previous_bytes);
        previous_bytes = current_bytes;
        match result {
            Ok(prepared) => {
                completed = Some(prepared);
                break;
            }
            Err(
                PropertySpillError::Work(crate::background::CheckpointWorkError::Stopped(_))
                | PropertySpillError::DescriptorTree(GraphDescriptorTreeError::Work(
                    crate::background::CheckpointWorkError::Stopped(_),
                )),
            ) => {}
            other => panic!("attempt={attempt}: {other:?}"),
        }
    }
    assert!(attempts >= 3);
    let mut prepared = completed.expect("fixed two-unit spill finish must complete");
    assert_eq!(prepared.value_count, 1);
    assert_eq!(prepared.block_count, 1);
    local.set_telemetry_sink(None);
    assert!(writer.push(vec![1]).is_err());
    assert!(writer.finish_with_work_context(&work).is_err());
    prepared.work = work.clone();
    let result = prepared
        .publish(&fixture.0.join("retained-final.hawdb"))
        .unwrap();
    drop(writer);
    assert_eq!(
        result.manifest.encode().unwrap(),
        expected.manifest.encode().unwrap()
    );
    for (left, right) in [
        ("retained-final.hawdb", "ordinary-final.hawdb"),
        ("retained.pages.hawdb", "ordinary.pages.hawdb"),
        ("retained.root.hawdb", "ordinary.root.hawdb"),
    ] {
        assert_eq!(
            fs::read(fixture.0.join(left)).unwrap(),
            fs::read(fixture.0.join(right)).unwrap()
        );
    }
    assert_eq!(used(&task, ceiling), 0);
    drop(work);
    drop(task);
    drop(permit);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
}

#[test]
fn retained_spill_push_retries_every_growth_cut_without_losing_values_or_ids() {
    let fixture = Fixture::new();
    let ceiling = 4 * 1024 * 1024;
    let governor = governor(ceiling);
    let permit = governor
        .try_admit(RuntimeWorkRequest::background_maintenance(ceiling).with_io_wave_slots(1))
        .unwrap();
    let task = permit.bind_task_context(RuntimeTaskContext::default());
    let local = local();
    let work = CheckpointWorkContext::new(task.clone()).with_scheduler(local.clone());
    let capture = |name: &str| {
        let mut writer = fixture.writer(name, work.clone());
        for id in 0..32 {
            assert_eq!(push(&mut writer, &[id as u8; 64]).unwrap(), id);
        }
        writer
    };
    let mut baseline_writer = capture("push-baseline");
    let baseline = Arc::new(CheckpointWorkProbe::default());
    local.set_telemetry_sink(Some(baseline.clone()));
    assert_eq!(push(&mut baseline_writer, &[32; 64]).unwrap(), 32);
    let units = baseline.completed.load(Ordering::SeqCst);
    baseline.assert_released(&local);
    drop(baseline_writer);
    local.set_telemetry_sink(None);
    for cut in 1..=units {
        let mut writer = capture(&format!("push-cut-{cut}"));
        let probe = Arc::new(CheckpointWorkProbe::default());
        probe.cancel_after.store(cut, Ordering::SeqCst);
        local.set_telemetry_sink(Some(probe.clone()));
        let stopped = CheckpointWorkContext::new(permit.bind_task_context(
            RuntimeTaskContext::without_deadline(probe.cancellation.clone()),
        ))
        .with_scheduler(local.clone());
        writer.bind_checkpoint_attempt(&stopped);
        assert!(matches!(
            push(&mut writer, &[32; 64]),
            Err(PropertySpillError::Work(
                crate::background::CheckpointWorkError::Stopped(_)
            ))
        ));
        probe.assert_released(&local);
        local.set_telemetry_sink(None);
        writer.bind_checkpoint_attempt(&work);
        if writer.has_pending_checkpoint_push() {
            assert_eq!(writer.resume_checkpoint_push(&work).unwrap(), 32);
        } else if writer.next_checkpoint_spill_id() == 32 {
            assert_eq!(push(&mut writer, &[32; 64]).unwrap(), 32);
        }
        assert_eq!(writer.next_checkpoint_spill_id(), 33);
        assert_eq!(writer.pending.len(), 33);
        for (id, (actual, value)) in writer.pending.iter().enumerate() {
            assert_eq!(*actual, id as u64, "cut={cut}");
            assert_eq!(&**value, &[id as u8; 64], "cut={cut}");
        }
        assert_eq!(push(&mut writer, &[33; 64]).unwrap(), 33);
        drop(writer);
        drop(stopped);
        assert_eq!(used(&task, ceiling), 0);
    }
    drop(work);
    drop(task);
    drop(permit);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
}

#[test]
fn retained_spill_publication_resumes_after_data_rename_without_replacing_it() {
    let fixture = Fixture::new();
    let bytes = vec![0x53; 8192];
    let mut ordinary = fixture.writer("publication-ordinary", CheckpointWorkContext::default());
    ordinary.push(bytes.clone()).unwrap();
    let expected = ordinary
        .finish()
        .unwrap()
        .publish(&fixture.0.join("publication-ordinary-final.hawdb"))
        .unwrap();
    for cut in 1..=2 {
        let name = format!("publication-cut-{cut}");
        let mut writer = fixture.writer(&name, CheckpointWorkContext::default());
        writer.push(bytes.clone()).unwrap();
        let mut prepared = writer.finish().unwrap();
        let destination = fixture.0.join(format!("{name}-final.hawdb"));
        let scheduler = local();
        let probe = Arc::new(CheckpointWorkProbe::default());
        probe.cancel_on_io_wave.store(cut, Ordering::SeqCst);
        let stopped = probe.context(scheduler.clone());
        assert!(matches!(
            prepared.publish_with_work_context(&destination, &stopped),
            Err(PropertySpillError::Work(
                crate::background::CheckpointWorkError::Io(
                    hawdb_core::RuntimeIoWaveError::Stopped(_)
                )
            )) | Err(PropertySpillError::DescriptorTree(
                GraphDescriptorTreeError::Work(crate::background::CheckpointWorkError::Io(
                    hawdb_core::RuntimeIoWaveError::Stopped(_)
                ))
            ))
        ));
        probe.assert_released(&scheduler);
        assert_eq!(prepared.data_published, cut == 2);
        assert_eq!(destination.exists(), cut == 2);
        assert_eq!(prepared.temporary_artifact_path.exists(), cut == 1);
        let fresh = CheckpointWorkContext::default();
        if cut == 2 {
            let wrong_path = fixture.0.join("publication-wrong-path.hawdb");
            assert!(matches!(
                prepared.publish_with_work_context(&wrong_path, &fresh),
                Err(PropertySpillError::Corrupt(_))
            ));
            assert!(!wrong_path.exists());
        }
        let actual = prepared
            .publish_with_work_context(&destination, &fresh)
            .unwrap();
        assert_eq!(actual, expected);
        // A lost successful response also reuses the published immutable tree.
        assert_eq!(
            prepared
                .publish_with_work_context(&destination, &fresh)
                .unwrap(),
            expected
        );
        for (actual_path, expected_path) in [
            (
                destination,
                fixture.0.join("publication-ordinary-final.hawdb"),
            ),
            (
                fixture.0.join(format!("{name}.pages.hawdb")),
                fixture.0.join("publication-ordinary.pages.hawdb"),
            ),
            (
                fixture.0.join(format!("{name}.root.hawdb")),
                fixture.0.join("publication-ordinary.root.hawdb"),
            ),
        ] {
            assert_eq!(
                fs::read(actual_path).unwrap(),
                fs::read(expected_path).unwrap(),
                "cut={cut}"
            );
        }
    }
}

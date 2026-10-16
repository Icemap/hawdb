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
use hawdb_core::{RuntimeMemoryError, RuntimeTaskContext};
use hawdb_qos::{
    IoConcurrencyBudget, LocalQosPolicy, LocalQosScheduler, RuntimeGovernor, RuntimeGovernorConfig,
    RuntimeMemorySnapshot, RuntimeResourceBudget, RuntimeResourceSnapshot, RuntimeWorkRequest,
};

mod immutable_handle;

struct Fixture {
    reader: FileSegmentRangeReader,
    range: SegmentReadRange,
    cache: Arc<SegmentCache>,
    expected: Vec<u8>,
    path: PathBuf,
}

impl Fixture {
    fn new(length: usize) -> Self {
        static ORDINAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let ordinal = ORDINAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "hawdb-checkpoint-range-memory-{}-{ordinal}.bin",
            std::process::id()
        ));
        let expected = (0..length)
            .map(|ordinal| (ordinal.wrapping_mul(79) % 256) as u8)
            .collect::<Vec<_>>();
        std::fs::write(&path, &expected).unwrap();
        let cache = Arc::new(SegmentCache::new(length as u64));
        let mut reader = FileSegmentRangeReader::new().with_cache(
            cache.clone(),
            StoreId(71),
            ManifestGeneration(3),
        );
        reader.register(7, &path);
        let range = SegmentReadRange::new(7, 11, 0, NonZeroU64::new(length as u64).unwrap())
            .with_content_digest(content_digest(&expected));
        Self {
            reader,
            range,
            cache,
            expected,
            path,
        }
    }

    fn assert_source_unchanged(&self) {
        assert_eq!(std::fs::read(&self.path).unwrap(), self.expected);
        assert_eq!(
            self.cache.snapshot(),
            SegmentCache::new(self.expected.len() as u64).snapshot()
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn governor(bytes: u64) -> RuntimeGovernor {
    RuntimeGovernor::new(
        RuntimeGovernorConfig {
            memory_budget_bytes: Some(bytes),
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

fn scheduler() -> LocalQosScheduler {
    LocalQosScheduler::new(LocalQosPolicy {
        max_background_operations: Some(1),
        max_total_background_operations: Some(1),
        ..LocalQosPolicy::default()
    })
}

#[test]
fn checkpoint_units_range_read_retains_memory_after_task_close_until_payload_drops() {
    for length in [1usize, 64 * 1024 - 1, 64 * 1024 + 1] {
        let fixture = Fixture::new(length);
        let ceiling = 2 * length as u64 + 4096;
        let governor = governor(ceiling);
        let permit = governor
            .try_admit(RuntimeWorkRequest::background_maintenance(ceiling).with_io_wave_slots(1))
            .unwrap();
        let local = scheduler();
        let work =
            CheckpointWorkContext::new(permit.bind_task_context(RuntimeTaskContext::default()))
                .with_scheduler(local.clone());
        let output = fixture
            .reader
            .checkpoint_range_with_work_context(&fixture.range, &work)
            .unwrap();
        assert_eq!(&*output, fixture.expected);
        drop(permit);
        drop(work);
        let closed = governor.snapshot();
        assert_eq!(closed.active_cpu_slots, 0);
        assert_eq!(closed.active_background_tasks, 0);
        assert_eq!(closed.active_background_io_slots, 0);
        assert_eq!(local.state().running_background_operations, 0);
        assert_eq!(closed.admitted_memory_bytes, ceiling);
        assert_eq!(&*output, fixture.expected);
        fixture.assert_source_unchanged();
        drop(output);
        assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    }
}

#[test]
fn checkpoint_units_range_read_admits_scratch_overlap_and_retries_complete_source() {
    let fixture = Fixture::new(64 * 1024 + 1);
    // The payload fits exactly, including its ownership lease; scratch does not.
    let ceiling = fixture.expected.len() as u64 + 24;
    let denied = governor(ceiling);
    let permit = denied
        .try_admit(RuntimeWorkRequest::background_maintenance(ceiling).with_io_wave_slots(1))
        .unwrap();
    let local = scheduler();
    let work = CheckpointWorkContext::new(permit.bind_task_context(RuntimeTaskContext::default()))
        .with_scheduler(local.clone());
    assert!(matches!(
        fixture
            .reader
            .checkpoint_range_with_work_context(&fixture.range, &work),
        Err(CheckpointRangeReadError::Work(CheckpointWorkError::Memory(
            RuntimeMemoryError::ReservationExceeded { .. }
        )))
    ));
    assert_eq!(local.state().running_background_operations, 0);
    fixture.assert_source_unchanged();
    drop(work);
    drop(permit);
    assert_eq!(denied.snapshot().admitted_memory_bytes, 0);
    let ceiling = 2 * fixture.expected.len() as u64 + 4096;
    let admitted = governor(ceiling);
    let permit = admitted
        .try_admit(RuntimeWorkRequest::background_maintenance(ceiling).with_io_wave_slots(1))
        .unwrap();
    let work = CheckpointWorkContext::new(permit.bind_task_context(RuntimeTaskContext::default()))
        .with_scheduler(local.clone());
    let output = fixture
        .reader
        .checkpoint_range_with_work_context(&fixture.range, &work)
        .unwrap();
    assert_eq!(&*output, fixture.expected);
    fixture.assert_source_unchanged();
    drop(output);
    drop(work);
    drop(permit);
    assert_eq!(admitted.snapshot().admitted_memory_bytes, 0);
}

#[test]
fn checkpoint_units_range_read_cancels_every_completed_unit_and_retries_every_byte() {
    use crate::background::CheckpointWorkProbe;
    use std::sync::atomic::Ordering;

    let fixture = Fixture::new(2 * 64 * 1024 + 1);
    let ceiling = 2 * fixture.expected.len() as u64 + 4096;
    let measure = governor(ceiling);
    let permit = measure
        .try_admit(RuntimeWorkRequest::background_maintenance(ceiling).with_io_wave_slots(1))
        .unwrap();
    let local = scheduler();
    let probe = Arc::new(CheckpointWorkProbe::default());
    local.set_telemetry_sink(Some(probe.clone()));
    let task = RuntimeTaskContext::without_deadline(probe.cancellation.clone());
    let work =
        CheckpointWorkContext::new(permit.bind_task_context(task)).with_scheduler(local.clone());
    let output = fixture
        .reader
        .checkpoint_range_with_work_context(&fixture.range, &work)
        .unwrap();
    assert_eq!(&*output, fixture.expected);
    let units = probe.completed.load(Ordering::SeqCst);
    assert!(units >= 12);
    assert_eq!(probe.peak_units.load(Ordering::SeqCst), 1);
    probe.assert_released(&local);
    assert_eq!(measure.snapshot().active_background_io_slots, 0);
    drop(output);
    drop(work);
    drop(permit);
    assert_eq!(measure.snapshot().admitted_memory_bytes, 0);

    for limit in 1..=units {
        let governor = governor(ceiling);
        let permit = governor
            .try_admit(RuntimeWorkRequest::background_maintenance(ceiling).with_io_wave_slots(1))
            .unwrap();
        let local = scheduler();
        let probe = Arc::new(CheckpointWorkProbe::default());
        probe.cancel_after.store(limit, Ordering::SeqCst);
        local.set_telemetry_sink(Some(probe.clone()));
        let task = RuntimeTaskContext::without_deadline(probe.cancellation.clone());
        let work = CheckpointWorkContext::new(permit.bind_task_context(task))
            .with_scheduler(local.clone());
        assert!(
            matches!(
                fixture
                    .reader
                    .checkpoint_range_with_work_context(&fixture.range, &work),
                Err(CheckpointRangeReadError::Work(
                    CheckpointWorkError::Stopped(_)
                ))
            ),
            "cancellation cut {limit} of {units}"
        );
        probe.assert_released(&local);
        assert_eq!(governor.snapshot().active_background_io_slots, 0);
        fixture.assert_source_unchanged();
        drop(work);

        let retry =
            CheckpointWorkContext::new(permit.bind_task_context(RuntimeTaskContext::default()))
                .with_scheduler(scheduler());
        let output = fixture
            .reader
            .checkpoint_range_with_work_context(&fixture.range, &retry)
            .unwrap();
        assert_eq!(&*output, fixture.expected);
        fixture.assert_source_unchanged();
        drop(output);
        drop(retry);
        drop(permit);
        let closed = governor.snapshot();
        assert_eq!(closed.active_cpu_slots, 0);
        assert_eq!(closed.active_background_tasks, 0);
        assert_eq!(closed.active_background_io_slots, 0);
        assert_eq!(closed.admitted_memory_bytes, 0);
    }
}

#[test]
fn checkpoint_units_range_read_errors_preserve_typed_diagnostics_and_release_buffers() {
    let fixture = Fixture::new(64 * 1024 + 1);
    let ceiling = 2 * fixture.expected.len() as u64 + 4096;
    let governor = governor(ceiling);
    let permit = governor
        .try_admit(RuntimeWorkRequest::background_maintenance(ceiling).with_io_wave_slots(1))
        .unwrap();
    let local = scheduler();
    let work = CheckpointWorkContext::new(permit.bind_task_context(RuntimeTaskContext::default()))
        .with_scheduler(local.clone());
    let mut wrong_digest = fixture.range.clone();
    wrong_digest.content_digest = Some(crate::cache::ContentDigest(
        content_digest(&fixture.expected).0 ^ u64::MAX,
    ));
    assert!(matches!(
        fixture
            .reader
            .checkpoint_range_with_work_context(&wrong_digest, &work),
        Err(CheckpointRangeReadError::Read(
            SegmentReadError::DigestMismatch {
                artifact_id: 7,
                segment_id: 11
            }
        ))
    ));
    let mut too_long = fixture.range.clone();
    too_long.length = NonZeroU64::new(fixture.expected.len() as u64 + 1).unwrap();
    too_long.content_digest = None;
    assert!(matches!(
        fixture.reader.checkpoint_range_with_work_context(&too_long, &work),
        Err(CheckpointRangeReadError::Read(SegmentReadError::Io {
            artifact_id: 7, source, ..
        })) if source.kind() == std::io::ErrorKind::UnexpectedEof
    ));
    assert_eq!(local.state().running_background_operations, 0);
    assert_eq!(governor.snapshot().active_background_io_slots, 0);
    let output = fixture
        .reader
        .checkpoint_range_with_work_context(&fixture.range, &work)
        .unwrap();
    assert_eq!(&*output, fixture.expected);
    assert_eq!(std::fs::read(&fixture.path).unwrap(), fixture.expected);
    let mut cache = SegmentCache::new(fixture.expected.len() as u64).snapshot();
    cache.digest_mismatch_count = 1;
    assert_eq!(fixture.cache.snapshot(), cache);
    drop(output);
    drop(work);
    drop(permit);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
}

#[test]
fn retained_range_read_completes_with_two_units_without_restarting_blocks_or_checksum() {
    use crate::background::CheckpointWorkProbe;
    use std::sync::atomic::Ordering;
    let fixture = Fixture::new(1024 * 1024 + 17);
    assert!(fixture
        .reader
        .artifacts
        .get(&7)
        .unwrap()
        .registration()
        .unwrap()
        .binding
        .is_none());
    let ceiling = 2 * 1024 * 1024;
    let governor = governor(ceiling);
    let permit = governor
        .try_admit(RuntimeWorkRequest::background_maintenance(ceiling).with_io_wave_slots(1))
        .unwrap();
    let local = scheduler();
    let mut state = None;
    let mut payload = None;
    let mut copied = 0;
    let mut checked = 0;
    let mut attempts = 0;
    for attempt in 1..=128 {
        attempts = attempt;
        let probe = Arc::new(CheckpointWorkProbe::default());
        probe.cancel_after.store(2, Ordering::SeqCst);
        local.set_telemetry_sink(Some(probe.clone()));
        let work = CheckpointWorkContext::new(permit.bind_task_context(
            RuntimeTaskContext::without_deadline(probe.cancellation.clone()),
        ))
        .with_scheduler(local.clone());
        match fixture
            .reader
            .checkpoint_range_with_preparation(&fixture.range, &mut state, &work)
        {
            Ok(output) => {
                payload = Some(output);
                break;
            }
            Err(CheckpointRangeReadError::Work(CheckpointWorkError::Stopped(_))) => {}
            other => panic!("unexpected retained range result: {other:?}"),
        }
        probe.assert_released(&local);
        assert_eq!(probe.completed.load(Ordering::SeqCst), 2);
        assert_eq!(governor.snapshot().active_background_io_slots, 0);
        let current = state.as_ref().unwrap();
        let length = current.payload.as_ref().map_or(0, |bytes| bytes.len());
        assert!(length >= copied);
        assert!(current.checked >= checked);
        copied = length;
        checked = current.checked;
        if let Some(bytes) = &current.payload {
            assert_eq!(&**bytes, &fixture.expected[..length]);
        }
    }
    let payload = payload.expect("retained range made no complete bounded progress");
    assert!(attempts >= 16);
    assert!(state.is_none());
    assert_eq!(&*payload, fixture.expected);
    fixture.assert_source_unchanged();
    local.set_telemetry_sink(None);
    drop(permit);
    assert_eq!(governor.snapshot().active_background_tasks, 0);
    assert_eq!(governor.snapshot().active_background_io_slots, 0);
    assert_eq!(governor.snapshot().admitted_memory_bytes, ceiling);
    drop(payload);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
}

#[test]
fn retained_range_read_rejects_changed_range_and_artifact_without_discarding_original_progress() {
    use crate::background::CheckpointWorkProbe;
    use std::sync::atomic::Ordering;
    let fixture = Fixture::new(128 * 1024 + 17);
    let ceiling = 1024 * 1024;
    let governor = governor(ceiling);
    let permit = governor
        .try_admit(RuntimeWorkRequest::background_maintenance(ceiling).with_io_wave_slots(1))
        .unwrap();
    let local = scheduler();
    let probe = Arc::new(CheckpointWorkProbe::default());
    probe.cancel_after.store(8, Ordering::SeqCst);
    local.set_telemetry_sink(Some(probe.clone()));
    let stopped = CheckpointWorkContext::new(permit.bind_task_context(
        RuntimeTaskContext::without_deadline(probe.cancellation.clone()),
    ))
    .with_scheduler(local.clone());
    let mut state = None;
    assert!(matches!(
        fixture
            .reader
            .checkpoint_range_with_preparation(&fixture.range, &mut state, &stopped),
        Err(CheckpointRangeReadError::Work(
            CheckpointWorkError::Stopped(_)
        ))
    ));
    probe.assert_released(&local);
    local.set_telemetry_sink(None);
    let before = state.as_ref().unwrap().payload.as_ref().unwrap().len();
    let work = CheckpointWorkContext::new(permit.bind_task_context(RuntimeTaskContext::default()))
        .with_scheduler(local.clone());
    let wrong = SegmentReadRange::new(7, 11, 1, fixture.range.length)
        .with_content_digest(fixture.range.content_digest.unwrap());
    assert!(matches!(
        fixture
            .reader
            .checkpoint_range_with_preparation(&wrong, &mut state, &work),
        Err(CheckpointRangeReadError::Read(_))
    ));
    let mut replaced = fixture.reader.clone();
    replaced.register(7, &fixture.path);
    assert!(matches!(
        replaced.checkpoint_range_with_preparation(&fixture.range, &mut state, &work),
        Err(CheckpointRangeReadError::Read(_))
    ));
    assert_eq!(
        state.as_ref().unwrap().payload.as_ref().unwrap().len(),
        before
    );
    let output = fixture
        .reader
        .checkpoint_range_with_preparation(&fixture.range, &mut state, &work)
        .unwrap();
    assert_eq!(&*output, fixture.expected);
    assert!(state.is_none());
    fixture.assert_source_unchanged();
    drop(output);
    drop(stopped);
    drop(work);
    drop(permit);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
}

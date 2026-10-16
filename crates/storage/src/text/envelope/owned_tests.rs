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
use crate::background::{CheckpointOperationError, CheckpointWorkProbe};
use hawdb_core::RuntimeTaskContext;
use hawdb_qos::{
    IoConcurrencyBudget, LocalQosPolicy, LocalQosScheduler, RuntimeGovernor, RuntimeGovernorConfig,
    RuntimeMemorySnapshot, RuntimeResourceBudget, RuntimeResourceSnapshot,
};
use std::num::NonZeroUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

fn governor() -> RuntimeGovernor {
    let governor = RuntimeGovernor::new(
        RuntimeGovernorConfig {
            memory_budget_bytes: Some(24 * 1024 * 1024),
            ..RuntimeGovernorConfig::shared_host()
        },
        RuntimeResourceSnapshot::from_parts(
            RuntimeResourceBudget::from_limits(NonZeroUsize::MIN, None, None),
            RuntimeMemorySnapshot::from_limits(Some(1 << 30), Some(1 << 30), None, None, None),
        ),
        IoConcurrencyBudget::new(2, 1),
    );
    governor.pin_resources();
    governor
}

fn scheduler() -> LocalQosScheduler {
    LocalQosScheduler::new(LocalQosPolicy {
        max_background_operations: Some(1),
        max_total_background_operations: Some(1),
        ..Default::default()
    })
}

fn text(length: usize) -> String {
    let mut state = 0x8659_67bf_718e_4d83u64;
    (0..length)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            char::from(32 + (state % 95) as u8)
        })
        .collect()
}

#[test]
fn owned_envelope_matches_both_existing_codecs_and_retains_only_output_capacity() {
    let resources = governor();
    let admission = resources
        .try_admit_incremental_maintenance(0, 24 * 1024 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone())
        .with_scheduler(scheduler());
    // The governor also admits the concrete allocation-permit object. Measure
    // that existing cost independently before testing the returned byte owner.
    let permit = work.reserve_memory(1).unwrap().unwrap();
    let permit_bytes = admission.memory_report().live_accounted_bytes - permit.bytes();
    assert!(permit_bytes > 0 && permit_bytes < 256);
    drop(permit);
    assert_eq!(admission.memory_report().live_accounted_bytes, 0);
    for length in [
        0, 1, 32767, 32768, 32769, 65535, 65536, 65537, 131073, 393217,
    ] {
        let text = text(length);
        let ordinary = encode_durable_text(&text, DurableCompression::default()).unwrap();
        let preceding = encode_durable_text_with_work_context(
            &text,
            DurableCompression::default(),
            &CheckpointWorkContext::default(),
        )
        .unwrap();
        let encoded = encode_owned_durable_text_with_work_context(
            &text,
            DurableCompression::default(),
            &work,
        )
        .unwrap();
        assert_eq!(&*encoded, ordinary);
        assert_eq!(&*encoded, preceding);
        assert_eq!(
            read_durable_text_bytes(&encoded, "owned envelope").unwrap(),
            text
        );
        assert_eq!(
            admission.memory_report().live_accounted_bytes,
            encoded.len() as u64 + permit_bytes
        );
        assert!(admission.memory_report().peak_accounted_bytes <= 24 * 1024 * 1024);
        drop(encoded);
        assert_eq!(admission.memory_report().live_accounted_bytes, 0);
    }
    let encoded = encode_owned_durable_text_with_work_context(
        &text(131073),
        DurableCompression::default(),
        &work,
    )
    .unwrap();
    let retained = encoded.len() as u64 + permit_bytes;
    drop(work);
    drop(admission);
    let snapshot = resources.snapshot();
    assert_eq!(snapshot.admitted_memory_bytes, retained);
    assert_eq!(snapshot.active_background_tasks, 0);
    assert_eq!(snapshot.active_cpu_slots, 0);
    drop(encoded);
    assert_eq!(resources.snapshot().admitted_memory_bytes, 0);
}

#[test]
fn compression_staging_buffer_is_admitted_before_native_context_creation() {
    let resources = governor();
    let admission = resources
        .try_admit_incremental_maintenance(0, 8 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone());
    let error = work
        .classify(|work| {
            crate::compression::Encoder::new_with_work_context(Vec::new(), 3, work).map(drop)
        })
        .unwrap_err();
    assert!(matches!(
        error,
        CheckpointOperationError::Work(CheckpointWorkError::Memory(_))
    ));
    assert_eq!(admission.memory_report().peak_accounted_bytes, 0);
    assert_eq!(admission.memory_report().live_accounted_bytes, 0);
}

#[test]
fn owned_envelope_denies_native_workspace_before_allocating_a_wide_unadmitted_block() {
    let text = text(16384);
    let resources = governor();
    let admission = resources
        .try_admit_incremental_maintenance(0, 256 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone())
        .with_scheduler(scheduler());
    let observer = crate::test_allocator::AllocationObservation::start();
    let error = work
        .classify(|work| {
            encode_owned_durable_text_with_work_context(&text, DurableCompression::default(), work)
        })
        .unwrap_err();
    assert_eq!(observer.finish(), 0);
    assert!(matches!(
        error,
        CheckpointOperationError::Work(CheckpointWorkError::Memory(_))
    ));
    assert!(admission.memory_report().peak_accounted_bytes > 32 * 1024);
    assert!(admission.memory_report().peak_accounted_bytes <= 256 * 1024);
    assert_eq!(admission.memory_report().live_accounted_bytes, 0);
}

#[test]
fn owned_envelope_cancels_at_every_observed_unit_and_refunds_all_native_and_byte_owners() {
    let text = text(131073);
    let resources = governor();
    let local = scheduler();
    let probe = Arc::new(CheckpointWorkProbe::default());
    local.set_telemetry_sink(Some(probe.clone()));
    let admission = resources
        .try_admit_incremental_maintenance(0, 24 * 1024 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone())
        .with_scheduler(local.clone());
    let encoded = work
        .classify(|work| {
            encode_owned_durable_text_with_work_context(&text, DurableCompression::default(), work)
        })
        .unwrap();
    let units = probe.completed.load(Ordering::SeqCst);
    assert!(units > 16);
    drop(encoded);
    drop(work);
    drop(admission);
    assert_eq!(resources.snapshot().admitted_memory_bytes, 0);
    for limit in 1..=units {
        let local = scheduler();
        let probe = Arc::new(CheckpointWorkProbe::default());
        probe.cancel_after.store(limit, Ordering::SeqCst);
        local.set_telemetry_sink(Some(probe.clone()));
        let admission = resources
            .try_admit_incremental_maintenance(
                0,
                24 * 1024 * 1024,
                1,
                RuntimeTaskContext::without_deadline(probe.cancellation.clone()),
            )
            .unwrap();
        let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone())
            .with_scheduler(local.clone());
        let error = work
            .classify(|work| {
                encode_owned_durable_text_with_work_context(
                    &text,
                    DurableCompression::default(),
                    work,
                )
            })
            .unwrap_err();
        assert!(
            matches!(
                error,
                CheckpointOperationError::Work(CheckpointWorkError::Stopped(_))
            ),
            "unit {limit}: {error:?}"
        );
        assert_eq!(
            admission.memory_report().live_accounted_bytes,
            0,
            "unit {limit}"
        );
        probe.assert_released(&local);
        drop(work);
        drop(admission);
        assert_eq!(
            resources.snapshot().admitted_memory_bytes,
            0,
            "unit {limit}"
        );
    }
}

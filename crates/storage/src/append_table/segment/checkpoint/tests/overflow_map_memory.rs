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
use crate::relational::overflow::CheckpointRelationalOverflow;
use hawdb_core::{RuntimeCancellationReason, RuntimeMemoryError, RuntimeTaskContext};

fn inputs(work: &CheckpointWorkContext) -> Vec<CheckpointRelationalOverflow> {
    (0..512u64)
        .map(|id| {
            let mut raw = [17u8; 128];
            raw[..8].copy_from_slice(&id.to_le_bytes());
            crate::relational::overflow::encode_overflow_envelope_with_work_context(
                RelationalScalarType::Bytea,
                &raw,
                RelationalOverflowConfig::default(),
                work,
            )
            .unwrap()
        })
        .collect()
}

#[test]
fn overflow_map_growth_covers_actual_backing_allocations() {
    let governor = output_memory::governor();
    let admission = governor
        .try_admit_incremental_maintenance(0, 8 * 1024 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone());
    let values = inputs(&work);
    let before = governor.snapshot().admitted_memory_bytes;
    let observation = crate::test_allocator::AllocationObservation::start_all();
    let mut map = CheckpointOverflows::default();
    for value in &values {
        assert!(map.insert(value.clone(), &work).unwrap().is_none());
    }
    let allocated = observation.finish_bytes();
    assert!(allocated > 16 * 1024);
    let covered = governor.snapshot().admitted_memory_bytes - before;
    let count = map.len();
    drop(map);
    assert_eq!(governor.snapshot().admitted_memory_bytes, before);
    drop(values);
    drop(work);
    drop(admission);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    assert_eq!(count, 512);
    assert!(
        covered >= allocated as u64,
        "overflow map allocated {allocated} backing bytes with only {covered} admitted"
    );
}

#[test]
fn overflow_map_growth_denial_refunds_every_private_container() {
    let governor = output_memory::governor();
    let input_admission = governor
        .try_admit_incremental_maintenance(0, 8 * 1024 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let input_work = CheckpointWorkContext::new(input_admission.task_context().unwrap().clone());
    let values = inputs(&input_work);
    drop(input_work);
    drop(input_admission);
    let before = governor.snapshot().admitted_memory_bytes;
    let admission = governor
        .try_admit_incremental_maintenance(0, 16 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone());
    let mut completed = 0;
    let result = work.classify(|work| -> Result<_, AppendTableError> {
        let mut map = CheckpointOverflows::default();
        for value in &values {
            map.insert(value.clone(), work)?;
            completed += 1;
        }
        Ok(map)
    });
    let denied = matches!(
        &result,
        Err(CheckpointOperationError::Work(CheckpointWorkError::Memory(
            RuntimeMemoryError::AdmissionDenied {
                requested_bytes,
                available_bytes,
                retryable: true,
            }
        ))) if requested_bytes > available_bytes
    );
    let diagnostic = format!("{:?}", result.as_ref().err());
    let peak = admission.memory_report().peak_accounted_bytes;
    drop(result);
    assert_eq!(admission.memory_report().live_accounted_bytes, 0);
    drop(work);
    drop(admission);
    assert_eq!(governor.snapshot().admitted_memory_bytes, before);
    drop(values);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    assert!(
        denied,
        "a 16 KiB working set did not return the expected denial: {diagnostic}"
    );
    assert!((1..512).contains(&completed));
    assert!((1..=16 * 1024).contains(&peak));
}

#[test]
fn overflow_map_denial_preserves_entries_and_duplicates_need_no_new_allocation() {
    let governor = output_memory::governor();
    let input_admission = governor
        .try_admit_incremental_maintenance(0, 8 * 1024 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let input_work = CheckpointWorkContext::new(input_admission.task_context().unwrap().clone());
    let values = inputs(&input_work);
    drop(input_work);
    drop(input_admission);
    let before = governor.snapshot().admitted_memory_bytes;
    let admission = governor
        .try_admit_incremental_maintenance(0, 16 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone());
    let mut map = CheckpointOverflows::default();
    let mut inserted = 0;
    for value in &values {
        let len = map.len();
        match map.insert(value.clone(), &work) {
            Ok(previous) => {
                assert!(previous.is_none());
                inserted += 1;
            }
            Err(_) => {
                assert_eq!(map.len(), len);
                assert!(!map.contains_key(&value.reference.digest));
                break;
            }
        }
    }
    assert!((1..512).contains(&inserted));
    for value in &values[..inserted] {
        let actual = &map[&value.reference.digest];
        assert_eq!(actual.reference, value.reference);
        assert_eq!(actual.bytes.as_ref(), value.bytes.as_ref());
    }
    let covered = governor.snapshot().admitted_memory_bytes;
    let observation = crate::test_allocator::AllocationObservation::start_all();
    for _ in 0..512 {
        let old = map.insert(values[0].clone(), &work).unwrap().unwrap();
        assert_eq!(old.reference, values[0].reference);
        assert_eq!(old.bytes.as_ref(), values[0].bytes.as_ref());
    }
    assert_eq!(observation.finish_bytes(), 0);
    assert_eq!(governor.snapshot().admitted_memory_bytes, covered);
    assert_eq!(map.len(), inserted);
    drop(map);
    assert_eq!(admission.memory_report().live_accounted_bytes, 0);
    drop(work);
    drop(admission);
    assert_eq!(governor.snapshot().admitted_memory_bytes, before);
    drop(values);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
}

#[test]
fn overflow_map_order_and_node_coverage_match_different_insertion_orders() {
    let governor = output_memory::governor();
    let admission = governor
        .try_admit_incremental_maintenance(0, 8 * 1024 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone());
    let values = inputs(&work);
    let mut order = (0..values.len()).collect::<Vec<_>>();
    order.sort_by_key(|&index| values[index].reference.digest);
    let expected = order
        .iter()
        .map(|&index| values[index].reference.digest)
        .collect::<Vec<_>>();
    for permutation in 0..3 {
        if permutation == 1 {
            order.reverse();
        } else if permutation == 2 {
            let mut seed = 0xbb67_ae85_84ca_a73bu64;
            for index in (1..order.len()).rev() {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                order.swap(index, seed as usize % (index + 1));
            }
        }
        let before = governor.snapshot().admitted_memory_bytes;
        let mut map = CheckpointOverflows::default();
        let mut allocated = 0;
        for (index, &value) in order.iter().enumerate() {
            let observation = crate::test_allocator::AllocationObservation::start_all();
            map.insert(values[value].clone(), &work).unwrap();
            allocated += observation.finish_bytes();
            let covered = governor.snapshot().admitted_memory_bytes - before;
            assert!(
                covered >= allocated as u64,
                "order {permutation}, prefix {} allocated {allocated} bytes, covered {covered}",
                index + 1
            );
        }
        assert!(map.keys().copied().eq(expected.iter().copied()));
        for value in &values {
            let actual = &map[&value.reference.digest];
            assert_eq!(actual.reference, value.reference);
            assert_eq!(actual.bytes.as_ref(), value.bytes.as_ref());
        }
        drop(map);
        assert_eq!(governor.snapshot().admitted_memory_bytes, before);
    }
    drop(values);
    drop(work);
    drop(admission);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
}

#[test]
fn overflow_map_keeps_nodes_and_envelopes_admitted_after_task_closure() {
    let governor = output_memory::governor();
    let admission = governor
        .try_admit_incremental_maintenance(0, 8 * 1024 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone());
    let values = inputs(&work);
    let expected = values
        .iter()
        .map(|value| (value.reference.digest, value.bytes.as_ref().to_vec()))
        .collect::<BTreeMap<_, _>>();
    let mut map = CheckpointOverflows::default();
    for value in values {
        map.insert(value, &work).unwrap();
    }
    let covered = governor.snapshot().admitted_memory_bytes;
    drop(work);
    drop(admission);
    assert_eq!(governor.snapshot().admitted_memory_bytes, covered);
    for (digest, bytes) in &expected {
        assert_eq!(map[digest].bytes.as_ref(), bytes);
    }
    let last = map.first_key_value().unwrap().1.bytes.clone();
    let last_bytes = last.as_ref().to_vec();
    drop(map);
    assert!(governor.snapshot().admitted_memory_bytes >= last.len() as u64);
    assert_eq!(last.as_ref(), last_bytes);
    drop(last);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
}

#[test]
fn overflow_map_cancels_every_unit_refunds_and_retries_complete_ordered_bytes() {
    let governor = output_memory::governor();
    let input_admission = governor
        .try_admit_incremental_maintenance(0, 8 * 1024 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let input_work = CheckpointWorkContext::new(input_admission.task_context().unwrap().clone());
    let values = inputs(&input_work);
    drop(input_work);
    drop(input_admission);
    let before = governor.snapshot().admitted_memory_bytes;
    let expected = values
        .iter()
        .map(|value| (value.reference.digest, value.bytes.as_ref().to_vec()))
        .collect::<BTreeMap<_, _>>();
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
        let result = work.classify(|work| -> Result<_, AppendTableError> {
            let mut map = CheckpointOverflows::default();
            for value in &values {
                map.insert(value.clone(), work)?;
            }
            Ok(map)
        });
        if cut == 0 {
            let map = result.unwrap();
            assert!(map.keys().eq(expected.keys()));
            for (digest, bytes) in &expected {
                assert_eq!(map[digest].bytes.as_ref(), bytes);
            }
            if index == 0 {
                cuts.extend(1..=probe.completed.load(Ordering::SeqCst));
                cuts.push(0);
            }
            drop(map);
        } else {
            assert!(
                matches!(
                    result,
                    Err(CheckpointOperationError::Work(
                        CheckpointWorkError::Stopped(RuntimeCancellationReason::Cancelled)
                    ))
                ),
                "overflow map cut {cut}: {result:?}"
            );
            assert_eq!(probe.completed.load(Ordering::SeqCst), cut);
        }
        assert_eq!(admission.memory_report().live_accounted_bytes, 0);
        probe.assert_released(&local);
        drop(work);
        drop(admission);
        assert_eq!(governor.snapshot().admitted_memory_bytes, before);
        index += 1;
    }
    drop(values);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    println!(
        "verified {} overflow map CPU cuts and retry",
        cuts.len() - 2
    );
}

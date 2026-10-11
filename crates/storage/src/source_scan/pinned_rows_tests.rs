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
use crate::background::CheckpointWorkProbe;
use hawdb_core::RuntimeTaskContext;
use hawdb_qos::{
    IoConcurrencyBudget, LocalQosPolicy, LocalQosScheduler, RuntimeGovernor, RuntimeGovernorConfig,
    RuntimeMemorySnapshot, RuntimeResourceBudget, RuntimeResourceSnapshot,
};
use std::num::NonZeroUsize;
use std::sync::Arc;

fn scheduler() -> LocalQosScheduler {
    LocalQosScheduler::new(LocalQosPolicy {
        max_background_operations: Some(1),
        max_total_background_operations: Some(1),
        ..Default::default()
    })
}

fn governor() -> RuntimeGovernor {
    let governor = RuntimeGovernor::new(
        RuntimeGovernorConfig {
            memory_budget_bytes: Some(4 * 1024 * 1024),
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

fn node(id: u64, payload: Value) -> NodeRecord {
    NodeRecord {
        id: NodeId(id),
        labels: std::collections::BTreeSet::from([LabelId(7)]),
        properties: BTreeMap::from([
            ("id".into(), Value::Int(id as i64)),
            ("payload".into(), payload),
        ]),
    }
}

#[test]
fn source_payload_text_preserves_tagged_bytes_and_owns_its_admitted_capacity() {
    let mut deep = Value::Null;
    for _ in 0..12 {
        deep = Value::List(vec![deep]);
    }
    let values = vec![
        Value::Null,
        Value::Bool(false),
        Value::Bool(true),
        Value::Int(i64::MIN),
        Value::Float(f64::from_bits(0xfff8_0000_0000_0042)),
        Value::String("é;=\t\n".repeat(4000)),
        Value::Binary((0..=255).collect()),
        Value::Uuid(hawdb_core::Uuid::from_u128(u128::MAX)),
        Value::List(vec![]),
        Value::Map(BTreeMap::new()),
        Value::Map(BTreeMap::from([
            ("é;=".into(), Value::List(vec![Value::Null, Value::Int(-1)])),
            ("binary".into(), Value::Binary(vec![0xff, 0x00, 0x6e])),
        ])),
        deep,
    ];
    let rows = values
        .into_iter()
        .enumerate()
        .map(|(id, value)| node(id as u64, value))
        .collect::<Vec<_>>();
    let expected = format!(
        "{SOURCE_SCAN_SEGMENT_HEADER}\n{}",
        rows.iter()
            .map(|row| format!(
                "row\t{}\t{}\n",
                row.id.0,
                crate::text::encode_properties(&row.properties)
            ))
            .collect::<String>()
    );
    let resources = governor();
    let admission = resources
        .try_admit_incremental_maintenance(0, 1024 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone())
        .with_scheduler(scheduler());
    let output = encode_segment_text_with_work_context(
        rows.iter().map(|row| (true, row.id.0, &row.properties)),
        &work,
    )
    .unwrap();
    assert_eq!(&*output, expected);
    let charged = admission.memory_report();
    assert!(charged.live_accounted_bytes >= output.len() as u64);
    assert!(charged.peak_accounted_bytes <= 1024 * 1024);
    drop(output);
    assert_eq!(admission.memory_report().live_accounted_bytes, 0);
    drop(work);
    drop(admission);
    assert_eq!(resources.snapshot().admitted_memory_bytes, 0);
}

#[test]
fn source_payload_denies_wide_encoding_before_unadmitted_property_copies() {
    let row = node(7, Value::List(vec![Value::Binary(vec![0x9f; 512 * 1024])]));
    let resources = governor();
    let admission = resources
        .try_admit_incremental_maintenance(0, 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let local = scheduler();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone())
        .with_scheduler(local.clone());
    let observer = crate::test_allocator::AllocationObservation::start();
    let error = work
        .classify(|work| {
            encode_segment_text_with_work_context(
                std::iter::once((true, row.id.0, &row.properties)),
                work,
            )
        })
        .unwrap_err();
    assert_eq!(observer.finish(), 0);
    assert!(matches!(
        error,
        crate::background::CheckpointOperationError::Work(
            crate::background::CheckpointWorkError::Memory(_)
        )
    ));
    assert!(admission.memory_report().peak_accounted_bytes <= 1024);
    assert_eq!(admission.memory_report().live_accounted_bytes, 0);
    assert_eq!(local.state().running_background_operations, 0);
}

#[test]
fn source_payload_cancels_inside_wide_nested_encoding_and_releases_text() {
    use std::sync::atomic::Ordering;

    let row = node(7, Value::List(vec![Value::Binary(vec![0x9f; 512 * 1024])]));
    let resources = governor();
    let local = scheduler();
    let probe = Arc::new(CheckpointWorkProbe::default());
    local.set_telemetry_sink(Some(probe.clone()));
    probe.cancel_after.store(24, Ordering::SeqCst);
    let admission = resources
        .try_admit_incremental_maintenance(
            0,
            3 * 1024 * 1024,
            1,
            RuntimeTaskContext::without_deadline(probe.cancellation.clone()),
        )
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone())
        .with_scheduler(local.clone());
    let error = encode_segment_text_with_work_context(
        std::iter::once((true, row.id.0, &row.properties)),
        &work,
    )
    .unwrap_err();
    assert_checkpoint_unit_cancellation(&error);
    assert_eq!(probe.completed.load(Ordering::SeqCst), 24);
    assert!(admission.memory_report().peak_accounted_bytes > 1024 * 1024);
    assert_eq!(admission.memory_report().live_accounted_bytes, 0);
    probe.assert_released(&local);
}

#[test]
fn pinned_preparation_avoids_wide_nested_record_copies_under_a_small_working_ceiling() {
    let mut nodes = CowSegmentedMap::default();
    for id in 0..4 {
        nodes.insert(
            NodeId(id),
            node(
                id,
                Value::List(vec![Value::Binary(vec![id as u8; 128 * 1024])]),
            ),
        );
    }
    let resources = governor();
    let admission = resources
        .try_admit_incremental_maintenance(0, 3 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone())
        .with_scheduler(scheduler());
    // The ordinary reference still proves the fixture contains four wide
    // maps. Both controlled builders must now account metadata as well.
    let observer = crate::test_allocator::AllocationObservation::start();
    let copied = build_with_work_context(
        4,
        Some(LabelId(7)),
        nodes.values(),
        &CheckpointWorkContext::default(),
    )
    .unwrap();
    assert!(observer.finish() >= 4);
    drop(copied);
    let original = nodes.clone();
    for pinned in [false, true] {
        let observer = crate::test_allocator::AllocationObservation::start();
        let result = work.classify(|work| {
            if pinned {
                build_with_pinned_nodes(4, Some(LabelId(7)), &nodes, work)
            } else {
                build_with_work_context(4, Some(LabelId(7)), nodes.values(), work)
            }
        });
        assert_eq!(observer.finish(), 0);
        assert!(matches!(
            result,
            Err(crate::background::CheckpointOperationError::Work(
                crate::background::CheckpointWorkError::Memory(_)
            ))
        ));
        assert!(nodes.shares_storage_with(&original));
        assert_eq!(nodes.len(), 4);
        for id in 0..4 {
            assert_eq!(nodes.get(&NodeId(id)), original.get(&NodeId(id)));
        }
        assert!(admission.memory_report().peak_accounted_bytes <= 3 * 1024);
        assert_eq!(admission.memory_report().live_accounted_bytes, 0);
    }
    drop(work);
    drop(admission);
    assert_eq!(resources.snapshot().admitted_memory_bytes, 0);
}

#[test]
fn pinned_projection_keeps_summary_leases_until_the_last_clone() {
    let mut nodes = CowSegmentedMap::default();
    nodes.insert(NodeId(7), node(7, Value::String("s".repeat(512 * 1024))));
    let resources = governor();
    let admission = resources
        .try_admit_incremental_maintenance(0, 1024 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone())
        .with_scheduler(scheduler());
    let observer = crate::test_allocator::AllocationObservation::start();
    let projection = build_with_pinned_nodes(1, Some(LabelId(7)), &nodes, &work).unwrap();
    assert_eq!(
        observer.finish(),
        1,
        "copy a distinct dictionary scalar once"
    );
    let bytes = admission.memory_report().live_accounted_bytes;
    assert!(bytes >= 512 * 1024);
    let observer = crate::test_allocator::AllocationObservation::start();
    let copied = projection.clone();
    assert_eq!(observer.finish(), 0);
    assert_eq!(admission.memory_report().live_accounted_bytes, bytes);
    drop(projection);
    assert_eq!(admission.memory_report().live_accounted_bytes, bytes);
    assert_eq!(copied.segments[0].summary.row_count, 1);
    drop(copied);
    assert_eq!(admission.memory_report().live_accounted_bytes, 0);
    drop(work);
    drop(admission);
    assert_eq!(resources.snapshot().admitted_memory_bytes, 0);
}

#[test]
fn pinned_segments_preserve_wire_bytes_and_original_sparse_rows_after_writer_changes() {
    let owned_directory = TestDir::new();
    let pinned_directory = TestDir::new();
    let mut nodes = CowSegmentedMap::default();
    for id in 0..330 {
        let mut record = node(
            id,
            Value::Map(BTreeMap::from([
                ("body".into(), Value::String(format!("source {id} 界🙂"))),
                (
                    "list".into(),
                    Value::List(vec![Value::Int(id as i64), Value::Binary(vec![0, 255])]),
                ),
            ])),
        );
        if id.is_multiple_of(5) {
            record.labels = std::collections::BTreeSet::from([LabelId(8)]);
        }
        nodes.insert(NodeId(id), record);
    }
    let probe = Arc::new(CheckpointWorkProbe::default());
    let local = scheduler();
    let work = probe.context(local.clone());
    let mut copied = build_with_work_context(330, Some(LabelId(7)), nodes.values(), &work).unwrap();
    let mut pinned = build_with_pinned_nodes(330, Some(LabelId(7)), &nodes, &work).unwrap();
    assert_eq!(pinned.segments.len(), 3);
    assert_eq!(
        pinned
            .segments
            .iter()
            .map(|segment| segment.summary.row_count)
            .sum::<u64>(),
        264
    );
    assert!(pinned
        .segments
        .iter()
        .all(|segment| segment.rows.is_empty()));
    nodes.remove(&NodeId(1));
    nodes.insert(
        NodeId(1000),
        node(1000, Value::String("later writer".into())),
    );
    let copied_publication =
        write_with_work_context(owned_directory.path(), &mut copied, &work).unwrap();
    let pinned_publication =
        write_with_work_context(pinned_directory.path(), &mut pinned, &work).unwrap();
    assert_eq!(
        copied_publication.descriptor_checksum(),
        pinned_publication.descriptor_checksum()
    );
    for name in [SOURCE_SCAN_DESCRIPTOR_FILE, SOURCE_SCAN_PAYLOAD_FILE] {
        assert_eq!(
            fs::read(owned_directory.path().join(name)).unwrap(),
            fs::read(pinned_directory.path().join(name)).unwrap()
        );
    }
    let manifest = load(
        pinned_directory.path(),
        330,
        pinned_publication.descriptor_checksum(),
    )
    .unwrap()
    .unwrap();
    let payload = fs::read(pinned_directory.path().join(SOURCE_SCAN_PAYLOAD_FILE)).unwrap();
    let mut recovered = Vec::new();
    for segment in manifest.segments() {
        let range = segment.payload_range;
        recovered.extend(
            decode_payload(
                &payload[range.offset as usize..(range.offset + range.length.get()) as usize],
            )
            .unwrap(),
        );
    }
    assert_eq!(recovered.len(), 264);
    assert_eq!(recovered[0].node_id, 1);
    assert!(recovered
        .iter()
        .all(|row| row.node_id < 330 && !row.node_id.is_multiple_of(5)));
    assert!(recovered.iter().any(|row| row.node_id == 329));
    probe.assert_released(&local);
}

#[test]
fn pinned_reference_capacity_denies_before_any_wide_copy_or_source_change() {
    let mut nodes = CowSegmentedMap::default();
    nodes.insert(NodeId(0), node(0, Value::Binary(vec![0; 128 * 1024])));
    let original = nodes.clone();
    let resources = governor();
    let admission = resources
        .try_admit_incremental_maintenance(0, 64, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone())
        .with_scheduler(scheduler());
    let observer = crate::test_allocator::AllocationObservation::start();
    assert!(matches!(
        work.classify(|work| build_with_pinned_nodes(1, Some(LabelId(7)), &nodes, work)),
        Err(crate::background::CheckpointOperationError::Work(
            crate::background::CheckpointWorkError::Memory(_)
        ))
    ));
    assert_eq!(observer.finish(), 0);
    assert!(nodes.shares_storage_with(&original));
    assert_eq!(nodes.len(), 1);
    assert_eq!(admission.memory_report().live_accounted_bytes, 0);
    assert!(build_with_pinned_nodes(1, None, &nodes, &work)
        .unwrap()
        .segments
        .is_empty());
    drop(work);
    drop(admission);
    assert_eq!(resources.snapshot().admitted_memory_bytes, 0);
}

#[test]
fn pinned_summary_denies_wide_scalar_copies_before_allocation() {
    let mut nodes = CowSegmentedMap::default();
    nodes.insert(NodeId(7), node(7, Value::String("s".repeat(512 * 1024))));
    let resources = governor();
    let admission = resources
        .try_admit_incremental_maintenance(0, 4 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let local = scheduler();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone())
        .with_scheduler(local.clone());
    let observer = crate::test_allocator::AllocationObservation::start();
    let result = work.classify(|work| build_with_pinned_nodes(1, Some(LabelId(7)), &nodes, work));
    let copies = observer.finish();
    assert!(result.is_err(), "a 4 KiB preparation ceiling admitted wide scalar summary copies: {copies} large allocations");
    assert!(matches!(
        result,
        Err(crate::background::CheckpointOperationError::Work(
            crate::background::CheckpointWorkError::Memory(_)
        ))
    ));
    assert_eq!(copies, 0, "deny before copying the 512 KiB scalar");
    assert!(admission.memory_report().peak_accounted_bytes <= 4 * 1024);
    assert_eq!(admission.memory_report().live_accounted_bytes, 0);
    assert_eq!(local.state().running_background_operations, 0);
    assert_eq!(
        nodes.get(&NodeId(7)).unwrap().properties["payload"],
        Value::String("s".repeat(512 * 1024))
    );
}

#[test]
fn scalar_copy_denial_reaches_scalar_after_admitting_metadata() {
    let mut nodes = CowSegmentedMap::default();
    nodes.insert(NodeId(7), node(7, Value::String("s".repeat(512 * 1024))));
    let resources = governor();
    let admission = resources
        .try_admit_incremental_maintenance(0, 32 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone())
        .with_scheduler(scheduler());
    let observer = crate::test_allocator::AllocationObservation::start();
    let result = work.classify(|work| build_with_pinned_nodes(1, Some(LabelId(7)), &nodes, work));
    let copies = observer.finish();
    assert!(
        result.is_err(),
        "unadmitted scalar result; large copies: {copies}"
    );
    let error = result.unwrap_err();
    assert_eq!(copies, 0);
    assert!(matches!(error,
        crate::background::CheckpointOperationError::Work(crate::background::CheckpointWorkError::Memory(hawdb_core::RuntimeMemoryError::ReservationExceeded { requested_bytes, available_bytes }))
        if requested_bytes == 512 * 1024 && available_bytes == 32 * 1024
    ));
    assert_eq!(admission.memory_report().live_accounted_bytes, 0);
    assert!(admission.memory_report().peak_accounted_bytes > 4 * 1024);
    assert!(admission.memory_report().peak_accounted_bytes <= 32 * 1024);
}

#[test]
fn source_descriptor_denies_before_unadmitted_scalar_encoding() {
    let mut nodes = CowSegmentedMap::default();
    nodes.insert(NodeId(7), node(7, Value::String("s".repeat(512 * 1024))));
    let mut projection = build_with_pinned_nodes(
        1,
        Some(LabelId(7)),
        &nodes,
        &CheckpointWorkContext::default(),
    )
    .unwrap();
    projection
        .segments
        .make_mut(&CheckpointWorkContext::default())
        .unwrap()[0]
        .payload_range = Some(SegmentPayloadRange {
        artifact_id: 1,
        offset: 0,
        length: NonZeroU64::MIN,
        checksum: 0,
    });
    let resources = governor();
    let admission = resources
        .try_admit_incremental_maintenance(0, 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone())
        .with_scheduler(scheduler());
    let observer = crate::test_allocator::AllocationObservation::start();
    let result = work.classify(|work| encode_descriptor_with_work_context(&projection, work));
    let copies = observer.finish();
    assert!(
        result.is_err(),
        "unadmitted descriptor result; large allocations: {copies}"
    );
    let error = result.unwrap_err();
    assert_eq!(copies, 0);
    assert!(matches!(
        error,
        crate::background::CheckpointOperationError::Work(
            crate::background::CheckpointWorkError::Memory(_)
        )
    ));
    assert_eq!(admission.memory_report().live_accounted_bytes, 0);
    assert!(admission.memory_report().peak_accounted_bytes <= 1024);
}

#[test]
fn source_descriptor_retains_its_lease_through_footer_and_extreme_ranges() {
    let record = node(u64::MAX, Value::String("é;\t\n".repeat(4096)));
    let expected_summary = reference::summary(
        0,
        &[SourceScanRow {
            node_id: u64::MAX,
            properties: record.properties.clone(),
        }],
    );
    let mut nodes = CowSegmentedMap::default();
    nodes.insert(record.id, record);
    let mut projection = build_with_pinned_nodes(
        u64::MAX,
        Some(LabelId(7)),
        &nodes,
        &CheckpointWorkContext::default(),
    )
    .unwrap();
    let range = SegmentPayloadRange {
        artifact_id: u64::MAX,
        offset: u64::MAX,
        length: NonZeroU64::new(u64::MAX).unwrap(),
        checksum: u64::MAX,
    };
    projection
        .segments
        .make_mut(&CheckpointWorkContext::default())
        .unwrap()[0]
        .payload_range = Some(range);
    let expected_body = reference::descriptor(u64::MAX, &[expected_summary], &[range]);
    let (expected_file, expected_checksum) = reference::descriptor_file(&expected_body);
    let resources = governor();
    let admission = resources
        .try_admit_incremental_maintenance(0, 1024 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let local = scheduler();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone())
        .with_scheduler(local.clone());
    let mut body = work
        .classify(|work| encode_descriptor_with_work_context(&projection, work))
        .unwrap();
    assert_eq!(body.as_bytes(), expected_body.as_bytes());
    assert!(admission.memory_report().live_accounted_bytes >= body.text.capacity() as u64);
    let checksum = work.checksum(body.as_bytes()).unwrap();
    assert_eq!(checksum, expected_checksum);
    body.fields(format_args!("checksum\t{checksum}\n"), &work)
        .unwrap();
    assert_eq!(body.as_bytes(), expected_file.as_bytes());
    assert!(admission.memory_report().live_accounted_bytes >= body.text.capacity() as u64);
    let decoded = decode_descriptor(&body, checksum).unwrap();
    assert_eq!(decoded.graph_epoch, u64::MAX);
    assert_eq!(decoded.segments[0].payload_range, Some(range));
    assert_eq!(decoded.segments[0].summary, projection.segments[0].summary);
    assert!(admission.memory_report().peak_accounted_bytes <= 1024 * 1024);
    drop(body);
    assert_eq!(admission.memory_report().live_accounted_bytes, 0);
    assert_eq!(local.state().running_background_operations, 0);
    drop(work);
    drop(admission);
    assert_eq!(resources.snapshot().admitted_memory_bytes, 0);
}

#[test]
fn pinned_summary_preserves_all_128_positions_for_a_repeated_exact_key() {
    let mut nodes = CowSegmentedMap::default();
    let mut rows = Vec::new();
    for id in 0..128 {
        let mut record = node(id, Value::Int(id as i64));
        record
            .properties
            .insert("id".into(), Value::String("same;é".into()));
        rows.push(SourceScanRow {
            node_id: id,
            properties: record.properties.clone(),
        });
        nodes.insert(record.id, record);
    }
    let expected = reference::summary(0, &rows);
    let resources = governor();
    let admission = resources
        .try_admit_incremental_maintenance(0, 256 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone())
        .with_scheduler(scheduler());
    let projection = build_with_pinned_nodes(128, Some(LabelId(7)), &nodes, &work).unwrap();
    assert_eq!(projection.segments.len(), 1);
    assert_eq!(projection.segments[0].summary, expected);
    let exact = &projection.segments[0].summary.fields["id"].exact_values;
    assert_eq!(exact.len(), 1);
    assert_eq!(
        exact.values().next().unwrap().iter().collect::<Vec<_>>(),
        (0..128).collect::<Vec<_>>()
    );
    assert!(admission.memory_report().live_accounted_bytes > 0);
    drop(projection);
    assert_eq!(admission.memory_report().live_accounted_bytes, 0);
}

#[test]
fn source_exact_cursor_allocations_fit_the_admitted_segment_bound() {
    // Observe every actual allocation, including replaced capacities. The
    // summary inserts local positions in ascending order for at most 128 rows.
    let observer = crate::test_allocator::AllocationObservation::start_all();
    let mut cursor = roaring::RoaringTreemap::new();
    for position in 0..128 {
        cursor.insert(position);
    }
    let bytes = observer.finish_bytes();
    assert!(bytes > 0);
    assert!(bytes <= summary::EXACT_CURSOR_ALLOCATION_BYTES, "{bytes}");
    assert_eq!(cursor.len(), 128);
    assert!(cursor.contains(127));
}

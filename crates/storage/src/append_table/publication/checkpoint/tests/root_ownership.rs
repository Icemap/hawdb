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

#[test]
fn segment_mount_shares_the_decoded_descriptor_allocation_without_a_full_array_copy() {
    let directory = Directory::new();
    let config = AppendSegmentConfig::default();
    let rows = (0..512i64)
        .map(|id| AppendTableRow {
            table: "events".into(),
            partition_key: RelationalKey(vec![RelationalValue::BigInt(id)]),
            order_key: RelationalKey(vec![RelationalValue::BigInt(1)]),
            row: RelationalRow::new(vec![RelationalValue::BigInt(id)]),
        })
        .collect::<Vec<_>>();
    let output = AppendSegmentWriter::encode(1, 1, &rows, config).unwrap();
    assert_eq!(output.descriptors.len(), 512);
    let path = directory.0.join(append_segment_file(1));
    fs::write(&path, &output.encoded).unwrap();
    let governor = manifest_governor();
    let admission = governor
        .try_admit_incremental_maintenance(0, 2 * 1024 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let work = CheckpointWorkContext::new(admission.task_context().unwrap().clone());
    let observation = crate::test_allocator::AllocationObservation::start();
    let reader = AppendSegmentReader::open_bound_file_with_work_context(
        &path,
        output.artifact,
        config,
        &work,
    )
    .unwrap();
    let large_allocations = observation.finish();
    assert_eq!(reader.descriptors(), output.descriptors);
    let retained = admission.memory_report().live_accounted_bytes;
    let clone = reader.clone();
    drop(reader);
    drop(work);
    drop(admission);
    assert_eq!(governor.snapshot().admitted_memory_bytes, retained);
    assert_eq!(clone.descriptors(), output.descriptors);
    drop(clone);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    assert_eq!(fs::read(path).unwrap(), output.encoded.as_ref());
    assert_eq!(large_allocations, 2, "mount needs the input directory and one decoded descriptor allocation; publishing shared ownership must not copy the full array");
}

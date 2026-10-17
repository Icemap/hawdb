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
use crate::build_memory::read::{tests::governor, ReadAdmission};
use crate::build_memory::BuildMemory;
use crate::test_allocation as allocation;
use hawdb_core::RuntimeTaskContext;
use hawdb_qos::RuntimeWorkRequest;

#[test]
fn segment_constructor_admits_descriptor_before_decoding_rejected_field_maps() {
    let _serial = allocation::serial();
    assert_eq!(allocation::live(), 0);
    let root = test_dir("descriptor-admission");
    let mut index = SearchIndex::open(&root).unwrap();
    index.upsert(document(0, "team")).unwrap();
    index.checkpoint().unwrap();
    let reader = SearchOutOfCoreReader::open(&root).unwrap();
    let descriptor = SearchSegmentDescriptor {
        target_documents: 256,
        document_count: 1,
        segments: vec![SearchSegmentDescriptorEntry {
            segment_id: 0,
            first_document_id: "a".into(),
            last_document_id: "z".into(),
            document_count: 1,
            payload_range: None,
            metadata: (0..8192)
                .map(|field| {
                    (
                        format!("field-{field}"),
                        crate::SearchSegmentFieldSummary {
                            present_count: 1,
                            values: BTreeSet::from(["value".into()]),
                            ..Default::default()
                        },
                    )
                })
                .collect(),
        }],
    };
    let mut bytes = Vec::new();
    crate::document_encoding::DescriptorEncoding::new(&descriptor, u64::MAX)
        .unwrap()
        .write_to(&mut bytes)
        .unwrap();
    let mut selected = reader.manifest.segments[0].clone();
    selected.descriptor_len = bytes.len() as u64;
    selected.descriptor_checksum = checksum_bytes(&bytes);
    fs::write(root.join(&selected.descriptor_file), &bytes).unwrap();
    // A schema-decodable, checksum-valid candidate rejected by the later
    // physical-range validator still needs admission before its maps exist.
    let (legacy, legacy_peak) = allocation::measure(|| {
        SearchOutOfCoreSegmentReader::open(
            &root,
            &reader.config,
            &reader.analyzer_lexicon,
            reader.lexical_source_policy,
            reader.lexical_term_policy,
            &reader.manifest,
            &selected,
        )
        .unwrap_err()
    });
    assert!(
        matches!(&legacy, HawDBError::Storage(message) if message.contains("no physical payload range"))
    );
    assert!(legacy_peak > 2 << 20);
    drop(legacy);
    assert_eq!(allocation::live(), 0);
    assert_segment_refused_before_expansion(&root, &reader, &selected, bytes.len());
    drop(reader);
    drop(index);
    fs::remove_dir_all(root).unwrap();
}

fn assert_segment_refused_before_expansion(
    root: &Path,
    reader: &SearchOutOfCoreReader,
    selected: &SearchOutOfCoreSegmentManifest,
    encoded_len: usize,
) {
    let governor = governor();
    let permit = governor
        .try_admit(RuntimeWorkRequest::background_maintenance(2 << 20).with_io_wave_slots(1))
        .unwrap();
    let task = permit.bind_task_context(RuntimeTaskContext::default());
    let memory = BuildMemory::new(&task).unwrap();
    for account in [&memory.input, &memory.spool, &memory.retained] {
        drop(account.reserve(1).unwrap());
    }
    drop(task.acquire_io_wave(NonZeroUsize::MIN).unwrap());
    let before = memory.ledger.snapshot().used_bytes;
    let host = governor.snapshot().admitted_memory_bytes;
    let (_, peak) = allocation::measure(|| {
        let admission = ReadAdmission {
            memory: &memory,
            task: &task,
            permit: &permit,
        };
        let result = SearchOutOfCoreSegmentReader::open_with_read_admission(
            SegmentOpenContext {
                root,
                config: &reader.config,
                analyzer_lexicon: &reader.analyzer_lexicon,
                lexical_source_policy: reader.lexical_source_policy,
                lexical_term_policy: reader.lexical_term_policy,
                active_manifest: &reader.manifest,
                admission: Some(&admission),
            },
            selected,
        );
        assert!(matches!(result, Err(HawDBError::Execution(_))));
    });
    assert!(
        peak <= encoded_len + 65536,
        "owned metadata allocated before admission: {peak}"
    );
    assert_eq!(memory.ledger.snapshot().used_bytes, before);
    assert_eq!(governor.snapshot().admitted_memory_bytes, host);
    drop(memory);
    drop(task);
    drop(permit);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    assert_eq!(allocation::live(), 0);
}

#[test]
fn segment_constructor_admits_compact_layout_before_decoding_rejected_records() {
    let _serial = allocation::serial();
    assert_eq!(allocation::live(), 0);
    let root = test_dir("layout-admission");
    let mut index = SearchIndex::open(&root).unwrap();
    index.upsert(document(0, "team")).unwrap();
    index.checkpoint().unwrap();
    let reader = SearchOutOfCoreReader::open(&root).unwrap();
    let mut selected = reader.manifest.segments[0].clone();
    // Serde accepts compact sequence records, so the encoded-byte cap alone
    // does not bound the retained struct vector or its growth overlap.
    let records = std::iter::repeat_n("[0,0,[0,0,0,0],[0,0,0,0]]", 16385)
        .collect::<Vec<_>>()
        .join(",");
    let body = format!(
        "{{\"format\":\"{}\",\"generation\":{},\"document_count\":1,\"segments\":[{}]}}",
        OUT_OF_CORE_LAYOUT_FORMAT, selected.generation, records
    );
    let decoded: SearchOutOfCoreLayoutBody = serde_json::from_str(&body).unwrap();
    // The envelope checksum covers canonical serialization of the decoded
    // schema, whereas the file checksum covers the exact compact input.
    let checksum = checksum_bytes(&serde_json::to_vec(&decoded).unwrap());
    let bytes = format!("{{\"body\":{body},\"checksum\":{checksum}}}").into_bytes();
    assert!(bytes.len() < 2 << 20);
    assert_eq!(
        SearchOutOfCoreLayoutBody::decode(&bytes)
            .unwrap()
            .segments
            .len(),
        16385
    );
    selected.layout_len = bytes.len() as u64;
    selected.layout_checksum = checksum_bytes(&bytes);
    fs::write(root.join(&selected.layout_file), &bytes).unwrap();
    let (legacy, peak) = allocation::measure(|| {
        SearchOutOfCoreSegmentReader::open(
            &root,
            &reader.config,
            &reader.analyzer_lexicon,
            reader.lexical_source_policy,
            reader.lexical_term_policy,
            &reader.manifest,
            &selected,
        )
        .unwrap_err()
    });
    assert!(matches!(&legacy, HawDBError::Storage(message) if message.contains("layout header")));
    assert!(
        peak > 2 << 20,
        "fixture must exceed native working budget: {peak}"
    );
    drop(legacy);
    assert_eq!(allocation::live(), 0);
    assert_segment_refused_before_expansion(&root, &reader, &selected, bytes.len());
    drop(reader);
    drop(index);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cancelled_layout_decode_preserves_execution_error_before_invalid_storage() {
    let task = RuntimeTaskContext::default();
    task.cancellation().cancel();
    let error =
        SearchOutOfCoreLayoutBody::decode_with_context(b"invalid", Some(&task)).unwrap_err();
    assert!(matches!(error, HawDBError::Execution(_)));
    assert!(matches!(
        SearchOutOfCoreLayoutBody::decode(b"invalid"),
        Err(HawDBError::Storage(_))
    ));
}

// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn allocation_fixture(
    root: &OwnedDirectory,
    count: usize,
    id_suffix: &str,
) -> (Arc<LexicalProjectionReader>, Vec<SearchDocument>) {
    let documents = (0..count)
        .map(|index| document(&format!("document-{index:08}{id_suffix}"), "", "needle"))
        .collect::<Vec<_>>();
    let reader = LexicalProjectionWriter::new(LexicalProjectionConfig {
        target_block_bytes: NonZeroU64::new(1024 * 1024).unwrap(),
        max_block_bytes: NonZeroU64::new(2 * 1024 * 1024).unwrap(),
        ..Default::default()
    })
    .write(
        &root.0,
        1,
        None,
        11,
        13,
        documents.iter(),
        &SearchAnalyzerLexicon::empty(),
    )
    .unwrap();
    (reader, documents)
}

#[test]
fn lexical_query_posting_allocations_grow_with_buffers_not_entries() {
    let _serial = crate::test_allocation::serial();
    for count in [8, 128, 4096] {
        let root = OwnedDirectory::new("posting-allocations");
        let (reader, _) = allocation_fixture(&root, count, "");
        let blocks = reader.posting_blocks("needle").count();
        let mut stream = TermPostingStream::new(&reader, "needle");
        let (visited, allocations) = count_allocations(|| {
            let mut visited = 0;
            while let Some(ordinal) = stream.peek_ordinal().unwrap() {
                assert_eq!(ordinal, visited as u64);
                stream.take().unwrap();
                visited += 1;
            }
            visited
        });
        assert_eq!(visited, count);
        eprintln!("posting entries={count}, blocks={blocks}, allocations={allocations}");
        // Frame decoding and stream-buffer growth may allocate per block.
        // Copying a term for every entry would exceed this buffer allowance.
        assert!(allocations <= blocks * 16);
    }
}

#[test]
fn lexical_query_document_lookup_borrows_skipped_and_matching_ids() {
    let _serial = crate::test_allocation::serial();
    let root = OwnedDirectory::new("document-lookup-allocations");
    let (reader, documents) =
        allocation_fixture(&root, 512, &format!("-\u{00e9}{}", "x".repeat(128)));
    assert_eq!(
        reader
            .manifest
            .blocks
            .iter()
            .filter(|block| block.kind == BlockKind::Documents)
            .count(),
        1
    );
    let mut lookup = DocumentLookup::new(&reader, None);
    assert_eq!(lookup.get(0).unwrap().0, documents[0].id);
    let bytes_read = lookup.bytes_read;
    let (_, allocations) = count_allocations(|| {
        // Both the skipped IDs and the requested IDs remain in the loaded block.
        for ordinal in [255, 256, 511] {
            let (id, length) = lookup.get(ordinal).unwrap();
            assert_eq!(id, documents[ordinal as usize].id);
            assert_eq!(length, 1);
        }
    });
    assert_eq!(lookup.bytes_read, bytes_read);
    eprintln!("document lookups=3, skipped IDs=508, allocations={allocations}");
    assert_eq!(allocations, 0);
}

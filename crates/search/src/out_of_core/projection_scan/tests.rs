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
use crate::{SearchProjectionKind, SearchProjectionRow};

fn budget() -> SearchProjectionScanBudget {
    SearchProjectionScanBudget {
        max_rows: 16,
        max_payload_bytes: 16384,
        max_encoded_record_bytes: 4096,
    }
}

fn row(id: &str, body: &str) -> SearchProjectionRow {
    SearchProjectionRow {
        kind: SearchProjectionKind::Memory,
        external_id: id.into(),
        title: "title".into(),
        body: body.into(),
        embedding: Some(vec![1.0, -0.0]),
        source_id: None,
        metadata: BTreeMap::from([("space_id".into(), "team".into())]),
    }
}

fn collect(
    reader: &SearchOutOfCoreReader,
) -> (SearchProjectionScanReport, BTreeMap<String, SearchDocument>) {
    let mut documents = BTreeMap::new();
    let report = reader
        .visit_projection_documents(budget(), &RuntimeTaskContext::default(), &mut |document| {
            assert!(documents
                .insert(document.id.clone(), document.clone())
                .is_none());
            Ok(())
        })
        .unwrap();
    (report, documents)
}

#[test]
#[cfg(feature = "full-text-search")]
fn projection_scan_retains_pin_and_exhausts_replacements_deletes_and_reopen() {
    let path = super::super::tests::test_dir("projection-scan-visibility");
    let initial = [row("a", "old a"), row("b", "old b")];
    let mut writer = SearchOutOfCoreGenerationWriter::create(&path, Default::default()).unwrap();
    for row in &initial {
        writer.push(row.clone().into_document()).unwrap();
    }
    writer.finish().unwrap();
    let pinned = SearchOutOfCoreReader::open(&path).unwrap();
    let before = collect(&pinned);
    let replacement = row("a", "new a");
    let extra = row("c", "new c");
    SearchOutOfCoreGenerationWriter::prepare_delta(
        &pinned,
        crate::SearchProjectionDelta {
            upserts: vec![replacement.clone(), extra.clone()],
            deletes: vec!["memory:b".into()],
            ..Default::default()
        },
        Default::default(),
    )
    .unwrap()
    .finish()
    .unwrap();
    let current = SearchOutOfCoreReader::open(&path).unwrap();
    assert_eq!(collect(&pinned), before);
    let actual = collect(&current);
    let expected = [replacement, extra]
        .into_iter()
        .map(|row| {
            let document = row.into_document();
            (document.id.clone(), document)
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(actual.1, expected);
    for (id, document) in &expected {
        let bits = |document: &SearchDocument| {
            document
                .embedding
                .as_ref()
                .unwrap()
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>()
        };
        assert_eq!(bits(&actual.1[id]), bits(document));
    }
    assert_eq!(actual.0.document_count, 2);
    assert_eq!(
        actual.0.payload_bytes,
        expected
            .values()
            .map(|document| payload_bytes(document).unwrap())
            .sum::<usize>()
    );
    assert_eq!(actual.0.artifact_generation, Some(current.generation()));
    assert_ne!(actual.0.artifact_generation, before.0.artifact_generation);
    crate::SearchOutOfCoreGenerationWriter::prepare_delta(
        &current,
        crate::SearchProjectionDelta {
            deletes: expected.keys().cloned().collect(),
            ..Default::default()
        },
        Default::default(),
    )
    .unwrap()
    .finish()
    .unwrap();
    let empty = SearchOutOfCoreReader::open(&path).unwrap();
    let empty_scan = collect(&empty);
    assert_eq!(empty_scan.0.document_count, 0);
    assert_eq!(empty_scan.0.payload_bytes, 0);
    assert!(empty_scan.1.is_empty());
    drop(empty);
    // The retained reader still owns the preceding live generation.
    assert_eq!(collect(&current), actual);
    drop(current);
    assert_eq!(
        collect(&SearchOutOfCoreReader::open(&path).unwrap())
            .0
            .document_count,
        0
    );
    drop(pinned);
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn projection_scan_preflights_budgets_and_corruption_before_callback() {
    use std::io::{Seek, SeekFrom, Write};
    let path = super::super::tests::test_dir("projection-scan-refusal");
    let mut writer = SearchOutOfCoreGenerationWriter::create(&path, Default::default()).unwrap();
    writer
        .push(row("a", "complete body").into_document())
        .unwrap();
    writer
        .push(row("b", "another complete body").into_document())
        .unwrap();
    writer.finish().unwrap();
    let reader = SearchOutOfCoreReader::open(&path).unwrap();
    let report = collect(&reader).0;
    let cancellation = hawdb_core::RuntimeCancellationToken::new();
    cancellation.cancel();
    let cancelled = RuntimeTaskContext::without_deadline(cancellation);
    let tiny_memory = RuntimeTaskContext::default()
        .with_memory_reservation(hawdb_core::RuntimeMemoryReservation::new(1, 16384));
    for task in [&cancelled, &tiny_memory] {
        let mut calls = 0;
        assert!(reader
            .visit_projection_documents(budget(), task, &mut |_| {
                calls += 1;
                Ok(())
            })
            .is_err());
        assert_eq!(calls, 0);
    }
    for limited in [
        SearchProjectionScanBudget {
            max_rows: 1,
            ..budget()
        },
        SearchProjectionScanBudget {
            max_payload_bytes: report.payload_bytes - 1,
            ..budget()
        },
        SearchProjectionScanBudget {
            max_encoded_record_bytes: 16,
            ..budget()
        },
    ] {
        let mut calls = 0;
        assert!(reader
            .visit_projection_documents(limited, &RuntimeTaskContext::default(), &mut |_| {
                calls += 1;
                Ok(())
            })
            .is_err());
        assert_eq!(calls, 0);
    }
    let range = reader.segments[0].descriptor.segments[0]
        .payload_range
        .unwrap();
    let mut file = fs::OpenOptions::new()
        .write(true)
        .open(path.join(&reader.manifest.segments[0].payload_file))
        .unwrap();
    file.seek(SeekFrom::Start(range.offset)).unwrap();
    file.write_all(&[0]).unwrap();
    file.sync_all().unwrap();
    let mut calls = 0;
    assert!(reader
        .visit_projection_documents(budget(), &RuntimeTaskContext::default(), &mut |_| {
            calls += 1;
            Ok(())
        })
        .is_err());
    assert_eq!(calls, 0);
    drop(file);
    drop(reader);
    fs::remove_dir_all(path).unwrap();
}

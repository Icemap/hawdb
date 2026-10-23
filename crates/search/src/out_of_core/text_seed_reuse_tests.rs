// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;
use hawdb_core::RuntimeCancellationToken;
use hawdb_executor::{QueryMemoryClass, QueryMemoryLedger};

struct OwnedRoot(PathBuf);

impl Drop for OwnedRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn metadata_reuse_keeps_exact_identity_false_results_and_cancellation() {
    let _serial = crate::test_allocation::serial();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = OwnedRoot(std::env::temp_dir().join(format!("hawdb-metadata-reuse-{nonce}")));
    let mut writer = SearchOutOfCoreGenerationWriter::create(&root.0, Default::default()).unwrap();
    for (id, label) in [("a", "selected"), ("b", "other"), ("c", "selected")] {
        writer
            .push(SearchDocument {
                id: id.into(),
                title: "graph storage".into(),
                content: "memory".into(),
                embedding: None,
                metadata: BTreeMap::from([("labels".into(), format!("[\"{label}\"]"))]),
            })
            .unwrap();
    }
    writer.finish().unwrap();
    let reader = SearchOutOfCoreReader::open(&root.0).unwrap();
    let budget = NonZeroUsize::new(64 * 1024 * 1024).unwrap();
    let ledger = QueryMemoryLedger::new(budget);
    let account = ledger.account(QueryMemoryClass::ExternalRead, "metadata reuse", budget);
    let cancellation = RuntimeCancellationToken::new();
    let task = crate::RuntimeTaskContext::without_deadline(cancellation.clone());
    let predicates = super::super::query_memory::Predicates::new(
        &BTreeMap::from([("labels__in".into(), "[\"selected\"]".into())]),
        None,
    )
    .unwrap();
    let mut lookup = AdmittedMetadataLookup::new(&reader, &account, &task, &predicates.predicates);
    for (id, expected) in [("a", true), ("b", false), ("c", true), ("a", true)] {
        assert_eq!(lookup.matches(id).unwrap(), expected);
        let baseline = crate::test_allocation::live();
        let (_, peak) = crate::test_allocation::measure(|| {
            for _ in 0..8 {
                assert_eq!(lookup.matches(id).unwrap(), expected);
                assert_eq!(lookup.get(id).unwrap().id, id);
            }
        });
        assert_eq!(
            peak, baseline,
            "repeated exact-ID filter and route must not allocate"
        );
    }
    assert!(lookup.get("absent").is_err());
    assert!(lookup.matches("a").unwrap());
    cancellation.cancel();
    assert!(lookup.get("a").is_err());
    assert!(lookup.matches("a").is_err());
    drop(lookup);
    assert_eq!(ledger.snapshot().used_bytes, 0);

    let task = crate::RuntimeTaskContext::default();
    let mut calls = Vec::new();
    let output = reader
        .text_seed_scores_with_context("graph storage", 3, &account, &task, |id| {
            calls.push(id.to_owned());
            Ok(true)
        })
        .unwrap();
    // The analyzer also retains the field phrase alias, so each row has three callbacks.
    assert_eq!(calls, ["a", "a", "a", "b", "b", "b", "c", "c", "c"]);
    drop(output);
    calls.clear();
    let output = reader
        .text_seed_scores_with_metadata_filters_and_observer(
            "graph storage",
            &BTreeMap::from([("labels__in".into(), "[\"selected\"]".into())]),
            3,
            &account,
            &task,
            |id| {
                calls.push(id.to_owned());
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(calls, ["a", "a", "a", "b", "b", "b", "c", "c", "c"]);
    assert_eq!(
        output
            .scores()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["a", "c"]
    );
    drop(output);
    assert_eq!(ledger.snapshot().used_bytes, 0);
}

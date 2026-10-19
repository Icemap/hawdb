// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;
use hawdb_core::{ResourceBudgetError, ResourceBudgetKind, ResourceBudgetScope};

fn assert_spill_clean(path: &Path) {
    let entries = fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(entries, vec![std::ffi::OsString::from("unrelated")]);
    assert_eq!(fs::read(path.join("unrelated")).unwrap(), b"preserve");
}

#[test]
fn matched_span_byte_refusal_preserves_query_usage_and_cleans_spill() {
    let root = test_dir("matched-span-byte-budget");
    let mut index = SearchIndex::open(&root).unwrap();
    for number in 0..3 {
        let mut source = document(number, if number == 2 { "private" } else { "team" });
        source.content = "graph graph".to_string();
        index.upsert(source).unwrap();
    }
    index.checkpoint().unwrap();
    let spill = root.join("matched-span-spill");
    fs::create_dir_all(&spill).unwrap();
    fs::write(spill.join("unrelated"), b"preserve").unwrap();
    let config = SearchOutOfCoreConfig {
        spill_directory: spill.clone(),
        ..Default::default()
    };
    let mut options = options(2, None);
    options
        .metadata_filters
        .insert("space_id".to_string(), "team".to_string());
    let reference = SearchOutOfCoreReader::open_with_config(&root, config.clone()).unwrap();
    let expected = reference
        .search_with_options("graph", None, SearchMode::Text, options.clone())
        .unwrap();
    assert_eq!(expected.result.hits.len(), 2);
    assert!(expected
        .result
        .hits
        .iter()
        .all(|hit| hit.matched_spans.len() >= 2));
    assert!(expected.metrics.candidate_spill_bytes > 0);
    assert_search_parity(
        &index
            .try_search_with_options("graph", None, SearchMode::Text, options.clone())
            .unwrap(),
        &expected.result,
    );
    assert_spill_clean(&spill);

    let first_bytes = expected.result.hits[0]
        .matched_spans
        .iter()
        .map(matched_span_bytes_for)
        .sum::<u64>();
    let last = &expected.result.hits[1].matched_spans;
    let last_bytes = last.iter().map(matched_span_bytes_for).sum::<u64>();
    let requested = matched_span_bytes_for(last.last().unwrap());
    let required = first_bytes + last_bytes;
    let limit = required - 1;
    let refusing = SearchOutOfCoreReader::open_with_config(
        &root,
        SearchOutOfCoreConfig {
            max_matched_span_bytes: NonZeroU64::new(limit).unwrap(),
            ..config.clone()
        },
    )
    .unwrap();
    let expected_error = HawDBError::from(ResourceBudgetError {
        resource: ResourceBudgetKind::MemoryBytes,
        scope: ResourceBudgetScope::Operator,
        owner: "search matched-span hydration".to_string(),
        memory_class: None,
        requested,
        used: required - requested,
        budget: limit,
        budget_option: "SearchOutOfCoreConfig.max_matched_span_bytes",
        message: format!(
            "search matched-span hydration requires {last_bytes} bytes, exceeding {}",
            limit - first_bytes
        ),
    });
    for _ in 0..2 {
        assert_eq!(
            refusing
                .search_with_options("graph", None, SearchMode::Text, options.clone())
                .unwrap_err(),
            expected_error
        );
        assert_spill_clean(&spill);
        let mut first_page = options.clone();
        first_page.limit = 1;
        let output = refusing
            .search_with_options("graph", None, SearchMode::Text, first_page.clone())
            .unwrap();
        assert_search_parity(
            &index
                .try_search_with_options("graph", None, SearchMode::Text, first_page)
                .unwrap(),
            &output.result,
        );
        assert_spill_clean(&spill);
    }
    let exact = SearchOutOfCoreReader::open_with_config(
        &root,
        SearchOutOfCoreConfig {
            max_matched_span_bytes: NonZeroU64::new(required).unwrap(),
            ..config.clone()
        },
    )
    .unwrap();
    let actual = exact
        .search_with_options("graph", None, SearchMode::Text, options.clone())
        .unwrap();
    assert_eq!(actual.result, expected.result);
    assert!(actual.metrics.candidate_spill_bytes > 0);
    assert_spill_clean(&spill);

    // Both guards reject the second span; the existing count guard stays first.
    let first_span_bytes = matched_span_bytes_for(&expected.result.hits[0].matched_spans[0]);
    let count_limited = SearchOutOfCoreReader::open_with_config(
        &root,
        SearchOutOfCoreConfig {
            max_matched_spans: NonZeroUsize::new(1).unwrap(),
            max_matched_span_bytes: NonZeroU64::new(first_span_bytes).unwrap(),
            ..config
        },
    )
    .unwrap();
    assert!(matches!(
        count_limited.search_with_options("graph", None, SearchMode::Text, options).unwrap_err(),
        HawDBError::Storage(message) if message == "search matched-span hydration exceeded 1 spans"
    ));
    assert_spill_clean(&spill);
    drop((reference, refusing, exact, count_limited, index));
    fs::remove_dir_all(root).unwrap();
}

// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn vocabulary() -> Vec<String> {
    (0..32u8)
        .map(|index| {
            format!(
                "word{}{}",
                char::from(b'a' + index / 26),
                char::from(b'a' + index % 26)
            )
        })
        .collect()
}

fn corpus(count: usize, vocabulary: &[String]) -> Vec<SearchDocument> {
    (0..count)
        .map(|index| {
            let mut content = String::new();
            for (term_index, term) in vocabulary.iter().enumerate() {
                // Identical adjacent documents exercise score ties; sparse
                // term membership interleaves the posting cursors.
                if (index / 2 * 7 + term_index * 11) % 13 < 8 {
                    for _ in 0..(index / 2 + term_index) % 3 + 1 {
                        content.push_str(term);
                        content.push(' ');
                    }
                }
            }
            document(&format!("doc-{index:05}"), "", &content)
        })
        .collect()
}

#[test]
fn cursor_selection_preserves_scores_counts_and_callback_order() {
    let root = projection_root("cursor-selection");
    fs::create_dir_all(&root).unwrap();
    let vocabulary = vocabulary();
    let documents = corpus(64, &vocabulary);
    let analyzer = SearchAnalyzerLexicon::default();
    let config = pruning_config(1);
    let reader = LexicalProjectionWriter::new(config)
        .write(&root, 1, None, 11, 13, documents.iter(), &analyzer)
        .unwrap();
    let statistics = crate::TextCorpusStats::from_documents(documents.iter(), &analyzer);
    let queries = [0, 1, 4, 16, 32]
        .into_iter()
        .map(|count| vocabulary[..count].iter().cloned().collect::<BTreeSet<_>>())
        .chain([BTreeSet::from(["absent".to_string()])]);
    for terms in queries {
        let allowed = |id: &str| !id.as_bytes().last().unwrap().is_multiple_of(3);
        let expected_calls = documents
            .iter()
            .flat_map(|document| {
                let matched = document
                    .content
                    .split_whitespace()
                    .filter(|term| terms.contains(*term))
                    .collect::<BTreeSet<_>>()
                    .len();
                std::iter::repeat_n(document.id.clone(), matched)
            })
            .collect::<Vec<_>>();
        let mut expected = documents
            .iter()
            .filter(|document| allowed(&document.id))
            .filter_map(|document| {
                let score = crate::bm25_score(&terms, document, &statistics, &analyzer);
                (score > 0.0).then_some((document.id.clone(), score))
            })
            .collect::<Vec<_>>();
        expected.sort_by(|(left_id, left), (right_id, right)| {
            right.total_cmp(left).then_with(|| left_id.cmp(right_id))
        });
        let windows = [None, Some(0), Some(1), Some(7), Some(64)]
            .into_iter()
            .map(|limit| (false, limit))
            // Pruning is eligible, but the filtered rank window never fills.
            .chain([(true, Some(64))]);
        for (prune, limit) in windows {
            let mut calls = Vec::new();
            let report = reader
                .score_with_term_limit_and_statistics(
                    &terms,
                    &LexicalMiniDelta::default(),
                    config.max_term_bytes,
                    limit,
                    None,
                    prune,
                    |id| {
                        calls.push(id.to_owned());
                        Ok(allowed(id))
                    },
                )
                .unwrap();
            let expected_scores = expected
                .iter()
                .take(limit.unwrap_or(usize::MAX))
                .map(|(id, score)| (id.clone(), score.to_bits()))
                .collect::<BTreeMap<_, _>>();
            let actual_scores = report
                .scores
                .iter()
                .map(|(id, score)| (id.clone(), score.to_bits()))
                .collect::<BTreeMap<_, _>>();
            assert_eq!(
                actual_scores, expected_scores,
                "terms={terms:?} limit={limit:?}"
            );
            assert_eq!(report.matching_document_count, expected.len());
            assert_eq!(calls, expected_calls);
            assert_eq!(report.blocks_skipped, 0);
        }
    }
    drop(reader);
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "developer measurement: compares warmed BM25 scans with 1/4/16/32 terms"]
fn cursor_selection_query_measurement() {
    let root = projection_root("cursor-selection-measurement");
    fs::create_dir_all(&root).unwrap();
    let vocabulary = vocabulary();
    let documents = corpus(4096, &vocabulary);
    let config = LexicalProjectionConfig {
        target_block_bytes: NonZeroU64::new(4096).unwrap(),
        max_block_bytes: NonZeroU64::new(8192).unwrap(),
        ..Default::default()
    };
    let reader = LexicalProjectionWriter::new(config)
        .write(
            &root,
            1,
            None,
            11,
            13,
            documents.iter(),
            &Default::default(),
        )
        .unwrap();
    for count in [1, 4, 16, 32] {
        let terms = vocabulary[..count].iter().cloned().collect();
        let run = || {
            reader
                .score(&terms, &LexicalMiniDelta::default(), Some(10), |_| Ok(true))
                .unwrap()
        };
        let expected = run();
        assert_eq!(run(), expected);
        let mut nanos = Vec::new();
        for _ in 0..21 {
            let started = std::time::Instant::now();
            let report = std::hint::black_box(run());
            nanos.push(started.elapsed().as_nanos());
            assert_eq!(report, expected);
        }
        nanos.sort_unstable();
        eprintln!(
            "cursor-selection terms={count} documents=4096 repetitions=21 median_ns={} min_ns={} max_ns={} matches={} postings={} bytes={}",
            nanos[nanos.len() / 2],
            nanos[0],
            nanos[nanos.len() - 1],
            expected.matching_document_count,
            expected.postings_visited,
            expected.bytes_read,
        );
    }
    drop(reader);
    fs::remove_dir_all(root).unwrap();
}

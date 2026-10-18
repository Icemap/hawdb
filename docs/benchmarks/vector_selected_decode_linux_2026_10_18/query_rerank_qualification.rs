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

//! Compare complete query outputs across sparse, dense and all-selected reranking.

use hawdb::{
    CompressedVectorSearchMode, SearchFusionWeights, SearchMode, SearchOutOfCoreConfig,
    SearchOutOfCoreOutput, SearchOutOfCoreReader, SearchQueryOptions, VectorSearchExecutionOptions,
    VectorSearchKernelPreference,
};
use serde_json::json;
use std::collections::BTreeMap;
use std::error::Error;
use std::io::{self, Write};
use std::num::NonZeroUsize;
use std::path::Path;
use std::time::Instant;

const DOCUMENTS: usize = 100_000;
const DIMENSION: usize = 384;
const SEED: u64 = 0x9940_2026_6f91_31c5;

fn vector(id: u64) -> Vec<f32> {
    let mut state = SEED ^ id.wrapping_mul(0xd6e8_feb8_6659_fd93);
    let mut values = (0..DIMENSION)
        .map(|_| {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            let bits = state.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 40;
            bits as f32 / 8_388_608.0 - 1.0
        })
        .collect::<Vec<_>>();
    let norm = values
        .iter()
        .map(|&value| f64::from(value).powi(2))
        .sum::<f64>()
        .sqrt();
    for value in &mut values {
        *value = (f64::from(*value) / norm) as f32;
    }
    values
}

fn query(
    reader: &SearchOutOfCoreReader,
    embedding: &[f32],
    candidate_limit: usize,
) -> hawdb::Result<SearchOutOfCoreOutput> {
    reader.search_with_options_compressed_vector_projection_execution_options(
        "",
        Some(embedding),
        SearchMode::Vector,
        SearchQueryOptions {
            limit: 10,
            offset: 0,
            rank_window: Some(candidate_limit),
            fusion_weights: SearchFusionWeights::default(),
            metadata_filters: BTreeMap::new(),
            policy_epoch: None,
        },
        CompressedVectorSearchMode::Required,
        VectorSearchExecutionOptions::default().with_kernel(VectorSearchKernelPreference::Scalar),
    )
}

fn hit_bits(output: &SearchOutOfCoreOutput) -> Vec<(String, u64)> {
    output
        .result
        .hits
        .iter()
        .map(|hit| (hit.id.clone(), hit.vector_score.to_bits()))
        .collect()
}

fn profile(path: &Path, candidate_limit: usize, repetitions: usize) -> Result<(), Box<dyn Error>> {
    let reader = SearchOutOfCoreReader::open_with_config(
        path,
        SearchOutOfCoreConfig {
            max_vector_candidates: NonZeroUsize::new(candidate_limit)
                .ok_or("candidate limit must be positive")?,
            ..Default::default()
        },
    )?;
    assert_eq!(reader.document_count(), DOCUMENTS);
    let embedding = vector(u64::MAX - 17);
    let mut warm = query(&reader, &embedding, candidate_limit)?;
    for _ in 0..2 {
        warm = query(&reader, &embedding, candidate_limit)?;
    }
    let expected = hit_bits(&warm);
    let expected_report = format!("{warm:?}");
    assert_eq!(expected.len(), 10);
    let retriever = warm
        .result
        .retrievers
        .iter()
        .find(|r| r.name == "vector")
        .unwrap();
    assert_eq!(
        retriever.backend,
        "hawdb_rabitq_out_of_core_candidate_projection"
    );
    assert_eq!(retriever.candidate_scan_kernel.as_deref(), Some("scalar"));
    // The report sums workers across sequential artifact layers.
    assert!(retriever.candidate_scan_worker_count > 0);
    assert_eq!(retriever.candidate_scan_scored_document_count, DOCUMENTS);
    assert_eq!(retriever.reranked_candidate_count, candidate_limit);
    assert!(retriever.raw_vector_bytes_read > 0);
    assert!(retriever.fallback_reason_codes.is_empty());
    println!(
        "{}",
        json!({
            "phase": "ready", "pid": std::process::id(), "repetitions": repetitions,
            "candidate_limit": candidate_limit, "full_output_debug": expected_report,
            "kernel": retriever.candidate_scan_kernel, "backend": retriever.backend,
            "configured_max_parallelism": 1,
            "layer_worker_count_sum": retriever.candidate_scan_worker_count,
            "candidate_score_source": retriever.candidate_score_source,
            "final_score_source": retriever.final_score_source,
            "scored_vectors": retriever.candidate_scan_scored_document_count,
            "reranked_candidates": retriever.reranked_candidate_count,
            "raw_vector_bytes_read": retriever.raw_vector_bytes_read,
            "admitted_working_bytes": retriever.candidate_scan_admitted_working_bytes,
            "hits": expected,
        })
    );
    io::stdout().flush()?;
    // The driver starts each timed block after warmups and provenance checks.
    let mut command = String::new();
    io::stdin().read_line(&mut command)?;
    if command.trim() != "run" {
        return Err("send 'run' on stdin after recording the ready report".into());
    }
    let mut elapsed = Vec::with_capacity(repetitions);
    for _ in 0..repetitions {
        let started = Instant::now();
        let result = query(&reader, &embedding, candidate_limit)?;
        elapsed.push(started.elapsed().as_nanos());
        assert_eq!(hit_bits(&result), expected);
        assert_eq!(format!("{result:?}"), expected_report);
    }
    println!("{}", json!({"phase": "complete", "elapsed_ns": elapsed}));
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    if cfg!(debug_assertions) {
        return Err("run qualification with --release".into());
    }
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments.len() != 3 {
        return Err(
            "usage: query_rerank_qualification <fixture-path> <candidate-limit> <repetitions>"
                .into(),
        );
    }
    let candidate_limit: usize = arguments[1].parse()?;
    let repetitions: usize = arguments[2].parse()?;
    if !(10..=DOCUMENTS).contains(&candidate_limit) || repetitions == 0 {
        return Err("invalid candidate limit or repetition count".into());
    }
    profile(Path::new(&arguments[0]), candidate_limit, repetitions)
}

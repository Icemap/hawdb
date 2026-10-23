# Complete lexical retrieval and grouped evidence qualification

Local macOS 27.0.1 arm64, Rust 1.97.1, bench/release opt-level 3, thin LTO,
codegen-units 1. Base `be0ddb52cce608fb38dc2c5fad38f6182103a336`, overlay source
manifest `a28e8140ca41cc7e6d085edde696fa2fe9d4aaa5dba32a0548eba250a34cd698`.
The [receipt](../../benches/grouped_evidence/results/2026-10-22-a28e8140.json)
contains every source hash, executable hash, fixture command, raw sample,
work/admission report and whole-process CPU/RSS reading. Documentation and this
receipt were added after the measured source freeze; benchmark/runtime code is
identical. These local results do not attest final-head remote CI or production
acceptance.

Every case discards one warmup per path and alternates path order over seven
samples. Tables show medians in milliseconds. Ratios divide grouped time by
reference time; above 1 is slower. The reference streams every eligible ID,
keeps its maximum score and fully sorts per-group evidence and groups on the
host. Its host state is outside engine query admission, so its resource counters
are not a whole-query memory comparison.

## Scalar stage

This in-memory fixture excludes lexical production and graph traversal. Query
memory is 256 MiB, blocking memory 128 MiB, batch rows 256 and payload 16 MiB.
Exact argument vectors and actual group counts are in the receipt.

| Case | Grouped ms | Reference ms | Ratio |
| --- | ---: | ---: | ---: |
| case-0 | 0.957 | 0.969 | 0.987 |
| case-1 | 6.085 | 6.965 | 0.874 |
| case-2 | 6.329 | 7.148 | 0.885 |
| case-3 | 9.210 | 6.687 | 1.377 |
| case-4 | 0.978 | 1.020 | 0.959 |
| case-5 | 8.391 | 8.418 | 0.997 |
| case-6 | 6.471 | 8.881 | 0.729 |
| case-7 | 1.576 | 0.761 | 2.070 |

Hot groups benefit from bounded evidence and borrowed group-key lookup. The
1000-singleton/256-byte-ID case remains approximately 2.1 times slower than the
lightweight unadmitted host reference. Retaining and admitting all group owners
has real cost; this is not a universal latency improvement. Other unfavorable
samples are retained. Targeted allocation tests, rather than these RSS values,
qualify admission before wide-key/ID allocation and callback ownership.

## Native lexical source and canonical graph eligibility

Each backend runs the same four argument vectors: `(1000,10,0,1,1,1,5,16,7)`,
`(10000,100,90,1,1,8,10,32,7)`, `(10000,1000,0,2,3,8,50,32,7)` and
`(1000,1000,0,1,1,8,50,256,7)`. Fields are corpus size, requested group count,
hot percentage, lexical match stride, graph eligibility stride, M, K, ID width
and repetitions. Requested and actual group counts differ under skew/selectivity;
the receipt reports the latter. Query memory is 512 MiB, blocking memory 128 MiB,
canonical expansion limit 64000 candidates/128 MiB, batches 256 rows/16 MiB.

The query performs one-hop canonical relationship eligibility before grouping.
The disk fixture retains two content layers, a replacement and a tombstone; its
sparse delta spans unchanged base IDs. All three paths agree exactly in every
sample. Full-window controls cover this fixture only because its corpus size is
known; they do not attest general window completeness. Independent full-corpus
score and retained-snapshot correctness belong to the differential regressions.

| Case | Grouped ms | Reference ms | Full-window ms | Ratio |
| --- | ---: | ---: | ---: | ---: |
| disk-0 | 41.136 | 41.168 | 41.975 | 0.999 |
| disk-1 | 4445.163 | 4524.835 | 4429.180 | 0.982 |
| disk-2 | 2230.974 | 2247.993 | 2256.699 | 0.992 |
| disk-3 | 102.650 | 103.302 | 102.392 | 0.994 |
| raw-0 | 22.885 | 22.950 | 22.964 | 0.997 |
| raw-1 | 1948.347 | 2007.466 | 1985.659 | 0.971 |
| raw-2 | 958.230 | 967.085 | 956.137 | 0.991 |
| raw-3 | 35.208 | 34.411 | 35.224 | 1.023 |
| resident-0 | 17.312 | 17.518 | 17.391 | 0.988 |
| resident-1 | 1715.582 | 1732.643 | 1694.738 | 0.990 |
| resident-2 | 1008.664 | 950.872 | 931.438 | 1.061 |
| resident-3 | 27.408 | 26.494 | 28.759 | 1.035 |

Grouping adds little end-to-end cost in these fixtures, but the complete query
still has substantial lexical, routing and graph costs. This comparison does
not isolate those stages or establish linear scaling. In particular, exact
membership probes for overlapping immutable layers read a bounded mapping block
per candidate; this is not a corpus-wide ID cache. Metadata routing/decoding I/O
is excluded from the recorded lexical-scoring byte counters. Repeated warm
reads are not cold-device measurements. No speedup over an incorrect fixed-prefix
collapse is claimed.

Process CPU/RSS includes fixture creation, source preparation, warmup and all
paths, and is cumulative. Peak admitted bytes are conservative query accounting,
not actual heap. The run requests no concurrent qualification job; unrelated host
processes are uncontrolled. Continuous projection publication, foreground writer
latency and host acceptance remain separate qualification boundaries.

Reproduce with the registered `grouped_evidence` and `grouped_evidence_native`
benches and the exact receipt argument vectors. See the
[execution contract](../specs/GROUPED_RETRIEVAL_SPEC.md) for semantics, budgets,
unsupported requests and report interpretation.

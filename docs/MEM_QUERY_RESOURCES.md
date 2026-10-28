# Mem query resource qualification

This manual benchmark accompanies #1287. It covers the embedded query phases
used by Mem candidate ranking and graph expansion. It does not measure a whole
Mem endpoint, embedding provider, durable storage, or a production cutover.

## Reproduce a cell

Use the pinned toolchain and one fresh process per cell:

```sh
cargo bench --locked --no-default-features --bench mem_query_resources --no-run
# Run the executable Cargo printed:
<benchmark-executable> 100 8192 16384 20 0 ranking
<benchmark-executable> 100 8192 16384 20 0 expand
<benchmark-executable> 100 8192 16384 20 0 mem-ranking
```

Arguments are candidate count, rows per Space, body bytes, repeats, background
writer (0/1), and workload. The fixture has two equally sized Spaces. Ranking
changes an input score between samples while preserving its parameter shape.
Expansion retains a source record while producing one result per neighbor.
`mem-ranking` adds Mem's actual scoped-read predicates, canonical usage columns,
floating point inputs and nullable confidence. It excludes the unchanged host
scorer's decay/authority callback.

Build a separate executable with `--features qualification` to collect Rust
allocation requests and query preparation counters. Instrumented timings must
not be substituted for ordinary release timings. The counting allocator covers
all Rust threads and requested bytes; it does not count foreign heaps,
allocator rounding, or prove physical memcpy traffic.

Each cell prewarms the query, checks the complete candidate ID cohort, and
records raw latency samples, nearest-rank p95, process CPU, process peak RSS,
query-account peaks, scan candidates, intermediate/output payload, and CASE
dispatch counts. CPU includes the background writer when selected. Completed
background writes are reported because faster foreground completion changes
how much work that saturating writer can perform. Compare foreground-only CPU
for equal-work query costs. RSS includes fixture setup and is a process high
water mark, not an isolated per-query peak.

The ordinary query memory ledger continues charging retained graph payload
conservatively even when immutable records are shared. Reduced allocator
traffic therefore does not imply that every ledger peak decreases.

## Retained exploratory evidence

The [manifest](benchmarks/mem_query_resources_2026_10_28/manifest.json),
[combined matrix](benchmarks/mem_query_resources_2026_10_28/matrix.csv), and
compressed raw release/allocation JSONL files in the same directory retain 96
release cells plus 96 instrumented cells. These cover 20/100/512 candidates,
512/8192 rows per Space, 64/16384-byte bodies, and foreground/background modes.
The frozen v2 harness and per-cell executable hashes preserve the measured
protocol. The newly added `mem-ranking` workload has not yet been qualified in
that retained matrix.

These measurements used a shared development workstation; some cells
coincided with compilation. They are exploratory, not a final-head performance
acceptance gate. Representative foreground cells at 8192 rows per Space and
16384-byte bodies are below. Allocation traffic comes from the separate
instrumented run; latency comes from release.

| Phase | Candidates | Baseline p95 ms | Candidate p95 ms | Baseline requested bytes/read | Candidate requested bytes/read |
|---|---:|---:|---:|---:|---:|
| Ranking | 20 | 3.429 | 0.499 | 16,845,446 | 2,441,777 |
| Ranking | 100 | 72.993 | 2.514 | 291,593,407 | 12,005,034 |
| Ranking | 512 | 2110.107 | 15.647 | 6,043,998,754 | 63,006,945 |
| Expansion | 20 | 0.855 | 1.086 | 952,768 | 540,370 |
| Expansion | 100 | 0.780 | 0.741 | 4,695,651 | 2,525,013 |
| Expansion | 512 | 2.166 | 1.904 | 29,138,458 | 15,621,468 |

The small expansion cohort illustrates why allocation savings alone do not
justify a universal latency claim. Mem's native collect/list and complete
search-router measurements remain separate qualification obligations.

The executor constructor test
`scan::binding_tests::expansion_inherits_graph_payload_without_copying_and_detaches_mutations`
audits actual inherited node/relationship record identity at each fanout. It
covers the same three cohort sizes and two payload sizes, then verifies that
mutating one result detaches its records. Its copied inherited payload count
excludes target decoding, scalar value copies, slot-array allocation, and final
result serialization. A production deep-copy control must make that test fail;
a successful restored run proves the guard is active.

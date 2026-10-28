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
protocol. That exploratory matrix does not contain `mem-ranking`; the separately
qualified matched-main canonical matrix is described below.

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

## Actual scoped Mem ranking shape

The canonical harness also includes the actual scoped Mem ranking join. Its
boolean scope comparisons exposed a fallback missed by the simplified shape:
markers folded out of boolean comparisons must retain their exact boolean
bindings, rather than removing a referenced parameter. The complete rebound
plan remains the proof boundary. The scoped-CASE regression test permits
shape-compatible score changes but refuses changed, missing or wrongly typed
scope booleans; compatible template reuse makes zero internal planning calls.

Matched main is `0a214f761f` (HawDB 0.18); the measured engine source is
`22cee11bcb` after the complete partial-folding guard repair.
The [canonical manifest](benchmarks/mem_query_resources_2026_10_28/canonical-manifest.json),
frozen canonical harness and raw preparation JSONL retain six instrumented
cells: 20/100/512 candidates, 512 rows per Space, 64-byte bodies, no background
writer, three measured requests after warmup. These instrumented windows ran separately from release timings after this
task finished compilation and tests. Instrumented timing is not release latency
evidence. Allocation requests include harness/profile bookkeeping as well as
the query; they exclude foreign heaps and allocator rounding.

| Candidates | Baseline requested bytes/read | Candidate requested bytes/read | Baseline allocation calls/read | Candidate allocation calls/read |
|---|---:|---:|---:|---:|
| 20 | 19,128,910 | 2,783,735 | 92,604 | 13,749 |
| 100 | 301,751,014 | 13,115,895 | 1,314,047 | 61,776 |
| 512 | 6,093,997,303 | 66,444,690 | 29,436,912 | 298,396 |

Lower allocation traffic does not imply lower peak memory. At 512 candidates,
process peak requested Rust bytes rise from 11.3 MB to 47.7 MB, including fixture
setup and initial template admission. Live requested bytes immediately before
the measured window rise from 2.50 MB to 7.74 MB, reflecting retained templates.
Both windows retain the same additional 2595 bytes after collecting three
samples. The cache has the configured entry bound, but its additional retained
state and cold preparation peak are real costs.

Preparation metric `cypher_plan_calls` counts outer phase invocations, not every
internal logical-planning call; its delta is three for both variants. The
zero-internal-planning reuse claim comes from the targeted template unit test,
not from relabeling those phase counters. The [canonical release matrix](benchmarks/mem_query_resources_2026_10_28/canonical-matrix.csv)
and compressed raw canonical release JSONL contain all 48 paired cells:
20/100/512 × Space512/8192 × body64/16384 × foreground/background × two variants,
20 requests per cell, zero errors. Each optimized sample reports five CASE
tables and `5 * candidates` lookups. Every background cell completed writes
(range14–63294); foreground duration changes the writer's completed work.

The benchmark uses ConcurrentDatabase, warms shared logical binding/catalog
state, and opens a fresh read transaction per sample. Physical caches are
private to those read forks (`DatabaseReadSnapshot::fork`), so this is not a
persistent physical-cache hit-latency measurement. The separate streaming
facade regression uses one read owner and checks physical-cache correctness.
Both variants use max_plan_cache_entries=16 and one query shape per process;
cache saturation and shape churn are outside the measured protocol.

Foreground reference at Space8192/body16384:

| Candidates | Baseline p95 ms | Candidate p95 ms | Baseline CPU ms/read | Candidate CPU ms/read | Baseline RSS MiB | Candidate RSS MiB |
|---|---:|---:|---:|---:|---:|---:|
| 20 | 94.710 | 0.893 | 5.689 | 0.662 | 581.3 | 577.2 |
| 100 | 92.527 | 3.583 | 84.523 | 3.092 | 583.6 | 588.5 |
| 512 | 2209.826 | 20.160 | 2137.340 | 19.626 | 599.2 | 638.9 |

The 20-row baseline's slow tail is disproportionate to its process CPU cost;
the cause was not traced. Keep the raw observations, and do not interpret its
p95 ratio as an isolated speedup. CPU and allocation work provide independent
context. The 512-row reference reduces CPU markedly but increases RSS.

Dispatch table lookups are measured work, not total expression evaluations or
binary-search comparisons. For this complete unique-key fixture the reference
linear evaluator's branch-key evaluations derive to `5*N*(N+1)/2`; that formula
is an analytical count, not an instrumented baseline counter. No unmeasured
expression-comparison reduction is claimed.

Before the partial-folding repair, the actual streaming regression returned
`[0]` instead of `[0,1]` when a parameter was shared between a folded condition
and simple CASE. AST-derived exact equality guards repair that domain;
nullness remains protected by complete type shapes. Current validation passes
31 plan-cache tests,515 executor tests (16 intentionally ignored),2 facade
regressions and all100 focused/local fuzz targets. The frozen canonical harness
matches the measured source; its old warmup comment is superseded by the cache
lifetime clarification above.

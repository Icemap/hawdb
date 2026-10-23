# Ordinary Mem query concurrency measurement

`mem_query_concurrency` is a manual library benchmark for issue #1193. It uses
ordinary parameterized Mem report writes with point, report or streaming reads.
It does not call the concurrent kernel directly or weaken commit durability.

## Workload and execution

Create a job file with a fresh fixture directory whose parent already exists:

```json
{
  "path": "/tmp/hawdb-mem-query-run-1",
  "writers": 2,
  "readers": 2,
  "writer_iterations": 1000,
  "reader_iterations": 4000,
  "rows": 256,
  "read_kind": "point",
  "out_of_core": false
}
```

Use `point`, `report` and `stream` separately, with materialized and out-of-core
storage. The tool creates a unique-key fixture, checkpoints, reopens and asserts
actual residency. One warmup mutation per writer precedes measurement. Every
writer then increments a distinct key through the ordinary report facade.
Fixed-work readers start at the same barrier. Reads and writes carry explicit
row/payload ceilings through their options/database configuration. Errors are
reported without retry; any error makes the process fail. Successful writes
must survive reopening with exact final values.

```sh
cargo bench --locked --bench mem_query_concurrency -- job.json
cargo bench --locked --features qualification --bench mem_query_concurrency -- instrumented-job.json
bazel run --config=release //:hawdb_bench_mem_query_concurrency -- job.json
```

The `qualification` build enables allocator and actual owner-lock diagnostics;
its latency is diagnostic only. Use the repository bench/release profile without
profile overrides, and record enabled features, source SHA, binary SHA, compiler,
filesystem/storage device and CPU/cgroup conditions. Match these between sides.
Record compilation time separately. Do not run paired timing while compilation,
fuzz or another local benchmark competes for resources.

## Baseline and comparisons

Copy this same benchmark, Cargo bench stanza and the observational publication
instrumentation to the integrated main baseline. Port only the metrics module,
owner guard timing/re-exports and Mem metrics forwarding/read guard type; do not
copy candidate query, admission, snapshot or transaction implementations. Retain
the exact patch and hashes with raw results. Baseline and candidate use distinct
fixture directories, identical logical jobs, features and durability. Alternate
execution order across at least three pairs; report every pair and aggregate,
not just the fastest sample. Run one writer as a regression control as well as
multiple writers/readers. Writer/read counts are explicit rather than inferred
from host CPUs; runtime resource detection still observes actual cgroup limits.
Admission refusal is a failed workload, not successful throughput.

The JSON contains raw per-call start/elapsed intervals, cumulative acknowledged
writes observed at call completion, success/error counts and reopen validation.
Latency quantiles use nearest rank and include failed calls; a run with errors
cannot pass acceptance. Fixed-work readers may finish before writers, so inspect
actual interval overlap and acknowledgement advancement rather than assuming
concurrency from thread count. There is no coordinated-omission correction or
open-loop arrival model. Use complementary bounded-progress regressions for
liveness, and do not infer saturation capacity from this fixed-work workload.

Instrumented output contains before/after owner histograms and Rust allocator
traffic. Subtract histogram buckets and totals; percentile intervals follow the
public bucket definition. Do not subtract lifetime maxima. Updates occur after
unlock and do not hold the measured owner. These are the actual outer Mem owner
intervals; they do not include other mutexes or semantic lock waits. Global
allocator traffic includes harness/thread cleanup and all engine threads;
non-Rust memory and allocator rounding are excluded. Cache hit/miss observations
come from returned writer reports; they are **not** counts of all parses or
preparations. Dedicated preparation evidence remains required.

This workload does not exercise search projections, SQL/Tokio adapters, large
payload borrowing, long-reader reclamation or crash/power-loss schedules. Those
remain separate parts of #1193 qualification. Adding this harness alone does not
satisfy its performance acceptance gate.

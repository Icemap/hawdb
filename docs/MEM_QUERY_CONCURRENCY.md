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
  "out_of_core": false,
  "sustain_readers": true
}
```

Use `point`, `report` and `stream` separately, with materialized and out-of-core
storage. The tool creates a unique-key fixture, checkpoints, reopens and asserts
actual residency. One warmup mutation per writer precedes measurement. Every
writer then increments a distinct key through the ordinary report facade.
All workers start at the same barrier. With `sustain_readers: true`, the configured
reader count is a minimum: readers continue until every fixed-work writer ends.
A completion guard releases them even if a writer panics; join/error validation
still fails the process. The default `false` preserves the original fixed-work
protocol for replay. The driver must enforce a process timeout and retain any
partial output; a timeout or error is a failed workload, never a reduced sample.
Reads and writes carry explicit
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
Use a distinct `CARGO_TARGET_DIR` for each source revision, including baseline
ports; never share the target directory between different checkouts of the same
package. A shared target was observed to report a candidate as Fresh and return
the exact baseline executable. Verify compiler artifact provenance and binary
hashes, and retain rejected build receipts. Matching manifest paths alone do not
prove that candidate sources were compiled. Record compilation time separately.
Do not run paired timing while compilation,
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
cannot pass acceptance. When `sustain_readers` is false, readers may finish before
writers, so inspect actual interval overlap and acknowledgement advancement rather
than assuming concurrency from thread count. Sustained readers perform different
amounts of work when the implementations
have different throughput. Report actual call counts, the overlapping cohort,
and normalized operation costs; total allocation traffic is not comparable as
equal work in that mode. There is no coordinated-omission correction or
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
preparations. Protocol v2 also reports `preparation_before`/`preparation_after`
from the shared Cypher runtime: parse calls/time and plan calls/time, including failures. Plan
preparation includes cache lookup and binding, plus optimization on misses.
These process-wide counters are read before the worker barrier and after all
workers join. They include admission and unprepared execution, unlike returned
writer report cache fields. They exclude SQL, standalone parser users and source
validation that never enters the preparation function. Do not sum parse and
plan timings into elapsed latency, or infer universal query-cache behavior from
this workload. Port the same observational hooks to the baseline and keep v1
and v2 instrumented data distinct.

This workload does not exercise search projections, SQL/Tokio adapters, large
payload borrowing, long-reader reclamation or crash/power-loss schedules. Those
remain separate parts of #1193 qualification. Adding this harness alone does not
satisfy its performance acceptance gate.

The [2026-10-24 v1 comparison](benchmarks/query_first_mem_linux_2026_10_24/README.md)
retains the complete fixed-work matrix, source/build provenance and raw traces.
It shows some tail-latency regressions and does not pass the strict performance
gate. The [v2 sustained-reader comparison](benchmarks/query_first_mem_sustained_linux_2026_10_24/README.md)
adds direct preparation counts, per-worker tails and read/write completion
deciles. Both matrices retain every pair and default synchronous durability;
v2 also has tail regressions and does not establish universal speedup.
Its losslessly packed traces restore to the original JSON hashes using the
included codec, allowing the analysis to be reproduced without discarding
slow runs or large reader cohorts.

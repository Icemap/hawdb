# Concurrent reader/writer qualification

Issue Number: ref #232

Single-writer measurements are controls, not evidence of multi-writer or mixed
reader/writer performance. `ConcurrentDatabase` already admits multiple optimistic
writers and concurrent snapshot readers. Its ordered commit/publication boundary
does not imply a single application writer, and overlapping requests do not by
themselves prove parallel mutation execution.

## CPU selection and manual overrides

The manual Linux qualification freezes the CPU evidence and complete matrix after
both binaries are built and before any timing. It uses:

1. `sched_getaffinity` intersected with visible cgroup cpuset masks;
2. distinct `(physical_package_id, core_id)` pairs among those logical CPUs;
3. the tightest finite CPU quota/period across the current cgroup and its visible
   mount ancestors, supporting v2 `cpu.max` and v1 `cpu.cfs_*`.

Effective CPU capacity is the minimum of the allowed physical-core count and the
quota fractions. The default maximum is `max(1, floor(effective_cpu / 2))`. Writer
counts are 1, powers of two up to the maximum, and the maximum itself. SMT threads
are not extra physical cores. A fractional quota is not rounded upward before
the division. The explicit minimum of one also applies to sub-core quotas.
Missing/malformed required evidence fails rather than falling back to host CPU
count. The report records host/VM-exposed topology and visible ancestors; it
cannot certify constraints hidden outside a cgroup namespace.

For each writer count `W`, default reader counts are `0`, `W`, and `2W`. Thus an
effective eight-core host measures writers `1,2,4`, including `4W/0R`, `4W/4R`
and `4W/8R`. A small runner may only permit `1W`; its receipt explicitly lacks
multi-writer coverage and cannot pass concurrent acceptance.

Manual `--writer-counts` and `--reader-counts` accept comma-separated lists. Either
option enables the concurrent protocol. An omitted list keeps its automatic
selection. Explicit readers are crossed with every selected writer count.
Manual writers may exceed the automatic CPU bound; retain the CPU evidence and
mark that override instead of silently clamping it. Writers must be between 1
and 256 so every writer receives work in the fixed 256-commit fixture. Readers
must be nonnegative. No process affinity or cgroup configuration is changed.

```sh
python3 scripts/run-concurrent-writer-gate.py \
  --baseline-revision 0f27ccb18a9cec229e4f653f49b97421faf2de08 \
  --concurrency-matrix --include-captured-wal \
  --work /path/to/new-build-directory \
  --fixture-parent /path/to/disk-backed-fixtures \
  --output /path/to/new-evidence-directory

python3 scripts/run-concurrent-writer-gate.py \
  --baseline-revision 0f27ccb18a9cec229e4f653f49b97421faf2de08 \
  --writer-counts 1,2,4 --reader-counts 0,4,8 --include-captured-wal \
  --work /path/to/another-build-directory \
  --fixture-parent /path/to/disk-backed-fixtures \
  --output /path/to/another-evidence-directory
```

The `Scheduled Quality` manual writer job exposes the same options through
`writer_concurrency_matrix` (default true), `writer_counts`, `reader_counts`, and
`writer_include_captured_wal`. It remains excluded from scheduled/default CI.
The existing 120-minute job limit is unchanged. Large explicit matrices may not
fit; incomplete evidence never counts as a passing matrix.

## Workload and correctness

Every case completes 256 disjoint-key commits without hidden retries. The six
cases retain memory, durable ungrouped, and durable grouped execution, each with
and without the host-side writer serialization control. Reader threads remain
concurrent in that control. The post-checkpoint and reopened captured-WAL
profiles are separate. Default synchronous durability remains enabled.

Mixed cases update two properties together. Each reader makes 64 fresh snapshot
reads over the complete 256-row dataset, checking ordered unique IDs, matching
property values, monotonic visibility, and that the number of changed rows
equals its snapshot epoch minus the initial epoch. A separate initial snapshot
is retained across the timed workload and verified unchanged afterwards. Final
rows, commit counts, epochs, and durable reopen state are also verified.

Writers and readers share one start barrier and run finite fixed work without
per-operation host coordination. Raw request intervals and reader epochs are
retained. Qualification requires completed reads inside the writer phase,
completed writes inside the reader phase, and reads of intermediate published
epochs. Merely finishing reads before/after all writes is a coverage failure.
These checks demonstrate progress during the same workload, not simultaneous
execution of CPU instructions or a general starvation bound.

Read latency includes snapshot acquisition, query execution and snapshot release;
oracle assertions occur afterwards. Writer commit and transaction latencies stay
separate. Case elapsed time and reported operations/second include completion of
all reader/writer work; they are fixed-work mixed-load rates, not steady-state
saturation throughput. Per-worker data and all raw latency samples are retained.
This small fixture targets commit/snapshot interaction; it does not qualify large
datasets, conflicting writes, sustained open-loop load or every read-plan shape.

## Frozen statistical protocol

Each selected concurrency/profile cohort has one warm-up pair and exactly 40
measured pairs, with alternating baseline/candidate and case order. Both revisions
receive identical benchmark sources. Strict-gate failure does not skip later
cohorts. Correctness errors stop with original failure evidence. CPU constraints
are rechecked at cohort boundaries; a changed constraint invalidates qualification.

For concurrent cohorts, primary endpoints are commit p50/p95/p99 and fixed-work
elapsed time in each of the three non-serialized storage cases. Mixed cohorts
also include read p50/p95/p99: 12 or 21 endpoints respectively. Every endpoint
uses candidate/baseline paired ratios; lower is better. Exact order-statistic
intervals use ranks 11/30 for 40 pairs, selected before timing with a 95%
Bonferroni simultaneous-coverage bound within each cohort. No joint confidence
claim spans different cohorts. Every unrounded upper endpoint must be <= 1.0;
otherwise classify the endpoint as regression or inconclusive and retain it.

The historical no-matrix `1W/0R` control retains six commit p50/p95 endpoints and
ranks 12/29. Its previous receipts are not rewritten. New concurrent endpoints
are declared separately; neither the old control nor a CPU-limited single-writer
run can stand in for mixed-concurrency acceptance. Separate syscall tracing runs
only after all untraced timing and includes setup, reopen and cleanup.

## Local proof and replay

CPU-discovery fixtures are manual and do not add a default CI job:

```sh
python3 scripts/tests/test_concurrent_writer_topology.py
cargo build --locked --bench concurrent_writers
```

The checked-profile benchmark uses 16 commits and eight reads per reader for
local correctness proof; its outputs are labelled `smoke=true` and rejected by
the performance driver. Release qualification uses the normal repository bench
profile, with no per-target optimization or assertion overrides.

## Admission work attribution

Generation-debt directory scans must be attributed separately from WAL barriers
and namespace fences. WAL admission can first evaluate a conservative upper
bound on retained-generation bytes. If that bound still permits the mutation,
measuring the exact inventory cannot make its permission more restrictive.
Refusal diagnostics then evaluate actual retained bytes before reporting the
existing pressure snapshot and reasons. Explicit pressure reports keep their
complete inventory; no cached file identity, free-space allowance, threshold or
durability barrier is substituted. Storage-unit comparisons cover full refusal
snapshots across pressure states, and separate syscall traces verify the work
boundary. Traced latency is not a performance result.

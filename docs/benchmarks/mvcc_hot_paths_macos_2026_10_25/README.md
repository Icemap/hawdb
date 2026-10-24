# Production MVCC hot paths, macOS, 2026-10-25

Issue: [#1241](https://github.com/nowledge-co/hawdb/issues/1241).
Baseline: `80c4b84357757ac6344e90f8fd4b73dbf9bcd0d0` (main, HawDB 0.15.0).
Candidate code: `16cf0e8320fac237d6026c7a84ede91060d49c80`.
The later evidence commit changes documentation and receipts only.

These are fixed-work release diagnostics for production COW MVCC. They show
repeatable reductions in the selected read/COW cases, with mixed synchronized
write results. They do not establish a universal write-throughput improvement,
a strict no-regression gate, end-to-end Mem performance, a lock-duration bound,
a process-memory limit, or power-loss qualification.

## Source and environment

Apple M3 Max, 16 logical CPUs, 128 GiB RAM; macOS 27.0.1;
pinned Rust 1.97.1; locked dependencies and default Cargo features. Both builds
use the repository's ordinary release benchmark profile, with independent target
directories. [metadata.json](metadata.json) records exact source/configuration
hashes, compiler details, build cost and validation. Each paired receipt records
both executable hashes and the identical benchmark source hash.

This is a shared developer host. Other Rust/Bazel compilation continued at
process boundaries; the receipts preserve process names, CPU observations and
load averages. Our storage tests/builds were finished before the accepted
cohorts. There is no CPU affinity, reserved host, device isolation, cache eviction
or cold-cache claim. No test profile, timeout, fixture or assertion was weakened.

## Workloads and reproduction

Copy the candidate's `benches/mvcc_hot_paths.rs` unchanged to a detached baseline
worktree and add its `[[bench]]` registration there. Keep baseline changes as
measurement probes. Build each worktree with an independent target directory:

```sh
CARGO_BUILD_JOBS=8 CARGO_TARGET_DIR=/path/to/independent-target \
  cargo bench --locked --bench mvcc_hot_paths --no-run
/usr/bin/time -l /path/to/independent-target/release/deps/mvcc_hot_paths-<hash>
```

Verify matching harness hashes and output schema `mvcc-hot-paths-v4` before
accepting a comparison. Run the resulting executables sequentially in baseline,
candidate, candidate, baseline order. This receipt contains two such cohorts,
using the same fixed binaries: eight fresh processes and 336 case observations.
Each process has three repetitions of all 14 fixture shapes. A cohort therefore
has six observations per shape per revision. Setup is outside each timed
interval; only Cypher has an explicit warmup query. Thread startup and joining
are included in read/write elapsed time. Persistent reopen checks are outside
the timer and validate every expected row's final value.

- Generic COW replacement: 64/4,096 integer rows; 10,000 replacements, with or
  without a snapshot pinned for each write. A snapshot is released after that
  individual write; this is not a long-lived-reader retention stress test.
- Cypher point reads: one in-memory node; parameterized MATCH/RETURN/LIMIT;
  1/4/8 readers, 2,000 requests per reader, result assertions on every request.
- Relationship mutation: 128 relationships, 0/8,192-byte bodies, 512 targeted
  updates; optional per-write snapshots. The normal storage mutation path is
  included, not just an isolated record clone.
- Persistent graph writes: 1/4/8 optimistic writers, eight transactions each,
  eight disjoint rows per transaction, 4,096-byte property values. Durability
  stays `SyncOnEveryWrite`; explicit benchmark group settings are eight entries,
  1 MiB and 100 microseconds. WAL grouping/fsync counters and transaction
  p50/p95/p99 are retained. Eight transactions per writer are a short diagnostic,
  not a steady-state tail-latency qualification. p95 and p99 coincide for the
  eight-sample single-writer fixture under the harness's floor-rank estimator.

## Accepted results

The first two numeric columns are the initial cohort median elapsed microseconds
per operation (per transaction for writes). Ratios are candidate/baseline;
less than one means less elapsed time. The confirmation column uses its own
baseline and candidate medians. Raw observations are in
[initial](paired_v4_initial.json) and
[confirmation](paired_v4_confirmation.json) receipts; neither cohort is discarded.

| Shape | Initial baseline us/op | Initial candidate us/op | Initial ratio | Confirmation ratio |
| --- | ---: | ---: | ---: | ---: |
| COW replace, 64 rows, unpinned | 0.250 | 0.043 | 0.171 | 0.171 |
| COW replace, 64 rows, pinned | 1.037 | 0.887 | 0.855 | 0.880 |
| COW replace, 4096 rows, unpinned | 1.778 | 0.092 | 0.052 | 0.051 |
| COW replace, 4096 rows, pinned | 8.537 | 7.311 | 0.856 | 0.714 |
| Cypher point read, 1 reader(s) | 39.640 | 34.785 | 0.878 | 0.900 |
| Cypher point read, 4 reader(s) | 26.117 | 21.745 | 0.833 | 0.889 |
| Cypher point read, 8 reader(s) | 18.598 | 13.196 | 0.710 | 0.738 |
| Relationship update, 0 B body, unpinned | 41.727 | 24.981 | 0.599 | 0.677 |
| Relationship update, 0 B body, pinned | 54.861 | 27.832 | 0.507 | 0.507 |
| Relationship update, 8192 B body, unpinned | 58.035 | 55.222 | 0.952 | 0.920 |
| Relationship update, 8192 B body, pinned | 159.176 | 89.031 | 0.559 | 0.921 |
| Synchronized graph transaction, 1 writer(s) | 12828.346 | 15065.484 | 1.174 | 1.040 |
| Synchronized graph transaction, 4 writer(s) | 4298.745 | 4212.708 | 0.980 | 0.860 |
| Synchronized graph transaction, 8 writer(s) | 2178.090 | 2340.994 | 1.075 | 0.940 |

Across the two cohorts, single/four/eight-reader elapsed ratios are
0.878/0.833/0.710 and 0.900/0.889/0.738. Unpinned page replacement removes the
whole-page recount and has ratios near 0.17 (64 rows) and 0.05 (4,096 rows).
Pinned pages still copy their index entries. Relationship record sharing shows
benefits in both empty and wide-payload fixtures, but the wide pinned result
varies substantially (0.559 versus 0.921); do not claim a fixed 44% improvement.

Synchronized write elapsed ratios are 1.174/0.980/1.075 initially and
1.040/0.860/0.940 on confirmation for 1/4/8 writers. Thus the initial eight-writer
slowdown changes direction; single-writer elapsed remains higher. Single-writer
p95 ratios are approximately 1.001 and 0.996. Four-writer p95 is higher in both
cohorts (approximately 1.104 and 1.063). These adverse observations remain part
of the result; this measurement does not establish a write or tail improvement.

Both single-writer sides have eight groups/flushes. Initial four/eight-writer
median group counts are baseline 8/9, candidate 9/9.5; confirmation counts are
baseline 8/9.5, candidate 8/8. All expected 32/64 WAL entries are present. Fsync
times and grouping vary alongside elapsed time; there is no consistent loss of
batching that justifies adding a speculative delay. Source inspection and the
deterministic paused-preparation regression establish that compaction and graph
WAL encoding run outside the sequencer. They do not substitute for measured
write throughput, and this change keeps the existing batching policy.

## Process resources and build cost

Full `/usr/bin/time -l` output is embedded in each process receipt, including
user/system time, peak RSS, context switches and OS counters. RSS is whole-process
high water across every fixture and is not a phase allocation/retention proof.
macOS reported zero block-input/output counters, so these are not usable physical
I/O measurements. No total-memory improvement is claimed.

| Processes | Wall seconds, in execution order within side | Maximum RSS MiB |
| --- | --- | --- |
| Initial baseline | 13.662, 11.997 | 37.23, 37.66 |
| Initial candidate | 11.387, 12.688 | 37.88, 39.89 |
| Confirmation baseline | 13.265, 13.458 | 36.48, 36.61 |
| Confirmation candidate | 12.308, 12.350 | 35.92, 36.42 |

First release builds in independent target directories took 25m25s baseline and
27m45s candidate under concurrent compilation/test load. Candidate v4 harness
rebuild took 1m11s; forced baseline v4 rebuild took 4m00s. These are observed
local build costs, not remote CI speedups. Exact build logs are retained here.

## Exploratory and rejected observations

[Exploratory v3](exploratory_v3.json) used the earlier harness with no sync/group
fields. Process walls were 89.84/98.26/69.00/25.50 seconds while storage verification
and host I/O load changed. Its synchronized write medians were worse (roughly
1.43/1.45/2.13 ratios for 1/4/8 writers). The adverse observations are retained,
but are not a controlled throughput conclusion. Earlier reused-target exploratory
runs were not used for final comparison; independent target directories and
binary/source identities are required for the accepted cohorts.

The initial v4 attempt caught a stale baseline executable still emitting v3,
despite a copied v4 source and Cargo reporting it fresh. Its
[output](excluded_schema_mismatch.json) and
[resources](excluded_schema_mismatch_resources.log) are retained separately and
excluded by the schema guard. Refreshing the benchmark source timestamp and
rebuilding produced the verified v4 executable used in both accepted cohorts.
No source-content change was needed.

## Correctness and remaining evidence limits

The final-source Bazel selection covers storage, root general/concurrent/recovery,
crash matrix, maintained Shiro/fuzz aliases and smoke: **103 targets pass**, 93
cached and ten executed during the serial repeat (2,407.362 seconds wall).
Storage reports 2,192 passed/37 existing ignored; root general 1,909/1 ignored;
root concurrent 176/1 ignored; recovery 187/1 ignored. Cached durations are not
new test execution times. The separate final-source sealed preparation test
passed both disjoint and conflicting writer/reopen scenarios.

The first parallel verification run had test timeouts and checkpoint phase-wait
failures while builds/benchmarks overlapped. [Its complete client output](initial_parallel_bazel.log)
and failing target logs under `initial_failures/` remain as evidence. The
[same selection repeated with one test process](final_serial_bazel.log) retained
the ordinary checked profile, cases, assertions and ceilings. Only scheduling
changed (`--jobs=4 --local_test_jobs=1`); these first-run failures are not counted
as passes. Cargo storage also passed separately.

The [final manual MVCC campaign](final_mvcc_campaign.log) passed 13,384 complete
cases in 30,011 ms after 1m59s incremental compilation. The earlier campaign
passed 40,717 cases/30,000 ms; case rate is not a performance comparison under
these different host loads. Strict all-workspace/all-target/all-feature Clippy,
formatting, minimal-feature native compilation, and staged HawkEye hooks passed.
Full-tree `prek run --all-files` still reports 675 historical invalid license
headers across 4,116 files, none in the 38-file code commit. Repository policy
separates that debt from staged validation; existing notices were preserved.

No target-specific code changed. There is no new native Windows, browser WASM,
real-device power-loss/reordering or platform synchronization qualification in
this receipt. Existing recovery/crash tests and byte-identical WAL encoder
regressions preserve their narrower claims. The historical root-lease TLA model
does not prove the new physical-page alias accounting; native page-ownership
regressions and the protocol document state that boundary explicitly.

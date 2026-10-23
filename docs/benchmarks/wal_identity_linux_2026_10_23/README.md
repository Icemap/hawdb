# WAL identity-cache qualification

PR #1181 establishes a grouped captured-WAL commit p50 improvement in the corrected CI cohort below; broader latency and tail improvements remain unproven. The original local measurement is retained in full, including adverse samples. It measured the post-checkpoint control path, which clears captured WAL ownership before timing. It cannot qualify the cached captured-WAL append path.

## Original local control

Baseline: `be0ddb52cce608fb38dc2c5fad38f6182103a336`; candidate: `a824d1ac8650c97f9634647641077105d3e6e0d4`. The identical original harness completed 600 cases, 153,600 commits and 400 reopens: 40 one-writer pairs and five pairs each at four/eight writers, with one labelled warm-up pair per writer count. Build times were approximately 17 minutes for the baseline and 5 minutes 9 seconds for the candidate. Both builds finished before timing.

| Storage | Endpoint | Paired median ratio | Ratio interval |
| --- | --- | ---: | --- |
| Memory | p50 | 1.065744 | [0.966591, 1.134698] |
| Memory | p95 | 1.041904 | [0.794393, 1.352261] |
| Memory | p99 | 1.108847 | Descriptive only |
| Durable, ungrouped | p50 | 0.997869 | [0.883072, 1.071213] |
| Durable, ungrouped | p95 | 0.995360 | [0.648086, 1.285014] |
| Durable, ungrouped | p99 | 1.191983 | Descriptive only |
| Durable, grouped | p50 | 0.977219 | [0.784756, 1.045177] |
| Durable, grouped | p95 | 1.085142 | [0.888351, 1.817592] |
| Durable, grouped | p99 | 1.922181 | Descriptive only |

All six one-writer p50/p95 intervals cross 1.0, so the strict zero-increase gate fails. The grouped durable p99 ratio of 1.922181 is retained, not filtered away. Four/eight-writer results are descriptive and remain in `receipt.json`; they are not pooled with single-writer inference.

This shared Linux host used disk-backed btrfs fixtures. No affinity, exclusive hardware or cache-flush claim is made. A background dependency download overlapped early one-writer pairs; `environment-events.json` records that limitation. The current-main CI cohort is separately declared and must not replace this adverse historical receipt.

## Coverage diagnosis

`ConcurrentDatabase::checkpoint()` publishes a WAL rotation, which clears both `wal_append_file` and `wal_append_owner` in `GraphStore::complete_immutable_root_handoff_inner`. `DurableStore::require_captured_wal_append` performs its cached ownership checks only when that owner exists. The original benchmark checkpoints immediately before timing and does not reopen, so measured commits miss the WAL-cache branch.

The complete one-writer `strace -f -c` controls recorded 30,880 baseline versus 30,784 candidate `statx` calls, only 96 fewer. Both recorded 1,032 `fdatasync` and 1,400 `fsync` calls. These whole-process counts include setup/reopen/cleanup and do not isolate commits. Traced latency is not performance evidence. The small aggregate difference is consistent with the missing hot-path coverage.

## Corrected manual protocol

Keep the original post-checkpoint control unchanged by default. `--reopen-before-measure` closes and reopens only durable fixtures after checkpoint and before timing; it restores captured WAL admission and labels every result. The same configuration, transactions, commit counts, serialized controls, row/epoch assertions and final reopen checks apply to both versions.

The manual writer job accepts `writer_include_captured_wal=true`. Before measurement, it builds the same harness against pinned main `a92f8dfa277eee1558d66baec02e91c427fb83a1` and the candidate. It then completes two separate fixed 40-pair cohorts, each after one warm-up pair: post-checkpoint control, followed by reopened captured WAL. Both cohorts run even if the first strict gate fails; correctness failures stop with their original evidence. The six-endpoint threshold and ranks 12/29 are unchanged within each cohort. Intervals are not pooled or claimed as joint coverage across both cohorts.

After both timing cohorts finish, separate `strace -f -c` executions retain whole-process syscall counts for both revisions and both profiles. These include setup and reopens; traced latency is excluded from the timing receipts.

The corrected CI results below establish one grouped captured-WAL p50 improvement, but neither complete strict gate passes. No tail-latency improvement, power-loss qualification or issue closure is inferred. These measurements are separate from #1177 and do not close #232.

## Artifacts and replay

`protocol.json`, `source.json`, `source.patch`, `receipt.json`, `results.jsonl.gz`, build logs and source manifests describe the historical local control. `measure.py` is its original driver and pins the historical baseline; use only with those sources. `SHA256SUMS` covers the packaged files.

For the corrected source, run from the repository root:

```sh
python3 scripts/run-concurrent-writer-gate.py \
  --baseline-revision a92f8dfa277eee1558d66baec02e91c427fb83a1 \
  --include-captured-wal \
  --work /path/to/new-build-directory \
  --fixture-parent /path/to/disk-backed-fixtures \
  --output /path/to/new-evidence-directory
```

Neither this manual benchmark nor the local fuzz campaigns are added to scheduled/default CI execution.


## Current-main CI comparison

[Run 39890742459](https://github.com/nowledge-co/hawdb/actions/runs/39890742459) compares main `a92f8dfa277eee1558d66baec02e91c427fb83a1` with `f51ead21472a495ad3332ef9fbf11b71acfb2964`. Both cohorts completed: 960 measured cases, 245,760 commits and 640 final durable reopen checks, excluding warm-ups and tracing. Each profile retains all 40 measured pairs, the same root lockfile/configuration and identical harness. Protocol, raw-sample, source-manifest and executable hashes were independently checked after download.

Both strict gates failed. The control has six inconclusive endpoints. The reopened cohort has one passing endpoint (grouped durable p50) and five inconclusive endpoints. No interval wholly exceeds 1.0, but that is not proof of neutrality. Only the grouped captured-WAL p50 establishes an improvement under this per-cohort protocol.

### Post-checkpoint control

| Storage | Endpoint | Baseline median us | Candidate median us | Paired median ratio | Interval | Result |
| --- | --- | ---: | ---: | ---: | --- | --- |
| Memory | p50 | 35.757 | 36.032 | 1.010277 | [0.994179, 1.022246] | inconclusive |
| Memory | p95 | 52.523 | 55.319 | 1.018852 | [0.981607, 1.105076] | inconclusive |
| Durable, ungrouped | p50 | 411.064 | 409.236 | 0.998407 | [0.971306, 1.025292] | inconclusive |
| Durable, ungrouped | p95 | 543.798 | 522.638 | 0.977258 | [0.903819, 1.044167] | inconclusive |
| Durable, grouped | p50 | 428.577 | 430.415 | 1.003972 | [0.963385, 1.044953] | inconclusive |
| Durable, grouped | p95 | 559.320 | 544.403 | 1.000837 | [0.930306, 1.068779] | inconclusive |

### Reopened captured WAL

| Storage | Endpoint | Baseline median us | Candidate median us | Paired median ratio | Interval | Result |
| --- | --- | ---: | ---: | ---: | --- | --- |
| Memory | p50 | 35.727 | 36.001 | 1.007103 | [0.988776, 1.026467] | inconclusive |
| Memory | p95 | 51.397 | 53.344 | 0.986378 | [0.958367, 1.048106] | inconclusive |
| Durable, ungrouped | p50 | 430.567 | 425.967 | 0.986586 | [0.963490, 1.009340] | inconclusive |
| Durable, ungrouped | p95 | 589.927 | 583.156 | 0.992134 | [0.888342, 1.076946] | inconclusive |
| Durable, grouped | p50 | 473.226 | 449.012 | 0.963570 | [0.920975, 0.992942] | pass |
| Durable, grouped | p95 | 599.307 | 594.615 | 0.972555 | [0.917841, 1.012536] | inconclusive |

### Secondary endpoints and syscall evidence

Reopened durable p99 paired median ratios are **1.011338 ungrouped** and **1.112498 grouped**. These descriptive results do not establish a tail improvement and are retained alongside the passing p50. Reopened throughput paired median ratios are 1.002978 ungrouped and 1.034200 grouped; these are also descriptive, not additional acceptance endpoints. Control p99 ratios are 0.980625 ungrouped and 0.972398 grouped. Raw samples and `summary.txt` retain all memory/control values.

| Profile | Call | Baseline | Candidate |
| --- | --- | ---: | ---: |
| Post-checkpoint | statx | 30,841 | 30,745 |
| Post-checkpoint | fsync / fdatasync | 1,392 / 1,032 | 1,392 / 1,032 |
| Reopened | statx | 46,485 | 39,189 |
| Reopened | fsync / fdatasync | 1,436 / 1,032 | 1,436 / 1,032 |

The reopened process makes 7,296 fewer `statx` calls (15.70%), versus only 96 fewer in the control. Synchronization counts are unchanged within each profile. These are whole-process, one-writer traces, including setup/reopens/cleanup; they do not isolate exact commit-only savings or qualify four/eight-writer grouping. Timing samples are untraced. This evidence confirms the intended cache path is exercised, not a general speedup across every append path.

The declared sample counts, ranks and thresholds are unchanged. No rerun was selected for a favorable result. The artifact download was retried after a proxy interruption; the benchmark itself was not repeated. Physical host exclusivity, CPU affinity, cache flushing and power-loss safety are not established by this timing run.

The complete [Actions artifact](https://github.com/nowledge-co/hawdb/actions/runs/39890742459/artifacts/12475770447) retains both executables while available. Permanent source/build/protocol/receipt/raw/syscall evidence is packaged under `ci-39890742459/`; executable hashes remain in each protocol. Recompute the summary with `python3 ci-39890742459/summarize.py ci-39890742459` from this directory.


## Correctness and CI at the measured source

At `f51ead21`, Rust 1.97.1 formatting, strict workspace/all-target/all-feature
Clippy, `prek run --all-files` and commit hooks pass. The default local Bazel
command completes all 98 fuzz targets (92 executed, six cache hits, zero skipped
or failed). The standalone MVCC campaign completes 5,738 cases in 30,004 ms after
a 7m40s build. Compressed logs and the MVCC report are in `validation/`.

Native Linux/macOS/Windows, Prow build/crates/analytics/crash/supply-chain pass. The first
root job reports 67/68 passing targets and one 300.1-second timeout in
`hawdb_mvcc_joined_reads_tests`: seven of its eight cases complete, while
`joined_reads_cover_absent_filtered_deleted_parents_and_off_page_children`
does not finish before the existing limit. No assertion failure appears in the
complete retained log. That is an unresolved runtime qualification failure;
a passing local fuzz campaign does not replace it. Its timeout, cases, assertions
and execution configuration are unchanged. Consult the current PR checks and
their tested SHA for later results; these receipts qualify only the identified
source.

## Integration after measurement

Main `0f27ccb18a9cec229e4f653f49b97421faf2de08` subsequently integrated #1177.
The four benchmark/workflow conflicts were resolved from main's canonical files
with only the captured-WAL extensions reapplied; no duplicate driver or altered
threshold was introduced. The WAL fence also documents its existing symlink
target-identity semantics. The comparison above predates this integration and
does not qualify the merged source against the newer main. Keep its receipts
unchanged, report subsequent CI by SHA, and retain Draft status while the strict
gate and runtime qualification remain unresolved.

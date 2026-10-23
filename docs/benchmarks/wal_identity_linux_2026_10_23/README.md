# WAL identity-cache qualification

PR #1181 has no established end-to-end latency improvement yet. The original local measurement is retained in full, including adverse samples. It measured the post-checkpoint control path, which clears captured WAL ownership before timing. It cannot qualify the cached captured-WAL append path.

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

`ConcurrentDatabase::checkpoint()` publishes a WAL rotation, which clears both `wal_append_file` and `wal_append_owner` in `GraphStore::publish_branch_wal_rotation`. `DurableStore::require_captured_wal_append` performs its cached ownership checks only when that owner exists. The original benchmark checkpoints immediately before timing and does not reopen, so measured commits miss the WAL-cache branch.

The complete one-writer `strace -f -c` controls recorded 30,880 baseline versus 30,784 candidate `statx` calls, only 96 fewer. Both recorded 1,032 `fdatasync` and 1,400 `fsync` calls. These whole-process counts include setup/reopen/cleanup and do not isolate commits. Traced latency is not performance evidence. The small aggregate difference is consistent with the missing hot-path coverage.

## Corrected manual protocol

Keep the original post-checkpoint control unchanged by default. `--reopen-before-measure` closes and reopens only durable fixtures after checkpoint and before timing; it restores captured WAL admission and labels every result. The same configuration, transactions, commit counts, serialized controls, row/epoch assertions and final reopen checks apply to both versions.

The manual writer job accepts `writer_include_captured_wal=true`. Before measurement, it builds the same harness against pinned main `a92f8dfa277eee1558d66baec02e91c427fb83a1` and the candidate. It then completes two separate fixed 40-pair cohorts, each after one warm-up pair: post-checkpoint control, followed by reopened captured WAL. Both cohorts run even if the first strict gate fails; correctness failures stop with their original evidence. The six-endpoint threshold and ranks 12/29 are unchanged within each cohort. Intervals are not pooled or claimed as joint coverage across both cohorts.

After both timing cohorts finish, separate `strace -f -c` executions retain whole-process syscall counts for both revisions and both profiles. These include setup and reopens; traced latency is excluded from the timing receipts.

The corrected CI results are pending. No improvement, strict-gate pass, power-loss qualification or issue closure is inferred from the fixture correction. These measurements are separate from #1177 and do not close #232.

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

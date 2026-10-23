# Final source 823e8e9c qualification

This report covers implementation source `823e8e9c735050dcafb3e0bf024e12f0a46ea140`. Later evidence-only commits do not replace that source receipt. The strict performance acceptance remains **failed**; correctness success and CI success do not waive it.

## Source and CI

All five required Prow checks and Linux/macOS/Windows native and Python checks passed on source 823e8e9c. The PR remains draft and requires independent approval. The root BEP retains 926 events, 84 result actions and 70 passed target summaries. The general unit target executed uncached for 140.883 seconds. Source and deterministic partition ownership establish inclusion of the three admission tests; individual case stdout is unavailable because CI exports local-file BEP references rather than those logs.

## Publication and resource workload

All 126 local Linux arm64 cases completed exact checks: 54 read/resource cases (119,808 verified pinned queries), 18 sustained cases and 54 fixed-publication writer controls. The read-enabled cases require the same query to begin and finish during native preparation while writers remain active and undrained. Disabled controls retain the separate before-drain requirement.

The build took 6m 17s with existing dependencies and the global bench profile. Measurements used the pinned Rust image, four-CPU quota and 6 GiB memory/swap ceiling. No concurrent build, fuzzing or heavy artifact I/O ran during the controlled measurement. This is a shared host VM, not physical exclusive hardware or a cold build.

Observed read/resource boundary RSS and sampled process-lifetime high-water marks ranged from 26,588 to 66,740 KiB. These are process measurements at explicit boundaries, not phase heap, full final-process peak or resolved storage residency. Sustained cases performed all six checkpoints and retained the original attempt cap; more successful publications are different background work and do not establish a throughput gain. Fixed 16-publication controls remain the latency comparison.

Required local verification recorded 98/98 Bazel targets (12 executed, 86 cached; 432.695 seconds) and 40,952 MVCC campaign cases in the requested 30-second execution budget. These do not replace CI-owned ordinary unit coverage.

## Strict single-writer allocations

- [Original baseline run 40014017501](https://github.com/nowledge-co/hawdb/actions/runs/40014017501): both 40-pair profiles complete; nine endpoints pass and three are inconclusive. No endpoint is classified as regression; complete acceptance is nevertheless failed.
- [Current-main run 40014022599](https://github.com/nowledge-co/hawdb/actions/runs/40014022599): both profiles complete; all eight durable endpoints pass and four memory endpoints are inconclusive. Complete acceptance remains failed.

Each allocation verifies 960 cases, 245,760 commits and 640 durable reopens. Preserve the two allocations separately, including every sample and the unrounded simultaneous intervals. Do not pool them or retry for a favorable bound.

## Mixed reader/writer allocation

[Run 40014027487](https://github.com/nowledge-co/hawdb/actions/runs/40014027487) completed all 18 cohorts: 8,640 cases, 2,211,840 commits, 2,396,160 reads and 5,760 durable reopens. Every exact row/epoch, pinned snapshot, overlap, process and reopen check passed. The 324 primary endpoints comprise 109 passes, 206 inconclusive results and nine regressions. All nine regressions are durable read p50:

| Profile / concurrency | Grouped | Median ratio | Simultaneous interval |
| --- | --- | ---: | --- |
| captured-measurement/1w-2r | False | 1.050290525558 | [1.010204413614, 1.119401278053] |
| captured-measurement/1w-2r | True | 1.049809929187 | [1.012976665381, 1.094336259269] |
| captured-measurement/8w-16r | True | 1.018363305443 | [1.000866627090, 1.031486968642] |
| measurement/1w-2r | False | 1.027614319533 | [1.005857965322, 1.074532676561] |
| measurement/1w-2r | True | 1.039691772735 | [1.024688649789, 1.076248217640] |
| measurement/4w-4r | True | 1.012868326476 | [1.000929867853, 1.021686330922] |
| measurement/4w-8r | True | 1.010272462621 | [1.002338000688, 1.033438528091] |
| measurement/8w-16r | True | 1.018495335009 | [1.005150788517, 1.037563569117] |
| measurement/8w-8r | True | 1.017251616779 | [1.005911413856, 1.034401810253] |

The allocation exposes two physical cores and four logical CPUs. Explicit 4/8-writer overrides exceed the automatically selected one-writer limit and include oversubscription; they do not prove CPU-parallel mutation.

The current main baseline is `8e9eab185ef880794d216d99ac6cd57442c511aa`. All three new allocations use candidate writer binary SHA-256 `25a6e6ce3421110245a496ee193b3fc08833012e50d79061bf94d3d7d1592f06`. This differs from the earlier writer binary; retain all earlier adverse results under their original sources. Original-baseline acceptance remains separate from current-main attribution.

## Read-path diagnosis and evidence boundary

`ConcurrentDatabase::begin_read_transaction` uses `CommitSequencer::read_view`, forks the published snapshot, refreshes observation snapshots and finishes the health checks. It does not acquire `DatabasePublicationOwner`. The outer admission fix therefore cannot be claimed to fix this independent concurrent path.

The query still returns all 256 preseeded rows with id/value/mirror and verifies the exact epoch. Explicit read transactions get a fresh private plan cache; same-epoch logical-plan-cache warming is not established. Do not replace the fixture with a point lookup, skip observation freshness or infer growing output cardinality.

A post-hoc diagnostic reconstructs every request duration from its original interval and verifies it against retained samples. It splits requests by writer interval and per-worker epoch progression. For captured 1W/2R, ungrouped advanced-epoch request counts increase from median 98 to 110.5 per process, while same-epoch counts decrease from 28 to 15.5. Its within-stratum p50 ratios are approximately 1.0246 and 0.9730. Captured grouped counts similarly change from 97 to 114 and 29 to 12. These are workload outcomes, not controlled strata or causal attribution. High-contention same-epoch strata also regress, so changed epoch composition does not explain all results.

The next separately predeclared diagnostic preserves the full query, all row/epoch assertions, 256 commits and 64 reads per reader. It adds phase timestamps for snapshot capture, query and snapshot drop using the same overlaid harness on baseline and candidate. It is a local arm64 diagnostic with extra timer overhead; it must not replace the original uninstrumented x86_64 gate or establish a performance waiver.

## Retained evidence

The cumulative [raw artifact index](followup/raw-artifact-index.json) retains all historical failures and new source-scoped datasets. Final local data are under `followup/read-admission-final-v1/`; the three new writer allocations have their run IDs in directory names. Independent full verifiers, raw epoch strata and final local verification receipts are under `followup/admission-final-verification/`. New root CI evidence is under `followup/ci-root-admission/`. Lossless gzip preserves original byte hashes; binaries and database fixtures remain excluded from the evidence tree.

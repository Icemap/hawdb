# Source-scoped publication follow-up

Issue Number: ref #1119, ref #1158, ref #1116

## Scope and acceptance

The writer measurements use commit `7546e2d09e34aa02bf3dfb623865fcec360d3dfb`; the Consumer correction overlays its benchmark/helper/driver on the same production library source (frozen base `0f641a98f` plus the WAL repair). Exact source patches, Cargo features/profiles, binary hashes and protocols are retained. Main `8e9eab185` is integrated in the delivery branch; these historical measurements precede that integration and are not final integrated-main receipts.

The complete strict performance acceptance remains open. Do not replace the failed or inconclusive original datasets, remove endpoints or alter thresholds. Native Linux/macOS and the Prow crate lane passed on the recorded PR context, but the root lane has three failures described below and Windows was still pending when this report was prepared. Final-head checks and integrated-runtime qualification remain required.

## Corrected caller accounting and complete manual cohorts

A 60,000-update diagnostic reaches only Mapping after 100,000 calls (50,000 intake and 50,000 mapping calls). It completes at 120,003 calls (60,001 intake, 60,001 mapping and one close). The helper had confused a call bound with its 100,000-item feed allowance. The fixture mapper always returns Done and emits no delta mutations: its corrected host bound is `2 * feed.items + 1 = 200,001` calls. Feed work/items/bytes, mapping limits, 64 MiB retention, production live-row limit, synchronous durability and acceptance predicates are unchanged. The original incomplete run is retained.

The actual corrected helper separately completed full-source coverage and native publication for that 60,000-update backlog: 120,003 calls, selected E=60,005, all 32 documents verified and the foreground value exactly 60,000. This exercises the formerly unreachable complete-publication path. It is correctness evidence, not sustained or latency acceptance.

The corrected workload completed **18/18** declared sustained cases: three rounds, both shapes, writers 1/4/8, 100,000 writes per case, six ordinary checkpoints per case, 32 quiet publications and the unchanged publication ceiling. Every case passed exact data/epoch/document checks and at least two real post-E advancements while writers were active. No active-writer cap hit was accepted. Actual repeated-progress counts were 58,17,13,26,9,12,6,12,36,6,15,45,34,11,11,27,19,11.

The independent same-binary **54/54** fixed-work controls also completed exact checks. They are finite controls, not sustained acceptance. All their samples and adverse rounds remain retained. The sustained cohort itself used only 5–72,025 staging calls per attempt, never over the old 100,000-call cap: its success cannot be causally attributed to the accounting correction. The separate 60,000-update helper regression establishes that correction. This shared-host experiment is not a starvation-freedom or steady-state throughput proof.

## Paired writer results

Each run has two complete 40-pair cohorts, 960 measured cases, 245,760 commits and 640 durable reopens. All exact assertions passed. Both allocations produced the identical candidate executable SHA-256 `777cb805714f4aaa1429a8f5fa24b119aee06cae4ebfe7c92ea698ade7b4d77b`; their machine measurements are not pooled. Protocol/raw hashes were checked. Original-baseline comparison uses separate version lockfiles and is not causal attribution to this PR alone.

| Comparison | Profile | Storage | Metric | Median ratio | Ratio interval | Result |
| --- | --- | --- | --- | ---: | --- | --- |
| Original a703 | measurement | Memory | p50 | 0.383922 | [0.369721, 0.398395] | pass |
| Original a703 | measurement | Memory | p95 | 0.459025 | [0.407779, 0.478565] | pass |
| Original a703 | measurement | Ungrouped durable | p50 | 0.905365 | [0.874153, 0.924670] | pass |
| Original a703 | measurement | Ungrouped durable | p95 | 0.923536 | [0.885967, 0.981776] | pass |
| Original a703 | measurement | Grouped durable | p50 | 0.938491 | [0.889410, 0.963275] | pass |
| Original a703 | measurement | Grouped durable | p95 | 0.943421 | [0.914341, 0.987877] | pass |
| Original a703 | captured-measurement | Memory | p50 | 0.378801 | [0.368607, 0.400198] | pass |
| Original a703 | captured-measurement | Memory | p95 | 0.443576 | [0.406602, 0.484187] | pass |
| Original a703 | captured-measurement | Ungrouped durable | p50 | 0.928483 | [0.907998, 0.940360] | pass |
| Original a703 | captured-measurement | Ungrouped durable | p95 | 0.930612 | [0.891414, 0.978853] | pass |
| Original a703 | captured-measurement | Grouped durable | p50 | 1.026143 | [0.999526, 1.058380] | inconclusive |
| Original a703 | captured-measurement | Grouped durable | p95 | 1.012025 | [0.955129, 1.075640] | inconclusive |
| Pinned main 0f | measurement | Memory | p50 | 1.004605 | [0.996605, 1.009767] | inconclusive |
| Pinned main 0f | measurement | Memory | p95 | 1.017392 | [1.003705, 1.044332] | regression |
| Pinned main 0f | measurement | Ungrouped durable | p50 | 0.778081 | [0.766121, 0.784979] | pass |
| Pinned main 0f | measurement | Ungrouped durable | p95 | 0.802337 | [0.749465, 0.831366] | pass |
| Pinned main 0f | measurement | Grouped durable | p50 | 0.777840 | [0.768486, 0.798937] | pass |
| Pinned main 0f | measurement | Grouped durable | p95 | 0.799394 | [0.769587, 0.840285] | pass |
| Pinned main 0f | captured-measurement | Memory | p50 | 0.997829 | [0.992290, 1.007611] | inconclusive |
| Pinned main 0f | captured-measurement | Memory | p95 | 0.983992 | [0.961117, 1.011169] | inconclusive |
| Pinned main 0f | captured-measurement | Ungrouped durable | p50 | 0.783487 | [0.767899, 0.801188] | pass |
| Pinned main 0f | captured-measurement | Ungrouped durable | p95 | 0.792309 | [0.771395, 0.839711] | pass |
| Pinned main 0f | captured-measurement | Grouped durable | p50 | 0.801596 | [0.779707, 0.816385] | pass |
| Pinned main 0f | captured-measurement | Grouped durable | p95 | 0.825262 | [0.801807, 0.852300] | pass |

[Original run 39979560713](https://github.com/nowledge-co/hawdb/actions/runs/39979560713): all six post-checkpoint endpoints pass. Captured-WAL grouped p50/p95 remain inconclusive, so the complete gate fails. [Pinned-main attribution 39979562261](https://github.com/nowledge-co/hawdb/actions/runs/39979562261): durable medians are approximately 17–22% lower, but memory post-checkpoint p95 regresses by about 1.7% with its interval above 1.0; the other memory endpoints are inconclusive. No unrelated-code/layout cause has been established.

## Root CI: distinct failures, complete source-bound BEP

ProwJob `75d8ea55-373c-4975-88eb-c7adbb1b3fab`, build `2113655888001634304`, uses refs main `8e9eab185` and PR head `7546e2d09`. Its complete 903-event BEP records 68 test results/summaries and TESTS_FAILED exit 3. This is not a one-hour job deadline: Bazel finished after 1,781.968 seconds.

- `hawdb_mvcc_joined_reads_minimal_tests`: 300.154 seconds of uncached execution, queue time zero, timeout exit 142.
- `hawdb_mvcc_joined_reads_tests`: 300.316 seconds of uncached execution, queue time zero, timeout exit 142.
- `hawdb_unit_general_tests`: 258.290 seconds, 1,866 passed/one failed/one ignored in the failing partition; `pending_idle_owner_blocks_pressure_growth_before_admission` unwraps `BranchBusy { resource: "automatic checkpoint capacity" }`. This is a separate assertion failure.

The Rust action span is 802.889 seconds and overlaps the joined-read execution; this includes cache handling and overlap, not pure local compilation time. Runner totals distinguish 405 remote cache hits, 653 internal actions and 254 processwrapper-sandbox actions. Pod CPU request/limit is 8/32; these declarations do not prove physical host availability or I/O contention. Baseline PR #1195 executed the joined targets successfully in 250.1/251.0 seconds and general in 187.6 seconds, already near the same 300-second joined ceiling. These observations motivate isolation but do not independently prove a scheduling cause.

Both joined targets now retain all eight cases, feature profiles, fixtures and SyncOnEveryWrite, with eight disjoint exact-case shards and the existing 300-second action deadline. The complete checkpoint-headroom namespace runs in a separate required exclusive partition with one libtest thread and its original 900-second partition deadline. This is not physical-machine exclusivity or a measured remote CI speedup. The #1116 success/refusal assertions and retry policy remain unchanged; headroom CREATE errors now record existing owner-phase, actual work progress, wake state, resource and libtest-concurrency diagnostics before returning the original error. No larger deadline or swallowed refusal is introduced, and #1116 remains open. New-source CI must verify these changes.

The checked-in BEP retains every event, result, metric and profile option, but removes client/test/repository environment options from command metadata. `ci-root/publication.json` binds its hash to the complete authorized original and records refs. Raw Pod/service configuration is not published. Complete original metadata remains at the authorized Prow artifact.

## Local qualification and replay

The 7546 frozen-source local campaign passed 98/98 (81 executed, 17 cached), including compilation in 2,033.558 seconds. Invocation `0d4cbf95-5c34-4f9f-ba55-2a6452e1c067`; all 98 XML reports were checked and archived. The manual MVCC campaign completed 23,870 cases in 30,000 ms after 1m 32s compilation. These do not qualify later merged-main source. No ordinary CI-owned unit suite was rerun locally.

The corrected Consumer build took 1m 53s with existing caches, and the standalone actual-helper proof build took 2m 52s. Diagnostic process wall time includes setup and is not latency evidence. All controlled local cohorts ran after local compilation/fuzz stopped. Shared-host/Docker arm64 and GitHub x86_64 measurements are distinct; source Auto residency was not resolved, phase heap was not measured, and callback duration is only a lower bound on outer-owner residency.

`raw-artifact-index.json` records stored/original hashes, lengths and compression for every new raw artifact; binaries and database fixtures are excluded. Failed fixtures remain preserved in local volumes. Use the retained source patches, Cargo artifact JSON and driver protocols; the small diagnostic cannot replace either full cohort or any strict writer endpoint.

## Completed mixed writer/read matrix (7546e2d09)

Run [39979561863](https://github.com/nowledge-co/hawdb/actions/runs/39979561863) completed all 18 declared cohorts: 8,640 measured cases, 2,211,840 commits, 2,396,160 reads and 5,760 durable reopens. Every cohort raw/protocol hash matches its receipt. Timing and multi-writer/mixed coverage are complete, but strict performance acceptance failed. This compares the original historical baseline with candidate `7546e2d09`; it does not isolate this PR from all intervening version changes.

Six high-contention cohorts contain regression endpoints in both post-checkpoint and captured-WAL modes. At 8 writers/16 readers, memory commit p99 paired median ratios are 2.034 and 1.836 respectively; ungrouped durable read p50 ratios are 1.459 and 1.438. Several additional endpoints remain inconclusive. All endpoint intervals, failures and raw samples are retained in `writer-mixed-39979561863/`; `writer-mixed-analysis.json` indexes each cohort without pooling confidence across the matrix. Current-main attribution and integrated-source qualification remain required. No threshold, sample count or endpoint was removed.

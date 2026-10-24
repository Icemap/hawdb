# Read phase diagnostic

This independent local arm64 diagnostic decomposes the unchanged 256-row read query into snapshot acquisition, query and snapshot release. It does not replace the uninstrumented x86_64 acceptance gate, which retains nine regressions. No production implementation change is delivered by this evidence.

## Provenance and coverage

Baseline is main `8e9eab185ef880794d216d99ac6cd57442c511aa`; candidate is implementation `823e8e9c735050dcafb3e0bf024e12f0a46ea140`. Both use identical diagnostic harness bytes and retain 256 writes, 64 reads per reader, all 256 id/value/mirror rows, fresh epochs, initial pinned-snapshot verification, writer/read overlap and durable reopen assertions. The four predeclared cohorts are 1W/2R and 8W/16R under post-checkpoint and reopened-WAL profiles, with four alternating pairs per cohort and the original six cases per process.

All 32 processes, 192 cases, 49,152 commits, 110,592 reads and 128 durable reopens completed exact checks. Every reader contributes 64 samples, and each request's acquisition/query/release durations sum exactly to its total. Extra timer calls perturb the workload; four descriptive pairs have no strict acceptance or causal significance.

Candidate compilation took 8m 39s with existing dependencies. The first attempted baseline build incorrectly reused the candidate binary because archived baseline timestamps were older than the shared Cargo target cache. That build is explicitly invalid, retained and never measured. The corrected baseline uses an independent target directory, recompiles both changed packages, takes 9m 32s and produces a different binary. `accepted-builds.json` and both binary hashes establish the accepted mapping; invalid build logs and audit remain retained.

A launch-time check refused measurement while another host task ran Clippy. No measurement occurred on that refusal. The actual launch and the recorded mid-run inventory found no known host compiler/fuzz process. These are point-in-time observations, not continuous monitoring or physical-host exclusivity. Measurements use the pinned Rust image, global bench profile, four-CPU quota and 6 GiB memory/swap ceiling.

## Durable nonserialized results

The table gives the median of four process p50 values, in microseconds; medians of components need not sum to the median total. All individual paired ratios, including adverse and inconsistent rounds, remain in `analysis.json`.

| Reopened | Writers/readers | Grouped | Revision | Acquire us | Query us | Release us | Total us |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: |
| False | 1/2 | False | baseline | 2.063 | 203.917 | 2.417 | 212.104 |
| False | 1/2 | False | candidate | 2.250 | 208.062 | 2.458 | 218.479 |
| False | 1/2 | True | baseline | 2.083 | 201.917 | 2.458 | 211.625 |
| False | 1/2 | True | candidate | 1.958 | 198.208 | 2.375 | 206.417 |
| False | 8/16 | False | baseline | 3.750 | 614.750 | 4.958 | 625.125 |
| False | 8/16 | False | candidate | 3.708 | 649.333 | 5.042 | 664.021 |
| False | 8/16 | True | baseline | 3.812 | 666.021 | 5.042 | 677.396 |
| False | 8/16 | True | candidate | 3.812 | 627.355 | 5.062 | 637.312 |
| True | 1/2 | False | baseline | 2.167 | 205.000 | 2.458 | 216.562 |
| True | 1/2 | False | candidate | 2.396 | 207.208 | 3.104 | 221.333 |
| True | 1/2 | True | baseline | 2.062 | 200.125 | 2.437 | 209.646 |
| True | 1/2 | True | candidate | 2.167 | 201.833 | 2.458 | 212.042 |
| True | 8/16 | False | baseline | 3.813 | 641.167 | 5.062 | 652.583 |
| True | 8/16 | False | candidate | 3.854 | 634.875 | 5.187 | 646.521 |
| True | 8/16 | True | baseline | 3.854 | 626.188 | 5.104 | 641.105 |
| True | 8/16 | True | candidate | 3.896 | 581.355 | 4.937 | 589.895 |

Across these durable nonserialized cohort/revision groups, the query phase accounts for 94.60%–99.45% of summed request durations. This sums overlapping request wall times, not process CPU or total elapsed time. Snapshot acquisition/release alone cannot account for most observed local query cost. The direction and magnitude of local ratios vary by cohort and pair; do not infer that the remote regressions disappeared or that a particular private lock caused them.

## Next diagnostic boundary

The existing read profile reports cardinalities and resources but does not time parsing, optimization and execution. A separately predeclared diagnostic may add those timings to external source overlays, using a thread-local integer side channel and retaining identical queries/assertions. Such temporary instrumentation must stay outside the production library and all acceptance claims.

Raw data, accepted and rejected build evidence, exact source/script hashes, reproduction scripts and the identical harness are indexed under `followup/read-phase-diagnostic-v1/`. Full source trees, binaries and databases are excluded.

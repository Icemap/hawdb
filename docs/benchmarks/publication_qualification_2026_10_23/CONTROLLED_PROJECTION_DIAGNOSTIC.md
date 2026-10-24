# Controlled projection representation diagnostic

**Disposition: compact-vector implementation selected only as a candidate for complete source qualification; no performance acceptance or regression-fix claim.** The fixed-stack implementation is not selected. All adverse observations remain retained.

[Manual CI run 40035817116](https://github.com/nowledge-co/hawdb/actions/runs/40035817116) completed successfully at experimental head `b93264aad446b43e7cf826ab58c7da6b855455f8`. Production Rust on that branch remains byte-identical to anchor `823e8e9c735050dcafb3e0bf024e12f0a46ea140`; the job creates three external source trees using preserved patches and temporary query-stage instrumentation. No temporary timing API is part of the production candidate.

All three representations use the same global bench profile, Rust 1.97.1, features, harness, 256-row result, six native cases, 64 reads per reader, pinned snapshot assertions and durable-reopen checks. Three independent builds completed before any timed process: map 488.45 seconds, stack 468.75 seconds, vector 500.48 seconds. Each starts from an independent target directory; these are the observed first-build costs for this job, not a universal cold-cache estimate. Six balanced order blocks per cohort produce 72 processes, 432 cases, 110,592 commits, 248,832 reads and 288 durable reopens. Independent raw reconstruction passes every recorded count, duration, percentile, overlap, worker, phase-sum, profile, source-manifest and binary-identity check.

All observed plans are the same `SortExec(NodeProjectionScanExec(LabelScan))`. Every preflight records exactly 256 borrowed node-projection rows. The numeric columnar shortcut requires a predicate and does not consume this unfiltered query. The optimization path is therefore exercised; a measured difference cannot be attributed to a different query, result size or physical plan.

The 1 Hz guest process inventory covers the timed interval with no recorded compiler overlap or inventory errors. This does not establish exclusive physical hardware, constant frequency, absence of all interference or unperturbed production latency: common stage timers and preflight instrumentation remain diagnostic perturbations.

## Complete descriptive comparison

Each value below is the median of the six separately retained candidate/map ratios. Values below one favor the candidate. Raw six-block ratios for read, execution, commit and transaction p50/p95/p99 are in `remote-projection-independent-analysis.json`; these descriptive ratios are not confidence bounds or the unchanged forty-pair strict gate.

| Reopened | Writers/readers | Durable | Grouped | Variant | Read p50 | Read p95 | Read p99 | Execute p50 | Commit p95 | Transaction p95 |
| --- | --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| False | 1/2 | False | False | stack | 0.948143 | 0.932123 | 0.960498 | 0.961621 | 1.029090 | 1.082497 |
| False | 1/2 | False | False | vector | 0.928493 | 0.939153 | 0.939435 | 0.924987 | 1.045342 | 1.086267 |
| False | 1/2 | True | False | stack | 0.978826 | 1.003162 | 1.001357 | 0.990324 | 1.050297 | 1.033191 |
| False | 1/2 | True | False | vector | 1.005879 | 0.988080 | 0.999031 | 1.015924 | 1.060386 | 1.045665 |
| False | 1/2 | True | True | stack | 0.992014 | 0.984041 | 0.947232 | 0.999223 | 1.059915 | 1.006860 |
| False | 1/2 | True | True | vector | 0.982545 | 0.979744 | 1.023054 | 0.976265 | 1.041403 | 1.014527 |
| False | 8/16 | False | False | stack | 0.980904 | 0.979623 | 1.086986 | 0.978046 | 1.988056 | 1.152068 |
| False | 8/16 | False | False | vector | 0.970751 | 1.019451 | 1.001000 | 0.965331 | 1.468580 | 1.278007 |
| False | 8/16 | True | False | stack | 0.980681 | 0.959710 | 1.003843 | 0.978024 | 0.920779 | 0.848448 |
| False | 8/16 | True | False | vector | 0.963868 | 0.956468 | 1.060774 | 0.965959 | 0.881743 | 0.980229 |
| False | 8/16 | True | True | stack | 0.976359 | 0.822664 | 0.988790 | 0.975079 | 0.983005 | 0.965497 |
| False | 8/16 | True | True | vector | 0.964479 | 0.912358 | 1.034418 | 0.964416 | 1.068449 | 1.070023 |
| True | 1/2 | False | False | stack | 0.939247 | 0.941567 | 0.993464 | 0.957354 | 0.993841 | 1.034572 |
| True | 1/2 | False | False | vector | 0.967774 | 0.947443 | 0.970408 | 0.941069 | 1.012949 | 1.002004 |
| True | 1/2 | True | False | stack | 0.982930 | 0.982828 | 0.978901 | 1.001760 | 0.969457 | 0.984106 |
| True | 1/2 | True | False | vector | 0.987259 | 1.011677 | 1.004072 | 0.986900 | 1.033138 | 1.037895 |
| True | 1/2 | True | True | stack | 1.034284 | 1.031233 | 1.114174 | 1.035095 | 1.047017 | 0.990854 |
| True | 1/2 | True | True | vector | 0.979698 | 0.976993 | 0.980474 | 0.992210 | 1.045572 | 1.003499 |
| True | 8/16 | False | False | stack | 0.965468 | 1.017629 | 0.918347 | 0.972290 | 0.753461 | 0.963652 |
| True | 8/16 | False | False | vector | 0.980585 | 0.943966 | 0.867443 | 0.972199 | 0.712739 | 1.087428 |
| True | 8/16 | True | False | stack | 0.994498 | 0.918198 | 0.931530 | 0.984112 | 0.779502 | 0.601991 |
| True | 8/16 | True | False | vector | 0.981008 | 0.975614 | 1.042114 | 0.966418 | 0.799905 | 0.823097 |
| True | 8/16 | True | True | stack | 0.979569 | 0.872071 | 0.953160 | 0.975256 | 0.931453 | 0.811832 |
| True | 8/16 | True | True | vector | 0.970249 | 1.071932 | 0.916291 | 0.970955 | 1.115029 | 1.000648 |

The vector candidate lowers the read-p50 descriptive median in eleven of twelve cohorts, but the post-build ungrouped durable 1W/2R ratio is 1.0059. Reopened grouped durable 8W/16R read p95 is 1.0719 and commit p95 is 1.1150. Post-build memory 8W/16R commit p95 is 1.4686. These tradeoffs and all individual adverse blocks remain evidence against treating the candidate as accepted. Six blocks cannot establish formal regression classification. The fixed-stack candidate also worsens reopened grouped durable 1W/2R read p50/p95/p99; earlier local rejected and contaminated allocations remain separate and are not pooled.

The production candidate retains the original conservative workspace allowance, exact output-row admission, duplicate-alias fallback, cancellation and release-before-callback behavior while replacing only the intermediate preparation map for 1–8 uniquely named simple columns. Actual-source local verification passes all 98 required Bazel targets (58 executed, 40 cached; 1,245.520 seconds including compilation) and 42,511 MVCC cases in 30,000 ms after 14.41 seconds of compilation. Pinned formatting, full strict workspace Clippy and all prek checks pass. The four new ordinary unit cases are compiled but remain CI-owned; no local unit execution claim is made. Delivery still requires final-head runtime CI, the complete publication/resource qualification and unchanged original/current-main strict writer allocations. Source 823e performance evidence and the rejected stack implementation receipts do not qualify a changed runtime.

Raw results SHA-256: `28b92ea6d8824a4aaa03e6b497b6aa5c46cd199094d6695e3db233e599c8c0f7`. The lossless indexed package includes raw measurements, all build streams, source manifests, receipts, complete independent analysis, monitor records, exact fixture patches and the independent verifier. Executable binaries, full source trees and database fixtures are excluded. The temporary root timing patch is diagnostic data only.

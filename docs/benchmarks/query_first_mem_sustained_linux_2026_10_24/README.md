# Sustained-reader ordinary Mem query API comparison

Issue Number: ref #1193

## Scope and status

Complete v2 matrix: 144 processes, 72 alternating A/B pairs, zero query errors and exact durable-reopen checks. This is measurement evidence; it does not complete issue acceptance. The fixed-work v1 report and its regressions remain valid and are retained separately.

Candidate: `c669b8d186574d2cda319089601d630910cfe919`; baseline: `8e9eab185ef880794d216d99ac6cd57442c511aa` plus only the identical benchmark and observational owner/preparation probes. Later test-fixture cleanup is not part of this measured source revision.

Each process uses two readers, one or two writers, 256 rows and the default `SyncOnEveryWrite` durability. Each writer performs 1,000 updates to its own unique key. Each reader performs at least 4,000 calls and continues until all writers finish. Three pairs alternate baseline/candidate order for each entry/residency/writer configuration and each instrumentation mode. Setup, warmup, checkpoint and durable reopen are outside the latency window; process receipts include their cost.

The sustained-reader design keeps read pressure during the writer lifetime. It permits different total read counts and work mixes between A/B runs. Elapsed time is not a fixed-total-work throughput ratio, and raw allocation totals are not equal-work costs.

Host: AMD Ryzen 7 7735HS, 8 physical cores / 16 logical CPUs; Linux 7.1.10-zen1-1-zen, pinned Rust 1.97.1, NVMe/Btrfs, boost enabled. Runtime reports effective parallelism 16 with no detected quota or cpuset cap. No compilation or fuzz was started during the paired timing window. This shared developer host has uncontrolled frequency and external activity; per-process load, memory, swap and page-fault observations are retained.

## Uninstrumented latency

Cells are medians of three paired candidate/baseline ratios, not ratios of pooled quantiles. Below 1 means lower. All pairs, including slow runs, are retained; absolute observations and per-worker tails are in `analysis.json.gz`. Instrumented timings are excluded.

| Read entry | Residency | Writers | Read p95 | Read p99 | Write p95 | Write p99 | Elapsed |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| point | materialized | 1 | 0.606 | 0.609 | 0.969 | 0.988 | 0.781 |
| point | materialized | 2 | 0.564 | 0.657 | 1.318 | 1.037 | 0.754 |
| point | out-of-core | 1 | 0.393 | 0.281 | 0.639 | 0.790 | 0.923 |
| point | out-of-core | 2 | 0.374 | 0.374 | 2.420 | 0.345 | 0.901 |
| report | materialized | 1 | 0.573 | 0.520 | 0.916 | 8.585 | 0.894 |
| report | materialized | 2 | 0.523 | 0.424 | 1.763 | 1.032 | 0.807 |
| report | out-of-core | 1 | 0.457 | 0.312 | 0.945 | 0.156 | 0.804 |
| report | out-of-core | 2 | 0.386 | 0.105 | 1.810 | 1.049 | 0.929 |
| stream | materialized | 1 | 0.810 | 0.773 | 0.940 | 0.666 | 0.839 |
| stream | materialized | 2 | 0.812 | 0.954 | 1.887 | 1.008 | 0.846 |
| stream | out-of-core | 1 | 1.006 | 0.894 | 0.901 | 0.846 | 0.795 |
| stream | out-of-core | 2 | 0.699 | 0.807 | 1.832 | 1.034 | 0.879 |

8 of 12 configurations have at least one median p95/p99 ratio above 1. This matrix does not establish a strict no-regression pass. Improvements in other quantiles or elapsed time cannot establish an across-the-board latency improvement.

The single-writer materialized report case illustrates the variation: write p99 is 31.845 / 6.203 ms (baseline / candidate) in pair 0, 12.261 / 105.251 ms in pair 1, and 8.991 / 104.252 ms in pair 2. All three remain in the paired median. Their read counts differ substantially, and this shared-host experiment does not isolate the cause of the stalls.

## Simultaneous progress and fairness

The overlap cohort includes every read whose call interval intersects the first-to-last writer-call interval. Progress counts use call completion timestamps in ten equal deciles of that writer lifetime. The table shows median overlap cohort size, median overlap-tail ratios, and the minimum number of deciles with both read and write completions across three runs. Nonzero activity in a decile does not bound stalls within it. Baseline overlap cohorts as small as four calls make their p95/p99 unstable; the ratios are descriptive and are not percentile acceptance for those small cohorts.

| Read entry | Residency | Writers | Overlap reads B / C | Overlap p95 ratio | Overlap p99 ratio | Active deciles B / C |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| point | materialized | 1 | 62 / 90351 | 0.000177 | 0.00021 | 5 / 10 |
| point | materialized | 2 | 18 / 138461 | 1.59e-05 | 2.42e-05 | 1 / 10 |
| point | out-of-core | 1 | 28 / 129196 | 9.01e-05 | 9.49e-05 | 6 / 10 |
| point | out-of-core | 2 | 4 / 303764 | 7.64e-06 | 9.71e-06 | 0 / 10 |
| report | materialized | 1 | 768 / 121764 | 0.447 | 0.000405 | 3 / 10 |
| report | materialized | 2 | 2126 / 225399 | 0.481 | 0.385 | 6 / 10 |
| report | out-of-core | 1 | 1530 / 147949 | 0.404 | 0.00266 | 5 / 10 |
| report | out-of-core | 2 | 1614 / 252632 | 0.32 | 0.000451 | 5 / 10 |
| stream | materialized | 1 | 38 / 50187 | 0.000245 | 0.000344 | 7 / 10 |
| stream | materialized | 2 | 8 / 105466 | 3.23e-05 | 4.02e-05 | 0 / 10 |
| stream | out-of-core | 1 | 36 / 34173 | 0.000379 | 0.000441 | 7 / 10 |
| stream | out-of-core | 2 | 38 / 75053 | 4.99e-05 | 6.29e-05 | 3 / 10 |

`worker_latency` retains each writer and reader's p95/p99, maximum call, maximum completion gap and last completion time. `reader_acknowledgement_ranges` samples the harness's global acknowledged-write counter at read completion; it is not a query-visible epoch or a Consumer watermark. Durable reopen verifies final writes. This workload therefore demonstrates ordinary read/write completion overlap, not Consumer catch-up or snapshot-freshness qualification.

## Direct preparation and allocation observations

Qualification builds count calls and nanoseconds at the Cypher parser and shared optimized-plan preparation boundaries. Plan-call time includes lookup/binding work, not only optimizer cache misses. Counters are subtracted over the measurement window. These diagnostic timings are not used in the release latency table. The table gives medians per successful read-or-write call; the A/B call mix differs, so preparation/allocation time and bytes are descriptive, not controlled per-reader/per-writer comparisons.

| Read entry | Residency | Writers | Parse calls B / C | Plan calls B / C | Parse us B / C | Plan us B / C | Allocated bytes B / C |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| point | materialized | 1 | 2.000 / 1.000 | 2.000 / 1.000 | 7.214 / 3.317 | 153.899 / 81.120 | 292128 / 121177 |
| point | materialized | 2 | 2.000 / 1.000 | 2.000 / 1.000 | 8.558 / 3.349 | 173.017 / 82.694 | 339634 / 120954 |
| point | out-of-core | 1 | 2.000 / 1.000 | 2.000 / 1.000 | 7.562 / 3.089 | 134.609 / 43.417 | 304165 / 86135 |
| point | out-of-core | 2 | 2.000 / 1.000 | 2.000 / 1.000 | 9.071 / 3.111 | 141.054 / 45.932 | 360065 / 86054 |
| report | materialized | 1 | 2.000 / 1.000 | 2.000 / 1.000 | 7.726 / 3.414 | 103.952 / 78.321 | 251422 / 121186 |
| report | materialized | 2 | 2.000 / 1.000 | 2.000 / 1.000 | 8.781 / 3.418 | 127.203 / 82.514 | 303329 / 125508 |
| report | out-of-core | 1 | 2.000 / 1.000 | 2.000 / 1.000 | 7.999 / 3.169 | 86.194 / 42.610 | 265856 / 88455 |
| report | out-of-core | 2 | 2.000 / 1.000 | 2.000 / 1.000 | 8.952 / 3.284 | 87.689 / 45.553 | 326096 / 90870 |
| stream | materialized | 1 | 2.000 / 1.000 | 2.000 / 1.000 | 7.099 / 3.248 | 113.552 / 61.623 | 4867617 / 5175776 |
| stream | materialized | 2 | 2.000 / 1.000 | 2.000 / 1.000 | 8.073 / 3.536 | 139.397 / 74.073 | 4457538 / 5157111 |
| stream | out-of-core | 1 | 2.000 / 1.000 | 2.000 / 1.000 | 7.343 / 3.648 | 90.736 / 34.528 | 5297905 / 5569610 |
| stream | out-of-core | 2 | 2.000 / 1.000 | 2.000 / 1.000 | 8.011 / 3.946 | 89.207 / 37.914 | 4853298 / 5519663 |

Allocation counters cover process-wide Rust requested allocations, including benchmark sample-vector growth and thread cleanup. They exclude foreign heaps and allocator rounding and are not RSS. Instrumented sustained readers can allocate more samples; raw totals, reallocation counts and work counts remain in JSON.

## Actual publication-owner locks

Each cell reports baseline / candidate. Mean per-acquisition hold is derived from measured window totals. Percentiles are power-of-two histogram intervals; the displayed interval spans all three runs' percentile bounds. Lifetime maxima are never subtracted. Acquisition counts and all read/write wait/hold buckets are retained in JSON.

| Read entry | Residency | Writers | Write hold mean us B / C | Write hold p95 us B / C | Write hold p99 us B / C | Write wait mean us B / C |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| point | materialized | 1 | 4647.745 / 1657.993 | [2097.153, 8388.608] / [2097.153, 8388.608] | [4194.305, 16777.216] / [2097.153, 16777.216] | 19.730 / 0.049 |
| point | materialized | 2 | 5200.889 / 2563.858 | [2097.153, 16777.216] / [4194.305, 8388.608] | [4194.305, 16777.216] / [8388.609, 16777.216] | 3236.209 / 2450.768 |
| point | out-of-core | 1 | 3097.374 / 1886.840 | [2097.153, 8388.608] / [2097.153, 16777.216] | [2097.153, 16777.216] / [2097.153, 33554.432] | 7.412 / 0.046 |
| point | out-of-core | 2 | 5716.288 / 2629.970 | [4194.305, 8388.608] / [4194.305, 8388.608] | [4194.305, 67108.864] / [4194.305, 16777.216] | 4588.420 / 2323.624 |
| report | materialized | 1 | 755.824 / 2447.573 | [4194.305, 8388.608] / [2097.153, 16777.216] | [4194.305, 8388.608] / [4194.305, 16777.216] | 1405.316 / 0.050 |
| report | materialized | 2 | 1228.699 / 2544.324 | [4194.305, 8388.608] / [4194.305, 8388.608] | [4194.305, 8388.608] / [4194.305, 16777.216] | 3173.861 / 2431.505 |
| report | out-of-core | 1 | 910.931 / 2469.486 | [4194.305, 8388.608] / [4194.305, 8388.608] | [4194.305, 8388.608] / [4194.305, 8388.608] | 1694.915 / 0.051 |
| report | out-of-core | 2 | 1418.717 / 3150.317 | [4194.305, 8388.608] / [4194.305, 16777.216] | [8388.609, 16777.216] / [4194.305, 33554.432] | 3922.046 / 2874.091 |
| stream | materialized | 1 | 5344.363 / 2634.341 | [4194.305, 8388.608] / [2097.153, 8388.608] | [4194.305, 134217.728] / [4194.305, 8388.608] | 32.420 / 0.052 |
| stream | materialized | 2 | 5407.032 / 2544.578 | [4194.305, 8388.608] / [4194.305, 8388.608] | [4194.305, 16777.216] / [4194.305, 16777.216] | 4177.425 / 2394.225 |
| stream | out-of-core | 1 | 5360.861 / 2955.496 | [4194.305, 16777.216] / [4194.305, 8388.608] | [4194.305, 33554.432] / [4194.305, 33554.432] | 28.334 / 0.048 |
| stream | out-of-core | 2 | 6202.958 / 2690.798 | [4194.305, 16777.216] / [4194.305, 8388.608] | [8388.609, 33554.432] / [4194.305, 16777.216] | 5118.379 / 2342.964 |

Candidate writes acquire the owner for workspace capture and publication; baseline report reads also take its write lock through execution. These counts represent real owner intervals, but differing operation attribution prevents calling the aggregate a pure commit-publication service time. Preparation no longer needs to hold the owner for the entire query, but this benchmark does not attribute every remaining write-tail stall.

## Reproducibility and remaining qualification

- `raw-point.tar.gz`, `raw-report.tar.gz` and `raw-stream.tar.gz` retain every workload, per-call trace and process receipt; the adjacent `protocol.json` binds the builds and run matrix. The analyzer validates raw SHA-256, successful write counts, minimum read counts and durable reopen before accepting a process. `SHA256SUMS` covers delivered evidence. Per-worker samples are losslessly encoded as three unsigned varints (acknowledgement delta, elapsed nanoseconds, start delta), with all remaining JSON fields preserved. Packing verified all 144 reconstructed JSON files byte-for-byte against the original SHA-256. Restore the original analyzer inputs with `python3 trace-codec-v2.py restore raw-point.tar.gz raw-report.tar.gz raw-stream.tar.gz --output restored`, then run `python3 analyze-v2-pairs.py restored`.
- Baseline and candidate use isolated build targets, identical benchmark source, source-validated Cargo artifacts and distinct binary hashes. Root and benchmark artifacts were actually rebuilt. Full build receipts and logs record first-build versus reused dependency cost. The host environment file was captured before v1; v2 revision/binary bindings are in `protocol.json`, and runtime CPU/cgroup observations are recorded per process.
- The earlier rejected shared-target executable alias is documented with v1; it is not used here.
- Process resource counters include setup/warmup/reopen; host swap/fault changes can include unrelated processes. Neither is an isolated query resource measurement.
- These observations do not qualify Consumer watermarks, search-projection callback latency, SQL/Tokio performance, long-read conflict-history reclamation or crash/power-loss schedules. Existing focused regression/model/platform suites own those requirements.
- P5 business API fixture removal and final-head CI are still incomplete. Keep PR #1194 in Draft.

## Build receipts

| Binary | Build seconds | SHA-256 |
| --- | ---: | --- |
| baseline-latency | 619.255 | `5069b7ba033e7e35fe62ba442da7fc3cdb9380041aaa6c7bcdc83320ba601d72` |
| baseline-instrumented | 190.575 | `c5e798f3bfe2304b64a3cb9b6ed1fc456542a4ab1deb41d07fbb5b0eb8be63e9` |
| candidate-latency | 178.339 | `b9998f227d4bead8a8f008e7b3f231520f3e73afe3bf88c22f0b880d19f8bc55` |
| candidate-instrumented | 181.226 | `e50a9485000acb1ad75a2e9788f6bf1a0ec12dfffa5d9460153e1cb3ed7d17c3` |

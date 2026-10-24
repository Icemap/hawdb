# Ordinary Mem query API release comparison

Issue Number: ref #1193

## Status

Complete v1 measurement matrix. This is measurement evidence, not full issue acceptance.

Candidate: `0e51caa18c881b08c7e49f55c41fd8d70fc7ed23`; integrated main baseline: `8e9eab185ef880794d216d99ac6cd57442c511aa`.

Three alternating pairs per workload use the same manual benchmark and default `SyncOnEveryWrite` durability. Each process has two readers, one or two writers, 256 rows, 4,000 calls per reader and 1,000 calls per writer. Each writer updates its own unique key. Setup/checkpoint/reopen and warmup precede measurement; exact final values are checked after reopening.

Host: AMD Ryzen 7 7735HS, 8 physical cores / 16 logical CPUs; Linux 7.1.10-zen1-1-zen, Rust 1.97.1, NVMe/Btrfs. Runtime effective parallelism is 16 with no detected quota or cpuset cap. CPU boost is enabled. No compilation or fuzz ran during the paired measurement window. This is a shared developer host; frequency and filesystem activity are not controlled.

## Uninstrumented latency

Each cell is the median of the three candidate/baseline ratios; below 1 is lower. This is not the ratio of pooled quantiles. Raw per-pair absolute observations are retained in `analysis.json.gz`; every raw process trace, job and exit receipt is included in `raw.tar.gz`. Instrumented timings are excluded from this table.

| Read entry | Residency | Writers | Pairs | Read p95 | Read p99 | Write p95 | Write p99 | Elapsed |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| point | materialized | 1 | 3 | 0.713 | 0.968 | 0.508 | 0.415 | 0.518 |
| point | materialized | 2 | 3 | 0.690 | 0.999 | 1.852 | 0.333 | 0.833 |
| point | out-of-core | 1 | 3 | 0.493 | 0.511 | 0.542 | 0.700 | 0.947 |
| point | out-of-core | 2 | 3 | 0.582 | 0.555 | 2.048 | 0.295 | 0.834 |
| report | materialized | 1 | 3 | 0.671 | 0.653 | 0.983 | 0.599 | 0.859 |
| report | materialized | 2 | 3 | 0.654 | 0.538 | 1.903 | 1.260 | 0.878 |
| report | out-of-core | 1 | 3 | 0.552 | 0.308 | 0.967 | 0.476 | 0.749 |
| report | out-of-core | 2 | 3 | 0.526 | 0.341 | 1.839 | 1.040 | 0.784 |
| stream | materialized | 1 | 3 | 0.905 | 0.763 | 0.946 | 0.977 | 0.773 |
| stream | materialized | 2 | 3 | 0.900 | 1.129 | 1.618 | 0.904 | 0.793 |
| stream | out-of-core | 1 | 3 | 0.915 | 0.681 | 1.034 | 0.998 | 0.789 |
| stream | out-of-core | 2 | 3 | 1.010 | 0.885 | 1.746 | 0.772 | 0.876 |

Some latency quantiles regress; this matrix does not establish a strict no-regression pass. Throughput/elapsed gains cannot substitute for tail-latency acceptance. All three pairs, including slow runs, are retained.

Fixed-work readers may finish before writers, and the baseline can delay reads until most writes complete. The JSON includes read latencies for calls overlapping the writer lifetime, the overlapping cohort size, writes completing during reader lifetime and each reader's observed acknowledgement range. Whole-run p95/p99 alone cannot establish simultaneous sustained progress; small overlapping cohorts are not stable percentile estimates.

## Instrumented observations

The qualification build adds actual publication-owner lock histograms and process-wide Rust requested-allocation traffic. These runs are diagnostic, separate from ordinary release latency. Below are median candidate/baseline ratios across three pairs. Window write-hold/wait totals are divided by successful writes before comparison; for the baseline report reader this includes owner intervals for read reports too, so it is not a pure per-write service-time attribution.

| Read entry | Residency | Writers | Pairs | Allocation calls | Allocated bytes | Owner write hold / write | Owner write wait / write |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| point | materialized | 1 | 3 | 0.437 | 0.444 | 0.793 | 0.009 |
| point | materialized | 2 | 3 | 0.487 | 0.517 | 1.125 | 1.756 |
| point | out-of-core | 1 | 3 | 0.628 | 0.632 | 0.905 | 0.009 |
| point | out-of-core | 2 | 3 | 0.781 | 0.796 | 0.895 | 1.041 |
| report | materialized | 1 | 3 | 0.516 | 0.530 | 0.766 | 0.000 |
| report | materialized | 2 | 3 | 0.544 | 0.588 | 0.840 | 0.335 |
| report | out-of-core | 1 | 3 | 0.732 | 0.736 | 0.742 | 0.000 |
| report | out-of-core | 2 | 3 | 0.871 | 0.890 | 0.849 | 0.306 |
| stream | materialized | 1 | 3 | 0.774 | 0.982 | 0.816 | 0.004 |
| stream | materialized | 2 | 3 | 0.740 | 0.977 | 0.888 | 1.079 |
| stream | out-of-core | 1 | 3 | 0.928 | 0.990 | 1.090 | 0.002 |
| stream | out-of-core | 2 | 3 | 0.966 | 0.996 | 0.772 | 0.983 |

Per-acquisition p95/p99 lock values are histogram intervals, not exact times. The JSON retains bucket deltas and percentile bounds for read/write wait/hold; lifetime maxima are not subtracted. Candidate writes acquire the owner both for workspace capture and publication, while baseline report operations hold it once through execution. Acquisition counts therefore differ. Rust allocations include engine workers and harness/thread cleanup, exclude non-Rust heaps and allocator rounding, and are not peak RSS.

## Provenance and limitations

- Baseline and candidate use distinct, source-validated executables. The baseline carries only the identical benchmark and observational owner metrics, not candidate query/transaction implementations.
- Initial shared-target candidate builds returned the baseline executable unchanged and were rejected. The accepted candidate builds use an isolated target directory, actual root/benchmark compilation and distinct executable hashes. Rejected receipts remain retained.
- Complete build receipts, source validation, baseline instrumentation patch, environment, workload manifests, raw per-call JSON and per-run load/exit receipts are retained under `target/query-first-measurement-evidence/`. `analysis.json` contains the raw-file SHA-256 for each process.
- Reported cache hits/misses do not count all preparation. Protocol v2 direct preparation counters are being added and must be measured separately; v1 does not satisfy that evidence requirement.
- This workload does not qualify Consumer watermarks, search-projection callbacks, SQL/Tokio latency, long-reader history pruning or crash/power-loss schedules. Existing regressions and targeted qualification own those boundaries.

## Executable hashes

| Binary | SHA-256 |
| --- | --- |
| `baseline-latency` | `423371c9d32271c8c6c643cd4e30c49b79368c48ce6e29a04639a73f19f41648` |
| `candidate-isolated-latency` | `b0b91ea4345af3e4b4bf8def9765b5bf1a6a090cf89fac56267ebd3c4c8da103` |
| `baseline-instrumented` | `cad4c119160d02e20cff31bfabdf2c7d63c449f75ececd4b884c5cd00640b38b` |
| `candidate-isolated-instrumented` | `bf17a4844f5f36adf019cb1d2d16a8a6e8cd0ee406077b34b0e5bc6fdb4384ac` |

## Raw evidence archive

The [evidence prerelease](https://github.com/nowledge-co/hawdb/releases/tag/evidence-pr-1194-2026-10-24)
retains all 22 original raw files from both reports (63,055,112 bytes), including
adverse results. The archive was downloaded and every member checked against
[the manifest](https://github.com/nowledge-co/hawdb/releases/download/evidence-pr-1194-2026-10-24/raw-manifest.json)
before removing the copies from Git. Archive source:
`a2c19825f268a5495e29a1dc21e13b09cdce1545`; this relocation does not qualify later
code revisions or change the measured revisions above.

From the repository root, restore the original paths with GNU tar before
running either report's existing analysis commands:

```console
evidence_dir="$(mktemp -d)"
gh release download evidence-pr-1194-2026-10-24 --repo nowledge-co/hawdb --pattern query-first-raw-a2c19825.tar --dir "$evidence_dir"
printf '%s  %s\n' 3a01d317c075ea4110c924d897250f73105c420345dcf12f4c736cb0638285dd "$evidence_dir/query-first-raw-a2c19825.tar" | sha256sum --check && tar --extract --keep-old-files --file "$evidence_dir/query-first-raw-a2c19825.tar"
```

The existing checksum indexes still apply; restored raw files are ignored.
This ordinary deletion does not erase blobs retained by historical PR refs.

## Reproduce the analysis

Extract `raw.tar.gz` into an empty directory, then run `python3 analyze-pairs.py <directory>`. The analysis asserts all 144 process outcomes, exact durable reopen, and fixed-work call counts; it retains every pair. `SHA256SUMS` covers the delivered files. See [the harness contract](../../MEM_QUERY_CONCURRENCY.md) for replay conditions. Build logs and receipts include first-build time and all local artifact provenance. This report describes the v1 benchmark at the candidate commit above; later v2 counters and sustained-reader mode require separate measurements.

## Build cost

| Executable | Build wall seconds |
| --- | ---: |
| `baseline-latency` | 509.408 |
| `candidate-isolated-latency` | 449.199 |
| `baseline-instrumented` | 270.886 |
| `candidate-isolated-instrumented` | 207.992 |

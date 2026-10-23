# Local layout measurements

Measured 2026-10-23 on Apple M5 Max, arm64, macOS 27.0.1 (26A434), pinned
Rust 1.97.1, repository-wide release profile, from this PR's implementation on
base `8dd9769f7`. The initial dependency build took 89 seconds with a reused
Cargo target directory; subsequent experiment-only release builds took about
3 seconds. This is neither a cold full build nor CI timing evidence.

Commands (fresh output paths required):

```console
cargo run --release --locked --manifest-path experiments/bftree-mvcc/Cargo.toml -- bench /tmp/bftree-small 16 2
cargo run --release --locked --manifest-path experiments/bftree-mvcc/Cargo.toml -- bench /tmp/bftree-history 512 24
```

Raw reports: [small](measurements/small.json), [history](measurements/history.json).
Each contains all three repetitions, sample counts, p50/p95/p99, totals and
memory observations. Tables below are **medians of the three per-run statistics**,
not percentiles pooled across runs. Layout order rotates in fresh processes;
100-byte values, synchronous complete journal batches, warm reads, in-memory
indexes and local filesystem journals are identical across layouts. Update
batch sizes are 16 and 64 respectively. COW is only a shared-coordinator
component reference, not full production HawDB performance.

## Retained history: 512 keys, 24 overwrite rounds

Latencies are microseconds. Commit values refer to a whole 64-key synchronized
transaction, including construction; they are not per-key times.

| Operation | Versioned p95 / p99 | Hybrid p95 / p99 | COW p95 / p99 |
| --- | ---: | ---: | ---: |
| Current point read | 2.500 / 8.125 | 0.250 / 0.334 | 0.291 / 0.375 |
| Old point read | 1.084 / 2.334 | 1.292 / 5.208 | 0.333 / 0.417 |
| Current full scan | 2091.333 / 2549.791 | 63.042 / 106.625 | 25.584 / 37.625 |
| Old full scan | 2072.833 / 2205.167 | 184.542 / 196.208 | 22.750 / 34.458 |
| Commit batch | 4298.375 / 4426.625 | 4306.625 / 4609.083 | 4424.667 / 8323.667 |

| Resource | Versioned | Hybrid | COW |
| --- | ---: | ---: | ---: |
| Resident / observed peak MiB | 14.375 | 14.484 | 9.328 |
| Reopen median ms | 5.376 | 10.093 | 16.499 |
| Journal bytes | 1466756 | 1466756 | 1466756 |

Hybrid current-scan p95 is about 33 times lower than the simple versioned
layout, with current point p95 about 10 times lower. Historical scans also
benefit from seeking one version per logical head. The tradeoff is real:
historical point p99 is about 2.2 times higher, commit p99 about 4.1% higher,
and replay about 1.9 times slower than versioned. Extra head/history index
writes are not extra journal bytes. The COW reference still scans faster and
uses less resident memory in this bounded working set. These results support
continuing an experiment, not replacing the existing engine backend.

## Small: 16 keys, two overwrite rounds

| Operation | Versioned p95 / p99 us | Hybrid p95 / p99 us | COW p95 / p99 us |
| --- | ---: | ---: | ---: |
| Current point read | 0.208 / 0.250 | 0.167 / 0.167 | 0.084 / 0.125 |
| Old point read | 0.209 / 0.291 | 0.292 / 0.333 | 0.084 / 0.125 |
| Current full scan | 7.541 / 11.458 | 5.167 / 6.167 | 1.666 / 3.208 |
| Old full scan | 8.334 / 11.000 | 5.625 / 6.375 | 0.792 / 1.833 |
| Commit batch | 3867.917 / 3867.917 | 3865.417 / 3865.417 | 3878.083 / 3878.083 |

Only two update batches exist per small run. Its commit p95/p99 are maxima,
not stable tail estimates. Tiny point timings are near the timer resolution.
All scan p99 figures use only 32 samples per process and are sample maxima.
The host was not exclusively reserved for measurement. Scheduling, filesystem
synchronization and cache promotion cause variation;
the raw reports preserve it. The benchmark does not measure mixed concurrent
read/write latency, CPU utilization, storage saturation, large values, GC,
checkpoint costs or out-of-memory index behavior.

## Dependency finding and configuration

An initial run using Bf-Tree 0.5.6's default record read cache panicked in
`mini_page_op.rs::load_base_page_from_buffer` from
`range_scan::promote_or_merge_mini_page`. Source inspection shows that
`try_merge_mini_page` can return `NoSplit` for a clean record-cache mini-page
before loading a base buffer, which the scan path then expects to exist.
No successful timing claim uses that failed configuration. The adapter uses
`read_record_cache(false)` (whole-page read caching) for **both** Bf-Tree
layouts. The alternating point-read/range-scan regression exercises this
configuration, including under the release profile. No dependency source or
repository compilation profile was patched. Native promotion probabilities
still follow Bf-Tree's defaults for the selected profile.

Allocation reductions apply to both comparable paths where applicable:
history seeks compare exact encoded key prefixes without decoding another
logical key; hybrid head reads decode directly from a stack buffer; scan
results reuse their already owned payload buffers. Native scans collect at
most 32 rows before releasing leaf guards, with explicit total-work bounds.

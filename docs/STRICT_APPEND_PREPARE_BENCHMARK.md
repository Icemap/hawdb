# Strict append preparation benchmark

This manual component benchmark measures `AppendState::prepare_transaction`,
which the existing embedded strict-append commit path invokes before WAL
publication. It does not measure journal synchronization, group commit,
checkpoint I/O, end-to-end committed throughput, CPU counters or RSS.

```console
cargo bench --locked -p hawdb-storage --bench strict_append_prepare
```

The benchmark uses the repository-wide release profile. It is not registered
in CI or a scheduled job. Ordinary storage regression tests remain owned by
`//crates/storage:hawdb_storage_tests` and the existing Storage Platform Cargo
job (`cargo test -p hawdb-storage` on macOS/Windows).

## Workload and measurement

- Caller-provided and generated commit-sequence order modes.
- One or 64 rows per batch, each with partition text, a BIGINT order and 128
  payload bytes. All measured writes target a fresh partition.
- Zero or 1024 unrelated tables, using the same order mode as the target.
- Zero or 1024 retained one-row live batches in a different partition.
- Nine warmup preparations followed by 257 timed samples per case in release
  (17 checked-profile samples). The source state remains immutable; output rows,
  exact order keys, mutation counts and retained source counts are checked
  outside the timing interval on every preparation. Prepared-state drop is
  also outside that interval. There are no timing pass/fail thresholds.

Local baseline: main `0f641a98f` plus this identical benchmark harness. Candidate:
this PR's append-preparation changes. Both use pinned Rust 1.97.1 and the same
release profile on Apple M5 Max/arm64, macOS 27.0.1. Five paired repetitions
alternate baseline/candidate order in fresh processes, with 16 cases each.
No test/build processes were started concurrently with measurement; the host
was not exclusively reserved. The first baseline build took 88 seconds and a
candidate rebuild 77 seconds with reused Cargo dependency artifacts; neither
is a clean full-repository build or remote CI timing claim.

[Raw observations and binary SHA-256 identifiers](benchmarks/strict_append_prepare_macos_2026_10_23.json)
retain every repetition. The table gives medians of the five per-process
percentiles, in microseconds; these are not pooled percentiles.

| Mode | Unrelated tables | History batches | Rows | Before p50 / p99 us | After p50 / p99 us |
| --- | ---: | ---: | ---: | ---: | ---: |
| CallerProvided | 0 | 0 | 1 | 0.458 / 1.875 | 0.250 / 1.041 |
| CallerProvided | 0 | 0 | 64 | 8.916 / 11.708 | 8.708 / 11.333 |
| CallerProvided | 0 | 1024 | 1 | 8.166 / 10.208 | 8.917 / 10.459 |
| CallerProvided | 0 | 1024 | 64 | 15.667 / 18.125 | 15.667 / 19.417 |
| CallerProvided | 1024 | 0 | 1 | 120.625 / 142.542 | 0.250 / 0.292 |
| CallerProvided | 1024 | 0 | 64 | 127.292 / 146.917 | 7.541 / 9.750 |
| CallerProvided | 1024 | 1024 | 1 | 129.417 / 145.625 | 8.167 / 10.208 |
| CallerProvided | 1024 | 1024 | 64 | 138.083 / 155.917 | 15.583 / 18.125 |
| CommitSequence | 0 | 0 | 1 | 0.458 / 0.500 | 0.375 / 0.500 |
| CommitSequence | 0 | 0 | 64 | 10.917 / 14.000 | 9.708 / 11.708 |
| CommitSequence | 0 | 1024 | 1 | 7.958 / 9.833 | 0.334 / 0.458 |
| CommitSequence | 0 | 1024 | 64 | 18.000 / 20.917 | 8.875 / 10.958 |
| CommitSequence | 1024 | 0 | 1 | 142.208 / 159.833 | 8.458 / 10.792 |
| CommitSequence | 1024 | 0 | 64 | 153.375 / 177.042 | 17.208 / 24.500 |
| CommitSequence | 1024 | 1024 | 1 | 153.208 / 175.500 | 8.333 / 9.583 |
| CommitSequence | 1024 | 1024 | 64 | 163.458 / 183.042 | 17.375 / 23.084 |

The large-metadata, long-history 64-row preparation improves by approximately
8.9x for caller keys and 9.4x for generated keys at p50. With one table and
1024 old batches, generated single-row preparation changes from 7.958 to
0.334 us: the unrelated-partition history walk is removed. Caller keys still
need their partition history check and remain close to baseline there.
One-table/no-history generated single-row p99 is 0.500 -> 0.500 us;
these sub-microsecond figures are near timer granularity and do not establish
a stable tail-latency regression or gain. See every per-run p95/p99 in the raw
report. Component improvements do not establish a durable commit speedup:
per-commit synchronization may dominate the host's actual workload.

## Preserved proof and remaining costs

Preparation borrows the schema catalog until DDL needs a private copy, and
retains only touched sequence-watermark overrides. Applying the materialized
batch still creates the private metadata required by snapshot isolation; in
particular, the generated-watermark map is copied once when it changes.
Its cost can still grow with the number of generated-order tables.

Generated-order replay requires the exact next table-wide sequence value.
Checkpoint loading verifies that the stored generated watermark equals the
maximum partition watermark. Together, these invariants imply that a valid
next generated value exceeds every old partition watermark. The additional
per-partition live-history lookup is redundant in this mode. Caller-provided
keys keep their complete existing strict partition-order check. Admission,
row/schema validation, checked sequence arithmetic and materialized WAL replay
remain in place. Generated watermarks are updated in place within the private
map rather than allocating/discarding another table-name key for every row.

No API, schema, WAL/checkpoint format, synchronization default or group-commit
activation changes. A failed preparation leaves its source snapshot unchanged.
The new `append_table::tests::preparation` regression family covers mixed
schema creation and both order modes in one batch, repeated writes and
independent sequence overrides, old/new partitions after deep live history,
checkpoint metadata restoration, corrupt recovered sequence gaps, late caller
key regression and failed generated allocation followed by retry. Existing
storage and facade CI owners retain durability/concurrency coverage; this
component benchmark does not replace them.

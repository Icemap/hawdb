# COW snapshot retention qualification

This report qualifies package A of [#1136](https://github.com/nowledge-co/hawdb/issues/1136),
tracked by [#1137](https://github.com/nowledge-co/hawdb/issues/1137), against the
[COW MVCC evolution spec](specs/MVCC_COW_EVOLUTION_SPEC.md).
The audited kernel already implements these mechanisms. This delivery adds
combined lifetime regressions and native test allocation observations; it makes
no production API, conflict-identity, admission-default or disk-format change.
Consumer lifecycle (#1138), read dependency semantics (#1139), serializable
validation (#1140), result ownership (#1008), Search cleanup remediation (#867)
and the complete #232 workload acceptance remain separate work.

## Ownership that callers can rely on

| Owner | Protection and retirement |
| --- | --- |
| `GraphStore::snapshot` | Registers a storage epoch before the snapshot escapes. Descendants inherit the minimum original protection epoch, including after private mutations and savepoint restoration. The last `VersionSnapshotPin` retires its registration. |
| `snapshot_for_read` | Shares canonical COW values, keeps the storage pin and retains a constant conflict baseline rather than the precise historical version map. It is a read path, not a writable transaction workspace. |
| Facade read transaction | Owns the published schema/data view and physical-generation `ReaderPin`. Forks share the facade registration while retaining their own storage snapshots. Destruction drops the store before its facade pin. |
| Writable/queued transaction | `DatabaseTransactionState` retains the source pin through private execution, failed-statement restoration, queue submission and commit/cancellation. Pessimistic first-statement refresh replaces the protected view coherently. |
| Version root | Clones share a history lease; a changed root reserves its complete estimated key/stamp payload before COW/WAL. The map field is destroyed before its lease field refunds capacity. Different roots may share pages and still have separate conservative charges. |
| Durable branch | Existence is persistent metadata. A branch with no open lease remains a GC root until explicit deletion. Reader-pin retirement does not authorize branch deletion. |

Source: [`GraphStore`](../crates/storage/src/store.rs),
[`VersionIndex` and storage pins](../crates/storage/src/version.rs),
[`history leases`](../crates/storage/src/version/history_budget.rs),
[`facade ownership`](../src/api/mod.rs),
[`commit queue`](../src/api/concurrent/coordinator.rs).

An old snapshot preserves canonical values; the version map preserves conflict
evidence. These have distinct owners and counters. The default 64 MiB current
index and shared 256 MiB retained-root budgets cover estimated conflict payload,
not COW data, all snapshots, allocator overhead, cache memory or process RSS.
See the [commit protocol](MVCC_COMMIT_VALIDATION_PROTOCOL.md#shared-retained-history-admission).
Insufficient mandatory version credit refuses before WAL/live publication;
retiring owners and a subsequent safe reclamation permit retry. Old readers are
not automatically cancelled. An idle reader can retain its data/pins without
retaining a foreground task, CPU slot or construction working-memory permit;
later work supplies a fresh admitted execution context.

## Physical reclamation

For one branch, canonical/checkpoint roots retained at checkpoint are the union
of the current generation, the actual previous checkpoint generation, and all
explicitly pinned physical generations. An intermediate published logical view
need not equal the previous checkpoint generation. Required physical relational
row pages, overflow extents and append segments are discovered transitively from
retained manifests. A failed retained-manifest decode keeps evidence and reports
reclamation debt rather than guessing that a file is obsolete.

Source: [`durable/reclamation.rs`](../crates/storage/src/store/durable/reclamation.rs).
Storage conflict reclamation uses the separate oldest storage snapshot epoch in
[`graph_checkpoint.rs`](../crates/storage/src/store/graph_checkpoint.rs).
Releasing a pin enables later reclamation; it is not a synchronous deletion API.

## Executable acceptance map

New facade cases are in
[`snapshot_retention.rs`](../src/api/tests/snapshot_retention.rs), registered in
the existing `//:hawdb_unit_test_binary`. New storage cases are in
[`snapshot_retention_tests.rs`](../crates/storage/src/store/snapshot_retention_tests.rs),
registered in `//crates/storage:hawdb_storage_tests`.

| Acceptance | Actual fixture/assertions |
| --- | --- |
| Mixed schema/data at E | `mixed_snapshot_cohorts_reclaim_exact_roots_after_concurrent_final_drop`: Materialized and OutOfCore; graph plus SQL values, ALTER TABLE, three mixed commits/checkpoints, independent old/middle readers and four concurrent final holders. Old readers keep their complete schema/data; fresh reads see the complete new state. |
| Necessary roots survive, unrelated roots retire | Same case compares the complete canonical/checkpoint filename generation sets at each checkpoint against pinned cohorts plus current/previous roots. It asserts overflow artifacts exist, reads large SQL values and unchanged rows, and opens the unleased durable child after reader retirement and reclamation. This is exact root evidence, not an assertion that every transitive artifact has the same generation number. |
| Descendants/savepoints and ABA | `restored_savepoint_preserves_conflicts_after_delete_reinsert_and_pruning`: private epochs advance past a real delete/reinsert, restoration retains the original floor, checkpoint keeps the tombstone, stale commit conflicts without advancing the epoch, then fresh disjoint writers commit. |
| Failed statement rollback | `result_budget_rollback_keeps_the_writer_snapshot_until_conflict_retirement`: one-row result limit; a failed bulk statement restores earlier private work; a later conflicting writer and three checkpoints do not retire its source; conflict leaves WAL/epoch unchanged and then releases the old generation. Both residency modes. |
| Real queue and cancellation | `queued_snapshot_survives_physical_reclamation_and_refunds_cancelled_admission`: pauses an actual submitted group-commit task; three checkpoints preserve its old source; successful and cancelled outcomes expose complete graph/SQL states; CPU/task/memory admission returns to zero and the retired generation reclaims. |
| Idle reader resources | `idle_snapshot_releases_construction_admission_and_accepts_a_fresh_execution_context`: drops the construction permit while the read pin stays live, checks zero working admission/CPU/tasks, and uses a new admitted context to read the original data after a write. This does not assert that retained canonical data consumes zero memory. |
| Allocation and final release | Four `cow_snapshot_profile_*` cases measure two row counts and reader ages, check shared untouched row owners/segments, original values, retained history after producer closure, final pin/charge retirement and actual frees. |

Existing tests retained and executed with this delivery:

- [`store.rs`](../crates/storage/src/store.rs):
  `retained_history_budget_refunds_wal_refusal_and_recovers_after_snapshot_drop`
  uses a four-stamp shared budget and injected durable WAL refusal;
  `version_history_budget_rejects_growth_before_wal_and_recovers_after_unpin`
  uses a two-stamp current limit. Both cover persistent and in-memory stores,
  unchanged WAL/epoch/data on refusal, owner retirement, retry and durable reopen.
  `read_snapshot_baseline_releases_history_without_losing_data_or_pins` and
  `checkpoint_reclaims_obsolete_live_version_history_without_losing_rows_or_conflicts`
  cover the separation of data from conflict metadata.
- [`version.rs`](../crates/storage/src/version.rs): lease sharing, partial-prune
  copying credit, concurrent reservations, exact watermark boundaries, barriers
  and conflict behavior.
- [`concurrent_transactions.rs`](../src/api/tests/concurrent_transactions.rs):
  writer pins on rollback/commit/drop, first pessimistic refresh, queue transfer,
  parallel preparation, first-committer-wins, disjoint commits and tombstone
  conflict/reopen. The `optimistic_*` selections are listed in the receipt.
- [`cow/checkpoint/tests.rs`](../crates/storage/src/cow/checkpoint/tests.rs):
  `retained_node_row_keeps_checkpoint_copy_charge_after_directory_release` and
  `denied_retained_node_checkpoint_copy_preserves_rows_and_releases_admission`
  retain charges with the actual last row owner and refund after retirement.
- [`incremental_memory_tests.rs`](../crates/storage/src/store/graph_checkpoint_candidate/incremental_memory_tests.rs):
  admitted scratch is released after preparation, paused candidates release
  execution slots, wide pinned-page copy denial leaves the source unchanged,
  and retained replay owners remain charged until their final holder drops.

## Allocation observations

Machine-readable observations and provenance:
[`mvcc_snapshot_retention_macos_2026_10_21.json`](benchmarks/mvcc_snapshot_retention_macos_2026_10_21.json).
Baseline kernel: `2d77562a2fe70326e21a4a7e926a11f79e3ffc6e` plus the measurement
harness. Final base: `97217f9a4d53bb2bca5995644fb74328a72f3879`; the intervening
change only landed the governing spec. Both observations use the unchanged
production kernel with identical allocation fixtures. They demonstrate
repeatability, not a production optimization or speedup.

Each row has a 16 KiB string body, integer ID and integer value. After fixture
setup/checkpoint, capture one read baseline; make 1 or 4 real commits updating
one node and retain each precise version root. Untouched node ownership stays
shared even when its directory/page detaches. Drop the producer before the
readers to check ownership independently of the original handle.

| Rows | Reader age (commits) | Capture requests / bytes | Shared unchanged segments | Version payload retained / after retirement | Baseline / final max RSS (bytes) |
| --- | --- | --- | --- | --- | --- |
| 64 | 1 | 2 / 272 | 15 | 96 / 0 | 19,775,488 / 19,742,720 |
| 64 | 4 | 2 / 272 | 15 at each update | 384 / 0 | 19,939,328 / 19,873,792 |
| 512 | 1 | 2 / 272 | 127 | 96 / 0 | 27,803,648 / 27,787,264 |
| 512 | 4 | 2 / 272 | 127 at each update | 384 / 0 | 27,951,104 / 28,000,256 |

Capture does not copy the record bodies: observed traffic is smaller than one
body and the storage-sharing assertions hold. Final retirement requests no new
allocations in these observations; observed frees range from 1,141,707 to
9,003,780 bytes, include all fixture bodies, and version/pin counters return to
zero. This is calling-thread allocator traffic, including frees of allocations
created before observation, not a live-heap subtraction.

The 272-byte capture scope is `GraphStore::snapshot_for_read`, not the complete
facade transaction/catalog/cache construction cost. The 96/384-byte estimates
exclude map/page/allocator overhead and canonical values. `/usr/bin/time -l`
reports peak RSS for an entire isolated native debug/fastbuild test process,
including setup, libtest, runtime and allocator/cache effects. Peak RSS cannot
show that memory returned to the OS at final drop. Process/library caches may
remain after owned data is freed; these measurements do not establish a
process-wide memory ceiling or an unbounded-reader workload guarantee.

Reproduce each profile in a separate process (replace the final suffix with
`small_old`, `large_young`, `large_old` for the other fixtures):

```sh
bazel build //crates/storage:hawdb_storage_tests
/usr/bin/time -l bazel-bin/crates/storage/hawdb_storage_tests \
  --exact store::snapshot_retention_tests::cow_snapshot_profile_small_young \
  --nocapture --test-threads=1
```

Native allocation observations are compiled only for storage unit tests outside
WASM; they do not replace the production allocator.

## Validation and remaining gates

The companion JSON records exact commands, source hashes, selected/passed/failed
counts, toolchain and local outcomes. The source is tested on macOS arm64 with
Rust 1.97.1 and Bazel 9.3.0; persistent fixtures use default
`SyncOnEveryWrite`. Native Cargo Clippy covers all workspace targets/features;
Bazel uses its normal configuration. Newly added cases are nine executed tests,
five storage and four facade. The focused selection also matches the existing
ignored long compaction campaign; the ordinary compaction differential smoke
runs. An ignored campaign is not counted as a passing test.

Broader native suites and supported platform/feature configurations remain CI
evidence to link from the PR. No production scheduling/commit path changed;
this report does not newly certify the complete #232 fairness/latency matrix.
These process-lifetime, cancellation and physical reclamation tests do not model
torn/reordered/unsynchronized machine writes. Windows power-loss qualification
remains in its existing post-merge track; positive Windows publication remains
a separate gate. Default durability and durable branch existence are unchanged.

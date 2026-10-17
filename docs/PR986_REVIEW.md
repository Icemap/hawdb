# PR986 review disposition

Issue Number: ref #987, ref #976

This records the bounded review fixes and remaining decisions. It does not
constitute independent approval of the retained implementation.

## Current delivery state

The policy and ordinary-tooling prerequisites have landed on main:

| Delivery | Actual main commit |
| --- | --- |
| #1050 session result policy and native SQL RETURNING payload guard | a1012720decca3844864c910360057a5c71dddd8 |
| #1051 checkpoint-release capacity and retry boundaries | a7900428 |
| #1052 reusable ordinary qualification and optional profiling tools | 1f2cad50 |
| #1055 Go host conversion/call/free observation | 020f1746 |

Merge 3793274a integrates actual main a1012720 and resolves its five conflicts.
There is one canonical SessionConfig and one effective native SQL RETURNING
payload guard. Retained-specific cursor, Arrow, ownership and reporting
extensions remain in this branch. Commit af1d4bb5 corrects a stale default-value
test assertion and adds independent None/Some(0)/Some(100000) memory/file
controls. Its complete execution-request contract passes 32/32, with no failed,
ignored or filtered tests, in 122.97 seconds. These are source-qualified
results, not a claim that every remaining acceptance gate has passed.

The first retained cut is [#1059](https://github.com/nowledge-co/hawdb/pull/1059),
directly against main: storage COW/source capacity, executor numeric ownership,
shared admission and the Rust numeric cursor. At 14e852d4, its complete core,
QoS, executor and storage targets pass; storage reports 1,912 passed and 30
existing ignored cases. Its nine CI checks pass. The initial combined local
root invocation has three timeouts plus incomplete targets and reported
checkpoint/concurrency failures. A complete unchanged general-partition replay
passes 1,711 tests, with one existing ignore, in 438.13 seconds. The failed run
remains recorded; neither that pass nor CI success establishes its cause or
waives the remaining local checks. The PR description tracks complete main
comparisons, other root partitions, mandatory fuzz and direct Windows numeric
qualification. No independent approval or readiness is implied here.

After that cut actually lands, the next main-based deliveries cover retained
numeric foreign bindings and Arrow. Keep #986 as the final indexed-node-view
and full-qualification delivery; merging prerequisites alone does not complete
this PR. Its original 90-group ordinary and 110-group resource matrices,
explicit-large/default-refusal controls, allocation/conversion costs,
whole-operation RSS, tail latency and exception paths remain required on the
final source. Preserve the original 3cdb2610 baseline and the additional
integrated-main a1012720 control separately. Independent review and genuine
maintainer/Tide approval remain required.

## Historical follow-up on 9bdea658

The Linux native-binding job
[117272297239](https://github.com/nowledge-co/hawdb/actions/runs/38990715330/job/117272297239)
failed in `//bindings/ffi:hawdb_ffi_tests`: 16 passed and
`descriptor_versions_sizes_flags_and_handles_refuse_with_empty_outputs` failed.
Its short-output fixture was a `[u8; 24]`, which does not guarantee the
descriptor's alignment. The new alignment guard correctly refused before
writing, while the test expected its first 16 bytes to be zeroed. The fixture
now uses three u64 words, keeping the short-buffer and misalignment contracts
separate. The dedicated misalignment regression still requires no writes.
No production alignment check or output-clearing rule is weakened.

At this review snapshot, #1050 and the checkpoint-release fix #1051 were
prerequisites. They have since received independent approval and landed on
actual main, as listed above. The #1050 approved source af2de01b and actual main
a1012720 have the identical tree a0f5fcc0750a3a4c5f35925a16d4d07b481000e6.
Its complete 30-case contract, full general partition and 103 registered local
targets pass, including all 98 mandatory fuzz targets. The immutable
[policy packet](https://github.com/nowledge-co/hawdb/blob/bf0c202781f6a1d7919c8c2a76be7a786f0feff5/docs/qualification/session-returning-af2de01b/packet.json)
retains its failed counterfactuals, RED/GREEN tests and final-source evidence.
These bounded approvals do not approve the retained delivery or its resource
and performance gates.

The benchmark harness is reusable local developer qualification code, also
called by the optional comparison workflow. `run.py`, `python_boundary.py`,
`checksum.py` and the Rust/Go host-boundary drivers share typed fixtures and
checksums; `compare.py` validates two immutable revisions. `retained_boundary.py`
and the two write controls exercise explicit retained lifetimes and ordinary
write controls outside that workflow. `native_alloc.rs` and `native_profile.rs`
support separately instrumented developer producers; driver tests validate
refusal, source identity and report completeness. These tools are documented
for local invocation and their Bazel targets are manual, rather than production
control planes or default CI workloads. Keeping a reproducible qualification
path on main is useful, but does not make all historical observations or the
whole harness an inseparable part of the retained-core delivery.

Ordinary tooling is now landed through #1052/#1055. The remaining delivery
boundaries are:

| Developer tooling | Delivery boundary |
| --- | --- |
| `checksum.py`, `run.py`, `compare.py`, `main-baseline-adapters.json`, `python_boundary.py`, `test_run.py`, and Rust/Go ordinary drivers | Reusable ordinary qualification landed through #1052; its observation additions landed through #1055. Retained Rust core delivery does not require the foreign consumer harness. |
| `native_alloc.rs`, `native_profile.rs` and their profiling build targets | Shared opt-in allocation/resource observation, kept with their Rust/FFI/Python consumers. FFI/Python imports are guarded by `boundary-profiling`; ordinary builds do not install this observer. |
| `retained_write_control.py` | Numeric retained binding qualification. |
| `retained_boundary.py` and `test_retained_boundary.py` | Numeric/Arrow consumer qualification; the current driver includes both modes and must not introduce Arrow into the first Rust-only core cut. |
| `retained_node_write_control.py` | Node-view delivery. |
| Shared BUILD/README and historical summaries/index | Select the relevant build/documentation hunks for each delivery; retain source-qualified evidence and artifact links without reintroducing raw reports. |

The five Python executable/test targets in this benchmark BUILD are manual.
The ordinary Rust allocation bench and optional FFI/Python profiling targets
also consume the shared observer source, so removing it requires handling
those build inputs together. The ordinary portion has landed independently. This source audit supports
keeping reproducible developer qualification on main; it does not approve the
remaining retained/Arrow/node harness or qualify its measurements.

## Review findings

| Finding | Current disposition |
| --- | --- |
| Raw benchmark JSON in Git | Removed 30 raw reports (27,019,916 bytes) from the final tree after verifying uploaded artifact bytes: the original 25 evidence files plus five historical benchmark-root observations. The compact [index](../bindings/benchmarks/evidence/README.md) preserves original hashes and sources. Branch history is retained; squash merging is required to exclude the raw reports from main history. |
| Separate session policy from retained bindings | The #1046 policy landed through #1050 as actual main a1012720, integrated in 3793274a. Canonical settings/defaults and the native SQL payload guard come from main, with one definition each; this branch keeps retained-specific extensions. Final policy source af2de01b has 30 passing contracts, a passing full general partition and 103/103 registered targets, including 98/98 mandatory fuzz. Remaining retained/bindings cuts and their independent review remain open. |
| Three meanings of zero | Public session and host settings normalize zero to unlimited at their explicit conversion helpers. Low-level request Some(0) remains a strict zero-row budget; language LIMIT 0 returns no rows. The landed policy has a regression exercising these boundaries together. Host default None and explicit Some(0) are unlimited; SessionConfig::max_result_rows defaults to numeric zero. The C retained request exposes batch_rows, not a session max_result_rows field. An internal RowLimit enum remains an optional refactor. |
| import_label stale name key | b0cdac83 removes the previous name mapping only when it still maps to that label ID. Tests preserve snapshots, exercise overwritten name mappings, and verify that only the retained symbol survives replacement. The capacity fixture uses public import with a preallocated string, without manual map removal. |
| Label retention expect | Capacity is computed by a private helper from the already borrowed Label. Admission no longer performs a second lookup or expects that lookup to succeed. Missing-ID and permit-refusal regressions remain selected. |
| FFI alignment and borrowed release | Aligned access is checked before dereferencing retained request/database pointers or writing outputs. Borrowed columns return owner_id=0; releasing that sentinel returns InvalidHandle and preserves the real batch. Both numeric and node query entry points are covered. Descriptor layouts and ABI version are unchanged. |
| FFI sanitizer coverage | Rust FFI17, Python retained/Arrow62, and Go retained17 cases pass with ASan-instrumented Rust producers. Details and instrumentation scope are below. |
| Release-capacity boundary and expiry | Tests derive the threshold from STORAGE_PRESSURE_DEFER_RATIO_PER_MILLION and limit/10, checking one byte below and exactly at the inclusive boundary. Preparing/Releasing expiry preserves retryable BranchBusy and one five-second deadline despite repeated notifications. An Idle transition remains reusable and release-before-deadline wakes the waiter. All five capacity-wait cases pass. |
| PR description history | The prior body, comments and merge resolution patch are archived with byte verification. The PR body now describes the current implementation, source-qualified validation and remaining gates. |

## Main merge conflict resolutions

Latest integration 3793274a merges actual main a1012720 after #1050 landed.
The five conflicted files are docs/READ_RESULT_LIMITS.md, src/api/mod.rs,
src/api/observability.rs, src/api/query_runtime.rs and
tests/execution_request_contract.rs. Resolution retains main's canonical
session/default/host/admitted SQL guards and preserves the retained/Arrow
ownership extensions. Two clean-text duplicate statements were removed.
Pinned formatting, strict native/minimal-WASM Clippy, prek and hooks pass on
that tree. Its focused native RETURNING check passes nine tests/54 controls;
its complete contract has one stale representation assertion failure, preserved
separately from the corrected complete 32-case GREEN at af1d4bb5. No second
SessionConfig or payload helper is introduced.

### Historical resolution inventory

The historical merge e33baaea has parents feature5d072971 and mainb3f00e6d.
The [verified review archive](https://github.com/nowledge-co/hawdb/actions/runs/38989949570/artifacts/12085055265) contains the
exact `merge-conflict-resolutions.patch`, generated with:

```console
git show --remerge-diff --format= e33baaea32102e9be9634f7d2630d39ba6f694c5
```

These are the 94 resolution hunks across 23 files, rather than the full main
integration diff. Review CatalogSnapshot versus retained symbol ownership;
shared RuntimeMemoryController admission and retained charges; COW page/row
ownership and capacity invalidation; admission before directory/row copies;
checkpoint capture and pruning; and preservation of main's read/write runtime
guards. b0cdac83 separately fixes the stale import key identified during review.

| Resolution file | Hunks |
| --- | ---: |
| [crates/core/src/schema.rs](../crates/core/src/schema.rs) | 2 |
| [crates/qos/src/lib.rs](../crates/qos/src/lib.rs) | 1 |
| [crates/qos/src/runtime/retained_memory.rs](../crates/qos/src/runtime/retained_memory.rs) | 3 |
| [crates/qos/src/runtime/retained_result.rs](../crates/qos/src/runtime/retained_result.rs) | 2 |
| [crates/storage/src/canonical.rs](../crates/storage/src/canonical.rs) | 3 |
| [crates/storage/src/cow.rs](../crates/storage/src/cow.rs) | 10 |
| [crates/storage/src/cow/checkpoint.rs](../crates/storage/src/cow/checkpoint.rs) | 4 |
| [crates/storage/src/cow/checkpoint/tests.rs](../crates/storage/src/cow/checkpoint/tests.rs) | 3 |
| [crates/storage/src/cow/node_map.rs](../crates/storage/src/cow/node_map.rs) | 14 |
| [crates/storage/src/cow/retained_capacity.rs](../crates/storage/src/cow/retained_capacity.rs) | 3 |
| [crates/storage/src/graph_overlay/cursor.rs](../crates/storage/src/graph_overlay/cursor.rs) | 3 |
| [crates/storage/src/source_scan.rs](../crates/storage/src/source_scan.rs) | 3 |
| [crates/storage/src/source_scan/pinned_rows_tests.rs](../crates/storage/src/source_scan/pinned_rows_tests.rs) | 11 |
| [crates/storage/src/statistics/checkpoint.rs](../crates/storage/src/statistics/checkpoint.rs) | 3 |
| [crates/storage/src/store/graph_checkpoint.rs](../crates/storage/src/store/graph_checkpoint.rs) | 2 |
| [crates/storage/src/store/graph_checkpoint/canonical_preparation.rs](../crates/storage/src/store/graph_checkpoint/canonical_preparation.rs) | 1 |
| [crates/storage/src/store/graph_checkpoint_candidate/incremental_memory_tests.rs](../crates/storage/src/store/graph_checkpoint_candidate/incremental_memory_tests.rs) | 2 |
| [crates/storage/src/store/graph_read.rs](../crates/storage/src/store/graph_read.rs) | 4 |
| [crates/storage/src/store/graph_read/retained_scan_tests.rs](../crates/storage/src/store/graph_read/retained_scan_tests.rs) | 10 |
| [src/api/mod.rs](../src/api/mod.rs) | 5 |
| [src/api/query_runtime.rs](../src/api/query_runtime.rs) | 3 |
| [src/api/retained_query/node.rs](../src/api/retained_query/node.rs) | 1 |
| [src/lib.rs](../src/lib.rs) | 1 |

The source patch preserves the concrete resolution context and test changes;
this inventory does not claim that the unread implementation has been reviewed.

## Historical sanitizer validation

Code implementation: b0cdac833deba26f0181e8c33c9de060a5589fbc, identical to the
six-file patch tested on 254f1cf3 before commit. Host: aarch64-apple-darwin.
Additional sanitizer compiler: rustc 1.101.0-nightly, 8d1a76430406c877b35d0b627e7f796dcf0dfeca,
2026-10-06, LLVM23.1.3. Required pinned 1.97.1 strict native/FFI/WASM checks pass
separately. The nightly build emits fetch_update deprecation warnings.

The [Rust sanitizer documentation](https://doc.rust-lang.org/unstable-book/compiler-flags/sanitizer.html)
describes the additional ASan build. Core commands:

```console
RUSTFLAGS=-Zsanitizer=address cargo +nightly test --manifest-path bindings/ffi/Cargo.toml --locked --target aarch64-apple-darwin --lib retained::
RUSTFLAGS=-Zsanitizer=address cargo +nightly build --manifest-path bindings/ffi/Cargo.toml --locked --target aarch64-apple-darwin
RUSTFLAGS='-Zsanitizer=address -C link-arg=-undefined -C link-arg=dynamic_lookup' cargo +nightly build --manifest-path bindings/python/Cargo.toml --locked --features abi3,pyo3/extension-module --target aarch64-apple-darwin
```

Builds used a separate per-worktree target/asan directory. The Python invocation
also selected the CPython 3.11 executable through PYO3_PYTHON. The resulting
extension and package were frozen before running test_retained.py,
test_retained_node.py, test_arrow.py and test_arrow_consumer.py with pytest and
PyArrow 26.0.0. The Go test binary was built by Bazel and frozen; its original
runfiles directory and the absolute sanitized FFI library were supplied before
running -test.run=Retained -test.v -test.count=1. Both foreign hosts loaded
librustc-nightly_rt.asan.dylib with DYLD_INSERT_LIBRARIES.

| Exercised consumer | Passed | Scope |
| --- | ---: | --- |
| Rust FFI retained suite |17| Instrumented Rust reads/writes, invalid handles, independent column/node lifetimes, alignment and borrowed sentinel |
| Python retained/Arrow |62| Ordinary CPython/PyArrow host loading instrumented Rust extension |
| Go retained |17| Ordinary Go host loading instrumented Rust FFI library |

ASan covers the Rust producers. CPython and Go runtimes were ordinary builds;
foreign heap/process bounds remain in the complete resource gate. macOS's
default ASan options were used. No sanitizer finding was reported in these runs.
Frozen producer/runtime SHA-256 values:

- Rust FFI library: `57a2cf31fedd2d0dbd0ba1e369efda9f428b10f3fed47e39e33c1257f6dce07d`.
- Python extension: `c39cb5006db24fe10ac9a5c7ee3d6052d1fd11f09d3d23fabb534a1e7ef3d527`.
- ASan runtime: `4b205559024dc60fdbe495f93d412ae393c17187adcffee5cb2785d79aa2d325`.

The full local Mac storage target on ab6d4ae1 passes 1,912 tests, with zero
failures and 30 existing ignored cases, in 526.17 seconds under the original
900-second deadline. Its earlier complete-target and individual-case timeouts
remain recorded; the shared host is not claimed idle and the later pass does
not establish their cause. The root general partition passes 1,742 tests with
one ignored, and the mandatory fuzz selection passes 98/98 on identical runtime
sources. These receipts do not substitute for later integration-head checks.
Predecessor performance evidence retains its original source and is assessed
separately; the complete original performance/resource gate remains open.

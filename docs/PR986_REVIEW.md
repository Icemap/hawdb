# PR986 review disposition

Issue Number: ref #987, ref #976

This records the bounded review fixes and remaining decisions. It does not
constitute independent approval of the retained implementation.

## Review findings

| Finding | Current disposition |
| --- | --- |
| Raw benchmark JSON in Git | Removed the 25 PR-added files (26,085,649 bytes) from the final tree after verifying uploaded artifact bytes. The compact [index](../bindings/benchmarks/evidence/README.md) preserves original hashes and sources. Branch history is retained; squash merging is required to exclude the raw reports from main history. |
| Separate session policy from retained bindings | The #1046 policy is extracted directly onto main in an independent working branch. Its owned/session contract target passes 17 tests; complete verification is running. The independent PR must land before its changes disappear from this PR's main-based diff. Remaining retained/bindings scope still requires independent review and a disposition of the requested split. |
| Three meanings of zero | Public session and host settings normalize zero to unlimited at their explicit conversion helpers. Low-level request Some(0) remains a strict zero-row budget; language LIMIT 0 returns no rows. The independent policy adds a regression that exercises these boundaries together. The C retained request exposes batch_rows, not a session max_result_rows field. An internal RowLimit enum remains an optional refactor. |
| import_label stale name key | b0cdac83 removes the previous name mapping only when it still maps to that label ID. Tests preserve snapshots, exercise overwritten name mappings, and verify that only the retained symbol survives replacement. The capacity fixture uses public import with a preallocated string, without manual map removal. |
| Label retention expect | Capacity is computed by a private helper from the already borrowed Label. Admission no longer performs a second lookup or expects that lookup to succeed. Missing-ID and permit-refusal regressions remain selected. |
| FFI alignment and borrowed release | Aligned access is checked before dereferencing retained request/database pointers or writing outputs. Borrowed columns return owner_id=0; releasing that sentinel returns InvalidHandle and preserves the real batch. Both numeric and node query entry points are covered. Descriptor layouts and ABI version are unchanged. |
| FFI sanitizer coverage | Rust FFI17, Python retained/Arrow62, and Go retained17 cases pass with ASan-instrumented Rust producers. Details and instrumentation scope are below. |
| Release-capacity boundary and expiry | Tests derive the threshold from STORAGE_PRESSURE_DEFER_RATIO_PER_MILLION and limit/10, checking one byte below and exactly at the inclusive boundary. Preparing/Releasing expiry preserves retryable BranchBusy and one five-second deadline despite repeated notifications. An Idle transition remains reusable and release-before-deadline wakes the waiter. All five capacity-wait cases pass. |
| PR description history | The prior body, comments and merge resolution patch are archived with byte verification. The PR body is being rewritten around the current implementation, exact-source validation and remaining gates. |

## Main merge conflict resolutions

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

## Sanitizer validation

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

Full final-head general, storage and mandatory fuzz verification is running.
Predecessor performance evidence retains its original source and is assessed
separately; the complete original performance/resource gate remains open.

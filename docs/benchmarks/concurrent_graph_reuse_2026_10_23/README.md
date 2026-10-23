# Covered graph staging: local release comparison

This is supporting evidence for #1192, based on main
`2fb566e16d5f8890852dc46ccb5107a0f9a0f4bb`, including #1181. The final candidate
reuses covered pessimistic graph staging, destroys retired read views after
releasing the coordinator, and leaves phase recording disabled until the host
explicitly calls `enable_concurrency_metrics()`.

`source.json` and `source.patch.gz` identify and reconstruct the measured
production source, tests and benchmark overlay. Both binaries use the identical
harness. `protocol.json` records binary hashes. `results.jsonl.gz` retains every
process, raw latency record, return code, stderr, ordering and host load.
`summary.json` retains every paired ratio, including adverse results.

## Protocol and limits

- Apple M3 Max, 16 physical/logical CPUs, 128 GiB RAM; macOS 27.0.1 arm64;
  local APFS `/private/tmp` on `/dev/disk3s5`. Device model was not collected.
- Rust 1.97.1, repository bench profile: opt-level 3, thin LTO, one codegen
  unit, default features. No private compiler or assertion overrides. Build
  wall times under competing builds are retained in validation logs; they
  are not comparable build-performance measurements.
- New database per case, checkpoint before timing and close/reopen before
  every durable measurement. Default automatic residency; observed residency
  was not collected. Persistent cases use `SyncOnEveryWrite`; group commit
  is enabled only in the existing explicitly grouped controls.
- Five alternating baseline/candidate pairs and one warmup process per binary.
  Each case completes 256 commits. The repeated-update matrix covers 1/4/8
  writers and 1/8/32 statements per transaction. Each statement changes a
  writer's disjoint node. Barriers separate staging and commit waves, and
  their/thread overhead contributes to elapsed workload time. No retries
  hide stale-epoch aborts. Existing optimistic/serialized controls are unchanged.
- The complete driver runs 120 measured processes, 360 measured cases and
  92,160 measured commits, plus two warmup processes. All row, epoch and 210
  measured durable-reopen checks must pass for it to finish. Warmups add two
  durable-reopen checks. No reader workers are present. Metrics are disabled
  in the final measured candidate; enabled instrumentation cost is not
  qualified by these timings.
- Our compilers/tests complete before timing. The host is shared and not
  reserved; load is retained per process. Five pairs do not establish
  confidence intervals, strict no-regression, CPU parallelism, remote speedup
  or the canonical Linux mixed-reader qualification in
  [CONCURRENT_WRITER_QUALIFICATION.md](../../CONCURRENT_WRITER_QUALIFICATION.md).
  Elapsed time includes fsync and synchronization, so less staging work does
  not imply proportional end-to-end throughput.

Reproduce with identically built baseline and final-candidate binaries:

```console
python3 docs/benchmarks/concurrent_graph_reuse_2026_10_23/measure.py --baseline /path/to/baseline --candidate /path/to/candidate --output /path/to/new-results --pairs 5 --reopen-before-measure
```

## Final candidate

Medians below are of five within-pair candidate/baseline ratios; below 1 means a decrease. Separate baseline/candidate medians and every pair remain in summary.json.

| Persistent | Writers | Statements | Staging p50 | Staging p95 | Commit p50 | Elapsed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| False | 1 | 1 | 1.098 | 1.068 | 1.085 | 1.064 |
| True | 1 | 1 | 0.935 | 1.040 | 0.992 | 0.967 |
| False | 4 | 1 | 1.014 | 0.930 | 0.960 | 0.972 |
| True | 4 | 1 | 0.992 | 1.037 | 1.006 | 1.000 |
| False | 8 | 1 | 0.963 | 0.948 | 0.971 | 0.945 |
| True | 8 | 1 | 0.984 | 1.012 | 0.991 | 0.978 |
| False | 1 | 8 | 0.709 | 0.633 | 1.180 | 0.695 |
| True | 1 | 8 | 0.668 | 0.579 | 1.028 | 0.901 |
| False | 4 | 8 | 0.688 | 0.767 | 1.070 | 0.733 |
| True | 4 | 8 | 0.633 | 0.669 | 1.005 | 0.983 |
| False | 8 | 8 | 0.660 | 0.651 | 1.127 | 0.751 |
| True | 8 | 8 | 0.635 | 0.626 | 0.999 | 0.984 |
| False | 1 | 32 | 0.551 | 0.597 | 0.993 | 0.583 |
| True | 1 | 32 | 0.722 | 0.797 | 1.042 | 0.935 |
| False | 4 | 32 | 0.593 | 0.609 | 0.971 | 0.620 |
| True | 4 | 32 | 0.581 | 0.639 | 1.021 | 0.887 |
| False | 8 | 32 | 0.592 | 0.641 | 1.036 | 0.653 |
| True | 8 | 32 | 0.646 | 0.783 | 1.084 | 1.016 |

### Existing ordinary controls

| Persistent | Grouped | Writers | Commit p50 | Commit p95 | Commit p99 | Elapsed |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| False | False | 1 | 0.997 | 0.933 | 0.778 | 1.002 |
| True | False | 1 | 1.022 | 1.044 | 1.162 | 1.080 |
| True | True | 1 | 1.207 | 1.043 | 2.293 | 1.211 |
| False | False | 4 | 1.086 | 1.021 | 0.881 | 1.055 |
| True | False | 4 | 0.932 | 0.765 | 0.643 | 0.930 |
| True | True | 4 | 1.097 | 0.996 | 1.405 | 1.034 |
| False | False | 8 | 0.957 | 0.987 | 0.914 | 1.008 |
| True | False | 8 | 0.946 | 0.999 | 0.751 | 0.982 |
| True | True | 8 | 0.928 | 0.693 | 0.767 | 0.850 |

For durable 4-writer repeated updates, staging p50 decreases about 37%/42% for 8/32 statements; elapsed ratios are 0.983/0.887. Durable 8-writer / 32-statement elapsed is adverse at 1.016 despite less staging work. Ordinary controls also retain adverse results: in-memory 4-writer commit p50 is 1.086, and durable grouped 1-writer commit p50/elapsed are 1.207/1.211. This is a bounded staging improvement, not a passing general throughput or strict no-regression gate. Five shared-host pairs do not identify the cause of each change.

All serialized controls, transaction latencies and individual tails remain in the raw/summary files. No adverse cohort or sample was removed.

## Initial always-on instrumentation cohort

`initial_always_on/` retains the complete earlier source, binary identities
and five-pair measurements. For durable 4-writer repeated updates its staging
p50 ratios were 0.617 (8 statements) and 0.567 (32). Its ordinary in-memory
4-writer commit p50/p95 ratios were adverse at 1.301/1.388, and durable
ungrouped 8-writer elapsed was 1.177. That evidence prompted opt-in metrics.
It does not prove instrumentation caused those results. The initial cohort
is historical evidence for a different source, not a verification receipt
for the final candidate.

## Resource and verification evidence

Separate resource-baseline.* and resource-candidate.* files are /usr/bin/time -l runs of the final binaries at 8 writers / 32 statements with reopened persistent fixtures. Each process also runs the in-memory case. CPU time, maximum RSS and block-I/O counts describe whole processes, including fixture creation/reopen, rather than isolated commit resource costs. Enabled-metrics cost and filesystem-byte writes were not measured; block counts can be zero because of caching.

The validation directory archives final strict workspace Clippy, pre-commit, minimal-feature build, the combined concurrent/local-fuzz aggregate, standalone 30-second MVCC campaign, and release build. Final commit hooks also run on the final tree. These are local source-bound checks, not remote CI or hardware power-loss qualification.

- Rust 1.97.1: cargo fmt --all -- --check; cargo clippy --locked --workspace --all-targets --all-features -- -D warnings; prek run --all-files: passed.
- cargo check --locked -p hawdb --no-default-features: passed.
- CARGO_BAZEL_ISOLATED=false CARGO_NET_OFFLINE=true bazel --nohome_rc test //:hawdb_unit_concurrent_tests //crates/fuzz:hawdb_fuzz_tests //crates/fuzz:hawdb_fuzz_cli_tests //tools/shiro:shiro_tests //tools/shiro:shiro_cli_tests //:hawdb_linux_ci_fuzz_smoke_test: all 99 targets passed, 13 executed and 86 cached from the earlier source-bound run. The concurrent owner executed 150 passing cases, one explicitly ignored campaign and 2,117 filtered cases in 644.85 seconds. It retains the complete existing concurrency/crash family and five new deterministic cases.
- cargo run --locked --manifest-path tools/mvcc-fuzz/Cargo.toml -- run --seconds 30 --explain: 2,791 complete cases passed in 30,103 ms.
- cargo bench --locked -p hawdb --bench concurrent_writers --no-run: passed; final incremental release rebuild took 14m49s under concurrent builds. The initial baseline and candidate builds took 3m42s and 15m55s. These are different cache/load states, not a build-speed comparison.

Deterministic tests assert graph work counts, no covered-path sequencer reacquisition, final results, rollback, lock timeout/upgrades, new-target stale-epoch fencing, statement atomicity and durable reopen without timing thresholds.


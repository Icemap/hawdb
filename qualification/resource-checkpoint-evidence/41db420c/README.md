# Supplemental allocation and row-error controls

Issue Number: ref #987, ref #976, ref #1081

Engine41db420c/tree d386d053; observerb9a5d167dc13ddbedb856965b9565b77be43e544. The evidence commit is not a producer source.

The original allocation-instrumented1M+1/file group (artifact12143095295) records31query samples for owned/retained, while Arrow fails during fixture construction with MVCC version-index payload exhaustion. That original group remains adverse and is not replaced or counted as successful. Complete archived-producer verification for that original report remains separately tracked by the full collector.

Six fresh file-backed supplemental processes complete: three default1k positive controls, then three explicit-large1M+1 exact-row-refusal controls. Each preserves31measured samples plus warmup and the unchanged complete consumer. Only the1M+1 fixture explicitly executes CHECKPOINT every65536rows, with15successful checkpoints recorded per process. Ordinary/default fixture setup,512-row mutations, safe durability and all read/runtime budgets remain unchanged.

All positive checksums match the independent oracle. Owned/retained/Arrow each rejects1M+1 with the exact1M-row cap error in32attempts. Retained bytes, buffer owners and view handles return to zero. Every measured Arrow pool window is balanced. Refused attempts have no qualified latency.

The allocation extension was compiled on061fc42b using the named opt boundary-profiling target. All tracked native build inputs are identical to the observer branch: their only differences are three qualification Python scripts. Frozen native/package/consumer hashes are checked before and after each process. Source-bound native/core correctness evidence is maintained separately.

There are186measured native allocation windows. Owned1M+1 has six nonzero live-byte windows and three cross-counter arithmetic differences; retained has two nonzero windows, Arrow one. These differences are preserved. Native counters include all native threads, and snapshot() explicitly loads individual relaxed atomics without an atomic cross-field snapshot. The data cannot prove exact per-operation net allocation, leak absence or a whole-process memory bound. RSS includes fixture/checkpoints, warmup and complete operations. Instrumented timing is separate from ordinary latency.

Final fmt, strict workspace/all-target/all-feature Clippy, all-files prek and commit hooks pass. The prior overlapping-check/index-change failure and its serialized correction remain included. Full original90ordinary/110resource acceptance, historical checkpoint disposition and independent approval remain open.

To reproduce, check out the exact observer commit and run the existing numeric driver with Python/PyArrow26.0.0 and the pinned toolchain:

```console
python -B qualification/resources/numeric_driver.py --output /tmp/hawdb-allocation-controls --rows 1000001 --backend file --configuration large --samples 31 --instrumented --setup-checkpoint-rows 65536
```

Use a fresh output path; the driver freezes/builds producers before all three modes. Use rows1000/configuration default/checkpoint cadence0 for the positive controls.

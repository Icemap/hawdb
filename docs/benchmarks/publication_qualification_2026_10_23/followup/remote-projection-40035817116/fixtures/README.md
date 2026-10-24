# Manual read projection representation experiment

Issue Number: ref #1119

This experiment-only branch compares three implementations of the same bounded
node projection. The main/default workflow and production Rust files are not
changed. Never merge the temporary query instrumentation into the embedded API.

- `map`: implementation anchor 823e8e9c and the common query-stage diagnostics.
- `stack`: the retained fixed-eight-slot prototype, rejected after local samples
  did not support a gain.
- `vector`: the same eligibility/admission logic with only actual columns held
  in a short vector, avoiding fixed-array movement.

The local vector timing overlapped unrelated Rust 1.98.1 Cargo/Clippy processes
and is unqualified for comparative latency. This predeclared independent x86_64
allocation repairs that environment problem; it does not discard, pool or
select favorable local samples. All local adverse allocations remain retained.

The fixed protocol has 1W/2R and 8W/16R, post-checkpoint and reopened-WAL cohorts,
six balanced three-variant order blocks, 256 writes and 64 reads per reader.
All six original cases, full 256-row id/value/mirror checks, pinned snapshots,
published epochs, read/write overlap and durable reopen assertions remain.
The preflight records the actual physical operator plan and borrowed-row signal;
its existing query uses the same bounded-profile execution path plus an untimed
EXPLAIN on every variant. No timed query or output is substituted.

Build all three sources sequentially with distinct fresh target directories,
the pinned Rust toolchain and unchanged global bench profile before timing.
Retain source/overlay/harness hashes, full build logs, binary hashes and profiles,
all 72 process records, exact oracles, the 1 Hz compiler inventory and every
block ratio. Four hundred thirty-two cases complete 110,592 commits, 248,832
reads and 288 reopens. Explicit eight-writer cases may oversubscribe the runner;
this does not claim CPU-parallel mutation or physical host exclusivity.

Select the existing `Scheduled Quality` workflow on this experiment ref with
`run_writer_gate=true`. Its branch-specific job is explicitly named as a manual
diagnostic, retains the existing 120-minute deadline, and does not run on a
schedule or default presubmit. Other writer input fields do not configure this
fixed experiment. Script replay requires Python 3.12 or newer and fresh paths:

```sh
python3 scripts/run-projection-representation-diagnostic.py \
  --work /tmp/projection-work \
  --fixture-parent /tmp/projection-fixtures \
  --output artifacts/read-projection-representations
```

Use `--prepare-only` to verify archives, patch application and identical common
inputs without compilation or timing. A successful diagnostic proves complete
exact checks and no detected guest compiler overlap; six descriptive blocks do
not establish the unchanged strict 40-pair acceptance gate or resolve the prior
nine read-latency regressions. No automatic retry is used.

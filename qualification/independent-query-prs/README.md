# Independent ordinary-query PR qualification

Issue Number: ref #1004, ref #1008, ref #992, ref #1001

This manual-only evidence branch qualifies the independent source heads in
`selection.json` against frozen main `051ec037`. It is not a PR dependency or
production delivery. The complete #986 100k/1M retained/Arrow gate remains open.

The workflow copies this directory to runner-owned temporary storage and clones
an independent source repository. `prepare-producers.py` checks out each exact
revision, applies the same additive ordinary harness and untimed memory-opening
adapter, records the measured tree and patch, builds all native dependencies
with default `bazel build -c opt`, freezes independent artifacts/runfiles, then
reverses only its own overlay. No target flags, budgets, durability, workloads,
features or assertions are weakened. The adapter does not change timed query,
conversion or write paths.

Each of ten case/backend shards compares all four producers on one Linux host:
1k/10k rows, Rust/Python/Go, one discarded plus seven measured rounds, rotating
producer order, fresh stores and equal-length producer paths. Original fixture
statements, calls, warmups, seed and checksum consumption are unchanged. Full
phase timings, process CPU/RSS, failures/refusals, source trees, patch hashes,
producer hashes and raw stdout/stderr are preserved. All builds finish before
ordinary timers start. A partial shard or refused group has no qualified ratio.
The full matrix is the requested cross-product, not just observed successes.

The local macOS default-opt attempt remains an unqualified historical receipt:
176 Rust successes and 320 Python/Go dynamic-loader failures were retained
before interruption. Mach-O LINKEDIT rejection affects main and all candidates;
no binary repair or unapproved target-flag override was applied.

Component qualification remains separate: exact normalizer protocol/allocation
parity and owned-result requested-allocation bounds do not prove ordinary
latency, whole-process memory, foreign-heap behavior or the original #986 gate.

Dispatch this evidence ref through the already registered
`native-bindings.yml` workflow path:

```console
gh workflow run native-bindings.yml --repo nowledge-co/hawdb --ref qualify/1028-1029-1036-linux-20261015
```

This ref owns only the manual qualification jobs. Its workflow definition is
separate from the workflows on the four source PRs.

## Component evidence

[Source-bound allocation records](component-evidence.tar.gz) and their
[archive/audit manifest](component-evidence.json) preserve 234 normalization and
138 owned-result observations. The 104 KiB source/raw package contains exact
normalizer modules, original capacity fixtures/counters, all raw outcomes,
build/source manifests and the independent auditor. It excludes compiled
binaries; their measured hashes remain recorded. Extract into a fresh directory
and run `python3 -B verify-package.py` to verify bytes, source modules, protocol
hashes, fixture checksums, capacity geometry, refusals and balanced release.

These macOS ARM64/Rust 1.97.1 requested-allocation witnesses are separate from
ordinary Linux latency/CPU/RSS and the comprehensive #986 performance gate.

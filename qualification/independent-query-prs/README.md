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

The default `ordinary` mode retains the full ten-shard matrix. A separate
`go-bulk` mode diagnoses adverse Go bulk-memory samples using the exact existing
Linux binaries and FFI libraries from run `38695324983`, without rebuilding.
It verifies the original artifact digest and producer bytes, then rotates two
identical-binary aliases for each of baseline, #1028 and #1029 at 1k/10k rows.
One discarded plus eleven measured rounds preserve all A/B and A/A outcomes.
A/A variation does not establish that adverse A/B observations are noise.

```console
gh workflow run native-bindings.yml --repo nowledge-co/hawdb --ref qualify/1028-1029-1036-linux-20261015 -f mode=go-bulk
```

The `identity` mode diagnoses the unchanged dense-identity test that timed out
in #1036's required root lane. It builds exact main `f7c4eb48` and the merge
tree of that main with #1036 head `dc73bed5` (`ec799755`, verified against
Prow's synthetic merge tree) with default Bazel fastbuild, adding only test-phase
logging. Both builds finish before the frozen test binaries execute on one
Linux host. Each runs the original exact test with its two residencies, two
budgets, full fixtures and assertions, retaining the 900-second owner deadline.
This diagnostic does not replace the required complete root selection.

```console
gh workflow run native-bindings.yml --repo nowledge-co/hawdb --ref qualify/1028-1029-1036-linux-20261015 -f mode=identity
```

## Component evidence

[Source-bound allocation records](component-evidence.tar.gz) and their
[archive/audit manifest](component-evidence.json) preserve 234 normalization and
138 owned-result observations. The 104,646-byte source/raw package contains exact
normalizer modules, original capacity fixtures/counters, all raw outcomes,
build/source manifests and the independent auditor. It excludes compiled
binaries; their measured hashes remain recorded. Extract into a fresh directory
and run `python3 -B verify-package.py` to verify bytes, source modules, protocol
hashes, fixture checksums, capacity geometry, refusals and balanced release.

These macOS ARM64/Rust 1.97.1 requested-allocation witnesses are separate from
ordinary Linux latency/CPU/RSS and the comprehensive #986 performance gate.

## Ordinary, mixed and diagnostic evidence

[Raw/source/audit package](ordinary-results.tar.gz) and its
[evidence manifest](ordinary-evidence.json) preserve the complete requested
1k/10k Linux matrix: 1920 records, including 1728 successful value/order checks
and 192 default-cap refusals. All ten shards finish. The 10k wide-result groups
refuse at the original 16 MiB cap for every source and language; they have no
qualified latency ratio. The remaining 54 case/size/language/backend groups
have complete paired measurements for the four frozen producers. All adverse
timing, CPU and process-RSS observations remain in the raw records.

The package also includes the 128-record mixed-growth control, its 128-record
small A/A plus A/B follow-up, and the 144-record frozen Linux Go bulk diagnostic.
At 1k, raw rotating-parameter reads after EXPLAIN refresh are about 8-10% slower
in replication (about 14% initially); the owner explicitly accepted this bounded
#1036 tradeoff. A/A variation does not prove that adverse A/B samples are noise.
This acceptance does not waive other findings or the complete original #986 gate.

The original dense-identity assertions pass in the same-host phase diagnostic
on main and integrated #1036 in 402.54/404.11 seconds. Only eight logging lines
are added; the production trees, unchanged fixture and both actual native
binaries were independently verified. This single pair is diagnostic evidence,
not a qualified latency ratio. The complete subsequent Prow root recheck passes
58/58, with the unchanged test actually executing in 523.2 seconds at its
900-second deadline. Prior cold/individual timeouts and CPU snapshots remain
included rather than being replaced by the successful recheck.

Extract into a fresh directory and run `python3 -B verify-files.py` to check
package integrity. This lightweight command does not repeat native binary or
Git-object verification. The original independent auditors and their terminal
receipts are included; complete Linux native ZIPs remain on the linked Actions
runs. Native binaries and copied runtimes are excluded from this compact package.
The incomplete macOS and sandboxed mixed attempts remain unqualified receipts.
These measurements do not establish an absolute tail SLO, whole-query or
foreign-heap bounds, or the complete original #986 performance acceptance.

# RaBitQ Vector Projection Benchmark

The 1-bit and 4-bit RaBitQ projections currently share one portable scalar
scoring kernel. Their code layout follows Faiss: an LSB-first sign plane and,
for multi-bit codes, a separate LSB-first refinement plane. There are no AVX2
or NEON throughput claims yet. On Arm, the explicit NEON preference reaches
the same scalar fallback that Faiss currently uses for RaBitQ; it is not a
vectorized kernel. Measurements from a different quantization format are not
comparable and are not evidence for RaBitQ.

Build options default to 4-bit RaBitQ (`RaBitQBitWidth::Four`). Select
`RaBitQBitWidth::One` explicitly for the 1-bit control. Qualify the recall and
resource profile of each width separately.

## Required Measurements Before SIMD Dispatch

Any native SIMD implementation must be measured against the scalar reference
on the exact same checksummed artifact and query corpus. The report must cover:

- candidate-ID and score-tolerance parity for unfiltered and sparse/dense
  allowlists;
- both 1-bit and 4-bit artifacts, including an odd dimension that crosses a
  refinement-byte boundary;
- dimensions 384, 768, and 1,536, plus an odd dimension;
- in-memory and file-backed scans;
- Linux x86_64 for AVX2 and native AArch64 for NEON;
- throughput, peak admitted working bytes, and cancellation latency.

The scalar path remains the baseline. Raw-vector reranking and recall against
canonical vectors are qualified separately; a faster candidate kernel cannot
change final ranking semantics.

## Reproduction

The ignored `kernel::benchmark::candidate_scan_matrix` test records a manual
scalar baseline. It uses 100,000 and 1,000,000 fixed-seed unit vectors, dimensions
67/384/768/1,536, both bit widths, unfiltered and 1%/50% allowlists, and one/four
requested workers. Every case keeps one warmup and five measured samples.
Requested and admitted workers are reported separately: a sparse allowlist may
admit fewer workers under the unchanged resource policy.

The two phases have different scopes:

- `score_codes` measures the single-threaded integer-code inner-product loop,
  including iteration, a function call and a score-bit accumulator. It excludes
  reconstruction, filtering, top-k, query transformation and fixture creation.
- `projection_search` measures a complete `search()` call over the in-memory
  or file-backed projection. Result validation and destruction are outside the
  timer. Backend order alternates by sample, and candidate IDs plus score bits
  must match the single-worker file reference for each filter.

Both backends consume the same checksummed persisted codes. The in-memory
fixture decodes the artifact instead of rebuilding its quantization. Artifacts
and JSONL outputs are retained on failure, and existing case directories or
output files are never overwritten. Reuse mode fails if an artifact is missing
or does not match the requested case. Fixture/code residency is outside the
reported per-query admitted working bytes; neither value is measured RSS.

Choose an artifact directory on a real filesystem when qualifying the file
backend. The full matrix builds roughly two GiB of artifacts; `/tmp` may be a
memory filesystem. For example, from the repository root:

```sh
mkdir -p .scratch/rabitq-baseline
HAWDB_RABITQ_BENCH_ARTIFACTS="$PWD/.scratch/rabitq-baseline/artifacts" \
HAWDB_RABITQ_BENCH_OUTPUT="$PWD/.scratch/rabitq-baseline/scalar-linux.jsonl" \
HAWDB_RABITQ_BENCH_BUILD=1 \
cargo test --locked --release -p hawdb-vector-projection --features storage-io \
  kernel::benchmark::candidate_scan_matrix -- --exact --ignored --nocapture --test-threads=1
```

Set `HAWDB_RABITQ_BENCH_BUILD=0` and a fresh output filename to reuse the same
artifacts. `HAWDB_RABITQ_BENCH_CASE` optionally selects one case in `0..16`,
ordered by document count, dimension, then bit width as listed above. A selected
case is partial evidence; the full baseline requires all 16 cases. These
environment variables configure the test harness only.

Audit and summarize a completed run with:

```sh
python3 scripts/summarize-rabitq-baseline.py \
  .scratch/rabitq-baseline/scalar-linux.jsonl \
  .scratch/rabitq-baseline/scalar-linux-summary.json
```

The auditor requires every timing group, one warmup and five measurements,
stable candidate bits, valid resource accounting, and a completion marker. It
retains all measured timings and hashes each artifact. Use `--allow-partial`
only for a selected-case probe; its summary is explicitly marked partial.
Use `--artifacts-root` when re-auditing artifacts downloaded to a new location.

The existing Native Bindings workflow has an optional manual baseline mode:

```sh
gh workflow run native-bindings.yml --ref BRANCH --field rabitq_baseline=true
```

This mode records the full matrix on Linux x86_64 and native macOS arm64,
checks the runner architecture, and uploads the raw samples, environment,
summary and original artifacts for 14 days. It is not part of ordinary PR or
push checks. Archive the evidence before it expires when comparing a later
kernel against these exact artifacts.

## Full Query Profiling

The `rabitq_query_profile` example uses the public embedded library facade to
build a separate 100,000-document, 384-dimension, default 4-bit serving
generation. Its fixed-seed xorshift64* unit vectors differ from the scan
matrix's SplitMix64 corpus. The unfiltered query requests ten final hits with
`rank_window = 100`, one worker and default resource budgets. Actual generated
and reranked candidate counts follow the serving policy and are reported;
the requested rank window is not assumed to equal those counts. The helper
verifies that RaBitQ candidate scanning and exact raw-vector reranking both
run, then checks that final hit IDs and score bits remain stable.

```sh
CARGO_PROFILE_RELEASE_DEBUG=1 cargo build --locked --release \
  --example rabitq_query_profile --features vector-search
target/release/examples/rabitq_query_profile build .scratch/rabitq-serving-profile
target/release/examples/rabitq_query_profile profile .scratch/rabitq-serving-profile
```

After three warmups, the helper prints a `ready` record with its PID and query
metrics, then waits for `run` on stdin. Attach a native sampling profiler to
that PID before sending the command. For example, on Linux, run the following
in another terminal and then send `run` to the helper:

```sh
perf record -e cpu-clock:u -F 997 --call-graph dwarf,16384 \
  --pid QUERY_PID --output .scratch/rabitq-query.perf.data
perf report --stdio --inline --input .scratch/rabitq-query.perf.data
```

The helper executes 100 full serving queries after the handshake, reports
each elapsed time, and exits. Sampled user CPU shares include the query loop
and result verification; they are not exact wall-time attribution. Record the
profiler version, event, frequency, lost samples, symbol coverage and binary
hash. The profiling build retains debug information; the ordinary scan matrix
uses the unchanged repository release profile.

The scan matrix alone does not establish end-to-end vector-query profile
share. Dividing its two phase timings is not a substitute for the separate
full-query profile below. Native-platform results and final SIMD qualification
remain separate evidence requirements for #994.

```bash
cargo test -p hawdb-vector-projection
cargo test -p hawdb-qualification production_vector
```

Future benchmark commands and results belong in this document only after their
matching implementation and scalar-parity tests are present.

## Recorded Linux Scalar Baseline

The first complete matrix was measured on 2026-10-17 21:17-21:30 UTC using
main `4c32610910cc78771eeb23d729be7107ff769bf6` plus this manual benchmark.
Kernel arithmetic and dispatch are unchanged. The timed source hashes and
retained binary hash are in the [environment record](benchmarks/rabitq_scalar_linux_2026_10_18/environment.json).
The current kernel, codec, scan and benchmark source hashes match that record.

Environment: Linux x86_64, AMD Ryzen 7 7735HS, 16 logical CPUs, roughly 28 GiB
RAM, pinned Rust 1.97.1, repository release profile, `storage-io`, and an NVMe
Btrfs artifact directory. This is a shared host, not an idle-host qualification.
One-minute load averages before/after were 1.84/1.87. No owned compiler or fuzz
process overlapped the matrix; light source editing, formatting and workflow
validation did occur during the overall run. The file scan is warm-cache.

All 16 corpora and 208 timing groups completed. The independent auditor checked
all 1,266 records, complete warmup/repetition sets, exact returned candidate IDs
and score bits across backends/workers, eligible counts and admitted memory.
No failed sample was omitted. Kernel score checksums are timing controls, not
full per-vector native-kernel parity. The separate full-query profile is below.
Native macOS results remain pending; this evidence does not complete #994.

- [Raw records](benchmarks/rabitq_scalar_linux_2026_10_18/samples.jsonl)
- [All timings, medians, ranges and artifact SHA-256 values](benchmarks/rabitq_scalar_linux_2026_10_18/summary.json)

Single-worker `score_codes` timings below are nanoseconds per vector. Ranges
cover all five measured repetitions; warmup remains in the raw records.

| Vectors | Dimension | Bits | Median ns/vector [min..max] | Million vectors/s |
| ---: | ---: | ---: | ---: | ---: |
| 100,000 | 67 | 1 | 48.30 [47.66..49.58] | 20.702 |
| 100,000 | 67 | 4 | 154.31 [153.90..154.68] | 6.480 |
| 100,000 | 384 | 1 | 271.50 [270.99..272.35] | 3.683 |
| 100,000 | 384 | 4 | 858.68 [857.87..860.11] | 1.165 |
| 100,000 | 768 | 1 | 537.75 [536.92..540.37] | 1.860 |
| 100,000 | 768 | 4 | 1709.34 [1708.71..1711.34] | 0.585 |
| 100,000 | 1,536 | 1 | 1070.68 [1064.84..1072.51] | 0.934 |
| 100,000 | 1,536 | 4 | 3376.76 [3362.47..3380.25] | 0.296 |
| 1,000,000 | 67 | 1 | 48.28 [47.56..48.38] | 20.714 |
| 1,000,000 | 67 | 4 | 155.14 [155.05..156.53] | 6.446 |
| 1,000,000 | 384 | 1 | 271.11 [270.01..274.89] | 3.688 |
| 1,000,000 | 384 | 4 | 850.39 [847.75..852.22] | 1.176 |
| 1,000,000 | 768 | 1 | 534.85 [533.04..537.64] | 1.870 |
| 1,000,000 | 768 | 4 | 1697.49 [1694.22..1706.01] | 0.589 |
| 1,000,000 | 1,536 | 1 | 1072.61 [1063.92..1073.28] | 0.932 |
| 1,000,000 | 1,536 | 4 | 3245.96 [3240.99..3252.95] | 0.308 |

These are scalar baseline rates, not speedups or end-to-end Mem request rates.
The full projection-search groups, including both filters and actual admitted
worker counts, are retained in the linked summary.

## Recorded Full-Query Profile

A separate public-API serving fixture on the same Linux host was profiled with
`perf 7.2.11-1`, `cpu-clock:u`, 997 Hz, and 16 KiB DWARF call stacks. The build
uses the repository release settings plus `CARGO_PROFILE_RELEASE_DEBUG=1`.
Three warmups precede profiler attachment; the driver enables events and waits
for the acknowledgement before starting 100 repeated queries. The capture
includes query execution, result validation and loop overhead. It excludes
fixture construction, opening the reader and warmups.

The query requested ten results and `rank_window = 100`. The actual report
identified the RaBitQ out-of-core candidate backend and raw-vector final
scores: 100,000 quantized vectors scored, 1,024 candidates reranked, 1,572,864
selected raw-vector bytes, and 904,704 admitted candidate working bytes.
Configured maximum parallelism is one. The worker report sums 13 sequential
artifact layers; it is not thirteen simultaneous workers. The raw-vector byte
counter counts selected embeddings, not all decoded segment data, and the
admitted byte value is not process RSS.

All 100 query iterations preserved final hit IDs and exact score bits. Their
profiled wall times were 1,501.66 / 1,509.96 / 1,557.62 ms (min/median/max).
These include sampling overhead and are not unprofiled production latency.
The capture contains 149,183 samples and reports 22 lost samples in two lost
chunks (0.0147% of recorded plus lost samples). Unresolved leaf addresses
account for 6.915% of sampled user CPU weight; their cost is not assigned to a
HawDB subsystem.

| Resolved self symbol | Sampled user CPU weight |
| --- | ---: |
| `ZSTD_decompressSequences_bmi2.constprop.0` | 21.260% |
| Character splitting (`CharSearcher::next_match`) | 15.266% |
| Decimal number parsing (`parse_number`) | 14.097% |
| `f32::from_str` | 13.630% |
| Float conversion (`compute_float<f32>`) | 5.406% |
| `decode_embedding` | 4.380% |
| `score_codes` | 5.415% |

These are non-overlapping self weights, not inclusive call-tree shares or
exact wall-time attribution. The profile does not justify optimizing the raw
`dot_product` arithmetic as #994's optional fourth slice. It also shows why a
candidate-only throughput gain cannot be presented as the same end-to-end gain.

Source inspection explains the larger preparation cost:
`SearchOutOfCoreReader::read_vector_segment` decompresses a touched sidecar and
`decode_vector_segment` parses all of its textual embeddings before the
RaBitQ rerank loop filters to selected ordinals. Parsing also validates finite
values, dimensions, IDs, bounds and order. A follow-up must preserve those
integrity checks; simply skipping unselected malformed rows is not an
acceptable optimization. No decoder, format, scoring arithmetic or admission
policy is changed in this baseline.

Evidence:

- [Query reports, all latency samples and binary/perf hashes](benchmarks/rabitq_scalar_linux_2026_10_18/profile.json)
- [Exact symbol counts, weights and unresolved coverage](benchmarks/rabitq_scalar_linux_2026_10_18/symbol-summary.json)
- [Complete flat symbol table](benchmarks/rabitq_scalar_linux_2026_10_18/flat-symbols.tsv)
- [Call graph](benchmarks/rabitq_scalar_linux_2026_10_18/callgraph.txt)
- [Recorder warnings and sample totals](benchmarks/rabitq_scalar_linux_2026_10_18/perf.log)
- [Profile build provenance](benchmarks/rabitq_scalar_linux_2026_10_18/profile-build.json)
- [Fixture build report](benchmarks/rabitq_scalar_linux_2026_10_18/profile-fixture.json)
- [Exact local sampling driver](benchmarks/rabitq_scalar_linux_2026_10_18/profile_driver.py), with its [hash helper module](benchmarks/rabitq_scalar_linux_2026_10_18/audit_records.py)

The checked-in text reports strip trailing whitespace; symbol fields and
sample counts are unchanged. Original exports remain with the raw capture.
The 2.31 GiB raw `perf.data` remains outside Git. Its SHA-256 is
`88cd555205c18d4133e9dc1635c59252b8252172281bde691662a84c938453bd`.
The first attachment attempt stopped before the query loop because the driver
rejected perf's NUL-terminated acknowledgement; it is retained separately and
contributes no measurements to this report. Native macOS measurement and the
remaining scalar/native-kernel qualification gates are still open.

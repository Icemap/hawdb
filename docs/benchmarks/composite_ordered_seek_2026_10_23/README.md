# Composite ordered seek: local read qualification

The measured source is `cac072479786f01c658022916e3ada75314ed129`; this includes the optional shared proof-owner cache-key follow-up. The following documentation/artifact commit does not change measured runtime or benchmark code. This is local macOS arm64 evidence with pinned Rust 1.97.1 and the repository release profile (opt-level 3, thin LTO, one codegen unit), without target overrides. It does not establish remote CI, concurrent-writer acceptance or hardware-independent performance.

## Protocol and correctness

Eight fresh processes cover 64, 1,000, 10,000 and 100,000 rows in resident and reopened OutOfCore modes. Every fifth row has `kind = archive`; the remaining rows have `kind = note` (approximately 80%). There are four rows per integer rank, unique string IDs and a 256-byte body. The composite index is `(kind, rank, id)`. Each direction uses the first page, a midpoint continuation and a near-end continuation; pages contain at most 32 rows. For DESC, a large continuation is near the beginning of the eligible descending stream, not an exhausted tail. Depth zero denotes no continuation.

The reference runs the identical filter and scalar projection, replacing only `ORDER BY rank` with `ORDER BY coalesce(rank, 0)` to disable index-order inference. Non-null INT rank makes those sort keys equivalent. The benchmark asserts the native ordered plan, a reference TopN/Sort plan and exact typed row/order equality on every pair. The same read transaction pins each comparison sequence. API regressions separately prove complete page concatenation, ties, updates/deletions, retained snapshots and reopen; this read-only benchmark alone does not qualify concurrent writes.

Each case retains one labelled warmup and seven measured pairs, alternating path order. The 48 cases contain 384 pairs including warmups, 336 measured pairs. Tables divide median ordered latency by median reference latency; ratios above one are slower. All samples, including unfavorable pairs, are retained in `raw/`; no favorable retry or sample removal is applied. No compiler or HawDB campaign was active at measurement start. Other host activity, CPU affinity and thermal state are uncontrolled.

Persistent fixtures are built in explicit Materialized mode using 4,096-row transactions, then fully validated by CREATE UNIQUE, indexed, checkpointed and reopened in OutOfCore mode. An exact count preflight runs before timing and can warm canonical storage. Default WAL limits and SyncOnEveryWrite remain in effect. These are warm read comparisons, not device-cold measurements or write/DDL throughput qualification.

## Latency

Values are median microseconds per page. Across these 48 cases, median latency ratios range from 0.0018 to 0.9496. Individual unfavorable pairs remain: the largest paired ratio is 1.1605. The finite point control has median paired ratios 0.9879 without, and 0.9994 with, an unrelated composite schema; individual point pairs also exceed one. These observations support the measured read optimization, not a universal latency guarantee.

| Rows | Backend | Direction | Continuation | Output rows | Ordered us | Reference us | Ratio |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: |
| 64 | memory | ASC | 0 | 32 | 53.4 | 80.5 | 0.6627 |
| 64 | memory | ASC | 32 | 25 | 60.3 | 73.2 | 0.8240 |
| 64 | memory | ASC | 62 | 1 | 42.4 | 44.6 | 0.9496 |
| 64 | memory | DESC | 0 | 32 | 48.1 | 76.4 | 0.6296 |
| 64 | memory | DESC | 32 | 25 | 54.9 | 67.7 | 0.8105 |
| 64 | memory | DESC | 62 | 32 | 60.2 | 85.9 | 0.7013 |
| 64 | disk | ASC | 0 | 32 | 72.3 | 88.8 | 0.8150 |
| 64 | disk | ASC | 32 | 25 | 81.0 | 88.3 | 0.9174 |
| 64 | disk | ASC | 62 | 1 | 46.5 | 66.9 | 0.6947 |
| 64 | disk | DESC | 0 | 32 | 74.3 | 88.0 | 0.8442 |
| 64 | disk | DESC | 32 | 25 | 76.8 | 88.2 | 0.8697 |
| 64 | disk | DESC | 62 | 32 | 91.3 | 106.2 | 0.8599 |
| 1000 | memory | ASC | 0 | 32 | 119.0 | 844.2 | 0.1410 |
| 1000 | memory | ASC | 500 | 32 | 61.6 | 372.3 | 0.1655 |
| 1000 | memory | ASC | 998 | 1 | 38.1 | 77.3 | 0.4933 |
| 1000 | memory | DESC | 0 | 32 | 59.5 | 727.5 | 0.0817 |
| 1000 | memory | DESC | 500 | 32 | 62.5 | 409.3 | 0.1528 |
| 1000 | memory | DESC | 998 | 32 | 60.6 | 726.5 | 0.0835 |
| 1000 | disk | ASC | 0 | 32 | 91.5 | 761.7 | 0.1202 |
| 1000 | disk | ASC | 500 | 32 | 211.6 | 652.8 | 0.3241 |
| 1000 | disk | ASC | 998 | 1 | 73.2 | 374.4 | 0.1955 |
| 1000 | disk | DESC | 0 | 32 | 253.5 | 839.7 | 0.3020 |
| 1000 | disk | DESC | 500 | 32 | 168.7 | 617.8 | 0.2731 |
| 1000 | disk | DESC | 998 | 32 | 267.6 | 896.3 | 0.2986 |
| 10000 | memory | ASC | 0 | 32 | 99.9 | 5951.1 | 0.0168 |
| 10000 | memory | ASC | 5000 | 32 | 110.2 | 3800.0 | 0.0290 |
| 10000 | memory | ASC | 9998 | 1 | 58.0 | 534.5 | 0.1085 |
| 10000 | memory | DESC | 0 | 32 | 90.2 | 7028.1 | 0.0128 |
| 10000 | memory | DESC | 5000 | 32 | 124.4 | 4157.4 | 0.0299 |
| 10000 | memory | DESC | 9998 | 32 | 123.5 | 7849.0 | 0.0157 |
| 10000 | disk | ASC | 0 | 32 | 289.0 | 7215.6 | 0.0400 |
| 10000 | disk | ASC | 5000 | 32 | 1185.4 | 5344.7 | 0.2218 |
| 10000 | disk | ASC | 9998 | 1 | 327.3 | 3130.5 | 0.1046 |
| 10000 | disk | DESC | 0 | 32 | 2079.6 | 8173.7 | 0.2544 |
| 10000 | disk | DESC | 5000 | 32 | 1192.8 | 6003.6 | 0.1987 |
| 10000 | disk | DESC | 9998 | 32 | 2140.2 | 8363.4 | 0.2559 |
| 100000 | memory | ASC | 0 | 32 | 136.5 | 61442.0 | 0.0022 |
| 100000 | memory | ASC | 50000 | 32 | 147.7 | 33371.9 | 0.0044 |
| 100000 | memory | ASC | 99998 | 1 | 88.3 | 3825.6 | 0.0231 |
| 100000 | memory | DESC | 0 | 32 | 122.4 | 67736.3 | 0.0018 |
| 100000 | memory | DESC | 50000 | 32 | 154.5 | 43509.8 | 0.0035 |
| 100000 | memory | DESC | 99998 | 32 | 156.5 | 76540.3 | 0.0020 |
| 100000 | disk | ASC | 0 | 32 | 503.2 | 75286.2 | 0.0067 |
| 100000 | disk | ASC | 50000 | 32 | 581.9 | 53335.2 | 0.0109 |
| 100000 | disk | ASC | 99998 | 1 | 155.2 | 30635.7 | 0.0051 |
| 100000 | disk | DESC | 0 | 32 | 244.2 | 80508.4 | 0.0030 |
| 100000 | disk | DESC | 50000 | 32 | 553.9 | 57721.2 | 0.0096 |
| 100000 | disk | DESC | 99998 | 32 | 287.6 | 84561.2 | 0.0034 |

## Index work and resource tradeoffs

Encoded block hashing still visits the bounded source bytes on warm access; warm binary key seeking does not imply logarithmic total byte/CPU work. The first cold ordered access fully validates one bounded block before caching a verified view. Admission precedes construction, and cache refusal keeps an admitted uncached path. The immutable bytes and offsets consume the existing shared SegmentCache, not an unbounded private cache.

Index byte counters describe logical property block bytes and omit canonical hydration and other scan I/O. A reference index-byte count of zero does not mean zero I/O. Query peak accounting is conservative admitted ownership rather than actual heap/RSS. Cache residency is shared with both paths, whose alternating order affects the observed before/after values. Raw reports retain capacity, residency, pinned bytes, evictions and admission refusals. No cold-device, total-IO or process-memory reduction is claimed.

| Disk rows | Direction | Continuation | Warm decoded entries | Blocks | Descriptor page visits | Logical index bytes | Ordered query peak bytes | Reference query peak bytes |
| ---: | --- | ---: | --- | --- | --- | --- | ---: | ---: |
| 64 | ASC | 0 | 40 | 1 | 4 | 3518 | 16783456 | 16994002 |
| 64 | ASC | 32 | 33 | 1 | 4 | 3518 | 16782091 | 16884186 |
| 64 | ASC | 62 | 9 | 1 | 4 | 3518 | 16777411 | 16782842 |
| 64 | DESC | 0 | 40 | 1 | 4 | 3518 | 16783456 | 16994002 |
| 64 | DESC | 32 | 34 | 1 | 4 | 3518 | 16782091 | 16884186 |
| 64 | DESC | 62 | 40 | 1 | 4 | 3518 | 16783456 | 16985594 |
| 1000 | ASC | 0 | 44 | 1 | 4 | 53687 | 16783456 | 17906382 |
| 1000 | ASC | 500 | 44 | 1 | 4 | 53687 | 16783456 | 17858894 |
| 1000 | ASC | 998 | 13 | 1 | 4 | 53687 | 16777411 | 16782842 |
| 1000 | DESC | 0 | 43 | 1 | 4 | 53687 | 16783456 | 17906382 |
| 1000 | DESC | 500 | 44 | 1 | 4 | 53687 | 16783456 | 17858894 |
| 1000 | DESC | 998 | 44 | 1 | 4 | 53687 | 16783456 | 17906382 |
| 10000 | ASC | 0 | 48 | 1 | 4 | 536087 | 16783456 | 17906382 |
| 10000 | ASC | 5000 | 47 | 1 | 4 | 536087 | 16783456 | 17906382 |
| 10000 | ASC | 9998 | 16 | 1 | 4 | 536087 | 16777411 | 16782842 |
| 10000 | DESC | 0 | 47 | 1 | 4 | 536087 | 16783456 | 17906382 |
| 10000 | DESC | 5000 | 47 | 1 | 4 | 536087 | 16783456 | 17906382 |
| 10000 | DESC | 9998 | 47 | 1 | 4 | 536087 | 16783456 | 17906382 |
| 100000 | ASC | 0 | 49 | 1 | 6 | 1048548 | 16783456 | 17906382 |
| 100000 | ASC | 50000 | 48 | 1 | 6 | 1048533 | 16783456 | 17906382 |
| 100000 | ASC | 99998 | 14 | 1 | 6 | 117800 | 16777411 | 16782842 |
| 100000 | DESC | 0 | 45 | 1 | 5 | 117800 | 16783456 | 17906382 |
| 100000 | DESC | 50000 | 48 | 1 | 6 | 1048533 | 16783456 | 17906382 |
| 100000 | DESC | 99998 | 45 | 1 | 5 | 117800 | 16783456 | 17906382 |

Ordered blocking-operator peak is zero in this fixture; the reference TopN retains admitted blocking state. Total query peak can remain substantial because read scratch/canonical hydration are separate from sort state. Resource refusal, cancellation, consumer failure and cache refusal belong to focused correctness tests, not these successful latency samples.

## Ordinary point-query control

Baseline `bb7d213cd114b69d248397dd93a093b0ad035841` contains both composite-range correctness fixes and has the same runtime as PR base `c6bd40630a94516b8a2c55abf466424c0a0e0788`; their differences are schema-index tests and TEST_INDEX documentation. The candidate uses the measured source. Both hosts use identical main.rs and Cargo.lock, the repository working directory/profile and pinned compiler. Their only dependency difference is the library checkout path. Saved binary hashes identify each executable.

Five fresh process pairs per schema variant alternate baseline/candidate order, and schema order also alternates. Each process warms 1,000 lookups and retains eleven batches of 1,000 lookups rotating 64 IDs in a 1,000-row fixture. Every retained batch has exactly 1,000 cache hits, zero misses and checksum 7,380; every query returns one correctly typed row. The optional composite schema is irrelevant to the point query and detects accidental proof/catalog work on that path. Ratios compare candidate and baseline medians within a fresh pair. This finite, single-host warm-cache control is not a universal no-regression bound.

| Composite schema | Pair | Baseline us/lookup | Candidate us/lookup | Ratio |
| --- | ---: | ---: | ---: | ---: |
| False | 0 | 16.034 | 15.552 | 0.9699 |
| False | 1 | 16.056 | 15.654 | 0.9750 |
| False | 2 | 15.913 | 15.721 | 0.9879 |
| False | 3 | 15.802 | 16.103 | 1.0190 |
| False | 4 | 15.891 | 15.908 | 1.0011 |
| True | 0 | 15.889 | 15.755 | 0.9916 |
| True | 1 | 16.422 | 15.688 | 0.9553 |
| True | 2 | 16.099 | 16.102 | 1.0002 |
| True | 3 | 16.020 | 16.011 | 0.9994 |
| True | 4 | 16.013 | 16.044 | 1.0019 |

Median paired ratio with composite schema `False`: **0.9879**.

Median paired ratio with composite schema `True`: **0.9994**.

## Preparation cost and retained adverse evidence

Complete setup includes schema, bounded fixture writes, full uniqueness validation, index construction, checkpoint/reopen when applicable and exact count preflight. It is outside read timing.

| Rows | Resident setup s | Disk setup s |
| ---: | ---: | ---: |
| 64 | 0.003 | 1.136 |
| 1000 | 0.012 | 1.099 |
| 10000 | 0.124 | 1.200 |
| 100000 | 1.346 | 3.262 |

- Initial source `38b5c6c4` decoded all 10,000 keys per page in a single default-sized block, with worse reverse work. The verified block view and in-block seek address that specific adverse result; old pilot samples remain under `historical/` and are not controlled latency evidence.
- Its point-control median paired ratio was 0.9977 without a composite index and 1.0217 with one. The later bcfcdedf run measured 1.0238 and 1.0242 respectively; its entire 48-case matrix and point controls are retained too. The measured overhead led to query-shape-scoped proof/catalog work. Historical samples remain available; they are not substituted for the final controls.
- A 100k single-transaction fixture refused the unchanged 16 MiB WAL record ceiling before append. It was replaced by bounded transactions, not a larger production allowance.
- Source552 completed the bounded-write OutOfCore pilot with exact results but 909.381 seconds of setup. A retained native sample identifies repeated full-record visits from incremental uniqueness validation. Moving CREATE UNIQUE after loading still reaches `validate_unique_property_streaming`, whose nested full scans also scale poorly. That next attempt was terminated during setup; its partial matrix is explicitly non-final. The final benchmark prepares in Materialized mode and measures reopened OutOfCore reads. These existing write/DDL costs remain unresolved and are excluded from any read speedup claim.

## Reproduction and evidence

From the repository root, build `cargo build --locked --release --bench composite_ordered_seek --jobs 2`, then run the executable under `target/release/deps/` as `<executable> <rows> <fresh-directory-or-dash> 7`. A dash selects resident mode. Use the checked-in point host source with a manifest pointing at the selected library checkout, and build from that checkout root so the pinned compiler and repository profiles apply. Do not mix executables from different heads.

`protocol/` records source hashes, executable hashes, driver, analysis script, build receipts and source-bound local qualification. The measurement driver contains the original absolute receipt paths; adapt those paths for replay, retaining its idle-process, source-cleanliness and binary-hash checks. `SHA256SUMS` covers all evidence files except itself. Binaries and fixture databases are intentionally omitted.

Ordinary regressions run in existing CI lanes. Local source552 Bazel qualification passed 98/98 (12 executed/86 cached, 673.408 seconds including build), with all XML archived; measured source passed 98/98 too (12 executed/86 cached, 392.034 seconds including build). Its manual MVCC campaign completed 36,911 complete cases / 30,000 ms after a separate 13.07-second build. All XML and MVCC reports remain source-bound and separate from remote CI. Incremental release benchmark builds took 7m56s at source552 with competing qualification and subsequent benchmark-only rebuild times are in build logs. These are not clean first-build measurements; a clean build cost was not measured. No claim of remote CI speedup follows.

Publication/writer qualification remains with existing Consumer owners. Administrative closure of #232 does not replace its missing performance receipts; #1136 and source-qualified writer/progress gaps are independent. See the [contract](../../specs/COMPOSITE_ORDERED_SEEK_SPEC.md) and [test ownership](../../TEST_INDEX.md).

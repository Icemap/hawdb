# Embedded architecture controls, 2026-10-24

Issue Number: ref #1222

The tables below predate the descriptor-memory admission correction from PR review.
They remain adverse/source-bound controls, not final corrected-source evidence.

The measured candidate is `9bb71b8c82120806a1304328dd6644c8f3a7dafc`, based on main
`01793dc2923da88e8b8d053ffda8daf6d8b84f06`. Main
`80c4b84357757ac6344e90f8fd4b73dbf9bcd0d0` subsequently updates release metadata
to 0.15.0; integrating it changes no Rust implementation, compiler profile or
benchmark harness. The measured binaries/manifests retain their actual 0.14.0
identity rather than claiming a new-version run. Both checkouts use the pinned
Rust toolchain and ordinary repository bench profile. Identical scan and
publication harnesses exercise baseline and candidate with fresh fixtures;
source/binary/harness hashes identify each measurement. Logical scan units are
separate from physical I/O and governor-admission costs.

The earlier `720d98f` controls are preserved under [preintegration](preintegration/README.md).
They include the dense resident scan regression and the original publication
negative controls with zero active-writer/post-E advances. They describe their
recorded sources, rather than the subsequently integrated main implementation.
The follow-up controls use the current baseline and keep all repetitions.

## Measurement contract

The storage scan uses small/bulk/sparse fixtures, resident and OutOfCore-reopen
views, three repetitions and exact complete selected-ID equality. Small has
32 unrelated + 2 selected nodes and 128-byte bodies; bulk has 4,096 + 32 with
1,024-byte bodies; sparse has 256 + 4 with IDs 4,096 apart and 128-byte bodies.
Total limits are 8,000,000 work/items and 128 MiB; steps allow 256 work, 64 items
and 1 MiB. Timing includes source capture and full enumeration, excluding fixture
creation/checkpoint/reopen. Resource-controller-free contexts isolate enumeration;
they do not measure governor admission. Bytes are logical cursor input, not
physical I/O traffic. Items include attempted IDs/metadata records and End; work
also charges scope decisions and complete label comparison bounds. Scoped graph
coverage never samples or publishes partial completion. Reopen does not imply
an evicted device cache. Resident unrelated rows and Bloom false positives still
consume work; the implementation does not promise work proportional only to
selected labels.

Publication controls use small 32-document/128-byte and bulk
1,024-document/512-byte projections, one foreground writer, 1,024 disjoint SQL
writes, 16 shared trigger writes and four quiet successful publications. Disabled
and detached modes receive identical foreground/trigger work. The optional
`--capture-before-writers true` schedule captures E before releasing workers,
then waits for their first real commit through bounded setup handshakes outside
measured write/preparation/construction intervals. Writers continue immediately;
no rate limiter, sleep or changed progress threshold is added. Total elapsed time
includes setup. The first detached publication must witness a committed post-E
advance. Active-writer advances and the existing two-advance qualification remain
separate. This finite workload does not establish starvation freedom.

## Unchanged Search segment reuse

Cross-generation segment reuse was evaluated and is not enabled. The registered
producer in `crates/search/src/consumer/registered.rs` retains its original
native authority, operation and complete compatibility snapshot. Reopen verifies
that snapshot against the selected native generation. The vector producer in
`out_of_core/generation_writer/consumer.rs` writes the complete compatibility
snapshot and builds admitted artifacts from all documents.

Main already shares in-process resident document snapshots and reduces owner
publication overhead. Such sharing does not authorize adopting durable artifacts
from another registered generation. Durable reuse needs an explicit native
manifest/closure contract, original custody, content validation, shared GC roots,
retained byte admission and crash-safe recovery. A new E still needs a new native
binding/receipt even when content is unchanged. Copying a previous receipt or
advancing only its watermark would bypass those fences. Segment reuse alone
would also leave the complete snapshot encoding/comparison costs.

This change retains exact full-source mapping/comparison and complete native
generation construction. These controls do not waive the original-baseline
performance gate, sustained 54-case matrix, native platform qualification or
Mem release/activation policy.

## Reproduction

Copy the candidate storage scan harness and its bench declaration into a clean
baseline checkout. Copy the identical publication harness into both. Build:

```console
cargo bench --no-run --locked -p hawdb-storage --bench projection_label_source
cargo bench --no-run --locked -p hawdb --bench consumer_publication
```

Run executables sequentially with new fixture roots. The scan binary takes its
root as its sole argument. Publication commands use the recorded parameters:

```console
consumer_publication --mode detached --documents 32 --payload 128 --writers 1 --writes 1024 --quiet-samples 4 --capture-before-writers true --root NEW_ROOT
consumer_publication --mode detached --documents 1024 --payload 512 --writers 1 --writes 1024 --quiet-samples 4 --capture-before-writers true --root ANOTHER_NEW_ROOT
```

Use `--mode disabled` for the corresponding foreground controls. Keep all raw
samples, source/binary hashes, successes and refusals. The benchmark removes only
its own newly created fixture; never point it at an existing database.


## Before the descriptor-admission correction: scan

All 36 complete fixture references match, with no duplicate IDs. Three repetitions
per cell; median microseconds. `cold` denotes OutOfCore reopen, with uncontrolled
device cache. The candidate work units account for complete label checks.

| Shape | Residency | Inputs baseline → candidate | Work baseline → candidate | Time µs baseline → candidate |
| --- | --- | ---: | ---: | ---: |
| small | resident | 35 → 35 | 69 → 103 | 8.75 → 11.46 |
| small | cold | 35 → 35 | 69 → 107 | 62.71 → 40.25 |
| bulk | resident | 4,129 → 4,129 | 8,257 → 12,385 | 190.38 → 549.42 |
| bulk | cold | 4,129 → 161 | 8,257 → 487 | 84414.17 → 88.29 |
| sparse | resident | 1,060,866 → 261 | 2,121,731 → 781 | 17850.29 → 25.71 |
| sparse | cold | 1,060,866 → 261 | 2,121,731 → 785 | 952393.79 → 57.38 |

Dense resident bulk regresses from 190 to 549 µs (+359 µs, 2.89×): actual-record
COW continuation and additional label accounting cost more than the old dense-ID
point path. Small resident capture also grows by 2.71 µs. The implementation keeps
one exact resident continuation; it adds no broad membership index or second
shape-dependent path. Cold bulk prunes unrelated segments without decoding their
properties, and sparse views avoid over a million historical ID probes. These
are workload-specific benefits and costs, not a universal scan-speedup claim.

## Before the descriptor-admission correction: publication

Each cell pools all 3,072 foreground transaction samples from three fresh runs.
Nearest-rank P50/P95/P99 are milliseconds, including owner wait and commit. No
repetition or tail is removed. Both enabled fixtures complete all 60 watermark
advances (48 concurrent-phase attempts and 12 quiet publications), and every
fixture checks every projected document against its full expected content.

| Shape | Mode | Baseline P50 / P95 / P99 ms | Candidate P50 / P95 / P99 ms |
| --- | --- | ---: | ---: |
| small | disabled | 4.52 / 5.51 / 9.71 | 4.70 / 5.77 / 9.99 |
| small | detached | 9.01 / 11.81 / 50.90 | 9.15 / 15.71 / 61.33 |
| bulk | disabled | 4.65 / 5.52 / 10.31 | 4.53 / 5.52 / 10.16 |
| bulk | detached | 5.50 / 7.50 / 10.85 | 5.47 / 7.50 / 11.11 |

| Shape | Source | Successful advances | Committed post-E witnesses | Post-E advances with writers active | Qualified repetitions |
| --- | --- | ---: | ---: | ---: | ---: |
| small | baseline | 60 | 38 | 35 | 3 / 3 |
| small | candidate | 60 | 38 | 35 | 3 / 3 |
| bulk | baseline | 60 | 3 | 0 | 0 / 3 |
| bulk | candidate | 60 | 3 | 0 | 0 / 3 |

The first gated publication always witnesses a real post-E commit. Small runs
also satisfy the existing repeated active-writer progress condition in every
repetition. Bulk writers drain before the first publication finishes: its three
post-E witnesses are valid completed publications, but active-writer advancement
and repeated progress qualification remain zero. Increasing the finite foreground
count did not establish concurrent bulk maintenance; no threshold is waived.

Small detached candidate tails regress locally: P95 rises from 11.81 to 15.71 ms
and P99 from 50.90 to 61.33 ms. Disabled small P99 also rises slightly. These three
local repetitions do not identify a universal causal effect or demonstrate the
historical zero-regression gate. Bulk detached foreground tails are similar, but
that cohort does not qualify active-writer progress. Original zero-overlap
controls stay separate in `preintegration/uncontrolled_start/`.

The following medians pool all 60 successful attempts per enabled source/shape,
including quiet attempts. Preparation includes bounded staging and exact complete
source verification. Native construction remains complete. Callback duration is
a lower bound on exclusive outer-owner hold, not the selector mutex duration.

| Shape | Source | Preparation ms | Native construction ms | Publication callback ms | Whole attempt ms |
| --- | --- | ---: | ---: | ---: | ---: |
| small | baseline | 60.84 | 513.30 | 42.31 | 704.20 |
| small | candidate | 64.79 | 515.80 | 42.26 | 715.72 |
| bulk | baseline | 630.88 | 262.01 | 41.72 | 1017.73 |
| bulk | candidate | 714.83 | 272.55 | 42.93 | 1177.53 |

These measurements establish hole removal/cold pruning and expose the remaining
complete-source/construction and foreground costs. They do not establish an
across-the-board maintenance latency improvement, production tail behavior,
RSS bounds or starvation freedom. External host activity is uncontrolled; no
other owned build/test/benchmark command ran during the measurements.

The [compact pre-correction statistics](current/summary.json) retain every pooled
result. All per-run JSON, complete source manifests and unfiltered observations
are in the [verified raw evidence archive](RAW_EVIDENCE.md). Restore its original
paths to inspect the raw transactions, attempts, quiet phases and full source
hash maps. The original-baseline Consumer gate and platform/sustained acceptance
remain open under #1136.

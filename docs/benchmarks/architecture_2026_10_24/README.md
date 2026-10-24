# Embedded architecture controls, 2026-10-24

Issue Number: ref #1222

The current implementation is based on main
`80c4b84357757ac6344e90f8fd4b73dbf9bcd0d0`. Both checkouts use the pinned
Rust toolchain and ordinary repository bench profile. Identical scan and
publication harnesses exercise baseline and candidate with fresh fixtures;
source/binary/harness hashes identify each measurement. Logical scan units are
separate from physical I/O and governor-admission costs.

The earlier `720d98f` controls remain available in the existing immutable
[source archive for `9bb71b8`](https://api.github.com/repos/nowledge-co/hawdb/tarball/9bb71b8c82120806a1304328dd6644c8f3a7dafc),
under `docs/benchmarks/architecture_2026_10_24/`. All 34 files were downloaded and
compared byte-for-byte by SHA-256 before removal from the current tree. The
archive SHA-256 is
`8c2e14cef5e6d44bdba1f2d0e286222759e51c3996b1f9dbae15f5deb3d24ff6`.
No release, evidence branch or workflow was created. Git history is unchanged.

Those historical controls reported dense resident bulk enumeration at
188.29 -> 536.21 microseconds, and zero active-writer/post-E advances in the
original publication runs. They do not describe the integrated implementation
or qualify a performance gate. The [PR validation section](https://github.com/nowledge-co/hawdb/pull/1240)
records current-source results and outstanding gates; raw runs remain outside Git.

## Measurement contract

The storage scan uses small/bulk/sparse fixtures, resident and OutOfCore-reopen
views, three repetitions and exact complete selected-ID equality. Scoped graph
coverage never samples or publishes partial completion. Reopen does not imply
an evicted device cache. Resident unrelated rows and Bloom false positives still
consume work; the implementation does not promise work proportional only to
selected labels.

Publication controls use small 32-document/128-byte and bulk
1,024-document/512-byte projections, four foreground writers, four pinned readers,
8,192 disjoint SQL writes, 1,024 reads per reader, 16 shared trigger writes and
32 quiet publications. Disabled, scoped-blocking and detached modes receive
identical foreground/trigger work. Repetitions, successful post-E publications,
reader overlap, foreground p95/p99 and outer-owner wait/hold times must be
reported separately. Owner callback hold time includes validation and selection;
it is not a direct measurement of an internal selector mutex.

The optional `--capture-before-writers true` schedule is a separate, reader-free
control: it captures E before releasing workers, then waits for their first real
commit through bounded setup handshakes outside measured
write/preparation/construction intervals. Writers continue immediately. Total
elapsed time includes setup. That first post-E publication does not establish
repeated progress. Qualification still requires at least two successful post-E
advances while writers remain active; finite workloads do not establish
starvation freedom.

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
consumer_publication --mode detached --documents 32 --payload 128 --writers 4 --readers 4 --reads-per-reader 1024 --writes 8192 --quiet-samples 32 --root NEW_ROOT
consumer_publication --mode detached --documents 1024 --payload 512 --writers 4 --readers 4 --reads-per-reader 1024 --writes 8192 --quiet-samples 32 --root ANOTHER_NEW_ROOT
```

Repeat each cohort three times. Use `--mode disabled` and
`--mode scoped-blocking` for the corresponding controls. Keep all raw
samples, source/binary hashes, successes and refusals. The benchmark removes only
its own newly created fixture; never point it at an existing database.

# Consumer progress and publication qualification

Issue Number: ref #867, ref #1043, ref #231, ref #232

Physical generation retirement does not establish concurrent Consumer progress
or shorter publication. The #232 gates remain required: unrelated foreground
writes must complete during expensive preparation, Consumer watermarks must
continue advancing under that workload, and the final publication section must
not scale with total preparation work. No such improvement is implied by an
old-snapshot retention test or by the separate MVCC transaction campaign.
The remaining publication acceptance is tracked in #232; the closed #231 issue
is referenced for the underlying MVCC/dependency-protocol context.

## Deterministic evidence

`registered_consumer_reclamation_preserves_continuously_invalidated_covered_base`
captures a registered preparation, pauses it, and requests a real commit from a
different writer thread. Preparation resumes only after receiving that thread's
completed epoch. It repeats this schedule eight times, runs physical reclamation
while the covered token pins its base, and checks the typed publication result
and unchanged durable Consumer receipt.

The current conservative Database dependency rejects all eight post-E attempts.
The source advances while the Consumer watermark remains fixed. A final drain
after stopping the writer succeeds and verifies the retained content, but is
explicitly not counted as simultaneous progress. This is a fail-closed regression
and an executable record of an unmet progress gate, not a passing concurrency
qualification. Changing it requires the dependency/publication proof tracked in
#231/#232; deleting the dependency check is not an acceptable repair.

## Performance workload

`consumer_publication` is a manual benchmark over `DatabasePublicationOwner`:

- Same binary, with catch-up disabled, the legacy whole-owner path, or covered
  staging outside the owner followed by registered publication.
- One, four and eight foreground writers, each updating its own relational
  counter. These counters are outside the Memory projection contract.
- Small (32 documents, 128 bytes/document) and larger (512 documents,
  1,024 bytes/document) immutable Memory projections. Payloads repeat `x`; this
  measures body hydration/construction, not a representative token distribution.
- Persistent `SyncOnEveryWrite`, group commit disabled, 512 MiB governed memory,
  one background slot and 4/1 total/background I/O slots. Automatic checkpoint
  age is explicitly 3,600 seconds so its periodic work is outside these short
  cases. This does not qualify group-commit interaction. Source residency uses the
  default `Auto` configuration; the resolved residency mode is not recorded.
- 1,024 foreground writes per case and three repetitions with reversed case
  order on alternating rounds. Each concurrent phase stops attempting publication
  after the writers finish or after 128 attempts, whichever occurs first.
  The result records whether the limit was reached with active writers; a capped
  phase does not establish unbounded fairness or sustained maintenance. Cleanup
  runs after every enabled attempt.
  Each enabled case also records 32 successful quiescent publications separately.
- Coverage comparison admits 64 MiB per compressed/expanded segment lookup and
  512 GiB cumulatively for each. These are pessimistic reservation totals, not
  measured I/O or resident-memory allocations: every document lookup prepays
  the full per-segment limit even when documents share a much smaller segment.
  The initial 4 GiB cumulative configuration exhausted admission at the 65th
  base lookup in the 512-document case; its aborted run is retained separately.

Every write retains total latency, owner wait plus callback-entry overhead and
time inside the exclusive callback. Every publication retains the prior and
selected Consumer watermarks, the covered source epoch, writes completed around
the whole attempt, writer activity, typed refusal/success, preparation time and
cleanup/publication timing. The
callback interval is a measured lower bound on the **outer owner** lock-held
time; it is not the inner selector mutex duration. These measurements must not
be relabeled as exact native-lock instrumentation.

Fast refusals are not successful-publication speedups. Thread activity alone
also cannot prove sustained watermark progress: an attempt completed after the
writers stop is excluded from active-writer progress counts. The deterministic
post-E schedule above remains the governing gate. Raw samples, including long
waits and adverse rounds, are retained; no p95/p99 samples are discarded.

Build with the repository's unmodified bench profile and run on a deliberately
chosen filesystem. Use a fresh fixture parent when replaying an aborted run
so a reused process ID cannot collide with retained failure evidence. The Linux
driver runs cases sequentially, captures source/binary digests, filesystem and
host load, and preserves failure output. Shared-host
measurements do not establish exclusive-machine or cold-device performance.

```console
cargo bench --locked --bench consumer_publication --no-default-features --features full-text-search,background-maintenance --no-run --message-format=json
python scripts/measure-consumer-publication.py --binary target/release/deps/consumer_publication-BINARY_HASH --output /tmp/consumer-publication-results --fixture-parent target/consumer-publication-fixtures
```

Use the executable path from Cargo's `compiler-artifact` JSON, replacing the
illustrative binary suffix. The manual Bazel entry is
`//:hawdb_bench_consumer_publication`; release measurements use the repository's
global release configuration. This benchmark is not added to CI. The public
regression belongs to the default/minimal `mvcc_staging_session` targets and the
existing Windows `registered_consumer` selection.

## Recorded Linux matrix

The [2026-10-22 evidence](benchmarks/consumer_publication_linux_2026_10_22/README.md)
contains all 54 completed release cases, raw samples, source/binary digests,
the initial budget-refusal attempt and explicit cap-hit limitations. All 18
covered cases recorded zero active-writer watermark advances. Faster refusals
are not evidence of an equivalent-work p95/p99 improvement.

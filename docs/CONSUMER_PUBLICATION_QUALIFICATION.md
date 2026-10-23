# Consumer progress and publication qualification

Issue Number: ref #867, ref #1043, ref #231, ref #232

Physical generation retirement does not establish concurrent Consumer progress
or shorter publication. The #232 gates remain required: unrelated foreground
writes must complete during expensive preparation, Consumer watermarks must
continue advancing under that workload, and the final publication section must
not scale with total preparation work. No such improvement is implied by an
old-snapshot retention test or by the separate MVCC transaction campaign.
The remaining publication acceptance is tracked in #1119 and the acceptance
ledger below. #232 is administratively closed; that status does not qualify its
performance gates. The closed #231 issue records the underlying MVCC context.

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
- 4,096 foreground writes per case and three repetitions with reversed case
  order on alternating rounds. Each concurrent phase performs the same 16
  maintenance-trigger commits in all modes and 16 successful publications in
  enabled modes, including attempts after foreground writers drain. Cleanup
  runs after every enabled attempt. Each enabled case also records 32 successful
  quiescent publications separately. This fixed-work control does not establish
  sustained concurrent progress; see the separate 100,000-write cohort below.
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
cargo bench --locked --bench consumer_publication --no-default-features --features full-text-search,background-maintenance --no-run --message-format=json > /tmp/consumer-build.jsonl
python3 scripts/measure-consumer-publication.py --build-artifacts /tmp/consumer-build.jsonl --binary target/release/deps/consumer_publication-BINARY_HASH --output /tmp/consumer-publication-results --fixture-parent target/consumer-publication-fixtures
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

The [2026-10-23 follow-up](benchmarks/publication_qualification_2026_10_23/README.md)
contains both clean-main and prototype 54-case matrices, failed sustained
cohorts, writer failures and separate syscall attribution. Their source scopes
and unmet acceptance are retained explicitly.

## Separate sustained fixed-work cohort

The manual driver accepts `--sustained --writes 100000` for detached publication
through the writer phase, with ordinary checkpoints every 16,384 foreground
commits and an explicit 4,096-attempt publication ceiling. Three rounds cover
both existing document shapes and all 1/4/8-writer counts: 18 cases, separately
from the unchanged 54-case latency controls. Source epochs, final rows/documents,
checkpoint samples and actual post-E progress remain mandatory. A cap hit with
active writers is not successful sustained progress. The existing 64 MiB
retention-byte ceiling and production live-row limit remain unchanged; entry
reservation includes the declared maximum trigger count.

The driver requires matching Cargo compiler-artifact JSON for the executable
and records its actual features and profile instead of assuming a default build.
A checked-profile or mismatched executable refuses before timing. Preflight
contracts run manually with `python3 scripts/measure-consumer-publication.test.py`.
Completed processes do not by themselves satisfy the sustained gate; the final
receipt records unmet progress and the driver fails after retaining every
completed case. Correctness/process errors stop with original failure evidence.
This finite workload is not an open-loop or starvation-freedom proof.

## Caller accounting follow-up

The [source-scoped follow-up](benchmarks/publication_qualification_2026_10_23/followup/README.md)
retains the complete 18-case sustained and 54-case control cohorts after correcting
host-call accounting. One feed item requires separate intake and Done-mapping
calls, including End, then a close; the helper now permits `2 * feed.items + 1`
calls and asserts completion before complete-source verification. Actual calls
are retained per attempt and checked by the driver. Feed, mapping, memory,
retention, live-row and durability limits remain unchanged. A separate 60,000-update
actual-helper regression completes coverage/publication at 120,003 calls. The
sustained cohort itself never exceeded the old host-call bound, so its progress
must not be attributed causally to that correction. Original failures remain
retained. Final integrated-source and strict performance acceptance remain open.

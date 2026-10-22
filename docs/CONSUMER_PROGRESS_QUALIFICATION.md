# Consumer progress qualification: open acceptance

The implementation is available for review in
[#1177](https://github.com/nowledge-co/hawdb/pull/1177). The original strict
single-stream writer gate **has not passed**. The independent CI measurement
found persistent-write regressions; implementation and correctness evidence do
not close #232, #1136, their sub-issues, or #1171's platform acceptance.

## Original-baseline writer gate

[Run 39784901600](https://github.com/nowledge-co/hawdb/actions/runs/39784901600)
compares baseline `a703cc0f18f549918c73f454cda521edb4f0fb13` with implementation
`55e6b3873c61b520635d6fe2b55f76a6de1c988d`. Both use the identical benchmark,
effective Cargo configuration, Rust 1.97.1 and bench profile. Separate lockfiles
make this a whole-version comparison; it does not isolate the effect of this PR.
The subsequent merge of `main` requires separate final-head qualification.

There are 40 measured pairs after one labelled warm-up pair. Execution alternates
revision order and case order. All 480 measured cases, 122,880 commits and 320
durable reopens completed their exact data/epoch assertions. The six primary
endpoints are non-serialized commit p50/p95. Every unrounded upper ratio endpoint
must be at most 1.0; the threshold, sample count and adverse samples are unchanged.

| Storage | Metric | Baseline median us | Candidate median us | Paired median ratio | Ratio interval | Result |
| --- | --- | ---: | ---: | ---: | --- | --- |
| Memory | p50 | 57.191 | 20.300 | 0.352813 | [0.343397, 0.367489] | Pass |
| Memory | p95 | 65.174 | 25.850 | 0.379929 | [0.350018, 0.407272] | Pass |
| Durable, ungrouped | p50 | 273.063 | 305.722 | 1.111630 | [1.096245, 1.144237] | Regression |
| Durable, ungrouped | p95 | 313.222 | 347.786 | 1.104605 | [1.063226, 1.141282] | Regression |
| Durable, grouped | p50 | 270.222 | 313.392 | 1.158272 | [1.129363, 1.184386] | Regression |
| Durable, grouped | p95 | 311.624 | 357.323 | 1.132090 | [1.104271, 1.168320] | Regression |

The intervals use paired-ratio ranks 12/29 and a simultaneous nominal coverage
of at least 95% under independent pairs from continuous distributions. The job
uses its own runner allocation and disk-backed ext4 fixtures; physical hardware
exclusivity, CPU affinity and cache flushing are not claimed. This receipt is a
failure, not an inconclusive noise result. It must not be replaced by repeated
runs selected for favorable outcomes.

The earlier local run is also retained. Its three passing and three inconclusive
endpoints did not satisfy the same zero-increase gate. Local and CI results are
not pooled or compared as if they came from the same machine.

## Consumer controls and the retained-E correction

The first local Consumer matrix completed 13 cases before a retention refusal.
The complete manifest, samples and failure are retained. A small-window
regression then demonstrated that reservation rejected old receipt C even when
the completed coverage epoch E was still exactly at the resume floor. The fix
validates retained E only for completed covered publication; lifecycle capture
still requires C. Relevant post-E writes and E below the floor still reject.

The benchmark additionally makes its finite retention configuration explicit:
`foreground writes + quiet publications + 64` entries, with the unchanged 64 MiB
byte cap. Production defaults, per-step limits and measured work are unchanged.
The first finite small/eight-writer detached case recorded only one active
post-E advancement, below the declared two-advancement criterion. That adverse
sample is retained. The incomplete dataset cannot establish the complete matrix,
publication callback p95/p99 improvement, or sustained-progress acceptance.

The full final-source 54-case matrix, separately declared sustained workload,
and current-source writer scaling/sync-count matrix remain outstanding. Quiet
publication callback timing is only a lower bound on outer-owner hold time; it
is not internal selector-mutex timing. A finite progress test is not a general
starvation-freedom proof.

## Correctness and platform boundaries

The implementation source above has seven stationary Consumer unit regressions,
23 coverage integration cases and a 17,160-case short local MVCC campaign. The
retained-E test has both the failing pre-fix receipt and passing post-fix receipt.
After integrating `main`'s descriptor-capacity policy, the stationary family has
eight passing cases and coverage still has 23. The capacity case exercises both
blocking and detached publication with occupied strict foreground capacity and
asserts that idle tokens retain no descriptor reservation.

The earlier 98/98 Bazel fuzz receipt predates the retained-E correction. The
subsequent run was interrupted to integrate `main`; 17 cached passes and 81
skipped tests do not qualify the final source. Final local fuzz remains required.
The [native run on the implementation commit](https://github.com/nowledge-co/hawdb/actions/runs/39784960608)
provides separate Linux/macOS/Windows job records. Consult the actual job outcome
and tested SHA; a successful Unix job or Windows Clippy step is not Windows
runtime qualification, and no prior-source run substitutes for final-head CI.

See [design and A/B/C/D acceptance ledger](CONSUMER_PROGRESS_AND_RETENTION.md)
for the exact implementation boundaries. The source-scoped regression proves
real writes between capture, reservation, native preparation and repeated
publication; a result for that schedule does not waive the performance gates.

## Retained evidence

[Evidence directory](benchmarks/consumer_progress_linux_2026_10_22/README.md)
contains raw samples, source/binary hashes, both strict receipts, the failed
Consumer dataset and labelled historical validation. Rebuild using the checked-in
scripts. No favorable-only filtering or threshold waiver is used.

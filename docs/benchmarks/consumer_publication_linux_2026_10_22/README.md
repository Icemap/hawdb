# Consumer publication qualification: Linux, 2026-10-22

Issue Number: ref #232, ref #867, ref #231

**The continuous-progress and short-publication gates are not met.** This is
qualification evidence for the path following #1162, not an optimization claim.
The closed #231 issue supplies protocol context; the remaining publication
acceptance belongs to #232.

## Findings

- The deterministic regression waits for eight real post-E commits from another
  thread. All eight covered publications are refused, GC retains their base,
  and the durable watermark stays unchanged. Only a separately identified
  quiescent retry catches up. [Regression log](validation/live-writer-regression.log).
- The release matrix completed 54 cases and 55,296 foreground writes. All 18
  covered cases recorded zero watermark advances while writers remained active.
  Their 142 attempts contain 139 post-E refusals and three advances completed
  after the writers stopped. The 576 successful quiescent covered publications
  do not satisfy the live-progress gate.
- Successful covered exclusive-callback p50 medians range from 439 to 663 ms
  across the six workload groups. These are lower bounds on outer owner lock
  duration. Source inspection still shows full-generation construction inside
  `publish_search_projection_consumer_covered_update`; the noisy size comparison
  does not establish a publication section independent of preparation size.
- Legacy publications can advance watermarks between serialized writes, but
  retain the owner throughout catch-up. Covered refusals do less successful
  publication work. Lower covered latency is not an equivalent-work improvement.

For 512 documents of 1,024 bytes and eight writers, medians of the three per-run
percentiles are:

| Mode | Write p95 ms | Write p99 ms | Writes/s | Quiet publish p50 ms | Live watermark advances per run |
| --- | --- | --- | --- | --- | --- |
| Disabled | 17.891 | 547.315 | 144.1 | n/a | 0, 0, 0 |
| Legacy | 157.169 | 1109.384 | 73.3 | 467.394 | 9, 2, 16 |
| Covered | 110.159 | 922.693 | 107.8 | 439.191 | 0, 0, 0 |

The shared-host variation is large: per-run p95 for that covered group spans
28.691--396.385 ms. These three repetitions are diagnostic measurements, not a
statistical performance acceptance or a historical before/after comparison.
See [all groups](summary.md) and [every raw sample](results.jsonl).

Three legacy runs reached the 128-attempt bound with writers still active:
round 1 / 512 documents / eight writers, round 1 / 32 documents / one writer,
and round 2 / 32 documents / one writer. Their foreground timings include a
remaining interval without catch-up attempts. They are retained in the table
and raw data, but do not establish full-interval maintenance or fairness.
No covered case reached the attempt bound. The boolean cap field makes these
limitations explicit rather than hiding or retrying the affected samples.

## Workload and provenance

The [protocol](../../CONSUMER_PUBLICATION_QUALIFICATION.md) defines the same-binary
controls, data, limits, durability, timing boundaries and replay commands. The
Linux host uses an NVMe-backed btrfs filesystem. Cases run sequentially, with no
qualification builds/tests running alongside them; the host is shared and no
exclusive-machine or cold-device claim is made. Per-case load averages and setup
wall time are retained. The sum of case wall timings, including setup, is
2,378.201 seconds. Automatic checkpoint age is deliberately 3,600 seconds;
group commit and the full Mem host integration remain outside this workload.

[Source binding](source-binding.json) verifies the benchmark/configuration hashes
and identifies production source
[`47fdffc9`](https://github.com/nowledge-co/hawdb/commit/47fdffc9d626cb0d0c7a7ba4c647366d723cab2e).
The measurement occurred in an uncommitted integration worktree, so the original
[manifest](manifest.json) retains its actual checkout HEAD and dirty status.
Production files match that reference; the qualification changes do not modify
engine behavior. [Build environment](build-environment.json) records Rust/Cargo
1.97.1, the normal bench profile and absent optimization-flag overrides.
The first recorded release build took 11m 51s; the final benchmark-only rebuild
took 1m 31s. These build timings are separate from workload execution.

The initial matrix used a 4 GiB cumulative comparison reservation, with 64 MiB
prepaid per document lookup. It failed at the 65th base lookup in the first
512-document covered case. The final matrix explicitly raises that cumulative
reservation to 512 GiB without changing the engine or per-segment limit.
All 11 completed initial cases, [failure output](aborted-v1/failure.json),
[original manifest](aborted-v1/manifest.json) and the
[patch reconstructing its exact benchmark sources](aborted-v1/source.patch)
are retained. That aborted configuration is not pooled into the final summary.
The failed fixture remains available at the location in its relocation receipt.

## Validation

The [validation receipt](validation/receipt.json) records commands and source
binding. The deterministic regression passed with one selected test. The required
Bazel fuzz command passed all 98 targets: 81 executed and 17 cached. Its
77m 35s wall time includes compilation; it is not a fuzz-campaign duration.
The separate MVCC campaign passed 5,352 cases in 30,000 ms with seed 1.
Formatting, strict workspace/all-target/all-feature Clippy, and `prek run --all-files` passed. Existing CI lanes retain Windows/minimal-feature coverage;
no new local execution of those lanes is claimed here.

[Per-target fuzz results](validation/fuzz-receipt.json),
[MVCC report](validation/mvcc-report.json), and the
[raw-sample arithmetic audit](validation/sample-audit.json) keep these independent
checks separate from the unmet continuous-progress and performance gates.

## Remaining acceptance

#232 still needs dependency-scoped publication that tolerates unrelated post-E
commits, construction outside the coarse owner, and a deterministic positive
progress proof with matching data/watermark correctness. Successful equivalent
work must then establish the publication-duration and p95/p99 comparison while
preserving relevant-change rejection, durability, cancellation and recovery.
This evidence does not waive those requirements.

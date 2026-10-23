# Concurrent workload verification

Issue Number: ref #232

These are correctness receipts for the [CPU-aware concurrent qualification
protocol](../../CONCURRENT_WRITER_QUALIFICATION.md), not latency improvement data.

The checked-profile harness completes 20 cohorts: writer counts 1/2/4 with
readers 0/W/2W, plus a manual non-power-of-two `3W/5R` override, in both
post-checkpoint and reopened captured-WAL profiles. Each cohort has six storage
and serialization-control cases. In total: **120 cases, 1,920 commits, 2,496
snapshot reads and 80 durable reopen checks**. All row-pair, snapshot epoch,
monotonic visibility, retained initial snapshot and mixed-progress assertions
pass. The checked build takes 2m11s initially and 2.26s after the case-enumeration
fix. Runtime records are in `mixed-smoke-final.jsonl.gz`.

The proof uses the repository's checked dev profile with 16 commits and eight
reads per reader, on temporary Linux tmpfs fixtures. It is labelled
`smoke=true` and cannot pass the disk-backed release measurement driver. Local
compilation/fuzz work overlaps this functional proof; none of its timings are
performance qualification. The first proof exposed an old hard-coded case list
that emitted no cases for `--writers 2`; it was fixed before the complete rerun.

Eight manual Python fixtures pass for SMT/core counting, ancestor and fractional
cgroup v2 quotas, v1 mount-root/controller mapping, inherited cpusets intersected
with affinity, minimum-one behavior, invalid evidence and manual overrides.
The host probe observes eight eligible physical cores and an automatic writer
maximum of four; all visible CPU quota entries are unlimited. This observation
does not imply dedicated host capacity.

Rust 1.97.1 formatting, strict workspace/all-target/all-feature Clippy and
`prek run --all-files` pass. The standalone local MVCC campaign passes 6,718
cases in 30,009 ms after a 2m08s build. The required Bazel local fuzz campaign
is still compiling at this receipt's cutoff; it is not recorded as a pass.

Source hashes and compressed receipts are retained alongside this file. Formal
release paired results must name their exact source/baseline and retain every
selected concurrency/profile cohort; the earlier single-writer WAL-cache
results do not qualify this matrix. The PR remains Draft pending that evidence.

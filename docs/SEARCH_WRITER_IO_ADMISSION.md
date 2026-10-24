# Search writer I/O admission

A governed standalone generation writer must declare at least one I/O slot.
Use `RuntimeWorkRequest::background_maintenance(bytes).with_io_wave_slots(1)`
for a wave-scoped request. A declared reservation permits future work; it does
not occupy the originating governor's device capacity while the writer or an
invisible prepared candidate is idle.

Creation, owned-document input, preparation and selector publication acquire
one actual wave before their synchronous native work. Streamed input admits
output writes/seeks between source reads and releases output admission before
calling the host's `Read::read`. Source-reader I/O belongs to the host and may
use another Wave permit from the same one-slot governor, or share the originating
Task reservation. The writer never occupies capacity while waiting for that
reader. These coarse synchronous
phases include their interleaved encoding and build CPU work; they do not claim
per-syscall scheduling or a latency bound. Nested partition builds and physical
scratch operations borrow that same capacity through a private single-slot
controller. Concurrent borrowers serialize and retain cancellation/deadline
checks. An expired phase controller falls back to original governor admission;
a cached task context cannot authorize a later operation for free. Task-scoped
requests keep their original lifetime reservation and their per-task wave limit.

Refusal before starting an input phase does not poison untouched input.
An I/O failure during an already started streamed frame poisons the writer. An unfinished spool
is discarded without BufWriter's implicit Drop flush. Stage destruction uses a
nonblocking wave attempt under a fresh cleanup context from the same permit;
foreground cancellation cannot suppress necessary cleanup. An active phase
lends its held wave to cleanup. When unrelated work occupies capacity, cleanup
retains the existing ticket for retry. Retained debt owns accounted metadata and
disk evidence, not active task or I/O slots. A retry needs fresh caller I/O
admission for a governed ticket. Captured Consumer native custody and its
separate recovery rules remain unchanged.

Successful selector publication retains its durability boundary. Cancellation
after that barrier may defer cleanup and cannot undo the selected generation.
The existing Consumer producer also uses the phase scope, avoiding a second
acquisition of a depth-one originating reservation.

This contract covers standalone generation creation, input, preparation and
publication, plus the existing Consumer producer's use of that writer. Governed
update/mutation preparation, target input and mutation spool flushes are not
made phase-complete by this change and remain follow-up work under #1161.
Per-document pushes reacquire admission; streamed bodies yield on each source
read (currently up to 8 KiB per analyzer read), so large input adds governor
acquisitions. No bulk-ingest throughput improvement is claimed.

The local ignored input probe uses 256 documents totaling 8,389,120 body bytes.
It reports original-governor acquisitions separately from elapsed input time,
includes the final admitted spool flush, and excludes generation artifacts and
publication. On one contended macOS run, owned input used 257 acquisitions and
streamed input used 1,793; raw controls used zero. Those counts describe the
current admission granularity. Single-run timings are not a throughput result.
Pending I/O refusals currently remain in the cleanup report's non-descriptor
`blocked_stages` count; the retained ticket is retryable with fresh admission.

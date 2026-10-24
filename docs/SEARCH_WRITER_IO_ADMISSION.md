# Search writer I/O admission

A governed standalone generation writer must declare at least one I/O slot.
Use `RuntimeWorkRequest::background_maintenance(bytes).with_io_wave_slots(1)`
for a wave-scoped request. A declared reservation permits future work; it does
not occupy the originating governor's device capacity while the writer or an
invisible prepared candidate is idle.

Creation, document/streamed input, preparation and selector publication acquire
one actual wave before their synchronous native work. These coarse synchronous
phases include their interleaved encoding and build CPU work; they do not claim
per-syscall scheduling or a latency bound. Nested partition builds and physical
scratch operations borrow that same capacity through a private single-slot
controller. Concurrent borrowers serialize and retain cancellation/deadline
checks. An expired phase controller falls back to original governor admission;
a cached task context cannot authorize a later operation for free. Task-scoped
requests keep their original lifetime reservation and their per-task wave limit.

Pre-effect wave refusal does not poison untouched input. An unfinished spool
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

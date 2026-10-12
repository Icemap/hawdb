# Task-bound retained node pulls

Issue Number: ref #987, ref #1008

## Rust host contract

`Database::query_with_params_retained_node_context` returns a
`RetainedNodeTaskCursor<'a>` borrowing a `RuntimeTaskContext`. It uses the same
indexed single-node physical plan and original immutable row owners as the
context-free API. The wrapper exposes no context-free pull or inner-cursor
escape. Every open pull checks cancellation/deadline before delivering a row
or publishing empty/post-delivery EOF. Checkpoints also surround constructor
planning and capture; planning remains synchronous and is not interruptible
inside each parser/optimizer operation.

The cursor does not clone the context, cancellation ancestry or I/O controller.
Its Rust borrow requires the host to keep the context alive through cursor use.
There is no background worker, prefetch or reserved foreground task held during
host idle time. Host-owned task/admission lifetime remains the host's contract.
Exported `RetainedNodeView` owners can outlive both cursor and context: their
independent query/runtime leases continue charging original row/catalog data
and foreign metadata until their final release.

An existing `RuntimeMemoryReservation` limits the retained ledger to the smaller
of configured and admitted query memory. Its admitted result reservation caps
the result account, including cursor control, source ownership and duplicate
view metadata. Zero reservations fail, and smaller admissions are never raised
to a configured floor. Runtime retained reservations remain independently
required; an existing host reservation does not exempt these owners from
runtime admission. Conservative overlap is allowed.

## State and ownership argument

Let `S` be cursor status, `V` its initial source view and `E` exported views.
Assume the existing cancellation/deadline checkpoint and retained admission
primitives satisfy their contracts, immutable row/catalog owners preserve their
allocations, and the observer respects its synchronous callback contract.

1. Construction checkpoints the task before admission and after capture. Failure
   unwinds provisional owners and all leases without delivering `E`.
2. An open pull checks the borrowed task. On failure, `fail(Stopped(reason))`
   changes `S` to Failed, publishes Failed to the shared source, drops `V` and
   the health barrier, and records one failed statement. Subsequent pulls return
   the stored cause before any task/source work. Close cannot convert Failed
   into Closed or Completed.
3. Existing `E` still owns its original immutable allocations and independent
   leases. Dropping `V`, cursor, context or database cannot revoke its borrowed
   data. Its provisional outcome becomes Failed; readable bytes are not proof
   of a successful statement.
4. Successful EOF remains gated on the task/source checks and successful
   observer return. Completed repeated EOF stays Completed even if the task is
   cancelled afterward. Cancellation concurrent with a checkpoint can race;
   this is checkpoint-based cooperative cancellation, not retroactive revocation
   or an atomic transaction with the host's cancellation operation.
5. Every retained charge uses the constructor's ledger/account limits, including
   later independent view retention. The configured/admitted minimum cannot
   be widened by a duplicate view. Last-owner release returns retained quota;
   query/result admission does not prove a bound for parser/optimizer workspace,
   caller context/controller, foreign allocators or whole-process RSS.

These are source-level invariants and an induction over constructor/pull/close/
release transitions, not a machine-checked proof. Regressions cover cancelled
construction, expired construction deadlines and post-capture pulls, cancellation before delivery and
at delivered/empty EOF, exact sticky causes, one failed observation, original
payload addresses after all task/database owners drop, zero/undersized query
and result reservations, duplicate metadata refusal, completed EOF stability,
and final zero quotas. Runtime execution belongs to exact-head platform CI.

## Remaining scope

This is Rust task-bound point delivery. Consumed read-transaction snapshots,
foreign C/Go/Python task binding, interruptible planning, Arrow node layouts,
SIMD, graph/FTS, multi-row/wide workloads and total-memory qualification remain
open. Existing context-free APIs remain independent of a host task. Performance
qualification, especially write/load timing on the current machine, is deferred;
no performance gain is claimed.

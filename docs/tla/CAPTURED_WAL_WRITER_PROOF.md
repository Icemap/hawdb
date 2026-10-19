# Captured current-generation WAL writer

## Native handoff

Writable captured branch admission opens the active WAL through its original
UUID `SourceDirectory`, with read and kernel append semantics. Unix uses
`O_RDWR | O_APPEND`; Windows requests read access and `FILE_GENERIC_WRITE`
without `FILE_WRITE_DATA`, preserving append-only writes after replay seeks.
The Windows access contract is described by
[Microsoft file access rights](https://learn.microsoft.com/en-us/windows/win32/fileio/file-access-rights-constants).
No creation, truncation, or symlink/reparse traversal is admitted by this open.
The managed file is charged as `MutableWal` in the original operation context.
Read-only admission continues to use a read-only native file.

Prefix authentication and replay consume that same file. Successful writable
admission moves it into `DurableStore::wal_append_file` and retains the original
UUID ownership lease in `wal_append_owner`. It does not reopen a name to acquire
the writer. The file field closes before the owner's last native lock/registry
reference can retire. A checkpoint source shares the existing file/owner rather
than clearing the handle and reopening the path for an explicit prefix barrier;
Arc cloning adds no native descriptor. The original permit remains charged
until the last owner closes.

Appending validates the current binding before taking the cached handle.
Complete-record flush, shared group flush and explicit source-prefix flush
preserve the existing durability policy, and synchronous barriers use the held
file. When a namespace barrier is required, it uses the original UUID directory
through `SourceDirectory::sync`, not the saved parent path. Native Windows
charges its temporary write-capable directory-sync handle before opening it.
The writer checks the file and directory binding again before acknowledging a
synchronous result. Missing captured append custody fails closed rather than
falling back to path reopening. Partial failures use the
[actual-object rollback](HELD_WAL_ROLLBACK_PROOF.md).

Binding checks are observations; they do not exclude subsequent external
renames. A replacement before append refuses without mutation. A replacement
between admission and append can leave a complete record on the original WAL
and an uncertain response. Both files are retained, foreign bytes remain
unchanged, the store is poisoned, and restoring the original binding can recover
the complete transaction. Missing acknowledgment never proves rollback.
An uncertain append also retains the original file in the poisoned store, not
only its directory lease, until that store closes. The dedicated sync-failure
regression verifies the retained native identity, complete record, unchanged live
commit boundary and full recovery. The preceding implementation failed that
regression because its error arm dropped the last file Arc.

Power-loss recording marks this native open as append-capable as well. The
recorder must calculate the write offset from the original object's length,
even when validation/replay moved the sequential cursor to zero. The recorder
and native operation must agree; a trace flag is not a substitute for native
append access.

## Explicit boundary

This handoff covers the active generation admitted above. Existing checkpoint
and WAL-rotation transitions clear both cached file and current-generation
owner; their successor creation, head/manifest publication, doctor and artifact
paths remain legacy operations. This is an incomplete part of the complete
native-owner migration. It must not be described as a captured writer across
all generations, a durable allocation/birth receipt, a stationary Consumer
serving view, complete C..E validation, or complete MVCC.

`SyncOnEveryWrite` remains the default. Relaxed ordinary acknowledgments still
flush complete WAL records to the OS and retain their documented power-loss
window; only their later explicit prefix/checkpoint barriers promise the covered
synchronized prefix. Group callers still rely on the shared flush before their
synchronous acknowledgment. Existing serialized WAL/head formats are unchanged.
Additional binding observations and native handle retention have real I/O/FD
cost; no single-stream latency or 1/4/8-writer fairness result is inferred from
functional tests.

## Executable evidence

`captured_wal_append_preserves_end_of_file_after_reads_and_name_replacement`
seeks the actual native file to zero before append, renames its file entry,
installs foreign bytes, and verifies that append and rollback affect only the
original object. The owner and file close back to the original descriptor count.

`captured_active_wal_writer_reuses_replayed_handle_across_sync_policies` covers
both durability policies, a reversible partial write, actual group completion,
a checkpoint-source prefix barrier and cold recovery, with one original append
open. `captured_active_wal_writer_preserves_foreign_name_before_and_after_append_admission`
changes the name on each side of the real append admission boundary. The early
case preserves the original prefix; the late case recovers the complete record
after its uncertain acknowledgment. Both preserve foreign bytes and poison the
live store when the binding is lost. Read-only/repair/replay and rollback
regressions remain enabled in the independent native CI selection.

`captured_wal_append_records_real_append_offsets_and_required_prefix_barriers`
compares actual bytes with the power-loss recorder after a seek-to-zero append.
An unsynchronized image may retain only the prior prefix or the complete write;
a successful file barrier must retain the full prefix. Native resize/rollback
and its required synchronization must preserve that same prefix. The Windows
capacity selection also refuses resize at the original three-descriptor cap
without changing any bytes, releases one blocker, and retries at the same cap.

On Unix, `captured_wal_namespace_barrier_persists_the_original_directory_after_replacement`
creates a file with durable payload but an unsynchronized name, replaces the
source directory spelling, calls the actual writer namespace-barrier helper,
and restores the original spelling for native image verification. The original
name must become required in the power-loss image. This is a supported-platform
native directory-move witness; Windows file replacement and its original-object
sync primitives require their separate native CI evidence.

## Finite safety model

[HawDBCapturedWalWriter](HawDBCapturedWalWriter.tla) models one complete record
in one admitted active generation. File and directory namespace replacement are
separate actions, allowed at every phase including after acknowledgment. Replay,
installation, pre-write admission, native append, pre-flush validation, file
barrier, directory barrier and the final observation are separate stages. An
uncertain outcome can contain the complete original record. The checked
acknowledgment represents a synchronous commit/shared flush or an explicit
prefix barrier, not an ordinary relaxed commit acknowledgment.

Invariants require held custody, the original prefix, unchanged foreign payload,
file/directory barriers through original objects, and original bindings at the
acknowledgment observation. Six negative controls use path append, a path parent
barrier, dropped capture, a skipped final fence, missing kernel append
semantics, or release of the original file after an uncertain result. They must violate the corresponding invariant rather than merely
fail parsing. The model does not qualify generation transitions, bootstrap,
logical incarnations, allocator debt, arbitrary native access/sharing behavior,
concurrent external content mutation, all torn/reordered-write plans, GC,
liveness, physical resource peaks or group-commit latency. The positive model
enumerated 55 generated / 34 distinct states, depth 11, with an empty queue. All
seven Bazel targets executed, including the six intended invariant violations.

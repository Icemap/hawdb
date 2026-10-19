# Held WAL append rollback

A partial WAL append must restore the original native object, even if its saved
name now selects a foreign file. Reopening the path for truncation can destroy
that foreign evidence and leave the real transaction tail torn. Unlinking a
newly created WAL at that path has the same problem when the committed prefix
is empty.

## Implementation and durability

`DurableStore::rollback_failed_wal_write` now receives the actual append `File`.
`File::rollback_wal_append` rejects logical immutable files, checks that the held
file still contains the committed prefix, truncates that same object, restores a valid empty-generation header if the
committed length was zero, and calls
`sync_all` before returning. On Unix the append handle supports resizing. On
Windows append-only access cannot resize; `ReOpenFile` obtains `GENERIC_WRITE`
on the original handle's object, without supplying any pathname. It acquires a
permit in the original descriptor context and category before the native open,
verifies native file identity, and closes the native handle before releasing
the permit. Sharing, ACL, quota, truncation, and synchronization errors fail
closed through the existing fatal WAL error path. Test-support tracing follows
the original file's trace rather than registering a new pathname open.

The native API's contract is documented by
[Microsoft ReOpenFile](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-reopenfile).
No namespace removal or directory synchronization is performed by rollback.
In particular a newly created WAL retains a valid empty-generation header.
The missing-header rejection in ordinary recovery remains unchanged. An ordinary
retry retains the same append handle. `wal_namespace_known_durable` is not
advanced by rollback. A later
successful synchronous append or group flush still synchronizes the namespace
under the existing policy. `SyncOnEveryWrite` remains the default. This avoids
adding a pathname mutation to rollback; it does not migrate the existing later
namespace barrier to a captured directory.

After restoration, the existing name is compared with the held native object.
A mismatch marks the store unusable through `StorageIntegrity`, retaining both
files and refusing normal retry. This is a pointwise observation. A name can
change afterward, so this check does not authorize future path opens, deletion,
rotation, or publication. Initial append acquisition still uses the legacy path;
this change starts from the actual file that received the partial write.

## Executable tests

`held_wal_rollback_repairs_original_and_preserves_replacement` changes the name
at the real partial-write/rollback boundary. It covers both an empty prefix and
an existing committed transaction. The original WAL must retain its
committed prefix, or the valid empty-generation header for a failed first write;
the replacement must retain all its bytes and its directory entry.
The live store must fail closed. Restoring the original name and reopening must
recover exactly the committed node count, with no partial mutation. The first prototype run failed both new tests on
close/reopen because it retained a zero-byte file; that adverse evidence led to
restoring the existing valid header rather than weakening recovery.

`held_wal_rollback_keeps_empty_header_for_reopen_and_same_handle_retry` exercises
both same-handle retry and close/reopen of the empty log. It checks the retained
append handle, one append open, unsynchronized namespace after rollback, and
successful durable retry followed by reopen with exactly one committed node.
Both tests are enabled in the independent native CI WAL selection, including
Windows. Existing partial-write, rollback-failure, sync-failure, and group-commit
selections remain enabled.

Two separately compiled source controls must fail: pathname reopening before
rollback changes the wrong object, and omitting the empty-header write prevents
valid retry/reopen. Restoring the original source must pass the regressions.

## Finite model

[HawDBHeldWalRollback](HawDBHeldWalRollback.tla) starts after one partial append
to held file 1. The committed length is either zero or two units. File 2 is
foreign and has seven units. Its retained directory entry is represented
separately from its payload, so unlink is distinct from truncation. The namespace can change at every phase, including
after the final observation. Rollback, empty-header repair, synchronization, and
the name observation are distinct actions. The payload lengths exclude the fixed
binary header, whose recoverability is
modeled separately. The positive model enumerated 34 generated / 26 distinct
states, depth 7, with an empty queue. All six Bazel targets executed: the positive
model and five intended invariant violations. The checked invariants require
held custody, an unchanged
foreign file, restoration of the original prefix, a recoverable header after
synchronization, and an original name at the
observation that permits retry. Five generated negative controls replace actual
behavior: `PathRollback`, `UnlinkName`, `DropBeforeRollback`, `SkipNameFence`,
and `SkipEmptyHeader`.
Each must violate its corresponding custody, content, or retry invariant.

This model assumes the original append file and committed boundary were already
qualified. Synchronization succeeds as one abstract action. It does not model
OS write reordering, lost synchronization responses, torn recovery barriers,
Windows access semantics, trace implementation, native descriptor exhaustion,
pre-capture ABA, logical incarnations, or source/Consumer publication. Those
require native and power-loss evidence and the complete stationary publication
protocol. This is a rollback argument, not a proof of complete MVCC.

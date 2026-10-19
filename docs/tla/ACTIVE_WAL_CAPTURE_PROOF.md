# Active WAL prefix and replay file custody

## Native refinement

Captured branch admission opens the active WAL relative to its held UUID owner.
`validate_active_wal_prefix_from_open_file` stats that opened file, verifies the
selected prefix in fixed 8 KiB hashing scratch, and returns the same managed
handle. It no longer allocates a vector from the prefix's declared length.
Legacy path validation also opens before applying the metadata bound; invalid
identity/budget inputs still reject before opening a pathname.

The handle remains charged in the original project domain while sealed intervals
and runtime initialization run. `WalRecordCursor::from_file` rewinds and decodes
that file, without another pathname open or descriptor duplication. Existing
contiguous LSN, generation, transaction, entry/byte/record budgets and relational
recovery hashing remain. Replay checks the opened file's complete length against
the remaining aggregate byte budget. The successful cursor returns its file
through `into_file`, retaining custody for post-replay and pre-installation
physical-name checks. A missing/replaced name refuses installation; it cannot
turn an earlier prefix check into permission to read a different file.

The same global descriptor cap applies. One active-WAL file now remains live
across initialization/replay; extra recovery can still refuse. The fixed hashing
scratch does not bound all I/O buffers, decoded rows, allocator capacity, RSS or
physical device traffic. No directory, native file or process-local identity is
serialized as an incarnation/birth receipt.

## Recovery and evidence

Captured corruption/sequence failures retain the original WAL as evidence. They
do not copy whatever the old pathname names into quarantine after consuming a
different held file. Legacy uncaptured entry points keep their existing
quarantine behavior. Neither case silently skips a malformed transaction.

Automatic repair still uses the existing doctor protocol. Before entering its
path-based mutation, a captured reader requires its native file to match that
name. The native read handle uses read/write/delete sharing and remains pinned
through the repair; replay returns that same file afterward. The native regression repairs
a one-byte torn tail and requires the complete original prefix and graph to
survive. This is guarded legacy repair, not a proof against a rename after the
check; doctor intent, backup and publication paths still need relative I/O.

## Finite argument

[HawDBActiveWalCapture](HawDBActiveWalCapture.tla) starts with actual file 1 held.
The name can change to file 2 before validation, between validation and replay,
or before/after the final check. Correct prefix and replay transitions select
`held`; none releases it. Thus `HeldThroughRecovery`, `PrefixFromCapture` and
`ReplayFromCapture` are inductive. Acceptance records an observation of name 1
and requires both reads from file 1. A subsequent name change remains enabled;
acceptance does not authorize later pathname writes.

Positive TLC: **11 generated / 9 distinct states, depth 5**, empty remaining
queue. Four generated controls execute the intended violations:

| Control | Violated invariant |
| --- | --- |
| PathPrefix | PrefixFromCapture |
| PathReplay | ReplayFromCapture |
| DropBeforeReplay | HeldThroughRecovery |
| SkipNameFence | AcceptedSameFile |

All five Bazel targets executed. The model assumes the initially captured file
and selected prefix are qualified by the owning branch/head. It does not prove
pre-capture birth/ABA, byte-content immutability, doctor effects, source-wide
recovery atomicity, liveness, or complete MVCC.

## Native correspondence and remaining work

The opened-file regression replaces the pathname before prefix validation, then
requires the held original header and EOF through the actual cursor. The foreign
name must not validate. The kernel regression commits a private WAL suffix,
then replaces its file after prefix capture and immediately before active replay.
The replacement keeps the valid header but has a malformed suffix. Recovery must
consume the original complete suffix, reach the specific physical-name refusal,
preserve both WALs/head/catalog, and install no runtime. Restoring the original
name permits recovery of all three original nodes. A pathname reopen instead
fails on the foreign suffix; that is not a successful custody refusal.

These changes qualify active-WAL reads, not every WAL lifecycle operation.
[Held append rollback](HELD_WAL_ROLLBACK_PROOF.md) separately restores the actual
partial-write object without pathname truncation or unlink. Legacy append-handle
opening, rotation, checkpoints, sealed-object staging,
doctor sidecars, quarantine, runtime/data/root-registry paths and later writes
are not all migrated. Source-root and file-name fences are observations, not
atomic exclusion against host rename. The independent durable allocator,
positive virgin bootstrap, birth/debt journal and stationary publication
requirements remain in the [Consumer contract](../specs/CONSUMER_STATIONARY_PUBLICATION_SPEC.md).
The persistent synchronization default and WAL/head formats are unchanged.

## Compiled controls and local qualification

Two independently compiled controls change the actual replay open to a pathname
reopen or disable the final name check. The former consumes the malformed foreign
suffix; the latter incorrectly installs the recovered candidate. Each fails the
specific native replacement regression. Restoring source passes **160 tests / 0
failures / 1 existing ignored local fuzz campaign** across ownership, project/head,
immutable-root and WAL/checkpoint modules. The full facade passes **52 tests / 0
failures / 0 ignored**. The final legacy path cursor keeps its original fresh-open
behavior without an added seek; captured cursors alone rewind the validated file.
Actual control executables, source and log hashes are archived. These are local
functional checks; native Windows guarded-repair behavior and full power-loss
qualification are not inferred from them.

[Current-generation writer handoff](CAPTURED_WAL_WRITER_PROOF.md) now retains
the writable replay handle for native append and original-directory barriers.
Explicit generation transitions remain outside that capture window.

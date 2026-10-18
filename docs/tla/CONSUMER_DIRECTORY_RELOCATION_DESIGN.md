# Consumer directory publication

Status: native no-replace publication and process-local failure handling are
implemented; native Windows qualification is pending. This is not a completed
MVCC, logical-incarnation, C/E installation or Mem activation contract.

## Namespace operation

Creating the destination directory and then renaming over it fails on Windows.
Deleting a reservation before a replacing rename would introduce a race with
another publisher. The storage library instead exposes `rename_directory_new`:
Linux uses `renameat2(RENAME_NOREPLACE)`, Darwin uses
`renamex_np(RENAME_EXCL)`, and Windows uses `MoveFileW`. Unsupported native
primitives/filesystems refuse; no replacing or copy/delete fallback is allowed.
The Windows adapter resolves absolute UTF-16 local/UNC paths and adds the
verbatim prefix for long paths, while rejecting embedded NUL.

Native Darwin testing found that an exclusive rename onto the very same entry
can return success without moving anything. The wrapper therefore rejects an
already existing destination before invoking the syscall. This check never
authorizes replacement: the native exclusive operation still handles a target
that appears after the check. Tests exercise that precise window by creating
an empty target and retaining its directory identity before the syscall.

The operation uses the existing descriptor-domain admission, namespace recorder
and immutable-alias invalidation. The recorder has a distinct no-replace
simulation that requires an absent target, including empty directories and the
source itself; its observations retain the existing Rename event kind. A native
fixture verifies recorded conflicts and successful same-parent moves against
the complete captured filesystem image.

Primary API references: [Linux rename](https://man7.org/linux/man-pages/man2/rename.2.html),
[Apple exclusive rename flag](https://developer.apple.com/documentation/kernel/1646220-anonymous/vfs_rename_excl),
and [Windows MoveFileW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefilew).

## Ownership and uncertain outcomes

Consumer publication acquires its existing operation slot, revokes the binding,
and removes the old pathname as authority before attempting the mutation. It
checks the source against its held native directory. After a successful move it
checks the destination's physical identity, follows the moved pathname, and
retains native custody through barriers and reader validation/reload.

A native or bookkeeping error does not prove that the directory stayed put.
The owner observes its held root at destination/source; if neither can be
established, its locator remains absent. All these error cases remain revoked
and keep the original receipt and native lock until explicit owner closure.
They never delete a destination or reuse a vacated pathname for mutation. A
caller can close and reopen the actual directory and recover the original
receipt, including after a lost response. Even an occupied-target refusal
requires owner reopen; it does not silently restore mutation readiness.

For different lexical parents, publication synchronizes the destination parent
before the source parent and reports success only after both barriers and
reader validation. For the same parent it needs one barrier. Destination-first
ordering avoids deliberately retiring the old link before the new-link barrier.
It is not by itself proof of crash atomicity across independently persisted
parents: unsynchronized changes, storage reordering, torn metadata, filesystem
journal behavior and parent-path replacement still need composed qualification.
The native syscall and successful directory flush semantics are assumptions;
unsupported flushes fail rather than weakening the barrier.

The before/after physical source probes also do not close an external source
entry swap between probe and rename or prove historical incarnation/ABA. Full
activation still requires the immutable-output/selector and source-incarnation
protocol. The source-binding, dependency and complete-verification gates remain
closed; this correction does not turn ordinary governed serving on.

## Finite model and native correspondence

`HawDBConsumerDirectoryRelocation` now includes target occupation before and
during the move, revocation before mutation, pre-effect failure, lost outcomes
with a known/unknown locator, source-name reuse, separate successful parent
barriers, owner use, evidence retention and close. The positive check visits
40 distinct states (64 generated, depth 8).

Existing controls still violate PathFollowsRename, FailedOwnerRevoked,
NativeRetained and UncertainRetainsEvidence. Three generated one-constant
controls additionally violate NoTargetClobber (`ReplaceDestination`),
PublishedBothParents (`ReleaseBeforeSourceSync`) and
SourceSyncFollowsDestination (`SyncSourceFirst`). This is a finite protocol
check under the assumptions above, not a native pointer/filesystem refinement
or composed power-loss proof.

Native coverage includes occupied empty/populated/file targets, two racing
publishers, a target appearing after preflight, same-entry no-op rejection,
and recorder/native image correspondence. Consumer controls exercise ordinary
move and mutation, target/source parent synchronization failure, lost response,
an unlocatable moved root, source replacement, retained peer exclusion and
receipt recovery. The synthetic fixtures do not establish full-dataset serving,
physical RSS/I/O envelopes, fairness or cross-parent power-loss safety.

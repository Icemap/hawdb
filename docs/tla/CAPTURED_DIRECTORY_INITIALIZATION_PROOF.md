# Captured directory creation and catalog initialization

This increment supplies a native operation needed by stationary Consumer
component construction, and uses it in actual projection-catalog initialization.
It does not implement the Consumer component ledger, producer sealing, governed
read-view exchange or activation. Those remain subject to the
[Consumer design](CONSUMER_INSTALLED_ACTIVATION_DESIGN.md) and full MVCC ledger.

## Native operation and effect boundary

`SourceDirectory::create_directory_new` validates one generated ASCII component,
checks the same project domain and fresh native scope, and admits temporary FD
capacity before entering native creation. It uses the held parent, never the
parent's current pathname. Existing files, directories and symlinks are not
adopted, overwritten or removed. The caller supplies publication/namespace
exclusion; this primitive creates no lease or source-watermark authority.

Unix uses exclusive `mkdirat` with mode `0700` (subject to umask). Windows uses
`NtCreateFile` with the captured `RootDirectory`, `FILE_CREATE` and
`FILE_DIRECTORY_FILE`, then closes the temporary native handle. Microsoft's
[NtCreateFile contract](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntcreatefile)
defines the relative root, exclusive disposition and directory option. This is
not a directory-move workaround: no descendant directory is moved and no
custody is released to permit a move.

The Windows exclusive create path performs no fallible post-create metadata
query before the recorder sees success. The kernel disposition/type specify
what was created. Ordinary existing-file/directory opens keep their previous
metadata and no-reparse validation. An operating-system API success is assumed
to return its documented valid handle. Unexpected API/verification failure must
fail closed and retain evidence, not silently invent a successful capture.

Creation returns no child handle. A separate admitted `open_directory` can fail
after successful creation. Similarly, a later barrier or a lost response can
fail after a namespace effect. None of those errors proves rollback. No error
path automatically unlinks the name or discards the parent's native authority;
its owner must retain the actual parent/name as candidate debt or recovery
evidence. This low-level operation does not implement a persistent debt ledger.

## Actual catalog initialization

The writable catalog constructor now performs:

1. Initial path-based admission creates the catalog root if needed and completes
   containing-name barriers through the admitted project boundary.
2. Capture that root once. Keep this native authority continuously held.
3. Exclusively create `generations` and `heads` relative to this capture.
   `AlreadyExists` is a possible interrupted/earlier initialization, not proof
   of a directory's kind or identity.
4. Synchronize the captured parent's entries, including on such an earlier
   creation. A successful later head commit may depend on these directory names.
5. Open both children relative to the same parent, validate non-reparse directory
   types, then register the complete three-directory catalog identity.

After step two, replacing the original label with another valid catalog B cannot
redirect child creation or hydration to B. Paths retained in the public store
remain labels; data/head operations continue using the captured directories.
The native test invokes the actual public constructor, replaces its label in
that interval, creates/populates B and verifies later A publication/read leaves
B's full tree unchanged. Windows replacement happens before descendant hydration,
while only the original root is open, preserving the qualified native boundary.
No close/reacquire is used to force a forbidden ancestor move.

This constructor may leave partial structural directories on an error. It does
not acknowledge schema/data, recreate a branch, authorize Consumer readiness or
remove old catalog contents. Read-only `open_existing` retains its existing
non-creating path. Initial admission still resolves its supplied root path;
concurrent external movement of that root's own parent link has separate
ownership and durability obligations. The new guarantee concerns descendants
of the chosen capture, not arbitrary hostile mutations inside it or a persisted
logical-incarnation/ABA proof.

## Recorder and power-loss boundaries

`ImageEngine::create_directory_at` allocates in the held logical directory inode.
It validates a single component and checks operation/inode limits before mutation.
The native bridge simulates privately, observes before actual native creation,
commits the model only after native success and exposes an after-operation cut.
No child-open success is needed to record creation. An unrecorded or uncertain
native outcome remains a coverage/verification failure, never qualification.

A native observation/image test shows a newly created directory can be lost or
retained before its parent barrier. After synchronizing that actual parent, the
required image retains it. A separate catalog test requires both newly created
child names in the default lost-unsynchronized-writes image immediately after a
successful public `open`. This demonstrates why child data/head barriers alone
cannot cover an omitted catalog-parent entry barrier.

These are bounded physical-image observations, not a machine power-cut or a
claim that every filesystem/device honors synchronization. The required platform
contract is durable native directory synchronization and storage honoring those
barriers. Parent relocation by another owner and full WAL/checkpoint/GC recovery
composition require their separately stated protocols and qualification.

## Finite initialization model

`HawDBCapturedDirectoryInitialization` fixes the chosen root at A and permits
replacement of its external label by B between phases. It explores fresh and
interrupted initialization (each subset of the two durable child names),
creation admission refusal, the parent barrier and final child hydration.
`opened` means only an acquired catalog, not a ready Consumer or complete MVCC.

The invariants require an untouched foreign namespace, admission before creation,
complete chosen-root children and their durable entries before returning an
opened catalog. Independent controls use the path label for creation, omit the
parent barrier, or ignore refused admission. Their expected violations are
`ForeignNamespaceUntouched`, `OpenedDependenciesDurable` and `CreationAdmitted`.

The model abstracts native identities, successful OS operation effects and
atomic directory synchronization. It does not prove native FFI refinement,
hardware durability, liveness/fairness, global memory/RSS, persistent incarnation
non-reuse or complete Consumer activation. Native source-qualified regressions
and independently compiled controls are additional evidence, not replaced by a
positive TLC result.

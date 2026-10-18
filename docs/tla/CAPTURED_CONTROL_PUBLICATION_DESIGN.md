# Captured mutable-control publication

Status: storage primitive, native tests and finite protocol model. Consumer
selector encoding, initial reservation, view/receipt selection, ordinary reader
and source-GC adoption remain incomplete. This does not enable governed serving
or discharge the [MVCC acceptance ledger](../specs/MVCC_VERSIONED_PUBLICATION_SPEC.md).

## One directory and one candidate owner

`SourceDirectory::create_control_candidate` exclusively creates a regular file
relative to the held directory. It rejects existing entries and invalid names
before adopting a writable object. `CapturedControlCandidate` borrows that
directory and holds one counted native file, without retaining the constructor's
descriptor reservation active. Its held FD receipt can retain inactive inventory
metadata until closure; later native opens use the caller’s fresh operation
context. Its fixed component buffer adds no pathname lookup or
corpus-sized allocation. The caller still admits its owning metadata, I/O and
complete recovery dependencies and retains actual publication/process exclusion.
The directory handle alone is not a publication lease or logical incarnation.

The owner permits writes only before `seal`. Sealing synchronizes its bytes and
revokes writes; it does not validate a logical selector format. The producer must
encode and validate its complete record and referenced data before publication.
This API is for mutable controls, not managed immutable aliases or ownership
lock names. Immutable artifact publication and no-overwrite remain separate.

`replace_head` validates the fresh operation context and destination component,
then marks the owner attempted before native I/O. It checks the source entry
against the held candidate's full native identity before replacement, and checks
the selected head against that same held file afterward. Existing targets must
be regular files and must not alias the candidate; same-inode rename no-ops are
refused consistently in native execution and the image model. Unix uses `renameat`
with the same captured directory for both components. Windows exclusively
creates a DELETE/write-capable candidate using root-relative `NtCreateFile`,
then applies `FileRenameInfoEx` with replacement/POSIX semantics and the captured
directory as `RootDirectory`. Old held readers retain their old inode.

`select_head` returns a pending token that exclusively borrows the candidate.
The coordinator must validate/exchange its read view under the same fence and
release SourceOwner before invoking the token’s `finish` with a fresh operation
context. Dropping a pending token leaves the candidate attempted, without
rolling back the visible effect or asserting durability. `replace_head` is a
synchronous convenience over those two steps, not the final fenced Consumer
activation entry point.

After native replacement, candidate file synchronization, identity verification
and captured-directory synchronization remain separate operations. Completion
is reported only after all succeed. No retry or write is allowed once an attempt
begins, including after admission, namespace, identity or barrier failure. A
lost response may leave a complete publication. Drop closes the file; it never
unlinks either name. The caller must retain publication/recovery custody and
resolve uncertain outcomes before readiness or cleanup. This is not a Consumer
read-view/receipt acknowledgement API.

Native identities do not prove same-inode content immutability or persisted
incarnation/ABA. The caller keeps the private candidate's name and contents
immutable under its actual publication exclusion. The before/after checks do not
close every externally induced source-entry race. Captured-relative operations
do prevent a replaced root pathname selecting another directory. No claim is
made that arbitrary external filesystem writers obey the cooperative protocol.

## Recorder and native qualification

The recorder binds candidate creation and replacement to the captured directory
inode, even after its original path is reused. Paths label observations only.
Same-directory replacement records one namespace operation containing removal
of the candidate name and installation of the head. File and directory barriers
are individually recorded; a post-effect failure cannot erase a completed event.
Native namespace mutation does not recursively enter the candidate's file trace
while holding ModelCore; later file synchronization uses that trace separately.

Tests cover existing/invalid names, old held reads, root-path reuse with a foreign
head retained, source-entry replacement refusal, actual peer lock exclusion,
foreign descriptor domains, descriptor denial before mutation and after a
completed rename, forbidden late writes/retries, and native-close accounting.
Recorder tests preserve the old selector across every torn candidate-write
interval, permute/drop unsynchronized namespace operations at Before/After
rename cuts, and require the new selector after a completed directory barrier.
Both captured and foreign directories are checked. These native operations must
execute on Windows; cross-target Clippy is not runtime qualification.

The crash model assumes trustworthy completed synchronization barriers and
atomic same-directory namespace replacement, as existing storage qualification
does. Tests do not establish arbitrary storage-device behavior or complete
Consumer recovery. Persistent instances and actual C/E/read dependencies still
need their composed lost/torn/reordered-write gates.

## Finite model

`HawDBCapturedControlPublication` models captured authority independently of a
reused pathname, mutable/sealed/attempted/completed ownership, separately
successful barriers, failures before/after effect, and response ordering. Payload
versions model byte immutability, not selector decoding or semantic completeness.
Source-entry immutability, native atomicity, identity checks and caller exclusion
are premises, not a mechanized Rust/filesystem refinement.

| Generated control | Required violation |
| --- | --- |
| PathForCreate | NoForeignMutation. |
| PathForReplace | NoForeignMutation. |
| AllowLateWrites | SealedBytesFrozen. |
| RetryFailed | FailureRevoked. |
| AckEarly | AcknowledgementOrdered. |
| SkipPostFileSync | CompleteBarriers. |

The positive model checks **104 generated / 62 distinct states / depth 12**.
Native fault controls currently cover **156** torn candidate-write images,
**7** loss/reordering plans and **2** completed-barrier images. These counts do
not qualify the complete Consumer format or native Windows implementation.

The positive model also checks eventual success or explicit failure under weak
fairness of finite local work with successful initial creation/writing/sealing.
Its modeled failures occur after the attempt starts; preparation failures remain
unselected debt and are covered separately by native tests. It does not prove
I/O deadlines, resource/scheduler
fairness or full activation availability; see the independent
[stationary publication gate](STATIONARY_CONSUMER_PUBLICATION_DESIGN.md).

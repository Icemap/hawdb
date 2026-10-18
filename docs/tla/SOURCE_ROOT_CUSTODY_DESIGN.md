# Source-root custody for governed immutable readers

Status: partial native refinement with finite safety models. Captured directory
I/O, private admitted root custody, captured head reads and recorder integration
exist. Private admitted construction now installs pending captures and exact
artifact registrations; both cleanup policies consult them under actual
publication authority. Artifact hydration/reopen and cleanup discovery/unlink
still resolve paths, and ordinary readers have not adopted these registrations.
The admitted root remains private and refuses public
serving/conversion. This document does not qualify legacy readers, source-root
incarnation, actual Mem activation or complete MVCC readiness.

## Ownership and publication protocol

A source-root authority must hold the native directory incarnation and perform
child opens/unlinks relative to that authority. A pathname recheck before normal
path I/O cannot protect the interval between validation and use. Unix and Windows
implementations both need native tests for rename/replacement and typed descriptor
admission. The existing short publication lease has a different lifetime from a
reader: retaining it for an entire view would block subsequent publication.

While holding actual publication exclusion, install a pending capture for the
captured root and open its head inode. An owned short lease can then close
before reading/decoding the manifest and constructing its validated closure.
GC defers every artifact in that root while this temporary capture is live.
Before dropping it, transfer exact registrations into each shared immutable
segment and each stored mutation-term file. The returned tokens retain neither
the capture's execution admission nor the producer's publication/busy authority.

Closing the producer or parent view cannot release a child's sources. Reusing
one segment retains only that segment's content/lexical/vector families, not
parent mutation runs or unrelated manifest segments. Mutation-run IDs occupy a
separate namespace from equal-valued content IDs. A root directory reference
alone does not pin every generation. Failed/aborted construction drops the
initialized prefix and pending guard before exposing a ready view.

Both native cleanup entrypoints must consult the same registrations:

- `generation_cleanup::SearchProjectionCleanupState::run_under_publication` handles
  discovered and queued retry candidates.
- `generation_cleanup::once::PreparedCleanup::run_under_publication` handles the
  one-pass writer cleanup.

Selection and unlink must share exclusion against new registration. Existing
Arc clones keep the registration present; they cannot resurrect a zero-owner
payload after unlink selection. Final removal can proceed independently once
all users close. The directory authority must also bind cleanup to the captured
root after namespace replacement, so cleanup cannot delete a different root.

Registration nodes, shared payload/control blocks and native directory handles
require admission before allocation/open and receipts through actual destruction.
A receipt embedded in a removed BTreeMap value does not necessarily cover the
remaining tree capacity. Weak registry control blocks can outlive payload guards.
These resource obligations remain separate from the finite model below.

## Private registration implementation and current evidence

The root entry owns a BTreeMap of exact ArtifactKey registrations plus a pending
capture count, protected by the existing structural gate. A permanent admitted
base covers sparse/empty retained tree capacity; each live key retains a
conservative node/split credit, and each shared token owns its control-block
receipt. Credits are admitted before insertion. Final token drop removes its
registration under the gate, then releases receipts and RootRef outside it.
Shared token clones allocate nothing and retain one registration until their
last owner closes. These credits are conservative bounds, not complete
allocator/native-heap/RSS qualification.

Only a mutable successful publication lease can begin capture. Its physical
identity and the root's governor must match; the returned guard borrows only
construction admission. Actual writer publication threads its lease into
one-pass cleanup. Consumer open/checkpoint and admitted mutation cleanup pass
their existing lease into retry cleanup instead of reacquiring it. Pure remover
fixtures remain test-only. Pending captures and exact keys are checked for both
newly scanned and queued candidates.

Scoped macOS/Rust 1.97.1 evidence currently selects 44 passing native cases,
including real publication of two replacement generations. Independent admitted
segment and stored-term clones survive parent closure and both cleanup paths;
they remain readable, unrelated artifacts disappear, and final clone drop
permits reclamation. Stored terms release the original execution permit and read
with fresh admission. Other guards exercise pending captures before exact keys
are known, family collisions, producer closure, admission/FD failure and refund.
Strict Search all-target/all-feature Clippy passes. This is **not** the original
ordinary-reader four-case qualification: those APIs still need capture adoption.

The remaining namespace work is essential. Captured head reads and live pins do
not make later path-based hydration or discovery safe after root replacement.
Do not change only unlink to a relative operation while discovering generations
through a potentially foreign pathname: that could delete the captured root's
active artifacts using the foreign root's generation set. Adopt the same
captured authority throughout discovery, validation and unlink, and preserve
bounded evictable-file behavior for retained readers. No public serving or
activation is authorized by the registration tests.

## Native ownership lifetime that must be separated

The target contract in `MVCC_VERSIONED_PUBLICATION_SPEC.md` excludes
multi-process writing. This model has one coordinating process; it does not
implement cross-process reader registrations or distributed GC. The native
ownership/exclusion mechanism must enforce that scope for the entire lifetime
of retained views, including after their producer closes.

Current `consumer::operation::Authority` owns a
`SearchProjectionPublishLease`; `ConsumerProjection::open` acquires that native
lease before constructing its authority. Keeping an Arc to this authority in a
new reader would keep the publication lock held after the consumer closes.
Reopening a new consumer for the same root would then fail at native acquisition,
even when the old operation-reference counter is zero. Merely avoiding a busy
operation reference therefore does not solve independent-view lifetime.

The refinement needs distinct root resource/namespace custody and short
publication admission. Do not implement reader registration by cloning the
current consumer authority or retaining its publication lease. Same-process
producer reopening must coordinate with existing reader registrations for the
same captured root, while retaining native ownership against unsupported peers.
Directory/resource identity must survive producer closure independently of that
producer's operation generation. Registration lookup and last-owner removal
also need real metadata admission and allocation lifetimes; a new uncharged weak
registry is insufficient.

Add a native case that closes the producer while a child remains usable, opens
a successor producer for the same root, publishes a newer generation and runs
both cleanup paths, then reads the old child again. Also test root replacement,
final child closure and subsequent reclamation. This is a required future native
case, not evidence already obtained. The finite `RetainBusy` control catches one
abstract form of this exclusion error; it does not certify the current native
lease implementation or its process-ownership mechanism.

## Native relative-I/O primitive

`hawdb_storage::file_io::SourceDirectory` is the native directory capability.
It captures one counted native directory from an existing project context.
Child opens and unlinks use that handle rather than the original root pathname.
Each operation takes a fresh context from the same project domain, so it can
consume a new operation's reserved FD capacity instead of reusing the expired
capture inventory. A foreign project context refuses before opening/deleting.
Unix uses `openat`/`unlinkat`; Windows uses `NtCreateFile` with `RootDirectory`
and handle-based disposition. Generated artifact names are limited to one
ASCII component of at most 255 bytes, with no separators, traversal or streams.
Child opens reject symbolic links/reparse points and nonregular files. Unix
opens nonblocking before checking file type so a corrupt FIFO cannot wait for
a writer. Returned child handles own separate descriptor receipts and can
outlive the directory wrapper. They are native handles, not an unbounded cache:
the future reader must acquire them within its short admitted I/O operation.

A cached physical identity is copied while the counted root is live. Registry
lookup can compare it without native I/O under the registry gate; keep the
captured handle alive while using the key. It is not an incarnation token.
Hidden `.lock` controls can be opened relative to that same root, either
read-only/existing or read-write/create, without truncating existing bytes.
Each lock open uses fresh project FD admission. The caller must serialize
lock-file opens through the process registry: on process-owned POSIX locks,
closing another descriptor for that lock file can release an existing lock.

The primitive does not install artifact registrations or govern caller heap
allocation. Native enumeration and directory synchronization exist; complete
reader/GC adoption and crash-cut qualification remain separate obligations. `same_directory` compares held physical identities;
it does not establish logical source/binding incarnation or cross-process GC.
The private admitted root uses the capability for custody and head reads. The
publication lease now captures its directory and opens its locks relative to it.
Artifact hydration and both cleanup paths still need captured-root adoption.
Do not expose governed serving or infer the finite model's full native
refinement from this API alone.

The initial prototype refused attached recorders because it had no relative
namespace mapping. A private NativeDirectoryTrace now binds the held directory
and returned files to the same ModelCore; supported operations and remaining
qualification are recorded in [the native correspondence](RELATIVE_DIRECTORY_IMAGE_DESIGN.md).
Unbound/foreign recorder handles and unsupported platforms still refuse before
native I/O. Complete capture/GC recovery, native platform qualification and the
original activation gates remain required; the new bridge is not their proof.

Thirteen Unix native guards (eleven portable-to-Windows plus symlink/FIFO guards)
are written for cold open after root replacement, root-relative deletion that
preserves a foreign replacement, held-root identity, independent child closure,
FD admission/cleanup, fresh operation reservations, foreign-context refusal,
invalid components, nonregular files, relative nontruncating lock opens,
read-only lock handles, pre-open FD denial, and peer-process native exclusion.
Power-loss-context refusal is also explicit. The v4 primitive ran all 13 Unix
native cases successfully (0 failures, 1,996 filtered out), including the
peer-process exclusion probe. Strict all-target/all-feature Storage Clippy
passed natively and for Windows GNU; documented minimal WASM Clippy and
workspace formatting also passed. These results cover the primitive, not its
future registry/reader/GC adoption. Windows strict
compilation does not establish native Windows behavior.

Primary API contracts: [POSIX openat](https://pubs.opengroup.org/onlinepubs/9799919799/functions/open.html),
[POSIX unlinkat](https://pubs.opengroup.org/onlinepubs/9699919799/functions/unlink.html),
[Windows NtCreateFile](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntcreatefile),
and [handle-based disposition](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-setfileinformationbyhandle).
These contracts motivate the implementation; they do not qualify its FFI,
native resource accounting or platform execution.

## Publication and retained-root process-lock coordination

`root_registry::PublicationRegistration` and `RootRef` share the same inline
structural gate. Registration admits its list node before entering the gate.
When an admitted root is ready, publication clones that root's custody under
the gate, rather than opening a second `.search-source-process.lock` descriptor.
When no root is registered, the publisher installs a marker under the gate and
opens the process lock relative to its captured directory outside the gate.
The marker rejects unrelated root initialization until that publisher closes or
explicitly promotes its existing custody. An
initializing or retiring root refuses publication; callers may retry later.
Same-process exclusion applies on Windows as well as Unix.

An exclusive mutable borrow of the publication lease can promote native-only
custody into an admitted shared `RootRef`. State/control-block and list-node
capacity are admitted first, and cancellation is checked before mutation. The
actual directory and process-lock `File` move into the new state under the
registry gate; no close/reopen or extra FD is needed. Failure before that point
leaves original publisher custody intact. Subsequent promotion requests clone
the existing root only when the FD and governor domains match. A publisher that
borrows a ready root releases its redundant directory capture outside the gate.

The private full-root constructor can borrow this mutable publication lease for
construction and retain only root custody afterwards. It first checks that the
requested reader directory is that captured source within the supplied FD
context, under admitted I/O. It still refuses public serving/conversion and
needs exact pins plus directory-relative artifact hydration and GC adoption.
Consumer `Authority` has not yet adopted this promotion path: a shared Arc of
the existing producer authority cannot supply an exclusive mutable lease borrow.
No reader should retain that authority or its operation-busy slot to bypass this
remaining integration work.

The lease opens `.search_projection.publish.lock` relative to its retained
capture. Acquisition compares the named root and lock against the captured
objects before granting authority. Its publication lock closes first; process
custody then closes before the directory, and the local marker is removed last.
No native open/close, payload destruction or memory refund runs under the
structural gate. Existing ready roots also reject a new admitted reader from a
different runtime governor, even when its FD domain matches. This prevents
borrowing retained metadata paid for by an unrelated governor.

[HawDBPublicationRootCustody](HawDBPublicationRootCustody.tla) checks the split
between root initialization, root readiness/retirement, publisher registration,
process-lock acquisition, closure and marker removal. The abstraction covers
both the conservative process-owned `F_SETLK` duplicate-close hazard and the
inherited-open-description hazard of `flock`: a fork can retain an inherited
description until exec/exit, despite CLOEXEC. Positive custody therefore avoids
opening duplicate process-lock descriptors and explicitly unlocks successful
ownership before closing. Native unlock success is assumed; close is the
fallback after an OS unlock error. The single publisher abstracts its separate
publication lock. This is not a file-lock syscall or memory refinement proof.

The inherited-description refinement has 159 generated / 54 distinct states,
depth 10, with an empty remaining queue. Its positive and five independent
controls pass. The earlier 60/27/depth9 graph is historical evidence without the
inherited-description transition. Controls must produce these violations:

| Control | Fault |
| --- | --- |
| DuplicateProcessHandle | Closing a publisher's duplicate descriptor releases a retained reader's process lock. |
| IgnorePublisherDuringInit | Root initialization opens another process-lock handle while publisher custody is active. |
| UnregisterEarly | Removing the marker before native closure lets new root custody overlap the closing publisher. |
| ReopenOnPromotion | Closing native custody during promotion releases the peer lock before retained ownership is installed. |
| CloseWithoutUnlock | A fork-inherited description retains a local kernel lock after all logical/local owners close; NoUnownedLocalLock fails. |

Two deterministic native cases duplicate the open description before locking,
then keep it alive after either the final root or native-only publisher closes.
The peer must refuse while the original owner lives and succeed after its
closure while the duplicate remains. Dropping the duplicate precedes fresh
reacquisition; production custody never creates that duplicate. Both cases
reproduced the same locked-after-close failure before correction and pass after
using File::try_lock_owned. No sleep, retry, quota or deadline change is used.
The guard is created only after successful acquisition; failed acquisition must
never construct an unlock owner. Borrowed publisher closure does not unlock a
still-live root. These regressions model the fork-to-exec mechanism; they do not
prove that a fork caused each previously observed CI failure.

The native peer-process case retains a reader across failed publisher FD
admission, successful publisher acquisition and reader closure. It checks
exclusive peer refusal at each stage, three held descriptors while root and
publisher coexist (one directory, one process lock, one publication lock), final
refund, and peer success
after the final publisher closes. Separate cases cover publisher-first refusal
and retry, initialization interleaving, root/lock pathname replacement and
admitted metadata lifetimes. The promotion case occupies all four FD slots,
transfers custody without opening anything, verifies peer refusal before/after
transfer and publisher closure, reads through the surviving root, then verifies
peer success after the last reader closes. Admission refusals at both metadata
steps and cancellation preserve original custody and allow retry. Current native
execution evidence is platform
specific; a Windows cross-compile is not a native Windows test.

This is a finite safety argument for one captured physical root and cooperative
lock-file identity. It omits external lock-file replacement, logical incarnation,
exact pins, publication durability, process crashes and liveness. It does not
prove that subsequent path-based artifact mutations use the held directory.
Those limitations remain part of the required end state below.

## Finite safety model

[HawDBSourceRootCustody](HawDBSourceRootCustody.tla) models two physical roots,
three artifact identities, one producer/parent, an independent child and an
in-flight read. One publication makes the selected child source reclaimable
without its registration. Namespace replacement does not change captured native
authority. Separate selection/unlink actions share publication exclusion. Both
cleanup kinds are independently selectable. The current and preceding artifacts
are abstracted; the model is not a recovery-format or durability specification.

The pending-capture refinement has 267 generated / 84 distinct states, depth 11,
with an empty remaining queue. Its registered positive check and ten independent
negative controls pass with complete input/log evidence. OpenBegin retains the
fence until InstallPending; InstallPin may complete while cleanup is active,
transferring protection before clearing the pending root. AbortCapture clears
construction protection without returning a reader. The original 66-state graph
is historical evidence for installing exact pins before unlock. Current controls
exercise these boundaries:

| Control | Violated invariant |
| --- | --- |
| DropOnParent | SourcesLive |
| ReleaseDuringRead | SourcesLive |
| IgnoreOncePins | SourcesLive |
| IgnoreRetryPins | SourcesLive |
| PathForUnlink | ForeignRootUntouched |
| PathForRead | ReadUsesCapturedRoot |
| RetainBusy | ReaderDoesNotExcludeWriter |
| PinWholeManifest | ChildPinIsExact |
| UnlockBeforePin (release before pending registration) | SourcesLive |
| IgnorePendingCapture | SourcesLive |

`ReaderDoesNotExcludeWriter` forbids retaining the reader's publication-busy
reference after installation. It is a structural safety condition, **not** a
proof of starvation freedom, bounded native waits or writer latency. Exact child
pins forbid retaining unrelated old-manifest sources after parent closure; this
does not establish physical reclamation latency or allocator bounds.

## Required native refinement and remaining gates

Current native cleanup uses generation sets and path-based discovery/unlink,
and now consults the live private-reader registry. Ordinary serving/refresh
constructors and all captured-relative I/O paths still need adoption. Current admitted construction checks project
containment, which does not prove source incarnation or namespace authority.
Before exposing governed serving, complete captured-directory discovery,
validation/hydration/reopen and unlink, ordinary reader/refresh capture and
logical incarnation validation. Preserve the existing registration consultation
in both cleanup paths. Native controls must reproduce
stale-root replacement, final-drop/read races, producer close, segment reuse,
last-owner removal, cancellation and publication concurrent with retained views.

The finite IDs assume physical root authority and immutable artifact identities;
no dev/ino, pathname, hash or Arc equality is itself a proof of those assumptions.
The model omits crashes, persistence barriers, GC replay, torn/reordered writes,
managed cache/registry allocation, descriptor eviction, physical read budgets and
liveness. Those require native implementation/evidence and composed recovery
arguments. Closed C-ready/E mapping, complete owner/parent/fanout dependencies,
durable activation receipts, actual Mem integration and the original writer,
fairness, RSS, I/O and group-fsync qualification remain open.

The image-engine prerequisite now has captured-directory operations and pure
model guards; see [the correspondence and remaining native gaps](RELATIVE_DIRECTORY_IMAGE_DESIGN.md).
The native directory bridge remains subject to its recorded platform/full-data
qualification gates; it does not establish either production GC path or authorize
activation.

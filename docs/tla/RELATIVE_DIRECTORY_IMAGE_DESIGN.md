# Captured directories in the power-loss image engine

## Scope and current implementation

`power_loss::image::ImageEngine` can now capture a directory inode, look up one
child, create a file in that directory, and remove a file entry from it. Existing
path-based file creation delegates to the same insertion implementation.
`sync_directory(inode)` already synchronizes the captured directory's entries.
Those image-engine operations alone are not a native directory recorder.
The native bridge and its current qualification boundary are described below.

This is a prerequisite for [source-root custody](SOURCE_ROOT_CUSTODY_DESIGN.md).
The initial SourceDirectory primitive refused attached recorders. The bridge
below now binds supported operations to the same ModelCore and held native
scope; unsupported platforms and unbound/foreign recorder handles still refuse.
Native Windows execution and broader qualification remain required.

## Transition argument

Let `d` be a captured directory inode and `n` a single normalized component.
Lookup reads `current[d][n]`. Create records an EntryChange `(d,n,new_inode)`;
unlink records `(d,n,None)`. Neither operation resolves the directory pathname
again. Rename changes the namespace entries naming `d`, leaving `d` itself
unchanged. Replacing that pathname with a foreign directory `f` therefore cannot
redirect a later operation from `d` to `f`.

Inode identifiers belong to one logical ImageEngine and its cloned candidates.
They are never reused there. They are not native filesystem IDs, logical source
incarnation tokens, or transferable authority for another model instance. The
native bridge must bind them to the same ModelCore and held native
handle, not accept a caller-supplied integer as physical authority.

Child names must be exactly one normalized component, with the existing 255-byte
bound. Traversal, root paths and normalized-away suffixes are rejected. Native
SourceDirectory additionally enforces its generated ASCII filename policy.
File removal rejects directories even when empty. Removing a name leaves the
logical inode's bytes available to other names or retained handles.

Creation checks parent/absence, inode limits and operation capacity before
allocation/publication. Removal checks the directory, entry type and operation
capacity before changing the namespace. Exclusive `&mut ImageEngine` access and
checked operation counters prevent a failed admission from publishing a partial
namespace mutation. The native recorder must simulate on a candidate,
perform the actual handle-relative operation under model exclusion, and publish
that candidate only after native success, as existing ModelCore mutation paths do.

The image engine abstracts logical inodes and filesystem persistence barriers.
Its preservation of completed barriers remains conditional on the existing
filesystem/storage assumptions. It is not an independent proof of hardware
behavior, inode reuse safety or arbitrary native error parity.

## Evidence

The pinned Rust 1.97.1 compiler builds this std-only module directly with
`--edition 2024 --test -D warnings`. All **12 tests passed**, zero ignored or
filtered: eight existing image-engine regressions plus four new guards. Pinned
standalone Clippy with `--test --emit=metadata -D warnings` also passed.
These direct checks isolate a filesystem-free model and do not replace the
required Cargo/Bazel integration checks.

The new guards exercise:

- read/unlink and recreation through the captured inode after a durable directory
  rename and foreign pathname replacement, preserving the foreign file;
- lost unlink and unlink persisted before/after unsynchronized rename and
  replacement, including an unlink-only crash image;
- operation/inode admission refusal without partial creation, removal or queued
  operations, while preserving both durable file contents;
- traversal, oversized/non-normal names, non-directory parents and directory
  removal targets, with unchanged existing files.

The existing cases continue to cover completed barriers, lost relaxed writes,
torn/reordered data writes, cross-directory rename halves, same-directory head
selection, hard links and namespace/resource rejection. No deadlines, seeds,
durability rules or old assertions were changed.

## Native correspondence requirements and initial limitations

A NativeDirectoryTrace must capture the model inode and actual counted directory
under the same namespace exclusion. Open/create/unlink must call the real held
handle and generate complete before/after observations. Returned native files
need the correct NativeTrace, so subsequent writes, truncation and file barriers
remain modeled. Native failure must not commit a simulated namespace change.
Registration changes cannot race an unrecorded native operation.

ModelCore currently classifies paths and verifies snapshots through its root
pathname. Project-root relocation/replacement is therefore an additional gap:
relative mutation alone does not make snapshot verification use the captured
physical scope. Establish counted native root identity/enumeration and reject
foreign contexts or unsupported scope changes before claiming this refinement.
Keep observation path labels separate from namespace authority.

Qualify native Unix and Windows behavior, lost/torn/reordered namespace/data
writes, limits and native errors, root replacement, hard-link identity, read-only
controls, captured directory synchronization, and checkpoint/head/GC ordering.
Composing those results with exact child pins, both GC paths, logical incarnation,
C-ready/E, writer/resource/fairness gates and actual Mem adoption remains required.

## Native enumeration prerequisite (implementation under qualification)

`SourceDirectory::visit_children` uses a fresh cursor opened from the retained
native directory. Linux/Apple use `openat(root,".")` plus `fdopendir`; Windows
uses an empty-name `NtCreateFile` relative to the held root with
`FILE_DIRECTORY_FILE`, plus single-entry `NtQueryDirectoryFile` queries. The
cursor's full native identity must equal its held root before enumeration. It
never enumerates through a stored root pathname. Each cursor admits one extra
project descriptor before native open and releases that receipt after cursor
closure, including callback error and unwind. Unix descriptor duplication is
insufficient: duplicated descriptors share their open-file-description offset.
Fresh cursor ownership also makes nested and repeated scans independent.

The callback borrows one generated ASCII component and cannot retain native
scratch. Windows uses fixed aligned 4096-byte query scratch and a 255-byte name
buffer. Linux/Apple's libc owns its directory-stream allocation; its memory is
not yet charged to HawDB host memory. This primitive therefore establishes FD
admission and bounded delivered rows, not complete governed memory qualification.
Other Unix platforms currently return Unsupported for this new operation.

Unsupported names, native errors and entry-budget overflow return failure rather
than silently omitting entries. Earlier callbacks may already have run; callers
must collect under admitted capacity or stage changes and publish only after a
successful complete scan. Neither native API promises a coherent view during
concurrent namespace mutation. GC or snapshot verification must hold the actual
publication/namespace exclusion across enumeration and the actions it authorizes.
No deletion is performed by the enumerator.

Registration exclusion is released before callbacks, which may open children or
start another cursor. This cannot enable a newly attached recorder mid-scan:
`PowerLossModel::attach` requires zero open and reserved project descriptors,
while this operation retains the counted root and cursor reservation. An
already attached recorder requires a directory trace bound to that same model
before cursor admission. The trace and its root remain live across callbacks;
no engine mutex is held across the callback. This relies on the same-project
check and attachment invariant; changing either requires rechecking the argument.
Enumeration alone neither mutates the model nor certifies a complete source.

Primary API contracts:

- [POSIX fdopendir](https://pubs.opengroup.org/onlinepubs/9799919799/functions/fdopendir.html)
  and [readdir](https://pubs.opengroup.org/onlinepubs/9799919799/functions/readdir.html)
  define stream ownership, enumeration/error handling and concurrent-mutation
  limits.
- [Microsoft NtCreateFile](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntcreatefile)
  defines root-relative opens and directory/access/disposition flags. The trusted
  empty native name is private to reopening the held cursor, never a public
  component input. Actual native behavior requires Windows execution.
- [Microsoft NtQueryDirectoryFile](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntquerydirectoryfile)
  defines per-handle scanning, restart/single-entry flags, returned length and
  incomplete-buffer behavior. Incomplete records are errors, never successful
  truncated scans.

This method is not yet used by the power-loss verifier or production GC. It
cannot alone qualify root replacement, hard-link identity, publication ordering,
logical incarnation or complete MVCC source retention.

## Captured child directories

`SourceDirectory::open_directory` captures one child relative to the retained
parent, with `O_DIRECTORY|O_NOFOLLOW` on Unix or `FILE_DIRECTORY_FILE` plus
reparse-point rejection on Windows. The child acquires its own counted handle
and the same project domain without retaining the producer's FD inventory. It
can outlive its parent; subsequent child opens, enumeration and unlinks use that
child handle. Files, traversal, foreign contexts and FD denial return failure
without an escaping authority or descriptor receipt.

This supports an inductive namespace argument: the root is a held authority;
each accepted child is opened from an already held authority using one validated
component and no symlink/reparse traversal. Replacing any ancestor pathname
cannot redirect later operations on the retained child. The argument assumes
native handle-relative APIs honor these flags; it does not assert immutable
contents or protect against concurrent changes to entries *inside* that child.
Actual publication exclusion and exact source pins remain separate obligations.

Tests move both a selected parent and its captured child, install foreign
replacements at both old names, close the parent, then enumerate/read/unlink the
captured child's payload while preserving both replacements. Denied descriptor,
file-versus-directory, missing name, foreign-domain and Unix symlink cases check
resource refunds. Windows runtime behavior still requires CI/native execution.
Recorder child captures additionally require the held native directory and
logical inode binding described below; unsupported or unbound captures refuse.


## Windows cursor correction

PR head 91c6c0c's Windows native job executed all 28 selected cases: 24 passed
and four failed with access denied. Three failed on enumeration; the fourth
failed earlier at the fixture's ancestor-directory rename, before enumeration. The cross-target
Clippy pass had not established runtime support for ReOpenFile on directories.
The implementation now reopens the held directory directly with NtCreateFile,
without pathname fallback, handle duplication or shared enumeration offsets.
Its separately admitted new directory handle must have the captured root's full
native identity. The three cursor regressions remain selected; the captured-child case also
qualifies native ancestor-rename outcomes as described below. Native Windows
execution is required; local cross-compilation cannot supply that evidence.

The primary [Wine Win32 conformance test](https://github.com/wine-mirror/wine/blob/master/dlls/kernel32/tests/file.c)
explicitly exercises ReOpenFile on a directory and expects access denied even
with backup semantics. The native CI failure is retained separately from this
source reference; the proposed NT cursor still needs its own native green run.
Public child-directory opens now explicitly test rejection of an empty name,
which remains reserved for the private captured-object reopen.


## Windows ancestor rename qualification

After the NT cursor correction, head cca99b8's native selection has 27 passed
and one failed. Source-line inspection places that last failure at the fixture's
standard-library ancestor rename (line 644), while a descendant directory handle
remains open. It is not evidence of a cursor failure. Native raw logs and the
previous failed result remain retained.

The authority test keeps Unix's original move-after-child-capture order and all
final data, foreign-path and descriptor-refund assertions. Windows moves the
parent while its own handle is retained, installs a foreign directory at its old
path, then captures the child through that retained parent, moves/replaces the
child name and closes the parent. It therefore checks both replaced paths and
independent child authority using an operation order supported by this platform.
A separate Windows case attempts ancestor rename with the descendant retained:
success must preserve authority at the moved name; a native permission refusal
must leave the old namespace and bytes intact. Both branches must still enumerate
and read the retained child after parent closure and refund every descriptor.
Other errors remain failures. No production error is ignored, no public access
mask is widened, and no final assertion, seed, budget or deadline is removed.
Test-only failure-stage reporting preserves raw error identity/classification.
These are platform-specific safety checks, not proof that every host namespace
mutation succeeds or that native GC/source-custody refinement is complete.

## Retained recorder root and persistent replacement refusal

The recorder now retains a counted native root for the lifetime of ModelCore,
including when only traced files keep that core alive. Its physical key remains
backed by a live handle; it cannot become another inode's key through reuse while
that handle remains open. Attachment owns registration exclusion while opening
this read-only anchor and checking the empty directory. Both the anchor and the
empty-directory cursor are admitted before opening and close before capacity
returns on failure. Generic native filenames and existing Unix support remain
available to the recorder. On Linux, Apple and Windows the emptiness check uses
an independently admitted cursor reopened from the held directory; other Unix
platforms retain the counted standard cursor under host namespace exclusion.

Before a pathname operation joins a ModelCore, the named root must still be a
direct directory with the captured physical identity. Capture checks the root
before and after namespace verification. A persistent replacement
fails even if its complete bytes match the model. A rejected open/create/unlink
must leave both original and foreign data untouched and return its transient
capacity. The native file handles already bound to a captured logical inode can
continue their own recorded I/O while the core's anchor remains live.

On Unix, named metadata supplies the same device/inode key as the held handle.
On Windows a separately admitted temporary directory handle supplies the full
volume/128-bit identity; truncated metadata IDs are insufficient. The recorder
therefore consumes one persistent descriptor plus a Windows path-identity audit
descriptor during these checks. Existing qualification quotas remain unchanged.
Descriptor-pressure tests subtract this explicit recorder overhead before leaving
the same one production transient descriptor available; after the model and
traced files close, the complete descriptor count must still return to zero.

This guard refuses persistent replacement under host namespace exclusion. On
Linux, Apple and Windows, native snapshot verification now walks through the
captured root and descendant directory handles as described below. Other Unix
platforms preserve the legacy path walk with pre/post guards; no handle-relative
verification claim applies there. Pre/post guards alone do not prove absence of
transient replacement. The read-only verification capability alone enables no native mutation, GC or
serving activation. Relative mutations require the separate trace below; GC,
source pins and serving activation remain open.

The retained-file regression additionally moves the root on Unix while a traced
WAL handle is open, installs a foreign WAL at the old pathname, and writes and
synchronizes through the retained file. Only the captured original receives the
bytes; the foreign WAL stays unchanged. Restoring the original root name allows
complete native verification and a crash image containing the completed file
barrier. During replacement, named-root capture still refuses. Windows ancestor
rename restrictions are qualified separately; this Unix case does not claim
that the same host rename succeeds on every platform.


## Read-only captured-root verification correspondence

Status: implementation argument with targeted native regressions; not a
mechanized Rust refinement or a complete native recorder qualification. The
supported native-walk implementations are Linux, Apple and Windows. The
remaining Unix fallback is explicitly path-based and retains its previous host
exclusion premise and generic-name behavior.

`PowerLossRoot::visit_tree` treats paths only as image labels. The initial root
is the counted attachment handle. Each directory cursor is independently
reopened from a live directory, and each child is opened from that same live
parent. No descendant open resolves the recorder's root pathname. The native
interfaces are [openat](https://man7.org/linux/man-pages/man2/open.2.html) and
[NtCreateFile with RootDirectory](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntcreatefile).
No-follow/reparse flags and post-open type checks prohibit traversal through a
link. Unix also checks the no-follow entry type before opening, so verification
never opens a FIFO/device just to discover that it is unsupported. The host must
exclude concurrent child namespace/content mutation throughout verification;
this is not a concurrent filesystem snapshot algorithm.

The correspondence obligations are:

1. Every pending directory is paired with its live counted handle and one
   relative image label. Moving its ancestor cannot redirect that handle. The
   root is not duplicated; temporary files and independently owned descendants
   close before their descriptor receipts return. Errors and callback unwinds
   discard the pending work and cannot produce a PowerLossSnapshot.
2. Every native child decrements a finite remaining-entry count before opening
   or allocating its label. The bound is the complete expected image's files
   plus directories, excluding its root. More entries refuse; fewer or different
   entries fail the final set equality. Every queued native handle also needs
   admission from the unchanged project FD limit. A wide tree may therefore
   refuse with a typed descriptor error, rather than exceed that limit.
3. Native names remain lossless: Unix uses the dirent bytes as OsStr, and
   Windows decodes native UTF-16 to OsString without replacement characters.
   The public generated-ASCII SourceDirectory API retains its existing stricter
   validation. Its Windows enumeration still uses fixed scratch without a
   per-name heap allocation; generic names are a separate test-support path.
4. The verifier checks the full expected length, compares all nonempty content
   in fixed 8192-byte scratch, checks EOF where byte-range locks permit it and
   checks the final handle length. Empty locked lease files use identity/length
   without an EOF probe. Scratch size bounds temporary comparison bytes, not
   image storage, namespace sets, pending-directory labels or native cursor
   internals. Those are not a complete governed-memory qualification.
5. Both native-to-model and model-to-native file identity maps remain injective
   across different inode classes. Recorded hard links share their physical
   identity; an unrecorded equal-byte hard link merging two modeled classes,
   or a copied replacement splitting one recorded hard-link class, refuses.
   These maps compare the current namespace's alias classes; they do not retain
   a historical native file-identity binding for every modeled inode and cannot
   certify arbitrary equal-byte replacement of a singleton file or physical
   inode ABA. The complete visited file/directory sets must equal the image's
   complete expected namespace before returning success.
6. Verification runs with the model engine locked and performs only read-only
   untraced audit I/O. It cannot add namespace/data history or strengthen a
   durability barrier. Existing before/after observations and crash plans remain
   driven by actual recorded mutations. A native root identity is still not a
   logical incarnation, and this walker does not implement NativeDirectoryTrace.

Regressions exercise cold reads after root replacement with a same-length
foreign tree; Unicode names; Linux non-UTF-8 names (APFS rejects their fixture
creation); FIFO/link refusal; entry/FD denial; visitor error/unwind cleanup;
Unicode hard links and a same-length unrecorded byte change beyond the first
scratch chunk. Existing hard-link, empty locked-file and crash-image guards stay
selected. Windows must execute its native selection before its runtime behavior
is qualified; cross-target Clippy is compilation evidence only. Source pins,
both GC paths, logical incarnation and the full MVCC activation/resource gates
remain required work.


## Native relative-directory trace correspondence

The prototype now pairs a SourceDirectory's counted native handle with a private
NativeDirectoryTrace containing its ModelCore Arc, logical directory inode and
observation label. This is implementation plus targeted native evidence, not a
mechanized Rust refinement or completed power-loss/MVCC qualification. The root
identity and full native walk remain prerequisites rather than being replaced
by a successful relative operation.

Initial capture holds recorder-registration exclusion and then model-engine
exclusion. It resolves a normalized logical directory in that model, opens each
native component from the recorder's captured root, and returns the logical
inode and actual native handle together. Intermediate native directories are
separately admitted and closed before returning their capacity. Capturing the
project root reopens that same native object; Windows checks the reopened full
identity. Named-root identity audits borrow the fresh operation's inventory,
rather than the anchor's old inventory-free context. The retained SourceDirectory
context strips inventory, and dropping the producer reservation releases unused
reserved slots while its directory stays open. A retained DescriptorPermit may
still reference its inactive inventory bookkeeping; this is not a proof of
complete governed-heap custody. Initial named-root guards remain, so a new pathname capture refuses
persistent project-root replacement.

A later operation requires the same FD domain and Arc identity as the active
recorder. Logical inode numbers from two engines are not transferable authority.
An unbound handle under an active recorder refuses before native I/O. A retained
bound directory does not consult the replaced root pathname: its logical inode
and native handle both survive renaming and parent closure. File opens return a
NativeTrace for the captured logical child, so later reads, seeks, clones,
writes, truncations and file barriers continue through existing recording paths.
Observation paths are original-open labels, not namespace lookup authority; a
modeled rename changes the image's namespace independently of those labels.

For each mutable operation, the engine gate covers complete candidate simulation,
the Before observation, actual native I/O, successful candidate publication and
the After observation. A native failure cannot publish the candidate. If native
success is followed by an audit/observation error, the caller must retain the
uncertain operation's evidence; a returned error is not proof that native I/O
was rolled back. This remains a test-support model with the same native
atomicity/synchronization premises as the existing recorder.

The relative operation correspondence is:

| Native operation | Image operation and refusal boundary |
| --- | --- |
| Read regular file / existing lock | `child_inode(d,n)` and file type; no namespace mutation; return NativeTrace bound to that child. |
| Open child directory | `child_inode(d,n)` and directory type; return an independently retained trace/handle. |
| Create mutable lock if modeled absent | Admit `create_file_at(d,n)`, then Unix exclusive create or NT FILE_CREATE. A foreign existing entry refuses instead of being adopted. |
| Open mutable modeled-existing lock | Native writable open without create/truncate. A missing native entry refuses instead of being recreated. |
| Unlink child file | Admit `remove_file_at(d,n)`, then held-parent native unlink; native failure leaves modeled history unchanged. |
| Synchronize directory | Simulate `sync_directory(d)` and complete the actual barrier on that held object before publishing it. |
| Enumerate | Read-only native held-parent cursor; preserve name/row limits and release registration exclusion before reentrant callbacks. |

Unix synchronizes the held read-only directory descriptor, matching the existing
namespace-barrier path. Windows pre-admits a separate write-capable reopen of
the same held directory, checks its full native identity, and calls the real
synchronization API. No pathname fallback or successful no-op is permitted. An
upgrade admission/native failure cannot advance the model's directory barrier.
API success still does not independently establish Windows filesystem/device
namespace durability; the existing platform qualification assumptions apply.

Targeted tests preserve the existing image, hard-link, empty locked-file and
native-file guards, replacing blanket supported-platform refusal with explicit
unbound/foreign-model refusal coverage. They exercise independent child custody
after two modeled renames, reentrant enumeration reads, actual create/unlink
failures with successful retries, image/FD admission, fresh operation inventory,
last-file lifetime after model/directory close, and Before/After create/unlink/
directory-sync cuts. Relative lock-file clones produce distinct lost, complete
and torn-write images. The project-root replacement test operates only on the
captured original, preserves the foreign tree, restores the original root name,
and then audits the complete image; it is not a qualification of durability for
the external parent directory's host renames.

The existing finite source/root custody models motivate held-root reads,
relative unlink and lifetime obligations. They do not prove this native bridge,
exact source pin installation, native producer cooperation, either production
GC path, historical singleton-file identity/ABA or composition with C-ready/E
activation. Source pins and both cleanup implementations still need integration.
Native Windows execution, complete current-source facade/fuzz regressions and
the original resource/performance/fairness/activation gates remain required.

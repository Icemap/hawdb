# Captured directories in the power-loss image engine

## Scope and current implementation

`power_loss::image::ImageEngine` can now capture a directory inode, look up one
child, create a file in that directory, and remove a file entry from it. Existing
path-based file creation delegates to the same insertion implementation.
`sync_directory(inode)` already synchronizes the captured directory's entries.
These are image-engine operations; they are not a native directory recorder.

This is a prerequisite for [source-root custody](SOURCE_ROOT_CUSTODY_DESIGN.md).
SourceDirectory still refuses a power-loss context before native I/O. Do not
remove that refusal until the native bridge and its qualification are complete.

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
future native bridge must bind them to the same ModelCore and held native
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
namespace mutation. The future native recorder must simulate on a candidate,
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

## Remaining native correspondence

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
already attached recorder is refused before cursor open. This relies on the
same-project domain check and current attachment invariant; changing either
requires rechecking this argument. Native recorder mapping remains incomplete.

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
The recorder refusal continues to apply to these new child captures.


## Windows cursor correction

PR head 91c6c0c's Windows native job executed all 28 selected cases: 24 passed
and four enumeration-dependent cases failed with access denied. The cross-target
Clippy pass had not established runtime support for ReOpenFile on directories.
The implementation now reopens the held directory directly with NtCreateFile,
without pathname fallback, handle duplication or shared enumeration offsets.
Its separately admitted new directory handle must have the captured root's full
native identity. The original four regressions remain unchanged and must pass
in the native Windows job; local cross-compilation cannot supply that evidence.

The primary [Wine Win32 conformance test](https://github.com/wine-mirror/wine/blob/master/dlls/kernel32/tests/file.c)
explicitly exercises ReOpenFile on a directory and expects access denied even
with backup semantics. The native CI failure is retained separately from this
source reference; the proposed NT cursor still needs its own native green run.
Public child-directory opens now explicitly test rejection of an empty name,
which remains reserved for the private captured-object reopen.

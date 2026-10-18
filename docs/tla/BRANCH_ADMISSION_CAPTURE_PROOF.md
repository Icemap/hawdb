# Captured branch metadata and UUID ownership admission

## Implemented boundary

The facade's deferred default recovery and branch selection now enter
`GraphStore::admit_captured_branch` with the retained `CapturedProject`.
The kernel borrows that exact descriptor domain and preserves the existing
configured-limit conflict check without rediscovering the domain through a
pathname. Diagnostic/legacy paths are derived inside this entry point.

Initial and post-recovery catalog validation use `CapturedProject::locked_catalog`.
The branches directory and its metadata lock are opened relative to the held
project root. The selector and bounded catalog are read through those held
handles under that actual lock. Selector identity, project/main binding, target
UUID, Ready state and expected metadata revision remain required. A busy lock
still produces the typed retryable catalog Busy outcome. The branches handle
is reused across lock and read, avoiding a second branches-directory capture.
Facade catalog reads consume the decoded snapshot with `into_catalog`; they do
not clone a second complete catalog. The returned description carries no native
source or allocation authority.

The UUID directory is opened relative to the captured branches directory.
`DatabaseDirectoryLease::from_captured` consumes it, registers its held physical
identity, then opens/locks the original ownership inode relative to it. The
existing close-before-unregister order remains. The branch head is opened
relative to this same native owner. `read_branch_head_file` stats the opened
file and limits the read to `MAX_HEAD_BYTES + 1`, refusing excess before decoding.
It cannot stat one pathname inode and consume a different one. This is a byte
read bound, not a proof of all allocation capacity, RSS or physical device I/O.

Before entering legacy object/WAL work, before runtime materialization, and
after catalog revalidation, physical fences compare both the project and UUID
namespace with their held directories. A detected replacement or failed binding
check does not authorize runtime cleanup through the uncertain pathname; the
failed candidate retains that evidence and releases its native owners normally.
A namespace observation is never an atomic authorization of later path effects.

## Finite argument and assumptions

[HawDBBranchAdmissionCapture](HawDBBranchAdmissionCapture.tla) separates the
project name, captured project, UUID name, UUID capture, lock source, head source
and two catalog reads. Equal logical UUIDs cannot identify physical roots.
After the initial project entry check, the project name can change before any
read. After UUID capture, its name can also change before lock or head open.

The correct catalog reads use project capture 1. Correct UUID ownership uses the
held child capture; the head uses that owner. All preserve the corresponding
physical-source invariants. Each successful fence records the physical root it
observed; replacement remains enabled after the observation. The revalidation
transition abstracts only its catalog read, not the effects of intervening
runtime recovery.

Registered positive TLC: **58 generated / 37 distinct states, depth 11**, empty
remaining queue. Four separately generated controls execute the expected
invariant violations:

| Control | Violated invariant |
| --- | --- |
| PathCatalog | CatalogFromCapture |
| PathLease | LeaseFromCapture |
| PathHead | HeadFromCapture |
| SkipFence | FenceObservedOriginal |

All five Bazel targets executed. The model chooses the initial UUID physical
directory at capture. It does not establish pre-capture UUID-directory birth,
non-reused logical incarnation, an exclusion against host rename, resource
fairness, or a complete storage transaction/MVCC recovery proof.

## Native correspondence

The directory-replacement regression injects a swap after UUID directory capture
and before ownership-lock acquisition. The replacement has the same UUID name
but a malformed head and no ownership file. Admission must lock/read the original
captured directory, then refuse its changed namespace before object/WAL/runtime
path work. It preserves both heads and catalog bytes, creates no replacement
lock/runtime/data, refunds reservations/owners and retains only the project
capture. Restoring that directory permits recovery of the original graph.

A separate regression holds the original metadata lock and requires Busy, checks
that a different requested FD configuration still refuses, and qualifies writable
and read-only recovery after releasing the lock. Another changes the original
catalog revision after complete recovery but before revalidation; no runtime may
be installed and the head remains unchanged. Held-file reader tests distinguish
old/new valid heads after replacement and refuse an oversized held file even
when its old pathname now contains a small valid head.

The first native group contained a test-only deadlock: its revalidation callback
held a metadata lock while calling `write_catalog`, which obtains that same lock
itself. The actual self-deadlocked test process was terminated and archived; the
callback now calls that existing serialized writer directly. This adverse run
is not a passing qualification or evidence of a production catalog-lock hang.

## Compiled qualification

Three independently compiled native controls replace the actual catalog capture,
UUID lock acquisition or head read with a pathname operation. The catalog control
returns revision 900 from a matching-selector replacement instead of original
revision 2. The lock/head controls consume the replacement's malformed head
instead of reaching the physical-binding refusal. Each fails its intended
regression. Actual changed source, executable and log hashes are retained.
Restored source passes **63 storage tests**, including all immutable-root tests,
metadata/head readers and ownership. The full facade passes **52 tests / 0
failures / 0 ignored**, including original descriptor caps and isolated native
FD accounting. These are local native functional checks, not native Windows or
power-loss qualification of this new entry point; CI exercises it separately.

## Remaining kernel and MVCC work

Immutable-object installation/open, active and sealed WAL work, runtime/data
materialization, root-registry pinning, cleanup and later runtime writes still
include path operations. A rename after a successful fence can race them. In
particular, this entry point does not grant source allocation/publication
ownership, mint birth authority or authorize a persistent Consumer ledger.
Legacy entry points and derived-repair paths have not all been migrated.

Complete kernel directory-relative I/O and qualified source custody must precede
using that owner for the independent durable allocation/birth/debt journal and
stationary view/selector receipts specified by the
[Consumer contract](../specs/CONSUMER_STATIONARY_PUBLICATION_SPEC.md).
No durability default, head format, release authorization or full-verification
gate changes in this step.

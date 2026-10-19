# Consumer stationary publication and durable allocation custody

Status: implementation contract with checked bounded safety models. A bounded
runtime allocation/bootstrap ledger, native birth, proven no-effect settlement,
private-unused retirement, target publication lease, bounded native artifact
copy and private metadata sealing now cover separate prerequisites. Complete
C/E construction and semantic closure, stationary
generation view/selector, exposed-debt recovery and adoption below remain
**not implemented**. This document preserves the complete acceptance scope in
[MVCC_VERSIONED_PUBLICATION_SPEC.md](MVCC_VERSIONED_PUBLICATION_SPEC.md).

## Source-backed problem

`Database::create_search_projection_consumer` currently creates a sibling
`.hawdb-consumer-<project UUID>-<incarnation>-<generation>-<operation>.stage`, initializes a resident SearchIndex,
then calls `ConsumerProjection::publish_directory`. The live lexical readers
keep files open. Native Windows refuses the enclosing directory move. Closing
those readers, changing fixture order, copying after E or weakening the tests
would not implement stationary publication.

`ConsumerRegistry::load` treats missing/corrupt registry metadata as unavailable;
explicit initialization can replace that cache. `ConsumerRegistry::publish`
publishes only completed consumer records. Neither pending creation custody nor
a non-reused allocation high-water mark exists there. Registry records, their
count and a maximum selected generation must not mint an allocation sequence.

`ConsumerProjection::reserve_preparation` already excludes competing internal
mutations and rejects dirty/failed checkpoints. The development-only generation
metadata V2 codec records nonzero incarnation, generation and operation identities;
a native target lease compares them with its original recorded reservation. This
retains descriptions without granting source or receipt authority. Snapshot and
all seven resident segment families now preserve producer expectations, but the
ordinary checkpoint still serializes the complete resident dataset. These are
prerequisites, not the stationary MVCC activation path.

The durable GraphStore currently exposes a registry path and a process ownership
lease. The native lease now retains its original SourceDirectory, opens the
reserved owner lock relative to that capture, indexes process ownership by the
live physical directory identity, and closes the lock before unregistering.
`owns_directory` uses a physical binding fence rather than canonical spelling;
branch catalog/root-pin guards already consume that fence. Initial directory
capture and the owner lock each consume the existing project FD inventory.
Branch admission funds its actual native base inventory rather than an unrelated
query worker wave. Extra recovery opens still share the original project cap;
a late refusal retains the UUID owner through cleanup before returning a failed
candidate. See [the admission accounting argument](../tla/BRANCH_DESCRIPTOR_ADMISSION_PROOF.md).

`ProjectMetadata` now captures its actual project root before selector/catalog
reads and shares that root across refresh. Equal selector UUIDs in a replacement
namespace cannot retarget it. The facade transfers a catalog-free capture into
its runtime cell, retains it through deferred admission and shares it before
branch-runtime exchange. Explicit recovery/abandonment metadata shares that
same root and descriptor domain. It has no branch writer lease and does not
make the remaining pathname kernel operations race-free. See the
[metadata capture argument](../tla/PROJECT_METADATA_CAPTURE_PROOF.md) and the
[deferred handoff argument](../tla/DEFERRED_PROJECT_CAPTURE_PROOF.md).
Default/selected branch admission now also borrows that exact FD domain, reads
both catalog images under the original relative metadata lock, acquires the UUID
owner relative to that project, and reads its head through that owner. This does
not migrate object/WAL/runtime/root-registry I/O or grant ledger authority; see
[the kernel metadata boundary](../tla/BRANCH_ADMISSION_CAPTURE_PROOF.md).
Active WAL prefix verification and replay now consume one file opened through
that UUID owner, retaining it through name checks before installation. Remaining
WAL generation/doctor/runtime operations are not thereby qualified; see
[the WAL custody refinement](../tla/ACTIVE_WAL_CAPTURE_PROOF.md).
Writable captured admission also hands the same native append-capable file and
original directory into its current-generation writer and prefix/group barriers.
Generation transitions still clear that capture and use legacy paths; this does
not mint Consumer ledger/birth authority. See the
[current-generation writer argument](../tla/CAPTURED_WAL_WRITER_PROOF.md).
Pending-child completion with that capture needs five charged slots on Unix
and six on Windows, whose policy retains a write-capable candidate through the
separately admitted rename. Native Windows confirmed typed refusal at five in
CI 39165974322 and six-slot completion in CI 39167950514 for head `3261af9f`.
The separate three directory-move failures remain. The lower-budget
refusal/retry and old-reader assertions remain; no owner or barrier is dropped.

This does not migrate all GraphStore/registry I/O. Initial creation now reserves
through the original captured project and actual UUID branch owner; its bounded
journal alone does not implement native birth or stationary publication.
The separate live native primitive and its no-effect settlement subset are
described below. Capturing whatever directory a saved label names later,
even with a matching decoded database UUID, is not an acceptable substitute.
Physical SourceDirectory identities remain process-local keys, never serialized
logical incarnations or durable birth receipts. The native ownership refinement
and its limits are described in [the custody argument](../tla/ROOT_PROCESS_CUSTODY_PROOF.md#native-database-directory-ownership).

## Implemented reservation boundary

`consumer-allocations.hawdb` lives in the project root, outside disposable runtime
directories. Under the captured catalog lock, bootstrap writes and synchronizes
an empty CRC-framed ledger before publishing the bootstrap intent. Project and
bootstrap headers are V2: they designate an initialized allocation domain.
Earlier development-only V1 project databases require recreation; there is no
compatibility migration. Valid legacy graph manifests remain adoptable. This
baseline change never authorizes deletion of authoritative legacy or Cloud data.

The kernel obtains database identity from the admitted store, verifies the ready
branch against its original UUID directory owner, and reserves three checked
monotonic role counters before stage creation or initializer callbacks. A sealed
control candidate replaces the ledger with file and parent barriers before a
reservation returns. Failures and lost replies do not refund selected identities.
An empty orphan bootstrap ledger retains its identity on retry. Existing intent,
catalog or selector evidence with a missing ledger refuses initialization rather
than recreating zero. Missing/corrupt evidence also refuses Consumer creation,
status, readiness and activation, while ordinary graph writes remain available.

The canonical bounded codec admits at most 1 MiB and 256 pending reservations;
locators are UTF-8 and at most 16 KiB. Legacy creations, exposed known-born and
uncertain attempts remain retained. Live proven no-effect attempts and completed
private births that never granted producer access can settle;
the 257th unresolved reservation fails closed. Registration/projection/checkpoint UUIDs are bound in
the record, but its locator and counters grant no native birth, adoption or
deletion authority. Rebind, clear and subsequent candidate producers still need
this protocol. The existing Consumer directory move and its Windows failures
remain open under #1092. This stage does not authorize Mem stable/GA activation.

See [the bootstrap/reservation refinement and its limits](../tla/CONSUMER_ALLOCATION_BOOTSTRAP_PROOF.md).

## Authorities and identity domains

The kernel now also has a separate native birth path, described in
[the birth/debt refinement](../tla/CONSUMER_NATIVE_BIRTH_PROOF.md). It captures the
original parent, persists intent, exclusively creates and synchronizes the
bound marker/directory, and records the receipt before exposing a producer
directory. It retains debt on errors and close. The legacy relocating facade
does not use that path yet. A live native token can revoke creation, close its
target-parent descriptor and durably settle only proven no-effect debt without
refunding identifiers. A lost response requires fresh file/root barriers even
when the selected ledger already lacks its row; see
[the no-effect settlement refinement](../tla/CONSUMER_NO_EFFECT_SETTLEMENT_PROOF.md).
A completed private birth can also persist retirement intent, delete through its
original native custody, persist the deletion/tombstone and close target handles
before settlement; see [the private-birth retirement refinement](../tla/CONSUMER_PRIVATE_BIRTH_RETIREMENT_PROOF.md).
Exposed known-born/uncertain debt, complete producer closure, recovery resolution and
stationary serving/selection remain unfinished. Allocation ledger
V2 replaces the earlier development-only envelope without a compatibility migration.

The source project's durable allocation ledger is authoritative for reservations,
pending native effects and unresolved publications. The existing consumer
registry remains a verification/retention cache and may lag an actual selected
receipt. Deleting, rebuilding or expiring that cache never resets the ledger.

The ledger is bound to the actual source project and durable database identity.
Every reservation also binds a logical target identity, operation identity and
allocation kind. Target incarnation, immutable generation identity and operation
identity have distinct roles. A binding-changing operation reserves a fresh
incarnation; every candidate reserves a fresh generation. Reserving a candidate
does not change the active target binding. A shared monotonic allocator may
supply their numeric components, but the typed roles and complete bindings must
remain explicit. Cancellation and successful cleanup never refund identifiers;
checked exhaustion refuses before a native effect.

The ledger's admission token is kernel-created and non-forgeable from decoded
metadata. It retains the actual source-root capability and owning exclusion.
The target session retains its actual publication lease, captured parent and
stationary generation directory. An absolute path is a diagnostic/configuration
locator, not allocation, adoption, deletion or publication authority.

## Bootstrap before the first reservation

An initialized allocation domain and an unknown/missing ledger are different
states. Normal open of an initialized domain with missing/corrupt ledger evidence
fails closed for consumer allocation and activation, retaining the source and
all target evidence. Ordinary authoritative graph data must not be discarded.
A copied foreign registry cannot authorize a fresh zero counter.

First-time bootstrap requires positive evidence that this allocation domain has
never been initialized, from a validated source-root opening under its actual
owner. An absent optional field returned after failed I/O is not that evidence.
An explicit, kernel-owned bootstrap operation creates and synchronizes the empty
ledger bound to its newly chosen source identity before publishing that identity
as an initialized allocation domain. The source identity barrier must complete
before reservation, directory creation, source callbacks or producer writes.

This ordering distinguishes two interrupted cases: an unreferenced empty bootstrap
candidate has never authorized a native target effect; an initialized source
with a lost ledger cannot be recreated empty. Failure or lost response retains
bootstrap evidence. Recovery resolves the exact source identity/ledger operation
rather than blindly retrying create or choosing zero. No compatibility migration
or authoritative legacy/Cloud datastore recreation is implied.

## Native operation sequence

The following stages are separate operational/durability boundaries. The model
abstracts several barriers, not their syscall or codec implementations.

| Stage | Required effect and evidence | Failure disposition |
| --- | --- | --- |
| Admission | Reserve the existing operation's metadata/workspace, native FD capacity and actual I/O admission; acquire Consumer operation custody | Refuse before effects; no new governor/domain or relabelled task |
| Reservation | Append the non-reused identity and pending-candidate record; synchronize the complete ledger transaction and required parent metadata | No mkdir or producer callback before this barrier; uncertain reservation stays owned |
| Native birth | Exclusively create the generation at its final stationary location through the captured parent | Collision owns no entry; uncertain result is pending, never adopted by name |
| Birth receipt | Through the known-created handle, persist its complete binding/birth marker and ledger birth receipt; synchronize their dependencies | A matching installed pathname/marker alone cannot recreate original creation authority |
| Preparation | Build immutable artifacts, complete producer references, metadata and a usable query view outside SourceOwner; pin C, capture C..E and retain cumulative budgets | Cancellation/failed admission retains known/unknown debt and original resource funding |
| Seal dependencies | Synchronize the complete candidate closure and precomputed metadata; persist publication intent retaining both previous and next generations | A partial seal or lost response cannot select or retire either dependency set |
| Final E fence | Reacquire SourceOwner and target exclusion; validate complete source/binding/base/incarnation/operation dependencies | Return candidate and old-view custody for disposal outside the fence |
| Selector attempt | Under the E fence, select bounded precomputed metadata and exchange the already built view; retain an unresolved outcome and release SourceOwner with both heavy custody sets | Revoke retry-on-error; preserve both possible outcomes and previous/next roots |
| Selector barrier | Outside SourceOwner, retain target operation custody while finishing source-dependency and selector file/directory barriers; only success publishes the completed receipt/ready state | No readiness/registry acknowledgement on uncertain publication; resolve by operation and durable receipt |
| Cache acknowledgement | Mirror the actual completed receipt to the source registry under its own contract | Registry failure cannot undo the selected target or authorize identity reuse |
| Reclamation | Prove no active selector, durable branch, reader pin, pending or unresolved operation references the generation; delete through original custody, then persist the tombstone | Close native handles before refund; failed cleanup retains the ledger owner |

A reservation is not a publication receipt. A completed private birth is not a
validated source epoch. A precomputed digest is not semantic-closure validation.
A selector must bind the complete generation metadata reference, source E,
binding/incarnation and operation identity. Cached `checkpoint_uuid` or a new
watermark alone cannot acknowledge activation.

I/O and descriptor admission for the bounded selection is obtained before the
E fence. The prebuilt-view exchange is not readiness acknowledgement. While its
outcome/barriers are pending, new consumer reads/publications return typed Busy;
already admitted pinned readers retain their complete versions. Heavy candidate,
old-reader and rejection destruction happens after SourceOwner is released. A
barrier failure never blindly restores a saved pointer or reruns source equality
against a later graph head. Resolution uses the original operation, native root,
selector and persisted receipt; a later source mutation does not retroactively
uncommit the operation. Only a completed outcome permits cache acknowledgement.

The ordinary resident checkpoint cannot be repurposed as post-E activation. A
native immutable generation-backed view must be prepared before final selection.
The serving adapter must support its complete lexical/vector/document semantics;
it cannot pair an E immutable manifest with a D resident snapshot. Existing
registered-root rejections remain until this path is real.

## Lost responses, close and recovery

Before selector durability, both old and new selections may be possible after
power loss. Both must therefore remain rooted before the native selection, not
only after its successful response. A complete new selection with a lost reply
is a committed possibility, never proof of rollback. No new target publication
may bypass the unresolved operation by recomputing an earlier equality check.

Closing a caller or releasing an execution lease does not delete a reservation,
branch or candidate. Idle debt retains accounted metadata/native custody without
busy execution slots. A retry uses fresh actual admission; it does not create a
governor, reopen a saved path or clear the original reservation to pass a limit.
Recovery reconstructs ledger custody with admitted bounded pages/streams.

An unknown creation without an actual recorded birth remains quarantined. Neither
matching names, inodes, encoded lengths, self-consistent metadata nor an object
hash alone proves original creation. Verified durable birth/producer/receipt
records may support recovery under the actual root authority; the process-local
writer-provenance flag is not a substitute for those persisted records.

A missing ledger quarantines the allocation domain. It must not silently erase
its pending owners or recreate an empty branch/target. Explicit restore/import
must establish a new qualified identity/lineage or recover the complete prior
ledger; copying files or accepting a foreign path is not that operation.

Synchronous and relaxed host DDL/DML modes use the same mandatory allocation,
birth, selector and branch-publication barriers. `SyncOnEveryWrite` remains the
persistent default. A successful checkpoint/seal covers the defined relaxed
window; machine-crash qualification must still distinguish lost unsynchronized
writes, torn records and reordered writes. Process-kill tests alone are weaker.

## Concrete integration work and qualification

1. Establish an owned source-root access contract from the actual durable
   kernel/branch owner, with same-project descriptor and original task admission.
   Prove source-label replacement cannot redirect ledger I/O. Do not implement
   this as a convenience path getter or another process lock acquisition.
2. Implement the allocation/birth/debt journal and bootstrap state through that
   contract, adopting the existing captured-control and power-loss image machinery.
   Limits refuse before effects; torn/uncertain publication retains evidence.
   Wire actual registration/rebind/clear/candidate producers to reserve first.
3. Construct generations at their final locations and build the immutable serving
   adapter. Replace the facade's sibling-stage directory relocation with real
   selector/receipt publication, preserving typed failure/retention outcomes.
4. Bind the existing C..E session to actual Consumer ledger, mapping, owner/parent,
   fanout and external-vector receipts. Complete E validation and bounded late
   activation; return heavy success/rejection custody outside SourceOwner.
5. Exercise every stage's lost-response and crash cut, including non-reuse after
   failed creation, registry replacement, close/restart and missing-ledger refusal.
   Test cancellation, tiny budgets, fresh retry, retained old readers/branches,
   malformed references, actual Windows stationary operation and eventual known
   debt reclamation. Keep current adverse relocation cases enabled until their
   real protocol obligations are replaced, not skipped.
6. Complete native Mem library adoption, full bootstrap import/verification,
   original physical memory/I/O and 1/4/8-writer latency/fairness/group-fsync gates,
   and source-linked composed recovery proofs. Preserve stable/GA HawDB exclusion.

The bounded [allocation custody model](../tla/CONSUMER_INCARNATION_CUSTODY_PROOF.md)
checks an ordering contract for fresh installation identities and debt. It does
not implement any step above or certify collectors, birth markers, codecs,
resource limits, serving semantics, complete C..E mapping, liveness or MVCC.

The primary generation input now has an [actual-file handoff refinement](../tla/HELD_GENERATION_SPOOL_PROOF.md). It preserves original input through single/partitioned preparation; auxiliary temporary sources and stationary artifact/serving integration remain open.

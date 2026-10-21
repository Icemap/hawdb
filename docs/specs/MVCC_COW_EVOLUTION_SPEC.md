# COW MVCC Lifecycle and Transaction Validation

## Status and scope

Status: proposed for review, October 21, 2026. This is an evolution plan for
HawDB's existing COW MVCC implementation. Requirement keywords describe the
proposed acceptance contract; they do not announce implemented capabilities.

Tracking: [#1136](https://github.com/nowledge-co/hawdb/issues/1136). Agent work
packages and their delivery acceptance are recorded in
[#1137](https://github.com/nowledge-co/hawdb/issues/1137),
[#1138](https://github.com/nowledge-co/hawdb/issues/1138),
[#1139](https://github.com/nowledge-co/hawdb/issues/1139), and
[#1140](https://github.com/nowledge-co/hawdb/issues/1140).

Retain immutable COW snapshots, transaction-private workspaces, and one ordered
WAL publication stream. Make ordinary snapshot ownership automatic, encapsulate
persistent search Consumer lifecycle, and improve validation using complete
query dependencies. General serializable transactions are a separate, opt-in
follow-up; Snapshot Isolation remains the current delivery target.

The [commit validation protocol](../MVCC_COMMIT_VALIDATION_PROTOCOL.md),
[versioned publication specification](MVCC_VERSIONED_PUBLICATION_SPEC.md),
[Consumer public contract](../SEARCH_PROJECTION_CONSUMER_PUBLIC_API_PROPOSAL.md),
and [embedded runtime contract](EMBEDDED_RUNTIME_SPEC.md) govern their existing
surfaces. This proposal does not replace their remaining acceptance criteria.
In particular, #1043, #1092, and #1106 retain source-coherent catch-up and positive
Windows publication requirements. Optional serializability is not a new gate
for completing #1039 or #232.

## Source baseline

The audit covers main `2d77562a` and the additional publication work in
[#1039 at c519dad3](https://github.com/nowledge-co/hawdb/tree/c519dad3a42bc92188decc7954a9c74f5676f38e).
Agents MUST recheck their main baseline before implementation. An old issue's
description of absent per-key stamps is not the current implementation.

| Boundary | Existing foundation | Remaining evolution |
| --- | --- | --- |
| Read views | `DatabaseReadTransaction` owns schema/data snapshots, `PublishedReadView`, and an `Arc<ReaderPin>` | Qualify combined lifetimes under churn and consolidate ownership where duplication is demonstrated |
| Canonical values | Segmented COW maps and immutable checkpoint bases share unchanged storage | Measure retained copying and reclaim safely under long readers |
| Write conflicts | `VersionIndex` stores identity stamps, not historical user values; optimistic commits check stamps against their base epoch | Refine proven scopes while retaining conservative barriers |
| Commit | Private execution feeds the serialized commit/group-sync coordinator | Preserve validation, durability, publication, and failure ordering |
| Read dependencies | Bounded query dependency primitives support versioned projection validation | Capture complete dependencies for explicitly supported ordinary transaction reads |
| Search lifecycle | Durable registration, receipt verification, expiry, typed rebuild reasons, explicit unregister | Coordinate stable registration reuse and explicit removal through one lifecycle owner |

Source locations: [COW storage](../../crates/storage/src/cow.rs),
[version index](../../crates/storage/src/version.rs),
[read dependencies](../../crates/storage/src/store/read_dependencies.rs),
[transaction facade](../../src/api/concurrent.rs),
[commit coordinator](../../src/api/concurrent/coordinator.rs), and
[Consumer facade](../../src/api/search_projection_consumer/mod.rs).

## Ownership and retention

Three lifetimes MUST remain distinct:

| Owner | Lifetime | Release effect |
| --- | --- | --- |
| Transaction or query snapshot | One live read/workspace, including queued commit and descendant snapshots | Final owner releases its epoch/generation protection automatically |
| Search Consumer registration | Persistent identity and verified durable progress across handle closure and restart | Explicit unregister removes the registration; closing a handle releases only its runtime lease |
| Durable branch | Until explicit logical branch deletion | Handle closure, idle time, and runtime lease expiry do not remove the branch or its GC roots |

A query MUST NOT create a durable Consumer registration. A long reader MAY
retain necessary old pages and generations, but MUST NOT freeze reclamation
of unrelated eligible generations. Snapshot creation MUST share unchanged
segments rather than copy all records. Workspace refresh, rollback, savepoints,
queued commit, clone, and concurrent final drop MUST preserve the original
protection floor until every dependent owner has retired.

Removing a version from a logical chain or registry does not alone authorize
freeing its storage. Reclaim MUST also respect physical-generation roots,
branch references, candidate/uncertain publication roots, lazy child reads, and
in-flight native operations. An open file alone does not protect other files
that the reader will need later. The existing durable GC protocol remains
authoritative.

Construction scratch and execution permits MUST retire after construction.
Long-lived readers retain only their actual data, metadata, capacity charges,
and required source custody. No new default governor or independent task may
be created inside a borrowed operation. Resource refusal MUST preserve the old
published state; snapshots MUST NOT be silently cancelled to recover capacity.

## Ordinary transaction protocol

The existing sequence remains:

1. Capture a coherent schema/data view and its protection before it escapes.
2. Execute parameterized Cypher/SQL against a private workspace, with
   read-your-own-writes and bounded statement/result resources.
3. Enter the commit sequencer and stage against the current canonical state.
4. Validate the complete applicable write identities and schema/database
   barriers before WAL publication. A narrower identity requires a complete
   collector; uncertain paths retain the conservative barrier.
5. Complete the selected durability policy's WAL requirements, install the
   coherent schema/data result, and publish the read view under the existing
   coordinator protocol. Group-commit callers wait for the shared barrier.
6. Release workspace/pin/admission ownership on completion or rollback.

The implementation may advance internal roots before a group fsync; these
roots MUST NOT escape as an acknowledged concurrent read publication until the
group barrier completes. A failed or uncertain barrier preserves evidence and
invalidates unsafe access. A lost response may correspond to a committed
transaction; it is not proof of rollback.

`SyncOnEveryWrite` remains the persistent default. Explicit `SyncOnCheckpoint`
may lose recent unsynchronized DDL/DML after power failure, but MUST NOT expose
partial transactions. Branch creation, deletion, and head publication retain
their stronger durability requirements in both modes.

## Search Consumer lifecycle

An application-owned, route-neutral lifecycle coordinator SHOULD reuse the
existing embedded Rust Consumer operations. It owns the stable logical Consumer
ID, the source/project/branch binding, projection definition, and the currently
verified runtime handle. It does not manufacture a new registration per request
or process start. A new public method is justified only by a reusable ownership
or recovery boundary that existing operations cannot express safely.

| Operation | Required result |
| --- | --- |
| First creation | Initialize from one pinned complete source; publish the matching registration/receipt only after successful initialization |
| Restart or handle reacquisition | Reopen the same registration and verify its receipt and binding before serving or advancing its cursor |
| Catch-up | Use the source-coherent preparation/activation contract; an intermediate batch cursor is not a complete watermark |
| Close or Drop | Release the runtime owner; retain registration and projection artifacts |
| Expiry or lost replay window | Return the existing typed rebuild reason; require explicit rebuild/re-registration rather than silently resetting progress |
| Explicit removal | Delegate idempotent unregister; revoke old handles through registration identity; preserve projection files unless a separately authorized cleanup operation removes them |
| Registry full | Refuse before initializer callbacks or allocation/publication effects; report the occupied registration capacity |

The current production registry cap remains 64 records, including retained
expired/invalid records. Ordinary readers do not consume those slots. Capacity
is an explicit policy, not a requirement of MVCC. Small lifecycle fixtures MAY
use a private capacity of two while the real codec boundary still verifies
64/65 records. No public environment variable or configuration expansion is
part of this proposal.

Missing/corrupt evidence MUST fail closed for the affected Consumer. Reopening
a path with matching text or UUID does not establish original native custody.
Preserve the current Windows capability refusal until the actual stationary
registered path is qualified positively. A lifecycle wrapper alone cannot
satisfy #1043, #1092, or #1106.

## Complete transaction read dependencies

The proposed internal handoff consists of a transaction-owned opaque dependency
set, its source instance/branch identity, base epoch, schema identity, and a
sticky completeness state. Reuse `GraphReadDependencies` and existing scope
identities where their contracts match; do not create a second public storage
integration endpoint. The query runtime collects, storage validates, and the
transaction owns the set until commit or abort.

All reads that influence a supported writing transaction MUST participate,
including reads from earlier statements. Coverage includes point absence,
empty ranges, indexed and scanned membership, projected payloads, join inputs,
aggregation inputs, adjacency, and predicates folded to an empty plan. Access
paths and expression semantics MUST agree with ordinary query execution for
NULL, duplicates, collation, and type comparisons.

Exact keys or prefixes are permitted only when completeness is established.
Otherwise capture a complete table/label/domain or database barrier before
execution. A whole-database barrier cannot validate external vectors or other
host inputs; those require explicit immutable identity/version evidence or an
unsupported result. Unsupported semantics MUST NOT silently downgrade the
transaction's requested isolation.

Budget/cancellation/capture refusal remains sticky. A caught error, failed
statement, savepoint restore, or optimizer rewrite MUST NOT turn an incomplete
capture into a valid proof. Conservative retention of extra dependencies is
safe. Add no host-side graph scans, joins, filters, or per-route executors.

The first coverage set is bounded point/equality-prefix reads, complete domain
scans, and the existing qualified SQL join shapes. Broader ranges, variable
length traversal, aggregates, and unsupported expressions may use a proven
complete conservative scope or explicitly refuse the optional mode. Publish
the actual supported-shape matrix with the implementation.

## Optional serializable transaction validation

This later capability retains COW value storage. Transaction isolation and
optimistic/pessimistic lock mode are separate concepts. Any new isolation
selection MUST be explicit in typed Rust transaction configuration; existing
constructors retain their current behavior. SQL isolation syntax requires its
own parser/runtime implementation and tests before it is documented as usable.

Under the commit sequencer, validate the complete read dependencies against
commits since the base snapshot, then validate/stage the write set and preserve
exclusion through canonical publication. Predicate/domain stamps MUST account
for insertions, deletions, and both old and new memberships of updates. The
existing version-summary approach is acceptable; adopting HyPer's predicate
tree or before-image representation is not a prerequisite.

A detected conflict returns a typed retryable transaction error before this
transaction appends WAL or changes the live view. Incomplete/unsupported capture
returns a distinct capability/execution refusal. Existing pessimistic and
conflict-noop rebasing MUST NOT bypass serializable validation. Retrying or
re-executing an application transaction remains explicit host behavior.

Read-only transactions retain their coherent starting snapshot and need no
persistent registration. Processes do not resume live transactions after a
restart. Resetting process-local conflict history is safe only because those
transaction owners no longer exist; canonical recovery still preserves whole
commits and their durability boundaries.

Acceptance MUST include write skew across disjoint rows, phantom insertion
after an empty read, updates entering/leaving a predicate, delete/reinsert ABA,
schema changes, mixed graph/SQL reads, and transaction-wide multi-statement
dependencies. For each supported schedule, compare committed outcomes with a
serial reference. Conservatively aborting a schedule is acceptable; claiming
general serializability from a few point-key tests is not.

## Delivery packages and agent ownership

Use four substantial work packages. Each agent audits main first and records
which prerequisites already exist. This plan creates implementation work items;
it does not mark implementation or qualification complete.

| Package | Owned boundary | Dependency and handoff |
| --- | --- | --- |
| [A: snapshot lifetime and retention](https://github.com/nowledge-co/hawdb/issues/1137) | Storage COW/version pins, facade reader ownership, reclamation/resource qualification | Can start independently; provides retained-owner invariants and measurements |
| [B: persistent Consumer lifecycle](https://github.com/nowledge-co/hawdb/issues/1138) | Consumer facade/coordinator, stable-ID reopen/removal, lifecycle integration tests | Can start independently; consumes the existing backend and keeps backend correctness gates open |
| [C: transaction read dependency capture](https://github.com/nowledge-co/hawdb/issues/1139) | Query-runtime collectors, expression/access-path completeness, opaque internal handoff | Later serializability foundation; can develop alongside A/B; hands complete or explicitly refused captures to D |
| [D: opt-in serializable commit](https://github.com/nowledge-co/hawdb/issues/1140) | Transaction configuration/state, commit validation, typed errors and schedule tests | Design against C's handoff in parallel; integrated completion depends on C |

A and C both use `version.rs`; A owns retention/accounting, C owns read identity
semantics. C and D agree on the internal handoff before editing common facade
state. B owns Consumer lifecycle and does not rewrite the stationary selector
or recovery kernel. Changes to shared files require coordination rather than
duplicate competing implementations.

Every implementation PR targets `main` directly and references its issue.
Land a prerequisite before building a dependent PR on main, or keep dependent
commits in one coherent PR. No stacked PR bases or new crate split is required.
Existing #1043/#1092/#1106 and #867/#1008 retain their own acceptance and are not
closed by these packages. #231 is currently closed; its historical text is a
reference, not a reason to recreate delivered foundations.

## Delivery acceptance and evidence

Each child issue MUST contain its own unchecked, observable acceptance list,
owned files, dependencies, focused commands, and remaining platform work. A
checkmark requires evidence from the delivered revision. Unit helpers alone
do not qualify facade or host integration.

| Package | Minimum delivery evidence |
| --- | --- |
| A | Old reader stays correct through writes/checkpoints; eligible unrelated generations reclaim; final drop releases only its own roots/charges; cancellation, queued commit, clone/drop races and low-budget refusal preserve state; measured COW/retained allocation on the same fixture |
| B | Repeated open/restart reuses one registration; queries allocate none; explicit removal revokes stale handles and permits recreation; invalid/expired/full registry behavior remains typed and bounded; source-coherent serving gates remain explicit |
| C | Coverage matrix for actual query plans; empty/negative reads and old/new predicate membership invalidate; disjoint scopes remain valid where proven; unsupported/budget/cancellation paths never return complete proof; executor results match the ordinary path |
| D | Default SI compatibility; explicit optional-mode refusal or complete validation; write-skew/phantom/ABA/schema/mixed-query schedules; pre-WAL conflict rejection; group-sync/cancellation/reopen boundaries and serial-reference agreement |

Local verification prioritizes the changed contract and smallest meaningful
reproductions. Other supported-target and broader suites may run in CI. Every
commit still requires repository-pinned `cargo fmt --all -- --check`, strict
workspace Clippy, installed prek hooks, and affected target-specific checks
from [AGENTS.md](../../AGENTS.md). Never suppress diagnostics or treat a
zero-selected-test command as coverage. Code changes retain the repository's
explicit local fuzz requirements; fuzz targets remain outside CI.

Record the source SHA/tree, commands, selected/passed/failed counts, environment,
features, durability mode, fixture budgets, and CI links. Runtime, finite-model,
process-reopen, physical-resource, and power-loss results are distinct evidence.
Windows power-loss qualification remains a post-merge task under its existing
tracker; positive Windows publication is a separate capability requirement.
Lost unsynchronized writes, torn writes and reordering need explicit fault
models; process-kill/reopen alone is insufficient.

This documentation delivery validates links, structure, scope and issue
relationships. It makes no runtime-test or performance claim. Mem stable/GA
exclusion, schema/import contracts, and full-data activation verification remain
governed by [AGENTS.md](../../AGENTS.md).

## Research references

[Neumann, Muehlbauer and Kemper, SIGMOD 2015](https://15721.courses.cs.cmu.edu/spring2016/papers/p677-neumann.pdf)
uses in-place latest values, undo before-images, read predicates and deferred
reclamation. Its useful lessons here are complete dependency validation and
safe retirement; HawDB retains COW. The paper's main-memory results are not a
HawDB WAL, disk-layout, or power-loss qualification.

[TiKV's Percolator overview](https://tikv.org/deep-dive/distributed-transaction/percolator/)
and [TiDB GC overview](https://docs.pingcap.com/tidb/stable/garbage-collection-overview/)
describe timestamped versions and GC safe points.
[TiCDC's service safe point](https://docs.pingcap.com/tidb/v6.5/ticdc-faq/)
illustrates why asynchronous consumers have retention progress distinct from
ordinary transaction lifetimes. Its TTL policy does not change HawDB's durable
branch or Consumer lifecycle contract.

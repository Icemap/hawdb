# MVCC dependency validation and versioned projection publication

Status: design, finite model and initial native publication path in progress for
#231 and #232. Full embedded integration and qualification remain incomplete.
Baseline: main `4a1fdb80472231767923330fed69d5c587f3aa50` (2026-10-15).
This specification preserves the complete acceptance scope of both issues. A
model pass, a dependency helper, or an isolated engine benchmark does not by
itself deliver the embedded publication path.

## Existing implementation and the missing contract

The current [commit protocol](../MVCC_COMMIT_VALIDATION_PROTOCOL.md) already has
immutable COW snapshots, private transaction execution, per-key write stamps,
first-committer-wins validation, serialized WAL/group publication, snapshot pins,
and estimated current/retained version-history budgets. `VersionIndex` is a
conflict index, not a second history of canonical values. Read consumers retain a
compressed conflict baseline while storage snapshots retain their original pin
floor. Restart discards process-local transactions and may reset conflict history;
that is separate from canonical WAL/checkpoint recovery.

The original descriptions in #231/#232 at `8cbe16f8` are historical. They must not
be used as evidence that these mechanisms are absent. The remaining acceptance
includes collector completeness, memory/cleanup qualification, constrained and
predicate paths, recovery composition, writer scaling/fairness, and the retained
single-stream latency gate. Existing benchmark receipts include adverse and
inconclusive latency results; this work does not replace those gates.

The newly required projection path additionally needs read-dependency validation.
Checking a write set only says that a transaction's destinations were not changed;
it does not prove that the graph, predicates, target, or receipt used to compute a
projection are still valid. A successful check is also insufficient if relevant
mutations can occur before the durable output and watermark become visible.

At this baseline, `NowledgeMemEmbeddedStore::catch_up_incremental_search_with_batch_hydrator`
in `src/nowledge_mem/incremental_search.rs` captures a batch, opens a read
transaction, hydrates it, builds its delta, drops the snapshot and applies the
incremental delta within the mutable store operation. The embedded handle's
store-wide writer ownership serializes that work. PR #1032 remains a separate,
unmerged owner/fence prerequisite at this audit; do not stack this work on its
feature branch or copy its unmerged API and claim main integration.

## Turso reference and selected lessons

The reference audit uses Turso commit
[`25550a3c8151b192ee53a2e9d290c8c3bf905439`](https://github.com/tursodatabase/turso/tree/25550a3c8151b192ee53a2e9d290c8c3bf905439).
Its [MVCC database](https://github.com/tursodatabase/turso/blob/25550a3c8151b192ee53a2e9d290c8c3bf905439/core/mvcc/database/mod.rs)
keeps per-row version chains in concurrent ordered maps, with transaction IDs or
timestamps marking visibility. Its dual cursor composes those versions with
the underlying B-tree. HawDB already retains canonical COW snapshots plus a
conflict index, so adopting that row-chain representation would require a
separate demonstrated need and recovery argument.

The [clock](https://github.com/tursodatabase/turso/blob/25550a3c8151b192ee53a2e9d290c8c3bf905439/core/mvcc/clock.rs)
allocates a commit timestamp and publishes `Preparing` under one lock, preventing
a reader from capturing a timestamp between those events. Speculative reads of
preparing transactions require dependency tracking and abort propagation. This
delivery preserves HawDB's published-snapshot path; it does not introduce those
speculative transaction dependencies.

The useful protocol lessons are explicit visibility/publication ordering,
separate written and durable prefixes in
[group commit](https://github.com/tursodatabase/turso/blob/25550a3c8151b192ee53a2e9d290c8c3bf905439/core/mvcc/database/group_commit.rs),
and [GC prerequisites](https://github.com/tursodatabase/turso/blob/25550a3c8151b192ee53a2e9d290c8c3bf905439/docs/internals/mvcc/GC.md)
that include materialization and reader/WAL position as well as transaction age.
HawDB's corresponding publication proof must compose source durability, active
generation roots and retained pins; a minimum epoch alone does not prove safe
artifact reclamation. Turso's Hermitage suite explicitly accepts SI write skew;
its implementation is not evidence for claiming general serializability here.

## Target contract

Retain Snapshot Isolation and its documented write-skew boundary. General SSI,
time travel and multi-process writing are not requirements of this delivery.
Keep one ordered graph WAL and the existing default `SyncOnEveryWrite` policy.
Production Mem integration remains a typed embedded Rust library path, subject
to the nightly/development-only release policy in `AGENTS.md`.

One publication attempt has these states:

1. **Captured**: under the authority that owns source/target replacement, capture
   one database instance identity, target incarnation, source snapshot and batch
   cursor. Register all required epoch/generation pins before the view escapes.
   Admit bounded preparation memory, I/O and background work before allocating.
2. **Preparing**: release the outer writer owner. Execute named parameterized
   Cypher/SQL reads through the pinned runtime, collect complete dependencies,
   hydrate within row/payload/work budgets and build private staging artifacts.
   Foreground writers must actually be able to commit during this phase.
3. **Staged**: retain pins and admission leases. Complete and verify every staged
   artifact; none is reachable through the active projection selector. Candidate
   generation and cleanup ownership protect artifacts from concurrent GC.
4. **Validated**: acquire the publication authority and revalidate source
   identity, target incarnation, schema/barriers, precise data dependencies,
   base projection generation and receipt/cursor prerequisites. Establish or
   verify the graph durability barrier covering the captured source epoch.
   Rejection changes neither active output nor durable watermark.
5. **Committed**: retain that authority, or an equivalent dependency exclusion,
   across every visible publication/checkpoint step. Install a selector binding
   the complete durable artifact closure and its exact covered watermark. Complete
   the required file/directory barriers before reporting the new view as visible.
6. **Visible/acknowledged**: attach the complete published reader and return its
   durable receipt. Release preparation ownership. Once committed, cancellation
   cannot turn the attempt into a reported rollback. A lost response is recovered
   by the operation/generation identity and durable selector, not by recomputing
   whether an earlier callback returned success.

Expensive data preparation belongs outside publication ownership. The eventual
short section may still include unavoidable durability latency; measure it rather
than assuming an atomic pointer swap makes it free. Do not expose a public
`validate() -> bool` followed by an arbitrary later mutation callback: that API
would leave the validation-to-publication race open.

## Complete dependency identities

The library owns the reusable mechanism and the host owns application bindings,
receipt schemas and their predicates. Host guard reads still go through named,
parameterized runtime statements with explicit row/payload/work budgets. The
host must not replace those reads with an unbounded scan or claim that an opaque
callback has complete dependencies merely because it returned successfully.

A prepared attempt must carry non-forgeable ownership of:

- the exact `DatabaseInstanceIdentity` supplying the snapshot; path, contents and
  commit epoch are insufficient. Reopen or whole-database replacement invalidates
  a prepared token even if data and epochs are equal (#1011);
- the target identity and a monotonic incarnation, advanced for every rebind,
  including A-to-B-to-A. Equality of a current binding value is not ABA protection;
- the pinned source epoch and complete data read dependencies;
- schema and conservative database barriers, including the direct/opaque write
  paths whose narrow footprints have not been proven;
- the expected active projection generation, input cursor, source receipt and
  complete covered batch range;
- retained source/generation pins, cancellation context and admitted staging
  resources for the entire attempt, including queued publication.

Start from existing node/relationship/adjacency and qualified relational-row
identities where their write collectors are complete. Add domain/membership
identities for reads that select a set, and collect them from the AST/logical or
physical plan shape, not REST route names. A scan must record membership/range
information even when it returns zero rows. Looking only at returned rows misses
newly inserted matches and absent-key changes. All writes that can change the
predicate's result must stamp that domain, including insert/delete, indexed value
changes and supported schema/constraint transitions.

Keep these read-invalidation summaries separate from writer conflict intents.
Adding a shared label/table/membership stamp to every writer's validated write
set would incorrectly make disjoint writers in that domain conflict. Validate
the existing destination write set first, then publish the complete invalidation
summaries with that transaction's stamps at the same WAL/publication boundary.
Read validation consumes both precise stamps and summaries; writer validation
must preserve the existing disjoint-write semantics. Native refinement must
prove both completeness and this separation.

Unknown read/write shapes retain an explicit conservative barrier. Such paths
must be observable and bounded, not silently advertised as precise. Qualified
point/domain paths must admit unrelated commits without comparing the complete
database epoch for equality. Narrowing the fallback requires a collector proof
and adversarial tests; it is not a syntactic convenience change.

The read-dependency lifetime pins the same conflict-history floor as a writable
workspace. Pruning may remove stamps strictly older than that floor, not changes
that occurred after it. Compare latest relevant stamps with the capture epoch;
copying arbitrary `VersionIndex` fields out of a compressed read baseline cannot
substitute for a retained lineage pin.

## Graph/Search durability and recovery

Graph and Search remain distinct durable domains. This protocol does not promise
an atomic transaction, automatic rollback or implicit distributed commit across
them. Stage Search output privately; authoritative activation must bind its
artifact closure and progress receipt. If the existing Search backend cannot do
that without exposing intermediate output, add the backend lifecycle operation
before moving its I/O outside the owner. A host-side check around the old visible
mutation sequence is insufficient.

Reuse `crates/storage/src/projection_generation.rs` for owner-scoped bounded
candidate batches, sealing, immutable manifests and active-head publication.
`Database::projection_generation_store` already exposes this component. Its
`publish` checks durable manifest equality, expected-head CAS, non-regressing
source watermark and idempotent generation reuse, then synchronizes the head
replacement. Those checks protect the projection catalog; they do not validate
Graph dependencies, database-instance identity or a host target incarnation.
The catalog lock is not the Graph publication fence. Compose the existing
component under the source/target authority rather than inventing a second
generation catalog or treating its current token as a source-validity proof.
Search backend activation still needs an explicit integration mapping to this
closure; catalog availability alone does not establish that integration.

A durable projection must not refer to graph transactions that may disappear on
power loss. In explicit `SyncOnCheckpoint`, capturing an acknowledged graph epoch
is not proof of a durability barrier. Publication must synchronize/verify the
captured source prefix or return a typed source-not-durable refusal. It must not
silently strengthen or weaken the host's write policy. Checkpointing a later
complete prefix is sufficient if it includes the captured snapshot's dependencies.

Recovery must distinguish: uncommitted staging (eligible for conservative cleanup),
a partial selector installation (old complete selector or fail closed), and a
committed selector with a lost response (retain/reopen the complete new generation).
Never advance a watermark independently of its complete durable output or advance
it beyond the batch/receipt coverage actually established by preparation. Rebinding,
instance replacement or schema changes during preparation cause typed rejection;
a generation already durably selected is recovered by its committed identity.

Candidate pins and durable active selectors are GC roots. Durable branches remain
roots without runtime leases (#778). Runtime fault qualification must cover
lost unsynced writes, torn metadata/data and storage reordering (#820); process
termination alone does not establish hardware power-loss safety.

## Typed failure and resource contract

Distinguish dependency conflict, instance/target replacement, source-not-durable,
stale projection generation/cursor, budget refusal, cancellation before commit,
and uncertain durable publication. Preserve typed retryability. The caller owns a
bounded retry policy; retries recapture/reprepare, rather than replaying a stale
proof. Resolve an uncertain/lost commit receipt before allocating another visible
generation. Cleanup failure retains evidence and its required ownership; it does
not authorize publication or release resources still used by pending I/O.

The dependency set, query outputs, hydration, artifact builders, file handles,
queued publication and cleanup must have explicit admitted limits. Existing
64-MiB/256-MiB version payload estimates do not bound total RSS or all stage memory.
Do not claim global bounds from a finite symbolic lease in the model.

## Initial native implementation boundary

`Database::begin_prepared_read(PreparedReadLimits)` captures a canonical source
registration and an immutable query context under one owner borrow. Its wrapper
executes bounded parameterized Cypher and collects qualified node-plan label
domains before execution. Filter predicates are checked as well as operators:
relationship existence can introduce adjacency dependencies below a node scan.
Unsupported operators/predicates fail closed. Optimizer-folded `EmptyExec` keeps
an observable whole-database epoch dependency. Any ignored statement or dependency
budget error prevents `finish` from producing a usable source proof.

The storage token registers a history pin and enables node-domain invalidation
summaries for its entire lifetime. Ordinary old snapshots cannot start this
registration, because summaries may have been disabled since their capture.
Every accepted source therefore starts at the canonical tip before concurrent
writes can resume. A node insert, property mutation or deletion stamps its label
domains and the all-node domain. Unknown cold label membership stamps a separate
read-only barrier. Existing legacy/database/schema barriers remain conservative.
Destination writes are validated before these summaries are added to the staged
index; the union is admitted before WAL and installed with canonical commit.
When no prepared source exists, ordinary commits do not build those summaries.

Qualified scalar equality/membership reads now collect exact declared index
keys, including necessary EQ/IN conditions under AND in node scan predicates.
The footprint follows semantics rather than the optimizer's scan/seek cost choice.
OR/NOT and conditions outside bounded candidate phases cannot supply that bound.
The active preparation registration enables all declared scalar-index summaries
before any later query chooses its values, closing the capture-to-query gap.
Writers stamp old keys for every matching node mutation, plus new indexed values;
cold preimages use admitted projected reads of indexed properties. Shared reader/
writer key bounds and explicit metadata work/ownership bounds retain domain or
read-barrier fallbacks. Precision counts are exposed on the prepared dependencies.
See the [collector argument and controls](../tla/INDEXED_PROJECTION_READ_VALIDATION_PROOF.md).

Necessary internal-ID EQ/IN root bounds now record existing GraphNode keys,
including absent roots. Qualified non-optional single-hop matches additionally
record directed root adjacency and every matching edge/neighbor identity before
property/label filtering. A bounded metadata prepass composes live postings with
canonical descriptors without materializing relationship property values; encoded
canonical blocks still undergo full validation. Incomplete or
over-budget collection refuses the token. Bounds cannot cross a candidate
selection boundary or full-text source. See the
[conditional argument and eleven controls](../tla/ADJACENCY_PROJECTION_READ_VALIDATION_PROOF.md).
This does not yet replace the Search graph-delta builder or establish a bounded
batch coverage receipt.

`publish_prepared_projection` validates the source identity, captured watermark
and dependencies and calls the existing generation catalog under one exclusive
database borrow. Its expected-head CAS protects the catalog head. It resolves an
already selected identical generation as an idempotent committed receipt before
revalidating an old source token. New publication requires no unfinished WAL sync group and a known durable
source prefix covering the capture. The default typed policy,
`ProjectionSourceDurability::RequireKnownDurable`, refuses an uncovered prefix;
`Synchronize` explicitly synchronizes it without changing subsequent host commits.
Replaying a WAL or choosing synchronous future writes does not certify its prefix.
Runtime durability knowledge starts at the selected checkpoint, advances only
with successful file/namespace barriers, and is reset conservatively on reopen.
Existing WAL names also require a namespace barrier when not already known
persistent. The [source durability model and argument](../tla/PROJECTION_SOURCE_DURABILITY_PROOF.md)
cover these distinctions; byte-level power-loss composition remains outstanding.

Sealed generation tokens now retain candidate GC leases through their last clone.
Sealing transfers writer protection to that lease under the catalog lock. The
publisher rejects tokens from another catalog, and the collector retains active
heads, readers, sealed tokens and open writers as distinct roots. Dropping an
unpublished token makes its candidate eligible for bounded reclamation; dropping
a committed token does not remove the durable head root.

Prepared SQL hydration now supports canonical public single-table SELECTs.
A complete necessary primary-key tuple under AND records the existing relational
row intent, including absent keys and UUID coercion shared with query/writer
keys. Other admitted reads expose a Database-epoch fallback; joins, functions,
system/projection-bound sources and locking/mutation statements fail closed.
SQL and Cypher share one sticky dependency budget. Pre-parse admission uses the
execution frontend's PostgreSQL tokenizer to bound recursive parser/lowering/drop
work, with separate expression/key and existing row/payload limits. See the
[relational collector argument and ten controls](../tla/RELATIONAL_PROJECTION_READ_VALIDATION_PROOF.md).

The Search backend additionally exposes owned `prepare_publication` candidates
for ordinary and partitioned full builds, incremental updates and governed
wrappers. A short capture lease chooses the prospective generation; expensive
artifact/partition construction then uses unique private stages without that
lease. Candidates keep stage cleanup and admission ownership while queued.
Activation reacquires publication exclusion, rechecks the captured selector
(including exact bounded bytes for the existing damaged-selector rebuild path),
and retains exclusion through complete manifest-last installation. Competing
same-name candidates cannot overwrite a winner after stale rejection.

`Database::publish_prepared_search` composes the shared source/dependency/durability
validator with those closed Search activation types under one exclusive database
borrow. No arbitrary callback can implement the sealed activation trait. This
path requires an exact capture watermark, rejects active-watermark regression
and refuses a damaged base whose prior floor cannot be established; changefeed
batch coverage and host
binding/receipt authority still need their own integration. Activation still
checksums/installs artifacts and performs cleanup under the Search lease, so a
size-independent publication section has not been delivered. See the
[Search lifecycle argument and ten controls](../tla/SEARCH_PREPARED_PUBLICATION_PROOF.md).

The catalog path publishes full generations; the native Search path activates
owned backend candidates. It does not yet move actual
incremental Search preparation out of `NowledgeMemEmbeddedStoreHandle`, collect
complete graph composite/range and additional traversal/relational dependencies, validate host binding
incarnations and batch receipts, integrate source-validated Search activation
into Mem catch-up, or qualify power loss and 1/4/8-writer latency. Unsupported value/access shapes deliberately retain
label domains or read barriers; those fallbacks are not advertised as exact keys.
Those requirements remain in scope and must not be replaced by this subset.

## Formal argument and finite-model boundary

The accompanying `HawDBVersionedProjectionPublication` model treats one prepared
full projection, two projected row identities (one initially absent), an unrelated
row, point/scan reads, scoped and broad stamps, monotonic target incarnations,
instance replacement, private two-part artifacts, graph relaxed/synchronous
commits, independent durability barriers, watermark/selector installation, GC,
cancellation, crash/reopen and lost-response acknowledgement.

It keeps an independent, unpruned stamp oracle. Validation uses the production-
shaped prunable index; publication must agree with the oracle. This exposes a
collector omission or unsafe pruning instead of assuming validation is correct.
Finite negative controls remove instance, incarnation, scan scope, barrier,
source/artifact durability, GC retention or publication-fence checks; additional
controls expose early visibility, early watermark and cancellation after commit.
Reachability controls require a disjoint commit during preparation followed by
successful publication, and idempotent acknowledgement after a committed crash.

The intended induction is: complete dependency capture plus retained history
makes index acceptance equivalent to no relevant intervening mutation; publication
exclusion preserves that result through selector commit; durable source coverage
and the durable artifact closure make the selected receipt recoverable; ownership
roots preserve every required source/artifact until the decision. Later graph
writes may make a projection observably stale; the theorem concerns validity at
publication, not equality with the forever-changing live database.

The model abstracts completed file/directory barriers as atomic events. It is
not Rust refinement, a byte-level power-loss model, a proof of arbitrary query
footprint extraction, numeric memory bounds, scheduling fairness or the complete
incremental multi-batch/constraint protocol. Existing MVCC/history/GC/durability
models retain their separate obligations. Native deterministic interleavings,
source-linked collector proofs, fault images and workload qualification are
required before claiming end-to-end delivery. Safety and a reachable successful
path do not prove starvation freedom.

## Delivery and acceptance ledger

| Requirement | Current authoritative evidence | Remaining delivery |
| --- | --- | --- |
| Snapshot reads and per-key writer conflicts | Current commit protocol and native transaction/version tests | Audit collectors and compose every supported path; preserve SI boundary |
| Version retention/admission | Existing history budget/pin models and tests | Churn/long-pin allocator/RSS and cleanup-latency qualification |
| Publication protocol | [Finite safety model and 14 controls](../tla/VERSIONED_PROJECTION_PUBLICATION_PROOF.md), registered safety action and initial source-validation/catalog path | Complete source refinement and embedded integration |
| Search preparation/activation | Owned single/partitioned/update candidates, selector CAS, shared source-validation facade and lifecycle model | Mem catch-up/outer-owner migration, host bindings and bounded batch receipts, size-independent activation I/O and fault/performance composition |
| Precise read dependencies | Pinned tokens, bounded exact scalar-index/domain, internal-ID/single-hop and relational primary-tuple collectors, native cold/hot regressions | Graph composite/range/other traversal and relational multi-key/range/join/domain collectors, host dependencies and full completeness qualification |
| Invisible staging and durable activation | Existing `ProjectionGenerationStore` candidate/seal/head protocol and Search mutation components | Compose source validation and Search activation with complete receipt/closure and failure states |
| Embedded owner concurrency | Current catch-up path and unmerged #1032 audit | Integrate on main; prove a competing disjoint writer commits while preparation is paused |
| Recovery/power loss | Existing WAL/group/branch proofs, #820 fault harness | Compose new staging/selector/source barriers, cancellation, lost responses and replay |
| Scaling/fairness/latency | Existing 1/4/8-writer harness and retained adverse samples | Small/large catch-up enabled/disabled runs, p50/p95/p99, lock wait/hold time, unchanged latency gates |

Deliver sequentially against main: establish the design/model, implement dependency
capture/validation and its native refinement, implement backend staging/activation,
integrate the outer owner, then finish fault and workload qualification. These can
remain commits in one PR; do not stack PR bases. Keep #231/#232 open until their
full original and added acceptance requirements are proven.

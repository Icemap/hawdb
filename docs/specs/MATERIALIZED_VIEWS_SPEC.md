# Real-Time Materialized Views

## Status and decision boundary

**Status: exploratory proposal; no implemented materialized-view surface.**
Initial workload design: https://github.com/nowledge-co/hawdb/issues/1142.
Execution protocol follow-up: https://github.com/nowledge-co/hawdb/issues/1204.

Real-time means continuous, asynchronous incremental maintenance with explicit
source epochs and bounded resource admission. It does not promise zero lag or
make an installed view immediately ready. The first adoption remains the Mem
workloads below; the execution mechanisms are reusable engine contracts.

This document proposes engine capabilities and prospective acceptance criteria.
It does not change existing API contracts, Mem business semantics, canonical
schemas, release policy, or activation authority. Implementation and Mem
adoption require separately scoped reviews and qualification. All interfaces
and view names introduced here are proposed, unless explicitly called existing.

The source review uses HawDB main at
`97217f9a4d53bb2bca5995644fb74328a72f3879` and Mem main at
`1e21098981051f060e8fa334709be179d0d24260`. Mem links below pin that revision;
they establish workload requirements, not performance measurements.

## 1. Outcome and first delivery

HawDB should maintain selected deterministic query results as reconstructable,
durable, indexed derived relations. Mem should read these relations through the
embedded query runtime instead of repeatedly deriving the same graph counts or
maintaining another set of application counters.

The first useful delivery MUST support actual graph workloads:

1. **Entity association counts:** replace repeated `MENTIONS` aggregation in
   entity candidate ranking while preserving scoped, zero-count entities and
   the current query's multiplicity semantics.
2. **Memory and Space inventories:** maintain the distinct Active, Stored,
   Archived, and Crystals axes used across lists, statistics, MCP, and Space
   usage. Separately defined physical graph counters are outside phase one.

A SQL-only `CREATE MATERIALIZED VIEW ... COUNT(*)` demonstration over an
unrelated table does not satisfy this milestone. It must exercise graph node
updates, relationship insertion/deletion, scope changes, and lifecycle changes.

Community retrieval follows once the first delivery is correct and useful.
Source/Thread rollups and deterministic search-document inputs are later
consumers with additional contracts, not reasons to build a general streaming
platform in the first change.

### Phase-one scope and completion

Phase one is a Mem delivery, not a general streaming-platform milestone. Its
required outputs are exactly the Entity association counts and Memory/Space
inventories above. Existing pinned Mem queries and visibility/count contracts
set the supported shapes. Register their versioned definitions through the
embedded query runtime; do not hard-code Mem routes or lifecycle rules in the
engine. New primitives need reusable ownership, but do not require speculative
operators or frontends without a phase-one consumer.

Deliver the two workloads as vertical slices. Start with Memory/Space inventory
to qualify node reclassification, grouped counts and exact indexed reads, then
extend the same capture/publication path to Entity usage with one-hop expansion,
endpoint scope changes and bag counts. Both are required to complete phase one;
a usable inventory slice alone is partial delivery.

Phase-one acceptance includes a development/nightly Mem query-path integration
or shadow comparison for the designated callers, preserving response fields,
zero-count entities, scope/lifecycle rules, read-only snapshot transactions
and explicit post-commit freshness.
Record reference-query equality and actual indexed access before measuring
query latency, maintenance lag and writer overhead. Thresholds come from the
same Mem workload baseline. Engine fixtures, capture APIs, DDL syntax or a
working background scheduler alone do not complete the milestone. Stable/GA
inclusion and activation remain subject to the existing AGENTS.md prohibitions
and separately authorized gates.

Physical graph inventories, Community membership/count relations, Source and
Thread rollups, search-document inputs and health/time-window summaries are
later deliveries. Phase one also excludes arbitrary query incrementalization,
view DAGs, automatic query substitution, full SQL materialized-view DDL and a
general external changefeed. Add only capture fields, arrangements, operators,
serving indexes and read interfaces needed by the two named workloads. Graph
and relational effects still share a complete transaction boundary; unrelated
relational tables need not gain row-image capture merely to ship this phase.

### Business coverage

| Priority | Mem business | Proposed relation | Required maintenance | Read contract |
| --- | --- | --- | --- | --- |
| P0 | Entity search and community seed ranking | `entity_usage`, grouped by entity identity and both endpoint scope keys | Graph expansion, endpoint changes, bag counts, indexed grouping | Exact snapshot shared with candidate filtering and ranking |
| P0 | Memory headlines, archive inventory, Space usage | `memory_inventory`, keyed by scope and predicate version | Deterministic visibility projection, grouped counts, old/new classification | Exact snapshot; committed-source floor for refresh after commit |
| Later | Graph statistics and density inputs | `graph_inventory`, with explicit node/edge kinds and scope policy | Node/edge counts, deletion and endpoint handling | One coherent snapshot; preserve each existing response's definitions |
| P1 | Community detail, representative memories, GraphRAG candidates | `community_memory` and membership support | Graph/derived-relation join, distinct reference counts, generation fencing | One community generation and one canonical snapshot; scope before Top-K |
| P1 | Source library counts and Thread activity | `source_memory_usage`, `thread_message_activity` | Edge counts, relational grouping, retractable extrema | Exact list/detail snapshot; preserve source provenance separately |
| P2 | Search catch-up and rebuild inputs | `search_document_input` | Deterministic graph/relational projection, deletion tombstones | Complete source coverage handed to the existing search pipeline |
| P2 | Context health and activity summaries | `memory_health_inputs`, optional time buckets | Count/sum/non-null count; explicitly scheduled time transitions | Explicit source epoch and, where needed, evaluation time |

These priorities are proposed delivery order, not issue severity labels.

## 2. Mem requirements and acceptance scenarios

### 2.1 Entity association counts

[`search_entity_candidates_in_spaces`](https://github.com/nowledge-co/mem/blob/1e21098981051f060e8fa334709be179d0d24260/nmem-rs/crates/nmem-graph/src/search_projection.rs#L577)
filters Entity names/aliases, optionally expands incoming `MENTIONS`, and uses
`COUNT(m)` before ranking and limiting candidates. Community seed lookup in
the same module repeats association counting. This is an opportunity to remove
repeated derivation, not evidence that every existing execution is a full scan.

Proposed logical key and value:

```text
entity_usage(
    entity_scope_key,
    entity_id,
    memory_scope_key,
    mention_rows
)
PRIMARY KEY (entity_scope_key, entity_id, memory_scope_key)
```

The entity itself remains a canonical query input. A left join supplies zero
for an entity with no qualifying usage rows. An optional scoped sum combines
only memory scopes admitted by this request. Entity admission also remains a
query predicate; admitting the Memory endpoint does not admit the Entity.

Requirements:

- Preserve `COUNT(m)` multiplicity. It is not interchangeable with
  `COUNT(DISTINCT m)` or an Active-only count. A distinct consumer needs its own
  definition or exact reference-count support.
- Keep endpoint scope dimensions independently. Do not assume every existing
  `MENTIONS` edge is same-Space, or sum a global count and filter afterward.
- Adding/removing an edge changes its contribution exactly once. Updating an
  endpoint's relevant properties retracts the old contribution and adds the
  new one, even when the edge itself is unchanged.
- Entity name, aliases, confidence, and PageRank remain inputs to the existing
  ranking expression. This view does not calculate PageRank or replace the
  query-dependent match score.
- Do not add lifecycle filtering merely because another inventory view uses
  it. Adopting a different search-visibility policy is a separate Mem change.

Acceptance fixture: an entity with no mentions; repeated mentions from one
Memory; memories in two Spaces; an Entity outside the admitted Space set; an
endpoint scope move; edge deletion; Memory deletion; and an exact-name match
whose ordering depends on its count. View-backed results MUST match the
unmodified scoped reference query, including zeroes and duplicates.

### 2.2 Memory, archive, crystal, and Space inventories

The
[`Memory Count Contract`](https://github.com/nowledge-co/mem/blob/1e21098981051f060e8fa334709be179d0d24260/docs/design/MEMORY_COUNT_CONTRACT.md)
defines several deliberately different totals:

- **Active:** non-crystal, latest, active lifecycle.
- **Stored:** non-crystal and not removed, including archived/history rows.
- **Archived:** Stored minus Active.
- **Crystals:** retained crystal rows on their separate axis.
- **Removed:** deleted/forgotten rows, excluded from those inventories.

The proposed relation has one row per admitted scope key and predicate version,
with `active_count`, `stored_count`, and `crystal_count`. Archived is derived as
`stored_count - active_count`; do not maintain an independent drifting value.
Empty scopes return zero inventory through query semantics, not missing-as-error.

[`memory_visibility.rs`](https://github.com/nowledge-co/mem/blob/1e21098981051f060e8fa334709be179d0d24260/nmem-rs/crates/nmem-graph/src/memory_visibility.rs)
owns the predicates. Mem MUST register a versioned query expression derived
from that contract, rather than let HawDB invent its own lifecycle rules.
Changes to `is_latest`, `is_crystal`, `lifecycle_state`, relevant legacy metadata,
or scope MUST reclassify the old and new row. The first-class lifecycle column
and legacy metadata fallback retain their existing precedence. Legacy NULL
crystal values retain ordinary-Memory semantics.

Required business checks:

1. Add an Active memory: Active and Stored rise together.
2. Archive it or supersede its version: Active falls; Stored stays unchanged.
3. Remove it: Stored falls; restoring it applies its actual new classification.
4. Reclassify it as a crystal: ordinary and crystal axes change independently.
5. Move it between Spaces: old and new scoped inventories change atomically.
6. Compare list totals, headline statistics, MCP inventory, and Space usage
   against their designated predicates at the same snapshot.
7. Reopen, replay, and rebuild: all axes and zero-result cases remain identical.

Stored is the App license count basis; hosted Cloud workspace counters use a
different physical-record rail. A materialized inventory MUST NOT silently
replace Cloud usage semantics. Quota enforcement and Space deletion MUST
revalidate under the canonical write/admission transaction; an earlier view
read, even an exact one, is not mutation authority.

### 2.3 Physical graph statistics

[`graph_statistics_payload`](https://github.com/nowledge-co/mem/blob/1e21098981051f060e8fa334709be179d0d24260/nmem-rs/crates/nmem-server/src/rest_graph/payloads.rs#L528)
currently issues eleven separate node/relationship count queries. Its raw
`memory_count` predicate is not the Active headline predicate. Its
`total_nodes` formula also selects a particular set of node kinds.

Maintain named physical counters separately from lifecycle inventories.
Density is computed from the selected counters at read time. Preserve existing
field meanings and formulas during substitution; aligning an inconsistent
product surface with the Memory Count Contract requires its own reviewed
behavior change. Do not relabel a raw physical count as Active.

Acceptance compares every response field with its original query/formula,
including empty graphs, crystals, archived rows, edge deletion, and coherent
multi-counter reads. No new cross-Space/global visibility is granted.

### 2.4 Community retrieval and GraphRAG

[`community_top_memory_scoped` and related queries](https://github.com/nowledge-co/mem/blob/1e21098981051f060e8fa334709be179d0d24260/nmem-rs/crates/nmem-graph/src/repo.rs#L1818)
derive community-to-memory candidates from member Entities and `MENTIONS`.
Some paths count matching edges; others count distinct Entities. The
[`recent-memory path`](https://github.com/nowledge-co/mem/blob/1e21098981051f060e8fa334709be179d0d24260/nmem-rs/crates/nmem-server/src/rest_graph/communities.rs)
also applies visibility while obtaining enough candidates.

Proposed relation:

```text
community_memory(
    community_generation,
    community_id,
    memory_scope_key,
    memory_id,
    mention_rows,
    distinct_entity_count
)
PRIMARY KEY (
    community_generation, community_id, memory_scope_key, memory_id
)
```

Maintain `(community, memory, entity)` reference counts to distinguish duplicate
edges from distinct membership. Full matched Entity IDs, if needed, belong in
a keyed support relation with bounded retrieval, not an unbounded `COLLECT`
payload in each row.

The existing
[`community_generation` integration](https://github.com/nowledge-co/mem/blob/1e21098981051f060e8fa334709be179d0d24260/integrations/hawdb/src/community_generation.rs)
already publishes complete Community/membership generations. Generation members
are derived, not canonical relational changefeed rows. An incremental consumer
therefore needs an explicit generation dependency/replacement event. Until
that is qualified, a replacement MUST invalidate and rebuild this dependent
view. Numeric community labels alone are not durable membership identity;
preserve the stable Community identity and generation binding.

Materialize complete membership/count relations, not one global Top-K.
Scope and the route's visibility predicate MUST precede ranking and limiting
inside the query runtime. Importance, PageRank, recency, and request-specific
selection remain query inputs. Do not automatically change existing route
filters while adopting the optimization.

Acceptance: duplicate edges; removing the final edge to an Entity; changing an
Entity's community; replacing a generation while readers retain the old one;
and a higher-ranked out-of-scope Memory ahead of an in-scope Memory. The latter
MUST NOT starve the admitted candidate window. A changed generation can never
combine with counts derived from the previous membership.

### 2.5 Source library and Thread activity

[`source_write.rs`](https://github.com/nowledge-co/mem/blob/1e21098981051f060e8fa334709be179d0d24260/nmem-rs/crates/nmem-graph/src/source_write.rs#L375)
checks exact endpoint ownership and existing `SOURCED_FROM` edges, creates a
new edge, and increments `Source.memory_count`. A view can remove this manual
counter maintenance only after every relevant create/delete/relink path is
covered and the field's intended count is agreed. Candidate key:
`(source_scope_key, source_id)`, with a separately versioned visibility policy.
An optimization MUST NOT relax the existing same-Space write gate.

Thread messages are canonical Content Store records; the graph Thread is an
anchor with projected activity. The
[`ThreadActivitySnapshot` publication logic](https://github.com/nowledge-co/mem/blob/1e21098981051f060e8fa334709be179d0d24260/nmem-rs/crates/nmem-graph/src/repo.rs#L2838)
distinguishes message counts, native first/last timestamps, preserved imported
ranges, mutation timestamps, and `source_time_complete` metadata.

Proposed `thread_message_activity(scope_key, thread_id)` maintains local
message count and nullable local first/last occurrence times. Source-provided
ranges and completeness remain separate canonical inputs to the existing
resolution policy. Deleting the locally earliest message MUST NOT erase the
source's original start time. A missing timestamp is not epoch zero; occurrence
time is not import or last-mutation time.

Retractable `MIN/MAX` requires an ordered value/refcount arrangement or a
bounded indexed recomputation for an affected Thread. A single cached minimum
is insufficient after deletion. If that operator is unavailable, reject the
incremental definition and retain the existing activity publication path.

Acceptance: duplicate imports, appended/edited/deleted messages, incomplete
imports, no timestamp, removal of an extremum, retained source time ranges,
cross-Space rejected linking, source version replacement, and full deletion.
Replace manual denormalization only after these paths compare successfully.

### 2.6 Search-document inputs

[`incremental_search_projection.rs`](https://github.com/nowledge-co/mem/blob/1e21098981051f060e8fa334709be179d0d24260/integrations/hawdb/src/incremental_search_projection.rs)
already coordinates unified canonical catch-up before applying external vector
updates. This proposal must integrate with that contract, not invent a second
independent watermark that bypasses it.

A later `search_document_input` view may centralize deterministic text and
metadata assembly, keyed by `(kind, external_id)`, with scope, source epoch,
mapping version, and deletion semantics. Large content must use the existing
bounded/streaming value contracts rather than fit in one in-memory delta.

Embeddings, lexical/vector indexes, query scoring, and LLM summaries remain
separate computations. SearchIndex remains an external derived projection.
Handoff acknowledges complete input coverage and tombstones; the search
pipeline still owns its durable publication, ranking, and readiness gates.
Repeated handoff after a lost acknowledgement is idempotent. A view definition
does not make external vector generation atomic with a canonical commit.

### 2.7 Context health and time-sensitive summaries

[`Q_HEALTH_AGG` and `Q_HEALTH_STALE`](https://github.com/nowledge-co/mem/blob/1e21098981051f060e8fa334709be179d0d24260/nmem-rs/crates/nmem-graph/src/agent_context.rs#L187)
aggregate persisted `decay_score_cached`. A view over these persisted inputs
can maintain count, sum, non-null count, and the existing threshold predicate.
It does not make the cached score represent the current wall clock.

Relative windows and continuously decaying values need explicit scheduled
evaluation/bucket expiry, with evaluation time in their identity and read
evidence, or must remain query-time expressions. First delivery rejects
`now()`, randomness, network calls, LLM calls, and other volatile definitions.
An empty/non-null-free average preserves the query's NULL semantics.

## 3. Existing engine foundations and missing capabilities

| Existing primitive | Reuse | Additional work required here |
| --- | --- | --- |
| [Projection generations](PROJECTION_GENERATION_REPLACEMENT_SPEC.md) | Bounded candidate staging, seal/digest, expected-head publication, pinned readers, reclaim | Incremental operator state, base/delta manifests, generation-native indexes, exact source identity |
| [Search identity changefeed](SEARCH_PROJECTION_CHANGEFEED_SPEC.md) | Unified canonical commit identity, bounded whole-commit coverage, explicit rebuild barriers | Exact old/new graph and relational source deltas; relationship identities; no current-hydration shortcut |
| [MVCC/publication design](MVCC_VERSIONED_PUBLICATION_SPEC.md) and prepared-read APIs | Source pins, dependency collection, publication fences, source durability barrier | Qualify the complete operator/dependency set, matched read snapshots, retained delta coverage and recovery |
| [COW MVCC evolution proposal](MVCC_COW_EVOLUTION_SPEC.md) | Snapshot lifetime, persistent Consumer lifecycle and complete read-dependency work packages | Respect their separate acceptance gates; optional serializability is not a prerequisite for these read-only views |
| Cypher and relational query runtime | Typed expressions, planning, parameter binding, filters, aggregation, query admission | Materialized relation scans and incremental plans derived from bound operators |
| [SQL/PGQ design](POSTGRES_SQL_PGQ_SPEC.md) and binding/lowering modules | Future SQL graph definitions through shared IR | End-to-end MV planning/execution qualification; frontend presence alone is not a supported MV path |

At the reviewed revision no general materialized-view DDL/catalog or automatic
maintenance pipeline was found. Existing generation-bound SQL scans are bounded
full scans; they do not establish indexed point/range performance. Existing
prepared publication and MVCC work are useful foundations, not proof that all
proposed concurrency, retention, or publication cases are already qualified.

The search feed can deduplicate identities and hydrate current values. Its
complete-through cursor is not generally an exact historical result snapshot.
It cannot be reinterpreted as a tuple retraction stream. Current catch-up
source-coherence checks remain binding; none are removed by this proposal.

The execution follow-up also checks HawDB main at
`7876b78845bd2e23453b7c10e3bbcabc0315f8ea` against `SearchProjectionChange` in
`crates/storage/src/projection.rs` and detached preparation in
`src/api/search_projection_consumer/covered/preparation.rs`. The former records
identities rather than old/new tuple images; the latter releases the database
owner before artifact preparation. Neither implements an incremental query
plan, a general view catalog, or the historical publication permit proposed
below. These source observations do not establish new qualification results.

## 4. Definition, catalog, and query surface

### 4.1 Proposed first interface

Keep `hawdb` as the embedded facade. Introduce a typed **catalog/maintenance**
contract that registers a named deterministic query, not typed Mem business
operations. A definition contains:

- name, owner namespace, definition revision, and source query dialect/text;
- immutable bound constants, output schema, unique result key, and requested
  indexes; no request-time parameters in a persisted definition;
- schema/object identities, referenced property set, expression/collation
  versions, plan fingerprint, and application predicate/mapping version;
- refresh policy, explicitly supported operator set, and resource limits.

Phase one accepts the qualified graph query shapes needed by the two named
Mem workloads and lowers them into shared logical operators. View contents are
tabular; relational SQL definitions and SQL `FROM` integration are later scope
unless a designated phase-one caller requires them. For Cypher consumers,
propose a
generic `CALL hawdb.materialized_view('view_name') YIELD ...` table source
that lowers to the same relation-scan operator and composes with graph queries.
The name is a bound catalog identifier, not executable dynamic query text.
Implementing and qualifying this source is part of the first delivery; it is
not an existing procedure. Mem filtering, graph joining, ranking, and limits
stay inside the runtime rather than a host-side scan/join.

The materialized source needs indexed key/predicate pushdown and zero-fill
left-join composition with canonical Entities. Emitting every usage row from
a procedure before filtering is not the required indexed serving path. Query
composition and actual access-path evidence are part of the milestone, not
assumed consequences of adding a catalog name.

SQL `CREATE MATERIALIZED VIEW ... AS SELECT ...`, `REFRESH MATERIALIZED VIEW`,
and `DROP MATERIALIZED VIEW` can later be syntax over this catalog. Graph SQL
definitions can use SQL/PGQ once its end-to-end runtime path is qualified.
First delivery does not depend on unverified PGQ execution or invent an
`AS CYPHER` SQL dialect. Broad automatic query substitution is deferred;
initial consumers explicitly read the named view.

The compiler returns a typed unsupported-shape/dependency diagnostic before
installation. It does not silently turn an unsupported incremental definition
into scheduled full scans. Complete refresh is an explicit alternative mode.
View rows are read-only to application DML; application counters and engine
maintenance cannot both own the same result rows.

### 4.2 Catalog persistence and dependencies

Persist definitions and schema dependencies in the engine catalog as control
metadata through the existing transactional DDL mechanisms. Result rows,
operator arrangements, indexes, and progress belong in derived artifacts,
outside the canonical graph/relational mutation WAL.

Bind each view instance to database instance, durable branch/target identity
and incarnation, definition revision, and source schema identity. An opaque
projection owner key is not an authorization decision. Do not substitute a
filesystem path or a Mem Space string for this identity tuple.

DDL affecting a dependency invalidates or transactionally replaces the
definition before it can serve as current. Drop publishes a durable tombstone
and prevents an in-flight old worker from resurrecting its head. First delivery
rejects view-on-view dependencies; Community generation input is an explicitly
declared later extension, not an implicit canonical table dependency.

Restore, import, branch creation, and target replacement must validate these
identities. Initially a new branch rebuilds its own derived view. Sharing
immutable artifacts later requires qualified source-root identity and retained
GC roots; identical epoch numbers do not authorize cross-branch reuse.

## 5. Source and incremental maintenance semantics

### 5.1 Exact source coverage

For source snapshot `S_E`, definition revision `D`, and its deterministic query
`Q_D`, a published result MUST satisfy:

```text
V(D, E) = Q_D(S_E)
```

`E` is a canonical commit epoch, not wall-clock time or projection publication
epoch. A publication records its complete-through source epoch separately from
its derived publication identity. All relevant effects of a canonical
transaction are included together, including mixed graph/relational effects.

For later definitions that also consume a Community generation `G`, extend
the equality to `V(D, E, G) = Q_D(S_E, G)` and retain `G` in the manifest and
read context. A canonical epoch alone does not identify that external input.

Add an engine-owned bounded capture seam over a transaction's actual before
and after states. Produce normalized old/new rows and relationship identities
for referenced source columns. Capture inserts, deletes, key changes,
relationship endpoint changes, node deletion/cascades, and dependency-changing
DDL; no-op writes do not create false contributions. A node property change
must reach plans that depend on it even without an edge mutation.

Persist exact capture in a **derived** commit journal when feasible. Coverage
certifies a contiguous range of complete canonical commits, including commits
with no relevant changes. Never use the existing identity feed's late current
hydration to manufacture an earlier row image. A rebuild barrier is an explicit
coverage outcome, not an empty delta.

The canonical commit path admits bounded capture work; it MUST NOT synchronously
expand an unbounded high-degree neighborhood. Capture overflow, sidecar I/O
failure, or lost source pins leave canonical commit semantics intact and make
the affected view require rebuild. They must never leave it falsely current.
The durable journal/coverage metadata must let reopen detect any crash gap
between canonical publication and derived capture, including a lost gap marker.
Publication waits for the covered source durability barrier; relaxed canonical
durability cannot cause a durable view to cite lost source commits.

### 5.2 Complete transaction delta protocol

The following is a logical protocol, not an existing Rust API or a canonical
WAL format change. One envelope identifies the database/branch incarnation,
source schema/capture revision, previous covered boundary, and committed epoch.
It contains typed graph-node, relationship and relational-row changes, each
with stable identity and old/new projected values, plus a completeness record.
Insert has no old row; delete has no new row; update carries both. Relationship
images retain endpoints and referenced properties; node deletion must preserve
all contributions needed to retract incident relationships.

Capture observes the actual transactional before/after states, not a later
lookup by identity. Coalescing repeated writes within one transaction is valid
only when the normalized transition preserves the committed result. No-op or
irrelevant commits may carry empty data but still advance contiguous coverage.
Rolled-back transactions produce no committed envelope. Schema changes that
invalidate capture produce an explicit rebuild boundary.

An incremental worker admits a complete commit before changing its candidate
state. It can page or spill private work inside that commit, but may neither
publish partial effects nor expose a resume cursor in its middle. Adjacent
commits may be batched only with equivalent ordered transition semantics and
retained complete boundaries. Duplicate delivery is handled by durable progress
and operation identity; equal row values are not evidence of duplicate delivery.
A missing interval, conflicting epoch payload, incompatible schema revision or
unverifiable source identity stops incremental advancement.

The capture protocol must not introduce a second authoritative transaction
commit. Persist covered data before certifying its coverage; on reopen compare
journal coverage with the canonical source boundary. A canonical commit may
succeed while derived capture fails, but no consumer can certify a range across
that gap. Missing or torn coverage metadata is not evidence of an empty commit.
Repair requires exact retained-source reconstruction or an explicit rebuild.

### 5.3 Incremental plan and operator state

Bind an ordinary deterministic query before deriving its maintenance plan.
The maintenance compiler accepts only qualified logical operators, records
schema/expression dependencies and versions, and produces both a full-build
plan and a delta plan with identical output schema and bag semantics. Existing
batch executors are semantic references, not automatically incremental
executors. Unsupported shapes fail at definition admission.

Prepare the maintenance plan once per bound definition revision, including
referenced-field dependencies, changed-key routing and typed input bindings.
Reuse that plan for admitted commit batches rather than reparsing generated
query text per mutation. Bind labels, properties and schema objects through
catalog identities; bind changed values as typed inputs. Schema/expression or
definition incompatibility invalidates the plan under the existing rebuild
contract. A reusable plan does not authorize stale source reads, bypass query
admission, or prove that maintenance is cheaper than the reference query.

```text
canonical commits -> complete transaction deltas -> incremental operators
                  -> private state/results/indexes -> sealed manifest
                  -> fenced head selection -> SQL/Cypher view reads
```

Each operator consumes typed signed row batches and produces typed signed row
batches. Filter/project retain only needed dependencies; joins maintain keyed
input arrangements; grouped aggregates retain their support state. Propagate
only affected keys through the plan. Consolidation preserves multiplicities,
NULL behavior and checked arithmetic. State and output updates remain private
until the complete source boundary is ready for publication.

Maintenance state lives behind existing internal ownership boundaries. The
embedded facade owns definitions, maintenance handles and typed diagnostics;
query planning owns qualification and delta derivation; derived storage owns
arrangements, result indexes and recovery. No new public crate, daemon, external
connector or process-per-view scheduler is required by this proposal. Broad
cross-view plan sharing is deferred until independent view ownership is sound.

### 5.4 Qualified operator semantics

Use signed bag deltas over engine-owned source arrangements. An update is an
old-row retraction plus a new-row insertion. Persist the arrangements needed to
apply the qualified operators, including reverse join keys for endpoint
property changes; keep them demand-paged with bounded memtables and scratch.
Do not build a second unbounded in-memory graph.

For a join changed on both sides in one commit, evaluate the transition exactly
once. One valid differential rule is:

```text
delta(L join R) = (deltaL join R_old) + (L_new join deltaR)
```

This includes the cross-term once. Comparing a full reference query at the
same before/after snapshots is the correctness oracle. Evaluate NULL, equality,
numeric overflow, and collation with the source dialect's existing semantics.

Contribution identity and delivery identity are different. Preserve independent
source matches even when their projected values or endpoints are equal. When
several maintenance branches discover the same source match for the same
transition, account for that match exactly once. Use the qualified operator's
algebra or bounded support identity/refcounts; do not globally deduplicate
output tuples or enumerate arbitrary paths to repair overlap. If a plan cannot
establish correct multiplicity within its budget, reject its shape or require
an explicit rebuild before advancing coverage.

For Entity usage, two distinct `MENTIONS` relationships from the same Memory to
the same Entity contribute two rows to `COUNT(m)`. Deleting one must change two
to one, not zero. Deleting the remaining edge removes the usage group; the
canonical Entity left join still supplies zero. An endpoint scope/property
update retracts each affected relationship contribution once even if both
endpoint and edge-change routes encounter it. Replaying an already covered
commit makes no additional change. These rules do not enable variable-length
paths or automatic query substitution in phase one.

| Operator | Initial contract | Important boundary |
| --- | --- | --- |
| Node scan, deterministic filter/project | Required for phase one | Referenced-field updates and predicate entry/exit |
| Relational base scans | Later unless a designated definition needs them | Preserve the same transaction/epoch contract |
| Directed one-hop graph expansion / inner equijoin | Required for entity counts | Indexed endpoint arrangements; duplicate edges preserved |
| Grouped `COUNT(*)`, `COUNT(expr)` | Required for the two phase-one workloads | NULL differences, checked arithmetic, group disappearance |
| Incrementally maintained exact integer `SUM` | Later unless a phase-one definition requires it | Preserve NULL/non-null support state and checked arithmetic |
| Zero-fill / left join to a canonical entity | Required serving capability | No-match entities must remain candidates with zero usage |
| `COUNT(DISTINCT key)` | Community milestone | Reference counts, including final-reference deletion |
| `AVG` | Later | Sum and non-null count, numeric semantics, NULL on empty input |
| Retractable `MIN/MAX` | Thread milestone | Ordered multiset state or explicitly bounded indexed recomputation |
| Outer/anti joins, arbitrary paths, recursive graph algorithms, global Top-K | Not initially incremental | Explicit rejection or separately selected complete refresh |
| Volatile functions, external calls, LLM/embedding generation | Excluded | Not deterministic incremental query results |

An ungrouped count over empty input is one zero row. A grouped count removes
the group when its input bag becomes empty unless a defined dimension/left
join supplies the zero row. Distinct state and arrangements are charged to
disk as well as memory budgets; supporting DISTINCT is not free metadata.

Incremental writes can be proportional to affected edges/groups. A high-degree
node update or community replacement is not constant-time. Exceeding a budget
leaves the last complete head intact and returns rebuild-required/resource
status, without acknowledging a partial epoch.

## 6. Bootstrap, persistence, and publication

### 6.1 Online bootstrap and rebuild

1. Install a build lease bound to the definition and source identity, then
   atomically capture/pin `S_E0` and reserve retained coverage after `E0`.
2. Execute the full definition against that snapshot in admitted pages. Build
   result, operator-state, and index artifacts without exposing a candidate.
3. Consume complete captured commits after `E0`. Rebuild if coverage expires,
   a barrier occurs, or required before-images/source identity are unavailable.
4. Seal a complete candidate for `E1`, validate all dependencies and source
   durability, then publish through the fenced protocol below.

Registration and snapshot/coverage capture must have no lost-write window.
A failed rebuild leaves the previous generation available only under its old
declared freshness contract. Starting a rebuild does not label it fresh.
Long builds and pins have deadlines, retained-byte/epoch limits and cancellation;
they cannot indefinitely hold canonical WAL or obsolete storage roots.

Build/refresh retention is a persistent maintenance-consumer lifetime; a query
snapshot is a separate RAII lifetime. A query MUST NOT create a durable Consumer
registration. Runtime handle closure retains the installed view and its durable
progress; explicit view removal retires that registration under the drop fence.
Reuse the qualified consumer lifecycle where its contract fits, or define an
explicit MV-owned registry extension. Do not silently charge ordinary reads to
the existing search registry's finite capacity or allocate registrations per
restart. Capacity refusal occurs before build callbacks and publication effects.

Persist snapshot scan progress together with the private build generation and
its retained delta boundary if resumable initialization is enabled. A progress
record binds source identity, snapshot epoch, definition revision and scan
position. Restore it only while the snapshot and post-snapshot coverage remain
valid; otherwise discard/reclaim the candidate through validated custody and
start an explicit rebuild. Never expose a partly scanned result as ready.
Initialization status distinguishes snapshot rows scanned, catch-up progress,
retained bytes and live lag; a scan finishing is not catch-up completion.

### 6.2 Derived storage and serving indexes

Reuse the projection-generation catalog. Extend its immutable manifest to
select a complete set of result pages, operator arrangements, serving indexes,
and progress for one definition/source epoch. For incremental work, use an
immutable base plus bounded delta runs and a bounded memtable; compaction
replaces complete manifests under the same publication discipline.

Unchanged pages/runs may be shared, with explicit GC roots. Incremental refresh
must not copy the entire result on every source commit. Record dependency
digests, codec/operator versions, counts, checksums and source identity in the
manifest. Never infer a complete result from an arbitrary directory scan.

Required serving access paths:

- Entity usage: prefix/key lookup by admitted endpoint scopes and Entity ID.
- Memory inventory: exact scope/predicate-version lookup over a small fixed row.
- Community candidates: generation/community/scope prefix and Memory key;
  apply the actual route's visibility and ranking in the runtime.
- Source/Thread rollups: exact scope and owner-object key.

Indexes must be generation-bound and verified with the selected manifest.
Reusing a canonical-table index for generation rows without its own completeness
proof is invalid. The existing bounded generation full scan is a correctness
fallback under an explicit query budget, not the P0 performance implementation.

### 6.3 Publish and recover

Publish result rows, arrangements, indexes and complete-through cursor as one
head selection. Candidate data is synchronized before manifest, manifest before
head, and required parent directories before acknowledgement. Expected-head
comparison prevents stale same-view writers from overwriting a newer head.

Reuse the prepared-publication authority and retain its fence across final
definition/instance/target/dependency validation and durable selector commit.
There is no separate `validate -> bool -> later publish` race. Moving from an
old epoch to a later one is valid only with complete admitted delta coverage;
dependency invalidation cannot be ignored because a candidate was already sealed.

Existing prepared-read validation can conservatively reject publication after
intervening source changes. Initially preserve that rejection, retry within the
maintenance budget, and retain canonical read fallback. Publishing an exact
historical result while current canonical state has advanced requires a
separately qualified MV publication permit: retained source identity plus exact
delta/manifest coverage, without claiming the result is current. It must not
weaken the existing prepared-publication validator for other consumers. Measure
fallback/refresh starvation under writers before accepting this design in Mem.

Crash outcomes MUST be either the old complete head or the new complete head,
never new counters with old arrangements/indexes or an advanced cursor alone.
A lost acknowledgement is retried with the same operation/generation identity;
it is not proof that publication rolled back. Ambiguous durability fails closed
and retains evidence. Torn unpublished artifacts can be reclaimed/resumed only
under their validated boundary; selected-artifact corruption is not an empty view.

Ordinary reopen validates the selected manifest, restores derived operator
state, and resumes from durable contiguous capture. It must not require a full
graph scan when that coverage is intact. Missing capture after crash, source
replacement, retention expiry or incompatible codecs require explicit rebuild.
Search documents and aggregate state remain outside the canonical WAL.

### 6.4 Publication under continuous writes

A future historical publication permit certifies a complete result at retained
source epoch `E`; it does not assert that the canonical head still equals `E`.
A later canonical commit alone must not invalidate this permit when the exact
source, complete delta coverage and definition remain retained and compatible.
This capability requires separate implementation and qualification; the current
prepared-publication validator remains unchanged until then.

The permit binds instance/branch incarnation, definition revision, covered
source boundary, candidate manifest digest and expected previous view head.
Final selection holds the publication fence across identity/drop/schema
validation and durable head selection. It rejects a replaced target, expired
source, changed definition, incomplete coverage or stale expected head. A
candidate cannot overwrite a view head that has already advanced beyond it.

Source advancement beyond `E` may be harmless to publishing that historical
result, while dependency-changing DDL or target replacement is not. Serving an
exact join still requires the canonical snapshot at `E`; historical publication
does not authorize mixing it with current canonical rows. Keep source commit,
derived publication, read visibility and durability boundaries distinct.

Preparation, source scanning, delta evaluation, artifact construction and
compaction occur outside the canonical owner write guard. Final publication
holds only the authority needed for bounded validation and selector commit.
Required synchronization may still dominate this interval: record actual lock
hold and writer latency rather than assuming a metadata-sized publish is fast.
Retire old artifacts after selection under pinned-reader and GC rules.

A worker chooses a fixed complete boundary per admitted batch rather than
restarting whenever writers advance the source. With sufficient maintenance
capacity and retained coverage, published source watermarks must advance while
writers continue committing; catching the instantaneous head is not required
for each publication. If input outruns maintenance, report backlog and bounded
lag, throttle maintenance admission or require rebuild on retention exhaustion.
Do not claim bounded freshness under arbitrary input load or block canonical
writes indefinitely to hide lag. Exact reads retain their wait/fallback/error
contract during overload.

## 7. Read consistency and Mem admission

Phase one supports standalone materialized-view reads and **explicit read-only
snapshot transactions**. Physical maintenance is asynchronous, with exact reads
and a bounded canonical fallback. Querying materialized views inside explicit
read-write transactions, transaction-private view deltas and reading pending
writes through a view are deferred. This limits the new materialized-view
surface; it does not remove HawDB's existing canonical read-write transactions.
All materialized-view capabilities here remain proposed and unimplemented.

### 7.1 Read-only transaction contract and extension boundary

For a read-only transaction `T`, select and retain one canonical snapshot `S_E`
under the existing supported isolation rules. Reads of its base data, named
views and their joins must agree with that same snapshot:

```text
ReadView(T, D) = Q_D(S_E)
```

Repeated reads remain at `E` even if canonical writers commit or background
maintenance publishes a newer head. A view at another epoch cannot be combined
with canonical rows at `E`. Use a retained matching manifest, wait within the
read deadline, evaluate the reference query against `S_E` within its budget,
or return a typed not-ready/resource error. Never silently return mixed epochs
or a partial result. These rules do not upgrade the existing isolation level
to serializability.

Reject unsupported materialized-view access in a read-write transaction with
a specific capability diagnostic, including when that transaction has not yet
written data. Do not silently ignore transaction mode, commit it, switch it to
read-only, or serve committed-only view state as if it included pending writes.
DML inside a read-only transaction follows existing rejection/abort rules.
Application DML cannot write derived view rows. Transactional view creation,
drop, refresh and savepoints are outside phase one.

Canonical graph/relational writes continue using existing transaction commit,
rollback, conflict and durability rules. Maintenance consumes complete committed
changes only; rolled-back or losing transactions contribute nothing. Async
materialization can lag after a successful canonical commit. A subsequent
separate read may carry its committed source floor through a typed read context
or commit receipt and use the exact wait/fallback/error path below. Such a floor
is a freshness request over committed data, not read-your-own-uncommitted-writes
or an atomic writable view transaction. View lag is not evidence that a lost
canonical commit response means rollback.

Keep definition versions, source epochs, contribution identities and publication
progress separate so a later separately qualified transaction-aware read path
can be added. A possible extension would evaluate `Q_D(S_E + W_T)` through a
bounded private delta or same-transaction reference execution, where `W_T` is
pending canonical state. Its isolation, rollback, conflicts, resource admission
and commit integration need their own design and tests. Phase one reserves no
public stub, transaction-private state storage or private-delta implementation
for that extension, and has no acceptance dependency on it.

### 7.2 Committed snapshots and read admission

Proposed read modes (logical contracts, not existing method signatures):

| Mode | Guarantee | Failure behavior |
| --- | --- | --- |
| `ReadOnlyTransaction(T)` | Evaluate the definition at the transaction's retained committed snapshot | Match the manifest/source epoch, execute the same-snapshot reference query, or fail within budget |
| `ExactSnapshot(E)` | View and every canonical join/filter use `S_E` | Wait within deadline for the retained epoch, execute the canonical reference query at `S_E`, or return typed not-ready/budget status |
| `AfterCommit(C)` | Choose a committed snapshot `E >= C`, then the same exact contract | Never return an older generation as the post-write refresh |
| `BoundedStale` | Explicitly allowed view-only result with source epoch and bounded lag | Reject if the limit/authorization contract cannot be established |

Epoch selection and source snapshot capture occur once per logical read.
Do not repeatedly chase a moving current epoch until a timeout. If a background
view advances past an already chosen snapshot, use a retained matching manifest
or the pinned canonical reference query; a newer watermark is not equality.

The read context pins both the chosen manifest and the exact source snapshot.
Do not combine a view at epoch `E` with canonical Entity/Memory filters at
`E+1`. Existing projection-bound transactions do not by themselves prove this
epoch equality. If the historical source snapshot is unavailable, an old
generation cannot participate in an exact canonical join. Multi-view reads
require a common source epoch and compatible dependency generations; choose a
canonical fallback if no such set exists.

Fallback executes the named reference query inside HawDB with its normal row,
payload, CPU/I/O and deadline limits. It is observable and cannot silently
recompute an unbounded graph. Definition drift/corruption does not bypass
validation via an undocumented fallback.

Scope and security requirements:

- The caller resolves and authorizes the request's Space set using canonical
  admission. A view never grants access or replaces permission evaluation.
- Preserve each query's legacy scope semantics. Some Mem predicates map
  NULL/empty ownership to `default`; others test stored IDs directly. Retain
  raw scope distinctions or bind a versioned normalization policy that exactly
  matches the reference query. Do not globally collapse those values.
- Apply both endpoint admission where required, lifecycle visibility and other
  request predicates before ranking/limits. Aggregation keys must retain enough
  scope information to do that; globally prelimited candidates are insufficient.
- Recheck write authority, quotas and retained-content deletion guards under
  the canonical mutation transaction. Bounded-stale mode is not allowed there.
- Cloud PostgreSQL counters and backend behavior remain separate; local HawDB
  adoption must compare the applicable clients' existing response semantics.

No Mem route is granted `BoundedStale` by this document. Enabling that mode is
a separately reviewed product choice, especially for deletion visibility.

## 8. Runtime ownership and resource limits

The embedded host drives typed maintenance steps using its existing scheduler;
HawDB owns state transitions and reports. No daemon, shell command, environment
flag, or process-per-view production control plane is introduced.

One observed worker per view/target stages work, with single-flight refresh,
bounded coalescing and explicit cancellation. The host retains and joins work
during shutdown; dropping a future does not cancel blocking I/O or release its
source/target lease. A wakeup can be coalesced; a canonical epoch cannot be
silently skipped. Global admission prevents many independent views from
exhausting a shared database's resources.

Limits cover captured rows/bytes per commit, retained delta bytes/age, source
pins, state/index disk bytes, scratch and memtable memory, fan-out/affected keys,
job duration, concurrent builds, artifact I/O, and reclaim work. Long-lived
readers and durable branches remain GC roots. Exhaustion returns the specific
blocker; neither GC nor retention can delete an active/pinned dependency.

Persistent asynchronous mode is an explicit native capability. Minimal
in-memory/WASM builds retain their existing feature boundary and must not gain
filesystem, thread, runtime, or timer dependencies just to compile the facade.
An in-memory view, if supported, obeys the same query semantics without claiming
durable reopen or background scheduling.

## 9. Status, errors, and explain evidence

Expose exact-view, bounded diagnostics through typed Rust reports:

- lifecycle: defined, building, catching up, ready at epoch, stale,
  rebuild-required, paused by caller, failed, or dropped;
- instance/target/definition identity, active generation, source epoch, source
  durable epoch, retained resume floor and rebuild reason;
- processed commits/rows, affected groups, arrangement/index/result bytes,
  fan-out, pin age, peak scratch, maintenance and fallback time;
- cancellation, unsupported shape, schema drift, source/coverage gap,
  expected-head conflict, resource exhaustion and uncertain publication as
  distinct typed causes.

These are proposed error categories, not invented public error-code literals.
Map them through the existing engine error conventions when implementing.
Status enumeration is bounded and owner-scoped; it must not expose another
owner's names or data.

`EXPLAIN`/profiles distinguish canonical execution, materialized scan/index
lookup, view generation/epoch, lag contract, rows/pages read and canonical
fallback reason. Source inspection or a successful full-scan query is not
evidence that an indexed serving path ran.

## 10. Implementation slices and exit gates

Each slice is reviewed against `main` independently. These are future work,
not tasks completed by the specification PR.

| Slice | Scope and dependency | Exit gate |
| --- | --- | --- |
| A: Phase-one prerequisites | Only definition binding, referenced-field capture, bounded coverage/gaps and publication needed by the two Mem workloads; depends on qualified MVCC pins and publication fences | Models/native negative controls cover the enabled subset; foundation completion alone is not a Mem delivery |
| B: Phase-one Mem delivery | Memory/Space inventory first, then Entity usage; the necessary node filter/project, one-hop joins, grouped counts, indexed reads and embedded Cypher composition | Both workloads pass business fixtures and development/nightly Mem query-path shadow comparison, deletion/reclassification, reopen, continuous writers and indexed-read cost gates |
| C: Community retrieval | Distinct reference counts, explicit community-generation dependency/rebuild, pinned candidate queries | Duplicate/delete/replacement and scope-before-Top-K scenarios pass; no mixed membership generation |
| D: Source and Thread rollups | Audit current write/delete/import paths, retractable extrema, relational messages and provenance resolution | Existing denormalization compares successfully before removal; imported time/completeness contract preserved |
| E: Search inputs and health | Existing search handoff, deterministic mapping versions, large-value handling; scheduled-time semantics only if separately adopted | No skipped search identities/deletes or weakened readiness; health values state their input epoch/evaluation time |

A supports B; together they form phase one. C through E are later scope, as
are physical graph inventories. Do not claim first delivery after
only catalog syntax or background refresh of an unrelated SQL table. Broad
optimizer substitution, arbitrary view DAGs, synchronous transaction maintenance,
recursive paths, rolling-window engines and distributed execution are deferred.

### First implementation PR

Start with the bounded complete-transaction delta seam needed by Memory/Space
inventory and its native reference oracle, not a general CDC API or full view
DDL. Capture old/new values only for the referenced node identity, scope and
classification fields. Mixed graph/relational transactions retain one complete
commit boundary even when their unrelated effects have no captured row images.
Cover rollback, repeated writes, empty relevant deltas, overflow and missing
coverage. Compare the captured transition on the referenced before-state
projection with that projection of the actual after-state at the same epoch.
Cover recovery and missing/torn sidecar records without weakening canonical
durability. Keep this seam internal; if capture and durable replay need separate
owners, split them into sequential PRs against main.

Follow promptly with the inventory vertical slice: the required grouped-count
state, bounded bootstrap, serving key lookup, exact publication/read behavior
and designated Mem shadow comparison. Then extend capture/arrangements for the
Entity usage workload, including relationship and endpoint-property changes.
Do not expand generic capture, SQL frontends or operator coverage before these
slices demonstrate their Mem value. All graph/relational data actually consumed
by an enabled definition must still obey the exact transaction protocol.

Each implementation PR names existing runtime/formal test owners and updates
the test index. This documentation change neither registers nor executes those
future tests, and no individual foundation PR completes phase one.

### Formal and native correctness qualification

Before implementing publication/lifecycle transitions, extend or compose the
existing projection-generation and MVCC/publication models with view coverage
and manifest state. Reuse existing invariants rather than starting an unrelated
durability model. Model-check at least:

1. `SelectedResultEqualsSource`: active contents equal the definition at their
   declared source epoch; definition revision and source identity agree.
2. `NoPartialCommitOrCursor`: graph/relational effects, state, indexes, rows and
   progress become selected as one complete unit.
3. `DurableSourceBeforeHead`: selected derived state cannot cite source writes
   lost after power failure, including relaxed durability and reordered I/O.
4. `NoCoverageGapAdvance`: expired/missing capture or a rebuild barrier cannot
   advance incremental freshness; online bootstrap has no capture window.
5. `PinnedReadAndSafeGC`: no mixed epoch/generation and no reclamation of pinned
   artifacts or source roots, including unleased durable branches.
6. `DropAndTargetFence`: stale workers cannot publish after drop, definition
   replacement, branch/instance replacement, or an intervening head.

Native tests then refine those invariants through actual facade calls: mixed
transactions; both sides of a join changed together; rollback/no-op updates;
node and final-reference deletion; NULL and empty groups; arbitrary grouping-key
changes; crash between each sync/head step; lost acknowledgement; capture
overflow/gap; old readers; DDL races; cancellation; shutdown; and resource caps.
Inject torn/reordered persistence as well as process kill/reopen. Include
negative controls that would pass if a cursor advanced alone or a stale source
were hydrated. Small deterministic graph/relational differential sequences
compare bag results against full queries after every complete source epoch.
Any exploratory fuzzing remains local under repository policy.

### Phase-one transaction regression matrix

These are future native facade and Mem query-path acceptance cases, not executed
results of this documentation change. They qualify only the read-only view
surface and committed-source maintenance boundary.

| Fixture | Required result / negative control |
| --- | --- |
| Read-only transaction reads base and view repeatedly while another writer commits | One retained snapshot; a newer background view cannot change the pinned result or produce a mixed-epoch join |
| Two views plus canonical filtering in one read-only transaction | Both views and filtering agree at the same source epoch, or take the bounded reference/error path |
| Query a view inside an explicit read-write transaction, before or after writes | Return the unsupported-capability diagnostic; preserve existing transaction error semantics and never silently change transaction mode |
| Attempt DML inside a read-only transaction or against derived rows | Reject through the applicable transaction/read-only relation rules |
| Commit a canonical Space move and archive together | A later view snapshot includes the complete transaction or none of it, with no intermediate Space/classification combination |
| Canonical transaction rolls back or loses a supported write conflict | No contribution from that transaction enters public view state or maintenance progress |
| Separate read requests the committed floor while physical materialization lags | Wait, use the same-snapshot reference query or report a bounded failure; never return a snapshot below the floor |
| Snapshot/coverage is unavailable or a read budget is exhausted | Return the specific not-ready/resource error without silently advancing the transaction snapshot |
| Commit response is lost, then reopen/retry | Resolve the canonical operation and resume complete capture without double-counting or inferring rollback from view lag |

Negative controls must catch mixed epochs, visible partial source transactions,
rollback contributions and a silently accepted unsupported read-write view
query. Pending-write overlays and transaction-local aggregate/arrangement state
are later work and are not required for phase-one completion.

### Phase-one contribution regression matrix

These are future focused regressions and differential fixtures, not test
results of this specification. Compare full result bags and classification axes
at the same pinned epoch after every complete transaction, not just total row
counts. Keep stable relationship identities in minimized replay fixtures.

| Fixture | Required result / negative control |
| --- | --- |
| Two distinct mentions with equal endpoints | Entity usage is two; output-value or endpoint deduplication must fail the oracle |
| Delete one, then the final mention | Counts become one, then zero through serving composition; no duplicate retraction or lost zero-count Entity |
| Overlapping endpoint and edge routes | A scope change plus a related edge update in one commit yields exactly the reference bag; maintenance-branch overlap cannot retract the same contribution twice |
| Both endpoints change scope | Retract each old contribution and add each new contribution exactly once; never expose mixed endpoint scopes |
| Repeated lifecycle updates / rollback | Inventory axes equal the final committed classification; rollback has no effect and equal-valued distinct source rows remain distinct contributions |
| Duplicate delivery / lost acknowledgement | Reapplying the same covered commit or resolving the same selected publication leaves state and counts unchanged |
| Delete and recreate an equivalent relationship | Stable relationship identity and ordered commit transitions preserve the reference multiplicity; equal endpoints alone do not establish replay identity |
| Unsupported recursive/repeated-binding shape | Definition admission rejects it before installation; do not extend phase-one syntax to exercise an unqualified maintenance algorithm |

A mutation test/negative control must demonstrate that replacing bag semantics
with a set, applying an overlapping retraction twice, or advancing progress
without aggregate state causes the reference oracle to fail. Support identity
and counters are part of the same durable selected state as results/progress;
restart and lost-ack tests must not reset only the deduplication state.

### Performance and business acceptance

Measure against the same canonical query and dataset, in release builds, with
explicit durability, residency, cold/warm cache, concurrency and budget settings.
Use small datasets as well as large/skewed fixtures: high-degree Entities, many
Spaces, duplicate mentions, large communities, burst imports and delete storms.

Required evidence:

- Matching result bags and scope/visibility semantics before timing changes.
- Inventory key lookup work independent of the total Memory population;
  Entity usage lookup avoids recomputing each candidate's mention expansion.
- No full-result copy per incremental refresh and no whole-graph rebuild on
  ordinary reopen when durable coverage is intact.
- Query p50/p95/p99, writer latency, maintenance lag, affected keys, CPU,
  page reads, peak memory, disk amplification and rebuild duration.
- Continuous-writer qualification uses a coordinated fixture that keeps writes
  active throughout snapshot build, catch-up and at least multiple successive
  publications. Independently observe committed-write and published-source
  progress; prove unrelated writes complete during preparation. Report source
  arrival/service rates, retained coverage and lag, and distinguish bounded
  overload from a stalled worker. Measure publication guard wait/hold time and
  writer p95/p99 against the same workload before/after the publication change.
- Direct-query fallback frequency under the proposed exact-read contract.
  A fast stale view with frequent expensive fallbacks is not a successful P0.
- Worst-case fan-out and retention exhaustion produce bounded, observable
  behavior and preserve canonical writes and the last complete head.

Set numerical latency/overhead acceptance thresholds from a recorded baseline
before approving an implementation milestone. This source review supplies no
measured speedup. Mem adoption additionally needs shadow comparison and the
applicable existing integration/activation gates; merging engine support is
not proof of a faster or activated Mem data plane.

## 11. Alternatives and recommendation

**Complete refresh only** is the smallest safe prototype and a useful explicit
mode for infrequent community replacements. It still requires bounded snapshot
builds and atomic publication, and is not sufficient for hot entity/count reads
under frequent writes.

**Application-maintained counters** are adequate for a narrow field with a
single complete mutation boundary. Mem's Source and Thread projections show
why old/new classification, deletion, imports and provenance make extending
that pattern costly. Retain them until view equivalence is demonstrated.

**Recommended:** engine-owned asynchronous incremental relations over qualified
graph/SQL operators, durable derived arrangements and indexed serving, with
exact snapshot reads and explicit bounded canonical fallback. Start with Entity
usage and Memory inventories. That delivers reusable engine semantics for
existing Mem businesses without embedding Mem routes in the database kernel.


## 12. References

The following are background references only. They do not define HawDB APIs,
provide HawDB test evidence, require upstream dependencies, or imply adoption
of a distributed runtime. The contracts and qualification gates above are
specific to HawDB's embedded execution and durability model.

- [RisingWave: stream row operation representation](https://github.com/risingwavelabs/risingwave/blob/73219848e4fb2193d0da283068824613973008db/src/common/src/array/stream_chunk.rs): insertion, deletion and paired update images.
- [RisingWave: materialization executor](https://github.com/risingwavelabs/risingwave/blob/73219848e4fb2193d0da283068824613973008db/src/stream/src/executor/mview/materialize.rs): persisted materialization state and barrier handling.
- [RisingWave: materialized-view creation](https://github.com/risingwavelabs/risingwave/blob/73219848e4fb2193d0da283068824613973008db/src/frontend/src/handler/create_mv.rs): query planning and maintained-job creation.
- [RisingWave: historical initialization and continuous maintenance](https://risingwave.com/blog/backfilling-in-risingwave-from-historical-initialization-to-continuous-streaming/): snapshot initialization, retained changes, resumable progress and catch-up.

- [MV4PG: Materialized Views for Property Graphs, arXiv:2411.18847v1](https://arxiv.org/abs/2411.18847v1): workload-selected graph views and pre-generated maintenance templates. Property-update maintenance is outside its evaluated views; overlapping-match deduplication remains future work. Its reported speedups are not HawDB measurements or evidence for HawDB recovery/concurrency guarantees.

- [RisingWave: transaction statement handling](https://github.com/risingwavelabs/risingwave/blob/73219848e4fb2193d0da283068824613973008db/src/frontend/src/handler/transaction.rs) and [transaction context](https://github.com/risingwavelabs/risingwave/blob/73219848e4fb2193d0da283068824613973008db/src/frontend/src/session/transaction.rs): explicit read-only snapshots are supported; explicit read-write transactions are not started, and implicit read-write mode is single-statement. This is a comparison for the proposed read-only view scope, not a restriction on HawDB's existing canonical transactions.

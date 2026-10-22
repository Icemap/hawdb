# Schema-bound vector indexes

Implementation tracked by https://github.com/nowledge-co/hawdb/issues/1157.
This document records the phase-one contract; delivery remains incomplete until
the query, persistence and resource acceptance tests listed below pass.

## Scope and ownership

SQL vector columns and Cypher node properties share one `VectorIndexDescriptor`.
The descriptor binds an index name, a resolved target, its declared
`VectorDimension`, metric, algorithm settings and canonical schema digest.
Relational targets use the normalized table name and column ordinal because the
relational schema has no independent persisted column identifier. Node targets
retain their `TableId` and `PropertyId`. Names remain diagnostic/resolution data;
the full resolved binding must match before a source is admitted.

The initial backend is RaBitQ with cosine distance. L2 remains an exact query
expression; an L2 RaBitQ definition is refused. Only one field is indexed. NULL
fields are omitted. Non-NULL cosine vectors must have a nonzero norm; a build
must fail rather than silently exclude an invalid non-NULL value.

The embedded facade resolves schema and captures data under the existing pinned
source snapshot. Index maintenance reuses registered Consumer publication and
reopen. It does not introduce a second directory selector, recovery ledger,
standalone ANN registry, or host graph scan. Per-operation resource policy is
not part of the persistent algorithm definition.

## Query semantics

An index query is explicit: bounded approximate candidates followed by exact
cosine-distance reranking. Both language frontends must lower to that same
contract. Ordinary filtered distance queries remain exact and cannot be
silently rewritten to approximate index access.

Canonical vectors and raw projection vectors use the same core distance
implementation. The result is `1 - cosine_similarity` in [0, 2], including
negative similarities. Legacy search relevance remains positive-only and
unchanged. Neither clipped relevance nor quantized estimates may substitute for
the reported exact distance. The kernel breaks equal-distance ties by the
lexicographic encoded identity, which is not necessarily SQL primary-key order.
Use an explicit `ORDER BY distance, id` (or the corresponding graph property)
when the query requires that ordering.

The candidate budget and final k are separate. Results are exact only within
the generated candidate set. An exhausted candidate window does not establish
global exact recall. Residual query predicates apply after that explicit window;
graph-aware eligibility before candidate generation is a separate delivery
related to https://github.com/nowledge-co/hawdb/issues/1119.

## Language entrypoints

The bound `VectorIndexReadTransaction` supplies the index and canonical snapshot
for both languages. `query_cypher_streaming` accepts an ordinary `QueryRequest`;
`query_sql_streaming` accepts a `VectorIndexSqlRequest`. Both require a fresh
same-governor permit and an explicit `VectorIndexLanguageBudget`: the index
sub-budget is deducted before the canonical query receives its allowance.
Governor identity is checked at each language entrypoint before execution or
allocation, including LIMIT 0 and plans that never reach the index operator.
The source name in the query must match the bound descriptor.

```cypher
CALL vector_index_search('documents', $embedding, candidates := 100, topK := 20)
YIELD node AS document, distance
WITH document, distance WHERE document.active = true
RETURN document.id AS id, distance
ORDER BY distance, id
```

```sql
SELECT document.id, distance
FROM vector_index_search('documents', $1, candidates => 100, top_k => 20) AS document
WHERE document.active = true
ORDER BY distance, document.id
LIMIT 10;
```

The embedding must be a canonical typed vector parameter. Both explicit limits
are required, with `0 < top_k <= candidates`. Filtering cannot refill the
already selected top-k window. Cypher binds real canonical nodes and can use
ordinary graph traversal afterward. Its node and distance output names must
remain distinct, including the default name of an omitted YIELD output. SQL decodes complete canonical primary keys,
including composite keys, and uses snapshot row point reads. It does not create
a temporary copy of the canonical table or run a host-side join/filter.

The first SQL operator supports one source with ordinary WHERE, projection,
ORDER BY, LIMIT and OFFSET. Joins, grouping, DISTINCT and locking clauses are
refused. The table function reserves the `distance` column, so a source table
already declaring that name is refused. Ordinary SQL/Cypher entrypoints do not
implicitly attach an index. SQL streaming currently accepts SELECT only; the
Cypher physical plan exposes its approximate window and post-window filtering
in EXPLAIN. Broader SQL plan integration remains subject to explicit qualification.

Language reports retain the kernel's actual backend, window, candidate/rerank
counts, source epoch and native generation. Cypher reports may omit the index
report if execution never reaches the operator. SQL currently executes candidate
selection even for LIMIT 0, whereas Cypher can skip the operator. Reports do not
yet count rows removed by canonical visibility or residual predicates; output
cardinality alone cannot distinguish a small index from post-window filtering.
These observations do not establish ANN recall, process RSS bounds or production
Mem acceptance. Native producer work remains bounded by the existing generation
writer and its component limits. Full graph capture currently point-queries each
node candidate, including nodes outside the target label. A sparse target label
can therefore exhaust the explicit feed-work budget; the build fails instead
of publishing a partial source. Label-aware source capture and plan/report cost
improvements remain subsequent work.

## Identity and lifecycle refinement

A definition is not serving authority. The existing Consumer supplies source
database/branch identity, incarnation, operation, source epoch, immutable
generation, artifact closure and the publication receipt. The definition must
be sealed inside that closure; an independent sidecar cannot authorize it.

The lifecycle refines the existing Consumer protocol:

1. Resolve and validate the definition under the captured schema.
2. Build a complete private projection from the same pinned source epoch.
3. Seal definition and artifacts before source-fence validation and selection.
4. Publish using the existing durable selector; propagate uncertain outcomes.
5. Reopen only the registered closure and verify its definition and receipt.
6. Retained pins continue reading their immutable generation. A new canonical
   snapshot must not be joined to an older index as if the index were current.

Source epochs and publication generations are independent: a full rebuild may
select a new generation at the same source epoch. Acknowledgement first verifies
the complete native selected receipt, then refreshes the cached receipt while
rejecting source-epoch regression; cached checkpoint bytes are not a second
publication authority.

Source writes make a current-source binding stale until explicit qualified
maintenance publishes its successor. Rebuilds and deletes must not reuse old
ordinal mappings. Existing Consumer generation custody, source fencing,
uncertain-publication resolution and pin reclamation remain authoritative.
Any additional state transition requires a refinement/model update before it
is enabled; merely adding descriptor bytes does not replace those proofs.

The vector facade refines these existing transitions without adding a selector:

| Operation | Existing authority | Vector-specific precondition/result |
| --- | --- | --- |
| Bind read | Completed Consumer pin | Database UUID, branch UUID, source epoch and resolved schema all match the retained canonical snapshot |
| Begin rebuild | `reserve_search_consumer_update` and successor birth | Rebuild the same target, dimension and algorithm from a complete new source snapshot |
| Prepare | `prepare_registered` | A fresh complete source mapping replaces all documents, including deletions and NULL transitions |
| Select | Existing source fence and stationary selector | Reject a source change after capture; acknowledge only the native completed receipt |
| Uncertain selection | Existing pending-publication custody | Retain the candidate and resolve through the ordinary Consumer recovery entrypoint |
| Old read | Existing immutable pin | Keep both the old canonical snapshot and its matching generation; hold no execution slot while idle |

The generic Mem document catch-up mapper is not the maintenance mapping for a
schema-bound vector index. The facade refuses that combination through both legacy and covered catch-up entrypoints. A full vector
rebuild may refresh the schema digest after an unrelated schema change, but a
changed target identity, dimension or algorithm requires a new definition.
Dropping and recreating a graph property changes its identity; reordering SQL
columns changes their bound ordinal. Rebuilding the old definition then refuses.
Remove the old Consumer through its normal lifecycle and create a new vector
index definition before admitting reads against the changed target.

Recovery preserves the existing project-authority boundary: a canonical read
transaction retained from a closed database can defer reopening an uncertain
publication with `BranchBusy`. That refusal retains pending evidence and the old
read remains usable. Releasing that transaction allows the ordinary recovery
entrypoint to resolve the selected generation; no vector-specific bypass is used.

## Resource ownership

A registered vector producer retains its source envelope through publication.
Its compatibility snapshot admits compression and encoding scratch before use;
its private artifacts use the existing generation writer under the original
Consumer publication lease, including the original governor admission. This
private producer cannot advance a previously selected root. It is installed and
selected only by the existing Consumer protocol. The producer holds an actual
I/O wave from that governor throughout synchronous private snapshot/artifact
work and releases it before Consumer target preparation. A saturated slot waits
cancellably before writing; staging cleanup shares the producer wave.

Canonical SQL point reads admit key/field/overflow decoding before copying and
retain the allocation owner through projection. The storage allocation controller
composes the query ledger with the original governor controller. Candidate and
canonical execution have disjoint sub-budgets; original vectors are reranked by
the shared distance implementation. Every projection, expression input and sort-key occurrence is admitted before
copying, including repeated wide columns and the empty-input binding probe.
LIKE/ILIKE scratch is admitted separately using the shared matcher's layout:
token slots, one folded-literal buffer, and original Unicode characters. Layout
and execution use the same escape/folding rules and preallocate those buffers;
matching has no growing temporary literal string. Repeated `%` tokens compact,
while alternating `%_` patterns retain their full admitted token cost. Invalid
patterns retain evaluator diagnostics, including short-circuit and NULL behavior.
A narrow grant fails before callback delivery
and does not change the selected index or poison the canonical read view.

## Acceptance evidence

Required end-to-end cases cover both SQL and node-property sources: create,
build, explicit query, close, reopen, update/delete and successor publication.
SQL and Cypher must agree on distance and deterministic ties, including opposite
vectors. Test NULL, zero vectors, invalid dimensions, unsupported metrics,
schema/source mismatches, retained old pins, resource boundaries and
cancellation. Feature-disabled builds refuse unavailable index execution.

Build, candidate selection, raw hydration, result retention and decoding must
be admitted before allocation. Budget exhaustion is an error, not successful
truncation. Query evidence identifies backend, candidate budget/count, exact
rerank count, source epoch and generation; it does not claim filtered ANN or
production Mem acceptance.

The root vector-index unit family is owned by `//:hawdb_unit_test_binary` in
the existing root presubmit lane; Search binding and producer-I/O cases are
owned by `//crates/search:hawdb_search_tests` and applicable existing feature
variants in the crates lane. The canonical point allocation guard belongs to
`//crates/relational:hawdb_relational_tests`. Fixtures use isolated UUID paths,
at most five vectors for semantic cases, and a 256-KiB synthetic text value for
allocation boundaries, including alternating LIKE tokens and Unicode/escape controls. The I/O test uses a channel to observe the acquisition
boundary and cancellation to release a saturated wait; its receive timeout is
a failure ceiling, not a timing assertion. No new CI job, serialization gate or
heavy benchmark is added.

These guards are separate from pre-existing ordinary relational projection and
standalone writer admission gaps, tracked by
https://github.com/nowledge-co/hawdb/issues/1160 and
https://github.com/nowledge-co/hawdb/issues/1161. This vector-index qualification
does not claim those independent paths are repaired.

Mem App schema/import migration, runtime activation, the graph-eligibility
planner, and FreshDiskANN evaluation remain subsequent roadmap phases.

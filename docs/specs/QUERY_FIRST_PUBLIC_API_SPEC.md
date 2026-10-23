# Query-First Public API Specification

## Status

This specification defines the release-facing embedded API boundary. It is
normative for production builds.

## Public query boundary

Application graph behavior MUST be expressed as parameterized Cypher and
executed through `Database`, `DatabaseReadTransaction`,
`DatabaseTransaction`, `NowledgeGraphAdapter`, or the bounded
`NowledgeMemGraph` query surface. Relational behavior MUST be expressed as
PostgreSQL-dialect SQL through the corresponding SQL entry points.

Idempotent ingestion uses PostgreSQL-style `INSERT ... ON CONFLICT (...) DO
NOTHING RETURNING ...`. `query_sql_with_result` exposes the statement's
provisional affected/conflict counts and returned rows; `commit_with_result`
exposes the outcomes recomputed at the durable commit boundary. These typed
result envelopes coordinate transaction state and do not replace SQL as the
mutation interface.

Strict Append tables use the same SQL boundary. The storage mode is declared
with PostgreSQL `CREATE TABLE ... WITH (...)` storage-parameter syntax; the
parameter names and values are HawDB extensions:

```sql
CREATE TABLE events (
    stream_id TEXT NOT NULL,
    sequence BIGINT NOT NULL,
    payload BYTEA NOT NULL
) WITH (
    storage_mode = 'strict_append',
    partition_key = 'stream_id',
    order_key = 'sequence'
);
```

The initial SQL contract accepts one partition-key column and one order-key
column. `INSERT` is parameterized and append-only. `UPDATE`, `DELETE`,
`ON CONFLICT`, `CREATE INDEX`, and `ALTER TABLE` fail closed for Strict Append
tables. Reads require an exact partition predicate, ascending order by the
order key, an optional exclusive lower bound on that key, and an explicit
`LIMIT`. `EXPLAIN` and `EXPLAIN ANALYZE` expose the
`StrictAppendPartitionScan` path. `system.append_tables` and
`system.append_storage` expose schema and storage-residency state without
making typed row CRUD a production integration surface.

Strict Append tables may opt into a table-wide generated order key:

```sql
CREATE TABLE generated_events (
    stream_id TEXT NOT NULL,
    sequence BIGINT NOT NULL,
    payload BYTEA NOT NULL
) WITH (
    storage_mode = 'strict_append',
    partition_key = 'stream_id',
    order_key = 'sequence',
    generated_order = 'commit_sequence'
);
```

`commit_sequence` requires one `BIGINT NOT NULL` order-key column without a
default. `INSERT` MUST omit that column; caller overrides and Strict Append
`INSERT ... RETURNING` fail closed. Values start at one and are assigned as one
contiguous, table-wide interval in caller row order at the serialized durable
commit boundary. Preparing, aborting, or rejecting a transaction consumes no
values. A transaction with pending generated rows cannot read that table,
because an order key does not exist before commit.

Generated assignments are returned only by the typed durable result:
`AppendCommitResult::mutations` for direct append commits and
`TransactionCommitResult::append_mutations` for mixed transactions. Each
`AppendMutationOutcome` preserves append-write order and caller row order.
Exhaustion fails before WAL publication as
`HawDBError::AppendSequenceExhausted`, retaining the table, prior watermark,
and requested row count as typed fields.
`system.append_tables` exposes `order_mode` and
`generated_order_watermark`; the watermark is null for caller-provided tables.
The assigned full rows and watermark are part of WAL replay and checkpoint
state, so recovery either retains an exact committed prefix or reuses values
from an uncommitted suffix.

Every application-owned read statement MUST have explicit row and payload
budgets. Multiple distinct read phases SHOULD remain separate named
statements. The host MAY normalize requests, account for budgets across
statements, and shape compatibility responses, but MUST NOT reimplement graph
scan, join, filter, sort, aggregate, or traversal semantics.

Application mutations MUST use parameterized Cypher inside
`DatabaseTransaction` when more than one statement must publish atomically.
The transaction owns read-your-own-writes behavior and the single canonical
WAL publication boundary.

## Shared request constraints

`QueryRequest` carries Cypher parameters, authorization, scoring, cancellation,
and read-output limits. `SqlQueryRequest` carries SQL parameters, cancellation,
output limits, and the relational join-planning directive. String overloads
remain convenience entry points. Synchronous database, read-snapshot, and
explicit transaction entry points MUST preserve the same request attachments;
concurrent transactions MUST retain their existing locking and read-dependency
validation. Embedded explicit transactions use the same statement kernel.

Request limits can tighten database limits but MUST NOT widen them. An explicit
request `Some(0)` is a zero-row or zero-payload ceiling, not an unlimited sentinel.
SQL mutation results MUST be validated before private state is installed or a
commit is published. Cypher mutations continue to use `MutationLimits`.

A request context MUST NOT replace an enclosing snapshot or transaction
context. Both cancellation tokens and the earlier deadline apply. CPU, executor,
working-memory and result-memory ceilings are intersected. Work budgets remain
shared, with common ancestors charged once. Cancelling a statement's combined
context does not cancel either enclosing scope. An existing dynamic memory or
I/O controller remains attached; two distinct controllers for the same resource
are rejected because they do not describe a single admission.

Prepared query reuse also checks authorization identity, including policy epoch,
and scoring-plan shape. Changed scoring coefficients, query parameters and
reference clocks MUST bind to the current execution. A mismatch uses the normal
planner rather than a plan prepared with different permissions.

`OwnedQueryRequest` and `OwnedSqlQueryRequest` own this same contract across
asynchronous admission. Their borrowed views use the synchronous statement
kernel. Tokio collection and streaming MUST retain authorization, scoring,
cancellation and output limits. `OwnedHostScoringRequest` owns a `Send` host
scorer; preparation reads its validated descriptor without invoking the scorer.
Execution borrows the scorer exclusively, and cancellation or future drop
releases it with the request. A busy or poisoned scorer fails closed. Admitted
autocommit requests MUST reject an open Embedded explicit transaction.

`DatabaseReadTransaction::into_bounded` consumes a pinned transaction and creates
one `BudgetedReadSnapshot`. All Cypher and SQL statements share its
`ReadSnapshotBudget`; per-statement limits intersect the remaining budget.
Successful owned results debit rows and logical payload bytes atomically. A
failed statement delivers no owned result and consumes no output quota. Payload
accounting charges column names per row and value payloads regardless of compact
physical schema storage. EXPLAIN charges its delivered report, not the inner
execution's row count. `ReadSnapshotUsage` counts completed statements and
delivered output. Mem snapshots use this same ledger, including their projection
visit charges, while retaining their existing positive-budget entry contract.

These request contracts do not by themselves establish execution outside outer
locks or end-to-end borrowed result delivery. Those remain separate acceptance
items in issue #1193.

## Borrowed result delivery

Borrowed consumers MUST receive views whose lifetime is bounded by the callback.
Immediate delivery may borrow producer-owned storage only while its source
reservation remains live. Bounded validated delivery MUST retain complete output
until row and payload ceilings have passed; a terminal execution or budget error
must not expose a successful prefix. Compact retained storage shares its schema
and admits copied payloads before allocation. Callback error, cancellation and
unwind MUST retire storage before releasing its result reservation.

The common accumulator accepts native columnar/projected views and exposes
validated compact views through `RowRef::Compact`. Eligible root numeric
projections, including a surrounding SKIP/LIMIT, use `RowRef::Projected` over
the existing lending, owned-input or ordered parallel-morsel scan protocol.
Projection names are admitted once, retain duplicate-alias suffixes, and follow
the same sorted schema as named maps. Payloads remain source/plan-owned during
immediate callbacks; retained output copies only admitted values into compact
segments. `native_projection_rows` records this path in execution reports.

The existing work-limit and out-of-core node-projection eligibility boundaries
remain in force. Other scalar/operator shapes still construct bindings and use
the shared accumulator. This path-specific integration does not establish
allocation or latency improvement for every query; release measurements remain
required.

## Preparation and admission ownership

Admission and execution SHOULD reuse one internally prepared statement when its
execution environment remains compatible. The prepared object MUST NOT retain
a database snapshot or generation pin across an admission wait. Reuse MUST
revalidate branch identity, schema/execution compatibility, database configuration
and system query settings; execution still applies request authorization and
cancellation. Ordinary data commits alone do not invalidate read preparation.

Mem report admission also includes the published host projection's I/O depth
when the prepared plan executes text or vector seeds. This capture retains only
capability metadata, not a source generation. A changed capability is revalidated
after admission and before execution, including the resident fallback. Plain
EXPLAIN and graph-only reports do not acquire projection I/O allowances.

Preparation memory MUST remain charged until the prepared AST and plan are
consumed or dropped. Transferring that reservation MUST release preparation's
CPU, blocking-task and I/O slots without a memory release/reacquire gap. Execution
requires a separate admission. Hard capacity checks MUST include any separately
charged memory that the caller cannot release while waiting, but MUST NOT charge
those bytes again. An impossible combined reservation fails non-retryably rather
than waiting for its own retained state. Rejection, cancellation, future drop,
error and panic MUST release the associated ownership. An adapter that queues
execution MUST release rebuildable preparation on a retryable admission refusal,
then reprepare under the execution reservation. This prevents multiple prepared
queries from holding all available memory while waiting for one another.

Admitted SQL retains a parameter-neutral relational template, database instance
identity and configuration, without a source snapshot. Relational access paths
bind against the execution snapshot, so schema and parameter changes cannot
reuse stale physical bindings. A database-instance or configuration change
during admission fails the request. SQL currently admits the configured query
working-memory ceiling conservatively; this is not a physical-plan estimate.
After a retryable refusal, Tokio reparses through the originally captured
template cache under the execution reservation and still checks that original
identity before execution. SQL mutation RETURNING limits apply before commit.

This transition is modeled by
[`HawDBPreparedAdmission`](../tla/PREPARED_ADMISSION_PROOF.md). It does not introduce
a public prepared-statement API or a persistent snapshot cache.

## Removed business facades

Release builds MUST NOT expose application-specific entity, relationship,
Memory, Source, Skill, Thread, Label, Community, or AugmentationJob CRUD batch
methods. Release builds also MUST NOT expose `read_graph_*` route methods or
their route response DTOs.

The old adapters and route response DTOs MUST NOT remain behind `cfg(test)`.
Regression coverage MUST execute parameterized queries through the ordinary
runtime. Route-evidence builders MAY retain private query fixtures, but neither
their statements nor route-specific response shapes are public database APIs.

## Stable typed boundaries

Branch lifecycle and selection use PostgreSQL query entry points:
`CREATE BRANCH`, `DROP BRANCH`, `USE BRANCH`, `SHOW BRANCHES`, `SHOW BRANCH`, and
`SHOW CURRENT BRANCH`. `DatabaseSession` borrows the mutable database's selection;
selection survives the end of that borrow. Catalog mutations and selection are
rejected inside explicit user transactions. Shared `ConcurrentDatabase` clones
cannot use `USE BRANCH` to retarget their shared runtime. See
[the branching contract](BRANCHING_STORAGE_SPEC.md) for value binding, exact
revision/identity requests, and the remaining project-admission boundaries.

Storage-owned typed helpers implement the same lease/publication/recovery kernel;
they do not introduce a public `open_branch()` alternative control plane.

A typed API remains appropriate only when a stable kernel contract coordinates
behavior that cannot be represented safely by one query language statement.
The retained categories are:

- database open, configuration, sessions, and transactions;
- bounded streaming and query reports;
- WAL, checkpoint, recovery, doctor, and storage inspection;
- schema migration and maintenance orchestration;
- search projection, changefeed, freshness, and rebuild boundaries;
- bounded unified knowledge retrieval over canonical graph identities;
- HawDB Lightning import and canonical snapshot boundaries;
- QoS admission, readiness, qualification, and telemetry evidence.

These APIs MUST remain route-neutral. A new REST route, scheduler operation, or
business entity is not sufficient justification for a new typed database
method.

## Compatibility and formal-model impact

HawDB has no released public compatibility obligation for the removed business
facades. This is an intentional source-breaking cleanup before the first
release.

The cleanup does not change transaction, WAL, checkpoint, MVCC, lock, recovery,
or query execution semantics. Existing TLA+ safety and liveness obligations
therefore remain unchanged. Any future typed boundary that introduces a new
publication, recovery, concurrency, or admission state transition MUST update
the corresponding specification and formal model before release.

## Verification

The release and test builds MUST compile without business type modules or
`read_graph_*` methods. Required validation is:

```console
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
bazel test --test_output=errors //...
```

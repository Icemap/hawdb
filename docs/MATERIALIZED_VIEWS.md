# Synchronous SQL materialized views

HawDB maintains SQL materialized views in the same transaction as their base
RowPage table. A successful commit makes both visible to subsequent reads.
There is no refresh job, asynchronous lag, or `REFRESH` command. Explicit
transactions can read their own staged view changes; rollback discards them.
Existing read transactions retain their ordinary database snapshot. Views do
not introduce a separate history store or a time-travel API.

```sql
CREATE TABLE events (
    id BIGINT PRIMARY KEY,
    category TEXT,
    amount BIGINT,
    active BOOLEAN NOT NULL
);

CREATE MATERIALIZED VIEW category_totals AS
SELECT category, COUNT(*) AS event_count, SUM(amount) AS total_amount
FROM events
WHERE active = true
GROUP BY category;

INSERT INTO events (id, category, amount, active)
VALUES (1, 'work', 12, true);

SELECT * FROM category_totals ORDER BY category;

DROP MATERIALIZED VIEW category_totals;
```

Use `Database::query_sql` or `ConcurrentDatabase::query_sql`. DDL also works
inside explicit SQL transactions. `CREATE MATERIALIZED VIEW IF NOT EXISTS`
accepts an existing materialized view; it does not replace its definition or
silently accept a base table with the same name. `DROP MATERIALIZED VIEW IF
EXISTS` accepts an absent relation, but cannot drop a base table.

## Supported definitions

- One resident RowPage base table, with an optional alias.
- Column projection, aliases, and `*`. Projection preserves duplicate rows even
  when the base primary key is omitted. Wildcards bind at creation time.
- `WHERE` using column-to-literal comparisons, `IS NULL`, `IN`, `NOT IN`, and
  Boolean combinations supported by the relational mutation predicate binder.
- `GROUP BY` columns, including nullable keys and keys omitted from the output.
- `COUNT(*)`, `COUNT(column)`, and `SUM(BIGINT column)`, with or without grouping.
  `SUM` ignores NULL, returns NULL for no non-NULL input, and rejects BIGINT
  overflow before commit. Empty ungrouped aggregates retain one result row;
  empty groups disappear.
- INSERT, UPSERT, UPDATE (including primary-key or group changes), DELETE, and
  foreign-key cascade changes to the base table.

Definitions reject parameters, arbitrary expressions/functions, joins,
DISTINCT, HAVING, ORDER BY, LIMIT/OFFSET, windows, locking clauses, non-BIGINT
SUM, and other aggregates. View-on-view definitions and strict append sources
are not supported. Cypher definitions are deferred. Result queries can use the
normal supported SELECT operations and must request their own ordering.

Views are read-only: direct DML, ALTER TABLE, and CREATE INDEX on a view are
rejected, including through typed relational writes. Foreign keys cannot target
a materialized view. `information_schema.tables` reports `MATERIALIZED VIEW`
and `is_insertable_into = 'NO'`; internal identity and aggregate-state columns
are excluded from SQL wildcards and `information_schema.columns`, and cannot
be explicitly selected.

## Maintenance and admission

Creation synchronously backfills the base table under the transaction budget.
After creation, maintenance visits changed primary keys and their old/new row
images, rather than rescanning the base table. Updates retract old contributions
and add new contributions. Hidden source identities preserve projection bag
semantics; encoded group keys preserve NULL grouping without nullable primary
keys. SUM maintains a separate non-NULL count. Aggregate deltas accumulate in
i128 and are checked when publishing BIGINT results, avoiding false overflow
while replacing extreme values.

Maintenance shares one row/payload work budget across all affected views,
derived from `RelationalMutationLimits`. Reads, output changes, hydration, and
aggregate working state are bounded. Exceeding admission or aggregate limits
rejects the whole transaction before WAL append; results are never truncated.
More views increase write latency and can cause a previously admitted base
batch to exceed maintenance limits. Large initial backfills must fit admission;
background/chunked creation is not implemented.

The current implementation requires materialized relational constraints and
resident source rows. `RelationalIndexMode::Authoritative` is rejected for
databases with materialized views, including reopen and out-of-core activation.
Shadow and demand-paged index modes retain the resident maintenance path.
Transactions that modify a source with dependent views conservatively acquire
a database-wide version footprint for optimistic conflict validation.

## Persistence and recovery

The typed view definition belongs to `RelationalTableSchema`. Materialized rows
and aggregate state use existing relational row pages, overflow storage, and
checkpoint publication. WAL replay deterministically reapplies the same
transaction and derives its view changes. There is no separately committed
view file or acknowledgement. Existing durability policies and complete-WAL
transaction recovery boundaries remain in force.

The relational WAL/checkpoint codec is now version 2, and embedded relational
schema encodings include the view definition. The append WAL codec is version 3
because append schemas embed that shared relational encoding. Earlier development databases
must be recreated; no migration for the earlier development format is supplied.
This follows HawDB's greenfield policy and does not change or migrate legacy or
Cloud data stores.

The regression suite is `api::tests::materialized_views`, discovered by the
existing Cargo library and Bazel root unit-test targets. It covers full-SELECT
differential checks, duplicate projections, NULL and extreme arithmetic,
transaction rollback, bounded incremental work, concurrent publication,
overflow payloads, WAL/checkpoint reopen, and fail-closed torn WAL recovery.
Torn-tail/reopen tests are not a new power-loss qualification: machine-level
durability continues to rely on the storage synchronization assumptions and
qualification described in `AGENTS.md`.

## RisingWave reference

The design borrows continuous incremental materialization and explicit
publication boundaries from RisingWave's
[materialize executor](https://github.com/risingwavelabs/risingwave/blob/main/src/stream/src/executor/mview/materialize.rs).
RisingWave also performs an initial
[backfill](https://docs.risingwave.com/sql/commands/sql-create-mv), but its default
DML visibility does not guarantee immediate read-after-write without
[implicit flush](https://docs.risingwave.com/sql/commands/sql-set-rw-implicit-flush).
HawDB's embedded contract synchronously maintains views before acknowledging
the base transaction instead of introducing a distributed epoch/barrier runtime.

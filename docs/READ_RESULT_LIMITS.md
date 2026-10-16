# Read result limits

Issue Number: ref #1046, ref #987, ref #976

## Session policy

`SessionConfig::max_result_rows` is an unsigned 64-bit session setting. Its
default is **0**, meaning **no result row-count limit**. Positive values impose
a ceiling on each statement's returned rows, rather than on the number of rows
an aggregate may scan. Independent Cypher and SQL sessions start at 0. Settings
are held in the session and captured by its transactions; they never mutate the
database configuration or another session. Changing a Cypher session while a
transaction is active affects subsequent transactions, not the active one.

```rust
use hawdb::Database;

let mut database = Database::new();
let mut session = database.session();
assert_eq!(session.config().max_result_rows, 0);
session.set_max_result_rows(100_000);
// Execute bounded Cypher or SQL through this session.
session.set_max_result_rows(0); // no session row-count ceiling

let mut sql = database.sql_session();
sql.set_max_result_rows(100_000);
// sql.query(...), including callback transactions, inherits this setting.
```

This is a typed embedded Rust setting; SQL `SET max_result_rows` syntax and
foreign session bindings are not introduced by this change. Its name, default
and zero convention follow [ClickHouse's result-row setting](https://clickhouse.com/docs/reference/settings/session-settings/max-result#max_result_rows).
HawDB enforces positive ceilings strictly and reports errors; it does not adopt
ClickHouse's allowance for a final block to exceed the threshold. SQL/Cypher
`LIMIT 0` still returns zero rows. Maximum representable positive ceilings do
not overflow the execution detector when it computes an extra detection row. Existing explicit per-request `Option` budgets
retain their strict meaning: `Some(0)` admits zero rows, while `None` adds no
request ceiling. These low-level budgets differ from the session setting.

## Host protection and memory

The existing `DatabaseConfig::max_read_result_rows` remains an optional host
ceiling. Its default is `Some(0)`; both `Some(0)` and `None` mean unlimited.
A positive host ceiling cannot be raised or disabled by a session or cursor.
The SQL frontend does not substitute the former 100,000-row default when the
host is unlimited. Explicit host ceilings continue to bound SQL intermediate
work; a session's result ceiling does not lower that intermediate-work budget.

`max_read_result_payload_bytes` remains 64 MiB by default. The default embedded
runtime governor independently allows 16 MiB of retained/result capacity;
physical query memory and plan-derived admitted reservations remain enforced.
A result may therefore refuse a byte budget despite having no row-count limit.
Python `hawdb.open()` inherits these database defaults. Rust hosts can use the
existing configured constructors and typed runtime-governor configuration.

Raising query-memory bytes alone does not enlarge a plan's admitted reservation.
Record effective admission, execution batch/operator allowances and separately
charged retained source capacity when qualifying large results. Historical
100,000-row default-refusal evidence describes its original source only; it is
not evidence for this changed default. Configured large-result measurements
remain separate from default-budget measurements.

## Completion and ownership

Owned reads validate output before successful return. An over-limit query
errors rather than publishing a truncated `QueryOutput`. Counts and aggregates
must compute complete values or explicitly refuse bounded work. Session-limited
SQL `RETURNING` is checked before publishing the staged mutation; a failed
statement/callback does not commit its partial writes.

Retained cursors opened through `DatabaseSession::query_with_params_retained`
and Arrow streams derived from them capture the session ceiling. They may emit
valid prefix batches before a terminal row/payload refusal. Consumers must
treat that error as incomplete execution, never successful EOF. Failures remain
sticky and release the source pin. Arrow reports a nonzero stream error with
diagnostic text. Batch size is not a total-row limit. Dropping all cursor and
batch owners releases their charges. Retained delivery inside an explicit
Cypher transaction remains unsupported and refuses explicitly.

Registered execution-request consumer tests cover default and explicit host
limits across 99,999/100,000/100,001 rows on memory/file databases; complete
owned/retained/Arrow values; sticky refusals; released ownership; unchanged read
epochs; query LIMIT; complete counts; subsequent writes; session isolation,
reset and transaction inheritance; SQL mutation rollback; and SQL output above
the former default, including ordered blocking projection. Targeted unit tests
also preserve independent file-I/O/payload budgets and final EXPLAIN admission.

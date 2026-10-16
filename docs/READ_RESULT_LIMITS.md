# Read result limits

Issue Number: ref #987, ref #976

`DatabaseConfig::default()` sets `max_read_result_rows` to 100,000 and
`max_read_result_payload_bytes` to 64 MiB. These bound one read's output;
they do not bound the number of records a database can store. The default
embedded runtime governor additionally allows 16 MiB of retained/result
capacity, independently of logical payload. A result can reach a memory
budget before reaching the row limit.

Python `hawdb.open()` uses the embedded facade's default configuration.
Rust hosts can supply an explicit `DatabaseConfig` through the existing
configured constructors and an explicit typed runtime governor. Raising the
database row limit alone does not raise payload, physical query-memory or
runtime-governor capacity. Large-result qualification must record each of
these settings and remain separate from the unchanged default controls.

Ordinary owned reads validate their result before successful return.
Over-limit delivery returns an error and does not publish a truncated
`QueryOutput`. A count or aggregate must compute its complete value or
refuse bounded work; the output row limit must not turn a count into the
size of an arbitrary prefix. SQL has an additional intermediate-work
limit derived from the database configuration, so an aggregate may refuse
that work even when its final output would contain only one row.

Retained and Arrow streams are incremental. Consumers may receive valid
prefix batches before a terminal row/payload refusal. They must treat the
terminal error as an incomplete query, never as successful EOF. The cursor
records a sticky failure, releases its source pin, and repeats the error.
Arrow reports a nonzero stream error with diagnostic text. A SQL/Cypher
`LIMIT` is a query operator; cursor options combine with the database cap
using the stricter limit and cannot raise it. Batch size is not a total-row
limit. Closing/dropping all cursor and batch owners releases their charges.

`default_read_row_limit_is_exact_and_never_silently_truncates_delivery`
in the registered execution-request consumer tests checks 99,999,
100,000 and 100,001 rows with the actual default configuration on memory
and file databases. It verifies ordered values through owned, retained
and Arrow C Stream delivery, sticky refusal, releases, aggregate counts,
query LIMIT, stricter caller limits, unchanged read epochs and successful
subsequent writes. `configured_sql_row_limits_refuse_work_instead_of_returning_partial_aggregates`
checks a low configured cap, complete aggregate-or-refusal semantics and
an exact lookup beyond the refused scan prefix.

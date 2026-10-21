# Vector column schema

This first implementation slice is tracked by
https://github.com/nowledge-co/hawdb/issues/1152. It establishes the relational
schema needed for Mem's filtered vector queries. It does not change the App
persistence schema, import mappings, storage authority or release activation.

## SQL and type contract

```sql
CREATE TABLE memories (
    id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL,
    embedding VECTOR(1024)
);
ALTER TABLE memories ADD COLUMN embedding_v2 VECTOR(768);
```

`VECTOR(n)` denotes a fixed-dimension Float32 vector. The dimension is a positive
32-bit unsigned integer (`1..=4294967295`), represented by the validated shared
`VectorDimension` type. This is a schema representation bound, not a promise
to admit a vector payload of that size. Declaring a dimension never allocates
a dimension-sized buffer. Future value admission must separately enforce byte
and work limits.

The type accepts one decimal integer modifier. Missing, zero, negative,
fractional, overflowed and multiple dimensions are errors. Unquoted names are
case-insensitive; quoted `"vector"` is accepted. Qualified names and vector
array types are not supported. Dimensions are not query parameters.

Nullability remains a column constraint. A nullable vector column can be added
to existing rows, and its value is NULL. `DEFAULT NULL` is accepted on nullable
columns. `NOT NULL` can be declared on an empty table but cannot be populated
until non-NULL vector values are supported. Existing NOT NULL validation remains
in force.

## Deliberate first-stage limits

Non-NULL vector values and defaults are rejected, including lists, bytes,
strings and scalar numbers. This stage does not add a vector value representation,
payload codec, implicit coercions or distance functions.

Vector columns cannot participate in primary/unique/foreign keys or ordinary
secondary indexes. Scalar indexes on other columns remain supported. Strict
Append tables reject vector columns. SQL/PGQ creation binding rejects vector
property exposure; explicitly selecting only supported scalar properties from
the same source remains valid. Cypher property bindings and ANN DDL are later
deliveries, not implicit capabilities of this schema change.

## Introspection

`information_schema.columns` reports `data_type = 'USER-DEFINED'`,
`udt_schema = 'public'` and `udt_name = 'vector'`. Other types retain their
existing catalog values. HawDB's read-only catalog exposes the dimension:

```sql
SELECT table_schema, table_name, column_name, dimensions, element_type, nullable
FROM system.vector_columns
WHERE table_name = 'memories'
ORDER BY column_name;
```

`element_type` is `float32`. This metadata does not assert that non-NULL values
are writable or that any vector index exists.

## Persistence and compatibility

The existing schema tags 1 through 6 are unchanged. Tag 7 is followed by a
little-endian u32 dimension, before column nullability/default fields. Zero
dimensions and truncated fields fail decoding. Existing scalar schema bytes
are unchanged; older readers reject the new schema tag. This additive
development format has no downgrade promise for databases containing vectors.

Both ordinary and cooperative schema writers/readers retain dimensions through
WAL replay and checkpoint reopen. Index schema digests include the dimension
even when only scalar columns have indexes. Rejected DDL/DML cannot publish a
partial transaction. This change adds no publication or concurrency transition;
it uses the existing schema transaction and recovery boundaries.

## Verification

The existing core and SQL targets own dimension/type and syntax tests. Storage
tests own malformed/truncated schema bytes and ordinary/cooperative fingerprint
parity. Root SQL session tests own CREATE/ALTER, catalog inspection, NULL rows,
scalar secondary indexes, rollback, WAL replay, checkpoint and read-only reopen.
The tests use synthetic data and do not activate HawDB in Mem.

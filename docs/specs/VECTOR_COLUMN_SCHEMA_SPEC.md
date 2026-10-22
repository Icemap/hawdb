# Shared vector schemas, values and exact distance

Tracked by https://github.com/nowledge-co/hawdb/issues/1152 and
https://github.com/nowledge-co/hawdb/issues/1154. This implements the database
foundation for filtered vector queries without changing Mem's App schema,
import mappings, storage authority or release activation.

## Type and value contract

SQL columns and Cypher properties share `VectorDimension`, `Vector` and
`VectorRef` from `hawdb-core`:

```sql
CREATE TABLE memories (
    id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL,
    embedding VECTOR(1024)
);
ALTER TABLE memories ADD COLUMN embedding_v2 VECTOR(768);
```

```cypher
CREATE NODE TABLE Memory;
CREATE PROPERTY ON NODE TABLE Memory(embedding) TYPE VECTOR(1024);
```

`VECTOR(n)` holds exactly `n` finite Float32 coordinates. Dimensions are positive
u32 values. This schema bound does not promise admission of equally large
payloads: existing value, page, query memory and checkpoint limits still apply.
Declaring a dimension does not allocate a dimension-sized buffer. Neither lists
nor bytes implicitly coerce to vectors. Missing, zero, negative, fractional,
overflowed and multiple dimensions are errors. SQL accepts unqualified,
case-insensitive `VECTOR` and quoted `"vector"`; arrays and qualified names are
unsupported. Dimensions are not query parameters.

Nullability is independent of dimension. Nullable SQL columns accept NULL;
`DEFAULT NULL` is accepted. Typed schema APIs also accept matching vector
literal defaults. Writes of a different dimension fail through the existing
transaction boundary. Vector SQL primary, unique, foreign and ordinary index
keys remain unsupported. Strict Append tables and SQL/PGQ property exposure
remain unsupported. Existing graph equality indexes retain their general
value equality behavior; they are not nearest-neighbor indexes.

Rust callers construct `Value::Vector(Vector::new(&coordinates)?)` and bind it
as a query parameter. JSON and Python bindings reserve the single-key object
`{"$vector": [1.0, 0.0]}` for explicit Float32 conversion. Retained C values use
kind 11; retained Python values use kind `vector`. Their read-only byte buffers
contain the canonical payload described below. Ordinary numeric lists keep
their existing list type. SQL/Cypher text vector constructors are not added. SQL distance operands in
this slice are columns, parameters or NULL; nested operand functions are
rejected during preparation, including on empty inputs. A distance result may
be wrapped in SQL COALESCE.

## Exact filtered top-k and candidate-window filtering

`cosine_distance(a, b)` and `l2_distance(a, b)` use the same borrowed core
implementation in SQL and Cypher. They require matching dimensions, propagate
NULL and accumulate in Float64. L2 returns Euclidean distance, not squared L2.
Cosine returns `1 - cosine_similarity`, in [0, 2], and rejects zero-norm
vectors. It does not reuse search relevance scores clipped to [0, 1].

The ordinary query pipeline filters eligible rows before exact scoring and
ranking. SQL currently projects the distance and orders by its alias:

```sql
SELECT id, cosine_distance(embedding, $1) AS distance
FROM memories
WHERE space_id = $2 AND embedding IS NOT NULL
ORDER BY distance, id
LIMIT 20;
```

Graph traversal can first produce a small eligible candidate set:

```cypher
MATCH (root:Collection)-[:CONTAINS]->(m:Memory)
WHERE m.space_id = $space AND m.embedding IS NOT NULL
RETURN m.id AS id, cosine_distance(m.embedding, $q) AS distance
ORDER BY distance, id
LIMIT 20;
```

These plans compute exact distance for their eligible input. No optimizer rule
replaces them with an approximate candidate window. The tie-breaker `id` makes
ordering deterministic. The caller supplies any tenant/ACL eligibility filters;
vector functions do not invent access policy.

The existing explicit candidate source has different semantics:

```cypher
CALL vector_search($q, topK := 100)
YIELD id, score
MATCH (m:Memory)
WHERE m.space_id = $space AND m.eligible = true
RETURN m.id AS id, score
ORDER BY score DESC
LIMIT 20;
```

Here `topK` bounds the search provider's candidate window. Existing safe
metadata predicates such as `space_id` can be pushed into the provider before
candidate generation. Residual graph predicates such as `eligible` filter the
returned window afterwards. Fewer than 20 results can remain even when other
eligible nodes exist.
The provider can be approximate; its score contract is unchanged. Typed vector
parameters and legacy numeric-list parameters are accepted at this existing
procedure boundary. This change adds neither ANN indexes nor filtered ANN
planning, and does not claim approximate global top-k completeness.

## Encoding, ownership and compatibility

The shared payload is a little-endian u32 dimension followed by exactly that
many little-endian f32 bit patterns. Coordinates are finite; signed zero bits
are preserved. Borrowed row-page views need no alignment and do not copy the
payload. Exact value equality is bitwise, distinct from distance equality.
Malformed lengths, dimensions and nonfinite coordinates fail decoding.

| Container | Vector discriminator and framing |
| --- | --- |
| Relational schema | Tag 7, then u32 dimension |
| Graph property type in WAL | `(u64(dimension) << 8) \| 7` |
| Relational WAL/checkpoint value | Tag 8, u64 byte length, canonical payload |
| Relational row-page value | Tag 8, u32 byte length, canonical payload |
| Canonical graph value | Tag 10, u32 byte length, canonical payload |
| Graph WAL value | Length-delimited field 10, canonical payload |
| Graph text value | `v` followed by hex canonical payload |
| Executor spill value | Tag 9, u64 byte length, canonical payload |

Existing scalar encodings are unchanged. Older readers reject new tags; vector
databases have no downgrade guarantee. Borrowed-to-owned copies charge payload
bytes. Cooperative copying, validation and comparison yield in bounded chunks;
checkpoint schema fingerprints include vector dimensions and defaults.

## Introspection and verification

`information_schema.columns` reports `USER-DEFINED`, public `vector`.
`system.vector_columns` exposes `table_schema`, `table_name`, `column_name`,
`dimensions`, `element_type = 'float32'` and `nullable`. Catalog metadata does
not imply an ANN index exists.

Core tests cover encoding, nonfinite values, exact math and bounded validation.
Storage tests cover codecs, fingerprints, cooperative cancellation and nested
values. SQL session tests compare both languages over identical data before
and after WAL/checkpoint read-only reopen, including dimension rejection and
rollback. Executor tests distinguish eligible exact top-k from candidate-window
post-filtering and check actual allocations. Native binding tests cover typed
parameters and retained payload ownership. See [test index](../TEST_INDEX.md).

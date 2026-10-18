# JSON scalar extraction in Cypher

[`json_value`](https://github.com/nowledge-co/hawdb/issues/1083) extracts one
scalar from a JSON document stored in a `STRING` property. The path is a string
literal or query parameter and uses `$` followed by one or more dotted field
names. Field names contain ASCII letters, digits, `_`, or `-`; arrays, quoted
keys, and wildcards are not supported. The path is at most 256 bytes.

```cypher
MATCH (m:Memory)
WITH json_value(m.metadata, $path) AS origin, COUNT(*) AS total
RETURN origin, total ORDER BY origin
```

For `$path = '$.original_source'`, this groups by the extracted value inside
the database. The function also works in scalar predicates, `CASE`, and
`COALESCE`. It returns `NULL` when the document is `NULL`, the path is absent,
or the selected value is JSON `null`. JSON strings become Cypher strings,
booleans become booleans, signed 64-bit integers become integers, and other
JSON numbers become floats. An unsigned integer outside the signed 64-bit
range, a selected object or array, malformed JSON, a non-string document, or
an invalid path fails the query; none is silently converted to a source name.

One JSON document may be at most 1 MiB. The evaluator reserves temporary query
memory before parsing, and the ordinary result row and payload budgets still
apply. The limit is per evaluated document, so callers should use a selective
predicate when possible. This expression does not add an index on JSON members
or alter stored property bytes.

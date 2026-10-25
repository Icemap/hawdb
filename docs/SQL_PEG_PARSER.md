# Owned SQL/PGQ token PEG

`hawdb-sql-syntax` uses a token PEG for its existing SELECT, CREATE PROPERTY
GRAPH and standalone GRAPH_TABLE entry points. It retains the lexer, byte
spans, syntax AST and `hawdb-sql` binder/lowering contracts. Production
relational SQL still uses `sqlparser`; production Cypher is unchanged. Parser
failures never trigger another frontend. This delivery does not qualify new
SQL syntax or activate SQL/PGQ execution.

## Design reference

DuckDB 2.0 makes its PEG frontend the default. Its
[parser documentation](https://duckdb.org/docs/preview/sql/peg_parser) describes
token matching, packrat memoization, source locations, suggestions and grammar
extensions. Its [implementation guide](https://github.com/duckdb/duckdb/blob/main/src/parser/peg/README.md)
separates grammar matching from AST transformation and describes named rule
composition and native matchers. Ordered choice makes rule order significant;
it does not guarantee that adding an alternative preserves the old language.

HawDB adopts that separation without copying DuckDB code or adding a parser
generator dependency. The bounded existing language does not yet justify a
textual grammar compiler, generated transformer framework, runtime plugin ABI,
custom tokenizer registration or catalog-aware completion. Those are separate
features, not consequences of using PEG.

## Grammar and transformation

`src/parser/frontend/grammar.rs` defines named rules using typed Rust
combinators: token/keyword, sequence, ordered choice, optional, repetition,
and positive/negative lookahead. Keywords match unquoted words case-insensitively.
The SQL grammar exports `Statement` and `FromItem` alternatives; PGQ prepends
its rules before immutable compilation. In particular, GRAPH_TABLE composes
inside joins and FROM lists rather than retrying after a SQL parse error.
A negative keyword guard prevents a broken GRAPH_TABLE prefix from becoming
an ordinary relation.

`src/peg.rs` builds a compact tree of named captures and native terminal values.
`src/parser/frontend/transform.rs` consumes only the selected tree to produce
the existing syntax AST. Native terminals in `frontend/native.rs` retain
bounded Pratt expressions, graph paths, property-graph element definitions,
and lexical alias rules. They do not mutate catalog or execution state.
There is no whole-statement handwritten parser behind a PEG acceptance check.

Grammar compilation rejects duplicate or unresolved names, direct/indirect
left recursion (including through nullable prefixes/lookahead), and nullable
repetition. Right recursion is permitted within the runtime depth bound.
Alternatives are attempted from the same token position; the first success
wins. A later sequence failure does not revisit an earlier successful choice.
The AST transformer and grammar form one internal contract and change together.

## Resources and diagnostics

A parser invocation owns its memo table and capture arena. Successful and
failed named rules are cached by rule ID and token position; successful hits
reuse capture IDs instead of cloning syntax trees. Token failures store positions
and static expectations, rendering source text only for the final error; native
errors share their existing diagnostic. Cached diagnostics preserve
the farthest failure through later reuse. Lookahead discards its own speculative diagnostic. Optional/repeated
branches discard absent-prefix mismatches while preserving deeper failures. Ordinary syntax errors select the farthest byte
position, with the first expectation retained on ties. Expected-token sets and
interactive completion are not exposed yet.

Default limits are 4,000,000 matching work units, 262,144 memo entries,
262,144 capture nodes and separately 262,144 capture edges, and 128 nested
matcher expressions. Work counts rule dispatch, combinator evaluation and
native consumed/scanned token ranges. Native terminals are atomic bounded
operations: their token-range charge is checked after they return; the lexer
input/token caps and their own nesting checks also apply. These are logical
bounds, not a byte-precise allocator admission guarantee or a wall-clock limit.
The existing lexer limits remain 16 MiB input, 1,000,000 tokens and 64 nested
block comments; native expression and graph nesting limits remain 128.

`ParserLimitExceeded` identifies PEG work/storage/depth refusal. Native resource
errors retain their existing error codes. Resource failures are fatal across
choice, optional, repetition and lookahead, and are never treated as a failed
alternative. All state is per invocation, so rejected input cannot poison the
next parse. No syntax repairs are applied.

## Qualification and remaining boundary

Existing syntax and frontend differential fixtures remain the compatibility
oracle. Engine regressions include malformed nested alternatives that would
have exponential cost without failure memoization, cache reuse, source spans,
and exact budget boundaries. Source-bound local parser campaigns supplement
those cases. See [test ownership](TEST_INDEX.md#owned-sqlpgq-token-peg).

This migration adds matching/capture overhead to the owned frontend; it does
not claim a throughput improvement. Full production SQL activation still
requires statement-family positive/negative parity, executable lowering,
query-result/plan coverage and resource measurements, as required by the
[SQL/PGQ specification](specs/POSTGRES_SQL_PGQ_SPEC.md).

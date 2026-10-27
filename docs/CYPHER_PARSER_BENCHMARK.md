# Cypher parser benchmark

HawDB keeps parser changes evidence-driven. The production benchmark covers
short exact lookup, bounded expansion, aggregate pagination, mutation, and
query-hint shapes while constructing the complete production AST.

Run the production parser benchmark with:

```bash
cargo bench -p hawdb-cypher --bench cypher_parser
```

The retired single-query Yacc experiment has been removed. It did not cover
the production grammar or establish a migration decision. Cypher continues to
use the production parser and weighted benchmark above. A future Cypher parser
migration still requires:

- equal AST and rejection behavior over the production query corpus;
- differential fuzz coverage for valid and invalid inputs;
- no allocation or error-path regression;
- at least 20 percent parser throughput improvement on the weighted corpus;
- a measurable end-to-end query CPU or latency improvement.

The owned SQL/PGQ frontend uses a separate [token PEG implementation](SQL_PEG_PARSER.md).
This is a syntax-maintainability change, not evidence for a Cypher throughput
improvement or a production relational SQL parser switch.

The asynchronous runtime prepares each query once. It reuses the prepared AST
and physical plan only while the optimizer schema and statistics epoch match;
otherwise it keeps the AST and replans against the execution snapshot.

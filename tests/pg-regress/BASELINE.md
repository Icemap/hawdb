# Initial PostgreSQL 18.0 comparison

Reference: checksum-verified official PostgreSQL 18.0 source, compiled on macOS
ARM64 into an isolated temporary prefix/cluster; server version number 180000.
HawDB engine base: `f77489ae9` (the harness was an uncommitted working tree).
The JSON report includes harness/input/binary hashes and the dirty-tree flag;
this is local development evidence, not remote CI or production qualification.

| Selection | PASS | MISMATCH | UNSUPPORTED |
| --- | ---: | ---: | ---: |
| Raw upstream selected cases | 3 | 57 | 4 |
| Separately named fixture-adapted isolation cases | 5 | 0 | 0 |
| Total | 8 | 57 | 4 |

The default full selection intentionally exits 1. Failures remain visible;
they are not rewritten into expected successes or dynamically skipped. A case
stops at its first difference, so later statements are not counted as covered.
Source files contain many additional statements that are not selected at all.

The raw differences encountered first are:

- `INT` is rejected, including the original write-skew table definition.
- Relational tables require primary keys, unlike the original Boolean/int8
  fixtures. This stops those cases before their comparison queries.
- PostgreSQL typed literals such as `BOOL 't'` are not accepted in projections.

The five passing adapted isolation cases cover both serial write-skew orders,
two overlapping orders with a required mapped serialization failure, and the
upstream read-only-anomaly snapshot schedule. Only fixture column types and
explicit INSERT targets are adapted, identically on both engines. These passes
do not establish native INT/DECIMAL/implicit-column compatibility or full SSI
outcome equivalence. The current scheduler does not model blocking/waiting steps.

Local harness controls are now native Rust tests for result comparisons,
libpq decoding and provenance. They deliberately change multiplicity, order,
value kinds, column associations, affected counts and typed errors, and corrupt
an upstream input. The pure Rust CLI reproduced the matrix above. An actual
PostgreSQL 18.6 server was refused before any case ran (exit 2, version 180006).
No Python scheduler or subprocess backend is used.

Reproduce the live comparison using the commands in [README.md](README.md).
Use `--case adapted/isolation/` for the bounded passing MVCC selection, or the
full schedule to retain the raw compatibility baseline. Never present this
matrix as the pass rate of the entire PostgreSQL regression suite.

## HawDB-only upstream regressions (2026-10-25)

Engine base: `19ca97b6bc79f78fa6f8fe72c45b8cf81095adad`; no engine changes.
The new tests execute `Database::query_sql` directly, with default HawDB in-memory
configuration and minimal features. They do not connect to PostgreSQL. The live
comparison matrix above is historical and was not rerun for this change.

```bash
make pg-regress-hawdb
```

Result: **21 passed**, comprising 19 migrated PostgreSQL cases and two existing
provenance/selection controls. No ignored cases or failures:

| Upstream selection | Cases | Expected output |
| --- | ---: | --- |
| `int8.sql`, column-left comparisons | 12 | `expected/int8.out` |
| `select_having.sql`, ungrouped HAVING | 6 | `expected/select_having.out` |
| `select_implicit.sql`, non-projected order key | 1 | `expected/select_implicit.out` |

`data/hawdb-expectations.json` records exact output ranges, integer/text kinds
and the two ungrouped-column error mappings. Each range must echo the unchanged
upstream query. Expected rows are decoded from the checksum-verified original
output, never recorded from HawDB results. The new SQL and expected files match
the pinned PostgreSQL repository byte-for-byte.

Only setup is adapted: mandatory primary keys, BIGINT/TEXT fixture types,
explicit INSERT targets and numeric literals for the int8 input strings. The
int8 assertion excludes only its added synthetic key column. Original values,
duplicate counts and explicit ordering remain covered. See [README.md](README.md)
for exclusions. All 19 selected cases passed without requiring an engine fix;
unimplemented SQL constructs were not added or counted as successful tests.

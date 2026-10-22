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

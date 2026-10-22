# PostgreSQL 18.0 compatibility runner

This independent, unpublished test project follows RisingWave's
[`src/tests/regress`](https://github.com/risingwavelabs/risingwave/tree/main/src/tests/regress)
layout. It reuses PostgreSQL inputs while calling the embedded HawDB library;
it does not require or add a PostgreSQL protocol server to HawDB.

## Layout and provenance

- `src/main.rs`: a JSON-lines Rust worker over the real embedded transaction API.
- `src/runner.py`: a standard-library Python driver using the installed libpq C
  client; no Python database package is needed.
- `data/sql/`, `data/expected/`, `data/isolation/`: unmodified PostgreSQL source.
- `data/cases.json`: checksums, upstream paths, exact SQL/line ranges, explicit
  adaptations and unsupported cases. Add a selected case here without changing
  the upstream source. A source edit requires a reviewed provenance update. Separately named `adapted/`
  cases may attach an explicit fixture-only `execute_sql` to a source step;
  both engines execute that same SQL and the report retains original/effective
  SQL and the reason. Raw upstream cases remain unchanged.
- `data/schedule`: one serial `test: GROUP` per line. Groups execute in schedule
  order and cases in manifest order; this is not a parallel pg_regress scheduler.
- `POSTGRESQL-LICENSE`: upstream license for the vendored files.

All upstream files are pinned to PostgreSQL `REL_18_0`, commit
`3d6a828938a5fa0444275d3d2f67b64ec3199eb7` (version number **180000**).
The stock expected files are retained for provenance. Actual PG 18.0 execution
is the reference; the runner never replaces it with an expected-output snapshot.
The manifest is a selected subset, not a claim to run all statements in the
vendored source files or the full PostgreSQL test suite.

## Run locally

Use Python 3.10+ on macOS/Linux (the worker pipe timeout uses POSIX select),
Cargo with the pinned toolchain, and an installed libpq shared
library. Set `--libpq` when library discovery cannot find it. The libpq client
may be a different version; the connected **server must be exactly 18.0**.

Start a dedicated PostgreSQL 18.0 test instance yourself. For example, a local
container can expose `postgres:18.0` on a chosen loopback port; the runner does
not create containers or install PostgreSQL. Give the test role permission to
create schemas. Each case gets a unique `hawdb_pg18_*` schema, which is removed
on completion after its sessions close; an interrupted process may leave that
named test schema for inspection. Do not point the tool at an application DB.

```bash
make pg-regress-list
export PGREGRESS_DSN='host=127.0.0.1 port=58432 dbname=postgres user=hawdb_test'
make pg-regress
make pg-regress PG_REGRESS_ARGS='--case boolean/scalar --report target/pg-regress/scalars.json'
```

The default command builds the worker once with Cargo. Connection information
stays in the environment and is not written into the report. `--dsn-env NAME`
selects a different variable. Connections have a five-second connect timeout,
three-second statement timeout and 500-ms lock timeout. A worker response has a
separate timeout. A timeout is never classified as a passing or unsupported case.

An existing Cargo/Bazel worker can be supplied explicitly:

```bash
bazel build //tests/pg-regress:hawdb_pg_regress_worker
make pg-regress PG_REGRESS_ARGS='--worker bazel-bin/tests/pg-regress/hawdb_pg_regress_worker'
```

The project, its worker and campaigns are **manual only**, excluded from default
CI, scheduled workflows, the library workspace and published library archives.
The worker target deliberately has a `manual` tag. It executes only synthetic
fixtures, with report limits of 4096 reference rows and 1 MiB per reference cell;
these checks occur after libpq receives the result, not before network allocation.

## Comparison contract

Each statement reports PostgreSQL and HawDB observations and its source range.
Rows retain value kinds and NULL. The embedded API exposes named rows, so
unique named columns are compared by name/value association rather than PG
projection order; projection-order equivalence is not claimed. PostgreSQL int2/int4/int8 map
losslessly to HawDB's signed integer domain; this does not verify integer-width
metadata equivalence. Float formatting is normalized through an IEEE float;
exact decimal values are not converted to float. Text/Boolean/integer/NULL stay
separate. Ordered cases retain order. Unordered cases compare multisets,
preserving duplicate counts. DML affected-row counts are compared. Column names
are compared unless an individual step documents the unnamed-label adaptation.
Empty-result value-type metadata cannot be compared because the embedded output
schema exposes names rather than PostgreSQL type OIDs.

- `PASS`: every selected statement agrees under these explicit mappings.
- `MISMATCH`: the first unequal observation; remaining case statements are not
  run. The trace retains both values/errors and original SQL.
- `UNSUPPORTED`: a manifest-declared scope exclusion with a reason; never a pass.
- `INFRA_ERROR`: wrong server, bad provenance, no matched cases, connection loss,
  worker failure, timeout, or cleanup failure. Incomplete execution cannot pass.

Exit codes: 0 when at least one case passes and no mismatch/infrastructure error
occurs; 1 for a mismatch or zero executed passes; 2 for infrastructure failure.
Unsupported cases remain visible in a successful supported-subset run.
Two arbitrary errors never count as agreement: a negative test needs a specific
PostgreSQL SQLSTATE/HawDB typed error mapping in its step manifest.

Transactions map `REPEATABLE READ` to HawDB Snapshot Isolation and `SERIALIZABLE`
to explicitly enabled Serializable validation. Both engines capture the snapshot
at the first statement, not BEGIN. Only nonblocking sequential schedules are
supported initially. The selected `simple-write-skew` permutations are a subset
of the source spec's automatically generated interleavings. PostgreSQL SSI may
abort at a different step or choose a different victim from HawDB's commit-time
validation. A strict stepwise mismatch is diagnostic evidence, not automatically
an isolation bug. Full wait-aware isolationtester semantics and allowed-outcome
validation remain separate follow-ups; unsupported schedules are not flattened
into serial execution.

## Verify the harness

```bash
python3 -m unittest discover -s tests/pg-regress/src -p 'test_*.py'
cargo fmt --manifest-path tests/pg-regress/Cargo.toml -- --check
cargo clippy --locked --manifest-path tests/pg-regress/Cargo.toml --all-targets -- -D warnings
```

Comparator controls deliberately change duplicate multiplicity, row order,
value kinds, result widths, affected-row counts and typed errors. Provenance is
checked before every listing/run. Real execution on the pinned PostgreSQL server
is additional evidence, separate from these controls and from ordinary HawDB CI.

## Initial compatibility baseline

The raw upstream fixtures deliberately expose current gaps: HawDB requires a
primary key, rejects the `INT` alias in these statements, and does not accept
these PostgreSQL typed Boolean literals. A default broad run therefore reports
mismatches and exits 1; this is useful baseline output, not a green test suite.
Do not silently exclude these failures to obtain a pass.

The separately named `adapted/isolation/` group spells out common fixture column
types and INSERT target columns. It retains the original transactional SQL and
step order. The read-only-anomaly fixture uses BIGINT for its exact integer
balances instead of DECIMAL. This validates a bounded transaction comparison,
not native INT/DECIMAL or implicit INSERT-column compatibility. Explicit
`40001`/`TransactionConflict` mappings cover the two selected write-skew aborts.

```bash
make pg-regress PG_REGRESS_ARGS='--case adapted/isolation/ --report target/pg-regress/isolation.json'
```

See [BASELINE.md](BASELINE.md) for the initial measured compatibility matrix.
The scoped `.gitattributes` preserves original psql padding in checksum-pinned
expected files; it does not relax Rust/Python formatting or runtime assertions.

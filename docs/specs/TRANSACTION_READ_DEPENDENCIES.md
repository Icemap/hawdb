# Transaction read dependencies and optional serializable validation

This implements packages C and D of [the COW MVCC evolution design](MVCC_COW_EVOLUTION_SPEC.md),
tracked by [read capture](https://github.com/nowledge-co/hawdb/issues/1139) and
[commit validation](https://github.com/nowledge-co/hawdb/issues/1140).
It does not activate HawDB in Mem or change the existing SI qualification gates.

## Public contract

`Database::begin_transaction_with_isolation` takes `TransactionIsolation` and
`TransactionReadDependencyLimits`. Its `_and_context` variant also carries host
cancellation. `ConcurrentTransactionOptions::with_isolation` selects the same
mode independently of optimistic/pessimistic locks. Existing constructors retain
snapshot isolation and allocate no read capture. No SQL isolation syntax or
automatic transaction replay is added.

`Serializable` is an explicitly qualified mode. All statements must be within
the supported set below. Unsupported inputs return
`HawDBError::TransactionReadDependencyRefused`; execution errors, parse errors,
cancellation and budget failures make completeness permanently refused. Ignoring
an error or restoring a statement savepoint cannot recover completeness. A host
must drop/roll back and begin a new transaction. Refusals are distinct from
retryable `TransactionConflict` errors. Rust exhaustive error matches and direct
`ConcurrentTransactionOptions` struct literals must accommodate the new variant
and fields; constructor callers keep their old behavior.

`read_dependency_usage()` exposes inspection counters, never a detached token,
cloneable proof or authorization to publish. Captures are internal and non-Clone.
Default limits are 4,096 keys and 1 MiB of estimated metadata per transaction.

## Capture and ownership

[The capture owner](../../src/api/transaction_read_dependencies.rs) is created
before the private workspace from one canonical database instance/branch and
base epoch, with a `Schema` stamp interpreted at that epoch. It owns the existing
storage history pin and read-summary tracking lease. Ordinary transaction state
retains it across SQL and Cypher statements, private writes and commit queueing;
`take_for_commit` moves it. Drop/rollback/error releases the same owner. No
validation token or descendant may outlive that protection. Retention and COW
accounting remain the existing storage contract, owned separately by package A.

Admission reserves the inline capture owner before registering its storage pin.
The remaining byte budget applies to `VersionWriteSet` key payload estimates;
its existing admission checks precede retained insertion. Duplicate identities
are deduplicated. Refusal preserves previously admitted entries and their byte
count, but invalidates completeness. This estimate excludes allocator nodes,
shared pin/summary registries, COW pages and result buffers; it is not a process
heap limit or an exact retained-root charge. Existing host admission and result
limits still apply independently.

Metadata preflight caps input plus scalar parameters at 64 KiB and 1,024
parameters. Compound parameter values refuse. SQL also caps 4,096 lexical tokens,
32 conservative recursive tokens, 1,024 expression nodes, 64 expression levels,
32 join/key columns, 4 KiB key/table names and 1,024 table columns. Cypher uses the
existing parser recursion limit and qualified collector's 1,024 adjacency-work
budget. These are metadata bounds, not a universal query execution work limit.
Cooperative context checkpoints run before capture work and execution; supplied
host deadlines/cancellation remain cooperative.

## Supported-shape matrix

Collection runs before ordinary execution against the transaction's private
view; it neither substitutes a new query engine nor infers dependencies from
returned rows. Sources are [SQL collection](../../src/api/dependency_read/relational.rs),
[SQL prefix qualification](../../src/api/dependency_read/relational/indexed.rs),
[graph collection](../../src/api/dependency_read.rs),
[adjacency qualification](../../src/api/dependency_read/graph_match.rs) and
[statement qualification](../../src/api/transaction_read_dependencies.rs).

| Shape | Complete capture or explicit refusal |
| --- | --- |
| SQL necessary full primary-key equality under AND | Exact row identity, including absence and payload changes; residual conditions do not remove the identity |
| SQL necessary leading equality on a declared index | Complete index-prefix domain; insert/delete, entering/leaving predicates and non-index payload changes invalidate it |
| SQL unsafe index/type or nullable unique suffix | Complete table domain where qualified; ordinary incompatible comparison errors remain errors and refuse completeness |
| Other qualified SQL single-table scans, ranges, projections, empty results, ordering, LIMIT and DISTINCT | Complete input table domain; no inference from output pages or duplicate multiplicity |
| Qualified SQL joins | Complete domains of every canonical input table, including absent/filtered parents; conservative for unrelated rows |
| SQL INSERT/UPDATE/DELETE, conflict handling and RETURNING | Complete target table domain, including mutation predicates and column/arithmetic inputs; ordinary constraint/write validation remains required |
| SQL omitted generated UUIDv7 default | Refused: an unversioned generated input is outside this subset |
| SQL aggregate/function expressions, strict-append tables, virtual catalogs, EXPLAIN, branch statements and noncanonical inputs | Refused; do not claim SQL aggregate support from graph aggregate coverage |
| Qualified graph point/id/equality reads | Existing exact node or typed property domain, including absent matches; unsafe narrow qualification uses complete label domains |
| Graph label scans, supported aggregate/count, payload, DISTINCT/order/LIMIT | Complete source label domains or the qualified underlying input scope |
| Graph optimizer-folded empty plan | Whole-database epoch barrier; conservative but never an empty proof |
| Qualified single-hop adjacency | Complete existing adjacency/end-point/relationship scopes; supported complete source domains can conservatively cover the hop |
| Variable-length traversal, unsupported operators and unqualified relationship predicates | Refused |
| Deterministic graph mutations and supported schema DDL | Whole-database epoch barrier, covering predicates/expressions and empty mutations; deliberately conservative |
| External/vector/host procedure calls, volatile values, unsupported expressions | Refused before execution; a database barrier cannot validate external state |

SQL values/results retain ordinary NULL, duplicates, text comparison and numeric
binding semantics. Qualification reuses the existing key binder and unsafe-index
guards. Original Cypher AST determinism is checked before relying on the physical
plan, so optimization cannot erase volatile inputs from completeness checks.
Conservative scopes can abort on writes that would not change a result. Explicit
SQL points/prefixes and separate graph label domains still permit disjoint
canonical writers. Legacy direct graph publication uses a database barrier and
can invalidate otherwise disjoint work.

## Commit and serial-reference argument

[Canonical commit](../../src/api/mod.rs) validates source identity, schema and all
captured read scopes before staging or appending WAL. Concurrent commits enter
through the existing commit sequencer and retain exclusive canonical ownership
through validation, ordinary write validation, durability and publication.
Pessimistic and conflict-noop rebase must pass the same read validator; neither
refreshes a serializable transaction's captured base. No storage layout, WAL
encoding, version key semantics or production group-sync protocol changes.

For a writing transaction in the qualified subset, every canonical input is
covered by a complete version scope. Successful validation means no intervening
publication invalidated those inputs since its base. Its deterministic private
operations can therefore be replayed immediately before its serialized commit,
with the same observations and writes. Own writes remain visible to later
statements. Existing write/constraint validation still establishes valid final
canonical state. Ordering successful writers by publication gives a serial
reference; conservative rejection only removes schedules from the accepted set.
Read-only snapshots may serialize at their snapshot point; this implementation
can conservatively reject a stale read-only commit too.

This argument requires invariant-participating transactions to request the
qualified mode. SI peers retain SI semantics: an SI writer that commits a stale
read last is not upgraded or made globally serializable by another transaction's
option. Unversioned external inputs, multiple writer processes, time travel and
persisted transactions are outside the claim.

A conflict changes no canonical data, commit epoch, WAL bytes or selector.
Diagnostic counters may advance. Pre-durable cancellation aborts without
publication. Once the existing durable boundary succeeds, cancellation cannot
turn that commit into a rollback claim. Group fsync failure poisons unsafe
access; a lost response can leave a complete committed operation.
`SyncOnEveryWrite` remains the default. Relaxed `SyncOnCheckpoint` retains its
explicit unsynchronized power-loss window and stronger branch/head barriers.

## Native refinement and evidence boundaries

[Ordinary facade regressions](../../src/api/tests/concurrent_transactions/serializable.rs)
exercise real parameterized SQL/Cypher, not a detached collector model:

- SI write skew versus optional rejection; all 20 order-preserving two-actor
  read/write/commit interleavings compared with an explicit serial reference.
- Absent points/prefix phantoms, predicate enter/leave, payload and parent
  changes, delete/reinsert ABA, schema invalidation, mixed/earlier statements,
  NULL/duplicate/text/type semantics, unsafe nullable indexes and disjoint progress.
- Sticky failure, savepoint restoration, capture overflow, context cancellation,
  both lock modes, RETURNING/rollback, conflict-noop/pessimistic rebase.
- Eight lock/durability/grouping combinations check pre-WAL rejection and reopen;
  deterministic queue/flush cancellation and fsync faults exercise real commit
  coordination. The only added fault seam is compiled under `cfg(test)`.
- Fourteen child-process interruptions cover whole mixed schema/data transactions
  in grouped/ungrouped and both durability modes. Exit/reopen is process-crash
  evidence, **not power-loss qualification**. Existing platform/storage fault
  obligations and Windows post-merge qualification remain separate.
- Same-workload measurements run each isolation in a fresh child process:
  32 rounds, 64 commit attempts, 64 parameterized point reads per attempt,
  identical schema/writes. Report commit p50/p99, abort count, validation-entry
  bound, estimated capture bytes and process RSS before/with/after captures.
  RSS is coarse whole-process evidence, not exact root retention. No timing
  assertion or performance improvement is claimed; existing fairness/fsync and
  single-stream performance gates remain unchanged.

Exact revision, selected test counts, mutation RED/restored GREEN, timing/resource
numbers, toolchain/features, local fuzz and platform CI belong in the delivery
receipt. A passing helper, zero selected tests, finite schedules or process
reopen alone do not qualify all these capabilities.

## Paper relationship

[Neumann, Muehlbauer and Kemper, SIGMOD 2015, sections 2.3-2.4](https://15721.courses.cs.cmu.edu/spring2016/papers/p677-neumann.pdf)
explain optimistic serializable validation over complete read predicates and
changes, including inserts/deletes, and why version lifetime must protect
validation inputs. HawDB adopts that completeness requirement and commit-time
validation principle. It retains COW snapshots and existing version-domain
summaries rather than importing HyPer's in-place undo-chain representation.
The paper is motivation, not proof of HawDB's durability or this implementation.

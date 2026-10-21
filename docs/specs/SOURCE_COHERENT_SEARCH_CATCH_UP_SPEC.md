# Source-coherent Search catch-up

Tracking: [#1043](https://github.com/nowledge-co/hawdb/issues/1043).
This is an additive embedded-library preparation path based on main, independent
of the stationary Consumer implementation in #1039. It does not enable Mem
Search ACK, replace registered Consumer recovery, or close #1092/#1106.

## Source and publication boundaries

One `ProjectionStagingSession` captures the complete C..E change interval and
one immutable source snapshot E. Its private ordinal can pause inside a commit.
Every visited item, including repeated keys, consumes cumulative admission;
input selection does not clone or repeatedly scan a retained transaction prefix.
Graph and SQL mapping use the same tracked E read context. Mapping failures,
ignored read errors, budget exhaustion and cancellation poison the session.

The mapper is a fixed trusted host function. It must propagate incomplete work,
use the supplied source reads, and represent all relevant graph and relational
effects. Schema/opaque capture gaps require rebuilding rather than skipping
input. Native cold preimage capture runs before WAL append and checkpoint replay
uses that same collector with its cooperative work context.

Mapping and external ordering retain one admitted stage, bounded continuation,
memory ledger and cumulative read/spill/sort counters. Repeated identical E
upserts coalesce; conflicting content refuses. E upserts supersede retractions.
This reduction alone cannot certify host mapping or a historical baseline.

An intermediate B < E is private progress only. `ProjectionStagedInputs` is
opaque and cannot enter the Search publication API. Successful enumeration of
End does not by itself make an arbitrary candidate publishable.

## Complete-E verification

`verify_complete_source` consumes a closed stage and retains its original E
without recapturing a live source. An exhaustive native source cursor and fixed
zero/one-document mapper produce independent expected E documents. The same
sorter admits them and rejects duplicate IDs, including identical duplicates.
This full mapper defines the trusted projection contract; it must not depend on
unversioned external state.

The comparison streams the reduced delta and unique expected documents:

- An expected ID with an upsert must have identical encoded content.
- An expected ID without an upsert must exactly match the held base document.
- A deleted ID must be absent from the expected set. An upsert missing from the
  expected set refuses.
- The expected count must equal the base count plus actual insertions minus
  actual deletions. Unique membership plus cardinality detects untouched extra
  base documents and missing retractions.

Both streams validate their recorded extent and digest. Only the original
compared mutation run is restored for artifact construction. The resulting
`ProjectionCoveredUpdate` privately owns that candidate and its dependencies;
callers cannot exchange a token, candidate or dependency set between sessions.

`Database::publish_projection_covered_update` rechecks the source instance,
dependencies, exact candidate/source E, observed source durability and expected
Search selector under publication exclusion. Rejection preserves the old head.
A configured synchronous policy or recovered WAL is not itself an observed
durability barrier. Explicit synchronization does not weaken future commits.

## Costs and remaining gates

This conservative implementation performs O(full source) extra verification and
records a Database dependency. **Every post-E source commit refuses activation,
including disjoint writes.** Fixed-E reads remain stable while writers continue,
but this fallback does not deliver the design's disjoint-write progress goal.
Do not remove that barrier without a complete source-enumeration/mapping proof.

Input, mapping, sorting and comparison have bounded logical work and resource
admission. Final artifact construction and publication remain blocking; these
limits are not physical I/O, RSS or elapsed-time guarantees. The full-source
mapper currently supports zero or one document per native input, not arbitrary
resumable fanout. Private work is discarded on cancellation/restart and rebuilt
from the last selected base. A committed E head survives a lost caller response;
this does not supply an idempotent registered-Consumer operation receipt.

Hosts must hold their binding authority through activation. Versioned host
binding/incarnation, durable coverage receipts, external vectors, optimized
owner reduction, short activation and actual Mem integration remain open.
The legacy API's #1108 prefix refusal remains intact and is not routed through
this new path automatically. Its earlier mixed-snapshot bug is historical;
current main rejects the incoherent prefix.

The [COW evolution design](MVCC_COW_EVOLUTION_SPEC.md) retains COW values,
ordinary snapshot retirement and persistent Consumer registration as separate
lifetimes. [Neumann et al., SIGMOD 2015](https://15721.courses.cs.cmu.edu/spring2016/papers/p677-neumann.pdf)
informs complete dependency validation and safe reclamation. Its undo-based
value representation and main-memory measurements do not mandate a HawDB layout
change or establish WAL/power-loss safety. Optional serializability is not a
prerequisite for this Search path.

## Verification ownership

The default/minimal `hawdb_mvcc_staging_session` targets include
`tests/support/projection_coverage.rs`: one-item update/delete and ABA,
hot/cold and relational equality with full rebuild, repeated keys/rename,
empty intervals, wrong content, omitted deletes, duplicate IDs, budget refusal,
same-E verification after a write, source/base mismatch and cancellation/reopen.
`hawdb_mvcc_full_source` and `hawdb_mvcc_baseline_comparison` qualify enumeration,
comparison and the independent full-rebuild reference. All belong to root CI.
Storage/Search component tests cover capture gaps, durability barriers and
private preparation. See [the test index](../TEST_INDEX.md) for execution lanes.
Reopen tests are not fault-image or power-loss qualification.

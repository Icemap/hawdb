# Legacy projection hydration must match its watermark

This repairs one source-coherence boundary under #1043. It is not complete MVCC,
stationary Consumer activation, verified mapping/baseline authority or Mem ACK.

## Concrete failure and runtime contract

The public changefeed collector can select a bounded C..B prefix while the live
source is already at E. That result describes changed IDs, not a historical data
view. Hydrating those IDs at E and publishing a B watermark can mix future data
with old untouched documents. A complete batch selected earlier can suffer the
same error if the source advances before hydration. Native before-fix regressions
returned success reports for both invalid operations (zero passing, two failing
checks); their source, executable and logs are retained independently.

Every watermarked graph delta now requires its complete-through epoch to equal
the actual source used for its rows. The live facade borrows one runtime guard
for catalog, store, epoch check and hydration. Both registered and standalone
catch-up wrappers also compare the selected epoch with the pinned read snapshot
before calling a relational/host hydrator. A later live-head change is checked
again when graph rows are built, before mutating the Search index.

`ProjectionSourceEpochMismatch { expected, actual }` preserves a typed refusal.
It is an execution/coherence error, not corruption, resource exhaustion or a
retryable write conflict. Existing error classifiers handle it explicitly; the
external-shadow JSON diagnostic retains its kind and both epochs. Requests
without a watermark keep their existing unversioned-delta behavior and acquire
no completeness proof from this guard.

Bounded ID discovery remains available. A caller that retained a real B read
transaction can call `DatabaseReadTransaction::build_search_projection_graph_delta`
to hydrate B even after the live head advances to E. The request must match both
the published read view and its actual data source. Copying the numeric B label
cannot reconstruct that snapshot. Current-head catch-up instead refuses a
prefix that requires unavailable history. A complete-interval retry or private
C..E staging is needed; simply changing the prefix watermark to E is invalid.

## Bounded model and native correspondence

`HawDBLegacyProjectionEpoch` models a fixed source domain with C baseline, B
update and E update/new document, independently selected ID prefix, retained
snapshot, live-head advancement, host hydration, graph hydration and publication.
The live graph read can observe a head advanced after host hydration. The pinned
mode uses its retained data source. Publication compares complete payload/tag
pairs while allowing a coherent historical B view with live E.

The positive exploration checked 59 generated / 44 distinct states to depth 7.
All four Bazel targets executed and passed. Negative controls were inspected to
require their intended failures:

| Control | Checked counterexample |
| --- | --- |
| SkipHostFence | HostReadsAtSelectedEpoch |
| SkipGraphFence | GraphReadsAtSelectedEpoch |
| ForbidPinnedWitness | NoPinnedWitness: coherent pinned B remains reachable with live E |

The witness establishes reachability, not liveness. Refusal states are permitted;
there is no fairness, historical-snapshot synthesis or background retry proof.

Native regressions cover rejecting a prefix before the hydrator runs, rejecting
an old batch after a source advance, accepting the actual matching pinned B,
and completing a retry whose checkpoint/reopened documents match a full E
rebuild. The relational variant compares reopened documents with an independent
full canonical SQL read. Existing bounded-resume coverage now advances the
source between complete durable batches, rather than blessing a prefix hydrated
from an already newer source. Native results must be taken from terminal runs.

## Remaining obligations

Epoch equality is necessary, not sufficient. This model assumes one source,
correct source-snapshot implementations and the declared graph/host mapping.
It does not prove project/branch/binding identity, target incarnation, complete
mapping coverage, foreign/vector freshness, C..E cumulative budget accounting,
actual heap/I/O limits, native birth/debt recovery or final publication custody.
The stationary view/selector/receipt and complete activation/reopen matrix in
`MVCC_VERSIONED_PUBLICATION_SPEC.md` remain required. Successful legacy
checkpointing does not substitute for that protocol or authorize Mem cutover.

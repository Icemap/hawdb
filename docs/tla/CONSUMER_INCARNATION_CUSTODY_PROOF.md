# Conditional Consumer incarnation and debt ordering model

Status: checked bounded design model. Initial runtime reservations and a separate
native birth operation now have their own refinements in
[allocation bootstrap](CONSUMER_ALLOCATION_BOOTSTRAP_PROOF.md) and
[native birth](CONSUMER_NATIVE_BIRTH_PROOF.md). Stationary Consumer activation and
debt settlement remain unfinished. See the full
[stationary publication contract](../specs/CONSUMER_STATIONARY_PUBLICATION_SPEC.md)
and [original acceptance scope](../specs/MVCC_VERSIONED_PUBLICATION_SPEC.md).

## Source refinement boundary

The source project currently owns `projection_consumers.meta` as a replaceable
verification cache (`crates/search/src/projection_consumer/registry.rs`). The
facade initializes a sibling stage, moves its directory and then publishes the
registry (`src/api/search_projection_consumer/mod.rs`). Those operations cannot
supply the model's durable reservation/birth ledger by renaming their metadata.
`ConsumerProjection` operation custody and actual seven-family producer
references are prerequisites, not this ledger's implementation.

`HawDBConsumerIncarnationCustody` models two fresh installation identities, one
crash, an initialized ledger and possible loss/corruption of that ledger. A
speculative identifier becomes issued only at the reservation barrier or a
native effect. The model remembers issued identities across crash so reuse is
observable even after a failed operation. It over-approximates namespace
persistence: a just-created directory may survive immediately. Unsynchronized
selector replacement may recover either the complete old or complete new head.
A native torn/unknown selector must instead fail closed and retain both outcome
dependencies; that codec/fault refinement is not modeled here.

Before selection, the old root is already retained in retirement/intent custody.
Pending native effects survive caller closure; unknown birth cannot be adopted.
After ledger loss, the whole domain is quarantined rather than reset. The model
has no GC action: retaining known debt forever is safe here and explicitly does
not discharge the real eventual-reclamation acceptance requirement. It checks
one crash; a second crash during recovery adoption is outside the explored
state space.

The single identity counter represents the fresh-install reservation boundary.
Production target incarnations, candidate generations and operation identities
retain distinct typed roles; injective, non-reused native allocation for each
role and their real ledger wiring still require implementation/refinement. The
model assumes actual root authority and actual created-handle birth evidence.
It does not prove those assumptions, artifact completeness, prebuilt-view
semantics, source C..E coverage, platform syscall behavior or power-loss barriers.

## Checked properties and counterexamples

| Property/control | Meaning or observed violation |
| --- | --- |
| `IncarnationsNeverReuse` | No issued identity is reserved again |
| `NativeCustodyRetained` | Every native generation remains in pending, selected, previous or retirement custody, or whole-domain quarantine |
| `AdoptionHasActualBirth` | Recovery adoption requires a recorded actual birth |
| `SelectedClosureComplete` | An installed selection cannot expose incomplete artifacts |
| `DurableSelectorUsable` | Previously durable selection remains usable until properly superseded |
| `AcknowledgedClosureDurable` | Acknowledgement cannot outrun complete durable closure |
| `CreateBeforeReservationBarrier` | Crash loses the pending owner while the directory survives: `NativeCustodyRetained` |
| `ReuseCancelledIncarnation` | Cancellation rolls back the allocator: `IncarnationsNeverReuse` |
| `AdoptUnknownBirth` | Matching pending path is treated as birth authority: `AdoptionHasActualBirth` |
| `ResetMissingLedger` | Recreating a zero ledger loses native owners: `NativeCustodyRetained` |
| `ForgetDebtOnClose` | Caller closure forgets an unselected directory: `NativeCustodyRetained` |
| `RetireBeforeSelectorBarrier` | Early physical retirement destroys the durable old head: `DurableSelectorUsable` |
| `SelectIncompleteClosure` | Selector is exposed before sealing: `SelectedClosureComplete` |

Positive TLC: **1,226 generated / 677 distinct states**, complete graph depth
**20**, no violations. The seven independently enabled controls each violated
the intended named invariant above. All eight registered Bazel test targets
executed and passed. Retained evidence includes exact checked module/config,
TLC arguments, Java/tool hashes, result files and full counterexample traces.

```console
bazel test --jobs=2 --local_test_jobs=1 \
  //docs/tla:HawDBConsumerIncarnationCustody_check \
  //docs/tla:HawDBConsumerIncarnationCustodyCreateBeforeReservationBarrier_check \
  //docs/tla:HawDBConsumerIncarnationCustodyReuseCancelledIncarnation_check \
  //docs/tla:HawDBConsumerIncarnationCustodyAdoptUnknownBirth_check \
  //docs/tla:HawDBConsumerIncarnationCustodyResetMissingLedger_check \
  //docs/tla:HawDBConsumerIncarnationCustodyForgetDebtOnClose_check \
  //docs/tla:HawDBConsumerIncarnationCustodyRetireBeforeSelectorBarrier_check \
  //docs/tla:HawDBConsumerIncarnationCustodySelectIncompleteClosure_check
```

This is finite safety evidence under explicit assumptions. There is no claim of
unbounded induction, complete fault-image refinement, replay implementation,
ABA protection from arbitrary metadata/path copies, physical budget/fairness
qualification, cleanup liveness, native Mem adoption or completed MVCC.

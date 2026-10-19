# Consumer allocation bootstrap and reservation safety

This is a bounded safety argument for initial allocation, not a complete MVCC,
native birth, stationary publication, GC, liveness or Mem cutover proof.

## Runtime refinement

The V2 project/bootstrap identity designates an initialized allocation domain.
`reserve_bootstrap_identity_captured` holds the original source root and catalog
metadata exclusion. A virgin domain has no intent/catalog/branch selector; a
validated legacy graph manifest may still exist. The kernel creates the fixed
ledger exclusively, writes its complete canonical CRC envelope, synchronizes
that file and the original parent, then publishes the source identity intent.
A retained empty orphan ledger wins over fresh proposed UUIDs; retry reopens
that exact file for writing and repeats its file and original-parent barriers. Once initialized,
a missing/corrupt or differently bound ledger fails closed; the registry is
never consulted to select a counter. Completed uncertain outcomes retain their
evidence. This baseline replaces development V1 projects without migration;
authoritative legacy/Cloud stores retain their own compatibility obligations.

`GraphStore::reserve_search_consumer_allocation` obtains the actual source DB
identity internally and borrows its captured project plus original UUID owner.
Under original catalog exclusion it validates the branch owner and ledger
binding, checks pending/byte/counter limits, adds three non-reused typed role
identities and the full binding, seals a private candidate, and replaces the
ledger through the native control-file barrier. Only then may the facade create
a stage or run an initializer. Cancellation, close and registry reconstruction
cannot refund the counter. The receipt retains source custody but does not
claim that any native target directory was born or installed.

Updates read at most 1 MiB plus one byte and serialize through a bounded writer;
at most 256 pending records are retained. There is no settlement yet, so this
also bounds lifetime successful creates until the next implementation stage.
Registry repair cannot bypass that refusal. Rebind/clear/new generation producers
still need integration. No pending record yet grants target GC authority.

## Model and negative controls

`HawDBConsumerAllocationBootstrap.tla` separates empty-ledger sync, source
identity, live/durable reservation, return, effects, lost reply, cancellation,
registry rebuild, missing ledger and crash. Its two operations use counters 3
and 6. The positive bounded exploration visits 45 distinct states (124 generated)
to depth 15. Five independently enabled mutations violate the intended invariant:

| Mutation | Required counterexample |
| --- | --- |
| EarlyIdentity | IdentityAfterEmptyBarrier |
| EarlyEffect | EffectsAfterReserveBarrier |
| RegistryReset | NoRefundOrReset |
| CancelRefund | NoRefundOrReset |
| MissingReset | NoRefundOrReset |

The six Bazel model targets passed, including verification that each negative
control triggers its specified invariant. This is not a native power-loss test.
The model abstracts canonical atomic replacement and ordered synchronization;
platform/storage guarantees and lost/torn/reordered writes still need actual
qualification across ledger, source identity, WAL, target birth and selectors.
Physical directory identity is a live process capability, never a serialized
logical incarnation. Copied UUIDs or saved paths do not reconstruct that owner.

Native regression coverage exercises callback ordering, retained failures,
reopen and registry loss, missing/corrupt evidence, exhaustion, bootstrap lost
reply and descriptor refusal/retry. CI runs the real Consumer allocation cases
on Linux, macOS and Windows and preserves logs even if other stages fail.
A passing stage does not resolve the known Windows legacy directory-publication
failures or establish stationary generation serving behavior.

## Native reservation crash-image refinement

The regression
`consumer_allocation_crash_images_preserve_reservation_barriers_and_nonreuse`
drives the real facade and allocator under the native I/O recorder. It starts
with a checkpointed graph row, reaches the initializer only after the kernel
reservation returns, deliberately fails that initializer, and captures both the
actual in-flight operation and the confirmed reservation. The latter is **not**
a successful Consumer publication acknowledgement.

| Actual recorded cut | TLA boundary | Allowed recovered high-water |
| --- | --- | --- |
| Candidate write, before seal | Propose, before SyncReserve | 0 |
| Candidate rename, before parent barrier | In-flight SyncReserve | 0 or 3 |
| Initializer reached after reservation return | Return, before target activation | 3 |

The snapshots retain completed barriers while independently losing, persisting,
isolating or reversing uncovered operations. Six prefix/suffix/interior fragment
ranges of the candidate write additionally exercise torn bytes in both pending
namespace orders. The test retains all generated physical schedules, including
uncovered source checkpoint cleanup operations; it does not reduce those to
logical JSON mutations. Each image is materialized and reopened by the actual
embedded database, verifies the original graph row, then uses the real kernel
allocator to require the next high-water to be exactly the recovered value plus
three. This avoids repeating unrelated facade checkpoint/Search initialization
for every recovery probe. Both configured graph durability policies are covered;
allocation barriers remain synchronous in both.

These are bounded generated schedules, not exhaustive device behavior. The
recorder assumes trustworthy completed file/directory synchronization and atomic
same-directory replacement. Windows namespace durability remains conditional on
the platform/storage assumptions in `BRANCH_POWER_LOSS_QUALIFICATION.md`. The
harness does not cover interrupted virgin bootstrap, native target birth,
stationary selectors, concurrent view exchange, arbitrary sector corruption or
GC authorization. Their separate qualification remains required. A compiled
negative control must also prove this test rejects returning a reservation after
selection without its parent barrier. Passing results must be recorded from the
actual terminal run rather than inferred from the harness definition.

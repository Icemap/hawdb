# Native Consumer no-effect settlement

This refines the initial native birth primitive with cancellation backed by
process-local evidence that the operation owns no native entry. It does not
resolve known-created or uncertain generations, reconstruct custody after
restart, publish a serving selector, or complete MVCC or Mem activation.

## Runtime correspondence

`PendingConsumerGeneration::settle_no_effect` accepts an unattempted reservation,
an admission refusal before the native call, or a proven exclusive-create
collision. `CapturedDirectoryCandidate::may_own_entry` is the native evidence:
an unknown syscall result remains possible ownership even when the pathname is
absent. A matching complete binding and CRC in a foreign marker does not change
that evidence. Actual child/marker custody also prevents settlement.

Settlement irrevocably revokes creation and closes the original target-parent
descriptor before attempting source publication. The token retains its original
source owner and reservation binding on errors, allowing fresh admission without
reopening a saved target label. Under the original catalog lock, the kernel
checks the source branch and complete reservation, then removes only its Pending
row using a sealed source-ledger candidate. It never decrements the high-water
counter. File and original source-parent barriers precede successful return.

An error can follow a successful native ledger rename. The retained token marks
that publication was attempted before the rename. If a retry finds its row
absent, it requires that process-local attempt evidence and a nondecreasing
high-water mark, then synchronizes the actual selected ledger file and original
source root again. Metadata absence alone cannot authorize acknowledgement.
Successful settlement drops the allocation capability; repeated calls are
idempotent and cannot re-enable generation creation.

The source catalog lock serializes ledger updates, including other reservations
between attempts. The original source identity checks and monotonic allocator
remain necessary assumptions. This operation does not mint a token from decoded
metadata, remove a target entry, or infer rollback from a lost response. Dropping
an unsettled token leaves its durable row for later explicit resolution.

## Bounded model and native evidence

`HawDBConsumerNoEffectSettlement` models one issued operation, native no-effect
versus possible ownership, creation revocation, target descriptor closure, source
selection, file/root barriers, lost responses, fresh retries, close and crash.
Its positive run explores **59 generated / 32 distinct states**, depth **10**.
All six Bazel targets executed successfully; each control requires its intended
violation rather than accepting an arbitrary model-checker error:

| Control | Required violation |
| --- | --- |
| SettleUnknown | MayOwnDebtRooted |
| RefundIdentity | IssuedIdentitiesNeverRefund |
| CloseTargetLate | NoRefundBeforeNativeClose |
| SkipRetryBarrier | AcknowledgedSettlementDurable |
| ReopenRevokedParent | CreationRemainsRevoked |

The finite model abstracts the ledger codec, complete binding checks, native
exclusive creation, catalog exclusion and atomic source replacement. It does not
prove unbounded progress, fairness, physical resource bounds or storage-device
synchronization semantics. The birth model separately checks actual-born debt.

Ten focused native cases passed, including the four birth cases and six
settlement cases: no-effect revocation/nonreuse, a foreign complete matching
marker collision, refusal for actual birth, admission failure/retry, unknown
native creation despite pathname absence, and original-source label replacement.
The native lost-response regression also passed: a scoped test-only callback
exhausts actual descriptor admission after successful ledger rename, forcing a
real post-selection error before the root barrier. A materialized default-loss
image retains the old row before retry. After releasing those descriptors,
retry observes an actual file synchronization; the next materialized image has
no pending row and preserves high-water 3. Successful repeated settlement does
not mutate the ledger again.

The separate native crash-image test covers written candidate, selected and
returned cuts, isolated/all/reversed uncovered persistence, six torn-write
ranges in both orders, both durability policies, graph preservation, retained
counter, no target creation, no registry ACK and next allocation 6. Its terminal
result must be recorded as qualification evidence; a defined test is not a pass.

## Remaining protocol

Only the live no-effect subset can settle. Known-born and uncertain native debt
still require explicit recovery/custody and safe retirement, independent of
registry rebuilding or lease expiry. Legacy Consumer creation still relocates a
stage and is not switched to this path. Target-root lease integration, stationary
immutable serving views, selector and receipt authority, complete C..E producer
closure and Mem full verification remain required. No Mem stable release policy,
default durability, allocation envelope or compatibility baseline changes here.

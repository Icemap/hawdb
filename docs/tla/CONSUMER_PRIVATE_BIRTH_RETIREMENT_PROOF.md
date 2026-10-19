# Retirement of an unexposed private Consumer birth

This cancels a completed native birth that has never granted a producer directory.
It is not GC for serving generations, restart adoption, uncertain-birth cleanup,
or completed MVCC.

## Runtime correspondence

`born_directory` records irreversible exposure before returning any native
reference. A borrowed directory can duplicate custody or create children; ending
the borrow or closing one duplicate cannot prove all descendants have disappeared.
`retire_unpublished` therefore refuses every exposed generation. It accepts only
a completed private birth; the caller still excludes mutation of its private
component. Path equality does not manufacture a target publication lease.

The token revokes creation/producer access before retirement I/O. Through the
original source owner and catalog lock it checks the full reservation and
durably changes Recorded to Retiring. Only then does it close its marker file,
remove the fixed private marker through the original child capture, synchronize
the child, close the child capture, and remove the known-created empty directory
through its original parent and candidate evidence. It never recursively removes
arbitrary children. Empty-directory removal failure retains debt.

Original-parent synchronization makes target removal durable before the source
ledger advances Retiring to Retired. This tombstone still occupies a pending
record. Target-parent custody and marker workspace close before removing that
record. Successful settlement never refunds the identity high-water.

Intermediate errors retain the token, original source binding and remaining
native custody. The native directory candidate records successful removal before
post-effect observation can fail, so retry cannot adopt a replacement directory.
An absent marker is accepted only inside the original private child after
retirement intent, not as evidence that this operation created a directory.

After an uncertain source rename, retry seeing its desired intent/tombstone
repeats actual ledger-file and original source-root barriers before advancing.
Final absent-row settlement similarly requires a live publication-attempt witness
and repeats both barriers. Visible metadata alone cannot acknowledge durability.
Close performs no cleanup; persistent Retiring/Retired records remain debt for
later explicit recovery. This stage adds no recovery constructor.

## Bounded checks

`HawDBConsumerPrivateBirthRetirement` separates exposure, intent selection and
durability, lost intent response/retry, marker/directory removal, target-parent
durability, source tombstone, native closure and settlement. One crash may retain
or lose uncovered namespace operations. The positive run explores **35 generated /
27 distinct states**, depth **10**. All seven Bazel targets executed successfully:

| Control | Required violation |
| --- | --- |
| SkipIntentBarrier | RecordedBirthComplete |
| SkipRemovalBarrier | RetiredMeansDeletionDurable |
| CloseTargetLate | CapacityOnlyAfterNativeClose |
| ExposeAllowed | ProducerAccessNeverRetired |
| RefundIdentity | IssuedIdentitiesNeverRefund |
| ForgetDebtOnClose | RetiredMeansDeletionDurable |

The model assumes source-owner, complete binding, codec and atomic replacement
refinements already qualified separately. Tombstone/final settlement barriers
abstract those control-file transactions; the no-effect settlement model checks
absent-row retry independently. This is finite safety evidence, not liveness,
fairness, physical-resource proof or actual device-loss qualification.

Four focused native cases passed: unused-birth retirement/nonreuse; permanent
refusal after a borrowed producer directory including a retained native duplicate;
actual descriptor refusal with retained token/fresh retry; and original-parent
label replacement leaving the foreign replacement untouched. The separate crash
test covers intent write/selection, marker removal, directory removal, tombstone
selection and returned completion under both durability policies, including
isolated/all/reversed persistence and torn intent writes. Record its terminal
result separately; a defined or running test is not passing evidence.

## Remaining protocol

Exposed births require selector, branch, reader and producer reference closure
before reclamation. Partial/unknown births and restart recovery need admitted
durable custody resolution. Target publication leases, immutable serving views,
selector/receipt publication, complete C..E closure and Mem full verification
remain open. Ledger V2 adds explicit Retiring/Retired states; older readers fail
closed on unknown states. Default durability and Mem stable/GA exclusions remain.

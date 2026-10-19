---------------- MODULE HawDBConsumerNoEffectSettlement ----------------
EXTENDS Naturals, TLC
CONSTANTS SettleUnknown, RefundIdentity, CloseTargetLate, SkipRetryBarrier,
          ReopenRevokedParent
VARIABLES s
vars == <<s>>
Init == s = [phase |-> "prepared", row |-> TRUE, durableRow |-> TRUE,
             highWater |-> 3, mayOwn |-> FALSE, parentHeld |-> TRUE,
             revoked |-> FALSE, publicationAttempted |-> FALSE,
             fileSynced |-> FALSE, parentSynced |-> FALSE, ack |-> FALSE,
             alive |-> TRUE, crashed |-> FALSE, lateCreation |-> FALSE]
NativeAttempt == /\ s.alive /\ s.phase = "prepared" /\ ~s.revoked
                 /\ \E mayOwn \in BOOLEAN :
                       s' = [s EXCEPT !.phase = "native", !.mayOwn = mayOwn]
Begin == /\ s.alive /\ s.phase \in {"prepared", "native"}
         /\ (~s.mayOwn \/ SettleUnknown)
         /\ s' = [s EXCEPT !.phase = "settling", !.revoked = TRUE,
                 !.parentHeld = IF CloseTargetLate THEN @ ELSE FALSE]
AdmissionFailure == /\ s.alive /\ s.phase = "settling"
                    /\ s' = [s EXCEPT !.phase = "retry"]
Select == /\ s.alive /\ s.phase = "settling"
          /\ s' = [s EXCEPT !.phase = "selected", !.row = FALSE,
                  !.publicationAttempted = TRUE,
                  !.highWater = IF RefundIdentity THEN 0 ELSE @]
FileBarrier == /\ s.alive /\ s.phase = "selected"
               /\ s' = [s EXCEPT !.phase = "fileSynced", !.fileSynced = TRUE]
ParentBarrier == /\ s.alive /\ s.phase = "fileSynced"
                 /\ s' = [s EXCEPT !.phase = "synced", !.parentSynced = TRUE,
                                    !.durableRow = FALSE]
Ack == /\ s.alive /\ s.phase = "synced"
       /\ s' = [s EXCEPT !.phase = "settled", !.ack = TRUE]
LostResponse == /\ s.alive /\ s.phase \in {"selected", "fileSynced", "synced"}
                /\ s' = [s EXCEPT !.phase = "retry"]
RetryPresent == /\ s.alive /\ s.phase = "retry" /\ s.row
                /\ s' = [s EXCEPT !.phase = "settling", !.fileSynced = FALSE,
                                   !.parentSynced = FALSE]
RetryAbsent == /\ s.alive /\ s.phase = "retry" /\ ~s.row /\ s.publicationAttempted
               /\ IF SkipRetryBarrier
                  THEN s' = [s EXCEPT !.phase = "settled", !.ack = TRUE]
                  ELSE s' = [s EXCEPT !.phase = "selected", !.fileSynced = FALSE,
                                     !.parentSynced = FALSE]
LateCreate == /\ ReopenRevokedParent /\ s.alive /\ s.revoked
              /\ s.phase \in {"settling", "retry", "settled"}
              /\ s' = [s EXCEPT !.phase = "native", !.mayOwn = TRUE,
                                 !.lateCreation = TRUE, !.parentHeld = TRUE]
Close == /\ s.alive
         /\ s' = [s EXCEPT !.phase = "closed", !.alive = FALSE,
                            !.parentHeld = FALSE, !.revoked = TRUE]
Crash == /\ s.alive /\ ~s.crashed
         /\ \E row \in (IF s.parentSynced THEN {s.durableRow} ELSE {s.durableRow, s.row}) :
               s' = [s EXCEPT !.phase = "crashed", !.alive = FALSE,
                     !.crashed = TRUE, !.parentHeld = FALSE, !.revoked = TRUE,
                     !.row = row, !.durableRow = row]
Next == NativeAttempt \/ Begin \/ AdmissionFailure \/ Select \/ FileBarrier \/
        ParentBarrier \/ Ack \/ LostResponse \/ RetryPresent \/ RetryAbsent \/
        LateCreate \/ Close \/ Crash
TypeOK == /\ s.phase \in {"prepared", "native", "settling", "retry", "selected",
                         "fileSynced", "synced", "settled", "closed", "crashed"}
          /\ s.highWater \in {0, 3}
          /\ \A f \in {"row", "durableRow", "mayOwn", "parentHeld", "revoked",
              "publicationAttempted", "fileSynced", "parentSynced", "ack",
              "alive", "crashed", "lateCreation"} : s[f] \in BOOLEAN
CreationRemainsRevoked == ~s.lateCreation
MayOwnDebtRooted == s.mayOwn => s.row /\ s.durableRow
IssuedIdentitiesNeverRefund == s.highWater = 3
NoRefundBeforeNativeClose == ~s.row => s.revoked /\ ~s.parentHeld
AcknowledgedSettlementDurable == s.ack => ~s.durableRow
Spec == Init /\ [][Next]_vars
=======================================================================

---------------- MODULE HawDBConsumerPrivateBirthRetirement ----------------
EXTENDS Naturals, TLC
CONSTANTS SkipIntentBarrier, SkipRemovalBarrier, CloseTargetLate,
          ExposeAllowed, RefundIdentity, ForgetDebtOnClose
VARIABLE s
vars == <<s>>
Init == s = [phase |-> "born", row |-> "Recorded", durableRow |-> "Recorded",
             marker |-> TRUE, entry |-> TRUE, durableMarker |-> TRUE,
             durableEntry |-> TRUE, parentHeld |-> TRUE, childHeld |-> TRUE,
             markerHeld |-> TRUE, exposed |-> FALSE, highWater |-> 3,
             ack |-> FALSE, alive |-> TRUE]
Expose == /\ s.alive /\ s.phase = "born" /\ ~s.exposed
          /\ s' = [s EXCEPT !.exposed = TRUE]
Begin == /\ s.alive /\ s.phase = "born" /\ (~s.exposed \/ ExposeAllowed)
         /\ s' = [s EXCEPT !.phase = "intentSelected", !.row = "Retiring"]
IntentBarrier == /\ s.alive /\ s.phase = "intentSelected"
                 /\ s' = [s EXCEPT !.phase = "deleting", !.durableRow = "Retiring"]
LostIntentReply == /\ s.alive /\ s.phase = "intentSelected"
                   /\ s' = [s EXCEPT !.phase = "retryIntent"]
RetryIntent == /\ s.alive /\ s.phase = "retryIntent"
               /\ s' = [s EXCEPT !.phase = "intentSelected"]
RemoveMarker == /\ s.alive
                /\ (s.phase = "deleting" \/ (SkipIntentBarrier /\ s.phase = "intentSelected"))
                /\ s' = [s EXCEPT !.phase = "markerRemoved", !.markerHeld = FALSE,
                                    !.marker = FALSE, !.durableMarker = FALSE]
RemoveDirectory == /\ s.alive /\ s.phase = "markerRemoved"
                   /\ s' = [s EXCEPT !.phase = "entryRemoved", !.childHeld = FALSE,
                                       !.entry = FALSE]
RemovalBarrier == /\ s.alive /\ s.phase = "entryRemoved"
                  /\ s' = [s EXCEPT !.phase = "removed", !.durableEntry = FALSE]
Tombstone == /\ s.alive
             /\ (s.phase = "removed" \/ (SkipRemovalBarrier /\ s.phase = "entryRemoved"))
             /\ s' = [s EXCEPT !.phase = "tombstoned", !.row = "Retired",
                                 !.durableRow = "Retired"]
CloseNative == /\ s.alive /\ s.phase = "tombstoned"
               /\ s' = [s EXCEPT !.phase = "closedNative", !.parentHeld = FALSE]
Settle == /\ s.alive
          /\ (s.phase = "closedNative" \/ (CloseTargetLate /\ s.phase = "tombstoned"))
          /\ s' = [s EXCEPT !.phase = "settled", !.row = "Absent",
                    !.durableRow = "Absent", !.ack = TRUE,
                    !.highWater = IF RefundIdentity THEN 0 ELSE @]
Close == /\ s.alive
         /\ s' = [s EXCEPT !.phase = "closed", !.alive = FALSE,
                   !.markerHeld = FALSE, !.childHeld = FALSE, !.parentHeld = FALSE,
                   !.row = IF ForgetDebtOnClose THEN "Absent" ELSE @,
                   !.durableRow = IF ForgetDebtOnClose THEN "Absent" ELSE @]
Crash == /\ s.alive
         /\ \E entry \in {s.entry, s.durableEntry}, marker \in {s.marker, s.durableMarker} :
              s' = [s EXCEPT !.phase = "crashed", !.alive = FALSE,
                    !.row = s.durableRow, !.entry = entry, !.marker = marker,
                    !.durableEntry = entry, !.durableMarker = marker,
                    !.parentHeld = FALSE, !.childHeld = FALSE, !.markerHeld = FALSE]
Next == Expose \/ Begin \/ IntentBarrier \/ LostIntentReply \/ RetryIntent \/
        RemoveMarker \/ RemoveDirectory \/ RemovalBarrier \/ Tombstone \/
        CloseNative \/ Settle \/ Close \/ Crash
TypeOK == /\ s.phase \in {"born", "intentSelected", "deleting", "retryIntent",
                         "markerRemoved", "entryRemoved", "removed", "tombstoned",
                         "closedNative", "settled", "closed", "crashed"}
          /\ s.row \in {"Recorded", "Retiring", "Retired", "Absent"}
          /\ s.durableRow \in {"Recorded", "Retiring", "Retired", "Absent"}
          /\ s.highWater \in {0, 3}
          /\ \A f \in {"marker", "entry", "durableMarker", "durableEntry",
               "parentHeld", "childHeld", "markerHeld", "exposed", "ack", "alive"} : s[f] \in BOOLEAN
RecordedBirthComplete == s.durableRow = "Recorded" => s.durableEntry /\ s.durableMarker
RetiredMeansDeletionDurable == s.durableRow \in {"Retired", "Absent"} => ~s.durableEntry
ProducerAccessNeverRetired == s.exposed => s.row = "Recorded" /\ s.entry /\ s.marker
CapacityOnlyAfterNativeClose == s.row = "Absent" => ~s.parentHeld /\ ~s.childHeld /\ ~s.markerHeld
IssuedIdentitiesNeverRefund == s.highWater = 3
Spec == Init /\ [][Next]_vars
=============================================================================

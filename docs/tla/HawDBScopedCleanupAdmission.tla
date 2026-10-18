---------------- MODULE HawDBScopedCleanupAdmission ----------------
EXTENDS Integers
CONSTANTS FreshLedger, RelabelGovernor, ExportRoot, RefundBeforeFree
VARIABLE s
vars == <<s>>
Budget == 2
Init == \E governed \in BOOLEAN, held \in {0, 2}:
 s = [phase |-> "admit", governed |-> governed, held |-> held,
      reserved |-> 0, live |-> FALSE, admitted |-> FALSE, root |-> FALSE]
Admit == /\ s.phase = "admit"
 /\ LET allowed == (~s.governed \/ RelabelGovernor)
                     /\ ((IF FreshLedger THEN 0 ELSE s.held) + 1 <= Budget)
    IN s' = [s EXCEPT !.phase = IF allowed THEN "allocate" ELSE "refused",
                      !.reserved = IF allowed THEN 1 ELSE 0,
                      !.admitted = allowed]
Allocate == /\ s.phase = "allocate"
            /\ s' = [s EXCEPT !.live = TRUE, !.phase = "validate", !.root = ExportRoot]
Validate == /\ s.phase = "validate"
            /\ s' = [s EXCEPT !.phase = "destroy"]
Destroy == /\ s.phase = "destroy"
 /\ s' = [s EXCEPT !.live = RefundBeforeFree, !.reserved = IF RefundBeforeFree THEN 0 ELSE @,
                   !.phase = "refund"]
Refund == /\ s.phase = "refund"
           /\ s' = [s EXCEPT !.live = FALSE, !.reserved = 0, !.phase = "done"]
Next == Admit \/ Allocate \/ Validate \/ Destroy \/ Refund
TypeOK == /\ s.phase \in {"admit", "allocate", "refused", "validate", "destroy", "refund", "done"}
 /\ s.governed \in BOOLEAN /\ s.held \in {0, 2} /\ s.reserved \in {0, 1}
 /\ s.live \in BOOLEAN /\ s.admitted \in BOOLEAN /\ s.root \in BOOLEAN
OriginalAggregateBudget == s.held + s.reserved <= Budget
NoGovernorRelabelling == s.governed => ~s.admitted
NoPersistentRoot == ~s.root
LivePayloadFunded == s.live => s.reserved = 1
RefusalAllocatesNothing == s.phase = "refused" => ~s.live /\ s.reserved = 0
Spec == Init /\ [][Next]_vars
=================================================================

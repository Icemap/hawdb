------------------ MODULE HawDBCapturedPublicationLease ------------------
EXTENDS Naturals, TLC
CONSTANTS ReopenLabel, SkipBinding, DefaultAccount, EarlyClose, SkipNativeLock
VARIABLE s
vars == <<s>>
Init == s = [phase |-> "captured", label |-> 1, root |-> 1, account |-> 1,
             entry |-> 1, file |-> 0, locked |-> FALSE, held |-> FALSE,
             admitted |-> FALSE]
ReplaceLabel == /\ s.phase \in {"captured", "opened", "locked"} /\ s.label = 1
                /\ s' = [s EXCEPT !.label = 2]
ReplaceEntry == /\ s.phase \in {"opened", "locked"} /\ s.entry = 1
                /\ s' = [s EXCEPT !.entry = 3]
Open == /\ s.phase = "captured"
        /\ LET root == IF ReopenLabel THEN s.label ELSE s.root IN
             s' = [s EXCEPT !.phase = "opened", !.root = root,
                   !.file = IF root = 1 THEN s.entry ELSE 2, !.held = TRUE,
                   !.account = IF DefaultAccount THEN 2 ELSE @]
Lock == /\ s.phase = "opened"
        /\ s' = [s EXCEPT !.phase = "locked", !.locked = ~SkipNativeLock]
Grant == /\ s.phase = "locked"
         /\ LET bound == s.file = (IF s.root = 1 THEN s.entry ELSE 2) IN
              s' = [s EXCEPT !.phase = IF bound \/ SkipBinding THEN "granted" ELSE "refused",
                    !.admitted = bound \/ SkipBinding,
                    !.held = IF EarlyClose THEN FALSE ELSE @]
Close == /\ s.phase \in {"captured", "opened", "locked", "granted", "refused"}
         /\ s' = [s EXCEPT !.phase = "closed", !.admitted = FALSE,
                            !.locked = FALSE, !.held = FALSE]
Next == ReplaceLabel \/ ReplaceEntry \/ Open \/ Lock \/ Grant \/ Close
TypeOK == /\ s.phase \in {"captured", "opened", "locked", "granted", "refused", "closed"}
          /\ s.label \in {1, 2} /\ s.root \in {1, 2} /\ s.account \in {1, 2}
          /\ s.entry \in {1, 3} /\ s.file \in {0, 1, 2, 3}
          /\ s.locked \in BOOLEAN /\ s.held \in BOOLEAN /\ s.admitted \in BOOLEAN
OriginalRoot == s.admitted => s.root = 1
OriginalAccount == s.admitted => s.account = 1
ActualNativeExclusion == s.admitted => s.held /\ s.locked
LockEntryStillBound == s.admitted => s.file = (IF s.root = 1 THEN s.entry ELSE 2)
Spec == Init /\ [][Next]_vars
=============================================================================

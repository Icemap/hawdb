--------------- MODULE HawDBCapturedDirectoryInitialization ---------------
EXTENDS FiniteSets
CONSTANTS PathForCreate, SkipParentBarrier, IgnoreAdmission
VARIABLE s
vars == <<s>>
Names == {"generations", "heads"}
Init == \E existing \in SUBSET Names:
 s = [label |-> "A", phase |-> "generations", a |-> existing, b |-> {},
      durableA |-> existing, denied |-> FALSE, unadmitted |-> FALSE,
      opened |-> FALSE]
ReplaceLabel == /\ s.label = "A"
 /\ s' = [s EXCEPT !.label = "B"]
DenyCapacity == /\ ~s.denied /\ s.phase \in Names
 /\ s' = [s EXCEPT !.denied = TRUE]
Create == /\ s.phase \in Names
 /\ IF s.denied /\ ~IgnoreAdmission
       THEN s' = [s EXCEPT !.phase = "failed"]
       ELSE s' = [s EXCEPT
          !.a = IF ~PathForCreate \/ s.label = "A" THEN @ \cup {s.phase} ELSE @,
          !.b = IF PathForCreate /\ s.label = "B" THEN @ \cup {s.phase} ELSE @,
          !.unadmitted = @ \/ s.denied,
          !.phase = IF s.phase = "generations" THEN "heads" ELSE "barrier"]
Barrier == /\ s.phase = "barrier"
 /\ s' = [s EXCEPT !.durableA = IF SkipParentBarrier THEN @ ELSE s.a,
                    !.phase = "open"]
Open == /\ s.phase = "open"
 /\ s' = [s EXCEPT !.opened = (Names \subseteq s.a),
                    !.phase = IF Names \subseteq s.a THEN "done" ELSE "failed"]
Next == ReplaceLabel \/ DenyCapacity \/ Create \/ Barrier \/ Open
TypeOK == /\ s.label \in {"A", "B"}
 /\ s.phase \in Names \cup {"barrier", "open", "done", "failed"}
 /\ s.a \subseteq Names /\ s.b \subseteq Names /\ s.durableA \subseteq Names
 /\ \A b \in {s.denied, s.unadmitted, s.opened}: b \in BOOLEAN
ForeignNamespaceUntouched == s.b = {}
CreationAdmitted == ~s.unadmitted
CapturedCatalogComplete == s.opened => Names \subseteq s.a
OpenedDependenciesDurable == s.opened => Names \subseteq s.durableA
Spec == Init /\ [][Next]_vars
=============================================================================

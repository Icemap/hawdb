---------------- MODULE HawDBCapturedRetention ----------------
EXTENDS Integers, FiniteSets
CONSTANTS PathDiscovery, TrustWarmCache, PartialClosure, IgnoreBudget, IgnoreChild
VARIABLE s
vars == <<s>>
Roots == {1, 2}
Dependencies == {"payload", "mutation"}
Artifacts == {0, 1}
Good == [d \in Dependencies |-> 0]
Init == s = [phase |-> "verify", namespace |-> 1, bytes |-> Good,
             budget |-> 2, child |-> TRUE, authorized |-> FALSE,
             discovery |-> 1, removed |-> [r \in Roots |-> {}]]
ReplaceNamespace == /\ s.namespace = 1
                    /\ s' = [s EXCEPT !.namespace = 2]
\* A cooperating publisher holds the selected head immutable from verification
\* through unlink. Corruption/replacement/missing dependencies are explored
\* before verification; arbitrary external writes after it are outside scope.
ChangeDependency == /\ s.phase = "verify"
                    /\ \E d \in Dependencies, value \in {1, 2}:
                         /\ s.bytes[d] = 0
                         /\ s' = [s EXCEPT !.bytes[d] = value]
DenyBudget == /\ s.phase = "verify" /\ s.budget = 2
              /\ s' = [s EXCEPT !.budget = 1]
DropChild == /\ s.child /\ s' = [s EXCEPT !.child = FALSE]
Verify == /\ s.phase = "verify"
          /\ LET source == IF PathDiscovery THEN s.namespace ELSE 1
                 actual == IF source = 2 THEN Good ELSE s.bytes
                 observed == IF TrustWarmCache THEN Good ELSE actual
                 checked == IF PartialClosure THEN {"payload"} ELSE Dependencies
                 accepted == (IgnoreBudget \/ s.budget >= Cardinality(Dependencies))
                              /\ \A d \in checked: observed[d] = 0
             IN s' = [s EXCEPT !.discovery = source,
                       !.authorized = accepted,
                       !.phase = IF accepted THEN "sweep" ELSE "refused"]
Sweep == /\ s.phase = "sweep"
         /\ LET obsolete == IF s.discovery = 1 THEN {0} ELSE {1}
                pinned == IF s.child /\ ~IgnoreChild THEN {0} ELSE {}
            IN s' = [s EXCEPT !.removed[1] = obsolete \ pinned,
                              !.phase = "done"]
Next == ReplaceNamespace \/ ChangeDependency \/ DenyBudget \/ DropChild \/ Verify \/ Sweep
TypeOK == /\ s.phase \in {"verify", "sweep", "refused", "done"}
          /\ s.namespace \in Roots /\ s.discovery \in Roots
          /\ s.bytes \in [Dependencies -> {0, 1, 2}]
          /\ s.budget \in {1, 2} /\ s.child \in BOOLEAN
          /\ s.authorized \in BOOLEAN /\ s.removed \in [Roots -> SUBSET Artifacts]
CompleteFreshClosure == s.authorized => s.bytes = Good
BudgetBeforeUnlink == s.authorized => s.budget = 2
DiscoveryUsesCapturedRoot == s.authorized => s.discovery = 1
SelectedClosureLive == 1 \notin s.removed[1]
IndependentChildLive == s.child => 0 \notin s.removed[1]
ForeignRootUntouched == s.removed[2] = {}
RefusalDoesNotDelete == s.phase = "refused" => s.removed = [r \in Roots |-> {}]
Spec == Init /\ [][Next]_vars
=================================================================

---------------- MODULE HawDBPrivateDirectoryCreation ----------------
EXTENDS Integers
CONSTANTS NoScopeCheck, RecordAfterOpen, ForgetOnOpenRefusal, AdoptUnknown
VARIABLE s
vars == <<s>>
\* One retained caller-owned candidate, bound to capture a0. Capture a1
\* aliases the same physical parent; b0 belongs to a different parent.
\* This model assumes the parent/evidence stay retained and the private name
\* is not rebound. It does not prove a producer ledger or persistent identity.
Init == \E caller \in {"a0", "a1", "b0"}, outcome \in {"ok", "unknown", "collision"},
           preflightDenied \in BOOLEAN, openDenied \in BOOLEAN:
 s = [phase |-> "create", caller |-> caller, outcome |-> outcome,
      preflightDenied |-> preflightDenied, openDenied |-> openDenied,
      record |-> "fresh", nativeAttempted |-> FALSE, effect |-> FALSE,
      seenOpen |-> FALSE, knownAtOpen |-> FALSE]
Create == /\ s.phase = "create"
 /\ IF s.preflightDenied \/ (s.caller # "a0" /\ ~NoScopeCheck)
    THEN s' = [s EXCEPT !.phase = "done"]
    ELSE IF s.outcome = "collision"
    THEN s' = [s EXCEPT !.phase = "done", !.nativeAttempted = TRUE, !.record = "refused"]
    ELSE IF s.outcome = "unknown"
    THEN \E effect \in BOOLEAN:
      s' = [s EXCEPT !.phase = "open", !.nativeAttempted = TRUE,
                    !.record = "attempted", !.effect = effect]
    ELSE s' = [s EXCEPT !.phase = "open", !.nativeAttempted = TRUE, !.effect = TRUE,
                       !.record = IF RecordAfterOpen THEN "fresh" ELSE "created"]
Open == /\ s.phase = "open"
 /\ LET allowed == s.record = "created" \/ (AdoptUnknown /\ s.record = "attempted")
    IN s' = [s EXCEPT !.phase = "done", !.seenOpen = allowed /\ ~s.openDenied,
      !.knownAtOpen = s.record = "created",
      !.record = IF ForgetOnOpenRefusal /\ s.openDenied /\ s.record = "created"
                 THEN "fresh" ELSE @]
Next == Create \/ Open
TypeOK == /\ s.phase \in {"create", "open", "done"}
 /\ s.caller \in {"a0", "a1", "b0"} /\ s.outcome \in {"ok", "unknown", "collision"}
 /\ s.record \in {"fresh", "attempted", "refused", "created"}
 /\ s.preflightDenied \in BOOLEAN /\ s.openDenied \in BOOLEAN
 /\ s.nativeAttempted \in BOOLEAN /\ s.effect \in BOOLEAN
 /\ s.seenOpen \in BOOLEAN /\ s.knownAtOpen \in BOOLEAN
NativeScopeMatches == s.nativeAttempted => s.caller = "a0"
PossibleEffectRetained == s.effect => s.record \in {"attempted", "created"}
KnownCreationRequiredForOpen == s.seenOpen => s.knownAtOpen
CollisionNeverAdopted == s.nativeAttempted /\ s.outcome = "collision"
                        => s.record = "refused" /\ ~s.seenOpen
Spec == Init /\ [][Next]_vars
=================================================================

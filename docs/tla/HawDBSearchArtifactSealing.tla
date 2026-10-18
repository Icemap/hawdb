------------------ MODULE HawDBSearchArtifactSealing ------------------
EXTENDS FiniteSets
CONSTANTS SkipDigestCheck, SealInstalledInstead, SelectBeforeVerify
VARIABLE s
vars == <<s>>
Artifacts == {"document", "metadata", "vector", "lexical"}
Original == [a \in Artifacts |-> 0]
Init == s = [phase |-> "seal", expected |-> Original, installed |-> Original,
             selected |-> FALSE, corrupted |-> FALSE]
Seal == /\ s.phase = "seal"
        /\ s' = [s EXCEPT !.expected = Original, !.phase = "install"]
Install == /\ s.phase = "install"
           /\ s' = [s EXCEPT !.installed = Original, !.phase = "verify"]
Corrupt == /\ s.phase = "verify" /\ ~s.corrupted
           /\ \E a \in Artifacts:
                 s' = [s EXCEPT !.installed[a] = 1, !.corrupted = TRUE]
Verify == /\ s.phase = "verify"
          /\ LET expected == IF SealInstalledInstead THEN s.installed ELSE s.expected
                 accepted == SkipDigestCheck \/ s.installed = expected
             IN s' = [s EXCEPT !.phase = IF accepted THEN "select" ELSE "failed",
                                !.selected = SelectBeforeVerify]
Select == /\ s.phase = "select"
          /\ s' = [s EXCEPT !.selected = TRUE, !.phase = "done"]
Cancel == /\ s.phase \in {"seal", "install", "verify", "select"}
          /\ s' = [s EXCEPT !.phase = "failed"]
Next == Seal \/ Install \/ Corrupt \/ Verify \/ Select \/ Cancel
TypeOK == /\ s.phase \in {"seal", "install", "verify", "select", "failed", "done"}
          /\ s.expected \in [Artifacts -> {0, 1}]
          /\ s.installed \in [Artifacts -> {0, 1}]
          /\ s.selected \in BOOLEAN /\ s.corrupted \in BOOLEAN
SelectedBytesMatchProducer == s.selected => s.installed = Original
Spec == Init /\ [][Next]_vars
======================================================================

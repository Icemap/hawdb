---------------- MODULE HawDBResidentVectorAbsence ----------------
EXTENDS Integers
CONSTANTS MissingExpectedAsAbsent, ExpectedFromInstalled, TrustDeclaredCount
VARIABLE s
vars == <<s>>
\* -1: no resident snapshot; 0/1: producer's zero/nonzero-vector snapshot.
Init == \E original \in {-1, 0, 1}:
 s = [phase |-> "verify", original |-> original, native |-> original,
      nativeReference |-> 0, receipt |-> original # -1, absenceProof |-> original = -1,
      declared |-> IF original = -1 THEN 0 ELSE original,
      namespace |-> 1, authorized |-> FALSE, removed |-> FALSE]
ReplaceNamespace == /\ s.namespace = 1 /\ s' = [s EXCEPT !.namespace = 2]
LoseSnapshot == /\ s.phase = "verify" /\ s.native # -1
                /\ s' = [s EXCEPT !.native = -1]
ReplaceWithValidForeignSnapshot == /\ s.phase = "verify" /\ s.native # -1 /\ s.nativeReference = 0
                                  /\ s' = [s EXCEPT !.native = 0, !.nativeReference = 1]
DropReceipt == /\ s.phase = "verify" /\ s.receipt
               /\ s' = [s EXCEPT !.receipt = FALSE]
ForgeZeroCount == /\ s.phase = "verify" /\ s.declared = 1
                  /\ s' = [s EXCEPT !.declared = 0]
Verify == /\ s.phase = "verify"
 /\ LET actual == s.native
        absent == actual = -1 /\ (s.absenceProof \/ MissingExpectedAsAbsent) /\ ~s.receipt
        expected == IF ExpectedFromInstalled THEN s.nativeReference ELSE 0
        count == IF TrustDeclaredCount THEN s.declared ELSE actual
        verified == actual # -1 /\ s.receipt /\ s.nativeReference = expected
                    /\ count = s.declared /\ count = 0
    IN s' = [s EXCEPT !.authorized = absent \/ verified,
                      !.phase = IF absent \/ verified THEN "sweep" ELSE "refused"]
Sweep == /\ s.phase = "sweep" /\ s' = [s EXCEPT !.removed = TRUE, !.phase = "done"]
Next == ReplaceNamespace \/ LoseSnapshot \/ ReplaceWithValidForeignSnapshot
        \/ DropReceipt \/ ForgeZeroCount \/ Verify \/ Sweep
TypeOK == /\ s.phase \in {"verify", "sweep", "refused", "done"}
 /\ s.original \in {-1, 0, 1} /\ s.native \in {-1, 0, 1} /\ s.nativeReference \in {0, 1}
 /\ s.receipt \in BOOLEAN /\ s.absenceProof \in BOOLEAN /\ s.declared \in {0, 1}
 /\ s.namespace \in {1, 2} /\ s.authorized \in BOOLEAN /\ s.removed \in BOOLEAN
AbsenceIsSourceBound == s.authorized =>
 (s.native = -1 /\ s.absenceProof /\ ~s.receipt) \/
 (s.receipt /\ s.nativeReference = 0 /\ s.native = s.original /\ s.native = 0 /\ s.declared = 0)
OnlyProvenAbsenceDeletes == s.removed => s.original # 1
RefusalPreservesEvidence == s.phase = "refused" => ~s.removed
Spec == Init /\ [][Next]_vars
=================================================================

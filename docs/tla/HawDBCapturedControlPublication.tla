---------------- MODULE HawDBCapturedControlPublication ----------------
EXTENDS Integers
CONSTANTS PathForCreate, PathForReplace, AllowLateWrites, RetryFailed,
 AckEarly, SkipPostFileSync
VARIABLE s
vars == <<s>>
Init == s=[phase |-> "empty", namespace |-> 1, version |-> 0, frozen |-> -1,
 foreignWritten |-> FALSE, postSynced |-> FALSE, dirSynced |-> FALSE,
 failed |-> FALSE, ack |-> FALSE]
Relabel == /\ s.namespace=1 /\ s'=[s EXCEPT !.namespace=2]
Create == /\ s.phase="empty"
 /\ s'=[s EXCEPT !.phase="writable",
         !.foreignWritten=PathForCreate /\ s.namespace=2]
Write == /\ s.version<2
 /\ (s.phase="writable" \/ (AllowLateWrites /\ s.frozen>=0))
 /\ s'=[s EXCEPT !.version=@+1]
Seal == /\ s.phase="writable"
 /\ s'=[s EXCEPT !.phase="sealed", !.frozen=s.version]
Begin == /\ s.phase="sealed" /\ s'=[s EXCEPT !.phase="attempted"]
Effect == /\ s.phase="attempted"
 /\ s'=[s EXCEPT !.phase="effect",
         !.foreignWritten=@ \/ (PathForReplace /\ s.namespace=2)]
FileSync == /\ s.phase="effect"
 /\ s'=[s EXCEPT !.phase="synced", !.postSynced=TRUE]
Verify == /\ s.phase="synced" \/ (SkipPostFileSync /\ s.phase="effect")
 /\ s'=[s EXCEPT !.phase="verified"]
DirectorySync == /\ s.phase="verified"
 /\ s'=[s EXCEPT !.phase="complete", !.dirSynced=TRUE]
Respond == /\ s.phase="complete" \/ (AckEarly /\ s.phase="effect")
 /\ s'=[s EXCEPT !.phase="done", !.ack=TRUE]
Fail == /\ s.phase \in {"attempted","effect","synced","verified"}
 /\ s'=[s EXCEPT !.phase="failed", !.failed=TRUE]
Retry == /\ RetryFailed /\ s.phase="failed"
 /\ s'=[s EXCEPT !.phase="attempted"]
Next == Relabel \/ Create \/ Write \/ Seal \/ Begin \/ Effect \/ FileSync
 \/ Verify \/ DirectorySync \/ Respond \/ Fail \/ Retry
TypeOK == /\ s.phase \in {"empty","writable","sealed","attempted","effect",
                          "synced","verified","complete","done","failed"}
 /\ s.namespace \in {1,2} /\ s.version \in 0..2 /\ s.frozen \in -1..2
 /\ \A k \in {"foreignWritten","postSynced","dirSynced","failed","ack"}: s[k] \in BOOLEAN
NoForeignMutation == ~s.foreignWritten
SealedBytesFrozen == s.frozen>=0 => s.version=s.frozen
FailureRevoked == s.failed => s.phase="failed"
CompleteBarriers == s.phase="complete" => s.postSynced /\ s.dirSynced
AcknowledgementOrdered == s.ack => s.postSynced /\ s.dirSynced
EventuallyResolved == <>(s.phase \in {"done","failed"})
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
=============================================================================

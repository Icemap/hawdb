---------------- MODULE HawDBPreparedCatalogPublication ----------------
EXTENDS Integers, FiniteSets
CONSTANTS EarlyRelease, ResolveLive, AckEarly, IgnoreGcRoots, RetryAttempt
VARIABLE s
vars == <<s>>
Occupied == s.phase \in {"prepared", "attempted", "uncertain"}
Init == s = [phase |-> "idle", head |-> 1, durable |-> 1,
 live |-> {1,2,3}, owner |-> FALSE, attempted |-> FALSE,
 overlap |-> FALSE, foreignResolved |-> FALSE, retried |-> FALSE,
 ack |-> FALSE, ackDurable |-> FALSE, nextDone |-> FALSE]
Prepare == /\ s.phase="idle"
 /\ s'=[s EXCEPT !.phase="prepared", !.owner=TRUE]
Select == /\ s.phase="prepared"
 /\ \E effect \in BOOLEAN:
    s'=[s EXCEPT !.phase="attempted", !.attempted=TRUE,
                !.head=IF effect THEN 2 ELSE @]
Retry == /\ RetryAttempt /\ s.attempted /\ s.owner
 /\ s'=[s EXCEPT !.retried=TRUE]
DropOwner == /\ s.owner /\ Occupied
 /\ s'=[s EXCEPT !.owner=FALSE,
                !.phase=IF s.phase="prepared" THEN "closed" ELSE "uncertain"]
Finish == /\ s.phase="attempted" /\ s.owner /\ s.head=2
 /\ s'=[s EXCEPT !.durable=2, !.phase="closed", !.owner=FALSE,
                !.ack=TRUE, !.ackDurable=TRUE]
EarlyAck == /\ AckEarly /\ s.phase="attempted" /\ s.head=2
 /\ s'=[s EXCEPT !.ack=TRUE, !.ackDurable=(s.durable=2)]
ResolveOwn == /\ s.phase="attempted" /\ s.owner
 /\ s'=[s EXCEPT !.phase="closed", !.owner=FALSE,
                !.durable=s.head, !.ack=(s.head=2), !.ackDurable=(s.head=2)]
ResolveOther == /\ (s.phase="uncertain" \/ (ResolveLive /\ s.phase="attempted"))
 /\ s'=[s EXCEPT !.phase="closed", !.durable=s.head,
                !.foreignResolved=@ \/ s.owner]
NextPublication == /\ ~s.nextDone /\ (s.phase="closed" \/ (EarlyRelease /\ Occupied))
 /\ s'=[s EXCEPT !.head=3, !.durable=3, !.nextDone=TRUE,
                !.overlap=@ \/ Occupied]
Gc == \E g \in {1,2}:
 /\ s.phase#"idle"
 /\ g \in s.live /\ g#s.head /\ g#s.durable
 /\ (~Occupied \/ IgnoreGcRoots \/ g=3)
 /\ s'=[s EXCEPT !.live=@\{g}]
Next == Prepare \/ Select \/ Retry \/ DropOwner \/ Finish \/ EarlyAck
 \/ ResolveOwn \/ ResolveOther \/ NextPublication \/ Gc
TypeOK == /\ s.phase \in {"idle","prepared","attempted","uncertain","closed"}
 /\ s.head \in 1..3 /\ s.durable \in 1..3 /\ s.live \subseteq {1,2,3}
 /\ \A b \in {s.owner,s.attempted,s.overlap,s.foreignResolved,s.retried,
                s.ack,s.ackDurable,s.nextDone}: b \in BOOLEAN
ExclusiveOwner == ~s.overlap
NoForeignResolution == ~s.foreignResolved
NoRetry == ~s.retried
DurableReceipt == s.ack => s.ackDurable
ActiveComplete == s.head \in s.live
DurableComplete == s.durable \in s.live
RecoveryRoots == Occupied => {1,2} \subseteq s.live
Spec == Init /\ [][Next]_vars
=============================================================================

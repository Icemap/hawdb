---------------- MODULE HawDBCapturedProjectionCatalog ----------------
EXTENDS Integers
CONSTANTS PathForBuild, PathForHead, PathForRead, PathForGc, SplitPin, KeyByPath,
 SkipGcHeadBarrier
VARIABLE s
vars == <<s>>
Roots == 1..2
Generations == 1..2
Init == s = [namespace |-> 1,
 head |-> [r \in Roots |-> 1], durableHead |-> 1,
 live |-> [r \in Roots |-> [g \in Generations |-> r=2 \/ g=1]],
 candidate |-> "absent", reader |-> "idle", chosen |-> 0,
 pin |-> 0, readRoot |-> 0, foreignWritten |-> FALSE, tokenTried |-> FALSE]
Move == /\ s.namespace=1 /\ s'=[s EXCEPT !.namespace=2]
Build == /\ s.candidate="absent"
 /\ LET r == IF PathForBuild THEN s.namespace ELSE 1 IN
    s'=[s EXCEPT !.live[r][2]=TRUE, !.candidate="private",
                 !.foreignWritten=@ \/ r#1]
Seal == /\ s.candidate="private" /\ s'=[s EXCEPT !.candidate="sealed"]
Publish == /\ s.candidate="sealed" /\ s.head[1]=1
 /\ LET r == IF PathForHead THEN s.namespace ELSE 1 IN
    s'=[s EXCEPT !.head[r]=2, !.candidate="published",
                 !.foreignWritten=@ \/ r#1]
DropCandidate == /\ s.candidate \in {"private","sealed","published"}
 /\ s'=[s EXCEPT !.candidate="released"]
Capture == /\ s.reader="idle"
 /\ s'=[s EXCEPT !.chosen=s.head[1],
                 !.pin=IF SplitPin THEN 0 ELSE s.head[1],
                 !.reader=IF SplitPin THEN "captured" ELSE "pinned"]
Register == /\ s.reader="captured"
 /\ s'=[s EXCEPT !.pin=s.chosen, !.reader="pinned"]
Read == /\ s.reader="pinned"
 /\ s'=[s EXCEPT !.readRoot=IF PathForRead THEN s.namespace ELSE 1,
                 !.reader="read"]
DropReader == /\ s.reader \in {"pinned","read"}
 /\ s'=[s EXCEPT !.pin=0, !.reader="finished"]
Gc == \E g \in Generations:
 /\ (s.durableHead=s.head[1] \/ SkipGcHeadBarrier)
 /\ LET h == IF PathForGc THEN s.namespace ELSE 1 IN g#s.head[h]
 /\ g#s.pin /\ s.live[1][g]
 /\ ~(g=2 /\ s.candidate \in {"private","sealed","published"})
 /\ s'=[s EXCEPT !.live[1][g]=FALSE]
HeadBarrier == /\ s.durableHead#s.head[1]
 /\ s'=[s EXCEPT !.durableHead=s.head[1]]
ForeignToken == /\ ~s.tokenTried
 /\ s'=[s EXCEPT !.tokenTried=TRUE,
                 !.foreignWritten=@ \/ KeyByPath]
Next == Move \/ Build \/ Seal \/ Publish \/ DropCandidate \/ Capture
 \/ Register \/ Read \/ DropReader \/ Gc \/ HeadBarrier \/ ForeignToken
TypeOK == /\ s.namespace \in Roots /\ s.head \in [Roots -> Generations]
 /\ s.durableHead \in Generations
 /\ s.live \in [Roots -> [Generations -> BOOLEAN]]
 /\ s.candidate \in {"absent","private","sealed","published","released"}
 /\ s.reader \in {"idle","captured","pinned","read","finished"}
 /\ s.chosen \in 0..2 /\ s.pin \in 0..2 /\ s.readRoot \in 0..2
 /\ s.foreignWritten \in BOOLEAN /\ s.tokenTried \in BOOLEAN
NoForeignMutation == ~s.foreignWritten
ActiveComplete == s.live[1][s.head[1]]
DurableActiveComplete == s.live[1][s.durableHead]
CandidateComplete == s.candidate \in {"private","sealed","published"} => s.live[1][2]
PinnedComplete == s.pin#0 => s.live[1][s.pin]
ReaderFromSource == s.readRoot \in {0,1}
ReadComplete == s.reader="read" => s.live[s.readRoot][s.chosen]
Spec == Init /\ [][Next]_vars
=============================================================================

---------------- MODULE HawDBStationaryConsumerPublication ----------------
EXTENDS Integers
CONSTANTS UseDirectoryMove, ReleaseCustody, SkipDataSync, AckBeforeBarrier
VARIABLES phase, dataSynced, headSynced, childHeld
vars == <<phase, dataSynced, headSynced, childHeld>>

\* Availability refinement for one already-qualified candidate, not the
\* C/E, receipt, filesystem or power-loss correctness model. The native
\* environment refuses moving a directory with held descendants. Replacing
\* its small head file remains executable without releasing those handles.
Init == /\ phase="private" /\ dataSynced=FALSE /\ headSynced=FALSE /\ childHeld=TRUE
Install == /\ phase="private" /\ phase'="installed"
 /\ UNCHANGED <<dataSynced,headSynced,childHeld>>
SyncData == /\ phase="installed" /\ ~dataSynced /\ dataSynced'=TRUE
 /\ UNCHANGED <<phase,headSynced,childHeld>>
Unlock == /\ ReleaseCustody /\ phase="installed" /\ childHeld
 /\ childHeld'=FALSE /\ UNCHANGED <<phase,dataSynced,headSynced>>
Select == /\ phase="installed" /\ (dataSynced \/ SkipDataSync)
 /\ (~UseDirectoryMove \/ ~childHeld) /\ phase'="selected"
 /\ UNCHANGED <<dataSynced,headSynced,childHeld>>
SyncHead == /\ phase="selected" /\ ~headSynced /\ headSynced'=TRUE
 /\ UNCHANGED <<phase,dataSynced,childHeld>>
Respond == /\ phase="selected" /\ (headSynced \/ AckBeforeBarrier)
 /\ phase'="done" /\ UNCHANGED <<dataSynced,headSynced,childHeld>>
Next == Install \/ SyncData \/ Unlock \/ Select \/ SyncHead \/ Respond
TypeOK == /\ phase \in {"private","installed","selected","done"}
 /\ dataSynced \in BOOLEAN /\ headSynced \in BOOLEAN /\ childHeld \in BOOLEAN
CustodyUnbroken == childHeld
DataBeforeSelection == phase \in {"selected","done"} => dataSynced
AcknowledgementDurable == phase="done" => headSynced
PublicationEventuallyReturns == <>(phase="done")
\* Weak fairness excludes endlessly stuttering while a required local step
\* remains enabled. It cannot invent the disabled directory-move operation.
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
=============================================================================

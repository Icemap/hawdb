---------------- MODULE HawDBCapturedFileCustody ----------------
EXTENDS Integers
CONSTANTS DropRootOnParent, UnlockBeforeCacheClose, RefundBeforeDirectoryClose
VARIABLES parent, logical, reading, cache, phase, process, directory, memory
vars == <<parent, logical, reading, cache, phase, process, directory, memory>>

\* One captured root, one independently owned logical source and one in-flight
\* read. Idle cache keys own memory but never a logical source or root pin.
Init == /\ parent=TRUE /\ logical=FALSE /\ reading=FALSE /\ cache=FALSE
 /\ phase="ready" /\ process=TRUE /\ directory=TRUE /\ memory=TRUE
OpenLogical == /\ parent /\ phase="ready" /\ ~logical
 /\ logical'=TRUE
 /\ UNCHANGED <<parent,reading,cache,phase,process,directory,memory>>
BeginRead == /\ logical /\ ~reading /\ phase="ready"
 /\ reading'=TRUE /\ cache'=TRUE
 /\ UNCHANGED <<parent,logical,phase,process,directory,memory>>
DropParent == /\ parent /\ parent'=FALSE
 /\ phase'=(IF DropRootOnParent \/ (~logical /\ ~reading) THEN "retiring" ELSE phase)
 /\ UNCHANGED <<logical,reading,cache,process,directory,memory>>
DropLogical == /\ logical /\ logical'=FALSE
 /\ phase'=(IF ~parent /\ ~reading THEN "retiring" ELSE phase)
 /\ UNCHANGED <<parent,reading,cache,process,directory,memory>>
EndRead == /\ reading /\ reading'=FALSE
 /\ phase'=(IF ~parent /\ ~logical THEN "retiring" ELSE phase)
 /\ UNCHANGED <<parent,logical,cache,process,directory,memory>>
EvictIdle == /\ cache /\ ~reading /\ cache'=FALSE
 /\ UNCHANGED <<parent,logical,reading,phase,process,directory,memory>>
CloseProcess == /\ phase="retiring" /\ process
 /\ (~cache \/ UnlockBeforeCacheClose) /\ process'=FALSE
 /\ UNCHANGED <<parent,logical,reading,cache,phase,directory,memory>>
CloseDirectory == /\ phase="retiring" /\ ~process /\ directory
 /\ directory'=FALSE
 /\ UNCHANGED <<parent,logical,reading,cache,phase,process,memory>>
RefundMemory == /\ phase="retiring" /\ memory
 /\ (~directory \/ RefundBeforeDirectoryClose) /\ memory'=FALSE
 /\ UNCHANGED <<parent,logical,reading,cache,phase,process,directory>>
Next == OpenLogical \/ BeginRead \/ DropParent \/ DropLogical \/ EndRead
 \/ EvictIdle \/ CloseProcess \/ CloseDirectory \/ RefundMemory
TypeOK == /\ parent \in BOOLEAN /\ logical \in BOOLEAN /\ reading \in BOOLEAN
 /\ cache \in BOOLEAN /\ phase \in {"ready","retiring"}
 /\ process \in BOOLEAN /\ directory \in BOOLEAN /\ memory \in BOOLEAN
LogicalSourceOwnsCustody == (logical \/ reading) => (process /\ directory /\ memory)
NativeChildOwnsCustody == cache => process
DirectoryCapacityOwned == directory => memory
Spec == Init /\ [][Next]_vars
=============================================================================

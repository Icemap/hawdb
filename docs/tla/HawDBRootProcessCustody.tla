---------------- MODULE HawDBRootProcessCustody ----------------
EXTENDS Integers, FiniteSets
CONSTANTS UnregisterBeforeNativeClose, DropRootOnProducerClose,
 ReleaseSlotDuringMutation, IgnoreReadingOwner, RetainProducerInChild,
 ProcessLocalOnly
VARIABLES registry, rootId, used, handles, closing, kernel,
 producerOpen, slotBusy, mutation, child, reading, producerGeneration
vars == <<registry,rootId,used,handles,closing,kernel,
 producerOpen,slotBusy,mutation,child,reading,producerGeneration>>
Ids == {1,2}
NeedsRoot == producerOpen \/ mutation \/ child \/ reading
RetirementNeedsRoot == producerOpen \/ mutation \/ child
 \/ (~IgnoreReadingOwner /\ reading)
Init == /\ registry="empty" /\ rootId=1 /\ used={} /\ handles={}
 /\ closing={} /\ kernel="none" /\ producerOpen=FALSE /\ slotBusy=FALSE
 /\ mutation=FALSE /\ child=FALSE /\ reading=FALSE /\ producerGeneration=0
BeginInit(id) == /\ registry="empty" /\ id \in Ids \ used
 /\ registry'="initializing" /\ rootId'=id /\ used'=used \cup {id}
 /\ UNCHANGED <<handles,closing,kernel,producerOpen,slotBusy,
 mutation,child,reading,producerGeneration>>
OpenNative == /\ registry="initializing" /\ rootId \notin handles /\ kernel#"peer"
 /\ handles'=handles \cup {rootId}
 /\ kernel'=(IF ProcessLocalOnly THEN kernel ELSE "local")
 /\ UNCHANGED <<registry,rootId,used,closing,producerOpen,slotBusy,
 mutation,child,reading,producerGeneration>>
FinishInit == /\ registry="initializing" /\ rootId \in handles
 /\ (ProcessLocalOnly \/ kernel="local")
 /\ registry'="ready" /\ producerOpen'=TRUE /\ slotBusy'=TRUE
 /\ producerGeneration'=1
 /\ UNCHANGED <<rootId,used,handles,closing,kernel,mutation,child,reading>>
FailInit == /\ registry="initializing" /\ registry'="retiring"
 /\ closing'=closing \cup {rootId}
 /\ UNCHANGED <<rootId,used,handles,kernel,producerOpen,slotBusy,
 mutation,child,reading,producerGeneration>>
OpenChild == /\ registry="ready" /\ producerOpen /\ ~child /\ child'=TRUE
 /\ UNCHANGED <<registry,rootId,used,handles,closing,kernel,
 producerOpen,slotBusy,mutation,reading,producerGeneration>>
BeginRead == /\ child /\ ~reading /\ reading'=TRUE
 /\ UNCHANGED <<registry,rootId,used,handles,closing,kernel,
 producerOpen,slotBusy,mutation,child,producerGeneration>>
DropChild == /\ child /\ child'=FALSE
 /\ slotBusy'=(producerOpen \/ mutation)
 /\ UNCHANGED <<registry,rootId,used,handles,closing,kernel,
 producerOpen,mutation,reading,producerGeneration>>
EndRead == /\ reading /\ reading'=FALSE
 /\ UNCHANGED <<registry,rootId,used,handles,closing,kernel,
 producerOpen,slotBusy,mutation,child,producerGeneration>>
BeginMutation == /\ producerOpen /\ slotBusy /\ ~mutation /\ mutation'=TRUE
 /\ UNCHANGED <<registry,rootId,used,handles,closing,kernel,
 producerOpen,slotBusy,child,reading,producerGeneration>>
CloseProducer == /\ producerOpen /\ producerOpen'=FALSE
 /\ slotBusy'=((~ReleaseSlotDuringMutation /\ mutation)
              \/ (RetainProducerInChild /\ child))
 /\ handles'=(IF DropRootOnProducerClose /\ (child \/ reading)
              THEN handles \ {rootId} ELSE handles)
 /\ kernel'=(IF DropRootOnProducerClose /\ (child \/ reading)
              /\ kernel="local" THEN "none" ELSE kernel)
 /\ UNCHANGED <<registry,rootId,used,closing,mutation,child,
 reading,producerGeneration>>
EndMutation == /\ mutation /\ mutation'=FALSE
 /\ slotBusy'=(producerOpen \/ (RetainProducerInChild /\ child))
 /\ UNCHANGED <<registry,rootId,used,handles,closing,kernel,
 producerOpen,child,reading,producerGeneration>>
OpenSuccessor == /\ registry="ready" /\ ~producerOpen /\ ~slotBusy
 /\ rootId \in handles /\ kernel="local" /\ producerGeneration=1
 /\ producerOpen'=TRUE /\ slotBusy'=TRUE /\ producerGeneration'=2
 /\ UNCHANGED <<registry,rootId,used,handles,closing,kernel,
 mutation,child,reading>>
BeginRetirement == /\ registry="ready" /\ ~RetirementNeedsRoot
 /\ registry'="retiring" /\ closing'=closing \cup {rootId}
 /\ UNCHANGED <<rootId,used,handles,kernel,producerOpen,slotBusy,
 mutation,child,reading,producerGeneration>>
CloseRetired(id) == /\ id \in closing
 /\ handles'=handles \ {id} /\ closing'=closing \ {id}
 /\ kernel'=(IF id \in handles /\ kernel="local" THEN "none" ELSE kernel)
 /\ UNCHANGED <<registry,rootId,used,producerOpen,slotBusy,
 mutation,child,reading,producerGeneration>>
Unregister == /\ registry="retiring"
 /\ (UnregisterBeforeNativeClose \/ rootId \notin handles)
 /\ registry'="empty"
 /\ UNCHANGED <<rootId,used,handles,closing,kernel,producerOpen,slotBusy,
 mutation,child,reading,producerGeneration>>
PeerAcquire == /\ kernel="none" /\ kernel'="peer"
 /\ UNCHANGED <<registry,rootId,used,handles,closing,producerOpen,slotBusy,
 mutation,child,reading,producerGeneration>>
PeerRelease == /\ kernel="peer" /\ kernel'="none"
 /\ UNCHANGED <<registry,rootId,used,handles,closing,producerOpen,slotBusy,
 mutation,child,reading,producerGeneration>>
Next == (\E id \in Ids: BeginInit(id) \/ CloseRetired(id))
 \/ OpenNative \/ FinishInit \/ FailInit \/ OpenChild \/ BeginRead
 \/ DropChild \/ EndRead \/ BeginMutation \/ CloseProducer \/ EndMutation
 \/ OpenSuccessor \/ BeginRetirement \/ Unregister \/ PeerAcquire \/ PeerRelease
TypeOK == /\ registry \in {"empty","initializing","ready","retiring"}
 /\ rootId \in Ids /\ used \subseteq Ids /\ handles \subseteq used
 /\ closing \subseteq used /\ kernel \in {"none","local","peer"}
 /\ producerOpen \in BOOLEAN /\ slotBusy \in BOOLEAN
 /\ mutation \in BOOLEAN /\ child \in BOOLEAN /\ reading \in BOOLEAN
 /\ producerGeneration \in 0..2
NativeCustody == NeedsRoot => (rootId \in handles /\ kernel="local")
MutationOwnsProducerSlot == mutation => slotBusy
ChildDoesNotOwnProducer == ~producerOpen /\ ~mutation => ~slotBusy
NoSuccessorWithRetainedChild == ~(producerOpen /\ child /\ producerGeneration=2)
Spec == Init /\ [][Next]_vars
=============================================================================

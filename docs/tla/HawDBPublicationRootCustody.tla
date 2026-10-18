---------------- MODULE HawDBPublicationRootCustody ----------------
EXTENDS Integers, FiniteSets
CONSTANTS DuplicateProcessHandle, IgnorePublisherDuringInit, UnregisterEarly,
 ReopenOnPromotion, CloseWithoutUnlock
VARIABLES root, reader, publisher, registered, mode, handles, kernel, inherited
vars == <<root, reader, publisher, registered, mode, handles, kernel, inherited>>

Init == /\ root = "empty" /\ reader = FALSE
 /\ publisher = "idle" /\ registered = FALSE /\ mode = "none"
 /\ handles = {} /\ kernel = "none" /\ inherited = FALSE

BeginRoot == /\ root = "empty"
 /\ (~registered \/ IgnorePublisherDuringInit) /\ root' = "initializing"
 /\ UNCHANGED <<reader,publisher,registered,mode,handles,kernel,inherited>>
OpenRoot == /\ root = "initializing" /\ "root" \notin handles
 /\ kernel # "peer" /\ handles' = handles \cup {"root"} /\ kernel' = "local"
 /\ UNCHANGED <<root,reader,publisher,registered,mode,inherited>>
ReadyRoot == /\ root = "initializing" /\ "root" \in handles
 /\ root' = "ready" /\ reader' = TRUE
 /\ UNCHANGED <<publisher,registered,mode,handles,kernel,inherited>>
FailRoot == /\ root = "initializing" /\ root' = "retiring"
 /\ UNCHANGED <<reader,publisher,registered,mode,handles,kernel,inherited>>
DropReader == /\ root = "ready" /\ reader /\ reader' = FALSE
 /\ root' = (IF mode = "borrowed" THEN root ELSE "retiring")
 /\ UNCHANGED <<publisher,registered,mode,handles,kernel,inherited>>
CloseRoot == /\ root = "retiring" /\ "root" \in handles
 /\ handles' = handles \ {"root"}
 /\ kernel' = (IF CloseWithoutUnlock /\ inherited THEN kernel ELSE "none")
 /\ UNCHANGED <<root,reader,publisher,registered,mode,inherited>>
UnregisterRoot == /\ root = "retiring" /\ "root" \notin handles
 /\ root' = "empty"
 /\ UNCHANGED <<reader,publisher,registered,mode,handles,kernel,inherited>>

RegisterPublisher == /\ publisher = "idle" /\ root \in {"empty","ready"}
 /\ registered' = TRUE /\ publisher' = "registered"
 /\ mode' = (IF root = "ready" THEN "borrowed" ELSE "native")
 /\ UNCHANGED <<root,reader,handles,kernel,inherited>>
OpenPublisher == /\ publisher = "registered"
 /\ (mode = "borrowed" \/ kernel # "peer")
 /\ publisher' = "held"
 /\ handles' = (IF mode = "native" \/ DuplicateProcessHandle
                THEN handles \cup {"publisher"} ELSE handles)
 /\ kernel' = "local"
 /\ UNCHANGED <<root,reader,registered,mode,inherited>>
\* Atomic ownership transfer under the registry gate; the physical handle
\* is unchanged. The external reader and publisher then share root custody.
PromotePublisher == /\ root = "empty" /\ publisher = "held"
 /\ registered /\ mode = "native" /\ "publisher" \in handles
 /\ kernel = "local" /\ root' = "ready" /\ reader' = TRUE
 /\ mode' = "borrowed"
 /\ handles' = (handles \ {"publisher"}) \cup {"root"}
 /\ kernel' = (IF ReopenOnPromotion THEN "none" ELSE kernel)
 /\ UNCHANGED <<publisher,registered,inherited>>
BeginPublisherClose == /\ publisher \in {"registered","held"}
 /\ publisher' = "closing"
 /\ registered' = (IF UnregisterEarly THEN FALSE ELSE registered)
 /\ UNCHANGED <<root,reader,mode,handles,kernel,inherited>>
ClosePublisherCustody == /\ publisher = "closing"
 /\ publisher' = "closed" /\ mode' = "none"
 /\ root' = (IF root = "ready" /\ ~reader /\ mode = "borrowed"
            THEN "retiring" ELSE root)
 \* Conservative F_SETLK semantics: closing any handle to the process-lock
 \* inode releases this process's lock, including a duplicate handle.
 /\ kernel' = (IF "publisher" \in handles
                THEN IF CloseWithoutUnlock /\ inherited THEN kernel ELSE "none"
                ELSE kernel)
 /\ handles' = handles \ {"publisher"}
 /\ UNCHANGED <<reader,registered,inherited>>
UnregisterPublisher == /\ publisher = "closed"
 /\ publisher' = "idle" /\ registered' = FALSE
 /\ UNCHANGED <<root,reader,mode,handles,kernel,inherited>>
PeerAcquire == /\ kernel = "none" /\ kernel' = "peer"
 /\ UNCHANGED <<root,reader,publisher,registered,mode,handles,inherited>>
PeerRelease == /\ kernel = "peer" /\ kernel' = "none"
 /\ UNCHANGED <<root,reader,publisher,registered,mode,handles,inherited>>
\* A fork inherits an open description until exec/exit. CLOEXEC does not
\* release an inherited description during that interval. Ownership must
\* explicitly unlock before closing its final local descriptor.
ForkDescription == /\ kernel = "local" /\ handles # {} /\ ~inherited
 /\ inherited' = TRUE
 /\ UNCHANGED <<root,reader,publisher,registered,mode,handles,kernel>>
CloseInherited == /\ inherited /\ inherited' = FALSE
 /\ kernel' = (IF handles = {} /\ kernel = "local" THEN "none" ELSE kernel)
 /\ UNCHANGED <<root,reader,publisher,registered,mode,handles>>
Next == BeginRoot \/ OpenRoot \/ ReadyRoot \/ FailRoot \/ DropReader
 \/ CloseRoot \/ UnregisterRoot \/ RegisterPublisher \/ OpenPublisher
 \/ PromotePublisher \/ BeginPublisherClose \/ ClosePublisherCustody \/ UnregisterPublisher
 \/ PeerAcquire \/ PeerRelease \/ ForkDescription \/ CloseInherited

TypeOK == /\ root \in {"empty","initializing","ready","retiring"}
 /\ reader \in BOOLEAN /\ registered \in BOOLEAN
 /\ publisher \in {"idle","registered","held","closing","closed"}
 /\ mode \in {"none","borrowed","native"}
 /\ handles \subseteq {"root","publisher"}
 /\ kernel \in {"none","local","peer"} /\ inherited \in BOOLEAN
ReadyRootOwnsNativeLock == root = "ready" => ("root" \in handles /\ kernel = "local")
BorrowedPublisherKeepsRoot == mode = "borrowed" => root = "ready"
HeldPublisherOwnsNativeLock == publisher = "held" => kernel = "local"
NoUnownedLocalLock == handles = {} => kernel # "local"
Spec == Init /\ [][Next]_vars
=============================================================================

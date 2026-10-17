---------------- MODULE HawDBPublicationRootCustody ----------------
EXTENDS Integers, FiniteSets
CONSTANTS DuplicateProcessHandle, IgnorePublisherDuringInit, UnregisterEarly,
 ReopenOnPromotion
VARIABLES root, reader, publisher, registered, mode, handles, kernel
vars == <<root, reader, publisher, registered, mode, handles, kernel>>

Init == /\ root = "empty" /\ reader = FALSE
 /\ publisher = "idle" /\ registered = FALSE /\ mode = "none"
 /\ handles = {} /\ kernel = "none"

BeginRoot == /\ root = "empty"
 /\ (~registered \/ IgnorePublisherDuringInit) /\ root' = "initializing"
 /\ UNCHANGED <<reader,publisher,registered,mode,handles,kernel>>
OpenRoot == /\ root = "initializing" /\ "root" \notin handles
 /\ kernel # "peer" /\ handles' = handles \cup {"root"} /\ kernel' = "local"
 /\ UNCHANGED <<root,reader,publisher,registered,mode>>
ReadyRoot == /\ root = "initializing" /\ "root" \in handles
 /\ root' = "ready" /\ reader' = TRUE
 /\ UNCHANGED <<publisher,registered,mode,handles,kernel>>
FailRoot == /\ root = "initializing" /\ root' = "retiring"
 /\ UNCHANGED <<reader,publisher,registered,mode,handles,kernel>>
DropReader == /\ root = "ready" /\ reader /\ reader' = FALSE
 /\ root' = (IF mode = "borrowed" THEN root ELSE "retiring")
 /\ UNCHANGED <<publisher,registered,mode,handles,kernel>>
CloseRoot == /\ root = "retiring" /\ "root" \in handles
 /\ handles' = handles \ {"root"} /\ kernel' = "none"
 /\ UNCHANGED <<root,reader,publisher,registered,mode>>
UnregisterRoot == /\ root = "retiring" /\ "root" \notin handles
 /\ root' = "empty"
 /\ UNCHANGED <<reader,publisher,registered,mode,handles,kernel>>

RegisterPublisher == /\ publisher = "idle" /\ root \in {"empty","ready"}
 /\ registered' = TRUE /\ publisher' = "registered"
 /\ mode' = (IF root = "ready" THEN "borrowed" ELSE "native")
 /\ UNCHANGED <<root,reader,handles,kernel>>
OpenPublisher == /\ publisher = "registered"
 /\ (mode = "borrowed" \/ kernel # "peer")
 /\ publisher' = "held"
 /\ handles' = (IF mode = "native" \/ DuplicateProcessHandle
                THEN handles \cup {"publisher"} ELSE handles)
 /\ kernel' = "local"
 /\ UNCHANGED <<root,reader,registered,mode>>
\* Atomic ownership transfer under the registry gate; the physical handle
\* is unchanged. The external reader and publisher then share root custody.
PromotePublisher == /\ root = "empty" /\ publisher = "held"
 /\ registered /\ mode = "native" /\ "publisher" \in handles
 /\ kernel = "local" /\ root' = "ready" /\ reader' = TRUE
 /\ mode' = "borrowed"
 /\ handles' = (handles \ {"publisher"}) \cup {"root"}
 /\ kernel' = (IF ReopenOnPromotion THEN "none" ELSE kernel)
 /\ UNCHANGED <<publisher,registered>>
BeginPublisherClose == /\ publisher \in {"registered","held"}
 /\ publisher' = "closing"
 /\ registered' = (IF UnregisterEarly THEN FALSE ELSE registered)
 /\ UNCHANGED <<root,reader,mode,handles,kernel>>
ClosePublisherCustody == /\ publisher = "closing"
 /\ publisher' = "closed" /\ mode' = "none"
 /\ root' = (IF root = "ready" /\ ~reader /\ mode = "borrowed"
            THEN "retiring" ELSE root)
 \* Conservative F_SETLK semantics: closing any handle to the process-lock
 \* inode releases this process's lock, including a duplicate handle.
 /\ kernel' = (IF "publisher" \in handles THEN "none" ELSE kernel)
 /\ handles' = handles \ {"publisher"}
 /\ UNCHANGED <<reader,registered>>
UnregisterPublisher == /\ publisher = "closed"
 /\ publisher' = "idle" /\ registered' = FALSE
 /\ UNCHANGED <<root,reader,mode,handles,kernel>>
PeerAcquire == /\ kernel = "none" /\ kernel' = "peer"
 /\ UNCHANGED <<root,reader,publisher,registered,mode,handles>>
PeerRelease == /\ kernel = "peer" /\ kernel' = "none"
 /\ UNCHANGED <<root,reader,publisher,registered,mode,handles>>
Next == BeginRoot \/ OpenRoot \/ ReadyRoot \/ FailRoot \/ DropReader
 \/ CloseRoot \/ UnregisterRoot \/ RegisterPublisher \/ OpenPublisher
 \/ PromotePublisher \/ BeginPublisherClose \/ ClosePublisherCustody \/ UnregisterPublisher
 \/ PeerAcquire \/ PeerRelease

TypeOK == /\ root \in {"empty","initializing","ready","retiring"}
 /\ reader \in BOOLEAN /\ registered \in BOOLEAN
 /\ publisher \in {"idle","registered","held","closing","closed"}
 /\ mode \in {"none","borrowed","native"}
 /\ handles \subseteq {"root","publisher"}
 /\ kernel \in {"none","local","peer"}
ReadyRootOwnsNativeLock == root = "ready" => ("root" \in handles /\ kernel = "local")
BorrowedPublisherKeepsRoot == mode = "borrowed" => root = "ready"
HeldPublisherOwnsNativeLock == publisher = "held" => kernel = "local"
Spec == Init /\ [][Next]_vars
=============================================================================

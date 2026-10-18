---------------- MODULE HawDBConsumerDirectoryRelocation ----------------
EXTENDS Integers
CONSTANTS KeepOldPath, TrustFailedPublish, ReleaseNativeEarly, DeleteUncertain,
 ReplaceDestination, ReleaseBeforeSourceSync, SyncSourceFirst
(* Process-local custody and abstract successful namespace barriers only.
   The source is already checkpointed and the native exclusive rename is atomic
   for running processes. This does not prove physical cross-parent crash
   atomicity, source-path incarnation, reader reload or filesystem guarantees. *)
VARIABLE s
vars == <<s>>
Init == s = [phase |-> "ready", path |-> "source", renamed |-> FALSE,
 valid |-> TRUE, native |-> TRUE, present |-> TRUE, oldReused |-> FALSE,
 badUse |-> FALSE, wroteForeign |-> FALSE, destination |-> "absent",
 clobbered |-> FALSE, destinationSynced |-> FALSE, sourceSynced |-> FALSE]
OccupyTarget == /\ s.phase \in {"ready","moving"} /\ s.destination="absent"
 /\ s'=[s EXCEPT !.destination="foreign"]
BeginMove == /\ s.phase="ready"
 /\ s'=[s EXCEPT !.phase="moving", !.valid=FALSE]
Rename == /\ s.phase="moving"
 /\ (s.destination="absent" \/ ReplaceDestination)
 /\ s'=[s EXCEPT !.phase="renamed", !.renamed=TRUE,
     !.path=IF KeepOldPath THEN "source" ELSE "destination",
     !.clobbered=(s.destination="foreign"), !.destination="ours"]
FailedBeforeRename == /\ s.phase="moving"
 /\ s'=[s EXCEPT !.phase="uncertain", !.valid=TrustFailedPublish,
     !.native=~ReleaseNativeEarly]
\* A lost outcome can be located at the destination, or remain unknown when
\* identity observation also fails. Neither permits mutation or cleanup.
PostRenameFailure(known) == /\ s.phase \in {"renamed","destination-synced","parents-synced"}
 /\ s'=[s EXCEPT !.phase="uncertain", !.valid=TrustFailedPublish,
     !.native=~ReleaseNativeEarly,
     !.path=IF KeepOldPath THEN "source" ELSE IF known THEN "destination" ELSE "unknown"]
SyncDestination == /\ s.phase="renamed" /\ ~s.destinationSynced
 /\ s'=[s EXCEPT !.destinationSynced=TRUE, !.phase="destination-synced"]
SyncSource == /\ ~s.sourceSynced
 /\ (s.phase="destination-synced" \/ (SyncSourceFirst /\ s.phase="renamed"))
 /\ s'=[s EXCEPT !.sourceSynced=TRUE,
     !.phase=IF s.destinationSynced THEN "parents-synced" ELSE "source-synced"]
SyncSuccess == /\ s.phase="parents-synced"
 /\ s'=[s EXCEPT !.phase="published", !.valid=TRUE, !.path="destination"]
ReleaseEarly == /\ ReleaseBeforeSourceSync /\ s.phase="destination-synced"
 /\ s'=[s EXCEPT !.phase="published", !.valid=TRUE, !.path="destination"]
ReuseSource == /\ s.renamed /\ ~s.oldReused
 /\ s'=[s EXCEPT !.oldReused=TRUE]
OwnerUse == /\ s.valid /\ s.phase \in {"ready","published","uncertain"}
 /\ s'=[s EXCEPT !.badUse=@ \/ s.phase="uncertain",
     !.wroteForeign=@ \/ (s.path="source" /\ s.oldReused)]
Cleanup == /\ s.phase="uncertain" /\ DeleteUncertain
 /\ s'=[s EXCEPT !.present=FALSE]
Close == /\ s.phase \in {"ready","published","uncertain"}
 /\ s'=[s EXCEPT !.phase="closed", !.valid=FALSE, !.native=FALSE]
Next == OccupyTarget \/ BeginMove \/ Rename \/ FailedBeforeRename
 \/ (\E known \in BOOLEAN: PostRenameFailure(known))
 \/ SyncDestination \/ SyncSource \/ SyncSuccess \/ ReleaseEarly
 \/ ReuseSource \/ OwnerUse \/ Cleanup \/ Close
TypeOK == /\ s.phase \in {"ready","moving","renamed","destination-synced","source-synced","parents-synced","published","uncertain","closed"}
 /\ s.path \in {"source","destination","unknown"}
 /\ s.destination \in {"absent","foreign","ours"}
 /\ \A key \in {"renamed","valid","native","present","oldReused","badUse",
     "wroteForeign","clobbered","destinationSynced","sourceSynced"}:s[key] \in BOOLEAN
PathFollowsRename == s.phase="uncertain" /\ s.renamed => s.path \in {"destination","unknown"}
FailedOwnerRevoked == ~s.badUse
NoForeignMutation == ~s.wroteForeign
NativeRetained == s.phase#"closed" => s.native
UncertainRetainsEvidence == s.renamed => s.present
NoTargetClobber == ~s.clobbered
PublishedBothParents == s.phase="published" => (s.destinationSynced /\ s.sourceSynced)
SourceSyncFollowsDestination == s.sourceSynced => s.destinationSynced
=============================================================================

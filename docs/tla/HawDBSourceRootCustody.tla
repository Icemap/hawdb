---------------- MODULE HawDBSourceRootCustody ----------------
EXTENDS Integers, FiniteSets
CONSTANTS DropOnParent, ReleaseDuringRead, IgnoreOncePins, IgnoreRetryPins,
 PathForUnlink, PathForRead, RetainBusy, PinWholeManifest, UnlockBeforePin, IgnorePendingCapture, PathForOnceDiscovery, PathForRetryDiscovery
VARIABLES head, phase, owners, pins, reading, readRoot, available,
 namespace, busy, pending, unlinkRoot, capturePending, discoveryRoot
vars == <<head, phase, owners, pins, reading, readRoot, available,
 namespace, busy, pending, unlinkRoot, capturePending, discoveryRoot>>
Roots == {1, 2}
Artifacts == {0, 1, 2}
OwnerNames == {"parent", "child", "read"}
Init == /\ head=1 /\ phase="unopened" /\ owners={} /\ pins={}
 /\ reading=FALSE /\ readRoot=1 /\ available=[r \in Roots |-> Artifacts]
 /\ namespace=1 /\ busy="idle" /\ pending={} /\ unlinkRoot=1 /\ capturePending=FALSE /\ discoveryRoot=1
OpenBegin == /\ phase="unopened" /\ head=1 /\ busy="idle"
 /\ phase'="opening" /\ busy'=(IF UnlockBeforePin THEN "idle" ELSE "reader")
 /\ UNCHANGED <<head,owners,pins,reading,readRoot,available,namespace,pending,unlinkRoot,capturePending,discoveryRoot>>
InstallPending == /\ phase="opening" /\ busy="reader" /\ ~capturePending
 /\ capturePending'=TRUE /\ busy'="idle"
 /\ UNCHANGED <<head,phase,owners,pins,reading,readRoot,available,namespace,pending,unlinkRoot,discoveryRoot>>
InstallPin == /\ phase="opening" /\ (capturePending \/ UnlockBeforePin)
 /\ phase'="ready" /\ owners'={"parent"}
 /\ pins'=(IF PinWholeManifest THEN {0,1} ELSE {0})
 /\ capturePending'=FALSE
 /\ busy'=(IF RetainBusy THEN "reader" ELSE busy)
 /\ UNCHANGED <<head,reading,readRoot,available,namespace,pending,unlinkRoot,discoveryRoot>>
AbortCapture == /\ phase="opening" /\ phase'="closed" /\ capturePending'=FALSE
 /\ busy'=(IF busy="reader" THEN "idle" ELSE busy)
 /\ UNCHANGED <<head,owners,pins,reading,readRoot,available,namespace,pending,unlinkRoot,discoveryRoot>>
CloneChild == /\ phase="ready" /\ "parent" \in owners /\ "child" \notin owners
 /\ owners'=owners \cup {"child"}
 /\ UNCHANGED <<head,phase,pins,reading,readRoot,available,namespace,busy,pending,unlinkRoot,capturePending,discoveryRoot>>
DropParent == /\ "parent" \in owners
 /\ owners'=owners \ {"parent"}
 /\ pins'=(IF DropOnParent \/ owners'={} THEN {} ELSE pins)
 /\ UNCHANGED <<head,phase,reading,readRoot,available,namespace,busy,pending,unlinkRoot,capturePending,discoveryRoot>>
BeginRead == /\ "child" \in owners /\ ~reading
 /\ owners'=owners \cup {"read"} /\ reading'=TRUE
 /\ readRoot'=(IF PathForRead THEN namespace ELSE 1)
 /\ UNCHANGED <<head,phase,pins,available,namespace,busy,pending,unlinkRoot,capturePending,discoveryRoot>>
DropChild == /\ "child" \in owners
 /\ owners'=owners \ {"child"}
 /\ pins'=(IF owners'={} \/ (ReleaseDuringRead /\ reading) THEN {} ELSE pins)
 /\ UNCHANGED <<head,phase,reading,readRoot,available,namespace,busy,pending,unlinkRoot,capturePending,discoveryRoot>>
EndRead == /\ reading /\ owners'=owners \ {"read"} /\ reading'=FALSE
 /\ pins'=(IF owners'={} THEN {} ELSE pins)
 /\ UNCHANGED <<head,phase,readRoot,available,namespace,busy,pending,unlinkRoot,capturePending,discoveryRoot>>
Publish == /\ busy="idle" /\ head=1 /\ head'=2
 /\ UNCHANGED <<phase,owners,pins,reading,readRoot,available,namespace,busy,pending,unlinkRoot,capturePending,discoveryRoot>>
ReplaceNamespace == /\ namespace=1 /\ namespace'=2
 /\ UNCHANGED <<head,phase,owners,pins,reading,readRoot,available,busy,pending,unlinkRoot,capturePending,discoveryRoot>>
\* Artifact 2 is the captured root's active closure after publication. A
\* replacement directory has an independent newer head which makes that same
\* numeric generation obsolete. Reading that head cannot authorize deleting 2
\* in the captured root, even if the final unlink is correctly relative.
ObsoleteAt(source) == IF source=1 THEN {0} ELSE {2}
SelectCleanup(kind) == /\ busy="idle" /\ head=2 /\ kind \in {"once","retry"}
 /\ busy'="cleanup"
 /\ discoveryRoot'=(IF (kind="once" /\ PathForOnceDiscovery)
                        \/ (kind="retry" /\ PathForRetryDiscovery)
                      THEN namespace ELSE 1)
 /\ pending'=(IF capturePending /\ ~IgnorePendingCapture THEN {} ELSE
              IF (kind="once" /\ IgnoreOncePins) \/ (kind="retry" /\ IgnoreRetryPins)
              THEN ObsoleteAt(discoveryRoot') \cap available[1]
              ELSE (ObsoleteAt(discoveryRoot') \cap available[1]) \ pins)
 /\ UNCHANGED <<head,phase,owners,pins,reading,readRoot,available,namespace,unlinkRoot,capturePending>>
Unlink == /\ busy="cleanup"
 /\ unlinkRoot'=(IF PathForUnlink THEN namespace ELSE 1)
 /\ available'=[available EXCEPT ![unlinkRoot']=@ \ pending]
 /\ pending'={} /\ busy'="idle"
 /\ UNCHANGED <<head,phase,owners,pins,reading,readRoot,namespace,capturePending,discoveryRoot>>
Next == OpenBegin \/ InstallPending \/ InstallPin \/ AbortCapture \/ CloneChild \/ DropParent \/ BeginRead \/ DropChild \/ EndRead
 \/ Publish \/ ReplaceNamespace \/ (\E kind \in {"once","retry"}:SelectCleanup(kind)) \/ Unlink
TypeOK == /\ head \in {1,2} /\ phase \in {"unopened","opening","ready","closed"}
 /\ owners \subseteq OwnerNames /\ pins \subseteq {0,1}
 /\ reading \in BOOLEAN /\ (reading <=> "read" \in owners)
 /\ readRoot \in Roots /\ available \in [Roots -> SUBSET Artifacts]
 /\ namespace \in Roots /\ busy \in {"idle","reader","cleanup"}
 /\ pending \subseteq {0,2} /\ unlinkRoot \in Roots /\ capturePending \in BOOLEAN
 /\ discoveryRoot \in Roots
PublishedClosureLive == IF head=1 THEN 0 \in available[1] ELSE 2 \in available[1]
SourcesLive == (owners#{} \/ phase="opening") => 0 \in available[1]
ForeignRootUntouched == available[2]=Artifacts
ReadUsesCapturedRoot == reading => readRoot=1
ReaderDoesNotExcludeWriter == phase="ready" => busy#"reader"
ChildPinIsExact == owners#{} /\ "parent" \notin owners => pins \subseteq {0}
Spec == Init /\ [][Next]_vars
=============================================================================

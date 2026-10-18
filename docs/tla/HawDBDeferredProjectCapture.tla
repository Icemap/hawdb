---------------- MODULE HawDBDeferredProjectCapture ----------------
EXTENDS Naturals
CONSTANTS DropOnTransfer, RecaptureForAdmission, SkipNamespaceFence
VARIABLES namespace, held, phase, metadataRoot, checkedRoot
vars == <<namespace, held, phase, metadataRoot, checkedRoot>>
Init == /\ namespace = 1 /\ held = 1 /\ phase = "snapshot"
        /\ metadataRoot = 0 /\ checkedRoot = 0
Replace == /\ namespace = 1 /\ namespace' = 2
           /\ UNCHANGED <<held, phase, metadataRoot, checkedRoot>>
Restore == /\ namespace = 2 /\ namespace' = 1
           /\ UNCHANGED <<held, phase, metadataRoot, checkedRoot>>
Transfer == /\ phase = "snapshot" /\ phase' = "fence"
            /\ held' = IF DropOnTransfer THEN 0 ELSE held
            /\ UNCHANGED <<namespace, metadataRoot, checkedRoot>>
Refresh == /\ phase = "passed" /\ phase' = "done"
           /\ metadataRoot' = IF RecaptureForAdmission THEN namespace ELSE held
           /\ UNCHANGED <<namespace, held, checkedRoot>>
Fence == /\ phase = "fence" /\ checkedRoot' = namespace
         /\ phase' = IF SkipNamespaceFence \/ namespace = held THEN "passed" ELSE "refused"
         /\ UNCHANGED <<namespace, held, metadataRoot>>
Retry == /\ phase = "refused" /\ phase' = "fence"
         /\ metadataRoot' = 0 /\ checkedRoot' = 0
         /\ UNCHANGED <<namespace, held>>
Next == Replace \/ Restore \/ Transfer \/ Refresh \/ Fence \/ Retry
TypeOK == /\ namespace \in {1, 2} /\ held \in {0, 1}
          /\ phase \in {"snapshot", "fence", "passed", "done", "refused"}
          /\ metadataRoot \in {0, 1, 2} /\ checkedRoot \in {0, 1, 2}
CaptureRetained == held = 1
MetadataFromCapture == metadataRoot \in {0, 1}
FenceObservedOriginal == phase \in {"passed", "done"} => checkedRoot = 1
\* Only the fence's observation is covered. Replace can run after passed:
\* this model deliberately makes no claim about legacy pathname recovery.
Spec == Init /\ [][Next]_vars
=============================================================================

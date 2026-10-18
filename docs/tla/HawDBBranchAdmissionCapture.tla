---------------- MODULE HawDBBranchAdmissionCapture ----------------
EXTENDS Naturals
CONSTANTS PathCatalog, PathLease, PathHead, SkipFence
VARIABLES projectName, branchName, branchCapture, leaseRoot, headRoot,
          firstCatalog, secondCatalog, phase, observed
vars == <<projectName, branchName, branchCapture, leaseRoot, headRoot,
          firstCatalog, secondCatalog, phase, observed>>
PathRoot == IF projectName = 2 THEN 2 ELSE branchName
Init == /\ projectName = 1 /\ branchName = 1 /\ branchCapture = 0
        /\ leaseRoot = 0 /\ headRoot = 0 /\ firstCatalog = 0
        /\ secondCatalog = 0 /\ phase = "entry" /\ observed = 0
ReplaceProject == /\ projectName = 1 /\ projectName' = 2
                  /\ UNCHANGED <<branchName, branchCapture, leaseRoot, headRoot,
                                 firstCatalog, secondCatalog, phase, observed>>
\* The physical UUID directory is selected at capture. Pre-capture branch ABA
\* and persisted birth/incarnation authority are intentionally not modeled.
ReplaceBranch == /\ branchCapture = 1 /\ branchName = 1 /\ branchName' = 2
                 /\ UNCHANGED <<projectName, branchCapture, leaseRoot, headRoot,
                                firstCatalog, secondCatalog, phase, observed>>
Entry == /\ phase = "entry"
         /\ phase' = IF projectName = 1 THEN "catalog" ELSE "refused"
         /\ UNCHANGED <<projectName, branchName, branchCapture, leaseRoot,
                        headRoot, firstCatalog, secondCatalog, observed>>
Catalog == /\ phase = "catalog" /\ phase' = "capture"
           /\ firstCatalog' = IF PathCatalog THEN projectName ELSE 1
           /\ UNCHANGED <<projectName, branchName, branchCapture, leaseRoot,
                          headRoot, secondCatalog, observed>>
Capture == /\ phase = "capture" /\ phase' = "lease" /\ branchCapture' = 1
           /\ UNCHANGED <<projectName, branchName, leaseRoot, headRoot,
                          firstCatalog, secondCatalog, observed>>
Lease == /\ phase = "lease" /\ phase' = "head"
         /\ leaseRoot' = IF PathLease THEN PathRoot ELSE branchCapture
         /\ UNCHANGED <<projectName, branchName, branchCapture, headRoot,
                        firstCatalog, secondCatalog, observed>>
Head == /\ phase = "head" /\ phase' = "fence"
        /\ headRoot' = IF PathHead THEN PathRoot ELSE leaseRoot
        /\ UNCHANGED <<projectName, branchName, branchCapture, leaseRoot,
                       firstCatalog, secondCatalog, observed>>
Fence == /\ phase \in {"fence", "finalFence"} /\ observed' = PathRoot
         /\ phase' = IF ~SkipFence /\ PathRoot # 1 THEN "refused"
                      ELSE IF phase = "fence" THEN "passed" ELSE "finalPassed"
         /\ UNCHANGED <<projectName, branchName, branchCapture, leaseRoot,
                        headRoot, firstCatalog, secondCatalog>>
\* This abstracts only the catalog read after recovery, not the effects of
\* legacy object/WAL/materialization or cleanup operations between fences.
Revalidate == /\ phase = "passed" /\ phase' = "finalFence"
              /\ secondCatalog' = IF PathCatalog THEN projectName ELSE 1
              /\ UNCHANGED <<projectName, branchName, branchCapture, leaseRoot,
                             headRoot, firstCatalog, observed>>
Next == ReplaceProject \/ ReplaceBranch \/ Entry \/ Catalog \/ Capture \/ Lease \/ Head \/ Fence \/ Revalidate
TypeOK == /\ projectName \in {1, 2} /\ branchName \in {1, 2}
          /\ branchCapture \in {0, 1} /\ leaseRoot \in {0, 1, 2}
          /\ headRoot \in {0, 1, 2} /\ firstCatalog \in {0, 1, 2}
          /\ secondCatalog \in {0, 1, 2} /\ observed \in {0, 1, 2}
          /\ phase \in {"entry", "catalog", "capture", "lease", "head", "fence", "passed", "finalFence", "finalPassed", "refused"}
CatalogFromCapture == firstCatalog \in {0, 1} /\ secondCatalog \in {0, 1}
LeaseFromCapture == leaseRoot \in {0, 1}
HeadFromCapture == headRoot \in {0, 1}
FenceObservedOriginal == phase \in {"passed", "finalPassed"} => observed = 1
Spec == Init /\ [][Next]_vars
=============================================================================

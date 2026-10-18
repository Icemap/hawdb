---------------- MODULE HawDBProjectMetadataCapture ----------------
EXTENDS Naturals
CONSTANTS PathForSelector, PathForCatalog, RecaptureOnRefresh
VARIABLES namespace, held, phase, attempt, selectorRoot, catalogRoot
vars == <<namespace, held, phase, attempt, selectorRoot, catalogRoot>>
Roots == {1, 2}
\* Both directories may encode the same logical project UUID. UUID equality
\* therefore cannot establish which physical root supplied either file.
Init == /\ namespace = 1 /\ held = 1 /\ phase = "selector"
        /\ attempt = 0 /\ selectorRoot = 0 /\ catalogRoot = 0
ReplaceNamespace == /\ namespace = 1 /\ namespace' = 2
                    /\ UNCHANGED <<held, phase, attempt, selectorRoot, catalogRoot>>
ReadSelector == /\ phase = "selector"
                /\ selectorRoot' = IF PathForSelector THEN namespace ELSE held
                /\ phase' = "catalog"
                /\ UNCHANGED <<namespace, held, attempt, catalogRoot>>
ReadCatalog == /\ phase = "catalog"
               /\ catalogRoot' = IF PathForCatalog THEN namespace ELSE held
               /\ phase' = "ready"
               /\ UNCHANGED <<namespace, held, attempt, selectorRoot>>
Refresh == /\ phase = "ready" /\ attempt = 0
           /\ held' = IF RecaptureOnRefresh THEN namespace ELSE held
           /\ attempt' = 1 /\ phase' = "selector"
           /\ selectorRoot' = 0 /\ catalogRoot' = 0
           /\ UNCHANGED namespace
Next == ReplaceNamespace \/ ReadSelector \/ ReadCatalog \/ Refresh
TypeOK == /\ namespace \in Roots /\ held \in Roots
          /\ phase \in {"selector", "catalog", "ready"}
          /\ attempt \in {0, 1}
          /\ selectorRoot \in {0, 1, 2} /\ catalogRoot \in {0, 1, 2}
CaptureRetained == held = 1
SelectorFromCapture == selectorRoot \in {0, 1}
CatalogFromCapture == catalogRoot \in {0, 1}
CompleteSameRoot == phase = "ready" => selectorRoot = 1 /\ catalogRoot = 1
Spec == Init /\ [][Next]_vars
=============================================================================

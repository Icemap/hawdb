------------------------ MODULE HawDBHeldWalRollback ------------------------
EXTENDS Naturals, TLC
CONSTANTS PathRollback, UnlinkName, DropBeforeRollback, SkipNameFence, SkipEmptyHeader
VARIABLES phase, name, held, committed, bytes, observed, header, present
vars == <<phase, name, held, committed, bytes, observed, header, present>>
Init == /\ phase = "partial" /\ name = 1 /\ held = 1
        /\ committed \in {0, 2}
        /\ bytes = [i \in {1, 2} |-> IF i = 1 THEN committed + 1 ELSE 7]
        /\ observed = 0 /\ header = (committed > 0)
        /\ present = [i \in {1, 2} |-> TRUE]
ReplaceName == /\ name = 1 /\ name' = 2
               /\ UNCHANGED <<phase, held, committed, bytes, observed, header, present>>
Prepare == /\ phase = "partial" /\ phase' = "rollback"
           /\ held' = IF DropBeforeRollback THEN 0 ELSE held
           /\ UNCHANGED <<name, committed, bytes, observed, header, present>>
Rollback == /\ phase = "rollback" /\ phase' = "header"
            /\ bytes' = [bytes EXCEPT ![IF PathRollback THEN name ELSE held] = committed]
            /\ header' = (committed > 0)
            /\ UNCHANGED <<name, held, committed, observed, present>>
RepairHeader == /\ phase = "header" /\ phase' = "sync"
                /\ header' = IF SkipEmptyHeader THEN header ELSE TRUE
                /\ UNCHANGED <<name, held, committed, bytes, observed, present>>
Synchronize == /\ phase = "sync" /\ phase' = "fence"
               /\ present' = IF UnlinkName /\ committed = 0 THEN [present EXCEPT ![name] = FALSE] ELSE present
               /\ UNCHANGED <<name, held, committed, bytes, observed, header>>
Fence == /\ phase = "fence" /\ observed' = IF present[name] THEN name ELSE 0
         /\ phase' = IF SkipNameFence \/ (name = held /\ present[name]) THEN "retryable" ELSE "fatal"
         /\ UNCHANGED <<name, held, committed, bytes, header, present>>
Next == ReplaceName \/ Prepare \/ Rollback \/ RepairHeader \/ Synchronize \/ Fence
TypeOK == /\ phase \in {"partial", "rollback", "header", "sync", "fence", "retryable", "fatal"}
          /\ name \in {1, 2} /\ held \in {0, 1} /\ committed \in {0, 2}
          /\ present \in [{1, 2} -> BOOLEAN]
          /\ header \in BOOLEAN
          /\ bytes \in [{1, 2} -> Nat] /\ observed \in {0, 1, 2}
HeldThroughRollback == held = 1
ForeignUntouched == bytes[2] = 7 /\ present[2]
OriginalRestored == phase \in {"header", "sync", "fence", "retryable", "fatal"} => bytes[1] = committed
RecoverableHeader == phase \in {"fence", "retryable", "fatal"} => header
RetryObservedOriginal == phase = "retryable" => observed = 1
\* Replacement remains possible after the observation. No later pathname
\* append, namespace barrier or publication is authorized by this model.
Spec == Init /\ [][Next]_vars
=============================================================================

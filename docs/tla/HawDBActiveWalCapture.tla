---------------- MODULE HawDBActiveWalCapture ----------------
EXTENDS Naturals
CONSTANTS PathPrefix, PathReplay, DropBeforeReplay, SkipNameFence
VARIABLES name, held, prefixRoot, replayRoot, phase, observed
vars == <<name, held, prefixRoot, replayRoot, phase, observed>>
Init == /\ name = 1 /\ held = 1 /\ prefixRoot = 0 /\ replayRoot = 0
        /\ phase = "prefix" /\ observed = 0
ReplaceName == /\ name = 1 /\ name' = 2
               /\ UNCHANGED <<held, prefixRoot, replayRoot, phase, observed>>
Prefix == /\ phase = "prefix" /\ phase' = "replay"
          /\ prefixRoot' = IF PathPrefix THEN name ELSE held
          /\ held' = IF DropBeforeReplay THEN 0 ELSE held
          /\ UNCHANGED <<name, replayRoot, observed>>
Replay == /\ phase = "replay" /\ phase' = "fence"
          /\ replayRoot' = IF PathReplay THEN name ELSE held
          /\ UNCHANGED <<name, held, prefixRoot, observed>>
Fence == /\ phase = "fence" /\ observed' = name
         /\ phase' = IF SkipNameFence \/ name = held THEN "accepted" ELSE "refused"
         /\ UNCHANGED <<name, held, prefixRoot, replayRoot>>
Next == ReplaceName \/ Prefix \/ Replay \/ Fence
TypeOK == /\ name \in {1, 2} /\ held \in {0, 1}
          /\ prefixRoot \in {0, 1, 2} /\ replayRoot \in {0, 1, 2}
          /\ phase \in {"prefix", "replay", "fence", "accepted", "refused"}
          /\ observed \in {0, 1, 2}
HeldThroughRecovery == held = 1
PrefixFromCapture == prefixRoot \in {0, 1}
ReplayFromCapture == replayRoot \in {0, 1}
AcceptedSameFile == phase = "accepted" => prefixRoot = 1 /\ replayRoot = 1 /\ observed = 1
\* A name change after acceptance remains enabled. This is a checked
\* observation, not atomic exclusion or authority for later pathname writes.
Spec == Init /\ [][Next]_vars
=============================================================================

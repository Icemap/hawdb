---------------- MODULE HawDBLegacyProjectionEpoch ----------------
EXTENDS Naturals, TLC
CONSTANTS SkipHostFence, SkipGraphFence, ForbidPinnedWitness
VARIABLES head, pin, mode, target, phase, hostEpoch, graphEpoch,
          hostRead, graphRead, tag, payload
vars == <<head, pin, mode, target, phase, hostEpoch, graphEpoch,
          hostRead, graphRead, tag, payload>>
Value(epoch) == CASE epoch = 0 -> "C" [] epoch = 1 -> "B" [] OTHER -> "E"
Rows(epoch) == <<Value(epoch), Value(epoch), epoch = 2>>
Init == /\ head = 1 /\ pin = 0 /\ mode \in {"live", "pinned"}
        /\ target = 0 /\ phase = "idle" /\ hostEpoch = 0 /\ graphEpoch = 0
        /\ hostRead = FALSE /\ graphRead = FALSE /\ tag = 0 /\ payload = Rows(0)
Advance == /\ head = 1 /\ head' = 2
           /\ UNCHANGED <<pin, mode, target, phase, hostEpoch, graphEpoch,
                hostRead, graphRead, tag, payload>>
Pin == /\ pin = 0 /\ phase \in {"idle", "selected"} /\ pin' = head
       /\ UNCHANGED <<head, mode, target, phase, hostEpoch, graphEpoch,
            hostRead, graphRead, tag, payload>>
Select == /\ phase = "idle" /\ target' \in 1..head /\ phase' = "selected"
          /\ UNCHANGED <<head, pin, mode, hostEpoch, graphEpoch,
               hostRead, graphRead, tag, payload>>
HydrateHost == /\ phase = "selected" /\ pin > 0
               /\ (SkipHostFence \/ target = pin)
               /\ hostEpoch' = pin /\ hostRead' = TRUE /\ phase' = "hosted"
               /\ UNCHANGED <<head, pin, mode, target, graphEpoch, graphRead, tag, payload>>
HydrateGraph == LET source == IF mode = "pinned" THEN pin ELSE head IN
                /\ phase = "hosted" /\ (SkipGraphFence \/ target = source)
                /\ graphEpoch' = source /\ graphRead' = TRUE /\ phase' = "prepared"
                /\ UNCHANGED <<head, pin, mode, target, hostEpoch, hostRead, tag, payload>>
Apply == /\ phase = "prepared" /\ tag' = target
         /\ payload' = <<Value(graphEpoch), Value(hostEpoch), target = 2>>
         /\ phase' = "done"
         /\ UNCHANGED <<head, pin, mode, target, hostEpoch, graphEpoch, hostRead, graphRead>>
Next == Advance \/ Pin \/ Select \/ HydrateHost \/ HydrateGraph \/ Apply
TypeOK == /\ head \in {1, 2} /\ pin \in 0..2 /\ mode \in {"live", "pinned"}
          /\ target \in 0..2 /\ phase \in {"idle", "selected", "hosted", "prepared", "done"}
          /\ hostEpoch \in 0..2 /\ graphEpoch \in 0..2
          /\ hostRead \in BOOLEAN /\ graphRead \in BOOLEAN /\ tag \in 0..2
          /\ payload \in {"C", "B", "E"} \X {"C", "B", "E"} \X BOOLEAN
HostReadsAtSelectedEpoch == hostRead => hostEpoch = target
GraphReadsAtSelectedEpoch == graphRead => graphEpoch = target
PublishedViewMatchesTag == payload = Rows(tag)
NoPinnedWitness == ~ForbidPinnedWitness \/
                   ~(mode = "pinned" /\ pin = 1 /\ head = 2 /\ tag = 1 /\ phase = "done")
Spec == Init /\ [][Next]_vars
=============================================================================

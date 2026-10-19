---------------- MODULE HawDBConsumerAllocationBootstrap ----------------
EXTENDS Naturals, FiniteSets, TLC
CONSTANTS EarlyIdentity, EarlyEffect, RegistryReset, CancelRefund, MissingReset
VARIABLES initialized, available, emptySynced, live, durable, highest,
          phase, candidate, active, exposed, effects, reused
vars == <<initialized, available, emptySynced, live, durable, highest,
          phase, candidate, active, exposed, effects, reused>>
Init == /\ initialized = FALSE /\ available = FALSE /\ emptySynced = FALSE
        /\ live = 0 /\ durable = 0 /\ highest = 0 /\ phase = "idle"
        /\ candidate = 0 /\ active = 0 /\ exposed = {} /\ effects = {} /\ reused = FALSE
EmptyLedger == /\ ~initialized /\ ~available /\ available' = TRUE
               /\ UNCHANGED <<initialized, emptySynced, live, durable, highest,
                    phase, candidate, active, exposed, effects, reused>>
SyncEmpty == /\ available /\ ~initialized /\ emptySynced' = TRUE
             /\ UNCHANGED <<initialized, available, live, durable, highest,
                  phase, candidate, active, exposed, effects, reused>>
Identity == /\ ~initialized /\ (emptySynced \/ EarlyIdentity) /\ initialized' = TRUE
            /\ UNCHANGED <<available, emptySynced, live, durable, highest,
                 phase, candidate, active, exposed, effects, reused>>
Propose == /\ initialized /\ available /\ phase = "idle" /\ live < 6
           /\ candidate' = live + 3 /\ phase' = "selected" /\ live' = live + 3
           /\ UNCHANGED <<initialized, available, emptySynced, durable, highest,
                active, exposed, effects, reused>>
SyncReserve == /\ phase = "selected" /\ durable' = live /\ highest' = live
               /\ phase' = "return"
               /\ UNCHANGED <<initialized, available, emptySynced, live,
                    candidate, active, exposed, effects, reused>>
Return == /\ phase = "return" /\ active' = candidate /\ phase' = "active"
          /\ reused' = (reused \/ candidate \in exposed) /\ exposed' = exposed \cup {candidate}
          /\ UNCHANGED <<initialized, available, emptySynced, live, durable, highest,
               candidate, effects>>
LostReply == /\ phase = "return" /\ phase' = "idle" /\ candidate' = 0
             /\ UNCHANGED <<initialized, available, emptySynced, live, durable, highest,
                  active, exposed, effects, reused>>
Effect == /\ phase = "active" /\ effects' = effects \cup {active}
          /\ UNCHANGED <<initialized, available, emptySynced, live, durable, highest,
               phase, candidate, active, exposed, reused>>
EarlyNativeEffect == /\ EarlyEffect /\ phase = "selected" /\ effects' = effects \cup {candidate}
                     /\ UNCHANGED <<initialized, available, emptySynced, live, durable, highest,
                          phase, candidate, active, exposed, reused>>
Cancel == /\ phase = "active" /\ phase' = "idle" /\ active' = 0 /\ candidate' = 0
          /\ live' = IF CancelRefund THEN live - 3 ELSE live
          /\ durable' = IF CancelRefund THEN durable - 3 ELSE durable
          /\ UNCHANGED <<initialized, available, emptySynced, highest, exposed, effects, reused>>
RebuildRegistry == /\ phase = "idle"
                   /\ live' = IF RegistryReset THEN 0 ELSE live
                   /\ durable' = IF RegistryReset THEN 0 ELSE durable
                   /\ UNCHANGED <<initialized, available, emptySynced, highest,
                        phase, candidate, active, exposed, effects, reused>>
LoseLedger == /\ initialized /\ phase = "idle" /\ available /\ available' = FALSE
              /\ UNCHANGED <<initialized, emptySynced, live, durable, highest,
                   phase, candidate, active, exposed, effects, reused>>
ResetMissing == /\ MissingReset /\ initialized /\ ~available /\ available' = TRUE
                /\ live' = 0 /\ durable' = 0
                /\ UNCHANGED <<initialized, emptySynced, highest,
                     phase, candidate, active, exposed, effects, reused>>
Crash == /\ live' = durable /\ phase' = "idle" /\ candidate' = 0 /\ active' = 0
         /\ UNCHANGED <<initialized, available, emptySynced, durable, highest,
              exposed, effects, reused>>
Next == EmptyLedger \/ SyncEmpty \/ Identity \/ Propose \/ SyncReserve \/ Return
        \/ LostReply \/ Effect \/ EarlyNativeEffect \/ Cancel \/ RebuildRegistry
        \/ LoseLedger \/ ResetMissing \/ Crash
TypeOK == /\ initialized \in BOOLEAN /\ available \in BOOLEAN /\ emptySynced \in BOOLEAN
          /\ live \in {0, 3, 6} /\ durable \in {0, 3, 6} /\ highest \in {0, 3, 6}
          /\ candidate \in {0, 3, 6} /\ active \in {0, 3, 6}
          /\ phase \in {"idle", "selected", "return", "active"}
          /\ exposed \subseteq {3, 6} /\ effects \subseteq {3, 6} /\ reused \in BOOLEAN
IdentityAfterEmptyBarrier == initialized => emptySynced
NoRefundOrReset == initialized /\ available => live >= highest /\ durable >= highest
EffectsAfterReserveBarrier == \A op \in effects : op <= highest
NeverReuse == ~reused
Spec == Init /\ [][Next]_vars
=============================================================================

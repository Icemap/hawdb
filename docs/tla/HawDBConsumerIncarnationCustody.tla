---------------- MODULE HawDBConsumerIncarnationCustody ----------------
EXTENDS Naturals, FiniteSets
CONSTANTS CreateBeforeReservationBarrier, ReuseCancelledIncarnation,
          AdoptUnknownBirth, ResetMissingLedger, ForgetDebtOnClose,
          RetireBeforeSelectorBarrier, SelectIncompleteClosure
VARIABLE s
vars == <<s>>
Ids == 1..2
Root(head) == IF head = 0 THEN {} ELSE {head}
Init == s = [phase |-> "idle", counter |-> 0, durableCounter |-> 0,
             pending |-> {}, durablePending |-> {}, dirs |-> {}, births |-> {},
             complete |-> {}, selected |-> 0, durableSelected |-> 0,
             retired |-> {}, acknowledged |-> {}, ever |-> {}, reused |-> FALSE,
             adopted |-> {}, op |-> 0, knownCreated |-> 0, crashes |-> 0,
             ledgerPresent |-> TRUE, quarantined |-> FALSE]
Reserve == /\ s.phase = "idle" /\ s.ledgerPresent /\ s.counter < 2
           /\ s.selected = s.durableSelected
           /\ LET id == s.counter + 1 IN
              s' = [s EXCEPT !.phase = "reserved", !.counter = id, !.op = id,
                             !.pending = @ \cup {id},
                             !.reused = @ \/ id \in s.ever]
ReservationBarrier == /\ s.phase = "reserved" /\ s.ledgerPresent
                      /\ s' = [s EXCEPT !.phase = "reserved_synced",
                                       !.durableCounter = s.counter,
                                       !.durablePending = s.pending, !.ever = @ \cup {s.op}]
Create == /\ s.phase = "reserved_synced" \/
                (CreateBeforeReservationBarrier /\ s.phase = "reserved")
          /\ s.ledgerPresent
          \* Worst case: the namespace effect survives power loss immediately.
          /\ s' = [s EXCEPT !.phase = "created", !.dirs = @ \cup {s.op},
                           !.knownCreated = s.op, !.ever = @ \cup {s.op}]
BirthBarrier == /\ s.phase = "created" /\ s.op = s.knownCreated
                /\ s.op \in s.durablePending /\ s.ledgerPresent
                \* Abstracts actual created-handle/birth-marker + ledger sync.
                /\ s' = [s EXCEPT !.phase = "born", !.births = @ \cup {s.op}]
SealClosure == /\ s.phase = "born" /\ s.ledgerPresent
               \* A complete durable producer closure/prebuilt view is assumed.
               /\ s' = [s EXCEPT !.phase = "sealed", !.complete = @ \cup {s.op}]
Select == /\ s.phase = "sealed" \/
                (SelectIncompleteClosure /\ s.phase = "born")
          /\ s.ledgerPresent
          /\ s' = [s EXCEPT !.phase = "selected", !.selected = s.op,
                           !.retired = @ \cup Root(s.durableSelected),
                           !.dirs = IF RetireBeforeSelectorBarrier
                                    THEN @ \ Root(s.durableSelected) ELSE @]
SelectorBarrier == /\ s.phase = "selected" /\ s.ledgerPresent
                   /\ s' = [s EXCEPT !.phase = "committed",
                                    !.durableSelected = s.selected,
                                    !.retired = @ \cup Root(s.durableSelected),
                                    !.pending = @ \ {s.op},
                                    !.durablePending = @ \ {s.op}]
Acknowledge == /\ s.phase = "committed" /\ s.ledgerPresent
               /\ s' = [s EXCEPT !.phase = "acknowledged",
                                !.acknowledged = @ \cup {s.op}]
Close == /\ s.op # 0 /\ s.phase \notin {"recovery", "refused"}
         /\ s' = [s EXCEPT !.phase = "idle", !.op = 0, !.knownCreated = 0,
                          !.pending = IF ForgetDebtOnClose THEN {} ELSE @,
                          !.durablePending = IF ForgetDebtOnClose THEN {} ELSE @]
Cancel == /\ s.phase \in {"reserved_synced", "created", "born", "sealed"}
          /\ s' = [s EXCEPT !.phase = "idle", !.op = 0, !.knownCreated = 0,
                           !.counter = IF ReuseCancelledIncarnation
                                       THEN s.op - 1 ELSE @,
                           !.durableCounter = IF ReuseCancelledIncarnation
                                              THEN s.op - 1 ELSE @]
Crash == /\ s.crashes = 0 /\ s.ledgerPresent
         \* An unsynchronized selector may survive or be lost, never be partial.
         /\ \E recovered \in {s.selected, s.durableSelected}:
              s' = [s EXCEPT !.phase = "recovery", !.counter = s.durableCounter,
                             !.pending = s.durablePending, !.selected = recovered,
                             !.durableSelected = recovered, !.op = 0,
                             !.knownCreated = 0, !.crashes = 1]
Adopt == /\ s.phase = "recovery" /\ s.ledgerPresent
         /\ \E id \in s.dirs \cap s.durablePending:
              /\ id \in s.births \/ AdoptUnknownBirth
              /\ s' = [s EXCEPT !.adopted = @ \cup {id}]
Reopen == /\ s.phase = "recovery" /\ s.ledgerPresent
          /\ s' = [s EXCEPT !.phase = "idle"]
LoseLedger == /\ s.ledgerPresent
              /\ s' = [s EXCEPT !.ledgerPresent = FALSE, !.quarantined = TRUE,
                               !.phase = "refused", !.op = 0, !.knownCreated = 0]
ResetLostLedger == /\ ~s.ledgerPresent /\ ResetMissingLedger
                   /\ s' = [s EXCEPT !.ledgerPresent = TRUE,
                                    !.quarantined = FALSE, !.phase = "idle",
                                    !.counter = 0, !.durableCounter = 0,
                                    !.pending = {}, !.durablePending = {}]
Next == Reserve \/ ReservationBarrier \/ Create \/ BirthBarrier \/ SealClosure \/
        Select \/ SelectorBarrier \/ Acknowledge \/ Close \/ Cancel \/ Crash \/
        Adopt \/ Reopen \/ LoseLedger \/ ResetLostLedger
TypeOK == /\ s.phase \in {"idle", "reserved", "reserved_synced", "created", "born",
                          "sealed", "selected", "committed", "acknowledged",
                          "recovery", "refused"}
          /\ s.counter \in 0..2 /\ s.durableCounter \in 0..2
          /\ s.op \in 0..2 /\ s.knownCreated \in 0..2 /\ s.crashes \in 0..1
          /\ s.selected \in 0..2 /\ s.durableSelected \in 0..2
          /\ \A field \in {"pending", "durablePending", "dirs", "births", "complete",
                           "retired", "acknowledged", "ever", "adopted"}:
                s[field] \subseteq Ids
          /\ s.reused \in BOOLEAN /\ s.ledgerPresent \in BOOLEAN
          /\ s.quarantined \in BOOLEAN
IncarnationsNeverReuse == ~s.reused
NativeCustodyRetained == s.quarantined \/
    s.dirs \subseteq (s.pending \cup s.durablePending \cup s.retired \cup
                      Root(s.selected) \cup Root(s.durableSelected))
AdoptionHasActualBirth == s.adopted \subseteq s.births
SelectedClosureComplete == Root(s.selected) \subseteq (s.dirs \cap s.births \cap s.complete)
DurableSelectorUsable == Root(s.durableSelected) \subseteq (s.dirs \cap s.births \cap s.complete)
AcknowledgedClosureDurable == s.acknowledged \subseteq (s.dirs \cap s.births \cap s.complete)
Spec == Init /\ [][Next]_vars
======================================================================

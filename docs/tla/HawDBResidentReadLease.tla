-------------------- MODULE HawDBResidentReadLease --------------------
EXTENDS Naturals, FiniteSets
CONSTANTS Readers, MaxGeneration, Capacity, CaptureAtomically,
          ChargeBeforeRead, PinWhileWaiting, DropBeforeRefund
VARIABLES current, phase, pair, pinned, charged
vars == <<current, phase, pair, pinned, charged>>
Phases == {"idle", "waiting", "admitted", "captured", "reading", "releasing", "done"}
Generations == 0..MaxGeneration
LiveLeases(g, except) ==
    {r \in Readers \ {except} : phase[r] \in {"reading", "releasing"} /\ pair[r][2] = g}

Init ==
    /\ current = 0
    /\ phase = [r \in Readers |-> "idle"]
    /\ pair = [r \in Readers |-> <<0, 0>>]
    /\ pinned = {}
    /\ charged = {}

Queue(r) ==
    /\ phase[r] = "idle"
    /\ phase' = [phase EXCEPT ![r] = "waiting"]
    /\ pinned' = IF PinWhileWaiting THEN pinned \cup {r} ELSE pinned
    /\ UNCHANGED <<current, pair, charged>>
Admit(r) ==
    /\ phase[r] = "waiting"
    /\ phase' = [phase EXCEPT ![r] = "admitted"]
    /\ UNCHANGED <<current, pair, pinned, charged>>
Capture(r) ==
    /\ phase[r] = "admitted"
    /\ phase' = [phase EXCEPT ![r] = "captured"]
    /\ pair' = [pair EXCEPT ![r] =
        <<current, IF CaptureAtomically THEN current ELSE MaxGeneration>>]
    /\ pinned' = pinned \cup {r}
    /\ UNCHANGED <<current, charged>>
BeginRead(r) ==
    /\ phase[r] = "captured"
    /\ pair[r][2] \in charged \/ Cardinality(charged) < Capacity
    /\ phase' = [phase EXCEPT ![r] = "reading"]
    /\ charged' = IF ChargeBeforeRead THEN charged \cup {pair[r][2]} ELSE charged
    /\ UNCHANGED <<current, pair, pinned>>
Refuse(r) ==
    /\ phase[r] \in {"waiting", "admitted", "captured"}
    /\ phase' = [phase EXCEPT ![r] = "done"]
    /\ pinned' = pinned \ {r}
    /\ UNCHANGED <<current, pair, charged>>
DropPin(r) ==
    /\ phase[r] = "reading"
    /\ phase' = [phase EXCEPT ![r] = "releasing"]
    /\ pinned' = IF DropBeforeRefund THEN pinned \ {r} ELSE pinned
    /\ charged' = IF DropBeforeRefund THEN charged ELSE charged \ {pair[r][2]}
    /\ UNCHANGED <<current, pair>>
Refund(r) ==
    /\ phase[r] = "releasing"
    /\ phase' = [phase EXCEPT ![r] = "done"]
    /\ pinned' = pinned \ {r}
    /\ charged' = IF LiveLeases(pair[r][2], r) = {}
                   THEN charged \ {pair[r][2]} ELSE charged
    /\ UNCHANGED <<current, pair>>
Publish ==
    /\ current < MaxGeneration
    /\ current' = current + 1
    /\ UNCHANGED <<phase, pair, pinned, charged>>
Next == Publish \/ \E r \in Readers :
    Queue(r) \/ Admit(r) \/ Capture(r) \/ BeginRead(r) \/ Refuse(r) \/ DropPin(r) \/ Refund(r)

TypeOK ==
    /\ current \in Generations
    /\ phase \in [Readers -> Phases]
    /\ pair \in [Readers -> (Generations \X Generations)]
    /\ pinned \subseteq Readers /\ charged \subseteq Generations
AtomicPair == \A r \in Readers : phase[r] \in {"captured", "reading", "releasing"}
    => pair[r][1] = pair[r][2]
WaitingUnpinned == \A r \in Readers : phase[r] = "waiting" => r \notin pinned
ReadingCharged == \A r \in Readers : phase[r] = "reading" => pair[r][2] \in charged
PinReleasedBeforeRefund == \A r \in pinned : phase[r] \in {"reading", "releasing"}
    => pair[r][2] \in charged
NoOverbooking == Cardinality(charged) <= Capacity
TerminalUnpinned == \A r \in Readers : phase[r] = "done" => r \notin pinned
SharedChargeRefund == \A g \in charged :
    \E r \in Readers : phase[r] \in {"reading", "releasing"} /\ pair[r][2] = g
=============================================================================

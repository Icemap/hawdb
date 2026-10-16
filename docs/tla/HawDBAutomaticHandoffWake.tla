-------------------- MODULE HawDBAutomaticHandoffWake --------------------
EXTENDS Naturals

(* A handoff guard and one worker, after thread registration. The runtime *)
(* borrow is released at most once and shutdown is requested at most once. *)
(* A different blocking operation may consume the worker's parking token *)
(* before its final predicate check. No blocking call follows that check. *)
(* Progress assumes weakly fair worker scheduling; guard release and stop *)
(* are optional. Durability, retirement and admission are modeled separately.*)

CONSTANT ForgetWakeEpoch
ASSUME ForgetWakeEpoch \in BOOLEAN

VARIABLES held, phase, epoch, observed, token, stopping
vars == <<held, phase, epoch, observed, token, stopping>>

Init ==
    /\ held = TRUE
    /\ phase = "checking"
    /\ epoch = 0
    /\ observed = 0
    /\ token = FALSE
    /\ stopping = FALSE

TryAdopt ==
    /\ phase = "checking" /\ ~stopping
    /\ observed' = epoch
    /\ phase' = IF held THEN "contended" ELSE "done"
    /\ UNCHANGED <<held, epoch, token, stopping>>

InterveningWait ==
    /\ phase = "contended"
    /\ phase' = "deciding"
    /\ token' \in {FALSE, token}
    /\ UNCHANGED <<held, epoch, observed, stopping>>

Decide ==
    /\ phase = "deciding"
    /\ phase' = IF ForgetWakeEpoch \/ epoch = observed
                 THEN "parking" ELSE "checking"
    /\ UNCHANGED <<held, epoch, observed, token, stopping>>

Park ==
    /\ phase = "parking"
    /\ phase' = IF token THEN "checking" ELSE "parked"
    /\ token' = FALSE
    /\ UNCHANGED <<held, epoch, observed, stopping>>

Wake ==
    /\ phase = "parked" /\ token
    /\ phase' = "checking"
    /\ token' = FALSE
    /\ UNCHANGED <<held, epoch, observed, stopping>>

ReleaseGuard ==
    /\ held
    /\ held' = FALSE
    /\ epoch' = epoch + 1
    /\ token' = TRUE
    /\ UNCHANGED <<phase, observed, stopping>>

Stop ==
    /\ ~stopping /\ phase \notin {"done", "exited"}
    /\ stopping' = TRUE
    /\ epoch' = epoch + 1
    /\ token' = TRUE
    /\ UNCHANGED <<held, phase, observed>>

Exit ==
    /\ phase = "checking" /\ stopping
    /\ phase' = "exited"
    /\ UNCHANGED <<held, epoch, observed, token, stopping>>

Next == TryAdopt \/ InterveningWait \/ Decide \/ Park \/ Wake
        \/ ReleaseGuard \/ Stop \/ Exit

Spec == Init /\ [][Next]_vars
             /\ WF_vars(TryAdopt) /\ WF_vars(InterveningWait)
             /\ WF_vars(Decide) /\ WF_vars(Park)
             /\ WF_vars(Wake) /\ WF_vars(Exit)

TypeOK ==
    /\ held \in BOOLEAN
    /\ phase \in {"checking", "contended", "deciding", "parking", "parked", "done", "exited"}
    /\ epoch \in 0..2
    /\ observed \in 0..2
    /\ token \in BOOLEAN
    /\ stopping \in BOOLEAN

ReleasedGuardProgress == ~held ~> phase \in {"done", "exited"}
StopProgress == stopping ~> phase = "exited"
=============================================================================

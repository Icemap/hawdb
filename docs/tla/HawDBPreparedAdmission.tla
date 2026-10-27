-------------------- MODULE HawDBPreparedAdmission --------------------
EXTENDS Naturals, FiniteSets

CONSTANTS Queries, MaxCapacity, PreserveCharge, ReleaseCpu, CheckRetainedFloor, DropBeforeWait
VARIABLES phase, memory, execution, cpu, pins, capacity, checkedCapacity, checkedMinimum, hardRefusal
vars == <<phase, memory, execution, cpu, pins, capacity, checkedCapacity, checkedMinimum, hardRefusal>>
Phases == {"idle", "preparing", "prepared", "retrying", "waiting", "executing", "done"}
PreparationBytes == 1
TotalMemory ==
    LET Sum[chosen \in SUBSET Queries] ==
        IF chosen = {} THEN 0
        ELSE LET q == CHOOSE q \in chosen : TRUE
             IN memory[q] + Sum[chosen \ {q}]
    IN Sum[Queries]

Init ==
    /\ phase = [q \in Queries |-> "idle"]
    /\ memory = [q \in Queries |-> 0]
    /\ execution = [q \in Queries |-> 0]
    /\ checkedCapacity = [q \in Queries |-> 0]
    /\ checkedMinimum = [q \in Queries |-> 0]
    /\ hardRefusal = [q \in Queries |-> "none"]
    /\ cpu = {} /\ pins = {}
    /\ capacity = MaxCapacity

Prepare(q, size) ==
    /\ phase[q] = "idle" /\ cpu = {}
    /\ TotalMemory + PreparationBytes <= capacity
    /\ phase' = [phase EXCEPT ![q] = "preparing"]
    /\ memory' = [memory EXCEPT ![q] = PreparationBytes]
    /\ execution' = [execution EXCEPT ![q] = size]
    /\ cpu' = {q} /\ pins' = {q}
    /\ UNCHANGED <<capacity, checkedCapacity, checkedMinimum, hardRefusal>>

Transfer(q) ==
    /\ phase[q] = "preparing"
    /\ phase' = [phase EXCEPT ![q] = "prepared"]
    /\ memory' = IF PreserveCharge THEN memory ELSE [memory EXCEPT ![q] = 0]
    /\ cpu' = IF ReleaseCpu THEN cpu \ {q} ELSE cpu
    /\ pins' = pins \ {q}
    /\ UNCHANGED <<execution, capacity, checkedCapacity, checkedMinimum, hardRefusal>>

AdmitExecution(q) ==
    /\ phase[q] \in {"prepared", "retrying", "waiting"}
    /\ checkedCapacity' = [checkedCapacity EXCEPT ![q] = capacity]
    /\ checkedMinimum' = [checkedMinimum EXCEPT ![q] = memory[q] + execution[q]]
    /\ LET minimum == execution[q] + IF CheckRetainedFloor THEN memory[q] ELSE 0
       IN IF minimum > capacity
          THEN IF phase[q] = "prepared"
               THEN /\ phase' = [phase EXCEPT ![q] = "retrying"]
                    /\ memory' = IF DropBeforeWait THEN [memory EXCEPT ![q] = 0] ELSE memory
                    /\ hardRefusal' = [hardRefusal EXCEPT ![q] =
                        IF execution[q] <= capacity THEN "rebuildable" ELSE "oversized"]
                    /\ UNCHANGED cpu
               ELSE /\ phase' = [phase EXCEPT ![q] = "done"]
                    /\ memory' = [memory EXCEPT ![q] = 0]
                    /\ cpu' = cpu \ {q}
                    /\ UNCHANGED hardRefusal
          ELSE IF cpu = {} /\ TotalMemory + execution[q] <= capacity
               THEN /\ phase' = [phase EXCEPT ![q] = "executing"]
                    /\ memory' = [memory EXCEPT ![q] = @ + execution[q]]
                    /\ cpu' = {q}
                    /\ UNCHANGED hardRefusal
               ELSE /\ phase' = [phase EXCEPT ![q] = "waiting"]
                    /\ memory' = IF DropBeforeWait THEN [memory EXCEPT ![q] = 0] ELSE memory
                    /\ UNCHANGED <<cpu, hardRefusal>>
    /\ UNCHANGED <<execution, pins, capacity>>

\* Failure, panic, cancellation and normal completion all retire owned state.
Retire(q) ==
    /\ phase[q] \notin {"idle", "done"}
    /\ phase' = [phase EXCEPT ![q] = "done"]
    /\ memory' = [memory EXCEPT ![q] = 0]
    /\ cpu' = cpu \ {q} /\ pins' = pins \ {q}
    /\ UNCHANGED <<execution, capacity, checkedCapacity, checkedMinimum, hardRefusal>>

\* A host/cgroup or process-policy change cannot revoke existing reservations.
Refresh ==
    /\ capacity' \in 0..MaxCapacity
    /\ UNCHANGED <<phase, memory, execution, cpu, pins, checkedCapacity, checkedMinimum, hardRefusal>>

Next ==
    \/ \E q \in Queries, size \in 1..(MaxCapacity + 1) : Prepare(q, size)
    \/ \E q \in Queries : Transfer(q) \/ AdmitExecution(q) \/ Retire(q)
    \/ Refresh

TypeOK ==
    /\ phase \in [Queries -> Phases]
    /\ memory \in [Queries -> 0..(MaxCapacity + 2)]
    /\ execution \in [Queries -> 0..(MaxCapacity + 1)]
    /\ checkedCapacity \in [Queries -> 0..MaxCapacity]
    /\ checkedMinimum \in [Queries -> 0..(MaxCapacity + 2)]
    /\ hardRefusal \in [Queries -> {"none", "rebuildable", "oversized"}]
    /\ cpu \subseteq Queries /\ pins \subseteq Queries
    /\ capacity \in 0..MaxCapacity
SingleCpu == Cardinality(cpu) <= 1
PreparedStateRemainsCharged ==
    \A q \in Queries : phase[q] \in {"preparing", "prepared", "executing"}
        => memory[q] >= PreparationBytes
WaitingHoldsNoExecutionOrPin ==
    \A q \in Queries : phase[q] \in {"prepared", "retrying", "waiting"}
        => q \notin cpu /\ q \notin pins
NoImpossibleSelfWait ==
    \A q \in Queries : phase[q] = "waiting"
        => checkedMinimum[q] <= checkedCapacity[q]
QueuedPreparationReleasesMemory ==
    \A q \in Queries : phase[q] \in {"retrying", "waiting"} => memory[q] = 0
TerminalReleasesOwnership ==
    \A q \in Queries : phase[q] = "done"
        => memory[q] = 0 /\ q \notin cpu /\ q \notin pins
ShrunkWaiterCanTerminate ==
    \A q \in Queries : phase[q] \in {"retrying", "waiting"} /\ memory[q] + execution[q] > capacity
        => ENABLED AdmitExecution(q)
\* Expected counterexamples witness both outcomes after a hard refusal.
HardRefusalRecoveryUnreachable ==
    ~(\E q \in Queries : hardRefusal[q] = "rebuildable" /\ phase[q] = "executing"
        /\ checkedCapacity[q] = execution[q] /\ checkedMinimum[q] = execution[q])
HardRefusalFailureUnreachable ==
    ~(\E q \in Queries : hardRefusal[q] = "oversized" /\ phase[q] = "done"
        /\ checkedMinimum[q] = execution[q] /\ execution[q] > checkedCapacity[q])
AdmissionDoesNotOvercommit ==
    [][TotalMemory' > TotalMemory => TotalMemory' <= capacity']_vars
=============================================================================

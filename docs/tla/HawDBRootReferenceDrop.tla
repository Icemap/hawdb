---------------- MODULE HawDBRootReferenceDrop ----------------
EXTENDS Integers, FiniteSets
CONSTANT DropNonfinalOffGate
VARIABLES refs, stage, registry, native
Actors == {"a","b"}
vars == <<refs,stage,registry,native>>
Count == Cardinality(refs) + (IF registry="empty" THEN 0 ELSE 1)
Users == {actor \in refs: stage[actor]="idle"}
Init == /\ refs=Actors /\ stage=[actor \in Actors |-> "idle"]
 /\ registry="ready" /\ native=TRUE
Nonfinal(actor) == /\ actor \in refs /\ stage[actor]="idle"
 /\ registry="ready" /\ Count>2
 /\ refs'=(IF DropNonfinalOffGate THEN refs ELSE refs \ {actor})
 /\ stage'=[stage EXCEPT ![actor]=(IF DropNonfinalOffGate THEN "checked" ELSE "done")]
 /\ UNCHANGED <<registry,native>>
OffGateDrop(actor) == /\ actor \in refs /\ stage[actor]="checked"
 /\ refs'=refs \ {actor} /\ stage'=[stage EXCEPT ![actor]="done"]
 /\ UNCHANGED <<registry,native>>
Retire(actor) == /\ actor \in refs /\ stage[actor]="idle"
 /\ registry="ready" /\ Count=2
 /\ registry'="retiring" /\ stage'=[stage EXCEPT ![actor]="retiring"]
 /\ UNCHANGED <<refs,native>>
CloseNative == /\ registry="retiring" /\ native /\ native'=FALSE
 /\ UNCHANGED <<refs,stage,registry>>
Unregister(actor) == /\ registry="retiring" /\ ~native /\ stage[actor]="retiring"
 /\ refs'=refs \ {actor} /\ stage'=[stage EXCEPT ![actor]="done"]
 /\ registry'="empty" /\ UNCHANGED native
Next == (\E actor \in Actors: Nonfinal(actor) \/ OffGateDrop(actor)
 \/ Retire(actor) \/ Unregister(actor)) \/ CloseNative
TypeOK == /\ refs \subseteq Actors
 /\ stage \in [Actors -> {"idle","checked","retiring","done"}]
 /\ registry \in {"empty","ready","retiring"} /\ native \in BOOLEAN
ReadyHasExternalReference == registry="ready" => refs#{}
UsersKeepNative == Users#{} => native
EmptyHasNoResources == registry="empty" => (~native /\ refs={})
Spec == Init /\ [][Next]_vars
=============================================================================

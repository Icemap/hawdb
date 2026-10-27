-------------------- MODULE HawDBAutocommitScope --------------------
EXTENDS Naturals, FiniteSets

CONSTANTS Writers, Keys, MaxChanges, ProtectScope, SnapshotAfterLocks, ValidateSchema
VARIABLES phase, scope, held, preparedSchema, snapshot, versions, schema, changes
vars == <<phase, scope, held, preparedSchema, snapshot, versions, schema, changes>>

Init ==
    /\ phase = [w \in Writers |-> "idle"]
    /\ scope = [w \in Writers |-> {}]
    /\ held = [w \in Writers |-> {}]
    /\ preparedSchema = [w \in Writers |-> 0]
    /\ snapshot = [w \in Writers |-> [k \in Keys |-> 0]]
    /\ versions = [k \in Keys |-> 0]
    /\ schema = 0 /\ changes = 0

Prepare(w, keys) ==
    /\ phase[w] = "idle"
    /\ phase' = [phase EXCEPT ![w] = "waiting"]
    /\ scope' = [scope EXCEPT ![w] = keys]
    /\ preparedSchema' = [preparedSchema EXCEPT ![w] = schema]
    /\ snapshot' = [snapshot EXCEPT ![w] = versions]
    /\ UNCHANGED <<held, versions, schema, changes>>

Acquire(w) ==
    /\ phase[w] = "waiting"
    /\ \A other \in Writers \ {w} : held[other] \intersect scope[w] = {}
    /\ phase' = [phase EXCEPT ![w] = "locked"]
    /\ held' = [held EXCEPT ![w] = IF ProtectScope THEN scope[w] ELSE {}]
    /\ UNCHANGED <<scope, preparedSchema, snapshot, versions, schema, changes>>

Stage(w) ==
    /\ phase[w] = "locked"
    /\ phase' = [phase EXCEPT ![w] =
        IF ValidateSchema /\ preparedSchema[w] # schema THEN "refused" ELSE "staged"]
    /\ snapshot' = IF SnapshotAfterLocks THEN [snapshot EXCEPT ![w] = versions] ELSE snapshot
    /\ UNCHANGED <<scope, held, preparedSchema, versions, schema, changes>>

Commit(w, key) ==
    /\ phase[w] = "staged" /\ key \in scope[w] /\ changes < MaxChanges
    /\ versions' = [versions EXCEPT ![key] = @ + 1]
    /\ changes' = changes + 1
    /\ phase' = [phase EXCEPT ![w] = "done"]
    /\ held' = [held EXCEPT ![w] = {}]
    /\ UNCHANGED <<scope, preparedSchema, snapshot, schema>>

Retire(w) ==
    /\ phase[w] \in {"waiting", "locked", "staged", "refused"}
    /\ phase' = [phase EXCEPT ![w] = "done"]
    /\ held' = [held EXCEPT ![w] = {}]
    /\ UNCHANGED <<scope, preparedSchema, snapshot, versions, schema, changes>>

OtherWrite(key) ==
    /\ changes < MaxChanges /\ \A w \in Writers : key \notin held[w]
    /\ versions' = [versions EXCEPT ![key] = @ + 1]
    /\ changes' = changes + 1
    /\ UNCHANGED <<phase, scope, held, preparedSchema, snapshot, schema>>

SchemaChange ==
    /\ schema = 0 /\ \A w \in Writers : held[w] = {}
    /\ schema' = 1
    /\ UNCHANGED <<phase, scope, held, preparedSchema, snapshot, versions, changes>>

Next ==
    \/ \E w \in Writers, keys \in (SUBSET Keys) \ {{}} : Prepare(w, keys)
    \/ \E w \in Writers : Acquire(w) \/ Stage(w) \/ Retire(w)
    \/ \E w \in Writers, key \in Keys : Commit(w, key)
    \/ \E key \in Keys : OtherWrite(key)
    \/ SchemaChange

TypeOK ==
    /\ phase \in [Writers -> {"idle", "waiting", "locked", "staged", "refused", "done"}]
    /\ scope \in [Writers -> SUBSET Keys] /\ held \in [Writers -> SUBSET Keys]
    /\ preparedSchema \in [Writers -> 0..1] /\ schema \in 0..1
    /\ versions \in [Keys -> 0..MaxChanges]
    /\ snapshot \in [Writers -> [Keys -> 0..MaxChanges]]
    /\ changes \in 0..MaxChanges
StablePredicateRead ==
    \A w \in Writers : phase[w] = "staged" =>
        \A k \in scope[w] : snapshot[w][k] = versions[k]
CompatiblePreparation ==
    \A w \in Writers : phase[w] = "staged" => preparedSchema[w] = schema
ScopesRemainHeld ==
    \A w \in Writers : phase[w] \in {"locked", "staged"} => held[w] = scope[w]
=============================================================================

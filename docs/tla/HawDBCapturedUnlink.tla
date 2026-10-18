---------------- MODULE HawDBCapturedUnlink ----------------
EXTENDS Integers
CONSTANTS DelayedUnlink, PathUnlink, RebindHeldHandle
VARIABLE s
vars == <<s>>
\* The unlink caller already owns publication exclusion and has proved that
\* no logical pin needs this name. An idle cache/native reader can retain the
\* old inode; that handle is not permission to keep the obsolete name visible.
Init == s = [phase |-> "replace", named |-> "old", foreign |-> "foreign",
             held |-> TRUE, contents |-> "old", ack |-> FALSE, pending |-> FALSE]
ReplacePath == /\ s.phase = "replace"
               /\ s' = [s EXCEPT !.phase = "unlink"]
\* Replacement changes the pathname's target, never the captured source A.
Unlink == /\ s.phase = "unlink"
 /\ s' = [s EXCEPT !.phase = "recreate", !.ack = TRUE,
    !.named = IF PathUnlink \/ (DelayedUnlink /\ s.held) THEN @ ELSE "absent",
    !.foreign = IF PathUnlink THEN "absent" ELSE @,
    !.pending = DelayedUnlink /\ ~PathUnlink /\ s.held]
Recreate == /\ s.phase = "recreate" /\ s.named = "absent"
 /\ s' = [s EXCEPT !.named = "new", !.phase = "drop",
                  !.contents = IF RebindHeldHandle THEN "new" ELSE @]
DropHeld == /\ s.held /\ s.phase \in {"recreate", "drop"}
 /\ s' = [s EXCEPT !.held = FALSE, !.phase = "done",
                  !.named = IF s.pending THEN "absent" ELSE @, !.pending = FALSE]
Next == ReplacePath \/ Unlink \/ Recreate \/ DropHeld
TypeOK == /\ s.phase \in {"replace", "unlink", "recreate", "drop", "done"}
 /\ s.named \in {"old", "absent", "new"} /\ s.foreign \in {"foreign", "absent"}
 /\ s.held \in BOOLEAN /\ s.ack \in BOOLEAN /\ s.pending \in BOOLEAN
 /\ s.contents \in {"old", "new"}
AckRemovesOldName == s.ack => s.named # "old"
ForeignNamespaceUntouched == s.foreign = "foreign"
OriginalHandleImmutable == s.held => s.contents = "old"
Spec == Init /\ [][Next]_vars
=================================================================

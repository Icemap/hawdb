---------------- MODULE HawDBCapturedScratch ----------------
EXTENDS Integers
CONSTANTS PathForCreate, PathForData, PathForRetry,
          KeepExecutionInDebt, ForgetUncertain, RefundBeforeClose
VARIABLE s
vars == <<s>>
\* A retained physical A and an independently replaceable diagnostic label.
\* The caller excludes private-name rebinding. Published data and persisted
\* incarnations are outside this bounded scratch lifecycle refinement.
Init == s = [phase |-> "admit", label |-> "A", owner |-> FALSE,
 meta |-> FALSE, parent |-> FALSE, child |-> FALSE, directory |-> FALSE,
 known |-> FALSE, files |-> 0, execution |-> TRUE, foreignIO |-> FALSE]
ReplaceLabel == /\ s.label = "A" /\ s' = [s EXCEPT !.label = "B"]
Admit == /\ s.phase = "admit"
 /\ s' = [s EXCEPT !.phase = "create", !.owner = TRUE, !.meta = TRUE, !.parent = TRUE]
Create == /\ s.phase = "create"
 /\ s' = [s EXCEPT !.phase = "active", !.directory = TRUE, !.known = TRUE,
                   !.child = TRUE, !.foreignIO = @ \/ (PathForCreate /\ s.label = "B")]
UnknownCreate ==
 /\ s.phase = "create"
 /\ \E effect \in BOOLEAN:
      s' = [s EXCEPT !.phase = "debt", !.directory = effect, !.known = FALSE,
               !.owner = ~ForgetUncertain, !.execution = KeepExecutionInDebt,
               !.foreignIO = @ \/ (PathForCreate /\ s.label = "B")]
DataIO == /\ s.phase = "active" /\ s.files < 2
 /\ s' = [s EXCEPT !.files = @ + 1,
                   !.foreignIO = @ \/ (PathForData /\ s.label = "B")]
Defer == /\ s.phase = "active"
 /\ s' = [s EXCEPT !.phase = "debt", !.execution = KeepExecutionInDebt]
Retry == /\ s.phase = "debt" /\ s.known
 /\ s' = [s EXCEPT !.phase = "cleanup", !.execution = TRUE,
                   !.foreignIO = @ \/ (PathForRetry /\ s.label = "B")]
DeleteFile == /\ s.phase = "cleanup" /\ s.files > 0
 /\ s' = [s EXCEPT !.files = @ - 1,
                   !.foreignIO = @ \/ (PathForRetry /\ s.label = "B")]
CloseChild == /\ s.phase = "cleanup" /\ s.files = 0 /\ s.child
 /\ s' = [s EXCEPT !.child = FALSE]
RemoveDirectory == /\ s.phase = "cleanup" /\ s.files = 0 /\ ~s.child
 /\ s' = [s EXCEPT !.directory = FALSE, !.phase = "close"]
CloseParent == /\ s.phase = "close" /\ s.parent
 /\ s' = [s EXCEPT !.parent = FALSE, !.phase = "refund"]
Refund == /\ s.meta /\ (s.phase = "refund" \/ RefundBeforeClose)
 /\ s' = [s EXCEPT !.meta = FALSE, !.owner = FALSE, !.execution = FALSE, !.phase = "done"]
Next == ReplaceLabel \/ Admit \/ Create \/ UnknownCreate \/ DataIO \/ Defer
 \/ Retry \/ DeleteFile \/ CloseChild \/ RemoveDirectory \/ CloseParent \/ Refund
TypeOK == /\ s.phase \in {"admit", "create", "active", "debt", "cleanup", "close", "refund", "done"}
 /\ s.label \in {"A", "B"} /\ s.files \in 0..2
 /\ \A field \in {"owner", "meta", "parent", "child", "directory", "known", "execution", "foreignIO"}: s[field] \in BOOLEAN
ForeignNamespaceUntouched == ~s.foreignIO
DebtHasNoExecution == s.phase = "debt" => ~s.execution
OwnedDirectoryRetained == s.directory => s.owner /\ s.parent /\ s.meta
NativeCustodyFunded == (s.parent \/ s.child) => s.meta
Spec == Init /\ [][Next]_vars
=================================================================

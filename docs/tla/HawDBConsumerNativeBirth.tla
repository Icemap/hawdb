---------------- MODULE HawDBConsumerNativeBirth ----------------
EXTENDS Naturals, TLC
CONSTANTS SkipParentBarrier, AdoptMarkerOnRecovery, ForgetDebtOnClose
VARIABLES s
vars == <<s>>
Init == \E foreign \in BOOLEAN :
    s = [phase |-> "reserved", source |-> "Reserved", alive |-> TRUE,
         crashed |-> FALSE, generation |-> foreign, marker |-> foreign,
         actualBirth |-> FALSE, fileSync |-> foreign, childSync |-> foreign,
         parentSync |-> foreign, exposed |-> FALSE]
Prepare == /\ s.alive /\ s.phase = "reserved"
           /\ s' = [s EXCEPT !.phase = "prepared", !.source = "Pending"]
Create == /\ s.alive /\ s.phase = "prepared"
          /\ IF s.generation THEN s' = [s EXCEPT !.phase = "refused"]
             ELSE s' = [s EXCEPT !.phase = "created", !.generation = TRUE,
                                !.actualBirth = TRUE]
Open == /\ s.alive /\ s.phase = "created"
        /\ s' = [s EXCEPT !.phase = "opened"]
WriteMarker == /\ s.alive /\ s.phase = "opened"
               /\ s' = [s EXCEPT !.phase = "marked", !.marker = TRUE]
SyncFile == /\ s.alive /\ s.phase = "marked"
            /\ s' = [s EXCEPT !.phase = "fileSynced", !.fileSync = TRUE]
SyncChild == /\ s.alive /\ s.phase = "fileSynced"
             /\ s' = [s EXCEPT !.phase = "childSynced", !.childSync = TRUE]
SyncParent == /\ s.alive /\ s.phase = "childSynced"
              /\ s' = [s EXCEPT !.phase = "parentSynced", !.parentSync = TRUE]
Record == /\ s.alive
          /\ (s.phase = "parentSynced" \/ (SkipParentBarrier /\ s.phase = "childSynced"))
          /\ s' = [s EXCEPT !.phase = "recorded", !.source = "Recorded"]
Expose == /\ s.alive /\ s.phase = "recorded"
          /\ s' = [s EXCEPT !.phase = "exposed", !.exposed = TRUE]
Close == /\ s.alive
         /\ s' = [s EXCEPT !.phase = "closed", !.alive = FALSE, !.exposed = FALSE,
                 !.source = IF ForgetDebtOnClose THEN "Absent" ELSE @]
Crash == /\ s.alive /\ ~s.crashed
         /\ \E generation \in (IF s.parentSync THEN {s.generation} ELSE {FALSE, s.generation}),
               marker \in (IF s.fileSync /\ s.childSync THEN {s.marker} ELSE {FALSE, s.marker}) :
               s' = [s EXCEPT !.phase = "crashed", !.alive = FALSE,
                     !.crashed = TRUE, !.exposed = FALSE,
                     !.generation = generation, !.marker = marker]
Adopt == /\ AdoptMarkerOnRecovery /\ s.phase = "crashed" /\ s.generation /\ s.marker
         /\ s' = [s EXCEPT !.phase = "adopted", !.exposed = TRUE]
Next == Prepare \/ Create \/ Open \/ WriteMarker \/ SyncFile \/ SyncChild \/
        SyncParent \/ Record \/ Expose \/ Close \/ Crash \/ Adopt
TypeOK == /\ s.phase \in {"reserved", "prepared", "created", "opened", "marked",
                "fileSynced", "childSynced", "parentSynced", "recorded", "exposed",
                "refused", "closed", "crashed", "adopted"}
          /\ s.source \in {"Reserved", "Pending", "Recorded", "Absent"}
          /\ \A f \in {"alive", "crashed", "generation", "marker", "actualBirth",
                       "fileSync", "childSync", "parentSync", "exposed"} : s[f] \in BOOLEAN
NativeDebtRooted == s.actualBirth /\ s.generation => s.source \in {"Pending", "Recorded"}
RecordedBirthSurvives == s.source = "Recorded" => s.actualBirth /\ s.generation /\ s.marker
ProducerHasActualBirth == s.exposed => s.actualBirth /\ s.source = "Recorded"
Spec == Init /\ [][Next]_vars
=================================================================

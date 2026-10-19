---------------------- MODULE HawDBCapturedWalWriter ----------------------
EXTENDS Naturals, TLC
CONSTANTS PathAppend, PathBarrier, DropCapture, SkipFence, NoAppendSemantics, DropUncertainFile
VARIABLES phase, fileName, directoryName, held, bytes, prefixIntact,
          fileBarrier, directoryBarrier, observedFile, observedDirectory
vars == <<phase, fileName, directoryName, held, bytes, prefixIntact,
          fileBarrier, directoryBarrier, observedFile, observedDirectory>>
Init == /\ phase = "replay" /\ fileName = 1 /\ directoryName = 1 /\ held = 1
        /\ bytes = [i \in {1, 2} |-> 0] /\ prefixIntact = TRUE
        /\ fileBarrier = 0 /\ directoryBarrier = 0
        /\ observedFile = 0 /\ observedDirectory = 0
ReplaceFile == /\ fileName = 1 /\ fileName' = 2
               /\ UNCHANGED <<phase, directoryName, held, bytes, prefixIntact,
                    fileBarrier, directoryBarrier, observedFile, observedDirectory>>
ReplaceDirectory == /\ directoryName = 1 /\ directoryName' = 2 /\ fileName' = 2
                    /\ UNCHANGED <<phase, held, bytes, prefixIntact,
                         fileBarrier, directoryBarrier, observedFile, observedDirectory>>
Replay == /\ phase = "replay" /\ phase' = "install"
          /\ UNCHANGED <<fileName, directoryName, held, bytes, prefixIntact,
               fileBarrier, directoryBarrier, observedFile, observedDirectory>>
Install == /\ phase = "install" /\ phase' = "admit"
           /\ held' = IF DropCapture THEN 0 ELSE held
           /\ UNCHANGED <<fileName, directoryName, bytes, prefixIntact,
                fileBarrier, directoryBarrier, observedFile, observedDirectory>>
Admit == /\ phase = "admit"
         /\ phase' = IF fileName = held /\ directoryName = 1 THEN "write" ELSE "refused"
         /\ UNCHANGED <<fileName, directoryName, held, bytes, prefixIntact,
              fileBarrier, directoryBarrier, observedFile, observedDirectory>>
Append == /\ phase = "write" /\ phase' = "preSync"
          /\ bytes' = [bytes EXCEPT ![IF PathAppend THEN fileName ELSE held] = @ + 1]
          /\ prefixIntact' = ~NoAppendSemantics
          /\ UNCHANGED <<fileName, directoryName, held,
               fileBarrier, directoryBarrier, observedFile, observedDirectory>>
PreSync == /\ phase = "preSync"
           /\ phase' = IF fileName = held /\ directoryName = 1 THEN "fileSync" ELSE "uncertain"
           /\ UNCHANGED <<fileName, directoryName, held, bytes, prefixIntact,
                fileBarrier, directoryBarrier, observedFile, observedDirectory>>
SyncFile == /\ phase = "fileSync" /\ phase' = "directorySync" /\ fileBarrier' = held
            /\ UNCHANGED <<fileName, directoryName, held, bytes, prefixIntact,
                 directoryBarrier, observedFile, observedDirectory>>
SyncDirectory == /\ phase = "directorySync" /\ phase' = "fence"
                 /\ directoryBarrier' = IF PathBarrier THEN directoryName ELSE 1
                 /\ UNCHANGED <<fileName, directoryName, held, bytes, prefixIntact,
                      fileBarrier, observedFile, observedDirectory>>
Fence == /\ phase = "fence" /\ observedFile' = fileName /\ observedDirectory' = directoryName
         /\ phase' = IF SkipFence \/ (fileName = held /\ directoryName = 1) THEN "acked" ELSE "uncertain"
         /\ UNCHANGED <<fileName, directoryName, held, bytes, prefixIntact,
              fileBarrier, directoryBarrier>>
LoseUncertainFile == /\ DropUncertainFile /\ phase = "uncertain" /\ held = 1
                     /\ held' = 0
                     /\ UNCHANGED <<phase, fileName, directoryName, bytes, prefixIntact,
                          fileBarrier, directoryBarrier, observedFile, observedDirectory>>
Next == ReplaceFile \/ ReplaceDirectory \/ Replay \/ Install \/ Admit \/ Append
        \/ PreSync \/ SyncFile \/ SyncDirectory \/ Fence \/ LoseUncertainFile
TypeOK == /\ phase \in {"replay", "install", "admit", "write", "preSync", "fileSync",
                       "directorySync", "fence", "acked", "refused", "uncertain"}
          /\ fileName \in {1, 2} /\ directoryName \in {1, 2} /\ held \in {0, 1}
          /\ bytes \in [{1, 2} -> Nat] /\ prefixIntact \in BOOLEAN
          /\ fileBarrier \in {0, 1, 2} /\ directoryBarrier \in {0, 1, 2}
          /\ observedFile \in {0, 1, 2} /\ observedDirectory \in {0, 1, 2}
HeldForActiveGeneration == held = 1
OriginalPrefix == prefixIntact
ForeignUntouched == bytes[2] = 0
FileBarrierFromCapture == fileBarrier \in {0, 1}
DirectoryBarrierFromCapture == directoryBarrier \in {0, 1}
AcknowledgedOriginal == phase = "acked" => /\ bytes[1] = 1 /\ fileBarrier = 1
    /\ directoryBarrier = 1 /\ observedFile = 1 /\ observedDirectory = 1
RefusedWithoutMutation == phase = "refused" => bytes[1] = 0 /\ bytes[2] = 0
\* Namespace replacement remains enabled after acknowledgment. Observations
\* do not establish exclusion or Consumer birth/publication authority.
Spec == Init /\ [][Next]_vars
=============================================================================

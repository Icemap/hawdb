---------------- MODULE HawDBVectorIndexReadDomain ----------------
EXTENDS Naturals
CONSTANTS SkipIdentityFence, SkipEpochFence, SkipSchemaFence,
          SkipPublicationFence, RetargetOldRead, NoValue
VARIABLE s
vars == <<s>>
None == NoValue
Sources == [database : 1..2, branch : 1..2, epoch : 0..2, schema : 0..1]
InitialSource == [database |-> 1, branch |-> 1, epoch |-> 0, schema |-> 0]

\* A sealed closure proves its claimed binding, not agreement with an arbitrary
\* canonical snapshot. Include mismatched claims to qualify admission itself.
Init == \E stored \in Sources:
    s = [current |-> InitialSource, roots |-> [g \in 1..2 |->
            IF g = 1 THEN stored ELSE None],
         selected |-> 1, candidate |-> None, snapshot |-> None,
         read |-> None, publication |-> None]

Advance == /\ s.current.epoch < 2
           /\ \E schema \in s.current.schema..1:
                s' = [s EXCEPT !.current.epoch = @ + 1,
                               !.current.schema = schema]
SwitchDatabase == /\ s.current.database = 1
                  /\ s' = [s EXCEPT !.current.database = 2]
SwitchBranch == /\ s.current.branch = 1
                /\ s' = [s EXCEPT !.current.branch = 2]
Capture == s' = [s EXCEPT !.snapshot = s.current]
Prepare == /\ s.selected = 1 /\ s.candidate = None
           /\ s' = [s EXCEPT !.candidate = s.current]
Cancel == /\ s.candidate # None
          /\ s' = [s EXCEPT !.candidate = None]
Select == /\ s.selected = 1 /\ s.candidate # None
          /\ (SkipPublicationFence \/ s.candidate = s.current)
          /\ s' = [s EXCEPT !.roots[2] = s.candidate, !.selected = 2,
                           !.publication = [source |-> s.current,
                                             built |-> s.candidate],
                           !.candidate = None,
                           !.read = IF RetargetOldRead /\ s.read # None
                                    THEN [s.read EXCEPT !.generation = 2]
                                    ELSE @]
Matches(source, index) ==
    /\ (SkipIdentityFence \/
        (source.database = index.database /\ source.branch = index.branch))
    /\ (SkipEpochFence \/ source.epoch = index.epoch)
    /\ (SkipSchemaFence \/ source.schema = index.schema)
BindRead == /\ s.snapshot # None /\ s.read = None
            /\ Matches(s.snapshot, s.roots[s.selected])
            /\ s' = [s EXCEPT !.read = [source |-> s.snapshot,
                                        generation |-> s.selected]]
CloseRead == /\ s.read # None /\ s' = [s EXCEPT !.read = None]
Next == Advance \/ SwitchDatabase \/ SwitchBranch \/ Capture \/ Prepare \/
        Cancel \/ Select \/ BindRead \/ CloseRead
TypeOK == /\ s.current \in Sources
          /\ s.roots \in [1..2 -> Sources \cup {None}]
          /\ s.selected \in 1..2 /\ s.roots[s.selected] # None
          /\ s.candidate \in Sources \cup {None}
          /\ s.snapshot \in Sources \cup {None}
          /\ s.read \in [source : Sources, generation : 1..2] \cup {None}
          /\ s.publication \in [source : Sources, built : Sources] \cup {None}
ReadUsesOneDomain == IF s.read = None THEN TRUE
                    ELSE s.read.source = s.roots[s.read.generation]
PublicationUsesCapturedSource == IF s.publication = None THEN TRUE
                                ELSE s.publication.source = s.publication.built
Spec == Init /\ [][Next]_vars
=================================================================

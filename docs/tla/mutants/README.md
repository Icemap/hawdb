# Storage and Query Memory TLA+ Negative Controls

These configurations enable one deliberately unsafe transition in a storage
or query-memory model. They are not production specifications and must never enter the
positive `storage_models` suite. `scripts/check-storage-tla.sh --check-mutants`
runs each configuration and succeeds only when TLC reports the invariant named
in `mutants.txt`.

The negative controls prove that the bounded models exercise these failures:

- visibility before WAL durability;
- duplicate or out-of-order partition appends;
- partial batch recovery;
- manifest publication before segment durability;
- reclamation of a reader-pinned generation;
- partial publication of a mixed graph/row/append transaction;
- child admission that exceeds the query root;
- releasing a child reservation while a lease still owns its capacity;
- invoking a public request consumer before complete result validation.

The automatic-checkpoint controls also reject selection of a synchronized
prefix after foreground writes advanced beyond it. That prefix must resume
replay on the same pinned base before entering the selection gate. The
`SelectedContainsCapturedPrefix` invariant reports the stale-selection control;
the ordinary positive model additionally checks `CandidateBaseStaysPinned`.

The per-key MVCC controls additionally reject skipped validation, either missing
broad-barrier direction, premature history cleanup, ignored retained source
snapshots, index reset with live transactions, unsafe current-epoch compaction,
and publication before sync. See
[the model proof and scope](../MVCC_VALIDATION_PROOF.md).

The governed-writer progress control rejects younger admission past an older
full-capacity writer. Its proof also records a separate temporal starvation
counterexample when the safety assertion is omitted; see
[the progress proof](../GOVERNED_WRITER_PROGRESS_PROOF.md).

The constrained-insert controls check omission of unique stamps and spurious
conflicts from NULL unique values or shared foreign-key parent reads. See the
[constrained-insert proof](../CONSTRAINED_INSERT_MVCC_PROOF.md).

The primary-key predicate controls reject extraction through an OR branch and
omission of absent-row intents; see the
[predicate replay proof](../PRIMARY_KEY_PREDICATE_MVCC_PROOF.md).

The aged-writer bypass control rejects a younger foreground grant after the
background large waiter ages; temporal starvation controls and witnesses are
described in the [aging refinement](../AGED_WRITER_PROGRESS_PROOF.md).

The governed-conflict-retry control rejects publication from a conflicting
attempt. Its [proof](../GOVERNED_CONFLICT_RETRY_PROOF.md) also records the
non-escalating retry starvation control and reachable conflict/retry witnesses.

The search mutation publication controls reject global-ID masking of replacement
versions, stale selector publication, publication before artifact durability,
pinned closure reclamation and orphaned targets after compaction. Two false
invariants witness repeated replacement and compaction reachability. See the
[search mutation proof boundary](../SEARCH_MUTATION_PUBLICATION_PROOF.md).

The branch lifecycle controls reject early head publication, descendant writes
to the main head, omission of creating roots, stale mark/sweep decisions,
deletion under lease, same-branch writer sharing, unsynchronized forks, and
split schema/data publication. Separate false invariants witness create
completion/abort and delete recovery after crash, independently open branches,
nested forks, parent deletion with a surviving descendant, and acknowledged
relaxed-write loss. See the
[branch lifecycle proof boundary](../BRANCH_LIFECYCLE_PROOF.md).

The constraint-preserving update controls reject omitting unique or FK columns
from eligibility; see the [projection proof](../CONSTRAINT_PRESERVING_UPDATE_MVCC_PROOF.md).

The shared-reader capacity controls reject returning backing or ledger capacity
before its physical owners are gone, allocating an uncharged deep clone, and
retaining execution slots after producer closure. The model separates query
refund from dropping the final ledger reference; see the
[resource protocol and proof limits](../SHARED_READER_CAPACITY_PROOF.md).

Root-process controls reject early unregister/native close, losing ownership
when a producer closes, ignored reads, early mutation-slot release, retained
producer slots and process-local-only exclusion. The reference-drop control
rejects count checks separated from nonfinal decrements. The successor witness
is separate from these mutants; see [the proof boundary](../ROOT_PROCESS_CUSTODY_PROOF.md).

`HawDBPublicationRootCustody` has four independent process-custody controls:
`DuplicateProcessHandle`, `IgnorePublisherDuringInit`, `UnregisterEarly`, and
`ReopenOnPromotion`.
Each expects an invariant violation, rather than a parsing error or deadlock.
See [the native correspondence](../SOURCE_ROOT_CUSTODY_DESIGN.md#publication-and-retained-root-process-lock-coordination).

`HawDBSourceRootCustody` now includes a temporary pending capture before short
publication unlock. `IgnorePendingCapture` must violate SourcesLive while the
closure is still opening. `UnlockBeforePin` now exercises release before any
pending registration; existing once/retry controls still honor pending capture
and independently forget installed exact pins.

`HawDBPublicationRootCustodyCloseWithoutUnlock` retains a fork-inherited open
description after original ownership closes, violating NoUnownedLocalLock. The
positive protocol explicitly unlocks successful ownership before native close.
This control is independent of duplicate-handle and early-unregister controls.

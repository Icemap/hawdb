# Prepared catalog publication

This is one storage primitive for the Consumer installed-activation design,
not the Consumer format, read-view exchange, receipt or readiness contract.
It reuses `ProjectionGenerationStore` and its captured native namespace.

## Ownership and transitions

`prepare_publication` runs outside SourceOwner. It checks the sealed manifest,
expected head and non-regressing source watermark, stabilizes the old recovery
root, encodes and synchronizes a private selector, and reserves the logical
projection/owner in the catalog's shared process state. The prepared owner
retains the actual native directory capture and the old head file. Retaining
that old file prevents its physical file ID being reused during comparison.
The candidate pin retains the new generation. No additional directory FD is
opened to turn the borrowed native control candidate into an owned candidate.

The source coordinator separately validates source dependencies and known
source durability under its exclusive database borrow. Selection uses a
nonblocking catalog mutex acquisition, compares the current native head identity
to the retained witness, marks the operation attempted before native selection,
and returns pending custody. It neither encodes the manifest nor synchronizes
files/directories. Native metadata/open/rename calls can block; no hard latency
bound is claimed. The facade clones the catalog label/Arc and validates source
metadata under the borrow; resource and latency qualification remain required.

The pending lifetime borrows the prepared owner, not the database. After the
source fence is released, `finish` completes the selected file and directory
barriers, removes the matching reservation, closes its witness outside the
catalog mutex, and returns the report. A live pending operation excludes
publication and outcome resolution by another operation for the same owner.
Other owners remain usable. General catalog operations still use a blocking
mutex; only selection promises immediate refusal when that mutex is busy.

A pre-selection rejection preserves the prepared value for disposal outside
SourceOwner. A native attempt permanently revokes selection retries. Dropping
pending custody alone leaves the original owner attempted; that owner can
resolve its own outcome. Dropping an attempted owner marks the slot uncertain.
Only the same sealed generation can resolve an abandoned uncertain operation.
Resolution stabilizes a selected complete generation or confirms the original
native head/absence. Unknown heads, corruption and failed barriers preserve
uncertainty and evidence. A confirmed non-selection requires a new preparation.

While any reservation exists, GC defers that owner's generations, including the
old durable head and new candidate, before synchronization or unlink. This is
conservative, owner-wide retention. Status reports pending/uncertain separately
from published. `open_active` remains a raw selected-catalog reader: observing a
selected generation is not a durable acknowledgement or Consumer readiness.

## Authority and recovery boundary

The reservation and nonce are process-local. The host must retain the actual
interprocess target publication lease across prepare, source validation,
selection, read-view exchange, barriers and resolution. The catalog does not
mint this lease or establish source correctness on its own. Checked nonce and
epoch exhaustion refuse before new selector creation. Native identity compares
assume immutable selectors and cooperative publication exclusion; they are not
persisted logical incarnation or arbitrary same-inode mutation detection.

The shared registry must retire before its final captured directory identities
close. Slots retain files, not a Store/Shared reference cycle. Prepared fields
close the control, then reservation, then candidate/store custody. Closing a
reservation removes its witness under the structural mutex and destroys the
witness after releasing that mutex. Uncertain slots retain their witnesses
until resolution or final catalog retirement. No destructor unlinks uncertain
files or reports success. Catalog recovery on reopen continues to select and
validate the durable head and complete immutable dependencies; the in-memory
slot is not a persisted operation receipt.

The complete installed Consumer implementation still requires its actual
operation lease, immutable generation-backed read view, selector and receipt
atomicity, logical incarnation, recovery-owned unresolved roots, admitted
bounded cleanup, full C..E source validation, Search hydration and Mem adoption.
This primitive must not bypass the existing registered Search root guard or
turn a resident index into an empty stand-in.

## Finite model correspondence

`HawDBPreparedCatalogPublication.tla` abstracts one prepared generation (2), its
old durable head (1), and a competing next generation (3). `Prepare`, `Select`,
`Finish`, `DropOwner`, `ResolveOwn`, `ResolveOther`, `NextPublication` and `Gc`
correspond to the custody transitions above. Selection includes a no-effect
failure; finishing/resolve separately cover durability. The model checks
exclusive ownership, no other-caller resolution of a live owner, no attempted
retry, acknowledgement after durability, selected/durable completeness, and
retention of both recovery roots while occupied.

The positive finite configuration has 26 distinct states (36 generated), depth
7. Five explicit negative configurations must violate their named invariant:

| Mutation | Required counterexample |
| --- | --- |
| EarlyRelease | ExclusiveOwner |
| ResolveLive | NoForeignResolution |
| AckEarly | DurableReceipt |
| IgnoreGcRoots | RecoveryRoots |
| RetryAttempt | NoRetry |

This is a finite state-machine check, not a refinement proof for Rust or native
syscalls, hardware synchronization, source epochs, Consumer readiness, memory
bounds, deadlock freedom or fairness. Existing captured-control/catalog models
and native physical crash images qualify additional publication/recovery
boundaries separately. Native regressions must exercise the same transitions,
including pending exclusion, dropped owner resolution, unavailable candidate,
head replacement with identical bytes, try-lock refusal, exhaustion, GC and
source dependency rejection.

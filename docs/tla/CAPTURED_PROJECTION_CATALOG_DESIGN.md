# Captured projection-generation catalog

The [MVCC publication specification](../specs/MVCC_VERSIONED_PUBLICATION_SPEC.md)
requires composition of the existing `ProjectionGenerationStore`, rather than
a parallel Consumer generation catalog. This change adopts captured native I/O
through that existing store. Its on-disk member/manifest/head formats and source
validation semantics are unchanged. Consumer generation-backed serving and
atomic view/operation-receipt activation remain subsequent integration work.

## One authority for every catalog phase

Opening the writable catalog creates its directories and synchronizes their
ancestry through the existing admitted project boundary. Directory existence
after an interrupted initialization is not a durability barrier. Constructor
path selection still requires the host's namespace/ownership discipline.
After capture, paths are labels for engine-generated components: head and
manifest reads, candidate creation/resumption/repair/append, seek-fence scans,
reader paging/scrub, reclamation discovery, retention checks, unlink and
directory synchronization all use held native directories.

The catalog retains three counted handles: the root and its `generations` and
`heads` directories. Each operation borrows fresh FD inventory from that same
descriptor domain. It does not create another project or allowance. Creation
is exclusive; resumption opens an existing writable regular file without
creation/truncation, validates its stream and repairs only a proven torn tail.
Append seeks to the validated end rather than using an append-only Windows
handle that lacks truncation authority. A changed data length after a partial or
unacknowledged append refuses further writes and sealing until reopening scans/
repairs the stream. A torn prefix is truncated; a complete unacknowledged record
is retained without duplication. Sealing checks the physical end as well as the
expected counters/digest, so ignoring an earlier append error cannot select a
manifest whose boundary excludes trailing data.

Resuming a complete unacknowledged record does not set the torn-tail flag.
Sealing synchronizes the complete data stream even in that case. Likewise an
already visible immutable manifest is synchronized, together with its containing
directory, before idempotent sealing returns. File existence or successful
decoding cannot stand in for those barriers after an interrupted seal. Receipt/
GC stabilization completes data and manifest barriers before the selected head's
barriers. Two additional materialized crash images lose all remaining uncovered
writes after resumed sealing and selection, then reopen and scrub the generation.

Head and manifest publication reuse the
[captured control candidate](CAPTURED_CONTROL_PUBLICATION_DESIGN.md). Writes
are sealed before the native relative selection; post-effect file/identity/
directory barriers precede successful return. A failed attempt retains its
candidate or selected-head evidence. Lost-response receipt resolution and
idempotent publication retry complete the current head's file/directory
barriers before returning a committed report. Resolution needs writable
storage; observing a matching head through a read-only catalog cannot certify
that an earlier rename survived a durability barrier.

GC stabilizes and validates the current selected manifest/data boundary and
head before retiring an inactive generation. Otherwise a failed head-directory
sync could leave visible E over durable D, let GC delete D, and recover a dangling
D selector after power loss. Missing active manifests fail closed and retain
their data. The new recovery regression injects failure after real selection,
then materializes/reopens physical crash images with every remaining
unsynchronized write lost, both before and after receipt resolution, idempotent
retry or GC. A visible head alone never authorizes old-root retirement.

One reclamation call retains the catalog mutex for its complete bounded scan.
Within that call, a successful selected-head stabilization covers subsequent
retirements for the same head, including abandoned unsealed data. The temporary
head set contains only completed barriers, is bounded by the file scan limit,
and is discarded before releasing the mutex. A later reclamation call must
establish the barriers again. This relies on the same host publication exclusion
and immutable-generation contract as the uncached path; it does not add an
interprocess lock. Active, pinned and unresolved publication roots still retain
their existing per-entry checks.

The synchronous catalog entry point still
performs preparation and barriers under its catalog mutex; this is not yet the
SourceOwner-free prepared/selection/pending-durability API required for Consumer
activation. The catalog mutex is not the graph publication fence or an
interprocess lease. The host must retain actual publication exclusion and
immutable private names/data, including against cooperative external processes.

## Physical catalog identity and reader admission

The process-local registry key is the tuple of all three captured native
directory identities. A retained handle prevents physical identity reuse while
that tuple is registered. Independent opens of the same physical catalog share
its writers/pins; replacing its old path with a byte-identical catalog creates
distinct authority. A sealed token from A cannot authorize publication in B,
even when every encoded manifest/member byte matches.

`ProjectionGenerationStore` declares its shared-registry Arc before its
captured-directory Arc. The final registry authority becomes non-upgradeable
before the last directory handles close; independent captures retain those
same physical IDs while any other store still holds the shared registry. Closing
the final handles first would permit an identity-reuse window before weak-key
retirement. This argument depends on Rust field destruction order.

This is an ephemeral physical key, not a persisted logical incarnation or
database/Consumer binding. Source instance/schema/mapping revision, C..E
continuity, target incarnation and operation identity still need their actual
typed protocols. No path, hash or physical ID substitutes for them.

Active-head lookup, manifest equality validation and reader-pin registration
now share one catalog critical section. Previously lookup could release the
catalog before pin registration: a publisher could select a new head and GC
could remove the old data before its reader became pinned. File construction
may occur after releasing the mutex because the pin is already installed;
construction failure releases that pin. The new deterministic regression stops
after native head lookup and checks the actual mutex before registration, then
verifies old data survives publication/GC and is reclaimed after reader closure.

Reclamation discovers and checks candidates through the same captured directories
that it unlinks. Its extra cursor entry establishes bounded backlog without
collecting an unbounded filename list. Active heads, writer roots and reader/
sealed-token pins remain roots under the catalog mutex. Candidate filenames
must match the decoded identity. Corrupt abandoned metadata is retained;
resource/I/O failures propagate rather than disappearing as an empty catalog.
No source-relative Search artifact/GC qualification follows from this catalog
implementation: those have a different manifest/closure and remain required.

## Finite model and native evidence obligations

`HawDBCapturedProjectionCatalog` models two physical catalogs, two generations,
path replacement, private/sealed candidate custody, publication, atomic reader
pinning, paging and GC. The positive configuration requires no foreign mutation,
complete active/candidate/pinned generations, source-relative reads and readable
selected data, including the root still covered by the durable head. Seven
generated one-constant controls exercise path-based build,
head publication, reads, GC discovery, split head/pin capture and path-keyed
foreign-token adoption, plus old-root retirement before a head barrier. Each
must produce its intended invariant violation.

The model assumes immutable generation contents and native identities held live
by their handles; it does not prove native syscall atomicity, persistent logical
incarnations, SHA collision resistance, allocation/RSS limits, deadlines,
fairness, full receipt recovery or hardware power-loss refinement. The native
capture/recorder and control-publication tests supply separate, narrower evidence.
No liveness theorem is claimed for blocking catalog mutex acquisition.

Native catalog regressions keep a writer and reader across complete root-path
reuse; build B with colliding logical generations/filenames; then append, seal,
publish, page, scrub and actually reclaim A's obsolete data while preserving A's
active/pinned data and every B byte. The same scenario runs with the recorder.
These post-open ancestor-move scenarios execute on Unix. Native Windows CI
`39101105952/117597375894` returned `AccessDenied (5)` for the three ancestor-move
fixtures with the catalog's read-directory descendants held open; the other 95
cases passed. No data-file write handle or byte-range process lock was needed
for that refusal. Directory READ_DATA/listing handles are a distinct observed
case from the earlier regular-file write-handle diagnostic.

Cross-platform constructor-boundary cases retain the chosen root continuously,
replace its label before child-directory hydration, and invoke the same native
`from_root`/catalog construction path used by `open`. They then exercise real A
publication, repair, pinned reads, reclamation and identical-copy token rejection
while preserving every B byte. Fixture seeding closes before starting this fresh
capture; it does not close/reacquire a chosen root or a live publication lease to
force movement. Windows separately must refuse a post-open move while retaining
all three handles, identities and bytes, then publish normally at its stationary
root. The original Unix post-open tests remain. No Windows movement capability
is inferred from Unix success or manufactured by releasing native custody.

Additional cases cover source-relative torn-tail repair, identical copied-token
rejection, FD refusal/refund, the head-to-pin critical section and real
lost-selection crash recovery. Windows must
execute these catalog tests in the existing native-contract step. Compile-only
checks and protocol-model success do not replace that execution.

# Captured Search reference integration (work in progress)

This worktree integrates the captured immutable-file/cache implementation with
actual Search artifact decoding. It is not qualified for publication yet. The
private admitted-root serving rejection remains mandatory. No Consumer selector,
source/target incarnation authority, installed view/receipt or Mem readiness is
created by this format.

## Reference representation

The proposed Search manifest V4 requires a fixed reference set per content
segment: descriptor, document, metadata, vector, layout, lexical manifest and
lexical payload, plus an optional RaBitQ reference whose presence must agree
with the other vector fields. Every mutation-run reference is mandatory too.
Each record stores a length and fixed 32-byte digest. Kind/version are fixed by
the wire contract to `CheckpointArtifact`/1, so reconstructing an
`ObjectReference` preserves the existing storage identity domain. There are no
per-digest owned strings or nested variable-length artifact maps.

`ObjectReferenceWriter` hashes the existing kind/version/declared-length prefix
and streamed payload. It retains no payload buffer. Overflow is rejected before
changing hash state; incomplete streams cannot finish. This is a byte identity,
not proof of producer origin, closure completeness, durability or authorization.
The controlled producer's streaming JSON encoder uses the same writer, without
allocating an additional complete mutation/layout buffer merely to hash it.

Lexical payload generation is recorded separately from the outer content and
versioned lexical-manifest name. A resident checkpoint may have different
counters. The lexical decoder must agree with that explicit payload generation
and length. Pins for the versioned manifest and actual payload are independent.
Treating these numeric namespaces as interchangeable would leave a GC hole.

## Captured reads and memory ownership

A pending capture already holds the actual native root/process custody and
protects the whole root while the closure is decoded. Every captured child now
receives its exact full producer reference and artifact-family pin before its
lazy file is constructed. Descriptor/layout/lexical envelopes, payload ranges,
vector construction and mutation stored terms must all use this source path.
Missing references cannot select native-ID/length validation or current-path
alias lookup. The ordinary standalone path remains a separate caller mode;
it is not permission to fall back when a captured reference is missing.

Logical file custody owns the exact pin and retained memory from the original
governor. Cache keys own admitted memory only, with no root or pin, preventing a
cache-to-root cycle. Readers retain no construction execution permit. Cold
reopens validate the complete managed object through the captured directory;
selected-range CRC checks and semantic decoders still run. Cache retirement must
close idle children before process unlock. The retained root outlives every
logical file/read/derived stored-term owner that needs it.

These statements describe intended ownership boundaries. Native reclamation,
allocation, cancellation, low-FD and same-governor tests must pass before this
integration can claim those properties. Ledger/host credits alone do not prove
physical heap reclamation or RSS bounds.

## Remaining producer and publication work

The streaming generation writer captures references before installation, and
both single and partitioned installers must verify the complete expected
reference after installing each new dependency. Unchanged references transfer
with the old immutable segment. This does not qualify flat-N metadata work,
writer fairness, physical I/O or latency gates.

The resident descriptor, document, sidecar and lexical checkpoint paths still
compute reference descriptions from materialized files. Those are **not**
qualified originating-output receipts. They must be replaced by references
retained from the actual writer-owned output handles/encoders.

The snapshot encoder now returns the domain-separated reference of the exact
encoded bytes written, alongside its existing raw SHA and length. It hashes the
same writes before installation; the compressed input handle remains owned
through envelope copying instead of reopening its temporary pathname. The
manifest preserves that snapshot receipt, and its vector count is checked against
the complete captured snapshot before authorizing absence-based reclamation.
This improves one output boundary; it does not qualify the other resident
outputs, Consumer activation, temporary-file cleanup or path-based publication. A current installed
pathname must not supply the expectation that authenticates that same output.
Native root custody and matching snapshot receipt must coordinate that producer
boundary; a self-consistent V4 table cannot mint it.

## Captured retention before either sweep

Production immediate and retry cleanup now require `VerifiedRetention`, borrowed
from the actual publication lease. The selected head, complete dependency graph,
semantic decoders, directory enumeration and unlink all use that captured native
source for projection artifacts and validation scratch. Body files, lexical
spill creation/reopen and deferred scratch cleanup now use the original held
parent/child directory capabilities. Path labels never select those I/O targets.
Caller-supplied generation sets
and the transitional namespace fences are restricted to synthetic test paths;
they cannot authorize production deletion.

Verification checks mandatory V4 references for descriptor, document, metadata,
vector, layout, lexical manifest, the separately numbered lexical payload,
optional RaBitQ and every mutation run. The admitted path performs fresh
relative opens and full object hashing before consulting shared lazy readers.
A warm cache may correctly serve an older unlinked inode to an existing reader;
that inode cannot prove that the current persisted dependency name is usable
for recovery. Verification failure precedes enumeration and every unlink.
These checks assume cooperating immutable dependencies throughout the held
publication interval; they do not exclude arbitrary external writes after hash
verification.

The admitted path promotes/borrows the publisher's original native custody
without closing/reopening the process lock. It uses the originating admission,
governor, cancellation and I/O-wave context. Bound work without that admission
is refused. Manifest decode admission remains live through closure destruction;
retention-set admission remains live through the synchronous sweep. Pending
capture and temporary child readers are destroyed before sweeping, so they do
not keep all obsolete generations pinned. Existing independent child/term pins
still protect precisely their own artifact families.

Ordinary ungoverned callers remain an explicit legacy resource mode. They use
captured native files, full references, bounded manifest reads and typed cleanup
verification ceilings. This is not a whole-operation memory/RSS or physical-I/O
qualification. The ceilings cover declared bytes and file references; repeated
semantic/CRC/cache validation can read those bytes again. Full fresh validation
currently scales with the selected closure, and source-bound metadata reuse and
performance/fairness gates remain required. The old writer-owned pathname reuse
snapshot and the partition installer's second retention-set allocation are
removed: neither can authorize this captured pass, and retaining them would
repeat metadata initialization and hold unrelated file handles without use.
Ordinary reader-refresh metadata sharing remains separate. The two obsolete
pathname-cleanup snapshot tests are replaced by production captured ownership,
missing-dependency and warm-cache/current-name tests; no failing assertion was
removed to obtain a pass.

Standalone resident RaBitQ output with actual vector data still lacks its complete
producer-output and recovery-selection ledger. Preserve its artifacts and
quarantine evidence, and report retry when that unknown family is encountered.
Reclamation can proceed when a persisted source-bound absence witness is freshly
revalidated, or a matching complete producer snapshot is semantically verified
to contain no vectors. Merely losing the snapshot and its receipt proves neither.
The vector-bearing and historical-fallback gap must be resolved before qualifying
complete cleanup; indefinite preservation is not the final reclamation contract.

The real immediate/retry namespace-GC probes reclaim obsolete A and retain active
A without changing replacement B projection files. The stronger native regression
`validation_scratch_keeps_all_operations_off_the_replacement_namespace` now also
passes its replacement B directory-metadata assertion in the local integration
source. Governed cleanup uses a separate validation authority alongside its
existing artifact capture; the two captures must identify the same source.
A 10,000-term governed fixture requires actual captured spill writes, cold opens
and removal, and preserves a foreign diagnostic-label directory unchanged.

The stage ledger registers creation evidence before native effects. A real mkdir
followed by an injected lost response retains native parent/candidate metadata;
retry removes A's private directory while preserving the same-named B directory.
A governed debt releases execution/I/O slots and refuses an ungoverned retry;
`retry_staging_cleanup_with_context` borrows fresh actual work/IO capacity. The
original metadata remains funded until native custody closes. Legacy path-based
producer stages are separate and have not gained this captured refinement.

These are targeted local evidence, not Windows scratch/Consumer activation or
whole-operation physical resource/performance qualification. Original Windows
source/publication directory-move failures remain enabled. Missing-dependency,
fresh-name and byte/file-ceiling refusals still precede projection unlink.

The actual Consumer generation metadata/producer ledger, persistent non-reused
incarnations, unresolved candidate custody, prebuilt immutable query view,
source-owner C..E revalidation, stationary selector/receipt exchange and recovery
remain open. The complete acceptance ledger in
[MVCC_VERSIONED_PUBLICATION_SPEC.md](../specs/MVCC_VERSIONED_PUBLICATION_SPEC.md)
continues to govern this work.

## Formal and native qualification still required

The inherited captured-file custody and source-root models cover isolated cache,
pin and namespace boundaries. They are not a refinement proof for this combined
V4 producer/reader implementation. A composed reference-publication model must
include originating producer output, a complete persisted closure, captured-root
identity, changed-name/eviction reads, selection, retained readers and unresolved
GC roots. Independent controls must exercise an installed-output-derived
expectation, missing-reference/native fallback, partial closure, wrong lexical
counter, path-based discovery and premature source/debt retirement.

Native tests must separately prove actual producer-reference equality, complete
managed cold validation, same-source hydration after legal namespace changes,
independent child/term lifetime, resource refunds and heap reclamation. Preserve
all adverse logs and exact-source receipts. Passing a parser, a bounded model or
a ledger check does not authorize Consumer serving or full MVCC completion.

## Bounded retention model and correspondence

`HawDBCapturedRetention` isolates the verification-to-unlink boundary. Its two
dependencies represent distinct required families, not a proof that the Rust
manifest enumerates every family. Root 1 is native publication custody; root 2
is a replacement namespace with an independent head. `Verify` corresponds to
complete captured discovery, fresh reference checks and semantic closure load;
`Sweep` corresponds to consuming `VerifiedRetention` under the same lease.
The child pin models an independently retained reader. A failure cannot produce
unlink authority, alter either root, or consume the child pin.

The bounded model checks fresh complete verification, budget refusal, source
identity, selected closure survival, exact child protection, foreign-root
preservation and no deletion on refusal. Independent constants substitute path
discovery, trust a warm cached inode, omit a required dependency, ignore the
verification budget or ignore the child pin. Each must produce a counterexample.
This is a finite safety check and correspondence argument, not an unbounded
mechanized refinement proof or a liveness/recovery proof. Producer-origin
receipts, incarnation uniqueness, crash/torn/reordered durability, resident
RaBitQ, full resource accounting and fair bounded work remain outside this model.

## Resident absence state and the model-found counterexample

The persistent distinction is `Present(snapshot reference, vector count)`,
`KnownAbsent`, or `Unknown`. The V4 fields are a fixed optional snapshot receipt
and a mandatory absence bit; validation rejects both being set. A missing receipt
with a false absence bit stays unknown. A new pure generation source can establish
genesis absence only through the actual publication directory, before installing
its outputs, with a bounded empty-projection scan. A later generation may carry a
previously persisted absence witness and recheck native absence. Damaged or
unknown parent ownership cannot be upgraded merely because a file is missing.
Append, mutation, full replacement and partition installation carry the actual
base's fixed ownership metadata, re-read under publication exclusion. They do
not reconstruct it from the caller's dirty resident document map.

The first `HawDBResidentVectorAbsence` model found a real implementation gap:
lose a vector snapshot, drop its metadata receipt, observe native `NotFound`,
then authorize deletion as if the root had never contained a resident snapshot.
Keep that adverse trace. The strengthened protocol requires persisted absence
provenance as well as native absence; the corresponding native test preserves
the active vector artifact and orphan evidence after both losses. Do not weaken
`OnlyProvenAbsenceDeletes` to admit the counterexample.

Before unlink, the verifier checks either the carried absence witness and actual
native `NotFound`, or the complete snapshot reference and its decoded vector
count. Missing expected bytes are a dependency failure. A valid foreign snapshot
cannot supply its own expectation; a forged zero count cannot hide actual
vector-bearing rows. The same lease owns both verification and the sweep.
No admitted decoder clears or substitutes its caller's task, governor or memory
reservation. Encoded bytes, decoded text/growth allowance and compression state
remain admitted until their actual owners die.

This legacy verification currently decodes a bounded complete snapshot to check
its count; it is not the final constant-work C..E or publication implementation.
The closed stationary producer ledger must retain source-qualified output
references, actual vector selection and historical fallback custody without
rehydrating a corpus after E. The current active-head checksum/booleans are not
an independent cryptographic producer-origin proof. The finite model assumes
closed producer construction and cooperating publication exclusion; it does not
model arbitrary forged heads, durable incarnation/replay authority, torn or
reordered persistence, or prove the complete Consumer contract. The source-root
and retention models supply separate boundaries, not an automatic composed
refinement proof.

Native differential checks also exposed an inadequate alternate-snapshot fixture:
a codec-valid replacement of a different length was rejected before the digest
comparison, so disabling that comparison did not fail the test. Preserve that
inadequate-control result. The strengthened fixture uses bounded search for an
independently encoded, codec-valid, equal-length snapshot with a different full
reference. Only then can the installed-output/self-expectation counter-control
qualify the full-reference boundary. The lost-snapshot/receipt, self-expectation
and trusted-count controls must each fail their intended real cleanup test, then
all six positive resident tests must pass after exact source restoration.

Budgeted synchronous cleanup now has a private `ReadPermit::Synchronous` that
borrows the actual publication lease and retains the original operation memory
accounts. It neither creates a governor nor clears the task reservation. Dynamic
resource controllers cannot be relabelled as synchronous, and persistent root
construction requires the original governed permit. The original budgeted
partition cleanup regression and three admission regressions pass locally.
`HawDBScopedCleanupAdmission` models aggregate admission, refusal, payload
funding and the prohibition on exporting a persistent root. Its four negative
controls are finite invariant checks, not a proof of the implementation or of
scratch namespace custody. Captured scratch adoption has its separate local
native and lifecycle evidence; composed MVCC qualification remains open.

Absence-proof I/O denial also remains `Unknown` before any proof I/O. A queued
producer may have a zero-slot request in the existing API; do not invent a slot,
clear its reservation or fail a valid publication merely because optional cleanup
proof cannot run. Governed cleanup still requires its real I/O admission and
reports deferral. Whole-producer physical-I/O qualification remains separate.

# Search staged-byte sealing and pre-selection verification

## Concrete path

`generation_writer::publication::install_generation` previously compared only
length for document, metadata, vector and lexical main payloads. A replacement
of the same length between staged-output inspection and installed-output
verification could therefore pass those checks and reach active-head selection.
Descriptor, lexical-manifest and optional RaBitQ checks already had separate
CRC checks; this change preserves those checks and the persistent formats.

The actual producer now captures an `ArtifactSeal` for each of the four staged
payloads before installing any of them. Each seal contains the complete length
and raw SHA-256. Each installed payload must match that previously captured
value before `InstalledGeneration` can be returned and its head selected.
The expected digest is never refreshed from the installed pathname. Retractions
and reused old segments retain their existing behavior; this does not scan all
historical payloads on each update.

This seal is an internal value, not a storage `ObjectReference`: the latter
includes kind/version/length domain separation. It must not be serialized as one
or passed as a managed-object identity to readers. No alias inventory, native
inode/length fallback, durable acknowledgement or Consumer source capability is
created here.

## Bounded operation and error outcomes

Each hash opens one file, holds it through the entire scan, and borrows one
`SPOOL_BUFFER_BYTES` scratch allowance from the producer's existing account
before opening. The file length is captured from that held file. Growth beyond
it and an incomplete EOF fail integrity checks. Reads retry interruptions,
check cancellation/deadline before every chunk and check again before returning
a seal. Scratch and descriptors are refunded on every return. Empty files also
require admission and cancellation checks.

A mismatch returns `StorageIntegrity` before head selection. Already installed
private files may remain and follow the existing cleanup protocol; an error is
not proof that no file installation occurred. The old active head is preserved.
The SHA work adds one complete staged read and one complete installed read for
each of these new payloads. No latency/throughput/RSS gate is inferred from a
bounded scratch allowance. Integrating digests with actual output writers and
persisting the producer-owned reference inventory remain required follow-up.

## Argument and model scope

Under the existing cooperating producer's private-stage, publication exclusion
and immutable-installed-file assumptions, the staged value is fixed before
installation. Verification accepts an installed value only if its length and
SHA-256 equal that fixed expectation. Assuming SHA-256 collision resistance,
accepted bytes equal the staged bytes. Head selection follows all verification,
so selection cannot acknowledge a mismatching output from this path.

`HawDBSearchArtifactSealing` abstracts four fixed-length artifact contents as
0 (producer bytes) or 1 (changed bytes). It permits one installed artifact to
change before verification and cancellation before selection. It checks
`SelectedBytesMatchProducer`. Its three independent controls omit the digest
check, derive the expectation from the installed bytes, or select before the
check. Each must produce a counterexample.

The model assumes ideal digest equality and no mutation after verification.
It does not prove SHA-256, FFI refinement, pathname origin/custody, prevention of
external in-place writes, complete closure discovery, sync/power-loss ordering,
GC, logical incarnation/ABA, C..E freshness or Consumer readiness. In particular,
this path-based standalone producer is not newly authorized for captured-source
serving. A trusted generation reader still needs the actual producer's full
managed references and captured native authority throughout hydration and GC.

## Native regression boundary

The production-path regression hook changes the last byte of one real installed
payload without changing its length, after all content installation and before
verification. Separate document, metadata, vector (feature-enabled) and lexical
cases require rejection, unchanged old head, and successful old-generation
hydration. The I/O tests cover first/intermediate/final-chunk differences, an
expectation that remains fixed after source mutation, exact scratch admission,
empty input, cancellation and complete account refunds. Mutation executions and
TLC results must be recorded separately with their actual source/binary hashes;
this document alone is not an execution receipt.

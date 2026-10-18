# Consumer generation metadata: format and authority boundary

Status: the metadata codec is implemented in
[`consumer/generation_metadata.rs`](../../crates/search/src/consumer/generation_metadata.rs).
It is a data description, not an installed Consumer, serving capability, sealed
producer output, completed receipt or non-reuse proof. No production initializer,
selector or registered-reader path writes or selects this format yet. Existing
Consumer directory publication, full captured Search hydration/GC and the complete
[activation acceptance ledger](../specs/MVCC_VERSIONED_PUBLICATION_SPEC.md) remain
open. Existing admitted roots must keep their serving rejection.

## Why the file references belong in generation metadata

The source-bound immutable-file reader required by the activation design must
reopen through a retained directory, retain exact pins, share the project FD
cache and validate a full trusted `ObjectReference`. It cannot invent that
reference. The separate captured-file integration is not delivered by this
metadata codec. Reading a replaced path's
current alias map can obtain another source's reference; treating an absent map
entry as permission for native-ID/length-only validation would also weaken
managed-object verification. Path guards around discovery do not prove absence
of an A/B/A namespace change.

The selected generation therefore needs an immutable, producer-sealed binding
from every actual artifact name to its complete object reference. It must survive
release of construction-time publication exclusion and later pathname reuse.
The new Consumer format stores this table alongside the generation identity,
with a separate reference to its Search manifest. The writer must compute or
reuse these references from its own sealed outputs, before fenced selection.
Existing private native-file capture remains a different contract; missing
managed references never select that contract automatically.

## Version-one encoding

All integers use little endian. There is no JSON or owned decoded collection.

| Field | Encoding |
| --- | --- |
| Format | Eight bytes `HAWCGM01` |
| Binding | Database, projection, registration and checkpoint UUIDs, 16 bytes each |
| Consumer ID | `u16` byte length and the existing canonical ASCII consumer ID |
| Source epoch | `u64` |
| Target incarnation | Nonzero `u64` |
| Generation | Nonzero `u64` |
| Component identity | Non-nil 16-byte UUID; a future selector derives its canonical child name |
| Search manifest | Complete 43-byte object reference |
| Artifact count | `u32` |
| Record offsets | `count + 1` `u32` offsets relative to the record area |
| Each record | `u16` filename length, canonical filename bytes, complete object reference |
| Checksum | SHA-256 of all preceding frame bytes |

An object reference encodes kind (`u8`), version (`u16`), byte length (`u64`) and
32 digest bytes. This format accepts `CheckpointArtifact` version one only. That
is the managed-object identity domain, not the embedded Search codec version.
Each artifact name must parse as an existing canonical content, lexical, RaBitQ
or mutation-run component. Names are strictly sorted and unique. The manifest
itself has its own reference, avoiding a self-reference in the file table.

The operation supplies maximum encoded bytes, artifact count and per-artifact
bytes. The complete frame must also fit `u32` offsets. These limits are ceilings,
not memory reservations: the caller must admit the input/output buffers through
its actual governor. An operation-limit refusal is an execution error, distinct
from invalid format/integrity. It must not authorize quarantining or rebuilding
a valid source as corrupt. No independent allowance or environment control is added.

## Parser and ownership argument

Let `n` be the decoded count and `m` the record-area length. Decode first bounds
the complete frame and verifies its checksum. It bounds `n` before computing or
borrowing the offset table, using checked addition and multiplication. Then:

```
o[0] = 0
o[n] = m
for every i < n:
    0 <= o[i] < o[i+1] <= m
    record[i] consumes exactly bytes[o[i]..o[i+1]]
    record[i] has a canonical name and bounded version-one reference
    i > 0 implies name[i-1] < name[i]
```

These conditions partition the whole record area without gaps, overlap or a
trailing suffix. A duplicate offset cannot decode a nonempty record. UTF-8,
reference fields and the end of every record are checked. All derived ranges
come from checked bounds. Empty tables require an empty record area.

The returned view borrows the immutable input, exposes no mutable header fields,
and allocates neither a filename vector nor a decoded map. Rust's borrow rules
keep the bytes live and immutable while the view is usable. Exact lookup uses
binary search over the already validated offset table and returns only the
stored full reference or absence. No pathname or mutable alias registry is
consulted. Successful decode uses O(1) auxiliary space; lookup is O(log n)
record comparisons, with canonically bounded names. Error reporting can allocate
its ordinary bounded error string; this is not a whole-process RSS theorem.

The operation supplies a borrowed `RuntimeTaskContext`. Decode checks cancellation
before hashing, between 64 KiB hash chunks and before validating each record.
Sizing and encoding check it before work and between records/index entries, then
again before and after the checksum footer. An observed body-time cancellation
prevents writing the footer even for an empty table. Cancellation during the
last write can leave a complete private frame and still return an error; it is
not evidence of rollback. The view retains none of this execution context.

Encoding validates every field, ordering constraint and complete checked size
before calling the writer. It then streams the header, offsets, records and
checksum with fixed hashing state. Partial and short writes obey `Write`; an
I/O error or cancellation may leave a prefix or complete private candidate. The codec never
opens a file, flushes, synchronizes, selects, deletes, releases candidate custody
or acknowledges durability. The actual producer must retain that debt on error.

The integrity argument assumes collision resistance of SHA-256. The footer is
an integrity checksum, not authentication: a different self-consistent frame can
be recomputed. The native malformed-frame tests include correctly recomputed
checksums precisely to distinguish semantic validation from checksum rejection.
This parser argument is not a new TLC check or a mechanized Rust proof.

## Required activation composition

The following steps remain implementation obligations, not properties supplied
by a decoded `MetadataView`:

1. An actual Consumer operation owns the completed target binding, native root
   and exclusive preparation slot. Its writer seals the C artifact references;
   E updates share unchanged C references and add only the changed outputs.
2. The immutable generation metadata and complete dependency closure are
   installed and synchronized outside SourceOwner. The engine obtains an
   expected digest from its own completed receipt/selector state, rather than
   trusting a caller-provided checksum or the frame footer alone.
3. Open the Search manifest and every descriptor/layout/payload/lexical/vector/
   mutation dependency through the captured component directory and the exact
   stored full references. Missing entries refuse. Validate complete semantic
   closure, analyzer identities, lengths and checksums; format validity alone
   does not prove that the table covers the actual Search closure.
4. Transfer the same governor's admitted encoded metadata, decoded reader state,
   source custody and exact pins into the live immutable view. An owned wrapper
   must retain its byte allocation before refunding memory and must not forge
   static borrows into its own buffer or retain a producer execution slot.
5. Under SourceOwner, validate source/target incarnation and D..E dependencies,
   select only preinstalled E, and exchange the real view. Retain actual target
   custody while finishing the selected head barrier outside SourceOwner.
6. Recovery and GC retain selected, reader-pinned and unresolved roots, including
   on-disk candidate debt after a crash. An unsupported/reserved Consumer format
   must fail closed before any initializer can make a new selector visible.

Nonzero counter parsing does not mint or prove non-reused incarnations. UUIDs,
content hashes and physical file IDs do not replace that engine-owned persisted
contract. Flat full-N metadata encoding after E also remains subject to the
original admitted work, writer-latency and fairness gates. Moving that work
outside the source fence alone does not qualify it as incremental.

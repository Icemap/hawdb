# Pinned-source Search batch coverage

Status: pinned bounded feed enumeration, tracked callback context and resumable
private external ordering/reduction are implemented. The artifact-owning
preparation session, coverage receipts, incremental artifact construction and
Mem integration below are **not implemented**. See the
[primitive argument](../tla/PROJECTION_FEED_CURSOR_PROOF.md).
This extends, rather than replaces, the full acceptance ledger in
[the versioned-publication spec](MVCC_VERSIONED_PUBLICATION_SPEC.md).

## Verified failure in the current path

At source `4e7d03217368db1ad1c226c51599240df43b4236`, create Memory `a`
with title `old`, build its Search baseline at C, update its title to `new` at B,
then delete it at E. Calling `build_search_projection_change_batch_after(C,
Some(1))` selects the B upsert and stops before the E delete. The existing
`build_search_projection_change_delta` hydrates against E, finds no node, emits
an empty delta and publishes B while retaining title `old`. That document is
wrong both at B (title `new`) and E (absent). This was reproduced with default
Cargo features, Rust 1.97.1, macOS arm64, a persistent SyncOnEveryWrite graph
and an in-memory Search index. It is a synthetic correctness failure, not a
claim of a production incident.

The selected epoch, hydration epoch and covered watermark are distinct.
Counting processed primary keys, accepting an arbitrary caller-supplied epoch,
or checking B <= E cannot establish coverage. Current public graph-delta and
change-batch structs are caller-constructible and must not become publication
authority for the new source-validated path.

## Publication unit and bounded steps

One preparation session binds the following immutable inputs under the current
authoritative owner: canonical source instance, pinned source epoch E, verified
Search base generation and complete base watermark C, host binding/incarnation,
projection mapping identity, schema and a retained changefeed resume floor.
Require C <= E and C >= floor. A future cursor, unknown base, wrong source,
unverified host binding or a gap requires an explicit refusal/rebuild decision.
Reopen/replayed WAL is not evidence that E is durable; the existing typed source
barrier remains a separate prerequisite for activation.

The publication unit is the complete C..E interval. Work is split into bounded,
invisible steps; an intermediate cursor B < E is a **private progress position**,
not a visible complete-through watermark. All graph and relational hydration
uses the same pinned E context. A final sealed coverage token exists only after
the complete interval has been enumerated and every selected input has been
processed successfully. This preserves bounded per-step work without inventing
historical snapshots that the current engine cannot supply. It does not relax
`publish_prepared_search`'s exact source-watermark requirement.

A step must charge visited records/items, duplicate keys, source bytes, emitted
keys, query work/rows/payload and retained output before the relevant ownership.
A unique-operation cap alone does not bound a million repeated updates to one
key. Enumeration must borrow immutable records or retain Arc records after
admission; `search_projection_changes_after`'s eager Vec clone is unsuitable.
Use an ordinal within an immutable log snapshot, including a position inside a
large transaction, so a chunk need not retain a whole large event or rescan the
prefix. Empty commits and an interval with no relevant changes still need an
explicit end-of-interval witness. Retention gaps and relational RequiresRebuild
records cannot be skipped while advancing that witness.

## Preparation ownership and native API contract

The reusable facade session owns PreparedRead, the immutable feed cursor,
budgets, binding/base identity and private staging. Raw mutable cursor fields,
public operation vectors and callback-provided processed counts cannot produce
the sealed token. Cursor exhaustion alone proves enumeration, not hydration or
mapping. The session orchestrates graph hydration through the parameterized
prepared Cypher path and records complete graph dependencies. Relational and
host mapping runs through a typed session context exposing the same prepared
Cypher/SQL reads, with per-step and cumulative work admission. Every selected
key must reach its mapper exactly once or an explicitly governed duplicate
reducer; ignored errors poison the entire session.

Host mapping correctness is an explicit trusted premise: HawDB can prove which
source keys and tracked queries were processed, not that arbitrary host code
implements its business meaning. Host binding schemas and business predicates
remain in the host. The host supplies a checked binding/incarnation under the
same owner that serializes replacement/rebinding and visible activation; an
unfenced earlier callback is not sufficient. A host must not be able to substitute
a receipt from another session/source/base merely by matching numeric epochs.

Bounded steps must support the complete Mem graph and relational mapping;
shipping only graph changes does not complete this protocol. Existing public
legacy builders remain unsafe for the reproduced partial-prefix case until
migration or a source-coherent legacy implementation is delivered. Do not add
an unsafe compatibility escape hatch to the sealed publication APIs.

## Global reduction and artifact sealing

Chunk order is not document conflict order. Repeated renames, deletion after an
upsert, ABA IDs and duplicate business IDs require a projection-defined reducer.
Gather old document identities and final E rows; apply deletions before the
final upserts for a document. An ordered streamed writer requires a governed
external sort/dedup spool when input arrives in commit/node-key order. It must
not concatenate independently completed updates or retain an unbounded in-memory
BTreeMap. The reducer must specify deterministic ownership when distinct source
rows map to one document ID, or reject it under the actual host uniqueness
contract; this is not proved by assuming a unique synthetic fixture.

The coverage token must be bound to the exact stage/reduction identity and final
source watermark E. It must be impossible to apply a valid token to an arbitrary
candidate built without those inputs. Ordinary prepared Search candidates may
retain their existing APIs, but do not implicitly gain batch coverage authority.
A failed/partial mapper, cancelled step, missing spool part or artifact-admission
failure cannot seal coverage. Stage disk/metadata, permits, retained source,
query dependencies and operation count all remain cumulatively governed.

## Activation, recovery and retries

Under one authoritative publication boundary, recheck canonical instance,
schema, host binding incarnation, base selector/generation, complete coverage,
tracked read dependencies and observed source durability. Keep the fence through
manifest/head and watermark installation. Disjoint foreground writes may commit
while preparation is paused; a changed global epoch alone must not reject work.
Relevant changes, replacement, ABA rebinding or a competing base must reject
before visibility. The new bounded session cannot fix the remaining unbounded
activation checksum/copy/cleanup critical section; that remains a separate gate.

Private steps may spill/checkpoint invisible artifacts, while the old C head
remains authoritative. Cancellation or a crash before activation discards or
reclaims those parts under the existing retained-stage policy; it does not
advance a durable watermark. Resuming private work after restart would need a
durable, validated source snapshot/identity and receipt closure; do not assume a
process-local pin survives. The first implementation may discard private work
and reprepare, with explicit bounded retry/cancellation and cleanup accounting.

Once activation has committed, a lost response can leave a committed E head.
Recover by a durable operation/stage identity and matching base/source/binding
receipt; do not replay an arbitrary operation against a newer head or infer
rollback from a missing response. Graph/Search cross-store atomic rollback is
not claimed. Qualify source sync, artifact sync, selector/watermark, directory
barriers and GC with fault images, not only process-kill/reopen tests.

## Required delivery evidence

The [external mutation ordering component](../tla/SEARCH_UNORDERED_MUTATIONS_PROOF.md)
now builds a private expected-base Search update from unordered fixed-E intents
under payload/record, shared task-memory and cumulative I/O/merge admission.
It checks exact duplicate bodies, coalesces retracts and rejects conflicts.
Its queue/advance/close protocol also admits each ordering quantum's work and
logical read/spill bytes, retaining heap, run, prefix and reduction state across
yields. A single record/comparison must fit a whole quantum or the attempt
refuses. This supplies output ordering for the session; it does not yet own
feed-to-stage coverage, make final artifact construction incremental, or
authorize a complete watermark.
The legacy partial-prefix catch-up path remains unchanged.

The companion coverage model must reject legacy early-prefix publication,
skipped/repeated cursor items, ignored mapper errors, missing delete reduction,
wrong source/base/binding/schema and a released validation fence. Native tests
must map those seams to the actual session, stage and publication APIs. Add
hot/cold graph and relational equivalence against a full E rebuild, empty and
oversized transactions, rename/delete/ABA, gap/checkpoint/replay, budget and
cancellation refusal, and cross-session token substitution tests.

Complete the original Mem outer-owner interleaving and 1/4/8-writer enabled/
disabled catch-up measurements, retained-source RSS, fault composition and
single-stream latency/fairness gates. A passing coverage model or primitive
iterator alone does not complete #231/#232.

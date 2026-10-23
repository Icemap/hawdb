# Composite ordered seek

HawDB can replace a blocking sort with ordered access to an existing composite
node index. This is a query-planning capability, not a new index format or
host-specific listing API.

## Supported domain and legality

An index consists of one or more equality-prefix properties, followed by exactly
two ordering properties. Both order items have the same direction. The last key
has an enforced unique constraint. Both suffix properties have active, enforced,
non-null `INT` or `STRING` descriptors. Prefix equality supplies non-null presence;
the initial numeric-prefix opt-in is restricted to an enforced `INT` descriptor.
This is a conservative supported-domain restriction. Equality uses typed value
equality; numeric range comparisons instead cross Int/Float domains, so continuation
bounds additionally require comparator-compatible suffix types.

The selected read view must advertise complete ordered index delivery. A schema
index declared after a canonical checkpoint does not by itself prove an available
projection. Index availability and legality proofs are part of cache/prepared-plan
compatibility. Unproven domains, nullable suffixes, ambiguous projection aliases,
mixed sort directions and bounded canonical-work scopes keep ordinary planning.

For example, with `(kind, created_at, id)`, non-null integer `created_at`, non-null
string `id` and a unique `id` constraint:

```cypher
MATCH (n:Item)
WHERE n.kind = $kind
  AND (n.created_at < $time OR (n.created_at = $time AND n.id < $id))
RETURN n.created_at AS created_at, n.id AS id
ORDER BY created_at DESC, id DESC
LIMIT 32
```

Omit the continuation predicate for the first page. Ascending continuation uses
`>` in both comparisons. Repeated first-key bounds must refer to the same literal
or parameter slot. The original predicate remains authoritative after index
selection. Additional predicates are evaluated before the semantic LIMIT; they
can require visiting more than one output page of candidates. The planner never
moves this access across expansion, grouping, distinct or an input LIMIT.

A continuation with incompatible comparison types retains ordinary query semantics
through the fallback plan. The optimizer never interprets an opaque parameter map
as a scalar type: the plan-cache marker owner supplies shape-specific proofs only
on eligible cache misses, and binding retains ownership of marker interpretation.
Cache hits and ordinary point queries do not clone the optimizer catalog for this
proof. The marker owner supplies proofs only for bindings retained by the verified
parameterized plan, omitting unused lists and other request fields.

## Ordering, snapshots and continuation

The suffix tuple uses the same `Value::cmp` ordering as sorting. A unique final
key makes the tuple a total order over the selected domain. Continuation excludes
the previously emitted tuple, preserving ties on the first key. OFFSET/LIMIT
retains its existing behavior; OFFSET still consumes skipped rows.

Retain one `DatabaseReadTransaction` for the entire pagination sequence and pass
the last emitted tuple to the next query. The existing read transaction owns
snapshot validity and lifetime. Updates, deletions, label/prefix moves and new
nodes in other transactions do not change that pinned sequence. A new read
transaction observes a new domain; tuple values alone do not identify an old
snapshot. No serialized cursor or externally supplied snapshot token is accepted
by this feature, and no token-validation or cross-session lifetime is implied.

Low-level `CompositeOrderedSeek` describes the complete non-null index-key domain;
it does not prove that a query's canonical domain is covered. The optimizer's
schema/predicate/runtime proof establishes that separate condition. Invalid
low-level prefix/continuation arity and incomplete required-key projections fail
before emitting rows.

## Storage and resources

Resident COW indexes use a borrowed double-ended key interval and exclusive tuple
filtering. Canonical data uses existing immutable composite blocks. Descriptor
endpoints and block min/max keys locate the starting block with binary probes;
forward or reverse descriptor traversal continues from that block. There is no
full-prefix collection or sort. A bounded cache representation contains fully
validated immutable block bytes, entry offsets and the decoder workspace bound.
Cold construction admits the complete representation before copying and validates
all ordering/bounds before publishing a reusable proof. Warm reads bind the same
artifact/generation/block identity and SHA-256 of the checksum-validated immutable
source bytes, recheck selecting metadata and seek within the
block by binary probes. They decode visited keys rather than the skipped prefix. Integrity hashing still
visits the bounded encoded block; logarithmic key decoding is not a claim of
logarithmic total byte work.
The existing global SegmentCache owns capacity and eviction; capacity refusal
uses an admitted uncached view and preserves complete results. Cold validation
remains linear in a bounded block and is counted. Canonical and local sorted streams merge
incrementally, masking replaced and tombstoned canonical IDs and validating each
hydrated key against its canonical node.

For B immutable blocks, locating a page uses O(log B) descriptor probes, each
using the existing descriptor tree, followed by blocks needed for the page and
residual filters. This is not a claim of O(log B) total tree work or exactly one
payload read per result. Rejected rows, stale base entries and selective residual
predicates can increase work. Query reports distinguish `CompositePropertyOrdered`
access; existing property-index metrics retain descriptor, block, decoded-entry,
candidate and byte counts.

Owned seek bounds, descriptor/key decoding and reverse offsets are admitted before
allocation through existing query resource owners. Admission refusal and caller
cancellation do not poison a healthy reader; physical corruption does. Explicit
checkpoints preserve cancellation in uncapped work scopes, including rejected
local entries. Query memory is refunded on completion, refusal, early Stop and
consumer failure. No persistence layout, WAL ownership or durability default changes.

## Verification and measurement

Storage tests compare bidirectional COW intervals and descriptor bounds with
complete references. Ordered projection tests exercise global block ordinals,
deep pages, exclusive boundaries and refusal/retry. Store and API tests compare
all pages against a complete canonical sort through checkpoint/delta/reopen and
retained snapshots. Planner/cache cases cover proof loss and parameter type
changes. Existing CI lanes own ordinary regressions; see `docs/TEST_INDEX.md`.

The opt-in `composite_ordered_seek` Cargo benchmark compares the same filter and
scalar projection with inference-disabled sorting, alternates path order and
retains every sample, including a labelled initial warmup excluded from latency summaries. It records setup cost, latency, index work and
admitted memory for first, middle and near-end pages in both directions. Its
persistent fixture checkpoints and reopens without flushing device caches.
Fixture writes use bounded 4,096-row transactions under the unchanged default WAL
record ceiling. The unique constraint validates the entire loaded fixture before
index creation; an exact row-count preflight runs after reopen. Complete setup
cost is recorded separately from reads. This avoids repeated incremental
uniqueness scans during fixture loading without changing production validation
or measuring write throughput. Cache capacity, residency,
evictions and admission refusals accompany query memory reports. Timings from that
mode are not device-cold results, and index bytes exclude canonical hydration and
unrelated scan I/O. Preserve negative cases and separate
source identity, profile, compilation cost and execution measurements.

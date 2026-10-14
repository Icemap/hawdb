# Owned-result map capacity admission

Issue Number: ref #1008

The owned-result collector moves payloads into bounded row-major segments. It
must admit both retained heap capacities and the temporary incoming row before
publishing another row. Logical map entry sizes do not bound std B-tree node
allocations: on the pinned 64-bit Rust toolchain, even a one-entry
`BTreeMap<String, Value>` requests a 632-byte leaf.

## Implementation assumptions

This bound targets the repository's pinned Rust 1.97.1 std implementation,
not a stable Rust API guarantee. Its B-tree uses 11 key/value slots per node,
12 child pointers per internal node, and at least five entries in every
non-root node. A root may retain one empty leaf after its last entry is removed.
Tree mutation is finished before the collector measures the map; consuming its
entries frees nodes without allocating replacement nodes.

Toolchain changes must requalify these assumptions against
`library/alloc/src/collections/btree/node.rs`, the supported target layouts,
and native allocation witnesses. Do not silently reuse this bound after a
change in branching factor, minimum occupancy or node allocation layout.

## Bound and ownership argument

For `n > 0` entries and `k` allocated nodes, root occupancy is at least one,
and all other nodes have at least five entries. Therefore
`n >= 1 + 5 * (k - 1)`, giving `k <= 1 + floor((n - 1) / 5)`.
For an empty map, conservatively admit one possible retained root.

Bound every node as an internal node. The requested-node allocation bound is
`11 * size_of::<(String, Value)>() + 14 * size_of::<usize>()`: eleven slots,
twelve child pointers, and two pointer-sized words for parent/header/alignment.
It is 728 bytes on the qualified 64-bit layout. Pair padding conservatively
covers the separately stored key/value arrays on the supported 32-bit layouts.
The calculation uses checked arithmetic and rejects overflow.

Add actual key capacities and recursively bounded value heaps. Inline map
metadata already resides in the parent `Value` slot or parent node slot.
String/Binary capacities, List slot capacities, and recursive payloads are
charged separately. An empty map may be overcharged even when it never
allocated a node; undercharging a retained empty root is forbidden.

Before ownership transfer, reserve the incoming row's node bound, key
capacities and recursive payload bound. Keep this lease until consuming the
row destroys its tree and keys. Admit destination heap and buffer replacement
overlap while this lease remains held. This can count the same moved payload
twice during transfer, deliberately bounding the overlap conservatively.
The payload traversal is reused for admission rather than performing a second
logical map-size traversal.

Failure must publish no additional row, preserve earlier rows, and release
temporary leases and any abandoned destination buffer. Existing capacity
rollback and checked growth remain required.

## Qualification scope

The earlier heterogeneous witness on `47cb6523` retained 5,408 rows with a
16,741,635-byte ledger charge but 28,293,123 native requested bytes. The
2,136-byte per-row discrepancy came from six small nested map nodes. Its
independent Python fixture checksums and final-owner release checks remain
historical evidence, not results for this repair.

Qualify the repair with requested-allocation counters for the original wide
fixture, scalar/string controls, fresh and emptied roots, insertion splits,
multi-level maps and maps made sparse by deletion. Verify byte-budget refusal,
unchanged prior rows and balanced final release. Ordinary regressions are
delegated to CI; formatting, strict native/WASM Clippy and required local fuzz
remain separate gates.

This is a requested-allocation bound under stated implementation assumptions.
It excludes allocator rounding/workspace, source storage, other execution
accounts, foreign heaps and host ownership after query execution. It does not
prove whole-process RSS, canonical-source reuse, zero copy or latency gains.
Keep the comprehensive read/write and memory performance gate open until the
final source is independently qualified.

# Borrowed source properties for simple node projections

This optimization removes the temporary selected-property node copy from eligible resident label scans. It keeps the ordinary owned result contract: output values are still materialized after output-row admission. It does not implement source-to-host zero-copy, change indexed point-source access, or qualify SIMD, Arrow, graph expansion, or FTS delivery.

## Eligibility and fallback

The bound physical plan must use resident label-scan access under the existing retained-source-key allowance, resolve exact label semantics, have no residual predicate, and have nonempty output. Every expression must be a property or ID of the scan variable or a bound literal. Every property must also occur in `required_properties`. The last condition preserves the old projected-record behavior for incomplete physical metadata.

Indexed access, out-of-core sources, computed expressions, other variables, predicates and empty projections retain the existing execution path. The resident-source check precedes expression eligibility so indexed point queries do not walk the simple-scan projection guard.

## Value equivalence

Let `n` be a selected source node and `R` the physical plan's required-property set. The old path clones the properties in `R` into a temporary node, then evaluates each projection against that node. For an eligible property `p`, `p` belongs to `R`, so both paths return `n.properties[p]`, or `Null` when absent. Both return the same signed node-ID cast and the same bound literal. Nested maps, lists, binary payloads and strings keep their ordinary `Value` semantics.

Both paths use the same projection-map construction, deterministic column ordering, duplicate-name suffix handling and bounded output-batch delivery. `ORDER BY` remains a separate physical operator; no source insertion-order assumption replaces it.

## Ownership and memory

The source iterator and its storage snapshot remain live through each projection callback. Borrowed projected values cannot escape the source input: `push_borrowed_projection` admits the complete owned output before materializing payloads and before advancing the source. No unsafe representation cast or foreign buffer alias is introduced.

Borrowed resident records require no temporary selected-property allocation. An owned/hydrating iterator input keeps the existing selected-input allowance and its source read boundary. Retained-source-key admission, output row/byte limits, result-account charges and value-buffer growth-overlap accounting remain enforced. Removing an allocation removes its charge; it does not raise an allowance or replace physical memory with a logical-payload estimate.

These are per-query ownership/accounting statements. Existing database data, pinned snapshots, setup and foreign consumers remain separate memory resources. A small result allowance is not a whole-process RSS bound, and ordinary owned collection is not bounded pull delivery.

## Control and observability

Cancellation is checked before source rows and inside the existing projection/output boundary. Consumer stop, byte/row flushes, the detection-row execution cap and terminal output-limit failures retain their existing behavior. The new `borrowed_node_projection_rows` counter identifies rows projected from source references in typed and serialized query reports. It is not a payload-copy counter or proof of foreign allocation identity.

The embedded regression covers source properties, missing values, nested values, NUL strings, node IDs and constants, plus computed-expression fallback. The executor regression covers complete and incomplete required-property metadata and the corresponding hit counts. Existing output-limit/refusal/retry tests remain applicable. Serialization fixtures include the counter. Compilation and lint checks do not substitute for runtime regression or performance evidence.

## Qualification

Freeze each ordinary producer before comparison. Preserve identical consumer work, checksums, default durability, budgets and prefetch settings. Retain negative point/write measurements and default-budget wide refusals. Benchmark evidence from an earlier prototype cannot qualify a revised guard or binary. Comprehensive performance, whole-memory and strict zero-copy acceptance remain separate gates.

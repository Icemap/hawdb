# Partial predecessor resource comparisons

Issue Number: ref #987, ref #976

This packet records the 67 currently complete resource groups from run39079471994: production engine6881bbcc/treefa0f7dc7; orchestration source2a1a677a; original baseline3cdb2610 plus the declared profiling adapter. Original-source manifest/producer identities are retained in inputs.json. It is not current/final #986 qualification and does not approve its performance/resource gates.

## Observations

- 30 ordinary paired groups, all 1000 rows with 3 measured samples per side and a discarded warmup, have successful complete parity. Native allocation-call medians are lower in all 30 (candidate/baseline0.046–0.900); these are Rust global-allocator observation windows, excluding foreign heaps.
- Process peak RSS medians are higher in28/30 (candidate/baseline0.982–1.243). The largest observed ratio is Go/file/wide: baseline75,104,256bytes, candidate93,347,840bytes. RSS includes startup, fixtures and the complete producer process; it is not an isolated query-memory or causal regression measurement. Three samples do not establish significance or tail behavior.
- 18 other complete ordinary groups preserve refused outcomes rather than timing ratios.19complete numeric groups contain 36 successful and 21 expected-refusal mode outcomes (31 measured samples per mode); declared positive/boundary acceptance failures are absent among those audited complete modes. One additional numeric/file/default fixture group remains incomplete and contributes no query samples or timing/limit proof.
- Native conversion/engine timers are nested under native_profile in the actual schema; Go also has separate go_host_profile timers/call counts. Go native phases nest inside its host ffi_call_ns and must not be added twice. Python/Go native timers are positive in these ordinary groups; Rust does not provide those foreign-conversion fields. Missing or uncovered zero fields are not zero-cost claims. The separate private phase-only collectorv2 does not retroactively instrument these observations.
- 108 successful ordinary native observation windows have nonzero live or allocated-minus-deallocated deltas. They are process-global windows; per-query isolation is absent, so these are not proof of a query leak or proof of exact release. Raw tuple/side/iteration deltas are retained in comparisons.json.

Both the allocation improvements and RSS increases remain evidence for the final resource review, not a waiver. Instrumented timings are separate from ordinary latency; process peaks are distinct from requested/native-accounted bytes. Numerical ratios are descriptive only. Numeric observed 31-sample median/p95/max values, profile availability, exact limits and refusal counts retain their own source/configuration/instrumentation scope.

## Reproduction and remaining work

The original audited summary, aggregate script, per-artifact extracted identities and per-group data are included. audit.py reads the original cached extracts; obtain them from the run artifacts and verify their producer hashes before aggregation. Cached-extract SHA values are not ZIP hashes. No native binaries, raw BEP, environment dump or signed URL is included.

The original ordinary 90/resource 110 matrices remain incomplete; all final-source allocation/conversion/source construction/whole-operation RSS/write/tail/exception/default-refusal/explicit-large and supported-platform checks and genuine review/approval remain required. Refusals, incomplete groups, early unobserved snapshots and initial failures are never relabeled as successful timing samples or observed zero. No test, dataset, sample count, default resource/durability policy or deadline changed for this analysis.

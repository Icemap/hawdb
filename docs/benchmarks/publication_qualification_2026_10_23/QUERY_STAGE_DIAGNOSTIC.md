# Query-stage diagnostic

This is a separate local arm64 diagnostic, using the same original query, all 256 id/value/mirror rows, 256 writes and 64 reads per reader. It does not replace the uninstrumented x86_64 gate or resolve its nine regressions.

The four predeclared cohorts and four alternating pairs per cohort completed all 32 processes, 192 cases, 49,152 commits, 110,592 reads and 128 reopens with exact checks. Every reader has 64 positive parse/planning/execution triples whose sum is bounded by its enclosing query duration. Extra timers and a thread-local integer side channel perturb execution; four pairs are descriptive, not a confidence or causal qualification.

The temporary root overlay measures parser invocation, bounded-read validation/planning before execute_with_request, and execution including storage-error/health handling. Its accessor exists only in the external diagnostic source, never in the production library. Both variants use identical harness and root-overlay bytes, the original global bench profile and pinned image; baseline/candidate roots are main 8e9eab185 and implementation 823e8e9c. Both changed root/benchmark targets rebuild in independently source-qualified target directories. Incremental build costs are 5m 56s and 5m 57s with dependencies reused.

## Durable nonserialized stage measurements

Values are medians of four process p50 values, in microseconds. Stage medians need not sum to the median total. All individual ratios, including inconsistent and adverse pairs, remain in the raw analysis.

| Reopened | Writers/readers | Grouped | Revision | Parse us | Plan/check us | Execute us | Query us |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: |
| False | 1/2 | False | baseline | 2.167 | 14.166 | 182.479 | 204.667 |
| False | 1/2 | False | candidate | 2.167 | 13.646 | 179.895 | 200.896 |
| False | 1/2 | True | baseline | 2.146 | 13.542 | 182.292 | 200.479 |
| False | 1/2 | True | candidate | 2.209 | 13.916 | 180.792 | 202.271 |
| False | 8/16 | False | baseline | 2.396 | 15.438 | 608.229 | 644.000 |
| False | 8/16 | False | candidate | 2.437 | 15.646 | 613.708 | 662.375 |
| False | 8/16 | True | baseline | 2.438 | 15.354 | 613.688 | 639.771 |
| False | 8/16 | True | candidate | 2.479 | 15.542 | 631.688 | 665.438 |
| True | 1/2 | False | baseline | 2.208 | 13.750 | 181.708 | 201.395 |
| True | 1/2 | False | candidate | 2.208 | 14.625 | 183.312 | 206.875 |
| True | 1/2 | True | baseline | 2.229 | 13.979 | 182.875 | 204.230 |
| True | 1/2 | True | candidate | 2.208 | 14.125 | 180.646 | 201.458 |
| True | 8/16 | False | baseline | 2.417 | 15.562 | 636.062 | 664.542 |
| True | 8/16 | False | candidate | 2.458 | 15.458 | 670.542 | 695.500 |
| True | 8/16 | True | baseline | 2.499 | 15.354 | 639.104 | 663.562 |
| True | 8/16 | True | candidate | 2.500 | 15.354 | 651.812 | 681.250 |

Parsing is approximately 2–2.5 us, planning/checks 13.5–15.7 us and execution 179.9–670.5 us. These measurements select execution as the next local optimization target; they do not attribute the remote regression to a specific allocation, invalidate caches, or prove that sharing private transaction plan caches would be safe or beneficial.

Source inspection finds that eligible node projections first build an owned-name BTreeMap of borrowed values and then convert it to the output BTreeMap. A separately measured candidate may avoid that temporary map for small, uniquely named simple projections while preserving full output admission, conservative workspace allowances, cancellation and callback-release boundaries. Duplicate, wide and computed expressions must retain the general path. This is a proposed implementation to validate, not a measured improvement in this report.

Raw records, source/script/protocol hashes, root instrumentation patches and identical harness text are under `followup/read-query-stages-diagnostic-v1/`. Executables, full source trees and databases are excluded.

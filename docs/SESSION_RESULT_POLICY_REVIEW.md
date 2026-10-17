# Session result-policy review

Issue Number: ref #1046

Disposition of [the ff7baf26 review](https://github.com/nowledge-co/hawdb/pull/1050#issuecomment-6227995303).
This is implementation evidence, not independent approval.

| Finding | Change and verification obligation |
| --- | --- |
| Runtime view shows raw zero | system_runtime_snapshot uses read_result_row_limit. The contract exercises None, Some(0) and a positive ceiling through database, Cypher session, read snapshot and SQL session views; unlimited is NULL. |
| Numeric default can become a zero-row budget | DEFAULT_MAX_READ_RESULT_ROWS now has type Option<usize> and value None. SessionConfig::max_result_rows still defaults to u64 zero, meaning unlimited. Explicit host Some(0) remains accepted; low-level request Some(0) stays strict. |
| SQL DML route depends on ceiling | DatabaseSession delegates all SQL to one bounded database entry with its own system variables. Prepared INSERT/UPDATE/DELETE use the existing native transaction staging path, validate result rows/payload before commit, and reuse the prepared statement. This removes the session-only parse and temporary SqlSession. Ordinary limited/unlimited DML follows the same route. |
| Session settings and rollback | The contract sets background/analytics/estimated-operation variables, runs parameterized RETURNING, UPDATE and DELETE with session ceilings zero/four, and checks settings and results. It then verifies row, strict zero-request and payload refusals preserve committed data/epoch. These variables currently govern Cypher work requests; SQL does not expose a current_setting expression or consume those Cypher work hints. The test does not claim new SQL variable syntax or scheduling behavior. |
| BEGIN/COMMIT expect | Runtime construction takes SessionConfig directly. COMMIT returns a storage-integrity error if its runtime is unexpectedly absent rather than panicking. |
| wasm32 ceiling | Public field documentation and READ_RESULT_LIMITS explain that a u64 ceiling at/above usize::MAX adds no representable bound, including five billion on wasm32. Finite host/byte/memory ceilings still apply. |
| Wide foreign-style results | A wide result includes quotes, backslashes, newlines and Unicode, 20 columns and 1,000 source rows. Under an unlimited row policy, a 4 KiB payload ceiling and 256 KiB tracked query-memory ceiling must produce typed PayloadBytes refusal with zero consumer delivery. Complete aggregation afterward proves reuse and no hidden row truncation. This is engine-boundary qualification; JSON encoder allocations, foreign heaps and whole-process RSS remain separate #986 resource gates. |

## Verification history and remaining gates

The original ff7baf26 contract suite passed 17 cases. Its complete general
partition failed with 1,668 passed, six failed and one ignored in 716.34 s.
Failures were automatic-checkpoint delta-pressure, pending/retiring-owner,
preparation-backoff, projected-artifact idle-handoff and invalid-read-handoff
cases. All six subsequently passed individually in the frozen same binary.
That diagnostic does not replace or waive the failed complete partition.
Original budgets, deadlines and case selections remain unchanged.

Pinned Rust 1.97.1 formatting, strict locked workspace/all-target/all-feature
Clippy, all-files prek and minimal-browser-WASM strict Clippy pass.
`cargo test --locked --test execution_request_contract` passes all 20 tests
with zero failures/ignored in 92.74 s, including the three new contracts.
The registered Bazel contract target, complete general, unchanged mandatory
local fuzz and final-head CI still need final-source receipts before leaving
Draft. No complete-source pass is claimed here.

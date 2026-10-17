# Session result policy qualification: 6a588f13

Engine head: `6a588f13857a51250b93073966f202f3a537aee7`.
Engine tree: `cfe97429dd6390cf5810db483d42cfdd95c06040`.
Integrated main: `a7900428a24a74fe373d05f9cfc1a63cf144bf3b`.

This archive is qualification documentation on a separate branch. It is not a PR
base and is not intended to land as the engine implementation. `source.json`
identifies every main-relative changed file; `manifest.json` records the exact
length and SHA-256 of every other packet file. `packet.json` records scope,
counts and cache reuse. All files must be checked against the immutable archive
commit rather than an evolving branch name.

`general.log` is the complete registered general partition with test-result
reuse disabled. `contract.log` includes all 21 execution-request contracts.
`test-events.json` projects all 102 registered binding/driver/fuzz target results,
the full general result and the original append benchmark result. The original
98 mandatory fuzz targets, both exclusive residency campaigns, seeds, internal
concurrency and deadlines are preserved. The two residency halves each cover
16 seeds in both Materialized and OutOfCore modes (32 distinct seeds, 64 durable
fixtures, 992 state checks per half); each registered campaign must execute its
one exact libtest. `registration.json` proves the configuration and fuzz source
are unchanged from integrated main. `commands.json` contains actual explicit
user options and requested patterns. Full BEP/client environment is not included.

The append benchmark keeps its original four-hit assertion, fixtures, samples
and deadline. Its final result is cached against unchanged production inputs;
that cache hit is not a new execution. Raw Go/Python/driver and both residency
logs are included separately. Compiler and lint commands are in strict logs.

Historical adverse results remain visible:

- `old-root-ci.log`: 8d4ec67b Prow root, 55 passed targets and one failed append
  benchmark; eight native hits versus the required four.
- `old-append-local.log`: local reproduction of that assertion.
- `old-custom-limit-local.log`: e8c1cfaf production plus the initial added test;
  INSERT returned a custom-limit error after advancing epoch 3 to 4.
- `intermediate-fixture-failure.log`: the first guarded source passes the INSERT
  rollback assertion, but the new test's unqualified UPDATE is refused by the
  existing requirement for an explicit predicate. UPDATE and DELETE fixtures
  gained WHERE clauses; production did not change for that fixture correction.
- `initial-test-and-guard.patch` preserves the initial test/guard delta, and
  `final-guard.patch` is the actual e8c1cfaf-to-6a588f13 correction. The first
  INSERT and epoch assertion are unchanged between red and final green tests.
  We do not claim that the entire red-test file equals the final file.

The single-pass empty-result path applies only to RowPage DML with default
mutation limits. Result-bearing DML and custom limits retain transaction
pre-commit validation. Final contracts check INSERT, qualified UPDATE and DELETE
refusals against complete stored rows and committed epochs, in addition to
session/host/request row semantics and payload/RETURNING rollback.

This packet proves only its stated correctness and local fuzz scope. Platform
CI, Prow, independent approval, Tide/main landing, remaining PR986 splits and
full final-source performance/resource acceptance are separate requirements.

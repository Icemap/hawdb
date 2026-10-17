# Session result-policy integration evidence

Issue Number: ref #1046

This packet records the complete general and mandatory local verification for
PR #1050 engine `8d4ec67bf4de0c53e31b4a634972d69eb4040d08`, after actual main
`a7900428a24a74fe373d05f9cfc1a63cf144bf3b` (including #1051 and #1052).
Its tree is `24145dc00d4687f7da06ac0a17ef308a61a99c17`.

`packet.json` binds source hashes, old/fixed general results and scope.
`general.log` is the full uncached general log: 1,677 passed, zero failed,
one existing ignored; all four old pressure failures pass.
`old-policy-9e80-general.log` retains the old complete failed partition.
`test-results.json` contains all 102 passed target events (16 executed,
86 cached on identical inputs), including all 98 mandatory fuzz targets.
The two residency campaigns retain their original exclusive execution,
cases, seeds and deadlines. Selected raw contract/Go/Python/driver/residency
logs and strict native/WASM logs are included. `commands.json` projects actual
BEP user options/patterns; repository options select a verified offline Cargo
cache and do not change tests or runtime limits. No full BEP/client environment
is published. `manifest.json` gives every other file's length and SHA-256.

These are source-qualified execution receipts, not independent approval or
#986 performance/resource acceptance. The general prospective tree and final
actual-main merge tree are identical; the tests preceded the branch merge
commit. Final-head remote CI remains a separate gate. This archive is not
intended to land on main or become a base for another PR.

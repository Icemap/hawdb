# Indexed-node local qualification

Issue Number: ref #1081, ref #987

This packet qualifies the local functional checks of Draft #1082 at head
`f4b2714a4ca5e3fd0e1cee98ad3d7eda719b6554`, tree
`adf8e234eddda5a48cb755bc1d8fe9560d7cf9c5`, based on main `6db1c013`.
The head and working tree were verified unchanged after the complete campaign.
`source-files.json` identifies all 33 changed files by SHA-256. Native binaries
are retained locally; only their hashes and logs are published.

## Results and scope

- Retained/Arrow/node: 90 passed; portable: 8 passed; execution contracts: 30 passed.
- Complete original mandatory fuzz: 98 targets passed, 79 executed and 19 cached.
  Both residency halves retained 16 seeds and 992 state checks each.
- The registered general target passed 1777 tests, with 0 failures, 1 existing
  ignored test and 322 tests filtered by the repository's partition arguments.
  The command added no test filter; this target does not replace other partitions.
- C ABI: 19 passed; registered pure-Go suite passed. Go retained race checks
  passed against the freshly built Node library with `-count=1`.
- Python: 355 passed, 7 optional skips. Pydantic: 43 passed, 1 optional skip.
  The public PyArrow 26.0.0 suite separately ran all 8 cases without skips using
  the recorded source-qualified extension, Python 3.11.15 and native addresses.
- Pinned Rust 1.97.1 formatting, strict native/minimal-browser WASM and independent
  C/Python Clippy, all-files prek and commit hooks passed.

`qualification.json` records extension/library identities and exact-source
results. Core state contains every target status and preserved local raw-log
path. Public predecessor-310 results are explicitly separate: those 8 passing
consumer cases belong to parent source 310c739d, not current parent c4b65500.
No predecessor result is relabeled as a new-source result.

The first independent Python `--locked` preflight refused because the new
worktree lacked its Git-ignored lock. Its original log is preserved. Refreshing
only local package identities to 0.10.0 preserved all 135 external resolutions
and checksums; the subsequent strict locked check passed. A first audit attempt
used system Python 3.9 without tomllib; the successful audit used Python 3.11.
No product or performance success is attributed to either invalid setup attempt.

## Remaining gates

Windows and Skein results, genuine independent review/approval and normal Tide
integration remain open. Main has since added e864272e (#1080 scalar-expression
IN); this packet does not qualify a future combined tree. The PR stays Draft
until its remaining acceptance is met. Independently reviewed components and
passing local checks do not complete parent #986's original final-composition,
90-group ordinary, 110-group resource, default/explicit-large and resource/tail/
exception acceptance. Earlier parent checkpoint failures and resource adverse
observations retain their original scope and are not waived.

This is additive experimental indexed-node ownership qualification. It makes
no comprehensive performance, whole-process RSS, general canonical-storage
zero-copy, broad graph/FTS or stable Mem activation claim.

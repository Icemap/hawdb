# Go host conversion collection qualification: 6067f3dc

Source head: `6067f3dce6238f5f37e6341e6627589ac34eb52e`.
Source tree: `e8b3975646c80f4a46cc326cffa7e2a2e1ca80ac`.
Base main: `a7900428a24a74fe373d05f9cfc1a63cf144bf3b`.

This packet validates optional collection plumbing, not latency improvement,
foreign-heap/process-memory bounds or PR986's final performance acceptance.
It is documentation on a separate archive branch, not a PR base. The source
PR remains unchanged. Manifest lengths and SHA-256 values refer to exact bytes.
Builds/tests began on identical frozen staged source before its source commit.

## Local source qualification

`test-events.json` contains all 101 registered results: the ordinary Go binding,
five tagged Go profile cases, 28 driver cases and the unchanged 98 mandatory fuzz
targets. Three targets executed; the 98 engine fuzz results were cached against
unchanged inputs. Rust engine sources and original fuzz configuration are
unchanged from main. Selected complete logs, a callback-only Go race run and
strict pinned formatting/Clippy/prek output are included. The race run exercises
conversion, errors, buffer release, concurrent counter updates and saturation
using Go callbacks standing in for the ABI; it does not run a database.

Actual optimized query disassembly shows no timer/profile-record calls in the
ordinary artifact and three timer starts/six static record call sites in the
tagged artifact. Wrappers inline into the shared record method; the initial
verifier's expectation of wrapper symbols was corrected. This proves observer
removal, not a speedup. Windows metadata proves only CGO-disabled amd64 tagged
test/producer cross-compilation. No Windows Go runtime execution is claimed.

## Actual data and error controls

Each controls file embeds exact stdout/stderr plus hashes and source/artifact
receipts. There are 13 real three-row probes per replay: all five existing
workloads on memory/file (ten successes), one plain producer control and warmup
and execute parse errors. Checksums, row counts and exact measured query-call
counts match. Warmup/setup counts are excluded from successful measurement;
failure query counters include setup/warmup/execution and are sampled after
Close. Close itself is not a query conversion. Errors do not qualify medians.

The original Rust production reference remains
`3cdb2610ba8b984676b80ba22df64821787886d3`. Its adapter overlay is
`4a235b06362ae6306a300e45d5974495c1229c2b`; the complete patch and source audit
are included. Production src/crates/default/lock/toolchain objects remain
unchanged, and root Cargo production sections are identical apart from benchmark
registration. Existing v1/v2 baseline worktrees remain frozen.

Both independently built Go producers pass their controls, but their binary
bytes differ despite identical source. `shared-go-controls.json` therefore
replays the candidate's exact same Go executable bytes against the original
reference native libraries. `shared-go-audit.json` checks shared hashes,
production objects and matched outcomes/call counts. Native phase times nest
inside the complete Go native-call interval and must not be added to it.

## Retained adverse observations

The initial ordinary reference dylib failed to load on macOS 27 due to malformed
stripped LINKEDIT data before DB open. Its ten successful profile controls and
failed plain attempt remain in `initial-linkedit-controls.json`. Matching main's
Mac-only strip setting fixes loading; no algorithm, budget or durability default
changes. The final four-target build and all 13 controls pass afterward. The
initial candidate build command used a nonexistent target alias and ran zero
build processes; its log is retained alongside the corrected successful build.

Full representative performance/resource replay on the settled PR986 source,
an additional integrated-main control, CI, independent approval and Tide/main
landing remain separate requirements. Full BEP/client environment is excluded.

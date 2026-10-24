# Exact-source publication/resource qualification

This experimental branch repairs the environment for the fixed #1119 publication
qualification after the local 8202d2f5 allocation observed foreign host compilers
and monitor gaps. All local data remains retained and unqualified for timing.

The manual job checks out the controller separately from the exact measured
runtime `8202d2f50cc057716b47cd4080e5bb56d37ab562`. It retains the original pinned
Rust image, global bench profile, four-CPU/six-GiB container budget, all 54 reader
resource cases, 18 sustained cases and 54 fixed controls. Native source, queries,
fixtures, row/write/read budgets, callback guards and deadlines are unchanged.
All compilation must finish before timing. Host compiler observations and monitor
coverage are retained; they do not establish exclusive physical hardware.

Use `run_writer_gate=true` only on this qualification branch. The other writer
inputs do not alter this fixed experiment. Main/default/scheduled jobs are
unchanged, and no fuzz target is registered in CI. Cross-architecture data is
reported separately and is never pooled with the local arm64 allocation. Passing
this job does not waive the independent strict writer/read performance failures.

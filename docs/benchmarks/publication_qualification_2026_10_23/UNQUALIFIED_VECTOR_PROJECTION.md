# Unqualified local compact-vector experiment

All 32 processes, 192 cases, 49,152 commits, 110,592 reads and 128 reopens
completed exact checks. Candidate build cost was 7m 06s with existing caches.
The source uses the same 823e anchor, query-stage instrumentation, unchanged
query and all original workload/assertion controls as the retained stack
experiment, replacing only the fixed stack representation with an actual-width
vector. No previous timing samples were reused.

The launch inventory was empty, but a recorded observation during measurement
found another workspace running Rust 1.98.1 Cargo and two Clippy drivers. This
allocation is unqualified for comparative latency. Do not discard individual
outliers, select favorable subgroups, pool this allocation, or claim a gain.
Production Rust remains restored to 7da5fc6c.

Source, build, script and binary hashes, every raw sample and the contamination
disposition are indexed under `followup/small-projection-vector-ablation-v1/`.
A separate predeclared x86_64 comparison of all three representations runs at
[40035817116](https://github.com/nowledge-co/hawdb/actions/runs/40035817116),
experiment commit b93264aad446b43e7cf826ab58c7da6b855455f8. It uses one independent
CI job, finishes all three builds before timing, records the actual operator
plan, and samples guest compiler activity every second. It keeps the 120-minute
deadline and cannot replace the original strict 40-pair performance gate.

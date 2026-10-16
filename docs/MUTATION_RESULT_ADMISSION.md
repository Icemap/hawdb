# Incremental mutation-result admission

This independent optimization is extracted from PR #986. It does not
change mutation limits, public APIs, WAL bytes, transaction publication, or
durability. The full check immediately before prepared mutation publication
remains in place.

## Equivalence argument

Let `w(r)` be the existing saturating sum of result-column name lengths and
`estimated_value_bytes` within row `r`. Let `S(R)` be the saturating sum of
`w(r)` for every row in the result prefix `R`.

The tracker starts at `(checked_rows, payload_bytes) = (0, 0)`. After each
successful check its invariant is `(len(R), S(R))`. Mutation preparation only
appends owned rows; it neither removes nor changes previously appended rows.
For an extension `R ++ D`, nonnegative saturating addition is associative,
so `S(R ++ D) = saturating_add(S(R), S(D))`. The tracker computes exactly that
value by visiting only the unchecked suffix. It updates its state only after
all checks pass. A preparation branch that skips the outer check leaves its
rows in the unchecked suffix for the next check or the final full check.

Operation count, affected-row count, result-row count, and payload checks run
in the same order as before, with unchanged errors and thresholds. Rejection
still precedes WAL append and publication. The final complete scan guards
other callers and the prepared transaction boundary independently of tracker
state.

For `n` one-row mutations, the old outer checks visit `n(n+1)/2` rows. The new
outer checks visit each appended row once, followed by the existing final
linear scan. Tracker state is two scalar fields; it adds no result owner,
payload buffer, or heap allocation. This is a logical admission equivalence,
not a whole-process memory bound or an end-to-end zero-copy claim.

## Verification and qualification

The prefix regression compares incremental and complete checks across nested
values, count limits, exact payload thresholds, and one-byte-under thresholds.
The graph regression checks refusal before state publication and an exact-limit
retry with preserved node IDs. Current-source CI must exercise these regressions.

Historical CPU samples, refused 100k controls, and paired ARM64 producer records,
including negative point and select outcomes, remain source-qualified in
[#986](https://github.com/nowledge-co/hawdb/pull/986). They do not qualify this
independent main-based source. Performance testing remains explicitly deferred;
all latency, tail, memory, language, read/write and graph/FTS gates are preserved.

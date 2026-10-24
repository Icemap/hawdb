# Rejected fixed-stack projection experiment

**Disposition: rejected; production files and their test index were restored to 7da5fc6c.** The rejected source patch and every sample remain retained. Correctness success does not establish a latency gain.

The control is the byte-identical 823e8e9c query-stage diagnostic binary from the preceding experiment. The candidate changes only executor projection preparation and its regression fixtures; the harness, temporary query-stage instrumentation, 256 rows, 256 writes, 64 reads per reader, original six cases, four concurrency/profile cohorts and four alternating pairs remain unchanged. No preceding measurement samples are reused or pooled.

All 32 processes, 192 cases, 49,152 commits, 110,592 reads and 128 durable reopens pass exact checks. The proposed production code also passed pinned all-target/all-feature strict Clippy compilation, 98/98 required local fuzz/Shiro targets (58 executed, 40 cached; 1,039.878 seconds), and a 30-second MVCC campaign with 66,539 cases after 13.52 seconds of tool compilation. The four new ordinary unit cases were compiled by Clippy and remain CI-owned; no claim is made that they executed locally.

The candidate uses a fixed eight-value stack buffer instead of an intermediate BTreeMap only for uniquely named simple projections. It preserves the original conservative preparation allowance, complete output-row admission, cancellation and byte-boundary callback/refund behavior. Duplicate, wide and computed expressions retain the general path; both resident and owned/cold node-projection visitors select the bounded alternative. Candidate build cost is 11m 13s with cached dependencies while local correctness compilation also ran; this is not a cold or isolated build.

## Observed durable execution p50 ratios

Each row retains all four paired ratios plus their median; four descriptive pairs are not a formal confidence or regression classification. Shared-host observations are launch-time inventory only, and several processes show substantial duration variation. These data are insufficient to support shipping the added complexity.

| Reopened | Writers/readers | Grouped | Paired execution ratios | Median |
| --- | --- | --- | --- | ---: |
| False | 1/2 | False | 1.051167, 1.045325, 1.727021, 0.952706 | 1.048246 |
| False | 1/2 | True | 1.050314, 1.023735, 1.626220, 1.562657 | 1.306486 |
| False | 8/16 | False | 1.070168, 0.935569, 1.052619, 1.000121 | 1.026370 |
| False | 8/16 | True | 1.190982, 0.912253, 0.953259, 0.970602 | 0.961930 |
| True | 1/2 | False | 1.044339, 1.039994, 1.045900, 1.050631 | 1.045120 |
| True | 1/2 | True | 1.025420, 1.028703, 1.056788, 1.276259 | 1.042746 |
| True | 8/16 | False | 0.960581, 0.899436, 0.973850, 1.055714 | 0.967215 |
| True | 8/16 | True | 1.045415, 0.986256, 0.950452, 1.040301 | 1.013279 |

The reopened ungrouped 1W/2R group is slower in all four execution samples by approximately 4%–5%, despite some other groups improving or varying widely. The implementation is not accepted. Large fixed-buffer movement is a hypothesis, not established attribution; a separate compact-vector experiment may test a different representation while retaining this adverse allocation and the same original control. No margin, fixture, guard, output budget or timeout is relaxed.

Raw records, rejected patch, source/script/protocol hashes, accepted build mapping and explicit disposition are under `followup/small-projection-ablation-v1/`. Local correctness source hashes and receipts are under `followup/small-projection-local-verification/`. Binaries, full source trees and databases are excluded.

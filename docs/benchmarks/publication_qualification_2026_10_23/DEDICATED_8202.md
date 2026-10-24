# Source 8202 dedicated publication/resource qualification

Issue Number: ref #1119, ref #232, ref #1136

[Run 40049133474](https://github.com/nowledge-co/hawdb/actions/runs/40049133474)
completed successfully. It measured source `8202d2f50cc057716b47cd4080e5bb56d37ab562`
with controller `e81001f28da7ccc1b0d9e08b95b63ecfe31e7b47`. The later merge head
`ef38a62742ac705b862e194b25757fb166b53423` preserves the measured production changes.
The full [50,688,759-byte ZIP](https://github.com/nowledge-co/hawdb/releases/download/evidence-pr-1217-2026-10-24/publication-resource-40049133474.zip)
has SHA-256 `769de7476d6f13fe67ebd58d3b5aceaa4df60b071dd3f6519040c3dbe2a999f0`.
Re-executing its verifier over all raw records reproduced `verified-summary.json`
byte for byte. Detailed per-case tables are in the ZIP under
`read-resource/summary.md`, `sustained/summary.md`, and `controls/summary.md`.

| Cohort | Cases | Observed scope |
| --- | ---: | --- |
| Pinned reads/resource | 54 | 119,808 reads; enabled modes read during preparation before writers drain; 5–7,673 staging calls. |
| Sustained | 18 | 100,000 writes/case, six checkpoints, no publisher cap, 31–1,048 post-E advances before writer drain in every case. |
| Controls | 54 | 4,096 writes/case and all declared controls; 5–7,115 staging calls. |

All 126 cases retained exact document/epoch checks and `SyncOnEveryWrite`:
2,242,368 writes in total. This closes the dedicated-run evidence gap; it does
not pass or waive the strict writer gate. The archived tables retain p50/p95/p99,
including worse tails, without selecting favorable modes. In the read/resource
matrix, detached quiet callback medians were 3.164–3.465 ms versus scoped
23.307–75.209 ms. These are same-binary cohort summaries, not paired confidence
intervals. Callback duration is only a lower bound on outer owner residency;
there is no inner selector-lock timing claim.

Build: pinned Rust 1.97.1 image, default features, global optimized release
profile, debug assertions and overflow checks disabled. Binary SHA-256:
`419ceb1271f706b8cf1bd006341f885fc2f3ec0a8339b000e79b41ea782b236b`.
Build time was 1,239.713 seconds, separate from 3,031.254 seconds of measurement.
The ZIP's `host-measurement-receipt.json` records 2,997
samples, zero compiler observations, no monitor errors and complete coverage.
Linux x86_64 had four logical CPUs, two physical cores and effective CPU count
two; 4/8 writers were manual oversubscription, while the automatic policy would
select one. This is not evidence of an exclusive physical host.

Observed RSS and lifetime HWM ranged from 30,996 to 70,312 KiB. Phase-boundary
RSS and process lifetime HWM do not establish phase heap ownership or an exact
peak. Old adverse runs remain in the archive and historical reports. See also
[the focused read-p50 follow-up](READ_P50_8202.md).

# Recovered resource comparison

Issue Number: ref #987, ref #976, ref #1081

Engine41db420c, resource source515a1e5b, run39133555481, artifact12139463897. The original Rust fill_bulk/1000/memory comparison is preserved, including its three measured samples plus warmup. No test was rerun.

The first independent download failed because Python3.9 socket.timeout bypassed the old TimeoutError handler. The corrected recovery also handles socket.timeout and incomplete HTTP reads, retaining three attempts/range and the original120s read timeout. All166 cached ranges (67230778bytes) were SHA256-checked before reuse. The failed attempt and recovery receipts remain included. All12 uploaded producer/launcher-manifest members and the baseline observer patch were independently matched against their recorded SHA256 values. The recovered reports also match the earlier report-only diagnostic.

| Observed median | Baseline | Candidate |
|---|---:|---:|
| Native allocation calls | 154418 | 135326 |
| Native allocated bytes | 39922402 | 37470225 |
| Process peak RSS bytes | 21176320 | 22421504 |

Allocations decrease while process RSS increases. These are instrumented observations, separate from ordinary latency; three samples do not establish statistical significance or a causal explanation. RSS covers the complete producer process, including setup/runtime. Allocation windows are process-global and do not prove per-query leaks or whole-process bounds.

The complete90ordinary/110resource matrices and independent disposition remain required. This one verified group does not approve PR986 or replace any adverse evidence. Raw reports, manifests and recovery/audit receipts are included for review; native artifacts remain available in the linked workflow run.

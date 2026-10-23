# Consumer publication measurements

Median of per-run percentiles across repetitions; all individual samples are retained in results.jsonl.
Publication callback time is a lower bound on the outer owner critical section and excludes lock-entry/drop overhead.
Quiescent publication samples cannot establish simultaneous source-write/Consumer-watermark progress.

| Documents / payload | Writers | Mode | Write p50 ms | Write p95 ms | Write p99 ms | Wait p95 ms | Quiet publish p50 ms | Writes/s | Active advances |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 32 / 128 B | 1 | detached | 0.402 | 2.363 | 7.503 | 0.982 | 3.231 | 1003.0 | 1596,1606,1605 |
| 32 / 128 B | 4 | detached | 1.611 | 4.862 | 24.216 | 4.339 | 3.588 | 1044.8 | 1324,1345,1357 |
| 32 / 128 B | 8 | detached | 3.794 | 14.935 | 71.131 | 13.502 | 2.811 | 1003.8 | 1103,1091,1053 |
| 512 / 1024 B | 1 | detached | 0.356 | 0.499 | 4.535 | 0.000 | 3.477 | 1784.6 | 108,85,99 |
| 512 / 1024 B | 4 | detached | 1.592 | 2.279 | 11.502 | 1.737 | 3.419 | 1834.2 | 99,103,82 |
| 512 / 1024 B | 8 | detached | 3.300 | 5.960 | 11.538 | 5.304 | 3.756 | 1846.5 | 97,98,115 |

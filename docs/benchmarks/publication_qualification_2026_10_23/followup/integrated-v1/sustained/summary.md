# Consumer publication measurements

Median of per-run percentiles across repetitions; all individual samples are retained in results.jsonl.
Publication callback time is a lower bound on the outer owner critical section and excludes lock-entry/drop overhead.
Quiescent publication samples cannot establish simultaneous source-write/Consumer-watermark progress.

| Documents / payload | Writers | Mode | Write p50 ms | Write p95 ms | Write p99 ms | Wait p95 ms | Quiet publish p50 ms | Writes/s | Active advances |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 32 / 128 B | 1 | detached | 0.264 | 0.415 | 6.544 | 0.000 | 3.186 | 2024.6 | 10,8,7 |
| 32 / 128 B | 4 | detached | 0.263 | 0.401 | 3.674 | 0.000 | 3.096 | 2250.3 | 6,10,7 |
| 32 / 128 B | 8 | detached | 0.299 | 0.600 | 6.315 | 0.000 | 3.130 | 1853.8 | 4,5,5 |
| 512 / 1024 B | 1 | detached | 0.290 | 0.505 | 6.467 | 0.000 | 3.778 | 1671.9 | 16,6,34 |
| 512 / 1024 B | 4 | detached | 0.281 | 0.464 | 1.721 | 0.000 | 3.572 | 1647.6 | 8,4,9 |
| 512 / 1024 B | 8 | detached | 0.293 | 0.438 | 0.629 | 0.000 | 3.417 | 2623.1 | 2,4,11 |

# Consumer publication measurements

Median of per-run percentiles across repetitions; all individual samples are retained in results.jsonl.
Publication callback time is a lower bound on the outer owner critical section and excludes lock-entry/drop overhead.
Quiescent publication samples cannot establish simultaneous source-write/Consumer-watermark progress.

| Documents / payload | Writers | Mode | Write p50 ms | Write p95 ms | Write p99 ms | Wait p95 ms | Quiet publish p50 ms | Writes/s | Active advances |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 32 / 128 B | 1 | detached | 0.459 | 0.727 | 2.046 | 0.000 | 3.990 | 1423.3 | 58,45,34 |
| 32 / 128 B | 4 | detached | 0.378 | 0.655 | 4.912 | 0.000 | 3.581 | 1561.0 | 17,15,11 |
| 32 / 128 B | 8 | detached | 0.416 | 0.734 | 5.780 | 0.000 | 3.495 | 1550.0 | 13,6,11 |
| 512 / 1024 B | 1 | detached | 0.395 | 0.721 | 4.792 | 0.000 | 4.096 | 1464.9 | 26,36,27 |
| 512 / 1024 B | 4 | detached | 0.431 | 0.645 | 0.899 | 0.000 | 5.168 | 1819.4 | 9,12,19 |
| 512 / 1024 B | 8 | detached | 0.467 | 0.762 | 1.070 | 0.000 | 5.096 | 1939.8 | 12,6,11 |

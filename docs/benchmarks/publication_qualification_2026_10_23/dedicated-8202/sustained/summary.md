# Consumer publication measurements

Median of per-run percentiles across repetitions; all individual samples are retained in results.jsonl.
Publication callback time is a lower bound on the outer owner critical section and excludes lock-entry/drop overhead.
Quiescent publication samples cannot establish simultaneous source-write/Consumer-watermark progress.

| Documents / payload | Writers | Mode | Write p50 ms | Write p95 ms | Write p99 ms | Wait p95 ms | Quiet publish p50 ms | Writes/s | Active advances |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 32 / 128 B | 1 | detached | 0.376 | 0.838 | 3.991 | 0.354 | 3.820 | 1870.0 | 1024,1023,1048 |
| 32 / 128 B | 4 | detached | 1.668 | 5.017 | 6.140 | 4.611 | 4.221 | 1850.6 | 954,962,963 |
| 32 / 128 B | 8 | detached | 3.397 | 7.058 | 8.615 | 6.653 | 4.505 | 1920.5 | 817,823,816 |
| 512 / 1024 B | 1 | detached | 0.358 | 0.475 | 0.750 | 0.000 | 4.608 | 2531.3 | 31,32,32 |
| 512 / 1024 B | 4 | detached | 1.626 | 2.078 | 4.197 | 1.575 | 4.684 | 2281.1 | 35,36,35 |
| 512 / 1024 B | 8 | detached | 3.321 | 4.541 | 6.642 | 3.893 | 4.640 | 2255.7 | 35,36,35 |

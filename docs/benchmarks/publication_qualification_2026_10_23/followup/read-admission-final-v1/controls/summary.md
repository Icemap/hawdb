# Consumer publication measurements

Median of per-run percentiles across repetitions; all individual samples are retained in results.jsonl.
Publication callback time is a lower bound on the outer owner critical section and excludes lock-entry/drop overhead.
Quiescent publication samples cannot establish simultaneous source-write/Consumer-watermark progress.

| Documents / payload | Writers | Mode | Write p50 ms | Write p95 ms | Write p99 ms | Wait p95 ms | Quiet publish p50 ms | Writes/s | Active advances |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 32 / 128 B | 1 | disabled | 0.236 | 0.302 | 0.494 | 0.000 | n/a | 3883.0 | 0,0,0 |
| 32 / 128 B | 1 | scoped-blocking | 0.239 | 0.371 | 0.707 | 0.000 | 14.769 | 3101.0 | 16,16,16 |
| 32 / 128 B | 1 | detached | 0.260 | 0.504 | 1.949 | 0.000 | 1.744 | 2899.2 | 16,16,16 |
| 32 / 128 B | 4 | disabled | 1.153 | 2.117 | 2.788 | 1.605 | n/a | 2589.6 | 0,0,0 |
| 32 / 128 B | 4 | scoped-blocking | 1.091 | 1.830 | 18.154 | 1.450 | 14.923 | 2738.3 | 16,16,16 |
| 32 / 128 B | 4 | detached | 1.084 | 2.535 | 3.915 | 2.025 | 1.739 | 2684.6 | 16,16,16 |
| 32 / 128 B | 8 | disabled | 2.210 | 3.484 | 4.317 | 3.070 | n/a | 3350.6 | 0,0,0 |
| 32 / 128 B | 8 | scoped-blocking | 2.277 | 4.311 | 17.843 | 4.005 | 15.571 | 2568.0 | 16,16,16 |
| 32 / 128 B | 8 | detached | 2.256 | 4.370 | 5.898 | 3.999 | 1.742 | 2704.6 | 16,16,16 |
| 512 / 1024 B | 1 | disabled | 0.261 | 0.402 | 1.482 | 0.000 | n/a | 1595.4 | 0,0,0 |
| 512 / 1024 B | 1 | scoped-blocking | 0.266 | 0.374 | 0.471 | 0.000 | 37.324 | 3209.6 | 3,3,2 |
| 512 / 1024 B | 1 | detached | 0.271 | 0.395 | 0.538 | 0.000 | 1.850 | 3396.4 | 3,3,3 |
| 512 / 1024 B | 4 | disabled | 1.158 | 1.562 | 2.119 | 1.212 | n/a | 3188.6 | 0,0,0 |
| 512 / 1024 B | 4 | scoped-blocking | 1.598 | 2.037 | 4.235 | 1.561 | 38.158 | 2173.3 | 4,3,9 |
| 512 / 1024 B | 4 | detached | 1.356 | 1.998 | 3.532 | 1.532 | 1.960 | 2618.1 | 3,3,6 |
| 512 / 1024 B | 8 | disabled | 2.374 | 3.492 | 4.961 | 3.073 | n/a | 2983.3 | 0,0,0 |
| 512 / 1024 B | 8 | scoped-blocking | 3.284 | 3.882 | 5.785 | 3.444 | 48.252 | 2160.0 | 4,3,7 |
| 512 / 1024 B | 8 | detached | 3.272 | 4.323 | 6.739 | 3.835 | 1.844 | 2375.4 | 3,3,4 |

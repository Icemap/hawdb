# Consumer publication measurements

Median of per-run percentiles across repetitions; all individual samples are retained in results.jsonl.
Publication callback time is a lower bound on the outer owner critical section and excludes lock-entry/drop overhead.
Quiescent publication samples cannot establish simultaneous source-write/Consumer-watermark progress.

| Documents / payload | Writers | Mode | Write p50 ms | Write p95 ms | Write p99 ms | Wait p95 ms | Quiet publish p50 ms | Writes/s | Active advances |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 32 / 128 B | 1 | disabled | 0.316 | 0.565 | 0.758 | 0.000 | n/a | 2754.4 | 0,0,0 |
| 32 / 128 B | 1 | scoped-blocking | 0.400 | 0.629 | 0.838 | 0.000 | 24.705 | 1986.2 | 7,7,3 |
| 32 / 128 B | 1 | detached | 0.319 | 0.581 | 0.817 | 0.000 | 2.960 | 2719.7 | 1,3,2 |
| 32 / 128 B | 4 | disabled | 0.360 | 0.599 | 0.779 | 0.000 | n/a | 2608.8 | 0,0,0 |
| 32 / 128 B | 4 | scoped-blocking | 0.427 | 0.675 | 0.910 | 0.000 | 26.302 | 2116.1 | 3,7,3 |
| 32 / 128 B | 4 | detached | 0.428 | 0.697 | 0.956 | 0.000 | 2.876 | 2168.9 | 2,1,1 |
| 32 / 128 B | 8 | disabled | 0.496 | 0.702 | 0.984 | 0.000 | n/a | 1889.0 | 0,0,0 |
| 32 / 128 B | 8 | scoped-blocking | 0.509 | 0.720 | 0.944 | 0.000 | 26.283 | 1751.9 | 4,5,1 |
| 32 / 128 B | 8 | detached | 0.380 | 0.651 | 0.836 | 0.000 | 2.822 | 2471.4 | 1,1,0 |
| 512 / 1024 B | 1 | disabled | 0.315 | 0.512 | 0.672 | 0.000 | n/a | 2925.1 | 0,0,0 |
| 512 / 1024 B | 1 | scoped-blocking | 0.424 | 0.604 | 0.810 | 0.000 | 70.100 | 2175.8 | 2,3,1 |
| 512 / 1024 B | 1 | detached | 0.545 | 0.812 | 1.018 | 0.000 | 3.160 | 1720.1 | 1,2,1 |
| 512 / 1024 B | 4 | disabled | 0.457 | 0.645 | 0.804 | 0.000 | n/a | 2044.8 | 0,0,0 |
| 512 / 1024 B | 4 | scoped-blocking | 0.464 | 0.747 | 0.964 | 0.000 | 57.703 | 1961.2 | 1,1,1 |
| 512 / 1024 B | 4 | detached | 0.458 | 0.720 | 0.961 | 0.000 | 2.927 | 2046.7 | 1,0,0 |
| 512 / 1024 B | 8 | disabled | 0.498 | 0.702 | 0.901 | 0.000 | n/a | 1935.7 | 0,0,0 |
| 512 / 1024 B | 8 | scoped-blocking | 0.602 | 1.026 | 1.325 | 0.000 | 57.454 | 1464.9 | 2,0,2 |
| 512 / 1024 B | 8 | detached | 0.413 | 0.749 | 0.979 | 0.000 | 3.092 | 2152.2 | 0,0,1 |

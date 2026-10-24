# Consumer publication measurements

Median of per-run percentiles across repetitions; all individual samples are retained in results.jsonl.
Publication callback time is a lower bound on the outer owner critical section and excludes lock-entry/drop overhead.
Quiescent publication samples cannot establish simultaneous source-write/Consumer-watermark progress.

| Documents / payload | Writers | Mode | Write p50 ms | Write p95 ms | Write p99 ms | Wait p95 ms | Quiet publish p50 ms | Writes/s | Active advances |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 32 / 128 B | 1 | disabled | 0.318 | 0.426 | 0.625 | 0.000 | n/a | 2908.1 | 0,0,0 |
| 32 / 128 B | 1 | scoped-blocking | 0.326 | 0.457 | 2.164 | 0.000 | 23.537 | 2169.2 | 16,16,16 |
| 32 / 128 B | 1 | detached | 0.328 | 0.477 | 3.073 | 0.000 | 3.170 | 2567.6 | 16,16,16 |
| 32 / 128 B | 4 | disabled | 1.359 | 1.725 | 3.348 | 1.340 | n/a | 2814.1 | 0,0,0 |
| 32 / 128 B | 4 | scoped-blocking | 1.437 | 2.433 | 25.033 | 1.907 | 24.072 | 2025.8 | 16,16,16 |
| 32 / 128 B | 4 | detached | 1.463 | 3.997 | 4.850 | 3.508 | 3.190 | 2362.9 | 16,16,16 |
| 32 / 128 B | 8 | disabled | 2.855 | 3.475 | 5.291 | 3.069 | n/a | 2685.0 | 0,0,0 |
| 32 / 128 B | 8 | scoped-blocking | 2.933 | 6.035 | 27.334 | 5.670 | 23.583 | 2023.5 | 16,16,16 |
| 32 / 128 B | 8 | detached | 3.050 | 5.989 | 6.801 | 5.618 | 3.216 | 2343.6 | 16,16,16 |
| 512 / 1024 B | 1 | disabled | 0.327 | 0.438 | 0.703 | 0.000 | n/a | 2832.7 | 0,0,0 |
| 512 / 1024 B | 1 | scoped-blocking | 0.351 | 0.486 | 0.770 | 0.000 | 73.774 | 2542.5 | 1,1,1 |
| 512 / 1024 B | 1 | detached | 0.355 | 0.465 | 0.628 | 0.000 | 3.257 | 2643.5 | 1,1,1 |
| 512 / 1024 B | 4 | disabled | 1.459 | 1.821 | 3.559 | 1.415 | n/a | 2614.5 | 0,0,0 |
| 512 / 1024 B | 4 | scoped-blocking | 1.605 | 2.024 | 3.774 | 1.536 | 73.853 | 2289.8 | 1,1,1 |
| 512 / 1024 B | 4 | detached | 1.595 | 2.045 | 3.917 | 1.562 | 3.310 | 2372.2 | 1,1,1 |
| 512 / 1024 B | 8 | disabled | 3.092 | 4.007 | 5.705 | 3.497 | n/a | 2476.2 | 0,0,0 |
| 512 / 1024 B | 8 | scoped-blocking | 3.245 | 4.075 | 6.023 | 3.558 | 73.802 | 2276.0 | 1,1,1 |
| 512 / 1024 B | 8 | detached | 3.236 | 3.885 | 5.908 | 3.393 | 3.260 | 2384.8 | 1,1,1 |

# Consumer publication measurements

Median of per-run percentiles across repetitions; all individual samples are retained in results.jsonl.
Publication callback time is a lower bound on the outer owner critical section and excludes lock-entry/drop overhead.
Quiescent publication samples cannot establish simultaneous source-write/Consumer-watermark progress.

| Documents / payload | Writers | Mode | Write p50 ms | Write p95 ms | Write p99 ms | Wait p95 ms | Quiet publish p50 ms | Writes/s | Active advances |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 32 / 128 B | 1 | disabled | 0.463 | 1.195 | 1.450 | 0.000 | n/a | 1765.1 | 0,0,0 |
| 32 / 128 B | 1 | scoped-blocking | 0.422 | 1.197 | 1.450 | 0.000 | 35.992 | 1863.4 | 9,3,3 |
| 32 / 128 B | 1 | detached | 0.429 | 1.212 | 1.468 | 0.000 | 3.512 | 1908.8 | 1,1,0 |
| 32 / 128 B | 4 | disabled | 0.422 | 1.226 | 1.469 | 0.000 | n/a | 1941.2 | 0,0,0 |
| 32 / 128 B | 4 | scoped-blocking | 0.413 | 1.154 | 1.368 | 0.000 | 29.612 | 1912.4 | 2,4,4 |
| 32 / 128 B | 4 | detached | 0.375 | 1.153 | 1.349 | 0.000 | 3.298 | 2128.9 | 1,2,1 |
| 32 / 128 B | 8 | disabled | 0.389 | 1.173 | 1.457 | 0.000 | n/a | 1906.7 | 0,0,0 |
| 32 / 128 B | 8 | scoped-blocking | 0.281 | 0.426 | 1.055 | 0.000 | 18.189 | 3123.5 | 0,5,2 |
| 32 / 128 B | 8 | detached | 0.280 | 0.393 | 0.532 | 0.000 | 2.129 | 3260.7 | 0,0,1 |
| 512 / 1024 B | 1 | disabled | 0.276 | 0.387 | 0.553 | 0.000 | n/a | 3327.7 | 0,0,0 |
| 512 / 1024 B | 1 | scoped-blocking | 0.332 | 0.560 | 0.726 | 0.000 | 47.577 | 2650.0 | 1,1,1 |
| 512 / 1024 B | 1 | detached | 0.326 | 0.552 | 0.722 | 0.000 | 2.321 | 2803.2 | 0,2,1 |
| 512 / 1024 B | 4 | disabled | 0.307 | 0.515 | 0.657 | 0.000 | n/a | 3033.2 | 0,0,0 |
| 512 / 1024 B | 4 | scoped-blocking | 0.365 | 0.626 | 0.816 | 0.000 | 48.488 | 2368.2 | 1,1,3 |
| 512 / 1024 B | 4 | detached | 0.286 | 0.521 | 0.660 | 0.000 | 2.264 | 3095.9 | 1,0,0 |
| 512 / 1024 B | 8 | disabled | 0.290 | 0.418 | 0.584 | 0.000 | n/a | 3149.5 | 0,0,0 |
| 512 / 1024 B | 8 | scoped-blocking | 0.345 | 0.618 | 0.754 | 0.000 | 41.799 | 2473.0 | 0,2,2 |
| 512 / 1024 B | 8 | detached | 0.301 | 0.558 | 0.696 | 0.000 | 2.254 | 2944.9 | 1,0,0 |

# Consumer publication measurements

Median of per-run percentiles across repetitions; all individual samples are retained in results.jsonl.
Publication callback time is a lower bound on the outer owner critical section and excludes lock-entry/drop overhead.
Quiescent publication samples cannot establish simultaneous source-write/Consumer-watermark progress.

| Documents / payload | Writers | Mode | Write p50 ms | Write p95 ms | Write p99 ms | Wait p95 ms | Quiet publish p50 ms | Writes/s | Active advances |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 32 / 128 B | 1 | disabled | 0.227 | 0.287 | 0.414 | 0.000 | n/a | 3896.0 | 0,0,0 |
| 32 / 128 B | 1 | scoped-blocking | 0.238 | 0.301 | 0.480 | 0.000 | 14.891 | 3462.6 | 1,2,2 |
| 32 / 128 B | 1 | detached | 0.251 | 0.434 | 0.581 | 0.000 | 1.683 | 3554.1 | 1,2,0 |
| 32 / 128 B | 4 | disabled | 0.268 | 0.482 | 0.789 | 0.000 | n/a | 3126.5 | 0,0,0 |
| 32 / 128 B | 4 | scoped-blocking | 0.240 | 0.289 | 0.391 | 0.000 | 16.207 | 3919.5 | 0,0,2 |
| 32 / 128 B | 4 | detached | 0.237 | 0.291 | 0.431 | 0.000 | 1.650 | 4097.0 | 0,0,0 |
| 32 / 128 B | 8 | disabled | 0.232 | 0.295 | 0.436 | 0.000 | n/a | 4165.3 | 0,0,0 |
| 32 / 128 B | 8 | scoped-blocking | 0.244 | 0.354 | 1.322 | 0.000 | 15.242 | 2673.2 | 0,0,5 |
| 32 / 128 B | 8 | detached | 0.223 | 0.291 | 0.445 | 0.000 | 1.615 | 4309.8 | 0,1,0 |
| 512 / 1024 B | 1 | disabled | 0.231 | 0.299 | 0.449 | 0.000 | n/a | 3941.1 | 0,0,0 |
| 512 / 1024 B | 1 | scoped-blocking | 0.245 | 0.316 | 0.424 | 0.000 | 43.165 | 3535.0 | 0,2,1 |
| 512 / 1024 B | 1 | detached | 0.235 | 0.303 | 0.416 | 0.000 | 1.683 | 4056.4 | 1,0,1 |
| 512 / 1024 B | 4 | disabled | 0.227 | 0.266 | 0.402 | 0.000 | n/a | 4268.3 | 0,0,0 |
| 512 / 1024 B | 4 | scoped-blocking | 0.234 | 0.300 | 0.451 | 0.000 | 43.016 | 4089.7 | 1,0,0 |
| 512 / 1024 B | 4 | detached | 0.227 | 0.268 | 0.390 | 0.000 | 1.614 | 4291.3 | 0,0,1 |
| 512 / 1024 B | 8 | disabled | 0.236 | 0.297 | 0.396 | 0.000 | n/a | 4089.8 | 0,0,0 |
| 512 / 1024 B | 8 | scoped-blocking | 0.232 | 0.303 | 0.459 | 0.000 | 35.327 | 3882.0 | 1,0,0 |
| 512 / 1024 B | 8 | detached | 0.237 | 0.350 | 0.514 | 0.000 | 1.693 | 3928.7 | 0,0,1 |

# Consumer publication measurements

Median of per-run percentiles across repetitions; all individual samples are retained in results.jsonl.
Publication callback time is a lower bound on the outer owner critical section and excludes lock-entry/drop overhead.
Quiescent publication samples cannot establish simultaneous source-write/Consumer-watermark progress.

| Documents / payload | Writers | Mode | Write p50 ms | Write p95 ms | Write p99 ms | Wait p95 ms | Quiet publish p50 ms | Writes/s | Active advances |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 32 / 128 B | 1 | disabled | 0.222 | 0.389 | 0.527 | 0.000 | n/a | 4232.7 | 0,0,0 |
| 32 / 128 B | 1 | scoped-blocking | 0.230 | 0.412 | 0.647 | 0.000 | 13.601 | 3624.0 | 4,3,4 |
| 32 / 128 B | 1 | detached | 0.217 | 0.250 | 0.372 | 0.000 | 1.535 | 4461.8 | 0,1,1 |
| 32 / 128 B | 4 | disabled | 0.217 | 0.269 | 0.458 | 0.000 | n/a | 4319.3 | 0,0,0 |
| 32 / 128 B | 4 | scoped-blocking | 0.222 | 0.266 | 0.448 | 0.000 | 13.235 | 4234.7 | 0,0,3 |
| 32 / 128 B | 4 | detached | 0.222 | 0.281 | 0.438 | 0.000 | 1.523 | 4316.9 | 0,0,0 |
| 32 / 128 B | 8 | disabled | 0.228 | 0.411 | 0.518 | 0.000 | n/a | 3864.1 | 0,0,0 |
| 32 / 128 B | 8 | scoped-blocking | 0.234 | 0.421 | 0.548 | 0.000 | 14.403 | 3828.8 | 0,6,0 |
| 32 / 128 B | 8 | detached | 0.226 | 0.403 | 0.498 | 0.000 | 1.604 | 4089.7 | 1,0,0 |
| 512 / 1024 B | 1 | disabled | 0.220 | 0.402 | 0.482 | 0.000 | n/a | 4215.2 | 0,0,0 |
| 512 / 1024 B | 1 | scoped-blocking | 0.226 | 0.265 | 0.380 | 0.000 | 42.528 | 4125.4 | 1,1,1 |
| 512 / 1024 B | 1 | detached | 0.227 | 0.288 | 0.409 | 0.000 | 1.623 | 4042.7 | 1,0,0 |
| 512 / 1024 B | 4 | disabled | 0.228 | 0.431 | 0.542 | 0.000 | n/a | 3897.5 | 0,0,0 |
| 512 / 1024 B | 4 | scoped-blocking | 0.231 | 0.387 | 0.580 | 0.000 | 37.023 | 3854.2 | 1,0,1 |
| 512 / 1024 B | 4 | detached | 0.255 | 0.394 | 0.601 | 0.000 | 1.688 | 3655.3 | 1,0,0 |
| 512 / 1024 B | 8 | disabled | 0.234 | 0.415 | 0.536 | 0.000 | n/a | 3839.6 | 0,0,0 |
| 512 / 1024 B | 8 | scoped-blocking | 0.250 | 0.449 | 0.575 | 0.000 | 41.741 | 3444.7 | 1,2,1 |
| 512 / 1024 B | 8 | detached | 0.235 | 0.411 | 0.531 | 0.000 | 1.628 | 3927.4 | 0,0,0 |

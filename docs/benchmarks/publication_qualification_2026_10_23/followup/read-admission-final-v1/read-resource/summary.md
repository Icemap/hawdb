# Consumer publication measurements

Median of per-run percentiles across repetitions; all individual samples are retained in results.jsonl.
Publication callback time is a lower bound on the outer owner critical section and excludes lock-entry/drop overhead.
Quiescent publication samples cannot establish simultaneous source-write/Consumer-watermark progress.

| Documents / payload | Writers | Mode | Write p50 ms | Write p95 ms | Write p99 ms | Wait p95 ms | Quiet publish p50 ms | Writes/s | Active advances |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 32 / 128 B | 1 | disabled | 0.252 | 0.366 | 0.551 | 0.023 | n/a | 3571.4 | 0,0,0 |
| 32 / 128 B | 1 | scoped-blocking | 0.256 | 0.456 | 1.706 | 0.028 | 16.476 | 2733.1 | 16,16,16 |
| 32 / 128 B | 1 | detached | 0.272 | 0.550 | 2.093 | 0.044 | 1.935 | 2926.8 | 16,16,16 |
| 32 / 128 B | 4 | disabled | 1.483 | 1.848 | 2.266 | 1.456 | n/a | 2620.8 | 0,0,0 |
| 32 / 128 B | 4 | scoped-blocking | 1.661 | 2.393 | 22.136 | 1.947 | 24.675 | 1880.6 | 16,16,16 |
| 32 / 128 B | 4 | detached | 1.224 | 3.934 | 7.231 | 3.197 | 1.886 | 2349.8 | 16,16,16 |
| 32 / 128 B | 8 | disabled | 3.894 | 4.888 | 6.399 | 4.361 | n/a | 2015.1 | 0,0,0 |
| 32 / 128 B | 8 | scoped-blocking | 4.019 | 7.859 | 30.819 | 7.182 | 22.490 | 1586.5 | 16,16,16 |
| 32 / 128 B | 8 | detached | 2.641 | 5.854 | 8.985 | 5.378 | 1.901 | 2510.9 | 16,16,16 |
| 512 / 1024 B | 1 | disabled | 0.256 | 0.383 | 0.593 | 0.025 | n/a | 3477.9 | 0,0,0 |
| 512 / 1024 B | 1 | scoped-blocking | 0.346 | 0.508 | 0.663 | 0.046 | 43.509 | 2414.0 | 4,4,4 |
| 512 / 1024 B | 1 | detached | 0.338 | 0.494 | 0.657 | 0.043 | 2.089 | 2739.5 | 3,3,3 |
| 512 / 1024 B | 4 | disabled | 1.283 | 1.674 | 2.114 | 1.328 | n/a | 2998.4 | 0,0,0 |
| 512 / 1024 B | 4 | scoped-blocking | 1.701 | 2.639 | 3.558 | 2.053 | 47.143 | 2089.8 | 9,4,4 |
| 512 / 1024 B | 4 | detached | 1.719 | 2.177 | 3.844 | 1.704 | 2.664 | 2250.0 | 5,3,4 |
| 512 / 1024 B | 8 | disabled | 3.280 | 4.224 | 6.435 | 3.742 | n/a | 2344.6 | 0,0,0 |
| 512 / 1024 B | 8 | scoped-blocking | 3.723 | 4.422 | 31.401 | 3.943 | 41.231 | 1869.5 | 7,5,4 |
| 512 / 1024 B | 8 | detached | 4.075 | 6.509 | 8.432 | 5.822 | 2.812 | 1741.4 | 5,5,4 |

## Pinned foreground reads and process observations

Process RSS is observed at phase boundaries; VmHWM is the process lifetime high-water mark, not a phase heap peak.
| Documents | Readers | Mode | Read p50 ms | Read p95 ms | Read p99 ms | Reads completed during preparation |
| --- | --- | --- | --- | --- | --- | --- |
| 32 | 1 | disabled | 0.380 | 1.102 | 1.309 | 0 |
| 32 | 1 | scoped-blocking | 0.407 | 3.923 | 27.696 | 75 |
| 32 | 1 | detached | 0.586 | 2.228 | 4.485 | 468 |
| 32 | 4 | disabled | 1.734 | 3.464 | 5.124 | 0 |
| 32 | 4 | scoped-blocking | 1.964 | 5.532 | 31.719 | 79 |
| 32 | 4 | detached | 2.254 | 5.591 | 9.940 | 869 |
| 32 | 8 | disabled | 3.838 | 7.394 | 10.979 | 0 |
| 32 | 8 | scoped-blocking | 3.963 | 9.078 | 31.983 | 94 |
| 32 | 8 | detached | 2.623 | 6.957 | 10.465 | 843 |
| 512 | 1 | disabled | 0.308 | 0.531 | 0.802 | 0 |
| 512 | 1 | scoped-blocking | 0.438 | 0.625 | 0.830 | 511 |
| 512 | 1 | detached | 0.417 | 0.649 | 0.848 | 511 |
| 512 | 4 | disabled | 3.764 | 5.398 | 8.390 | 0 |
| 512 | 4 | scoped-blocking | 4.409 | 10.767 | 67.212 | 1975 |
| 512 | 4 | detached | 2.540 | 3.532 | 5.203 | 2004 |
| 512 | 8 | disabled | 4.169 | 5.382 | 8.246 | 0 |
| 512 | 8 | scoped-blocking | 5.510 | 7.077 | 51.897 | 3938 |
| 512 | 8 | detached | 5.060 | 8.164 | 9.983 | 3913 |
| 512 | 8 | detached | 4.071 | 6.585 | 8.541 | 3962 |
| 512 | 8 | scoped-blocking | 3.719 | 4.470 | 12.222 | 3962 |
| 512 | 8 | disabled | 3.277 | 4.220 | 8.325 | 0 |
| 512 | 4 | detached | 1.857 | 2.399 | 3.795 | 2021 |
| 512 | 4 | scoped-blocking | 1.813 | 2.881 | 3.433 | 2012 |
| 512 | 4 | disabled | 1.347 | 1.728 | 1.996 | 0 |
| 512 | 1 | detached | 0.415 | 0.614 | 0.758 | 511 |
| 512 | 1 | scoped-blocking | 0.433 | 0.655 | 0.859 | 511 |
| 512 | 1 | disabled | 0.390 | 1.199 | 1.700 | 0 |
| 32 | 8 | detached | 4.430 | 9.439 | 14.420 | 913 |
| 32 | 8 | scoped-blocking | 4.024 | 9.844 | 36.057 | 103 |
| 32 | 8 | disabled | 3.865 | 7.607 | 10.955 | 0 |
| 32 | 4 | detached | 1.614 | 5.394 | 8.819 | 840 |
| 32 | 4 | scoped-blocking | 1.219 | 3.533 | 18.368 | 113 |
| 32 | 4 | disabled | 1.213 | 2.175 | 2.713 | 0 |
| 32 | 1 | detached | 0.434 | 1.228 | 2.808 | 477 |
| 32 | 1 | scoped-blocking | 0.305 | 2.459 | 17.221 | 105 |
| 32 | 1 | disabled | 0.288 | 0.538 | 0.672 | 0 |
| 32 | 1 | disabled | 0.282 | 0.502 | 0.613 | 0 |
| 32 | 1 | scoped-blocking | 0.312 | 2.326 | 16.482 | 102 |
| 32 | 1 | detached | 0.434 | 1.144 | 4.081 | 477 |
| 32 | 4 | disabled | 1.573 | 2.603 | 3.172 | 0 |
| 32 | 4 | scoped-blocking | 1.766 | 4.737 | 23.249 | 119 |
| 32 | 4 | detached | 1.590 | 3.638 | 8.344 | 899 |
| 32 | 8 | disabled | 2.562 | 5.061 | 7.942 | 0 |
| 32 | 8 | scoped-blocking | 2.574 | 6.625 | 20.680 | 121 |
| 32 | 8 | detached | 2.592 | 6.115 | 9.544 | 900 |
| 512 | 1 | disabled | 0.321 | 0.577 | 1.073 | 0 |
| 512 | 1 | scoped-blocking | 0.437 | 0.606 | 0.782 | 511 |
| 512 | 1 | detached | 0.413 | 0.628 | 0.743 | 509 |
| 512 | 4 | disabled | 1.369 | 1.709 | 2.051 | 0 |
| 512 | 4 | scoped-blocking | 1.801 | 2.235 | 3.826 | 2017 |
| 512 | 4 | detached | 1.784 | 2.167 | 5.641 | 2018 |
| 512 | 8 | disabled | 2.838 | 3.584 | 5.608 | 0 |
| 512 | 8 | scoped-blocking | 3.597 | 4.367 | 26.437 | 3964 |
| 512 | 8 | detached | 3.752 | 4.378 | 6.529 | 3956 |

# Consumer publication measurements

Median of per-run percentiles across repetitions; all individual samples are retained in results.jsonl.
Publication callback time is a lower bound on the outer owner critical section and excludes lock-entry/drop overhead.
Quiescent publication samples cannot establish simultaneous source-write/Consumer-watermark progress.

| Documents / payload | Writers | Mode | Write p50 ms | Write p95 ms | Write p99 ms | Wait p95 ms | Quiet publish p50 ms | Writes/s | Active advances |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 32 / 128 B | 1 | disabled | 0.329 | 0.478 | 0.731 | 0.032 | n/a | 2796.8 | 0,0,0 |
| 32 / 128 B | 1 | scoped-blocking | 0.330 | 0.496 | 2.305 | 0.041 | 23.582 | 2108.6 | 16,16,16 |
| 32 / 128 B | 1 | detached | 0.345 | 0.531 | 3.115 | 0.039 | 3.198 | 2423.2 | 16,16,16 |
| 32 / 128 B | 4 | disabled | 1.558 | 1.940 | 3.548 | 1.523 | n/a | 2500.1 | 0,0,0 |
| 32 / 128 B | 4 | scoped-blocking | 1.545 | 2.595 | 25.172 | 2.078 | 23.307 | 1949.2 | 16,16,16 |
| 32 / 128 B | 4 | detached | 1.576 | 4.075 | 5.053 | 3.583 | 3.164 | 2225.3 | 16,16,16 |
| 32 / 128 B | 8 | disabled | 3.334 | 5.121 | 6.120 | 4.699 | n/a | 2270.0 | 0,0,0 |
| 32 / 128 B | 8 | scoped-blocking | 3.331 | 7.011 | 27.468 | 6.591 | 24.261 | 1811.7 | 16,16,16 |
| 32 / 128 B | 8 | detached | 3.431 | 6.577 | 9.826 | 6.170 | 3.187 | 2061.3 | 16,16,16 |
| 512 / 1024 B | 1 | disabled | 0.338 | 0.468 | 0.729 | 0.032 | n/a | 2755.1 | 0,0,0 |
| 512 / 1024 B | 1 | scoped-blocking | 0.386 | 0.519 | 0.982 | 0.031 | 73.874 | 2321.6 | 1,1,1 |
| 512 / 1024 B | 1 | detached | 0.372 | 0.499 | 0.917 | 0.034 | 3.405 | 2510.6 | 1,1,1 |
| 512 / 1024 B | 4 | disabled | 1.654 | 2.351 | 4.092 | 1.843 | n/a | 2287.5 | 0,0,0 |
| 512 / 1024 B | 4 | scoped-blocking | 1.707 | 3.462 | 4.980 | 2.883 | 74.770 | 1982.6 | 1,1,1 |
| 512 / 1024 B | 4 | detached | 1.703 | 3.609 | 5.363 | 3.006 | 3.465 | 2045.5 | 1,1,1 |
| 512 / 1024 B | 8 | disabled | 3.777 | 6.613 | 8.554 | 6.119 | n/a | 1984.0 | 0,0,0 |
| 512 / 1024 B | 8 | scoped-blocking | 3.973 | 8.206 | 11.494 | 7.660 | 75.209 | 1749.8 | 1,1,1 |
| 512 / 1024 B | 8 | detached | 4.029 | 8.415 | 11.441 | 7.796 | 3.430 | 1793.7 | 1,1,1 |

## Pinned foreground reads and process observations

Process RSS is observed at phase boundaries; VmHWM is the process lifetime high-water mark, not a phase heap peak.
| Documents | Readers | Mode | Read p50 ms | Read p95 ms | Read p99 ms | Reads completed during preparation |
| --- | --- | --- | --- | --- | --- | --- |
| 32 | 1 | disabled | 0.382 | 0.552 | 0.819 | 0 |
| 32 | 1 | scoped-blocking | 0.444 | 2.420 | 28.345 | 447 |
| 32 | 1 | detached | 0.426 | 1.079 | 3.620 | 473 |
| 32 | 4 | disabled | 1.616 | 1.965 | 3.796 | 0 |
| 32 | 4 | scoped-blocking | 1.683 | 5.216 | 29.712 | 571 |
| 32 | 4 | detached | 1.816 | 4.912 | 6.296 | 1396 |
| 32 | 8 | disabled | 3.437 | 4.560 | 6.556 | 0 |
| 32 | 8 | scoped-blocking | 3.541 | 7.104 | 28.243 | 560 |
| 32 | 8 | detached | 3.543 | 7.027 | 9.872 | 1544 |
| 512 | 1 | disabled | 0.757 | 0.983 | 1.482 | 0 |
| 512 | 1 | scoped-blocking | 0.863 | 1.140 | 2.612 | 511 |
| 512 | 1 | detached | 0.863 | 1.093 | 2.280 | 511 |
| 512 | 4 | disabled | 2.125 | 3.104 | 4.603 | 0 |
| 512 | 4 | scoped-blocking | 2.099 | 3.924 | 5.115 | 2043 |
| 512 | 4 | detached | 2.074 | 3.867 | 5.247 | 2044 |
| 512 | 8 | disabled | 4.044 | 6.681 | 8.322 | 0 |
| 512 | 8 | scoped-blocking | 3.831 | 7.280 | 9.887 | 4082 |
| 512 | 8 | detached | 2.452 | 7.441 | 10.169 | 4080 |
| 512 | 8 | detached | 3.923 | 7.670 | 10.525 | 4065 |
| 512 | 8 | scoped-blocking | 2.420 | 7.847 | 10.311 | 4052 |
| 512 | 8 | disabled | 3.973 | 6.609 | 8.598 | 0 |
| 512 | 4 | detached | 2.084 | 3.874 | 5.225 | 2044 |
| 512 | 4 | scoped-blocking | 2.076 | 3.911 | 5.049 | 2037 |
| 512 | 4 | disabled | 2.117 | 3.065 | 5.272 | 0 |
| 512 | 1 | detached | 0.837 | 1.091 | 1.886 | 511 |
| 512 | 1 | scoped-blocking | 0.838 | 1.118 | 1.744 | 511 |
| 512 | 1 | disabled | 0.757 | 0.952 | 1.319 | 0 |
| 32 | 8 | detached | 3.336 | 6.641 | 9.995 | 1881 |
| 32 | 8 | scoped-blocking | 3.318 | 6.582 | 27.485 | 524 |
| 32 | 8 | disabled | 3.281 | 5.277 | 7.783 | 0 |
| 32 | 4 | detached | 1.758 | 4.663 | 5.918 | 1477 |
| 32 | 4 | scoped-blocking | 1.656 | 4.702 | 27.609 | 511 |
| 32 | 4 | disabled | 1.642 | 2.008 | 3.913 | 0 |
| 32 | 1 | detached | 0.441 | 1.035 | 3.744 | 474 |
| 32 | 1 | scoped-blocking | 0.434 | 1.497 | 24.288 | 451 |
| 32 | 1 | disabled | 0.374 | 0.608 | 0.827 | 0 |
| 32 | 1 | disabled | 0.375 | 0.512 | 0.824 | 0 |
| 32 | 1 | scoped-blocking | 0.431 | 2.826 | 24.460 | 445 |
| 32 | 1 | detached | 0.429 | 1.116 | 3.583 | 476 |
| 32 | 4 | disabled | 1.591 | 1.898 | 3.590 | 0 |
| 32 | 4 | scoped-blocking | 1.633 | 4.692 | 26.225 | 528 |
| 32 | 4 | detached | 1.758 | 4.615 | 5.243 | 1487 |
| 32 | 8 | disabled | 3.243 | 4.974 | 6.394 | 0 |
| 32 | 8 | scoped-blocking | 3.290 | 7.518 | 27.203 | 807 |
| 32 | 8 | detached | 3.402 | 6.659 | 9.164 | 1824 |
| 512 | 1 | disabled | 0.722 | 0.903 | 1.290 | 0 |
| 512 | 1 | scoped-blocking | 0.858 | 1.064 | 2.291 | 511 |
| 512 | 1 | detached | 0.832 | 1.064 | 2.366 | 511 |
| 512 | 4 | disabled | 2.142 | 3.086 | 4.240 | 0 |
| 512 | 4 | scoped-blocking | 1.950 | 3.946 | 5.499 | 2043 |
| 512 | 4 | detached | 1.798 | 3.752 | 5.066 | 2042 |
| 512 | 8 | disabled | 3.921 | 6.787 | 8.510 | 0 |
| 512 | 8 | scoped-blocking | 3.933 | 7.524 | 9.992 | 4071 |
| 512 | 8 | detached | 1.002 | 7.708 | 10.239 | 4095 |

# Consumer publication measurements

Median of per-run percentiles across repetitions; all individual samples are retained in results.jsonl.
Publication callback time is a lower bound on the outer owner critical section and excludes lock-entry/drop overhead.
Quiescent publication samples cannot establish simultaneous source-write/Consumer-watermark progress.

| Documents / payload | Writers | Mode | Write p50 ms | Write p95 ms | Write p99 ms | Wait p95 ms | Quiet publish p50 ms | Writes/s | Active advances |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 32 / 128 B | 1 | disabled | 4.009 | 8.319 | 16.317 | 0.001 | n/a | 191.7 | 0,0,0 |
| 32 / 128 B | 1 | legacy | 4.635 | 14.157 | 456.733 | 0.001 | 639.102 | 53.0 | 14,38,81 |
| 32 / 128 B | 1 | covered | 4.692 | 13.879 | 96.144 | 0.001 | 506.052 | 125.9 | 0,0,0 |
| 32 / 128 B | 4 | disabled | 7.001 | 135.341 | 535.830 | 126.098 | n/a | 101.9 | 0,0,0 |
| 32 / 128 B | 4 | legacy | 6.267 | 679.505 | 1497.540 | 672.383 | 551.511 | 36.7 | 2,26,36 |
| 32 / 128 B | 4 | covered | 4.713 | 105.525 | 265.665 | 99.228 | 485.545 | 155.4 | 0,0,0 |
| 32 / 128 B | 8 | disabled | 6.209 | 182.859 | 761.030 | 178.551 | n/a | 131.4 | 0,0,0 |
| 32 / 128 B | 8 | legacy | 4.649 | 104.969 | 1013.164 | 83.445 | 529.005 | 82.7 | 3,20,9 |
| 32 / 128 B | 8 | covered | 6.530 | 83.978 | 834.429 | 16.166 | 457.243 | 100.3 | 0,0,0 |
| 512 / 1024 B | 1 | disabled | 4.170 | 10.425 | 109.808 | 0.000 | n/a | 132.3 | 0,0,0 |
| 512 / 1024 B | 1 | legacy | 4.732 | 37.362 | 729.976 | 0.001 | 764.075 | 35.4 | 6,29,54 |
| 512 / 1024 B | 1 | covered | 2.001 | 6.007 | 78.309 | 0.000 | 462.831 | 261.7 | 0,0,0 |
| 512 / 1024 B | 4 | disabled | 4.838 | 37.857 | 357.520 | 14.978 | n/a | 148.7 | 0,0,0 |
| 512 / 1024 B | 4 | legacy | 4.685 | 85.252 | 1392.178 | 0.001 | 678.721 | 66.7 | 24,8,5 |
| 512 / 1024 B | 4 | covered | 8.692 | 81.411 | 429.413 | 9.977 | 662.591 | 98.1 | 0,0,0 |
| 512 / 1024 B | 8 | disabled | 4.799 | 17.891 | 547.315 | 0.001 | n/a | 144.1 | 0,0,0 |
| 512 / 1024 B | 8 | legacy | 5.073 | 157.169 | 1109.384 | 0.001 | 467.394 | 73.3 | 9,2,16 |
| 512 / 1024 B | 8 | covered | 7.917 | 110.159 | 922.693 | 0.001 | 439.191 | 107.8 | 0,0,0 |

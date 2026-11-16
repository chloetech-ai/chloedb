Machine: {"arch":"aarch64","cores":2,"cpu":null,"memory_gb":null,"os":"Linux 6.8.0-117-generic","setup":"Docker (colima, vz, 8 vCPUs, 16 GB) on an Apple M1 Pro, 10 cores, 32 GB, macOS 27.0.1: one shard of 1000000 rows, each of its 3 copies a container on 2 vCPUs of its own (cpusets 2-3 4-5 6-7), the coordinator, clients and delay lines a container on vCPUs 0-1, one Docker network"}


### busy

| clients | config | round | qps | p50 ms | p90 ms | p99 ms | p99.9 ms | max ms | read-after-write p50 / p99 / max ms | errors | hedges | 429s | timeouts | avoided | shard 0 answers by copy |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | round-robin | 0 | 520.0 | 1.95 | 2.13 | 2.46 | 4.7 | 5.2 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1733, 1734, 1733]] |
| 1 | least-cost | 0 | 520.0 | 1.95 | 2.13 | 2.58 | 4.49 | 5.33 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1258, 1971, 1970]] |
| 1 | least-cost no-load | 0 | 521.0 | 1.94 | 2.12 | 2.44 | 4.97 | 5.46 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1738, 1739, 1739]] |
| 1 | round-robin | 1 | 519.0 | 1.95 | 2.12 | 2.61 | 4.95 | 5.91 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1732, 1733, 1732]] |
| 1 | least-cost | 1 | 521.0 | 1.95 | 2.1 | 2.54 | 5.02 | 5.74 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1737, 1738, 1738]] |
| 1 | least-cost no-load | 1 | 526.0 | 1.93 | 2.07 | 2.4 | 4.83 | 5.35 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1755, 1755, 1755]] |
| 16 | round-robin | 0 | 7075.0 | 2.12 | 3.26 | 4.55 | 5.5 | 8.79 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[23605, 23608, 23607]] |
| 16 | least-cost | 0 | 6711.0 | 2.19 | 3.54 | 4.78 | 6.13 | 8.53 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[18117, 24524, 24535]] |
| 16 | least-cost no-load | 0 | 6871.0 | 2.16 | 3.4 | 4.71 | 5.76 | 7.76 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[21446, 23629, 23696]] |
| 16 | round-robin | 1 | 7058.0 | 2.12 | 3.26 | 4.58 | 5.57 | 7.83 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[23538, 23539, 23540]] |
| 16 | least-cost | 1 | 6783.0 | 2.18 | 3.48 | 4.69 | 5.55 | 7.48 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[19132, 24535, 24220]] |
| 16 | least-cost no-load | 1 | 6883.0 | 2.15 | 3.4 | 4.7 | 5.69 | 7.49 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[21472, 23663, 23753]] |
| 64 | round-robin | 0 | 12225.0 | 4.97 | 7.67 | 10.87 | 13.42 | 20.26 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[40794, 40816, 40827]] |
| 64 | least-cost | 0 | 15623.0 | 3.95 | 5.78 | 8.06 | 10.49 | 22.37 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[40107, 50655, 65669]] |
| 64 | least-cost no-load | 0 | 16095.0 | 3.87 | 5.51 | 7.29 | 9.28 | 24.17 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[42002, 59766, 59387]] |
| 64 | round-robin | 1 | 12598.0 | 3.17 | 10.58 | 13.04 | 16.66 | 23.83 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[42017, 42049, 42049]] |
| 64 | least-cost | 1 | 16493.0 | 3.8 | 5.29 | 7.1 | 9.8 | 26.49 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[37705, 63952, 63519]] |
| 64 | least-cost no-load | 1 | 16228.0 | 3.84 | 5.51 | 7.31 | 9.55 | 32.18 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[41818, 60551, 60134]] |

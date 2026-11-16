Machine: {"arch":"aarch64","cores":2,"cpu":null,"memory_gb":null,"os":"Linux 6.8.0-117-generic","setup":"Docker (colima, vz, 8 vCPUs, 16 GB) on an Apple M1 Pro, 10 cores, 32 GB, macOS 27.0.1: one shard of 1000000 rows, each of its 3 copies a container on 2 vCPUs of its own (cpusets 2-3 4-5 6-7), the coordinator, clients and delay lines a container on vCPUs 0-1, one Docker network"}


### equal

| clients | config | round | qps | p50 ms | p90 ms | p99 ms | p99.9 ms | max ms | read-after-write p50 / p99 / max ms | errors | hedges | 429s | timeouts | avoided | shard 0 answers by copy |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | round-robin | 0 | 514.0 | 1.96 | 2.15 | 2.56 | 5.0 | 5.37 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1715, 1716, 1715]] |
| 1 | round-robin adaptive | 0 | 521.0 | 1.94 | 2.13 | 2.46 | 4.91 | 6.14 | null / null / null | 0 | 11 | 0 | 0 | 0.0 | [[1737, 1736, 1737]] |
| 1 | least-cost | 0 | 522.0 | 1.94 | 2.12 | 2.46 | 4.82 | 5.28 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1740, 1740, 1740]] |
| 1 | least-cost adaptive | 0 | 517.0 | 1.95 | 2.13 | 2.61 | 5.4 | 6.26 | null / null / null | 0 | 22 | 0 | 0 | 0.0 | [[1724, 1723, 1723]] |
| 1 | round-robin | 1 | 513.0 | 1.97 | 2.16 | 2.58 | 4.9 | 5.75 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1712, 1712, 1711]] |
| 1 | round-robin adaptive | 1 | 523.0 | 1.94 | 2.1 | 2.39 | 3.89 | 5.97 | null / null / null | 0 | 8 | 0 | 0 | 0.0 | [[1744, 1744, 1744]] |
| 1 | least-cost | 1 | 520.0 | 1.95 | 2.11 | 2.43 | 4.92 | 5.03 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1736, 1735, 1736]] |
| 1 | least-cost adaptive | 1 | 525.0 | 1.94 | 2.11 | 2.42 | 3.3 | 6.08 | null / null / null | 0 | 4 | 0 | 0 | 0.0 | [[1749, 1750, 1750]] |
| 16 | round-robin | 0 | 6576.0 | 2.23 | 3.69 | 4.71 | 5.47 | 7.86 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[21938, 21939, 21938]] |
| 16 | round-robin adaptive | 0 | 6601.0 | 2.22 | 3.68 | 4.76 | 5.55 | 7.86 | null / null / null | 0 | 2 | 0 | 0 | 0.0 | [[22023, 22023, 22023]] |
| 16 | least-cost | 0 | 6612.0 | 2.22 | 3.64 | 4.76 | 5.89 | 8.59 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[22080, 21915, 22193]] |
| 16 | least-cost adaptive | 0 | 6538.0 | 2.24 | 3.72 | 4.74 | 5.43 | 7.6 | null / null / null | 0 | 2 | 0 | 0 | 0.0 | [[21773, 21807, 21879]] |
| 16 | round-robin | 1 | 6533.0 | 2.24 | 3.73 | 4.76 | 5.67 | 6.9 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[21810, 21808, 21805]] |
| 16 | round-robin adaptive | 1 | 6674.0 | 2.2 | 3.62 | 4.67 | 5.62 | 10.47 | null / null / null | 0 | 9 | 0 | 0 | 0.0 | [[22270, 22264, 22275]] |
| 16 | least-cost | 1 | 6629.0 | 2.22 | 3.64 | 4.73 | 5.48 | 7.06 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[22146, 22097, 22117]] |
| 16 | least-cost adaptive | 1 | 6594.0 | 2.23 | 3.66 | 4.72 | 5.53 | 7.24 | null / null / null | 0 | 2 | 0 | 0 | 0.0 | [[22200, 21873, 21929]] |
| 64 | round-robin | 0 | 12250.0 | 5.02 | 7.31 | 10.33 | 12.91 | 19.15 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[40892, 40895, 40894]] |
| 64 | round-robin adaptive | 0 | 12303.0 | 5.0 | 7.24 | 10.25 | 12.61 | 17.33 | null / null / null | 0 | 79 | 0 | 0 | 0.0 | [[41072, 41074, 41071]] |
| 64 | least-cost | 0 | 12357.0 | 4.97 | 7.21 | 10.08 | 12.82 | 18.45 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[41325, 41281, 41134]] |
| 64 | least-cost adaptive | 0 | 12264.0 | 5.02 | 7.25 | 10.12 | 12.62 | 16.34 | null / null / null | 0 | 35 | 0 | 0 | 0.0 | [[40825, 41094, 41000]] |
| 64 | round-robin | 1 | 12130.0 | 5.08 | 7.37 | 10.39 | 12.92 | 16.87 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[40502, 40519, 40500]] |
| 64 | round-robin adaptive | 1 | 11936.0 | 5.15 | 7.54 | 10.59 | 13.17 | 17.55 | null / null / null | 0 | 53 | 0 | 0 | 0.0 | [[39853, 39862, 39863]] |
| 64 | least-cost | 1 | 12354.0 | 4.99 | 7.18 | 9.96 | 12.6 | 16.59 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[41506, 41285, 40988]] |
| 64 | least-cost adaptive | 1 | 12276.0 | 5.02 | 7.22 | 10.13 | 13.08 | 16.97 | null / null / null | 0 | 103 | 0 | 0 | 0.0 | [[40456, 41179, 41286]] |

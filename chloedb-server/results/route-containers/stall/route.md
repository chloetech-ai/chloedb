Machine: {"arch":"aarch64","cores":2,"cpu":null,"memory_gb":null,"os":"Linux 6.8.0-117-generic","setup":"Docker (colima, vz, 8 vCPUs, 16 GB) on an Apple M1 Pro, 10 cores, 32 GB, macOS 27.0.1: one shard of 1000000 rows, each of its 3 copies a container on 2 vCPUs of its own (cpusets 2-3 4-5 6-7), the coordinator, clients and delay lines a container on vCPUs 0-1, one Docker network"}


### stall

| clients | config | round | qps | p50 ms | p90 ms | p99 ms | p99.9 ms | max ms | read-after-write p50 / p99 / max ms | errors | hedges | 429s | timeouts | avoided | shard 0 answers by copy |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | round-robin | 0 | 488.0 | 1.94 | 2.13 | 2.55 | 64.04 | 68.65 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1629, 1630, 1629]] |
| 1 | round-robin adaptive | 0 | 509.0 | 1.94 | 2.13 | 5.78 | 6.96 | 7.91 | null / null / null | 0 | 74 | 0 | 0 | 0.0 | [[1638, 1759, 1698]] |
| 1 | least-cost | 0 | 522.0 | 1.94 | 2.09 | 2.42 | 5.04 | 63.81 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[61, 2572, 2593]] |
| 1 | least-cost adaptive | 0 | 525.0 | 1.93 | 2.08 | 2.44 | 4.41 | 16.99 | null / null / null | 0 | 14 | 0 | 0 | 0.0 | [[122, 2547, 2589]] |
| 1 | round-robin | 1 | 495.0 | 1.92 | 2.08 | 2.45 | 63.6 | 68.17 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1651, 1651, 1651]] |
| 1 | round-robin adaptive | 1 | 508.0 | 1.95 | 2.12 | 5.86 | 7.4 | 8.88 | null / null / null | 0 | 66 | 0 | 0 | 0.0 | [[1634, 1753, 1693]] |
| 1 | least-cost | 1 | 523.0 | 1.93 | 2.09 | 2.4 | 4.74 | 62.25 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1, 2612, 2625]] |
| 1 | least-cost adaptive | 1 | 525.0 | 1.93 | 2.1 | 2.43 | 3.35 | 27.87 | null / null / null | 0 | 5 | 0 | 0 | 0.0 | [[33, 2598, 2624]] |
| 16 | round-robin | 0 | 6300.0 | 2.21 | 3.6 | 4.71 | 61.85 | 65.92 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[21024, 21028, 21023]] |
| 16 | round-robin adaptive | 0 | 6200.0 | 2.25 | 3.7 | 4.86 | 51.6 | 62.65 | null / null / null | 0 | 121 | 0 | 0 | 0.0 | [[20647, 20887, 20768]] |
| 16 | least-cost | 0 | 6412.0 | 2.24 | 3.62 | 4.68 | 59.03 | 63.79 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[20143, 21884, 22152]] |
| 16 | least-cost adaptive | 0 | 6529.0 | 2.23 | 3.65 | 4.85 | 10.29 | 46.51 | null / null / null | 0 | 247 | 0 | 0 | 0.0 | [[20452, 22534, 22376]] |
| 16 | round-robin | 1 | 6245.0 | 2.22 | 3.62 | 4.72 | 62.66 | 67.6 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[20906, 20910, 20905]] |
| 16 | round-robin adaptive | 1 | 6224.0 | 2.24 | 3.68 | 4.84 | 52.75 | 63.34 | null / null / null | 0 | 120 | 0 | 0 | 0.0 | [[20645, 20885, 20765]] |
| 16 | least-cost | 1 | 6332.0 | 2.26 | 3.67 | 4.81 | 59.34 | 64.12 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[19774, 21742, 21864]] |
| 16 | least-cost adaptive | 1 | 6591.0 | 2.22 | 3.59 | 4.76 | 9.84 | 45.27 | null / null / null | 0 | 230 | 0 | 0 | 0.0 | [[20575, 22682, 22718]] |
| 64 | round-robin | 0 | 11512.0 | 5.05 | 7.52 | 11.79 | 65.39 | 71.41 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[38426, 38441, 38438]] |
| 64 | round-robin adaptive | 0 | 11861.0 | 4.9 | 7.29 | 11.64 | 64.78 | 71.49 | null / null / null | 0 | 224 | 0 | 0 | 0.0 | [[39462, 39759, 39627]] |
| 64 | least-cost | 0 | 12323.0 | 4.86 | 7.14 | 10.56 | 63.73 | 67.79 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[38448, 42857, 42120]] |
| 64 | least-cost adaptive | 0 | 12094.0 | 5.02 | 7.26 | 10.64 | 56.54 | 69.3 | null / null / null | 0 | 270 | 0 | 0 | 0.0 | [[37562, 41570, 41973]] |
| 64 | round-robin | 1 | 11710.0 | 4.97 | 7.38 | 11.6 | 63.98 | 68.37 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[39097, 39094, 39099]] |
| 64 | round-robin adaptive | 1 | 11359.0 | 5.11 | 7.71 | 12.21 | 63.58 | 70.48 | null / null / null | 0 | 212 | 0 | 0 | 0.0 | [[37794, 38074, 37934]] |
| 64 | least-cost | 1 | 12285.0 | 4.9 | 7.11 | 10.42 | 63.43 | 68.45 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[38558, 42579, 41922]] |
| 64 | least-cost adaptive | 1 | 12262.0 | 4.95 | 7.17 | 10.34 | 59.52 | 70.7 | null / null / null | 0 | 244 | 0 | 0 | 0.0 | [[38208, 42154, 42483]] |

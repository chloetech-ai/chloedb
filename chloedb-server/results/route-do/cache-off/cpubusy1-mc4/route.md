Machine: {"arch":"x86_64","cores":32,"cpu":"Intel(R) Xeon(R) Platinum 8358 CPU @ 2.60GHz","memory_gb":null,"os":"Linux 6.12.113+deb13-amd64","setup":"DigitalOcean nyc1, DOKS 1.37.2: one shard of 1M rows, each of its 3 copies a pod on a c-2 node of its own (2 dedicated vCPUs), the coordinator, clients and delay lines one pod on a c-32 node (32 dedicated vCPUs), pod network; every server with --answer-cache-mb 0 (no answer cache); every copy with --max-concurrent 4, and the node of copy 0 also runs stress-ng --cpu 2 (two CPU-bound threads at the priority of the server) (asked by the simulation thread)"}


### equal

| clients | config | round | qps | p50 ms | p90 ms | p99 ms | p99.9 ms | max ms | read-after-write p50 / p99 / max ms | errors | hedges | 429s | timeouts | avoided | shard 0 answers by copy |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | round-robin | 0 | 259.0 | 3.35 | 5.6 | 10.28 | 14.23 | 35.22 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[863, 864, 864]] |
| 1 | least-cost | 0 | 346.0 | 2.79 | 3.34 | 4.56 | 7.24 | 8.97 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1, 1737, 1727]] |
| 1 | round-robin | 1 | 303.0 | 2.98 | 4.28 | 7.57 | 11.46 | 16.95 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1012, 1012, 1012]] |
| 1 | least-cost | 1 | 351.0 | 2.76 | 3.3 | 4.61 | 6.59 | 8.06 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1, 1737, 1779]] |
| 1 | round-robin | 2 | 301.0 | 3.0 | 4.32 | 8.35 | 12.85 | 17.74 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1003, 1003, 1003]] |
| 1 | least-cost | 2 | 363.0 | 2.66 | 3.2 | 4.25 | 6.41 | 7.04 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1, 1807, 1823]] |
| 1 | round-robin | 3 | 302.0 | 2.99 | 4.23 | 7.65 | 13.32 | 22.77 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1008, 1008, 1008]] |
| 1 | least-cost | 3 | 343.0 | 2.81 | 3.4 | 5.15 | 7.4 | 15.84 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[42, 1688, 1702]] |
| 1 | round-robin | 4 | 306.0 | 2.93 | 4.24 | 8.06 | 10.9 | 27.58 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1021, 1020, 1020]] |
| 1 | least-cost | 4 | 349.0 | 2.75 | 3.33 | 5.3 | 7.66 | 11.06 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[85, 1717, 1699]] |
| 16 | round-robin | 0 | 3718.0 | 3.17 | 8.17 | 15.8 | 24.42 | 52.93 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[12413, 12415, 12415]] |
| 16 | least-cost | 0 | 4305.0 | 3.23 | 5.81 | 10.07 | 16.1 | 32.92 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[10677, 17475, 14965]] |
| 16 | round-robin | 1 | 3676.0 | 3.21 | 8.25 | 15.84 | 23.69 | 31.61 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[12273, 12266, 12272]] |
| 16 | least-cost | 1 | 4310.0 | 3.24 | 5.68 | 9.99 | 16.58 | 47.09 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[10257, 17532, 15359]] |
| 16 | round-robin | 2 | 3772.0 | 3.19 | 7.88 | 15.16 | 24.13 | 45.32 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[12585, 12586, 12585]] |
| 16 | least-cost | 2 | 4300.0 | 3.27 | 5.72 | 9.63 | 15.1 | 35.3 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[10234, 17227, 15640]] |
| 16 | round-robin | 3 | 3712.0 | 3.16 | 8.17 | 16.13 | 23.89 | 59.31 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[12390, 12388, 12388]] |
| 16 | least-cost | 3 | 4342.0 | 3.21 | 5.68 | 9.85 | 15.38 | 25.04 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[10097, 17179, 16196]] |
| 16 | round-robin | 4 | 3719.0 | 3.21 | 8.05 | 15.53 | 26.56 | 51.5 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[12435, 12434, 12437]] |
| 16 | least-cost | 4 | 4348.0 | 3.22 | 5.58 | 9.88 | 16.08 | 40.44 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[9975, 17903, 15639]] |
| 64 | round-robin | 0 | 3976.0 | 3.51 | 45.02 | 58.29 | 70.08 | 80.38 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[13296, 13288, 13288]] |
| 64 | least-cost | 0 | 5297.0 | 11.23 | 17.31 | 24.98 | 33.23 | 68.02 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[13430, 20883, 18779]] |
| 64 | round-robin | 1 | 3418.0 | 9.64 | 44.25 | 66.94 | 85.23 | 103.67 | null / null / null | 0 | 1 | 0 | 0 | 0.49 | [[6049, 16788, 11418]] |
| 64 | least-cost | 1 | 5397.0 | 11.12 | 16.91 | 24.76 | 33.82 | 47.5 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[13469, 21429, 19190]] |
| 64 | round-robin | 2 | 3781.0 | 3.5 | 47.66 | 60.09 | 70.57 | 82.39 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[12620, 12624, 12624]] |
| 64 | least-cost | 2 | 5337.0 | 11.2 | 17.01 | 25.01 | 35.04 | 57.24 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[13530, 21299, 18688]] |
| 64 | round-robin | 3 | 3908.0 | 3.58 | 46.8 | 61.21 | 75.05 | 86.7 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[13065, 13055, 13056]] |
| 64 | least-cost | 3 | 5277.0 | 11.45 | 17.16 | 24.95 | 32.07 | 46.83 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[13240, 20983, 18641]] |
| 64 | round-robin | 4 | 3980.0 | 3.59 | 45.02 | 58.39 | 75.46 | 91.58 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[13279, 13287, 13287]] |
| 64 | least-cost | 4 | 5183.0 | 11.59 | 17.79 | 26.75 | 38.86 | 75.11 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[13280, 20755, 17901]] |

Machine: {"arch":"x86_64","cores":32,"cpu":"Intel(R) Xeon(R) Platinum 8358 CPU @ 2.60GHz","memory_gb":null,"os":"Linux 6.12.113+deb13-amd64","setup":"DigitalOcean nyc1, DOKS 1.37.2: one shard of 1M rows, each of its 3 copies a pod on a c-2 node of its own (2 dedicated vCPUs), the coordinator, clients and delay lines one pod on a c-32 node (32 dedicated vCPUs), pod network; every server with --answer-cache-mb 0 (no answer cache); every copy with --max-concurrent 2, and the node of copy 0 also runs stress-ng --cpu 2 (two CPU-bound threads at the priority of the server) (asked by the simulation thread)"}


### equal

| clients | config | round | qps | p50 ms | p90 ms | p99 ms | p99.9 ms | max ms | read-after-write p50 / p99 / max ms | errors | hedges | 429s | timeouts | avoided | shard 0 answers by copy |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | round-robin | 0 | 258.0 | 3.29 | 5.79 | 10.91 | 14.19 | 32.23 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[862, 862, 863]] |
| 1 | least-cost | 0 | 350.0 | 2.77 | 3.33 | 4.65 | 6.37 | 8.6 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[3, 1709, 1793]] |
| 1 | round-robin | 1 | 299.0 | 3.01 | 4.38 | 8.08 | 11.09 | 15.77 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[998, 999, 998]] |
| 1 | least-cost | 1 | 339.0 | 2.82 | 3.42 | 5.51 | 10.08 | 12.01 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[131, 1804, 1465]] |
| 1 | round-robin | 2 | 308.0 | 2.94 | 4.19 | 7.77 | 13.64 | 16.77 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1027, 1027, 1028]] |
| 1 | least-cost | 2 | 351.0 | 2.72 | 3.33 | 5.24 | 9.94 | 13.93 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[71, 1605, 1841]] |
| 1 | round-robin | 3 | 318.0 | 2.81 | 4.16 | 7.29 | 10.41 | 17.49 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1062, 1062, 1063]] |
| 1 | least-cost | 3 | 365.0 | 2.62 | 3.16 | 4.82 | 6.89 | 20.28 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[11, 1813, 1829]] |
| 1 | round-robin | 4 | 315.0 | 2.84 | 4.18 | 8.19 | 12.91 | 19.92 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1050, 1050, 1050]] |
| 1 | least-cost | 4 | 328.0 | 2.87 | 3.58 | 6.61 | 10.9 | 14.28 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[419, 1334, 1537]] |
| 16 | round-robin | 0 | 3359.0 | 3.18 | 9.82 | 16.73 | 26.44 | 41.46 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[11211, 11207, 11207]] |
| 16 | least-cost | 0 | 4156.0 | 3.38 | 5.92 | 10.17 | 16.27 | 25.93 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[10196, 16569, 14834]] |
| 16 | round-robin | 1 | 3335.0 | 3.2 | 9.95 | 17.05 | 24.32 | 40.08 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[11133, 11131, 11131]] |
| 16 | least-cost | 1 | 4112.0 | 3.42 | 6.01 | 10.22 | 16.04 | 35.06 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[10195, 15997, 14988]] |
| 16 | round-robin | 2 | 3004.0 | 3.02 | 12.02 | 18.87 | 27.87 | 35.83 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[10031, 10027, 10025]] |
| 16 | least-cost | 2 | 4081.0 | 3.52 | 6.01 | 10.08 | 15.18 | 26.88 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[9738, 15411, 15699]] |
| 16 | round-robin | 3 | 3227.0 | 3.09 | 10.72 | 17.8 | 28.12 | 55.92 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[10780, 10774, 10775]] |
| 16 | least-cost | 3 | 4151.0 | 3.47 | 5.88 | 9.54 | 14.16 | 30.03 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[9790, 16219, 15572]] |
| 16 | round-robin | 4 | 3331.0 | 3.2 | 9.92 | 16.38 | 25.58 | 37.08 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[11119, 11118, 11118]] |
| 16 | least-cost | 4 | 4172.0 | 3.39 | 5.89 | 10.01 | 15.04 | 40.06 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[9947, 16071, 15752]] |
| 64 | round-robin | 0 | 2219.0 | 34.76 | 49.71 | 67.02 | 98.85 | 106.25 | null / null / null | 0 | 11 | 0 | 0 | 1.37 | [[3387, 3304, 15528]] |
| 64 | least-cost | 0 | 4657.0 | 12.96 | 18.97 | 26.12 | 35.66 | 43.72 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[10922, 18943, 16772]] |
| 64 | round-robin | 1 | 3312.0 | 3.21 | 55.45 | 69.69 | 84.02 | 91.95 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[11054, 11055, 11055]] |
| 64 | least-cost | 1 | 4688.0 | 12.85 | 18.91 | 26.9 | 35.98 | 47.22 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[11006, 18760, 17189]] |
| 64 | round-robin | 2 | 3072.0 | 5.21 | 52.19 | 70.19 | 81.0 | 88.97 | null / null / null | 0 | 0 | 0 | 0 | 0.34 | [[10206, 7288, 13283]] |
| 64 | least-cost | 2 | 4644.0 | 12.96 | 18.99 | 26.92 | 37.37 | 48.46 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[10990, 18407, 17131]] |
| 64 | round-robin | 3 | 3066.0 | 3.4 | 55.17 | 78.55 | 99.95 | 106.76 | null / null / null | 0 | 16 | 0 | 0 | 0.24 | [[9608, 9586, 11511]] |
| 64 | least-cost | 3 | 4585.0 | 13.01 | 19.24 | 27.9 | 40.57 | 50.81 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[10599, 18679, 16626]] |
| 64 | round-robin | 4 | 3337.0 | 3.18 | 54.92 | 69.6 | 83.11 | 93.57 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[11134, 11134, 11133]] |
| 64 | least-cost | 4 | 4550.0 | 13.08 | 19.46 | 28.52 | 39.34 | 65.97 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[10655, 18618, 16295]] |

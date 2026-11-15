Machine: {"arch":"x86_64","cores":32,"cpu":"Intel(R) Xeon(R) Platinum 8358 CPU @ 2.60GHz","memory_gb":null,"os":"Linux 6.12.113+deb13-amd64","setup":"DigitalOcean nyc1, DOKS 1.37.2: one shard of 1M rows, each of its 3 copies a pod on a c-2 node of its own (2 dedicated vCPUs), the coordinator, clients and delay lines one pod on a c-32 node (32 dedicated vCPUs), pod network; every server with --answer-cache-mb 0 (no answer cache); the node of copy 0 also runs stress-ng --cpu 12 (12 CPU-bound threads at the priority of the server); least-cost fixed is an experimental build that spares a copy outrun before --hedge-ms and chooses by cost when every copy is in its cool-off"}


### equal

| clients | config | round | qps | p50 ms | p90 ms | p99 ms | p99.9 ms | max ms | read-after-write p50 / p99 / max ms | errors | hedges | 429s | timeouts | avoided | shard 0 answers by copy |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 16 | round-robin | 0 | 2588.0 | 5.55 | 10.0 | 19.0 | 61.69 | 103.4 | null / null / null | 0 | 2 | 0 | 0 | 1.41 | [[126, 11160, 14632]] |
| 16 | least-cost | 0 | 3819.0 | 3.42 | 6.17 | 17.98 | 35.57 | 66.02 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[2869, 18540, 16860]] |
| 16 | least-cost fixed | 0 | 3896.0 | 3.4 | 6.02 | 17.07 | 34.93 | 58.25 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[2956, 18689, 17429]] |
| 16 | round-robin | 1 | 1621.0 | 6.58 | 19.86 | 64.54 | 103.45 | 147.88 | null / null / null | 0 | 23 | 0 | 0 | 2.46 | [[2219, 2309, 11737]] |
| 16 | least-cost | 1 | 3864.0 | 3.58 | 6.1 | 14.97 | 29.9 | 63.04 | null / null / null | 0 | 0 | 0 | 0 | 0.39 | [[1778, 19343, 17638]] |
| 16 | least-cost fixed | 1 | 3803.0 | 3.45 | 6.21 | 18.15 | 33.08 | 58.94 | null / null / null | 0 | 0 | 0 | 0 | 0.02 | [[2754, 18385, 16934]] |
| 16 | round-robin | 2 | 3115.0 | 4.28 | 8.16 | 18.18 | 62.97 | 105.16 | null / null / null | 0 | 4 | 0 | 0 | 0.98 | [[288, 20518, 10405]] |
| 16 | least-cost | 2 | 3867.0 | 3.47 | 6.08 | 16.86 | 34.09 | 60.72 | null / null / null | 0 | 0 | 0 | 0 | 0.1 | [[2557, 18617, 17538]] |
| 16 | least-cost fixed | 2 | 3876.0 | 3.4 | 6.12 | 17.0 | 32.83 | 49.64 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[3508, 18399, 16920]] |
| 16 | round-robin | 3 | 2938.0 | 4.21 | 8.66 | 32.26 | 65.97 | 110.44 | null / null / null | 0 | 3 | 0 | 0 | 0.8 | [[1066, 18555, 9804]] |
| 16 | least-cost | 3 | 3888.0 | 3.39 | 6.16 | 16.76 | 32.0 | 60.7 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[3641, 18367, 16921]] |
| 16 | least-cost fixed | 3 | 3856.0 | 3.42 | 6.15 | 17.34 | 32.19 | 57.57 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[3233, 18375, 17016]] |
| 16 | round-robin | 4 | 2471.0 | 5.46 | 10.75 | 27.08 | 55.56 | 102.39 | null / null / null | 0 | 1 | 0 | 0 | 1.32 | [[632, 9871, 14259]] |
| 16 | least-cost | 4 | 3891.0 | 3.38 | 6.07 | 17.17 | 31.07 | 55.73 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[3410, 18368, 17217]] |
| 16 | least-cost fixed | 4 | 3860.0 | 3.4 | 6.13 | 17.43 | 31.28 | 56.98 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[3353, 18516, 16826]] |
| 64 | round-robin | 0 | 2097.0 | 29.9 | 45.29 | 67.41 | 177.77 | 229.62 | null / null / null | 0 | 44 | 0 | 0 | 2.0 | [[200, 668, 20254]] |
| 64 | least-cost | 0 | 2215.0 | 21.36 | 46.37 | 187.04 | 298.85 | 477.69 | null / null / null | 0 | 271 | 0 | 0 | 2.1 | [[2163, 16199, 4109]] |
| 64 | least-cost fixed | 0 | 4352.0 | 13.83 | 21.91 | 36.41 | 70.11 | 110.16 | null / null / null | 0 | 10 | 0 | 0 | 0.78 | [[935, 22592, 20218]] |
| 64 | round-robin | 1 | 1872.0 | 26.95 | 54.69 | 219.34 | 334.31 | 587.78 | null / null / null | 0 | 264 | 0 | 0 | 2.29 | [[1480, 3954, 13402]] |
| 64 | least-cost | 1 | 3266.0 | 16.7 | 34.07 | 53.93 | 72.04 | 112.62 | null / null / null | 0 | 3 | 0 | 0 | 1.34 | [[485, 12221, 20097]] |
| 64 | least-cost fixed | 1 | 4267.0 | 14.02 | 22.25 | 38.27 | 71.39 | 115.6 | null / null / null | 0 | 5 | 0 | 0 | 0.83 | [[904, 22100, 19817]] |
| 64 | round-robin | 2 | 1762.0 | 27.83 | 96.97 | 201.05 | 294.82 | 448.26 | null / null / null | 0 | 359 | 0 | 0 | 2.49 | [[2198, 2861, 12682]] |
| 64 | least-cost | 2 | 4038.0 | 12.99 | 22.84 | 103.6 | 201.71 | 266.74 | null / null / null | 0 | 126 | 0 | 0 | 0.95 | [[2521, 19978, 18227]] |
| 64 | least-cost fixed | 2 | 4337.0 | 13.79 | 21.96 | 39.97 | 79.68 | 118.07 | null / null / null | 0 | 7 | 0 | 0 | 0.76 | [[1248, 21886, 20363]] |
| 64 | round-robin | 3 | 2080.0 | 30.3 | 46.06 | 71.32 | 142.53 | 191.5 | null / null / null | 0 | 38 | 0 | 0 | 1.9 | [[195, 1162, 19593]] |
| 64 | least-cost | 3 | 1870.0 | 26.33 | 90.96 | 192.8 | 341.81 | 647.75 | null / null / null | 0 | 348 | 0 | 0 | 2.37 | [[2176, 3259, 13371]] |
| 64 | least-cost fixed | 3 | 4434.0 | 13.12 | 21.68 | 45.0 | 76.96 | 113.02 | null / null / null | 0 | 2 | 0 | 0 | 0.51 | [[2325, 22023, 20178]] |
| 64 | round-robin | 4 | 2479.0 | 25.99 | 43.03 | 68.0 | 137.06 | 169.66 | null / null / null | 0 | 41 | 0 | 0 | 1.68 | [[142, 7562, 17252]] |
| 64 | least-cost | 4 | 4269.0 | 14.32 | 21.59 | 33.2 | 52.79 | 110.97 | null / null / null | 0 | 2 | 0 | 0 | 0.93 | [[235, 22516, 20089]] |
| 64 | least-cost fixed | 4 | 4313.0 | 14.13 | 21.76 | 32.22 | 57.05 | 115.34 | null / null / null | 0 | 3 | 0 | 0 | 0.98 | [[252, 22264, 20772]] |

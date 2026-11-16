Machine: {"arch":"x86_64","cores":32,"cpu":"Intel(R) Xeon(R) Platinum 8358 CPU @ 2.60GHz","memory_gb":null,"os":"Linux 6.12.113+deb13-amd64","setup":"DigitalOcean nyc1, DOKS 1.37.2: one shard of 1M rows, each of its 3 copies a pod on a c-2 node of its own (2 dedicated vCPUs), the coordinator, clients and delay lines one pod on a c-32 node (32 dedicated vCPUs), pod network; every server with --answer-cache-mb 0 (no answer cache); the node of copy 0 also runs stress-ng --cpu 24 (24 CPU-bound threads at the priority of the server); least-cost fixed is an experimental build that spares a copy outrun before --hedge-ms and chooses by cost when every copy is in its cool-off"}


### equal

| clients | config | round | qps | p50 ms | p90 ms | p99 ms | p99.9 ms | max ms | read-after-write p50 / p99 / max ms | errors | hedges | 429s | timeouts | avoided | shard 0 answers by copy |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 16 | round-robin | 0 | 3004.0 | 4.6 | 8.72 | 15.89 | 81.55 | 108.41 | null / null / null | 0 | 12 | 0 | 0 | 0.98 | [[87, 19965, 10027]] |
| 16 | least-cost | 0 | 3606.0 | 3.62 | 6.44 | 21.32 | 55.09 | 97.36 | null / null / null | 0 | 0 | 0 | 0 | 0.1 | [[1153, 17366, 17598]] |
| 16 | least-cost fixed | 0 | 3764.0 | 3.53 | 6.04 | 19.62 | 47.8 | 75.09 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1100, 18835, 17764]] |
| 16 | round-robin | 1 | 1999.0 | 7.28 | 11.77 | 20.22 | 68.8 | 107.96 | null / null / null | 0 | 9 | 0 | 0 | 1.98 | [[102, 374, 19550]] |
| 16 | least-cost | 1 | 3767.0 | 3.54 | 6.05 | 20.08 | 44.94 | 78.5 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1251, 19020, 17534]] |
| 16 | least-cost fixed | 1 | 3717.0 | 3.55 | 6.09 | 21.14 | 51.82 | 82.66 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1095, 18546, 17619]] |
| 16 | round-robin | 2 | 2531.0 | 5.62 | 10.12 | 18.28 | 92.88 | 153.69 | null / null / null | 0 | 17 | 0 | 0 | 1.46 | [[89, 10013, 15280]] |
| 16 | least-cost | 2 | 3820.0 | 3.68 | 6.08 | 13.0 | 42.0 | 113.15 | null / null / null | 0 | 1 | 0 | 0 | 0.49 | [[481, 20014, 17780]] |
| 16 | least-cost fixed | 2 | 3782.0 | 3.55 | 6.02 | 18.89 | 46.72 | 103.98 | null / null / null | 0 | 1 | 0 | 0 | 0.12 | [[969, 19283, 17671]] |
| 16 | round-robin | 3 | 2754.0 | 5.05 | 9.54 | 17.13 | 78.67 | 106.51 | null / null / null | 0 | 14 | 0 | 0 | 1.41 | [[60, 13140, 14419]] |
| 16 | least-cost | 3 | 3613.0 | 3.69 | 6.35 | 21.24 | 51.37 | 84.2 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1017, 17830, 17328]] |
| 16 | least-cost fixed | 3 | 3756.0 | 3.56 | 6.04 | 19.9 | 46.84 | 97.12 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1103, 18979, 17584]] |
| 16 | round-robin | 4 | 2513.0 | 5.52 | 10.36 | 21.08 | 79.01 | 107.2 | null / null / null | 0 | 11 | 0 | 0 | 1.44 | [[181, 9969, 15072]] |
| 16 | least-cost | 4 | 3723.0 | 3.59 | 6.1 | 20.15 | 47.72 | 82.04 | null / null / null | 0 | 0 | 0 | 0 | 0.0 | [[1275, 18383, 17654]] |
| 16 | least-cost fixed | 4 | 3748.0 | 3.83 | 6.17 | 11.96 | 37.76 | 104.03 | null / null / null | 0 | 3 | 0 | 0 | 0.68 | [[353, 19571, 17746]] |
| 64 | round-robin | 0 | 3188.0 | 20.34 | 36.37 | 59.65 | 372.46 | 530.27 | null / null / null | 0 | 33 | 0 | 0 | 1.1 | [[130, 20383, 11652]] |
| 64 | least-cost | 0 | 2804.0 | 16.16 | 34.86 | 230.01 | 400.88 | 507.37 | null / null / null | 0 | 87 | 0 | 0 | 1.71 | [[650, 16465, 11079]] |
| 64 | least-cost fixed | 0 | 4117.0 | 14.65 | 22.87 | 34.18 | 92.29 | 334.09 | null / null / null | 0 | 18 | 0 | 0 | 0.98 | [[71, 21126, 20153]] |
| 64 | round-robin | 1 | 2651.0 | 24.35 | 39.89 | 61.69 | 246.1 | 320.07 | null / null / null | 0 | 38 | 0 | 0 | 1.56 | [[124, 10284, 16307]] |
| 64 | least-cost | 1 | 3934.0 | 14.63 | 24.16 | 44.01 | 103.33 | 303.65 | null / null / null | 0 | 34 | 0 | 0 | 1.07 | [[341, 19701, 20581]] |
| 64 | least-cost fixed | 1 | 4252.0 | 14.32 | 21.97 | 32.04 | 62.1 | 116.14 | null / null / null | 0 | 6 | 0 | 0 | 0.98 | [[89, 22144, 20443]] |
| 64 | round-robin | 2 | 2585.0 | 25.13 | 42.54 | 67.72 | 193.87 | 276.07 | null / null / null | 0 | 39 | 0 | 0 | 1.46 | [[105, 10732, 15385]] |
| 64 | least-cost | 2 | 2557.0 | 24.26 | 41.35 | 56.99 | 82.19 | 118.18 | null / null / null | 0 | 13 | 0 | 0 | 1.73 | [[67, 6044, 19599]] |
| 64 | least-cost fixed | 2 | 4248.0 | 14.28 | 22.09 | 33.97 | 61.45 | 115.48 | null / null / null | 0 | 5 | 0 | 0 | 0.95 | [[161, 22371, 20143]] |
| 64 | round-robin | 3 | 2775.0 | 23.28 | 40.57 | 60.61 | 227.64 | 343.9 | null / null / null | 0 | 49 | 0 | 0 | 1.44 | [[133, 12650, 15136]] |
| 64 | least-cost | 3 | 2225.0 | 14.74 | 39.76 | 310.84 | 573.96 | 971.0 | null / null / null | 0 | 245 | 0 | 0 | 2.07 | [[1453, 8963, 11980]] |
| 64 | least-cost fixed | 3 | 4308.0 | 14.12 | 21.71 | 32.12 | 54.91 | 109.7 | null / null / null | 0 | 3 | 0 | 0 | 0.98 | [[93, 22620, 20611]] |
| 64 | round-robin | 4 | 3372.0 | 19.18 | 34.87 | 51.58 | 274.62 | 404.66 | null / null / null | 0 | 41 | 0 | 0 | 1.0 | [[122, 22410, 11259]] |
| 64 | least-cost | 4 | 1816.0 | 25.8 | 46.57 | 362.01 | 518.52 | 874.04 | null / null / null | 0 | 118 | 0 | 0 | 2.17 | [[631, 11713, 5925]] |
| 64 | least-cost fixed | 4 | 4336.0 | 14.03 | 21.36 | 31.62 | 79.22 | 164.07 | null / null / null | 0 | 20 | 0 | 0 | 0.95 | [[196, 22490, 20906]] |

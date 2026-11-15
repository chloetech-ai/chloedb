Machine: {"arch":"x86_64","cores":32,"cpu":"Intel(R) Xeon(R) Platinum 8358 CPU @ 2.60GHz","memory_gb":null,"os":"Linux 6.12.113+deb13-amd64","setup":"DigitalOcean nyc1, DOKS 1.37.2: one shard of 1M rows, each of its 3 copies a pod on a c-2 node of its own (2 dedicated vCPUs), the coordinator, clients and delay lines one pod on a c-32 node (32 dedicated vCPUs), pod network; every server with --answer-cache-mb 0 (no answer cache); the node of copy 0 also runs stress-ng --cpu 48 (48 CPU-bound threads at the priority of the server); least-cost fixed is an experimental build that spares a copy outrun before --hedge-ms and chooses by cost when every copy is in its cool-off"}


### equal

| clients | config | round | qps | p50 ms | p90 ms | p99 ms | p99.9 ms | max ms | read-after-write p50 / p99 / max ms | errors | hedges | 429s | timeouts | avoided | shard 0 answers by copy |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 16 | round-robin | 0 | 2046.0 | 7.08 | 11.44 | 19.3 | 103.71 | 108.96 | null / null / null | 0 | 30 | 0 | 0 | 2.0 | [[22, 227, 20328]] |
| 16 | least-cost | 0 | 3849.0 | 3.83 | 6.14 | 10.11 | 17.0 | 104.21 | null / null / null | 0 | 1 | 0 | 0 | 0.95 | [[10, 20732, 17918]] |
| 16 | least-cost fixed | 0 | 3836.0 | 3.83 | 6.13 | 10.36 | 18.72 | 106.89 | null / null / null | 0 | 2 | 0 | 0 | 0.9 | [[25, 20218, 18283]] |
| 16 | round-robin | 1 | 3205.0 | 4.29 | 7.99 | 14.4 | 78.78 | 287.21 | null / null / null | 0 | 25 | 0 | 0 | 1.0 | [[11, 21529, 10765]] |
| 16 | least-cost | 1 | 3872.0 | 3.81 | 6.08 | 10.11 | 16.25 | 107.11 | null / null / null | 0 | 1 | 0 | 0 | 0.93 | [[6, 20426, 18435]] |
| 16 | least-cost fixed | 1 | 3838.0 | 3.8 | 6.14 | 10.5 | 18.83 | 104.87 | null / null / null | 0 | 4 | 0 | 0 | 0.73 | [[26, 20187, 18426]] |
| 16 | round-robin | 2 | 2486.0 | 5.72 | 10.45 | 19.0 | 95.77 | 105.3 | null / null / null | 0 | 20 | 0 | 0 | 1.54 | [[35, 9427, 15529]] |
| 16 | least-cost | 2 | 3820.0 | 3.83 | 6.17 | 10.68 | 19.76 | 107.12 | null / null / null | 0 | 1 | 0 | 0 | 0.95 | [[13, 20213, 18097]] |
| 16 | least-cost fixed | 2 | 3855.0 | 3.81 | 6.08 | 10.27 | 19.06 | 105.62 | null / null / null | 0 | 2 | 0 | 0 | 0.78 | [[38, 20411, 18322]] |
| 16 | round-robin | 3 | 2149.0 | 6.83 | 11.02 | 18.3 | 102.88 | 108.66 | null / null / null | 0 | 24 | 0 | 0 | 1.93 | [[18, 12228, 9359]] |
| 16 | least-cost | 3 | 2042.0 | 3.93 | 7.19 | 145.28 | 339.19 | 1264.59 | null / null / null | 0 | 83 | 0 | 0 | 1.9 | [[437, 10272, 9784]] |
| 16 | least-cost fixed | 3 | 3858.0 | 3.82 | 6.11 | 9.91 | 15.73 | 104.07 | null / null / null | 0 | 1 | 0 | 0 | 0.93 | [[15, 20616, 18237]] |
| 16 | round-robin | 4 | 2015.0 | 7.2 | 11.96 | 20.86 | 103.56 | 122.71 | null / null / null | 0 | 24 | 0 | 0 | 1.9 | [[17, 1280, 19072]] |
| 16 | least-cost | 4 | 2921.0 | 4.61 | 9.07 | 16.15 | 30.22 | 106.6 | null / null / null | 0 | 2 | 0 | 0 | 1.39 | [[43, 10319, 19364]] |
| 16 | least-cost fixed | 4 | 3790.0 | 3.85 | 6.14 | 10.74 | 29.69 | 110.81 | null / null / null | 0 | 3 | 0 | 0 | 0.8 | [[55, 20017, 18136]] |
| 64 | round-robin | 0 | 3347.0 | 19.21 | 35.29 | 52.21 | 307.96 | 870.22 | null / null / null | 0 | 43 | 0 | 0 | 1.0 | [[117, 22515, 11317]] |
| 64 | least-cost | 0 | 3243.0 | 16.69 | 34.29 | 53.98 | 91.71 | 206.95 | null / null / null | 0 | 23 | 0 | 0 | 1.49 | [[34, 12171, 20425]] |
| 64 | least-cost fixed | 0 | 4098.0 | 14.9 | 22.8 | 33.89 | 58.28 | 122.45 | null / null / null | 0 | 26 | 0 | 0 | 0.98 | [[37, 21189, 20068]] |
| 64 | round-robin | 1 | 2564.0 | 24.67 | 41.4 | 62.92 | 339.11 | 522.62 | null / null / null | 0 | 34 | 0 | 0 | 1.51 | [[105, 9753, 16200]] |
| 64 | least-cost | 1 | 3280.0 | 16.98 | 33.01 | 51.37 | 108.77 | 288.51 | null / null / null | 0 | 29 | 0 | 0 | 1.44 | [[34, 21236, 11747]] |
| 64 | least-cost fixed | 1 | 4291.0 | 14.21 | 21.74 | 32.82 | 54.53 | 114.39 | null / null / null | 0 | 13 | 0 | 0 | 1.0 | [[61, 22052, 20943]] |
| 64 | round-robin | 2 | 3228.0 | 19.0 | 36.61 | 58.15 | 434.44 | 648.16 | null / null / null | 0 | 40 | 0 | 0 | 1.0 | [[89, 21718, 10905]] |
| 64 | least-cost | 2 | 3164.0 | 17.58 | 34.01 | 51.66 | 103.48 | 204.41 | null / null / null | 0 | 31 | 0 | 0 | 1.49 | [[27, 21383, 10445]] |
| 64 | least-cost fixed | 2 | 4220.0 | 14.47 | 21.97 | 33.13 | 60.09 | 126.99 | null / null / null | 0 | 19 | 0 | 0 | 1.0 | [[26, 22228, 20293]] |
| 64 | round-robin | 3 | 2709.0 | 23.7 | 40.36 | 59.88 | 341.33 | 553.44 | null / null / null | 0 | 39 | 0 | 0 | 1.4 | [[90, 11941, 15400]] |
| 64 | least-cost | 3 | 3089.0 | 18.01 | 34.93 | 52.86 | 97.65 | 234.88 | null / null / null | 0 | 17 | 0 | 0 | 1.49 | [[34, 22077, 9041]] |
| 64 | least-cost fixed | 3 | 4012.0 | 15.14 | 23.28 | 35.64 | 60.94 | 132.93 | null / null / null | 0 | 15 | 0 | 0 | 0.98 | [[36, 22030, 18221]] |
| 64 | round-robin | 4 | 3274.0 | 18.77 | 35.94 | 51.68 | 474.17 | 565.79 | null / null / null | 0 | 42 | 0 | 0 | 1.0 | [[85, 22149, 11124]] |
| 64 | least-cost | 4 | 2016.0 | 15.04 | 30.03 | 639.02 | 1146.96 | 2243.05 | null / null / null | 0 | 102 | 0 | 0 | 1.73 | [[441, 12056, 9963]] |
| 64 | least-cost fixed | 4 | 4261.0 | 14.29 | 22.0 | 31.9 | 50.96 | 134.89 | null / null / null | 0 | 23 | 0 | 0 | 0.98 | [[31, 22684, 20226]] |

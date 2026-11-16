## Recall@10 and latency per HNSW candidates value

Latency is the median of the passes: p50 and p99 at one client, qps at the most clients timed.

| engine | kind | candidates | recall@10 | p50 ms | p99 ms | qps | passes |
|---|---|---|---|---|---|---|---|
| Postgres | vector | 64 | 0.780 | 3.31 | 6.77 | 1389 (16 clients) | 3 |
| Postgres | vector | 128 | 0.868 | 5.06 | 10.48 | 897 (16 clients) | 3 |
| Postgres | vector | 256 | 0.922 | 8.64 | 17.09 | 529 (16 clients) | 3 |
| Postgres | vector | 512 | 0.955 | 15.39 | 27.72 | 293 (16 clients) | 3 |
| Postgres | vector + filter | 64 | 0.725 | 3.53 | 10.10 | 1257 (16 clients) | 3 |
| Postgres | vector + filter | 128 | 0.827 | 4.97 | 10.63 | 886 (16 clients) | 3 |
| Postgres | vector + filter | 256 | 0.900 | 8.77 | 17.88 | 522 (16 clients) | 3 |
| Postgres | vector + filter | 512 | 0.937 | 15.86 | 28.23 | 287 (16 clients) | 3 |
| Postgres | similar items | 64 | 0.984 | 2.97 | 5.96 | 1606 (16 clients) | 3 |
| Postgres | similar items | 128 | 0.990 | 4.49 | 8.70 | 1023 (16 clients) | 3 |
| Postgres | similar items | 256 | 0.997 | 7.28 | 14.84 | 609 (16 clients) | 3 |
| Postgres | similar items | 512 | 0.998 | 13.81 | 29.06 | 328 (16 clients) | 3 |
| Postgres | hybrid (RRF) | 64 | n/a | 255.84 | 1018.29 | 4 (16 clients) | 3 |
| Postgres | hybrid (RRF) | 128 | n/a | 235.14 | 994.44 | 4 (16 clients) | 3 |
| Postgres | hybrid (RRF) | 256 | n/a | 253.44 | 1019.04 | 4 (16 clients) | 3 |
| Postgres | hybrid (RRF) | 512 | n/a | 251.50 | 1011.18 | 4 (16 clients) | 3 |
| Postgres | hybrid (weighted) | 64 | n/a | 266.39 | 1010.76 | 4 (16 clients) | 3 |
| Postgres | hybrid (weighted) | 128 | n/a | 259.73 | 997.88 | 4 (16 clients) | 3 |
| Postgres | hybrid (weighted) | 256 | n/a | 252.40 | 1010.30 | 4 (16 clients) | 3 |
| Postgres | hybrid (weighted) | 512 | n/a | 267.57 | 1028.71 | 4 (16 clients) | 3 |
| Qdrant | vector | 64 | 0.896 | 1.90 | 2.73 | 1730 (16 clients) | 3 |
| Qdrant | vector | 128 | 0.944 | 2.83 | 4.27 | 1068 (16 clients) | 3 |
| Qdrant | vector | 256 | 0.965 | 4.62 | 6.91 | 622 (16 clients) | 3 |
| Qdrant | vector | 512 | 0.984 | 8.16 | 12.20 | 340 (16 clients) | 3 |
| Qdrant | vector + filter | 64 | 0.836 | 3.72 | 5.55 | 875 (16 clients) | 3 |
| Qdrant | vector + filter | 128 | 0.918 | 5.85 | 8.79 | 526 (16 clients) | 3 |
| Qdrant | vector + filter | 256 | 0.965 | 9.78 | 14.95 | 308 (16 clients) | 3 |
| Qdrant | vector + filter | 512 | 0.988 | 17.15 | 26.18 | 166 (16 clients) | 3 |
| Qdrant | similar items | 64 | 0.972 | 9.58 | 14.28 | 370 (16 clients) | 3 |
| Qdrant | similar items | 128 | 0.991 | 15.82 | 22.92 | 217 (16 clients) | 3 |
| Qdrant | similar items | 256 | 0.998 | 26.58 | 39.65 | 122 (16 clients) | 3 |
| Qdrant | similar items | 512 | 0.999 | 47.09 | 70.85 | 67 (16 clients) | 3 |
| Qdrant | hybrid (RRF) | 64 | n/a | 9.64 | 15.97 | 345 (16 clients) | 3 |
| Qdrant | hybrid (RRF) | 128 | n/a | 9.86 | 14.63 | 318 (16 clients) | 3 |
| Qdrant | hybrid (RRF) | 256 | n/a | 9.68 | 14.07 | 323 (16 clients) | 3 |
| Qdrant | hybrid (RRF) | 512 | n/a | 11.21 | 16.12 | 270 (16 clients) | 3 |
| Qdrant | hybrid (weighted) | 64 | n/a | 9.88 | 16.65 | 340 (16 clients) | 3 |
| Qdrant | hybrid (weighted) | 128 | n/a | 9.77 | 13.97 | 321 (16 clients) | 3 |
| Qdrant | hybrid (weighted) | 256 | n/a | 9.70 | 14.42 | 322 (16 clients) | 3 |
| Qdrant | hybrid (weighted) | 512 | n/a | 11.19 | 16.17 | 269 (16 clients) | 3 |

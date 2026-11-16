## Recall@10 and latency per HNSW candidates value

Latency is the median of the passes: p50 and p99 at one client, qps at the most clients timed.

| engine | kind | candidates | recall@10 | p50 ms | p99 ms | qps | passes |
|---|---|---|---|---|---|---|---|
| Elasticsearch | vector | 64 | 0.665 | 4.01 | 5.32 | 1292 (16 clients) | 3 |
| Elasticsearch | vector | 128 | 0.723 | 4.27 | 5.90 | 1156 (16 clients) | 3 |
| Elasticsearch | vector | 256 | 0.767 | 4.83 | 6.83 | 987 (16 clients) | 3 |
| Elasticsearch | vector + filter | 64 | 0.605 | 5.27 | 229.99 | 269 (16 clients) | 3 |
| Elasticsearch | vector + filter | 128 | 0.691 | 6.39 | 232.29 | 143 (16 clients) | 3 |
| Elasticsearch | vector + filter | 256 | 0.771 | 8.24 | 238.86 | 83 (16 clients) | 3 |
| Elasticsearch | similar items | 64 | 0.905 | 24.34 | 406.47 | 87 (16 clients) | 3 |
| Elasticsearch | similar items | 128 | 0.938 | 24.67 | 396.44 | 80 (16 clients) | 3 |
| Elasticsearch | similar items | 256 | 0.958 | 25.29 | 406.10 | 78 (16 clients) | 3 |
| Elasticsearch | hybrid (weighted) | 64 | n/a | 14.51 | 129.95 | 103 (16 clients) | 3 |
| Elasticsearch | hybrid (weighted) | 128 | n/a | 14.12 | 131.38 | 105 (16 clients) | 3 |
| Elasticsearch | hybrid (weighted) | 256 | n/a | 14.12 | 127.46 | 105 (16 clients) | 3 |
| OpenSearch | vector | 64 | 0.637 | 4.22 | 6.79 | 1082 (16 clients) | 3 |
| OpenSearch | vector | 128 | 0.674 | 4.48 | 6.81 | 988 (16 clients) | 3 |
| OpenSearch | vector | 256 | 0.757 | 5.69 | 8.58 | 683 (16 clients) | 3 |
| OpenSearch | vector + filter | 64 | 0.787 | 8.43 | 12.71 | 436 (16 clients) | 3 |
| OpenSearch | vector + filter | 128 | 0.815 | 9.38 | 12.86 | 365 (16 clients) | 3 |
| OpenSearch | vector + filter | 256 | 0.879 | 14.89 | 20.11 | 212 (16 clients) | 3 |
| OpenSearch | similar items | 64 | 0.908 | 31.38 | 35.58 | 64 (16 clients) | 3 |
| OpenSearch | similar items | 128 | 0.923 | 32.00 | 36.46 | 64 (16 clients) | 3 |
| OpenSearch | similar items | 256 | 0.949 | 34.43 | 41.12 | 60 (16 clients) | 3 |
| OpenSearch | hybrid (RRF) | 64 | n/a | 11.88 | 58.14 | 180 (16 clients) | 3 |
| OpenSearch | hybrid (RRF) | 128 | n/a | 11.87 | 59.42 | 182 (16 clients) | 3 |
| OpenSearch | hybrid (RRF) | 256 | n/a | 11.66 | 57.57 | 181 (16 clients) | 3 |
| OpenSearch | hybrid (weighted) | 64 | n/a | 11.96 | 58.41 | 181 (16 clients) | 3 |
| OpenSearch | hybrid (weighted) | 128 | n/a | 11.63 | 57.52 | 183 (16 clients) | 3 |
| OpenSearch | hybrid (weighted) | 256 | n/a | 11.62 | 58.84 | 183 (16 clients) | 3 |
| Strata | vector | 64 | 0.798 | 2.67 | 4.36 | 1317 (16 clients) | 3 |
| Strata | vector | 128 | 0.863 | 3.04 | 5.66 | 1005 (16 clients) | 3 |
| Strata | vector | 256 | 0.914 | 3.74 | 6.77 | 680 (16 clients) | 3 |
| Strata | vector + filter | 64 | 0.816 | 2.81 | 4.91 | 1185 (16 clients) | 3 |
| Strata | vector + filter | 128 | 0.865 | 3.31 | 5.80 | 861 (16 clients) | 3 |
| Strata | vector + filter | 256 | 0.918 | 4.06 | 7.07 | 582 (16 clients) | 3 |
| Strata | similar items | 64 | 0.981 | 3.71 | 5.92 | 765 (16 clients) | 3 |
| Strata | similar items | 128 | 0.987 | 3.80 | 6.40 | 701 (16 clients) | 3 |
| Strata | similar items | 256 | 0.993 | 4.49 | 7.78 | 530 (16 clients) | 3 |
| Strata | hybrid (RRF) | 64 | n/a | 7.75 | 29.99 | 255 (16 clients) | 3 |
| Strata | hybrid (RRF) | 128 | n/a | 7.82 | 32.02 | 248 (16 clients) | 3 |
| Strata | hybrid (RRF) | 256 | n/a | 8.49 | 31.66 | 224 (16 clients) | 3 |
| Strata | hybrid (weighted) | 64 | n/a | 7.88 | 30.73 | 250 (16 clients) | 3 |
| Strata | hybrid (weighted) | 128 | n/a | 7.91 | 31.00 | 248 (16 clients) | 3 |
| Strata | hybrid (weighted) | 256 | n/a | 8.67 | 34.95 | 224 (16 clients) | 3 |

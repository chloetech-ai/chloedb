## Recall@10 and latency per HNSW candidates value

Latency is the median of the passes: p50 and p99 at one client, qps at the most clients timed.

| engine | kind | candidates | recall@10 | p50 ms | p99 ms | qps | passes |
|---|---|---|---|---|---|---|---|
| Strata | vector | 64 | 0.807 | 2.37 | 3.61 | 2202 (16 clients) | 3 |
| Strata | vector | 128 | 0.873 | 2.58 | 4.45 | 1825 (16 clients) | 3 |
| Strata | vector | 256 | 0.930 | 3.02 | 5.19 | 1379 (16 clients) | 3 |
| Strata | vector | 512 | 0.957 | 3.80 | 6.26 | 949 (16 clients) | 3 |
| Strata | vector + filter | 64 | 0.862 | 2.43 | 4.01 | 2109 (16 clients) | 3 |
| Strata | vector + filter | 128 | 0.913 | 2.71 | 5.39 | 1716 (16 clients) | 3 |
| Strata | vector + filter | 256 | 0.952 | 3.22 | 5.40 | 1286 (16 clients) | 3 |
| Strata | vector + filter | 512 | 0.979 | 4.12 | 6.98 | 889 (16 clients) | 3 |
| Strata | similar items | 64 | 0.994 | 3.06 | 4.84 | 1564 (16 clients) | 3 |
| Strata | similar items | 128 | 0.997 | 3.24 | 6.07 | 1455 (16 clients) | 3 |
| Strata | similar items | 256 | 0.999 | 3.61 | 5.78 | 1164 (16 clients) | 3 |
| Strata | similar items | 512 | 0.999 | 4.39 | 7.23 | 856 (16 clients) | 3 |
| Strata | hybrid (RRF) | 64 | n/a | 5.31 | 11.13 | 721 (16 clients) | 3 |
| Strata | hybrid (RRF) | 128 | n/a | 5.45 | 11.54 | 695 (16 clients) | 3 |
| Strata | hybrid (RRF) | 256 | n/a | 5.96 | 13.10 | 616 (16 clients) | 3 |
| Strata | hybrid (RRF) | 512 | n/a | 6.60 | 12.80 | 514 (16 clients) | 3 |
| Strata | hybrid (weighted) | 64 | n/a | 5.28 | 11.56 | 722 (16 clients) | 3 |
| Strata | hybrid (weighted) | 128 | n/a | 5.42 | 11.40 | 692 (16 clients) | 3 |
| Strata | hybrid (weighted) | 256 | n/a | 5.87 | 11.87 | 615 (16 clients) | 3 |
| Strata | hybrid (weighted) | 512 | n/a | 6.63 | 12.74 | 516 (16 clients) | 3 |
| Vespa | vector | 64 | 0.781 | 3.42 | 4.45 | 507 (16 clients) | 3 |
| Vespa | vector | 128 | 0.854 | 4.10 | 5.03 | 507 (16 clients) | 3 |
| Vespa | vector | 256 | 0.911 | 5.51 | 7.33 | 505 (16 clients) | 3 |
| Vespa | vector | 512 | 0.944 | 7.94 | 10.36 | 384 (16 clients) | 3 |
| Vespa | vector + filter | 64 | 0.866 | 6.17 | 8.00 | 495 (16 clients) | 3 |
| Vespa | vector + filter | 128 | 0.917 | 8.93 | 11.46 | 325 (16 clients) | 3 |
| Vespa | vector + filter | 256 | 0.954 | 13.91 | 17.70 | 186 (16 clients) | 3 |
| Vespa | vector + filter | 512 | 0.976 | 22.89 | 29.29 | 105 (16 clients) | 3 |
| Vespa | similar items | 64 | 0.987 | 5.84 | 7.55 | 472 (16 clients) | 3 |
| Vespa | similar items | 128 | 0.996 | 7.58 | 9.94 | 458 (16 clients) | 3 |
| Vespa | similar items | 256 | 0.999 | 10.51 | 13.49 | 279 (16 clients) | 3 |
| Vespa | similar items | 512 | 1.000 | 16.08 | 20.41 | 161 (16 clients) | 3 |
| Vespa | hybrid (RRF) | 64 | n/a | 8.32 | 55.50 | 295 (16 clients) | 3 |
| Vespa | hybrid (RRF) | 128 | n/a | 8.29 | 54.70 | 294 (16 clients) | 3 |
| Vespa | hybrid (RRF) | 256 | n/a | 8.28 | 55.91 | 296 (16 clients) | 3 |
| Vespa | hybrid (RRF) | 512 | n/a | 8.43 | 55.32 | 294 (16 clients) | 3 |
| Vespa | hybrid (weighted) | 64 | n/a | 8.51 | 55.99 | 298 (16 clients) | 3 |
| Vespa | hybrid (weighted) | 128 | n/a | 8.52 | 55.83 | 292 (16 clients) | 3 |
| Vespa | hybrid (weighted) | 256 | n/a | 8.47 | 56.82 | 292 (16 clients) | 3 |
| Vespa | hybrid (weighted) | 512 | n/a | 8.53 | 54.27 | 292 (16 clients) | 3 |

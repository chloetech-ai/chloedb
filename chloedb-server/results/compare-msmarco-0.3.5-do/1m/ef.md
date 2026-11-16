## Recall@10 and latency per HNSW candidates value

Latency is the median of the passes: p50 and p99 at one client, qps at the most clients timed.

| engine | kind | candidates | recall@10 | p50 ms | p99 ms | qps | passes |
|---|---|---|---|---|---|---|---|
| Elasticsearch | vector | 64 | 0.738 | 3.90 | 5.15 | 1398 (16 clients) | 3 |
| Elasticsearch | vector | 128 | 0.817 | 4.10 | 5.52 | 1232 (16 clients) | 3 |
| Elasticsearch | vector | 256 | 0.869 | 4.65 | 6.69 | 1019 (16 clients) | 3 |
| Elasticsearch | vector | 512 | 0.900 | 5.43 | 7.88 | 808 (16 clients) | 3 |
| Elasticsearch | vector + filter | 64 | 0.699 | 4.48 | 6.26 | 1084 (16 clients) | 3 |
| Elasticsearch | vector + filter | 128 | 0.778 | 4.96 | 7.35 | 891 (16 clients) | 3 |
| Elasticsearch | vector + filter | 256 | 0.831 | 6.05 | 9.14 | 690 (16 clients) | 3 |
| Elasticsearch | vector + filter | 512 | 0.868 | 7.49 | 11.29 | 515 (16 clients) | 3 |
| Elasticsearch | similar items | 64 | 0.930 | 6.65 | 9.82 | 555 (16 clients) | 3 |
| Elasticsearch | similar items | 128 | 0.954 | 6.89 | 10.29 | 528 (16 clients) | 3 |
| Elasticsearch | similar items | 256 | 0.969 | 7.62 | 11.07 | 462 (16 clients) | 3 |
| Elasticsearch | similar items | 512 | 0.976 | 8.84 | 13.83 | 391 (16 clients) | 3 |
| Elasticsearch | hybrid (weighted) | 64 | n/a | 6.10 | 19.00 | 535 (16 clients) | 3 |
| Elasticsearch | hybrid (weighted) | 128 | n/a | 5.94 | 18.73 | 540 (16 clients) | 3 |
| Elasticsearch | hybrid (weighted) | 256 | n/a | 5.98 | 19.10 | 535 (16 clients) | 3 |
| Elasticsearch | hybrid (weighted) | 512 | n/a | 6.07 | 20.33 | 536 (16 clients) | 3 |
| OpenSearch | vector | 64 | 0.650 | 4.42 | 6.59 | 993 (16 clients) | 3 |
| OpenSearch | vector | 128 | 0.673 | 4.72 | 6.87 | 883 (16 clients) | 3 |
| OpenSearch | vector | 256 | 0.739 | 6.05 | 8.81 | 626 (16 clients) | 3 |
| OpenSearch | vector | 512 | 0.766 | 8.50 | 11.96 | 413 (16 clients) | 3 |
| OpenSearch | vector + filter | 64 | 0.755 | 8.59 | 12.25 | 412 (16 clients) | 3 |
| OpenSearch | vector + filter | 128 | 0.769 | 9.87 | 13.65 | 345 (16 clients) | 3 |
| OpenSearch | vector + filter | 256 | 0.802 | 15.34 | 20.10 | 208 (16 clients) | 3 |
| OpenSearch | vector + filter | 512 | 0.825 | 24.76 | 31.52 | 122 (16 clients) | 3 |
| OpenSearch | similar items | 64 | 0.949 | 8.78 | 12.56 | 391 (16 clients) | 3 |
| OpenSearch | similar items | 128 | 0.951 | 9.54 | 12.96 | 358 (16 clients) | 3 |
| OpenSearch | similar items | 256 | 0.957 | 12.38 | 17.36 | 261 (16 clients) | 3 |
| OpenSearch | similar items | 512 | 0.965 | 17.69 | 23.29 | 176 (16 clients) | 3 |
| OpenSearch | hybrid (RRF) | 64 | n/a | 7.86 | 15.07 | 429 (16 clients) | 3 |
| OpenSearch | hybrid (RRF) | 128 | n/a | 7.95 | 15.31 | 430 (16 clients) | 3 |
| OpenSearch | hybrid (RRF) | 256 | n/a | 7.83 | 14.79 | 432 (16 clients) | 3 |
| OpenSearch | hybrid (RRF) | 512 | n/a | 7.73 | 14.91 | 431 (16 clients) | 3 |
| OpenSearch | hybrid (weighted) | 64 | n/a | 7.79 | 15.02 | 437 (16 clients) | 3 |
| OpenSearch | hybrid (weighted) | 128 | n/a | 7.75 | 14.82 | 436 (16 clients) | 3 |
| OpenSearch | hybrid (weighted) | 256 | n/a | 7.78 | 15.04 | 436 (16 clients) | 3 |
| OpenSearch | hybrid (weighted) | 512 | n/a | 7.69 | 14.77 | 435 (16 clients) | 3 |
| Solr | vector | 64 | 0.788 | 3.93 | 5.53 | 750 (16 clients) | 3 |
| Solr | vector | 128 | 0.858 | 4.58 | 7.18 | 611 (16 clients) | 3 |
| Solr | vector | 256 | 0.909 | 5.77 | 8.17 | 494 (16 clients) | 3 |
| Solr | vector | 512 | 0.939 | 7.55 | 9.99 | 369 (16 clients) | 3 |
| Solr | vector + filter | 64 | 0.921 | 6.99 | 30.44 | 376 (16 clients) | 3 |
| Solr | vector + filter | 128 | 0.951 | 9.36 | 31.69 | 271 (16 clients) | 3 |
| Solr | vector + filter | 256 | 0.968 | 13.40 | 31.84 | 211 (16 clients) | 3 |
| Solr | vector + filter | 512 | 0.984 | 20.45 | 32.28 | 132 (16 clients) | 3 |
| Solr | similar items | 64 | 0.998 | 6.36 | 10.36 | 429 (16 clients) | 3 |
| Solr | similar items | 128 | 0.998 | 7.99 | 14.35 | 355 (16 clients) | 3 |
| Solr | similar items | 256 | 0.998 | 10.36 | 16.44 | 292 (16 clients) | 3 |
| Solr | similar items | 512 | 0.998 | 14.04 | 22.52 | 202 (16 clients) | 3 |
| Solr | hybrid (RRF) | 64 | n/a | 7.98 | 21.41 | 345 (16 clients) | 3 |
| Solr | hybrid (RRF) | 128 | n/a | 8.17 | 25.98 | 346 (16 clients) | 3 |
| Solr | hybrid (RRF) | 256 | n/a | 7.93 | 21.63 | 344 (16 clients) | 3 |
| Solr | hybrid (RRF) | 512 | n/a | 7.75 | 21.43 | 356 (16 clients) | 3 |
| Solr | hybrid (weighted) | 64 | n/a | 7.74 | 25.58 | 311 (16 clients) | 3 |
| Solr | hybrid (weighted) | 128 | n/a | 7.90 | 26.13 | 312 (16 clients) | 3 |
| Solr | hybrid (weighted) | 256 | n/a | 7.92 | 26.84 | 294 (16 clients) | 3 |
| Solr | hybrid (weighted) | 512 | n/a | 7.98 | 25.86 | 302 (16 clients) | 3 |
| Strata | vector | 64 | 0.809 | 2.17 | 3.22 | 2374 (16 clients) | 3 |
| Strata | vector | 128 | 0.875 | 2.34 | 3.67 | 1955 (16 clients) | 3 |
| Strata | vector | 256 | 0.931 | 2.66 | 4.21 | 1511 (16 clients) | 3 |
| Strata | vector | 512 | 0.957 | 3.33 | 5.53 | 1055 (16 clients) | 3 |
| Strata | vector + filter | 64 | 0.862 | 2.26 | 3.46 | 2285 (16 clients) | 3 |
| Strata | vector + filter | 128 | 0.913 | 2.46 | 3.99 | 1885 (16 clients) | 3 |
| Strata | vector + filter | 256 | 0.951 | 2.86 | 4.41 | 1448 (16 clients) | 3 |
| Strata | vector + filter | 512 | 0.979 | 3.61 | 5.42 | 1027 (16 clients) | 3 |
| Strata | similar items | 64 | 0.995 | 2.80 | 4.40 | 1658 (16 clients) | 3 |
| Strata | similar items | 128 | 0.997 | 2.82 | 4.51 | 1574 (16 clients) | 3 |
| Strata | similar items | 256 | 0.999 | 3.13 | 4.82 | 1270 (16 clients) | 3 |
| Strata | similar items | 512 | 0.999 | 3.85 | 6.65 | 951 (16 clients) | 3 |
| Strata | hybrid (RRF) | 64 | n/a | 4.67 | 10.71 | 768 (16 clients) | 3 |
| Strata | hybrid (RRF) | 128 | n/a | 4.80 | 10.51 | 741 (16 clients) | 3 |
| Strata | hybrid (RRF) | 256 | n/a | 5.13 | 11.15 | 670 (16 clients) | 3 |
| Strata | hybrid (RRF) | 512 | n/a | 5.80 | 11.59 | 559 (16 clients) | 3 |
| Strata | hybrid (weighted) | 64 | n/a | 4.75 | 10.75 | 762 (16 clients) | 3 |
| Strata | hybrid (weighted) | 128 | n/a | 4.81 | 10.70 | 745 (16 clients) | 3 |
| Strata | hybrid (weighted) | 256 | n/a | 5.14 | 10.72 | 669 (16 clients) | 3 |
| Strata | hybrid (weighted) | 512 | n/a | 5.80 | 11.51 | 563 (16 clients) | 3 |
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

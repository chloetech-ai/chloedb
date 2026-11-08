## Load time and index size

| engine | load (s) | of which: send / build | searchable | merge to 1 segment | index size (GB) | documents |
|---|---|---|---|---|---|---|
| Strata | 191 | load 341 + merge 28 | (built) | (built as one) | 4.91 | 1,000,000 |
| Elasticsearch | 585 | send 397 | 6 | 182 | 4.89 | 1,000,000 |
| OpenSearch | 799 | send 474 | 2 | 323 | 5.11 | 1,000,000 |
| Solr | 1107 | send 506 | 2 | 600 | 8.96 | 1,000,000 |
| Vespa | 1464 | send 1464 | on write | (on its own) | 9.07 (+ 0.07 transaction log) | 1,000,000 |
| Meilisearch (one node) | 1614 | send 102 | – | index 1512 | 20.63 | 1,000,000 |
| Typesense (one node) | 1291 | send 1291 | – | none | 18.73 | 1,000,000 |
| Manticore (one node) | 1861 | send 606 | – | flush 4, merge 1250 | 5.03 | 1,000,000 |
| Qdrant (one node) | 3889 | send 340 | – | index 3549 | 8.65 | 1,000,000 |
| Weaviate (one node) | 538 | send 538 | – | none | 7.39 | 1,000,000 |
| Milvus (one node) | 574 | send 249 | – | flush 2, index 323, load 0 | 24.71 | 1,000,000 |
| ParadeDB (one node) | 200 | send 98 | – | analyze 1, primary key 4, search index 97 | 21.20 | 1,000,000 |
| Postgres (one node) | 1210 | send 98 | – | analyze 1, gin index 57, hnsw index 1050, primary key 4 | 16.37 | 1,000,000 |
| Redis (one node) | 383 | send 383 | – | index 0 | 8.17 (in memory) | 1,000,000 |

## 1 client: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | Elasticsearch, trial licence q/s | Elasticsearch, trial licence ms | OpenSearch q/s | OpenSearch ms | Solr q/s | Solr ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 532 | 1.70 / 2.59 / 5.00 | 315 | 2.98 / 4.03 / 6.04 | n/a | n/a | 305 | 3.18 / 4.17 / 5.97 | 125 | 7.36 / 11.92 / 17.01 |
| bm25 + filter | 361 | 2.42 / 4.42 / 7.20 | 292 | 3.18 / 4.58 / 7.89 | n/a | n/a | 261 | 3.64 / 5.11 / 8.29 | 154 | 6.09 / 9.48 / 14.43 |
| phrase | 558 | 1.65 / 2.42 / 4.16 | 323 | 3.10 / 4.36 / 5.86 | n/a | n/a | 329 | 3.02 / 4.33 / 5.74 | 456 | 2.07 / 2.94 / 4.15 |
| typo | 53 | 19.65 / 26.26 / 37.09 | 64 | 14.83 / 24.68 / 35.52 | n/a | n/a | 45 | 20.25 / 35.88 / 53.20 | 53 | 17.66 / 28.92 / 43.43 |
| prefix | 325 | 2.80 / 4.60 / 7.32 | 237 | 4.00 / 5.55 / 8.97 | n/a | n/a | 212 | 4.37 / 6.50 / 10.86 | 424 | 2.23 / 3.11 / 4.31 |
| vector | 546 | 1.79 / 2.07 / 2.46 | 219 | 4.53 / 5.34 / 6.53 | n/a | n/a | 213 | 4.59 / 5.64 / 6.80 | 57 | 16.44 / 21.23 / 24.24 |
| vector + filter | 474 | 2.07 / 2.36 / 2.86 | 199 | 4.94 / 5.74 / 6.71 | n/a | n/a | 125 | 7.95 / 9.42 / 11.17 | 23 | 40.68 / 53.51 / 57.30 |
| hybrid (RRF) | 232 | 4.05 / 5.58 / 9.26 | n/a | n/a | 96 | 10.22 / 12.28 / 14.25 | 125 | 7.70 / 9.89 / 15.99 | 38 | 25.30 / 35.42 / 45.48 |
| hybrid (weighted) | 230 | 4.08 / 5.54 / 9.66 | 136 | 6.43 / 10.79 / 17.73 | 77 | 11.90 / 19.36 / 24.25 | 131 | 7.26 / 9.72 / 15.34 | 115 | 7.46 / 14.66 / 25.26 |
| boosted (bm25 × decay) | 189 | 4.52 / 9.04 / 15.48 | 111 | 7.00 / 18.58 / 30.07 | n/a | n/a | 117 | 6.71 / 16.94 / 28.17 | 120 | 6.38 / 18.98 / 28.63 |
| similar items | 452 | 2.17 / 2.49 / 2.91 | 145 | 6.63 / 7.69 / 9.69 | n/a | n/a | 115 | 8.53 / 10.17 / 11.91 | 138 | 6.15 / 6.93 / 76.39 |
| facet | 619 | 1.56 / 2.04 / 2.49 | 434 | 2.20 / 3.00 / 4.58 | n/a | n/a | 479 | 1.98 / 2.73 / 4.80 | 353 | 2.22 / 4.54 / 12.47 |
| key lookup | 1,187 | 0.81 / 1.11 / 1.37 | 895 | 1.08 / 1.67 / 1.98 | n/a | n/a | 1,090 | 0.90 / 1.41 / 1.73 | 694 | 1.36 / 1.94 / 2.34 |
| sql aggregate | 5 | 182.86 / 299.44 / 330.26 | 145 | 6.82 / 7.97 / 10.20 | n/a | n/a | 193 | 5.17 / 6.15 / 8.59 | 15 | 65.10 / 83.74 / 93.08 |

| kind | Vespa q/s | Vespa ms | Meilisearch q/s | Meilisearch ms | Typesense q/s | Typesense ms | Manticore q/s | Manticore ms | Qdrant q/s | Qdrant ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 81 | 4.42 / 34.13 / 80.99 | 160 | 5.92 / 10.35 / 14.27 | 140 | 4.43 / 15.00 / 49.94 | 133 | 5.52 / 18.10 / 33.58 | 596 | 1.63 / 2.67 / 3.53 |
| bm25 + filter | 162 | 4.44 / 9.56 / 35.23 | 115 | 8.25 / 12.74 / 16.16 | 20 | 48.71 / 54.94 / 94.36 | 159 | 6.46 / 10.96 / 17.35 | 535 | 1.78 / 2.82 / 4.14 |
| phrase | 423 | 2.20 / 3.38 / 5.86 | 328 | 1.93 / 7.45 / 12.02 | 491 | 0.45 / 4.46 / 23.70 | 479 | 1.57 / 4.00 / 8.43 | 486 | 0.96 / 3.82 / 20.35 |
| typo | n/a | n/a | 153 | 6.17 / 10.59 / 14.59 | 60 | 6.02 / 51.38 / 135.69 | 18 | 49.69 / 94.13 / 133.55 | n/a | n/a |
| prefix | n/a | n/a | 178 | 5.21 / 8.98 / 13.29 | 84 | 7.92 / 22.64 / 67.50 | 51 | 12.77 / 40.47 / 160.58 | n/a | n/a |
| vector | 250 | 3.31 / 3.92 / 35.16 | 218 | 4.54 / 5.37 / 6.20 | 224 | 4.42 / 4.82 / 5.75 | 733 | 1.35 / 1.68 / 2.01 | 576 | 1.68 / 2.05 / 2.53 |
| vector + filter | 170 | 5.87 / 6.67 / 7.43 | 80 | 12.42 / 14.86 / 16.89 | 38 | 26.20 / 27.43 / 28.72 | 95 | 10.48 / 11.75 / 12.68 | 288 | 3.43 / 4.15 / 5.23 |
| hybrid (RRF) | 95 | 7.50 / 12.82 / 54.61 | 107 | 9.20 / 14.21 / 18.00 | 104 | 6.42 / 17.50 / 53.11 | 40 | 17.01 / 65.02 / 130.45 | 108 | 8.66 / 12.21 / 14.91 |
| hybrid (weighted) | 93 | 7.79 / 13.42 / 54.72 | 95 | 10.19 / 14.94 / 19.49 | 105 | 6.37 / 17.15 / 53.71 | n/a | n/a | 108 | 8.76 / 12.14 / 14.76 |
| boosted (bm25 × decay) | 234 | 3.21 / 5.55 / 21.28 | n/a | n/a | n/a | n/a | 121 | 6.17 / 19.70 / 35.24 | 504 | 1.95 / 2.99 / 3.88 |
| similar items | 200 | 4.99 / 5.61 / 6.27 | 114 | 8.78 / 10.71 / 11.99 | 10 | 99.87 / 103.85 / 107.73 | 284 | 3.39 / 5.19 / 6.19 | 112 | 8.93 / 11.11 / 13.86 |
| facet | 117 | 3.41 / 18.17 / 73.60 | 333 | 2.96 / 5.25 / 9.13 | 98 | 3.87 / 22.51 / 98.89 | 402 | 1.65 / 5.14 / 9.08 | 57 | 12.43 / 38.71 / 53.39 |
| key lookup | 808 | 1.05 / 1.44 / 2.20 | 3,393 | 0.28 / 0.35 / 0.49 | 1,831 | 0.52 / 0.64 / 0.86 | 2,144 | 0.45 / 0.54 / 0.67 | 3,364 | 0.29 / 0.35 / 0.46 |
| sql aggregate | 9 | 98.45 / 205.89 / 220.10 | n/a | n/a | n/a | n/a | 29 | 33.68 / 48.75 / 61.24 | n/a | n/a |

| kind | Weaviate q/s | Weaviate ms | Milvus q/s | Milvus ms | ParadeDB q/s | ParadeDB ms | Postgres q/s | Postgres ms | Redis q/s | Redis ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 305 | 2.68 / 5.33 / 12.49 | 470 | 2.00 / 2.70 / 3.86 | 71 | 16.45 / 18.56 / 20.23 | 4 | 255.23 / 855.92 / 878.20 | 13 | 68.14 / 133.16 / 176.30 |
| bm25 + filter | 238 | 3.66 / 6.43 / 12.56 | 344 | 2.84 / 3.31 / 4.34 | 61 | 18.28 / 20.46 / 23.07 | 5 | 108.77 / 739.42 / 771.10 | 13 | 72.69 / 117.46 / 137.81 |
| phrase | n/a | n/a | 510 | 1.92 / 2.36 / 2.97 | 228 | 3.78 / 6.39 / 11.72 | 95 | 4.24 / 23.45 / 90.39 | 1,106 | 0.60 / 1.90 / 4.50 |
| typo | n/a | n/a | n/a | n/a | 14 | 70.25 / 102.44 / 122.46 | n/a | n/a | 5 | 208.87 / 297.88 / 327.26 |
| prefix | 170 | 5.16 / 9.92 / 16.72 | n/a | n/a | 52 | 18.55 / 22.64 / 27.26 | 3 | 268.55 / 600.62 / 923.06 | 90 | 6.84 / 23.41 / 66.22 |
| vector | 184 | 5.22 / 5.86 / 9.87 | 368 | 2.65 / 3.02 / 3.69 | 54 | 18.42 / 21.04 / 23.63 | 309 | 3.08 / 4.19 / 6.22 | 942 | 1.03 / 1.30 / 1.66 |
| vector + filter | 131 | 7.50 / 8.45 / 11.09 | 292 | 3.35 / 3.80 / 4.57 | 34 | 29.51 / 32.27 / 34.40 | 277 | 3.21 / 4.71 / 8.36 | 507 | 1.90 / 2.71 / 3.47 |
| hybrid (RRF) | 114 | 7.69 / 11.50 / 22.85 | 168 | 5.85 / 6.67 / 8.74 | 26 | 40.08 / 44.16 / 48.35 | 4 | 210.77 / 867.28 / 909.50 | 13 | 68.13 / 135.07 / 168.31 |
| hybrid (weighted) | 114 | 7.78 / 11.96 / 21.98 | 170 | 5.77 / 6.66 / 8.68 | 25 | 41.72 / 46.31 / 51.03 | 4 | 222.39 / 860.81 / 914.66 | 13 | 67.28 / 134.54 / 171.41 |
| boosted (bm25 × decay) | n/a | n/a | 325 | 2.92 / 3.77 / 5.42 | 1 | 705.74 / 1476.37 / 1508.55 | 3 | 303.29 / 458.83 / 1271.98 | 2 | 787.31 / 973.46 / 1528.40 |
| similar items | 118 | 8.25 / 9.08 / 16.16 | 305 | 3.19 / 3.63 / 4.78 | 6 | 168.06 / 185.26 / 211.91 | 338 | 2.81 / 3.78 / 5.46 | 507 | 1.89 / 2.72 / 3.58 |
| facet | 18 | 13.69 / 108.13 / 721.03 | n/a | n/a | 202 | 4.23 / 6.62 / 16.65 | 46 | 5.38 / 56.56 / 147.92 | 18 | 9.86 / 115.78 / 499.43 |
| key lookup | 3,205 | 0.30 / 0.38 / 0.51 | 951 | 1.03 / 1.14 / 1.48 | 3,250 | 0.30 / 0.35 / 0.44 | 4,696 | 0.21 / 0.25 / 0.33 | 10,714 | 0.09 / 0.11 / 0.13 |
| sql aggregate | 0 | 3552.17 / 3552.17 / 3552.17 | n/a | n/a | 23 | 43.04 / 58.41 / 82.70 | 5 | 210.96 / 230.36 / 279.88 | 0 | 2058.91 / 2128.48 / 2128.48 |


## 4 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | Elasticsearch, trial licence q/s | Elasticsearch, trial licence ms | OpenSearch q/s | OpenSearch ms | Solr q/s | Solr ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 1,379 | 2.62 / 4.18 / 7.42 | 988 | 3.83 / 5.48 / 8.00 | n/a | n/a | 1,063 | 3.59 / 5.07 / 7.01 | 330 | 10.50 / 20.34 / 35.29 |
| bm25 + filter | 1,020 | 3.37 / 6.48 / 10.99 | 854 | 4.36 / 6.56 / 11.13 | n/a | n/a | 855 | 4.31 / 6.71 / 11.48 | 403 | 8.88 / 15.68 / 25.70 |
| phrase | 1,465 | 2.53 / 3.78 / 6.09 | 1,167 | 3.35 / 5.13 / 7.30 | n/a | n/a | 1,206 | 3.19 / 5.04 / 7.32 | 1,348 | 2.69 / 4.35 / 7.16 |
| typo | 133 | 30.84 / 43.11 / 60.35 | 169 | 22.39 / 37.40 / 52.71 | n/a | n/a | 111 | 33.31 / 57.69 / 84.28 | 131 | 27.98 / 47.76 / 81.25 |
| prefix | 896 | 4.04 / 7.00 / 11.34 | 738 | 5.05 / 7.57 / 12.07 | n/a | n/a | 669 | 5.51 / 8.68 / 14.40 | 1,240 | 3.03 / 4.56 / 6.79 |
| vector | 1,418 | 2.76 / 3.40 / 4.25 | 735 | 5.28 / 6.76 / 8.64 | n/a | n/a | 633 | 6.06 / 8.33 / 10.92 | 127 | 30.06 / 42.98 / 55.26 |
| vector + filter | 1,294 | 2.99 / 3.81 / 4.96 | 649 | 5.76 / 7.25 / 10.20 | n/a | n/a | 349 | 11.11 / 14.71 / 19.04 | 49 | 79.28 / 109.90 / 133.76 |
| hybrid (RRF) | 568 | 6.52 / 9.70 / 14.88 | n/a | n/a | 289 | 13.46 / 17.00 / 20.95 | 343 | 11.05 / 15.78 / 22.89 | 86 | 44.15 / 64.94 / 88.06 |
| hybrid (weighted) | 561 | 6.62 / 9.83 / 14.92 | 380 | 9.32 / 16.06 / 29.14 | 222 | 16.72 / 25.68 / 36.24 | 356 | 10.63 / 15.10 / 23.31 | 271 | 12.48 / 25.68 / 43.18 |
| boosted (bm25 × decay) | 493 | 6.76 / 13.75 / 25.48 | 292 | 11.15 / 27.65 / 47.53 | n/a | n/a | 299 | 10.94 / 26.43 / 46.55 | 298 | 10.45 / 29.34 / 51.21 |
| similar items | 1,118 | 3.48 / 4.32 / 5.56 | 416 | 9.31 / 11.93 / 15.01 | n/a | n/a | 319 | 12.18 / 16.13 / 19.71 | 323 | 10.44 / 14.93 / 104.64 |
| facet | 1,539 | 2.51 / 3.39 / 4.39 | 1,662 | 2.22 / 3.37 / 5.36 | n/a | n/a | 1,791 | 2.01 / 3.13 / 6.00 | 1,108 | 2.83 / 6.13 / 16.86 |
| key lookup | 3,904 | 0.98 / 1.34 / 1.79 | 3,933 | 1.03 / 1.40 / 1.88 | n/a | n/a | 4,958 | 0.82 / 1.14 / 1.56 | 2,830 | 1.35 / 1.83 / 2.52 |
| sql aggregate | 10 | 404.70 / 594.17 / 616.06 | 550 | 7.01 / 8.72 / 11.11 | n/a | n/a | 671 | 5.80 / 7.60 / 9.88 | 41 | 95.25 / 127.77 / 159.18 |

| kind | Vespa q/s | Vespa ms | Meilisearch q/s | Meilisearch ms | Typesense q/s | Typesense ms | Manticore q/s | Manticore ms | Qdrant q/s | Qdrant ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 290 | 4.47 / 54.44 / 78.71 | 578 | 6.37 / 11.65 / 17.54 | 445 | 5.65 / 19.17 / 62.72 | 154 | 13.82 / 59.34 / 168.35 | 1,785 | 2.16 / 3.47 / 4.68 |
| bm25 + filter | 388 | 5.15 / 23.84 / 64.10 | 372 | 10.05 / 16.48 / 21.71 | 66 | 57.00 / 77.87 / 135.66 | 223 | 12.65 / 39.11 / 91.92 | 1,643 | 2.32 / 3.62 / 5.41 |
| phrase | 812 | 2.68 / 8.14 / 40.64 | 1,035 | 2.46 / 9.33 / 16.08 | 1,545 | 0.46 / 6.38 / 29.83 | 808 | 2.84 / 11.75 / 28.12 | 1,423 | 1.30 / 5.44 / 28.12 |
| typo | n/a | n/a | 514 | 7.06 / 13.28 / 18.93 | 187 | 7.92 / 67.69 / 157.27 | 55 | 69.32 / 117.97 / 178.79 | n/a | n/a |
| prefix | n/a | n/a | 544 | 6.75 / 12.15 / 17.92 | 267 | 10.03 / 28.87 / 92.60 | 57 | 32.41 / 147.30 / 541.63 | n/a | n/a |
| vector | 431 | 4.18 / 19.78 / 70.97 | 685 | 5.62 / 7.58 / 10.00 | 704 | 5.40 / 7.30 / 9.75 | 2,564 | 1.50 / 2.04 / 2.92 | 1,432 | 2.68 / 3.53 / 4.64 |
| vector + filter | 525 | 7.46 / 9.20 / 12.52 | 275 | 14.28 / 18.31 / 21.92 | 127 | 30.16 / 38.96 / 43.44 | 321 | 12.14 / 15.53 / 17.86 | 846 | 4.65 / 5.73 / 7.37 |
| hybrid (RRF) | 297 | 9.37 / 19.28 / 76.30 | 341 | 11.25 / 18.07 / 24.85 | 331 | 8.40 / 21.47 / 72.86 | 121 | 21.53 / 83.83 / 156.83 | 317 | 12.19 / 16.64 / 20.79 |
| hybrid (weighted) | 290 | 9.93 / 19.37 / 67.05 | 313 | 12.12 / 18.96 / 24.51 | 343 | 8.19 / 20.50 / 65.58 | n/a | n/a | 318 | 12.13 / 16.37 / 20.60 |
| boosted (bm25 × decay) | 824 | 3.52 / 8.21 / 24.29 | n/a | n/a | n/a | n/a | 143 | 15.47 / 67.81 / 171.68 | 1,530 | 2.55 / 3.87 / 5.02 |
| similar items | 488 | 6.41 / 13.68 / 28.77 | 375 | 10.41 / 13.68 / 16.87 | 34 | 114.12 / 141.09 / 159.47 | 1,081 | 3.49 / 5.58 / 7.10 | 364 | 11.08 / 13.59 / 15.82 |
| facet | 437 | 3.46 / 22.89 / 65.15 | 1,078 | 3.53 / 6.51 / 10.27 | 269 | 5.78 / 34.97 / 144.04 | 856 | 2.24 / 10.84 / 34.53 | 68 | 54.93 / 104.64 / 153.69 |
| key lookup | 2,308 | 1.03 / 1.41 / 23.83 | 11,173 | 0.35 / 0.42 / 0.53 | 5,615 | 0.67 / 1.00 / 1.40 | 7,644 | 0.50 / 0.65 / 0.91 | 11,790 | 0.33 / 0.40 / 0.50 |
| sql aggregate | 35 | 109.65 / 176.17 / 190.88 | n/a | n/a | n/a | n/a | 33 | 85.14 / 264.83 / 357.16 | n/a | n/a |

| kind | Weaviate q/s | Weaviate ms | Milvus q/s | Milvus ms | ParadeDB q/s | ParadeDB ms | Postgres q/s | Postgres ms | Redis q/s | Redis ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 931 | 3.30 / 7.35 / 18.25 | 1,171 | 3.21 / 4.66 / 6.41 | 154 | 29.86 / 40.67 / 48.93 | 5 | 655.81 / 1656.03 / 2819.09 | 44 | 82.14 / 160.47 / 213.89 |
| bm25 + filter | 802 | 4.19 / 7.98 / 16.78 | 759 | 5.08 / 6.66 / 8.73 | 129 | 35.17 / 46.07 / 56.39 | 6 | 298.20 / 1677.34 / 2464.65 | 42 | 90.79 / 146.79 / 185.43 |
| phrase | n/a | n/a | 1,325 | 2.91 / 3.95 / 5.28 | 800 | 4.22 / 7.57 / 17.24 | 370 | 4.54 / 24.04 / 93.91 | 3,655 | 0.73 / 2.31 / 5.67 |
| typo | n/a | n/a | n/a | n/a | 17 | 213.30 / 330.35 / 637.27 | n/a | n/a | 17 | 209.87 / 359.50 / 541.74 |
| prefix | 530 | 6.38 / 13.20 / 23.45 | n/a | n/a | 94 | 41.73 / 55.14 / 64.79 | 7 | 422.53 / 937.01 / 1947.41 | 325 | 7.19 / 24.96 / 80.02 |
| vector | 560 | 6.74 / 8.66 / 13.06 | 898 | 4.26 / 5.66 / 7.48 | 107 | 36.87 / 47.88 / 55.67 | 1,233 | 3.08 / 4.24 / 6.85 | 3,001 | 1.29 / 1.70 / 2.24 |
| vector + filter | 402 | 9.56 / 12.26 / 17.48 | 652 | 5.92 / 7.74 / 10.50 | 61 | 65.54 / 79.70 / 92.69 | 1,094 | 3.23 / 4.90 / 8.34 | 1,759 | 2.16 / 3.17 / 4.37 |
| hybrid (RRF) | 307 | 11.39 / 19.37 / 36.10 | 314 | 12.18 / 16.54 / 23.15 | 50 | 81.77 / 101.31 / 111.19 | 4 | 635.47 / 1875.10 / 5752.31 | 44 | 78.34 / 152.72 / 236.96 |
| hybrid (weighted) | 303 | 11.45 / 20.13 / 36.38 | 316 | 12.26 / 16.40 / 21.54 | 47 | 86.66 / 107.79 / 118.57 | 5 | 596.82 / 1833.70 / 2371.14 | 44 | 79.15 / 162.91 / 215.54 |
| boosted (bm25 × decay) | n/a | n/a | 793 | 4.76 / 6.82 / 10.00 | 3 | 1212.87 / 1832.33 / 2528.32 | 4 | 800.05 / 2665.70 / 3366.35 | 2 | 1672.08 / 3164.58 / 3782.02 |
| similar items | 358 | 10.62 / 13.76 / 20.61 | 673 | 5.71 / 7.40 / 10.20 | 8 | 454.23 / 599.90 / 1319.95 | 1,350 | 2.79 / 3.83 / 6.46 | 1,793 | 2.11 / 3.14 / 4.28 |
| facet | 32 | 29.06 / 238.78 / 1232.12 | n/a | n/a | 756 | 4.34 / 7.82 / 17.45 | 119 | 6.04 / 72.77 / 320.69 | 26 | 121.13 / 393.98 / 769.15 |
| key lookup | 11,884 | 0.32 / 0.42 / 0.55 | 3,275 | 1.18 / 1.45 / 1.93 | 11,902 | 0.33 / 0.40 / 0.48 | 16,048 | 0.25 / 0.30 / 0.36 | 38,153 | 0.10 / 0.13 / 0.17 |
| sql aggregate | 0 | 9376.76 / 9407.73 / 9407.73 | n/a | n/a | 82 | 41.67 / 87.98 / 124.41 | 5 | 634.07 / 1062.63 / 2208.69 | 1 | 6584.60 / 8303.06 / 8303.06 |


## 16 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | Elasticsearch, trial licence q/s | Elasticsearch, trial licence ms | OpenSearch q/s | OpenSearch ms | Solr q/s | Solr ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 2,067 | 7.34 / 10.09 / 14.65 | 1,686 | 8.37 / 14.79 / 26.18 | n/a | n/a | 1,917 | 7.59 / 12.93 / 20.19 | 498 | 27.81 / 55.41 / 138.00 |
| bm25 + filter | 1,492 | 9.52 / 16.37 / 26.76 | 1,428 | 9.87 / 18.41 / 30.42 | n/a | n/a | 1,375 | 10.34 / 18.60 / 31.14 | 585 | 23.27 / 48.68 / 118.79 |
| phrase | 2,033 | 7.67 / 9.79 / 13.14 | 2,149 | 6.84 / 12.33 / 19.84 | n/a | n/a | 2,156 | 6.77 / 12.19 / 19.39 | 2,039 | 6.83 / 13.06 / 23.95 |
| typo | 126 | 125.88 / 193.44 / 267.43 | 193 | 77.25 / 118.54 / 159.09 | n/a | n/a | 122 | 123.69 / 180.81 / 262.38 | 141 | 100.03 / 193.14 / 320.95 |
| prefix | 1,192 | 11.25 / 23.43 / 39.97 | 1,186 | 12.28 / 21.34 / 31.32 | n/a | n/a | 1,013 | 13.79 / 26.03 / 39.64 | 1,854 | 7.69 / 14.05 / 23.69 |
| vector | 1,660 | 9.45 / 11.63 / 13.92 | 1,402 | 10.86 / 15.70 / 21.65 | n/a | n/a | 1,048 | 13.94 / 23.77 / 31.94 | 153 | 103.54 / 152.31 / 198.51 |
| vector + filter | 1,558 | 10.00 / 12.45 / 16.10 | 1,207 | 12.61 / 17.91 / 24.26 | n/a | n/a | 464 | 33.36 / 47.15 / 57.44 | 63 | 252.02 / 344.11 / 471.46 |
| hybrid (RRF) | 698 | 21.71 / 30.19 / 45.21 | n/a | n/a | 464 | 33.71 / 44.45 / 55.62 | 408 | 37.54 / 54.68 / 73.58 | 99 | 154.47 / 241.91 / 320.38 |
| hybrid (weighted) | 639 | 23.38 / 33.04 / 52.69 | 523 | 27.44 / 48.02 / 86.16 | 318 | 48.12 / 69.36 / 98.33 | 461 | 33.20 / 48.21 / 63.56 | 297 | 43.02 / 102.30 / 193.82 |
| boosted (bm25 × decay) | 572 | 21.67 / 53.80 / 103.48 | 358 | 39.39 / 76.03 / 119.01 | n/a | n/a | 358 | 38.64 / 76.57 / 126.63 | 336 | 31.94 / 112.34 / 212.14 |
| similar items | 1,282 | 12.25 / 15.05 / 18.46 | 605 | 24.78 / 36.58 / 51.87 | n/a | n/a | 422 | 36.78 / 50.57 / 59.50 | 414 | 32.91 / 53.29 / 291.85 |
| facet | 2,018 | 7.77 / 9.68 / 11.88 | 3,261 | 4.37 / 7.61 / 12.68 | n/a | n/a | 3,462 | 4.13 / 7.23 / 13.10 | 1,659 | 7.51 / 17.76 / 45.55 |
| key lookup | 5,550 | 2.85 / 3.63 / 4.47 | 10,414 | 1.47 / 2.28 / 3.25 | n/a | n/a | 13,515 | 1.12 / 1.83 / 2.80 | 5,439 | 2.71 / 4.54 / 6.81 |
| sql aggregate | 10 | 1468.20 / 2575.98 / 2978.82 | 1,200 | 12.56 / 17.62 / 27.37 | n/a | n/a | 1,053 | 15.87 / 25.72 / 32.52 | 49 | 312.47 / 480.95 / 665.44 |

| kind | Vespa q/s | Vespa ms | Meilisearch q/s | Meilisearch ms | Typesense q/s | Typesense ms | Manticore q/s | Manticore ms | Qdrant q/s | Qdrant ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 404 | 11.09 / 88.09 / 97.35 | 758 | 18.84 / 37.25 / 54.95 | 654 | 15.32 / 48.56 / 163.21 | 150 | 47.41 / 367.19 / 810.22 | 2,461 | 6.08 / 9.19 / 16.15 |
| bm25 + filter | 579 | 14.69 / 73.78 / 90.42 | 578 | 25.97 / 45.43 / 61.60 | 90 | 178.26 / 230.55 / 342.71 | 221 | 42.68 / 81.30 / 1169.76 | 2,213 | 6.79 / 10.42 / 16.82 |
| phrase | 780 | 5.87 / 78.31 / 83.86 | 1,599 | 7.14 / 22.56 / 39.63 | 2,645 | 1.74 / 13.61 / 59.35 | 777 | 15.06 / 36.40 / 82.14 | 1,771 | 7.60 / 15.51 / 36.67 |
| typo | n/a | n/a | 776 | 18.65 / 35.70 / 55.22 | 266 | 27.00 / 144.40 / 498.42 | 75 | 196.93 / 318.44 / 566.66 | n/a | n/a |
| prefix | n/a | n/a | 856 | 16.97 / 32.23 / 47.40 | 385 | 28.62 / 80.30 / 247.01 | 55 | 89.45 / 579.14 / 2926.41 | n/a | n/a |
| vector | 492 | 9.83 / 82.88 / 90.82 | 1,116 | 13.74 / 21.45 / 28.58 | 1,052 | 13.92 / 24.95 / 34.73 | 4,602 | 3.02 / 3.75 / 15.89 | 1,939 | 7.73 / 11.67 / 17.44 |
| vector + filter | 516 | 25.82 / 52.63 / 63.03 | 443 | 35.35 / 52.05 / 67.99 | 175 | 96.01 / 134.98 / 163.71 | 465 | 32.05 / 44.22 / 49.23 | 973 | 15.78 / 21.83 / 27.21 |
| hybrid (RRF) | 308 | 42.99 / 83.28 / 195.61 | 528 | 28.29 / 50.63 / 71.51 | 505 | 22.38 / 56.40 / 172.10 | 147 | 87.66 / 188.77 / 410.70 | 372 | 41.70 / 54.58 / 70.86 |
| hybrid (weighted) | 302 | 48.28 / 81.41 / 165.60 | 486 | 30.90 / 51.55 / 75.02 | 501 | 22.56 / 54.95 / 175.24 | n/a | n/a | 371 | 41.82 / 54.99 / 68.66 |
| boosted (bm25 × decay) | 804 | 10.13 / 57.75 / 75.70 | n/a | n/a | n/a | n/a | 140 | 50.64 / 387.84 / 759.21 | 2,155 | 7.12 / 10.78 / 15.47 |
| similar items | 470 | 19.88 / 69.84 / 77.68 | 614 | 25.30 / 38.14 / 49.63 | 47 | 353.30 / 503.17 / 567.61 | 1,767 | 7.94 / 11.67 / 24.70 | 413 | 37.95 / 49.39 / 56.28 |
| facet | 512 | 18.52 / 72.75 / 143.20 | 1,689 | 8.55 / 16.90 / 25.51 | 368 | 23.49 / 95.41 / 331.89 | 970 | 9.52 / 28.86 / 64.06 | 68 | 235.22 / 307.20 / 388.31 |
| key lookup | 2,640 | 1.54 / 2.63 / 77.11 | 19,397 | 0.81 / 1.05 / 1.45 | 11,108 | 1.35 / 2.08 / 2.89 | 19,140 | 0.73 / 0.94 / 3.08 | 30,941 | 0.49 / 0.68 / 0.98 |
| sql aggregate | 36 | 453.81 / 627.29 / 736.16 | n/a | n/a | n/a | n/a | 39 | 402.66 / 620.85 / 825.71 | n/a | n/a |

| kind | Weaviate q/s | Weaviate ms | Milvus q/s | Milvus ms | ParadeDB q/s | ParadeDB ms | Postgres q/s | Postgres ms | Redis q/s | Redis ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 1,104 | 11.26 / 28.89 / 50.75 | 1,677 | 9.16 / 13.26 / 18.81 | 478 | 24.45 / 72.62 / 99.17 | 5 | 1576.04 / 5244.73 / 8890.02 | 65 | 230.40 / 341.48 / 442.29 |
| bm25 + filter | 1,175 | 11.40 / 24.61 / 43.63 | 1,140 | 13.51 / 18.71 / 24.62 | 303 | 45.84 / 90.65 / 126.17 | 7 | 1006.74 / 6024.94 / 9700.44 | 59 | 264.48 / 350.22 / 407.96 |
| phrase | n/a | n/a | 1,638 | 9.43 / 13.09 / 18.49 | 1,025 | 12.83 / 28.69 / 60.19 | 372 | 15.57 / 98.72 / 364.97 | 5,777 | 2.35 / 4.31 / 7.82 |
| typo | n/a | n/a | n/a | n/a | 17 | 735.79 / 1539.07 / 2416.64 | n/a | n/a | 25 | 600.36 / 826.25 / 1021.23 |
| prefix | 732 | 18.79 / 40.77 / 67.55 | n/a | n/a | 185 | 83.50 / 133.79 / 217.84 | 5 | 2026.58 / 4588.30 / 6778.37 | 495 | 26.72 / 51.26 / 112.81 |
| vector | 739 | 17.29 / 40.02 / 75.12 | 1,266 | 11.94 / 17.32 / 25.83 | 200 | 79.26 / 107.61 / 145.03 | 1,566 | 8.26 / 20.32 / 33.53 | 4,365 | 3.42 / 5.09 / 6.30 |
| vector + filter | 548 | 24.75 / 51.66 / 85.32 | 844 | 17.90 / 25.84 / 33.47 | 89 | 172.61 / 226.57 / 306.29 | 1,342 | 9.03 / 22.56 / 39.30 | 3,269 | 4.69 / 6.25 / 9.78 |
| hybrid (RRF) | 343 | 41.95 / 70.99 / 132.57 | 356 | 44.20 / 54.76 / 65.87 | 83 | 187.41 / 252.70 / 315.67 | 5 | 1533.85 / 5380.48 / 6784.77 | 35 | 453.53 / 585.30 / 723.50 |
| hybrid (weighted) | 348 | 40.61 / 73.31 / 129.93 | 356 | 44.00 / 54.74 / 65.37 | 80 | 194.40 / 255.35 / 331.85 | 5 | 1904.89 / 6091.95 / 7218.94 | 35 | 443.77 / 565.49 / 705.37 |
| boosted (bm25 × decay) | n/a | n/a | 1,068 | 14.36 / 20.60 / 28.30 | 3 | 4256.68 / 7999.42 / 9127.34 | 3 | 2027.92 / 8676.27 / 12156.22 | 2 | 6051.42 / 7387.98 / 8544.28 |
| similar items | 481 | 27.81 / 60.97 / 97.87 | 793 | 19.29 / 25.13 / 34.24 | 10 | 1350.67 / 2766.42 / 2931.01 | 1,730 | 7.57 / 18.53 / 30.83 | 3,093 | 5.02 / 6.99 / 8.21 |
| facet | 36 | 256.70 / 1000.94 / 2519.06 | n/a | n/a | 901 | 14.43 / 31.37 / 72.36 | 127 | 28.45 / 314.15 / 827.96 | 27 | 509.96 / 998.45 / 1501.49 |
| key lookup | 32,331 | 0.44 / 0.68 / 1.23 | 5,756 | 2.62 / 3.79 / 6.06 | 21,648 | 0.66 / 0.88 / 5.01 | 32,242 | 0.41 / 0.63 / 1.06 | 87,556 | 0.17 / 0.25 / 0.33 |
| sql aggregate | 1 | 28687.12 / 29523.93 / 29588.24 | n/a | n/a | 80 | 182.49 / 332.59 / 479.45 | 6 | 2474.65 / 4439.17 / 4758.14 | 0 | 11403.67 / 11403.82 / 17372.47 |


Errors during the timed runs: Strata 0, Elasticsearch 0, Elasticsearch, trial licence 0, OpenSearch 0, Solr 0, Vespa 0, Meilisearch 0, Typesense 0, Manticore 0, Qdrant 0, Weaviate 0, Milvus 0, ParadeDB 1, Postgres 0, Redis 10

## Answers: recall@10 against an exact search, nDCG@10 against the collection's judgments

Unavailable kinds are n/a; recall is for the kinds an exact search can answer.

| kind | Strata recall | Strata nDCG | Elasticsearch recall | Elasticsearch nDCG | Elasticsearch, trial licence recall | Elasticsearch, trial licence nDCG | OpenSearch recall | OpenSearch nDCG | Solr recall | Solr nDCG |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | – | – | – | – | n/a | n/a | – | – | – | – |
| bm25 + filter | – | – | – | – | n/a | n/a | – | – | – | – |
| phrase | – | – | – | – | n/a | n/a | – | – | – | – |
| typo | – | – | – | – | n/a | n/a | – | – | – | – |
| prefix | – | – | – | – | n/a | n/a | – | – | – | – |
| vector | 0.807 | – | 0.720 | – | n/a | n/a | 0.655 | – | 0.851 | – |
| vector + filter | 0.828 | – | 0.692 | – | n/a | n/a | 0.809 | – | 0.945 | – |
| hybrid (RRF) | – | – | n/a | n/a | – | – | – | – | – | – |
| hybrid (weighted) | – | – | – | – | – | – | – | – | – | – |
| boosted (bm25 × decay) | – | – | – | – | n/a | n/a | – | – | – | – |
| similar items | 0.990 | – | 0.924 | – | n/a | n/a | 0.971 | – | 0.996 | – |

| kind | Vespa recall | Vespa nDCG | Meilisearch recall | Meilisearch nDCG | Typesense recall | Typesense nDCG | Manticore recall | Manticore nDCG | Qdrant recall | Qdrant nDCG |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | – | – | – | – | – | – | – | – | – | – |
| bm25 + filter | – | – | – | – | – | – | – | – | – | – |
| phrase | – | – | – | – | – | – | – | – | – | – |
| typo | n/a | n/a | – | – | – | – | – | – | n/a | n/a |
| prefix | n/a | n/a | – | – | – | – | – | – | n/a | n/a |
| vector | 0.784 | – | 0.780 | – | 0.777 | – | 0.696 | – | 0.902 | – |
| vector + filter | 0.861 | – | 0.905 | – | 0.914 | – | 0.675 | – | 0.850 | – |
| hybrid (RRF) | – | – | – | – | – | – | – | – | – | – |
| hybrid (weighted) | – | – | – | – | – | – | n/a | n/a | – | – |
| boosted (bm25 × decay) | – | – | n/a | n/a | n/a | n/a | – | – | – | – |
| similar items | 0.989 | – | 0.984 | – | 0.989 | – | 0.897 | – | 0.969 | – |

| kind | Weaviate recall | Weaviate nDCG | Milvus recall | Milvus nDCG | ParadeDB recall | ParadeDB nDCG | Postgres recall | Postgres nDCG | Redis recall | Redis nDCG |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | – | – | – | – | – | – | – | – | – | – |
| bm25 + filter | – | – | – | – | – | – | – | – | – | – |
| phrase | n/a | n/a | – | – | – | – | – | – | – | – |
| typo | n/a | n/a | n/a | n/a | – | – | n/a | n/a | – | – |
| prefix | – | – | n/a | n/a | – | – | – | – | – | – |
| vector | 0.750 | – | 0.881 | – | 0.716 | – | 0.792 | – | 0.750 | – |
| vector + filter | 0.742 | – | 0.909 | – | 0.854 | – | 0.742 | – | 0.721 | – |
| hybrid (RRF) | – | – | – | – | – | – | – | – | – | – |
| hybrid (weighted) | – | – | – | – | – | – | – | – | – | – |
| boosted (bm25 × decay) | n/a | n/a | – | – | – | – | – | – | – | – |
| similar items | 0.971 | – | 0.979 | – | 0.968 | – | 0.985 | – | 0.969 | – |


## SQL aggregates equal to the true answer

| Strata | Elasticsearch | Elasticsearch, trial licence | OpenSearch | Solr | Vespa | Meilisearch | Typesense | Manticore | Qdrant | Weaviate | Milvus | ParadeDB | Postgres | Redis |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 300 of 300 | 300 of 300 | n/a | 300 of 300 | 300 of 300 | 300 of 300 | n/a | n/a | 300 of 300 | n/a | 300 of 300 | n/a | 300 of 300 | 300 of 300 | 300 of 300 |

## Agreement with Strata's answers

| kind | Elasticsearch | Elasticsearch, trial licence | OpenSearch | Solr | Vespa | Meilisearch | Typesense |
|---|---|---|---|---|---|---|---|
| bm25 | 0.957 of the top 10 | n/a | 0.957 of the top 10 | 0.955 of the top 10 | 0.932 of the top 10 | 0.127 of the top 10 | 0.215 of the top 10 |
| bm25 + filter | 0.952 of the top 10 | n/a | 0.948 of the top 10 | 0.952 of the top 10 | 0.933 of the top 10 | 0.127 of the top 10 | 0.213 of the top 10 |
| phrase | 0.546 of the top 10 | n/a | 0.544 of the top 10 | 0.544 of the top 10 | 0.649 of the top 10 | 0.217 of the top 10 | 0.057 of the top 10 |
| typo | 0.422 of the top 10 | n/a | 0.424 of the top 10 | 0.262 of the top 10 | n/a | 0.100 of the top 10 | 0.169 of the top 10 |
| prefix | 0.965 of the top 10 | n/a | 0.964 of the top 10 | 0.966 of the top 10 | n/a | 0.132 of the top 10 | 0.171 of the top 10 |
| vector | 0.754 of the top 10 | n/a | 0.656 of the top 10 | 0.794 of the top 10 | 0.794 of the top 10 | 0.781 of the top 10 | 0.776 of the top 10 |
| vector + filter | 0.710 of the top 10 | n/a | 0.808 of the top 10 | 0.843 of the top 10 | 0.833 of the top 10 | 0.850 of the top 10 | 0.863 of the top 10 |
| hybrid (RRF) | n/a | 0.811 of the top 10 | 0.762 of the top 10 | 0.603 of the top 10 | 0.732 of the top 10 | 0.316 of the top 10 | 0.360 of the top 10 |
| hybrid (weighted) | 0.454 of the top 10 | 0.708 of the top 10 | 0.697 of the top 10 | 0.442 of the top 10 | 0.736 of the top 10 | 0.640 of the top 10 | 0.591 of the top 10 |
| boosted (bm25 × decay) | 0.970 of the top 10 | n/a | 0.972 of the top 10 | 0.972 of the top 10 | 0.854 of the top 10 | n/a | n/a |
| similar items | 0.926 of the top 10 | n/a | 0.970 of the top 10 | 0.991 of the top 10 | 0.984 of the top 10 | 0.979 of the top 10 | 0.988 of the top 10 |
| facet | 286 of 300 equal | n/a | 286 of 300 equal | 284 of 300 equal | 131 of 300 equal | 51 of 300 equal | 46 of 300 equal |
| key lookup | 300 of 300 equal | n/a | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal |

| kind | Manticore | Qdrant | Weaviate | Milvus | ParadeDB | Postgres | Redis |
|---|---|---|---|---|---|---|---|
| bm25 | 0.557 of the top 10 | 0.943 of the top 10 | 0.530 of the top 10 | 0.745 of the top 10 | 0.750 of the top 10 | 0.462 of the top 10 | 0.397 of the top 10 |
| bm25 + filter | 0.592 of the top 10 | 0.932 of the top 10 | 0.532 of the top 10 | 0.749 of the top 10 | 0.754 of the top 10 | 0.465 of the top 10 | 0.416 of the top 10 |
| phrase | 0.456 of the top 10 | 0.640 of the top 10 | n/a | 0.536 of the top 10 | 0.533 of the top 10 | 0.467 of the top 10 | 0.373 of the top 10 |
| typo | 0.065 of the top 10 | n/a | n/a | n/a | 0.023 of the top 10 | n/a | 0.074 of the top 10 |
| prefix | 0.420 of the top 10 | n/a | 0.564 of the top 10 | n/a | 0.616 of the top 10 | 0.526 of the top 10 | 0.140 of the top 10 |
| vector | 0.689 of the top 10 | 0.825 of the top 10 | 0.766 of the top 10 | 0.814 of the top 10 | 0.684 of the top 10 | 0.782 of the top 10 | 0.769 of the top 10 |
| vector + filter | 0.701 of the top 10 | 0.815 of the top 10 | 0.760 of the top 10 | 0.831 of the top 10 | 0.783 of the top 10 | 0.776 of the top 10 | 0.765 of the top 10 |
| hybrid (RRF) | 0.673 of the top 10 | 0.860 of the top 10 | 0.623 of the top 10 | 0.755 of the top 10 | 0.649 of the top 10 | 0.584 of the top 10 | 0.566 of the top 10 |
| hybrid (weighted) | n/a | 0.703 of the top 10 | 0.696 of the top 10 | 0.750 of the top 10 | 0.636 of the top 10 | 0.683 of the top 10 | 0.297 of the top 10 |
| boosted (bm25 × decay) | 0.546 of the top 10 | 0.646 of the top 10 | n/a | 0.580 of the top 10 | 0.743 of the top 10 | 0.491 of the top 10 | 0.431 of the top 10 |
| similar items | 0.898 of the top 10 | 0.966 of the top 10 | 0.971 of the top 10 | 0.975 of the top 10 | 0.962 of the top 10 | 0.985 of the top 10 | 0.972 of the top 10 |
| facet | 279 of 300 equal | 274 of 300 equal | 56 of 300 equal | n/a | 274 of 300 equal | 83 of 300 equal | 28 of 300 equal |
| key lookup | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal |


## Writes: documents a second, and latency in ms (p50 / p99)

| phase | clients | Strata docs/s | Strata ms | Elasticsearch docs/s | Elasticsearch ms | OpenSearch docs/s | OpenSearch ms | Solr docs/s | Solr ms | Vespa docs/s | Vespa ms |
|---|---|---|---|---|---|---|---|---|---|---|---|
| insert one | 1 | 417 | 2.54 / 4.61 | 234 | 3.89 / 12.03 | 267 | 3.67 / 7.88 | 520 | 1.78 / 4.29 | 177 | 5.32 / 8.70 |
| insert one | 4 | 1,321 | 2.76 / 5.58 | 662 | 4.97 / 24.71 | 850 | 4.39 / 10.03 | 1,593 | 2.11 / 6.52 | 582 | 6.25 / 13.53 |
| insert one | 16 | 2,153 | 5.70 / 64.30 | 2,019 | 6.78 / 31.18 | 2,235 | 6.44 / 21.95 | 2,557 | 5.16 / 22.88 | 718 | 12.51 / 71.87 |
| insert batch | 1 | 2,127 | 32.37 / 107.59 | 1,760 | 42.97 / 156.31 | 1,402 | 59.04 / 118.34 | 1,493 | 42.26 / 158.41 | 795 | 117.36 / 180.84 |
| insert batch | 4 | 3,469 | 88.11 / 352.38 | 3,697 | 72.22 / 376.42 | 3,385 | 99.02 / 230.61 | 4,075 | 67.14 / 267.21 | 655 | 389.15 / 1433.29 |
| insert batch | 16 | 3,955 | 379.01 / 715.75 | 3,828 | 375.56 / 1100.99 | 3,385 | 438.76 / 645.80 | 3,788 | 383.30 / 685.75 | 652 | 2276.34 / 4176.64 |
| update one | 1 | 547 | 2.03 / 3.77 | 222 | 3.93 / 11.58 | 242 | 3.83 / 9.32 | 516 | 1.72 / 4.68 | 287 | 3.12 / 28.11 |
| update one | 4 | 2,042 | 2.01 / 3.95 | 806 | 4.29 / 18.19 | 867 | 4.26 / 10.55 | 1,310 | 2.51 / 10.56 | 812 | 3.28 / 48.00 |
| update one | 16 | 5,495 | 2.83 / 5.52 | 2,329 | 5.90 / 22.90 | 2,198 | 6.57 / 19.41 | 2,091 | 6.32 / 25.47 | 1,636 | 4.24 / 60.98 |

## How soon a written document is found (ms from the write's start)

| engine | searchable | write returned (p50) | found (p50) | found (p90) | found (max) |
|---|---|---|---|---|---|
| Strata | searchable when the write returns | 2.1 | 5.4 | 7.5 | 84.6 |
| Elasticsearch | refresh_interval 1s (the default; loading turned it off) | 6.9 | 703.6 | 1012.2 | 1031.5 |
| OpenSearch | refresh_interval 1s (the default; loading turned it off) | 5.0 | 1005.7 | 1020.9 | 1084.7 |
| Solr | autoSoftCommit.maxTime 1000 ms (as Elasticsearch's 1 s refresh; loading turned it off) | 2.8 | 1014.6 | 1022.2 | 1514.1 |
| Vespa | searchable when the write returns | 6.6 | 8.6 | 10.3 | 35.2 |

## Streams through Redpanda: documents written a second, and lag in ms from when a document was due (p50 / p99)

| phase | Strata docs/s | Strata lag | Elasticsearch docs/s | Elasticsearch lag | OpenSearch docs/s | OpenSearch lag | Solr docs/s | Solr lag | Vespa docs/s | Vespa lag |
|---|---|---|---|---|---|---|---|---|---|---|
| 250/s | 249 | 77 / 105 | 250 | 24 / 44 | 250 | 23 / 40 | 250 | 20 / 36 | 250 | 26 / 41 |
| 500/s | 499 | 77 / 107 | 500 | 24 / 48 | 499 | 24 / 47 | 499 | 19 / 34 | 499 | 28 / 44 |
| 1000/s | 997 | 77 / 399 | 999 | 28 / 79 | 999 | 26 / 74 | 999 | 21 / 37 | 783 | 5406 / 9692 (fell behind) |
| 2000/s | 1,993 | 85 / 521 | 1,998 | 32 / 165 | 1,998 | 34 / 195 | 1,984 | 30 / 311 | not run | not run |
| flat out | 3,827 | 2950 / 4497 | 3,543 | 2352 / 4924 | 2,859 | 2716 / 6365 | 3,178 | 2676 / 5612 | 802 | 13123 / 24132 |

Lag includes reading the topic (Redpanda's part, the same for every engine, is the 'read' column below).

| engine | what written means | read lag p50 (ms) | crash: events written again | documents lost |
|---|---|---|---|---|
| Strata | searchable (the server answers a batch once its rows are visible) | 33 | 0 (the consumer resumes after the offsets the server keeps with the rows (GET /v1/sources)) | 0 |
| Elasticsearch | written: searchable within the 1 s refresh | 19 | 1436 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |
| OpenSearch | written: searchable within the 1 s refresh | 19 | 718 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |
| Solr | written: searchable within the 1 s soft commit | 18 | 193 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |
| Vespa | searchable (a put is visible when it returns) | 19 | 140 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |

## CPU used by each engine's containers (cgroup cpu.stat)

| engine | phase | pod | CPU seconds | wall seconds | average cores | throttled (s) |
|---|---|---|---|---|---|---|
| Strata | timed run | strata-0 | 462 | 423 | 1.09 | 2 |
| Strata | timed run | strata-1 | 583 | 422 | 1.38 | 6 |
| Strata | timed run | strata-coordinator | 219 | 423 | 0.52 | 0 |
| Strata | writes | strata-0 | 36 | 70 | 0.51 | 1 |
| Strata | writes | strata-1 | 47 | 69 | 0.68 | 2 |
| Strata | writes | strata-coordinator | 28 | 69 | 0.41 | 0 |
| Strata | stream | strata-0 | 55 | 150 | 0.37 | 1 |
| Strata | stream | strata-1 | 75 | 150 | 0.50 | 2 |
| Strata | stream | strata-coordinator | 27 | 151 | 0.18 | 0 |
| Elasticsearch | load | es-0 | 1188 | 599 | 1.98 | 66 |
| Elasticsearch | load | es-1 | 1533 | 599 | 2.56 | 63 |
| Elasticsearch | timed run | es-0 | 659 | 394 | 1.67 | 0 |
| Elasticsearch | timed run | es-1 | 799 | 394 | 2.03 | 9 |
| Elasticsearch | writes | es-0 | 113 | 102 | 1.11 | 2 |
| Elasticsearch | writes | es-1 | 132 | 103 | 1.29 | 1 |
| Elasticsearch | stream | es-0 | 124 | 151 | 0.82 | 0 |
| Elasticsearch | stream | es-1 | 151 | 151 | 1.00 | 1 |
| Elasticsearch, trial licence | timed run | es-0 | 102 | 72 | 1.41 | 0 |
| Elasticsearch, trial licence | timed run | es-1 | 130 | 72 | 1.80 | 1 |
| OpenSearch | load | os-0 | 1267 | 813 | 1.56 | 4 |
| OpenSearch | load | os-1 | 1930 | 813 | 2.37 | 176 |
| OpenSearch | timed run | os-0 | 696 | 416 | 1.67 | 0 |
| OpenSearch | timed run | os-1 | 865 | 417 | 2.07 | 22 |
| OpenSearch | writes | os-0 | 115 | 109 | 1.05 | 0 |
| OpenSearch | writes | os-1 | 130 | 109 | 1.19 | 3 |
| OpenSearch | stream | os-0 | 134 | 154 | 0.87 | 0 |
| OpenSearch | stream | os-1 | 173 | 155 | 1.11 | 3 |
| Solr | load | solr-0 | 2237 | 1124 | 1.99 | 139 |
| Solr | load | solr-1 | 2492 | 1124 | 2.22 | 142 |
| Solr | timed run | solr-0 | 603 | 460 | 1.31 | 0 |
| Solr | timed run | solr-1 | 1111 | 461 | 2.41 | 16 |
| Solr | writes | solr-0 | 155 | 111 | 1.40 | 2 |
| Solr | writes | solr-1 | 161 | 111 | 1.45 | 2 |
| Solr | stream | solr-0 | 143 | 151 | 0.95 | 1 |
| Solr | stream | solr-1 | 170 | 151 | 1.12 | 1 |
| Vespa | load | vespa-0 | 1467 | 1479 | 0.99 | 5944 |
| Vespa | load | vespa-1 | 1610 | 1479 | 1.09 | 4 |
| Vespa | load | vespa-2 | 2056 | 1479 | 1.39 | 14 |
| Vespa | timed run | vespa-0 | 275 | 382 | 0.72 | 214 |
| Vespa | timed run | vespa-1 | 346 | 382 | 0.91 | 2 |
| Vespa | timed run | vespa-2 | 502 | 382 | 1.32 | 25 |
| Vespa | writes | vespa-0 | 70 | 89 | 0.78 | 62 |
| Vespa | writes | vespa-1 | 53 | 90 | 0.59 | 0 |
| Vespa | writes | vespa-2 | 69 | 90 | 0.77 | 0 |
| Vespa | stream | vespa-0 | 151 | 204 | 0.74 | 261 |
| Vespa | stream | vespa-1 | 153 | 204 | 0.75 | 0 |
| Vespa | stream | vespa-2 | 197 | 204 | 0.96 | 0 |
| Meilisearch | load | meili-0 | 7221 | 1627 | 4.44 | 298 |
| Meilisearch | timed run | meili-0 | 1043 | 326 | 3.20 | 5 |
| Typesense | load | typesense-0 | 3929 | 1304 | 3.01 | 0 |
| Typesense | timed run | typesense-0 | 1145 | 379 | 3.02 | 60 |
| Manticore | load | manticore-0 | 7155 | 1873 | 3.82 | 579 |
| Manticore | timed run | manticore-0 | 1859 | 391 | 4.75 | 91 |
| Qdrant | load | qdrant-0 | 21347 | 3923 | 5.44 | 86 |
| Qdrant | timed run | qdrant-0 | 1265 | 300 | 4.22 | 21 |
| Weaviate | load | weaviate-0 | 3496 | 551 | 6.34 | 6 |
| Weaviate | timed run | weaviate-0 | 4928 | 987 | 4.99 | 3 |
| Milvus | load | milvus-0 | 3022 | 587 | 5.15 | 127 |
| Milvus | timed run | milvus-0 | 1216 | 264 | 4.61 | 12 |
| ParadeDB | load | paradedb-0 | 555 | 212 | 2.62 | 1 |
| ParadeDB | timed run | paradedb-0 | 2845 | 883 | 3.22 | 104 |
| Postgres | load | postgres-0 | 3833 | 1222 | 3.14 | 14 |
| Postgres | timed run | postgres-0 | 4778 | 1010 | 4.73 | 196 |
| Redis | load | redis-0 | 1631 | 395 | 4.13 | 0 |
| Redis | timed run | redis-0 | 1907 | 1217 | 1.57 | 9 |

## Memory of each engine's containers (cgroup memory.current, memory.stat, memory.peak; GB, all its pods)

| engine | after the load | of it anonymous | after the timed run | of it anonymous | of it files (page cache) | pods' peaks added |
|---|---|---|---|---|---|---|
| Strata | – | – | 0.64 | 0.55 | 0.07 | 6.77 |
| Elasticsearch | 35.20 | 30.09 | 35.84 | 30.72 | 4.89 | 42.00 |
| Elasticsearch, trial licence | – | – | 35.93 | 30.81 | 4.89 | 42.00 |
| OpenSearch | 35.44 | 30.08 | 35.92 | 30.56 | 5.13 | 45.29 |
| Solr | 27.98 | 18.52 | 24.46 | 19.25 | 5.00 | 33.26 |
| Vespa | 10.84 | 6.25 | 9.84 | 4.84 | 4.80 | 15.88 |
| Meilisearch | 23.30 | 1.80 | 22.74 | 1.31 | 20.63 | 25.00 |
| Typesense | 24.15 | 4.51 | 24.23 | 4.63 | 18.72 | 25.87 |
| Manticore | 6.71 | 1.43 | 6.94 | 1.67 | 5.04 | 12.82 |
| Qdrant | 9.73 | 3.45 | 9.80 | 3.49 | 4.66 | 24.64 |
| Weaviate | 12.81 | 3.43 | 11.38 | 3.09 | 7.15 | 16.27 |
| Milvus | 39.52 | 11.21 | 38.45 | 10.82 | 24.81 | 42.80 |
| ParadeDB | 32.25 | 0.01 | 34.64 | 0.01 | 33.94 | 35.08 |
| Postgres | 27.49 | 0.02 | 27.50 | 0.02 | 26.94 | 30.21 |
| Redis | 8.12 | 8.10 | 8.16 | 8.14 | 0.00 | 9.28 |

- Elasticsearch, hybrid (RRF): unavailable: {"license.expired.feature":"Reciprocal Rank Fusion (RRF)","reason":"current license is non-compliant for [Reciprocal Rank Fusion (RRF)]","root_cause":[{"license.expired.feature":"Reciprocal Rank Fusion (RRF)","reason":"current license is non-compliant for [Reciprocal Rank Fusion (RRF)]","type":"secu

- Elasticsearch, hybrid (RRF): the rrf retriever

- Elasticsearch, hybrid (weighted): no min-max normalised fusion on the free licence (the linear retriever needs a paid one): the sum of 0.2 x the raw BM25 score and 0.8 x the vector score ((1 + dot) / 2) over the lexical matches and the knn top 100

- Elasticsearch, trial licence, hybrid (RRF): the rrf retriever

- Elasticsearch, trial licence, hybrid (weighted): the linear retriever (a paid feature, here on a trial licence): match and knn top 100, each min-max normalised over the top 100, weighted 0.2 and 0.8

- OpenSearch, hybrid (RRF): a hybrid query (match, knn k 100, pagination depth 100) through a search pipeline's score-ranker-processor, rrf, k 60

- OpenSearch, hybrid (weighted): a hybrid query (match, knn k 100, pagination depth 100) through a search pipeline's normalization-processor: min_max, arithmetic_mean weighted 0.2 and 0.8

- Solr, hybrid (RRF): the combined query handler, rrf, k 60, knn topK 100; the lexical query's depth is the request's limit

- Solr, hybrid (weighted): no min-max normalised fusion: the sum of 0.2 x the raw BM25 score (scaled by k1 + 1 = 2.2 as Elasticsearch's) and 0.8 x the vector score ((1 + dot) / 2) over the lexical matches and the knn top 100

- Solr, typo: each word~N with Strata's 'auto' edits; the Lucene parser does not stem a fuzzy word, so it is compared with the stemmed terms as typed

- Vespa, prefix: unavailable: Vespa matches prefixes on attributes only, not on indexed text fields (n-gram fields are its way)

- Vespa, typo: unavailable: Vespa matches typos (fuzzy) on attributes only, not on indexed text fields

- Vespa, bm25: weakAnd (Vespa's text query, which keeps the best 100 matches by a cheaper score, then ranks them by BM25)

- Vespa, bm25 + filter: weakAnd (Vespa's text query, which keeps the best 100 matches by a cheaper score, then ranks them by BM25)

- Vespa, boosted (bm25 × decay): weakAnd (Vespa's text query, which keeps the best 100 matches by a cheaper score, then ranks them by BM25)

- Vespa, facet: grouping over the rows whose body contains the word, precision 17 (each content node sends its 17 largest groups, as Elasticsearch's default shard_size of size x 1.5 + 10 does; at Vespa's default precision the counts were wrong in 11 of 60 SciFact facets)

- Vespa, hybrid (RRF): the union of the lexical matches and the nearestNeighbor top 100, ranked first by BM25 + closeness on each content node; the best 200 of those re-ranked in the global phase by reciprocal_rank_fusion (k 60) or 0.2 x normalize_linear(bm25) + 0.8 x normalize_linear(closeness)

- Vespa, hybrid (weighted): the union of the lexical matches and the nearestNeighbor top 100, ranked first by BM25 + closeness on each content node; the best 200 of those re-ranked in the global phase by reciprocal_rank_fusion (k 60) or 0.2 x normalize_linear(bm25) + 0.8 x normalize_linear(closeness)

- Vespa, phrase: weakAnd (Vespa's text query, which keeps the best 100 matches by a cheaper score, then ranks them by BM25)

- Vespa, sql aggregate: grouping (no SQL): the 10 largest categories of the filtered rows, those of 1,000 rows or fewer dropped by the client (grouping has no HAVING); precision 1,000, so that every content node sends all its groups and the counts are exact

- Meilisearch, boosted (bm25 × decay): unavailable: no function scores: a numeric field can sort results (sort ranking rule), not multiply their relevance

- Meilisearch, sql aggregate: unavailable: no grouped aggregates: facets count values, and facet stats give a numeric field's minimum and maximum only

- Meilisearch, bm25: Meilisearch's ranking rules (words, typo, proximity, attribute, exactness), not BM25; no stemming; typos and a prefix last word on, as by default

- Meilisearch, bm25 + filter: Meilisearch's ranking rules (words, typo, proximity, attribute, exactness), not BM25; no stemming; typos and a prefix last word on, as by default

- Meilisearch, facet: the word as a phrase (no typos, no prefix): Meilisearch does not stem, so its rows hold the word as typed

- Meilisearch, hybrid (RRF): hybrid search with semanticRatio 0.5, which merges the keyword and vector results by ranking score, not RRF

- Meilisearch, hybrid (weighted): hybrid search with semanticRatio 0.8 (the keyword and vector ranking scores, merged; not min-max normalised)

- Meilisearch, phrase: Meilisearch's ranking rules (words, typo, proximity, attribute, exactness), not BM25; no stemming; typos and a prefix last word on, as by default

- Meilisearch, prefix: the words before, then the prefix as the query's last word (which Meilisearch matches as a prefix); its ranking rules, words dropped from the end while too few match

- Meilisearch, typo: its typo tolerance (1 typo from 5 letters, 2 from 9), ranked by its rules

- Typesense, boosted (bm25 × decay): unavailable: no function scores: decay functions only sort (before or after the text match), they do not multiply it

- Typesense, sql aggregate: unavailable: no grouped aggregates: facets count values, and facet stats cover the whole result, not each group

- Typesense, bm25: Typesense's text match (words matched, then their proximity), not BM25; no typos or prefixes; words dropped from the end while too few match

- Typesense, bm25 + filter: Typesense's text match (words matched, then their proximity), not BM25; no typos or prefixes; words dropped from the end while too few match

- Typesense, hybrid (RRF): rank fusion with alpha 0.5: (1 - alpha) / (1 + keyword rank) + alpha / (1 + vector rank), over the vector top 100

- Typesense, hybrid (weighted): rank fusion with alpha 0.8 (reciprocal ranks, not normalised scores), over the vector top 100

- Typesense, phrase: Typesense's text match (words matched, then their proximity), not BM25; no typos or prefixes; words dropped from the end while too few match

- Typesense, prefix: the words before, then the prefix as the query's last word (prefix true); ranked by its text match

- Typesense, typo: its typo correction (num_typos 2), ranked by its text match

- Manticore, hybrid (weighted): unavailable: its hybrid search fuses by RRF only (fusion_method='rrf')

- Manticore, bm25: BM25 as the ranker expression bm25a(1.2, 0.75) with plain IDF (weight() is an integer: 10,000 x the score); Manticore's English stopwords

- Manticore, bm25 + filter: BM25 as the ranker expression bm25a(1.2, 0.75) with plain IDF (weight() is an integer: 10,000 x the score); Manticore's English stopwords

- Manticore, boosted (bm25 × decay): weight() x the decay, where weight() is bm25a(1.2, 0.75), which Manticore scales into 0 to 1 (not BM25's raw sum), so the decay weighs more than on Strata

- Manticore, hybrid (RRF): MATCH and knn fused by RRF (fusion_method='rrf', rank_constant 60, window 100)

- Manticore, phrase: BM25 as the ranker expression bm25a(1.2, 0.75) with plain IDF (weight() is an integer: 10,000 x the score); Manticore's English stopwords

- Manticore, prefix: BM25 as the ranker expression bm25a(1.2, 0.75) with plain IDF (weight() is an integer: 10,000 x the score); Manticore's English stopwords

- Manticore, typo: every word must match, each fuzzy (OPTION fuzzy=1, distance 2, preserve 1: fuzzy search takes no OR; it needs min_infix_len, set to 2)

- Qdrant, prefix: unavailable: prefixes need the prefix tokenizer, an index of its own that would replace the word index the facet and phrase kinds use on the field

- Qdrant, sql aggregate: unavailable: no grouped aggregates: its facet API counts values only

- Qdrant, typo: unavailable: no fuzzy matching: its text index matches whole words (or prefixes, with a prefix tokenizer)

- Qdrant, bm25: BM25 from the client: sparse vectors of Strata's analysis weighted tf (k1 + 1) / (tf + k1 (1 - b + b dl / avgdl)), Qdrant applying the IDF

- Qdrant, bm25 + filter: BM25 from the client: sparse vectors of Strata's analysis weighted tf (k1 + 1) / (tf + k1 (1 - b + b dl / avgdl)), Qdrant applying the IDF

- Qdrant, boosted (bm25 × decay): the BM25 top 100 re-ranked by a formula query: score x exp_decay(year, target 2024, scale 5, midpoint 0.5)

- Qdrant, hybrid (RRF): prefetch of the BM25 and vector top 100, fused by rrf (k 60)

- Qdrant, hybrid (weighted): prefetch of the BM25 and vector top 100, fused by dbsf (each normalised by its mean ± 3 standard deviations, summed unweighted: no weights)

- Qdrant, phrase: BM25 over the whole query, filtered by the text index's phrase match

- Weaviate, boosted (bm25 × decay): unavailable: no function scores

- Weaviate, phrase: unavailable: no phrase queries

- Weaviate, typo: unavailable: no fuzzy matching

- Weaviate, bm25: Weaviate's BM25 (k1 1.2, b 0.75) without stemming (word tokenization); prefix: a Like filter on the body's words

- Weaviate, bm25 + filter: Weaviate's BM25 (k1 1.2, b 0.75) without stemming (word tokenization); prefix: a Like filter on the body's words

- Weaviate, facet: a grouped Aggregate over the rows whose body contains the word (no stemming); the 5 largest groups picked by the client

- Weaviate, hybrid (RRF): hybrid search with rankedFusion (k 60) and alpha 0.5

- Weaviate, hybrid (weighted): hybrid search with relativeScoreFusion (min-max normalised) and alpha 0.8

- Weaviate, prefix: Weaviate's BM25 (k1 1.2, b 0.75) without stemming (word tokenization); prefix: a Like filter on the body's words

- Weaviate, sql aggregate: a grouped Aggregate (GraphQL, no SQL); HAVING and ORDER BY in the client

- Milvus, facet: unavailable: no aggregations: a query counts the rows of a filter, not of each value

- Milvus, prefix: unavailable: no word prefixes: TEXT_MATCH matches whole terms and LIKE a prefix of the whole string

- Milvus, sql aggregate: unavailable: no GROUP BY

- Milvus, typo: unavailable: no fuzzy matching

- Milvus, bm25: Milvus's BM25 function with its english analyzer (its stopwords, no query stopwords); phrase: PHRASE_MATCH as the filter

- Milvus, bm25 + filter: Milvus's BM25 function with its english analyzer (its stopwords, no query stopwords); phrase: PHRASE_MATCH as the filter

- Milvus, boosted (bm25 × decay): the BM25 top 100 re-ranked by the decay ranker (exp, origin 2024, scale 5, decay 0.5); the client keeps the top 10

- Milvus, hybrid (RRF): hybrid_search of the BM25 and vector top 100, rrf ranker (k 60)

- Milvus, hybrid (weighted): hybrid_search of the BM25 and vector top 100, weighted ranker 0.2 and 0.8 with norm_score (an arctan normalisation, not min-max)

- Milvus, phrase: Milvus's BM25 function with its english analyzer (its stopwords, no query stopwords); phrase: PHRASE_MATCH as the filter

- ParadeDB, hybrid (RRF): SQL over the BM25 and vector top 100 (ParadeDB's own pattern)

- ParadeDB, hybrid (weighted): SQL over the BM25 and vector top 100 (ParadeDB's own pattern)

- ParadeDB, prefix: a regex on the terms (pre.*), ranked with the words before

- ParadeDB, typo: each word fuzzy with Strata's 'auto' edits, unstemmed (as Solr's), the others matched as words

- Postgres, typo: unavailable: no fuzzy matching in full-text search (pg_trgm compares whole strings, not the words of a tsvector)

- Postgres, bm25: ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Postgres, bm25 + filter: ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Postgres, boosted (bm25 × decay): ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Postgres, hybrid (RRF): ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Postgres, hybrid (weighted): ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Postgres, phrase: ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Postgres, prefix: ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Redis, bm25: BM25STD over Redis's English stemming; typo: %word% (1 edit) or %%word%% (2) by Strata's 'auto'; boosted: FT.AGGREGATE with ADDSCORES and APPLY

- Redis, bm25 + filter: BM25STD over Redis's English stemming; typo: %word% (1 edit) or %%word%% (2) by Strata's 'auto'; boosted: FT.AGGREGATE with ADDSCORES and APPLY

- Redis, boosted (bm25 × decay): BM25STD over Redis's English stemming; typo: %word% (1 edit) or %%word%% (2) by Strata's 'auto'; boosted: FT.AGGREGATE with ADDSCORES and APPLY

- Redis, hybrid (RRF): FT.HYBRID, COMBINE RRF (window 100, constant 60)

- Redis, hybrid (weighted): FT.HYBRID, COMBINE LINEAR: ALPHA 0.2 x the raw BM25STD score + BETA 0.8 x the vector score (Redis 8.8 does not normalise them), over each top 100

- Redis, key lookup: HMGET of the document's hash (its own key-value read)

- Redis, phrase: BM25STD over Redis's English stemming; typo: %word% (1 edit) or %%word%% (2) by Strata's 'auto'; boosted: FT.AGGREGATE with ADDSCORES and APPLY

- Redis, prefix: BM25STD over Redis's English stemming; typo: %word% (1 edit) or %%word%% (2) by Strata's 'auto'; boosted: FT.AGGREGATE with ADDSCORES and APPLY

- Redis, typo: BM25STD over Redis's English stemming; typo: %word% (1 edit) or %%word%% (2) by Strata's 'auto'; boosted: FT.AGGREGATE with ADDSCORES and APPLY

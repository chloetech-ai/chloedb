## Load time and index size

| engine | load (s) | of which: send / build | searchable | merge to 1 segment | index size (GB) | documents |
|---|---|---|---|---|---|---|
| Strata | 339 | read 14 + build 325 | (built) | (built as one) | 4.91 | 1,000,000 |
| Elasticsearch | 665 | send 448 | 5 | 211 | 4.89 | 1,000,000 |
| OpenSearch | 730 | send 495 | 4 | 231 | 5.22 | 1,000,000 |
| Solr | 1218 | send 527 | 1 | 689 | 9.16 | 1,000,000 |
| Vespa | 1332 | send 1332 | on write | (on its own) | 9.07 (+ 0.04 transaction log) | 1,000,000 |
| Meilisearch (one node) | 1761 | send 137 | – | index 1624 | 20.73 | 1,000,000 |
| Typesense (one node) | 1336 | send 1336 | – | none | 18.72 | 1,000,000 |
| Manticore (one node) | 1515 | send 524 | – | flush 2, merge 988 | 5.03 | 1,000,000 |
| Qdrant (one node) | 4518 | send 389 | – | index 4128 | 8.70 | 1,000,000 |
| Weaviate (one node) | 526 | send 526 | – | none | 7.48 | 1,000,000 |
| Milvus (one node) | 508 | send 235 | – | flush 2, index 272, load 0 | 24.45 | 1,000,000 |
| ParadeDB (one node) | 217 | send 126 | – | analyze 1, primary key 3, search index 88 | 21.14 | 1,000,000 |
| Postgres (one node) | 1237 | send 152 | – | analyze 1, gin index 54, hnsw index 1026, primary key 4 | 16.38 | 1,000,000 |
| Redis (one node) | 404 | send 404 | – | index 0 | 8.17 (in memory) | 1,000,000 |

## 1 client: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | Elasticsearch, trial licence q/s | Elasticsearch, trial licence ms | OpenSearch q/s | OpenSearch ms | Solr q/s | Solr ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 328 | 2.90 / 3.78 / 5.68 | 150 | 6.20 / 9.04 / 14.36 | n/a | n/a | 212 | 4.35 / 6.45 / 9.87 | 126 | 7.30 / 11.08 / 16.67 |
| bm25 + filter | 256 | 3.70 / 5.14 / 7.04 | 211 | 4.29 / 6.80 / 11.27 | n/a | n/a | 240 | 3.78 / 6.04 / 10.21 | 124 | 7.28 / 11.76 / 18.57 |
| phrase | 340 | 2.83 / 3.70 / 5.05 | 357 | 2.71 / 4.07 / 6.87 | n/a | n/a | 366 | 2.59 / 3.94 / 5.65 | 378 | 2.42 / 3.57 / 5.93 |
| typo | 8 | 119.59 / 191.47 / 261.35 | 66 | 14.44 / 23.96 / 35.52 | n/a | n/a | 46 | 19.40 / 35.47 / 50.56 | 50 | 19.15 / 30.55 / 42.00 |
| prefix | 235 | 4.04 / 5.56 / 8.48 | 252 | 3.67 / 5.26 / 8.95 | n/a | n/a | 225 | 4.06 / 6.32 / 10.46 | 380 | 2.49 / 3.48 / 5.07 |
| vector | 435 | 2.23 / 2.69 / 3.15 | 212 | 4.45 / 5.61 / 9.37 | n/a | n/a | 213 | 4.52 / 5.84 / 8.36 | 57 | 16.45 / 21.15 / 26.76 |
| vector + filter | 385 | 2.54 / 2.99 / 3.43 | 223 | 4.29 / 5.04 / 7.56 | n/a | n/a | 144 | 6.78 / 8.21 / 10.51 | 23 | 40.79 / 53.12 / 56.65 |
| hybrid (RRF) | 169 | 5.77 / 7.08 / 10.13 | n/a | n/a | 48 | 20.23 / 24.29 / 28.53 | 126 | 7.53 / 10.25 / 15.46 | 129 | 7.15 / 10.97 / 15.08 |
| hybrid (weighted) | 163 | 5.93 / 7.36 / 10.28 | 139 | 6.27 / 11.18 / 18.48 | 48 | 20.31 / 24.94 / 33.69 | 132 | 7.15 / 9.71 / 15.92 | 105 | 8.40 / 15.83 / 25.85 |
| boosted (bm25 × decay) | 161 | 5.62 / 9.01 / 15.37 | 113 | 6.94 / 18.63 / 29.53 | n/a | n/a | 109 | 7.01 / 19.51 / 32.59 | 112 | 7.06 / 19.44 / 31.36 |
| similar items | 360 | 2.75 / 3.15 / 3.78 | 135 | 7.06 / 8.99 / 11.42 | n/a | n/a | 124 | 7.84 / 9.51 / 12.20 | 113 | 8.19 / 10.15 / 14.35 |
| facet | 495 | 1.98 / 2.48 / 3.04 | 388 | 2.32 / 3.55 / 6.22 | n/a | n/a | 395 | 2.25 / 3.56 / 7.14 | 282 | 2.98 / 5.49 / 11.78 |
| key lookup | 727 | 1.30 / 1.72 / 2.10 | 509 | 1.91 / 2.46 / 3.31 | n/a | n/a | 515 | 1.83 / 2.61 / 3.47 | 389 | 2.53 / 3.03 / 4.33 |
| sql aggregate | 6 | 182.35 / 230.57 / 253.87 | 107 | 9.08 / 11.38 / 14.92 | n/a | n/a | 164 | 5.78 / 7.41 / 10.67 | 14 | 72.02 / 87.59 / 100.12 |

| kind | Vespa q/s | Vespa ms | Meilisearch q/s | Meilisearch ms | Typesense q/s | Typesense ms | Manticore q/s | Manticore ms | Qdrant q/s | Qdrant ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 134 | 4.26 / 10.30 / 59.32 | 134 | 6.93 / 12.25 / 18.13 | 130 | 4.78 / 16.21 / 54.18 | 148 | 4.92 / 15.90 / 28.86 | 564 | 1.70 / 2.84 / 3.77 |
| bm25 + filter | 141 | 4.63 / 10.09 / 44.16 | 101 | 9.32 / 14.33 / 20.17 | 19 | 51.35 / 57.65 / 93.32 | 187 | 5.52 / 9.11 / 15.17 | 517 | 1.83 / 2.98 / 4.26 |
| phrase | 367 | 2.52 / 3.83 / 6.23 | 247 | 2.67 / 9.33 / 15.20 | 427 | 0.60 / 4.93 / 26.02 | 548 | 1.40 / 3.33 / 6.94 | 419 | 1.04 / 4.80 / 21.82 |
| typo | n/a | n/a | 130 | 7.04 / 12.38 / 17.90 | 55 | 6.54 / 57.73 / 146.29 | 21 | 43.72 / 77.80 / 114.47 | n/a | n/a |
| prefix | n/a | n/a | 148 | 6.12 / 10.84 / 15.05 | 78 | 8.45 / 24.71 / 73.25 | 56 | 11.54 / 36.61 / 132.78 | n/a | n/a |
| vector | 197 | 3.64 / 4.35 / 44.44 | 180 | 5.35 / 7.13 / 8.85 | 205 | 4.83 / 5.21 / 5.84 | 877 | 1.14 / 1.35 / 1.55 | 517 | 1.87 / 2.32 / 2.78 |
| vector + filter | 162 | 5.86 / 6.76 / 18.55 | 75 | 13.19 / 16.37 / 19.45 | 36 | 27.85 / 29.27 / 32.07 | 141 | 7.00 / 8.35 / 10.26 | 250 | 3.88 / 4.96 / 6.06 |
| hybrid (RRF) | 70 | 8.10 / 40.00 / 57.98 | 95 | 10.25 / 15.70 / 22.10 | 97 | 6.89 / 18.62 / 54.84 | 45 | 15.06 / 58.84 / 111.45 | 97 | 9.55 / 13.85 / 16.37 |
| hybrid (weighted) | 95 | 7.86 / 13.10 / 51.41 | 82 | 11.71 / 17.68 / 22.85 | 97 | 6.84 / 18.63 / 56.00 | n/a | n/a | 94 | 9.84 / 14.14 / 16.92 |
| boosted (bm25 × decay) | 216 | 3.62 / 5.96 / 23.00 | n/a | n/a | n/a | n/a | 137 | 5.54 / 17.18 / 33.25 | 478 | 2.02 / 3.22 / 4.15 |
| similar items | 195 | 5.08 / 5.79 / 6.72 | 99 | 9.96 / 12.55 / 15.15 | 10 | 105.01 / 108.05 / 111.19 | 366 | 2.61 / 3.97 / 4.57 | 105 | 9.66 / 11.65 / 14.01 |
| facet | 114 | 3.70 / 17.85 / 75.18 | 258 | 3.83 / 6.42 / 9.97 | 93 | 4.05 / 24.44 / 100.65 | 498 | 1.40 / 3.86 / 7.20 | 52 | 13.61 / 41.82 / 58.31 |
| key lookup | 354 | 1.88 / 2.32 / 43.51 | 2,810 | 0.35 / 0.40 / 0.47 | 1,278 | 0.77 / 0.87 / 1.07 | 1,888 | 0.52 / 0.56 / 0.64 | 2,581 | 0.38 / 0.43 / 0.55 |
| sql aggregate | 6 | 172.25 / 242.00 / 254.62 | n/a | n/a | n/a | n/a | 37 | 27.51 / 36.34 / 45.12 | n/a | n/a |

| kind | Weaviate q/s | Weaviate ms | Milvus q/s | Milvus ms | ParadeDB q/s | ParadeDB ms | Postgres q/s | Postgres ms | Redis q/s | Redis ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 254 | 3.28 / 6.12 / 14.06 | 449 | 2.11 / 2.76 / 4.06 | 80 | 14.87 / 16.58 / 18.61 | 4 | 237.26 / 825.00 / 873.39 | 13 | 67.15 / 137.55 / 171.77 |
| bm25 + filter | 219 | 4.04 / 6.64 / 13.18 | 354 | 2.74 / 3.26 / 3.86 | 67 | 16.76 / 18.59 / 20.79 | 6 | 103.45 / 704.34 / 733.36 | 12 | 77.83 / 123.87 / 147.93 |
| phrase | n/a | n/a | 509 | 1.93 / 2.36 / 3.01 | 223 | 3.98 / 6.29 / 10.41 | 100 | 4.07 / 22.17 / 88.87 | 775 | 0.83 / 2.65 / 6.20 |
| typo | n/a | n/a | n/a | n/a | 15 | 64.52 / 95.78 / 114.49 | n/a | n/a | 5 | 222.28 / 315.64 / 372.87 |
| prefix | 166 | 5.31 / 10.03 / 16.88 | n/a | n/a | 57 | 16.90 / 20.58 / 25.16 | 3 | 264.51 / 573.85 / 910.03 | 83 | 7.36 / 25.23 / 65.37 |
| vector | 178 | 5.42 / 6.18 / 9.75 | 377 | 2.58 / 2.97 / 3.66 | 63 | 15.65 / 18.13 / 20.53 | 318 | 3.03 / 4.06 / 5.76 | 790 | 1.24 / 1.51 / 1.89 |
| vector + filter | 128 | 7.68 / 8.83 / 14.02 | 305 | 3.18 / 3.66 / 4.47 | 37 | 26.78 / 28.89 / 30.65 | 289 | 3.16 / 4.85 / 7.99 | 422 | 2.21 / 3.31 / 4.71 |
| hybrid (RRF) | 114 | 7.82 / 12.00 / 22.78 | 180 | 5.47 / 6.27 / 7.49 | 29 | 35.99 / 39.10 / 40.87 | 4 | 216.36 / 820.12 / 887.72 | 12 | 72.46 / 138.29 / 177.91 |
| hybrid (weighted) | 113 | 7.86 / 12.06 / 21.92 | 181 | 5.42 / 6.26 / 7.66 | 29 | 36.45 / 40.73 / 42.74 | 4 | 216.40 / 826.62 / 887.49 | 12 | 73.48 / 144.59 / 175.34 |
| boosted (bm25 × decay) | n/a | n/a | 332 | 2.86 / 3.64 / 5.47 | 1 | 625.27 / 1302.00 / 1305.09 | 4 | 243.29 / 423.03 / 1162.59 | 1 | 847.74 / 1059.51 / 1816.73 |
| similar items | 119 | 8.18 / 8.96 / 16.22 | 313 | 3.10 / 3.52 / 4.16 | 7 | 134.79 / 163.17 / 185.97 | 361 | 2.65 / 3.51 / 5.10 | 422 | 2.17 / 3.38 / 5.17 |
| facet | 19 | 14.68 / 99.14 / 690.75 | n/a | n/a | 184 | 4.86 / 7.01 / 14.85 | 48 | 5.05 / 53.58 / 131.84 | 17 | 11.34 / 140.50 / 542.57 |
| key lookup | 1,050 | 0.93 / 1.07 / 1.39 | 1,014 | 0.97 / 1.06 / 1.37 | 2,501 | 0.39 / 0.43 / 0.51 | 4,499 | 0.22 / 0.25 / 0.32 | 4,458 | 0.22 / 0.25 / 0.30 |
| sql aggregate | 0 | 3582.32 / 3582.32 / 3582.32 | n/a | n/a | 27 | 36.97 / 50.52 / 63.47 | 5 | 198.27 / 214.29 / 258.13 | 0 | 2236.10 / 2329.05 / 2329.05 |


## 4 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | Elasticsearch, trial licence q/s | Elasticsearch, trial licence ms | OpenSearch q/s | OpenSearch ms | Solr q/s | Solr ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 866 | 4.45 / 5.91 / 8.19 | 571 | 6.67 / 9.70 / 13.72 | n/a | n/a | 677 | 5.60 / 8.33 / 12.18 | 268 | 13.19 / 25.31 / 41.28 |
| bm25 + filter | 714 | 5.30 / 7.67 / 10.31 | 653 | 5.50 / 9.43 / 15.48 | n/a | n/a | 696 | 5.06 / 8.86 / 15.40 | 306 | 10.72 / 22.34 / 41.94 |
| phrase | 900 | 4.27 / 5.73 / 7.74 | 1,118 | 3.44 / 5.52 / 8.15 | n/a | n/a | 1,223 | 3.05 / 4.89 / 7.61 | 1,165 | 3.09 / 5.10 / 8.46 |
| typo | 16 | 237.06 / 419.83 / 562.37 | 164 | 23.26 / 37.18 / 53.85 | n/a | n/a | 111 | 33.57 / 57.64 / 81.87 | 123 | 30.45 / 49.71 / 81.92 |
| prefix | 625 | 6.07 / 8.77 / 12.65 | 689 | 5.38 / 8.44 / 13.24 | n/a | n/a | 657 | 5.59 / 9.19 / 14.87 | 1,111 | 3.36 / 5.15 / 7.62 |
| vector | 1,025 | 3.86 / 4.81 / 5.95 | 647 | 5.84 / 7.86 / 11.09 | n/a | n/a | 663 | 5.83 / 7.97 / 10.30 | 128 | 29.90 / 44.36 / 55.45 |
| vector + filter | 1,084 | 3.61 / 4.60 / 5.85 | 634 | 6.05 / 7.82 / 10.33 | n/a | n/a | 385 | 9.97 / 13.91 / 18.26 | 93 | 20.35 / 93.60 / 122.44 |
| hybrid (RRF) | 352 | 11.11 / 14.92 / 18.94 | n/a | n/a | 87 | 44.41 / 59.96 / 81.66 | 345 | 10.97 / 15.81 / 24.31 | 272 | 13.69 / 22.11 / 34.06 |
| hybrid (weighted) | 365 | 10.74 / 14.28 / 17.68 | 366 | 9.61 / 17.08 / 30.79 | 82 | 46.40 / 61.97 / 85.72 | 364 | 10.31 / 15.08 / 23.44 | 247 | 13.96 / 27.00 / 46.04 |
| boosted (bm25 × decay) | 417 | 8.74 / 14.00 / 24.31 | 278 | 11.73 / 28.77 / 49.92 | n/a | n/a | 267 | 11.61 / 31.36 / 54.82 | 267 | 11.90 / 31.56 / 53.51 |
| similar items | 911 | 4.23 / 5.65 / 6.91 | 363 | 10.69 / 14.30 / 18.74 | n/a | n/a | 320 | 12.09 / 16.24 / 21.23 | 327 | 11.15 / 15.38 / 35.29 |
| facet | 1,193 | 3.25 / 4.42 / 5.60 | 1,346 | 2.69 / 4.43 / 7.07 | n/a | n/a | 1,369 | 2.51 / 4.53 / 9.53 | 876 | 3.74 / 7.61 / 19.42 |
| key lookup | 2,219 | 1.75 / 2.30 / 2.92 | 1,870 | 2.06 / 2.67 / 3.61 | n/a | n/a | 2,230 | 1.73 / 2.30 / 3.16 | 1,201 | 3.19 / 4.30 / 6.05 |
| sql aggregate | 11 | 405.50 / 582.17 / 616.25 | 356 | 10.64 / 14.92 / 22.89 | n/a | n/a | 570 | 6.75 / 9.07 / 11.99 | 37 | 107.75 / 139.55 / 175.42 |

| kind | Vespa q/s | Vespa ms | Meilisearch q/s | Meilisearch ms | Typesense q/s | Typesense ms | Manticore q/s | Manticore ms | Qdrant q/s | Qdrant ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 265 | 4.66 / 68.61 / 84.02 | 458 | 8.02 / 14.74 / 21.80 | 424 | 6.04 / 19.96 / 64.70 | 182 | 12.24 / 51.14 / 148.07 | 1,673 | 2.31 / 3.67 / 4.94 |
| bm25 + filter | 351 | 5.14 / 37.84 / 69.24 | 326 | 11.49 / 18.44 / 25.47 | 62 | 61.09 / 80.53 / 129.57 | 261 | 10.97 / 32.67 / 80.88 | 1,517 | 2.51 / 3.93 / 5.69 |
| phrase | 767 | 2.64 / 4.63 / 54.59 | 838 | 3.08 / 11.30 / 19.09 | 1,408 | 0.60 / 6.62 / 33.18 | 981 | 2.46 / 9.13 / 21.50 | 1,233 | 1.41 / 6.42 / 31.04 |
| typo | n/a | n/a | 422 | 8.72 / 15.56 / 22.97 | 177 | 8.56 / 70.13 / 163.33 | 65 | 55.13 / 104.57 / 159.61 | n/a | n/a |
| prefix | n/a | n/a | 466 | 7.92 / 14.07 / 20.15 | 251 | 10.56 / 31.14 / 94.77 | 69 | 27.62 / 122.93 / 463.82 | n/a | n/a |
| vector | 351 | 4.23 / 55.79 / 73.18 | 567 | 6.86 / 9.12 / 11.84 | 657 | 5.82 / 7.78 / 10.13 | 2,984 | 1.29 / 1.72 / 2.31 | 1,320 | 2.92 / 3.80 / 4.86 |
| vector + filter | 473 | 7.15 / 9.82 / 53.36 | 258 | 15.52 / 18.72 / 22.02 | 120 | 32.05 / 41.70 / 46.36 | 466 | 8.30 / 10.90 / 13.28 | 717 | 5.47 / 6.77 / 8.70 |
| hybrid (RRF) | 217 | 9.46 / 56.97 / 87.57 | 319 | 11.98 / 19.41 / 26.42 | 322 | 8.76 / 21.93 / 71.94 | 137 | 18.88 / 69.08 / 146.13 | 286 | 13.33 / 18.74 / 23.16 |
| hybrid (weighted) | 278 | 9.39 / 27.46 / 82.75 | 277 | 13.83 / 21.03 / 28.57 | 325 | 8.70 / 21.86 / 69.53 | n/a | n/a | 281 | 13.51 / 19.29 / 23.46 |
| boosted (bm25 × decay) | 763 | 3.73 / 8.99 / 26.50 | n/a | n/a | n/a | n/a | 164 | 13.41 / 60.76 / 166.88 | 1,441 | 2.71 / 4.15 / 5.44 |
| similar items | 462 | 6.26 / 9.45 / 43.06 | 338 | 11.64 / 15.08 / 18.39 | 33 | 116.68 / 146.70 / 157.54 | 1,291 | 2.98 / 4.58 / 5.87 | 324 | 12.54 / 15.34 / 17.99 |
| facet | 354 | 3.75 / 38.98 / 75.67 | 851 | 4.56 / 7.78 / 11.83 | 262 | 6.29 / 35.64 / 136.79 | 1,056 | 2.03 / 8.73 / 25.97 | 62 | 64.07 / 115.20 / 174.01 |
| key lookup | 850 | 1.88 / 2.39 / 68.80 | 9,272 | 0.42 / 0.51 / 0.65 | 4,274 | 0.89 / 1.20 / 1.60 | 6,889 | 0.56 / 0.67 / 0.90 | 8,287 | 0.47 / 0.58 / 0.75 |
| sql aggregate | 37 | 106.56 / 169.86 / 187.10 | n/a | n/a | n/a | n/a | 43 | 68.74 / 196.13 / 252.36 | n/a | n/a |

| kind | Weaviate q/s | Weaviate ms | Milvus q/s | Milvus ms | ParadeDB q/s | ParadeDB ms | Postgres q/s | Postgres ms | Redis q/s | Redis ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 828 | 3.81 / 8.23 / 18.96 | 1,206 | 3.14 / 4.44 / 6.33 | 173 | 26.70 / 35.72 / 44.25 | 4 | 603.35 / 1952.49 / 3774.81 | 43 | 82.88 / 158.29 / 245.86 |
| bm25 + filter | 717 | 4.76 / 8.74 / 17.04 | 783 | 4.95 / 6.40 / 8.49 | 144 | 31.06 / 41.76 / 47.85 | 7 | 271.14 / 1754.56 / 3678.23 | 40 | 98.36 / 158.74 / 199.41 |
| phrase | n/a | n/a | 1,355 | 2.84 / 3.83 / 5.19 | 900 | 3.79 / 6.47 / 14.20 | 380 | 4.35 / 23.46 / 85.77 | 2,629 | 1.01 / 3.08 / 7.87 |
| typo | n/a | n/a | n/a | n/a | 18 | 195.73 / 317.68 / 593.73 | n/a | n/a | 17 | 215.63 / 388.71 / 572.80 |
| prefix | 533 | 6.40 / 12.70 / 23.07 | n/a | n/a | 103 | 39.06 / 48.36 / 59.91 | 7 | 416.62 / 947.56 / 1913.18 | 291 | 7.98 / 27.23 / 95.94 |
| vector | 552 | 6.82 / 8.79 / 13.12 | 970 | 3.95 / 5.12 / 7.07 | 128 | 31.26 / 39.60 / 48.90 | 1,231 | 3.09 / 4.32 / 6.40 | 2,628 | 1.48 / 1.90 / 2.51 |
| vector + filter | 404 | 9.46 / 12.20 / 17.90 | 705 | 5.46 / 7.18 / 9.69 | 71 | 54.79 / 69.33 / 77.87 | 1,142 | 3.20 / 4.93 / 8.09 | 1,533 | 2.47 / 3.64 / 4.99 |
| hybrid (RRF) | 303 | 11.40 / 19.62 / 36.25 | 341 | 11.27 / 15.10 / 21.36 | 57 | 71.12 / 88.33 / 105.61 | 5 | 630.59 / 1935.65 / 2508.03 | 41 | 89.99 / 180.47 / 256.17 |
| hybrid (weighted) | 310 | 11.33 / 19.30 / 33.27 | 341 | 11.35 / 15.21 / 20.26 | 55 | 74.10 / 91.88 / 109.25 | 5 | 568.21 / 1850.88 / 5649.94 | 42 | 86.95 / 172.98 / 241.35 |
| boosted (bm25 × decay) | n/a | n/a | 822 | 4.62 / 6.52 / 9.10 | 3 | 1120.55 / 2154.51 / 2740.15 | 3 | 842.25 / 2228.34 / 5680.91 | 2 | 2163.50 / 3670.40 / 4499.98 |
| similar items | 358 | 10.62 / 13.61 / 20.71 | 730 | 5.26 / 6.88 / 9.24 | 9 | 429.96 / 556.59 / 640.21 | 1,387 | 2.77 / 3.69 / 5.33 | 1,506 | 2.46 / 3.77 / 5.53 |
| facet | 37 | 27.42 / 216.79 / 1377.65 | n/a | n/a | 762 | 4.49 / 7.53 / 14.66 | 123 | 6.48 / 71.32 / 280.01 | 25 | 92.94 / 471.06 / 788.03 |
| key lookup | 3,681 | 1.06 / 1.29 / 1.72 | 3,397 | 1.13 / 1.39 / 1.84 | 8,754 | 0.45 / 0.52 / 0.60 | 14,892 | 0.27 / 0.33 / 0.39 | 16,482 | 0.23 / 0.27 / 0.34 |
| sql aggregate | 0 | 8597.75 / 8920.63 / 8920.63 | n/a | n/a | 91 | 37.94 / 78.67 / 109.82 | 6 | 598.33 / 872.34 / 2465.83 | 1 | 7127.20 / 7424.58 / 7424.58 |


## 16 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | Elasticsearch, trial licence q/s | Elasticsearch, trial licence ms | OpenSearch q/s | OpenSearch ms | Solr q/s | Solr ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 1,245 | 12.60 / 15.83 / 19.86 | 1,089 | 13.57 / 22.80 / 32.33 | n/a | n/a | 1,389 | 10.59 / 17.74 / 25.89 | 529 | 26.00 / 51.71 / 142.89 |
| bm25 + filter | 987 | 15.83 / 20.17 / 25.43 | 1,181 | 12.09 / 22.06 / 35.10 | n/a | n/a | 1,215 | 11.43 / 22.33 / 37.30 | 539 | 25.07 / 53.23 / 126.23 |
| phrase | 1,207 | 13.19 / 16.14 / 19.01 | 2,066 | 7.13 / 12.43 / 19.54 | n/a | n/a | 2,134 | 6.83 / 12.38 / 19.80 | 1,864 | 7.54 / 14.02 / 25.63 |
| typo | 15 | 997.35 / 1731.89 / 2103.76 | 185 | 82.84 / 122.79 / 168.56 | n/a | n/a | 121 | 124.94 / 185.20 / 245.60 | 131 | 107.35 / 207.63 / 352.70 |
| prefix | 953 | 16.24 / 21.99 / 28.34 | 1,109 | 13.05 / 22.78 / 33.88 | n/a | n/a | 961 | 15.26 / 25.72 / 39.01 | 1,749 | 8.24 / 14.64 / 24.48 |
| vector | 1,132 | 14.19 / 17.10 / 20.28 | 1,213 | 12.47 / 18.27 / 26.12 | n/a | n/a | 1,102 | 13.62 / 21.47 / 28.98 | 164 | 95.09 / 137.34 / 210.82 |
| vector + filter | 1,198 | 13.25 / 16.46 / 20.34 | 1,014 | 14.47 / 21.51 / 44.07 | n/a | n/a | 517 | 30.04 / 42.01 / 49.91 | 342 | 43.65 / 69.35 / 103.98 |
| hybrid (RRF) | 402 | 39.02 / 49.74 / 61.31 | n/a | n/a | 91 | 174.47 / 219.86 / 265.21 | 424 | 35.98 / 52.82 / 72.37 | 391 | 36.96 / 65.89 / 105.16 |
| hybrid (weighted) | 425 | 37.20 / 47.74 / 56.73 | 471 | 30.61 / 52.69 / 90.95 | 77 | 203.79 / 265.56 / 310.11 | 462 | 33.56 / 46.80 / 65.89 | 271 | 48.16 / 107.16 / 205.72 |
| boosted (bm25 × decay) | 553 | 26.85 / 41.67 / 66.65 | 325 | 42.82 / 81.83 / 139.17 | n/a | n/a | 297 | 46.28 / 90.98 / 147.37 | 302 | 36.67 / 121.62 / 214.66 |
| similar items | 926 | 17.10 / 21.26 / 25.76 | 565 | 27.07 / 39.58 / 50.79 | n/a | n/a | 395 | 40.33 / 51.72 / 62.16 | 428 | 34.11 / 52.06 / 83.11 |
| facet | 1,409 | 11.19 / 14.26 / 17.21 | 2,694 | 5.11 / 9.76 / 17.90 | n/a | n/a | 2,517 | 5.12 / 11.58 / 22.98 | 1,243 | 10.04 / 24.19 / 56.66 |
| key lookup | 2,807 | 5.69 / 7.06 / 8.48 | 4,655 | 3.30 / 4.72 / 6.64 | n/a | n/a | 5,107 | 2.97 / 4.32 / 6.35 | 2,097 | 7.23 / 11.25 / 16.13 |
| sql aggregate | 10 | 1548.34 / 2434.37 / 2763.68 | 825 | 18.54 / 27.17 / 36.54 | n/a | n/a | 950 | 16.25 / 27.69 / 35.14 | 47 | 330.99 / 487.33 / 616.51 |

| kind | Vespa q/s | Vespa ms | Meilisearch q/s | Meilisearch ms | Typesense q/s | Typesense ms | Manticore q/s | Manticore ms | Qdrant q/s | Qdrant ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 393 | 11.84 / 87.97 / 97.49 | 708 | 20.20 / 39.92 / 58.12 | 638 | 15.98 / 52.42 / 165.57 | 178 | 40.38 / 346.66 / 737.14 | 2,275 | 6.60 / 9.97 / 16.93 |
| bm25 + filter | 443 | 15.93 / 81.82 / 93.02 | 549 | 26.64 / 47.85 / 69.24 | 86 | 187.88 / 233.81 / 358.68 | 257 | 37.94 / 71.77 / 1296.06 | 2,038 | 7.33 / 11.31 / 18.35 |
| phrase | 820 | 5.71 / 77.72 / 82.17 | 1,415 | 8.30 / 24.94 / 44.03 | 2,520 | 2.72 / 13.88 / 59.77 | 971 | 12.71 / 31.70 / 62.50 | 1,566 | 8.16 / 18.38 / 42.78 |
| typo | n/a | n/a | 707 | 20.59 / 39.11 / 59.43 | 253 | 28.34 / 171.11 / 448.84 | 88 | 156.31 / 274.26 / 624.95 | n/a | n/a |
| prefix | n/a | n/a | 775 | 18.67 / 35.27 / 54.05 | 364 | 30.89 / 84.62 / 257.55 | 68 | 79.25 / 543.51 / 2361.57 | n/a | n/a |
| vector | 407 | 10.27 / 88.99 / 93.31 | 1,016 | 15.09 / 23.52 / 32.53 | 999 | 15.23 / 24.84 / 33.59 | 5,934 | 2.34 / 2.90 / 13.93 | 1,694 | 8.82 / 13.31 / 20.75 |
| vector + filter | 516 | 23.01 / 56.77 / 67.22 | 432 | 36.31 / 53.71 / 70.80 | 166 | 101.33 / 116.78 / 140.22 | 689 | 20.78 / 33.89 / 39.33 | 840 | 18.13 / 26.13 / 31.34 |
| hybrid (RRF) | 324 | 41.26 / 80.91 / 151.11 | 499 | 29.60 / 53.20 / 77.88 | 475 | 24.12 / 59.06 / 180.70 | 172 | 76.95 / 162.94 / 335.29 | 329 | 47.33 / 62.10 / 76.77 |
| hybrid (weighted) | 298 | 52.83 / 84.85 / 170.23 | 456 | 32.99 / 55.10 / 77.63 | 485 | 23.39 / 57.34 / 180.53 | n/a | n/a | 327 | 47.38 / 63.64 / 78.81 |
| boosted (bm25 × decay) | 772 | 10.43 / 59.13 / 80.00 | n/a | n/a | n/a | n/a | 161 | 43.75 / 365.41 / 749.11 | 1,972 | 7.64 / 11.85 / 18.38 |
| similar items | 473 | 18.48 / 70.12 / 84.48 | 581 | 26.53 / 40.53 / 54.10 | 45 | 367.86 / 415.65 / 517.86 | 2,232 | 6.27 / 8.83 / 22.07 | 343 | 45.16 / 60.56 / 68.23 |
| facet | 458 | 16.91 / 82.76 / 118.78 | 1,476 | 9.90 / 18.93 / 28.51 | 354 | 24.75 / 96.59 / 331.90 | 1,192 | 8.00 / 25.49 / 54.39 | 62 | 258.91 / 350.06 / 432.11 |
| key lookup | 1,001 | 3.15 / 83.41 / 85.68 | 15,927 | 0.98 / 1.26 / 1.75 | 9,943 | 1.51 / 2.21 / 2.88 | 21,048 | 0.70 / 0.91 / 1.86 | 19,607 | 0.77 / 1.10 / 1.63 |
| sql aggregate | 38 | 401.97 / 621.52 / 707.64 | n/a | n/a | n/a | n/a | 47 | 341.55 / 577.02 / 612.08 | n/a | n/a |

| kind | Weaviate q/s | Weaviate ms | Milvus q/s | Milvus ms | ParadeDB q/s | ParadeDB ms | Postgres q/s | Postgres ms | Redis q/s | Redis ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 973 | 13.16 / 31.51 / 61.53 | 1,756 | 8.70 / 12.62 / 18.42 | 531 | 22.58 / 64.92 / 89.96 | 5 | 1366.87 / 8498.14 / 9990.65 | 63 | 238.20 / 340.84 / 428.31 |
| bm25 + filter | 1,065 | 12.67 / 27.30 / 49.14 | 1,184 | 12.95 / 18.10 / 24.48 | 336 | 42.25 / 83.03 / 107.61 | 7 | 947.14 / 6982.70 / 8674.36 | 55 | 285.22 / 362.69 / 418.37 |
| phrase | n/a | n/a | 1,711 | 9.05 / 12.49 / 17.36 | 1,067 | 12.65 / 26.88 / 53.91 | 390 | 15.24 / 93.07 / 341.61 | 4,979 | 2.65 / 4.88 / 10.31 |
| typo | n/a | n/a | n/a | n/a | 19 | 743.70 / 1379.93 / 2140.64 | n/a | n/a | 24 | 614.85 / 832.41 / 1030.67 |
| prefix | 745 | 18.44 / 38.66 / 71.74 | n/a | n/a | 204 | 76.05 / 120.54 / 175.51 | 6 | 1827.81 / 4283.29 / 6809.42 | 457 | 29.01 / 54.28 / 124.98 |
| vector | 714 | 18.31 / 41.82 / 65.71 | 1,391 | 10.85 / 15.84 / 23.40 | 246 | 63.12 / 89.46 / 114.27 | 1,617 | 7.96 / 19.87 / 33.35 | 3,997 | 3.72 / 5.68 / 6.73 |
| vector + filter | 555 | 24.47 / 51.09 / 79.92 | 915 | 16.54 / 23.26 / 33.11 | 107 | 144.39 / 185.60 / 253.93 | 1,407 | 8.89 / 22.96 / 40.24 | 3,176 | 4.65 / 6.53 / 13.66 |
| hybrid (RRF) | 351 | 39.61 / 74.01 / 128.35 | 384 | 41.21 / 50.59 / 62.84 | 96 | 158.64 / 214.27 / 280.65 | 5 | 1492.10 / 6615.63 / 8651.00 | 33 | 476.03 / 591.65 / 711.59 |
| hybrid (weighted) | 345 | 39.82 / 77.63 / 149.47 | 385 | 41.24 / 50.59 / 60.26 | 98 | 158.03 / 205.23 / 257.91 | 5 | 1475.97 / 4964.32 / 7171.77 | 33 | 474.18 / 592.59 / 731.82 |
| boosted (bm25 × decay) | n/a | n/a | 1,118 | 13.62 / 19.73 / 27.59 | 3 | 4431.19 / 6951.45 / 8405.56 | 3 | 1881.80 / 8199.22 / 12707.61 | 2 | 7872.97 / 8657.74 / 11944.22 |
| similar items | 483 | 27.84 / 59.94 / 95.18 | 863 | 17.79 / 22.92 / 31.83 | 10 | 1388.31 / 2754.19 / 2941.50 | 1,786 | 7.49 / 16.98 / 28.65 | 2,790 | 5.54 / 7.78 / 9.06 |
| facet | 40 | 209.58 / 920.35 / 2147.40 | n/a | n/a | 973 | 13.61 / 29.06 / 61.96 | 135 | 25.62 / 318.78 / 755.72 | 26 | 545.79 / 1145.96 / 1350.90 |
| key lookup | 8,183 | 1.59 / 3.10 / 6.89 | 5,987 | 2.52 / 3.62 / 5.77 | 24,377 | 0.55 / 0.88 / 1.57 | 33,028 | 0.40 / 0.64 / 1.47 | 47,130 | 0.33 / 0.43 / 0.53 |
| sql aggregate | 1 | 27221.85 / 28144.11 / 28221.44 | n/a | n/a | 93 | 164.10 / 270.24 / 384.18 | 7 | 2220.12 / 4310.75 / 4896.10 | 1 | 15618.18 / 15618.37 / 15618.42 |


Errors during the timed runs: Strata 0, Elasticsearch 0, Elasticsearch, trial licence 0, OpenSearch 0, Solr 0, Vespa 0, Meilisearch 0, Typesense 0, Manticore 0, Qdrant 0, Weaviate 0, Milvus 0, ParadeDB 0, Postgres 0, Redis 7

## Answers: recall@10 against an exact search, nDCG@10 against the collection's judgments

Unavailable kinds are n/a; recall is for the kinds an exact search can answer.

| kind | Strata recall | Strata nDCG | Elasticsearch recall | Elasticsearch nDCG | Elasticsearch, trial licence recall | Elasticsearch, trial licence nDCG | OpenSearch recall | OpenSearch nDCG | Solr recall | Solr nDCG |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | – | – | – | – | n/a | n/a | – | – | – | – |
| bm25 + filter | – | – | – | – | n/a | n/a | – | – | – | – |
| phrase | – | – | – | – | n/a | n/a | – | – | – | – |
| typo | – | – | – | – | n/a | n/a | – | – | – | – |
| prefix | – | – | – | – | n/a | n/a | – | – | – | – |
| vector | 0.801 | – | 0.718 | – | n/a | n/a | 0.592 | – | 0.845 | – |
| vector + filter | 0.828 | – | 0.680 | – | n/a | n/a | 0.735 | – | 0.947 | – |
| hybrid (RRF) | – | – | n/a | n/a | – | – | – | – | – | – |
| hybrid (weighted) | – | – | – | – | – | – | – | – | – | – |
| boosted (bm25 × decay) | – | – | – | – | n/a | n/a | – | – | – | – |
| similar items | 0.992 | – | 0.938 | – | n/a | n/a | 0.920 | – | 0.998 | – |

| kind | Vespa recall | Vespa nDCG | Meilisearch recall | Meilisearch nDCG | Typesense recall | Typesense nDCG | Manticore recall | Manticore nDCG | Qdrant recall | Qdrant nDCG |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | – | – | – | – | – | – | – | – | – | – |
| bm25 + filter | – | – | – | – | – | – | – | – | – | – |
| phrase | – | – | – | – | – | – | – | – | – | – |
| typo | n/a | n/a | – | – | – | – | – | – | n/a | n/a |
| prefix | n/a | n/a | – | – | – | – | – | – | n/a | n/a |
| vector | 0.777 | – | 0.789 | – | 0.801 | – | 0.707 | – | 0.901 | – |
| vector + filter | 0.861 | – | 0.897 | – | 0.905 | – | 0.693 | – | 0.833 | – |
| hybrid (RRF) | – | – | – | – | – | – | – | – | – | – |
| hybrid (weighted) | – | – | – | – | – | – | n/a | n/a | – | – |
| boosted (bm25 × decay) | – | – | n/a | n/a | n/a | n/a | – | – | – | – |
| similar items | 0.988 | – | 0.983 | – | 0.990 | – | 0.896 | – | 0.973 | – |

| kind | Weaviate recall | Weaviate nDCG | Milvus recall | Milvus nDCG | ParadeDB recall | ParadeDB nDCG | Postgres recall | Postgres nDCG | Redis recall | Redis nDCG |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | – | – | – | – | – | – | – | – | – | – |
| bm25 + filter | – | – | – | – | – | – | – | – | – | – |
| phrase | n/a | n/a | – | – | – | – | – | – | – | – |
| typo | n/a | n/a | n/a | n/a | – | – | n/a | n/a | – | – |
| prefix | – | – | n/a | n/a | – | – | – | – | – | – |
| vector | 0.750 | – | 0.857 | – | 0.718 | – | 0.789 | – | 0.775 | – |
| vector + filter | 0.723 | – | 0.907 | – | 0.854 | – | 0.751 | – | 0.736 | – |
| hybrid (RRF) | – | – | – | – | – | – | – | – | – | – |
| hybrid (weighted) | – | – | – | – | – | – | – | – | – | – |
| boosted (bm25 × decay) | n/a | n/a | – | – | – | – | – | – | – | – |
| similar items | 0.979 | – | 0.986 | – | 0.969 | – | 0.978 | – | 0.974 | – |


## SQL aggregates equal to the true answer

| Strata | Elasticsearch | Elasticsearch, trial licence | OpenSearch | Solr | Vespa | Meilisearch | Typesense | Manticore | Qdrant | Weaviate | Milvus | ParadeDB | Postgres | Redis |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 300 of 300 | 300 of 300 | n/a | 300 of 300 | 300 of 300 | 300 of 300 | n/a | n/a | 300 of 300 | n/a | 300 of 300 | n/a | 300 of 300 | 300 of 300 | 300 of 300 |

## Agreement with Strata's answers

| kind | Elasticsearch | Elasticsearch, trial licence | OpenSearch | Solr | Vespa | Meilisearch | Typesense |
|---|---|---|---|---|---|---|---|
| bm25 | 0.958 of the top 10 | n/a | 0.957 of the top 10 | 0.955 of the top 10 | 0.933 of the top 10 | 0.127 of the top 10 | 0.215 of the top 10 |
| bm25 + filter | 0.953 of the top 10 | n/a | 0.949 of the top 10 | 0.952 of the top 10 | 0.933 of the top 10 | 0.127 of the top 10 | 0.213 of the top 10 |
| phrase | 0.545 of the top 10 | n/a | 0.544 of the top 10 | 0.545 of the top 10 | 0.649 of the top 10 | 0.217 of the top 10 | 0.057 of the top 10 |
| typo | 0.681 of the top 10 | n/a | 0.686 of the top 10 | 0.367 of the top 10 | n/a | 0.065 of the top 10 | 0.109 of the top 10 |
| prefix | 0.966 of the top 10 | n/a | 0.965 of the top 10 | 0.964 of the top 10 | n/a | 0.132 of the top 10 | 0.171 of the top 10 |
| vector | 0.737 of the top 10 | n/a | 0.596 of the top 10 | 0.784 of the top 10 | 0.776 of the top 10 | 0.776 of the top 10 | 0.778 of the top 10 |
| vector + filter | 0.719 of the top 10 | n/a | 0.731 of the top 10 | 0.842 of the top 10 | 0.841 of the top 10 | 0.845 of the top 10 | 0.857 of the top 10 |
| hybrid (RRF) | n/a | 0.824 of the top 10 | 0.702 of the top 10 | 0.597 of the top 10 | 0.722 of the top 10 | 0.315 of the top 10 | 0.372 of the top 10 |
| hybrid (weighted) | 0.459 of the top 10 | 0.733 of the top 10 | 0.658 of the top 10 | 0.443 of the top 10 | 0.733 of the top 10 | 0.645 of the top 10 | 0.599 of the top 10 |
| boosted (bm25 × decay) | 0.970 of the top 10 | n/a | 0.972 of the top 10 | 0.972 of the top 10 | 0.854 of the top 10 | n/a | n/a |
| similar items | 0.939 of the top 10 | n/a | 0.920 of the top 10 | 0.991 of the top 10 | 0.987 of the top 10 | 0.977 of the top 10 | 0.987 of the top 10 |
| facet | 286 of 300 equal | n/a | 286 of 300 equal | 284 of 300 equal | 131 of 300 equal | 51 of 300 equal | 46 of 300 equal |
| key lookup | 300 of 300 equal | n/a | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal |

| kind | Manticore | Qdrant | Weaviate | Milvus | ParadeDB | Postgres | Redis |
|---|---|---|---|---|---|---|---|
| bm25 | 0.557 of the top 10 | 0.943 of the top 10 | 0.530 of the top 10 | 0.745 of the top 10 | 0.750 of the top 10 | 0.460 of the top 10 | 0.397 of the top 10 |
| bm25 + filter | 0.592 of the top 10 | 0.932 of the top 10 | 0.532 of the top 10 | 0.749 of the top 10 | 0.757 of the top 10 | 0.465 of the top 10 | 0.416 of the top 10 |
| phrase | 0.458 of the top 10 | 0.640 of the top 10 | n/a | 0.536 of the top 10 | 0.533 of the top 10 | 0.468 of the top 10 | 0.373 of the top 10 |
| typo | 0.055 of the top 10 | n/a | n/a | n/a | 0.081 of the top 10 | n/a | 0.063 of the top 10 |
| prefix | 0.422 of the top 10 | n/a | 0.564 of the top 10 | n/a | 0.617 of the top 10 | 0.523 of the top 10 | 0.140 of the top 10 |
| vector | 0.703 of the top 10 | 0.809 of the top 10 | 0.748 of the top 10 | 0.799 of the top 10 | 0.687 of the top 10 | 0.777 of the top 10 | 0.783 of the top 10 |
| vector + filter | 0.724 of the top 10 | 0.811 of the top 10 | 0.749 of the top 10 | 0.830 of the top 10 | 0.781 of the top 10 | 0.786 of the top 10 | 0.774 of the top 10 |
| hybrid (RRF) | 0.655 of the top 10 | 0.845 of the top 10 | 0.609 of the top 10 | 0.750 of the top 10 | 0.647 of the top 10 | 0.578 of the top 10 | 0.558 of the top 10 |
| hybrid (weighted) | n/a | 0.711 of the top 10 | 0.706 of the top 10 | 0.749 of the top 10 | 0.643 of the top 10 | 0.682 of the top 10 | 0.298 of the top 10 |
| boosted (bm25 × decay) | 0.546 of the top 10 | 0.647 of the top 10 | n/a | 0.580 of the top 10 | 0.742 of the top 10 | 0.492 of the top 10 | 0.431 of the top 10 |
| similar items | 0.898 of the top 10 | 0.970 of the top 10 | 0.980 of the top 10 | 0.983 of the top 10 | 0.964 of the top 10 | 0.980 of the top 10 | 0.977 of the top 10 |
| facet | 279 of 300 equal | 274 of 300 equal | 56 of 300 equal | n/a | 274 of 300 equal | 83 of 300 equal | 28 of 300 equal |
| key lookup | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal |


## Writes: documents a second, and latency in ms (p50 / p99)

| phase | clients | Strata docs/s | Strata ms | Elasticsearch docs/s | Elasticsearch ms | OpenSearch docs/s | OpenSearch ms | Solr docs/s | Solr ms | Vespa docs/s | Vespa ms |
|---|---|---|---|---|---|---|---|---|---|---|---|
| insert one | 1 | 427 | 2.07 / 3.18 | 250 | 3.62 / 9.75 | 354 | 2.52 / 6.46 | 532 | 1.73 / 3.97 | 166 | 5.71 / 8.80 |
| insert one | 4 | 1,513 | 2.23 / 3.82 | 724 | 4.71 / 16.92 | 1,077 | 3.17 / 8.50 | 1,578 | 2.10 / 6.17 | 549 | 6.60 / 13.57 |
| insert one | 16 | 1,749 | 7.37 / 65.47 | 2,096 | 6.44 / 28.05 | 2,052 | 6.23 / 33.00 | 2,516 | 5.23 / 21.49 | 744 | 11.67 / 69.01 |
| insert batch | 1 | 2,041 | 31.81 / 143.22 | 1,614 | 42.19 / 282.76 | 1,335 | 62.33 / 91.08 | 1,479 | 38.55 / 162.38 | 814 | 103.91 / 198.28 |
| insert batch | 4 | 3,371 | 90.64 / 374.38 | 3,348 | 73.58 / 566.02 | 3,100 | 108.34 / 245.96 | 3,363 | 79.11 / 396.59 | 752 | 354.29 / 1345.10 |
| insert batch | 16 | 4,163 | 363.63 / 709.19 | 3,800 | 364.08 / 1378.68 | 2,954 | 503.24 / 715.82 | 3,621 | 385.24 / 896.12 | 653 | 1168.12 / 6895.76 |
| update one | 1 | 615 | 1.38 / 4.39 | 199 | 3.87 / 11.47 | 331 | 2.52 / 7.63 | 482 | 1.85 / 4.59 | 219 | 3.35 / 40.66 |
| update one | 4 | 2,673 | 1.42 / 2.43 | 725 | 4.50 / 24.03 | 1,079 | 3.15 / 8.15 | 1,348 | 2.49 / 8.01 | 674 | 3.63 / 57.20 |
| update one | 16 | 4,680 | 3.36 / 5.31 | 2,089 | 6.40 / 28.59 | 1,960 | 6.66 / 30.41 | 2,207 | 5.74 / 23.66 | 1,317 | 4.47 / 76.90 |

## How soon a written document is found (ms from the write's start)

| engine | searchable | write returned (p50) | found (p50) | found (p90) | found (max) |
|---|---|---|---|---|---|
| Strata | searchable when the write returns | 2.2 | 5.7 | 6.3 | 67.3 |
| Elasticsearch | refresh_interval 1s (the default; loading turned it off) | 6.1 | 715.3 | 1022.8 | 1033.5 |
| OpenSearch | refresh_interval 1s (the default; loading turned it off) | 5.1 | 771.5 | 1012.0 | 1027.1 |
| Solr | autoSoftCommit.maxTime 1000 ms (as Elasticsearch's 1 s refresh; loading turned it off) | 3.0 | 1013.5 | 1020.6 | 1511.5 |
| Vespa | searchable when the write returns | 6.8 | 9.4 | 12.2 | 39.4 |

## Streams through Redpanda: documents written a second, and lag in ms from when a document was due (p50 / p99)

| phase | Strata docs/s | Strata lag | Elasticsearch docs/s | Elasticsearch lag | OpenSearch docs/s | OpenSearch lag | Solr docs/s | Solr lag | Vespa docs/s | Vespa lag |
|---|---|---|---|---|---|---|---|---|---|---|
| 250/s | 249 | 79 / 109 | 250 | 24 / 42 | 250 | 22 / 39 | 250 | 21 / 36 | 250 | 28 / 776 |
| 500/s | 498 | 78 / 123 | 500 | 25 / 49 | 499 | 21 / 46 | 500 | 19 / 34 | 500 | 31 / 729 |
| 1000/s | 997 | 79 / 151 | 999 | 29 / 80 | 999 | 24 / 62 | 999 | 21 / 40 | 833 | 3929 / 7601 (fell behind) |
| 2000/s | 1,991 | 93 / 637 | 1,997 | 34 / 170 | 1,997 | 34 / 266 | 1,996 | 33 / 308 | not run | not run |
| flat out | 3,513 | 3207 / 4917 | 3,509 | 2740 / 4669 | 3,182 | 2808 / 5643 | 3,242 | 3237 / 5402 | 861 | 12196 / 22492 |

Lag includes reading the topic (Redpanda's part, the same for every engine, is the 'read' column below).

| engine | what written means | read lag p50 (ms) | crash: events written again | documents lost |
|---|---|---|---|---|
| Strata | searchable (the server answers a batch once its rows are visible) | 35 | 0 (the consumer resumes after the offsets the server keeps with the rows (GET /v1/sources)) | 0 |
| Elasticsearch | written: searchable within the 1 s refresh | 19 | 1413 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |
| OpenSearch | written: searchable within the 1 s refresh | 18 | 104 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |
| Solr | written: searchable within the 1 s soft commit | 19 | 107 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |
| Vespa | searchable (a put is visible when it returns) | 21 | 211 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |

## CPU used by each engine's containers (cgroup cpu.stat)

| engine | phase | pod | CPU seconds | wall seconds | average cores | throttled (s) |
|---|---|---|---|---|---|---|
| Strata | timed run | strata-0 | 365 | 361 | 1.01 | 6 |
| Strata | timed run | strata-1 | 409 | 362 | 1.13 | 12 |
| Strata | timed run | strata-coordinator | 167 | 361 | 0.46 | 0 |
| Strata | writes | strata-0 | 43 | 69 | 0.63 | 1 |
| Strata | writes | strata-1 | 54 | 69 | 0.78 | 1 |
| Strata | writes | strata-coordinator | 34 | 70 | 0.49 | 0 |
| Strata | stream | strata-0 | 73 | 150 | 0.49 | 2 |
| Strata | stream | strata-1 | 77 | 150 | 0.51 | 3 |
| Strata | stream | strata-coordinator | 35 | 151 | 0.23 | 0 |
| Elasticsearch | load | es-0 | 1233 | 679 | 1.82 | 11 |
| Elasticsearch | load | es-1 | 1716 | 679 | 2.53 | 160 |
| Elasticsearch | timed run | es-0 | 532 | 289 | 1.84 | 1 |
| Elasticsearch | timed run | es-1 | 572 | 289 | 1.98 | 18 |
| Elasticsearch | writes | es-0 | 120 | 103 | 1.17 | 0 |
| Elasticsearch | writes | es-1 | 124 | 103 | 1.20 | 2 |
| Elasticsearch | stream | es-0 | 140 | 152 | 0.92 | 0 |
| Elasticsearch | stream | es-1 | 146 | 152 | 0.96 | 1 |
| Elasticsearch, trial licence | timed run | es-0 | 75 | 63 | 1.19 | 0 |
| Elasticsearch, trial licence | timed run | es-1 | 135 | 64 | 2.11 | 4 |
| OpenSearch | load | os-0 | 1204 | 743 | 1.62 | 33 |
| OpenSearch | load | os-1 | 1939 | 743 | 2.61 | 139 |
| OpenSearch | timed run | os-0 | 515 | 303 | 1.70 | 1 |
| OpenSearch | timed run | os-1 | 625 | 304 | 2.06 | 17 |
| OpenSearch | writes | os-0 | 112 | 106 | 1.05 | 1 |
| OpenSearch | writes | os-1 | 134 | 105 | 1.28 | 3 |
| OpenSearch | stream | os-0 | 136 | 152 | 0.89 | 1 |
| OpenSearch | stream | os-1 | 168 | 153 | 1.10 | 2 |
| Solr | load | solr-0 | 2558 | 1236 | 2.07 | 105 |
| Solr | load | solr-1 | 2628 | 1236 | 2.13 | 198 |
| Solr | timed run | solr-0 | 506 | 351 | 1.44 | 1 |
| Solr | timed run | solr-1 | 806 | 351 | 2.30 | 22 |
| Solr | writes | solr-0 | 156 | 114 | 1.37 | 1 |
| Solr | writes | solr-1 | 157 | 114 | 1.38 | 2 |
| Solr | stream | solr-0 | 154 | 152 | 1.01 | 1 |
| Solr | stream | solr-1 | 172 | 153 | 1.13 | 1 |
| Vespa | load | vespa-0 | 1337 | 1347 | 0.99 | 6159 |
| Vespa | load | vespa-1 | 1873 | 1347 | 1.39 | 20 |
| Vespa | load | vespa-2 | 1848 | 1347 | 1.37 | 16 |
| Vespa | timed run | vespa-0 | 232 | 306 | 0.76 | 399 |
| Vespa | timed run | vespa-1 | 330 | 306 | 1.08 | 5 |
| Vespa | timed run | vespa-2 | 339 | 306 | 1.11 | 4 |
| Vespa | writes | vespa-0 | 75 | 91 | 0.83 | 174 |
| Vespa | writes | vespa-1 | 65 | 91 | 0.71 | 0 |
| Vespa | writes | vespa-2 | 64 | 90 | 0.72 | 0 |
| Vespa | stream | vespa-0 | 154 | 202 | 0.76 | 319 |
| Vespa | stream | vespa-1 | 181 | 201 | 0.90 | 0 |
| Vespa | stream | vespa-2 | 180 | 201 | 0.90 | 0 |
| Meilisearch | load | meili-0 | 7652 | 1774 | 4.31 | 250 |
| Meilisearch | timed run | meili-0 | 803 | 256 | 3.14 | 6 |
| Typesense | load | typesense-0 | 4073 | 1349 | 3.02 | 0 |
| Typesense | timed run | typesense-0 | 890 | 309 | 2.88 | 52 |
| Manticore | load | manticore-0 | 5855 | 1527 | 3.83 | 236 |
| Manticore | timed run | manticore-0 | 1334 | 291 | 4.58 | 18 |
| Qdrant | load | qdrant-0 | 24983 | 4552 | 5.49 | 132 |
| Qdrant | timed run | qdrant-0 | 951 | 235 | 4.05 | 24 |
| Weaviate | load | weaviate-0 | 3420 | 539 | 6.35 | 17 |
| Weaviate | timed run | weaviate-0 | 4313 | 880 | 4.90 | 9 |
| Milvus | load | milvus-0 | 2752 | 520 | 5.29 | 166 |
| Milvus | timed run | milvus-0 | 880 | 202 | 4.35 | 12 |
| ParadeDB | load | paradedb-0 | 486 | 230 | 2.11 | 0 |
| ParadeDB | timed run | paradedb-0 | 2227 | 751 | 2.97 | 66 |
| Postgres | load | postgres-0 | 3632 | 1251 | 2.90 | 0 |
| Postgres | timed run | postgres-0 | 4153 | 895 | 4.64 | 136 |
| Redis | load | redis-0 | 1669 | 417 | 4.00 | 0 |
| Redis | timed run | redis-0 | 1675 | 1163 | 1.44 | 11 |

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

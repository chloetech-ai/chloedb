## Load time and index size

| engine | load (s) | of which: send / build | searchable | merge to 1 segment | index size (GB) | documents |
|---|---|---|---|---|---|---|
| Strata | 369 | load 555 + merge 33 | (built) | (built as one) | 5.00 | 1,000,000 |
| Elasticsearch | 588 | send 408 | 4 | 175 | 4.88 | 1,000,000 |
| OpenSearch | 851 | send 494 | 1 | 355 | 5.13 | 1,000,000 |
| Solr | 1271 | send 503 | 4 | 764 | 4.10 | 1,000,000 |
| Vespa | 1395 | send 1395 | on write | (on its own) | 9.07 (+ 0.03 transaction log) | 1,000,000 |
| Meilisearch (one node) | 1770 | send 118 | – | index 1653 | 20.62 | 1,000,000 |
| Typesense (one node) | 1515 | send 1515 | – | none | 18.72 | 1,000,000 |
| Manticore (one node) | 2162 | send 663 | – | flush 5, merge 1495 | 5.03 | 1,000,000 |
| Qdrant (one node) | 4482 | send 385 | – | index 4097 | 8.69 | 1,000,000 |
| Weaviate (one node) | 575 | send 575 | – | none | 7.41 | 1,000,000 |
| Milvus (one node) | 558 | send 317 | – | flush 2, index 239, load 0 | 24.03 | 1,000,000 |
| ParadeDB (one node) | 221 | send 114 | – | analyze 1, primary key 4, search index 102 | 20.95 | 1,000,000 |
| Postgres (one node) | 1418 | send 113 | – | analyze 1, gin index 63, hnsw index 1236, primary key 4 | 16.36 | 1,000,000 |
| Redis (one node) | 405 | send 405 | – | index 0 | 8.17 (in memory) | 1,000,000 |

## 1 client: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | Elasticsearch, trial licence q/s | Elasticsearch, trial licence ms | OpenSearch q/s | OpenSearch ms | Solr q/s | Solr ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 443 | 2.03 / 3.01 / 5.78 | 281 | 3.44 / 4.34 / 6.24 | n/a | n/a | 331 | 2.85 / 3.83 / 5.79 | 325 | 2.90 / 4.18 / 6.55 |
| bm25 + filter | 398 | 2.26 / 3.39 / 6.57 | 285 | 3.29 / 4.50 / 8.63 | n/a | n/a | 277 | 3.37 / 4.74 / 8.12 | 410 | 2.28 / 3.15 / 5.15 |
| phrase | 576 | 1.71 / 1.94 / 2.60 | 373 | 2.69 / 3.78 / 6.03 | n/a | n/a | 383 | 2.58 / 3.71 / 5.39 | 492 | 1.89 / 2.64 / 3.72 |
| typo | 61 | 16.52 / 24.27 / 29.59 | 65 | 14.55 / 24.27 / 32.66 | n/a | n/a | 46 | 20.39 / 35.57 / 54.52 | 53 | 17.72 / 28.04 / 44.13 |
| prefix | 420 | 2.15 / 3.31 / 5.06 | 258 | 3.61 / 5.01 / 8.92 | n/a | n/a | 232 | 3.87 / 5.84 / 10.03 | 453 | 2.10 / 2.74 / 3.99 |
| vector | 452 | 2.17 / 2.39 / 3.20 | 242 | 4.04 / 4.68 / 5.71 | n/a | n/a | 209 | 4.65 / 5.75 / 7.07 | 242 | 4.00 / 4.64 / 6.70 |
| vector + filter | 432 | 2.26 / 2.52 / 3.32 | 212 | 4.53 / 5.34 / 6.57 | n/a | n/a | 113 | 8.71 / 10.34 / 12.15 | 135 | 6.72 / 7.90 / 30.35 |
| hybrid (RRF) | 202 | 4.63 / 6.40 / 10.40 | n/a | n/a | 103 | 9.49 / 11.27 / 14.33 | 118 | 8.08 / 10.56 / 15.51 | 140 | 6.57 / 10.28 / 15.75 |
| hybrid (weighted) | 199 | 4.71 / 6.41 / 10.60 | 140 | 6.10 / 10.64 / 21.51 | 82 | 11.08 / 18.45 / 23.42 | 121 | 7.85 / 10.25 / 15.85 | 111 | 7.74 / 15.37 / 23.63 |
| boosted (bm25 × decay) | 236 | 3.67 / 6.94 / 10.97 | 113 | 7.06 / 17.89 / 28.51 | n/a | n/a | 116 | 6.88 / 17.62 / 28.49 | 121 | 6.35 / 18.37 / 30.87 |
| similar items | 350 | 2.80 / 3.12 / 4.11 | 147 | 6.52 / 7.50 / 10.25 | n/a | n/a | 114 | 8.78 / 10.13 / 12.56 | 146 | 6.36 / 7.08 / 9.54 |
| facet | 544 | 1.82 / 2.12 / 2.71 | 471 | 1.99 / 2.64 / 4.37 | n/a | n/a | 515 | 1.77 / 2.44 / 5.13 | 337 | 2.28 / 4.62 / 13.86 |
| key lookup | 1,081 | 1.02 / 1.22 / 1.55 | 977 | 1.05 / 1.34 / 1.68 | n/a | n/a | 1,180 | 0.89 / 1.13 / 1.42 | 821 | 1.22 / 1.44 / 1.71 |
| sql aggregate | 132 | 7.47 / 9.90 / 15.44 | 20 | 49.28 / 69.60 / 82.21 | n/a | n/a | 19 | 53.48 / 77.06 / 85.40 | 16 | 60.80 / 75.63 / 92.24 |

| kind | Vespa q/s | Vespa ms | Meilisearch q/s | Meilisearch ms | Typesense q/s | Typesense ms | Manticore q/s | Manticore ms | Qdrant q/s | Qdrant ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 130 | 3.99 / 10.43 / 58.18 | 141 | 6.89 / 11.56 / 15.77 | 123 | 5.17 / 17.46 / 56.88 | 131 | 5.64 / 17.94 / 35.09 | 511 | 1.89 / 3.02 / 3.98 |
| bm25 + filter | 145 | 4.93 / 9.95 / 42.00 | 101 | 9.49 / 14.28 / 18.72 | 17 | 55.75 / 62.34 / 105.16 | 146 | 6.94 / 11.77 / 19.19 | 463 | 2.06 / 3.23 / 4.47 |
| phrase | 377 | 2.31 / 3.62 / 6.67 | 275 | 2.37 / 8.49 / 13.26 | 399 | 0.66 / 5.30 / 27.28 | 411 | 1.86 / 4.43 / 8.86 | 412 | 1.19 / 4.02 / 21.69 |
| typo | n/a | n/a | 137 | 6.90 / 12.06 / 16.03 | 49 | 7.18 / 60.63 / 161.31 | 17 | 55.39 / 101.35 / 142.46 | n/a | n/a |
| prefix | n/a | n/a | 152 | 6.06 / 10.37 / 14.24 | 71 | 9.20 / 26.82 / 80.07 | 47 | 13.47 / 44.23 / 167.75 | n/a | n/a |
| vector | 238 | 3.56 / 4.08 / 42.09 | 186 | 5.32 / 6.17 / 7.97 | 191 | 5.21 / 5.55 / 6.26 | 641 | 1.55 / 1.89 / 2.23 | 479 | 2.00 / 2.50 / 3.07 |
| vector + filter | 159 | 6.21 / 7.11 / 8.56 | 73 | 13.76 / 16.24 / 18.06 | 33 | 30.27 / 31.64 / 33.03 | 86 | 11.14 / 15.29 / 17.77 | 260 | 3.78 / 4.70 / 5.53 |
| hybrid (RRF) | 89 | 8.26 / 14.71 / 56.54 | 93 | 10.49 / 16.00 / 20.21 | 91 | 7.56 / 19.20 / 63.34 | 41 | 16.73 / 61.36 / 132.19 | 94 | 9.84 / 14.37 / 16.90 |
| hybrid (weighted) | 89 | 8.31 / 13.79 / 56.05 | 85 | 11.51 / 16.26 / 20.22 | 90 | 7.45 / 19.64 / 60.32 | n/a | n/a | 95 | 9.74 / 14.19 / 16.97 |
| boosted (bm25 × decay) | 224 | 3.49 / 5.68 / 22.07 | n/a | n/a | n/a | n/a | 115 | 6.51 / 19.99 / 36.47 | 450 | 2.16 / 3.33 / 4.31 |
| similar items | 170 | 5.84 / 6.48 / 7.28 | 100 | 10.00 / 12.06 / 13.60 | 9 | 112.54 / 115.64 / 116.57 | 279 | 3.39 / 5.26 / 5.90 | 102 | 9.90 / 12.17 / 14.31 |
| facet | 106 | 3.97 / 22.28 / 74.92 | 287 | 3.51 / 6.09 / 10.39 | 84 | 4.44 / 26.12 / 109.76 | 384 | 1.83 / 4.64 / 8.70 | 52 | 13.70 / 41.38 / 57.19 |
| key lookup | 591 | 1.23 / 1.48 / 31.43 | 2,181 | 0.45 / 0.49 / 0.57 | 1,323 | 0.73 / 0.84 / 1.30 | 1,535 | 0.64 / 0.72 / 0.92 | 2,303 | 0.43 / 0.47 / 0.53 |
| sql aggregate | 6 | 171.89 / 236.79 / 259.97 | n/a | n/a | n/a | n/a | 29 | 34.71 / 49.56 / 62.79 | n/a | n/a |

| kind | Weaviate q/s | Weaviate ms | Milvus q/s | Milvus ms | ParadeDB q/s | ParadeDB ms | Postgres q/s | Postgres ms | Redis q/s | Redis ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 227 | 3.67 / 7.01 / 14.39 | 152 | 6.01 / 8.92 / 14.72 | 66 | 17.72 / 19.82 / 21.78 | 4 | 272.36 / 398.85 / 949.51 | 13 | 67.37 / 136.48 / 173.64 |
| bm25 + filter | 201 | 4.33 / 7.64 / 14.47 | 146 | 5.94 / 8.82 / 20.47 | 57 | 19.97 / 22.16 / 24.62 | 5 | 115.60 / 764.92 / 801.16 | 12 | 79.58 / 129.62 / 150.56 |
| phrase | n/a | n/a | 355 | 2.56 / 3.57 / 8.78 | 214 | 4.04 / 6.80 / 11.76 | 86 | 5.06 / 25.65 / 95.65 | 918 | 0.75 / 2.23 / 4.93 |
| typo | n/a | n/a | n/a | n/a | 13 | 73.50 / 110.82 / 124.66 | n/a | n/a | 5 | 216.26 / 304.00 / 343.98 |
| prefix | 148 | 5.86 / 11.49 / 19.30 | n/a | n/a | 48 | 20.07 / 24.53 / 29.00 | 3 | 271.64 / 551.25 / 886.56 | 86 | 7.30 / 23.20 / 66.73 |
| vector | 168 | 5.81 / 6.24 / 8.82 | 148 | 6.61 / 7.59 / 11.39 | 49 | 20.07 / 22.45 / 25.37 | 265 | 3.60 / 4.81 / 6.93 | 770 | 1.28 / 1.53 / 1.87 |
| vector + filter | 119 | 8.32 / 9.44 / 12.15 | 111 | 8.84 / 10.06 / 12.90 | 32 | 31.12 / 34.96 / 37.09 | 249 | 3.69 / 5.59 / 8.84 | 441 | 2.15 / 3.07 / 3.88 |
| hybrid (RRF) | 101 | 8.81 / 13.33 / 25.23 | 65 | 13.92 / 17.85 / 52.08 | 24 | 43.49 / 46.81 / 49.59 | 4 | 221.72 / 374.52 / 945.70 | 13 | 68.45 / 136.56 / 171.42 |
| hybrid (weighted) | 101 | 8.74 / 13.42 / 24.27 | 76 | 12.83 / 15.02 / 18.86 | 23 | 45.53 / 50.43 / 54.18 | 4 | 247.05 / 370.89 / 953.96 | 13 | 67.46 / 137.17 / 178.18 |
| boosted (bm25 × decay) | n/a | n/a | 154 | 6.16 / 8.17 / 12.71 | 1 | 768.68 / 1573.60 / 1599.43 | 3 | 359.93 / 486.16 / 1278.11 | 2 | 742.28 / 936.47 / 1513.66 |
| similar items | 111 | 8.82 / 9.46 / 13.16 | 255 | 3.83 / 4.40 / 6.11 | 6 | 164.38 / 186.83 / 202.13 | 303 | 3.15 / 4.15 / 5.54 | 442 | 2.18 / 2.99 / 3.93 |
| facet | 17 | 15.18 / 113.91 / 802.34 | n/a | n/a | 173 | 5.03 / 7.67 / 18.52 | 43 | 5.68 / 57.91 / 150.32 | 18 | 10.29 / 150.34 / 492.00 |
| key lookup | 2,161 | 0.46 / 0.51 / 0.62 | 735 | 1.35 / 1.53 / 2.11 | 2,206 | 0.45 / 0.48 / 0.60 | 2,745 | 0.36 / 0.40 / 0.48 | 4,190 | 0.23 / 0.26 / 0.33 |
| sql aggregate | 0 | 3879.98 / 3879.98 / 3879.98 | n/a | n/a | 21 | 45.42 / 64.87 / 79.83 | 5 | 210.40 / 231.84 / 274.91 | 0 | 2143.30 / 2238.43 / 2238.43 |


## 4 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | Elasticsearch, trial licence q/s | Elasticsearch, trial licence ms | OpenSearch q/s | OpenSearch ms | Solr q/s | Solr ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 1,413 | 2.51 / 4.19 / 7.58 | 857 | 4.42 / 6.47 / 9.66 | n/a | n/a | 1,116 | 3.43 / 4.75 / 6.64 | 1,010 | 3.51 / 6.16 / 10.59 |
| bm25 + filter | 1,231 | 2.88 / 4.77 / 9.54 | 862 | 4.32 / 6.46 / 11.62 | n/a | n/a | 880 | 4.20 / 6.40 / 10.79 | 1,063 | 3.47 / 5.49 / 8.81 |
| phrase | 2,193 | 1.77 / 2.17 / 2.97 | 1,195 | 3.23 / 5.01 / 7.46 | n/a | n/a | 1,222 | 3.15 / 4.95 / 7.25 | 1,364 | 2.68 / 4.20 / 6.57 |
| typo | 158 | 25.20 / 37.99 / 47.98 | 169 | 22.23 / 36.58 / 54.83 | n/a | n/a | 112 | 34.02 / 56.25 / 84.82 | 130 | 28.99 / 48.23 / 76.13 |
| prefix | 1,315 | 2.73 / 4.54 / 6.92 | 754 | 4.92 / 7.47 / 12.10 | n/a | n/a | 686 | 5.35 / 8.46 / 14.22 | 1,222 | 3.08 / 4.61 / 6.92 |
| vector | 1,450 | 2.65 / 3.39 / 4.62 | 717 | 5.27 / 6.79 / 9.32 | n/a | n/a | 581 | 6.55 / 9.12 / 12.23 | 547 | 6.93 / 10.10 / 13.52 |
| vector + filter | 1,414 | 2.73 / 3.50 / 4.60 | 655 | 5.88 / 7.45 / 9.40 | n/a | n/a | 321 | 12.17 / 16.28 / 20.23 | 320 | 11.29 / 15.87 / 50.47 |
| hybrid (RRF) | 562 | 6.58 / 10.04 / 15.46 | n/a | n/a | 289 | 13.56 / 16.84 / 20.59 | 320 | 11.86 / 16.84 / 24.10 | 316 | 11.62 / 19.65 / 29.25 |
| hybrid (weighted) | 578 | 6.40 / 9.66 / 15.52 | 384 | 9.16 / 16.06 / 29.56 | 226 | 16.65 / 24.97 / 35.46 | 333 | 11.33 / 16.23 / 23.73 | 265 | 13.08 / 25.91 / 41.46 |
| boosted (bm25 × decay) | 650 | 5.10 / 10.43 / 18.65 | 294 | 11.09 / 26.79 / 47.40 | n/a | n/a | 298 | 10.98 / 26.09 / 45.03 | 294 | 10.35 / 29.68 / 56.00 |
| similar items | 1,073 | 3.56 / 4.73 / 6.31 | 403 | 9.56 / 12.41 / 15.91 | n/a | n/a | 299 | 12.93 / 17.24 / 21.75 | 350 | 10.30 / 14.89 / 28.78 |
| facet | 1,972 | 1.97 / 2.51 / 3.40 | 1,699 | 2.18 / 3.23 / 5.28 | n/a | n/a | 1,787 | 2.01 / 3.11 / 6.33 | 1,004 | 3.12 / 6.80 / 18.63 |
| key lookup | 4,336 | 1.00 / 1.25 / 1.61 | 3,977 | 1.05 / 1.35 / 1.68 | n/a | n/a | 4,799 | 0.86 / 1.15 / 1.53 | 2,855 | 1.36 / 1.79 / 2.36 |
| sql aggregate | 322 | 11.96 / 17.77 / 24.31 | 47 | 84.98 / 119.97 / 150.35 | n/a | n/a | 40 | 100.21 / 146.91 / 193.64 | 40 | 98.03 / 129.33 / 165.49 |

| kind | Vespa q/s | Vespa ms | Meilisearch q/s | Meilisearch ms | Typesense q/s | Typesense ms | Manticore q/s | Manticore ms | Qdrant q/s | Qdrant ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 311 | 4.34 / 65.52 / 76.15 | 513 | 7.21 / 13.24 / 20.51 | 392 | 6.48 / 21.58 / 69.24 | 156 | 14.73 / 65.06 / 143.76 | 1,604 | 2.40 / 3.82 / 5.09 |
| bm25 + filter | 390 | 5.44 / 15.17 / 66.70 | 348 | 10.81 / 17.05 / 23.25 | 59 | 64.12 / 86.53 / 130.03 | 207 | 14.27 / 41.04 / 102.83 | 1,453 | 2.62 / 4.04 / 5.90 |
| phrase | 754 | 2.72 / 5.06 / 51.37 | 924 | 2.78 / 10.42 / 17.18 | 1,329 | 0.64 / 7.14 / 34.66 | 758 | 3.15 / 12.08 / 27.99 | 1,263 | 1.55 / 5.30 / 30.75 |
| typo | n/a | n/a | 438 | 8.41 / 15.50 / 21.80 | 161 | 9.29 / 75.66 / 191.93 | 50 | 76.00 / 133.47 / 190.83 | n/a | n/a |
| prefix | n/a | n/a | 500 | 7.41 / 12.99 / 18.97 | 233 | 11.59 / 32.05 / 102.65 | 53 | 31.53 / 148.63 / 463.09 | n/a | n/a |
| vector | 410 | 4.38 / 8.69 / 72.13 | 618 | 6.27 / 8.26 / 10.53 | 614 | 6.16 / 8.25 / 10.33 | 2,286 | 1.71 / 2.19 / 3.00 | 1,263 | 3.05 / 3.95 / 5.11 |
| vector + filter | 497 | 7.76 / 9.60 / 14.73 | 254 | 15.60 / 19.55 / 23.56 | 109 | 35.64 / 44.49 / 50.06 | 301 | 12.60 / 17.18 / 22.33 | 729 | 5.38 / 6.66 / 8.37 |
| hybrid (RRF) | 243 | 10.42 / 33.37 / 84.89 | 311 | 12.45 / 19.70 / 26.70 | 302 | 9.32 / 23.59 / 72.51 | 121 | 21.57 / 85.55 / 173.00 | 285 | 13.40 / 18.87 / 23.09 |
| hybrid (weighted) | 284 | 10.47 / 19.10 / 68.81 | 278 | 13.69 / 20.77 / 27.79 | 294 | 9.52 / 23.29 / 75.12 | n/a | n/a | 287 | 13.30 / 18.75 / 23.00 |
| boosted (bm25 × decay) | 738 | 3.87 / 8.56 / 31.27 | n/a | n/a | n/a | n/a | 142 | 15.41 / 66.57 / 174.21 | 1,369 | 2.85 / 4.25 / 5.66 |
| similar items | 461 | 7.11 / 10.62 / 31.22 | 348 | 11.31 / 14.56 / 17.36 | 31 | 128.32 / 157.01 / 177.54 | 1,054 | 3.58 / 5.66 / 6.94 | 331 | 12.23 / 14.88 / 17.16 |
| facet | 355 | 4.09 / 29.78 / 75.11 | 1,002 | 3.81 / 6.92 / 11.33 | 233 | 7.13 / 39.45 / 157.40 | 861 | 2.44 / 10.18 / 33.42 | 62 | 62.92 / 115.81 / 161.07 |
| key lookup | 1,938 | 1.21 / 1.53 / 39.55 | 7,919 | 0.50 / 0.58 / 0.69 | 4,486 | 0.84 / 1.18 / 1.66 | 6,116 | 0.63 / 0.76 / 1.02 | 8,430 | 0.46 / 0.53 / 0.63 |
| sql aggregate | 35 | 113.27 / 177.29 / 201.69 | n/a | n/a | n/a | n/a | 32 | 91.34 / 266.28 / 366.97 | n/a | n/a |

| kind | Weaviate q/s | Weaviate ms | Milvus q/s | Milvus ms | ParadeDB q/s | ParadeDB ms | Postgres q/s | Postgres ms | Redis q/s | Redis ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 748 | 4.17 / 9.14 / 21.54 | 219 | 16.36 / 30.16 / 43.51 | 142 | 32.44 / 44.98 / 52.84 | 5 | 567.98 / 2035.29 / 2695.89 | 44 | 80.11 / 164.52 / 211.83 |
| bm25 + filter | 740 | 4.62 / 8.18 / 16.59 | 288 | 13.26 / 19.21 / 25.56 | 118 | 37.48 / 50.02 / 61.76 | 7 | 333.06 / 1773.08 / 2454.04 | 39 | 96.20 / 160.00 / 216.43 |
| phrase | n/a | n/a | 894 | 4.30 / 5.88 / 8.11 | 781 | 4.33 / 7.42 / 17.94 | 326 | 5.15 / 28.33 / 94.35 | 3,144 | 0.88 / 2.53 / 6.08 |
| typo | n/a | n/a | n/a | n/a | 16 | 220.34 / 383.37 / 604.81 | n/a | n/a | 17 | 213.76 / 399.43 / 508.51 |
| prefix | 475 | 7.22 / 14.61 / 25.09 | n/a | n/a | 87 | 45.81 / 60.18 / 71.19 | 7 | 481.24 / 959.01 / 1921.45 | 311 | 7.52 / 24.70 / 80.10 |
| vector | 523 | 7.15 / 9.19 / 14.09 | 297 | 12.77 / 18.60 / 26.44 | 99 | 39.83 / 49.64 / 60.38 | 1,036 | 3.68 / 5.08 / 7.30 | 2,648 | 1.47 / 1.84 / 2.40 |
| vector + filter | 377 | 10.15 / 12.96 / 19.55 | 180 | 20.95 / 30.33 / 41.11 | 54 | 72.99 / 87.39 / 104.74 | 959 | 3.88 / 5.82 / 8.86 | 1,659 | 2.27 / 3.31 / 4.49 |
| hybrid (RRF) | 274 | 12.82 / 21.21 / 40.43 | 67 | 45.11 / 113.28 / 231.81 | 46 | 88.66 / 109.92 / 131.87 | 5 | 624.50 / 965.12 / 5010.64 | 44 | 79.43 / 158.67 / 228.33 |
| hybrid (weighted) | 275 | 12.79 / 21.68 / 34.54 | 101 | 38.05 / 50.32 / 61.71 | 43 | 92.37 / 117.38 / 134.47 | 5 | 666.06 / 2048.99 / 2574.46 | 45 | 78.52 / 157.52 / 237.69 |
| boosted (bm25 × decay) | n/a | n/a | 391 | 9.50 / 14.61 / 22.15 | 3 | 1302.95 / 1979.52 / 2620.84 | 3 | 914.96 / 2908.05 / 4442.06 | 2 | 1456.86 / 3755.77 / 4362.21 |
| similar items | 335 | 11.30 / 15.09 / 21.83 | 551 | 7.00 / 9.11 / 12.56 | 8 | 488.07 / 603.18 / 1611.06 | 1,158 | 3.30 / 4.39 / 6.38 | 1,636 | 2.32 / 3.34 / 4.60 |
| facet | 29 | 28.79 / 274.33 / 1081.47 | n/a | n/a | 678 | 4.92 / 8.75 / 18.82 | 108 | 6.48 / 77.03 / 339.85 | 25 | 109.95 / 404.81 / 791.28 |
| key lookup | 8,304 | 0.47 / 0.55 / 0.68 | 2,308 | 1.62 / 2.29 / 3.35 | 8,113 | 0.48 / 0.56 / 0.62 | 9,828 | 0.41 / 0.46 / 0.51 | 17,478 | 0.22 / 0.26 / 0.31 |
| sql aggregate | 0 | 9956.67 / 10050.59 / 10050.59 | n/a | n/a | 77 | 45.31 / 96.80 / 130.70 | 5 | 652.02 / 1171.83 / 1536.92 | 1 | 6703.08 / 6961.21 / 6961.21 |


## 16 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | Elasticsearch, trial licence q/s | Elasticsearch, trial licence ms | OpenSearch q/s | OpenSearch ms | Solr q/s | Solr ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 2,406 | 5.92 / 10.48 / 18.12 | 1,638 | 8.73 / 15.71 / 23.88 | n/a | n/a | 2,062 | 7.05 / 11.77 / 18.54 | 1,729 | 8.07 / 15.85 / 27.57 |
| bm25 + filter | 2,086 | 6.67 / 12.40 / 23.00 | 1,423 | 9.84 / 18.55 / 30.53 | n/a | n/a | 1,472 | 9.54 / 18.06 / 27.82 | 1,566 | 8.94 / 17.38 / 30.72 |
| phrase | 4,674 | 3.17 / 4.58 / 8.78 | 2,133 | 6.76 / 12.79 / 20.48 | n/a | n/a | 2,179 | 6.56 / 12.37 / 19.85 | 2,045 | 6.92 / 12.84 / 22.85 |
| typo | 178 | 85.42 / 147.74 / 196.37 | 196 | 75.67 / 117.32 / 166.25 | n/a | n/a | 125 | 122.10 / 176.04 / 241.56 | 138 | 103.71 / 204.61 / 353.63 |
| prefix | 2,307 | 6.30 / 10.86 / 16.89 | 1,179 | 11.89 / 22.32 / 34.23 | n/a | n/a | 1,033 | 13.65 / 25.28 / 39.35 | 1,839 | 7.77 / 14.15 / 23.40 |
| vector | 2,422 | 6.11 / 9.66 / 15.64 | 1,384 | 10.96 / 16.00 / 21.91 | n/a | n/a | 971 | 14.90 / 25.80 / 33.59 | 757 | 19.58 / 32.03 / 49.46 |
| vector + filter | 2,319 | 6.36 / 10.23 / 16.45 | 1,084 | 13.46 / 20.24 / 39.52 | n/a | n/a | 412 | 37.75 / 53.47 / 66.04 | 362 | 41.68 / 64.80 / 99.14 |
| hybrid (RRF) | 780 | 18.93 / 31.71 / 49.09 | n/a | n/a | 454 | 34.33 / 45.36 / 57.88 | 397 | 38.65 / 55.75 / 73.06 | 384 | 36.73 / 70.85 / 117.40 |
| hybrid (weighted) | 762 | 19.18 / 32.70 / 51.96 | 523 | 27.48 / 46.83 / 88.40 | 321 | 47.77 / 67.56 / 93.63 | 437 | 34.80 / 50.96 / 66.43 | 282 | 45.24 / 106.39 / 213.03 |
| boosted (bm25 × decay) | 893 | 14.50 / 32.56 / 62.33 | 370 | 37.77 / 72.77 / 121.28 | n/a | n/a | 352 | 40.67 / 77.45 / 131.64 | 322 | 33.24 / 115.26 / 232.21 |
| similar items | 1,676 | 8.98 / 14.08 / 20.45 | 501 | 30.27 / 45.82 / 63.32 | n/a | n/a | 393 | 40.00 / 55.40 / 66.34 | 463 | 31.77 / 48.50 / 74.68 |
| facet | 3,935 | 3.72 / 5.68 / 10.97 | 3,259 | 4.23 / 7.91 / 13.61 | n/a | n/a | 3,426 | 4.05 / 7.75 / 13.59 | 1,322 | 8.98 / 23.25 / 59.73 |
| key lookup | 13,176 | 1.23 / 1.80 / 2.66 | 10,732 | 1.45 / 2.20 / 3.16 | n/a | n/a | 14,224 | 1.08 / 1.70 / 2.53 | 5,459 | 2.68 / 4.64 / 7.23 |
| sql aggregate | 354 | 42.83 / 73.53 / 97.86 | 49 | 318.66 / 460.96 / 540.78 | n/a | n/a | 39 | 399.98 / 590.32 / 665.62 | 50 | 322.98 / 455.50 / 597.52 |

| kind | Vespa q/s | Vespa ms | Meilisearch q/s | Meilisearch ms | Typesense q/s | Typesense ms | Manticore q/s | Manticore ms | Qdrant q/s | Qdrant ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 409 | 11.29 / 87.56 / 98.27 | 701 | 20.59 / 40.08 / 59.19 | 588 | 17.59 / 55.62 / 181.34 | 154 | 44.81 / 359.02 / 703.22 | 2,281 | 6.60 / 9.93 / 16.15 |
| bm25 + filter | 422 | 18.01 / 81.80 / 93.32 | 534 | 28.07 / 48.27 / 67.88 | 81 | 198.61 / 299.64 / 394.08 | 202 | 48.96 / 91.51 / 1086.95 | 2,043 | 7.45 / 11.30 / 16.36 |
| phrase | 781 | 6.16 / 77.69 / 83.98 | 1,462 | 7.85 / 24.60 / 43.32 | 2,302 | 2.62 / 14.89 / 65.92 | 741 | 12.54 / 35.66 / 76.41 | 1,646 | 8.17 / 16.11 / 39.15 |
| typo | n/a | n/a | 717 | 20.15 / 38.54 / 56.35 | 236 | 28.06 / 154.52 / 648.65 | 69 | 202.57 / 365.77 / 612.82 | n/a | n/a |
| prefix | n/a | n/a | 796 | 18.22 / 34.98 / 51.64 | 341 | 32.21 / 91.91 / 279.06 | 57 | 90.28 / 709.60 / 2934.27 | n/a | n/a |
| vector | 451 | 11.23 / 85.85 / 93.60 | 1,026 | 14.99 / 22.91 / 31.00 | 926 | 16.53 / 25.94 / 36.13 | 4,355 | 3.19 / 4.12 / 11.58 | 1,725 | 8.78 / 12.98 / 18.65 |
| vector + filter | 496 | 26.16 / 54.35 / 65.85 | 412 | 37.44 / 56.50 / 74.53 | 153 | 110.03 / 132.07 / 152.91 | 446 | 33.22 / 46.08 / 54.05 | 867 | 17.95 / 24.07 / 29.75 |
| hybrid (RRF) | 289 | 46.78 / 83.84 / 200.19 | 482 | 30.77 / 55.13 / 79.78 | 451 | 25.91 / 60.70 / 199.34 | 147 | 88.61 / 191.58 / 411.48 | 336 | 46.12 / 60.86 / 75.50 |
| hybrid (weighted) | 298 | 46.60 / 76.63 / 173.86 | 442 | 34.09 / 56.44 / 79.57 | 445 | 26.25 / 62.92 / 198.58 | n/a | n/a | 337 | 46.00 / 60.87 / 75.69 |
| boosted (bm25 × decay) | 772 | 10.98 / 57.53 / 75.07 | n/a | n/a | n/a | n/a | 140 | 50.81 / 378.75 / 774.85 | 1,992 | 7.72 / 11.52 / 16.21 |
| similar items | 465 | 21.47 / 66.59 / 78.31 | 569 | 27.40 / 41.20 / 53.80 | 42 | 391.11 / 462.05 / 516.55 | 1,782 | 8.00 / 13.20 / 21.97 | 361 | 42.75 / 57.31 / 67.49 |
| facet | 412 | 18.41 / 86.38 / 156.85 | 1,548 | 9.37 / 18.27 / 27.42 | 322 | 26.13 / 106.31 / 379.58 | 1,034 | 9.40 / 29.53 / 61.32 | 62 | 249.19 / 347.53 / 422.92 |
| key lookup | 2,351 | 1.75 / 2.82 / 77.22 | 18,319 | 0.85 / 1.09 / 1.49 | 9,376 | 1.59 / 2.43 / 3.25 | 18,041 | 0.79 / 1.06 / 3.45 | 25,729 | 0.60 / 0.76 / 1.04 |
| sql aggregate | 35 | 448.48 / 645.98 / 776.86 | n/a | n/a | n/a | n/a | 36 | 442.34 / 679.26 / 780.73 | n/a | n/a |

| kind | Weaviate q/s | Weaviate ms | Milvus q/s | Milvus ms | ParadeDB q/s | ParadeDB ms | Postgres q/s | Postgres ms | Redis q/s | Redis ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 1,042 | 12.34 / 28.78 / 55.33 | 448 | 33.92 / 51.39 / 67.60 | 438 | 27.66 / 77.06 / 101.25 | 5 | 1637.01 / 7478.21 / 9507.03 | 62 | 245.87 / 346.43 / 436.94 |
| bm25 + filter | 994 | 13.40 / 29.02 / 50.01 | 388 | 39.22 / 55.00 / 85.26 | 274 | 50.88 / 99.18 / 130.22 | 6 | 962.13 / 7158.42 / 9862.46 | 55 | 287.85 / 368.91 / 449.62 |
| phrase | n/a | n/a | 1,246 | 12.30 / 17.48 / 24.77 | 944 | 14.71 / 27.99 / 58.24 | 351 | 16.50 / 104.66 / 367.24 | 5,509 | 2.49 / 4.58 / 8.33 |
| typo | n/a | n/a | n/a | n/a | 16 | 770.86 / 1869.03 / 2385.74 | n/a | n/a | 24 | 630.05 / 869.17 / 1123.18 |
| prefix | 641 | 21.24 / 45.96 / 81.08 | n/a | n/a | 171 | 90.77 / 142.19 / 208.58 | 6 | 2049.84 / 4237.90 / 6062.93 | 468 | 27.82 / 53.50 / 119.19 |
| vector | 703 | 18.99 / 41.01 / 68.40 | 293 | 53.83 / 67.03 / 84.55 | 182 | 86.42 / 117.62 / 151.29 | 1,405 | 9.70 / 20.57 / 32.23 | 4,277 | 3.54 / 5.14 / 6.21 |
| vector + filter | 518 | 26.10 / 55.51 / 89.25 | 187 | 85.27 / 92.93 / 109.99 | 81 | 189.91 / 248.03 / 332.74 | 1,269 | 10.70 / 22.52 / 36.89 | 3,176 | 4.67 / 6.39 / 14.31 |
| hybrid (RRF) | 308 | 44.56 / 81.86 / 151.88 | 107 | 150.82 / 165.50 / 185.34 | 74 | 203.53 / 288.27 / 342.03 | 5 | 1824.80 / 5403.98 / 8179.06 | 33 | 471.07 / 588.22 / 736.76 |
| hybrid (weighted) | 307 | 46.58 / 79.51 / 135.10 | 121 | 129.45 / 146.71 / 167.79 | 72 | 211.18 / 272.85 / 352.98 | 4 | 1689.42 / 6533.77 / 9430.36 | 33 | 474.68 / 608.36 / 740.67 |
| boosted (bm25 × decay) | n/a | n/a | 443 | 34.87 / 49.13 / 69.27 | 3 | 5254.13 / 9205.87 / 10055.39 | 3 | 2137.87 / 9107.17 / 14054.38 | 2 | 5976.32 / 8515.43 / 9664.10 |
| similar items | 447 | 29.49 / 63.68 / 121.03 | 639 | 24.16 / 30.52 / 40.72 | 9 | 1609.60 / 2828.76 / 3176.78 | 1,571 | 8.68 / 18.06 / 28.89 | 2,903 | 5.38 / 7.26 / 8.66 |
| facet | 31 | 313.88 / 1173.21 / 2616.28 | n/a | n/a | 839 | 15.57 / 32.53 / 74.89 | 121 | 30.22 / 324.07 / 850.25 | 27 | 515.09 / 986.37 / 1442.14 |
| key lookup | 25,788 | 0.57 / 0.78 / 1.39 | 4,222 | 3.51 / 5.51 / 8.83 | 20,411 | 0.63 / 1.14 / 2.27 | 30,224 | 0.46 / 0.66 / 1.47 | 57,954 | 0.26 / 0.33 / 0.45 |
| sql aggregate | 0 | 29839.55 / 29858.67 / 29858.67 | n/a | n/a | 78 | 199.59 / 326.27 / 398.09 | 6 | 2346.72 / 4660.58 / 5287.97 | 1 | 16992.21 / 16992.44 / 16992.67 |


Errors during the timed runs: Strata 0, Elasticsearch 0, Elasticsearch, trial licence 0, OpenSearch 0, Solr 0, Vespa 0, Meilisearch 0, Typesense 0, Manticore 0, Qdrant 0, Weaviate 12, Milvus 0, ParadeDB 0, Postgres 0, Redis 6

## Answers: recall@10 against an exact search, nDCG@10 against the collection's judgments

Unavailable kinds are n/a; recall is for the kinds an exact search can answer.

| kind | Strata recall | Strata nDCG | Elasticsearch recall | Elasticsearch nDCG | Elasticsearch, trial licence recall | Elasticsearch, trial licence nDCG | OpenSearch recall | OpenSearch nDCG | Solr recall | Solr nDCG |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | – | – | – | – | n/a | n/a | – | – | – | – |
| bm25 + filter | – | – | – | – | n/a | n/a | – | – | – | – |
| phrase | – | – | – | – | n/a | n/a | – | – | – | – |
| typo | – | – | – | – | n/a | n/a | – | – | – | – |
| prefix | – | – | – | – | n/a | n/a | – | – | – | – |
| vector | 0.809 | – | 0.738 | – | n/a | n/a | 0.650 | – | 0.788 | – |
| vector + filter | 0.862 | – | 0.699 | – | n/a | n/a | 0.755 | – | 0.921 | – |
| hybrid (RRF) | – | – | n/a | n/a | – | – | – | – | – | – |
| hybrid (weighted) | – | – | – | – | – | – | – | – | – | – |
| boosted (bm25 × decay) | – | – | – | – | n/a | n/a | – | – | – | – |
| similar items | 0.995 | – | 0.930 | – | n/a | n/a | 0.949 | – | 0.998 | – |

| kind | Vespa recall | Vespa nDCG | Meilisearch recall | Meilisearch nDCG | Typesense recall | Typesense nDCG | Manticore recall | Manticore nDCG | Qdrant recall | Qdrant nDCG |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | – | – | – | – | – | – | – | – | – | – |
| bm25 + filter | – | – | – | – | – | – | – | – | – | – |
| phrase | – | – | – | – | – | – | – | – | – | – |
| typo | n/a | n/a | – | – | – | – | – | – | n/a | n/a |
| prefix | n/a | n/a | – | – | – | – | – | – | n/a | n/a |
| vector | 0.781 | – | 0.798 | – | 0.777 | – | 0.716 | – | 0.909 | – |
| vector + filter | 0.866 | – | 0.913 | – | 0.908 | – | 0.682 | – | 0.844 | – |
| hybrid (RRF) | – | – | – | – | – | – | – | – | – | – |
| hybrid (weighted) | – | – | – | – | – | – | n/a | n/a | – | – |
| boosted (bm25 × decay) | – | – | n/a | n/a | n/a | n/a | – | – | – | – |
| similar items | 0.987 | – | 0.986 | – | 0.990 | – | 0.896 | – | 0.975 | – |

| kind | Weaviate recall | Weaviate nDCG | Milvus recall | Milvus nDCG | ParadeDB recall | ParadeDB nDCG | Postgres recall | Postgres nDCG | Redis recall | Redis nDCG |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | – | – | – | – | – | – | – | – | – | – |
| bm25 + filter | – | – | – | – | – | – | – | – | – | – |
| phrase | n/a | n/a | – | – | – | – | – | – | – | – |
| typo | n/a | n/a | n/a | n/a | – | – | n/a | n/a | – | – |
| prefix | – | – | n/a | n/a | – | – | – | – | – | – |
| vector | 0.741 | – | 0.906 | – | 0.739 | – | 0.808 | – | 0.755 | – |
| vector + filter | 0.722 | – | 0.931 | – | 0.862 | – | 0.753 | – | 0.729 | – |
| hybrid (RRF) | – | – | – | – | – | – | – | – | – | – |
| hybrid (weighted) | – | – | – | – | – | – | – | – | – | – |
| boosted (bm25 × decay) | n/a | n/a | – | – | – | – | – | – | – | – |
| similar items | 0.974 | – | 0.986 | – | 0.972 | – | 0.980 | – | 0.964 | – |


## SQL aggregates equal to the true answer

| Strata | Elasticsearch | Elasticsearch, trial licence | OpenSearch | Solr | Vespa | Meilisearch | Typesense | Manticore | Qdrant | Weaviate | Milvus | ParadeDB | Postgres | Redis |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 300 of 300 | 300 of 300 | n/a | 300 of 300 | 300 of 300 | 300 of 300 | n/a | n/a | 300 of 300 | n/a | 300 of 300 | n/a | 300 of 300 | 300 of 300 | 300 of 300 |

## Agreement with Strata's answers

| kind | Elasticsearch | Elasticsearch, trial licence | OpenSearch | Solr | Vespa | Meilisearch | Typesense |
|---|---|---|---|---|---|---|---|
| bm25 | 0.958 of the top 10 | n/a | 0.957 of the top 10 | 0.955 of the top 10 | 0.932 of the top 10 | 0.127 of the top 10 | 0.215 of the top 10 |
| bm25 + filter | 0.953 of the top 10 | n/a | 0.950 of the top 10 | 0.951 of the top 10 | 0.933 of the top 10 | 0.127 of the top 10 | 0.213 of the top 10 |
| phrase | 0.545 of the top 10 | n/a | 0.544 of the top 10 | 0.544 of the top 10 | 0.649 of the top 10 | 0.217 of the top 10 | 0.057 of the top 10 |
| typo | 0.421 of the top 10 | n/a | 0.422 of the top 10 | 0.266 of the top 10 | n/a | 0.100 of the top 10 | 0.171 of the top 10 |
| prefix | 0.966 of the top 10 | n/a | 0.966 of the top 10 | 0.965 of the top 10 | n/a | 0.132 of the top 10 | 0.171 of the top 10 |
| vector | 0.759 of the top 10 | n/a | 0.641 of the top 10 | 0.784 of the top 10 | 0.794 of the top 10 | 0.802 of the top 10 | 0.786 of the top 10 |
| vector + filter | 0.729 of the top 10 | n/a | 0.749 of the top 10 | 0.879 of the top 10 | 0.865 of the top 10 | 0.884 of the top 10 | 0.878 of the top 10 |
| hybrid (RRF) | n/a | 0.812 of the top 10 | 0.737 of the top 10 | 0.603 of the top 10 | 0.732 of the top 10 | 0.310 of the top 10 | 0.363 of the top 10 |
| hybrid (weighted) | 0.459 of the top 10 | 0.718 of the top 10 | 0.701 of the top 10 | 0.439 of the top 10 | 0.734 of the top 10 | 0.648 of the top 10 | 0.592 of the top 10 |
| boosted (bm25 × decay) | 0.971 of the top 10 | n/a | 0.973 of the top 10 | 0.972 of the top 10 | 0.854 of the top 10 | n/a | n/a |
| similar items | 0.931 of the top 10 | n/a | 0.950 of the top 10 | 0.995 of the top 10 | 0.987 of the top 10 | 0.984 of the top 10 | 0.989 of the top 10 |
| facet | 286 of 300 equal | n/a | 286 of 300 equal | 284 of 300 equal | 131 of 300 equal | 51 of 300 equal | 46 of 300 equal |
| key lookup | 300 of 300 equal | n/a | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal |

| kind | Manticore | Qdrant | Weaviate | Milvus | ParadeDB | Postgres | Redis |
|---|---|---|---|---|---|---|---|
| bm25 | 0.559 of the top 10 | 0.943 of the top 10 | 0.530 of the top 10 | 0.745 of the top 10 | 0.752 of the top 10 | 0.460 of the top 10 | 0.397 of the top 10 |
| bm25 + filter | 0.590 of the top 10 | 0.932 of the top 10 | 0.532 of the top 10 | 0.749 of the top 10 | 0.758 of the top 10 | 0.465 of the top 10 | 0.416 of the top 10 |
| phrase | 0.456 of the top 10 | 0.640 of the top 10 | n/a | 0.536 of the top 10 | 0.533 of the top 10 | 0.467 of the top 10 | 0.373 of the top 10 |
| typo | 0.065 of the top 10 | n/a | n/a | n/a | 0.023 of the top 10 | n/a | 0.074 of the top 10 |
| prefix | 0.422 of the top 10 | n/a | 0.564 of the top 10 | n/a | 0.617 of the top 10 | 0.526 of the top 10 | 0.140 of the top 10 |
| vector | 0.721 of the top 10 | 0.822 of the top 10 | 0.750 of the top 10 | 0.809 of the top 10 | 0.707 of the top 10 | 0.793 of the top 10 | 0.764 of the top 10 |
| vector + filter | 0.705 of the top 10 | 0.828 of the top 10 | 0.749 of the top 10 | 0.863 of the top 10 | 0.814 of the top 10 | 0.770 of the top 10 | 0.746 of the top 10 |
| hybrid (RRF) | 0.681 of the top 10 | 0.858 of the top 10 | 0.623 of the top 10 | 0.757 of the top 10 | 0.664 of the top 10 | 0.585 of the top 10 | 0.563 of the top 10 |
| hybrid (weighted) | n/a | 0.707 of the top 10 | 0.713 of the top 10 | 0.740 of the top 10 | 0.661 of the top 10 | 0.691 of the top 10 | 0.298 of the top 10 |
| boosted (bm25 × decay) | 0.548 of the top 10 | 0.646 of the top 10 | n/a | 0.580 of the top 10 | 0.743 of the top 10 | 0.493 of the top 10 | 0.431 of the top 10 |
| similar items | 0.897 of the top 10 | 0.975 of the top 10 | 0.976 of the top 10 | 0.983 of the top 10 | 0.969 of the top 10 | 0.981 of the top 10 | 0.966 of the top 10 |
| facet | 279 of 300 equal | 274 of 300 equal | 56 of 300 equal | n/a | 274 of 300 equal | 83 of 300 equal | 28 of 300 equal |
| key lookup | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal |


## Writes: documents a second, and latency in ms (p50 / p99)

| phase | clients | Strata docs/s | Strata ms | Elasticsearch docs/s | Elasticsearch ms | OpenSearch docs/s | OpenSearch ms | Solr docs/s | Solr ms | Vespa docs/s | Vespa ms |
|---|---|---|---|---|---|---|---|---|---|---|---|
| insert one | 1 | 326 | 2.94 / 5.19 | 244 | 3.96 / 9.48 | 239 | 3.98 / 8.22 | 537 | 1.83 / 3.90 | 138 | 6.76 / 10.76 |
| insert one | 4 | 1,076 | 3.14 / 11.61 | 760 | 4.46 / 16.90 | 840 | 4.30 / 11.10 | 1,636 | 2.03 / 5.06 | 473 | 7.76 / 15.27 |
| insert one | 16 | 2,157 | 4.76 / 69.61 | 2,079 | 6.45 / 32.67 | 2,226 | 6.41 / 20.20 | 2,456 | 5.17 / 23.46 | 712 | 14.61 / 66.35 |
| insert batch | 1 | 1,446 | 54.72 / 126.30 | 1,679 | 41.21 / 238.42 | 1,371 | 60.05 / 93.10 | 1,574 | 38.92 / 160.97 | 800 | 109.70 / 141.35 |
| insert batch | 4 | 2,582 | 115.70 / 523.67 | 3,728 | 71.55 / 421.65 | 3,521 | 91.78 / 244.90 | 3,782 | 68.95 / 387.08 | 819 | 461.34 / 1277.85 |
| insert batch | 16 | 3,921 | 338.70 / 989.55 | 3,854 | 364.55 / 1114.65 | 2,942 | 522.87 / 794.70 | 3,617 | 406.51 / 769.45 | 634 | 2518.18 / 4330.64 |
| update one | 1 | 413 | 2.21 / 4.47 | 218 | 4.07 / 10.14 | 224 | 3.98 / 12.46 | 425 | 2.05 / 9.23 | 215 | 3.64 / 40.07 |
| update one | 4 | 1,466 | 2.44 / 9.80 | 806 | 4.16 / 17.40 | 814 | 4.38 / 12.06 | 1,295 | 2.47 / 11.51 | 698 | 3.75 / 57.60 |
| update one | 16 | 5,576 | 2.80 / 7.10 | 2,308 | 5.88 / 22.48 | 2,139 | 6.38 / 23.30 | 2,146 | 5.90 / 26.05 | 1,192 | 4.65 / 77.95 |

## How soon a written document is found (ms from the write's start)

| engine | searchable | write returned (p50) | found (p50) | found (p90) | found (max) |
|---|---|---|---|---|---|
| Strata | searchable when the write returns | 3.5 | 7.4 | 9.4 | 91.5 |
| Elasticsearch | refresh_interval 1s (the default; loading turned it off) | 6.7 | 892.2 | 1011.7 | 1018.4 |
| OpenSearch | refresh_interval 1s (the default; loading turned it off) | 6.8 | 629.7 | 1020.3 | 1162.5 |
| Solr | autoSoftCommit.maxTime 1000 ms (as Elasticsearch's 1 s refresh; loading turned it off) | 3.2 | 1012.6 | 1022.0 | 1514.2 |
| Vespa | searchable when the write returns | 7.7 | 10.9 | 13.1 | 48.7 |

## Streams through Redpanda: documents written a second, and lag in ms from when a document was due (p50 / p99)

| phase | Strata docs/s | Strata lag | Elasticsearch docs/s | Elasticsearch lag | OpenSearch docs/s | OpenSearch lag | Solr docs/s | Solr lag | Vespa docs/s | Vespa lag |
|---|---|---|---|---|---|---|---|---|---|---|
| 250/s | 250 | 23 / 67 | 250 | 24 / 43 | 250 | 24 / 42 | 250 | 21 / 37 | 250 | 29 / 45 |
| 500/s | 498 | 25 / 110 | 499 | 25 / 49 | 500 | 24 / 51 | 500 | 19 / 34 | 500 | 31 / 85 |
| 1000/s | 998 | 30 / 131 | 999 | 28 / 79 | 999 | 28 / 82 | 999 | 21 / 37 | 829 | 4185 / 7760 (fell behind) |
| 2000/s | 1,979 | 177 / 1001 | 1,998 | 33 / 166 | 1,998 | 36 / 222 | 1,990 | 31 / 286 | not run | not run |
| flat out | 3,189 | 2148 / 5588 | 3,760 | 2043 / 4478 | 3,130 | 3070 / 5514 | 3,397 | 2487 / 5101 | 828 | 12653 / 23396 |

Lag includes reading the topic (Redpanda's part, the same for every engine, is the 'read' column below).

| engine | what written means | read lag p50 (ms) | crash: events written again | documents lost |
|---|---|---|---|---|
| Strata | searchable (the server answers a batch once its rows are visible) | 19 | 0 (the consumer resumes after the offsets the server keeps with the rows (GET /v1/sources)) | 0 |
| Elasticsearch | written: searchable within the 1 s refresh | 19 | 1114 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |
| OpenSearch | written: searchable within the 1 s refresh | 19 | 528 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |
| Solr | written: searchable within the 1 s soft commit | 18 | 1570 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |
| Vespa | searchable (a put is visible when it returns) | 21 | 399 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |

## CPU used by each engine's containers (cgroup cpu.stat)

| engine | phase | pod | CPU seconds | wall seconds | average cores | throttled (s) |
|---|---|---|---|---|---|---|
| Strata | timed run | strata-0 | 724 | 368 | 1.97 | 7 |
| Strata | timed run | strata-1 | 543 | 368 | 1.47 | 0 |
| Strata | writes | strata-0 | 87 | 70 | 1.25 | 3 |
| Strata | writes | strata-1 | 64 | 71 | 0.90 | 0 |
| Strata | stream | strata-0 | 186 | 154 | 1.21 | 6 |
| Strata | stream | strata-1 | 130 | 155 | 0.84 | 1 |
| Elasticsearch | load | es-0 | 1579 | 603 | 2.62 | 100 |
| Elasticsearch | load | es-1 | 966 | 603 | 1.60 | 12 |
| Elasticsearch | timed run | es-0 | 806 | 393 | 2.05 | 16 |
| Elasticsearch | timed run | es-1 | 594 | 393 | 1.51 | 0 |
| Elasticsearch | writes | es-0 | 139 | 102 | 1.36 | 1 |
| Elasticsearch | writes | es-1 | 99 | 102 | 0.97 | 0 |
| Elasticsearch | stream | es-0 | 150 | 151 | 0.99 | 0 |
| Elasticsearch | stream | es-1 | 112 | 151 | 0.74 | 0 |
| Elasticsearch, trial licence | timed run | es-0 | 125 | 69 | 1.82 | 3 |
| Elasticsearch, trial licence | timed run | es-1 | 91 | 69 | 1.31 | 0 |
| OpenSearch | load | os-0 | 2060 | 865 | 2.38 | 137 |
| OpenSearch | load | os-1 | 1084 | 865 | 1.25 | 10 |
| OpenSearch | timed run | os-0 | 893 | 430 | 2.08 | 26 |
| OpenSearch | timed run | os-1 | 651 | 430 | 1.51 | 0 |
| OpenSearch | writes | os-0 | 136 | 104 | 1.30 | 2 |
| OpenSearch | writes | os-1 | 94 | 105 | 0.89 | 0 |
| OpenSearch | stream | os-0 | 179 | 153 | 1.17 | 3 |
| OpenSearch | stream | os-1 | 119 | 153 | 0.78 | 0 |
| Solr | load | solr-0 | 2707 | 1291 | 2.10 | 119 |
| Solr | load | solr-1 | 2029 | 1290 | 1.57 | 73 |
| Solr | timed run | solr-0 | 949 | 428 | 2.22 | 5 |
| Solr | timed run | solr-1 | 695 | 428 | 1.62 | 1 |
| Solr | writes | solr-0 | 173 | 114 | 1.51 | 3 |
| Solr | writes | solr-1 | 122 | 114 | 1.07 | 1 |
| Solr | stream | solr-0 | 176 | 153 | 1.15 | 1 |
| Solr | stream | solr-1 | 135 | 152 | 0.89 | 1 |
| Meilisearch | load | meili-0 | 7993 | 1783 | 4.48 | 94 |
| Meilisearch | timed run | meili-0 | 1039 | 323 | 3.22 | 2 |
| Typesense | load | typesense-0 | 4812 | 1528 | 3.15 | 0 |
| Typesense | timed run | typesense-0 | 1147 | 389 | 2.95 | 20 |
| Manticore | load | manticore-0 | 7958 | 2174 | 3.66 | 643 |
| Manticore | timed run | manticore-0 | 1800 | 386 | 4.66 | 81 |
| Qdrant | load | qdrant-0 | 24713 | 4517 | 5.47 | 63 |
| Qdrant | timed run | qdrant-0 | 1245 | 302 | 4.12 | 12 |
| Weaviate | load | weaviate-0 | 3734 | 582 | 6.42 | 12 |
| Weaviate | timed run | weaviate-0 | 5322 | 1060 | 5.02 | 4 |
| Milvus | load | milvus-0 | 3293 | 571 | 5.77 | 57 |
| Milvus | timed run | milvus-0 | 1676 | 284 | 5.90 | 26 |
| ParadeDB | load | paradedb-0 | 601 | 233 | 2.58 | 1 |
| ParadeDB | timed run | paradedb-0 | 2879 | 919 | 3.13 | 78 |
| Postgres | load | postgres-0 | 4440 | 1430 | 3.10 | 2 |
| Postgres | timed run | postgres-0 | 4947 | 1045 | 4.73 | 49 |
| Redis | load | redis-0 | 1752 | 416 | 4.21 | 0 |
| Redis | timed run | redis-0 | 1928 | 1226 | 1.57 | 1 |

## Memory of each engine's containers (cgroup memory.current, memory.stat, memory.peak; GB, all its pods)

| engine | after the load | of it anonymous | after the timed run | of it anonymous | of it files (page cache) | pods' peaks added |
|---|---|---|---|---|---|---|
| Strata | – | – | 0.76 | 0.44 | 0.06 | 8.37 |
| Elasticsearch | 35.13 | 30.03 | 35.64 | 30.53 | 4.88 | 41.60 |
| Elasticsearch, trial licence | – | – | 35.69 | 30.58 | 4.88 | 41.60 |
| OpenSearch | 35.37 | 29.99 | 35.84 | 30.46 | 5.14 | 45.46 |
| Solr | 22.84 | 18.47 | 24.41 | 19.23 | 4.97 | 33.01 |
| Vespa | – | – | – | – | – | 3.96 |
| Meilisearch | 23.23 | 1.81 | 22.76 | 1.34 | 20.62 | 24.89 |
| Typesense | 24.19 | 4.50 | 24.24 | 4.53 | 18.71 | 25.23 |
| Manticore | 6.90 | 1.53 | 6.93 | 1.70 | 5.04 | 13.01 |
| Qdrant | 9.41 | 3.44 | 9.55 | 3.45 | 4.71 | 22.85 |
| Weaviate | 12.58 | 3.44 | 10.99 | 3.23 | 7.19 | 15.70 |
| Milvus | 41.63 | 14.52 | 38.29 | 10.69 | 24.79 | 42.58 |
| ParadeDB | 31.41 | 0.01 | 34.15 | 0.01 | 33.46 | 34.58 |
| Postgres | 27.49 | 0.02 | 27.50 | 0.02 | 26.94 | 30.25 |
| Redis | 8.12 | 8.10 | 8.32 | 8.30 | 0.00 | 9.18 |

- Elasticsearch, hybrid (RRF): unavailable: {"license.expired.feature":"Reciprocal Rank Fusion (RRF)","reason":"current license is non-compliant for [Reciprocal Rank Fusion (RRF)]","root_cause":[{"license.expired.feature":"Reciprocal Rank Fusion (RRF)","reason":"current license is non-compliant for [Reciprocal Rank Fusion (RRF)]","type":"secu

- Elasticsearch, hybrid (RRF): the rrf retriever

- Elasticsearch, hybrid (weighted): no min-max normalised fusion on the free licence (the linear retriever needs a paid one): the sum of 0.2 x the raw BM25 score and 0.8 x the vector score ((1 + dot) / 2) over the lexical matches and the knn top 100

- Elasticsearch, trial licence, hybrid (RRF): the rrf retriever

- Elasticsearch, trial licence, hybrid (weighted): the linear retriever (a paid feature, here on a trial licence): match and knn top 100, each min-max normalised over the top 100, weighted 0.2 and 0.8

- OpenSearch, hybrid (RRF): a hybrid query (match, knn k 100, pagination depth 100) through a search pipeline's score-ranker-processor, rrf, k 60

- OpenSearch, hybrid (weighted): a hybrid query (match, knn k 100, pagination depth 100) through a search pipeline's normalization-processor: min_max, arithmetic_mean weighted 0.2 and 0.8

- Solr, hybrid (RRF): the combined query handler, rrf, k 60, knn topK 100; the lexical query's depth is the request's limit

- Solr, hybrid (weighted): no min-max normalised fusion: the sum of 0.2 x the raw BM25 score (scaled by k1 + 1 = 2.2 as Elasticsearch's) and 0.8 x the vector score ((1 + dot) / 2) over the lexical matches and the knn top 100

- Solr, typo: each word~N with ChloeDB's 'auto' edits; the Lucene parser does not stem a fuzzy word, so it is compared with the stemmed terms as typed

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

- Manticore, boosted (bm25 × decay): weight() x the decay, where weight() is bm25a(1.2, 0.75), which Manticore scales into 0 to 1 (not BM25's raw sum), so the decay weighs more than on ChloeDB

- Manticore, hybrid (RRF): MATCH and knn fused by RRF (fusion_method='rrf', rank_constant 60, window 100)

- Manticore, phrase: BM25 as the ranker expression bm25a(1.2, 0.75) with plain IDF (weight() is an integer: 10,000 x the score); Manticore's English stopwords

- Manticore, prefix: BM25 as the ranker expression bm25a(1.2, 0.75) with plain IDF (weight() is an integer: 10,000 x the score); Manticore's English stopwords

- Manticore, typo: every word must match, each fuzzy (OPTION fuzzy=1, distance 2, preserve 1: fuzzy search takes no OR; it needs min_infix_len, set to 2)

- Qdrant, prefix: unavailable: prefixes need the prefix tokenizer, an index of its own that would replace the word index the facet and phrase kinds use on the field

- Qdrant, sql aggregate: unavailable: no grouped aggregates: its facet API counts values only

- Qdrant, typo: unavailable: no fuzzy matching: its text index matches whole words (or prefixes, with a prefix tokenizer)

- Qdrant, bm25: BM25 from the client: sparse vectors of ChloeDB's analysis weighted tf (k1 + 1) / (tf + k1 (1 - b + b dl / avgdl)), Qdrant applying the IDF

- Qdrant, bm25 + filter: BM25 from the client: sparse vectors of ChloeDB's analysis weighted tf (k1 + 1) / (tf + k1 (1 - b + b dl / avgdl)), Qdrant applying the IDF

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

- ParadeDB, typo: each word fuzzy with ChloeDB's 'auto' edits, unstemmed (as Solr's), the others matched as words

- Postgres, typo: unavailable: no fuzzy matching in full-text search (pg_trgm compares whole strings, not the words of a tsvector)

- Postgres, bm25: ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Postgres, bm25 + filter: ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Postgres, boosted (bm25 × decay): ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Postgres, hybrid (RRF): ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Postgres, hybrid (weighted): ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Postgres, phrase: ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Postgres, prefix: ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Redis, bm25: BM25STD over Redis's English stemming; typo: %word% (1 edit) or %%word%% (2) by ChloeDB's 'auto'; boosted: FT.AGGREGATE with ADDSCORES and APPLY

- Redis, bm25 + filter: BM25STD over Redis's English stemming; typo: %word% (1 edit) or %%word%% (2) by ChloeDB's 'auto'; boosted: FT.AGGREGATE with ADDSCORES and APPLY

- Redis, boosted (bm25 × decay): BM25STD over Redis's English stemming; typo: %word% (1 edit) or %%word%% (2) by ChloeDB's 'auto'; boosted: FT.AGGREGATE with ADDSCORES and APPLY

- Redis, hybrid (RRF): FT.HYBRID, COMBINE RRF (window 100, constant 60)

- Redis, hybrid (weighted): FT.HYBRID, COMBINE LINEAR: ALPHA 0.2 x the raw BM25STD score + BETA 0.8 x the vector score (Redis 8.8 does not normalise them), over each top 100

- Redis, key lookup: HMGET of the document's hash (its own key-value read)

- Redis, phrase: BM25STD over Redis's English stemming; typo: %word% (1 edit) or %%word%% (2) by ChloeDB's 'auto'; boosted: FT.AGGREGATE with ADDSCORES and APPLY

- Redis, prefix: BM25STD over Redis's English stemming; typo: %word% (1 edit) or %%word%% (2) by ChloeDB's 'auto'; boosted: FT.AGGREGATE with ADDSCORES and APPLY

- Redis, typo: BM25STD over Redis's English stemming; typo: %word% (1 edit) or %%word%% (2) by ChloeDB's 'auto'; boosted: FT.AGGREGATE with ADDSCORES and APPLY

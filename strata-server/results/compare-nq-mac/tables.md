Solr: sql aggregate from run-solr+sql.json, a later run of these kinds alone.

## Load time and index size

| engine | load (s) | of which: send / build | searchable | merge to 1 segment | index size (GB) | documents |
|---|---|---|---|---|---|---|
| Strata | 172 | read 6 + build 167 | (built) | (built as one) | 2.61 | 1,000,000 |
| Elasticsearch | 714 | send 515 | 1 | 198 | 2.55 | 1,000,000 |
| OpenSearch | 785 | send 424 | 1 | 360 | 2.84 | 1,000,000 |
| Solr | 1182 | send 541 | 2 | 639 | 2.14 | 1,000,000 |
| Vespa | 1324 | send 1324 | on write | (on its own) | 4.60 (+ 0.01 transaction log) | 1,000,000 |
| Meilisearch (one node) | 2037 | send 45 | – | index 1992 | 15.48 | 1,000,000 |
| Typesense (one node) | 1094 | send 1094 | – | none | 9.60 | 1,000,000 |
| Manticore (one node) | 1193 | send 354 | – | flush 3, merge 836 | 2.64 | 1,000,000 |
| Qdrant (one node) | 1934 | send 77 | – | index 1857 | 4.77 | 1,000,000 |
| Weaviate (one node) | 366 | send 366 | – | none | 4.34 | 1,000,000 |
| Milvus (one node) | 1784 | send 105 | – | flush 2, index 1678, load 0 | 13.35 | 1,000,000 |
| ParadeDB (one node) | 106 | send 37 | – | analyze 3, primary key 2, search index 64 | 11.86 | 1,000,000 |
| Postgres (one node) | 586 | send 50 | – | analyze 4, gin index 23, hnsw index 507, primary key 2 | 8.39 | 1,000,000 |
| Redis (one node) | 210 | send 210 | – | index 0 | 4.45 (in memory) | 1,000,000 |

## 1 client: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | OpenSearch q/s | OpenSearch ms | Solr q/s | Solr ms | Vespa q/s | Vespa ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 961 | 0.94 / 1.38 / 2.03 | 246 | 3.53 / 5.96 / 12.85 | 331 | 2.60 / 4.34 / 9.92 | 249 | 3.35 / 5.90 / 15.14 | 425 | 1.54 / 2.43 / 34.72 |
| bm25 + filter | 696 | 1.35 / 2.01 / 2.74 | 329 | 2.49 / 4.61 / 11.94 | 416 | 2.00 / 3.52 / 9.58 | 504 | 1.67 / 3.00 / 6.14 | 471 | 1.68 / 3.73 / 6.30 |
| phrase | 1,024 | 0.84 / 1.39 / 2.20 | 786 | 1.12 / 1.79 / 4.13 | 789 | 1.07 / 1.78 / 5.26 | 824 | 1.00 / 1.81 / 4.14 | 1,012 | 0.84 / 1.33 / 2.85 |
| typo | 19 | 49.34 / 83.48 / 109.83 | 127 | 7.47 / 11.55 / 17.26 | 87 | 10.79 / 18.31 / 26.99 | 72 | 13.05 / 20.15 / 29.35 | n/a | n/a |
| prefix | 707 | 1.30 / 2.03 / 2.88 | 545 | 1.62 / 2.74 / 4.16 | 522 | 1.64 / 3.01 / 5.48 | 707 | 1.20 / 1.93 / 4.23 | n/a | n/a |
| vector | 1,610 | 0.58 / 0.67 / 1.28 | 514 | 1.71 / 2.48 / 5.59 | 480 | 1.86 / 2.47 / 5.39 | 542 | 1.61 / 2.35 / 4.93 | 681 | 1.40 / 1.63 / 2.71 |
| vector + filter | 1,436 | 0.67 / 0.75 / 1.12 | 525 | 1.80 / 2.15 / 3.80 | 265 | 3.64 / 4.36 / 7.00 | 210 | 4.12 / 6.80 / 11.05 | 298 | 3.30 / 3.92 / 5.64 |
| hybrid (RRF) | 501 | 1.91 / 2.52 / 3.43 | n/a | n/a | 209 | 4.50 / 5.98 / 9.30 | 253 | 3.43 / 5.87 / 12.80 | 275 | 3.20 / 4.20 / 13.18 |
| hybrid (weighted) | 488 | 1.96 / 2.59 / 3.54 | 298 | 3.04 / 4.69 / 8.43 | 228 | 4.21 / 5.47 / 7.61 | 203 | 4.30 / 7.64 / 13.58 | 301 | 3.16 / 3.89 / 7.58 |
| boosted (bm25 × decay) | 394 | 2.35 / 3.76 / 5.20 | 192 | 4.42 / 9.09 / 12.96 | 194 | 4.34 / 8.98 / 13.01 | 162 | 5.07 / 11.30 / 18.10 | 746 | 1.25 / 1.80 / 2.99 |
| similar items | 1,286 | 0.76 / 0.83 / 1.18 | 262 | 3.43 / 4.18 / 16.83 | 230 | 4.18 / 4.76 / 7.66 | 232 | 3.71 / 6.67 / 9.62 | 397 | 2.43 / 2.88 / 4.35 |
| facet | 1,640 | 0.58 / 0.74 / 0.93 | 889 | 0.92 / 1.54 / 4.71 | 1,105 | 0.76 / 1.23 / 3.75 | 701 | 1.12 / 2.19 / 5.51 | 499 | 1.14 / 4.93 / 11.18 |
| key lookup | 2,523 | 0.38 / 0.42 / 0.65 | 1,324 | 0.73 / 0.88 / 1.39 | 1,546 | 0.65 / 0.76 / 1.13 | 1,144 | 0.81 / 1.01 / 2.07 | 1,349 | 0.63 / 0.69 / 1.62 |
| sql aggregate | 14 | 73.57 / 109.79 / 117.14 | 195 | 4.63 / 6.45 / 14.69 | 308 | 3.00 / 4.22 / 7.99 | 31 | 30.69 / 41.54 / 67.91 | 36 | 27.54 / 42.60 / 45.46 |

| kind | Meilisearch q/s | Meilisearch ms | Typesense q/s | Typesense ms | Manticore q/s | Manticore ms | Qdrant q/s | Qdrant ms | Weaviate q/s | Weaviate ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 283 | 3.23 / 5.44 / 8.35 | 245 | 2.19 / 7.79 / 30.84 | 135 | 5.85 / 14.63 / 23.71 | 1,333 | 0.73 / 0.82 / 1.13 | 437 | 1.90 / 3.84 / 7.51 |
| bm25 + filter | 207 | 4.53 / 6.64 / 9.42 | 34 | 27.77 / 35.08 / 51.06 | 134 | 6.90 / 10.66 / 14.77 | 1,190 | 0.80 / 0.91 / 1.64 | 448 | 1.98 / 3.25 / 6.29 |
| phrase | 745 | 0.91 / 2.77 / 4.48 | 393 | 0.23 / 7.19 / 30.26 | 525 | 1.43 / 3.92 / 7.07 | 1,098 | 0.49 / 1.24 / 8.55 | n/a | n/a |
| typo | 342 | 2.69 / 4.56 / 6.54 | 54 | 7.33 / 48.81 / 99.40 | 27 | 34.88 / 53.98 / 68.54 | n/a | n/a | n/a | n/a |
| prefix | 339 | 2.74 / 4.56 / 7.14 | 130 | 5.21 / 14.28 / 41.43 | 63 | 7.74 / 33.86 / 102.97 | n/a | n/a | 317 | 2.81 / 5.31 / 9.22 |
| vector | 253 | 3.80 / 4.93 / 7.52 | 564 | 1.75 / 1.98 / 2.33 | 2,254 | 0.43 / 0.55 / 0.69 | 1,659 | 0.59 / 0.64 / 0.91 | 543 | 1.73 / 1.99 / 4.42 |
| vector + filter | 87 | 11.30 / 14.76 / 17.37 | 60 | 16.63 / 17.67 / 19.19 | 108 | 9.23 / 10.10 / 10.87 | 991 | 0.98 / 1.12 / 1.69 | 293 | 3.31 / 3.97 / 7.17 |
| hybrid (RRF) | 164 | 5.84 / 8.03 / 10.94 | 179 | 3.73 / 9.32 / 32.07 | 72 | 11.05 / 27.23 / 42.32 | 324 | 2.85 / 4.21 / 4.79 | 259 | 3.45 / 5.62 / 8.68 |
| hybrid (weighted) | 168 | 5.73 / 7.80 / 10.50 | 179 | 3.73 / 9.33 / 32.06 | n/a | n/a | 329 | 2.80 / 4.15 / 4.61 | 259 | 3.44 / 5.48 / 9.24 |
| boosted (bm25 × decay) | n/a | n/a | n/a | n/a | 123 | 6.65 / 15.34 / 23.47 | 1,073 | 0.92 / 1.01 / 1.34 | n/a | n/a |
| similar items | 172 | 5.67 / 7.35 / 9.38 | 19 | 53.01 / 55.18 / 57.14 | 625 | 1.54 / 2.51 / 2.91 | 629 | 1.55 / 1.83 / 2.73 | 314 | 2.99 / 3.36 / 6.86 |
| facet | 528 | 1.75 / 3.60 / 4.81 | 253 | 2.31 / 10.11 / 19.74 | 648 | 0.88 / 3.84 / 7.84 | 68 | 13.26 / 30.32 / 44.11 | 44 | 9.01 / 58.59 / 184.58 |
| key lookup | 4,029 | 0.25 / 0.26 / 0.33 | 3,574 | 0.28 / 0.30 / 0.34 | 4,354 | 0.23 / 0.25 / 0.30 | 4,134 | 0.23 / 0.28 / 0.34 | 2,973 | 0.33 / 0.36 / 0.40 |
| sql aggregate | n/a | n/a | n/a | n/a | 25 | 41.61 / 63.97 / 68.69 | n/a | n/a | 0 | 2160.33 / 2276.66 / 2276.66 |

| kind | Milvus q/s | Milvus ms | ParadeDB q/s | ParadeDB ms | Postgres q/s | Postgres ms | Redis q/s | Redis ms |
|---|---|---|---|---|---|---|---|---|
| bm25 | 651 | 1.28 / 2.19 / 5.64 | 123 | 2.36 / 18.98 / 30.19 | 3 | 231.46 / 671.70 / 733.12 | 31 | 30.01 / 47.57 / 58.15 |
| bm25 + filter | 487 | 1.84 / 2.51 / 5.18 | 110 | 4.62 / 19.30 / 22.40 | 4 | 135.56 / 637.69 / 661.72 | 26 | 36.26 / 55.21 / 61.51 |
| phrase | 992 | 0.92 / 1.32 / 2.49 | 462 | 1.68 / 3.28 / 5.67 | 173 | 1.96 / 13.62 / 50.23 | 1,343 | 0.37 / 1.65 / 4.40 |
| typo | n/a | n/a | 16 | 61.34 / 85.17 / 111.36 | n/a | n/a | 10 | 97.77 / 128.40 / 156.02 |
| prefix | n/a | n/a | 181 | 5.13 / 8.40 / 13.41 | 5 | 110.91 / 531.57 / 643.66 | 149 | 4.79 / 14.16 / 26.73 |
| vector | 541 | 1.72 / 2.17 / 3.25 | 62 | 15.58 / 17.17 / 24.57 | 611 | 1.47 / 1.88 / 6.25 | 2,316 | 0.43 / 0.48 / 0.57 |
| vector + filter | 352 | 2.70 / 3.56 / 5.82 | 52 | 17.77 / 20.86 / 38.25 | 415 | 2.10 / 3.60 / 7.60 | 1,166 | 0.80 / 1.20 / 1.57 |
| hybrid (RRF) | 163 | 5.98 / 7.55 / 9.82 | 35 | 21.10 / 42.26 / 62.36 | 3 | 241.48 / 641.38 / 747.99 | 31 | 30.55 / 47.48 / 61.23 |
| hybrid (weighted) | 168 | 5.86 / 7.12 / 9.12 | 40 | 19.08 / 36.55 / 53.08 | 3 | 248.42 / 665.67 / 761.97 | 31 | 30.03 / 48.13 / 58.33 |
| boosted (bm25 × decay) | 504 | 1.85 / 2.66 / 4.01 | 2 | 532.05 / 697.60 / 700.84 | 3 | 322.21 / 819.20 / 861.82 | 4 | 212.17 / 348.07 / 416.99 |
| similar items | 467 | 2.07 / 2.48 / 3.47 | 3 | 362.88 / 393.11 / 546.52 | 629 | 1.39 / 1.80 / 7.24 | 1,196 | 0.79 / 1.14 / 1.53 |
| facet | n/a | n/a | 627 | 1.38 / 2.35 / 4.26 | 65 | 5.83 / 41.49 / 140.98 | 59 | 8.02 / 47.97 / 78.42 |
| key lookup | 2,387 | 0.41 / 0.45 / 0.64 | 8,156 | 0.12 / 0.14 / 0.18 | 9,240 | 0.11 / 0.12 / 0.17 | 13,930 | 0.07 / 0.08 / 0.09 |
| sql aggregate | n/a | n/a | 50 | 18.91 / 27.42 / 59.01 | 4 | 215.29 / 303.56 / 426.99 | 1 | 743.77 / 878.12 / 1082.19 |


## 4 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | OpenSearch q/s | OpenSearch ms | Solr q/s | Solr ms | Vespa q/s | Vespa ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 1,962 | 1.84 / 2.72 / 7.37 | 560 | 5.01 / 12.53 / 32.93 | 707 | 3.77 / 9.60 / 33.73 | 564 | 5.09 / 12.56 / 34.60 | 1,246 | 2.53 / 4.45 / 24.67 |
| bm25 + filter | 1,103 | 2.98 / 4.76 / 19.83 | 754 | 4.02 / 8.37 / 30.85 | 893 | 3.68 / 6.90 / 22.59 | 1,061 | 3.16 / 6.00 / 16.94 | 763 | 3.50 / 8.50 / 33.62 |
| phrase | 1,987 | 1.76 / 2.86 / 5.56 | 1,948 | 1.78 / 3.15 / 7.22 | 1,994 | 1.73 / 3.14 / 6.74 | 1,244 | 2.43 / 5.22 / 17.73 | 2,228 | 1.47 / 2.80 / 8.24 |
| typo | 18 | 203.09 / 325.98 / 417.65 | 193 | 18.04 / 36.72 / 53.03 | 114 | 30.99 / 59.91 / 83.71 | 100 | 36.85 / 64.47 / 87.70 | n/a | n/a |
| prefix | 1,166 | 2.78 / 4.49 / 19.85 | 1,128 | 3.10 / 5.59 / 10.78 | 773 | 4.13 / 8.36 / 28.83 | 1,332 | 2.71 / 4.72 / 8.22 | n/a | n/a |
| vector | 3,668 | 1.04 / 1.37 / 2.12 | 1,210 | 2.54 / 4.40 / 22.68 | 954 | 3.38 / 5.97 / 22.23 | 931 | 3.67 / 6.66 / 16.92 | 1,235 | 2.61 / 4.24 / 19.17 |
| vector + filter | 3,012 | 1.29 / 1.71 / 2.39 | 1,107 | 3.15 / 4.84 / 14.00 | 425 | 8.11 / 14.42 / 29.57 | 406 | 9.07 / 15.05 / 22.51 | 387 | 7.07 / 17.60 / 44.57 |
| hybrid (RRF) | 827 | 4.49 / 6.45 / 13.43 | n/a | n/a | 349 | 9.73 / 18.90 / 39.02 | 470 | 7.64 / 13.41 / 23.56 | 448 | 6.37 / 13.36 / 38.74 |
| hybrid (weighted) | 819 | 4.50 / 6.43 / 13.32 | 515 | 6.72 / 12.15 / 24.99 | 374 | 9.22 / 16.77 / 31.84 | 320 | 10.98 / 21.64 / 32.27 | 437 | 6.67 / 14.78 / 38.49 |
| boosted (bm25 × decay) | 556 | 5.60 / 10.67 / 30.71 | 304 | 11.33 / 24.41 / 38.43 | 309 | 10.97 / 24.59 / 37.37 | 262 | 12.96 / 28.65 / 45.30 | 1,400 | 2.15 / 3.76 / 23.90 |
| similar items | 2,620 | 1.49 / 1.95 / 2.72 | 444 | 7.71 / 13.62 / 39.11 | 320 | 10.50 / 20.12 / 45.02 | 393 | 9.23 / 16.14 / 25.16 | 560 | 5.07 / 9.61 / 36.67 |
| facet | 3,609 | 1.07 / 1.43 / 2.01 | 2,954 | 1.02 / 1.93 / 8.46 | 3,623 | 0.90 / 1.63 / 4.45 | 1,416 | 2.37 / 4.68 / 12.17 | 732 | 2.89 / 11.24 / 40.82 |
| key lookup | 7,537 | 0.49 / 0.67 / 1.31 | 4,885 | 0.71 / 1.03 / 2.50 | 5,549 | 0.65 / 0.92 / 1.94 | 2,335 | 1.46 / 2.38 / 6.60 | 2,999 | 0.67 / 0.86 / 47.06 |
| sql aggregate | 14 | 297.06 / 483.18 / 523.62 | 347 | 8.33 / 22.27 / 43.74 | 734 | 4.10 / 8.98 / 28.83 | 47 | 83.01 / 114.01 / 170.68 | 36 | 104.54 / 180.41 / 205.87 |

| kind | Meilisearch q/s | Meilisearch ms | Typesense q/s | Typesense ms | Manticore q/s | Manticore ms | Qdrant q/s | Qdrant ms | Weaviate q/s | Weaviate ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 825 | 4.10 / 8.42 / 14.22 | 738 | 2.99 / 10.90 / 41.62 | 210 | 12.49 / 42.92 / 71.45 | 2,775 | 1.33 / 1.95 / 3.60 | 920 | 3.55 / 7.05 / 16.12 |
| bm25 + filter | 478 | 6.83 / 13.05 / 24.91 | 97 | 35.93 / 56.75 / 84.73 | 192 | 15.45 / 38.74 / 64.85 | 2,605 | 1.39 / 2.13 / 3.76 | 910 | 3.73 / 7.12 / 15.24 |
| phrase | 1,894 | 1.65 / 4.24 / 7.88 | 1,173 | 0.27 / 9.38 / 40.09 | 943 | 2.63 / 8.07 / 29.90 | 2,378 | 1.04 / 3.18 / 11.19 | n/a | n/a |
| typo | 914 | 3.69 / 7.61 / 13.10 | 159 | 9.48 / 71.32 / 145.93 | 79 | 49.64 / 78.12 / 100.99 | n/a | n/a | n/a | n/a |
| prefix | 936 | 3.63 / 7.37 / 12.52 | 377 | 6.93 / 25.77 / 52.23 | 100 | 18.55 / 86.98 / 257.10 | n/a | n/a | 735 | 4.79 / 8.88 / 14.85 |
| vector | 734 | 4.62 / 8.75 / 14.38 | 1,585 | 2.07 / 3.66 / 5.75 | 7,872 | 0.43 / 0.64 / 1.32 | 3,936 | 0.95 / 1.40 / 2.18 | 1,457 | 2.31 / 4.14 / 8.60 |
| vector + filter | 247 | 14.40 / 25.60 / 38.56 | 174 | 18.45 / 38.69 / 44.61 | 286 | 10.96 / 31.98 / 37.44 | 1,983 | 1.87 / 2.82 / 4.44 | 617 | 5.10 / 11.10 / 20.92 |
| hybrid (RRF) | 482 | 6.88 / 13.22 / 24.48 | 521 | 4.62 / 16.03 / 47.34 | 200 | 13.70 / 42.83 / 67.61 | 651 | 5.79 / 8.46 / 12.05 | 423 | 8.55 / 13.90 / 25.74 |
| hybrid (weighted) | 442 | 7.72 / 14.57 / 24.37 | 516 | 4.68 / 16.15 / 47.08 | n/a | n/a | 639 | 5.78 / 8.59 / 13.13 | 432 | 8.47 / 13.40 / 23.52 |
| boosted (bm25 × decay) | n/a | n/a | n/a | n/a | 193 | 14.37 / 44.99 / 68.33 | 2,414 | 1.51 / 2.30 / 3.52 | n/a | n/a |
| similar items | 485 | 7.13 / 12.94 / 20.51 | 54 | 77.99 / 85.08 / 96.24 | 1,876 | 1.72 / 2.80 / 21.57 | 1,132 | 3.36 / 4.69 / 7.22 | 757 | 4.53 / 8.32 / 13.22 |
| facet | 1,401 | 2.38 / 5.39 / 8.83 | 557 | 5.28 / 16.18 / 30.16 | 1,144 | 1.52 / 8.01 / 28.31 | 73 | 55.21 / 85.35 / 112.44 | 65 | 29.15 / 144.07 / 434.14 |
| key lookup | 14,826 | 0.26 / 0.35 / 0.56 | 14,034 | 0.27 / 0.35 / 0.61 | 18,941 | 0.19 / 0.29 / 0.46 | 15,830 | 0.24 / 0.34 / 0.52 | 11,303 | 0.31 / 0.45 / 1.09 |
| sql aggregate | n/a | n/a | n/a | n/a | 41 | 95.83 / 149.77 / 337.81 | n/a | n/a | 1 | 6283.37 / 6448.23 / 6448.23 |

| kind | Milvus q/s | Milvus ms | ParadeDB q/s | ParadeDB ms | Postgres q/s | Postgres ms | Redis q/s | Redis ms |
|---|---|---|---|---|---|---|---|---|
| bm25 | 1,353 | 2.53 / 4.45 / 11.52 | 283 | 2.91 / 50.13 / 71.02 | 4 | 799.13 / 1927.55 / 2097.24 | 94 | 42.17 / 63.58 / 79.33 |
| bm25 + filter | 772 | 4.39 / 6.71 / 19.95 | 212 | 6.32 / 55.62 / 75.84 | 7 | 335.83 / 1772.21 / 1908.84 | 77 | 51.81 / 74.63 / 85.31 |
| phrase | 1,970 | 1.88 / 2.90 / 5.35 | 1,406 | 1.76 / 3.95 / 25.52 | 507 | 2.19 / 19.36 / 73.31 | 4,382 | 0.49 / 1.88 / 6.03 |
| typo | n/a | n/a | 19 | 197.58 / 274.27 / 383.69 | n/a | n/a | 26 | 145.16 / 213.59 / 276.07 |
| prefix | n/a | n/a | 531 | 5.57 / 11.58 / 33.43 | 8 | 270.70 / 1186.04 / 1662.17 | 438 | 7.27 / 16.64 / 31.33 |
| vector | 869 | 3.70 / 5.68 / 22.60 | 96 | 38.66 / 64.07 / 99.43 | 1,654 | 1.70 / 2.34 / 24.56 | 7,766 | 0.48 / 0.61 / 1.18 |
| vector + filter | 488 | 6.87 / 11.24 / 25.04 | 66 | 58.08 / 87.83 / 150.74 | 1,061 | 2.47 / 5.42 / 27.89 | 3,605 | 1.01 / 1.59 / 2.78 |
| hybrid (RRF) | 201 | 17.92 / 30.05 / 36.60 | 49 | 76.15 / 128.03 / 206.49 | 3 | 954.99 / 2202.85 / 4203.08 | 80 | 48.27 / 74.90 / 93.06 |
| hybrid (weighted) | 205 | 17.52 / 30.03 / 36.52 | 58 | 65.45 / 105.69 / 153.53 | 3 | 810.96 / 2098.27 / 3600.82 | 80 | 48.03 / 74.35 / 94.13 |
| boosted (bm25 × decay) | 978 | 3.52 / 5.84 / 14.58 | 6 | 578.79 / 1020.48 / 1382.41 | 3 | 1270.03 / 2766.94 / 4850.59 | 7 | 580.54 / 754.58 / 927.34 |
| similar items | 676 | 4.79 / 7.40 / 24.21 | 4 | 1021.85 / 1343.03 / 2007.30 | 1,758 | 1.58 / 2.20 / 23.72 | 3,745 | 0.99 / 1.45 / 2.96 |
| facet | n/a | n/a | 1,803 | 1.47 / 2.69 / 22.98 | 151 | 8.71 / 69.15 / 315.33 | 71 | 51.67 / 112.31 / 196.70 |
| key lookup | 6,947 | 0.54 / 0.77 / 1.46 | 31,027 | 0.12 / 0.17 / 0.26 | 36,344 | 0.10 / 0.15 / 0.23 | 65,113 | 0.07 / 0.09 / 0.12 |
| sql aggregate | n/a | n/a | 120 | 23.86 / 58.67 / 172.38 | 4 | 804.11 / 1149.12 / 2622.00 | 2 | 1844.96 / 2595.31 / 3510.39 |


## 16 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | OpenSearch q/s | OpenSearch ms | Solr q/s | Solr ms | Vespa q/s | Vespa ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 2,242 | 5.77 / 9.13 / 28.88 | 996 | 11.74 / 34.64 / 59.05 | 1,379 | 8.52 / 25.76 / 46.40 | 998 | 12.11 / 33.54 / 55.99 | 1,519 | 7.47 / 22.39 / 43.28 |
| bm25 + filter | 1,209 | 9.44 / 35.95 / 44.54 | 1,065 | 11.56 / 31.24 / 50.08 | 1,054 | 11.40 / 33.53 / 50.23 | 1,229 | 11.01 / 23.11 / 38.53 | 786 | 12.26 / 51.12 / 67.73 |
| phrase | 2,443 | 5.59 / 9.10 / 23.37 | 2,389 | 5.41 / 10.35 / 27.74 | 2,362 | 5.37 / 10.75 / 29.10 | 1,767 | 7.80 / 15.51 / 28.56 | 2,831 | 4.77 / 9.04 / 20.66 |
| typo | 18 | 789.48 / 1465.49 / 2090.77 | 196 | 80.64 / 109.47 / 146.00 | 112 | 139.02 / 180.91 / 207.46 | 100 | 144.71 / 277.38 / 388.14 | n/a | n/a |
| prefix | 1,275 | 9.02 / 34.40 / 44.42 | 1,208 | 10.59 / 29.15 / 38.55 | 812 | 15.47 / 37.60 / 60.29 | 1,471 | 9.50 / 18.88 / 31.48 | n/a | n/a |
| vector | 4,487 | 3.50 / 4.59 / 6.09 | 1,633 | 7.28 / 19.39 / 42.56 | 1,126 | 11.02 / 30.39 / 47.63 | 1,050 | 13.42 / 26.37 / 42.47 | 1,348 | 8.41 / 31.25 / 43.73 |
| vector + filter | 3,419 | 4.37 / 6.00 / 14.61 | 1,323 | 9.89 / 24.19 / 38.05 | 432 | 32.46 / 57.40 / 73.80 | 344 | 41.61 / 76.92 / 126.85 | 391 | 29.78 / 72.18 / 88.45 |
| hybrid (RRF) | 871 | 14.97 / 36.43 / 44.34 | n/a | n/a | 364 | 41.35 / 65.51 / 83.61 | 414 | 34.00 / 63.94 / 124.59 | 442 | 27.34 / 64.87 / 82.49 |
| hybrid (weighted) | 876 | 14.80 / 36.24 / 44.30 | 564 | 24.11 / 47.29 / 62.75 | 377 | 38.75 / 63.74 / 82.31 | 269 | 53.17 / 102.06 / 164.62 | 446 | 27.22 / 64.81 / 88.19 |
| boosted (bm25 × decay) | 582 | 18.59 / 57.30 / 66.57 | 306 | 53.35 / 77.43 / 101.05 | 313 | 50.87 / 73.75 / 92.50 | 260 | 54.52 / 106.83 / 185.13 | 1,446 | 7.10 / 33.13 / 46.77 |
| similar items | 3,038 | 5.21 / 6.79 / 9.39 | 506 | 26.72 / 50.39 / 64.77 | 355 | 42.73 / 65.62 / 84.61 | 396 | 37.34 / 67.03 / 99.10 | 570 | 18.90 / 58.45 / 71.75 |
| facet | 4,502 | 3.45 / 4.54 / 6.41 | 4,733 | 2.57 / 4.70 / 19.61 | 5,008 | 2.51 / 4.63 / 18.88 | 1,914 | 7.22 / 14.56 / 24.86 | 752 | 10.61 / 58.06 / 89.78 |
| key lookup | 10,710 | 1.48 / 1.83 / 2.65 | 8,477 | 1.58 / 2.70 / 10.69 | 8,957 | 1.47 / 2.50 / 10.07 | 3,126 | 4.38 / 8.25 / 19.61 | 4,371 | 1.69 / 2.91 / 54.39 |
| sql aggregate | 14 | 1263.94 / 1775.62 / 2293.90 | 727 | 17.68 / 42.22 / 56.64 | 830 | 17.35 / 38.41 / 62.75 | 58 | 260.48 / 393.09 / 535.54 | 36 | 411.22 / 670.28 / 780.08 |

| kind | Meilisearch q/s | Meilisearch ms | Typesense q/s | Typesense ms | Manticore q/s | Manticore ms | Qdrant q/s | Qdrant ms | Weaviate q/s | Weaviate ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 748 | 19.49 / 35.55 / 64.09 | 739 | 15.62 / 40.51 / 93.62 | 212 | 23.12 / 64.83 / 1269.47 | 3,080 | 4.70 / 7.42 / 13.07 | 959 | 14.16 / 32.88 / 64.02 |
| bm25 + filter | 543 | 23.61 / 59.95 / 103.47 | 98 | 162.44 / 197.02 / 251.91 | 205 | 24.51 / 64.27 / 1260.38 | 2,699 | 5.23 / 8.63 / 14.98 | 915 | 15.25 / 30.82 / 56.61 |
| phrase | 1,702 | 8.59 / 14.20 / 26.65 | 1,168 | 8.10 / 31.69 / 81.44 | 944 | 6.77 / 22.73 / 50.55 | 2,613 | 5.06 / 10.95 / 21.94 | n/a | n/a |
| typo | 1,014 | 14.40 / 26.72 / 45.20 | 155 | 88.07 / 182.49 / 315.11 | 80 | 180.32 / 242.80 / 1177.06 | n/a | n/a | n/a | n/a |
| prefix | 1,008 | 13.88 / 29.05 / 50.68 | 373 | 38.41 / 66.21 / 113.23 | 102 | 33.46 / 576.80 / 1453.28 | n/a | n/a | 685 | 19.48 / 46.14 / 69.43 |
| vector | 736 | 20.28 / 34.20 / 55.31 | 1,741 | 7.10 / 15.96 / 30.23 | 8,451 | 1.76 / 2.18 / 16.16 | 4,720 | 3.13 / 4.95 / 8.33 | 1,475 | 9.39 / 19.67 / 34.37 |
| vector + filter | 260 | 55.63 / 111.57 / 186.45 | 172 | 93.22 / 109.51 / 135.15 | 285 | 68.10 / 78.74 / 84.20 | 2,164 | 6.78 / 10.62 / 16.04 | 739 | 19.39 / 39.22 / 67.06 |
| hybrid (RRF) | 474 | 27.35 / 68.86 / 119.79 | 506 | 25.22 / 50.78 / 99.78 | 202 | 82.46 / 96.43 / 119.70 | 663 | 22.44 / 31.26 / 38.54 | 425 | 35.73 / 52.20 / 91.01 |
| hybrid (weighted) | 445 | 27.89 / 72.71 / 148.08 | 496 | 25.16 / 51.39 / 122.80 | n/a | n/a | 664 | 22.51 / 30.89 / 39.03 | 428 | 34.74 / 52.97 / 107.86 |
| boosted (bm25 × decay) | n/a | n/a | n/a | n/a | 192 | 26.38 / 81.45 / 1229.82 | 2,718 | 5.40 / 8.54 / 14.30 | n/a | n/a |
| similar items | 490 | 27.04 / 62.61 / 101.18 | 53 | 301.38 / 346.65 / 378.63 | 1,903 | 7.79 / 11.24 / 30.92 | 1,199 | 12.61 / 17.42 / 22.74 | 790 | 18.33 / 36.82 / 55.00 |
| facet | 1,397 | 10.90 / 16.84 / 26.51 | 556 | 24.20 / 50.18 / 74.35 | 1,159 | 7.71 / 23.83 / 42.43 | 73 | 215.55 / 268.23 / 326.51 | 64 | 183.77 / 586.45 / 895.55 |
| key lookup | 17,222 | 0.88 / 1.12 / 2.36 | 21,795 | 0.69 / 0.98 / 1.69 | 24,018 | 0.60 / 0.87 / 2.02 | 29,814 | 0.47 / 0.79 / 1.80 | 12,101 | 1.13 / 2.02 / 6.93 |
| sql aggregate | n/a | n/a | n/a | n/a | 40 | 400.74 / 773.30 / 1129.41 | n/a | n/a | 1 | 21359.43 / 21695.80 / 21803.95 |

| kind | Milvus q/s | Milvus ms | ParadeDB q/s | ParadeDB ms | Postgres q/s | Postgres ms | Redis q/s | Redis ms |
|---|---|---|---|---|---|---|---|---|
| bm25 | 1,732 | 8.27 / 13.80 / 22.30 | 881 | 9.19 / 37.30 / 159.65 | 3 | 3891.46 / 6299.27 / 8372.33 | 86 | 180.06 / 223.39 / 315.00 |
| bm25 + filter | 1,387 | 10.31 / 18.18 / 25.37 | 388 | 27.35 / 79.37 / 204.47 | 6 | 1941.24 / 4774.75 / 6408.64 | 78 | 205.33 / 233.64 / 255.88 |
| phrase | 1,976 | 7.66 / 10.21 / 15.87 | 1,266 | 8.28 / 29.80 / 52.42 | 306 | 12.28 / 117.35 / 580.01 | 4,599 | 3.02 / 5.08 / 14.59 |
| typo | n/a | n/a | 20 | 704.34 / 1156.92 / 1682.57 | n/a | n/a | 28 | 563.47 / 626.02 / 662.16 |
| prefix | n/a | n/a | 478 | 27.57 / 61.91 / 103.22 | 5 | 2126.34 / 5419.44 / 6305.56 | 438 | 35.61 / 48.12 / 58.32 |
| vector | 999 | 13.72 / 26.87 / 36.01 | 234 | 52.72 / 142.87 / 242.33 | 956 | 8.55 / 36.90 / 127.40 | 8,376 | 1.62 / 1.88 / 15.58 |
| vector + filter | 574 | 24.89 / 38.15 / 45.69 | 80 | 171.29 / 346.19 / 556.59 | 587 | 14.93 / 58.19 / 175.98 | 3,741 | 3.94 / 4.85 / 14.08 |
| hybrid (RRF) | 229 | 70.61 / 80.55 / 92.37 | 69 | 204.34 / 359.40 / 622.51 | 2 | 5244.67 / 8244.65 / 8791.47 | 79 | 201.29 / 230.78 / 254.72 |
| hybrid (weighted) | 230 | 70.27 / 79.12 / 91.41 | 74 | 196.12 / 327.04 / 638.58 | 3 | 4896.78 / 7947.84 / 8473.74 | 79 | 199.36 / 239.98 / 272.77 |
| boosted (bm25 × decay) | 1,069 | 13.78 / 21.74 / 30.38 | 4 | 3968.23 / 6671.88 / 7024.79 | 3 | 4596.59 / 7996.15 / 8988.46 | 7 | 2197.84 / 2624.10 / 2905.47 |
| similar items | 680 | 19.56 / 36.95 / 44.33 | 4 | 4358.01 / 5840.85 / 6061.62 | 948 | 8.19 / 37.52 / 136.72 | 3,859 | 3.75 / 4.54 / 14.63 |
| facet | n/a | n/a | 1,625 | 6.86 / 22.75 / 41.53 | 85 | 51.67 / 418.28 / 2747.37 | 71 | 212.92 / 312.28 / 450.46 |
| key lookup | 7,310 | 2.06 / 2.69 / 4.94 | 42,625 | 0.31 / 0.55 / 1.35 | 59,883 | 0.22 / 0.41 / 1.05 | 126,351 | 0.12 / 0.16 / 0.19 |
| sql aggregate | n/a | n/a | 114 | 116.29 / 230.87 / 494.48 | 4 | 2774.23 / 6152.96 / 6408.55 | 3 | 5874.90 / 6917.60 / 7728.67 |


Errors during the timed runs: Strata 0, Elasticsearch 0, OpenSearch 0, Solr 0, Vespa 0, Meilisearch 0, Typesense 0, Manticore 0, Qdrant 0, Weaviate 0, Milvus 0, ParadeDB 0, Postgres 0, Redis 0

## Answers: recall@10 against an exact search, nDCG@10 against the collection's judgments

Unavailable kinds are n/a; recall is for the kinds an exact search can answer.

| kind | Strata recall | Strata nDCG | Elasticsearch recall | Elasticsearch nDCG | OpenSearch recall | OpenSearch nDCG | Solr recall | Solr nDCG | Vespa recall | Vespa nDCG |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | – | 0.311 | – | 0.309 | – | 0.306 | – | 0.310 | – | 0.305 |
| bm25 + filter | – | 0.123 | – | 0.123 | – | 0.122 | – | 0.123 | – | 0.123 |
| phrase | – | 0.189 | – | 0.176 | – | 0.179 | – | 0.179 | – | 0.190 |
| typo | – | 0.220 | – | 0.220 | – | 0.220 | – | 0.160 | n/a | n/a |
| prefix | – | 0.268 | – | 0.270 | – | 0.267 | – | 0.264 | n/a | n/a |
| vector | 0.975 | 0.519 | 0.969 | 0.520 | 0.791 | 0.438 | 0.973 | 0.515 | 0.975 | 0.516 |
| vector + filter | 0.962 | 0.147 | 0.884 | 0.139 | 0.907 | 0.141 | 0.989 | 0.147 | 0.991 | 0.147 |
| hybrid (RRF) | – | 0.448 | n/a | n/a | – | 0.424 | – | 0.462 | – | 0.426 |
| hybrid (weighted) | – | 0.537 | – | 0.345 | – | 0.487 | – | 0.340 | – | 0.505 |
| boosted (bm25 × decay) | – | 0.059 | – | 0.059 | – | 0.057 | – | 0.059 | – | 0.061 |
| similar items | 0.988 | – | 0.942 | – | 0.927 | – | 0.998 | – | 0.996 | – |

| kind | Meilisearch recall | Meilisearch nDCG | Typesense recall | Typesense nDCG | Manticore recall | Manticore nDCG | Qdrant recall | Qdrant nDCG | Weaviate recall | Weaviate nDCG |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | – | 0.045 | – | 0.105 | – | 0.287 | – | 0.305 | – | 0.288 |
| bm25 + filter | – | 0.026 | – | 0.040 | – | 0.120 | – | 0.122 | – | 0.114 |
| phrase | – | 0.098 | – | 0.033 | – | 0.184 | – | 0.193 | n/a | n/a |
| typo | – | 0.044 | – | 0.109 | – | 0.041 | n/a | n/a | n/a | n/a |
| prefix | – | 0.042 | – | 0.090 | – | 0.236 | n/a | n/a | – | 0.247 |
| vector | 0.978 | 0.521 | 0.973 | 0.520 | 0.912 | 0.495 | 0.982 | 0.525 | 0.964 | 0.518 |
| vector + filter | 0.992 | 0.147 | 0.988 | 0.147 | 0.853 | 0.139 | 0.944 | 0.143 | 0.918 | 0.141 |
| hybrid (RRF) | – | 0.484 | – | 0.361 | – | 0.461 | – | 0.444 | – | 0.449 |
| hybrid (weighted) | – | 0.521 | – | 0.512 | n/a | n/a | – | 0.482 | – | 0.525 |
| boosted (bm25 × decay) | n/a | n/a | n/a | n/a | – | 0.015 | – | 0.064 | n/a | n/a |
| similar items | 0.995 | – | 0.995 | – | 0.892 | – | 0.968 | – | 0.995 | – |

| kind | Milvus recall | Milvus nDCG | ParadeDB recall | ParadeDB nDCG | Postgres recall | Postgres nDCG | Redis recall | Redis nDCG |
|---|---|---|---|---|---|---|---|---|
| bm25 | – | 0.309 | – | 0.311 | – | 0.244 | – | 0.265 |
| bm25 + filter | – | 0.123 | – | 0.123 | – | 0.102 | – | 0.113 |
| phrase | – | 0.182 | – | 0.182 | – | 0.169 | – | 0.153 |
| typo | n/a | n/a | – | 0.018 | n/a | n/a | – | 0.088 |
| prefix | n/a | n/a | – | 0.263 | – | 0.197 | – | 0.071 |
| vector | 0.990 | 0.523 | 0.865 | 0.480 | 0.977 | 0.522 | 0.968 | 0.516 |
| vector + filter | 0.986 | 0.148 | 0.949 | 0.142 | 0.926 | 0.147 | 0.927 | 0.145 |
| hybrid (RRF) | – | 0.459 | – | 0.429 | – | 0.463 | – | 0.436 |
| hybrid (weighted) | – | 0.509 | – | 0.498 | – | 0.546 | – | 0.290 |
| boosted (bm25 × decay) | – | 0.066 | – | 0.059 | – | 0.025 | – | 0.053 |
| similar items | 0.991 | – | 0.969 | – | 0.990 | – | 0.984 | – |


## SQL aggregates equal to the true answer

| Strata | Elasticsearch | OpenSearch | Solr | Vespa | Meilisearch | Typesense | Manticore | Qdrant | Weaviate | Milvus | ParadeDB | Postgres | Redis |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 300 of 300 | 300 of 300 | 300 of 300 | 300 of 300 | 300 of 300 | n/a | n/a | 300 of 300 | n/a | 300 of 300 | n/a | 300 of 300 | 300 of 300 | 300 of 300 |

## Agreement with Strata's answers

| kind | Elasticsearch | OpenSearch | Solr | Vespa | Meilisearch | Typesense | Manticore |
|---|---|---|---|---|---|---|---|
| bm25 | 0.984 of the top 10 | 0.984 of the top 10 | 0.983 of the top 10 | 0.957 of the top 10 | 0.041 of the top 10 | 0.137 of the top 10 | 0.434 of the top 10 |
| bm25 + filter | 0.986 of the top 10 | 0.989 of the top 10 | 0.991 of the top 10 | 0.967 of the top 10 | 0.037 of the top 10 | 0.121 of the top 10 | 0.488 of the top 10 |
| phrase | 0.718 of the top 10 | 0.717 of the top 10 | 0.719 of the top 10 | 0.845 of the top 10 | 0.281 of the top 10 | 0.086 of the top 10 | 0.495 of the top 10 |
| typo | 0.797 of the top 10 | 0.794 of the top 10 | 0.406 of the top 10 | n/a | 0.036 of the top 10 | 0.105 of the top 10 | 0.047 of the top 10 |
| prefix | 0.981 of the top 10 | 0.981 of the top 10 | 0.980 of the top 10 | n/a | 0.040 of the top 10 | 0.104 of the top 10 | 0.460 of the top 10 |
| vector | 0.966 of the top 10 | 0.790 of the top 10 | 0.971 of the top 10 | 0.969 of the top 10 | 0.976 of the top 10 | 0.971 of the top 10 | 0.909 of the top 10 |
| vector + filter | 0.893 of the top 10 | 0.901 of the top 10 | 0.967 of the top 10 | 0.963 of the top 10 | 0.966 of the top 10 | 0.969 of the top 10 | 0.853 of the top 10 |
| hybrid (RRF) | n/a | 0.882 of the top 10 | 0.660 of the top 10 | 0.833 of the top 10 | 0.547 of the top 10 | 0.462 of the top 10 | 0.660 of the top 10 |
| hybrid (weighted) | 0.515 of the top 10 | 0.845 of the top 10 | 0.491 of the top 10 | 0.791 of the top 10 | 0.799 of the top 10 | 0.735 of the top 10 | n/a |
| boosted (bm25 × decay) | 0.991 of the top 10 | 0.989 of the top 10 | 0.992 of the top 10 | 0.819 of the top 10 | n/a | n/a | 0.464 of the top 10 |
| similar items | 0.945 of the top 10 | 0.921 of the top 10 | 0.986 of the top 10 | 0.989 of the top 10 | 0.986 of the top 10 | 0.987 of the top 10 | 0.895 of the top 10 |
| facet | 300 of 300 equal | 300 of 300 equal | 299 of 300 equal | 81 of 300 equal | 36 of 300 equal | 12 of 300 equal | 290 of 300 equal |
| key lookup | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal |

| kind | Qdrant | Weaviate | Milvus | ParadeDB | Postgres | Redis |
|---|---|---|---|---|---|---|
| bm25 | 0.962 of the top 10 | 0.591 of the top 10 | 0.759 of the top 10 | 0.762 of the top 10 | 0.345 of the top 10 | 0.482 of the top 10 |
| bm25 + filter | 0.963 of the top 10 | 0.585 of the top 10 | 0.746 of the top 10 | 0.748 of the top 10 | 0.381 of the top 10 | 0.485 of the top 10 |
| phrase | 0.813 of the top 10 | n/a | 0.667 of the top 10 | 0.691 of the top 10 | 0.495 of the top 10 | 0.556 of the top 10 |
| typo | n/a | n/a | n/a | 0.034 of the top 10 | n/a | 0.121 of the top 10 |
| prefix | n/a | 0.584 of the top 10 | n/a | 0.723 of the top 10 | 0.435 of the top 10 | 0.213 of the top 10 |
| vector | 0.972 of the top 10 | 0.963 of the top 10 | 0.975 of the top 10 | 0.864 of the top 10 | 0.975 of the top 10 | 0.964 of the top 10 |
| vector + filter | 0.934 of the top 10 | 0.915 of the top 10 | 0.959 of the top 10 | 0.933 of the top 10 | 0.936 of the top 10 | 0.935 of the top 10 |
| hybrid (RRF) | 0.950 of the top 10 | 0.748 of the top 10 | 0.853 of the top 10 | 0.768 of the top 10 | 0.593 of the top 10 | 0.677 of the top 10 |
| hybrid (weighted) | 0.755 of the top 10 | 0.858 of the top 10 | 0.803 of the top 10 | 0.779 of the top 10 | 0.824 of the top 10 | 0.355 of the top 10 |
| boosted (bm25 × decay) | 0.681 of the top 10 | n/a | 0.598 of the top 10 | 0.753 of the top 10 | 0.375 of the top 10 | 0.504 of the top 10 |
| similar items | 0.964 of the top 10 | 0.987 of the top 10 | 0.982 of the top 10 | 0.962 of the top 10 | 0.985 of the top 10 | 0.978 of the top 10 |
| facet | 264 of 300 equal | 42 of 300 equal | n/a | 285 of 300 equal | 91 of 300 equal | 22 of 300 equal |
| key lookup | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal |


## Writes: documents a second, and latency in ms (p50 / p99)

| phase | clients | Strata docs/s | Strata ms | Elasticsearch docs/s | Elasticsearch ms | OpenSearch docs/s | OpenSearch ms | Solr docs/s | Solr ms | Vespa docs/s | Vespa ms |
|---|---|---|---|---|---|---|---|---|---|---|---|
| insert one | 1 | 1,082 | 0.74 / 2.03 | 453 | 1.60 / 10.53 | 547 | 1.48 / 6.74 | 769 | 0.86 / 6.26 | 101 | 10.10 / 14.52 |
| insert one | 4 | 2,162 | 1.40 / 5.04 | 861 | 3.30 / 28.26 | 1,141 | 2.89 / 15.30 | 1,964 | 1.06 / 17.73 | 240 | 11.27 / 40.92 |
| insert one | 16 | 3,725 | 2.96 / 34.24 | 1,808 | 4.93 / 59.16 | 2,098 | 6.03 / 33.51 | 2,141 | 4.00 / 48.24 | 864 | 19.49 / 35.53 |
| insert batch | 1 | 6,613 | 7.93 / 62.54 | 3,542 | 20.40 / 118.49 | 2,645 | 28.38 / 120.05 | 3,174 | 22.87 / 99.09 | 1,111 | 86.51 / 189.10 |
| insert batch | 4 | 8,031 | 39.03 / 248.21 | 6,394 | 47.55 / 201.30 | 3,852 | 81.95 / 348.67 | 4,239 | 64.46 / 341.41 | 933 | 347.67 / 1243.67 |
| insert batch | 16 | 7,779 | 140.81 / 1068.36 | 5,248 | 259.16 / 816.56 | 3,436 | 436.67 / 806.01 | 4,888 | 235.35 / 1213.78 | 930 | 572.35 / 5128.33 |
| update one | 1 | 450 | 0.69 / 49.60 | 549 | 1.30 / 9.28 | 445 | 1.55 / 12.20 | 298 | 1.17 / 35.10 | 78 | 16.61 / 22.96 |
| update one | 4 | 2,295 | 1.16 / 20.65 | 1,342 | 2.45 / 11.21 | 940 | 2.97 / 36.60 | 1,279 | 1.44 / 27.25 | 222 | 20.03 / 40.55 |
| update one | 16 | 4,600 | 2.27 / 29.13 | 2,410 | 4.41 / 34.21 | 1,912 | 5.98 / 51.17 | 1,929 | 4.28 / 53.18 | 867 | 19.75 / 41.05 |

## How soon a written document is found (ms from the write's start)

| engine | searchable | write returned (p50) | found (p50) | found (p90) | found (max) |
|---|---|---|---|---|---|
| Strata | searchable when the write returns | 0.9 | 1.8 | 3.3 | 396.5 |
| Elasticsearch | refresh_interval 1s (the default; loading turned it off) | 4.8 | 972.7 | 1020.9 | 1036.7 |
| OpenSearch | refresh_interval 1s (the default; loading turned it off) | 4.2 | 880.6 | 1019.0 | 1026.8 |
| Solr | autoSoftCommit.maxTime 1000 ms (as Elasticsearch's 1 s refresh; loading turned it off) | 2.2 | 1014.8 | 1022.3 | 1518.8 |
| Vespa | searchable when the write returns | 9.4 | 10.1 | 11.0 | 27.2 |

## Streams through Redpanda: documents written a second, and lag in ms from when a document was due (p50 / p99)

| phase | Strata docs/s | Strata lag | Elasticsearch docs/s | Elasticsearch lag | OpenSearch docs/s | OpenSearch lag | Solr docs/s | Solr lag | Vespa docs/s | Vespa lag |
|---|---|---|---|---|---|---|---|---|---|---|
| 250/s | 250 | 48 / 103 | 250 | 23 / 81 | 250 | 22 / 67 | 250 | 23 / 68 | 250 | 49 / 95 |
| 500/s | 499 | 51 / 104 | 500 | 21 / 66 | 500 | 23 / 72 | 500 | 22 / 67 | 499 | 64 / 108 |
| 1000/s | 998 | 51 / 105 | 998 | 20 / 73 | 999 | 22 / 69 | 999 | 21 / 67 | 997 | 77 / 382 |
| 2000/s | 1,997 | 54 / 154 | 1,996 | 23 / 81 | 1,996 | 45 / 336 | 1,991 | 25 / 113 | 1,753 | 3534 / 5590 (fell behind) |
| flat out | 12,100 | 693 / 1390 | 5,947 | 1810 / 2835 | 5,442 | 2048 / 3185 | 4,969 | 1821 / 3370 | 2,048 | 5136 / 9285 |

Lag includes reading the topic (Redpanda's part, the same for every engine, is the 'read' column below).

| engine | what written means | read lag p50 (ms) | crash: events written again | documents lost |
|---|---|---|---|---|
| Strata | searchable (the server answers a batch once its rows are visible) | 30 | 0 (the consumer resumes after the offsets the server keeps with the rows (GET /v1/sources)) | 0 |
| Elasticsearch | written: searchable within the 1 s refresh | 19 | 1975 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |
| OpenSearch | written: searchable within the 1 s refresh | 19 | 2261 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |
| Solr | written: searchable within the 1 s soft commit | 21 | 2005 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |
| Vespa | searchable (a put is visible when it returns) | 29 | 884 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |

## CPU used by each engine's containers (cgroup cpu.stat)

| engine | phase | pod | CPU seconds | wall seconds | average cores | throttled (s) |
|---|---|---|---|---|---|---|
| Strata | timed run | strata-0 | 254 | 300 | 0.85 | 75 |
| Strata | timed run | strata-1 | 254 | 300 | 0.85 | 66 |
| Strata | timed run | strata-coordinator | 123 | 300 | 0.41 | 0 |
| Strata | writes | strata-0 | 46 | 61 | 0.76 | 18 |
| Strata | writes | strata-1 | 46 | 61 | 0.75 | 18 |
| Strata | writes | strata-coordinator | 17 | 61 | 0.27 | 0 |
| Elasticsearch | load | es-0 | 939 | 720 | 1.30 | 231 |
| Elasticsearch | load | es-1 | 938 | 719 | 1.30 | 208 |
| Elasticsearch | timed run | es-0 | 339 | 271 | 1.25 | 41 |
| Elasticsearch | timed run | es-1 | 331 | 270 | 1.23 | 37 |
| Elasticsearch | writes | es-0 | 79 | 87 | 0.90 | 13 |
| Elasticsearch | writes | es-1 | 72 | 87 | 0.82 | 4 |
| Elasticsearch | stream | es-0 | 52 | 134 | 0.38 | 1 |
| Elasticsearch | stream | es-1 | 52 | 134 | 0.39 | 3 |
| OpenSearch | load | os-0 | 916 | 790 | 1.16 | 145 |
| OpenSearch | load | os-1 | 916 | 789 | 1.16 | 154 |
| OpenSearch | timed run | os-0 | 358 | 289 | 1.24 | 53 |
| OpenSearch | timed run | os-1 | 347 | 289 | 1.20 | 33 |
| OpenSearch | writes | os-0 | 78 | 87 | 0.89 | 9 |
| OpenSearch | writes | os-1 | 74 | 87 | 0.85 | 9 |
| OpenSearch | stream | os-0 | 71 | 136 | 0.52 | 8 |
| OpenSearch | stream | os-1 | 71 | 136 | 0.52 | 9 |
| Solr | load | solr-0 | 1459 | 1188 | 1.23 | 330 |
| Solr | load | solr-1 | 1379 | 1188 | 1.16 | 338 |
| Solr | timed run | solr-0 | 330 | 266 | 1.24 | 33 |
| Solr | timed run | solr-1 | 332 | 266 | 1.25 | 29 |
| Solr | writes | solr-0 | 99 | 96 | 1.03 | 21 |
| Solr | writes | solr-1 | 94 | 96 | 0.98 | 16 |
| Solr | stream | solr-0 | 74 | 135 | 0.55 | 5 |
| Solr | stream | solr-1 | 71 | 136 | 0.52 | 6 |
| Vespa | load | vespa-0 | 430 | 1328 | 0.32 | 48 |
| Vespa | load | vespa-1 | 1133 | 1328 | 0.85 | 648 |
| Vespa | load | vespa-2 | 1270 | 1328 | 0.96 | 715 |
| Vespa | timed run | vespa-0 | 141 | 252 | 0.56 | 24 |
| Vespa | timed run | vespa-1 | 194 | 252 | 0.77 | 67 |
| Vespa | timed run | vespa-2 | 195 | 252 | 0.77 | 71 |
| Vespa | writes | vespa-0 | 28 | 77 | 0.37 | 1 |
| Vespa | writes | vespa-1 | 34 | 77 | 0.44 | 9 |
| Vespa | writes | vespa-2 | 34 | 77 | 0.44 | 10 |
| Meilisearch | load | meili-0 | 4709 | 2042 | 2.31 | 36 |
| Meilisearch | timed run | meili-0 | 498 | 283 | 1.76 | 25 |
| Typesense | load | typesense-0 | 2967 | 1097 | 2.70 | 446 |
| Typesense | timed run | typesense-0 | 548 | 269 | 2.04 | 80 |
| Manticore | load | manticore-0 | 2258 | 1196 | 1.89 | 251 |
| Manticore | timed run | manticore-0 | 655 | 282 | 2.32 | 104 |
| Qdrant | load | qdrant-0 | 3765 | 1943 | 1.94 | 1 |
| Qdrant | timed run | qdrant-0 | 510 | 216 | 2.36 | 4 |
| Weaviate | load | weaviate-0 | 1076 | 370 | 2.91 | 1 |
| Weaviate | timed run | weaviate-0 | 1566 | 637 | 2.46 | 1 |
| Milvus | load | milvus-0 | 2139 | 1788 | 1.20 | 44 |
| Milvus | timed run | milvus-0 | 496 | 194 | 2.56 | 27 |
| ParadeDB | load | paradedb-0 | 185 | 109 | 1.70 | 2 |
| ParadeDB | timed run | paradedb-0 | 1199 | 587 | 2.04 | 130 |
| Postgres | load | postgres-0 | 1649 | 590 | 2.79 | 12 |
| Postgres | timed run | postgres-0 | 1908 | 811 | 2.35 | 131 |
| Redis | load | redis-0 | 628 | 213 | 2.95 | 123 |
| Redis | timed run | redis-0 | 823 | 570 | 1.44 | 18 |

- Elasticsearch, hybrid (RRF): unavailable: {"license.expired.feature":"Reciprocal Rank Fusion (RRF)","reason":"current license is non-compliant for [Reciprocal Rank Fusion (RRF)]","root_cause":[{"license.expired.feature":"Reciprocal Rank Fusion (RRF)","reason":"current license is non-compliant for [Reciprocal Rank Fusion (RRF)]","type":"secu

- Elasticsearch, hybrid (RRF): the rrf retriever

- Elasticsearch, hybrid (weighted): no min-max normalised fusion on the free licence (the linear retriever needs a paid one): the sum of 0.2 x the raw BM25 score and 0.8 x the vector score ((1 + dot) / 2) over the lexical matches and the knn top 100

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

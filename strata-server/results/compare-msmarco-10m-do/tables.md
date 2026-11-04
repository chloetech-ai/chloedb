## Load time and index size

| engine | load (s) | of which: send / build | searchable | merge to 1 segment | index size (GB) | documents |
|---|---|---|---|---|---|---|
| Strata | 4425 | load 3784 + merge 4382 | (built) | (built as one) | 48.93 | 10,000,000 |
| Elasticsearch | 8169 | send 4894 | 4 | 3271 | 48.56 | 10,000,000 |
| OpenSearch | 9726 | send 5497 | 2 | 4227 | 48.39 | 10,000,000 |
| Solr | 15498 | send 7348 | 4 | 8146 | 89.72 | 10,000,000 |
| Vespa | 13121 | send 13121 | on write | (on its own) | 90.58 (+ 0.07 transaction log) | 10,000,000 |
| Qdrant (one node) | 46080 | send 4477 | – | index 41602 | 86.97 | 10,000,000 |
| Weaviate (one node) | 7083 | send 7083 | – | none | 70.09 | 10,000,000 |
| Milvus (one node) | 7234 | send 3690 | – | flush 2, index 3542, load 0 | 234.89 | 10,000,000 |
| ParadeDB (one node) | 2571 | send 1209 | – | analyze 24, primary key 38, search index 1300 | 127.24 | 10,000,000 |
| Postgres (one node) | 29753 | send 1426 | – | analyze 100, gin index 1080, hnsw index 27085, primary key 62 | 110.55 | 10,000,000 |
| Redis (one node) | 4078 | send 4078 | – | index 0 | 78.67 (in memory) | 10,000,000 |

## 1 client: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | Elasticsearch, trial licence q/s | Elasticsearch, trial licence ms | OpenSearch q/s | OpenSearch ms | Solr q/s | Solr ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 187 | 4.36 / 8.70 / 20.09 | 155 | 5.39 / 9.59 / 20.32 | n/a | n/a | 183 | 4.32 / 8.84 / 19.27 | 89 | 10.00 / 18.29 / 35.56 |
| bm25 + filter | 96 | 8.93 / 18.97 / 32.59 | 112 | 6.71 / 14.69 / 49.83 | n/a | n/a | 104 | 6.55 / 15.98 / 55.36 | 86 | 9.87 / 18.82 / 44.49 |
| phrase | 190 | 3.82 / 9.09 / 18.22 | 141 | 5.00 / 13.22 / 26.14 | n/a | n/a | 147 | 4.57 / 13.56 / 25.02 | 90 | 8.87 / 18.74 / 36.14 |
| typo | 1 | 823.82 / 1050.26 / 1746.49 | 28 | 32.75 / 57.61 / 93.36 | n/a | n/a | 21 | 45.31 / 78.99 / 112.47 | 4 | 245.72 / 338.31 / 480.25 |
| prefix | 81 | 10.66 / 20.98 / 35.77 | 92 | 8.28 / 19.66 / 48.04 | n/a | n/a | 66 | 10.54 / 28.26 / 72.57 | 115 | 7.41 / 14.37 / 26.69 |
| vector | 360 | 2.73 / 3.13 / 4.09 | 222 | 4.38 / 5.27 / 7.39 | n/a | n/a | 211 | 4.60 / 5.59 / 7.47 | 12 | 35.46 / 344.21 / 600.79 |
| vector + filter | 263 | 3.73 / 4.34 / 5.65 | 93 | 5.41 / 6.49 / 177.32 | n/a | n/a | 128 | 7.70 / 9.28 / 11.77 | 5 | 113.96 / 350.00 / 1421.98 |
| hybrid (RRF) | 91 | 9.57 / 16.06 / 35.05 | n/a | n/a | 74 | 12.37 / 18.34 / 34.52 | 70 | 11.58 / 22.62 / 58.72 | 9 | 71.54 / 116.51 / 802.61 |
| hybrid (weighted) | 90 | 9.67 / 16.21 / 33.95 | 38 | 16.13 / 56.52 / 122.91 | 26 | 32.00 / 79.93 / 109.23 | 70 | 11.47 / 23.77 / 58.42 | 10 | 85.35 / 176.97 / 230.67 |
| boosted (bm25 × decay) | 53 | 15.48 / 31.85 / 73.02 | 17 | 46.57 / 130.16 / 203.94 | n/a | n/a | 17 | 46.66 / 126.03 / 194.73 | 12 | 65.67 / 216.99 / 263.66 |
| similar items | 211 | 4.65 / 5.40 / 6.84 | 31 | 29.91 / 31.64 / 34.92 | n/a | n/a | 31 | 31.61 / 33.90 / 58.54 | 4 | 93.38 / 590.57 / 1898.94 |
| facet | 249 | 3.29 / 6.73 / 12.38 | 189 | 3.92 / 9.60 / 24.59 | n/a | n/a | 179 | 3.70 / 10.77 / 38.57 | 70 | 8.03 / 30.14 / 81.22 |
| key lookup | 1,144 | 0.86 / 0.96 / 1.24 | 934 | 1.07 / 1.46 / 1.79 | n/a | n/a | 1,138 | 0.86 / 1.16 / 1.49 | 581 | 1.68 / 2.23 / 2.80 |
| sql aggregate | 0 | 4274.12 / 4274.12 / 4274.12 | 125 | 7.75 / 9.28 / 12.32 | n/a | n/a | 201 | 4.86 / 5.81 / 7.82 | 2 | 571.45 / 653.05 / 653.67 |

| kind | Vespa q/s | Vespa ms | Qdrant q/s | Qdrant ms | Weaviate q/s | Weaviate ms | Milvus q/s | Milvus ms | ParadeDB q/s | ParadeDB ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 101 | 6.01 / 17.47 / 51.14 | 187 | 5.07 / 9.11 / 11.82 | 96 | 7.51 / 16.30 / 60.29 | 113 | 7.41 / 14.14 / 22.74 | 58 | 16.40 / 20.97 / 31.54 |
| bm25 + filter | 57 | 8.21 / 49.24 / 71.85 | 163 | 5.89 / 9.95 / 13.54 | 60 | 13.85 / 28.12 / 53.26 | 64 | 14.12 / 21.61 / 28.19 | 36 | 27.04 / 35.39 / 42.55 |
| phrase | 165 | 3.89 / 11.39 / 30.88 | 126 | 3.61 / 18.90 / 63.56 | n/a | n/a | 152 | 6.08 / 8.86 / 11.57 | 86 | 7.12 / 25.24 / 57.35 |
| typo | n/a | n/a | n/a | n/a | n/a | n/a | n/a | n/a | 5 | 206.36 / 276.81 / 325.73 |
| prefix | n/a | n/a | n/a | n/a | 24 | 35.30 / 77.79 / 172.46 | n/a | n/a | 20 | 45.20 / 76.11 / 112.00 |
| vector | 274 | 3.36 / 3.91 / 7.14 | 144 | 6.84 / 7.56 / 8.61 | 112 | 7.55 / 14.07 / 23.35 | 99 | 10.02 / 10.88 / 11.92 | 30 | 32.26 / 38.18 / 43.09 |
| vector + filter | 93 | 10.71 / 11.83 / 13.13 | 72 | 13.86 / 15.12 / 16.77 | 65 | 14.17 / 21.68 / 31.68 | 58 | 15.54 / 26.08 / 30.99 | 12 | 79.90 / 88.79 / 92.11 |
| hybrid (RRF) | 52 | 10.50 / 22.56 / 207.37 | 31 | 26.20 / 47.05 / 131.50 | 40 | 18.42 / 44.25 / 94.71 | 28 | 35.41 / 42.97 / 46.95 | 16 | 61.46 / 75.49 / 126.31 |
| hybrid (weighted) | 51 | 10.75 / 23.05 / 195.77 | 39 | 25.34 / 29.43 / 33.91 | 49 | 16.52 / 29.36 / 92.37 | 27 | 36.12 / 42.70 / 48.12 | 15 | 66.56 / 76.95 / 95.26 |
| boosted (bm25 × decay) | 115 | 5.42 / 12.94 / 70.11 | 166 | 5.86 / 9.86 / 12.55 | n/a | n/a | 71 | 12.56 / 19.49 / 28.56 | 0 | 6187.23 / 6187.23 / 6187.23 |
| similar items | 70 | 14.16 / 14.85 / 16.27 | 30 | 34.16 / 39.84 / 44.30 | 65 | 13.61 / 20.56 / 30.02 | 72 | 13.68 / 15.16 / 18.72 | 1 | 1153.72 / 1168.00 / 1168.00 |
| facet | 27 | 12.52 / 93.73 / 355.78 | 5 | 259.97 / 437.05 / 480.76 | n/a | n/a | n/a | n/a | 49 | 12.15 / 60.80 / 118.91 |
| key lookup | 734 | 1.14 / 1.37 / 2.00 | 2,112 | 0.47 / 0.52 / 0.60 | 2,426 | 0.39 / 0.54 / 0.68 | 711 | 1.38 / 1.66 / 2.03 | 2,231 | 0.40 / 0.58 / 0.90 |
| sql aggregate | 1 | 831.93 / 877.64 / 881.68 | n/a | n/a | n/a | n/a | n/a | n/a | 5 | 196.71 / 268.57 / 282.34 |

| kind | Postgres q/s | Postgres ms | Redis q/s | Redis ms |
|---|---|---|---|---|
| bm25 | n/a | n/a | 1 | 1437.96 / 2777.44 / 2777.44 |
| bm25 + filter | n/a | n/a | 1 | 839.82 / 1482.12 / 1515.48 |
| phrase | n/a | n/a | 146 | 2.90 / 17.98 / 49.18 |
| typo | n/a | n/a | 1 | 1527.23 / 3445.39 / 3445.39 |
| prefix | n/a | n/a | 5 | 30.15 / 256.67 / 3341.31 |
| vector | 292 | 3.16 / 4.73 / 7.12 | 817 | 1.19 / 1.61 / 2.08 |
| vector + filter | 242 | 3.69 / 5.59 / 9.41 | 238 | 4.00 / 6.02 / 7.75 |
| hybrid (RRF) | n/a | n/a | 1 | 1428.97 / 2805.60 / 2805.60 |
| hybrid (weighted) | n/a | n/a | 1 | 1427.19 / 2840.32 / 2840.32 |
| boosted (bm25 × decay) | n/a | n/a | n/a | n/a |
| similar items | 300 | 3.12 / 4.36 / 6.76 | 234 | 4.00 / 6.51 / 8.86 |
| facet | n/a | n/a | 1 | 1019.56 / 1670.23 / 1980.25 |
| key lookup | 2,607 | 0.37 / 0.43 / 0.58 | 4,751 | 0.20 / 0.23 / 0.32 |
| sql aggregate | 0 | 2380.30 / 2676.98 / 2676.98 | n/a | n/a |


## 4 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | Elasticsearch, trial licence q/s | Elasticsearch, trial licence ms | OpenSearch q/s | OpenSearch ms | Solr q/s | Solr ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 430 | 7.33 / 15.57 / 39.22 | 423 | 8.09 / 15.50 / 31.02 | n/a | n/a | 527 | 6.21 / 12.47 / 26.40 | 205 | 15.46 / 33.20 / 77.76 |
| bm25 + filter | 204 | 15.90 / 36.59 / 63.21 | 291 | 10.75 / 22.62 / 70.57 | n/a | n/a | 292 | 10.15 / 24.75 / 75.63 | 202 | 15.84 / 34.37 / 89.11 |
| phrase | 431 | 6.56 / 17.98 / 38.42 | 353 | 8.53 / 22.17 / 39.77 | n/a | n/a | 362 | 8.17 / 22.46 / 39.27 | 203 | 15.03 / 36.67 / 67.93 |
| typo | 2 | 1686.56 / 2692.94 / 3244.85 | 66 | 54.93 / 98.14 / 143.87 | n/a | n/a | 47 | 80.07 / 138.58 / 190.95 | 8 | 460.74 / 844.30 / 1240.84 |
| prefix | 154 | 22.04 / 44.05 / 78.80 | 220 | 14.82 / 30.73 / 74.38 | n/a | n/a | 180 | 16.83 / 40.46 / 94.91 | 266 | 12.47 / 25.62 / 56.44 |
| vector | 916 | 4.20 / 5.61 / 7.24 | 695 | 5.43 / 6.79 / 8.54 | n/a | n/a | 598 | 6.40 / 8.80 / 11.85 | 18 | 59.43 / 552.38 / 698.16 |
| vector + filter | 600 | 6.45 / 8.72 / 10.83 | 285 | 7.58 / 10.60 / 245.19 | n/a | n/a | 345 | 11.25 / 15.15 / 18.80 | 7 | 171.85 / 1477.98 / 1939.74 |
| hybrid (RRF) | 204 | 16.47 / 29.98 / 67.50 | n/a | n/a | 183 | 19.99 / 31.42 / 54.92 | 168 | 19.55 / 39.18 / 92.21 | 11 | 143.24 / 979.15 / 1219.35 |
| hybrid (weighted) | 197 | 17.37 / 31.23 / 70.94 | 93 | 26.64 / 91.07 / 211.68 | 48 | 63.75 / 166.89 / 301.10 | 171 | 19.03 / 38.44 / 91.51 | 13 | 159.49 / 1089.48 / 1423.26 |
| boosted (bm25 × decay) | 102 | 30.26 / 71.45 / 144.42 | 39 | 67.88 / 248.91 / 429.56 | n/a | n/a | 43 | 67.33 / 220.25 / 343.83 | 24 | 130.36 / 408.91 / 637.78 |
| similar items | 468 | 8.25 / 10.93 / 14.31 | 64 | 54.46 / 64.23 / 442.98 | n/a | n/a | 64 | 62.15 / 72.55 / 85.39 | 7 | 147.92 / 1337.80 / 1657.54 |
| facet | 692 | 4.78 / 10.24 / 16.96 | 499 | 6.05 / 14.90 / 39.56 | n/a | n/a | 454 | 6.10 / 17.90 / 58.07 | 163 | 13.18 / 56.37 / 168.84 |
| key lookup | 3,375 | 1.15 / 1.51 / 1.86 | 3,789 | 1.06 / 1.45 / 1.82 | n/a | n/a | 4,982 | 0.80 / 1.08 / 1.40 | 2,417 | 1.55 / 2.29 / 3.28 |
| sql aggregate | 1 | 6170.86 / 7946.27 / 7946.27 | 432 | 8.75 / 11.42 / 14.84 | n/a | n/a | 652 | 5.80 / 7.87 / 11.04 | 4 | 1018.06 / 1428.59 / 1965.24 |

| kind | Vespa q/s | Vespa ms | Qdrant q/s | Qdrant ms | Weaviate q/s | Weaviate ms | Milvus q/s | Milvus ms | ParadeDB q/s | ParadeDB ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 259 | 7.68 / 49.75 / 84.32 | 259 | 15.34 / 22.74 / 28.93 | 245 | 12.22 / 27.62 / 81.42 | 139 | 23.07 / 51.77 / 69.90 | 110 | 36.50 / 48.46 / 69.26 |
| bm25 + filter | 148 | 11.81 / 74.83 / 119.55 | 234 | 16.62 / 24.46 / 32.28 | 121 | 27.05 / 57.24 / 103.11 | 63 | 73.65 / 83.15 / 89.99 | 65 | 58.59 / 82.89 / 115.69 |
| phrase | 500 | 4.98 / 15.83 / 45.68 | 187 | 13.89 / 53.62 / 92.53 | n/a | n/a | 188 | 16.61 / 42.04 / 61.51 | 212 | 10.23 / 46.24 / 97.21 |
| typo | n/a | n/a | n/a | n/a | n/a | n/a | n/a | n/a | 5 | 697.95 / 1126.08 / 2341.30 |
| prefix | n/a | n/a | n/a | n/a | 71 | 45.04 / 102.61 / 218.12 | n/a | n/a | 29 | 117.32 / 200.77 / 402.09 |
| vector | 379 | 4.20 / 47.41 / 72.95 | 215 | 17.97 / 23.77 / 27.86 | 448 | 8.92 / 10.82 / 14.17 | 113 | 25.39 / 59.97 / 66.53 | 53 | 74.18 / 93.12 / 106.04 |
| vector + filter | 285 | 13.59 / 16.88 / 22.83 | 95 | 41.87 / 54.50 / 58.19 | 186 | 19.88 / 30.68 / 37.93 | 63 | 70.84 / 82.81 / 96.59 | 17 | 226.08 / 303.08 / 366.68 |
| hybrid (RRF) | 132 | 15.00 / 40.65 / 315.75 | 50 | 78.91 / 100.20 / 114.62 | 109 | 31.14 / 57.01 / 117.95 | 28 | 141.97 / 162.75 / 177.27 | 24 | 160.06 / 215.49 / 361.06 |
| hybrid (weighted) | 135 | 15.60 / 53.86 / 309.81 | 53 | 75.11 / 98.31 / 104.83 | 101 | 30.88 / 68.68 / 163.83 | 27 | 144.08 / 162.97 / 173.84 | 24 | 165.90 / 203.94 / 247.79 |
| boosted (bm25 × decay) | 305 | 7.80 / 20.92 / 104.98 | 240 | 16.26 / 24.33 / 31.66 | n/a | n/a | 96 | 37.09 / 64.92 / 77.58 | 0 | 6503.11 / 15524.47 / 15524.47 |
| similar items | 207 | 17.94 / 25.83 / 32.17 | 38 | 107.05 / 137.02 / 151.97 | 210 | 17.36 / 28.28 / 33.76 | 68 | 69.54 / 77.62 / 84.25 | 1 | 3353.00 / 4032.69 / 6927.48 |
| facet | 59 | 22.35 / 185.25 / 705.78 | 5 | 754.68 / 1332.57 / 1520.48 | n/a | n/a | n/a | n/a | 96 | 15.90 / 128.32 / 301.47 |
| key lookup | 2,091 | 1.17 / 1.48 / 37.34 | 7,491 | 0.52 / 0.60 / 0.73 | 10,966 | 0.35 / 0.44 / 0.59 | 2,911 | 1.36 / 1.55 / 1.87 | 8,618 | 0.46 / 0.54 / 0.60 |
| sql aggregate | 2 | 1710.43 / 2133.45 / 2141.90 | n/a | n/a | n/a | n/a | n/a | n/a | 4 | 825.05 / 1366.29 / 2859.52 |

| kind | Postgres q/s | Postgres ms | Redis q/s | Redis ms |
|---|---|---|---|---|
| bm25 | n/a | n/a | 2 | 2227.87 / 3389.20 / 4130.22 |
| bm25 + filter | n/a | n/a | 3 | 1228.79 / 1917.65 / 2199.59 |
| phrase | n/a | n/a | 550 | 2.95 / 20.24 / 54.83 |
| typo | n/a | n/a | 1 | 3368.50 / 5105.31 / 5856.11 |
| prefix | n/a | n/a | 27 | 22.48 / 183.80 / 2415.37 |
| vector | 1,052 | 3.48 / 5.29 / 8.60 | 3,336 | 1.18 / 1.51 / 1.93 |
| vector + filter | 939 | 3.82 / 5.93 / 9.78 | 1,000 | 3.72 / 5.87 / 7.55 |
| hybrid (RRF) | n/a | n/a | 2 | 2320.10 / 3516.99 / 4179.46 |
| hybrid (weighted) | n/a | n/a | 2 | 2241.49 / 3409.68 / 4094.00 |
| boosted (bm25 × decay) | n/a | n/a | n/a | n/a |
| similar items | 1,143 | 3.25 / 4.68 / 7.46 | 994 | 3.72 / 6.26 / 8.69 |
| facet | n/a | n/a | 2 | 1820.61 / 3379.41 / 3636.00 |
| key lookup | 9,319 | 0.43 / 0.48 / 0.56 | 18,290 | 0.21 / 0.24 / 0.29 |
| sql aggregate | 0 | 8316.25 / 13483.11 / 13483.11 | n/a | n/a |


## 16 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | Elasticsearch, trial licence q/s | Elasticsearch, trial licence ms | OpenSearch q/s | OpenSearch ms | Solr q/s | Solr ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 493 | 26.16 / 56.27 / 144.21 | 608 | 23.43 / 42.21 / 77.43 | n/a | n/a | 777 | 17.70 / 34.46 / 70.91 | 299 | 41.14 / 101.82 / 228.34 |
| bm25 + filter | 223 | 55.38 / 142.60 / 261.40 | 372 | 35.34 / 68.31 / 187.39 | n/a | n/a | 364 | 36.75 / 71.24 / 188.38 | 260 | 46.93 / 120.02 / 269.22 |
| phrase | 530 | 23.16 / 56.09 / 114.62 | 440 | 29.99 / 65.70 / 126.52 | n/a | n/a | 477 | 28.39 / 60.83 / 111.05 | 244 | 48.63 / 134.67 / 274.05 |
| typo | 3 | 5524.39 / 8725.49 / 9200.99 | 65 | 233.67 / 351.75 / 501.32 | n/a | n/a | 47 | 314.01 / 468.82 / 738.44 | 9 | 1611.58 / 2799.40 / 3354.58 |
| prefix | 150 | 90.95 / 196.40 / 377.76 | 258 | 52.05 / 95.89 / 214.96 | n/a | n/a | 204 | 65.92 / 126.35 / 257.85 | 359 | 35.80 / 85.82 / 184.59 |
| vector | 1,223 | 12.56 / 17.68 / 24.84 | 1,150 | 12.73 / 19.59 / 27.54 | n/a | n/a | 1,114 | 13.59 / 19.56 / 26.50 | 44 | 254.08 / 745.44 / 1072.96 |
| vector + filter | 733 | 20.35 / 33.43 / 45.94 | 459 | 22.09 / 35.00 / 462.62 | n/a | n/a | 471 | 32.69 / 45.34 / 58.40 | 16 | 718.25 / 2105.24 / 2670.76 |
| hybrid (RRF) | 221 | 61.38 / 116.40 / 313.02 | n/a | n/a | 235 | 63.63 / 95.16 / 158.86 | 187 | 75.87 / 130.85 / 254.11 | 19 | 640.61 / 1670.74 / 2046.73 |
| hybrid (weighted) | 219 | 61.96 / 122.97 / 264.51 | 98 | 132.69 / 293.23 / 550.59 | 62 | 230.81 / 398.03 / 617.37 | 191 | 71.45 / 136.53 / 252.31 | 17 | 637.16 / 1960.28 / 2457.74 |
| boosted (bm25 × decay) | 101 | 113.49 / 300.55 / 743.38 | 38 | 352.23 / 694.91 / 1104.73 | n/a | n/a | 41 | 335.91 / 677.11 / 988.60 | 29 | 332.43 / 1146.42 / 2330.26 |
| similar items | 521 | 29.44 / 45.51 / 60.71 | 66 | 234.72 / 299.53 / 604.14 | n/a | n/a | 59 | 260.43 / 363.78 / 419.81 | 17 | 664.50 / 2183.30 / 2402.02 |
| facet | 962 | 14.18 / 28.39 / 47.05 | 615 | 21.03 / 45.69 / 110.08 | n/a | n/a | 593 | 21.57 / 47.69 / 136.42 | 202 | 39.42 / 180.24 / 648.23 |
| key lookup | 5,009 | 3.05 / 4.26 / 5.14 | 9,620 | 1.54 / 2.39 / 3.48 | n/a | n/a | 14,441 | 1.02 / 1.66 / 2.61 | 4,516 | 3.12 / 5.86 / 9.70 |
| sql aggregate | 1 | 19370.35 / 20114.35 / 20161.08 | 802 | 19.03 / 27.59 / 36.98 | n/a | n/a | 821 | 17.43 / 33.57 / 49.06 | 5 | 2810.67 / 4024.62 / 5153.51 |

| kind | Vespa q/s | Vespa ms | Qdrant q/s | Qdrant ms | Weaviate q/s | Weaviate ms | Milvus q/s | Milvus ms | ParadeDB q/s | ParadeDB ms |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | 351 | 32.13 / 80.48 / 211.36 | 267 | 58.35 / 75.42 / 98.68 | 276 | 50.48 / 100.98 / 198.85 | 165 | 96.56 / 121.42 / 150.97 | 252 | 59.45 / 105.36 / 193.54 |
| bm25 + filter | 146 | 76.10 / 239.18 / 364.66 | 235 | 65.60 / 88.17 / 110.15 | 147 | 95.83 / 175.88 / 266.03 | 93 | 177.16 / 191.74 / 200.74 | 98 | 145.28 / 232.83 / 485.83 |
| phrase | 517 | 22.37 / 60.91 / 116.13 | 191 | 77.77 / 131.10 / 169.22 | n/a | n/a | 191 | 85.04 / 103.58 / 131.14 | 245 | 37.71 / 145.77 / 382.19 |
| typo | n/a | n/a | n/a | n/a | n/a | n/a | n/a | n/a | 5 | 2243.80 / 6329.89 / 7059.29 |
| prefix | n/a | n/a | n/a | n/a | 87 | 118.94 / 354.32 / 928.93 | n/a | n/a | 35 | 364.60 / 902.06 / 1437.42 |
| vector | 433 | 9.82 / 88.74 / 92.62 | 214 | 72.09 / 88.54 / 108.73 | 473 | 30.43 / 59.31 / 93.74 | 134 | 114.97 / 140.15 / 161.18 | 72 | 203.58 / 304.96 / 444.39 |
| vector + filter | 263 | 63.12 / 79.78 / 88.35 | 96 | 158.21 / 196.25 / 284.97 | 289 | 49.02 / 88.61 / 151.32 | 77 | 206.70 / 225.04 / 251.32 | 20 | 739.74 / 1246.27 / 1409.11 |
| hybrid (RRF) | 129 | 85.86 / 151.20 / 849.98 | 52 | 287.25 / 345.75 / 518.87 | 88 | 136.77 / 328.39 / 726.27 | 36 | 452.87 / 484.26 / 523.36 | 32 | 462.39 / 732.03 / 1093.75 |
| hybrid (weighted) | 130 | 85.81 / 152.92 / 797.56 | 53 | 287.18 / 343.44 / 518.16 | 106 | 122.89 / 249.45 / 716.18 | 35 | 453.81 / 489.52 / 516.08 | 31 | 465.84 / 726.16 / 942.77 |
| boosted (bm25 × decay) | 283 | 38.68 / 81.62 / 346.38 | 247 | 64.32 / 80.82 / 94.77 | n/a | n/a | 112 | 142.99 / 172.54 / 196.11 | 0 | 36347.38 / 49713.56 / 52559.70 |
| similar items | 198 | 83.65 / 98.31 / 108.68 | 38 | 401.13 / 473.14 / 729.50 | 251 | 56.11 / 103.61 / 163.10 | 66 | 232.40 / 272.13 / 283.49 | 1 | 14652.35 / 15635.89 / 15685.60 |
| facet | 58 | 176.04 / 546.89 / 1560.95 | 5 | 2563.84 / 4022.82 / 4592.75 | n/a | n/a | n/a | n/a | 86 | 50.80 / 311.23 / 3485.22 |
| key lookup | 2,440 | 1.71 / 2.65 / 77.17 | 22,102 | 0.70 / 0.91 / 1.25 | 27,203 | 0.53 / 0.86 / 1.45 | 5,540 | 2.68 / 4.15 / 6.35 | 22,173 | 0.58 / 1.06 / 1.91 |
| sql aggregate | 3 | 4640.13 / 7631.76 / 8507.37 | n/a | n/a | n/a | n/a | n/a | n/a | 6 | 2175.74 / 3964.28 / 6709.68 |

| kind | Postgres q/s | Postgres ms | Redis q/s | Redis ms |
|---|---|---|---|---|
| bm25 | n/a | n/a | 3 | 4612.12 / 6425.41 / 7636.97 |
| bm25 + filter | n/a | n/a | 5 | 2761.12 / 3736.07 / 4091.66 |
| phrase | n/a | n/a | 1,035 | 11.48 / 31.67 / 63.54 |
| typo | n/a | n/a | 2 | 5843.54 / 8606.23 / 9820.90 |
| prefix | n/a | n/a | 55 | 146.99 / 413.40 / 1368.94 |
| vector | 1,309 | 10.25 / 22.53 / 36.89 | 6,885 | 2.25 / 2.95 / 4.21 |
| vector + filter | 1,105 | 10.70 / 26.63 / 51.31 | 1,978 | 7.31 / 11.11 / 19.41 |
| hybrid (RRF) | n/a | n/a | 1 | 5583.46 / 9411.74 / 10015.72 |
| hybrid (weighted) | n/a | n/a | 1 | 5672.00 / 9719.19 / 9946.80 |
| boosted (bm25 × decay) | n/a | n/a | n/a | n/a |
| similar items | 1,427 | 9.46 / 20.14 / 33.71 | 1,961 | 7.41 / 12.28 / 17.61 |
| facet | n/a | n/a | 3 | 5403.02 / 7254.12 / 7581.21 |
| key lookup | 28,476 | 0.48 / 0.72 / 1.59 | 62,354 | 0.25 / 0.30 / 0.38 |
| sql aggregate | 0 | 11282.85 / 17064.54 / 17064.54 | n/a | n/a |


Errors during the timed runs: Strata 0, Elasticsearch 0, Elasticsearch, trial licence 0, OpenSearch 0, Solr 0, Vespa 0, Qdrant 0, Weaviate 0, Milvus 0, ParadeDB 0, Postgres 13, Redis 20

## Answers: recall@10 against an exact search, nDCG@10 against the collection's judgments

Unavailable kinds are n/a; recall is for the kinds an exact search can answer.

| kind | Strata recall | Strata nDCG | Elasticsearch recall | Elasticsearch nDCG | Elasticsearch, trial licence recall | Elasticsearch, trial licence nDCG | OpenSearch recall | OpenSearch nDCG | Solr recall | Solr nDCG |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | – | – | – | – | n/a | n/a | – | – | – | – |
| bm25 + filter | – | – | – | – | n/a | n/a | – | – | – | – |
| phrase | – | – | – | – | n/a | n/a | – | – | – | – |
| typo | – | – | – | – | n/a | n/a | – | – | – | – |
| prefix | – | – | – | – | n/a | n/a | – | – | – | – |
| vector | 0.795 | – | 0.669 | – | n/a | n/a | 0.702 | – | 0.765 | – |
| vector + filter | 0.823 | – | 0.634 | – | n/a | n/a | 0.856 | – | 0.917 | – |
| hybrid (RRF) | – | – | n/a | n/a | – | – | – | – | – | – |
| hybrid (weighted) | – | – | – | – | – | – | – | – | – | – |
| boosted (bm25 × decay) | – | – | – | – | n/a | n/a | – | – | – | – |
| similar items | 0.978 | – | 0.915 | – | n/a | n/a | 0.944 | – | 0.983 | – |

| kind | Vespa recall | Vespa nDCG | Qdrant recall | Qdrant nDCG | Weaviate recall | Weaviate nDCG | Milvus recall | Milvus nDCG | ParadeDB recall | ParadeDB nDCG |
|---|---|---|---|---|---|---|---|---|---|---|
| bm25 | – | – | – | – | – | – | – | – | – | – |
| bm25 + filter | – | – | – | – | – | – | – | – | – | – |
| phrase | – | – | – | – | n/a | n/a | – | – | – | – |
| typo | n/a | n/a | n/a | n/a | n/a | n/a | n/a | n/a | – | – |
| prefix | n/a | n/a | n/a | n/a | – | – | n/a | n/a | – | – |
| vector | 0.634 | – | 0.941 | – | 0.735 | – | 0.902 | – | 0.823 | – |
| vector + filter | 0.820 | – | 0.876 | – | 0.690 | – | 0.930 | – | 0.874 | – |
| hybrid (RRF) | – | – | – | – | – | – | – | – | – | – |
| hybrid (weighted) | – | – | – | – | – | – | – | – | – | – |
| boosted (bm25 × decay) | – | – | – | – | n/a | n/a | – | – | – | – |
| similar items | 0.962 | – | 0.981 | – | 0.958 | – | 0.984 | – | 0.989 | – |

| kind | Postgres recall | Postgres nDCG | Redis recall | Redis nDCG |
|---|---|---|---|---|
| bm25 | n/a | n/a | – | – |
| bm25 + filter | n/a | n/a | – | – |
| phrase | n/a | n/a | – | – |
| typo | n/a | n/a | – | – |
| prefix | n/a | n/a | – | – |
| vector | 0.826 | – | 0.684 | – |
| vector + filter | 0.796 | – | 0.667 | – |
| hybrid (RRF) | n/a | n/a | – | – |
| hybrid (weighted) | n/a | n/a | – | – |
| boosted (bm25 × decay) | n/a | n/a | n/a | n/a |
| similar items | 0.970 | – | 0.919 | – |


## SQL aggregates equal to the true answer

| Strata | Elasticsearch | Elasticsearch, trial licence | OpenSearch | Solr | Vespa | Qdrant | Weaviate | Milvus | ParadeDB | Postgres | Redis |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 300 of 300 | 300 of 300 | n/a | 300 of 300 | 300 of 300 | 300 of 300 | n/a | n/a | n/a | 300 of 300 | 300 of 300 | n/a |

## Agreement with Strata's answers

| kind | Elasticsearch | Elasticsearch, trial licence | OpenSearch | Solr | Vespa | Qdrant | Weaviate |
|---|---|---|---|---|---|---|---|
| bm25 | 0.979 of the top 10 | n/a | 0.972 of the top 10 | 0.975 of the top 10 | 0.935 of the top 10 | 0.949 of the top 10 | 0.551 of the top 10 |
| bm25 + filter | 0.981 of the top 10 | n/a | 0.982 of the top 10 | 0.981 of the top 10 | 0.950 of the top 10 | 0.961 of the top 10 | 0.570 of the top 10 |
| phrase | 0.658 of the top 10 | n/a | 0.660 of the top 10 | 0.660 of the top 10 | 0.773 of the top 10 | 0.761 of the top 10 | n/a |
| typo | 0.541 of the top 10 | n/a | 0.538 of the top 10 | 0.316 of the top 10 | n/a | n/a | n/a |
| prefix | 0.977 of the top 10 | n/a | 0.976 of the top 10 | 0.978 of the top 10 | n/a | n/a | 0.520 of the top 10 |
| vector | 0.645 of the top 10 | n/a | 0.650 of the top 10 | 0.719 of the top 10 | 0.605 of the top 10 | 0.800 of the top 10 | 0.667 of the top 10 |
| vector + filter | 0.635 of the top 10 | n/a | 0.799 of the top 10 | 0.819 of the top 10 | 0.765 of the top 10 | 0.807 of the top 10 | 0.665 of the top 10 |
| hybrid (RRF) | n/a | 0.728 of the top 10 | 0.744 of the top 10 | 0.563 of the top 10 | 0.721 of the top 10 | 0.820 of the top 10 | 0.573 of the top 10 |
| hybrid (weighted) | 0.449 of the top 10 | 0.657 of the top 10 | 0.731 of the top 10 | 0.430 of the top 10 | 0.728 of the top 10 | 0.701 of the top 10 | 0.687 of the top 10 |
| boosted (bm25 × decay) | 0.982 of the top 10 | n/a | 0.985 of the top 10 | 0.982 of the top 10 | 0.814 of the top 10 | 0.557 of the top 10 | n/a |
| similar items | 0.909 of the top 10 | n/a | 0.931 of the top 10 | 0.969 of the top 10 | 0.951 of the top 10 | 0.967 of the top 10 | 0.945 of the top 10 |
| facet | 272 of 300 equal | n/a | 271 of 300 equal | 268 of 300 equal | 78 of 300 equal | 219 of 300 equal | n/a |
| key lookup | 300 of 300 equal | n/a | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal |

| kind | Milvus | ParadeDB | Postgres | Redis |
|---|---|---|---|---|
| bm25 | 0.733 of the top 10 | 0.738 of the top 10 | n/a | 0.392 of the top 10 |
| bm25 + filter | 0.764 of the top 10 | 0.768 of the top 10 | n/a | 0.425 of the top 10 |
| phrase | 0.633 of the top 10 | 0.626 of the top 10 | n/a | 0.465 of the top 10 |
| typo | n/a | 0.075 of the top 10 | n/a | 0.051 of the top 10 |
| prefix | n/a | 0.463 of the top 10 | n/a | 0.042 of the top 10 |
| vector | 0.790 of the top 10 | 0.743 of the top 10 | 0.742 of the top 10 | 0.624 of the top 10 |
| vector + filter | 0.817 of the top 10 | 0.792 of the top 10 | 0.754 of the top 10 | 0.636 of the top 10 |
| hybrid (RRF) | 0.723 of the top 10 | 0.668 of the top 10 | n/a | 0.493 of the top 10 |
| hybrid (weighted) | 0.735 of the top 10 | 0.739 of the top 10 | n/a | 0.314 of the top 10 |
| boosted (bm25 × decay) | 0.518 of the top 10 | 0.785 of the top 10 | n/a | n/a |
| similar items | 0.969 of the top 10 | 0.972 of the top 10 | 0.957 of the top 10 | 0.912 of the top 10 |
| facet | n/a | 248 of 300 equal | n/a | 8 of 300 equal |
| key lookup | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal | 300 of 300 equal |


## Writes: documents a second, and latency in ms (p50 / p99)

| phase | clients | Strata docs/s | Strata ms | Elasticsearch docs/s | Elasticsearch ms | OpenSearch docs/s | OpenSearch ms | Vespa docs/s | Vespa ms |
|---|---|---|---|---|---|---|---|---|---|
| insert one | 1 | 257 | 3.56 / 6.90 | 275 | 3.21 / 9.00 | 262 | 3.46 / 7.31 | 150 | 6.33 / 10.41 |
| insert one | 4 | 920 | 3.56 / 7.76 | 919 | 3.81 / 10.79 | 827 | 4.12 / 11.43 | 527 | 6.96 / 17.43 |
| insert one | 16 | 2,148 | 6.08 / 71.30 | 2,292 | 5.69 / 22.26 | 2,135 | 6.28 / 30.99 | 689 | 13.95 / 69.97 |
| insert batch | 1 | 1,754 | 34.37 / 198.02 | 1,550 | 44.75 / 303.98 | 1,379 | 62.27 / 91.20 | 779 | 113.65 / 169.98 |
| insert batch | 4 | 3,348 | 96.02 / 355.30 | 3,438 | 79.53 / 435.49 | 3,357 | 99.58 / 241.83 | 702 | 369.32 / 1551.43 |
| insert batch | 16 | 3,841 | 384.68 / 687.37 | 3,790 | 336.59 / 1057.51 | 3,265 | 467.08 / 694.02 | 632 | 1730.92 / 5672.80 |
| update one | 1 | 322 | 2.98 / 5.52 | 238 | 3.57 / 12.69 | 232 | 3.76 / 10.02 | 255 | 3.12 / 38.34 |
| update one | 4 | 1,135 | 3.04 / 8.21 | 856 | 3.95 / 10.99 | 798 | 4.14 / 28.40 | 858 | 3.28 / 55.22 |
| update one | 16 | 3,927 | 3.96 / 6.51 | 1,797 | 6.72 / 38.37 | 1,993 | 6.49 / 35.85 | 1,493 | 3.81 / 77.15 |

## How soon a written document is found (ms from the write's start)

| engine | searchable | write returned (p50) | found (p50) | found (p90) | found (max) |
|---|---|---|---|---|---|
| Strata | searchable when the write returns | 3.9 | 8.2 | 10.0 | 112.6 |
| Elasticsearch | refresh_interval 1s (the default; loading turned it off) | 5.9 | 796.3 | 1023.0 | 1057.1 |
| OpenSearch | refresh_interval 1s (the default; loading turned it off) | 6.0 | 641.3 | 1016.9 | 1032.2 |
| Vespa | searchable when the write returns | 7.2 | 10.0 | 11.3 | 13.8 |

## Streams through Redpanda: documents written a second, and lag in ms from when a document was due (p50 / p99)

| phase | Strata docs/s | Strata lag | Elasticsearch docs/s | Elasticsearch lag | OpenSearch docs/s | OpenSearch lag | Vespa docs/s | Vespa lag |
|---|---|---|---|---|---|---|---|---|
| 250/s | 249 | 78 / 118 | 250 | 22 / 57 | 250 | 24 / 57 | 250 | 27 / 47 |
| 500/s | 498 | 78 / 149 | 500 | 22 / 57 | 500 | 23 / 63 | 499 | 30 / 61 |
| 1000/s | 998 | 79 / 233 | 999 | 27 / 86 | 999 | 27 / 82 | 803 | 4799 / 8889 (fell behind) |
| 2000/s | 1,994 | 91 / 402 | 1,998 | 34 / 244 | 1,998 | 41 / 333 | not run | not run |
| flat out | 3,372 | 2687 / 5153 | 3,424 | 2403 / 4873 | 3,410 | 2632 / 5018 | 824 | 12799 / 23473 |

Lag includes reading the topic (Redpanda's part, the same for every engine, is the 'read' column below).

| engine | what written means | read lag p50 (ms) | crash: events written again | documents lost |
|---|---|---|---|---|
| Strata | searchable (the server answers a batch once its rows are visible) | 34 | 0 (the consumer resumes after the offsets the server keeps with the rows (GET /v1/sources)) | 0 |
| Elasticsearch | written: searchable within the 1 s refresh | 18 | 1532 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |
| OpenSearch | written: searchable within the 1 s refresh | 19 | 1513 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |
| Vespa | searchable (a put is visible when it returns) | 20 | 208 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |

## CPU used by each engine's containers (cgroup cpu.stat)

| engine | phase | pod | CPU seconds | wall seconds | average cores | throttled (s) |
|---|---|---|---|---|---|---|
| Strata | timed run | strata-0 | 1730 | 1301 | 1.33 | 61 |
| Strata | timed run | strata-1 | 1562 | 1301 | 1.20 | 71 |
| Strata | timed run | strata-coordinator | 128 | 1301 | 0.10 | 0 |
| Strata | writes | strata-0 | 49 | 71 | 0.68 | 2 |
| Strata | writes | strata-1 | 45 | 71 | 0.63 | 1 |
| Strata | writes | strata-coordinator | 25 | 71 | 0.35 | 0 |
| Strata | stream | strata-0 | 88 | 154 | 0.57 | 3 |
| Strata | stream | strata-1 | 82 | 153 | 0.54 | 3 |
| Strata | stream | strata-coordinator | 29 | 153 | 0.19 | 0 |
| Elasticsearch | load | es-0 | 19497 | 8186 | 2.38 | 1331 |
| Elasticsearch | load | es-1 | 14517 | 8186 | 1.77 | 395 |
| Elasticsearch | timed run | es-0 | 876 | 451 | 1.94 | 19 |
| Elasticsearch | timed run | es-1 | 807 | 451 | 1.79 | 7 |
| Elasticsearch | writes | es-0 | 169 | 103 | 1.64 | 4 |
| Elasticsearch | writes | es-1 | 123 | 103 | 1.20 | 2 |
| Elasticsearch | stream | es-0 | 180 | 152 | 1.18 | 1 |
| Elasticsearch | stream | es-1 | 150 | 151 | 0.99 | 0 |
| Elasticsearch, trial licence | timed run | es-0 | 143 | 81 | 1.77 | 4 |
| Elasticsearch, trial licence | timed run | es-1 | 129 | 80 | 1.61 | 1 |
| OpenSearch | load | os-0 | 19087 | 9740 | 1.96 | 453 |
| OpenSearch | load | os-1 | 22124 | 9741 | 2.27 | 998 |
| OpenSearch | timed run | os-0 | 888 | 491 | 1.81 | 5 |
| OpenSearch | timed run | os-1 | 944 | 491 | 1.92 | 13 |
| OpenSearch | writes | os-0 | 126 | 105 | 1.20 | 1 |
| OpenSearch | writes | os-1 | 147 | 105 | 1.40 | 2 |
| OpenSearch | stream | os-0 | 167 | 154 | 1.09 | 0 |
| OpenSearch | stream | os-1 | 194 | 154 | 1.26 | 1 |
| Solr | load | solr-0 | 33320 | 15516 | 2.15 | 2576 |
| Solr | load | solr-1 | 34747 | 15516 | 2.24 | 2148 |
| Solr | timed run | solr-0 | 570 | 5021 | 0.11 | 3 |
| Solr | timed run | solr-1 | 5334 | 5021 | 1.06 | 42 |
| Vespa | load | vespa-0 | 13120 | 13137 | 1.00 | 54531 |
| Vespa | load | vespa-1 | 21601 | 13137 | 1.64 | 310 |
| Vespa | load | vespa-2 | 20241 | 13137 | 1.54 | 229 |
| Vespa | timed run | vespa-0 | 231 | 533 | 0.43 | 145 |
| Vespa | timed run | vespa-1 | 836 | 533 | 1.57 | 90 |
| Vespa | timed run | vespa-2 | 770 | 533 | 1.45 | 38 |
| Vespa | writes | vespa-0 | 71 | 91 | 0.78 | 112 |
| Vespa | writes | vespa-1 | 70 | 91 | 0.77 | 0 |
| Vespa | writes | vespa-2 | 65 | 91 | 0.71 | 0 |
| Vespa | stream | vespa-0 | 152 | 202 | 0.75 | 298 |
| Vespa | stream | vespa-1 | 203 | 202 | 1.01 | 0 |
| Vespa | stream | vespa-2 | 188 | 202 | 0.93 | 0 |
| Qdrant | load | qdrant-0 | 260808 | 46329 | 5.63 | 991 |
| Qdrant | timed run | qdrant-0 | 2248 | 449 | 5.01 | 22 |
| Weaviate | load | weaviate-0 | 46710 | 7098 | 6.58 | 318 |
| Weaviate | timed run | weaviate-0 | 2214 | 509 | 4.35 | 7 |
| Milvus | load | milvus-0 | 41880 | 7248 | 5.78 | 18765 |
| Milvus | timed run | milvus-0 | 1823 | 305 | 5.98 | 387 |
| ParadeDB | load | paradedb-0 | 7381 | 2586 | 2.85 | 103 |
| ParadeDB | timed run | paradedb-0 | 9489 | 5072 | 1.87 | 154 |
| Postgres | load | postgres-0 | 86433 | 29769 | 2.90 | 58 |
| Postgres | timed run | postgres-0 | 11200 | 4924 | 2.27 | 39 |
| Redis | load | redis-0 | 17200 | 4091 | 4.20 | 12 |
| Redis | timed run | redis-0 | 5243 | 4304 | 1.22 | 7 |

## Memory of each engine's containers (cgroup memory.current, memory.stat, memory.peak; GB, all its pods)

| engine | after the load | of it anonymous | after the timed run | of it anonymous | of it files (page cache) | pods' peaks added |
|---|---|---|---|---|---|---|
| Strata | – | – | 32.84 | 2.37 | 30.36 | 34.53 |
| Elasticsearch | 38.94 | 30.14 | 41.56 | 30.68 | 10.53 | 55.84 |
| Elasticsearch, trial licence | – | – | 41.61 | 30.73 | 10.54 | 55.84 |
| OpenSearch | 39.24 | 30.22 | 42.21 | 30.60 | 11.09 | 55.84 |
| Solr | 38.54 | 18.79 | 54.24 | 19.12 | 34.69 | 55.85 |
| Vespa | 40.60 | 8.40 | 41.80 | 8.17 | 32.56 | 44.63 |
| Meilisearch | – | – | – | – | – | 0.12 |
| Typesense | – | – | – | – | – | 0.18 |
| Manticore | – | – | – | – | – | 0.03 |
| Qdrant | 50.71 | 31.44 | 54.86 | 31.50 | 21.46 | 55.84 |
| Weaviate | 55.83 | 26.46 | 55.83 | 31.39 | 15.84 | 55.84 |
| Milvus | 117.28 | 69.68 | 116.06 | 68.51 | 44.65 | 118.12 |
| ParadeDB | 55.58 | 0.04 | 53.60 | 0.04 | 52.89 | 55.84 |
| Postgres | 54.50 | 0.03 | 54.23 | 0.03 | 53.12 | 55.84 |
| Redis | 77.50 | 77.34 | 77.60 | 77.42 | 0.00 | 80.04 |

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

- Weaviate, facet: unavailable: error sending request for url (http://weaviate-0.weaviate:8080/v1/graphql)

- Weaviate, phrase: unavailable: no phrase queries

- Weaviate, sql aggregate: unavailable: error sending request for url (http://weaviate-0.weaviate:8080/v1/graphql)

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

- Postgres, bm25: unavailable: ERROR 57014: canceling statement due to statement timeout

- Postgres, bm25 + filter: unavailable: ERROR 57014: canceling statement due to statement timeout

- Postgres, boosted (bm25 × decay): unavailable: ERROR 57014: canceling statement due to statement timeout

- Postgres, facet: unavailable: ERROR 57014: canceling statement due to statement timeout

- Postgres, hybrid (RRF): unavailable: ERROR 57014: canceling statement due to statement timeout

- Postgres, hybrid (weighted): unavailable: ERROR 57014: canceling statement due to statement timeout

- Postgres, phrase: unavailable: ERROR 57014: canceling statement due to statement timeout

- Postgres, prefix: unavailable: ERROR 57014: canceling statement due to statement timeout

- Postgres, typo: unavailable: no fuzzy matching in full-text search (pg_trgm compares whole strings, not the words of a tsvector)

- Postgres, bm25: ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Postgres, bm25 + filter: ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Postgres, boosted (bm25 × decay): ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Postgres, hybrid (RRF): ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Postgres, hybrid (weighted): ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Postgres, phrase: ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Postgres, prefix: ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Redis, boosted (bm25 × decay): unavailable: SEARCH_TIMEOUT Timeout limit was reached

- Redis, sql aggregate: unavailable: SEARCH_TIMEOUT Timeout limit was reached

- Redis, bm25: BM25STD over Redis's English stemming; typo: %word% (1 edit) or %%word%% (2) by Strata's 'auto'; boosted: FT.AGGREGATE with ADDSCORES and APPLY

- Redis, bm25 + filter: BM25STD over Redis's English stemming; typo: %word% (1 edit) or %%word%% (2) by Strata's 'auto'; boosted: FT.AGGREGATE with ADDSCORES and APPLY

- Redis, boosted (bm25 × decay): BM25STD over Redis's English stemming; typo: %word% (1 edit) or %%word%% (2) by Strata's 'auto'; boosted: FT.AGGREGATE with ADDSCORES and APPLY

- Redis, hybrid (RRF): FT.HYBRID, COMBINE RRF (window 100, constant 60)

- Redis, hybrid (weighted): FT.HYBRID, COMBINE LINEAR: ALPHA 0.2 x the raw BM25STD score + BETA 0.8 x the vector score (Redis 8.8 does not normalise them), over each top 100

- Redis, key lookup: HMGET of the document's hash (its own key-value read)

- Redis, phrase: BM25STD over Redis's English stemming; typo: %word% (1 edit) or %%word%% (2) by Strata's 'auto'; boosted: FT.AGGREGATE with ADDSCORES and APPLY

- Redis, prefix: BM25STD over Redis's English stemming; typo: %word% (1 edit) or %%word%% (2) by Strata's 'auto'; boosted: FT.AGGREGATE with ADDSCORES and APPLY

- Redis, typo: BM25STD over Redis's English stemming; typo: %word% (1 edit) or %%word%% (2) by Strata's 'auto'; boosted: FT.AGGREGATE with ADDSCORES and APPLY

## Load time and index size

| engine | load (s) | of which: send / build | searchable | merge to 1 segment | index size (GB) | documents |
|---|---|---|---|---|---|---|
| Strata | 5572 | load 6114 + merge 4969 | (built) | (built as one) | 49.75 | 10,000,000 |
| Elasticsearch | 9033 | send 5420 | 3 | 3610 | 48.56 | 10,000,000 |
| OpenSearch | 10947 | send 6559 | 3 | 4385 | 48.68 | 10,000,000 |

## 1 client: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | Elasticsearch, trial licence q/s | Elasticsearch, trial licence ms | OpenSearch q/s | OpenSearch ms |
|---|---|---|---|---|---|---|---|---|
| bm25 | 231 | 3.24 / 7.33 / 20.20 | 158 | 5.20 / 9.97 / 21.03 | n/a | n/a | 191 | 4.23 / 8.78 / 18.68 |
| bm25 + filter | 206 | 3.76 / 8.37 / 22.16 | 111 | 6.74 / 14.61 / 49.05 | n/a | n/a | 116 | 6.35 / 14.53 / 50.47 |
| phrase | 507 | 1.87 / 2.36 / 3.69 | 146 | 4.86 / 12.80 / 23.65 | n/a | n/a | 155 | 4.44 / 12.22 / 25.25 |
| typo | 17 | 55.06 / 88.22 / 136.73 | 28 | 32.39 / 58.27 / 100.92 | n/a | n/a | 21 | 44.37 / 75.39 / 109.10 |
| prefix | 157 | 4.62 / 11.97 / 28.01 | 92 | 8.44 / 18.89 / 48.34 | n/a | n/a | 68 | 10.19 / 26.83 / 65.80 |
| vector | 366 | 2.64 / 3.11 / 4.36 | 232 | 4.23 / 4.88 / 6.37 | n/a | n/a | 213 | 4.53 / 5.62 / 7.65 |
| vector + filter | 353 | 2.75 / 3.22 / 4.31 | 58 | 5.39 / 6.83 / 229.99 | n/a | n/a | 117 | 8.43 / 10.21 / 12.71 |
| hybrid (RRF) | 108 | 7.75 / 14.09 / 35.19 | n/a | n/a | 75 | 11.77 / 18.72 / 36.69 | 61 | 12.83 / 26.41 / 62.66 |
| hybrid (weighted) | 106 | 7.89 / 14.31 / 30.73 | 41 | 14.51 / 50.08 / 129.95 | 26 | 30.95 / 83.32 / 118.45 | 62 | 12.76 / 26.88 / 65.08 |
| boosted (bm25 × decay) | 96 | 8.07 / 18.73 / 39.83 | 18 | 43.64 / 122.97 / 187.37 | n/a | n/a | 18 | 43.57 / 139.77 / 189.81 |
| similar items | 249 | 3.83 / 4.77 / 7.15 | 30 | 27.50 / 29.04 / 406.47 | n/a | n/a | 31 | 31.80 / 33.58 / 41.96 |
| facet | 344 | 2.60 / 4.35 / 6.36 | 179 | 4.21 / 10.10 / 25.47 | n/a | n/a | 167 | 4.03 / 12.11 / 37.22 |
| key lookup | 1,082 | 0.99 / 1.19 / 1.48 | 899 | 1.09 / 1.44 / 1.67 | n/a | n/a | 1,158 | 0.86 / 1.11 / 1.37 |
| sql aggregate | 28 | 35.94 / 51.04 / 55.00 | 2 | 490.38 / 574.31 / 584.74 | n/a | n/a | 2 | 651.04 / 713.08 / 722.07 |


## 4 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | Elasticsearch, trial licence q/s | Elasticsearch, trial licence ms | OpenSearch q/s | OpenSearch ms |
|---|---|---|---|---|---|---|---|---|
| bm25 | 525 | 5.43 / 14.33 / 41.75 | 420 | 8.09 / 15.60 / 31.23 | n/a | n/a | 525 | 6.28 / 12.76 / 27.05 |
| bm25 + filter | 460 | 6.21 / 16.23 / 42.59 | 279 | 10.98 / 23.44 / 78.68 | n/a | n/a | 287 | 10.33 / 24.60 / 79.83 |
| phrase | 1,596 | 2.32 / 3.43 / 5.04 | 367 | 8.28 / 21.63 / 37.91 | n/a | n/a | 379 | 7.72 / 20.65 / 38.42 |
| typo | 36 | 103.87 / 168.27 / 249.90 | 66 | 54.92 / 98.30 / 153.11 | n/a | n/a | 50 | 74.71 / 124.18 / 181.75 |
| prefix | 294 | 9.78 / 27.38 / 56.07 | 228 | 14.51 / 29.60 / 73.00 | n/a | n/a | 153 | 19.67 / 46.17 / 127.20 |
| vector | 897 | 4.29 / 5.69 / 7.66 | 704 | 5.55 / 6.99 / 9.01 | n/a | n/a | 574 | 6.52 / 9.14 / 12.55 |
| vector + filter | 840 | 4.60 / 6.08 / 7.92 | 177 | 8.19 / 12.49 / 316.83 | n/a | n/a | 312 | 12.49 / 16.81 / 21.01 |
| hybrid (RRF) | 229 | 13.83 / 28.74 / 71.93 | n/a | n/a | 177 | 20.32 / 31.95 / 60.96 | 151 | 20.57 / 43.62 / 110.71 |
| hybrid (weighted) | 226 | 14.25 / 29.44 / 71.52 | 98 | 25.60 / 85.21 / 218.12 | 60 | 55.53 / 134.38 / 196.75 | 153 | 20.36 / 42.56 / 103.75 |
| boosted (bm25 × decay) | 209 | 14.75 / 35.94 / 75.15 | 41 | 70.11 / 256.80 / 394.38 | n/a | n/a | 39 | 69.07 / 244.70 / 375.42 |
| similar items | 565 | 6.78 / 9.18 / 13.24 | 72 | 49.00 / 58.26 / 495.13 | n/a | n/a | 68 | 58.19 / 70.12 / 81.16 |
| facet | 1,002 | 3.60 / 6.42 / 9.68 | 478 | 6.02 / 15.68 / 42.57 | n/a | n/a | 399 | 6.71 / 20.18 / 63.96 |
| key lookup | 4,127 | 1.00 / 1.29 / 1.77 | 3,546 | 1.12 / 1.48 / 1.87 | n/a | n/a | 4,695 | 0.85 / 1.15 / 1.48 |
| sql aggregate | 40 | 99.44 / 148.98 / 167.78 | 5 | 862.19 / 1167.50 / 1238.29 | n/a | n/a | 3 | 1174.58 / 1519.66 / 1672.19 |


## 16 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | Elasticsearch, trial licence q/s | Elasticsearch, trial licence ms | OpenSearch q/s | OpenSearch ms |
|---|---|---|---|---|---|---|---|---|
| bm25 | 667 | 16.55 / 47.83 / 134.99 | 585 | 23.57 / 45.35 / 88.67 | n/a | n/a | 720 | 18.70 / 36.51 / 83.15 |
| bm25 + filter | 539 | 19.80 / 59.55 / 180.13 | 368 | 36.72 / 70.20 / 182.76 | n/a | n/a | 358 | 37.12 / 72.81 / 191.75 |
| phrase | 2,901 | 5.03 / 8.26 / 12.89 | 468 | 29.45 / 63.10 / 100.91 | n/a | n/a | 438 | 30.17 / 63.99 / 172.42 |
| typo | 35 | 404.63 / 754.28 / 1126.08 | 66 | 221.27 / 344.12 / 566.03 | n/a | n/a | 50 | 308.84 / 435.73 / 584.24 |
| prefix | 320 | 33.71 / 111.16 / 224.32 | 274 | 52.00 / 90.52 / 183.45 | n/a | n/a | 182 | 70.63 / 144.14 / 289.80 |
| vector | 1,344 | 11.21 / 17.20 / 24.81 | 1,292 | 11.95 / 16.27 / 21.71 | n/a | n/a | 1,028 | 14.54 / 21.44 / 28.99 |
| vector + filter | 1,173 | 12.74 / 20.11 / 28.74 | 268 | 27.78 / 52.96 / 718.62 | n/a | n/a | 424 | 36.00 / 50.32 / 64.36 |
| hybrid (RRF) | 251 | 48.88 / 110.00 / 309.52 | n/a | n/a | 239 | 63.18 / 90.86 / 152.52 | 168 | 85.07 / 146.37 / 282.21 |
| hybrid (weighted) | 250 | 49.84 / 111.87 / 286.64 | 103 | 123.76 / 270.32 / 546.47 | 62 | 228.38 / 402.69 / 569.71 | 170 | 84.03 / 137.95 / 311.97 |
| boosted (bm25 × decay) | 222 | 50.65 / 148.71 / 364.75 | 39 | 356.40 / 716.16 / 1027.20 | n/a | n/a | 38 | 379.63 / 682.78 / 973.68 |
| similar items | 772 | 19.63 / 30.43 / 40.65 | 73 | 216.27 / 293.33 / 608.34 | n/a | n/a | 63 | 246.67 / 334.61 / 363.61 |
| facet | 1,586 | 9.07 / 16.55 / 25.30 | 638 | 20.58 / 43.78 / 110.40 | n/a | n/a | 513 | 24.56 / 55.42 / 159.18 |
| key lookup | 12,081 | 1.31 / 1.96 / 2.72 | 9,059 | 1.66 / 2.52 / 3.53 | n/a | n/a | 12,285 | 1.19 / 1.93 / 2.98 |
| sql aggregate | 37 | 388.85 / 721.75 / 811.96 | 5 | 3523.53 / 4344.87 / 5146.83 | n/a | n/a | 4 | 3414.05 / 5199.09 / 5722.53 |


Errors during the timed runs: Strata 0, Elasticsearch 0, Elasticsearch, trial licence 0, OpenSearch 0

## Answers: recall@10 against an exact search, nDCG@10 against the collection's judgments

Unavailable kinds are n/a; recall is for the kinds an exact search can answer.

| kind | Strata recall | Strata nDCG | Elasticsearch recall | Elasticsearch nDCG | Elasticsearch, trial licence recall | Elasticsearch, trial licence nDCG | OpenSearch recall | OpenSearch nDCG |
|---|---|---|---|---|---|---|---|---|
| bm25 | – | – | – | – | n/a | n/a | – | – |
| bm25 + filter | – | – | – | – | n/a | n/a | – | – |
| phrase | – | – | – | – | n/a | n/a | – | – |
| typo | – | – | – | – | n/a | n/a | – | – |
| prefix | – | – | – | – | n/a | n/a | – | – |
| vector | 0.798 | – | 0.665 | – | n/a | n/a | 0.637 | – |
| vector + filter | 0.816 | – | 0.605 | – | n/a | n/a | 0.787 | – |
| hybrid (RRF) | – | – | n/a | n/a | – | – | – | – |
| hybrid (weighted) | – | – | – | – | – | – | – | – |
| boosted (bm25 × decay) | – | – | – | – | n/a | n/a | – | – |
| similar items | 0.981 | – | 0.905 | – | n/a | n/a | 0.908 | – |


## SQL aggregates equal to the true answer

| Strata | Elasticsearch | Elasticsearch, trial licence | OpenSearch |
|---|---|---|---|
| 300 of 300 | 300 of 300 | n/a | 300 of 300 |

## Agreement with Strata's answers

| kind | Elasticsearch | Elasticsearch, trial licence | OpenSearch |
|---|---|---|---|
| bm25 | 0.978 of the top 10 | n/a | 0.972 of the top 10 |
| bm25 + filter | 0.981 of the top 10 | n/a | 0.982 of the top 10 |
| phrase | 0.657 of the top 10 | n/a | 0.660 of the top 10 |
| typo | 0.443 of the top 10 | n/a | 0.442 of the top 10 |
| prefix | 0.977 of the top 10 | n/a | 0.976 of the top 10 |
| vector | 0.641 of the top 10 | n/a | 0.604 of the top 10 |
| vector + filter | 0.610 of the top 10 | n/a | 0.751 of the top 10 |
| hybrid (RRF) | n/a | 0.700 of the top 10 | 0.706 of the top 10 |
| hybrid (weighted) | 0.449 of the top 10 | 0.654 of the top 10 | 0.685 of the top 10 |
| boosted (bm25 × decay) | 0.982 of the top 10 | n/a | 0.985 of the top 10 |
| similar items | 0.901 of the top 10 | n/a | 0.900 of the top 10 |
| facet | 272 of 300 equal | n/a | 271 of 300 equal |
| key lookup | 300 of 300 equal | n/a | 300 of 300 equal |


## Writes: documents a second, and latency in ms (p50 / p99)

| phase | clients | Strata docs/s | Strata ms | Elasticsearch docs/s | Elasticsearch ms | OpenSearch docs/s | OpenSearch ms |
|---|---|---|---|---|---|---|---|
| insert one | 1 | 407 | 2.25 / 3.82 | 323 | 2.83 / 7.85 | 270 | 3.09 / 10.93 |
| insert one | 4 | 1,317 | 2.40 / 8.96 | 962 | 3.61 / 9.25 | 866 | 3.60 / 15.06 |
| insert one | 16 | 2,418 | 3.69 / 69.62 | 2,283 | 5.84 / 22.26 | 1,655 | 7.46 / 41.03 |
| insert batch | 1 | 1,546 | 51.05 / 115.57 | 1,498 | 45.39 / 263.07 | 1,178 | 68.40 / 134.60 |
| insert batch | 4 | 2,872 | 104.76 / 356.40 | 3,680 | 72.74 / 520.28 | 2,418 | 138.35 / 344.91 |
| insert batch | 16 | 3,172 | 423.47 / 1219.18 | 3,430 | 372.41 / 955.59 | 3,098 | 459.60 / 755.87 |
| update one | 1 | 462 | 1.85 / 7.76 | 263 | 3.18 / 10.42 | 257 | 3.13 / 12.44 |
| update one | 4 | 2,290 | 1.66 / 3.45 | 881 | 3.81 / 11.31 | 865 | 3.74 / 21.26 |
| update one | 16 | 6,730 | 2.08 / 5.27 | 1,858 | 7.00 / 30.29 | 1,964 | 6.77 / 29.24 |

## How soon a written document is found (ms from the write's start)

| engine | searchable | write returned (p50) | found (p50) | found (p90) | found (max) |
|---|---|---|---|---|---|
| Strata | searchable when the write returns | 2.6 | 6.7 | 9.4 | 133.4 |
| Elasticsearch | refresh_interval 1s (the default; loading turned it off) | 5.3 | 730.9 | 1018.1 | 1167.6 |
| OpenSearch | refresh_interval 1s (the default; loading turned it off) | 5.2 | 866.7 | 1019.3 | 1033.5 |

## Streams through Redpanda: documents written a second, and lag in ms from when a document was due (p50 / p99)

| phase | Strata docs/s | Strata lag | Elasticsearch docs/s | Elasticsearch lag | OpenSearch docs/s | OpenSearch lag |
|---|---|---|---|---|---|---|
| 250/s | 250 | 21 / 56 | 250 | 23 / 42 | 250 | 22 / 39 |
| 500/s | 499 | 23 / 183 | 499 | 22 / 46 | 499 | 22 / 50 |
| 1000/s | 999 | 27 / 143 | 999 | 25 / 86 | 999 | 26 / 604 |
| 2000/s | 1,998 | 392 / 2082 | 1,992 | 33 / 182 | 1,995 | 43 / 744 |
| flat out | 2,801 | 3903 / 6504 | 3,311 | 2583 / 5326 | 2,240 | 3354 / 7057 |

Lag includes reading the topic (Redpanda's part, the same for every engine, is the 'read' column below).

| engine | what written means | read lag p50 (ms) | crash: events written again | documents lost |
|---|---|---|---|---|
| Strata | searchable (the server answers a batch once its rows are visible) | 18 | 0 (the consumer resumes after the offsets the server keeps with the rows (GET /v1/sources)) | 0 |
| Elasticsearch | written: searchable within the 1 s refresh | 18 | 833 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |
| OpenSearch | written: searchable within the 1 s refresh | 18 | 459 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |

## CPU used by each engine's containers (cgroup cpu.stat)

| engine | phase | pod | CPU seconds | wall seconds | average cores | throttled (s) |
|---|---|---|---|---|---|---|
| Strata | timed run | strata-0 | 893 | 413 | 2.16 | 23 |
| Strata | timed run | strata-1 | 815 | 414 | 1.97 | 7 |
| Strata | writes | strata-0 | 94 | 70 | 1.34 | 1 |
| Strata | writes | strata-1 | 84 | 71 | 1.19 | 1 |
| Strata | stream | strata-0 | 179 | 156 | 1.15 | 2 |
| Strata | stream | strata-1 | 170 | 156 | 1.09 | 2 |
| Elasticsearch | load | es-0 | 20798 | 9043 | 2.30 | 1697 |
| Elasticsearch | load | es-1 | 18451 | 9043 | 2.04 | 888 |
| Elasticsearch | timed run | es-0 | 996 | 552 | 1.81 | 29 |
| Elasticsearch | timed run | es-1 | 909 | 552 | 1.65 | 11 |
| Elasticsearch | writes | es-0 | 156 | 103 | 1.52 | 3 |
| Elasticsearch | writes | es-1 | 129 | 103 | 1.26 | 2 |
| Elasticsearch | stream | es-0 | 164 | 151 | 1.09 | 1 |
| Elasticsearch | stream | es-1 | 153 | 151 | 1.01 | 1 |
| Elasticsearch, trial licence | timed run | es-0 | 143 | 80 | 1.79 | 5 |
| Elasticsearch, trial licence | timed run | es-1 | 125 | 80 | 1.56 | 0 |
| OpenSearch | load | os-0 | 26506 | 10962 | 2.42 | 973 |
| OpenSearch | load | os-1 | 21212 | 10962 | 1.94 | 1023 |
| OpenSearch | timed run | os-0 | 1128 | 612 | 1.84 | 10 |
| OpenSearch | timed run | os-1 | 981 | 612 | 1.60 | 10 |
| OpenSearch | writes | os-0 | 165 | 107 | 1.54 | 1 |
| OpenSearch | writes | os-1 | 137 | 107 | 1.28 | 2 |
| OpenSearch | stream | os-0 | 196 | 158 | 1.24 | 2 |
| OpenSearch | stream | os-1 | 176 | 158 | 1.12 | 3 |

## Memory of each engine's containers (cgroup memory.current, memory.stat, memory.peak; GB, all its pods)

| engine | after the load | of it anonymous | after the timed run | of it anonymous | of it files (page cache) | pods' peaks added |
|---|---|---|---|---|---|---|
| Strata | – | – | 29.58 | 0.96 | 28.27 | 51.89 |
| Elasticsearch | 38.94 | 30.13 | 41.45 | 30.56 | 10.51 | 55.85 |
| Elasticsearch, trial licence | – | – | 41.69 | 30.67 | 10.63 | 55.85 |
| OpenSearch | 40.83 | 30.17 | 43.67 | 30.56 | 12.55 | 55.85 |

- Elasticsearch, hybrid (RRF): unavailable: {"license.expired.feature":"Reciprocal Rank Fusion (RRF)","reason":"current license is non-compliant for [Reciprocal Rank Fusion (RRF)]","root_cause":[{"license.expired.feature":"Reciprocal Rank Fusion (RRF)","reason":"current license is non-compliant for [Reciprocal Rank Fusion (RRF)]","type":"secu

- Elasticsearch, hybrid (RRF): the rrf retriever

- Elasticsearch, hybrid (weighted): no min-max normalised fusion on the free licence (the linear retriever needs a paid one): the sum of 0.2 x the raw BM25 score and 0.8 x the vector score ((1 + dot) / 2) over the lexical matches and the knn top 100

- Elasticsearch, trial licence, hybrid (RRF): the rrf retriever

- Elasticsearch, trial licence, hybrid (weighted): the linear retriever (a paid feature, here on a trial licence): match and knn top 100, each min-max normalised over the top 100, weighted 0.2 and 0.8

- OpenSearch, hybrid (RRF): a hybrid query (match, knn k 100, pagination depth 100) through a search pipeline's score-ranker-processor, rrf, k 60

- OpenSearch, hybrid (weighted): a hybrid query (match, knn k 100, pagination depth 100) through a search pipeline's normalization-processor: min_max, arithmetic_mean weighted 0.2 and 0.8

## Load time and index size

| engine | load (s) | of which: send / build | searchable | merge to 1 segment | index size (GB) | documents |
|---|---|---|---|---|---|---|
| Strata | 4056 | load 3919 + merge 3789 | (built) | (built as one) | 48.93 | 10,000,000 |
| Elasticsearch | 9178 | send 5038 | 3 | 4138 | 48.55 | 10,000,000 |
| OpenSearch | 11978 | send 7464 | 3 | 4511 | 48.57 | 10,000,000 |

## 1 client: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | Elasticsearch, trial licence q/s | Elasticsearch, trial licence ms | OpenSearch q/s | OpenSearch ms |
|---|---|---|---|---|---|---|---|---|
| bm25 | 244 | 2.92 / 7.61 / 20.52 | 128 | 6.70 / 11.91 / 25.01 | n/a | n/a | 203 | 3.90 / 7.75 / 17.82 |
| bm25 + filter | 124 | 6.37 / 15.85 / 30.77 | 93 | 8.28 / 16.70 / 55.19 | n/a | n/a | 122 | 5.91 / 13.56 / 44.82 |
| phrase | 266 | 2.37 / 7.37 / 16.52 | 120 | 6.17 / 15.62 / 27.46 | n/a | n/a | 154 | 4.36 / 12.41 / 23.35 |
| typo | 12 | 81.72 / 114.41 / 142.34 | 25 | 37.75 / 63.25 / 104.23 | n/a | n/a | 22 | 43.62 / 74.36 / 102.89 |
| prefix | 93 | 8.99 / 17.91 / 36.35 | 78 | 10.19 / 21.38 / 56.55 | n/a | n/a | 73 | 9.57 / 25.32 / 63.76 |
| vector | 463 | 2.11 / 2.43 / 3.17 | 192 | 5.04 / 6.41 / 8.41 | n/a | n/a | 211 | 4.50 / 5.60 / 7.65 |
| vector + filter | 331 | 2.93 / 3.55 / 4.80 | 46 | 6.59 / 9.12 / 296.17 | n/a | n/a | 115 | 8.57 / 10.58 / 13.66 |
| hybrid (RRF) | 107 | 7.73 / 15.04 / 35.76 | n/a | n/a | 63 | 14.53 / 22.57 / 37.34 | 71 | 11.53 / 21.05 / 54.82 |
| hybrid (weighted) | 106 | 7.85 / 14.65 / 37.35 | 34 | 18.07 / 66.34 / 142.21 | 24 | 34.02 / 88.53 / 116.61 | 64 | 12.28 / 26.71 / 63.41 |
| boosted (bm25 × decay) | 61 | 12.73 / 30.75 / 68.29 | 15 | 58.36 / 152.90 / 223.19 | n/a | n/a | 19 | 42.01 / 134.04 / 178.96 |
| similar items | 272 | 3.60 / 4.29 / 5.53 | 24 | 38.38 / 45.45 / 58.68 | n/a | n/a | 26 | 37.20 / 46.25 / 55.24 |
| facet | 323 | 2.24 / 5.83 / 10.96 | 155 | 4.91 / 11.29 / 28.71 | n/a | n/a | 213 | 3.26 / 9.10 / 28.10 |
| key lookup | 1,268 | 0.76 / 0.88 / 1.29 | 822 | 1.24 / 1.64 / 2.11 | n/a | n/a | 1,195 | 0.86 / 1.09 / 1.44 |
| sql aggregate | 0 | 2664.25 / 2743.57 / 2743.57 | 91 | 10.42 / 14.02 / 19.60 | n/a | n/a | 217 | 4.55 / 5.42 / 7.24 |


## 4 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | Elasticsearch, trial licence q/s | Elasticsearch, trial licence ms | OpenSearch q/s | OpenSearch ms |
|---|---|---|---|---|---|---|---|---|
| bm25 | 506 | 5.25 / 15.87 / 44.10 | 329 | 10.20 / 20.65 / 43.86 | n/a | n/a | 572 | 5.74 / 11.61 / 23.94 |
| bm25 + filter | 236 | 13.23 / 34.53 / 71.97 | 238 | 12.99 / 28.57 / 87.29 | n/a | n/a | 321 | 9.41 / 22.47 / 66.38 |
| phrase | 548 | 4.49 / 15.62 / 37.43 | 306 | 9.80 / 26.35 / 49.28 | n/a | n/a | 391 | 7.56 / 20.46 / 35.11 |
| typo | 25 | 159.33 / 225.72 / 336.74 | 55 | 67.11 / 110.87 / 199.52 | n/a | n/a | 50 | 74.66 / 129.39 / 275.04 |
| prefix | 167 | 20.09 / 43.60 / 82.06 | 191 | 16.86 / 34.33 / 87.20 | n/a | n/a | 187 | 15.95 / 36.41 / 100.38 |
| vector | 1,128 | 3.40 / 4.58 / 5.92 | 528 | 6.85 / 9.28 / 14.04 | n/a | n/a | 562 | 6.63 / 9.34 / 12.61 |
| vector + filter | 732 | 5.21 / 7.25 / 9.82 | 129 | 9.98 / 16.93 / 354.20 | n/a | n/a | 304 | 12.54 / 16.90 / 21.45 |
| hybrid (RRF) | 218 | 14.41 / 31.02 / 83.24 | n/a | n/a | 150 | 24.14 / 38.63 / 67.23 | 171 | 19.43 / 36.55 / 95.93 |
| hybrid (weighted) | 217 | 14.63 / 31.61 / 81.93 | 79 | 31.30 / 112.14 / 263.71 | 54 | 65.43 / 146.02 / 230.60 | 164 | 19.63 / 41.14 / 100.76 |
| boosted (bm25 × decay) | 104 | 27.86 / 75.01 / 175.19 | 35 | 75.87 / 296.93 / 453.16 | n/a | n/a | 42 | 63.72 / 232.19 / 358.19 |
| similar items | 576 | 6.69 / 8.99 / 11.85 | 57 | 62.83 / 76.78 / 460.39 | n/a | n/a | 65 | 60.48 / 71.73 / 85.99 |
| facet | 860 | 3.54 / 8.94 / 16.50 | 412 | 7.42 / 18.25 / 45.71 | n/a | n/a | 547 | 5.07 / 13.72 / 43.25 |
| key lookup | 4,469 | 0.86 / 1.09 / 1.45 | 3,241 | 1.23 / 1.66 / 2.30 | n/a | n/a | 4,799 | 0.84 / 1.12 / 1.44 |
| sql aggregate | 1 | 6035.66 / 6239.03 / 6239.03 | 293 | 12.68 / 18.80 / 27.09 | n/a | n/a | 699 | 5.46 / 7.24 / 10.39 |


## 16 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | Elasticsearch, trial licence q/s | Elasticsearch, trial licence ms | OpenSearch q/s | OpenSearch ms |
|---|---|---|---|---|---|---|---|---|
| bm25 | 482 | 17.96 / 72.76 / 214.14 | 464 | 30.71 / 55.32 / 101.22 | n/a | n/a | 817 | 16.95 / 32.14 / 62.49 |
| bm25 + filter | 228 | 48.60 / 156.85 / 318.74 | 277 | 49.46 / 89.69 / 223.66 | n/a | n/a | 391 | 33.97 / 66.46 / 180.75 |
| phrase | 589 | 13.11 / 67.97 / 156.39 | 325 | 41.41 / 90.59 / 154.78 | n/a | n/a | 506 | 27.36 / 57.51 / 98.87 |
| typo | 24 | 656.27 / 1017.16 / 1290.70 | 57 | 243.98 / 445.22 / 614.62 | n/a | n/a | 53 | 279.93 / 417.56 / 615.58 |
| prefix | 154 | 84.62 / 197.75 / 429.14 | 204 | 66.82 / 123.75 / 240.54 | n/a | n/a | 181 | 76.81 / 144.49 / 293.83 |
| vector | 1,508 | 9.32 / 16.21 / 27.75 | 932 | 15.78 / 24.21 / 33.99 | n/a | n/a | 988 | 15.41 / 23.23 / 30.59 |
| vector + filter | 816 | 16.66 / 35.58 / 49.33 | 167 | 44.70 / 184.99 / 805.86 | n/a | n/a | 389 | 39.14 / 53.86 / 72.17 |
| hybrid (RRF) | 201 | 59.91 / 151.84 / 388.82 | n/a | n/a | 181 | 81.94 / 123.30 / 207.52 | 200 | 71.16 / 121.81 / 243.89 |
| hybrid (weighted) | 201 | 61.00 / 157.30 / 400.09 | 79 | 162.02 / 342.02 / 670.04 | 56 | 258.49 / 452.36 / 743.84 | 209 | 67.49 / 113.10 / 236.29 |
| boosted (bm25 × decay) | 94 | 109.75 / 352.06 / 885.48 | 34 | 399.64 / 747.80 / 1108.20 | n/a | n/a | 41 | 320.38 / 677.76 / 1013.29 |
| similar items | 604 | 23.14 / 45.64 / 61.20 | 62 | 246.98 / 343.38 / 517.98 | n/a | n/a | 65 | 245.34 / 320.92 / 361.89 |
| facet | 1,088 | 10.23 / 31.27 / 56.27 | 480 | 28.72 / 56.01 / 131.88 | n/a | n/a | 687 | 18.35 / 40.61 / 123.63 |
| key lookup | 6,936 | 2.23 / 2.95 / 3.71 | 7,977 | 1.87 / 3.06 / 4.80 | n/a | n/a | 13,095 | 1.13 / 1.81 / 2.69 |
| sql aggregate | 1 | 19789.17 / 20402.18 / 20704.64 | 617 | 23.53 / 40.43 / 57.54 | n/a | n/a | 1,121 | 14.14 / 21.56 / 27.34 |


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
| vector | 0.782 | – | 0.680 | – | n/a | n/a | 0.714 | – |
| vector + filter | 0.824 | – | 0.655 | – | n/a | n/a | 0.854 | – |
| hybrid (RRF) | – | – | n/a | n/a | – | – | – | – |
| hybrid (weighted) | – | – | – | – | – | – | – | – |
| boosted (bm25 × decay) | – | – | – | – | n/a | n/a | – | – |
| similar items | 0.968 | – | 0.910 | – | n/a | n/a | 0.951 | – |


## SQL aggregates equal to the true answer

| Strata | Elasticsearch | Elasticsearch, trial licence | OpenSearch |
|---|---|---|---|
| 300 of 300 | 300 of 300 | n/a | 300 of 300 |

## Agreement with Strata's answers

| kind | Elasticsearch | Elasticsearch, trial licence | OpenSearch |
|---|---|---|---|
| bm25 | 0.978 of the top 10 | n/a | 0.971 of the top 10 |
| bm25 + filter | 0.981 of the top 10 | n/a | 0.981 of the top 10 |
| phrase | 0.657 of the top 10 | n/a | 0.659 of the top 10 |
| typo | 0.446 of the top 10 | n/a | 0.445 of the top 10 |
| prefix | 0.977 of the top 10 | n/a | 0.976 of the top 10 |
| vector | 0.646 of the top 10 | n/a | 0.672 of the top 10 |
| vector + filter | 0.649 of the top 10 | n/a | 0.799 of the top 10 |
| hybrid (RRF) | n/a | 0.724 of the top 10 | 0.740 of the top 10 |
| hybrid (weighted) | 0.450 of the top 10 | 0.661 of the top 10 | 0.738 of the top 10 |
| boosted (bm25 × decay) | 0.982 of the top 10 | n/a | 0.985 of the top 10 |
| similar items | 0.903 of the top 10 | n/a | 0.931 of the top 10 |
| facet | 272 of 300 equal | n/a | 271 of 300 equal |
| key lookup | 300 of 300 equal | n/a | 300 of 300 equal |


## Writes: documents a second, and latency in ms (p50 / p99)

| phase | clients | Strata docs/s | Strata ms | Elasticsearch docs/s | Elasticsearch ms | OpenSearch docs/s | OpenSearch ms |
|---|---|---|---|---|---|---|---|
| insert one | 1 | 405 | 2.19 / 3.86 | 219 | 4.02 / 9.70 | 225 | 4.06 / 11.10 |
| insert one | 4 | 1,316 | 2.64 / 5.34 | 646 | 5.54 / 15.26 | 700 | 4.89 / 16.94 |
| insert one | 16 | 2,282 | 4.79 / 81.34 | 1,884 | 7.15 / 33.25 | 1,556 | 7.80 / 41.96 |
| insert batch | 1 | 2,496 | 26.39 / 123.81 | 1,645 | 50.89 / 93.33 | 1,266 | 67.00 / 135.23 |
| insert batch | 4 | 3,815 | 73.12 / 396.45 | 3,185 | 81.86 / 515.29 | 2,760 | 120.48 / 297.51 |
| insert batch | 16 | 4,297 | 317.63 / 716.04 | 3,312 | 418.59 / 1168.47 | 2,946 | 510.18 / 714.29 |
| update one | 1 | 564 | 1.99 / 2.81 | 161 | 5.09 / 15.19 | 234 | 3.85 / 10.70 |
| update one | 4 | 1,850 | 2.00 / 5.27 | 636 | 5.55 / 16.50 | 826 | 4.39 / 11.07 |
| update one | 16 | 6,042 | 2.51 / 4.98 | 1,414 | 8.48 / 41.37 | 1,837 | 7.52 / 29.27 |

## How soon a written document is found (ms from the write's start)

| engine | searchable | write returned (p50) | found (p50) | found (p90) | found (max) |
|---|---|---|---|---|---|
| Strata | searchable when the write returns | 2.9 | 6.5 | 8.2 | 120.3 |
| Elasticsearch | refresh_interval 1s (the default; loading turned it off) | 6.6 | 557.2 | 1017.3 | 1105.3 |
| OpenSearch | refresh_interval 1s (the default; loading turned it off) | 6.2 | 800.8 | 1015.9 | 1023.1 |

## Streams through Redpanda: documents written a second, and lag in ms from when a document was due (p50 / p99)

| phase | Strata docs/s | Strata lag | Elasticsearch docs/s | Elasticsearch lag | OpenSearch docs/s | OpenSearch lag |
|---|---|---|---|---|---|---|
| 250/s | 249 | 78 / 114 | 250 | 24 / 56 | 250 | 24 / 42 |
| 500/s | 499 | 77 / 123 | 498 | 26 / 62 | 499 | 24 / 51 |
| 1000/s | 997 | 78 / 177 | 998 | 30 / 106 | 999 | 27 / 71 |
| 2000/s | 1,993 | 86 / 403 | 1,985 | 35 / 471 | 1,994 | 36 / 362 |
| flat out | 3,799 | 1540 / 4562 | 2,972 | 2848 / 5980 | 2,935 | 3425 / 6172 |

Lag includes reading the topic (Redpanda's part, the same for every engine, is the 'read' column below).

| engine | what written means | read lag p50 (ms) | crash: events written again | documents lost |
|---|---|---|---|---|
| Strata | searchable (the server answers a batch once its rows are visible) | 34 | 0 (the consumer resumes after the offsets the server keeps with the rows (GET /v1/sources)) | 0 |
| Elasticsearch | written: searchable within the 1 s refresh | 18 | 421 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |
| OpenSearch | written: searchable within the 1 s refresh | 19 | 545 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |

## CPU used by each engine's containers (cgroup cpu.stat)

| engine | phase | pod | CPU seconds | wall seconds | average cores | throttled (s) |
|---|---|---|---|---|---|---|
| Strata | timed run | strata-0 | 1415 | 993 | 1.42 | 100 |
| Strata | timed run | strata-1 | 1167 | 993 | 1.18 | 4 |
| Strata | timed run | strata-coordinator | 110 | 993 | 0.11 | 0 |
| Strata | writes | strata-0 | 50 | 70 | 0.71 | 2 |
| Strata | writes | strata-1 | 46 | 70 | 0.65 | 1 |
| Strata | writes | strata-coordinator | 25 | 70 | 0.36 | 0 |
| Strata | stream | strata-0 | 76 | 153 | 0.50 | 3 |
| Strata | stream | strata-1 | 66 | 154 | 0.43 | 2 |
| Strata | stream | strata-coordinator | 24 | 152 | 0.16 | 0 |
| Elasticsearch | load | es-0 | 18985 | 9195 | 2.06 | 1052 |
| Elasticsearch | load | es-1 | 17595 | 9195 | 1.91 | 1008 |
| Elasticsearch | timed run | es-0 | 718 | 481 | 1.49 | 4 |
| Elasticsearch | timed run | es-1 | 974 | 480 | 2.03 | 23 |
| Elasticsearch | writes | es-0 | 121 | 102 | 1.19 | 1 |
| Elasticsearch | writes | es-1 | 172 | 102 | 1.69 | 5 |
| Elasticsearch | stream | es-0 | 150 | 153 | 0.98 | 0 |
| Elasticsearch | stream | es-1 | 209 | 154 | 1.36 | 1 |
| Elasticsearch, trial licence | timed run | es-0 | 113 | 82 | 1.38 | 0 |
| Elasticsearch, trial licence | timed run | es-1 | 152 | 82 | 1.85 | 4 |
| OpenSearch | load | os-0 | 20975 | 11992 | 1.75 | 622 |
| OpenSearch | load | os-1 | 29689 | 11992 | 2.48 | 2487 |
| OpenSearch | timed run | os-0 | 956 | 497 | 1.92 | 14 |
| OpenSearch | timed run | os-1 | 901 | 496 | 1.82 | 18 |
| OpenSearch | writes | os-0 | 143 | 104 | 1.37 | 3 |
| OpenSearch | writes | os-1 | 147 | 104 | 1.41 | 3 |
| OpenSearch | stream | os-0 | 187 | 154 | 1.21 | 2 |
| OpenSearch | stream | os-1 | 183 | 154 | 1.19 | 1 |

## Memory of each engine's containers (cgroup memory.current, memory.stat, memory.peak; GB, all its pods)

| engine | after the load | of it anonymous | after the timed run | of it anonymous | of it files (page cache) | pods' peaks added |
|---|---|---|---|---|---|---|
| Strata | – | – | 37.90 | 2.04 | 35.76 | 50.90 |
| Elasticsearch | 39.04 | 30.23 | 41.63 | 30.73 | 10.52 | 55.84 |
| Elasticsearch, trial licence | – | – | 41.70 | 30.80 | 10.53 | 55.84 |
| OpenSearch | 41.72 | 30.06 | 44.80 | 30.55 | 13.66 | 55.85 |

- Elasticsearch, hybrid (RRF): unavailable: {"license.expired.feature":"Reciprocal Rank Fusion (RRF)","reason":"current license is non-compliant for [Reciprocal Rank Fusion (RRF)]","root_cause":[{"license.expired.feature":"Reciprocal Rank Fusion (RRF)","reason":"current license is non-compliant for [Reciprocal Rank Fusion (RRF)]","type":"secu

- Elasticsearch, hybrid (RRF): the rrf retriever

- Elasticsearch, hybrid (weighted): no min-max normalised fusion on the free licence (the linear retriever needs a paid one): the sum of 0.2 x the raw BM25 score and 0.8 x the vector score ((1 + dot) / 2) over the lexical matches and the knn top 100

- Elasticsearch, trial licence, hybrid (RRF): the rrf retriever

- Elasticsearch, trial licence, hybrid (weighted): the linear retriever (a paid feature, here on a trial licence): match and knn top 100, each min-max normalised over the top 100, weighted 0.2 and 0.8

- OpenSearch, hybrid (RRF): a hybrid query (match, knn k 100, pagination depth 100) through a search pipeline's score-ranker-processor, rrf, k 60

- OpenSearch, hybrid (weighted): a hybrid query (match, knn k 100, pagination depth 100) through a search pipeline's normalization-processor: min_max, arithmetic_mean weighted 0.2 and 0.8

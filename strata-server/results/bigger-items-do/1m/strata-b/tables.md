## Load time and index size

| engine | load (s) | of which: send / build | searchable | merge to 1 segment | index size (GB) | documents |
|---|---|---|---|---|---|---|
| Strata | 211 | load 364 + merge 29 | (built) | (built as one) | 4.91 | 1,000,000 |
| Elasticsearch | 680 | send 466 | 2 | 212 | 4.87 | 1,000,000 |
| OpenSearch | 871 | send 566 | 2 | 303 | 5.14 | 1,000,000 |

## 1 client: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | Elasticsearch, trial licence q/s | Elasticsearch, trial licence ms | OpenSearch q/s | OpenSearch ms |
|---|---|---|---|---|---|---|---|---|
| bm25 | 536 | 1.64 / 2.62 / 5.34 | 292 | 3.27 / 4.26 / 6.72 | n/a | n/a | 288 | 3.34 / 4.20 / 6.19 |
| bm25 + filter | 359 | 2.44 / 4.54 / 7.20 | 271 | 3.44 / 4.85 / 9.10 | n/a | n/a | 271 | 3.39 / 4.96 / 8.87 |
| phrase | 576 | 1.54 / 2.37 / 4.34 | 361 | 2.78 / 3.86 / 5.08 | n/a | n/a | 348 | 2.83 / 4.01 / 5.43 |
| typo | 55 | 18.59 / 25.58 / 32.72 | 64 | 14.99 / 23.86 / 34.20 | n/a | n/a | 44 | 21.10 / 36.88 / 54.10 |
| prefix | 324 | 2.82 / 4.65 / 7.17 | 247 | 3.75 / 5.36 / 8.96 | n/a | n/a | 220 | 4.12 / 6.26 / 10.97 |
| vector | 543 | 1.80 / 2.04 / 2.72 | 240 | 4.06 / 4.76 / 6.45 | n/a | n/a | 217 | 4.51 / 5.38 / 6.81 |
| vector + filter | 451 | 2.10 / 2.47 / 4.97 | 221 | 4.41 / 5.16 / 6.36 | n/a | n/a | 125 | 7.93 / 9.11 / 11.54 |
| hybrid (RRF) | 222 | 4.24 / 5.76 / 9.36 | n/a | n/a | 97 | 10.02 / 12.30 / 15.02 | 119 | 7.97 / 10.46 / 16.67 |
| hybrid (weighted) | 221 | 4.27 / 5.80 / 9.85 | 136 | 6.36 / 11.27 / 21.22 | 82 | 11.37 / 17.01 / 23.25 | 123 | 7.62 / 10.15 / 15.73 |
| boosted (bm25 × decay) | 187 | 4.57 / 8.90 / 16.46 | 108 | 7.17 / 19.73 / 32.89 | n/a | n/a | 108 | 7.07 / 19.69 / 34.00 |
| similar items | 444 | 2.20 / 2.51 / 3.21 | 155 | 6.30 / 6.91 / 9.05 | n/a | n/a | 117 | 8.50 / 9.80 / 12.15 |
| facet | 604 | 1.61 / 2.06 / 2.63 | 430 | 2.09 / 3.08 / 5.41 | n/a | n/a | 472 | 1.94 / 2.73 / 5.44 |
| key lookup | 1,195 | 0.81 / 0.94 / 1.30 | 941 | 1.08 / 1.37 / 1.77 | n/a | n/a | 1,051 | 0.95 / 1.27 / 1.69 |
| sql aggregate | 6 | 193.59 / 262.56 / 284.44 | 20 | 51.25 / 69.55 / 79.98 | n/a | n/a | 17 | 59.88 / 84.38 / 94.41 |


## 4 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | Elasticsearch, trial licence q/s | Elasticsearch, trial licence ms | OpenSearch q/s | OpenSearch ms |
|---|---|---|---|---|---|---|---|---|
| bm25 | 1,302 | 2.82 / 4.38 / 7.76 | 900 | 4.16 / 6.11 / 9.20 | n/a | n/a | 925 | 4.09 / 5.91 / 8.43 |
| bm25 + filter | 933 | 3.78 / 6.91 / 11.05 | 794 | 4.64 / 7.08 / 12.33 | n/a | n/a | 797 | 4.58 / 7.19 / 12.71 |
| phrase | 1,350 | 2.78 / 4.00 / 6.16 | 1,130 | 3.45 / 5.29 / 7.71 | n/a | n/a | 1,139 | 3.40 / 5.24 / 7.35 |
| typo | 134 | 30.49 / 42.09 / 56.35 | 156 | 24.52 / 38.71 / 56.99 | n/a | n/a | 103 | 36.51 / 60.84 / 88.44 |
| prefix | 825 | 4.40 / 7.57 / 11.40 | 695 | 5.39 / 8.07 / 12.50 | n/a | n/a | 605 | 6.08 / 9.68 / 15.95 |
| vector | 1,240 | 3.13 / 4.11 / 5.07 | 696 | 5.45 / 6.97 / 9.36 | n/a | n/a | 633 | 6.14 / 8.01 / 10.01 |
| vector + filter | 1,259 | 3.06 / 4.00 / 5.12 | 635 | 6.12 / 7.62 / 9.63 | n/a | n/a | 334 | 11.69 / 15.18 / 18.24 |
| hybrid (RRF) | 530 | 7.11 / 10.56 / 15.89 | n/a | n/a | 254 | 15.31 / 19.58 / 24.76 | 311 | 12.09 / 17.43 / 25.74 |
| hybrid (weighted) | 517 | 7.22 / 10.81 / 16.17 | 356 | 9.89 / 17.60 / 33.49 | 213 | 17.57 / 26.63 / 36.73 | 331 | 11.43 / 16.36 / 23.85 |
| boosted (bm25 × decay) | 484 | 7.06 / 14.05 / 25.58 | 260 | 12.46 / 30.47 / 53.97 | n/a | n/a | 263 | 12.40 / 29.60 / 53.52 |
| similar items | 1,039 | 3.74 / 4.93 / 6.08 | 321 | 11.98 / 16.01 / 20.29 | n/a | n/a | 299 | 12.96 / 17.15 / 21.52 |
| facet | 1,323 | 2.97 / 3.92 / 5.02 | 1,461 | 2.43 / 3.90 / 7.32 | n/a | n/a | 1,602 | 2.25 / 3.51 / 6.84 |
| key lookup | 3,499 | 1.13 / 1.41 / 1.78 | 3,691 | 1.08 / 1.43 / 1.80 | n/a | n/a | 4,579 | 0.87 / 1.20 / 1.58 |
| sql aggregate | 10 | 413.99 / 614.95 / 650.71 | 42 | 96.02 / 142.23 / 176.19 | n/a | n/a | 37 | 107.80 / 163.88 / 192.23 |


## 16 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | Elasticsearch, trial licence q/s | Elasticsearch, trial licence ms | OpenSearch q/s | OpenSearch ms |
|---|---|---|---|---|---|---|---|---|
| bm25 | 1,872 | 8.11 / 11.45 / 16.18 | 1,527 | 9.40 / 16.29 / 25.90 | n/a | n/a | 1,613 | 8.95 / 15.46 / 23.28 |
| bm25 + filter | 1,442 | 10.04 / 16.51 / 26.83 | 1,328 | 10.86 / 19.09 / 30.40 | n/a | n/a | 1,303 | 11.01 / 19.61 / 32.29 |
| phrase | 1,657 | 9.47 / 11.82 / 15.38 | 2,035 | 7.31 / 12.60 / 20.24 | n/a | n/a | 2,117 | 7.11 / 11.99 / 17.58 |
| typo | 123 | 129.88 / 197.03 / 275.53 | 173 | 83.81 / 134.70 / 178.84 | n/a | n/a | 111 | 138.05 / 197.84 / 258.95 |
| prefix | 1,144 | 12.62 / 21.47 / 31.24 | 1,054 | 13.43 / 24.69 / 37.62 | n/a | n/a | 913 | 15.95 / 26.65 / 43.52 |
| vector | 1,511 | 10.27 / 13.23 / 16.68 | 1,312 | 11.72 / 16.47 / 22.46 | n/a | n/a | 1,082 | 14.23 / 20.31 / 27.62 |
| vector + filter | 1,517 | 10.17 / 13.31 / 16.77 | 1,083 | 14.01 / 19.81 / 29.83 | n/a | n/a | 466 | 33.82 / 45.19 / 53.66 |
| hybrid (RRF) | 686 | 21.80 / 32.09 / 47.57 | n/a | n/a | 408 | 38.42 / 50.30 / 62.12 | 386 | 39.40 / 57.64 / 87.53 |
| hybrid (weighted) | 653 | 23.16 / 31.90 / 50.95 | 472 | 30.72 / 52.29 / 93.09 | 273 | 56.25 / 79.90 / 122.21 | 433 | 34.85 / 52.49 / 72.46 |
| boosted (bm25 × decay) | 562 | 23.10 / 50.67 / 112.03 | 315 | 43.76 / 87.54 / 153.56 | n/a | n/a | 310 | 44.88 / 86.99 / 167.27 |
| similar items | 1,230 | 12.71 / 16.46 / 20.84 | 557 | 27.66 / 39.43 / 49.68 | n/a | n/a | 393 | 40.07 / 52.10 / 62.71 |
| facet | 1,861 | 8.25 / 11.02 / 14.17 | 2,438 | 5.36 / 11.68 / 22.09 | n/a | n/a | 3,059 | 4.66 / 8.27 / 14.58 |
| key lookup | 4,474 | 3.60 / 4.25 / 4.95 | 9,415 | 1.60 / 2.40 / 3.68 | n/a | n/a | 12,775 | 1.17 / 1.88 / 2.88 |
| sql aggregate | 10 | 1550.82 / 2338.64 / 2828.03 | 41 | 399.57 / 546.19 / 623.27 | n/a | n/a | 36 | 431.13 / 635.09 / 784.07 |


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
| vector | 0.807 | – | 0.733 | – | n/a | n/a | 0.694 | – |
| vector + filter | 0.829 | – | 0.690 | – | n/a | n/a | 0.822 | – |
| hybrid (RRF) | – | – | n/a | n/a | – | – | – | – |
| hybrid (weighted) | – | – | – | – | – | – | – | – |
| boosted (bm25 × decay) | – | – | – | – | n/a | n/a | – | – |
| similar items | 0.990 | – | 0.930 | – | n/a | n/a | 0.976 | – |


## SQL aggregates equal to the true answer

| Strata | Elasticsearch | Elasticsearch, trial licence | OpenSearch |
|---|---|---|---|
| 300 of 300 | 300 of 300 | n/a | 300 of 300 |

## Agreement with Strata's answers

| kind | Elasticsearch | Elasticsearch, trial licence | OpenSearch |
|---|---|---|---|
| bm25 | 0.957 of the top 10 | n/a | 0.959 of the top 10 |
| bm25 + filter | 0.953 of the top 10 | n/a | 0.949 of the top 10 |
| phrase | 0.545 of the top 10 | n/a | 0.544 of the top 10 |
| typo | 0.421 of the top 10 | n/a | 0.424 of the top 10 |
| prefix | 0.966 of the top 10 | n/a | 0.965 of the top 10 |
| vector | 0.751 of the top 10 | n/a | 0.713 of the top 10 |
| vector + filter | 0.720 of the top 10 | n/a | 0.796 of the top 10 |
| hybrid (RRF) | n/a | 0.813 of the top 10 | 0.776 of the top 10 |
| hybrid (weighted) | 0.451 of the top 10 | 0.717 of the top 10 | 0.722 of the top 10 |
| boosted (bm25 × decay) | 0.971 of the top 10 | n/a | 0.972 of the top 10 |
| similar items | 0.933 of the top 10 | n/a | 0.973 of the top 10 |
| facet | 286 of 300 equal | n/a | 286 of 300 equal |
| key lookup | 300 of 300 equal | n/a | 300 of 300 equal |


## Writes: documents a second, and latency in ms (p50 / p99)

| phase | clients | Elasticsearch docs/s | Elasticsearch ms | OpenSearch docs/s | OpenSearch ms |
|---|---|---|---|---|---|
| insert one | 1 | 230 | 3.88 / 9.30 | 231 | 3.98 / 7.88 |
| insert one | 4 | 685 | 4.93 / 16.95 | 768 | 4.63 / 11.37 |
| insert one | 16 | 1,919 | 6.96 / 28.94 | 1,953 | 7.27 / 22.38 |
| insert batch | 1 | 1,594 | 42.71 / 256.29 | 1,280 | 65.36 / 113.90 |
| insert batch | 4 | 3,278 | 77.59 / 503.91 | 3,345 | 95.77 / 240.76 |
| insert batch | 16 | 3,513 | 353.88 / 983.79 | 2,891 | 520.86 / 771.40 |
| update one | 1 | 201 | 4.15 / 11.13 | 206 | 4.12 / 12.20 |
| update one | 4 | 745 | 4.65 / 16.48 | 751 | 4.71 / 12.03 |
| update one | 16 | 2,032 | 6.62 / 27.08 | 1,893 | 7.40 / 25.19 |

## How soon a written document is found (ms from the write's start)

| engine | searchable | write returned (p50) | found (p50) | found (p90) | found (max) |
|---|---|---|---|---|---|
| Elasticsearch | refresh_interval 1s (the default; loading turned it off) | 7.0 | 762.6 | 1016.5 | 1138.1 |
| OpenSearch | refresh_interval 1s (the default; loading turned it off) | 5.8 | 672.6 | 1017.8 | 1044.2 |

## Streams through Redpanda: documents written a second, and lag in ms from when a document was due (p50 / p99)

| phase | Elasticsearch docs/s | Elasticsearch lag | OpenSearch docs/s | OpenSearch lag |
|---|---|---|---|---|
| 250/s | 250 | 24 / 47 | 250 | 24 / 45 |
| 500/s | 500 | 24 / 52 | 499 | 25 / 103 |
| 1000/s | 999 | 29 / 85 | 999 | 28 / 79 |
| 2000/s | 1,998 | 33 / 172 | 1,997 | 37 / 380 |
| flat out | 3,703 | 2260 / 4542 | 3,026 | 3267 / 5796 |

Lag includes reading the topic (Redpanda's part, the same for every engine, is the 'read' column below).

| engine | what written means | read lag p50 (ms) | crash: events written again | documents lost |
|---|---|---|---|---|
| Elasticsearch | written: searchable within the 1 s refresh | 19 | 1326 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |
| OpenSearch | written: searchable within the 1 s refresh | 20 | 439 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |

## CPU used by each engine's containers (cgroup cpu.stat)

| engine | phase | pod | CPU seconds | wall seconds | average cores | throttled (s) |
|---|---|---|---|---|---|---|
| Strata | timed run | strata-0 | 528 | 419 | 1.26 | 20 |
| Strata | timed run | strata-1 | 527 | 419 | 1.26 | 15 |
| Strata | timed run | strata-coordinator | 227 | 419 | 0.54 | 0 |
| Elasticsearch | load | es-0 | 1811 | 694 | 2.61 | 174 |
| Elasticsearch | load | es-1 | 1235 | 694 | 1.78 | 5 |
| Elasticsearch | timed run | es-0 | 814 | 411 | 1.98 | 22 |
| Elasticsearch | timed run | es-1 | 740 | 411 | 1.80 | 0 |
| Elasticsearch | writes | es-0 | 136 | 103 | 1.32 | 4 |
| Elasticsearch | writes | es-1 | 106 | 103 | 1.03 | 0 |
| Elasticsearch | stream | es-0 | 153 | 151 | 1.01 | 1 |
| Elasticsearch | stream | es-1 | 138 | 151 | 0.91 | 0 |
| Elasticsearch, trial licence | timed run | es-0 | 124 | 70 | 1.77 | 3 |
| Elasticsearch, trial licence | timed run | es-1 | 105 | 69 | 1.53 | 0 |
| OpenSearch | load | os-0 | 2239 | 884 | 2.53 | 143 |
| OpenSearch | load | os-1 | 1444 | 884 | 1.63 | 32 |
| OpenSearch | timed run | os-0 | 869 | 436 | 1.99 | 14 |
| OpenSearch | timed run | os-1 | 792 | 435 | 1.82 | 1 |
| OpenSearch | writes | os-0 | 133 | 104 | 1.28 | 2 |
| OpenSearch | writes | os-1 | 111 | 104 | 1.06 | 1 |
| OpenSearch | stream | os-0 | 182 | 155 | 1.18 | 2 |
| OpenSearch | stream | os-1 | 153 | 155 | 0.99 | 1 |

## Memory of each engine's containers (cgroup memory.current, memory.stat, memory.peak; GB, all its pods)

| engine | after the load | of it anonymous | after the timed run | of it anonymous | of it files (page cache) | pods' peaks added |
|---|---|---|---|---|---|---|
| Strata | – | – | 1.01 | 0.89 | 0.10 | 1.03 |
| Elasticsearch | 35.20 | 30.11 | 35.80 | 30.70 | 4.87 | 42.99 |
| Elasticsearch, trial licence | – | – | 35.84 | 30.74 | 4.87 | 42.99 |
| OpenSearch | 35.34 | 29.97 | 35.89 | 30.51 | 5.14 | 45.66 |

- Elasticsearch, hybrid (RRF): unavailable: {"license.expired.feature":"Reciprocal Rank Fusion (RRF)","reason":"current license is non-compliant for [Reciprocal Rank Fusion (RRF)]","root_cause":[{"license.expired.feature":"Reciprocal Rank Fusion (RRF)","reason":"current license is non-compliant for [Reciprocal Rank Fusion (RRF)]","type":"secu

- Elasticsearch, hybrid (RRF): the rrf retriever

- Elasticsearch, hybrid (weighted): no min-max normalised fusion on the free licence (the linear retriever needs a paid one): the sum of 0.2 x the raw BM25 score and 0.8 x the vector score ((1 + dot) / 2) over the lexical matches and the knn top 100

- Elasticsearch, trial licence, hybrid (RRF): the rrf retriever

- Elasticsearch, trial licence, hybrid (weighted): the linear retriever (a paid feature, here on a trial licence): match and knn top 100, each min-max normalised over the top 100, weighted 0.2 and 0.8

- OpenSearch, hybrid (RRF): a hybrid query (match, knn k 100, pagination depth 100) through a search pipeline's score-ranker-processor, rrf, k 60

- OpenSearch, hybrid (weighted): a hybrid query (match, knn k 100, pagination depth 100) through a search pipeline's normalization-processor: min_max, arithmetic_mean weighted 0.2 and 0.8

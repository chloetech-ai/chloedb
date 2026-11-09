## Load time and index size

| engine | load (s) | of which: send / build | searchable | merge to 1 segment | index size (GB) | documents |
|---|---|---|---|---|---|---|
| Strata | 4071 | load 3704 + merge 3977 | (built) | (built as one) | 48.93 | 10,000,000 |
| Elasticsearch | 9337 | send 5094 | 5 | 4237 | 48.51 | 10,000,000 |
| OpenSearch | 12051 | send 6392 | 2 | 5658 | 48.57 | 10,000,000 |

## 1 client: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | Elasticsearch, trial licence q/s | Elasticsearch, trial licence ms | OpenSearch q/s | OpenSearch ms |
|---|---|---|---|---|---|---|---|---|
| bm25 | 237 | 2.93 / 8.16 / 21.58 | 151 | 5.61 / 10.14 / 20.63 | n/a | n/a | 168 | 5.00 / 9.41 / 18.73 |
| bm25 + filter | 116 | 7.15 / 17.10 / 29.78 | 117 | 6.56 / 13.25 / 44.05 | n/a | n/a | 120 | 6.11 / 13.70 / 48.75 |
| phrase | 252 | 2.47 / 7.80 / 17.10 | 121 | 6.04 / 15.26 / 28.75 | n/a | n/a | 141 | 5.06 / 13.20 / 24.34 |
| typo | 12 | 83.32 / 113.21 / 130.94 | 27 | 34.58 / 62.13 / 93.04 | n/a | n/a | 20 | 46.62 / 80.15 / 105.83 |
| prefix | 89 | 9.66 / 18.91 / 36.90 | 93 | 8.67 / 17.86 / 42.81 | n/a | n/a | 71 | 10.14 / 24.87 / 59.56 |
| vector | 435 | 2.26 / 2.61 / 3.36 | 195 | 5.04 / 5.86 / 7.81 | n/a | n/a | 177 | 5.52 / 6.73 / 8.89 |
| vector + filter | 310 | 3.17 / 3.74 / 5.12 | 68 | 6.05 / 7.29 / 221.53 | n/a | n/a | 104 | 9.42 / 11.39 / 14.20 |
| hybrid (RRF) | 100 | 8.20 / 15.26 / 38.01 | n/a | n/a | 69 | 13.40 / 19.66 / 36.05 | 66 | 12.73 / 22.17 / 51.79 |
| hybrid (weighted) | 96 | 8.57 / 16.33 / 37.98 | 42 | 14.73 / 48.33 / 130.97 | 24 | 32.80 / 83.54 / 136.66 | 66 | 12.82 / 24.61 / 51.24 |
| boosted (bm25 × decay) | 58 | 13.34 / 31.40 / 78.14 | 18 | 42.38 / 139.19 / 183.60 | n/a | n/a | 19 | 41.31 / 141.26 / 183.61 |
| similar items | 261 | 3.78 / 4.37 / 5.63 | 34 | 24.03 / 26.87 / 362.80 | n/a | n/a | 26 | 38.32 / 47.54 / 50.49 |
| facet | 313 | 2.37 / 5.85 / 11.52 | 180 | 4.18 / 9.81 / 24.94 | n/a | n/a | 183 | 3.71 / 10.09 / 33.44 |
| key lookup | 1,339 | 0.73 / 0.87 / 1.26 | 750 | 1.31 / 1.84 / 2.38 | n/a | n/a | 971 | 1.01 / 1.52 / 1.91 |
| sql aggregate | 0 | 2726.74 / 3130.45 / 3130.45 | 2 | 588.97 / 698.37 / 940.20 | n/a | n/a | 1 | 699.18 / 801.74 / 850.91 |


## 4 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | Elasticsearch, trial licence q/s | Elasticsearch, trial licence ms | OpenSearch q/s | OpenSearch ms |
|---|---|---|---|---|---|---|---|---|
| bm25 | 491 | 5.35 / 16.36 / 46.74 | 408 | 8.40 / 15.98 / 30.54 | n/a | n/a | 470 | 7.20 / 14.00 / 29.64 |
| bm25 + filter | 224 | 13.93 / 36.68 / 73.94 | 291 | 10.66 / 22.29 / 69.46 | n/a | n/a | 308 | 9.94 / 21.63 / 72.46 |
| phrase | 539 | 4.59 / 15.39 / 36.15 | 352 | 8.72 / 21.80 / 38.74 | n/a | n/a | 309 | 9.06 / 26.08 / 52.87 |
| typo | 25 | 160.70 / 232.77 / 312.14 | 66 | 54.66 / 98.78 / 159.05 | n/a | n/a | 50 | 73.93 / 131.06 / 178.40 |
| prefix | 158 | 21.26 / 46.22 / 85.13 | 231 | 14.65 / 28.35 / 69.35 | n/a | n/a | 152 | 19.61 / 49.41 / 115.87 |
| vector | 1,039 | 3.68 / 5.04 / 6.78 | 618 | 6.27 / 8.08 / 10.66 | n/a | n/a | 514 | 7.45 / 10.32 / 14.02 |
| vector + filter | 696 | 5.52 / 7.59 / 9.83 | 203 | 8.40 / 12.11 / 308.24 | n/a | n/a | 278 | 14.07 / 18.31 / 22.83 |
| hybrid (RRF) | 211 | 14.91 / 33.03 / 74.65 | n/a | n/a | 175 | 20.71 / 31.83 / 56.75 | 165 | 19.92 / 39.38 / 91.03 |
| hybrid (weighted) | 168 | 17.44 / 43.25 / 117.14 | 98 | 25.42 / 87.11 / 221.75 | 62 | 52.53 / 134.28 / 183.23 | 169 | 19.68 / 36.93 / 91.05 |
| boosted (bm25 × decay) | 99 | 29.47 / 77.28 / 183.62 | 44 | 60.47 / 206.59 / 340.57 | n/a | n/a | 42 | 68.79 / 215.20 / 374.01 |
| similar items | 563 | 6.82 / 9.22 / 12.41 | 79 | 41.22 / 52.62 / 446.36 | n/a | n/a | 64 | 60.98 / 70.58 / 88.49 |
| facet | 819 | 3.77 / 9.40 / 16.63 | 514 | 5.82 / 14.46 / 37.30 | n/a | n/a | 474 | 5.95 / 16.44 / 53.37 |
| key lookup | 4,219 | 0.92 / 1.15 / 1.50 | 3,194 | 1.25 / 1.73 / 2.28 | n/a | n/a | 4,360 | 0.92 / 1.31 / 1.73 |
| sql aggregate | 1 | 5944.47 / 6341.75 / 6341.75 | 5 | 844.79 / 1123.15 / 1215.04 | n/a | n/a | 4 | 1112.26 / 1563.86 / 1617.05 |


## 16 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Elasticsearch q/s | Elasticsearch ms | Elasticsearch, trial licence q/s | Elasticsearch, trial licence ms | OpenSearch q/s | OpenSearch ms |
|---|---|---|---|---|---|---|---|---|
| bm25 | 472 | 21.68 / 71.61 / 212.74 | 587 | 24.00 / 42.27 / 90.06 | n/a | n/a | 720 | 19.20 / 36.61 / 70.09 |
| bm25 + filter | 219 | 50.48 / 160.46 / 362.88 | 421 | 32.12 / 60.03 / 159.87 | n/a | n/a | 392 | 34.04 / 65.94 / 174.89 |
| phrase | 608 | 16.05 / 60.24 / 133.62 | 468 | 29.99 / 61.28 / 97.81 | n/a | n/a | 472 | 29.39 / 61.97 / 98.82 |
| typo | 24 | 643.81 / 994.33 / 1651.31 | 71 | 213.75 / 319.45 / 460.31 | n/a | n/a | 46 | 312.78 / 494.25 / 725.20 |
| prefix | 146 | 89.37 / 215.11 / 407.28 | 292 | 48.68 / 83.52 / 179.70 | n/a | n/a | 168 | 83.86 / 145.12 / 298.72 |
| vector | 1,429 | 10.45 / 15.69 / 24.06 | 1,208 | 12.43 / 17.29 / 25.55 | n/a | n/a | 892 | 16.55 / 24.95 / 34.55 |
| vector + filter | 813 | 17.90 / 31.73 / 47.14 | 267 | 32.46 / 65.06 / 745.42 | n/a | n/a | 365 | 40.56 / 59.31 / 170.93 |
| hybrid (RRF) | 197 | 63.58 / 151.84 / 428.36 | n/a | n/a | 262 | 57.30 / 81.15 / 140.14 | 193 | 72.34 / 120.04 / 246.77 |
| hybrid (weighted) | 200 | 59.97 / 147.91 / 410.58 | 113 | 109.70 / 240.83 / 475.47 | 72 | 193.55 / 346.24 / 526.73 | 199 | 71.80 / 120.28 / 241.73 |
| boosted (bm25 × decay) | 94 | 118.11 / 343.91 / 775.85 | 42 | 316.52 / 611.33 / 962.50 | n/a | n/a | 41 | 316.03 / 636.52 / 1060.22 |
| similar items | 628 | 23.73 / 39.74 / 55.11 | 86 | 176.79 / 231.55 / 638.50 | n/a | n/a | 67 | 236.40 / 321.72 / 352.85 |
| facet | 1,100 | 10.91 / 28.42 / 53.28 | 708 | 18.81 / 39.81 / 95.75 | n/a | n/a | 620 | 19.54 / 46.17 / 139.80 |
| key lookup | 5,864 | 2.70 / 3.33 / 4.26 | 9,050 | 1.68 / 2.62 / 3.83 | n/a | n/a | 12,620 | 1.20 / 1.98 / 2.99 |
| sql aggregate | 1 | 18542.21 / 19793.38 / 20266.10 | 5 | 2874.04 / 4358.59 / 4855.49 | n/a | n/a | 4 | 3222.44 / 5263.62 / 5883.90 |


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
| vector | 0.786 | – | 0.721 | – | n/a | n/a | 0.715 | – |
| vector + filter | 0.817 | – | 0.641 | – | n/a | n/a | 0.860 | – |
| hybrid (RRF) | – | – | n/a | n/a | – | – | – | – |
| hybrid (weighted) | – | – | – | – | – | – | – | – |
| boosted (bm25 × decay) | – | – | – | – | n/a | n/a | – | – |
| similar items | 0.971 | – | 0.925 | – | n/a | n/a | 0.931 | – |


## SQL aggregates equal to the true answer

| Strata | Elasticsearch | Elasticsearch, trial licence | OpenSearch |
|---|---|---|---|
| 300 of 300 | 300 of 300 | n/a | 300 of 300 |

## Agreement with Strata's answers

| kind | Elasticsearch | Elasticsearch, trial licence | OpenSearch |
|---|---|---|---|
| bm25 | 0.978 of the top 10 | n/a | 0.972 of the top 10 |
| bm25 + filter | 0.981 of the top 10 | n/a | 0.982 of the top 10 |
| phrase | 0.658 of the top 10 | n/a | 0.660 of the top 10 |
| typo | 0.446 of the top 10 | n/a | 0.445 of the top 10 |
| prefix | 0.976 of the top 10 | n/a | 0.976 of the top 10 |
| vector | 0.672 of the top 10 | n/a | 0.660 of the top 10 |
| vector + filter | 0.632 of the top 10 | n/a | 0.796 of the top 10 |
| hybrid (RRF) | n/a | 0.717 of the top 10 | 0.731 of the top 10 |
| hybrid (weighted) | 0.453 of the top 10 | 0.684 of the top 10 | 0.732 of the top 10 |
| boosted (bm25 × decay) | 0.982 of the top 10 | n/a | 0.985 of the top 10 |
| similar items | 0.914 of the top 10 | n/a | 0.911 of the top 10 |
| facet | 272 of 300 equal | n/a | 271 of 300 equal |
| key lookup | 300 of 300 equal | n/a | 300 of 300 equal |


## Writes: documents a second, and latency in ms (p50 / p99)

| phase | clients | Elasticsearch docs/s | Elasticsearch ms | OpenSearch docs/s | OpenSearch ms |
|---|---|---|---|---|---|
| insert one | 1 | 168 | 5.21 / 12.89 | 167 | 5.28 / 14.30 |
| insert one | 4 | 515 | 6.89 / 20.85 | 560 | 6.06 / 20.95 |
| insert one | 16 | 1,580 | 8.38 / 34.65 | 1,434 | 9.20 / 38.95 |
| insert batch | 1 | 1,556 | 47.26 / 172.24 | 1,242 | 67.83 / 134.94 |
| insert batch | 4 | 3,321 | 80.42 / 472.67 | 2,959 | 110.19 / 292.49 |
| insert batch | 16 | 3,493 | 407.49 / 1153.90 | 1,985 | 589.67 / 2803.74 |
| update one | 1 | 168 | 4.98 / 11.90 | 121 | 5.17 / 17.96 |
| update one | 4 | 550 | 6.48 / 20.84 | 606 | 5.75 / 17.14 |
| update one | 16 | 1,516 | 8.26 / 39.00 | 1,567 | 8.79 / 29.54 |

## How soon a written document is found (ms from the write's start)

| engine | searchable | write returned (p50) | found (p50) | found (p90) | found (max) |
|---|---|---|---|---|---|
| Elasticsearch | refresh_interval 1s (the default; loading turned it off) | 7.4 | 820.5 | 1014.4 | 1068.9 |
| OpenSearch | refresh_interval 1s (the default; loading turned it off) | 7.7 | 673.9 | 1017.3 | 1030.3 |

## Streams through Redpanda: documents written a second, and lag in ms from when a document was due (p50 / p99)

| phase | Elasticsearch docs/s | Elasticsearch lag | OpenSearch docs/s | OpenSearch lag |
|---|---|---|---|---|
| 250/s | 250 | 27 / 50 | 247 | 27 / 1778 |
| 500/s | 499 | 29 / 60 | 500 | 30 / 2206 |
| 1000/s | 999 | 32 / 673 | 999 | 30 / 86 |
| 2000/s | 1,998 | 35 / 362 | 1,997 | 53 / 1867 |
| flat out | 3,339 | 2654 / 5280 | 2,373 | 3637 / 7734 |

Lag includes reading the topic (Redpanda's part, the same for every engine, is the 'read' column below).

| engine | what written means | read lag p50 (ms) | crash: events written again | documents lost |
|---|---|---|---|---|
| Elasticsearch | written: searchable within the 1 s refresh | 20 | 1308 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |
| OpenSearch | written: searchable within the 1 s refresh | 21 | 134 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |

## CPU used by each engine's containers (cgroup cpu.stat)

| engine | phase | pod | CPU seconds | wall seconds | average cores | throttled (s) |
|---|---|---|---|---|---|---|
| Strata | timed run | strata-0 | 1377 | 953 | 1.44 | 62 |
| Strata | timed run | strata-1 | 1263 | 953 | 1.33 | 47 |
| Strata | timed run | strata-coordinator | 111 | 953 | 0.12 | 0 |
| Elasticsearch | load | es-0 | 19707 | 9371 | 2.10 | 1140 |
| Elasticsearch | load | es-1 | 16656 | 9371 | 1.78 | 289 |
| Elasticsearch | timed run | es-0 | 1013 | 570 | 1.78 | 20 |
| Elasticsearch | timed run | es-1 | 927 | 570 | 1.63 | 6 |
| Elasticsearch | writes | es-0 | 129 | 100 | 1.29 | 2 |
| Elasticsearch | writes | es-1 | 147 | 100 | 1.47 | 2 |
| Elasticsearch | stream | es-0 | 173 | 152 | 1.14 | 0 |
| Elasticsearch | stream | es-1 | 158 | 152 | 1.04 | 0 |
| Elasticsearch, trial licence | timed run | es-0 | 141 | 81 | 1.75 | 4 |
| Elasticsearch, trial licence | timed run | es-1 | 133 | 81 | 1.64 | 0 |
| OpenSearch | load | os-0 | 27037 | 12065 | 2.24 | 2568 |
| OpenSearch | load | os-1 | 21682 | 12065 | 1.80 | 469 |
| OpenSearch | timed run | os-0 | 1117 | 632 | 1.77 | 37 |
| OpenSearch | timed run | os-1 | 1036 | 632 | 1.64 | 6 |
| OpenSearch | writes | os-0 | 143 | 102 | 1.40 | 3 |
| OpenSearch | writes | os-1 | 119 | 102 | 1.16 | 0 |
| OpenSearch | stream | os-0 | 210 | 158 | 1.33 | 6 |
| OpenSearch | stream | os-1 | 202 | 158 | 1.28 | 1 |

## Memory of each engine's containers (cgroup memory.current, memory.stat, memory.peak; GB, all its pods)

| engine | after the load | of it anonymous | after the timed run | of it anonymous | of it files (page cache) | pods' peaks added |
|---|---|---|---|---|---|---|
| Strata | – | – | 5.00 | 1.43 | 3.45 | 5.02 |
| Elasticsearch | 38.98 | 30.17 | 41.41 | 30.54 | 10.50 | 55.85 |
| Elasticsearch, trial licence | – | – | 41.51 | 30.62 | 10.51 | 55.85 |
| OpenSearch | 40.80 | 30.02 | 43.87 | 30.53 | 12.77 | 55.85 |

- Elasticsearch, hybrid (RRF): unavailable: {"license.expired.feature":"Reciprocal Rank Fusion (RRF)","reason":"current license is non-compliant for [Reciprocal Rank Fusion (RRF)]","root_cause":[{"license.expired.feature":"Reciprocal Rank Fusion (RRF)","reason":"current license is non-compliant for [Reciprocal Rank Fusion (RRF)]","type":"secu

- Elasticsearch, hybrid (RRF): the rrf retriever

- Elasticsearch, hybrid (weighted): no min-max normalised fusion on the free licence (the linear retriever needs a paid one): the sum of 0.2 x the raw BM25 score and 0.8 x the vector score ((1 + dot) / 2) over the lexical matches and the knn top 100

- Elasticsearch, trial licence, hybrid (RRF): the rrf retriever

- Elasticsearch, trial licence, hybrid (weighted): the linear retriever (a paid feature, here on a trial licence): match and knn top 100, each min-max normalised over the top 100, weighted 0.2 and 0.8

- OpenSearch, hybrid (RRF): a hybrid query (match, knn k 100, pagination depth 100) through a search pipeline's score-ranker-processor, rrf, k 60

- OpenSearch, hybrid (weighted): a hybrid query (match, knn k 100, pagination depth 100) through a search pipeline's normalization-processor: min_max, arithmetic_mean weighted 0.2 and 0.8

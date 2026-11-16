## Load time and index size

| engine | load (s) | of which: send / build | searchable | merge to 1 segment | index size (GB) | documents |
|---|---|---|---|---|---|---|
| Strata | 405 | load 704 + merge 35 | (built) | (built as one) | 5.00 | 1,000,000 |
| Vespa | 1395 | send 1395 | on write | (on its own) | 9.07 (+ 0.03 transaction log) | 1,000,000 |

## 1 client: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Vespa q/s | Vespa ms |
|---|---|---|---|---|
| bm25 | 411 | 2.18 / 3.32 / 6.15 | 130 | 3.99 / 10.43 / 58.18 |
| bm25 + filter | 351 | 2.54 / 3.91 / 7.44 | 145 | 4.93 / 9.95 / 42.00 |
| phrase | 569 | 1.72 / 2.00 / 2.80 | 377 | 2.31 / 3.62 / 6.67 |
| typo | 56 | 18.07 / 26.20 / 33.39 | n/a | n/a |
| prefix | 373 | 2.41 / 3.80 / 5.79 | n/a | n/a |
| vector | 414 | 2.37 / 2.64 / 3.57 | 238 | 3.56 / 4.08 / 42.09 |
| vector + filter | 397 | 2.46 / 2.74 / 3.85 | 159 | 6.21 / 7.11 / 8.56 |
| hybrid (RRF) | 177 | 5.38 / 7.14 / 11.13 | 89 | 8.26 / 14.71 / 56.54 |
| hybrid (weighted) | 176 | 5.37 / 7.27 / 11.02 | 89 | 8.31 / 13.79 / 56.05 |
| boosted (bm25 × decay) | 202 | 4.20 / 8.36 / 13.64 | 224 | 3.49 / 5.68 / 22.07 |
| similar items | 315 | 3.10 / 3.52 / 4.71 | 170 | 5.84 / 6.48 / 7.28 |
| facet | 494 | 2.01 / 2.34 / 3.13 | 106 | 3.97 / 22.28 / 74.92 |
| key lookup | 1,073 | 0.99 / 1.19 / 1.57 | 591 | 1.23 / 1.48 / 31.43 |
| sql aggregate | 115 | 8.52 / 11.25 / 17.74 | 6 | 171.89 / 236.79 / 259.97 |


## 4 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Vespa q/s | Vespa ms |
|---|---|---|---|---|
| bm25 | 1,291 | 2.73 / 4.62 / 8.92 | 311 | 4.34 / 65.52 / 76.15 |
| bm25 + filter | 1,099 | 3.22 / 5.47 / 10.68 | 390 | 5.44 / 15.17 / 66.70 |
| phrase | 2,069 | 1.87 / 2.34 / 3.23 | 754 | 2.72 / 5.06 / 51.37 |
| typo | 147 | 27.59 / 40.48 / 52.68 | n/a | n/a |
| prefix | 1,181 | 3.05 / 5.07 / 7.75 | n/a | n/a |
| vector | 1,284 | 2.98 / 3.89 / 5.37 | 410 | 4.38 / 8.69 / 72.13 |
| vector + filter | 1,216 | 3.15 / 4.14 / 5.66 | 497 | 7.76 / 9.60 / 14.73 |
| hybrid (RRF) | 501 | 7.44 / 11.04 / 17.74 | 243 | 10.42 / 33.37 / 84.89 |
| hybrid (weighted) | 499 | 7.51 / 11.14 / 17.50 | 284 | 10.47 / 19.10 / 68.81 |
| boosted (bm25 × decay) | 584 | 5.82 / 11.48 / 21.99 | 738 | 3.87 / 8.56 / 31.27 |
| similar items | 926 | 4.15 / 5.41 / 7.33 | 461 | 7.11 / 10.62 / 31.22 |
| facet | 1,816 | 2.13 / 2.77 / 3.80 | 355 | 4.09 / 29.78 / 75.11 |
| key lookup | 4,108 | 1.00 / 1.29 / 1.81 | 1,938 | 1.21 / 1.53 / 39.55 |
| sql aggregate | 277 | 14.26 / 20.36 / 26.79 | 35 | 113.27 / 177.29 / 201.69 |


## 16 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms | Vespa q/s | Vespa ms |
|---|---|---|---|---|
| bm25 | 2,189 | 6.53 / 11.58 / 19.59 | 409 | 11.29 / 87.56 / 98.27 |
| bm25 + filter | 1,909 | 7.47 / 13.35 / 22.75 | 422 | 18.01 / 81.80 / 93.32 |
| phrase | 4,333 | 3.50 / 4.87 / 8.13 | 781 | 6.16 / 77.69 / 83.98 |
| typo | 173 | 91.63 / 143.49 / 184.14 | n/a | n/a |
| prefix | 2,134 | 6.89 / 11.58 / 17.26 | n/a | n/a |
| vector | 2,202 | 6.82 / 10.22 / 15.04 | 451 | 11.23 / 85.85 / 93.60 |
| vector + filter | 2,059 | 7.28 / 11.03 / 16.17 | 496 | 26.16 / 54.35 / 65.85 |
| hybrid (RRF) | 721 | 20.75 / 32.90 / 50.09 | 289 | 46.78 / 83.84 / 200.19 |
| hybrid (weighted) | 718 | 21.07 / 32.98 / 49.03 | 298 | 46.60 / 76.63 / 173.86 |
| boosted (bm25 × decay) | 847 | 15.98 / 32.38 / 58.98 | 772 | 10.98 / 57.53 / 75.07 |
| similar items | 1,538 | 9.81 / 14.88 / 20.80 | 465 | 21.47 / 66.59 / 78.31 |
| facet | 3,684 | 4.09 / 5.90 / 9.66 | 412 | 18.41 / 86.38 / 156.85 |
| key lookup | 11,772 | 1.34 / 2.01 / 3.08 | 2,351 | 1.75 / 2.82 / 77.22 |
| sql aggregate | 311 | 50.37 / 79.92 / 104.79 | 35 | 448.48 / 645.98 / 776.86 |


Errors during the timed runs: Strata 0, Vespa 0

## Answers: recall@10 against an exact search, nDCG@10 against the collection's judgments

Unavailable kinds are n/a; recall is for the kinds an exact search can answer.

| kind | Strata recall | Strata nDCG | Vespa recall | Vespa nDCG |
|---|---|---|---|---|
| bm25 | – | – | – | – |
| bm25 + filter | – | – | – | – |
| phrase | – | – | – | – |
| typo | – | – | n/a | n/a |
| prefix | – | – | n/a | n/a |
| vector | 0.807 | – | 0.781 | – |
| vector + filter | 0.862 | – | 0.866 | – |
| hybrid (RRF) | – | – | – | – |
| hybrid (weighted) | – | – | – | – |
| boosted (bm25 × decay) | – | – | – | – |
| similar items | 0.994 | – | 0.987 | – |


## SQL aggregates equal to the true answer

| Strata | Vespa |
|---|---|
| 300 of 300 | 300 of 300 |

## Agreement with Strata's answers

| kind | Vespa |
|---|---|
| bm25 | 0.932 of the top 10 |
| bm25 + filter | 0.933 of the top 10 |
| phrase | 0.649 of the top 10 |
| typo | n/a |
| prefix | n/a |
| vector | 0.792 of the top 10 |
| vector + filter | 0.865 of the top 10 |
| hybrid (RRF) | 0.732 of the top 10 |
| hybrid (weighted) | 0.735 of the top 10 |
| boosted (bm25 × decay) | 0.854 of the top 10 |
| similar items | 0.986 of the top 10 |
| facet | 131 of 300 equal |
| key lookup | 300 of 300 equal |


## Writes: documents a second, and latency in ms (p50 / p99)

| phase | clients | Strata docs/s | Strata ms | Vespa docs/s | Vespa ms |
|---|---|---|---|---|---|
| insert one | 1 | 258 | 3.69 / 8.15 | 138 | 6.76 / 10.76 |
| insert one | 4 | 913 | 3.63 / 7.99 | 473 | 7.76 / 15.27 |
| insert one | 16 | 2,063 | 5.46 / 73.41 | 712 | 14.61 / 66.35 |
| insert batch | 1 | 1,155 | 65.97 / 155.47 | 800 | 109.70 / 141.35 |
| insert batch | 4 | 2,766 | 115.16 / 395.38 | 819 | 461.34 / 1277.85 |
| insert batch | 16 | 3,188 | 417.72 / 1288.54 | 634 | 2518.18 / 4330.64 |
| update one | 1 | 293 | 2.98 / 7.43 | 215 | 3.64 / 40.07 |
| update one | 4 | 1,228 | 2.95 / 8.56 | 698 | 3.75 / 57.60 |
| update one | 16 | 4,212 | 3.56 / 7.52 | 1,192 | 4.65 / 77.95 |

## How soon a written document is found (ms from the write's start)

| engine | searchable | write returned (p50) | found (p50) | found (p90) | found (max) |
|---|---|---|---|---|---|
| Strata | searchable when the write returns | 4.2 | 9.9 | 11.3 | 99.0 |
| Vespa | searchable when the write returns | 7.7 | 10.9 | 13.1 | 48.7 |

## Streams through Redpanda: documents written a second, and lag in ms from when a document was due (p50 / p99)

| phase | Strata docs/s | Strata lag | Vespa docs/s | Vespa lag |
|---|---|---|---|---|
| 250/s | 250 | 24 / 67 | 250 | 29 / 45 |
| 500/s | 499 | 27 / 115 | 500 | 31 / 85 |
| 1000/s | 999 | 33 / 151 | 829 | 4185 / 7760 (fell behind) |
| 2000/s | 1,992 | 154 / 743 | not run | not run |
| flat out | 2,555 | 3800 / 7079 | 828 | 12653 / 23396 |

Lag includes reading the topic (Redpanda's part, the same for every engine, is the 'read' column below).

| engine | what written means | read lag p50 (ms) | crash: events written again | documents lost |
|---|---|---|---|---|
| Strata | searchable (the server answers a batch once its rows are visible) | 19 | 0 (the consumer resumes after the offsets the server keeps with the rows (GET /v1/sources)) | 0 |
| Vespa | searchable (a put is visible when it returns) | 21 | 399 (the consumer resumes after the offsets it committed, every second, as Kafka's consumers do) | 0 |

## CPU used by each engine's containers (cgroup cpu.stat)

| engine | phase | pod | CPU seconds | wall seconds | average cores | throttled (s) |
|---|---|---|---|---|---|---|
| Strata | timed run | strata-0 | 704 | 371 | 1.90 | 7 |
| Strata | timed run | strata-1 | 675 | 370 | 1.82 | 3 |
| Strata | writes | strata-0 | 85 | 70 | 1.22 | 1 |
| Strata | writes | strata-1 | 75 | 71 | 1.06 | 0 |
| Strata | stream | strata-0 | 172 | 156 | 1.10 | 2 |
| Strata | stream | strata-1 | 155 | 157 | 0.99 | 1 |
| Vespa | load | vespa-0 | 1399 | 1409 | 0.99 | 4936 |
| Vespa | load | vespa-1 | 2241 | 1409 | 1.59 | 37 |
| Vespa | load | vespa-2 | 2075 | 1410 | 1.47 | 22 |
| Vespa | timed run | vespa-0 | 293 | 393 | 0.75 | 451 |
| Vespa | timed run | vespa-1 | 482 | 393 | 1.23 | 21 |
| Vespa | timed run | vespa-2 | 479 | 393 | 1.22 | 9 |
| Vespa | writes | vespa-0 | 72 | 91 | 0.80 | 160 |
| Vespa | writes | vespa-1 | 76 | 91 | 0.83 | 0 |
| Vespa | writes | vespa-2 | 68 | 90 | 0.76 | 0 |
| Vespa | stream | vespa-0 | 156 | 204 | 0.76 | 322 |
| Vespa | stream | vespa-1 | 222 | 204 | 1.09 | 0 |
| Vespa | stream | vespa-2 | 202 | 204 | 0.99 | 0 |

## Memory of each engine's containers (cgroup memory.current, memory.stat, memory.peak; GB, all its pods)

| engine | after the load | of it anonymous | after the timed run | of it anonymous | of it files (page cache) | pods' peaks added |
|---|---|---|---|---|---|---|
| Strata | – | – | 0.67 | 0.40 | 0.06 | 2.64 |
| Vespa | 10.86 | 6.26 | 9.68 | 4.68 | 4.79 | 15.77 |

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

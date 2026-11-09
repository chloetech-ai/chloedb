## Load time and index size

| engine | load (s) | of which: send / build | searchable | merge to 1 segment | index size (GB) | documents |
|---|---|---|---|---|---|---|
| Strata | 4468 | load 3982 + merge 4327 | (built) | (built as one) | 48.93 | 10,000,000 |

## 1 client: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms |
|---|---|---|
| bm25 | 193 | 4.12 / 8.53 / 20.76 |
| bm25 + filter | 98 | 8.59 / 18.57 / 31.04 |
| phrase | 192 | 3.83 / 9.32 / 18.77 |
| typo | 1 | 939.53 / 1163.91 / 1670.49 |
| prefix | 79 | 11.14 / 20.90 / 41.35 |
| vector | n/a | n/a |
| vector + filter | n/a | n/a |
| hybrid (RRF) | n/a | n/a |
| hybrid (weighted) | n/a | n/a |
| boosted (bm25 × decay) | 54 | 14.78 / 32.86 / 71.44 |
| similar items | 209 | 4.69 / 5.54 / 6.66 |
| facet | 258 | 3.04 / 6.69 / 12.80 |
| key lookup | 1,074 | 0.91 / 1.07 / 1.36 |
| sql aggregate | 0 | 4057.54 / 4057.54 / 4057.54 |


## 4 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms |
|---|---|---|
| bm25 | 429 | 7.29 / 15.91 / 39.82 |
| bm25 + filter | 202 | 16.43 / 37.65 / 65.69 |
| phrase | 426 | 6.60 / 17.53 / 37.52 |
| typo | 2 | 1549.06 / 2298.31 / 3207.49 |
| prefix | 152 | 22.84 / 43.42 / 82.14 |
| vector | n/a | n/a |
| vector + filter | n/a | n/a |
| hybrid (RRF) | n/a | n/a |
| hybrid (weighted) | n/a | n/a |
| boosted (bm25 × decay) | 110 | 28.91 / 66.31 / 141.88 |
| similar items | 474 | 8.17 / 10.59 / 13.76 |
| facet | 700 | 4.68 / 10.04 / 17.92 |
| key lookup | 3,930 | 0.97 / 1.27 / 1.64 |
| sql aggregate | 0 | 0.00 / 0.00 / 0.00 |


## 16 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms |
|---|---|---|
| bm25 | 509 | 23.92 / 55.47 / 134.81 |
| bm25 + filter | 226 | 56.61 / 135.59 / 275.75 |
| phrase | 534 | 21.70 / 59.57 / 110.35 |
| typo | 1 | 4144.17 / 4893.77 / 4907.09 |
| prefix | 146 | 90.17 / 203.17 / 390.62 |
| vector | n/a | n/a |
| vector + filter | n/a | n/a |
| hybrid (RRF) | n/a | n/a |
| hybrid (weighted) | n/a | n/a |
| boosted (bm25 × decay) | 103 | 109.23 / 285.41 / 843.23 |
| similar items | 523 | 27.83 / 48.32 / 66.10 |
| facet | 937 | 13.37 / 32.70 / 56.16 |
| key lookup | 5,381 | 2.82 / 3.99 / 4.86 |
| sql aggregate | 0 | 0.00 / 0.00 / 0.00 |


Errors during the timed runs: Strata 60

## Answers: recall@10 against an exact search, nDCG@10 against the collection's judgments

Unavailable kinds are n/a; recall is for the kinds an exact search can answer.

| kind | Strata recall | Strata nDCG |
|---|---|---|
| bm25 | – | – |
| bm25 + filter | – | – |
| phrase | – | – |
| typo | – | – |
| prefix | – | – |
| vector | n/a | n/a |
| vector + filter | n/a | n/a |
| hybrid (RRF) | n/a | n/a |
| hybrid (weighted) | n/a | n/a |
| boosted (bm25 × decay) | – | – |
| similar items | 0.978 | – |


## SQL aggregates equal to the true answer

| Strata |
|---|
| 300 of 300 |

## Agreement with Strata's answers


## Writes: documents a second, and latency in ms (p50 / p99)

| phase | clients | Strata docs/s | Strata ms |
|---|---|---|---|
| insert one | 1 | 240 | 3.74 / 12.37 |
| insert one | 4 | 792 | 3.83 / 31.84 |
| insert one | 16 | 2,083 | 6.17 / 78.08 |
| insert batch | 1 | 1,663 | 35.84 / 246.05 |
| insert batch | 4 | 3,428 | 87.63 / 307.64 |
| insert batch | 16 | 4,001 | 354.32 / 830.92 |
| update one | 1 | 231 | 3.58 / 16.99 |
| update one | 4 | 852 | 3.86 / 15.84 |
| update one | 16 | 2,694 | 4.83 / 18.11 |

## How soon a written document is found (ms from the write's start)

| engine | searchable | write returned (p50) | found (p50) | found (p90) | found (max) |
|---|---|---|---|---|---|
| Strata | searchable when the write returns | 4.0 | 8.3 | 10.1 | 130.3 |

## Streams through Redpanda: documents written a second, and lag in ms from when a document was due (p50 / p99)

| phase | Strata docs/s | Strata lag |
|---|---|---|
| 250/s | 249 | 78 / 122 |
| 500/s | 498 | 78 / 167 |
| 1000/s | 996 | 80 / 234 |
| 2000/s | 1,992 | 99 / 549 |
| flat out | 2,976 | 2735 / 5938 |

Lag includes reading the topic (Redpanda's part, the same for every engine, is the 'read' column below).

| engine | what written means | read lag p50 (ms) | crash: events written again | documents lost |
|---|---|---|---|---|
| Strata | searchable (the server answers a batch once its rows are visible) | 34 | 0 (the consumer resumes after the offsets the server keeps with the rows (GET /v1/sources)) | 0 |

## CPU used by each engine's containers (cgroup cpu.stat)

| engine | phase | pod | CPU seconds | wall seconds | average cores | throttled (s) |
|---|---|---|---|---|---|---|
| Strata | timed run | strata-0 | 1501 | 1213 | 1.24 | 67 |
| Strata | timed run | strata-1 | 1364 | 1213 | 1.12 | 16 |
| Strata | timed run | strata-coordinator | 81 | 1214 | 0.07 | 0 |
| Strata | writes | strata-0 | 50 | 71 | 0.70 | 2 |
| Strata | writes | strata-1 | 45 | 70 | 0.65 | 0 |
| Strata | writes | strata-coordinator | 22 | 70 | 0.31 | 0 |
| Strata | stream | strata-0 | 88 | 153 | 0.58 | 4 |
| Strata | stream | strata-1 | 82 | 154 | 0.53 | 0 |
| Strata | stream | strata-coordinator | 28 | 154 | 0.18 | 0 |

## Memory of each engine's containers (cgroup memory.current, memory.stat, memory.peak; GB, all its pods)

| engine | after the load | of it anonymous | after the timed run | of it anonymous | of it files (page cache) | pods' peaks added |
|---|---|---|---|---|---|---|
| Strata | – | – | 32.06 | 2.39 | 29.57 | 33.90 |

- Strata, hybrid (RRF): unavailable: "shard 0 (http://strata-0.strata:7701): no answer in time"

- Strata, hybrid (weighted): unavailable: "shard 0 (http://strata-0.strata:7701): no answer in time"

- Strata, vector: unavailable: "shard 0 (http://strata-0.strata:7701): no answer in time"

- Strata, vector + filter: unavailable: "shard 0 (http://strata-0.strata:7701): no answer in time"

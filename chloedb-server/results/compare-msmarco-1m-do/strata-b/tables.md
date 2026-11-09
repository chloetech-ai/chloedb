## Load time and index size

| engine | load (s) | of which: send / build | searchable | merge to 1 segment | index size (GB) | documents |
|---|---|---|---|---|---|---|
| Strata | 344 | read 15 + build 329 | (built) | (built as one) | 4.91 | 1,000,000 |

## 1 client: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms |
|---|---|---|
| bm25 | 375 | 2.48 / 3.42 / 5.47 |
| bm25 + filter | 274 | 3.29 / 5.53 / 8.26 |
| phrase | 384 | 2.42 / 3.39 / 5.28 |
| typo | 8 | 125.67 / 206.74 / 294.08 |
| prefix | 254 | 3.64 / 5.42 / 8.44 |
| vector | 488 | 2.02 / 2.32 / 2.80 |
| vector + filter | 438 | 2.23 / 2.61 / 3.16 |
| hybrid (RRF) | 193 | 4.98 / 6.34 / 9.54 |
| hybrid (weighted) | 195 | 4.88 / 6.38 / 9.77 |
| boosted (bm25 × decay) | 166 | 5.34 / 9.23 / 14.20 |
| similar items | 405 | 2.41 / 2.83 / 3.46 |
| facet | 548 | 1.77 / 2.29 / 2.84 |
| key lookup | 787 | 1.19 / 1.58 / 2.05 |
| sql aggregate | 6 | 190.41 / 281.73 / 317.72 |


## 4 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms |
|---|---|---|
| bm25 | 985 | 3.84 / 5.32 / 8.04 |
| bm25 + filter | 749 | 4.92 / 7.82 / 11.39 |
| phrase | 952 | 4.00 / 5.37 / 7.54 |
| typo | 16 | 235.52 / 405.39 / 516.39 |
| prefix | 662 | 5.69 / 8.56 / 12.25 |
| vector | 1,193 | 3.25 / 4.28 / 5.37 |
| vector + filter | 1,130 | 3.44 / 4.43 / 5.80 |
| hybrid (RRF) | 409 | 9.52 / 12.71 / 15.72 |
| hybrid (weighted) | 416 | 9.35 / 12.70 / 16.23 |
| boosted (bm25 × decay) | 434 | 8.06 / 14.46 / 25.01 |
| similar items | 1,113 | 3.45 / 4.53 / 5.96 |
| facet | 1,422 | 2.68 / 3.75 / 5.00 |
| key lookup | 2,648 | 1.44 / 1.91 / 2.50 |
| sql aggregate | 10 | 399.59 / 608.50 / 738.18 |


## 16 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms |
|---|---|---|
| bm25 | 1,329 | 11.88 / 14.92 / 19.06 |
| bm25 + filter | 1,213 | 12.54 / 17.69 / 24.65 |
| phrase | 1,255 | 12.62 / 15.36 / 18.40 |
| typo | 15 | 991.14 / 1714.85 / 1936.64 |
| prefix | 1,030 | 14.47 / 22.43 / 31.81 |
| vector | 1,483 | 10.45 / 13.79 / 17.01 |
| vector + filter | 1,462 | 10.63 / 13.98 / 17.70 |
| hybrid (RRF) | 554 | 28.14 / 36.58 / 48.31 |
| hybrid (weighted) | 570 | 27.37 / 35.98 / 44.78 |
| boosted (bm25 × decay) | 576 | 24.12 / 44.85 / 78.70 |
| similar items | 1,340 | 11.52 / 14.98 / 19.24 |
| facet | 1,921 | 8.04 / 10.62 / 13.32 |
| key lookup | 3,367 | 4.65 / 6.02 / 7.43 |
| sql aggregate | 10 | 1479.12 / 2413.99 / 2643.18 |


Errors during the timed runs: Strata 0

## Answers: recall@10 against an exact search, nDCG@10 against the collection's judgments

Unavailable kinds are n/a; recall is for the kinds an exact search can answer.

| kind | Strata recall | Strata nDCG |
|---|---|---|
| bm25 | – | – |
| bm25 + filter | – | – |
| phrase | – | – |
| typo | – | – |
| prefix | – | – |
| vector | 0.802 | – |
| vector + filter | 0.828 | – |
| hybrid (RRF) | – | – |
| hybrid (weighted) | – | – |
| boosted (bm25 × decay) | – | – |
| similar items | 0.992 | – |


## SQL aggregates equal to the true answer

| Strata |
|---|
| 300 of 300 |

## Agreement with Strata's answers


## Writes: documents a second, and latency in ms (p50 / p99)

| phase | clients | Strata docs/s | Strata ms |
|---|---|---|---|
| insert one | 1 | 321 | 3.04 / 10.87 |
| insert one | 4 | 1,036 | 3.17 / 14.42 |
| insert one | 16 | 2,150 | 5.74 / 64.74 |
| insert batch | 1 | 1,957 | 34.23 / 149.76 |
| insert batch | 4 | 3,366 | 91.92 / 317.12 |
| insert batch | 16 | 3,971 | 398.97 / 699.31 |
| update one | 1 | 397 | 2.51 / 5.88 |
| update one | 4 | 1,531 | 2.44 / 6.55 |
| update one | 16 | 4,738 | 3.30 / 7.16 |

## How soon a written document is found (ms from the write's start)

| engine | searchable | write returned (p50) | found (p50) | found (p90) | found (max) |
|---|---|---|---|---|---|
| Strata | searchable when the write returns | 2.5 | 6.4 | 8.0 | 91.7 |

## Streams through Redpanda: documents written a second, and lag in ms from when a document was due (p50 / p99)

| phase | Strata docs/s | Strata lag |
|---|---|---|
| 250/s | 249 | 80 / 106 |
| 500/s | 498 | 77 / 124 |
| 1000/s | 997 | 79 / 171 |
| 2000/s | 1,993 | 85 / 312 |
| flat out | 3,556 | 2828 / 4809 |

Lag includes reading the topic (Redpanda's part, the same for every engine, is the 'read' column below).

| engine | what written means | read lag p50 (ms) | crash: events written again | documents lost |
|---|---|---|---|---|
| Strata | searchable (the server answers a batch once its rows are visible) | 32 | 0 (the consumer resumes after the offsets the server keeps with the rows (GET /v1/sources)) | 0 |

## CPU used by each engine's containers (cgroup cpu.stat)

| engine | phase | pod | CPU seconds | wall seconds | average cores | throttled (s) |
|---|---|---|---|---|---|---|
| Strata | timed run | strata-0 | 403 | 370 | 1.09 | 10 |
| Strata | timed run | strata-1 | 467 | 370 | 1.26 | 25 |
| Strata | timed run | strata-coordinator | 157 | 371 | 0.42 | 0 |
| Strata | writes | strata-0 | 42 | 70 | 0.60 | 1 |
| Strata | writes | strata-1 | 51 | 70 | 0.73 | 3 |
| Strata | writes | strata-coordinator | 27 | 69 | 0.39 | 0 |
| Strata | stream | strata-0 | 65 | 152 | 0.43 | 2 |
| Strata | stream | strata-1 | 80 | 152 | 0.53 | 4 |
| Strata | stream | strata-coordinator | 29 | 152 | 0.19 | 0 |

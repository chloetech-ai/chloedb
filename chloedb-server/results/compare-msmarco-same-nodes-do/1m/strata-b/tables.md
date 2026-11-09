## Load time and index size

| engine | load (s) | of which: send / build | searchable | merge to 1 segment | index size (GB) | documents |
|---|---|---|---|---|---|---|
| Strata | 191 | load 341 + merge 28 | (built) | (built as one) | 4.91 | 1,000,000 |

## 1 client: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms |
|---|---|---|
| bm25 | 518 | 1.75 / 2.63 / 5.16 |
| bm25 + filter | 358 | 2.48 / 4.50 / 7.41 |
| phrase | 551 | 1.67 / 2.45 / 4.34 |
| typo | 53 | 19.74 / 26.46 / 36.61 |
| prefix | 319 | 2.85 / 4.66 / 7.28 |
| vector | 554 | 1.76 / 2.06 / 2.43 |
| vector + filter | 469 | 2.08 / 2.40 / 2.93 |
| hybrid (RRF) | 228 | 4.12 / 5.63 / 9.69 |
| hybrid (weighted) | 226 | 4.17 / 5.66 / 9.34 |
| boosted (bm25 × decay) | 187 | 4.53 / 8.82 / 16.19 |
| similar items | 428 | 2.29 / 2.59 / 3.33 |
| facet | 597 | 1.63 / 2.10 / 2.57 |
| key lookup | 1,175 | 0.83 / 1.10 / 1.40 |
| sql aggregate | 5 | 187.46 / 318.65 / 351.35 |


## 4 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms |
|---|---|---|
| bm25 | 1,435 | 2.50 / 4.02 / 6.94 |
| bm25 + filter | 1,010 | 3.44 / 6.60 / 11.05 |
| phrase | 1,450 | 2.56 / 3.82 / 6.00 |
| typo | 133 | 30.40 / 43.73 / 60.15 |
| prefix | 884 | 4.10 / 7.11 / 11.04 |
| vector | 1,371 | 2.87 / 3.59 / 4.41 |
| vector + filter | 1,261 | 3.04 / 3.93 / 5.24 |
| hybrid (RRF) | 555 | 6.68 / 9.95 / 15.08 |
| hybrid (weighted) | 567 | 6.58 / 9.68 / 14.74 |
| boosted (bm25 × decay) | 486 | 7.01 / 13.95 / 25.59 |
| similar items | 1,145 | 3.38 / 4.26 / 5.57 |
| facet | 1,488 | 2.61 / 3.46 / 4.48 |
| key lookup | 3,741 | 1.03 / 1.38 / 1.81 |
| sql aggregate | 10 | 381.62 / 610.54 / 708.41 |


## 16 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms |
|---|---|---|
| bm25 | 1,877 | 8.20 / 10.78 / 15.31 |
| bm25 + filter | 1,516 | 9.18 / 16.57 / 28.34 |
| phrase | 2,040 | 7.60 / 9.72 / 13.16 |
| typo | 126 | 132.03 / 194.24 / 270.39 |
| prefix | 1,200 | 11.26 / 23.01 / 36.62 |
| vector | 1,712 | 9.20 / 11.24 / 13.65 |
| vector + filter | 1,529 | 10.20 / 12.76 / 16.10 |
| hybrid (RRF) | 703 | 21.82 / 29.32 / 41.92 |
| hybrid (weighted) | 703 | 21.87 / 29.05 / 41.78 |
| boosted (bm25 × decay) | 581 | 22.49 / 48.74 / 99.50 |
| similar items | 1,263 | 12.49 / 15.32 / 18.92 |
| facet | 1,928 | 8.13 / 10.10 / 12.29 |
| key lookup | 5,771 | 2.73 / 3.47 / 4.35 |
| sql aggregate | 10 | 1557.25 / 2225.16 / 2613.42 |


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
| vector | 0.807 | – |
| vector + filter | 0.828 | – |
| hybrid (RRF) | – | – |
| hybrid (weighted) | – | – |
| boosted (bm25 × decay) | – | – |
| similar items | 0.990 | – |


## SQL aggregates equal to the true answer

| Strata |
|---|
| 300 of 300 |

## Agreement with Strata's answers


## CPU used by each engine's containers (cgroup cpu.stat)

| engine | phase | pod | CPU seconds | wall seconds | average cores | throttled (s) |
|---|---|---|---|---|---|---|
| Strata | timed run | strata-0 | 465 | 435 | 1.07 | 8 |
| Strata | timed run | strata-1 | 594 | 435 | 1.36 | 19 |
| Strata | timed run | strata-coordinator | 221 | 435 | 0.51 | 0 |

## Memory of each engine's containers (cgroup memory.current, memory.stat, memory.peak; GB, all its pods)

| engine | after the load | of it anonymous | after the timed run | of it anonymous | of it files (page cache) | pods' peaks added |
|---|---|---|---|---|---|---|
| Strata | – | – | 3.70 | 0.51 | 3.17 | 3.72 |

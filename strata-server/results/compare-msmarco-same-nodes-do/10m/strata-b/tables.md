## Load time and index size

| engine | load (s) | of which: send / build | searchable | merge to 1 segment | index size (GB) | documents |
|---|---|---|---|---|---|---|
| Strata | 4056 | load 3919 + merge 3789 | (built) | (built as one) | 48.93 | 10,000,000 |

## 1 client: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms |
|---|---|---|
| bm25 | 253 | 2.74 / 7.43 / 22.32 |
| bm25 + filter | 130 | 6.10 / 14.97 / 28.41 |
| phrase | 265 | 2.39 / 7.40 / 16.04 |
| typo | 13 | 80.04 / 104.63 / 147.42 |
| prefix | 97 | 8.64 / 17.88 / 33.86 |
| vector | n/a | n/a |
| vector + filter | 340 | 2.88 / 3.42 / 4.17 |
| hybrid (RRF) | 106 | 7.79 / 14.76 / 37.53 |
| hybrid (weighted) | 106 | 7.69 / 14.72 / 39.98 |
| boosted (bm25 × decay) | 63 | 12.14 / 29.25 / 61.05 |
| similar items | 274 | 3.58 / 4.25 / 5.76 |
| facet | 330 | 2.16 / 5.55 / 10.90 |
| key lookup | 1,240 | 0.77 / 0.90 / 1.28 |
| sql aggregate | 0 | 3793.87 / 3793.87 / 3793.87 |


## 4 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms |
|---|---|---|
| bm25 | 530 | 5.00 / 15.47 / 42.28 |
| bm25 + filter | 246 | 12.08 / 33.58 / 68.44 |
| phrase | 563 | 4.49 / 15.19 / 34.53 |
| typo | 26 | 153.95 / 236.26 / 313.90 |
| prefix | 175 | 18.76 / 40.92 / 76.03 |
| vector | n/a | n/a |
| vector + filter | 753 | 5.10 / 6.97 / 9.32 |
| hybrid (RRF) | 225 | 13.70 / 31.33 / 71.58 |
| hybrid (weighted) | 225 | 14.11 / 30.08 / 77.68 |
| boosted (bm25 × decay) | 110 | 26.63 / 67.49 / 166.86 |
| similar items | 588 | 6.52 / 8.78 / 11.75 |
| facet | 871 | 3.51 / 8.84 / 15.58 |
| key lookup | 4,370 | 0.88 / 1.12 / 1.49 |
| sql aggregate | 1 | 5621.01 / 5980.18 / 6288.93 |


## 16 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms |
|---|---|---|
| bm25 | 513 | 17.56 / 69.91 / 204.40 |
| bm25 + filter | 234 | 45.60 / 153.22 / 327.13 |
| phrase | 614 | 13.65 / 61.01 / 139.91 |
| typo | 24 | 621.31 / 1017.04 / 1359.24 |
| prefix | 161 | 77.87 / 196.67 / 405.98 |
| vector | n/a | n/a |
| vector + filter | 835 | 17.46 / 30.89 / 47.46 |
| hybrid (RRF) | 210 | 58.48 / 147.17 / 388.46 |
| hybrid (weighted) | 212 | 56.34 / 143.83 / 377.20 |
| boosted (bm25 × decay) | 96 | 108.62 / 361.42 / 843.50 |
| similar items | 642 | 23.28 / 39.50 / 55.54 |
| facet | 1,143 | 10.20 / 27.62 / 50.15 |
| key lookup | 6,689 | 2.31 / 3.04 / 3.87 |
| sql aggregate | 1 | 18513.49 / 19603.92 / 19725.69 |


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
| vector | n/a | n/a |
| vector + filter | – | – |
| hybrid (RRF) | – | – |
| hybrid (weighted) | – | – |
| boosted (bm25 × decay) | – | – |
| similar items | 0.968 | – |


## SQL aggregates equal to the true answer

| Strata |
|---|
| 300 of 300 |

## Agreement with Strata's answers


## CPU used by each engine's containers (cgroup cpu.stat)

| engine | phase | pod | CPU seconds | wall seconds | average cores | throttled (s) |
|---|---|---|---|---|---|---|
| Strata | timed run | strata-0 | 1351 | 962 | 1.40 | 76 |
| Strata | timed run | strata-1 | 1118 | 962 | 1.16 | 14 |
| Strata | timed run | strata-coordinator | 99 | 962 | 0.10 | 0 |

## Memory of each engine's containers (cgroup memory.current, memory.stat, memory.peak; GB, all its pods)

| engine | after the load | of it anonymous | after the timed run | of it anonymous | of it files (page cache) | pods' peaks added |
|---|---|---|---|---|---|---|
| Strata | – | – | 48.66 | 1.41 | 47.11 | 48.72 |

- Strata, vector: unavailable: error sending request for url (http://strata-coordinator:7700/v1/query)

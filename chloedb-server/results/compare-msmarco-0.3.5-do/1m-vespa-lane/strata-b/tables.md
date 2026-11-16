## Load time and index size

| engine | load (s) | of which: send / build | searchable | merge to 1 segment | index size (GB) | documents |
|---|---|---|---|---|---|---|
| Strata | 405 | load 704 + merge 35 | (built) | (built as one) | 5.00 | 1,000,000 |

## 1 client: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms |
|---|---|---|
| bm25 | 417 | 2.17 / 3.24 / 6.08 |
| bm25 + filter | 361 | 2.49 / 3.83 / 7.51 |
| phrase | 544 | 1.80 / 2.09 / 3.06 |
| typo | 57 | 18.27 / 26.41 / 31.92 |
| prefix | 380 | 2.38 / 3.73 / 5.44 |
| vector | 408 | 2.40 / 2.68 / 3.63 |
| vector + filter | 398 | 2.45 / 2.73 / 3.69 |
| hybrid (RRF) | 180 | 5.28 / 6.98 / 11.44 |
| hybrid (weighted) | 176 | 5.34 / 7.12 / 10.91 |
| boosted (bm25 × decay) | 206 | 4.17 / 7.96 / 13.28 |
| similar items | 310 | 3.15 / 3.55 / 4.69 |
| facet | 499 | 1.98 / 2.31 / 3.09 |
| key lookup | 1,050 | 1.01 / 1.21 / 1.65 |
| sql aggregate | 116 | 8.56 / 11.16 / 14.24 |


## 4 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms |
|---|---|---|
| bm25 | 1,302 | 2.71 / 4.60 / 8.77 |
| bm25 + filter | 1,094 | 3.21 / 5.49 / 10.72 |
| phrase | 2,067 | 1.86 / 2.34 / 3.29 |
| typo | 146 | 28.06 / 40.19 / 50.52 |
| prefix | 1,169 | 3.06 / 5.16 / 7.87 |
| vector | 1,288 | 2.99 / 3.86 / 5.15 |
| vector + filter | 1,215 | 3.15 / 4.12 / 5.76 |
| hybrid (RRF) | 503 | 7.42 / 11.12 / 17.55 |
| hybrid (weighted) | 497 | 7.49 / 11.10 / 18.37 |
| boosted (bm25 × decay) | 593 | 5.82 / 11.08 / 18.69 |
| similar items | 923 | 4.15 / 5.52 / 7.59 |
| facet | 1,776 | 2.16 / 2.83 / 4.05 |
| key lookup | 4,096 | 1.01 / 1.30 / 1.83 |
| sql aggregate | 277 | 14.08 / 20.65 / 25.92 |


## 16 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms |
|---|---|---|
| bm25 | 2,253 | 6.39 / 11.09 / 19.15 |
| bm25 + filter | 1,942 | 7.38 / 12.78 / 22.50 |
| phrase | 4,307 | 3.53 / 4.96 / 7.98 |
| typo | 170 | 89.90 / 149.51 / 207.05 |
| prefix | 2,096 | 6.99 / 11.75 / 18.16 |
| vector | 2,204 | 6.81 / 10.40 / 15.13 |
| vector + filter | 2,116 | 7.09 / 10.74 / 15.43 |
| hybrid (RRF) | 716 | 20.78 / 33.51 / 52.94 |
| hybrid (weighted) | 718 | 20.88 / 33.57 / 47.81 |
| boosted (bm25 × decay) | 843 | 16.14 / 32.81 / 57.47 |
| similar items | 1,486 | 10.09 / 15.63 / 22.74 |
| facet | 3,585 | 4.13 / 6.26 / 10.59 |
| key lookup | 11,692 | 1.35 / 2.03 / 3.03 |
| sql aggregate | 310 | 49.32 / 80.34 / 104.38 |


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
| vector + filter | 0.862 | – |
| hybrid (RRF) | – | – |
| hybrid (weighted) | – | – |
| boosted (bm25 × decay) | – | – |
| similar items | 0.994 | – |


## SQL aggregates equal to the true answer

| Strata |
|---|
| 300 of 300 |

## Agreement with Strata's answers


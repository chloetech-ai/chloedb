## Load time and index size

| engine | load (s) | of which: send / build | searchable | merge to 1 segment | index size (GB) | documents |
|---|---|---|---|---|---|---|
| Strata | 5572 | load 6114 + merge 4969 | (built) | (built as one) | 49.75 | 10,000,000 |

## 1 client: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms |
|---|---|---|
| bm25 | 229 | 3.33 / 7.48 / 19.62 |
| bm25 + filter | 203 | 3.78 / 8.40 / 24.58 |
| phrase | 496 | 1.94 / 2.39 / 3.32 |
| typo | 17 | 54.45 / 86.46 / 106.77 |
| prefix | 156 | 5.00 / 11.93 / 23.28 |
| vector | 368 | 2.63 / 3.09 / 4.12 |
| vector + filter | 346 | 2.79 / 3.31 / 4.39 |
| hybrid (RRF) | 109 | 7.75 / 14.03 / 30.58 |
| hybrid (weighted) | 108 | 7.80 / 13.72 / 30.44 |
| boosted (bm25 × decay) | 96 | 8.35 / 18.67 / 37.98 |
| similar items | 253 | 3.79 / 4.71 / 6.72 |
| facet | 315 | 2.72 / 4.84 / 7.56 |
| key lookup | 1,048 | 1.00 / 1.22 / 1.69 |
| sql aggregate | 28 | 36.09 / 51.38 / 58.26 |


## 4 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms |
|---|---|---|
| bm25 | 535 | 5.33 / 14.53 / 39.49 |
| bm25 + filter | 465 | 6.21 / 15.91 / 42.76 |
| phrase | 1,553 | 2.40 / 3.53 / 5.12 |
| typo | 36 | 109.55 / 167.23 / 231.51 |
| prefix | 288 | 9.81 / 28.64 / 59.94 |
| vector | 855 | 4.43 / 6.12 / 8.64 |
| vector + filter | 830 | 4.64 / 6.16 / 8.30 |
| hybrid (RRF) | 229 | 14.03 / 28.93 / 74.99 |
| hybrid (weighted) | 226 | 14.25 / 29.51 / 73.11 |
| boosted (bm25 × decay) | 211 | 14.32 / 35.67 / 79.22 |
| similar items | 577 | 6.63 / 9.05 / 11.88 |
| facet | 980 | 3.64 / 6.54 / 10.44 |
| key lookup | 4,180 | 0.99 / 1.27 / 1.67 |
| sql aggregate | 40 | 99.11 / 146.67 / 168.09 |


## 16 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms |
|---|---|---|
| bm25 | 667 | 16.82 / 47.49 / 130.14 |
| bm25 + filter | 558 | 19.36 / 56.37 / 161.73 |
| phrase | 2,907 | 4.98 / 8.36 / 13.25 |
| typo | 35 | 404.28 / 789.41 / 1088.14 |
| prefix | 326 | 34.25 / 105.40 / 214.77 |
| vector | 1,241 | 12.16 / 18.80 / 26.13 |
| vector + filter | 1,173 | 12.88 / 19.77 / 27.99 |
| hybrid (RRF) | 257 | 48.27 / 112.92 / 279.39 |
| hybrid (weighted) | 256 | 48.94 / 106.64 / 290.92 |
| boosted (bm25 × decay) | 220 | 49.30 / 144.40 / 390.03 |
| similar items | 769 | 19.88 / 29.79 / 41.95 |
| facet | 1,602 | 9.02 / 16.34 / 26.15 |
| key lookup | 12,018 | 1.31 / 1.97 / 2.86 |
| sql aggregate | 37 | 428.12 / 670.38 / 857.02 |


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
| vector | 0.798 | – |
| vector + filter | 0.816 | – |
| hybrid (RRF) | – | – |
| hybrid (weighted) | – | – |
| boosted (bm25 × decay) | – | – |
| similar items | 0.981 | – |


## SQL aggregates equal to the true answer

| Strata |
|---|
| 300 of 300 |

## Agreement with Strata's answers


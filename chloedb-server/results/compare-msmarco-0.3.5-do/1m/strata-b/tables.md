## Load time and index size

| engine | load (s) | of which: send / build | searchable | merge to 1 segment | index size (GB) | documents |
|---|---|---|---|---|---|---|
| Strata | 369 | load 555 + merge 33 | (built) | (built as one) | 5.00 | 1,000,000 |

## 1 client: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms |
|---|---|---|
| bm25 | 400 | 2.23 / 3.41 / 6.55 |
| bm25 + filter | 344 | 2.60 / 3.99 / 7.79 |
| phrase | 549 | 1.80 / 2.05 / 2.53 |
| typo | 60 | 16.46 / 24.70 / 29.30 |
| prefix | 391 | 2.33 / 3.52 / 5.26 |
| vector | 428 | 2.28 / 2.57 / 3.50 |
| vector + filter | 407 | 2.39 / 2.66 / 4.10 |
| hybrid (RRF) | 194 | 4.85 / 6.59 / 10.94 |
| hybrid (weighted) | 191 | 4.90 / 6.82 / 10.94 |
| boosted (bm25 × decay) | 225 | 3.82 / 7.14 / 12.10 |
| similar items | 323 | 3.01 / 3.40 / 4.97 |
| facet | 507 | 1.93 / 2.27 / 3.24 |
| key lookup | 1,012 | 1.08 / 1.32 / 1.72 |
| sql aggregate | 131 | 7.54 / 9.91 / 12.45 |


## 4 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms |
|---|---|---|
| bm25 | 1,316 | 2.68 / 4.53 / 8.27 |
| bm25 + filter | 1,187 | 3.01 / 4.94 / 9.65 |
| phrase | 2,141 | 1.81 / 2.21 / 2.97 |
| typo | 156 | 25.44 / 38.46 / 52.97 |
| prefix | 1,283 | 2.82 / 4.55 / 6.82 |
| vector | 1,430 | 2.68 / 3.44 / 4.70 |
| vector + filter | 1,369 | 2.80 / 3.62 / 4.76 |
| hybrid (RRF) | 562 | 6.62 / 10.03 / 15.28 |
| hybrid (weighted) | 555 | 6.70 / 10.02 / 15.43 |
| boosted (bm25 × decay) | 648 | 5.16 / 10.23 / 19.67 |
| similar items | 1,035 | 3.69 / 4.89 / 6.63 |
| facet | 1,930 | 2.01 / 2.56 / 3.38 |
| key lookup | 4,166 | 1.03 / 1.31 / 1.72 |
| sql aggregate | 321 | 12.01 / 17.69 / 23.44 |


## 16 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Strata q/s | Strata ms |
|---|---|---|
| bm25 | 2,270 | 6.21 / 11.19 / 20.14 |
| bm25 + filter | 2,075 | 6.82 / 12.08 / 22.02 |
| phrase | 4,619 | 3.25 / 4.64 / 7.81 |
| typo | 178 | 85.73 / 146.21 / 195.73 |
| prefix | 2,274 | 6.39 / 10.93 / 17.00 |
| vector | 2,385 | 6.12 / 10.03 / 15.31 |
| vector + filter | 2,317 | 6.39 / 10.12 / 15.72 |
| hybrid (RRF) | 775 | 19.21 / 31.67 / 50.26 |
| hybrid (weighted) | 766 | 19.44 / 31.95 / 49.82 |
| boosted (bm25 × decay) | 889 | 14.78 / 31.22 / 60.47 |
| similar items | 1,579 | 9.48 / 15.11 / 21.98 |
| facet | 3,885 | 3.79 / 5.80 / 10.32 |
| key lookup | 12,342 | 1.30 / 1.95 / 2.73 |
| sql aggregate | 354 | 42.60 / 73.35 / 100.14 |


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
| vector | 0.809 | – |
| vector + filter | 0.862 | – |
| hybrid (RRF) | – | – |
| hybrid (weighted) | – | – |
| boosted (bm25 × decay) | – | – |
| similar items | 0.995 | – |


## SQL aggregates equal to the true answer

| Strata |
|---|
| 300 of 300 |

## Agreement with Strata's answers


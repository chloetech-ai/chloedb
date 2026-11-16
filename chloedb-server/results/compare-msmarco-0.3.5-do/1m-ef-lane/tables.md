## Load time and index size

| engine | load (s) | of which: send / build | searchable | merge to 1 segment | index size (GB) | documents |
|---|---|---|---|---|---|---|
| Qdrant (one node) | 4531 | send 387 | – | index 4144 | 8.69 | 1,000,000 |
| Postgres (one node) | 1393 | send 143 | – | analyze 1, gin index 74, hnsw index 1171, primary key 4 | 16.36 | 1,000,000 |

## 1 client: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Qdrant q/s | Qdrant ms | Postgres q/s | Postgres ms |
|---|---|---|---|---|
| bm25 | 585 | 1.63 / 2.73 / 3.57 | 4 | 291.97 / 456.00 / 1011.39 |
| bm25 + filter | 515 | 1.82 / 2.93 / 4.39 | 5 | 177.02 / 284.47 / 872.06 |
| phrase | 353 | 1.34 / 5.50 / 31.62 | 80 | 4.97 / 26.82 / 101.00 |
| typo | n/a | n/a | n/a | n/a |
| prefix | n/a | n/a | 3 | 301.96 / 620.34 / 859.21 |
| vector | 502 | 1.93 / 2.34 / 2.93 | 275 | 3.46 / 4.82 / 6.88 |
| vector + filter | 273 | 3.64 / 4.27 / 5.23 | 239 | 3.82 / 5.83 / 10.26 |
| hybrid (RRF) | 97 | 9.67 / 14.06 / 16.02 | 4 | 240.57 / 410.81 / 1018.29 |
| hybrid (weighted) | 95 | 9.88 / 14.21 / 16.65 | 4 | 260.13 / 433.14 / 1001.77 |
| boosted (bm25 × decay) | 476 | 2.03 / 3.11 / 4.00 | 2 | 386.34 / 556.83 / 1306.81 |
| similar items | 106 | 9.45 / 11.62 / 14.07 | 313 | 3.06 / 4.16 / 5.94 |
| facet | 48 | 16.22 / 45.65 / 58.99 | 40 | 6.25 / 61.93 / 175.90 |
| key lookup | 2,890 | 0.34 / 0.39 / 0.55 | 3,718 | 0.27 / 0.31 / 0.38 |
| sql aggregate | n/a | n/a | 4 | 236.89 / 268.55 / 310.76 |


## 4 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Qdrant q/s | Qdrant ms | Postgres q/s | Postgres ms |
|---|---|---|---|---|
| bm25 | 1,676 | 2.27 / 3.66 / 4.95 | 4 | 655.67 / 1934.57 / 6284.47 |
| bm25 + filter | 1,532 | 2.49 / 3.83 / 5.67 | 7 | 321.39 / 1839.48 / 2630.76 |
| phrase | 1,145 | 1.58 / 6.45 / 45.83 | 310 | 5.27 / 28.99 / 108.90 |
| typo | n/a | n/a | n/a | n/a |
| prefix | n/a | n/a | 6 | 477.92 / 1591.86 / 2174.02 |
| vector | 1,301 | 2.95 / 3.87 / 5.04 | 1,010 | 3.74 / 5.28 / 8.27 |
| vector + filter | 742 | 5.28 / 6.53 / 8.36 | 920 | 3.97 / 6.12 / 10.30 |
| hybrid (RRF) | 290 | 13.04 / 18.81 / 22.99 | 5 | 788.54 / 1908.82 / 2651.15 |
| hybrid (weighted) | 288 | 13.06 / 18.88 / 23.20 | 4 | 800.01 / 2103.41 / 2830.68 |
| boosted (bm25 × decay) | 1,413 | 2.75 / 4.15 / 5.55 | 2 | 1044.28 / 2538.47 / 5694.39 |
| similar items | 347 | 11.67 / 14.24 / 16.46 | 1,182 | 3.19 / 4.37 / 7.21 |
| facet | 58 | 67.21 / 122.26 / 174.28 | 112 | 7.48 / 83.34 / 343.33 |
| key lookup | 9,836 | 0.39 / 0.48 / 0.60 | 13,064 | 0.31 / 0.36 / 0.44 |
| sql aggregate | n/a | n/a | 5 | 694.91 / 882.87 / 2934.01 |


## 16 clients: queries per second, and latency in ms (p50 / p90 / p99)

| kind | Qdrant q/s | Qdrant ms | Postgres q/s | Postgres ms |
|---|---|---|---|---|
| bm25 | 2,362 | 6.48 / 9.85 / 13.80 | 4 | 1994.36 / 5462.30 / 7920.91 |
| bm25 + filter | 2,112 | 7.19 / 10.91 / 16.11 | 6 | 1153.99 / 8170.98 / 11600.20 |
| phrase | 1,607 | 8.35 / 16.49 / 52.85 | 344 | 17.11 / 106.72 / 388.40 |
| typo | n/a | n/a | n/a | n/a |
| prefix | n/a | n/a | 5 | 2182.38 / 4349.25 / 6263.99 |
| vector | 1,730 | 8.80 / 12.85 / 17.55 | 1,352 | 9.59 / 22.54 / 39.39 |
| vector + filter | 875 | 17.55 / 24.75 / 30.05 | 1,220 | 10.94 / 23.44 / 44.37 |
| hybrid (RRF) | 347 | 44.55 / 60.17 / 74.31 | 4 | 2099.40 / 5708.85 / 11851.54 |
| hybrid (weighted) | 340 | 45.32 / 60.23 / 75.31 | 4 | 2159.29 / 6958.25 / 9885.92 |
| boosted (bm25 × decay) | 2,038 | 7.52 / 11.31 / 16.31 | 3 | 2358.98 / 8029.31 / 14417.04 |
| similar items | 370 | 42.01 / 55.34 / 65.21 | 1,600 | 8.00 / 19.85 / 33.44 |
| facet | 58 | 268.51 / 376.89 / 446.10 | 106 | 27.98 / 366.14 / 834.29 |
| key lookup | 25,096 | 0.59 / 0.90 / 1.29 | 28,197 | 0.48 / 0.74 / 1.82 |
| sql aggregate | n/a | n/a | 5 | 1691.80 / 5905.52 / 6367.84 |


Errors during the timed runs: Qdrant 0, Postgres 0

## Answers: recall@10 against an exact search, nDCG@10 against the collection's judgments

Unavailable kinds are n/a; recall is for the kinds an exact search can answer.

| kind | Qdrant recall | Qdrant nDCG | Postgres recall | Postgres nDCG |
|---|---|---|---|---|
| bm25 | – | – | – | – |
| bm25 + filter | – | – | – | – |
| phrase | – | – | – | – |
| typo | n/a | n/a | n/a | n/a |
| prefix | n/a | n/a | – | – |
| vector | 0.896 | – | 0.780 | – |
| vector + filter | 0.836 | – | 0.725 | – |
| hybrid (RRF) | – | – | – | – |
| hybrid (weighted) | – | – | – | – |
| boosted (bm25 × decay) | – | – | – | – |
| similar items | 0.972 | – | 0.984 | – |


## SQL aggregates equal to the true answer

| Qdrant | Postgres |
|---|---|
| n/a | 300 of 300 |

## CPU used by each engine's containers (cgroup cpu.stat)

| engine | phase | pod | CPU seconds | wall seconds | average cores | throttled (s) |
|---|---|---|---|---|---|---|
| Qdrant | load | qdrant-0 | 24869 | 4570 | 5.44 | 72 |
| Qdrant | timed run | qdrant-0 | 1280 | 306 | 4.18 | 20 |
| Postgres | load | postgres-0 | 4232 | 1404 | 3.01 | 2 |
| Postgres | timed run | postgres-0 | 5165 | 1093 | 4.73 | 171 |

## Memory of each engine's containers (cgroup memory.current, memory.stat, memory.peak; GB, all its pods)

| engine | after the load | of it anonymous | after the timed run | of it anonymous | of it files (page cache) | pods' peaks added |
|---|---|---|---|---|---|---|
| Qdrant | 10.41 | 3.95 | 10.44 | 3.94 | 4.70 | 23.72 |
| Postgres | 27.49 | 0.02 | 27.50 | 0.02 | 26.94 | 30.20 |

- Qdrant, prefix: unavailable: prefixes need the prefix tokenizer, an index of its own that would replace the word index the facet and phrase kinds use on the field

- Qdrant, sql aggregate: unavailable: no grouped aggregates: its facet API counts values only

- Qdrant, typo: unavailable: no fuzzy matching: its text index matches whole words (or prefixes, with a prefix tokenizer)

- Qdrant, bm25: BM25 from the client: sparse vectors of ChloeDB's analysis weighted tf (k1 + 1) / (tf + k1 (1 - b + b dl / avgdl)), Qdrant applying the IDF

- Qdrant, bm25 + filter: BM25 from the client: sparse vectors of ChloeDB's analysis weighted tf (k1 + 1) / (tf + k1 (1 - b + b dl / avgdl)), Qdrant applying the IDF

- Qdrant, boosted (bm25 × decay): the BM25 top 100 re-ranked by a formula query: score x exp_decay(year, target 2024, scale 5, midpoint 0.5)

- Qdrant, hybrid (RRF): prefetch of the BM25 and vector top 100, fused by rrf (k 60)

- Qdrant, hybrid (weighted): prefetch of the BM25 and vector top 100, fused by dbsf (each normalised by its mean ± 3 standard deviations, summed unweighted: no weights)

- Qdrant, phrase: BM25 over the whole query, filtered by the text index's phrase match

- Postgres, typo: unavailable: no fuzzy matching in full-text search (pg_trgm compares whole strings, not the words of a tsvector)

- Postgres, bm25: ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Postgres, bm25 + filter: ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Postgres, boosted (bm25 × decay): ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Postgres, hybrid (RRF): ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Postgres, hybrid (weighted): ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Postgres, phrase: ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

- Postgres, prefix: ts_rank (term frequencies, no IDF), not BM25, over to_tsquery('english') of the words OR-ed; hybrids in SQL over the top 100 of each

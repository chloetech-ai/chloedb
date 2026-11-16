## Places summed over 47 tests, on the median of the passes

| place | engine | points | average place | tests run | average place on those | first on |
|---|---|---|---|---|---|---|
| 1 | qdrant | 68 | 1.45 | 38 | 1.32 | bm25 p50, bm25 p99, bm25 q/s, bm25 + filter p50, bm25 + filter p99, bm25 + filter q/s, phrase p50, phrase p99, phrase q/s, boosted (bm25 × decay) p50, boosted (bm25 × decay) p99, boosted (bm25 × decay) q/s, facet p99, vector p50, vector p99, vector q/s, vector + filter p99, hybrid (RRF) p50, hybrid (RRF) p99, hybrid (RRF) q/s, hybrid (weighted) p50, hybrid (weighted) p99, hybrid (weighted) q/s, vector recall, vector + filter recall, load size |
| 2 | postgres | 76 | 1.62 | 44 | 1.59 | prefix p50, prefix p99, prefix q/s, facet p50, facet q/s, vector + filter p50, vector + filter q/s, similar items p50, similar items p99, similar items q/s, key lookup p50, key lookup p99, key lookup q/s, sql aggregate p50, sql aggregate p99, sql aggregate q/s, similar items recall, load time |

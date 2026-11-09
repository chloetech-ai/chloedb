## Places summed over 45 tests, on the median of the passes

| place | engine | points | average place | tests run | average place on those | first on |
|---|---|---|---|---|---|---|
| 1 | strata | 55 | 1.22 | 45 | 1.22 | bm25 p50, bm25 q/s, bm25 + filter p50, bm25 + filter p99, bm25 + filter q/s, phrase p50, phrase p99, phrase q/s, prefix p50, prefix p99, prefix q/s, boosted (bm25 × decay) p50, boosted (bm25 × decay) p99, boosted (bm25 × decay) q/s, facet p50, facet p99, facet q/s, vector p50, vector p99, vector q/s, vector + filter p50, vector + filter p99, vector + filter q/s, hybrid (RRF) p50, hybrid (RRF) q/s, hybrid (weighted) p50, hybrid (weighted) p99, hybrid (weighted) q/s, similar items p50, similar items p99, similar items q/s, key lookup p50, key lookup p99, key lookup q/s, sql aggregate p50, sql aggregate p99, sql aggregate q/s, vector recall, similar items recall |
| 2 | es | 106 | 2.36 | 45 | 2.36 | typo p50, typo p99, typo q/s, hybrid (RRF) p99 |
| 3 | os | 109 | 2.42 | 45 | 2.42 | bm25 p99, vector + filter recall |

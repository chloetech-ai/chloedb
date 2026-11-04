## Places summed over 45 tests, on the median of the passes

| place | engine | points | average place | tests run | average place on those | first on |
|---|---|---|---|---|---|---|
| 1 | strata | 67 | 1.49 | 45 | 1.49 | bm25 p50, bm25 + filter p99, phrase p50, phrase p99, phrase q/s, prefix p50, prefix p99, boosted (bm25 × decay) p50, boosted (bm25 × decay) p99, boosted (bm25 × decay) q/s, facet p50, facet p99, facet q/s, vector p50, vector p99, vector q/s, vector + filter p50, vector + filter p99, vector + filter q/s, hybrid (RRF) p50, hybrid (RRF) q/s, hybrid (weighted) p50, hybrid (weighted) p99, hybrid (weighted) q/s, similar items p50, similar items p99, similar items q/s, key lookup p50, key lookup p99, vector recall, similar items recall |
| 2 | os | 83 | 1.84 | 45 | 1.84 | bm25 p99, bm25 q/s, bm25 + filter p50, bm25 + filter q/s, prefix q/s, typo p99, key lookup q/s, sql aggregate p50, sql aggregate p99, sql aggregate q/s, vector + filter recall |
| 3 | es | 120 | 2.67 | 45 | 2.67 | typo p50, typo q/s, hybrid (RRF) p99 |

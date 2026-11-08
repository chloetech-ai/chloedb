## Places summed over 45 tests, on the median of the passes

| place | engine | points | average place | tests run | average place on those | first on |
|---|---|---|---|---|---|---|
| 1 | strata | 133 | 2.96 | 45 | 2.96 | phrase p99, facet p50, facet p99, facet q/s, vector p50, vector p99, vector + filter p99, hybrid (RRF) p50, hybrid (RRF) q/s, hybrid (weighted) p50, hybrid (weighted) q/s |
| 2 | os | 159 | 3.53 | 45 | 3.53 | bm25 p50, bm25 q/s, bm25 + filter q/s, sql aggregate p50, sql aggregate p99, sql aggregate q/s |
| 3 | es | 179 | 3.98 | 45 | 3.98 | typo p50, typo p99, typo q/s, hybrid (RRF) p99 |
| 4 | qdrant | 240 | 5.33 | 36 | 4.42 | bm25 p99, bm25 + filter p50, bm25 + filter p99, phrase p50, boosted (bm25 × decay) p99, hybrid (weighted) p99, vector recall |
| 5 | vespa | 241 | 5.36 | 39 | 4.79 | phrase q/s, boosted (bm25 × decay) p50, boosted (bm25 × decay) q/s |
| 6 | paradedb | 257 | 5.71 | 45 | 5.71 | similar items recall |
| 7 | solr | 270 | 6.00 | 45 | 6.00 | prefix p50, prefix p99, prefix q/s, vector + filter recall |
| 8 | postgres | 292 | 6.49 | 18 | 2.72 | vector q/s, vector + filter p50, vector + filter q/s, similar items p50, similar items p99, similar items q/s, key lookup p50, key lookup p99 |
| 9 | weaviate | 293 | 6.51 | 30 | 5.27 | key lookup q/s |

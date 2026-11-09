## Places summed over 47 tests, on the median of the passes

| place | engine | points | average place | tests run | average place on those | first on |
|---|---|---|---|---|---|---|
| 1 | strata | 159 | 3.38 | 47 | 3.38 | facet p50, facet p99, vector + filter p99, hybrid (RRF) p50, hybrid (RRF) q/s, hybrid (weighted) p50, hybrid (weighted) q/s, similar items p99, load time |
| 2 | es | 291 | 6.19 | 47 | 6.19 | load size |
| 3 | os | 303 | 6.45 | 47 | 6.45 | facet q/s, sql aggregate p50, sql aggregate p99, sql aggregate q/s |
| 4 | solr | 303 | 6.45 | 47 | 6.45 | prefix p50, prefix p99, prefix q/s, vector + filter recall, similar items recall |
| 5 | qdrant | 323 | 6.87 | 38 | 5.18 | bm25 p50, bm25 p99, bm25 q/s, bm25 + filter p50, bm25 + filter p99, bm25 + filter q/s, boosted (bm25 × decay) p50, boosted (bm25 × decay) p99, boosted (bm25 × decay) q/s, vector recall |
| 6 | milvus | 329 | 7.00 | 35 | 4.60 | phrase p99, hybrid (RRF) p99, hybrid (weighted) p99 |
| 7 | redis | 359 | 7.64 | 47 | 7.64 | phrase q/s, vector p50, vector p99, vector q/s, vector + filter p50, vector + filter q/s, similar items p50, similar items q/s, key lookup p50, key lookup p99, key lookup q/s |
| 8 | manticore | 386 | 8.21 | 44 | 7.82 | – |
| 9 | meili | 392 | 8.34 | 41 | 7.51 | typo p99, typo q/s |
| 10 | vespa | 397 | 8.45 | 41 | 7.63 | – |
| 11 | typesense | 422 | 8.98 | 41 | 8.24 | phrase p50, typo p50 |
| 12 | weaviate | 432 | 9.19 | 38 | 8.05 | – |
| 13 | postgres | 448 | 9.53 | 44 | 9.23 | – |
| 14 | paradedb | 455 | 9.68 | 47 | 9.68 | – |

## Places summed over 45 tests, on the median of the passes

| place | engine | points | average place | tests run | average place on those | first on |
|---|---|---|---|---|---|---|
| 1 | strata | 156 | 3.47 | 45 | 3.47 | facet p50, facet p99, vector + filter p99, hybrid (RRF) p50, hybrid (RRF) q/s, hybrid (weighted) p50, hybrid (weighted) q/s, similar items p99 |
| 2 | es | 284 | 6.31 | 45 | 6.31 | – |
| 3 | solr | 287 | 6.38 | 45 | 6.38 | prefix p50, prefix p99, prefix q/s, vector + filter recall, similar items recall |
| 4 | os | 292 | 6.49 | 45 | 6.49 | facet q/s, sql aggregate p50, sql aggregate p99, sql aggregate q/s |
| 5 | qdrant | 302 | 6.71 | 36 | 4.89 | bm25 p50, bm25 p99, bm25 q/s, bm25 + filter p50, bm25 + filter p99, bm25 + filter q/s, boosted (bm25 × decay) p50, boosted (bm25 × decay) p99, boosted (bm25 × decay) q/s, vector recall |
| 6 | milvus | 310 | 6.89 | 33 | 4.30 | phrase p99, hybrid (RRF) p99, hybrid (weighted) p99 |
| 7 | redis | 350 | 7.78 | 45 | 7.78 | phrase q/s, vector p50, vector p99, vector q/s, vector + filter p50, vector + filter q/s, similar items p50, similar items q/s, key lookup p50, key lookup p99, key lookup q/s |
| 8 | meili | 368 | 8.18 | 39 | 7.28 | typo p99, typo q/s |
| 9 | manticore | 370 | 8.22 | 42 | 7.81 | – |
| 10 | vespa | 377 | 8.38 | 39 | 7.51 | – |
| 11 | typesense | 401 | 8.91 | 39 | 8.13 | phrase p50, typo p50 |
| 12 | weaviate | 423 | 9.40 | 36 | 8.25 | – |
| 13 | postgres | 429 | 9.53 | 42 | 9.21 | – |
| 14 | paradedb | 440 | 9.78 | 45 | 9.78 | – |

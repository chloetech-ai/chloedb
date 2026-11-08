## Places summed over 47 tests, on the median of the passes

| place | engine | points | average place | tests run | average place on those | first on |
|---|---|---|---|---|---|---|
| 1 | strata | 226 | 4.81 | 47 | 4.81 | facet p99, vector + filter p99, similar items p99, load time |
| 2 | es | 269 | 5.72 | 47 | 5.72 | hybrid (weighted) q/s, load size |
| 3 | os | 281 | 5.98 | 47 | 5.98 | facet q/s, hybrid (RRF) q/s, sql aggregate p50, sql aggregate p99, sql aggregate q/s |
| 4 | milvus | 302 | 6.43 | 35 | 3.83 | bm25 + filter p99, phrase p99, hybrid (RRF) p50, hybrid (RRF) p99, hybrid (weighted) p50, hybrid (weighted) p99 |
| 5 | solr | 305 | 6.49 | 47 | 6.49 | prefix p50, prefix p99, prefix q/s, vector + filter recall, similar items recall |
| 6 | qdrant | 337 | 7.17 | 38 | 5.55 | bm25 p50, bm25 p99, bm25 q/s, bm25 + filter p50, bm25 + filter q/s, boosted (bm25 × decay) p50, boosted (bm25 × decay) p99, boosted (bm25 × decay) q/s, vector recall |
| 7 | manticore | 346 | 7.36 | 44 | 6.91 | facet p50, vector p50, vector p99, vector q/s |
| 8 | redis | 366 | 7.79 | 47 | 7.79 | phrase q/s, vector + filter p50, vector + filter q/s, similar items p50, similar items q/s, key lookup q/s |
| 9 | vespa | 393 | 8.36 | 41 | 7.54 | – |
| 10 | meili | 409 | 8.70 | 41 | 7.93 | typo p99, typo q/s |
| 11 | typesense | 427 | 9.09 | 41 | 8.37 | phrase p50, typo p50 |
| 12 | weaviate | 439 | 9.34 | 38 | 8.24 | – |
| 13 | paradedb | 449 | 9.55 | 47 | 9.55 | – |
| 14 | postgres | 450 | 9.57 | 44 | 9.27 | key lookup p50, key lookup p99 |

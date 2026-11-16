## Places summed over 47 tests, on the median of the passes

| place | engine | points | average place | tests run | average place on those | first on |
|---|---|---|---|---|---|---|
| 1 | strata | 135 | 2.87 | 47 | 2.87 | phrase p99, prefix q/s, facet p99, facet q/s, vector + filter p99, hybrid (RRF) p50, hybrid (RRF) p99, hybrid (RRF) q/s, hybrid (weighted) p50, hybrid (weighted) p99, hybrid (weighted) q/s, sql aggregate p50, sql aggregate p99, sql aggregate q/s |
| 2 | es | 276 | 5.87 | 47 | 5.87 | – |
| 3 | solr | 279 | 5.94 | 47 | 5.94 | prefix p50, prefix p99, similar items recall, load size |
| 4 | os | 300 | 6.38 | 47 | 6.38 | – |
| 5 | qdrant | 330 | 7.02 | 38 | 5.37 | bm25 p50, bm25 p99, bm25 q/s, bm25 + filter p50, bm25 + filter p99, bm25 + filter q/s, boosted (bm25 × decay) p50, boosted (bm25 × decay) p99, boosted (bm25 × decay) q/s, vector recall |
| 6 | milvus | 355 | 7.55 | 35 | 5.34 | vector + filter recall |
| 7 | redis | 355 | 7.55 | 47 | 7.55 | phrase q/s, vector p50, vector p99, vector q/s, vector + filter p50, vector + filter q/s, similar items p50, similar items p99, similar items q/s, key lookup p50, key lookup p99, key lookup q/s |
| 8 | manticore | 383 | 8.15 | 44 | 7.75 | facet p50 |
| 9 | vespa | 395 | 8.40 | 41 | 7.59 | – |
| 10 | meili | 397 | 8.45 | 41 | 7.63 | typo p50, typo p99, typo q/s |
| 11 | typesense | 439 | 9.34 | 41 | 8.66 | phrase p50 |
| 12 | weaviate | 448 | 9.53 | 38 | 8.47 | – |
| 13 | postgres | 454 | 9.66 | 44 | 9.36 | – |
| 14 | paradedb | 455 | 9.68 | 47 | 9.68 | load time |

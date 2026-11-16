## Places summed over 45 tests, on the median of the passes

| place | engine | points | average place | tests run | average place on those | first on |
|---|---|---|---|---|---|---|
| 1 | strata | 130 | 2.89 | 45 | 2.89 | phrase p99, prefix q/s, facet p99, facet q/s, vector + filter p99, hybrid (RRF) p50, hybrid (RRF) p99, hybrid (RRF) q/s, hybrid (weighted) p50, hybrid (weighted) p99, hybrid (weighted) q/s, sql aggregate p50, sql aggregate p99, sql aggregate q/s |
| 2 | es | 268 | 5.96 | 45 | 5.96 | – |
| 3 | solr | 270 | 6.00 | 45 | 6.00 | prefix p50, prefix p99, similar items recall |
| 4 | os | 288 | 6.40 | 45 | 6.40 | – |
| 5 | qdrant | 308 | 6.84 | 36 | 5.06 | bm25 p50, bm25 p99, bm25 q/s, bm25 + filter p50, bm25 + filter p99, bm25 + filter q/s, boosted (bm25 × decay) p50, boosted (bm25 × decay) p99, boosted (bm25 × decay) q/s, vector recall |
| 6 | milvus | 337 | 7.49 | 33 | 5.12 | vector + filter recall |
| 7 | redis | 345 | 7.67 | 45 | 7.67 | phrase q/s, vector p50, vector p99, vector q/s, vector + filter p50, vector + filter q/s, similar items p50, similar items p99, similar items q/s, key lookup p50, key lookup p99, key lookup q/s |
| 8 | manticore | 366 | 8.13 | 42 | 7.71 | facet p50 |
| 9 | meili | 373 | 8.29 | 39 | 7.41 | typo p50, typo p99, typo q/s |
| 10 | vespa | 377 | 8.38 | 39 | 7.51 | – |
| 11 | typesense | 417 | 9.27 | 39 | 8.54 | phrase p50 |
| 12 | postgres | 434 | 9.64 | 42 | 9.33 | – |
| 13 | weaviate | 437 | 9.71 | 36 | 8.64 | – |
| 14 | paradedb | 441 | 9.80 | 45 | 9.80 | – |

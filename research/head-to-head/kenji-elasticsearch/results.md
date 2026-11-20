# Kenji: a multi-tenant ticket search, ChloeDB's OpenSearch API against Elasticsearch

Kenji, a backend engineer in the persona study, runs a support-ticket search on Elasticsearch: one index, a `tenant_id` on each ticket, and per request a `bool` query (`multi_match` on title^3 and body; filters on the tenant, the status and a date range), `terms` aggregations on `tags` (an array) and `status`, highlights, a sort by score or by date, and "load more" by `search_after`. His CTO wants memory and latency at his sizes against his Elasticsearch, measured the same way. This page sends the same requests to Elasticsearch and to ChloeDB's OpenSearch API, on made-up tickets of his shape ([run.py](run.py): 200 tenants with Zipf 1.07 sizes, titles and bodies written from a support vocabulary, up to 8 tags).

Measured on 2026-11-20 on Intel(R) Xeon(R) Processor @ 2.10GHz, 4 cores, 15 GB, Linux 6.18.44-fc-v726; Elasticsearch 9.5.5 (Docker mirror.gcr.io/library/elasticsearch:9.5.5, heap 4g); chloedb-server 0.3.9 (built from 2f21f4dd0c) (its OpenSearch API). Both on the same machine, one at a time, each loaded by `_bulk` in batches of 2,000 from one client. The index has one shard and no replica on Elasticsearch, `english` analyzers on title and body in both. The search mix is 1,000 searches over tenants drawn by their size, 40% sorted by date with an `id` tie-breaker; it runs with 1 and with 4 clients after 50 warm-up searches. Run it again with `python3 run.py [DOCS] && python3 report.py` (needs Docker and a chloedb-server build). Raw numbers: [raw/results-300000.json](raw/results-300000.json), [raw/results-2000000.json](raw/results-2000000.json).

## What it says

- **300,000 tickets.** Of 200 searches compared, both found the same total for 200, the same first page in the same order for 165 (the same ids in some order for 168; 9.44 of the first ten shared on average), and the same buckets for 200. His search, one client: Elasticsearch p50 7.4 ms, p95 10.6; ChloeDB p50 17.6, p95 44.1. Four clients: Elasticsearch 372.7 searches/s at p95 16.7 ms, ChloeDB 87.3/s at p95 90.0 ms. The `tags` aggregation alone on the biggest tenant: Elasticsearch 2.7 ms, ChloeDB 75.1 ms (KENJI-401).
- **2,000,000 tickets.** Of 200 searches compared, both found the same total for 200, the same first page in the same order for 181 (the same ids in some order for 186; 9.71 of the first ten shared on average), and the same buckets for 200. His search, one client: Elasticsearch p50 7.9 ms, p95 15.6; ChloeDB p50 91.3, p95 277.7. Four clients: Elasticsearch 308.8 searches/s at p95 23.6 ms, ChloeDB 14.4/s at p95 652.9 ms. The `tags` aggregation alone on the biggest tenant: Elasticsearch 3.0 ms, ChloeDB 493.5 ms (KENJI-401).
- **Where the time goes.** The text query alone is as fast on ChloeDB as on Elasticsearch (11.6 ms against 13.8 on the biggest tenant at 2,000,000 tickets). What makes his search slower is two parts of it: the `terms` aggregation on the `tags` array (above) and the `id` tie-breaker on a date sort (933.9 ms against 4.0), both through the OpenSearch layer. Round 4 traced the facet to arrays read from each row's `_source`, and measured it natively, as `GROUP BY` on a `KEYWORD[]` column, at 3.6 ms; step x10 of round 4 (KENJI-401, KENJI-404) is meant to fix both.
- **Memory is ChloeDB's.** Its server held 1,914 MB after the run against Elasticsearch's 4,877 MB. Elasticsearch's figure is mostly the heap it was given (`-Xmx` 4g), which it takes at start whatever the data; the comparison that matters for his CTO is the smallest heap at which Elasticsearch keeps these latencies, which this page does not measure. Disk and loading favour Elasticsearch.

## His search, end to end

| tickets | system | clients | searches/s | p50 | p95 | p99 | errors |
|---|---|---|---|---|---|---|---|
| 300,000 | Elasticsearch | 1 | 126.5 | 7.4 ms | 10.6 ms | 14.3 ms | 0 |
| 300,000 | Elasticsearch | 4 | 372.7 | 9.3 ms | 16.7 ms | 62.5 ms | 0 |
| 300,000 | ChloeDB | 1 | 44.6 | 17.6 ms | 44.1 ms | 60.9 ms | 0 |
| 300,000 | ChloeDB | 4 | 87.3 | 39.0 ms | 90.0 ms | 125.2 ms | 0 |
| 2,000,000 | Elasticsearch | 1 | 110.3 | 7.9 ms | 15.6 ms | 19.9 ms | 0 |
| 2,000,000 | Elasticsearch | 4 | 308.8 | 11.4 ms | 23.6 ms | 31.8 ms | 0 |
| 2,000,000 | ChloeDB | 1 | 8.4 | 91.3 ms | 277.7 ms | 372.4 ms | 0 |
| 2,000,000 | ChloeDB | 4 | 14.4 | 220.3 ms | 652.9 ms | 883.3 ms | 0 |

## Its parts, on the biggest tenant

Each request 15 times, after one; p50 and p95 in milliseconds. The filters are the search's: the tenant, three statuses, the last year.

| part | Elasticsearch, 300k | ChloeDB, 300k | Elasticsearch, 2,000k | ChloeDB, 2,000k |
|---|---|---|---|---|
| text query | 4.8 / 11.0 | 3.6 / 4.2 | 13.8 / 23.2 | 11.6 / 13.4 |
| + highlight | 7.2 / 7.8 | 5.2 / 6.3 | 13.2 / 20.3 | 11.0 / 12.7 |
| terms on status (filters only) | 3.0 / 7.4 | 4.1 / 7.0 | 3.0 / 7.5 | 8.1 / 9.9 |
| terms on tags, an array (filters only) | 2.7 / 5.1 | 75.1 / 91.2 | 3.0 / 4.4 | 493.5 / 546.4 |
| sort by date | 3.9 / 5.1 | 2.1 / 3.0 | 4.2 / 6.1 | 3.0 / 9.7 |
| sort by date, id tie-breaker | 3.7 / 6.5 | 128.1 / 153.2 | 4.0 / 6.1 | 933.9 / 1,014.5 |
| terms on tags, whole index | 2.0 / 2.2 | 885.9 / 947.5 | 2.5 / 2.8 | timed out (504) |

## Paging a tenant by search_after, loading, memory and disk

| tickets | system | pages of 100 | unique hits | in order | page p50 | `_bulk` load | memory after load | memory after the run | disk |
|---|---|---|---|---|---|---|---|---|---|
| 300,000 | Elasticsearch | 75 | 7,463 | yes | 4.9 ms | 21.5 s | 4,845 MB | 4,920 MB | 104 MB |
| 300,000 | ChloeDB | 75 | 7,463 | yes | 23.8 ms | 38.8 s | 272 MB | 578 MB | 180 MB |
| 2,000,000 | Elasticsearch | 498 | 49,758 | yes | 5.2 ms | 136.7 s | 4,794 MB | 4,877 MB | 693 MB |
| 2,000,000 | ChloeDB | 498 | 49,758 | yes | 174.7 ms | 250.0 s | 921 MB | 1,914 MB | 970 MB |

Elasticsearch's memory is its container's (`docker stats`), heap included; ChloeDB's is the server's resident set. Elasticsearch's disk is its store after a force merge to one segment, ChloeDB's its database files after a vacuum.

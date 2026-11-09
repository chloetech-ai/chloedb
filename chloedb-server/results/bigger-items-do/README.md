# Strata before and after the bigger items, against Elasticsearch and OpenSearch on the same nodes (DigitalOcean, 5 and 6 November 2026)

The bigger items plan's one rerun. Eleven pull requests merged on 5
November between the two images measured here:

- #252 GROUP BY and aggregates read from the columns, and #264 their
  segments grouped in parallel, with exact FLOAT sums;
- #253 filters kept between queries, per segment;
- #258 a ranked BM25 query's later segments start at the k-th score the
  earlier ones found, and #259 read counts without a scan of the summary;
- #260 and #265 rank-safe boosts: a shard sends its own top k;
- #262 the harness turns every engine's result cache off while it times
  them (Strata has none to turn off);
- #266 typo queries walk an in-memory table of each segment's terms;
- #274 a third fewer allocations per HTTP request in the coordinator;
- #275 a crc32 per doc-store block, checked when the block is read.

Each lane ran one job, `strata-ab+es+os`: Strata's shards were built once,
with the newer image, and served by the two images in turn, in the order
after, before, before, after, after, before (`strata-a` and `strata-b`, three
passes each); then Elasticsearch and OpenSearch loaded and ran on the same
nodes. The older image reads the newer one's segments as segments without a
per-block crc (it checks `store.data` whole, as it always did), so both read
the same files. A change between the two images is the code alone; the
gap between Strata and the Lucene engines is not the gap between two
machines either.

| | |
|---|---|
| after (`strata-a`, and `run-strata*` below) | `ghcr.io/chloetech-ai/strata-server:18fb4bc4ba69b1d0abf4adaca2be0fa9e1ad8bb4` (main after the eleven merges) |
| before (`strata-b`) | `ghcr.io/chloetech-ai/strata-server:6d02ba6544b92f60f8b36c108d9ccbaceca7f9db` (the last image built before them; cfeaec9, #272, between the two, changes only strata-py) |
| scripts | `fanout.sh` at 18fb4bc with the harness of the same commit, whose result caches are off for every engine (#262); jobs in `jobs-1m.tsv` and `jobs-10m.tsv` |
| cloud | DigitalOcean Kubernetes, sfo2, a cluster of its own with an `s-8vcpu-16gb` base node |
| 10M lane | 2 x `m3-4vcpu-32gb` (both Xeon Platinum 8168) and a `c-8` client (8280) |
| 1M lane | 2 x `m-4vcpu-32gb` (Xeon Gold 6248 and Platinum 8168) and a `c-8` client (8168) |
| CPUs and memory | as in the same-node run ([compare-msmarco-same-nodes-do](../compare-msmarco-same-nodes-do/)): Strata's two shards 2.9 CPUs each and its coordinator 1, an Elasticsearch or OpenSearch data pod 3.4 |
| passes | three of every kind after warm-up rounds; `tables.md` shows pass 1, `passes.md`, the ranking and every number below are medians of the three |
| data | 10M: the 10M run's collection. 1M: its first million passages, cut on the base node. At both sizes every file hashes as the same-node run's (`data/SHA256SUMS`) except `meta.json`, which a newer `prep` writes with three more keys (`attrs`, `categories`, `heldout`) |
| time | 11-05 18:50 to 11-06 04:54 UTC: the 1M lane 2.1 h, the 10M lane 9.7 h (Strata's offline build 68 minutes, Elasticsearch's load 156, OpenSearch's 201) |
| cost | $12.65 at list price (pools rounded up to whole hours: the 1M lane's $2.25, the 10M lane's $8.69, and the base node's $1.71 from 11-05 18:25 to the cluster's deletion on 11-06 at 05:49) |

Every step of both jobs passed.

## Before and after

Medians of three passes: p50 in ms with one client, queries a second with 16.
Answers are the same before and after (the SQL aggregate's averages differ
in their last digits: FLOAT sums are exact now).

1M:

| kind | p50 before | p50 after | p50 ES | p50 OS | q/s before | q/s after | q/s ES | q/s OS |
|---|---|---|---|---|---|---|---|---|
| bm25 | 1.68 | 1.66 | 3.03 | 2.89 | 1,534 | 2,023 | 1,738 | 1,973 |
| bm25 + filter | 2.47 | 1.84 | 3.47 | 3.40 | 1,442 | 1,808 | 1,284 | 1,302 |
| vector | 1.79 | 1.74 | 4.06 | 4.51 | 1,511 | 1,666 | 1,312 | 1,089 |
| vector + filter | 2.10 | 2.04 | 4.45 | 7.98 | 1,506 | 1,557 | 1,083 | 471 |
| hybrid (RRF) | 4.31 | 4.29 | – | 7.86 | 686 | 692 | – | 430 |
| hybrid (weighted) | 4.27 | 4.25 | 6.34 | 7.62 | 653 | 673 | 472 | 435 |
| boosted (bm25 × decay) | 4.56 | 3.28 | 7.17 | 7.25 | 562 | 845 | 314 | 301 |
| facet | 1.60 | 1.48 | 2.09 | 1.94 | 1,861 | 1,954 | 2,988 | 3,055 |
| key lookup | 0.81 | 0.77 | 1.08 | 0.95 | 4,639 | 6,304 | 9,580 | 12,947 |
| phrase | 1.56 | 1.34 | 2.81 | 2.71 | 1,834 | 2,188 | 2,032 | 2,117 |
| typo | 18.67 | 11.17 | 14.73 | 20.77 | 123 | 216 | 172 | 112 |
| prefix | 2.87 | 1.79 | 3.79 | 4.11 | 1,141 | 1,661 | 1,050 | 922 |
| similar items | 2.20 | 2.15 | 6.41 | 8.46 | 1,291 | 1,396 | 554 | 393 |
| sql aggregate | 181.70 | 7.42 | 50.07 | 59.88 | 10 | 282 | 42 | 37 |

10M:

| kind | p50 before | p50 after | p50 ES | p50 OS | q/s before | q/s after | q/s ES | q/s OS |
|---|---|---|---|---|---|---|---|---|
| bm25 | 2.93 | 2.90 | 4.87 | 4.22 | 482 | 591 | 683 | 720 |
| bm25 + filter | 7.20 | 3.59 | 6.28 | 5.94 | 219 | 502 | 435 | 428 |
| vector | 2.29 | 2.25 | 4.70 | 5.11 | 1,429 | 1,460 | 1,287 | 934 |
| vector + filter | 3.17 | 3.09 | 6.05 | 9.40 | 812 | 818 | 267 | 381 |
| hybrid (RRF) | 8.71 | 7.55 | – | 12.73 | 198 | 236 | – | 193 |
| hybrid (weighted) | 8.58 | 7.53 | 14.73 | 12.33 | 200 | 238 | 115 | 195 |
| boosted (bm25 × decay) | 14.12 | 8.39 | 43.71 | 44.30 | 93 | 200 | 42 | 41 |
| facet | 2.59 | 2.27 | 3.80 | 3.54 | 1,098 | 1,585 | 715 | 665 |
| key lookup | 0.78 | 0.72 | 1.27 | 1.01 | 5,595 | 6,756 | 9,050 | 12,620 |
| phrase | 2.78 | 1.51 | 5.51 | 4.98 | 608 | 2,028 | 468 | 472 |
| typo | 83.32 | 48.75 | 34.58 | 46.62 | 24 | 41 | 71 | 51 |
| prefix | 10.03 | 5.18 | 8.62 | 10.06 | 147 | 295 | 298 | 224 |
| similar items | 3.78 | 4.33 | 23.05 | 33.22 | 624 | 504 | 88 | 72 |
| sql aggregate | 2,726.74 | 43.49 | 551.67 | 699.18 | 1 | 31 | 5 | 4 |

At 10M the aggregate is 63 times faster (2,727 to 43.5 ms) and now ahead of
Elasticsearch (552) and OpenSearch (699); phrase queries serve 3.3 times as
many queries a second under load, boosted queries 2.2 times, filtered BM25
2.3 times, typo queries 1.7 times. Where Strata is still behind at 10M: BM25
under load (591 queries a second against Elasticsearch's 683 and
OpenSearch's 720; the plan's item 5), typo queries (48.8 ms against
Elasticsearch's 34.6), and key lookups under load (6,756 against 9,050 and
12,620); prefix queries under load are level with Elasticsearch (295 against
298). At 1M it is behind on facets and key lookups under load only.

The Lucene engines' aggregates are slower than in the same-node run (at 1M,
Elasticsearch 50.1 ms against 6.44 there) because their result caches are
off while timed now (#262); the same-node run timed them with those caches
on.

## Similar items at 10M: slower, and why

One cell got worse: similar items at 10M, 3.78 to 4.33 ms with one client
and 624 to 504 queries a second with 16, in all three passes (at 1M, 2.20 to
2.15). The query is `rating >= 3 AND id NOT IN (20 keys)` around a vector
search that tests its candidates against the filter. #253's filter cache
kept the whole filter for each item, a set of the segment's rows made by
reading every row's rating, and it did so on an item's first query: the
search looks at its filter twice (once to sample how many rows near the
query pass it), and the cache counted that as two sights. A 10M shard's
three segments hold 5M rows, so each item's sets take about 0.6 MB a shard,
and the harness's 300 items about 190 MB, three times the cache's 64 MiB:
each set was pushed out before its item came round again and made anew,
a scan of every segment on every query where the search tests a few hundred
rows. A 1M shard's take about 19 MB, which fit, so later rounds read them
from the cache and were a little faster.

On the cloud container (5M rows in four segments, the default 64 MiB cache,
the same query over 300 items, one thread), it took 1.97 to 2.17 ms p50 with
429 sets kept (67.1 MB), against 1.70 to 2.19 with the cache off. #285 keeps
lists of the table's own keys out of the cache (a request's own list, cheap
to test on a candidate) and counts a query once: the cache then keeps
`rating >= 3` and the key's `IS NOT NULL`, 8 sets and 1.3 MB, and the query
took 1.34 to 1.66 ms (four threads: 0.94 to 0.99 ms before, 0.64 to 0.65
after). Not measured on DigitalOcean yet.

## Ranking

`rank.py --no-load` on each image's set (that image's Strata with
Elasticsearch and OpenSearch): places summed over 45 tests (14 kinds at p50
and p99 with 1 client and queries a second with 4, and recall@10 of the 3
vector kinds), on the medians of the three passes.

| | Strata | Elasticsearch | OpenSearch |
|---|---|---|---|
| 1M, before | 58 | 101 | 111 |
| 1M, after | 48 | 108 | 114 |
| 10M, before | 70 | 99 | 101 |
| 10M, after | 55 | 106 | 109 |

After the merges, Strata is first on 43 of the 45 tests at 1M (OpenSearch
takes facets and key lookups a second) and on 39 at 10M (30 before), where
Elasticsearch takes the three typo tests and hybrid (RRF) p99, and
OpenSearch BM25 p99 and the recall of vector + filter.

## What else went differently

- Two `fanout.sh` calls started in the same second (the 10M `prep` and the 1M
  `run`) copied the harness into the same directory, and the second copy
  failed: the first 1M run stopped before starting anything and was started
  again two minutes later. #277 gives each copy a directory of its own.
- The cloud container restarted at 00:07 UTC, during Elasticsearch's 10M
  load. The load, a Kubernetes Job, went on; the job was resumed after it
  with `fanout.sh _job`, which skips the steps already done, and the load's
  CPU lines were written by hand from the Job's own times. Nothing was run
  twice.

## Running it again

```bash
cd strata-server/deploy/compare
export STATE=~/.strata-fanout-ab PASSES=3 BUDGET=30 \
  IMAGE=ghcr.io/chloetech-ai/strata-server:18fb4bc4ba69b1d0abf4adaca2be0fa9e1ad8bb4
./fanout.sh up 18
./fanout.sh prep 1000000
./fanout.sh run ../../results/bigger-items-do/jobs-1m.tsv
./fanout.sh prep 10000000    # after the 1M run has started (#277 lets them start together)
./fanout.sh run ../../results/bigger-items-do/jobs-10m.tsv
python3 rank.py --no-load ../../results/bigger-items-do/10m
```

## Files

`1m/` and `10m/`, each as `fanout.sh analyze` lays a run out, with the
after image as `strata`: `run-<engine>.json.gz` (pass 1 with its answers),
`run-<engine>-pass2/3.json.gz`, `load-*`, `build-strata-s*.json`, `writes-*`
and `stream-*` (Elasticsearch and OpenSearch; Strata's shards were not
written to, so both images read the same files), `cpu.jsonl`, `tables.md`
and `summary.json` (analyze.py, pass 1), `passes.md` (both images and the
Lucene engines, medians of the passes), `rank-no-load.md`, `strata-b/` (the
before image, analysed alone with the same Elasticsearch and OpenSearch
reports), `jobs/<job>/` (nodes and their checks, steps, logs) and `data/`.
Every report of every pass, whole, with the run's state and scripts, is in
the project's files as `bigger-items-do/raw-run-state.tar.gz`.

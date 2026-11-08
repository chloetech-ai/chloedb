# The engine comparison again on latest main, each lane's engines on the same nodes (DigitalOcean, 4 November 2026)

The 1M and 10M MS MARCO comparisons ([compare-msmarco-1m-do](../compare-msmarco-1m-do/),
[compare-msmarco-10m-do](../compare-msmarco-10m-do/)) ran every engine on
nodes of its own. Strata's own image on two such sets of nodes differed by
39% on BM25 at 16 clients and by up to 58% on other kinds
([bm25-do-1m](../bm25-do-1m/)), more than most of the gaps between engines. This run puts each lane's engines on one
set of nodes, one engine after another, so that the gap between two engines
is not the gap between two machines. It runs main's Strata, with the BM25
term-statistics cache and boxed gRPC calls (#245) and the faster typo
queries (#242).

| | |
|---|---|
| Strata image | `ghcr.io/chloetech-ai/strata-server:055f6bf72d82812b98b30df5088dfd7295fecc0e` (main after #245; main's later commits up to d6c1540 change only docs, results and `rank.py`) |
| scripts | `fanout.sh` of this branch, `fanout-jobs-same-nodes.tsv`: an engine field `a+b+c` runs those engines one after another on the job's one set of nodes; `strata-first` and `strata-last` run Strata before the others and again after them on the same shards (labelled `strata-b`, the drift of the nodes over those hours), then its writes and stream, which change the shards |
| cloud | DigitalOcean Kubernetes, sfo2, the 10M run's cluster and base node |
| 10M lane | Strata, Elasticsearch, OpenSearch, Strata again: 2 x `m3-4vcpu-32gb` (Xeon Platinum 8168 and 8280) and a `c-8` client (8358) |
| 1M sharded lane | Strata, Elasticsearch, OpenSearch, Solr, Vespa, Strata again: 2 x `m-4vcpu-32gb` (8280 and 8168) and a `c-8` client (8280) |
| 1M one-node lane | Qdrant, Meilisearch, Postgres, Redis, Weaviate, Manticore, ParadeDB, Typesense, Milvus: 1 x `m-8vcpu-64gb` (8280) and a `c-8` client (8280); Milvus again alone on a fresh `m-8vcpu-64gb` (8358), see below |
| CPUs and memory | as in the 1M and 10M runs: 6.8 CPUs per engine (a sharded engine's two data pods 3.4 each, or Strata's shards 2.9 each and its coordinator 1), 52 GiB for a one-node engine |
| passes | three passes of every kind after warm-up rounds; `tables.md` shows pass 1, `passes.md`, the ranking and every number below are the median of the three |
| data | 10M: the 10M run's collection (`data/SHA256SUMS`). 1M: the first million passages of it, cut on the base node: vectors, queries, judgments and `meta.json` are byte for byte the first 1M run's, `docs.jsonl` is not (its hash differs from the first 1M run's, `1m/data/SHA256SUMS.first-1m-run`; the passages are the same, and some synthetic field, likely a k-means topic computed on another node, differs) |
| time | 05:01 to 14:59 UTC (10M lane 9.7 h, 1M one-node lane 8.1 h, 1M sharded lane 3.7 h, all at once) |
| cost | $20.76 at list price (pools rounded up to whole hours, and the base node from 05:00, when the 10M run handed it over, to the cluster's deletion at 15:28) |

Every step of every job passed.

## Ranking

`rank.py --no-load` on each set: places summed over 45 tests (14 kinds at
p50 and p99 with 1 client and queries a second with 4, and recall@10 of the
3 vector kinds), on the median of the three passes. Load time and size are
left out, as in the 10M report, since Strata's load is an offline build
(`rank.md` has them in; the order is the same).

| 1M, all 14 engines | points | | 10M, Strata, Elasticsearch, OpenSearch | points |
|---|---|---|---|---|
| Strata | 156 | | Strata | 67 |
| Elasticsearch | 284 | | OpenSearch | 83 |
| Solr | 287 | | Elasticsearch | 120 |
| OpenSearch | 292 | | | |
| Qdrant | 302 | | | |
| Milvus | 310 | | | |
| Redis | 350 | | | |
| Meilisearch | 368 | | | |
| Manticore | 370 | | | |
| Vespa | 377 | | | |
| Typesense | 401 | | | |
| Weaviate | 423 | | | |
| Postgres | 429 | | | |
| ParadeDB | 440 | | | |

By the same rule, the first 1M run ranked Strata 223, Elasticsearch 262,
OpenSearch 270. The one-node lane's engines ran on another node than the
sharded lane's, so a place between a sharded engine and a one-node engine
still rests on two machines.

## Strata against the Lucene engines on the same nodes

Medians of three passes; p50 in ms at 1 client, queries a second at 16.

| 1M | Strata | Elasticsearch | OpenSearch | Solr | Vespa |
|---|---|---|---|---|---|
| bm25 p50 | 1.70 | 3.25 | 3.18 | 2.31 | 2.94 |
| bm25 q/s, 16 clients | 1,962 | 1,847 | 1,925 | 1,713 | 803 |
| phrase p50 | 1.64 | 3.13 | 2.92 | 2.13 | 2.21 |
| typo p50 | 19.59 | 14.75 | 20.25 | 17.79 | – |
| vector p50 | 1.80 | 4.42 | 4.56 | 3.87 | 3.25 |
| vector q/s, 16 clients | 1,660 | 1,435 | 1,079 | 764 | 503 |
| hybrid (RRF) p50 | 4.05 | 10.22 (trial licence, one pass) | 7.60 | 7.67 | 7.50 |
| sql aggregate p50 | 184.10 | 6.44 | 4.69 | 62.97 | 98.45 |

| 10M | Strata | Elasticsearch | OpenSearch |
|---|---|---|---|
| bm25 p50 | 2.96 | 5.25 | 3.48 |
| bm25 q/s, 16 clients | 470 | 543 | 896 |
| phrase p50 | 2.37 | 5.79 | 4.22 |
| typo p50 | 81.43 | 37.75 | 44.81 |
| vector p50 | 2.24 | 4.84 | 4.36 |
| vector q/s, 16 clients | 1,508 | 980 | 995 |
| similar items p50 | 3.60 | 36.37 | 28.01 |
| sql aggregate p50 | 2,894 | 7.11 | 4.08 |

Of the 14 kinds, Strata had the lowest p50 at 1 client on 10 among the five
sharded engines at 1M and on 11 of the three at 10M. Where it is behind:
typo queries (2x Elasticsearch at 10M), SQL aggregates (by two to three
orders of magnitude), and BM25 under load at 10M, where OpenSearch serves
1.9 times as many queries a second at 16 clients; Strata's p90 there is
76 ms against a p50 of 19 (OpenSearch 30 and 15), so the shards, not the
coordinator's statistics round that #245 removed, are what waits.

Recall@10 of the vector kinds against an exact search (pass 1): at 1M
Strata 0.807 (Elasticsearch 0.720, OpenSearch 0.655, Solr 0.851); at 10M
Strata 0.782 (Elasticsearch 0.680, OpenSearch 0.714).

## Strata before and after #242 and #245

On other nodes than the first runs, so the change is the code and the
machines together; the Lucene engines' own change between the runs shows
how much is the machines.

| median of passes | first 1M run | this run | first 10M run (one pass) | this run |
|---|---|---|---|---|
| Strata bm25 p50 ms | 2.96 | 1.70 | 4.36 | 2.96 |
| Strata bm25 q/s, 16 clients | 1,128 | 1,962 | 493 | 470 |
| Strata typo p50 ms | 119.59 | 19.59 | 823.82 | 81.43 |
| Strata sql aggregate p50 ms | 178.15 | 184.10 | 4,274 | 2,894 |
| Elasticsearch bm25 p50 ms | 2.74 | 3.25 | 5.39 | 5.25 |
| OpenSearch bm25 p50 ms | 2.89 | 3.18 | 4.32 | 3.48 |

## How far the nodes drifted

Strata ran first and again last in each sharded lane, on the same shard
files (`strata-b/`). Over the 42 timed cells (14 kinds at 1, 4 and 16
clients; p50 at 1 client, queries a second at 4 and 16), the second round
differed from the first by a median of 1.4% at 1M (the most 9.9%, hybrid
(weighted) at 16 clients) and 3.4% at 10M (the most 17.7%, SQL aggregates;
BM25 at 16 clients 8.1% faster the second time). That bounds what the order
of the engines on the nodes can have done to them.

## Milvus

Last in the one-node lane, Milvus ran 2 to 3 times slower than in the first
1M run on every kind, while the other eight there were within about 20% of
theirs. It was not the lane: the one-node engines keep their files in an
emptyDir, deleted with the pod. Milvus was still compacting during its first
two passes: its index wait ended after 163 s (325 s when alone), it used 4
times the CPU per query in pass 1, less in pass 2, and its pass 3 matched
the run alone (bm25 p50 2.13 ms against 2.01). Run again alone on a fresh
node (job `milvus-solo`, its three passes alike and like the first 1M run's),
those are the numbers in `1m/`; the lane's files are kept in
`1m/jobs/one-1m/lane-*`. The fresh node had an 8358, not the lane's 8280.

## What else went differently

- OpenSearch's 10M load took 200 minutes (162 in the 10M run): sending took
  124 minutes (92), its merge 75 (70). Elasticsearch's took 153 (136), its
  merge 69 (55). Strata's offline build took 68 minutes.
- Solr's first pass at 1M was 3 to 4 times slower than its other two on its
  first kinds (bm25 p50 7.36 ms, then 2.29 and 2.31; vector 16.44, then 3.80
  and 3.87): it came up cold after Strata, Elasticsearch and OpenSearch. The
  median takes the warm passes; `tables.md`, pass 1, shows the cold one.
- The cloud container did not restart this time; nothing was finished by
  hand.

## Running it again

```bash
cd strata-server/deploy/compare
STATE=~/.strata-fanout-rerun ./fanout.sh up 16          # or reuse a cluster
STATE=~/.strata-fanout-rerun ./fanout.sh prep 10000000  # 1M: its first million (see data above)
STATE=~/.strata-fanout-rerun PASSES=3 BUDGET=100 \
  IMAGE=ghcr.io/chloetech-ai/strata-server:055f6bf72d82812b98b30df5088dfd7295fecc0e \
  ./fanout.sh run "$PWD/fanout-jobs-same-nodes.tsv"
python3 rank.py --no-load ../../results/compare-msmarco-same-nodes-do/1m
```

## Files

`1m/` and `10m/`, each as `fanout.sh analyze` lays a run out:
`run-<engine>.json.gz` (pass 1 with its answers), `run-<engine>-pass2/3.json.gz`
(timings only: the answers are pass 1's queries again), `load-*`, `disk-*`,
`build-strata-s*.json`, `writes-*`, `stream-*`, `cpu.jsonl`, `tables.md` and
`summary.json` (analyze.py, pass 1), `passes.md`, `rank.md` and
`rank-no-load.md` (medians of the passes), `strata-b/` (Strata's second
round, analysed alone), `jobs/<job>/` (nodes and their checks, steps, logs)
and `data/`. Every report of every pass, whole, with the run's state and
scripts, is in the project's files as
`compare-msmarco-same-nodes-do/raw-run-state.tar.gz`; the 10M collection is
in the Spaces bucket `strata-bench-datasets` (sfo2).

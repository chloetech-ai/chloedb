# The engine comparison on ChloeDB 0.3.5, result caches off (DigitalOcean, 15 and 16 November 2026)

The website's comparison ([compare-msmarco-same-nodes-do](../compare-msmarco-same-nodes-do/),
4 November) ran ChloeDB `055f6bf7`, from before the beam's default moved
from 64 to 200 (cf17355e) and before 143 later pull requests. This run is
the same comparison on 0.3.5, laid out the same way (each lane's engines on
one set of nodes, one after another, ChloeDB first and again last), with
the harness of #575 and three changes to how it is run:

- Every engine's result cache is off, and each run records the cache's
  counters before and after (`result_cache` in each `run-*.json`):
  Elasticsearch and OpenSearch with `request_cache=false` on each search and
  `index.requests.cache.enable: false` on the index (0 hits in every run),
  Solr's `queryResultCache` of size 0, Manticore's `qcache_max_bytes=0`,
  Typesense without `use_cache`, and ChloeDB's answer cache off
  (`--answer-cache-mb 0`, so its counter is `null`: there is no cache to
  count). The others have no result cache. Page caches, filter caches and
  each engine's own memory stay as they are.
- Every vector statement carries its candidates: ChloeDB gets `ef => 64` in
  the default run (its own default is 200 since cf17355e, so the 4 November
  run's ChloeDB vector rows did search 64), and Postgres sets
  `hnsw.ef_search` to them (before, it searched with pgvector's default,
  whatever the run asked).
- A candidates sweep (128, 256, 512; 128 and 256 at 10M) for ChloeDB,
  Elasticsearch, OpenSearch, Solr and Vespa in the sharded lanes, and for
  Qdrant and Postgres in a lane of their own (`1m-ef-lane/`).

| | |
|---|---|
| ChloeDB image | `ghcr.io/chloetech-ai/chloedb-server:324199900231276e6e34e3600710784e61361d1b-bench`: the 0.3.5 tag's commit (1445c103) and #575's two harness commits, which change only `engine-compare` |
| Vespa lane's image | `ghcr.io/chloetech-ai/chloedb-server:6b4cf08e578ea8a87ecfb0e9ca1e53fd1103e9c9-bench`: main at 15807c92 (0.3.6's freeze and #587) and this branch's Vespa retry, see below; its ChloeDB rounds are only that lane's yardstick, and no ChloeDB number in `1m/` comes from it |
| scripts | `fanout.sh` and `fanout-jobs-0.3.5.tsv`, `fanout-jobs-0.3.5-10m.tsv`, `fanout-jobs-0.3.5-vespa.tsv` in `chloedb-server/deploy/compare/`, `PASSES=3` |
| cloud | DigitalOcean Kubernetes, sfo2; a base node (`s-8vcpu-16gb`) keeps the collections and runs `analyze.py` |
| 1M sharded lane | ChloeDB, Elasticsearch, OpenSearch, Solr, ChloeDB again: 2 x `m-4vcpu-32gb` (Xeon Platinum 8168 and 8358) and a `c-8` client (8168) |
| 1M Vespa lane | ChloeDB, Vespa, ChloeDB again: 2 x `m-4vcpu-32gb` (8168 and 8168) and a `c-8` client (8168) |
| 1M one-node lane | Qdrant, Meilisearch, Postgres, Redis, Weaviate, Manticore, ParadeDB, Typesense: 1 x `m-8vcpu-64gb` (Gold 6248) and a `c-8` client (8168) |
| 1M Milvus | alone, as the 4 November run ended up running it: 1 x `m-8vcpu-64gb` (Gold 5318Y) and a `c-8` client (8358) |
| 1M candidates lane | Qdrant and Postgres, each at 64, 128, 256 and 512: 1 x `m-8vcpu-64gb` (8168) and a `c-8` client (8168) |
| 10M lane | ChloeDB, Elasticsearch, OpenSearch, ChloeDB again: 2 x `m3-4vcpu-32gb` (8280 and 8280) and a `c-8` client (8168) |
| CPUs and memory | as in the 4 November run: 6.8 CPUs per engine (a sharded engine's two data pods 3.4 each, or ChloeDB's shards 2.9 each and its coordinator 1), 52 GiB for a one-node engine |
| passes | three passes of every kind after warm-up rounds; `tables.md` shows pass 1, `passes.md`, the rankings and every number below are the median of the three |
| data | the 4 November run's collections: `docs.jsonl`, the vectors, queries and judgments are byte for byte the same (`data/SHA256SUMS`); `meta.json` has three more fields, which the newer prep writes |
| time | 16 November 00:58 to 11:28 UTC (15 November 4:58 PM to 16 November 3:28 AM PST): the 1M sharded lane 4.8 h, one-node lane 8.3 h, Milvus 0.5 h, candidates lane 4.2 h, Vespa lane 2.5 h (from 06:15), 10M lane 10.2 h, all at once |
| cost | $28.67 at list price (pools rounded up to whole hours, and the base node from 00:18 UTC, when the cluster came up, to its deletion at 12:38, after the analysis); the cluster was deleted with its resources, and no droplet of it is left |

Every step of every job passed but one: the sharded lane's Vespa load,
which ended at its last batch (below), so Vespa ran again in a lane of its
own.

## Ranking

`rank.py --no-load` on each set: places summed over 45 tests (14 kinds at
p50 and p99 with 1 client and queries a second with 4, and recall@10 of the
3 vector kinds), on the median of the three passes, load time and size left
out as before. Vespa's numbers in `1m/` are its own lane's.

| 1M, all 14 engines | points | 4 November | | 10M, ChloeDB, Elasticsearch, OpenSearch | points | 4 November |
|---|---|---|---|---|---|---|
| ChloeDB | 130 | 156 | | ChloeDB | 56 | 67 |
| Elasticsearch | 268 | 284 | | Elasticsearch | 106 | 120 |
| Solr | 270 | 287 | | OpenSearch | 108 | 83 |
| OpenSearch | 288 | 292 | | | | |
| Qdrant | 308 | 302 | | | | |
| Milvus | 337 | 310 | | | | |
| Redis | 345 | 350 | | | | |
| Manticore | 366 | 370 | | | | |
| Meilisearch | 373 | 368 | | | | |
| Vespa | 377 | 377 | | | | |
| Typesense | 417 | 401 | | | | |
| Postgres | 434 | 429 | | | | |
| Weaviate | 437 | 423 | | | | |
| ParadeDB | 441 | 440 | | | | |

The one-node lane's engines ran on another node than the sharded lane's,
and Vespa and Milvus on nodes of their own, so a place between engines of
two lanes still rests on two machines.

## ChloeDB against the Lucene engines and Vespa

Medians of three passes; p50 in ms at 1 client, queries a second at 16.
Elasticsearch's hybrid (RRF) needs its trial licence and has one pass
(`es-trial`), so it is left out here.

| 1M | ChloeDB | Elasticsearch | OpenSearch | Solr | Vespa |
|---|---|---|---|---|---|
| bm25 p50 | 2.03 | 2.88 | 2.80 | 2.12 | 3.16 |
| bm25 q/s, 16 clients | 2,387 | 1,870 | 2,062 | 1,688 | 733 |
| bm25 + filter q/s, 16 clients | 2,079 | 1,388 | 1,437 | 1,622 | 707 |
| phrase p50 | 1.65 | 2.76 | 2.72 | 1.92 | 2.29 |
| typo p50 | 16.52 | 14.56 | 20.39 | 17.71 | – |
| prefix p50 | 2.15 | 3.60 | 3.81 | 2.11 | – |
| key lookup p50 | 1.03 | 1.03 | 0.89 | 1.27 | 1.15 |
| vector p50 | 2.17 | 3.90 | 4.42 | 3.93 | 3.42 |
| vector q/s, 16 clients | 2,374 | 1,398 | 993 | 750 | 507 |
| vector + filter p50 | 2.26 | 4.48 | 8.59 | 6.99 | 6.17 |
| hybrid (RRF) p50 | 4.67 | – | 7.86 | 7.98 | 8.32 |
| hybrid (RRF) q/s, 16 clients | 768 | – | 429 | 345 | 295 |
| similar items p50 | 2.80 | 6.65 | 8.78 | 6.36 | 5.84 |
| sql aggregate p50 | 7.47 | 49.28 | 55.32 | 54.76 | 168.94 |

| 10M | ChloeDB | Elasticsearch | OpenSearch | ChloeDB, 4 November |
|---|---|---|---|---|
| bm25 p50 | 3.28 | 4.44 | 3.71 | 2.96 |
| bm25 q/s, 16 clients | 650 | 662 | 820 | 470 |
| bm25 + filter q/s, 16 clients | 550 | 386 | 368 | 222 |
| phrase p50 | 1.88 | 4.86 | 4.32 | 2.37 |
| typo p50 | 55.06 | 32.39 | 44.37 | 81.43 |
| prefix p50 | 5.12 | 8.42 | 9.74 | 9.07 |
| key lookup p50 | 0.99 | 1.06 | 0.81 | 0.76 |
| vector p50 | 2.67 | 4.01 | 4.22 | 2.24 |
| vector q/s, 16 clients | 1,317 | 1,292 | 1,082 | 1,508 |
| vector + filter p50 | 2.81 | 5.27 | 8.43 | 2.97 |
| hybrid (RRF) p50 | 7.75 | – | 11.88 | 8.06 |
| hybrid (RRF) q/s, 16 clients | 255 | – | 180 | 196 |
| similar items p50 | 3.71 | 24.34 | 31.38 | 3.60 |
| sql aggregate p50 | 35.64 | 488.48 | 650.68 | 2,894 |

At 10M ChloeDB had the lowest p50 at one client on 12 of the 14 kinds
(Elasticsearch on typo, OpenSearch on key lookups) and the most queries a
second at 4 clients on 11 (OpenSearch on BM25 and key lookups,
Elasticsearch on typo); at 16 clients OpenSearch serves 1.3 times as many
BM25 queries a second (820 against 650). Its SQL aggregate went from 2.9 s
to 36 ms; Elasticsearch's and OpenSearch's, uncached now, take 0.5 and
0.65 s (7.11 and 4.08 ms on 4 November, from their request caches).
ChloeDB's vector search at 16 clients is 13% slower than on 4 November, at
the same 64 candidates.
ChloeDB's offline build of the two shards took 93 minutes (68 on 4
November); Elasticsearch's load 151 (153), OpenSearch's 183 (200).

Recall@10 of the vector kinds against an exact search (pass 1), at 64
candidates: at 1M ChloeDB 0.809 (vector), 0.862 (vector + filter), 0.995
(similar items), against 0.807, 0.828 and 0.990 on 4 November;
Elasticsearch 0.738, OpenSearch 0.650, Solr 0.788 (0.851 on 4 November, not
looked into), Qdrant 0.909, Milvus 0.906 on vector. At 10M ChloeDB 0.798,
0.816 and 0.981 (0.782, 0.824 and 0.968 on 4 November), Elasticsearch
0.665, OpenSearch 0.637 on vector.

`ef.md` in each set has recall and latency at each candidates value. On
vector at 1M, from 64 to 512 candidates, ChloeDB goes from 0.809 to 0.957
(p50 2.17 to 3.33 ms, 2,374 to 1,055 q/s at 16 clients), Solr 0.788 to
0.939, Vespa 0.781 to 0.944, Elasticsearch 0.738 to 0.900, OpenSearch 0.650
to 0.766, and Qdrant, on its own node, 0.896 to 0.984 (1.90 to 8.16 ms, 1,730
to 340 q/s). At 10M and 256 candidates ChloeDB reaches 0.914 (3.74 ms),
Elasticsearch 0.767 (4.83), OpenSearch 0.757 (5.69).

## What changed since 4 November, and what is the machines

The sharded lane's Lucene engines had about the same machines as on
4 November (8168 and 8358 against 8280 and 8168): over their timed cells
Elasticsearch moved by a median of -0.9%, OpenSearch 0.0% and Solr -0.4%
(a minus is faster). ChloeDB on the same nodes moved by -17.1%, with these
rows changing most:

| 1M ChloeDB, median of passes | 4 November | 0.3.5 |
|---|---|---|
| bm25 q/s, 16 clients | 1,962 | 2,387 |
| bm25 + filter q/s, 16 clients | 1,492 | 2,079 |
| vector q/s, 16 clients | 1,660 | 2,374 |
| prefix p50 ms | 2.80 | 2.15 |
| typo p50 ms | 19.59 | 16.52 |
| sql aggregate p50 ms | 184.10 | 7.47 |
| key lookup p50 ms | 0.79 | 1.03 |
| bm25 p50 ms | 1.70 | 2.03 |
| vector p50 ms | 1.80 | 2.17 |

ChloeDB serves more queries a second at 4 and 16 clients and answers SQL
aggregates 25 times faster, but its p50 at one client is about 0.25 ms a
query higher on key lookups, BM25 and vectors, where the Lucene engines did
not move. On a cloud container (4 CPUs, Xeon at 2.1 GHz),
`scripts/perf-gate.py`'s in-process BM25 timings of 0.3.5 against 055f6bf7
are faster on four of five measures (plain BM25 on ArguAna 15% slower on
its median, not a regression by the gate's rule), and
`chloedb-server-bench` at one client on SciFact answers 6 to 17% slower
over HTTP on one database (0.02 to 0.04 ms a request; vectors 25%, since
its default beam is now 200) and from -10% to +11% through a coordinator
over two shards. So the extra quarter of a millisecond is in the server and
coordinator path on those machines, not the engine; it is not bisected yet.

The one-node lane got a Gold 6248 where it had a Platinum 8280, and every
engine there is slower by a median of 5 to 15% (Qdrant 12.1%, Meilisearch
10.1%, Postgres 6.3%, Redis 4.8%, Weaviate 12.8%, Manticore 5.1%, ParadeDB
9.4%, Typesense 15.0%); their key lookups all rose by 0.13 to 0.15 ms
(Redis 0.09 to 0.23), so part of it is the client and the network. Milvus,
alone on a Gold 5318Y (8358 before), is 21.8% slower, and Vespa, in its own
lane, 5.9%. Postgres now searches with `hnsw.ef_search` set to the run's
candidates (pgvector's default is 40): its vector recall is 0.808 (0.792 on
4 November); in its own lane, on another build of its index, it is 0.780 at
64 and 0.955 at 512 (`1m-ef-lane/ef.md`).

Elasticsearch's and OpenSearch's SQL aggregates take 49 and 55 ms with
their request caches off, against 6.44 and 4.69 on 4 November, when the
cache answered them.

## How far the nodes drifted

ChloeDB ran first and again last in each sharded lane, on the same shard
files (`strata-b/`). Over the 42 timed cells (14 kinds; p50 at 1 client,
queries a second at 4 and 16 clients), the second round was slower by a
median of 2.3% in the 1M sharded lane (the most 9.9%, BM25 p50), 2.2% in
the Vespa lane (the most 4.7%, phrase p50) and 1.0% in the 10M lane (the
most 4.9%, vectors at 16 clients). The Vespa lane's ChloeDB (0.3.6's main)
was 7.9% slower than the sharded lane's, on its other pair of nodes.

## Vespa

In the sharded lane, Vespa's 1M load ended at its last batch after 23
minutes with `put: error sending request`: the container closed a
connection under a request, and engine-compare retried only refusals
(429, 503, 504, 507). It now retries any put that fails to send, as a put
of an id is idempotent (this branch's 6b4cf08e). Vespa ran again on a fresh
pair of nodes with ChloeDB before and after it (`fanout-jobs-0.3.5-vespa.tsv`,
job `vespa-1m`): every step passed and its load took 26 minutes.
`1m-vespa-lane/` is that lane analysed alone; `1m/` takes its Vespa files.

## Running it again

```bash
cd chloedb-server/deploy/compare
STATE=~/.chloedb-fanout ./fanout.sh up 16
STATE=~/.chloedb-fanout ./fanout.sh prep 1000000     # and 10000000 for the 10M lane
STATE=~/.chloedb-fanout PASSES=3 BUDGET=35 \
  IMAGE=ghcr.io/chloetech-ai/chloedb-server:324199900231276e6e34e3600710784e61361d1b-bench \
  ./fanout.sh run "$PWD/fanout-jobs-0.3.5.tsv"
python3 rank.py --no-load ../../results/compare-msmarco-0.3.5-do/1m
```

`fanout.sh analyze` merges every job into one directory, and these jobs
share labels (two ChloeDB lanes and two Qdrant lanes at 1M), so each set
here was put together from its jobs' files and analysed alone.

## Files

`1m/` (the 1M sharded, one-node and Milvus jobs, Vespa from its own lane),
`1m-vespa-lane/`, `1m-ef-lane/` and `10m/`, each as `fanout.sh analyze`
lays a run out: `run-<label>.json.gz` (pass 1 with its answers),
`run-<label>-pass2/3.json.gz` (timings only: the answers are pass 1's
queries again), `load-*`, `disk-*`, `build-strata-s*.json`, `writes-*`,
`stream-*`, `cpu.jsonl`, `tables.md` and `summary.json` (analyze.py, pass
1), `ef.md` (ef.py, the sweep), `passes.md`, `rank.md` and
`rank-no-load.md` (medians of the passes), `strata-b/` (ChloeDB's second
round, analysed alone), `jobs/<job>/` (nodes and their checks, steps, logs)
and `data/`. Every report of every pass, whole, with the run's state, logs
and the scripts that put the sets together (no credentials), is in the
project's files as `compare-msmarco-0.3.5-do/raw-run-state.tar.gz`; the 10M
collection is the one in the Spaces bucket `strata-bench-datasets` (sfo2).

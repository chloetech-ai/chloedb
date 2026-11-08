# Strata and thirteen other engines on DigitalOcean, MS MARCO 10M

**Each engine on machines of its own, all at once, at ten million
passages.** Measured between 09:16 UTC on 2026-11-03 and 01:53 UTC on
2026-11-04 with [`deploy/compare/fanout.sh`](../../deploy/compare/fanout.sh),
which runs [`deploy/compare/compare.sh`](../../deploy/compare/compare.sh)'s
steps for every engine on DigitalOcean node pools of its own (method,
budgets and query mapping in
[`deploy/compare/README.md`](../../deploy/compare/README.md)). This is stage
2 of the plan: the [1M run](../compare-msmarco-1m-do/README.md) tried the
runner and the data; this one runs every engine once at 10M, with the jobs
in [`fanout-jobs-10m.tsv`](../../deploy/compare/fanout-jobs-10m.tsv).

| | |
|---|---|
| cloud | DigitalOcean, region sfo2, one DOKS cluster (Kubernetes 1.37.0, nodes on Debian 13, kernel 6.12.113+deb13-amd64) |
| machines per engine | Elasticsearch, OpenSearch, Solr, Vespa and Strata: 2 x `m3-4vcpu-32gb` (4 vCPUs, 32 GB, 300 GB disk each); Meilisearch, Typesense, Manticore, Qdrant, Weaviate, Milvus, ParadeDB and Postgres: 1 x `m3-8vcpu-64gb` (8 vCPUs, 64 GB, 600 GB disk), since Meilisearch's, Milvus's and ParadeDB's files pass 200 GB at 10M; every engine also has a `c-8` (8 vCPUs, 16 GB) for its client (and Strata's coordinator, Vespa's `vespa-0`, Redpanda). Each job's nodes are its own: labelled and tainted for it, and deleted when it ends |
| the 128 GB lane | Redis, which keeps everything in memory (8.2 GB at 1M), and Milvus after it ran out of memory at 52 GiB (Typesense's rerun there did not fit the budget): 1 x `m-16vcpu-128gb` with the same 6.8 CPUs and 110 GiB of memory. These are not equal resources, so their numbers are reported apart and are not in the ranking |
| CPUs | Intel Xeon Platinum 8168 and 8280, Gold 6248, 5318Y and 6140, as DigitalOcean handed them out (the table below says which job had which) |
| node check | before a job starts, each of its nodes hashes 1 GB per vCPU (SHA-256); a node more than 5% slower than the median of its size so far was replaced once (3 of the 46 recorded checks were) |
| data | MS MARCO v2.1 segmented passages, the first 10,000,000, with their Snowflake Arctic Embed M v1.5 vectors (768 dimensions) as published, made by [`strata/scripts/prepare_msmarco_compare.py`](../../../strata/scripts/prepare_msmarco_compare.py); 1,000 queries from the passage v2 dev set, embedded with the same model; synthetic `year`, `category`, `rating`, `topic`. Copied to every node that reads it and checked against SHA-256 sums |
| budget | 6.8 CPUs per engine (Kubernetes limits), as at 1M. Elasticsearch, OpenSearch and Solr: 2 data nodes of 3.4 CPUs and 26 GiB (heap 13 GB, Solr 8 GB). Strata: 2 shards of 2.9 CPUs and 24 GiB and a coordinator of 1 CPU and 4 GiB; each shard built on its node with 3.4 CPUs, 26 GiB and 4 threads, both at once. Vespa: 2 content nodes of 2.9 CPUs and 23 GiB, `vespa-0` 1 CPU and 6 GiB. One-node engines: 6.8 CPUs and 52 GiB (Postgres and ParadeDB with `shared_buffers` 13 GB) |
| queries | 300 of the 1,000 queries, every kind at 1, 4 and 16 clients for 6 s each, **one pass**; before a kind is timed it is warmed up in rounds until two rounds' medians agree within 10% (`warm_up_p50_ms` in each run's report). Writes and streams for Strata, Elasticsearch, OpenSearch and Vespa (Solr's writes step failed, below) |
| deadlines | the client waits 30 s for an answer from every engine, and Strata's server stops a query after 30 s too (`--timeout-ms`, #244) |
| quality | recall@10 against an exact search over the same 10M vectors (computed on the base node). BM25 has no exact answer here: `tables.md` gives each engine's overlap with Strata's top 10 instead. There is no nDCG: the TREC RAG judgments cover the 113M passages, not this slice |
| cost | $83.07 at list price when the last job ended (01:53), plus the base node until the cluster is deleted ($0.14 an hour), every node pool counted in whole hours (DigitalOcean bills by the hour), the base node (`s-8vcpu-16gb`) and the failed attempts included; the cap was $75, raised to $90 during the run so that every engine could be tried |

## Jobs

Each job's engine nodes and client node, with each node's check (seconds
to hash 1 GB per vCPU; lower is faster), and when its steps ran (UTC, 3
November unless marked). Every job's client (and Strata's server) came from
the image `ghcr.io/chloetech-ai/strata-server:f64dbb459febab46154e465ca2c87f0dc9b183a5`,
the last commit of #240; #244's changes are to the deployment (Strata's deadline,
Solr's stall limit, the jobs' order), which the run took from its copy of
`deploy/compare`.

| job | engine nodes | client node | ran (UTC) | outcome |
|---|---|---|---|---|
| es | m3-4vcpu-32gb x2: Gold 6248 @ 2.50GHz (11.5 s), not recorded | Platinum 8280 (10.0 s) | 13:46-16:17 | done |
| manticore | m3-8vcpu-64gb: Platinum 8280 (10.5 s) | Platinum 8168 (11.5 s) | 09:55-17:42 | load not done after 8 h |
| meili | m3-8vcpu-64gb: Gold 6248 @ 2.50GHz (11.4 s) | Platinum 8168 (11.6 s) | 09:55-18:42 | load not done after 9 h |
| milvus | m-16vcpu-128gb: Gold 6140 @ 2.30GHz (12.9 s) | Platinum 8168 (11.3 s) | 19:00-21:07 | done, 128 GB lane |
| milvus.attempt2 | m3-8vcpu-64gb: Gold 6248 @ 2.50GHz (11.4 s) | Platinum 8168 (11.4 s) | 14:18-15:15 | out of memory building its index (first try) |
| os | m3-4vcpu-32gb x2: Gold 5318Y @ 2.10GHz (13.8 s), Gold 5318Y @ 2.10GHz (14.2 s) | Platinum 8280 (9.9 s) | 15:24-18:20 | done |
| paradedb | m3-8vcpu-64gb: Platinum 8280 (10.1 s) | Platinum 8168 (11.4 s) | 21:15-23:24 | done |
| postgres | m3-8vcpu-64gb: Platinum 8168 (11.3 s) | Platinum 8168 (11.3 s) | 09:52-19:30 | searched (text kinds at a 20 s limit); past its 9 h before `down` |
| qdrant | m3-8vcpu-64gb: Platinum 8168 (11.4 s) | Platinum 8168 (11.4 s) | 09:40-22:40 | done |
| redis | m-16vcpu-128gb: Platinum 8280 (10.1 s) | Platinum 8168 (11.2 s) | 23:33-01:53 (4 Nov) | done, 128 GB lane |
| solr | m3-4vcpu-32gb x2: Platinum 8280 (9.9 s), Gold 6248 @ 2.50GHz (11.7 s) | Platinum 8168 (11.1 s) | 16:43-22:27 | loaded and searched; writes failed |
| solr.attempt1 | m3-4vcpu-32gb x2: Gold 6248 @ 2.50GHz (11.6 s), Platinum 8168 (11.7 s) | Platinum 8168 (11.2 s) | 11:29-12:21 | load stopped at 4.4M (stalled update) |
| strata | m3-4vcpu-32gb x2: Platinum 8168 (12.2 s), Platinum 8168 (11.2 s) | Platinum 8168 (11.3 s) | 12:48-14:28 | done |
| strata-b | m3-4vcpu-32gb x2: Platinum 8168 (11.8 s), Platinum 8168 (11.5 s) | Platinum 8168 (11.5 s) | 09:41-11:21 | done (5 s deadline: no vector or hybrid numbers) |
| strata.attempt1 | m3-4vcpu-32gb x2: Gold 6248 @ 2.50GHz (11.3 s), Platinum 8168 (11.3 s) | Platinum 8168 (11.7 s) | 09:41-11:18 | done, but with the 5 s deadline: rerun as `strata` |
| typesense | m3-8vcpu-64gb: Gold 6248 @ 2.50GHz (11.6 s) | Platinum 8168 (11.6 s) | 10:04-13:37 | out of memory at 7.2M; 128 GB rerun did not fit the budget |
| vespa | m3-4vcpu-32gb x2: Gold 5318Y @ 2.10GHz (14.4 s), Gold 5318Y @ 2.10GHz (13.8 s) | Platinum 8280 (10.8 s) | 10:08-14:08 | done |
| weaviate | m3-8vcpu-64gb: Gold 6248 @ 2.50GHz (11.4 s) | Platinum 8280 (9.8 s) | 16:43-18:50 | done |

`<job>.attempt<n>` is an earlier try of the same job (below says why).
Elasticsearch's second data node passed its check, but its log could not be
read (the cluster's agent was unavailable), so its CPU and time are not
recorded.

The engine images are the 1M run's (its README lists them):
Elasticsearch 9.5.5 (Basic licence; its RRF hybrid on the 30-day trial, as
its own row `es-trial`), OpenSearch 3.9.0, Solr 10.1.0, Vespa 8.778.45,
Meilisearch v1.56.0, Typesense 30.2, Manticore 29.9.0, Qdrant v1.20.0,
Weaviate 1.39.14, Milvus v2.6.26 (standalone), ParadeDB 0.26.4-pg17,
Postgres (the same image without `pg_search`), Redis 8.8.4 and Redpanda
v25.2.9.

## Ranking

[rank.md](rank.md) ranks the nine engines that finished at equal resources
by the sum of their places over 45 tests: each of the 14 query kinds at
p50 and p99 latency with 1 client and queries a second with 4, and recall
for the three vector kinds. A test an engine could not run counts as last
(9th). Strata is first with 133 points, OpenSearch second with 159 and
Elasticsearch third with 179.

    python3 deploy/compare/rank.py --without milvus,redis --no-load results/compare-msmarco-10m-do > rank.md

`--without` leaves out the 128 GB lane. `--no-load` leaves out the 1M
ranking's load time and index size tests, because Strata's load is an
offline build of its files from local copies on its own nodes, while every
other engine took the documents over HTTP. With them (47 tests) the top
three keep their places (Strata 138, OpenSearch 165, Elasticsearch 185),
and only Weaviate and Postgres swap at the bottom. The rule for kinds an
engine cannot run decides the bottom of the order: counted only on the
tests each engine ran, Postgres averages 2.72 (on its 18 vector tests)
against Strata's 2.96.

## The 128 GB lane

Milvus and Redis ran on an `m-16vcpu-128gb` with the same 6.8 CPUs and
110 GiB, so they are in `tables.md` and `summary.json` but not in the
ranking. Against Strata on its 52 GiB:

| | Strata | Milvus | Redis |
|---|---|---|---|
| BM25 p50, 1 client | 4.36 ms | 7.41 ms | 1,438 ms |
| BM25 q/s, 4 clients | 430 | 139 | 2 |
| vector p50, 1 client | 2.73 ms | 10.02 ms | 1.19 ms |
| vector q/s, 4 clients | 916 | 113 | 3,336 |
| vector recall@10 | 0.795 | 0.902 | 0.684 |
| vector + filter recall@10 | 0.823 | 0.930 | 0.667 |
| similar items recall@10 | 0.978 | 0.984 | 0.919 |
| key lookup p50, 1 client | 0.86 ms | 1.38 ms | 0.20 ms |
| load | 74 min (offline build) | 121 min | 68 min |
| index size | 48.9 GB | 234.9 GB | 78.7 GB (in memory) |
| own memory (anon) while searching | 2.2 GiB | 63.8 GiB | 72.1 GiB |

Redis held 72 GiB of its own during the searches (74.5 GiB at its peak),
more than the main lane's 52 GiB. Its recall went from 0.775 at 1M to
0.684 at 10M with the same 64 candidates; its BM25, facet, typo and hybrid
kinds took 0.8 to 1.5 s a query at 1 client, and its boosted BM25 and SQL
aggregate hit its own search timeout (`unavailable` in `run-redis.json`).
Milvus's first try, at 52 GiB, ran out of memory building its index.

## How far apart two runs of the same engine land

`strata-b` is Strata again, the same image and steps, on nodes of its own,
run before the 30 s deadline went in: its vector and hybrid kinds hit the
server's old 5 s limit on their first cold queries and have no numbers, so
they are not compared. On the kinds both answered, at 1, 4 and 16 clients,
p50 latency and queries a second differ by 4.5% at the median and 19% at
most (key lookups, the fastest kind: 0.97 against 1.15 ms at 4 clients).
At 1M the two differed by 12%; at 10M, with every kind warmed up first, the
gap halved. Differences between engines of less than about 10% here are
within what a second set of machines changes.

## What went wrong on the way

No number in `tables.md`, `summary.json` or `rank.md` comes from a failed
attempt.

- **Strata's first run timed out on vectors** (`jobs/strata.attempt1`):
  strata-server stops a query after 5 s by default, and the first cold
  vector search over 10M took longer, while the client waits 30 s for every
  other engine. Strata ran again with a 30 s deadline (#244). The second
  Strata (`strata-b`) had started with the first settings and is used only
  for the noise check above.
- **Solr's first load stopped at 4.4M documents** (`jobs/solr.attempt1`):
  SolrCloud's client gave up on a forwarded update it took for stalled
  after 15 s. With `solr.cloud.client.stallTime` at 10 minutes (#244) it
  loaded all 10M. Its writes step then failed waiting for a configuration
  change to reach every core within 30 s (`jobs/solr/log`), so Solr has no
  writes or stream numbers.
- **Out of memory at 52 GiB**: Typesense at 7.2M documents, and Milvus
  while building its index after all 10M were sent (`jobs/milvus.attempt2`,
  its first try: an attempt directory takes the number of the attempt
  counter, which a blocked retry also moved; `jobs/typesense/failures`).
  Milvus ran again on the 128 GB lane. Typesense's rerun there (8 hours on
  the 128 GB node) did not fit the $90 budget and was skipped, so Typesense
  has no 10M numbers.
- **Loads that did not finish**: Meilisearch had indexed about 4M documents
  when its 9 hours ran out, and Manticore was still merging (OPTIMIZE, two
  chunks left) after 8. Both were stopped at their time limits; the budget
  allowed no longer ones.
- **Postgres's text search**: its BM25 averaged 28.4 s a query on a 107 GB
  table that does not fit in 52 GiB (127 of 300 probe queries in 67
  minutes), so the text kinds alone would have run for about 20 hours. At
  19:09 a 20 s `statement_timeout` was set on the running server
  (`jobs/postgres/notes.txt`, with `pg_stat_statements` at 19:15), which
  turned its text kinds into errors and let the vector kinds finish. The job
  then passed its 9 hour limit before its last step (`down`), so its state
  reads failed; its reports were fetched.
- **Weaviate's facet and SQL aggregate requests** failed with a connection
  error at 10M; they ran at 1M.
- **Strata's image predates two fixes** for its weak spots here: the typo
  rewrite (#242, merged after this run's image) and the BM25 term
  statistics cache (#245, merged at 01:47 on 4 November, after this run's image).
- **The first base node ran out of memory** preparing the 10M data, on an
  `s-4vcpu-8gb`; the run used an `s-8vcpu-16gb`.
- **The cloud container driving the run restarted at about 01:45**, while
  Redis's searches ran, and took the driver with it (its state, on disk,
  survived). Redis's job on the cluster went on; its last steps (the
  engine's CPU and memory after the run, `down`, fetching the reports,
  deleting its pools) were done by hand as the driver does them, and the
  part of its log after the cut was read again from the cluster
  (`jobs/redis/log`).
- **The proxy between the cloud container and the cluster cut long
  connections** (followed logs, `kubectl exec` streams), so the analysis
  ran detached on the base node, as at 1M.

## Running it again

The run used the scripts of `deploy/compare` at main `bd1981c` (#244),
with the jobs file as committed here, and this command (`DIGITALOCEAN_TOKEN`
in the environment; `up` and `prep` first):

    cd strata-server/deploy/compare
    BASE_SIZE=s-8vcpu-16gb ./fanout.sh up 24
    ./fanout.sh prep 10000000
    IMAGE=ghcr.io/chloetech-ai/strata-server:f64dbb459febab46154e465ca2c87f0dc9b183a5 \
      MAX_NODES=21 SHARDED_SIZE=m3-4vcpu-32gb ONE_SIZE=m3-8vcpu-64gb PASSES=1 \
      LOAD_HOURS=16 COPY_JOBS=3 BUDGET=90 COLLECTION=msmarco-10000k \
      ./fanout.sh run fanout-jobs-10m.tsv
    ./fanout.sh analyze /path/to/reports && ./fanout.sh down

`prep` builds the collection from the public MS MARCO v2.1 segmented
passages and their published Arctic Embed vectors with
`prepare_msmarco_compare.py`, and [`data/SHA256SUMS`](data/SHA256SUMS)
holds the sums of what this run used (`fanout.sh` checks every copy against
them), so a rebuilt collection can be checked byte for byte before it is
compared with these numbers.

## Files

- `tables.md`: every table, generated by `analyze.py` on the base node from
  each job's single pass: load and size, latency and throughput at 1, 4 and
  16 clients, recall, SQL aggregates, agreement with Strata, writes,
  freshness, streams, CPU and memory. It covers the engines of both lanes;
  the 128 GB lane's rows are marked only by this README.
- `latency-full.md`, `summary.json`: the same numbers, every percentile.
- `rank.md`: the ranking above, from `rank.py`.
- `run-*.json.gz`, `load-*.json`, `disk-*`, `build-strata-s*.json` (and the
  builds' logs), `writes-*.json`, `stream-*.json`, `cpu.jsonl`: the reports.
- `strata-b/`: the second Strata's reports and tables.
- `data/`: the input collection's `meta.json` (documents, queries, vector
  size, rows per source shard) and `SHA256SUMS`.
- `jobs/<job>/`: each job's nodes and their check (`nodes.jsonl`), its steps
  with their start and end (`steps.tsv`), its log, why it failed (`reason`)
  and on failure the pods' descriptions (`failures/`); a failed attempt's
  own reports in `reports/`.

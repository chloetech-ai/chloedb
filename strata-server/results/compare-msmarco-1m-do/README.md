# Strata and thirteen other engines on DigitalOcean, MS MARCO 1M

**Each engine on machines of its own, all at once.** Measured on 2026-11-03
between 02:38 and 05:47 UTC with
[`deploy/compare/fanout.sh`](../../deploy/compare/fanout.sh), which runs
[`deploy/compare/compare.sh`](../../deploy/compare/compare.sh)'s steps for
every engine on DigitalOcean node pools of its own (method, budgets and
query mapping in [`deploy/compare/README.md`](../../deploy/compare/README.md)).
This is stage 1 of the plan to run the comparison at 10M: the same steps at
1M, to try the runner and the data before the large run.

| | |
|---|---|
| cloud | DigitalOcean, region sfo2, one DOKS cluster (Kubernetes 1.37.0, nodes on Debian 13, kernel 6.12.113+deb13-amd64) |
| machines per engine | Elasticsearch, OpenSearch, Solr, Vespa and Strata: 2 x `m-4vcpu-32gb` (4 vCPUs, 32 GB each); Meilisearch, Typesense, Manticore, Qdrant, Weaviate, Milvus, ParadeDB, Postgres and Redis: 1 x `m-8vcpu-64gb`; every engine also has a `c-8` (8 vCPUs, 16 GB) for its client (and Strata's coordinator, Vespa's `vespa-0`, Redpanda). Each job's nodes are its own: labelled and tainted for it, and deleted when it ends |
| CPUs | Intel Xeon Platinum 8168, 8280 and 8358 and Gold 6248, as DigitalOcean handed them out (the table below says which job had which) |
| node check | before a job starts, each of its nodes hashes 1 GB per vCPU (SHA-256); a node more than 5% slower than the median of its size so far was replaced once (7 of the 63 nodes checked were) |
| data | MS MARCO v2.1 segmented passages, the first 1,000,000 of shard 00, with their Snowflake Arctic Embed M v1.5 vectors (768 dimensions) as published, made by [`strata/scripts/prepare_msmarco_compare.py`](../../../strata/scripts/prepare_msmarco_compare.py); 1,000 queries from the passage v2 dev set, embedded with the same model; synthetic `year`, `category`, `rating`, `topic`. Copied to every node that reads it and checked against SHA-256 sums |
| budget | 6.8 CPUs per engine (Kubernetes limits), what two 4-vCPU nodes have left after DOKS's own pods (cilium and do-node-agent take 412m of a node's 3,890m). Elasticsearch, OpenSearch and Solr: 2 data nodes of 3.4 CPUs and 26 GiB (heap 13 GB, Solr 8 GB). Strata: 2 shards of 2.9 CPUs and 24 GiB and a coordinator of 1 CPU and 4 GiB; each shard built on its node with 3.4 CPUs, 26 GiB and 4 threads, both at once. Vespa: 2 content nodes of 2.9 CPUs and 23 GiB, `vespa-0` 1 CPU and 6 GiB. One-node engines: 6.8 CPUs and 52 GiB |
| queries | 300 of the 1,000 queries, every kind at 1, 4 and 16 clients for 6 s each, three passes one after the other; writes and streams for Strata, Elasticsearch, OpenSearch, Solr and Vespa only |
| quality | recall@10 against an exact search over the same 1M vectors (computed on the base node). There is no nDCG: the TREC RAG judgments cover the 113M passages, not this slice |
| cost | $21.29 at list price, every node pool counted in whole hours (DigitalOcean bills by the hour), the base node and the reruns included |

## Jobs

Each job's engine nodes and client node, with each node's check (seconds
to hash 1 GB per vCPU; lower is faster), the image its client (and, for
Strata, its server) came from, and when it ran (UTC).

| job | engine nodes | client node | image | ran |
|---|---|---|---|---|
| strata | m-4vcpu-32gb x2: Platinum 8280 (9.9 s), Platinum 8280 (11.0 s) | Platinum 8168 (11.4 s) | `e4b7680` | 05:19-05:45 |
| strata-b | m-4vcpu-32gb x2: Platinum 8280 (10.3 s), Platinum 8168 (11.1 s) | Platinum 8168 (11.3 s) | `e4b7680` | 05:01-05:27 |
| es | m-4vcpu-32gb x2: Platinum 8168 (11.2 s), Platinum 8280 (10.8 s) | Platinum 8168 (11.4 s) | `7a0e023` | 03:37-04:09 |
| os | m-4vcpu-32gb x2: Platinum 8280 (9.9 s), Platinum 8280 (11.1 s) | Platinum 8168 (11.5 s) | `7a0e023` | 03:17-03:49 |
| solr | m-4vcpu-32gb x2: Platinum 8280 (10.3 s), Platinum 8168 (11.5 s) | Platinum 8168 (11.3 s) | `7a0e023` | 03:20-04:03 |
| vespa-r | m-4vcpu-32gb x2: Platinum 8168 (11.3 s), Platinum 8168 (11.3 s) | Platinum 8168 (11.4 s) | `e4b7680` | 04:41-05:28 |
| meili-r | m-8vcpu-64gb: Platinum 8168 (11.5 s) | Platinum 8168 (11.5 s) | `e4b7680` | 04:41-05:24 |
| typesense | m-8vcpu-64gb: Platinum 8280 (10.4 s) | Platinum 8168 (11.3 s) | `7a0e023` | 03:17-03:55 |
| manticore | m-8vcpu-64gb: Platinum 8358 @ 2.60GHz (10.4 s) | Platinum 8168 (11.7 s) | `7a0e023` | 03:17-03:57 |
| qdrant | m-8vcpu-64gb: Gold 6248 @ 2.50GHz (11.2 s) | Platinum 8280 (10.6 s) | `7a0e023` | 03:16-04:44 |
| weaviate | m-8vcpu-64gb: Platinum 8280 (10.2 s) | Platinum 8168 (11.3 s) | `7a0e023` | 03:37-04:30 |
| milvus-r | m-8vcpu-64gb: Platinum 8358 @ 2.60GHz (10.0 s) | Platinum 8168 (11.5 s) | `e4b7680` | 04:41-05:01 |
| paradedb-r | m-8vcpu-64gb: Platinum 8358 @ 2.60GHz (10.6 s) | Platinum 8168 (11.7 s) | `e4b7680` | 04:43-05:25 |
| postgres-r | m-8vcpu-64gb: Platinum 8280 (10.3 s) | Platinum 8168 (11.2 s) | `e4b7680` | 04:41-05:47 |
| redis | m-8vcpu-64gb: Platinum 8168 (11.4 s) | Platinum 8168 (11.3 s) | `7a0e023` | 02:38-03:43 |

The Platinum 8168 and 8280 run at 2.70 GHz.

The images are `ghcr.io/chloetech-ai/strata-server:<full sha>`:
`7a0e023f5d8d543179dd7221fdebc23e7e4c0bf0` (main before the runner) and
`e4b76809a7529518d38e33af6716ef09dcc7b435` (the runner's branch, #235).
Strata ran only on `e4b7680`, which is main after #230 (`optimize()`
compacts, a SQL parse cache) and #229, plus the runner. For the other
engines the image is only their client, `engine-compare`; its adapters
differ between the two images only for the engines that refused the data
(below).

Engine images are those of the Mac run:

| engine | image |
|---|---|
| Elasticsearch | `elasticsearch:9.5.5` (Basic licence; its hybrid retrievers ran on the 30-day trial as their own row) |
| OpenSearch | `opensearchproject/opensearch:3.9.0` |
| Solr | `solr:10.1.0` |
| Vespa | `vespaengine/vespa:8.778.45` |
| Meilisearch | `getmeili/meilisearch:v1.56.0` |
| Typesense | `typesense/typesense:30.2` |
| Manticore | `manticoresearch/manticore:29.9.0` |
| Qdrant | `qdrant/qdrant:v1.20.0` |
| Weaviate | `semitechnologies/weaviate:1.39.14` |
| Milvus | `milvusdb/milvus:v2.6.26` (standalone) |
| ParadeDB | `paradedb/paradedb:0.26.4-pg17` |
| Postgres | the same image, without `pg_search` |
| Redis | `redis:8.8.4` |
| stream broker | `redpandadata/redpanda:v25.2.9` |

## How far apart two runs of the same engine land

`strata-b` is Strata again, the same image and steps, on nodes of its own.
Its numbers against `strata`'s, kind by kind at 1, 4 and 16 clients
(first passes): p50 latency differs by 12% at the median (33% at most), and
queries a second by 12% (45% at most), `strata-b` faster nearly everywhere
(BM25 p50 2.48 against 2.90 ms at 1 client; vectors 2.02 against 2.23 ms).
The three passes of one job sit much closer: BM25 at 4 clients 866 to 876
q/s, vectors 1,003 to 1,072 ([passes.md](passes.md)). So most of the spread
is the machines, not the run, and the node check does not predict it (the
slower Strata had the faster-checking nodes). Differences between engines of
less than about 15% here are within what a second set of machines changes.

## The first pass is not the engines' speed

The JVM engines were slowest on their first pass: Elasticsearch's BM25 at
4 clients ran at 571 q/s in it and 1,063 and 1,074 in the next two,
OpenSearch's at 677 then 1,168 and 1,185, Solr's at 268 then 1,087 and 1,136, while
Strata's three passes were 866 to 876 ([passes.md](passes.md)). `tables.md`
uses the first pass, so it understates them; [rank.md](rank.md) ranks on
the median of the three (Strata first with 226 points, Elasticsearch 269,
OpenSearch 281). Since then each kind warms up until its median holds
steady before it is timed (`warm_up_p50_ms` in a run's report).

## What went wrong on the way

- **Engines that refused MS MARCO's text.** Five loads stopped on the data
  with client `7a0e023`, and their retries were stopped by hand (they would
  have failed the same way): Meilisearch refuses ids with `.` and `#`
  (MS MARCO's ids have both); Vespa refuses control characters (158 of the
  1M passages have one); Postgres and ParadeDB refuse NUL (6 passages);
  Milvus held titles to 8 KB (30 titles are longer, up to 45 KB). Client
  `e4b7680` sends Meilisearch reversibly encoded ids and decodes its answers,
  sends Vespa, Postgres and ParadeDB spaces in place of what they refuse, and
  gives Milvus's titles 64 KB. Those five ran again as `meili-r`, `vespa-r`,
  `postgres-r`, `paradedb-r` and `milvus-r`; their first tries are in
  `jobs/<engine>.attempt-ids` (and `jobs/milvus.attempt1`,
  `jobs/vespa.attempt1`, the automatic retries).
- **Strata's two shard builds raced for a secret.** With both shards built
  at once, each created the `strata-token` secret if it was missing, and the
  one that lost failed the step while the other kept building. Fixed in
  `compare.sh` (#235): the secret is made once before both start.
  `strata` failed twice on it (`jobs/strata.attempt2` and
  `strata.attempt3`; the second because the running driver works from a
  copy of its scripts, which got the fix only after that try had started)
  and `strata-b` once (`jobs/strata-b.attempt1`); the runs here are the
  attempts after the fix.
- **`tables.md` and `summary.json` add up Strata's two shard builds**
  (339 s), although the two shards were built at the same time, each on its
  own node: Strata's load took 178 s (`build-strata-s1.json`, the longer of
  the two), and the build step in `jobs/strata/steps.tsv` 192 s with its
  pod's start. The generated files keep the sum, since the analysis cannot
  be run again without the collection; `analyze.py` now takes a sharded
  build's time from its first start to its last end. The same holds for
  `strata-b/`.
- **Qdrant took 75 minutes to load**: it took the 1M documents in 6.5 minutes
  and spent the rest building its index (`load-qdrant.json`).
- **The proxy between the cloud container and the cluster cut long
  connections** (followed logs, `kubectl exec` streams). `compare.sh`
  follows a job's log again from where it stopped, so no step failed on it;
  the analysis on the base node had to be run again.

## Files

- `tables.md`: every table, generated by `analyze.py` on the base node from
  the first pass of each job: load and size, latency and throughput at 1, 4
  and 16 clients, recall, SQL aggregates, agreement with Strata, writes,
  freshness, streams and CPU.
- `latency-full.md`, `summary.json`: the same numbers, every percentile.
- `passes.md`: the three passes of every engine, kind and concurrency
  (median, lowest and highest), from `passes.py`.
- `rank.md`: the engines ranked by the sum of their places over 47 tests,
  each on the median of the three passes, from `rank.py`.
- `run-*.json.gz`, `load-*.json`, `disk-*`, `build-strata-s*.json`,
  `writes-*.json`, `stream-*.json`, `cpu.jsonl`: the first pass's reports;
  `run-*-pass2.json.gz` and `run-*-pass3.json.gz` the other two passes'.
- `strata-b/`: the second Strata's reports and tables.
- `jobs/<job>/`: each job's nodes and their check (`nodes.jsonl`), its steps
  with their start and end (`steps.tsv`), its log, and on failure the pods'
  descriptions.

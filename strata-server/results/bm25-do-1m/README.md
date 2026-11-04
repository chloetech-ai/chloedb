# BM25 through a coordinator on DigitalOcean, before and after #245

Measured 2026-11-04 on DigitalOcean (sfo2), with the 1M comparison's setup
(`../compare-msmarco-1m-do/`, `deploy/compare/fanout.sh`): MS MARCO's first
million passages in two shards, each on an `m-4vcpu-32gb` node with 2.9
CPUs and 24 GiB, and a coordinator with 1 CPU on a `c-8` node, beside
engine-compare's client. Shards built offline by `compare.sh build`;
300 queries per kind; 1, 4 and 16 clients; all with result caches off.

The images are `ghcr.io/chloetech-ai/strata-server:<full sha>`:

- before: `bf640522bf3207225b134297ebc82b50267eafa2`, main just before #245;
- after: `055f6bf72d82812b98b30df5088dfd7295fecc0e`, #245's merge: the
  coordinator keeps BM25 term statistics per shard version (protocol 12),
  and a gRPC call's future is boxed once (`../bm25-stats-cache-container/`).

## Same nodes (`same-nodes/`)

Both images on the same three nodes, one after the other, three passes
each in the order before, after; after, before; before, after (the
`strata-ab` job of `fanout.sh`, `KINDS="bm25,bm25 + filter,phrase,prefix"`).
The shard files were built once, by the after image, and each image's
servers started on them in turn (no writes in between). Nodes:

| role | node | CPU | check (s to hash 1 GB per vCPU) |
|---|---|---|---|
| shard 0 | `m-4vcpu-32gb` | Platinum 8280 @ 2.70GHz | 10.3 |
| shard 1 | `m-4vcpu-32gb` | Platinum 8358 @ 2.60GHz | 10.5 |
| coordinator and client | `c-8` | Platinum 8168 @ 2.70GHz | 11.3 |

The 1M comparison's Strata job had its coordinator on a Platinum 8168 too,
and two 8280s for its shards.

Medians of 3 passes (lowest to highest in `same-nodes/passes.md`;
`python3 table.py same-nodes/passes.md strata-b same-nodes/passes.md strata-a`):

| kind | clients | q/s before | q/s after | change | p50 ms before | p50 ms after |
|---|---|---|---|---|---|---|
| bm25 | 1 | 393 | 565 | +44% | 2.35 | 1.57 |
| bm25 | 4 | 956 | 1,423 | +49% | 3.94 | 2.54 |
| bm25 | 16 | 1,375 | 1,787 | +30% | 11.38 | 8.53 |
| bm25 + filter | 1 | 313 | 438 | +40% | 2.95 | 2.03 |
| bm25 + filter | 4 | 788 | 1,107 | +40% | 4.78 | 3.22 |
| bm25 + filter | 16 | 1,202 | 1,620 | +35% | 12.72 | 9.05 |
| phrase | 1 | 410 | 621 | +51% | 2.28 | 1.44 |
| phrase | 4 | 1,026 | 1,510 | +47% | 3.74 | 2.46 |
| phrase | 16 | 1,387 | 1,886 | +36% | 11.26 | 8.26 |
| prefix | 1 | 280 | 355 | +27% | 3.35 | 2.60 |
| prefix | 4 | 704 | 948 | +35% | 5.36 | 3.89 |
| prefix | 16 | 1,054 | 1,298 | +23% | 14.44 | 10.69 |

In every row the three passes of one image are clear of the other's. Every
p99 fell but prefix's at 16 clients, which rose from 29.18 ms to 33.63
(`passes.md`). All 1,059 answers of the six passes have the same keys and
scores. Over a pass (all four kinds, every concurrency, warm-up included,
`cpu.jsonl`), shard 0 used 1.14 to 1.22 CPUs, shard 1 0.98 to 1.04 and the
coordinator 0.59 to 0.63, much the same with both images: in the same time
and about the same CPU, the after image answers 23 to 51% more queries.

## Separate nodes (`separate-nodes/`)

Before the same-node job, three ordinary Strata jobs ran at once, each on
nodes of its own, with every kind (`passes.md` in each):

| job | image | shards | coordinator and client |
|---|---|---|---|
| `old` | before | Platinum 8280, Platinum 8280 | Platinum 8280 |
| `new` | after | Platinum 8358, Platinum 8280 | Platinum 8358 |
| `new-b` | after | Platinum 8280, Platinum 8168 (both replaced once as slow) | Platinum 8168 |

BM25 at 16 clients: `old` 1,519 q/s, `new` 2,228 and `new-b` 1,599. The
two jobs of the same image differ by 39% there, and by 43 to 58% at 16
clients in the kinds that use no BM25 statistics (vector, vector + filter,
facet, key lookup, similar items), so the nodes weigh as much as the change
here, and these three jobs do not measure it: that is what the same-node
job is for. In the four BM25 kinds, `old` against `new` gives +26 to +60%,
`old` against `new-b` -4 to +23%. Every answer of the kinds that read no
vectors is the same in all three; in the five that do (1,500 answers), the
jobs differ in 10 to 22, as each built its own shards (the two jobs of the
after image in 10).

Against the 1M comparison's Elasticsearch (`../compare-msmarco-1m-do/passes.md`,
other nodes: 8168 and 8280 data nodes, an 8168 client), 1,063 q/s at 4
clients and 1,874 at 16: these runs do not rank the two, which would need
both engines on the same nodes.

## Files

- `same-nodes/`, `separate-nodes/{old,new,new-b}/`: engine-compare's
  reports (`run-*.json.gz`, every query's answer and timing), the builds
  (`build-strata-s*.json`), CPU per pod around each step (`cpu.jsonl`),
  `passes.md`, and the separate jobs' writes and streams; `job/` holds
  each job's image, nodes and their checks (`nodes.jsonl`), steps and log.
- `table.py`: the before and after tables above, from two `passes.md`.

Cost: $3.29 at list price for the four jobs and the base node (whole
hours), on a cluster of its own, deleted after.

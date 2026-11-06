# Strata on main, 6 November 2026, at 1M and 10M

The engine comparison run again for the engines whose code or settings
changed since their last run, on DigitalOcean (sfo2) with `fanout.sh`:

| engine | image or version | why it ran again |
|---|---|---|
| Strata | `ghcr.io/chloetech-ai/strata-server:b022e39d6f73295afb2e4bbde8c9d99da262059b` (main on 6 November) | everything merged after 18fb4bc, the image of [5 November's run](../bigger-items-do/README.md): #263 (filtered vector search re-ranks at full depth), #278 (a vector graph per value of a column; the harness now builds `emb` partitioned by `rating`, 7d081da), #284 (MEDIAN and percentiles), #285 (the filter cache keeps a table's own keys out), #287 (a coordinator in each shard's pod, the harness's default since 54c2892) |
| Manticore, 1M only | 29.9.0 | #262 turned its query cache off (`qcache_max_bytes = 0`); it last ran with it on, on 4 November. At 10M it did not finish loading in 8 hours, which the cache cannot change |

Not run again, because nothing of theirs changed since their newest run:
Elasticsearch and OpenSearch (5 November, result caches off since #262,
[`bigger-items-do`](../bigger-items-do/README.md)), and the other engines
(4 November at 1M, [`compare-msmarco-same-nodes-do`](../compare-msmarco-same-nodes-do/README.md);
3 November at 10M, [`compare-msmarco-10m-do`](../compare-msmarco-10m-do/README.md)).

Every engine had the budget of the earlier runs: 6.8 CPUs and 52 GiB, data
and queries checked against `data/SHA256SUMS` (the same files as on 4 and 5
November). Strata's budget is now two pods of 3.4 CPUs and 26 GiB, each a
shard and a coordinator (`STRATA_COORD=shards`, #287); until 5 November it
was two shards of 2.9 CPUs and a 1-CPU coordinator on the client's node
(`STRATA_COORD=pod`).

## The node check

DigitalOcean hands out several Xeon models, and the same image moves on
other nodes (up to 39% on BM25 at 16 clients, [the BM25 study](../bm25-do-1m)).
So each job first ran the image of 5 November (18fb4bc, `STRATA_COORD=pod`,
as it ran then; the `strata-was` engine of `fanout.sh`, 98ba19c) for three
passes, then Strata on main for three passes, writes and the stream, on the
same nodes. `check/` holds the first, renamed `strata` so the tools read it.

| | nodes (shards; client) |
|---|---|
| 1M, `jobs/rerun-1m` | two m-4vcpu-32gb, Xeon Platinum 8168; client c-8, 8168 |
| 10M, `jobs/rerun-10m` | see below |

## 1M

`1m/`: Strata on main (`run-strata*.json.gz`, writes, stream, builds),
Manticore (`run-manticore*`, load), `check/`, `jobs/`. `tables.md` and
`summary.json` are `analyze.py`'s, `passes.md` the passes side by side.
Every engine from its newest run, ranked: [`1m-ranked.md`](1m-ranked.md)
(Strata first with 137 points, Solr 288, Elasticsearch 292).

Medians of three passes, ms at one client and queries a second at 4 and 16:

| kind | p50 main | p50 check | p50 5 Nov | q/s 4 main | q/s 4 check | q/s 16 main | q/s 16 check | q/s 16 5 Nov |
|---|---|---|---|---|---|---|---|---|
| bm25 | 2.03 | 1.64 | 1.66 | 1,194 | 1,273 | 1,982 | 1,888 | 2,023 |
| bm25 + filter | 2.37 | 1.91 | 1.84 | 1,039 | 1,134 | 1,728 | 1,571 | 1,808 |
| phrase | 1.66 | 1.34 | 1.34 | 1,818 | 1,605 | 3,551 | 1,862 | 2,188 |
| prefix | 2.25 | 1.83 | 1.79 | 1,114 | 1,202 | 1,922 | 1,760 | 1,661 |
| typo | 13.15 | 12.69 | 11.17 | 200 | 210 | 252 | 221 | 216 |
| boosted (bm25 × decay) | 4.18 | 3.62 | 3.28 | 558 | 634 | 820 | 830 | 845 |
| facet | 1.92 | 1.53 | 1.48 | 1,556 | 1,404 | 2,959 | 1,646 | 1,954 |
| key lookup | 0.89 | 0.70 | 0.77 | 3,904 | 3,992 | 9,437 | 5,509 | 6,304 |
| sql aggregate | 8.29 | 7.60 | 7.42 | 295 | 306 | 347 | 296 | 282 |
| vector | 2.28 | 1.82 | 1.74 | 1,177 | 1,210 | 2,029 | 1,456 | 1,666 |
| vector + filter | 2.35 | 2.13 | 2.04 | 1,145 | 1,117 | 1,954 | 1,289 | 1,557 |
| similar items | 2.90 | 2.25 | 2.15 | 905 | 1,012 | 1,501 | 1,121 | 1,396 |
| hybrid (RRF) | 5.50 | 4.54 | 4.29 | 454 | 466 | 648 | 596 | 692 |
| hybrid (weighted) | 5.55 | 4.55 | 4.25 | 448 | 479 | 651 | 613 | 673 |

- **The layout trades one client's latency for throughput under load.**
  Main is 4 to 29% slower than the check at the median on one client
  (median of the kinds 24%): the client now reaches a coordinator across the
  network, and that coordinator shares its pod's CPUs with a shard. At 4
  clients it is 12% slower to 13% faster; at 16 clients it serves up to 1.9
  times as many queries (phrase 1,862 to 3,551, key lookups 5,509 to 9,437,
  facets 1,646 to 2,959), since both coordinators work and no CPU sits idle
  in a coordinator pod. Pods averaged 1.86 to 1.94 cores of their 3.4,
  throttled in 5.5 to 11.6% of periods; the check's shards 1.2 cores and its
  coordinator 0.65 (`cpu.jsonl`).
- **Vector + filter recall@10** 0.861, against 0.83 (#263, #278).
- **The nodes.** The check against 5 November's own run of the same image:
  0.91 to 1.14 at the median, 0.80 to 1.06 at 16 clients, slower on most
  kinds. So main's numbers here are, if anything, a little low against the
  engines that ran on 4 and 5 November.
- **Writes, from one client, are slower than on 4 November** (287 inserts a
  second against 417, batches of 100 1,301 against 2,127, updates 285
  against 547; a write searchable in 7.6 ms at the median, against 5.4). At
  16 clients they are level or faster (inserts 2,186 against 2,153, batches
  4,947 against 3,955) except updates (4,509 against 5,495). The check ran
  queries only, so other nodes, the coordinator across the network and the
  partitioned vectors are not separated.
- **The build** took 533 s a shard against 261 (merges 180 s against 29):
  the shards now build a vector graph per rating (7d081da). Shown, not
  ranked, as before.
- **Manticore** with its query cache off: 0.88 to 1.16 of its 4 November
  median time, 0.89 to 1.18 of its queries a second at 4 clients, on another
  node (8280, as then); 8th with 364 points, against 9th with 370.

## 10M

Still running when this was written: the check's three passes are done,
and Strata on main's shards are building. This section comes with the
results.

## Cost

Added when the cluster `efc0d554` (sfo2) is deleted.

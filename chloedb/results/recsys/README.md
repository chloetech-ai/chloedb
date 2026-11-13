# Recommender systems: Yambda-50M next listens, and serving r1-r6 on eight engines

Two tracks, both run free in one cloud container (Intel Xeon @ 2.10GHz,
4 cores, 15 GB, Linux 6.18), 2026-11-06. Scores do not depend on the
machine; times do, and the serving times are of one container where each
engine had 3 cores and 6 GB in Docker while a client ran beside it, so they
compare engines with each other, not with a production cluster.

- **Quality** ([`yambda.py`](yambda.py)): which tracks a user listens to on
  the next day of Yandex Music, from Strata's recommendation sources and
  from the baselines of the dataset's paper.
- **Serving** (`engine-compare recsys-load` and `recsys-run`,
  [`strata-server/src/bin/engine-compare/recsys.rs`](../../../strata-server/src/bin/engine-compare/recsys.rs)):
  the six requests of a two-tower recommender's serving path, against exact
  answers, on Strata, Elasticsearch, OpenSearch, Vespa, Qdrant, Milvus,
  Redis and Postgres (pgvector), over 100,000 tracks made by
  [`serving_data.py`](serving_data.py) from the quality track's model.

## Quality: next-day listens on Yambda-50M

[`yambda.py`](yambda.py) follows the dataset paper's protocol (its
docstring has the details): Listen+ events (at least half the track
played), train on everything up to 30 minutes before the last day, test on
that day for the users seen in training, tune on the day before. Here that
is 29,280,230 training listens of 629,298 tracks, and
157,695 targets of 4,599 test users. Every source
answers 100 tracks for each test user (filled with DecayPop's when it has
fewer). Recall@k is the paper's (hits over all of a user's targets);
`recall@100` divides by min(targets, 100) instead. The baselines are written
here from the paper's descriptions, and their hours were chosen on the day
before over the paper's grids.

The sequence model (`strata.train.sequence`: two layers of causal attention,
width 64, item vectors learned with it, in-batch negatives, temperature
0.05, batch 64, each user's last 1,000 listens) started from an SVD of the
user-track play counts; from random vectors it stayed at chance after an
epoch. It ran 5 epochs in its 80 minutes on the CPU, its held-out recall@10
still rising (0.006, 0.012, 0.018, 0.023, 0.026: [`quality/train.json`](quality/train.json)),
so its rows are a floor.

| source | NDCG@10 | NDCG@100 | Recall@10 | Recall@100 | recall@100 (of min(n, 100)) | HR@10 | Coverage@100 | query p50 / p99, ms |
|---|---|---|---|---|---|---|---|---|
| MostPop | 0.0174 | 0.0231 | 0.0064 | 0.0321 | 0.0327 | 0.093 | 0.0002 | – |
| DecayPop (2 h) | 0.0256 | 0.0345 | 0.0122 | 0.0479 | 0.0486 | 0.144 | 0.0002 | – |
| ItemKNN (2 h) | 0.1152 | 0.1279 | 0.0486 | 0.1360 | 0.1385 | 0.380 | 0.1719 | – |
| history (720 h): a user's own tracks by plays | 0.1854 | 0.2297 | 0.0915 | 0.2664 | 0.2702 | 0.452 | 0.1681 | – |
| Strata `pop` (DecayPop as a column) | 0.0256 | 0.0345 | 0.0122 | 0.0479 | 0.0486 | 0.144 | 0.0002 | 0.7 / 2.2 |
| Strata `history` (`GROUP BY` over their events) | 0.1854 | 0.2297 | 0.0915 | 0.2663 | 0.2702 | 0.451 | 0.1681 | 3.5 / 12.1 |
| Strata `encode` (the sequence model) | 0.0799 | 0.0978 | 0.0378 | 0.1173 | 0.1189 | 0.308 | 0.1766 | 4.1 / 53.3 |
| Strata `next_items` (counted transitions) | 0.0658 | 0.0693 | 0.0316 | 0.0726 | 0.0735 | 0.258 | 0.1203 | 2.6 / 32.1 |
| Strata `ppr` (PageRank over what came next) | 0.1188 | 0.1122 | 0.0608 | 0.1063 | 0.1074 | 0.341 | 0.2014 | 433.9 / 617.6 |
| Strata `fused`: `rrf(history, encode, ppr)` | 0.1655 | 0.1993 | 0.0834 | 0.2325 | 0.2351 | 0.447 | 0.1868 | 437.0 / 617.2 |

Published on the same split:

| model (the paper's Table 6, Yambda-50M Listen+) | NDCG@10 | NDCG@100 | Recall@10 | Recall@100 | Coverage@100 |
|---|---|---|---|---|---|
| MostPop | 0.0186 | 0.0249 | 0.0064 | 0.0321 | 0.0002 |
| DecayPop | 0.0260 | 0.0323 | 0.0122 | 0.0479 | 0.0002 |
| ItemKNN | 0.0781 | 0.0934 | 0.0373 | 0.1297 | 0.0874 |
| iALS | 0.0407 | 0.0642 | 0.0128 | 0.0808 | 0.0156 |
| BPR | 0.0389 | 0.0641 | 0.0139 | 0.0836 | 0.0999 |
| SASRec | 0.0748 | 0.0764 | 0.0325 | 0.1026 | 0.0310 |

[`quality/baselines.json`](quality/baselines.json) has every validation-day
run of the grids, and [`quality/strata.json`](quality/strata.json) Strata's
sources. Query times are of the embedded database (strata-py) on the test
day, one query at a time.

### What it says

- **The protocol reproduces the paper.** MostPop's and DecayPop's Recall@10
  and Recall@100 equal Table 6's to four digits (0.0064 and 0.0321, 0.0122
  and 0.0479). NDCG is within 0.0004 to 0.0022 of it: the paper's ideal
  ranking may count repeated targets differently. ItemKNN is above the
  paper's (Recall@100 0.1360 against 0.1297, NDCG@10 0.1152 against 0.0781).
  Its best decay sat at the grid's edge (2 hours), so a longer grid may do
  better still.
- **Strata's `encode` is above published SASRec on every column**, after 80
  minutes on 4 CPU cores and still improving: NDCG@10 0.0799 against 0.0748,
  Recall@10 0.0378 against 0.0325, Recall@100 0.1173 against 0.1026,
  Coverage@100 0.1766 against 0.0310. It is still below this ItemKNN
  (0.1152, 0.0486, 0.1360).
- **Re-listening decides this task.** A user's own most played tracks
  (`history`, one `GROUP BY` at 3.5 ms) has twice the Recall@100 of
  anything in Table 6 (0.2663). 68% of a test day's listens are of tracks
  the user played before, which the paper's protocol keeps as targets. A
  next-day benchmark that leaves `history` out understates what a
  recommender gets right.
- **`ppr` is the best source after `history` at the top** (NDCG@10 0.1188,
  above ItemKNN's 0.1152) **but takes 434 ms** at p50 and 618 ms at p99,
  too slow to serve: the next step is item neighbours computed ahead and
  served from a table.
- **Fusing by plain RRF loses to `history` alone** (NDCG@10 0.1655 against
  0.1854, Recall@100 0.2325 against 0.2663; HR@100 0.756 against 0.728):
  equal weights let two weaker lists push repeats down. Weights or a
  learned blend, chosen on the validation day, are the next step there
  ([measured below](#the-corrected-model-on-the-next-day-and-fusion-weights)).

### In-batch negatives, corrected

The sequence model above trains against the other targets of its batch,
which teaches it to rank popular tracks low and never shows it most of the
catalog as a negative. `yambda.py train --logq --uniform-negatives 4096`
takes from each negative's score the log of the chance that the batch
holds it, and adds 4,096 tracks drawn evenly to every batch
(`strata.train.sequence(logq=True, uniform_negatives=4096)`). Same split,
histories, model and 80 minutes; Recall@10 on each user's held-out last
listen after each epoch, measured on a cloud container (Intel Xeon @
2.30GHz, 4 cores, 16.5 GB), 2026-11-12, with the trainer loaded on its own
(the train stage's code, without the database package):

| training | epoch 1 | 2 | 3 | 4 | 5 |
|---|---|---|---|---|---|
| in-batch, from the SVD ([`quality/train.json`](quality/train.json), 2026-11-06) | 0.0057 | 0.0119 | 0.0181 | 0.0229 | 0.0258 |
| corrected, 4,096 uniform, from the SVD ([`quality/train-logq.json`](quality/train-logq.json)) | 0.0110 | 0.0205 | 0.0274 | **0.0334** | not reached |
| the same, run to five epochs ([`quality/train-logq-5.json`](quality/train-logq-5.json)) | 0.0110 | 0.0205 | 0.0273 | 0.0336 | **0.0373** |
| corrected, 4,096 uniform, from random vectors ([`quality/train-logq-random.json`](quality/train-logq-random.json)) | 0.0039 | 0.0026 | 0.0047 | 0.0076 | 0.0099 |

- **Epoch for epoch the corrected model is ahead by about half** (0.0334
  against 0.0229 at epoch 4, NDCG@10 0.0182 against 0.0124), and it was
  still rising when the 80 minutes ended. It reached 4 epochs in them,
  with other work on the machine, and its fourth is ahead of the plain
  run's fifth (0.0258).
- **From random vectors it now learns** (0.0099 after five epochs, rising),
  where the plain run stayed at chance after an epoch. The SVD start is
  still worth three epochs.
- **Five epochs take it to 0.0373**, against the plain run's 0.0258,
  and it was still rising. That run was paused and shared the machine,
  so its time is not a measure; its last epoch, alone on the 4 cores,
  took 17 minutes.

### The corrected model on the next day, and fusion weights

The five-epoch corrected model went through `load` and `eval`, and
`yambda.py fuse` then chose weights for fusing `history`, `encode` and
`ppr`. It reads each source's top 100 and scores once, tries every share
of 1 among the three in steps of 0.1, and splits the test users in two:
each half is scored with the weights that had the best NDCG@10 on the
other half. The fusion is a weighted sum of normalised scores, which the
query language has
(`0.8 * tmm(ranked(:hist), lo => -101) + 0.1 * tmm(similarity(emb, encode(..))) + 0.1 * tmm(ppr(..))`),
and its row below is that query run for every user. Same container,
2026-11-12 ([`quality/strata-logq.json`](quality/strata-logq.json),
[`quality/fuse.json`](quality/fuse.json)):

| source | NDCG@10 | NDCG@100 | Recall@10 | Recall@100 | HR@10 | HR@100 | Coverage@100 | query p50 / p99, ms |
|---|---|---|---|---|---|---|---|---|
| `history` | 0.1854 | 0.2297 | 0.0915 | 0.2663 | 0.451 | 0.728 | 0.1681 | 3.1 / 9.6 |
| `encode`, the plain model (the table above) | 0.0799 | 0.0978 | 0.0378 | 0.1173 | 0.308 | 0.639 | 0.1766 | 4.1 / 53.3 |
| `encode`, the corrected model | 0.0970 | 0.1250 | 0.0444 | 0.1567 | 0.377 | 0.748 | 0.0596 | 3.3 / 50.7 |
| `ppr` | 0.1199 | 0.1167 | 0.0610 | 0.1139 | 0.343 | 0.565 | 0.1876 | 584.0 / 792.9 |
| `fused`: `rrf(history, encode, ppr)`, equal weights | 0.1672 | 0.2064 | 0.0843 | 0.2460 | 0.459 | **0.793** | 0.1436 | 585.1 / 802.6 |
| weighted sum, the query | 0.1894 | 0.2344 | 0.0947 | **0.2722** | 0.460 | 0.740 | 0.1738 | 583.8 / 826.0 |
| weighted reciprocal ranks, worked out in Python (`rrf` takes no weights) | **0.1912** | **0.2352** | **0.0958** | 0.2719 | **0.465** | 0.740 | 0.1729 | – |

- **The corrected model passes ItemKNN at 100**: Recall@100 0.1567
  against ItemKNN's 0.1360 and the plain model's 0.1173. At 10 it is
  still below ItemKNN (NDCG@10 0.0970 against 0.1152).
- **It answers with far fewer tracks**: Coverage@100 0.0596 against the
  plain model's 0.1766. The plain model ranked popular tracks low, which
  spread its answers; corrected, it no longer does.
- **Weights turn the fusion from a loss into a small gain.** With equal
  weights `rrf` is below `history` alone (NDCG@10 0.1672 against
  0.1854). The weighted sum is above it: NDCG@10 0.1894, Recall@100
  0.2722 against 0.2663. Both halves gave `history` most of the weight
  (0.8, with 0.1 each for `encode` and `ppr`; and 0.9, with 0.1 for
  `ppr`), so on this task the other sources add about 2% to `history`.
- **Equal weights find something for more users** (HR@100 0.793 against
  0.740) and rank it lower: they give a user with little history two
  sources that do not need one.
- **Weights on reciprocal ranks do no better than weights on scores**
  (NDCG@10 0.1912 and 0.1890 from the same lists in Python), so the
  weighted sum the query language has is enough here, and `rrf` needs
  no weights for it.
- The weights were chosen on other users of the test day, not on the
  day before, and in steps of 0.1. The weighted sum's times were taken
  with a training job on the machine. `ppr`'s row differs from the
  table above's (0.1199 against 0.1188, and 584 ms against 434): another
  build of the database on another machine.

## Serving: r1 to r6 on eight engines

[`serving_data.py`](serving_data.py) took the 100,000 most played tracks of
the training listens with the quality track's learned vectors (64 numbers),
their first artist, a DecayPop score, twelve ranking features and the regions
they may be played in (5 regions, each track in each with probability 0.8,
standing for licensing), and 300 users with their vector at the start of the
test day, their region and the up to 500 tracks they played. It trained a
LightGBM ranker (300 trees) over the twelve features and the user-track dot
product. Each kind ([the requests](../../../strata-server/deploy/compare/README.md#recommender-kinds))
was sent once for each user and scored against an exact answer the
client computed from the same data, then timed closed-loop for 6 s at 1, 4
and 16 clients after a 2 s warm-up:

- **r1** the 500 nearest tracks in the user's region, minus those they played;
- **r2** three sources under r1's filters, 1,000 deep (nearest the user,
  nearest their last track, most popular), fused by RRF (k 60), the best 1,000;
- **r3** the twelve features of 1,000 tracks by key;
- **r4** those 1,000 ranked by the GBDT, the best 50;
- **r5** r1's ranking with at most 2 tracks an artist, the best 20;
- **r6** r1 while plays are written at 200 a second, and how soon a play
  shows in a query of the user's last five (every 50th play is polled).

One engine ran at a time in Docker with 3 CPUs and 6 GB (Strata's server ran
outside Docker on 3 of the 4 cores), with the client on the same machine:
Elasticsearch 9.5.5 (trial licence, for its `rrf` retriever), OpenSearch
3.9.0, Vespa 8.778.45, Qdrant 1.20.0, Milvus 2.6.26 standalone, Redis 8.8.4,
ParadeDB 0.26.4 (Postgres 17.11 with pgvector), and Strata at this branch.
Every engine searched an HNSW graph (m 16, ef_construction 200, dot product)
of full-precision floats with ef 1,000 for r1 and twice the depth for r2 and
r5; Strata walks its graph over 8-bit codes with the same ef and re-scores
every candidate with the floats. Elasticsearch's, OpenSearch's and Strata's
items were merged into one segment before the runs. Where an engine has no
request for a kind's whole job the client does the rest (each report's
`differs` says what): r4 outside Strata and Vespa reads the features (r3's
request) and scores the trees in the client with Strata's own evaluator.
Every engine's r3 asks for the twelve features alone (Strata's
`/v1/kv/get` with `columns`).
The reports are in [`serving/`](serving/); [`serving_tables.py`](serving_tables.py)
writes these tables from them.

**r1 user to items** (recall@500 of the exact answer)

| engine | score | p50 / p99 at 1, ms | req/s at 4 | req/s at 16 | p99 at 16, ms |
|---|---|---|---|---|---|
| Strata | 0.979 | 5.0 / 7.3 | 587 | 712 | 42.4 |
| Elasticsearch | 0.912 | 11.9 / 19.9 | 244 | 281 | 94.2 |
| OpenSearch | 0.976 | 25.6 / 37.5 | 110 | 109 | 258 |
| Vespa | 0.985 | 9.5 / 15.4 | 301 | 383 | 78.0 |
| Qdrant | 0.984 | 4.6 / 7.1 | 490 | 610 | 49.9 |
| Milvus | 0.978 | 6.5 / 11.9 | 453 | 479 | 57.6 |
| Redis | 0.988 | 6.3 / 9.7 | 572 | 700 | 40.7 |
| Postgres | 0.980 | 10.8 / 17.1 | 310 | 283 | 140 |

**r2 candidates (RRF)** (recall@1000 of the exact answer)

| engine | score | p50 / p99 at 1, ms | req/s at 4 | req/s at 16 | p99 at 16, ms |
|---|---|---|---|---|---|
| Strata | 0.998 | 12.9 / 19.4 | 232 | 267 | 114 |
| Elasticsearch | 0.985 | 20.8 / 30.5 | 97 | 102 | 216 |
| OpenSearch | 0.997 | 115 / 135 | 23 | 23 | 1077 |
| Vespa | 0.998 | 17.6 / 25.6 | 88 | 99 | 265 |
| Qdrant | 0.998 | 12.9 / 18.1 | 203 | 217 | 104 |
| Milvus | 0.998 | 12.1 / 22.4 | 158 | 177 | 142 |
| Redis | 0.999 | 15.2 / 21.4 | 127 | 119 | 179 |
| Postgres | 0.993 | 42.6 / 60.9 | 73 | 65 | 386 |

**r3 features** (rows_found of the exact answer)

| engine | score | p50 / p99 at 1, ms | req/s at 4 | req/s at 16 | p99 at 16, ms |
|---|---|---|---|---|---|
| Strata | 1.000 | 7.0 / 9.9 | 385 | 568 | 50.3 |
| Elasticsearch | 1.000 | 54.6 / 80.9 | 56 | 59 | 437 |
| OpenSearch | 1.000 | 61.2 / 79.3 | 47 | 48 | 376 |
| Vespa | 1.000 | 16.4 / 21.7 | 165 | 191 | 143 |
| Qdrant | 1.000 | 13.2 / 19.2 | 243 | 292 | 95.5 |
| Milvus | 1.000 | 15.4 / 33.0 | 163 | 175 | 203 |
| Redis | 1.000 | 6.5 / 10.2 | 259 | 295 | 73.7 |
| Postgres | 1.000 | 7.2 / 11.0 | 389 | 425 | 83.5 |

**r4 GBDT rerank** (precision@50 of the exact answer)

| engine | score | p50 / p99 at 1, ms | req/s at 4 | req/s at 16 | p99 at 16, ms |
|---|---|---|---|---|---|
| Strata | 1.000 | 13.6 / 20.8 | 191 | 223 | 128 |
| Elasticsearch | 1.000 | 61.7 / 81.6 | 53 | 59 | 412 |
| OpenSearch | 1.000 | 69.0 / 96.1 | 45 | 45 | 443 |
| Vespa | 1.000 | 7.4 / 11.0 | 358 | 398 | 77.2 |
| Qdrant | 1.000 | 22.3 / 28.3 | 122 | 176 | 151 |
| Milvus | 1.000 | 24.2 / 40.9 | 119 | 140 | 210 |
| Redis | 1.000 | 14.7 / 19.5 | 177 | 235 | 118 |
| Postgres | 1.000 | 16.2 / 21.5 | 192 | 224 | 128 |

**r5 diversify** (recall@20 of the exact answer)

| engine | score | p50 / p99 at 1, ms | req/s at 4 | req/s at 16 | p99 at 16, ms |
|---|---|---|---|---|---|
| Strata | 0.984 | 5.8 / 8.7 | 534 | 661 | 49.5 |
| Elasticsearch | 0.955 | 8.6 / 14.7 | 154 | 153 | 148 |
| OpenSearch | 0.981 | 15.5 / 26.8 | 76 | 84 | 258 |
| Vespa | 0.988 | 5.6 / 8.6 | 483 | 590 | 55.4 |
| Qdrant | 0.996 | 18.4 / 25.0 | 124 | 134 | 149 |
| Milvus | 0.989 | 5.0 / 6.9 | 571 | 610 | 48.2 |
| Redis | 0.990 | 4.4 / 6.4 | 734 | 886 | 31.6 |
| Postgres | 0.983 | 5.3 / 7.7 | 604 | 550 | 74.8 |

**r6 events while serving** (recall@500 of the exact answer)

| engine | score | r1 p50 / p99 at 1, ms | r1 p99 at 16, ms | write p99 at 16, ms | play visible at 16, p50 / p99, ms |
|---|---|---|---|---|---|
| Strata | 0.979 | 5.3 / 7.7 | 49.9 | 48.2 | 21.2 / 41.4 |
| Elasticsearch | 0.912 | 10.5 / 18.4 | 93.9 | 34.6 | 672 / 1171 |
| OpenSearch | 0.976 | 22.5 / 32.4 | 223 | 38.3 | 728 / 1254 |
| Vespa | 0.985 | 8.6 / 13.3 | 86.0 | 39.2 | 21.6 / 39.4 |
| Qdrant | 0.984 | 4.6 / 7.3 | 45.9 | 37.8 | 24.6 / 39.9 |
| Milvus | 0.978 | 6.0 / 10.6 | 65.5 | 29.8 | 151 / 238 |
| Redis | 0.988 | 6.2 / 8.8 | 39.7 | 20.3 | 15.6 / 23.7 |
| Postgres | 0.980 | 10.3 / 15.0 | 130 | 52.6 | 1.1 / 21.4 |

### What it says

- **Strata's retrieval kinds are as fast as the fastest engine's, at a
  recall in the others' range.** r1 at 5.0 ms p50 (Qdrant 4.6, Redis 6.3,
  Milvus 6.5, Vespa 9.5) and the most requests a second at 16 clients (712;
  Redis 700, Qdrant 610), recall 0.979 (the others 0.976 to 0.988,
  Elasticsearch 0.912). r2 at 12.9 ms and the most a second at 16 (267;
  Qdrant 217), recall 0.998, where the others take 12.1 to 115 ms (Milvus,
  Redis and Vespa fuse in the client). r5 is level with Vespa, Postgres,
  Milvus and Redis (5.8 ms against 4.4 to 5.6), at recall 0.984 against
  their 0.983 to 0.990.
- **Reading features by key is level with Redis and Postgres, and serves
  the most at 16 clients**: 1,000 rows of twelve features in 7.0 ms p50
  (Redis's pipelined `HMGET` 6.5, Postgres's `WHERE id = ANY` 7.2, Qdrant
  13.2, Vespa 16.4) and 568 a second at 16 (Postgres 425, Redis 295).
  Before this change it was Strata's slowest kind, at 23.6 ms and 156 a
  second: a read by key decompressed a block of the doc store for every key,
  and read all sixteen columns where the others ask for the twelve.
- **Vespa ranks 1,000 candidates by the trees 1.8 times as fast** (r4,
  7.4 ms p50 and 398 a second at 16, against Strata's 13.6 ms and 223): its
  rank profile evaluates the model where the features are. Strata's walk of
  the trees is three times as fast as it was (r4 was 18.3 ms), and is about
  half of what r4 takes now; parsing the request's 1,000 keys is another
  seventh (profiled on a 20,000-track set). Redis and Postgres score in the client with the same evaluator
  (14.7 and 16.2 ms, from 16.3 and 18.6).
- **A play is visible in 21 ms at p50 and 41 ms at p99** with 16 clients
  reading, as on Vespa (22 / 39) and Qdrant (25 / 40), behind Redis (16 /
  24); on Postgres at once (1 ms); on Milvus at its Bounded consistency
  (0.15 s); on Elasticsearch and OpenSearch after their 1 s refresh
  (~0.7 s).
- **The search breadth decides Strata's recall here.** With its default
  (ef 64, raised to the limit) it had 0.961 at r1 and 0.947 at r5; the
  harness asks it for the others' ef.

### Before and after the read-by-key and tree fixes

The first run of these kinds (in the same container, 2026-11-06, before
`columns` and the new tree walk) had Strata at:

| kind | then: p50 at 1, ms | then: req/s at 16 | now: p50 at 1, ms | now: req/s at 16 |
|---|---|---|---|---|
| r3 features | 23.6 | 156 | 7.0 | 568 |
| r4 GBDT rerank | 18.3 | 172 | 13.6 | 223 |

and the other kinds within a few percent of where they are now. Profiles of
the r3 server then put half its CPU in decompressing doc store blocks and
11% in writing JSON; of r4, 73% in walking the trees, one row at a time, a
mispredicted branch at every other step.

## Running it

```bash
python3 -m venv ~/data/venv && . ~/data/venv/bin/activate
pip install polars pyarrow numpy scipy lightgbm
(cd strata-py && pip install maturin && maturin develop --release)
Y=https://huggingface.co/datasets/yandex/yambda/resolve/main
mkdir -p ~/data/yambda/flat/50m
for f in listens likes dislikes; do curl -fL -o ~/data/yambda/flat/50m/$f.parquet $Y/flat/50m/$f.parquet; done
curl -fL -o ~/data/yambda/artist_item_mapping.parquet $Y/artist_item_mapping.parquet
cd strata/results/recsys
python yambda.py prep  --data ~/data/yambda --work ~/data/yambda/work    # split and baselines (18 min here)
python yambda.py train --data ~/data/yambda --work ~/data/yambda/work --minutes 80
python yambda.py load  --data ~/data/yambda --work ~/data/yambda/work
python yambda.py eval  --data ~/data/yambda --work ~/data/yambda/work
python yambda.py fuse  --data ~/data/yambda --work ~/data/yambda/work    # fusion weights (90 min here: every source for every user, then the query)
python serving_data.py --data ~/data/yambda --work ~/data/yambda/work --out ~/data/recsys-100k --items 100000
cd ../../../strata-server && cargo build --profile ci --bins
DATA=~/data/recsys-100k OUT=~/data/recsys-out RUN_ARGS="--users 300" \
    deploy/compare/recsys/local.sh strata es os vespa qdrant milvus redis postgres
```

Pass `--data` and `--work` explicitly where `$HOME` is not the user's
(they default to `~/data/yambda`). `train` runs for hours: start it with
`setsid nohup … &` where a shell's background jobs are time-limited.

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

QUALITY_SECTION

## Serving: r1 to r6 on eight engines

[`serving_data.py`](serving_data.py) took the 100,000 most played tracks of
the training listens with the quality track's learned vectors (64 numbers),
their first artist, a DecayPop score, twelve ranking features and the regions
they may be played in (5 regions, each track in each with probability 0.8,
standing for licensing), and 300 users with their vector at the start of the
test day, their region and the up to 500 tracks they played. It trained a
LightGBM ranker (300 trees) over the twelve features and the user-track dot
product. Each kind (deploy/compare/README.md, "Recommender kinds", has the
requests) was sent once for each user and scored against an exact answer the
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
The reports are in [`serving/`](serving/); [`serving_tables.py`](serving_tables.py)
writes these tables from them.

**r1 user to items** (recall@500 of the exact answer)

| engine | score | p50 / p99 at 1, ms | req/s at 4 | req/s at 16 | p99 at 16, ms |
|---|---|---|---|---|---|
| Strata | 0.978 | 4.9 / 8.8 | 614 | 737 | 39.8 |
| Elasticsearch | 0.917 | 12.4 / 17.0 | 235 | 285 | 94.1 |
| OpenSearch | 0.975 | 23.4 / 36.9 | 121 | 122 | 210 |
| Vespa | 0.985 | 9.7 / 19.1 | 280 | 351 | 82.0 |
| Qdrant | 0.984 | 4.9 / 8.3 | 482 | 601 | 42.8 |
| Milvus | 0.978 | 6.8 / 12.9 | 374 | 392 | 73.3 |
| Redis | 0.988 | 7.2 / 12.5 | 496 | 622 | 47.6 |
| Postgres | 0.980 | 10.9 / 18.4 | 279 | 204 | 191 |

**r2 candidates (RRF)** (recall@1000 of the exact answer)

| engine | score | p50 / p99 at 1, ms | req/s at 4 | req/s at 16 | p99 at 16, ms |
|---|---|---|---|---|---|
| Strata | 0.998 | 12.7 / 19.0 | 235 | 260 | 111 |
| Elasticsearch | 0.985 | 20.9 / 31.7 | 99 | 103 | 197 |
| OpenSearch | 0.997 | 105 / 133 | 25 | 25 | 1022 |
| Vespa | 0.998 | 18.5 / 27.7 | 88 | 96 | 268 |
| Qdrant | 0.998 | 13.5 / 19.2 | 166 | 190 | 139 |
| Milvus | 0.998 | 13.6 / 26.8 | 145 | 164 | 149 |
| Redis | 0.999 | 21.2 / 31.2 | 109 | 109 | 202 |
| Postgres | 0.993 | 48.8 / 72.2 | 64 | 57 | 510 |

**r3 features** (rows_found of the exact answer)

| engine | score | p50 / p99 at 1, ms | req/s at 4 | req/s at 16 | p99 at 16, ms |
|---|---|---|---|---|---|
| Strata | 1.000 | 23.6 / 40.0 | 126 | 156 | 186 |
| Elasticsearch | 1.000 | 51.8 / 81.5 | 59 | 58 | 455 |
| OpenSearch | 1.000 | 56.1 / 80.3 | 51 | 48 | 410 |
| Vespa | 1.000 | 18.3 / 32.7 | 99 | 151 | 213 |
| Qdrant | 1.000 | 12.9 / 18.4 | 223 | 266 | 110 |
| Milvus | 1.000 | 16.8 / 41.5 | 143 | 167 | 193 |
| Redis | 1.000 | 6.4 / 9.8 | 254 | 271 | 78.7 |
| Postgres | 1.000 | 7.6 / 14.0 | 371 | 413 | 83.6 |

**r4 GBDT rerank** (precision@50 of the exact answer)

| engine | score | p50 / p99 at 1, ms | req/s at 4 | req/s at 16 | p99 at 16, ms |
|---|---|---|---|---|---|
| Strata | 1.000 | 18.3 / 24.7 | 155 | 172 | 164 |
| Elasticsearch | 1.000 | 62.1 / 78.6 | 52 | 57 | 450 |
| OpenSearch | 1.000 | 67.0 / 91.5 | 47 | 48 | 387 |
| Vespa | 1.000 | 7.7 / 13.1 | 330 | 374 | 84.7 |
| Qdrant | 1.000 | 23.1 / 31.0 | 112 | 164 | 166 |
| Milvus | 1.000 | 27.8 / 50.8 | 104 | 118 | 257 |
| Redis | 1.000 | 16.3 / 22.9 | 165 | 213 | 132 |
| Postgres | 1.000 | 18.6 / 25.4 | 170 | 207 | 132 |

**r5 diversify** (recall@20 of the exact answer)

| engine | score | p50 / p99 at 1, ms | req/s at 4 | req/s at 16 | p99 at 16, ms |
|---|---|---|---|---|---|
| Strata | 0.980 | 5.8 / 10.1 | 553 | 686 | 48.9 |
| Elasticsearch | 0.960 | 8.3 / 15.1 | 152 | 156 | 137 |
| OpenSearch | 0.980 | 15.6 / 28.0 | 80 | 84 | 233 |
| Vespa | 0.988 | 5.7 / 8.9 | 469 | 572 | 59.7 |
| Qdrant | 0.995 | 19.1 / 27.7 | 117 | 127 | 161 |
| Milvus | 0.989 | 5.7 / 10.2 | 495 | 564 | 52.1 |
| Redis | 0.990 | 4.8 / 7.6 | 645 | 769 | 35.6 |
| Postgres | 0.982 | 6.1 / 10.6 | 524 | 489 | 79.7 |

**r6 events while serving** (recall@500 of the exact answer)

| engine | score | r1 p50 / p99 at 1, ms | r1 p99 at 16, ms | write p99 at 16, ms | play visible at 16, p50 / p99, ms |
|---|---|---|---|---|---|
| Strata | 0.978 | 5.2 / 8.0 | 44.0 | 41.1 | 20.9 / 37.1 |
| Elasticsearch | 0.917 | 10.1 / 17.3 | 109 | 36.5 | 703 / 1092 |
| OpenSearch | 0.975 | 22.7 / 32.8 | 260 | 33.2 | 689 / 1201 |
| Vespa | 0.985 | 8.1 / 12.8 | 82.6 | 41.0 | 22.0 / 84.0 |
| Qdrant | 0.984 | 4.8 / 8.4 | 60.6 | 41.9 | 27.7 / 35.2 |
| Milvus | 0.978 | 7.3 / 12.5 | 92.4 | 48.0 | 201 / 307 |
| Redis | 0.988 | 7.0 / 10.4 | 41.7 | 17.2 | 23.1 / 32.8 |
| Postgres | 0.980 | 12.1 / 22.8 | 153 | 45.8 | 1.1 / 12.4 |

### What it says

- **Strata's retrieval kinds are as fast as the fastest engine's, at a
  recall in the others' range.** r1 at 4.9 ms p50 (Qdrant 4.9, Milvus 6.8,
  Redis 7.2, Vespa 9.7) and the most requests a second at 16 clients (737;
  Redis 622, Qdrant 601), recall 0.978 (the others 0.975 to 0.988,
  Elasticsearch 0.917). r2 at 12.7 ms and 260 a second, recall 0.998,
  against 13.5 to 105 ms for the others (Milvus, Redis and Vespa fuse in the
  client). r5 is level in time with Redis, Vespa, Milvus and Postgres (4.8
  to 6.1 ms), at recall 0.980 against their 0.982 to 0.990.
- **Reading features by key is where Strata is slowest**: 1,000 rows of
  twelve numbers take 23.6 ms by `/v1/kv/get`, three to four times Redis's
  pipelined `HMGET` (6.4 ms) and Postgres's `WHERE id = ANY` (7.6 ms), and
  twice Qdrant's (12.9 ms). Only Elasticsearch and OpenSearch are slower.
- **Vespa ranks 1,000 candidates by the trees 2.4 times as fast** (r4,
  7.7 ms p50 and 374 a second at 16 against Strata's 18.3 ms and 172): its
  rank profile evaluates the LightGBM model where the features are, and
  Strata's rerank phase over the same 1,000 costs what reading them and
  scoring in the client costs Redis and Postgres (16.3 and 18.6 ms).
- **A play is visible in 21 ms at p50 and 37 ms at p99** (24 polled plays)
  with 16 clients reading, as on Vespa, Qdrant and Redis; on Postgres at
  once (1 ms); on Elasticsearch and OpenSearch after their 1 s refresh
  (~0.7 s); on Milvus at its Bounded consistency (0.2 s).
- **The search breadth decides Strata's recall here.** With its default
  (ef 64, raised to the limit) it had 0.961 at r1 and 0.947 at r5; the
  harness now asks it for the others' ef.

## Running it

```bash
python3 -m venv ~/data/venv && . ~/data/venv/bin/activate
pip install polars pyarrow numpy scipy lightgbm huggingface_hub
(cd strata-py && pip install maturin && maturin develop --release)
# Yambda-50M's flat files into ~/data/yambda/flat/50m (listens, likes, dislikes, multi_event, embeddings, artist_item_mapping)
cd strata/results/recsys
python yambda.py prep  --data ~/data/yambda --work ~/data/yambda/work    # split, baselines (about 40 min)
python yambda.py train --data ~/data/yambda --work ~/data/yambda/work --minutes 80
python yambda.py load  --data ~/data/yambda --work ~/data/yambda/work
python yambda.py eval  --data ~/data/yambda --work ~/data/yambda/work
python serving_data.py --data ~/data/yambda --work ~/data/yambda/work --out ~/data/recsys-100k --items 100000
cd ../../../strata-server && cargo build --profile ci --bins
DATA=~/data/recsys-100k OUT=~/data/recsys-out RUN_ARGS="--users 300" \
    deploy/compare/recsys/local.sh strata es os vespa qdrant milvus redis postgres
```

Pass `--data` and `--work` explicitly where `$HOME` is not the user's
(they default to `~/data/yambda`). `train` runs for hours: start it with
`setsid nohup … &` where a shell's background jobs are time-limited.

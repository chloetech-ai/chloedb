# Parley load benchmark: results

Measured on 2026-11-09 with `bench/load.py`, unchanged, on three ChloeDB
backends, one run at a time. The JSON files beside this page are the harness's
own output; every number below comes from them, or from the two extra
measurements described under [Notes](#notes).

**All servers ran on one 4-vCPU machine.** The coordinator's two shards shared
those CPUs with it and with the benchmark, so the cluster column shows what the
coordinator adds. It does not show what more machines would gain.

## Machine

A cloud container (the harness records it as `vm x86_64 4 CPUs, Linux 6.18.44-fc-v310`).

```
$ nproc
4
$ free -g
               total        used        free      shared  buff/cache   available
Mem:              15           5           1           0           9          10
Swap:              0           0           0
$ uname -r
6.18.44-fc-v310
```

The CPU is an `Intel(R) Xeon(R) Processor @ 2.10GHz`. Software: Python 3.13.18
with the `chloedb` extension, and `chloedb-server` 0.1.0 (release build,
`--features vision`). Posts and agents are embedded on write with
potion-base-8M (256 dimensions). A live Parley demo (three chloedb-servers and
its app) ran on the same machine throughout, at 1-3% CPU each.

## Dataset

Each run starts from an empty database. With `--seed 7` (the default), the
three 100k runs load identical data.

| | the three 100k runs | the 1M run |
|---|---|---|
| agents | 2,000 | 10,000 |
| follows | 79,329 | 418,588 |
| posts | 100,000 | 1,000,000 |

The timed writes then add up to 200 posts, likes and follows.

## Load

Rows go in through `write()` in batches of 2,000. The times include the
harness generating the rows in Python (see [Notes](#notes)).

| backend | agents (s) | follows (s) | posts (s) | posts per second | whole run (wall) |
|---|---|---|---|---|---|
| embedded | 0.2 | 0.9 | 19.6 | 5,100 | 28 s |
| server | 0.1 | 1.0 | 10.4 | 9,611 | 20 s |
| coordinator, 2 shards | 0.1 | 1.3 | 10.2 | 9,849 | 25 s |
| server, 1M posts | 0.7 | 11.5 | 183.1 | 5,462 | 212 s |

The three 100k runs took 73 s together, under the 25-minute limit, so the
1M-post run went ahead.

## Latency

Each operation runs 200 times with random agents. It is timed from the app's
side: Parley's Python plus every ChloeDB call it makes, without HTTP to Parley
itself. Each cell is **p50 / p95 / p99 in ms**. The number in brackets is how
many ChloeDB calls the operation makes. With 200 samples, p99 is the
third-slowest call.

| operation (calls) | embedded | server | coordinator, 2 shards | server, 1M posts, right after load | server, 1M posts, settled |
|---|---|---|---|---|---|
| feed_following (2) | 1.76 / 3.51 / 5.51 | 4.18 / 9.06 / 12.67 | 5.61 / 15.42 / 23.32 | 5.63 / 17.77 / 37.57 | 4.94 / 10.89 / 11.69 |
| feed_for_you (3) | 2.59 / 3.94 / 6.55 | 5.81 / 12.02 / 13.69 | 10.27 / 12.24 / 14.01 | 11.97 / 35.27 / 84.9 | 5.98 / 7.84 / 10.58 |
| search_posts (1) | 1.72 / 2.96 / 5.88 | 2.77 / 7.92 / 10.8 | 6.76 / 8.02 / 8.97 | 11.01 / 19.89 / 24.89 | 4.15 / 5.22 / 6.47 |
| search_agents (1) | 0.49 / 0.65 / 0.89 | 1.63 / 5.03 / 6.42 | 4.83 / 6.5 / 9.06 | 2.25 / 6.5 / 11.58 | 1.81 / 2.14 / 2.57 |
| who_to_follow (2; app walk: 3) | 3.1 / 4.53 / 5.15 | 5.83 / 12.31 / 18.02 | 9.13 / 13.38 / 15.02 (app walk) | 14.16 / 22.04 / 25.84 | 7.39 / 11.07 / 12.14 |
| thread (2) | 0.11 / 0.21 / 0.27 | 1.66 / 4.66 / 7.31 | 3.61 / 4.18 / 6.11 | 2.74 / 6.97 / 10.17 | 1.94 / 2.25 / 3.59 |
| profile (2) | 0.41 / 0.52 / 0.75 | 2.41 / 5.9 / 9.58 | 4.1 / 5.07 / 6.55 | 3.93 / 9.48 / 14.98 | 2.57 / 3.05 / 3.35 |
| post (write) (2) | 0.79 / 1.55 / 5 | 2.45 / 5.52 / 8.26 | 4.65 / 5.34 / 6.63 | 3.99 / 13.4 / 21.13 | 2.76 / 3.48 / 5.13 |
| like (write) (3) | 0.8 / 1.06 / 2.85 | 3.32 / 7.57 / 10.43 | 5.78 / 6.89 / 7.54 | 5.16 / 15.45 / 24.22 | 3.83 / 4.43 / 4.95 |
| follow (write) (3) | 0.77 / 1 / 4.47 | 3.51 / 6.26 / 8.76 | 5.1 / 6.29 / 7.05 | 5.29 / 16.8 / 26.07 | n/a: no write happened (see notes) |

Through the coordinator, `who_to_follow` walks the follow graph from the app,
because the coordinator refuses graph queries. The two paths, each timed on
the same backend for the same 200 agents (`who-to-follow-paths.json`), p50 / p95 / p99 ms:

- embedded: REACHABLE + ppr 2.66 / 3.55 / 5.58, app walk 1.74 / 3.29 / 3.69
- server: REACHABLE + ppr 5.53 / 7.72 / 11.52, app walk 5.33 / 7.31 / 8.31
- coordinator, 2 shards: app walk only, 9.38 / 13.68 / 15.46

## What the numbers show

Taking the database out of the process adds about 1-3 ms at p50 to each operation (one HTTP JSON round trip per call, and operations make one to three calls), and on one server every operation still answers in under 6 ms at p50. On this one machine the coordinator adds about 0.5-1 ms per key read or one-shard write, and 3-4.5 ms to the ranked queries that ask both shards and merge their answers (search_posts +4.0, search_agents +3.2, feed_for_you +4.5 ms at p50 over one server), which is pure overhead: every shard shares the same 4 vCPUs, so there is no scale-out gain to see. The two slowest operations everywhere are the ranked ones: who_to_follow (REACHABLE, with personalized PageRank fused with bio similarity) and feed_for_you (three calls, the last of which embeds the agent's taste text at query time and ranks the last 72 hours of posts by vector similarity, recency and likes). On one machine the app-side two-hop walk costs no more than REACHABLE/ppr (1.74 vs 2.66 ms embedded, 5.33 vs 5.53 ms on a server), though it ranks by a count of shared follows rather than by PageRank and bio similarity; through the coordinator it takes 9.38 ms, because each of its three calls pays the extra hop, so on a cluster its cost is round trips, not computation. With 10x the posts, once the server had settled, p50s stay within 1.5x of the 100k server's (search_posts 2.77 to 4.15, who_to_follow 5.83 to 7.39, feed_for_you 5.81 to 5.98 ms), but measured right after the load, while the server was still merging in the background, the p99s were 2-8x worse (feed_for_you 84.9 vs 10.58 ms).

## Notes

- **Nothing failed.** All five harness invocations exited 0, and their stderr
  was empty. No server log has an ERROR line. The only warning, in every log,
  was an alert,
  `alert firing: Disk almost full rule=disk-full severity=warning value=0.9125 threshold=0.9`.
  It weighs the container volume's 252 GB size against the 22 GB that were
  free, which was plenty: the 1M database took 1.9 GB.
- **The coordinator refuses graph queries.** Sent the in-database
  who_to_follow query, it answers, verbatim:
  `ChloeDBError: graph queries (REACHABLE, PATH, hops(), ppr) are not supported through a coordinator yet: a row's edges point at rows other shards hold. Run them on a server or an embedded database that holds the whole table, or walk from the application, one round of reads by key per hop (the Graphs page of the docs, On a cluster) (HTTP 400)`.
  Parley knows this (`Store.graph_in_db` is false for a coordinator) and walks
  the graph from the app, so the benchmark never sends it that query.
- **The 1M latencies were measured twice.** The harness times operations
  right after it loads. When the 1M run ended, the server still ran at about
  395% CPU with no client, merging what it had just taken, for about 40 s more.
  The "right after load" column therefore shares the CPUs with those merges.
  Once its CPU was idle, the same data was measured again with the harness's
  own `--skip-load` (`server-1m-settled.json`). In that pass the `follow
  (write)` row is not a write. The harness seeds its random choices the same
  way every time, so it picked the same 200 pairs the first pass had already
  followed, and `Parley.follow` returned "already following" after two key
  reads: the follows count was 418,787 before that pass and after it. Its
  posts and likes did grow by 200 each, so those rows are real writes. The
  100k runs were not checked for merges still running while they were timed.
- **The load rates include the harness's own Python.** The same post-generating
  loop, timed alone with no writes, takes 3.0 s for 100k posts and 2,000
  agents, but 85.1 s for 1M posts and 10,000 agents.
  `rng.choices(handles, weights=weights)` adds up all the agents' weights again
  for each of the ~300,000 picks of a popular author. Writes are synchronous,
  so the database's share of the 183.1 s 1M post load is about 98 s, roughly
  10,000 posts per second. That is an estimate: the generation was timed in a
  separate process, without the server competing for the CPUs. `bench/load.py`
  was left as it is.
- The embedded load (5,100 posts/s) was slower than one server's (9,611
  posts/s), on the same data. This was not looked into.
- `PARLEY_BENCH_MEDIA` was set to `data/bench/media`, not the harness's
  default under `/tmp`. The benchmark posts no pictures.

## How it was run

From `examples/parley`, with `. .venv/bin/activate`, each run on fresh
directories under `data/bench/`:

```sh
python bench/load.py --target data/bench/emb/parley.strata --agents 2000 --posts 100000 --queries 200 --label embedded --out bench/results/embedded-100k.json

chloedb-server --db data/bench/srv/s.strata --seq-no --listen 127.0.0.1:9700 --grpc 127.0.0.1:9701 --no-token &
python bench/load.py --target http://127.0.0.1:9700 --agents 2000 --posts 100000 --queries 200 --label server --out bench/results/server-100k.json

chloedb-server --db data/bench/c2/shard-0.strata --seq-no --listen 127.0.0.1:9710 --grpc 127.0.0.1:9711 --no-token &
chloedb-server --db data/bench/c2/shard-1.strata --seq-no --listen 127.0.0.1:9720 --grpc 127.0.0.1:9721 --no-token &
chloedb-server --shards http://127.0.0.1:9711,http://127.0.0.1:9721 --listen 127.0.0.1:9600 --grpc off --no-token &
python bench/load.py --target http://127.0.0.1:9600 --agents 2000 --posts 100000 --queries 200 --label "coordinator, 2 shards" --out bench/results/cluster2-100k.json

chloedb-server --db data/bench/srv1m/s.strata --seq-no --listen 127.0.0.1:9700 --grpc 127.0.0.1:9701 --no-token &
python bench/load.py --target http://127.0.0.1:9700 --agents 10000 --posts 1000000 --queries 200 --label server --out bench/results/server-1m.json
# once the server's CPU was idle:
python bench/load.py --target http://127.0.0.1:9700 --agents 10000 --posts 1000000 --queries 200 --skip-load --label "server, settled" --out bench/results/server-1m-settled.json
```

`who-to-follow-paths.json` times `Parley.who_to_follow` on the 100k data after
those runs. It runs the same 200 seeded agents with `Store.graph_in_db` set to
true, then to false, and is read-only. The coordinator offers only the walk.

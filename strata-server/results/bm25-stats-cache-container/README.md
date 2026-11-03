# BM25 through a coordinator: kept statistics and a boxed gRPC call

Measured 2026-11-03 in a cloud container: 4 vCPUs (Intel Xeon Processor
@ 2.10GHz), 15 GB of memory, Linux. Each server has a core of its own
(`taskset`): shard 0 on cpu0, shard 1 on cpu1, the coordinator on cpu2,
and engine-compare's client on cpu3.

It reproduces, on one machine, the gap under load in the 1M DigitalOcean
comparison (`../compare-msmarco-1m-do/passes.md`, medians of 3 passes):
Strata's BM25 at 873 q/s with 4 clients and 1,128 with 16, against ES's
1,063 and 1,874. Here, main gives 751 to 834 and 1,022 to 1,026.

## Data and servers

- MS MARCO v2.1 segmented, the first million passages of shard 00, with
  the comparison's ids, synthetic year/category/rating and its 1,000 dev
  queries (`prep.py`), but 8-dimension placeholder vectors, since the BM25
  kinds never read the vector field.
- Two shards built offline, `engine-compare --collection msmarco-1000k
  build --shard {0,1} --of 2 --threads 2`, and a coordinator over them
  (`serve.sh`), all with `--timeout-ms 30000`.
- Three builds, each `cargo build --release --bins`:
  - `base`: main at bf64052;
  - `cache`: the coordinator keeps BM25 statistics per shard version
    (protocol 12), at af3eae7;
  - `grpc`: `cache`, and a gRPC call's future boxed once in `Grpc::serve`
    rather than moved through its 8 wrappers (a shard's `Service::query`
    future is 16,192 bytes, `term_stats`'s 3,600).

## Method

`BUILDS="A B" ab.sh OUT 3 "bm25,bm25 + filter,phrase,prefix"`: three
passes, each running build A, then build B, each on fresh copies of the two
shard files, so both start from the same databases. A pass is
engine-compare's `run` (warm-up rounds until p50 settles, then 1, 4 and 16
clients). `summary.py DIR A B` gives the tables. The reports hold each
query's answer: all three builds' 1,059 answers have the same keys and the
same scores, bit for bit.

| runs | builds | summary |
|---|---|---|
| `runs/` | base, cache | `summary.md` |
| `runs-grpc/` | cache, grpc | `summary-grpc.md` |
| `runs-both/` | base, grpc | `summary-both.md` |

With kept statistics, a pass sent 77,000 to 89,000 queries the kept statistics,
298 a statistics round (the first time each term was seen), and none stale
(`*.metrics`): the comparison writes nothing during its reads.

## Result

main against both changes (`runs-both/`), medians of 3 passes (lowest to
highest in `summary-both.md`):

| kind | clients | q/s before | q/s after | change | p50 ms before | p50 ms after |
|---|---|---|---|---|---|---|
| bm25 | 1 | 381 | 569 | +49% | 2.35 | 1.53 |
| bm25 | 4 | 751 | 1151 | +53% | 4.78 | 2.97 |
| bm25 | 16 | 1026 | 1547 | +51% | 14.66 | 9.24 |
| bm25 + filter | 1 | 310 | 415 | +34% | 2.86 | 2.09 |
| bm25 + filter | 4 | 546 | 759 | +39% | 6.77 | 4.65 |
| bm25 + filter | 16 | 691 | 929 | +35% | 21.87 | 14.79 |
| phrase | 1 | 399 | 562 | +41% | 2.29 | 1.54 |
| phrase | 4 | 817 | 1210 | +48% | 4.49 | 2.89 |
| phrase | 16 | 1062 | 1710 | +61% | 14.72 | 8.63 |
| prefix | 1 | 282 | 354 | +25% | 3.23 | 2.53 |
| prefix | 4 | 486 | 610 | +26% | 7.74 | 5.89 |
| prefix | 16 | 521 | 735 | +41% | 29.78 | 17.32 |

Each change alone, BM25 at 16 clients: kept statistics 1,022 to 1,270 q/s
(+24%, `summary.md`), the boxed call 1,223 to 1,527 (+25%,
`summary-grpc.md`). The same build differs by up to 5% between sets of
runs (`cache`: 1,270 in `runs/`, 1,223 in `runs-grpc/`), so compare
builds within one set.

## Where the time went before

With main at 16 clients, each shard used 0.93 of its core and the
coordinator 0.67 of its own (`/proc` over 10 s). `perf record --call-graph
dwarf` of shard 0 over the same load: scoring 46% of its samples, `memmove`
13.5% (almost all of it in the gRPC `Query` and `TermStats` handlers,
moving their calls' futures), system calls and futex wakes 12%, and the
statistics round's handler 7.3%. The kept statistics take a call to every
shard off each query; the boxed future takes the copies off the calls left.
These shares are of this container and setup; on DigitalOcean each shard
had 2.9 CPUs and the coordinator one.

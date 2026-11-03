# BM25 through a coordinator, with and without kept statistics

Measured 2026-11-03 in a cloud container: 4 vCPUs (Intel Xeon Processor
@ 2.10GHz), 15 GB of memory, Linux. Each server has a core of its own
(`taskset`): shard 0 on cpu0, shard 1 on cpu1, the coordinator on cpu2,
and engine-compare's client on cpu3.

It reproduces, on one machine, the gap under load in the 1M DigitalOcean
comparison (`../compare-msmarco-1m-do/passes.md`, medians of 3 passes):
Strata's BM25 at 873 q/s with 4 clients and 1,128 with 16, against ES's
1,063 and 1,874. Here, with the old build, the same kind gives 834 and
1,022.

## Data and servers

- MS MARCO v2.1 segmented, the first million passages of shard 00, with
  the comparison's ids, synthetic year/category/rating and its 1,000 dev
  queries (`prep.py`), but 8-dimension placeholder vectors, since the BM25
  kinds never read the vector field.
- Two shards built offline, `engine-compare --collection msmarco-1000k
  build --shard {0,1} --of 2 --threads 2`, and a coordinator over them
  (`serve.sh`), all with `--timeout-ms 30000`.
- `base`: main at bf64052. `cache`: this branch at af3eae7, which keeps the
  statistics (protocol 12). Both `cargo build --release --bins`.

## Method

`ab.sh OUT 3 "bm25,bm25 + filter,phrase,prefix"`: three passes, each
running the old build, then the new one, each on fresh copies of the two
shard files, so both start from the same databases. A pass is engine-compare's `run` (warm-up rounds until p50 settles, then 1, 4
and 16 clients). `summary.py runs` gives `summary.md`. The reports in
`runs/` hold each query's answer: the two builds' 1,059 answers have the
same keys and the same scores, bit for bit.

The new build sent 77,472 queries of its last pass the kept statistics, 298
a statistics round (the first time each term was seen), and none stale
(`runs/cache-p3.metrics`): the comparison writes nothing during its reads.

## Result

Medians of 3 passes (lowest to highest in `summary.md`):

| kind | clients | q/s before | q/s after | change | p50 ms before | p50 ms after |
|---|---|---|---|---|---|---|
| bm25 | 1 | 409 | 529 | +29% | 2.18 | 1.67 |
| bm25 | 4 | 834 | 1016 | +22% | 4.36 | 3.53 |
| bm25 | 16 | 1022 | 1270 | +24% | 14.74 | 11.74 |
| bm25 + filter | 1 | 332 | 399 | +20% | 2.71 | 2.19 |
| bm25 + filter | 4 | 602 | 704 | +17% | 6.12 | 5.13 |
| bm25 + filter | 16 | 710 | 787 | +11% | 21.26 | 18.88 |
| phrase | 1 | 420 | 525 | +25% | 2.16 | 1.67 |
| phrase | 4 | 918 | 1121 | +22% | 3.92 | 3.17 |
| phrase | 16 | 1178 | 1417 | +20% | 13.08 | 10.76 |
| prefix | 1 | 301 | 354 | +18% | 3.01 | 2.55 |
| prefix | 4 | 491 | 597 | +22% | 7.55 | 6.07 |
| prefix | 16 | 575 | 629 | +9% | 26.46 | 23.24 |

## Where the time went before

With the old build at 16 clients, each shard used 0.93 of its core and the
coordinator 0.67 of its own (`/proc` over 10 s). `perf record --call-graph
dwarf` of shard 0 over the same load: scoring 46% of its samples, copying
large futures in the gRPC query handler 13.5%, system calls and futex
wakes 12%, and the statistics round's handler 7.3%. A query was two calls
to every shard (statistics, then the query), so the round cost each shard
a call's overhead as well as its own work; the kept statistics take that
call away. These shares are of this container and setup; on DigitalOcean
each shard had 2.9 CPUs and the coordinator one.

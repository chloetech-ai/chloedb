# Part A on DigitalOcean (2026-11-02)

The laptop benchmark's data preparation and Part A (A1 replacing SQLite, A2
search, writes and streams, A3 recommendations), run on one DigitalOcean
Kubernetes node by `strata-py/bench/laptop/do_part_a.sh`, one system at a time
on an otherwise idle node.

- **Machine:** DOKS node `g-8vcpu-32gb-intel` in nyc3: Intel Xeon Platinum 8358
  @ 2.60GHz, 8 vCPU, 31 GiB, Linux 6.12 (the node's Debian 13 image), Python
  3.12.16 in the bench pod. Every report's `machine` field says the same.
- **Strata:** the rows named `strata` are 2061b64 (main with the join fix #219,
  plus #221's thread defaults and HNSW fix). `strata (1 thread)` is 128459b,
  before #221 (embedded strata-py then built with one vector thread and one
  executor thread; its A1 key reads are from 1b128eb). `strata (before HNSW
  fix)` is fbbf33d (thread defaults, without the HNSW race fix 8248900 or #219).
- **Others:** SQLite 3.53.1 with sqlite-vec 0.1.9 (built from the amalgamation
  on the node: Debian's 3.40.1 has no FTS5 `contentless_delete`, so its writes
  failed; the 3.40.1 rows that ran before the switch are in `sqlite-3.40.1/`),
  DuckDB 2.0.0, Tantivy 0.27.0, LanceDB and FAISS.
- **Cost:** one node for 16.2 hours at $0.4494/h, about $7.30. The cluster
  `strata-bench-a` was deleted with its resources at 21:34 UTC, and no droplet,
  volume or load balancer tagged `strata-bench` is left.

| directory | what |
|---|---|
| `part-a/` | the final reports, one JSON per system and part (`a1_*`, `a2_*_<collection>`, `writes_*`, `a3_*`) |
| `part-a-1thread/`, `before-hnsw-fix/` | the same parts on the two earlier Strata builds (and A1 for SQLite 3.40.1 and DuckDB in `part-a-1thread/`) |
| `sqlite-3.40.1/` | SQLite 3.40.1's A1 and its A2 on scifact, nfcorpus and fiqa, replaced by 3.53.1's |
| `build-split/` | `a2_build_split.py`: Strata's A2 build with and without its vector field, beside Tantivy |

The tables below come from

```bash
python3 strata-py/bench/laptop/part_a_report.py part-a --compare "1 thread=part-a-1thread" --compare "before HNSW fix=before-hnsw-fix"
```

The JSON files are the reports as written, without indentation. The nfcorpus
workload (`strata/results/workloads/nfcorpus.json`) was made on the node with
`strata-baselines --collections nfcorpus --workload-only`, as the driver does.

Caveats:

- The A2 runs of Strata 2061b64 on nfcorpus, fiqa, trec-covid and quora ran
  after its A1, writes and A3 runs, not before them: the first attempt stopped
  after scifact because the nfcorpus workload was missing from that tree.
- At 19:10 UTC a gzip ran on one core for about 3 seconds during DuckDB's quora
  build (179.9 s).
- Eight threads give recall noise of about ±0.003 between builds: compare the
  averages across collections, not one cell.
- Strata's file sizes include space that merges freed: strata-py has no
  `vacuum()`, and `optimize()` does not return it. On fiqa (locally, 4 vCPU) the
  2,000-row-batch build is 646 MB; `.vacuum` in the `strata` shell takes it to
  451 MB; one `insert()` of all rows gives 350 MB.

## Results

Machine: g-8vcpu-32gb-intel nyc3, Intel(R) Xeon(R) Platinum 8358 CPU @ 2.60GHz, 8 vCPU, 31 GiB, Linux-6.12.113+deb13-amd64-x86_64-with-glibc2.36, Python 3.12.16.

### A1: the app (Shop, 137,260 products and 4,624,176 reviews)

Reads (p50 / p99, one thread):

| system | get by id (SQL) | get by key (no SQL) | get, 4 threads | 100 by id (SQL) | 100 by key | a user's reviews | page 1 | page 10,000 |
|---|---|---|---|---|---|---|---|---|
| strata | 64 µs / 112 µs | 21 µs / 40 µs | 37,879/s | 1.94 ms / 2.20 ms | 1.79 ms / 2.17 ms | 84 µs / 141 µs | 1.33 ms / 1.73 ms | 3.56 ms / 4.32 ms |
| sqlite-full, 3.53.1 | 8 µs / 20 µs | - | 14,326/s | 321 µs / 443 µs | - | 12 µs / 38 µs | 8.63 ms / 10.7 ms | 29.9 ms / 36.9 ms |
| sqlite-normal, 3.53.1 | 8 µs / 19 µs | - | 14,232/s | 319 µs / 443 µs | - | 12 µs / 41 µs | 6.92 ms / 10.5 ms | 28.8 ms / 31.3 ms |
| duckdb, 2.0.0 | 661 µs / 905 µs | - | 4,600/s | 4.99 ms / 6.36 ms | - | 33.3 ms / 38.9 ms | 9.64 ms / 12.2 ms | 15.7 ms / 19.6 ms |
| strata (1 thread) | 59 µs / 105 µs | - | 41,794/s | - | - | 79 µs / 137 µs | 1.40 ms / 2.01 ms | 3.57 ms / 4.66 ms |
| strata (before HNSW fix) | 60 µs / 100 µs | 21 µs / 40 µs | 40,334/s | 1.90 ms / 2.24 ms | 1.80 ms / 2.51 ms | 80 µs / 131 µs | 1.27 ms / 1.76 ms | 3.57 ms / 4.48 ms |

Loads, aggregates and joins:

| system | load reviews | file | reviews by brand | reviews by month | join: top products | join: a user's |
|---|---|---|---|---|---|---|
| strata | 30,446/s | 3,353 MB | 55.5 ms / 63.3 ms | 881 ms / 895 ms | 516 ms / 546 ms | 657 µs / 1.19 ms |
| sqlite-full, 3.53.1 | 20,872/s | 2,867 MB | 103 ms / 108 ms | 3,569 ms / 3,627 ms | 2,547 ms / 2,575 ms | 22 µs / 90 µs |
| sqlite-normal, 3.53.1 | 21,509/s | 2,867 MB | 99.5 ms / 109 ms | 3,489 ms / 3,499 ms | 2,577 ms / 2,606 ms | 22 µs / 93 µs |
| duckdb, 2.0.0 | 51,354/s | 3,230 MB | 5.17 ms / 6.61 ms | 10.1 ms / 13.0 ms | 215 ms / 241 ms | 32.6 ms / 39.1 ms |
| strata (1 thread) | 30,803/s | 3,353 MB | 57.7 ms / 63.7 ms | 881 ms / 903 ms | 2,795 ms / 2,811 ms | 635 µs / 1.15 ms |
| strata (before HNSW fix) | 30,206/s | 3,353 MB | 55.1 ms / 58.6 ms | 930 ms / 937 ms | 2,838 ms / 2,840 ms | 615 µs / 1.11 ms |

Writes (one row per transaction unless noted) and a mixed run of 2 readers and 1 writer:

| system | insert | insert, 1,000 per txn | update price | counter + 1 | delete by id | mixed reads | mixed writes | errors |
|---|---|---|---|---|---|---|---|---|
| strata | 4,022/s | 95,670/s | 1,864/s | 1,971/s | 4,272/s | 2,629/s, p99 8.60 ms | 1,518/s, p99 1.22 ms | 0 |
| sqlite-full, 3.53.1 | 5,128/s | 18,776/s | 6,640/s | 9,646/s | 6,113/s | 546/s, p99 41.2 ms | 1,948/s, p99 4.80 ms | 0 |
| sqlite-normal, 3.53.1 | 11,091/s | 18,905/s | 18,629/s | 69,896/s | 15,181/s | 570/s, p99 39.5 ms | 17,382/s, p99 162 µs | 0 |
| duckdb, 2.0.0 | 327/s | 108,111/s | 865/s | 881/s | 968/s | 97/s, p99 168 ms | 301/s, p99 10.4 ms | 0 |
| strata (1 thread) | 4,070/s | 107,039/s | 1,741/s | 1,880/s | 4,364/s | 3,101/s, p99 8.02 ms | 1,324/s, p99 1.32 ms | 0 |
| strata (before HNSW fix) | 4,072/s | 102,690/s | 1,849/s | 1,908/s | 4,820/s | 3,265/s, p99 7.96 ms | 1,529/s, p99 1.20 ms | 0 |

### A2: search (BEIR, bge-small 384-d vectors)

Each cell is p50 / p99 on one thread, then nDCG@10 (vector: and recall@10 against exact search). Hybrid is RRF of BM25 and vectors at depth 100.

#### scifact (5,183 documents)

| system | build | size | BM25 | vector | hybrid | filtered hybrid | hybrid, 4 threads |
|---|---|---|---|---|---|---|---|
| strata | 1.0 s | 35 MB | 179 µs / 322 µs · 0.690 | 311 µs / 455 µs · 0.718 · r 0.999 | 643 µs / 944 µs · 0.729 | 500 µs / 968 µs | 4,018/s |
| sqlite | 0.6 s | 42 MB | 2.95 ms / 6.34 ms · 0.681 | 9.46 ms / 10.3 ms · 0.718 · r 1.000 | 14.8 ms / 18.8 ms · 0.738 | 9.16 ms / 74.8 ms | 259/s |
| tantivy | 0.3 s | 3 MB | 103 µs / 201 µs · 0.686 | - | - | - | - |
| duckdb | 2.9 s | 50 MB | 21.8 ms / 25.4 ms · 0.685 | 4.33 ms / 5.56 ms · 0.718 · r 0.997 | 26.4 ms / 31.7 ms · 0.737 | 33.3 ms / 53.2 ms | 99/s |
| lancedb | 0.8 s | 29 MB | 3.15 ms / 4.54 ms · 0.686 | 11.8 ms / 15.6 ms · 0.718 · r 1.000 | 14.8 ms / 19.4 ms · 0.736 | 8.30 ms / 16.0 ms | 90/s |
| strata (1 thread) | 1.7 s | 35 MB | 178 µs / 410 µs · 0.690 | 403 µs / 535 µs · 0.718 · r 1.000 | 892 µs / 1.29 ms · 0.729 | 560 µs / 1.25 ms | 4,304/s |
| strata (before HNSW fix) | 1.0 s | 35 MB | 169 µs / 322 µs · 0.690 | 316 µs / 498 µs · 0.711 · r 0.992 | 656 µs / 965 µs · 0.726 | 485 µs / 900 µs | 4,112/s |

#### nfcorpus (3,633 documents)

| system | build | size | BM25 | vector | hybrid | filtered hybrid | hybrid, 4 threads |
|---|---|---|---|---|---|---|---|
| strata | 0.7 s | 25 MB | 97 µs / 178 µs · 0.329 | 297 µs / 467 µs · 0.385 · r 0.986 | 487 µs / 742 µs · 0.378 | 390 µs / 756 µs | 5,574/s |
| sqlite | 0.4 s | 29 MB | 262 µs / 2.58 ms · 0.320 | 6.14 ms / 6.73 ms · 0.387 · r 1.000 | 7.51 ms / 10.8 ms · 0.377 | 1.53 ms / 46.5 ms | 456/s |
| tantivy | 0.2 s | 2 MB | 38 µs / 103 µs · 0.320 | - | - | - | - |
| duckdb | 1.8 s | 35 MB | 19.6 ms / 28.3 ms · 0.323 | 4.22 ms / 5.54 ms · 0.384 · r 0.976 | 23.9 ms / 32.5 ms · 0.379 | 30.0 ms / 43.5 ms | 121/s |
| lancedb | 0.6 s | 21 MB | 2.91 ms / 4.03 ms · 0.326 | 10.2 ms / 12.0 ms · 0.387 · r 0.999 | 13.0 ms / 15.8 ms · 0.384 | 8.35 ms / 14.8 ms | 102/s |
| strata (1 thread) | 1.1 s | 25 MB | 71 µs / 148 µs · 0.329 | 306 µs / 433 µs · 0.386 · r 0.990 | 527 µs / 861 µs · 0.379 | 357 µs / 750 µs | 6,025/s |
| strata (before HNSW fix) | 0.6 s | 25 MB | 92 µs / 163 µs · 0.329 | 287 µs / 443 µs · 0.382 · r 0.976 | 485 µs / 735 µs · 0.378 | 389 µs / 751 µs | 5,656/s |

#### fiqa (57,638 documents)

| system | build | size | BM25 | vector | hybrid | filtered hybrid | hybrid, 4 threads |
|---|---|---|---|---|---|---|---|
| strata | 15.5 s | 646 MB | 338 µs / 724 µs · 0.253 | 529 µs / 878 µs · 0.417 · r 0.994 | 1.27 ms / 1.99 ms · 0.361 | 947 µs / 3.23 ms | 1,575/s |
| sqlite | 5.8 s | 325 MB | 30.8 ms / 63.7 ms · 0.252 | 104 ms / 108 ms · 0.421 · r 1.000 | 144 ms / 180 ms · 0.365 | 108 ms / 1,567 ms | 28/s |
| tantivy | 1.3 s | 19 MB | 419 µs / 914 µs · 0.252 | - | - | - | - |
| duckdb | 20.1 s | 541 MB | 35.1 ms / 42.3 ms · 0.230 | 4.96 ms / 5.79 ms · 0.414 · r 0.982 | 42.8 ms / 49.8 ms · 0.349 | 97.2 ms / 195 ms | 48/s |
| lancedb | 12.2 s | 346 MB | 3.61 ms / 5.30 ms · 0.256 | 5.69 ms / 7.10 ms · 0.416 · r 0.977 | 7.65 ms / 10.2 ms · 0.380 | 10.5 ms / 61.1 ms | 130/s |
| strata (1 thread) | 36.6 s | 646 MB | 544 µs / 1.44 ms · 0.253 | 1.12 ms / 1.36 ms · 0.418 · r 0.994 | 2.68 ms / 3.99 ms · 0.358 | 1.47 ms / 6.17 ms | 1,435/s |
| strata (before HNSW fix) | 14.8 s | 646 MB | 327 µs / 698 µs · 0.253 | 511 µs / 853 µs · 0.417 · r 0.991 | 1.20 ms / 2.02 ms · 0.358 | 897 µs / 3.12 ms | 1,625/s |

#### trec-covid (171,332 documents)

| system | build | size | BM25 | vector | hybrid | filtered hybrid | hybrid, 4 threads |
|---|---|---|---|---|---|---|---|
| strata | 57.9 s | 2,306 MB | 733 µs / 2.51 ms · 0.664 | 582 µs / 949 µs · 0.819 · r 0.990 | 2.09 ms / 4.68 ms · 0.834 | 1.36 ms / 10.4 ms | 1,089/s |
| sqlite | 37.2 s | 1,050 MB | 109 ms / 203 ms · 0.605 | 304 ms / 310 ms · 0.819 · r 0.998 | 439 ms / 533 ms · 0.808 | 331 ms / 4,717 ms | 9/s |
| tantivy | 4.8 s | 68 MB | 1.43 ms / 2.83 ms · 0.611 | - | - | - | - |
| duckdb | 55.9 s | 1,751 MB | 91.6 ms / 122 ms · 0.649 | 5.38 ms / 7.62 ms · 0.816 · r 0.988 | 99.7 ms / 122 ms · 0.839 | 237 ms / 362 ms | 15/s |
| lancedb | 41.6 s | 1,072 MB | 4.12 ms / 8.73 ms · 0.657 | 5.31 ms / 7.03 ms · 0.817 · r 0.980 | 7.91 ms / 11.9 ms · 0.768 | 15.1 ms / 192 ms | 130/s |
| strata (1 thread) | 144.1 s | 2,306 MB | 1.05 ms / 4.03 ms · 0.664 | 1.27 ms / 1.50 ms · 0.820 · r 0.994 | 4.19 ms / 8.43 ms · 0.834 | 2.12 ms / 13.7 ms | 907/s |
| strata (before HNSW fix) | 56.0 s | 2,306 MB | 707 µs / 2.41 ms · 0.664 | 569 µs / 956 µs · 0.818 · r 0.990 | 2.09 ms / 4.67 ms · 0.835 | 1.36 ms / 10.1 ms | 1,098/s |

#### quora (522,931 documents)

| system | build | size | BM25 | vector | hybrid | filtered hybrid | hybrid, 4 threads |
|---|---|---|---|---|---|---|---|
| strata | 214.4 s | 6,223 MB | 280 µs / 722 µs · 0.755 | 651 µs / 1.36 ms · 0.892 · r 0.990 | 1.37 ms / 2.29 ms · 0.851 | 1.90 ms / 13.8 ms | 1,310/s |
| sqlite | 220.4 s | 2,277 MB | 180 ms / 336 ms · 0.791 | 1,085 ms / 1,110 ms · 0.893 · r 1.000 | 1,334 ms / 1,508 ms · 0.867 | 2,475 ms / 90,465 ms | 3/s |
| tantivy | 8.7 s | 33 MB | 2.29 ms / 4.06 ms · 0.792 | - | - | - | - |
| duckdb | 179.9 s | 4,432 MB | 55.6 ms / 69.2 ms · 0.722 | 5.98 ms / 8.11 ms · 0.892 · r 0.994 | 65.5 ms / 78.7 ms · 0.833 | 304 ms / 681 ms | 29/s |
| lancedb | 119.8 s | 2,820 MB | 3.54 ms / 5.38 ms · 0.787 | 5.83 ms / 7.62 ms · 0.891 · r 0.989 | 7.86 ms / 10.8 ms · 0.870 | 30.9 ms / 206 ms | 127/s |
| strata (before HNSW fix) | 204.6 s | 6,223 MB | 274 µs / 707 µs · 0.755 | 660 µs / 1.38 ms · 0.890 · r 0.990 | 1.33 ms / 2.20 ms · 0.849 | 1.85 ms / 13.6 ms | 1,363/s |


### Writes, updates and streams (137,260 products with a text index and a vector index)

Rows per second, one writer:

| system | load | insert 1/txn | 100/txn | 1,000/txn | 10,000/txn | update price | update title | update vector | upsert row | delete |
|---|---|---|---|---|---|---|---|---|---|---|
| strata | 16.8 s | 1,954/s | 3,420/s | 5,716/s | 6,534/s | 1,160/s | 60/s | 31/s | 1,944/s | 4,277/s |
| sqlite-full | 9.7 s | 787/s | 8,674/s | 12,681/s | 12,436/s | 7,670/s | 735/s | 2,546/s | 496/s | 2,404/s |
| sqlite-normal | 9.7 s | 853/s | 10,942/s | 12,894/s | 12,354/s | 27,806/s | 833/s | 3,861/s | 524/s | 3,316/s |
| duckdb | 35.0 s | 166/s | 1,494/s | 1,388/s | 1,556/s | 841/s | 611/s | 178/s | 167/s | 856/s |
| lancedb | 18.3 s | 271/s | 9,864/s | 53,651/s | 96,300/s | 2/s | 2/s | 2/s | 1/s | 5/s |
| strata (before HNSW fix) | 16.9 s | 1,694/s | 3,411/s | 6,459/s | 6,397/s | 1,089/s | 57/s | 29/s | 1,941/s | 4,527/s |

A stream of new rows at a target rate, with a reader running:

| system | target | reached | commit p50 | lag p99 | reads |
|---|---|---|---|---|---|
| strata | 1,000/s | 1,000/s | 1.73 ms | 3.20 ms | 17,218/s, p99 646 µs |
| strata | 10,000/s | 10,000/s | 9.92 ms | 239 ms | 6,424/s, p99 18.0 ms |
| strata | 50,000/s | 24,032/s | 107 ms | 7,838 ms | 3,502/s, p99 8.56 ms |
| sqlite-full | 1,000/s | 1,000/s | 7.91 ms | 37.7 ms | 5,281/s, p99 5.82 ms |
| sqlite-full | 10,000/s | 9,295/s | 44.8 ms | 1,069 ms | 533/s, p99 80.8 ms |
| sqlite-full | 50,000/s | 12,960/s | 188 ms | 11,266 ms | 168/s, p99 175 ms |
| sqlite-normal | 1,000/s | 1,000/s | 7.16 ms | 38.1 ms | 5,572/s, p99 5.40 ms |
| sqlite-normal | 10,000/s | 9,360/s | 43.3 ms | 1,026 ms | 530/s, p99 83.0 ms |
| sqlite-normal | 50,000/s | 12,388/s | 182 ms | 11,428 ms | 145/s, p99 219 ms |
| duckdb | 1,000/s | 1,000/s | 6.94 ms | 11.1 ms | 1,664/s, p99 5.54 ms |
| duckdb | 10,000/s | 10,000/s | 10.5 ms | 2,019 ms | 1,005/s, p99 8.58 ms |
| duckdb | 50,000/s | 49,622/s | 24.7 ms | 767 ms | 399/s, p99 24.5 ms |
| duckdb | flat out | 63,316/s | 40.8 ms | 503 ms | 200/s, p99 40.5 ms |
| lancedb | 1,000/s | 1,000/s | 18.0 ms | 50.5 ms | 77/s, p99 191 ms |
| lancedb | 10,000/s | 9,999/s | 23.6 ms | 58.2 ms | 7/s, p99 2,247 ms |
| lancedb | 50,000/s | 49,963/s | 32.4 ms | 55.6 ms | 2/s, p99 8,216 ms |
| lancedb | flat out | 111,419/s | 43.9 ms | 55.7 ms | 1/s, p99 15,158 ms |
| strata (before HNSW fix) | 1,000/s | 1,000/s | 1.80 ms | 6.65 ms | 17,671/s, p99 612 µs |
| strata (before HNSW fix) | 10,000/s | 10,000/s | 9.74 ms | 228 ms | 6,751/s, p99 16.0 ms |
| strata (before HNSW fix) | 50,000/s | 24,093/s | 105 ms | 7,802 ms | 3,418/s, p99 8.84 ms |

### A3: recommendations (similar products under filters; a feed fusing vectors with a walk of the co-purchase graph)

| system | build | size | similar | similar recall@10 | similar, 4 threads | feed |
|---|---|---|---|---|---|---|
| strata | 153.9 s | 2,366 MB | 866 µs / 1.53 ms | 0.996 | 3,287/s | 125 ms / 200 ms |
| lancedb | 30.1 s | 825 MB | 12.9 ms / 17.2 ms | 0.976 | 138/s | 28.3 ms / 34.0 ms |
| sqlite-vec | 31.8 s | 948 MB | 384 ms / 485 ms | 0.999 | 11/s | 897 ms / 1,068 ms |
| faiss-sqlite | 49.0 s | 1,683 MB | 570 µs / 8.49 ms | 0.994 | 2,202/s | - |
| strata (before HNSW fix) | 153.4 s | 2,366 MB | 821 µs / 1.45 ms | 0.995 | 3,461/s | 115 ms / 185 ms |

## Build time: the vector field against Tantivy

`a2_build_split.py`, after the runs above on the same node, best of two. Strata
is built as A2 builds it (2,000-row `insert()` calls, then `optimize()`; the
seconds in brackets are inserts + optimize), then without its 384-d vector
field. Tantivy is built as A2 builds it, then again storing the title and text
as stored, unindexed bytes fields, as Strata keeps them.

| collection | documents | Strata | Strata without the vector field | Tantivy | Tantivy, storing title and text | vector share of Strata's build |
|---|---|---|---|---|---|---|
| scifact | 5,183 | 0.9 s (0.9 + 0.0), 35 MB | 0.3 s (0.3 + 0.0), 8 MB | 0.3 s, 3 MB | 0.3 s, 8 MB | 71% |
| nfcorpus | 3,633 | 0.6 s (0.6 + 0.0), 25 MB | 0.2 s (0.2 + 0.0), 6 MB | 0.2 s, 2 MB | 0.2 s, 6 MB | 68% |
| fiqa | 57,638 | 15.1 s (10.0 + 5.1), 646 MB | 3.2 s (2.1 + 1.1), 93 MB | 1.3 s, 19 MB | 1.8 s, 50 MB | 79% |
| trec-covid | 171,332 | 56.8 s (29.8 + 27.0), 2,306 MB | 16.8 s (9.0 + 7.8), 402 MB | 4.9 s, 68 MB | 6.4 s, 188 MB | 70% |
| quora | 522,931 | 210.8 s (93.3 + 117.5), 6,223 MB | 23.9 s (12.4 + 11.6), 185 MB | 8.2 s, 33 MB | 10.2 s, 58 MB | 89% |

The vector field is 68-89% of Strata's build. Without it Strata still takes
1.8-2.6x as long as Tantivy storing the same text on fiqa, trec-covid and quora
(fiqa 3.2 s against 1.8 s, quora 23.9 s against 10.2 s), and the same time on
the two small collections. A local profile on fiqa (4 vCPU) found the 2,000-row batches cost the
text index most of that: each `insert()` of 1,000 rows or more builds its own
segment (29 for fiqa, then a merge); one `insert()` of every row builds it in
1.35 s, Tantivy's time. With vectors, HNSW construction re-quantises each kept
neighbour (`Sq8::prepare_int8` and `roundf`, 29% of the build), then searches
(14%) and dots (9.5%). The thread "Speed up Strata's index build" (PR #226)
takes these on.

## The A3 feed: where its time goes

The feed (A3, `rrf(similarity(emb, :v), ppr(cooc, <last 3 reviewed>))`) takes
113 ms at p50 against LanceDB's 28 ms. On the same node, 2061b64, 200 probe
users:

| query | p50 | p99 |
|---|---|---|
| the feed | 113.2 ms | 180.8 ms |
| vector and filters only | 1.18 ms | 2.08 ms |
| ppr and filters only | 109.0 ms | 174.4 ms |
| ppr only | 108.0 ms | 169.8 ms |
| the history subquery | 0.07 ms | 0.13 ms |
| the feed without `MAX 2 PER brand` | 115.0 ms | 173.2 ms |

The walk is the cost. For ten of those users its plan lines show 10 to 60
pushes and 1,231 to 4,632 edges followed per query, over a graph of 3,683,756 edges, yet 52 to 146 ms. A
`perf` profile of 485 ppr-only queries (60 s, 19,973 samples):

| function | share |
|---|---|
| `GraphAdj::neighbors` (with `edge_table_neighbors` inlined) | 35.2% |
| `GraphAdj::resolve` | 13.9% |
| `PkIndex::lookup_hashed` | 9.3% |
| `PkIndex::key_ref` | 8.5% |
| `Snapshot::lookup_key` | 7.7% |
| unnamed addresses in the same library (likely hashing and key compares) | ~18% |
| `traverse::ppr` itself | 1.2% |

So almost all of it is fetching neighbour lists and resolving each neighbour's
key to a row of `products`. `traverse::ppr` fetches the whole list of every
node it pops whose residual is at least `eps`, only to learn its degree, and
most of those nodes are then not pushed (`r < eps · deg`). The pushes are few;
the lists fetched for nothing are many, and each of their edges costs a key
lookup. `dir => 'out'` halves it (64 ms). A degree read without resolving keys
(the edge table's rows and weights are already there), or a resolved-ends cache
kept across queries, would remove most of this work.

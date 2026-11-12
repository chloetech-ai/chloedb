# Reproducing the results

Every timing on these pages comes from a file committed under `chloectx/results/` or `chloedb/results/`, produced on a MacBook Air with an Apple M2. They replace an earlier set of runs on a 4-core cloud VM with an Emerald Rapids Xeon. The runs that call a model (Claude Code with and without ChloeCtx, the reader study, fact extraction and the probe sessions) were not repeated; their results are the original ones. This page describes the machine, what changed against the VM, and how to run everything.

## The machine

- **MacBook Air, Apple M2:** 8 cores (4 performance, 4 efficiency), 24 GB, macOS 26.0.1, on mains power. Rust 1.92.0; every crate builds with `-C target-cpu=native`, which on the M2 enables NEON, `dotprod` and `i8mm`.
- **One benchmark at a time.** A driver started each step only when no compiler was running and the CPU was at least 55% idle three checks in a row, and logged the idle share every 30 s. Steps ran at 75–88% idle on average (one busy core of eight is 12.5%), with the desktop apps and a QA Electron app in the background. Two development servers that had been stuck at 115–160% CPU for two days were stopped first.
- **Tests:** `cargo test --release` passes in full: ChloeDB 221 (with the posting-list suite; 14 timing tests are ignored), ChloeCtx 22.

## What changed on the laptop

| | cloud VM (Xeon, AVX-512) | MacBook Air (M2, NEON) | change |
| **Posting lists** | | | |
| Loom AND, clustered (ms/query) | 0.021 | 0.013 | 1.58× faster |
| Loom OR, clustered | 0.509 | 0.514 | 1.01× slower |
| Pyramid OR, clustered | 0.308 | 0.355 | 1.16× slower |
| Roaring AND, WikiText-103 | 0.0070 | 0.0044 | 1.61× faster |
| Entropic OR, WikiText-103 | 0.424 | 0.279 | 1.52× faster |
| PEF AND, uniform | 0.080 | 0.077 | 1.04× faster |
| **ChloeDB (median ms per query)** | | | |
| BM25 top-10, warm, TREC-COVID | 0.613 | 0.391 | 1.57× faster |
| BM25 top-10, warm, Tantivy, TREC-COVID | 1.13 | 0.681 | 1.66× faster |
| vector top-10, warm, Quora | 0.196 | 0.097 | 2.02× faster |
| hybrid (RRF), warm, Quora | 0.702 | 0.343 | 2.05× faster |
| vector top-10, cold, Quora | 43.1 | 115 | 2.67× slower |
| HNSW ef 64, Quora (vector bench) | 0.131 | 0.086 | 1.52× faster |
| filtered-vector planner, Quora, 300 queries (total ms) | 451 | 179 | 2.52× faster |
| build, Quora (s) | 243 | 153 | 1.58× faster |
| durable 1-document commit (ms) | 0.273 | 4.12 | 15.09× slower |
| WAL, 1,000-document transactions with fsync (docs/s) | 87,215 | 98,204 | 1.13× faster |
| **ChloeDB graphs (ms)** | | | |
| 2 hops both ways, arXiv, p95 | 2.59 | 1.57 | 1.65× faster |
| same, SQLite, p95 | 24.5 | 14.8 | 1.66× faster |
| PageRank ε = 1e-4, 1M rows, p50 | 5.81 | 3.49 | 1.67× faster |
| **ChloeCtx, 1M records (ms)** | | | |
| recall query p50 | 11.5 | 5.71 | 2.01× faster |
| full hook p50 | 17.2 | 26.1 | 1.52× slower |
| prompt hook with knowledge query p50 | 14.7 | 24.9 | 1.70× slower |
| hook process doing nothing p50 | 3.32 | 10.4 | 3.14× slower |
| append p50 | 0.296 | 4.82 | 16.27× slower |
| static embedder (records/s) | 9,513 | 51,481 | 5.41× faster |

*Medians unless marked. The posting-list rows are the best algorithm per codec; the ChloeDB rows come from `chloedb/results/strata/` and `chloedb/results/storage/wal.json`; the ChloeCtx rows from `chloectx/results/scale/` and `chloectx/results/latency/`. Every page quotes the M2 numbers; the VM's are in this repository's history.*

Most of the work runs 1.5–2.5× faster on the M2. Four things behave differently, and each page says where they matter:

- **NEON kernels.** The SIMD paths of the posting lists (Loom, Pyramid, Roaring, the tries and Entropic) and of ChloeDB's vector scoring were written for AVX-512 and fell back to scalar code on Apple silicon. Each now has a NEON path, checked against the scalar code in the tests; the x86 paths are unchanged. NEON has 128-bit registers and no compress or gather instructions, so some of these take a different route (Entropic decodes four blocks in scalar registers, for example), and a few stay scalar where NEON was not faster. Unions over bitmaps gained least: Loom's and Pyramid's unions on the clustered collection are at or slightly behind the Xeon's.
- **Cold cache.** ChloeDB's cold measurements evict the files before each query. Eviction used `posix_fadvise`, which macOS lacks, so on macOS it now invalidates a fresh mapping of the file (`msync(MS_INVALIDATE)`, as `vmtouch -e` does). Pages on macOS are 16 KiB, and every fault reads at least one, so random-access queries (HNSW, the key index) read about four times the data of a 4 KiB page and are slower cold than on the VM. Page counts are still reported in 4 KiB units.
- **Durable commits.** On macOS, Rust's `sync_data` is `F_FULLFSYNC`, which makes the drive flush its write cache; the VM's `fdatasync` on a virtual disk returned far sooner. A durable single-document commit takes about 4 ms instead of 0.27 ms, and a ChloeCtx append, which is one, about 4.5 ms. Batched commits are faster than on the VM.
- **Process start.** Starting a hook process takes about 10 ms on macOS against about 3 ms on the VM, so a ChloeCtx hook costs more although the search inside it is twice as fast.

Two benchmarks gained a warm-up pass. `chloedb-vector-bench` timed its recall/latency sweep right after building the index, and `chloedb-graph-bench quality` timed each method's first query cold; on Linux, fault-around hid the cost of first touching each page, on macOS it made the first settings look slower than the deeper ones. Both now run one untimed pass first, as the main ChloeDB workload always did.

## ChloeCtx retrieval, earlier on the same laptop

`chloectx-bench retrieval` with the current code, which cuts records at 400 characters (the `_c400` result files). BM25 does not depend on the embedder or the CPU, and its rows came out identical everywhere.

**Cloud VM, LoCoMo, BM25** (run with the model-free hash embedder, so only the BM25 row is comparable): R@5 0.652, R@10 0.757, nDCG@10 0.473, injection at 4,000 characters 0.770, identical to `chloectx/results/retrieval/locomo_static_request_c400.json`.

**MacBook Air, LongMemEval-S and LoCoMo, static embedder** (`--embedder static --ctx request`, 470 and 1,536 eligible questions):

| | committed (VM) | MacBook Air |
|---|---|---|
| LongMemEval-S, BM25: R@5 / R@10 / nDCG@10 / 4k pack | 0.751 / 0.851 / 0.522 / 0.768 | 0.751 / 0.851 / 0.522 / 0.768 |
| LongMemEval-S, vectors | 0.819 / 0.898 / 0.609 / 0.891 | 0.819 / 0.898 / 0.609 / 0.891 |
| LongMemEval-S, hybrid with priors | 0.849 / 0.919 / 0.649 / 0.900 | 0.849 / 0.917 / 0.648 / 0.900 |
| LoCoMo, BM25 | 0.652 / 0.757 / 0.473 / 0.770 | 0.652 / 0.757 / 0.473 / 0.770 |
| LoCoMo, vectors | 0.303 / 0.391 / 0.238 / 0.410 | 0.304 / 0.391 / 0.238 / 0.411 |
| LoCoMo, hybrid with priors | 0.443 / 0.550 / 0.350 / 0.571 | 0.450 / 0.557 / 0.353 / 0.577 |

The vector and hybrid rows move by at most 0.007, while the BM25 rows are identical. That pattern fits floating-point rounding: the static embedder and the vector scores are sums of floats, which can round differently on the M2 than on the Xeon and reorder near-tied candidates. The whole LongMemEval-S run took 138 s on the laptop against 178 s on the VM.

The headline LongMemEval-S numbers on the [ChloeCtx results](chloectx/results.md#retrieval-on-long-conversation-benchmarks) page (R@10 0.90, 87% in the 4,000-character injection) come from `longmemeval_s_static_round.json`. That file was produced before records were cut at 400 characters; today's code always cuts at 400, so the `_c400` files are the ones it reproduces.

## Statistics recomputed from committed raw results

These need no model calls or downloads, only the per-run and per-query files in the repository.

- **Claude Code with and without ChloeCtx.** `chloectx/scripts/ab_stats.py` over the held-out projects' `runs.jsonl` gives the same counts (100/100 against 60/100; 60/60 against 20/60 on conversation-only facts), the same Fisher p-values (1.34 × 10<sup>−14</sup> pooled) and the same permutation p-values (< 1/20,001). The bootstrap intervals differ in the second decimal, because the committed `stats.json` also covers the Tally runs, which shifts the resampling's random stream.
- **Does the graph help search?** `chloedb/scripts/graph_quality_stats.py` over `quality_arxiv.rows.jsonl.gz` reproduces `chloedb/results/graph/quality_stats.json` exactly, including nDCG@20 0.253 against 0.123 and its interval and p-value.

## Not re-run

The runs that call a model were not repeated: Claude Code with and without ChloeCtx (`chloectx/results/ab/`, `chloectx/results/long/`), the reader study (`chloectx/results/reader*`), fact extraction and record structure (`chloectx/results/structure/`), and the probes on the 26-hour session (`chloectx/results/probes*`), which need that session's transcript. Their counts and costs do not depend on the machine. Where a page quotes a timing from them (the 6 s back-fill of the 26-hour session, BGE's 50 ms per record, the recall step inside the kestrel runs), it says it was measured on the cloud VM.

The first long-session A/B (`chloectx/results/long/*_v1_leaky`) is kept for the record only, and its own `NOTE.md` marks it invalid: Claude Code's auto-memory was not reset between runs, so runs within a lane were not independent. The clean rerun (`chloectx/results/long/stockroom` and `slotwise`) used the isolated harness in `chloectx/scripts/long_session_ab.py`, which resets that memory before each lane's history and restores it before every task.

## Requirements

- Rust (stable; 1.92 on the laptop above) and Python 3.
- For ChloeCtx's live tests, A/B runs and distillation: the [Claude Code](https://claude.com/claude-code) CLI. Those runs call a model and cost money; retrieval, scale and latency studies do not.
- Several scripts default to data under `/home/user/data/…`; pass paths explicitly, or set `CHLOEDB_DATA` to the BEIR directory. `chloedb/scripts/run_eval.sh` uses `bash` 4 features (`declare -A`); on macOS, whose `bash` is 3.2, run its steps directly or use a newer `bash`.
- On macOS, ChloeDB's cold-cache measurements need no privileges: eviction invalidates a mapping of each file.
- Benchmarks are single-threaded unless a flag says otherwise. Run them on an otherwise idle machine.

## Posting lists

```sh
cd chloedb
cargo test --release postings                               # conformance suite for every codec, and the algorithms
scripts/postings/prepare_wikitext.sh data                   # WikiText-103 via Hugging Face (needs pyarrow)
scripts/postings/run_experiments.sh data/wikitext103.txt    # first round: every codec and algorithm
scripts/postings/run_designs.sh data/wikitext103.txt        # Pyramid, Loom and Entropic against the existing codecs
python3 scripts/postings/build_report.py                    # report/postings/report.html and designs.html
```

`chloedb/.cargo/config.toml` builds with `-C target-cpu=native`. Every SIMD kernel has an AVX-512 version, a NEON version and a portable fallback; the committed timings are from the NEON build on the M2, so x86 timings are not directly comparable with them. chloedb-server, engram and the Python package build for a generic CPU and pick ChloeDB's vector, column and posting-list kernels at run time (`chloedb::simd`; the server logs which), so their binaries run on any CPU of their architecture.

## ChloeDB

The BEIR evaluation (about 4.8 GB of data with embeddings; about 2 hours on the M2):

```sh
cd chloedb
python3 scripts/prepare_beir.py /home/user/data/beir    # SciFact, FiQA, TREC-COVID, Quora + Cohere embeddings
cargo test --release                                    # components, crash recovery, end-to-end SQL
scripts/run_eval.sh                                     # every experiment in order; or name steps, e.g. strata_small text
python3 scripts/build_report.py                         # report/strata.html
```

`run_eval.sh` builds with `--features baselines` (SQLite with FTS5 and Tantivy) and writes JSON to `chloedb/results/` and logs to `chloedb/results/logs/`.

### ChloeDB graphs

```sh
cd chloedb
# ogbn-arxiv and the papers' titles and abstracts, into /home/user/data/graph/arxiv
curl -O https://snap.stanford.edu/ogb/data/nodeproppred/arxiv.zip
curl -O https://snap.stanford.edu/ogb/data/misc/ogbn_arxiv/titleabs.tsv.gz
python3 -I scripts/prepare_arxiv.py /home/user/data/graph/arxiv
../chloectx/target/release/chloectx-bench embed --input /home/user/data/graph/arxiv/arxiv.jsonl --out /home/user/data/graph/arxiv/arxiv.f32
cargo build --release --features baselines --bin chloedb-graph-bench
target/release/chloedb-graph-bench load                  # holds out 1,000 papers of OGB's test split
target/release/chloedb-graph-bench latency --sqlite /home/user/data/graph/arxiv/edges.sqlite
target/release/chloedb-graph-bench quality --seeds 1,2,3,5,10,20,50
python3 scripts/graph_quality_stats.py results/graph/quality_arxiv.rows.jsonl.gz --out results/graph/quality_stats.json
target/release/chloedb-graph-bench synthetic --nodes 1000000 --db /tmp/synthetic.chloedb --sqlite /tmp/synthetic.sqlite --out /tmp/graph
python3 scripts/build_graph_report.py                   # report/graph.html
```

The synthetic graph needs no download. Without `--out`, results overwrite the committed files in `results/graph/`.

## ChloeCtx

```sh
cd chloectx
cargo build --release
target/release/chloectx fetch-model                        # static embedder, 129 MB
python3 scripts/download_memory_benchmarks.py --out data --max-m-bytes 0 --skip-original-s
python3 scripts/convert_memory_benchmarks.py --root data
```

The download needs `huggingface_hub` and `requests`; `--max-m-bytes 0` skips the 2.7 GB LongMemEval-M file, which no result here uses.

**Retrieval** (minutes, no model calls):

```sh
target/release/chloectx-bench retrieval --data data/longmemeval_s.jsonl --embedder static --ctx request --tag c400 --out /tmp/retrieval
target/release/chloectx-bench retrieval --data data/locomo.jsonl --embedder static --ctx request --tag c400 --out /tmp/retrieval
python3 scripts/fit_gate.py results/retrieval/longmemeval_s_static_request_c400.rows.jsonl --out /tmp/gate_static_c400.json
```

**Scale and latency** (a million records take about 3.5 GB):

```sh
target/release/chloectx-bench scale --data data/longmemeval_s.jsonl --sizes 10000,100000,1000000
target/release/chloectx-bench latency --data data/longmemeval_s.jsonl --sizes 10000,100000,1000000
```

**Probes on a real session** need a Claude Code transcript of your own; the probe files in `chloectx/probes/` were written for this repository's 26-hour session and only make sense against it:

```sh
target/release/chloectx-bench probes --transcript <session.jsonl> --probes probes/lost_probes.json --points 1952:4632,4633:5392
```

**Record structure and answers** (`chloectx-structure`, `scripts/extract_facts.py`, `scripts/reader_eval.py`) and **Claude Code with and without ChloeCtx** call a model; see the [ChloeCtx README](https://github.com/chloetech-ai/chloedb/blob/main/chloectx/README.md#evaluation) for the full commands. The A/B statistics alone need nothing but the committed runs:

```sh
python3 scripts/ab_stats.py stockroom=results/ab/stockroom/runs.jsonl slotwise=results/ab/slotwise/runs.jsonl --out /tmp/ab_stats.json
```

## The website and these pages

GitHub Pages publishes `docs/` as it is, with no build step (`docs/.nojekyll`): the root redirects to the website in `docs/site/`, whose benchmark charts read `docs/site/assets/bench-data.js` (written by `docs/site/scripts/bench_data.py`). The pages link each stylesheet and script with a hash of it (`?v=`), so a browser never pairs a new page with a stylesheet it cached; after editing anything in `docs/site/assets/`, run `docs/site/scripts/stamp_assets.py` (`bench_data.py` runs it). These Markdown pages are read on GitHub. The long-form reports in `docs/reports/` are copies of the HTML reports each crate builds.

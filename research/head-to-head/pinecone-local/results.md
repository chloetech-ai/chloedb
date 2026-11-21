# Vector search: ChloeDB against Pinecone Local

Asked directly, not by a persona of the study: did we ever measure ChloeDB against Pinecone, and can we try it locally, especially the vector search? Pinecone is a hosted service. The one part of it that runs on a machine is Pinecone Local (`ghcr.io/pinecone-io/pinecone-local`), Pinecone's in-memory emulator of its API for development and tests, which reached end of life on 7 October 2026 ([Pinecone's release notes](https://docs.pinecone.io/release-notes/2026)); its last image still pulls and runs. This page puts the two side by side on GloVe-100 from ann-benchmarks (`glove-100-angular`: word vectors of 100 dimensions), at the first 100,000 vectors (the limit per index Pinecone Local's documentation gave, which the image does not enforce) and at all 1,183,514.

Both get the same vectors, scaled to unit length so that cosine and the dot product rank alike. Pinecone Local has an index of `metric: cosine`, loaded by `POST /vectors/upsert` and asked `POST /query {vector, topK: 10}`. ChloeDB has `CREATE TABLE docs (id KEYWORD PRIMARY KEY, emb VECTOR(100))`, loaded by `POST /v1/write` and then `POST /v1/admin/optimize` (which writes the memtable to segments and merges them), and is asked `SELECT id FROM docs ORDER BY similarity(emb, :v) DESC LIMIT 10`. Both are loaded 500 vectors a request (1,000 of them as JSON pass Pinecone's 2 MB a request) from one client. Nothing is tuned on either side; ChloeDB's other `ef` values are shown beside its default.

Measured on 2026-11-21 on Intel(R) Xeon(R) Processor @ 2.10GHz, 4 cores, 15 GB, Linux 6.18.44-fc-v726; Pinecone Local v1.0.0.rc0 (image of 2025-02-27, ghcr.io/pinecone-io/pinecone-local); chloedb-server 0.3.9 (built from dd285f4e63). Both on the same machine, one at a time, over HTTP JSON from Python. Recall@10 is the share of the exact top 10 (numpy, over the same unit vectors) a system returned, over the first 1,000 test queries. Latency is each query once after 20 warm-ups, with 1 and with 4 clients. Run it again with `python3 run.py && python3 report.py` (needs Docker, numpy, h5py and a chloedb-server build; it downloads the collection, checked against [glove.sha256](glove.sha256)). Raw numbers: [raw/results-100k.json](raw/results-100k.json), [raw/results-full.json](raw/results-full.json).

## What it says

- **100,000 vectors.** Recall@10: Pinecone Local 1.0000, ChloeDB 0.9285 at its default ef. One client: Pinecone Local p50 22.1 ms, p99 29.5 ms; ChloeDB p50 1.48 ms, p99 2.18 ms (15x at the median). Four clients: Pinecone Local 164 queries/s, ChloeDB 1,378/s. Loading took 6.8 s against 14.6 s (9.6 s of writes and 5.0 s to write and merge segments with their HNSW graphs).
- **1,183,514 vectors.** Recall@10: Pinecone Local 0.9999, ChloeDB 0.8614 at its default ef. One client: Pinecone Local p50 247.9 ms, p99 294.0 ms; ChloeDB p50 1.57 ms, p99 3.27 ms (158x at the median). Four clients: Pinecone Local 13 queries/s, ChloeDB 1,304/s. Loading took 83.0 s against 220.7 s (132.8 s of writes and 87.8 s to write and merge segments with their HNSW graphs).
- **Pinecone Local scans every vector.** Its recall is exact at both sizes, and its median query grows with the collection, 22.1 ms at 100,000 and 247.9 ms at 1,183,514, where ChloeDB's HNSW goes from 1.48 ms to 1.57 ms. That is what an emulator for tests does, and it is why these numbers say nothing about hosted Pinecone's speed or recall, which runs its own index on its own machines.
- **Exact against exact.** Asked to measure every vector too (`strategy => 'flat'`), ChloeDB finds 1.0000 of the exact top 10 at 1,183,514 vectors, at p50 24.7 ms against Pinecone Local's 247.9 ms (10x), and 167 queries/s at 4 clients against 13: it scans 8-bit codes on every core and re-scores the best in float32.
- **What ChloeDB's recall costs.** GloVe is one of ann-benchmarks' hardest collections for a graph. At 1,183,514 vectors ChloeDB's default ef (200) finds 0.8614 of the exact top 10; ef 400 finds 0.9070, and ef 1600 0.9698 at p50 3.27 ms. An application that needs more recall than the default on vectors like these sets `ef` in the query.
- **Memory and disk.** At 1,183,514 vectors Pinecone Local's container held 866 MB after the run and keeps nothing on disk (a restart loses the index); ChloeDB's server held 1,007 MB and its files took 883 MB (float32 rows, 8-bit codes and the graph).
- **What this does not show.** Hosted Pinecone (serverless or pods), whose latency includes the network to its region and whose recall depends on its index; filters, namespaces, sparse or hybrid search. A hosted run, with a free-tier key and the index's host in place of the emulator's, is the next step.

## 100,000 vectors

| system | recall@10 | p50, 1 client | p95, 1 client | p99, 1 client | queries/s, 4 clients | p99, 4 clients |
|---|---|---|---|---|---|---|
| Pinecone Local, `topK: 10` | 1.0000 | 22.1 ms | 28.0 ms | 29.5 ms | 164 | 40.8 ms |
| ChloeDB `similarity(emb, :v)`, default ef (200) | 0.9285 | 1.48 ms | 1.81 ms | 2.18 ms | 1,378 | 5.17 ms |
| ChloeDB `similarity(emb, :v, ef => 32)` | 0.7655 | 1.40 ms | 1.66 ms | 1.83 ms | – | – |
| ChloeDB `similarity(emb, :v, ef => 64)` | 0.8235 | 1.32 ms | 1.54 ms | 1.64 ms | – | – |
| ChloeDB `similarity(emb, :v, ef => 128)` | 0.8944 | 1.50 ms | 1.79 ms | 2.46 ms | – | – |
| ChloeDB `similarity(emb, :v, ef => 400)` | 0.9684 | 1.80 ms | 2.14 ms | 2.52 ms | – | – |
| ChloeDB `similarity(emb, :v, ef => 800)` | 0.9890 | 2.13 ms | 2.51 ms | 4.18 ms | – | – |
| ChloeDB `similarity(emb, :v, ef => 1600)` | 0.9981 | 2.77 ms | 3.48 ms | 5.41 ms | – | – |
| ChloeDB `similarity(emb, :v, strategy => 'flat')`, exact | 1.0000 | 2.02 ms | 2.75 ms | 3.99 ms | 1,156 | 6.50 ms |

ChloeDB's own time for the default query (`took_ms`): p50 0.51 ms, p95 0.63 ms; the rest of its latency is HTTP and JSON, the same path Pinecone Local's answers take. Of the 1,000 queries, the two returned the same top 10 in the same order for 66.0%.

| loading | Pinecone Local | ChloeDB |
|---|---|---|
| writes, 500 a request | 6.8 s | 9.6 s |
| until every vector is searched as it will be | 6.8 s | 14.6 s (one segment of 100,000 after optimize) |
| memory after the run | 75 MB | 252 MB |
| disk | none (in memory) | 82 MB |

## 1,183,514 vectors

| system | recall@10 | p50, 1 client | p95, 1 client | p99, 1 client | queries/s, 4 clients | p99, 4 clients |
|---|---|---|---|---|---|---|
| Pinecone Local, `topK: 10` | 0.9999 | 247.9 ms | 275.2 ms | 294.0 ms | 13 | 463.0 ms |
| ChloeDB `similarity(emb, :v)`, default ef (200) | 0.8614 | 1.57 ms | 1.92 ms | 3.27 ms | 1,304 | 5.79 ms |
| ChloeDB `similarity(emb, :v, ef => 32)` | 0.6955 | 1.33 ms | 1.57 ms | 2.62 ms | – | – |
| ChloeDB `similarity(emb, :v, ef => 64)` | 0.7543 | 1.38 ms | 1.68 ms | 1.97 ms | – | – |
| ChloeDB `similarity(emb, :v, ef => 128)` | 0.8271 | 1.48 ms | 1.75 ms | 2.41 ms | – | – |
| ChloeDB `similarity(emb, :v, ef => 400)` | 0.9070 | 1.83 ms | 2.13 ms | 2.38 ms | – | – |
| ChloeDB `similarity(emb, :v, ef => 800)` | 0.9421 | 2.32 ms | 2.76 ms | 3.40 ms | – | – |
| ChloeDB `similarity(emb, :v, ef => 1600)` | 0.9698 | 3.27 ms | 4.16 ms | 4.95 ms | – | – |
| ChloeDB `similarity(emb, :v, strategy => 'flat')`, exact | 1.0000 | 24.7 ms | 32.5 ms | 35.2 ms | 167 | 37.1 ms |

ChloeDB's own time for the default query (`took_ms`): p50 0.60 ms, p95 0.78 ms; the rest of its latency is HTTP and JSON, the same path Pinecone Local's answers take. Of the 1,000 queries, the two returned the same top 10 in the same order for 55.0%.

| loading | Pinecone Local | ChloeDB |
|---|---|---|
| writes, 500 a request | 83.0 s | 132.8 s |
| until every vector is searched as it will be | 83.0 s | 220.7 s (one segment of 1,183,514 after optimize) |
| memory after the run | 866 MB | 1,007 MB |
| disk | none (in memory) | 883 MB |

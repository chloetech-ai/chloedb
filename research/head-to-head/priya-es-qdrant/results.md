# Priya: hybrid retrieval, ChloeDB against Elasticsearch plus Qdrant

Priya, an ML engineer in the persona study, runs RAG retrieval as Elasticsearch for BM25 and Qdrant for vectors, with bge-small-en-v1.5 vectors she computes herself, fused by reciprocal rank fusion in her service. Her tech lead asks for "a head-to-head on our judged set against Elasticsearch plus Qdrant: same relevance or better, p95 no worse, fewer machines". This page runs it on two public judged sets: BEIR SciFact (5,183 passages, 300 test questions); BEIR FiQA-2018 (57,638 passages, 648 test questions). Every passage and question is embedded once with fastembed's bge-small-en-v1.5, and both sides get the same vectors.

Elasticsearch has the passages (`english` analyzer, one shard), Qdrant the vectors (cosine, its default HNSW), and the service asks both at once and fuses their top 100 by RRF with k = 60. ChloeDB has the passages and vectors in one table, `CREATE TABLE docs (id KEYWORD PRIMARY KEY, body TEXT, emb VECTOR(384))`, and answers `SELECT id FROM docs ORDER BY rrf(bm25(body, :q, syntax => 'plain'), similarity(emb, :v)) DESC LIMIT 100`, its defaults also k = 60 and the top 100 of each. Nothing is tuned on either side.

Measured on 2026-11-20 on Intel(R) Xeon(R) Processor @ 2.10GHz, 4 cores, 15 GB, Linux 6.18.44-fc-v726; Elasticsearch 9.5.5 (Docker mirror.gcr.io/library/elasticsearch:9.5.5, heap 1g); Qdrant 1.20.0 (Docker mirror.gcr.io/qdrant/qdrant:v1.20.0); chloedb-server 0.3.9 (built from 2f21f4dd0c). All three on the same machine, one system at a time, each loaded over HTTP from one client and asked over HTTP. Latency is each test question once after 20 warm-ups, with 1 and with 4 clients; it leaves out embedding the question, which costs the same on both sides. Run it again with `python3 run.py && python3 report.py` (needs Docker, `pip install -r ../requirements.txt` and a chloedb-server build; it downloads the collections, pinned by hash, and the model). Raw numbers: [raw/results-scifact.json](raw/results-scifact.json), [raw/results-fiqa.json](raw/results-fiqa.json).

## What it says

- **SciFact.** Hybrid nDCG@10: ChloeDB 0.7373, Elasticsearch + Qdrant 0.7300; the difference +0.0074 (95% interval -0.0051 to +0.0198), not distinguishable. Recall@100 0.9650 against 0.9583. One client: ChloeDB p50 2.6 ms, p95 3.1; Elasticsearch + Qdrant (asked at once) p50 6.9, p95 8.9. Four clients: ChloeDB 772 questions/s at p95 8.4 ms, Elasticsearch + Qdrant 264/s at p95 22.7 ms.
- **FiQA-2018.** Hybrid nDCG@10: ChloeDB 0.3618, Elasticsearch + Qdrant 0.3634; the difference -0.0015 (95% interval -0.0100 to +0.0074), not distinguishable. Recall@100 0.6978 against 0.6856. One client: ChloeDB p50 3.4 ms, p95 3.9; Elasticsearch + Qdrant (asked at once) p50 6.5, p95 8.2. Four clients: ChloeDB 643 questions/s at p95 9.4 ms, Elasticsearch + Qdrant 274/s at p95 21.5 ms.
- **Where the difference comes from.** BM25 alone, ChloeDB against Elasticsearch's `multi_match` on FiQA-2018: -0.0003 nDCG@10 (not distinguishable); against the same text in one Elasticsearch field: -0.0003 (not distinguishable). Vectors alone, ChloeDB against Qdrant: +0.0060 (better); of the exact top 10, Qdrant found 0.983 and ChloeDB 0.999.
- **On FiQA-2018 fusing hurts on both sides.** BM25 is weak there (0.2533), and RRF at equal weights ranks below the vectors alone: ChloeDB 0.3618 against 0.3848, Elasticsearch + Qdrant 0.3634 against 0.3788. ChloeDB's `rrf` takes `weights => [...]` for that in the same statement; this page tunes neither side.
- **One system instead of two.** At 57,638 passages ChloeDB's server held 312 MB after the run, against 1,559 MB for Elasticsearch (heap 1g) and 130 MB for Qdrant; its files took 172 MB, against 63 MB and 294 MB. Loading took 18.6 s, against 8.5 s and 15.6 s (Qdrant's until its index was built). There is no sync job to keep the two stores in step and no fusion code in the service.
- **What this does not show.** Both sets fit one machine many times over; Priya's service has millions of chunks on replicas. Her own judged set and her size are the next step, which this script runs on any BEIR-layout collection.

## SciFact: each ranking

5,183 passages, 300 test questions with 339 judgements.

| ranking | nDCG@10 | recall@100 | p50, 1 client | p95, 1 client | questions/s, 4 clients | p95, 4 clients |
|---|---|---|---|---|---|---|
| Elasticsearch + Qdrant, RRF in Python (k 60, top 100 of each) | 0.7300 | 0.9583 | 6.9 ms | 8.9 ms | 264 | 22.7 ms |
| ChloeDB `rrf(bm25(body, :q), similarity(emb, :v))` | 0.7373 | 0.9650 | 2.6 ms | 3.1 ms | 772 | 8.4 ms |
| Elasticsearch BM25, `multi_match` on title and text (BEIR's query) | 0.6906 | 0.9192 | 5.8 ms | 7.4 ms | 371 | 17.1 ms |
| Elasticsearch BM25, `match` on title and text in one field | 0.6826 | 0.9282 | – | – | – | – |
| ChloeDB `bm25(body, :q)` | 0.6910 | 0.9216 | 2.3 ms | 2.8 ms | 906 | 7.0 ms |
| Qdrant, bge-small vectors, its default HNSW | 0.7201 | 0.9533 | 2.2 ms | 2.6 ms | 747 | 8.2 ms |
| ChloeDB `similarity(emb, :v)` | 0.7201 | 0.9533 | 2.1 ms | 2.6 ms | 910 | 6.9 ms |
| Exact vector search (numpy), for reference | 0.7201 | 0.9533 | – | – | – | – |

Elasticsearch + Qdrant asked one after the other instead of at once: p50 8.3 ms, p95 10.0 ms. ChloeDB's own time for the hybrid (`took_ms`): p50 1.17 ms, p95 1.47 ms.

| difference | nDCG@10 (95% interval) | questions better / worse | recall@100 (95% interval) |
|---|---|---|---|
| ChloeDB `rrf(bm25(body, :q), similarity(emb, :v))` minus Elasticsearch + Qdrant, RRF in Python | +0.0074 (-0.0051 to +0.0198) | 32 / 29 | +0.0067 (+0.0000 to +0.0167) |
| ChloeDB `bm25(body, :q)` minus Elasticsearch BM25, `multi_match` on title and text | +0.0003 (-0.0144 to +0.0162) | 36 / 38 | +0.0023 (-0.0150 to +0.0200) |
| ChloeDB `bm25(body, :q)` minus Elasticsearch BM25, `match` on title and text in one field | +0.0084 (-0.0010 to +0.0185) | 21 / 15 | -0.0067 (-0.0167 to +0.0000) |
| ChloeDB `similarity(emb, :v)` minus Qdrant, bge-small vectors, its default HNSW | +0.0000 (+0.0000 to +0.0000) | 0 / 0 | +0.0000 (+0.0000 to +0.0000) |

## FiQA-2018: each ranking

57,638 passages, 648 test questions with 1,706 judgements.

| ranking | nDCG@10 | recall@100 | p50, 1 client | p95, 1 client | questions/s, 4 clients | p95, 4 clients |
|---|---|---|---|---|---|---|
| Elasticsearch + Qdrant, RRF in Python (k 60, top 100 of each) | 0.3634 | 0.6856 | 6.5 ms | 8.2 ms | 274 | 21.5 ms |
| ChloeDB `rrf(bm25(body, :q), similarity(emb, :v))` | 0.3618 | 0.6978 | 3.4 ms | 3.9 ms | 643 | 9.4 ms |
| Elasticsearch BM25, `multi_match` on title and text (BEIR's query) | 0.2536 | 0.5489 | 5.8 ms | 7.5 ms | 400 | 16.1 ms |
| Elasticsearch BM25, `match` on title and text in one field | 0.2536 | 0.5489 | – | – | – | – |
| ChloeDB `bm25(body, :q)` | 0.2533 | 0.5551 | 2.4 ms | 3.0 ms | 751 | 8.2 ms |
| Qdrant, bge-small vectors, its default HNSW | 0.3788 | 0.6730 | 2.5 ms | 3.0 ms | 729 | 8.5 ms |
| ChloeDB `similarity(emb, :v)` | 0.3848 | 0.6861 | 2.8 ms | 3.7 ms | 680 | 8.8 ms |
| Exact vector search (numpy), for reference | 0.3848 | 0.6866 | – | – | – | – |

Elasticsearch + Qdrant asked one after the other instead of at once: p50 7.9 ms, p95 9.5 ms. ChloeDB's own time for the hybrid (`took_ms`): p50 1.77 ms, p95 2.18 ms.

FiQA-2018's passages have no titles, so the two Elasticsearch queries rank alike.

| difference | nDCG@10 (95% interval) | questions better / worse | recall@100 (95% interval) |
|---|---|---|---|
| ChloeDB `rrf(bm25(body, :q), similarity(emb, :v))` minus Elasticsearch + Qdrant, RRF in Python | -0.0015 (-0.0100 to +0.0074) | 106 / 112 | +0.0122 (+0.0015 to +0.0235) |
| ChloeDB `bm25(body, :q)` minus Elasticsearch BM25, `multi_match` on title and text | -0.0003 (-0.0093 to +0.0089) | 103 / 93 | +0.0062 (-0.0055 to +0.0180) |
| ChloeDB `bm25(body, :q)` minus Elasticsearch BM25, `match` on title and text in one field | -0.0003 (-0.0093 to +0.0089) | 103 / 93 | +0.0062 (-0.0055 to +0.0180) |
| ChloeDB `similarity(emb, :v)` minus Qdrant, bge-small vectors, its default HNSW | +0.0060 (+0.0012 to +0.0119) | 8 / 5 | +0.0131 (+0.0023 to +0.0246) |

## Loading, memory and disk

| | SciFact | FiQA-2018 |
|---|---|---|
| Elasticsearch: `_bulk` load | 2.7 s | 8.5 s |
| Qdrant: upsert until indexed | 1.5 s | 15.6 s |
| ChloeDB: `/v1/write` load | 1.6 s | 18.6 s |
| Elasticsearch: memory after the run | 1,496 MB | 1,559 MB |
| Qdrant: memory after the run | 36 MB | 130 MB |
| ChloeDB: memory after the run | 178 MB | 312 MB |
| Elasticsearch: disk | 10 MB | 63 MB |
| Qdrant: disk | 206 MB | 294 MB |
| ChloeDB: disk | 19 MB | 172 MB |
| embedding the passages (fastembed, both sides) | 619 s | 7,825 s |

Elasticsearch's and Qdrant's memory is their containers' (`docker stats`); Elasticsearch's is mostly the heap it was given, which it takes at start. ChloeDB's is the server's proportional set size. Elasticsearch's disk is its store after a force merge to one segment, Qdrant's its storage folder, ChloeDB's its database files after a vacuum.

## Setup, step by step

Elasticsearch + Qdrant: run Elasticsearch; create the index with its mapping; run Qdrant; create the collection; embed the passages; `_bulk` the passages into Elasticsearch; upsert the vectors into Qdrant with an integer id and the passage's id as payload (Qdrant takes integers or UUIDs); wait for Qdrant's index; in the service, ask both, map Qdrant's ids back and fuse; and keep a job that writes every change to both. Nine steps and two systems to run.

ChloeDB: run chloedb-server; `CREATE TABLE`; embed the passages; write the rows with their vectors; ask one SQL query. Five steps and one system.

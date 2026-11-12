## Use 2: chloedb-server as a shared KV-cache tier (Qwen2.5-1.5B, bf16, 4 CPU cores)

| prefix tokens | cache size | prefill + first token (cold) | from chloedb-server: lookup + fetch + rebuild + first token | from a local file | write to chloedb-server | same answer |
|---|---|---|---|---|---|---|
| 2048 | 56 MiB | 6.9 s | 0.64 s (0.005 + 0.27 + 0.07 + 0.29) | 0.42 s | 1.3 s | yes (48213) |
| 8192 | 224 MiB | 32.5 s | 1.46 s (0.005 + 0.86 + 0.13 + 0.47) | 0.82 s | 5.0 s | yes (48213) |
| 16384 | 448 MiB | 72.9 s | 2.61 s (0.005 + 1.59 + 0.46 + 0.55) | 1.50 s | 11.4 s | yes (48213) |
| 32768 | 896 MiB | 172.1 s | 5.79 s (0.005 + 3.92 + 0.95 + 0.92) | 3.43 s | 26.8 s | yes (48213) |

## Use 3: project memory on a local model (17982 tokens of ChloeCtx docs in 49 chunks, 26 questions)

One-off costs: prefill of the corpus 87 s (492 MiB of KV); caches of every chunk 64 s; the cluster index of the corpus 32 s.

| condition | right | mean seconds per answer (incl. decoding) |
|---|---|---|
| no context | 1/26 | 1.1 |
| whole corpus, stored cache, dense | 23/26 | 5.1 |
| whole corpus, stored cache, sparse (6%) | 21/26 | 6.4 |
| ChloeDB BM25 top 4 chunks, text prefilled | 23/26 | 6.7 |
| ChloeDB BM25 top 4 chunks, stored chunk caches | 20/26 | 1.3 |

## Use 1: multi-document QA from stored caches (LongBench-E HotpotQA + 2WikiMQA, 40 questions, mean 5155 tokens, 10 passages each)

| condition | F1 | exact match | time to answer |
|---|---|---|---|
| no passages | 23.9 | 17.5 | 1.0 s |
| all passages prefilled together (re-read every question) | 55.7 | 42.5 | 19.2 s |
| all passages together, sparse (6%) | 50.6 | 32.5 | 21.6 s |
| stored per-passage caches stitched | 37.0 | 27.5 | 1.6 s |
| stitched caches, sparse (6%) | 33.6 | 20.0 | 3.8 s |
| ChloeDB BM25 top 3, text prefilled | 42.8 | 30.0 | 5.7 s |
| ChloeDB BM25 top 3, stored caches | 38.9 | 27.5 | 1.1 s |

hotpotqa_e (20): closed 17.8, full 44.4, full_sparse 43.5, stitch 34.7, stitch_sparse 36.5, bm25_text 36.1, bm25_stitch 37.0
2wikimqa_e (20): closed 30.0, full 67.0, full_sparse 57.8, stitch 39.3, stitch_sparse 30.7, bm25_text 49.6, bm25_stitch 40.8

Per question, building the passage caches took 18.6 s once (vs. 18.0 s for the joint prefill); loading them 0.10 s and stitching 0.12 s; the cluster index of a stitched prefix 2.0 s; 140 MiB of caches per question.

Stitched minus joint F1, per question: mean -18.6, 4 better, 13 worse, 23 the same.

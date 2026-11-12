# Block-max retrieval attention over a KV cache

A research prototype, not part of any crate. Question: attention over a long
KV cache is a top-k inner-product search (each query wants the few keys with
the largest `q·k`), so can ChloeDB's top-k machinery (block upper bounds and
skipping as in block-max WAND, segment layout and reordering, RaBitQ codes with
error bounds) serve a KV cache? This measures it on a small model's real
attention.

## Setup

- **Model**: Qwen2.5-0.5B-Instruct (24 layers, 14 query heads sharing 2 KV
  heads, head dim 64, bf16 weights), on CPU. One check on Qwen3-0.6B (28
  layers, 16 query heads over 8 KV heads, head dim 128).
- **Text**: PG19 test books (`emozilla/pg19-test`), 1000 tokens past the
  start to skip the Gutenberg header.
- **Protocol**: an `L`-token prefix is prefilled with dense attention into the
  KV cache (the cache a store would hold). The next chunk (256 tokens of the
  book, or a question about the prefix) is then run with **sparse attention in
  layers 2 to 23** (layers 0 and 1 dense, as Quest does): each query, per KV
  head, reads the first page (the attention sink, 16 tokens) plus `budget-16`
  prefix tokens chosen by the method, and the new chunk itself. A KV head is
  read once for its 7 query heads, so selection is per KV head (query-head
  scores max-pooled).
- **Measured**: attention mass kept on what was read (recall); relative error
  of the attention output; NLL of the next 256 tokens (mean minus log
  probability; dense is about 3.05, perplexity 21; +0.03 NLL is 3% more
  perplexity); passkey retrieval (a 5-digit key hidden at a depth of the
  prefix, asked after it, scored by greedy decoding under teacher forcing;
  dense gets 6/6 at 8k and 16k, 3/4 at 32k); and, for exact top-k, the
  fraction of the prefix a block-max search must read so that no skipped block
  could hold a true top-k key.
- The harness was checked against plain `transformers` (NLL 2.805 vs 2.813 on
  the same text; the difference is fp32 vs bf16 attention).

Methods (`kvsel.py`). All of them keep metadata of the same size, about 16
bytes per token per KV head next to 256 bytes of K and V (6%):

| method | metadata | what is read |
|---|---|---|
| `window` | none | sink + the most recent pages (StreamingLLM) |
| `quest` | per 16-token page, the min and max of every channel | pages by the box bound `Σ max(q_i·m_i, q_i·M_i)`: the block-max bound of an inner product (Quest, Tang et al. 2024) |
| `cluster_box` | keys regrouped by k-means into as many clusters as pages, min/max per cluster | whole clusters by the same box bound |
| `cluster_cent` | the same clusters, one centroid each | whole clusters by `q·centroid` (ClusterKV, RetrievalAttention-like) |
| `rabitq` | RaBitQ 1-bit code per token (8 bytes) + 2 floats, as `chloedb/src/vector/rabitq.rs` | the top tokens by RaBitQ's estimate |
| `rabitq_page` | the same codes | positional pages by the max estimate in the page |
| `oracle_page` | (needs the dense scores) | pages by true attention mass: the ceiling for any page selector |
| `oracle_token` | (needs the dense scores) | tokens by true score: the ceiling for any token selector |

## Results

Run on a cloud container: 4 cores of an Intel Xeon @ 2.1 GHz with AMX, 15 GB, CPU only,
PyTorch 2.15, transformers 5.22. 8k: 3 books; 16k: 2 books; 32k: 1 book.
Full tables, per layer and per head, in [results/summary.md](results/summary.md)
(`python analyze.py results/*.json`).

### 1. Exact block-max skipping does not carry over

Fraction of the prefix that must be read for exact top-16 (no skipped block's
bound below the 16th best true score; the true score is given, so this is the
best case for pruning):

| prefix | Quest page bounds | perfect page bounds (true page max) | clustered boxes | RaBitQ + error bound (tokens rescored; top-k missed) |
|---|---|---|---|---|
| 8k | 98.4% | 2.3% | 95.0% | 55.0% (0.04%) |
| 16k | 96.9% | 1.1% | 92.3% | 48.6% (0.03%) |
| 32k | 93.9% | 0.6% | 88.2% | 44.1% (0.03%) |

The true top keys are concentrated in a few pages (perfect bounds would skip
98 to 99% of them), but the box bound over 16 keys in 64 dimensions is so loose
that almost no page can be ruled out: the bound adds up the worst case of every
channel independently, and RoPE spreads the keys of a page apart. Regrouping by
content (k-means) shrinks the boxes a little, not enough. The WAND guarantee,
which is what makes block-max useful for text, buys nothing here.

### 2. As an approximate ranking, content beats position

NLL increase over dense, by tokens read per KV head (and % of the prefix):

| | 8k, 512 (6.2%) | 8k, 1024 (12.5%) | 16k, 1024 (6.2%) | 16k, 2048 (12.5%) | 32k, 2048 (6.2%) | 32k, 4096 (12.5%) |
|---|---|---|---|---|---|---|
| window | +0.172 | +0.132 | +0.164 | +0.092 | +0.101 | +0.050 |
| quest | +0.105 | +0.069 | +0.068 | +0.036 | +0.107 | +0.062 |
| cluster_box | +0.126 | +0.078 | +0.201 | +0.172 | +0.183 | +0.154 |
| **cluster_cent** | **+0.038** | **+0.017** | **+0.032** | **+0.012** | **+0.038** | **+0.011** |
| rabitq | +0.023 | +0.008 | +0.046 | +0.024 | +0.066 | +0.037 |
| rabitq_page | +0.078 | +0.041 | +0.071 | +0.036 | +0.093 | +0.071 |
| oracle_page | +0.015 | +0.002 | +0.029 | +0.015 | +0.014 | +0.006 |
| oracle_token | +0.015 | +0.006 | +0.027 | +0.014 | +0.010 | +0.006 |

Passkey retrieval, trials right (dense: all):

| | 8k, 128 | 8k, 256 | 8k, 512 | 16k, 128 | 16k, 256 | 16k, 512 | 32k, 256 | 32k, 1024 | 32k, 4096 |
|---|---|---|---|---|---|---|---|---|---|
| dense | 6/6 | 6/6 | 6/6 | 6/6 | 6/6 | 6/6 | 3/4 | 3/4 | 3/4 |
| window | 0/6 | 0/6 | 0/6 | 0/6 | 0/6 | 0/6 | 0/4 | 0/4 | 1/4 |
| quest | 5/6 | 6/6 | 6/6 | 1/6 | 5/6 | 6/6 | 0/4 | 1/4 | 3/4 |
| cluster_box | 1/6 | 4/6 | 5/6 | 0/6 | 0/6 | 3/6 | 0/4 | 3/4 | 4/4 |
| **cluster_cent** | **6/6** | **6/6** | **6/6** | **6/6** | **6/6** | **6/6** | **3/4** | **4/4** | **3/4** |
| rabitq | 4/6 | 6/6 | 6/6 | 3/6 | 5/6 | 6/6 | 0/4 | 1/4 | 2/4 |
| rabitq_page | 4/6 | 5/6 | 5/6 | 1/6 | 2/6 | 3/6 | 0/4 | 0/4 | 3/4 |
| oracle_page | 6/6 | 6/6 | 6/6 | 6/6 | 6/6 | 6/6 | 3/4 | 3/4 | 3/4 |
| oracle_token | 6/6 | 6/6 | 6/6 | 6/6 | 6/6 | 6/6 | 4/4 | 3/4 | 3/4 |

(Read as tokens per KV head: 256 of a 32k prefix is 0.8%. The 32k passkey is
one haystack at four depths, and the dense model misses one of them.)

- Quest works as a ranking at 8k and 16k but is no better than a sliding
  window on book text at 32k (one book), and needs 512 tokens to find a
  passkey that the window never finds.
- Grouping keys by content and scoring each group by its centroid is the best
  method that does not peek at the dense scores, close to the page oracle at
  6% of the prefix. It is a ranking by an *estimate*, not a bound.
- RaBitQ's 1-bit token estimates are as good at 8k and fall behind with
  length: one bit per dimension separates the top keys from 32k competitors
  less well.
- Taking the max of noisy estimates over a page (`rabitq_page`) picks noise.

### 3. A second model: Qwen3-0.6B (8 KV heads, head dim 128), 8k, 2 books

| | 256 (3.1%) | 512 (6.2%) | 1024 (12.5%) |
|---|---|---|---|
| window | +0.195 | +0.159 | +0.144 |
| quest | +0.027 | +0.027 | -0.023 |
| cluster_cent | -0.034 | -0.048 | -0.034 |
| rabitq | -0.016 | -0.040 | -0.036 |
| oracle_page | +0.001 | -0.016 | -0.022 |

Exact top-16: Quest bounds must read 99.5% of the prefix, clustered boxes
98.6%, perfect page bounds 2.3%; RaBitQ with its error bound rescores 41%
(it gets better with the dimension, as its error shrinks like `1/√d`; the box
bound gets worse). Reading a few percent of the prefix scores slightly *better*
than dense attention on these two books: a 0.6B model's attention over 8k
tokens is diluted, and cutting the long tail helps it a little. Take it as "no
loss", not as a gain.

## Verdict for ChloeDB

Write-up: https://claude.ai/artifact/HLY4CtwTmBA1AyKM9kfdjW (private to the project). In short: the parts of ChloeDB that
apply are its quantised scans with rescoring (RaBitQ codes and kernels), its
IVF-style clustering of a segment, and its segment layout (reordering rows so
neighbours are alike), not block-max WAND, because inner-product box bounds on
keys are too loose to skip safely.

## Reproduce

```bash
pip install torch transformers pandas huggingface_hub
python -c "from huggingface_hub import hf_hub_download as d; d('emozilla/pg19-test', 'data/test-00000-of-00001-29a571947c0b5ccc.parquet', repo_type='dataset', local_dir='data')"
./queue.sh   # about 3.5 hours on 4 cores
python analyze.py results/ppl_8k.json results/ppl_16k.json results/ppl_32k.json results/passkey_*.json results/qwen3_ppl_8k.json > results/summary.md
```

## Three uses, measured (Qwen2.5-1.5B-Instruct)

LongBench comes from `THUDM/LongBench`'s `data.zip` on Hugging Face, unzipped
into `longbench/`. Results in [results/uses.md](results/uses.md)
(`python analyze_uses.py`), from `queue_uses.sh` against a local chloedb-server started with
`--media-store DIR --media-max-bytes 100000000 --max-body-mb 512`.

- `run_tier.py` (use 2): one process prefills a prefix and writes its KV cache
  to chloedb-server, a row per layer with the layer's K and V in a `MEDIA`
  field; another process looks the rows up by the prefix's hash, fetches the
  bytes from `/v1/media/<sha256>` and answers. At 32k tokens: 172 s cold,
  5.8 s from the server, 3.4 s from a local file, the same answer.
- `run_ctx.py` (use 3): ChloeCtx's docs (18k tokens) as one stored cache, read
  densely or sparsely, against ChloeDB BM25 retrieval of chunks, as text or as
  stored chunk caches; 26 questions in `ctx/questions.json`. Stored cache
  23/26, BM25 text 23/26, no context 1/26.
- `run_rag.py` (use 1): LongBench-E HotpotQA and 2WikiMQA, 40 questions, each
  passage's cache computed once (after the shared instruction) and stitched
  with its keys re-rotated to their new positions (`kvrag.py`). Stitching
  loses 19 F1 against a joint prefill (37.0 vs 55.7) and 6 against BM25 text
  retrieval: passages cached alone never attended to each other.

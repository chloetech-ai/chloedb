# ChloeDB inside a language model: prototypes (20 Nov 2026)

Prototypes of ChloeDB in a model's inference loop and in its weight space,
run on Qwen3 0.6B, 1.7B and 4B on a 4-core cloud machine (CPU only), against
ChloeDB 0.3.9 (the server for the HN and doc-chunk tables, the embedded
Python package `chloedb` inside the model's process for every row store).
The write-up, with the numbers, is the project doc "Embedding ChloeDB inside
an LLM"; the raw results are in `results/`.

| script | idea | what it measures |
|---|---|---|
| `setup.py` | | loads HN stories of July 2026 and ChloeDB's own docs (120-word chunks) into a server on :7700 |
| `grammar.py`, `proto_a_sql.py`, `summarize_a.py` | SQL under a grammar | a GBNF grammar made from the live schema; 36 questions, free vs constrained |
| `proto_bc_collect.py`, `proto_bc_analyze.py` | retrieval while reading; a reranker from the model's feedback | perplexity with BM25 chunks once or every 64 tokens; LightGBM on each candidate's effect |
| `proto_d_knnlm.py` | W3: hidden states as rows (kNN-LM) | perplexity; INSERT of a doc's rest, then DELETE |
| `proto_e_edits.py` | W2: facts written into a layer as rows | facts, rewordings, unrelated prompts, DELETE of half |
| `proto_f_bigger.py` | bar | 1.7B and 4B with no rows on the same tokens |
| `common_hf.py`, `proto_w56_adapters.py` | W5: LoRA rows; W6: router as one query with WHERE tenant / as-of | perplexity per arm, routing accuracy |
| `proto_w4_soft.py` | W4: chunks as 4 soft tokens | perplexity, with wrong-chunk and mean-vector controls |

Each script reads `S`, a scratch directory holding `models/` (the GGUF files
from unsloth/Qwen3-*-GGUF and `qwen3-0.6b-hf` from Qwen/Qwen3-0.6B) and
`data/` (open-index/hacker-news, `data/2026/2026-07.parquet`).

Found on the way: the embedded package stores vectors as given and
`similarity()` is their raw dot product, while the docs say every vector is
normalized (`proto_e_edits.py` normalizes its own).

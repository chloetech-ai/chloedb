# Fast CPU encoders for ChloeDB

A study of fastText-style ways to embed text on a CPU, and of what ChloeDB
can take from them: static (lookup) models, ModernBERT run cheaply,
static students in a transformer's space, and learned sparse retrieval run
by ChloeDB's inverted index. The findings and the recommendation are in
the write-up shared in the project thread; this folder holds the code that
measured them, so the numbers can be reproduced.

Measured on a 4-core cloud Intel Xeon at 2.1 GHz with AVX-512 VNNI and AMX,
15 GB, Linux, 2026-11-12, on the BEIR test splits of SciFact, NFCorpus and
ArguAna (`chloedb/tests/fixtures/beir/fetch.sh`). Quality does not depend
on the machine; the speeds do.

## Setup

```bash
chloedb/tests/fixtures/beir/fetch.sh ~/beir
python3 -m venv ~/venv/ml && . ~/venv/ml/bin/activate
pip install torch --index-url https://download.pytorch.org/whl/cpu
pip install transformers sentence-transformers model2vec onnxruntime onnx numpy scipy scikit-learn faiss-cpu safetensors
export HF_HOME=~/hf      # models are fetched from Hugging Face into it
```

`common.py` finds a model under `$HF_HOME/hub` (download each repository
once with `huggingface_hub.snapshot_download`); embeddings are cached in
`~/enc-cache` (`ENC_CACHE`), so a later script reuses an earlier one's
vectors. `DTYPE=bfloat16` (the default) runs PyTorch in bf16, which AMX
runs 2.5 times faster than float32 with the same vectors (cosine 1.000);
`DTYPE=float32` for a CPU without it.

## Scripts, in the order they ran

| script | what it measures |
|---|---|
| `baselines.py MODEL` | nDCG@10 and recall@100 of a model on the three collections (exact cosine search; documents are title + newline + text, as `chloedb-embed-eval`'s `emb_both`); `MAXLEN` cuts texts; `onnx:DIR:FILE` runs an ONNX export |
| `static_compress.py` | the Model2Vec table in float16, int8, PCA and product quantisation (fastText.zip) |
| `quantize_static.py SRC DST int8` | writes an int8 (or f16) Model2Vec model, which ChloeDB's StaticEmbedder reads |
| `distill_static.py REPO OUT` | Model2Vec's distillation of a transformer, without PCA (the student starts in the teacher's space) |
| `prep_corpus.py`, `teacher_embed.py` | 40K C4 passages and 40K short spans, and the teacher's vectors of them |
| `student.py` | trains a static student toward the teacher (cosine), optionally with hashed bigram buckets (`--bigrams`), the collections' own documents (`--domain`) and potion projected into the teacher's space (`--fit-projection`); reports the student on both sides and the asymmetric pairs |
| `cascade.py STUDENT` | progressive upgrade (a share of documents re-embedded by the teacher) and student-then-teacher cascades |
| `onnx_export.py REPO OUT`, `onnx_encode.py` | ONNX export of an encoder, its int8 copy (dynamic quantisation), and an encoder over it |
| `sparse.py --export DIR` | inference-free learned sparse retrieval (OpenSearch doc-v3-distill) and the static sparse expansion; writes the term weights for ChloeDB |
| `static_sparse_sweep.py` | the static expansion pruned |
| `speed.py` | documents a second and one query's latency of each encoder, at 1 and 4 threads, nothing else running |

ChloeDB's side (from `chloedb/`):

```bash
cargo run --profile ci --example static_tables -- ~/beir/scifact/corpus.jsonl F32_MODEL_DIR INT8_MODEL_DIR
cargo run --profile ci --example sparse_eval -- DIR scifact nfcorpus arguana   # DIR from sparse.py --export
cargo run --profile ci --features onnx --example embed_beir -- ONNX_MODEL_DIR ~/beir/scifact
```

An ONNX model directory for `OnnxEmbedder` holds `model.onnx` (the int8
export renamed), `tokenizer.json`, `config.json` and, from the
sentence-transformers repository, `1_Pooling/config.json` (and
`sentence_bert_config.json` to cut texts shorter than 512 tokens).

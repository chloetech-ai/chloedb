# PACK n TOKENS, measured

Two measurements of `PACK n TOKENS` (`chloedb/src/query/pack.rs`), made on 13 Nov 2026 on a cloud container: Intel Xeon @ 2.30 GHz, 4 vCPUs, Linux, `--release`.

## Against a top k cut in the client

`chloedb-pack-bench` ([pack_vs_top_k.json](pack_vs_top_k.json)) ranks each judged test query of SciFact and NFCorpus (BEIR) by `bm25(body, :q)` and fills a budget three ways: `top_k_trim`, the top rows whole in order while they fit, then the first that does not cut to what is left (what an application does with `LIMIT k`); `pack`, `PACK n TOKENS` with its default row cap of a quarter of the budget; `pack_dedupe`, with `DEDUPE 0.85`. Recall is the share of a query's relevant documents the context holds some of; "relevant tokens" is the share of the budget spent on them.

| collection | budget | recall, top k | recall, PACK | relevant tokens, top k | relevant tokens, PACK |
|---|---|---|---|---|---|
| SciFact (364 tokens a document) | 500 | 0.647 | **0.741** | **0.432** | 0.197 |
| | 1,000 | 0.723 | **0.741** | **0.287** | 0.193 |
| | 2,000 | 0.766 | 0.763 | 0.163 | 0.146 |
| | 4,000 | 0.827 | 0.827 | 0.091 | 0.090 |
| NFCorpus (393 tokens a document) | 500 | 0.084 | **0.119** | **0.414** | 0.331 |
| | 1,000 | 0.112 | **0.121** | **0.377** | 0.329 |
| | 2,000 | 0.131 | 0.134 | 0.297 | 0.294 |
| | 4,000 | 0.160 | 0.159 | 0.234 | 0.234 |

What it says: when the budget holds only one to three documents, PACK reaches more of the relevant ones (SciFact 0.65 to 0.74 at 500 tokens) because it cuts every row to a quarter of the budget, but for the same reason it spends half as much of the budget on them: the top document, often the relevant one, is cut too. Once the budget holds five documents or more, the two are the same within a hundredth. `DEDUPE 0.85` changes nothing on these collections, which have few near-duplicates. PACK is faster per query here (about 1 ms against 1.9 ms at p50) because it reads 64 rows' text, not 200. What helps an answer more, more documents in part or fewer whole, this does not measure; a model reading the contexts would.

## The estimate of tokens

[token_error.py](token_error.py) ([token_error.json](token_error.json)) compares `count_tokens` with OpenAI's cl100k_base and o200k_base (tiktoken 0.12.0) on 519 abstracts each of SciFact and NFCorpus, 400 MIRACL Wikipedia passages each in Japanese, Chinese, Korean, Thai, Arabic, Hindi and Russian, and ChloeDB's query code in 2,000-byte pieces. Estimate over true count, summed:

| text | cl100k_base | o200k_base |
|---|---|---|
| English (SciFact, NFCorpus) | 1.13 to 1.15 | 1.15 to 1.17 |
| Rust code | 1.27 | 1.27 |
| Japanese | 0.89 | 1.21 |
| Chinese | 0.74 | 1.16 |
| Korean | 0.75 | 1.18 |
| Thai | 0.99 | 2.28 |
| Arabic | 0.50 | 1.09 |
| Hindi | 0.41 | 1.23 |
| Russian | 0.72 | 1.27 |

Above 1 is the safe side: the pack spends less of the model's budget than it says. English and code are 13 to 27% over with both. The first run found other alphabets counted at a third of cl100k_base's count (Arabic 0.33, Hindi 0.34, Russian 0.48), as letters of every alphabet were a token per six; they are now a token per three, which brings o200k_base above 1 and leaves cl100k_base, an older tokenizer that splits these scripts finely, under. No single estimate fits both tokenizers for these scripts: for a model with cl100k_base, give a budget of about half the window for Arabic or Hindi text.

To run both:

```sh
chloedb/tests/fixtures/beir/fetch.sh ~/beir
cargo run --release --bin chloedb-pack-bench -- --dir ~/beir --token-sample sample.jsonl \
    --token-extra ja.jsonl,zh.jsonl,ko.jsonl,th.jsonl,ar.jsonl,hi.jsonl,ru.jsonl --token-code src/query
pip install tiktoken==0.12.0 && python3 results/pack/token_error.py sample.jsonl results/pack/token_error.json
```

The MIRACL files are the first 400 passages of each language's first corpus file (`miracl/miracl-corpus` at revision `d921ec7e`, as `chloedb/tests/fixtures/miracl/fetch.sh` fetches it).

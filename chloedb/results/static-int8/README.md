# Static models with int8 tables

Model2Vec models (`potion-retrieval-32M`, `potion-base-8M`) against their int8
copies, which `chloedb::embed::quantize_static` writes (`chloedb-server
fetch-model potion-retrieval-32M-int8`, or `chloedb-server quantize-model`):
one scale for the whole table, as Model2Vec's own int8 format has it.

Measured on 2026-11-12 on a 4-core cloud VM (Intel Xeon @ 2.10GHz) while
another job built chloedb-server on two of its cores, so the times are
rough; the nDCG@10 numbers do not depend on the machine.

## Ranking (`beir_sql.txt`)

nDCG@10 on the BEIR test splits, exact (flat) search, each query ranked by
SQL that embeds its text itself, `ORDER BY similarity(emb, :q) DESC` with
`:q` the query's text, and the query's own document left out of its results,
as BEIR's evaluation does:

| model | table | SciFact | NFCorpus | ArguAna |
|---|---|---|---|---|
| potion-retrieval-32M | F32, 123 MB | 0.6331 | 0.3080 | 0.4508 |
| potion-retrieval-32M-int8 | I8, 31 MB | 0.6315 | 0.3079 | 0.4513 |
| potion-base-8M | F32, 29 MB | 0.5064 | 0.2434 | 0.4218 |
| potion-base-8M-int8 | I8, 7 MB | 0.5029 | 0.2437 | 0.4209 |

```text
chloedb/tests/fixtures/beir/fetch.sh ~/beir
chloedb-server fetch-model potion-retrieval-32M --models ~/models
chloedb-server fetch-model potion-retrieval-32M-int8 --models ~/models
cd chloedb && cargo run --profile ci --example static_sql_eval -- ~/beir/scifact ~/models potion-retrieval-32M-int8
```

## Vectors (`tables.txt`)

Each SciFact document embedded by the F32 model and by its copies, and the
cosine of each document's two vectors (`examples/static_tables.rs`).

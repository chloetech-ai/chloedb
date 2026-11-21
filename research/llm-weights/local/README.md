# ChloeDB inside an LLM, on your laptop

`inside_llm.py` loads Qwen3-0.6B and an embedded ChloeDB in the same Python
process and runs ChloeDB's vector search **inside the model's forward pass**,
on the model's own hidden states. Nothing is served over the network.

```sh
./setup.sh                                   # .venv + torch, transformers, chloedb; downloads the model
.venv/bin/python inside_llm.py facts         # type help
.venv/bin/python inside_llm.py ffn           # first run builds the rows (a few minutes)
.venv/bin/python inside_llm.py facts < smoke.txt   # the scripted tour
```

## `facts`: facts as rows at layer 12

Each row is a key (layer 12's hidden state of a prompt, averaged over its
tokens, 1,024 numbers), a value (a replacement hidden state, 1,024 numbers,
trained by gradient descent so the model says the answer) and the fact's
text. In every forward pass the prompt's key is a vector query
(`ORDER BY similarity(key, :v) DESC LIMIT 1`); a row with cosine above the
threshold (0.9) replaces the last token's state. The weights never change.

- `seed`, `teach <prompt> | <answer>`: INSERT a fact (5 to 10 s each).
- `ask <prompt>`: the answer without and with the rows, and the row used.
- `near <prompt>`: the vector search the model runs, top 5 with cosines.
- `forget <id>`: DELETE; the model stops saying it.
- `sql ...`: any SQL on table `facts`, e.g. `WHERE MATCH(prompt, 'port')`.

Measured in the cloud (4 CPUs): the taught port is said for the prompt and
for a rewording (cosine 0.96), Paris stays Paris (nearest row 0.75, unused),
and a forgotten fact is gone.

## `ffn`: one feed-forward block taken out, replaced by rows

Layer 16's MLP (9.4 million weights) is removed. Its replacement is a table of
rows, each a key and a value of the model's dimension (1,024). For every
token, the block's input is a top-32 vector query, and the block's output is
the softmax-weighted sum of those rows' values. The rows are first fitted to
imitate the block on ChloeDB's docs, then:

- `ppl`: perplexity of a held-out doc (postgres.md) with the original block,
  no block and the rows.
- `gen <prompt>`: the three continuations side by side.
- `trace <text>`: for each token, the row it pulled hardest and the text that
  row was learned from.
- `teach [FILE [steps]]`: INSERT new rows keyed on a text, their values
  trained on it with the model's own loss (old rows and the model frozen).
- `sql DELETE FROM rows WHERE source != 'base'`: take that knowledge out;
  `sql SELECT id, ctx FROM rows WHERE MATCH(ctx, 'shard')`: search the rows by
  the text they came from.

Small run in the cloud (2,048 rows, 100 fitting steps): original 57.4,
removed 64.5, rows 60.5; `teach` (100 steps) 57.3; DELETE back to 60.5
exactly. `--rows 16384 --distil-steps 800` (the defaults) fit better.

Options: `--layer N`, `--rows N`, `--pairs N`, `--distil-steps N`,
`--rebuild`, `--db PATH`, `--model PATH_OR_ID`, `--threads N`.

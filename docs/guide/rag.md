# Retrieval for RAG

A model answers questions about your documents well when it is handed the right passages, with their sources. This recipe splits documents into passages, stores them in ChloeDB with a vector each, and retrieves for a question by words and meaning at once, at most two passages from any one document, so that the model sees several sources. Then it builds the prompt and, if you have an Anthropic API key, asks Claude.

You need Python with the `chloedb` package and `chloedb-server` on your `PATH` ([quickstart](../quickstart.md)).

## The documents

Any directory of text or Markdown files will do. Here are three short ones:

```markdown file=handbook/returns.md
# Returns

Customers can return an item within 30 days of delivery for a full refund.
Items must be unused and in their original packaging.

## Damaged items

If an item arrives damaged, the customer should send a photo within 7 days.
We ship a replacement at no cost and do not ask for the damaged item back.
```

```markdown file=handbook/shipping.md
# Shipping

Orders placed before 2 pm ship the same day from the Rotterdam warehouse.
Standard delivery takes 2 to 4 working days inside the EU.

## Express

Express delivery arrives the next working day and costs 12 euros.
It is not available for furniture or other bulky items.
```

```markdown file=handbook/warranty.md
# Warranty

Electronics carry a two-year warranty against manufacturing defects.
The warranty does not cover water damage or a cracked screen.

## Claims

To make a claim, the customer needs the order number and a description of the fault.
A repaired or replaced item keeps the remaining warranty of the original.
```

## Split and store

A passage is a section of a file: the text under one heading, cut further if it is long. Each passage keeps its file and heading, so an answer can cite them. The `emb` field is computed from the heading and the text by the embedding model when a row is written:

```python file=ingest.py
import pathlib
import re
import chloedb

SCHEMA = {
    "table": "passages",
    "key": "id",
    "fields": [
        {"name": "id", "type": "keyword"},
        {"name": "doc", "type": "keyword"},
        {"name": "heading", "type": "text"},
        {"name": "body", "type": "text"},
        {"name": "n", "type": "int"},
        {"name": "emb", "type": "vector",
         "embed": {"from": ["heading", "body"], "model": "potion-retrieval-32M"}},
    ],
}


def passages(path, max_chars=800):
    """The sections of a Markdown file, as (heading, text), long ones cut at paragraphs."""
    heading, lines = path.stem, []
    for line in path.read_text().splitlines() + ["# end"]:
        if re.match(r"#+ ", line):
            text = "\n".join(lines).strip()
            while text:
                cut = text.rfind("\n\n", 0, max_chars) if len(text) > max_chars else len(text)
                cut = cut if cut > 0 else max_chars
                yield heading, text[:cut].strip()
                text = text[cut:].strip()
            heading, lines = line.lstrip("# ").strip(), []
        else:
            lines.append(line)


db = chloedb.open("rag.chloedb", schema=SCHEMA)
rows = []
for path in sorted(pathlib.Path("handbook").glob("*.md")):
    for n, (heading, body) in enumerate(passages(path)):
        rows.append({"id": f"{path.name}#{n}", "doc": path.name, "heading": heading, "body": body, "n": n})
db.upsert(rows)
print(f"{len(rows)} passages from {len({r['doc'] for r in rows})} files")
db.close()
```

```sh
chloedb-server fetch-model potion-retrieval-32M    # once: about 130 MB
```

```sh
python3 ingest.py
```

```text output
6 passages from 3 files
```

`upsert` replaces a passage with the same key, so running it again after editing a file updates its passages. (A section that disappears from a file stays until you delete it: `db.delete(keys)`, or `DELETE FROM passages WHERE doc = 'returns.md'` before writing the file's passages again.)

## Retrieve

One query does the retrieval. `rrf` fuses a BM25 ranking over the text with a vector ranking, which finds passages that use the question's words and passages that mean the same thing in others; `MAX 2 PER doc` keeps at most two passages of a file:

```python file=retrieve.py
import chloedb

QUERY = """
SELECT id, doc, heading, body FROM passages
ORDER BY rrf(bm25(body, :q), bm25(heading, :q), similarity(emb, :v)) DESC
MAX 2 PER doc
LIMIT :k
"""


def retrieve(db, question, k=4):
    return list(db.query(QUERY, q=question, v={"embed": question}, k=k))


if __name__ == "__main__":
    db = chloedb.open("rag.chloedb", read_only=True)
    for row in retrieve(db, "my parcel came broken, what do I do?"):
        print(row["id"], "|", row["heading"])
```

```sh
python3 retrieve.py
```

```text output
returns.md#1 | Damaged items
warranty.md#1 | Claims
returns.md#0 | Returns
shipping.md#1 | Express
```

The question says *parcel* and *broken*, which no passage does: the vector side finds the damaged-items passage by meaning, and BM25 still ranks exact terms such as an order number or a product name first when a question has them.

## Ask

The passages go into the prompt with their sources, and the model is told to answer from them and cite them. Without `ANTHROPIC_API_KEY` set, the script prints the prompt it would send:

```python file=ask.py
import os
import sys
import chloedb
from retrieve import retrieve

question = sys.argv[1]
db = chloedb.open("rag.chloedb", read_only=True)
sources = "\n\n".join(
    f'<source id="{r["id"]}">\n{r["heading"]}\n{r["body"]}\n</source>' for r in retrieve(db, question)
)
prompt = (
    "Answer the question from the sources below. Cite each source you use by its id, "
    "like [returns.md#1]. If the sources do not answer it, say so.\n\n"
    f"{sources}\n\nQuestion: {question}"
)

if not os.environ.get("ANTHROPIC_API_KEY"):
    print(prompt)
    sys.exit(0)

import anthropic  # pip install anthropic

client = anthropic.Anthropic()
response = client.messages.create(
    model="claude-opus-5-5",
    max_tokens=16000,
    messages=[{"role": "user", "content": prompt}],
)
if response.stop_reason == "refusal":
    sys.exit("the model declined to answer")
print("".join(b.text for b in response.content if b.type == "text"))
```

```sh
python3 ask.py "Can I get express delivery for a sofa?"
```

```text output
Answer the question from the sources below. Cite each source you use by its id, like [returns.md#1]. If the sources do not answer it, say so.

<source id="shipping.md#1">
Express
Express delivery arrives the next working day and costs 12 euros.
It is not available for furniture or other bulky items.
</source>

<source id="returns.md#0">
Returns
Customers can return an item within 30 days of delivery for a full refund.
Items must be unused and in their original packaging.
</source>

<source id="shipping.md#0">
Shipping
Orders placed before 2 pm ship the same day from the Rotterdam warehouse.
Standard delivery takes 2 to 4 working days inside the EU.
</source>

<source id="returns.md#1">
Damaged items
If an item arrives damaged, the customer should send a photo within 7 days.
We ship a replacement at no cost and do not ask for the damaged item back.
</source>

Question: Can I get express delivery for a sofa?
```

## Make it better

- **Rank by more than text.** Multiply in what you know about a passage: `ORDER BY rrf(...) * (1 + 0.2 * is_current) DESC`, or decay old ones by date with [`decay`](../chloedb/query-language.md#decayfield-origin-scale--closeness-to-a-value).
- **Filter before ranking.** A `WHERE` clause (a product line, a language, what a user may see) is planned with the search, not applied after it: `WHERE audience IN ('public', :team)`.
- **Rerank the top.** A cross-encoder reads the question and each of the top passages together and scores them more precisely, in the same query: [rerank phases](../chloedb/query-language.md#rerank-phases).
- **Serve it.** The same query runs on chloedb-server over HTTP (`chloedb.connect(url)` has the same `query`), so several workers can retrieve from one index, and on a cluster when the documents outgrow a machine.
- **Bring your own vectors.** A `VECTOR(1024)` field without `embed` takes vectors you computed, and `similarity(emb, :v)` takes the question's vector as a list. Hosted models (`openai/text-embedding-3-small`, `voyage/voyage-3.5` and others) can also fill a field: [embedded vector fields](../chloedb/server.md#embedded-vector-fields).

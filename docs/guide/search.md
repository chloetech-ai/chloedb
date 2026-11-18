# Search guide

This page builds a small catalogue of books and searches it every way ChloeDB can: words, phrases and typos, filters and facets, meaning, a blend of the two, and results spread across authors. Each query is SQL, and every block runs, in order, against the code it documents. The [SQL reference](../chloedb/query-language.md) has every function and option; this page shows which to reach for.

You need Python with the `chloedb` package and `chloedb-server` on your `PATH` ([quickstart](../quickstart.md)). The server program fetches the embedding model once; everything else runs in your Python process, with no server.

**Running this page's SQL yourself.** Each SQL block below is one statement, with the rows it returns under it. Pass it to the database as a string: in Python, `db.query(sql)`, with a value for each `:name` placeholder as a keyword argument (`db.query(sql, q="dragon")`); in JavaScript, `await db.query(sql, {q: "dragon"})` ([JavaScript](../chloedb/javascript.md#querying)); over HTTP, `POST /v1/sql` with `{"sql": "...", "params": {"q": "dragon"}}` to a chloedb-server that serves the file (`chloedb-server --db books.chloedb`, [HTTP API](../chloedb/server.md#http-api)). The command center's SQL workbench runs them as they are. The first search below shows the Python form.

## The catalogue

Twelve books, one JSON object a line:

```json file=books.jsonl
{"id": "b01", "title": "The River and the Dragon", "author": "Ana Ruiz", "genres": ["fantasy"], "year": 1999, "rating": 4.1, "description": "A girl follows a river north to find the dragon that drowned her village."}
{"id": "b02", "title": "Winter Garden", "author": "Tomas Berg", "genres": ["mystery"], "year": 2005, "rating": 3.8, "description": "A gardener finds a letter buried under the frozen roses and a quiet village mystery begins."}
{"id": "b03", "title": "Dragons of the Salt Coast", "author": "Ana Ruiz", "genres": ["fantasy", "adventure"], "year": 2003, "rating": 4.4, "description": "Sea dragons guard the salt flats, and a smuggler crew sails into their storm."}
{"id": "b04", "title": "The Last Lighthouse", "author": "Mei Chen", "genres": ["literary"], "year": 2018, "rating": 4.6, "description": "An old keeper writes letters to the ships that no longer pass her lighthouse."}
{"id": "b05", "title": "Machine Hearts", "author": "Jonas Abara", "genres": ["science fiction"], "year": 2021, "rating": 4.0, "description": "A repair robot learns to grieve after its owner dies on a mining station."}
{"id": "b06", "title": "Stars Over Lagos", "author": "Jonas Abara", "genres": ["science fiction", "adventure"], "year": 2016, "rating": 4.2, "description": "Engineers in Lagos build a launch site and race a rival company to orbit."}
{"id": "b07", "title": "The Cartographer's Daughter", "author": "Mei Chen", "genres": ["historical", "adventure"], "year": 2012, "rating": 4.3, "description": "A mapmaker's daughter crosses the mountains with her father's unfinished maps."}
{"id": "b08", "title": "Murder at the Observatory", "author": "Tomas Berg", "genres": ["mystery"], "year": 2019, "rating": 3.9, "description": "An astronomer is found dead beneath the telescope, and every guest saw the stars."}
{"id": "b09", "title": "Bread and Salt", "author": "Lena Novak", "genres": ["literary", "historical"], "year": 2008, "rating": 4.5, "description": "Three generations of bakers keep a village alive through war and famine."}
{"id": "b10", "title": "The Dragon Accords", "author": "Lena Novak", "genres": ["fantasy"], "year": 2022, "rating": 3.7, "description": "Diplomats negotiate a treaty between kingdoms and the last of the dragons."}
{"id": "b11", "title": "Quiet Machines", "author": "Mei Chen", "genres": ["science fiction", "literary"], "year": 2024, "rating": 4.7, "description": "Household robots begin to write poems, and a lonely engineer reads every one."}
{"id": "b12", "title": "Tides of Glass", "author": "Ana Ruiz", "genres": ["fantasy", "romance"], "year": 2011, "rating": 4.0, "description": "A glassblower falls for a sea witch who trades storms for memories."}
```

A table with a primary key, text to search, keywords and numbers to filter on, and a vector that ChloeDB computes from the title and description with a small embedding model. Fetch the model once (it is about 130 MB, kept in `~/.local/share/chloedb/models`, or wherever `CHLOEDB_MODELS` says; a machine with `~/.local/share/strata/models` from before the rename keeps using that):

```sh
chloedb-server fetch-model potion-retrieval-32M
```

```python file=load.py
import json
import chloedb

db = chloedb.open("books.chloedb", schema={
    "table": "books",
    "key": "id",
    "fields": [
        {"name": "id", "type": "keyword"},
        {"name": "title", "type": "text"},
        {"name": "description", "type": "text"},
        {"name": "author", "type": "keyword"},
        {"name": "genres", "type": "keyword", "multi": True},
        {"name": "year", "type": "int"},
        {"name": "rating", "type": "float"},
        {"name": "emb", "type": "vector",
         "embed": {"from": ["title", "description"], "model": "potion-retrieval-32M"}},
    ],
})
with open("books.jsonl") as f:
    db.insert(json.loads(line) for line in f)
print(len(db), "books")
db.close()
```

```sh
python3 load.py
```

```text output
12 books
```

The same table in SQL, as `db.sql(...)` and a server's `POST /v1/sql` take it, is `CREATE TABLE books (id KEYWORD PRIMARY KEY, title TEXT, description TEXT, author KEYWORD, genres KEYWORD[], year INT, rating FLOAT, emb VECTOR OPTIONS(embed = 'title, description', model = 'potion-retrieval-32M'))`.

## Words: `MATCH` and `bm25`

`MATCH` says whether a row has the words; `bm25` scores how well, and `score()` is the score a row was ranked by. Both take the field to search:

```sql db=books.chloedb
SELECT id, title, score() FROM books
WHERE MATCH(description, 'village')
ORDER BY bm25(description, 'village') DESC LIMIT 5
```

```text output
id  | title                    | score()
----+--------------------------+--------
b01 | The River and the Dragon | 1.322
b09 | Bread and Salt           | 1.322
b02 | Winter Garden            | 1.213
```

The same statement from Python, with the search's words as a parameter:

```python
import chloedb

db = chloedb.open("books.chloedb", read_only=True)
r = db.query("""SELECT id, title, score() FROM books
                WHERE MATCH(description, :q)
                ORDER BY bm25(description, :q) DESC LIMIT 5""", q="village")
for row in r:
    print(row["id"], row["title"], round(row["score()"], 3))
```

```text output
b01 The River and the Dragon 1.322
b09 Bread and Salt 1.322
b02 Winter Garden 1.213
```

Words are stemmed, so `dragon` finds *dragons*. `MATCH` reads a small query language: words side by side must all be there, `OR` and `-` (not) combine them, quotes ask for a phrase, `*` for a prefix:

```sql db=books.chloedb
SELECT id, title FROM books WHERE MATCH(description, '(dragon OR robot) -sea') ORDER BY id
```

```text output
id  | title
----+-------------------------
b01 | The River and the Dragon
b05 | Machine Hearts
b10 | The Dragon Accords
b11 | Quiet Machines
```

```sql db=books.chloedb
SELECT id, title FROM books WHERE MATCH(description, '"salt flats"') OR MATCH(title, 'mach*') ORDER BY id
```

```text output
id  | title
----+--------------------------
b03 | Dragons of the Salt Coast
b05 | Machine Hearts
b11 | Quiet Machines
```

### As you type: prefixes

`bm25` reads a prefix as `MATCH` does: `drag*` ranks by the words of the field that begin *drag* ([weights, required and excluded words](../chloedb/query-language.md#weights-required-and-excluded-words)). So a search box can send what has been typed so far, its last word with a `*` while it is being typed, to both:

```sql db=books.chloedb
SELECT id, title FROM books WHERE MATCH(title, 'drag*') ORDER BY bm25(title, 'drag*') DESC
```

```text output
id  | title
----+--------------------------
b01 | The River and the Dragon
b10 | The Dragon Accords
b03 | Dragons of the Salt Coast
```

```sql db=books.chloedb
SELECT id, title FROM books WHERE MATCH(title, 'salt drag*') ORDER BY bm25(title, 'salt drag*') DESC
```

```text output
id  | title
----+--------------------------
b03 | Dragons of the Salt Coast
```

**Changed:** `bm25` read `drag*` as the word *drag* before, which no book has, and a ranking by `bm25` returns only the rows it scores, so the first query found nothing until the last word was whole. Against a server or package of 0.3.7 or earlier, rank by the words already typed in full, or by a column while there are none:

```sql db=books.chloedb
SELECT id, title FROM books WHERE MATCH(title, 'drag*') ORDER BY rating DESC
```

```text output
id  | title
----+--------------------------
b03 | Dragons of the Salt Coast
b01 | The River and the Dragon
b10 | The Dragon Accords
```

A prefix is lowercased, and finds the words that start with it and the words it may be the start of whose stems do not. The index keeps stems (*mystery* as *mysteri*, *running* as *run*), so a prefix also looks up the stem of what was typed as a word, and in English the stems of what was typed completed by a common ending (`runni*` finds *running*). Every letter of a word typed keeps its rows:

```sql db=books.chloedb
SELECT id, title FROM books WHERE MATCH(description, 'mystery*')
```

```text output
id  | title
----+--------------
b02 | Winter Garden
```

**Changed:** a prefix matched only the stems that start with it before, so against a server or package of 0.3.7 or earlier a word typed in full and then `*` (`mystery*`, `battery*`, `headphone*`) finds nothing, though each letter before it found the rows.

`db.search(text)` ranks with `bm25` too, so it finds nothing for a word half typed unless the word ends in `*`. To complete whole values instead (the searches people ran before, names kept as `KEYWORD`), `PREFIX(field, :typed)` reads a keyword field's sorted values, with typos if asked ([prefixes and autocomplete](../chloedb/query-language.md#prefixes-and-autocomplete)).

## Typos

`fuzzy => 'auto'` lets a word be an edit or two away, more for longer words, counted on the word as typed rather than on its stem: *choclate* is one edit from *chocolate*, though its stem, *choclat*, is three from *chocol*. Ranked, it corrects a typo rather than widening every word:

```sql db=books.chloedb
SELECT id, title FROM books ORDER BY bm25(description, 'lighthuose keeper', fuzzy => 'auto') DESC LIMIT 1
```

```text output
id  | title
----+--------------------
b04 | The Last Lighthouse
```

`db.search(text)` searches the words as written. The search box on a table's page in the command center forgives a typo in each word, and when nothing is found offers the correction ("did you mean") if there is one. A search that should forgive typos asks for it in its SQL, or writes `~` after a word (`dragn~`), which `bm25` reads as `fuzzy => 'auto'` for that word. With a `MATCH`, a row must have every word, each as typed or within its edits, and a `bm25` of the same text on the same field takes the `MATCH`'s `fuzzy`:

```python
import chloedb

db = chloedb.open("books.chloedb", read_only=True)
r = db.query("""SELECT id, title FROM books
                WHERE MATCH(description, :q, fuzzy => 'auto')
                ORDER BY bm25(description, :q) DESC LIMIT 3""", q="dragn villge")
for row in r:
    print(row["id"], row["title"])
print(len(db.search("dragn villge", mode="text")), "rows from db.search, by the words as written")
```

```text output
b01 The River and the Dragon
0 rows from db.search, by the words as written
```

[Build a search page](search-page.md) puts this behind a search box, with the matched words highlighted and "did you mean" for a misspelt word.

## Filters and facets

Text, keyword, number and list conditions mix freely in `WHERE`, and the planner picks the order. A multi-valued keyword such as `genres` matches a row when any of its values does:

```sql db=books.chloedb
SELECT id, title, year FROM books
WHERE genres = 'adventure' AND year >= 2010
ORDER BY rating DESC
```

```text output
id  | title                       | year
----+-----------------------------+-----
b07 | The Cartographer's Daughter | 2012
b06 | Stars Over Lagos            | 2016
```

Facets, the counts beside a search box, are a `GROUP BY` over the rows that match:

```sql db=books.chloedb
SELECT author, COUNT(*) AS books, AVG(rating) AS avg_rating FROM books
WHERE MATCH(description, 'dragon OR dragons OR sea OR robot')
GROUP BY author ORDER BY books DESC, author
```

```text output
author      | books | avg_rating
------------+-------+-----------
Ana Ruiz    | 3     | 4.167
Jonas Abara | 1     | 4
Lena Novak  | 1     | 3.7
Mei Chen    | 1     | 4.7
```

## Meaning: vectors

`similarity(emb, :v)` scores rows by how close their vector is to a query's. In Python a parameter `{"embed": text}` is the text's vector by the field's own model, so a question finds books that share none of its words:

```python
import chloedb

db = chloedb.open("books.chloedb", read_only=True)
r = db.query("SELECT id, title FROM books ORDER BY similarity(emb, :v) DESC LIMIT 3",
             v={"embed": "artificial intelligence and feelings"})
for row in r:
    print(row["id"], row["title"])
```

```text output
b05 Machine Hearts
b11 Quiet Machines
b06 Stars Over Lagos
```

## Similar rows: more like this

`{"vector_of": key}` is the vector a row already has, so ranking by `similarity` to it lists the rows nearest it in meaning: "more like this" under a product, "readers also liked" under a book. `id <> :id` leaves the row itself out, and any other condition goes in the same `WHERE`:

```python
import chloedb

db = chloedb.open("books.chloedb", read_only=True)
r = db.query("SELECT id, title FROM books WHERE id <> :id ORDER BY similarity(emb, :v) DESC LIMIT 3",
             id="b01", v={"vector_of": "b01"})
for row in r:
    print(row["id"], row["title"])
```

```text output
b10 The Dragon Accords
b03 Dragons of the Salt Coast
b07 The Cartographer's Daughter
```

The vector is read from the table the statement names (`{"vector_of": "b01", "table": "books"}` names one). The parameter is the same from JavaScript (`{vector_of: "b01"}`), in the `params` of a server's `POST /v1/sql`, and through a client of a server (`chloedb.connect`).

## Both: hybrid ranking

Words find exact names and rare terms; vectors find paraphrases. `rrf` fuses the two rankings by reciprocal rank, and needs no tuning:

```python
import chloedb

db = chloedb.open("books.chloedb", read_only=True)
q = "a story about the sea and storms"
r = db.query("""
    SELECT id, title, score() FROM books
    ORDER BY rrf(bm25(description, :q), similarity(emb, :v)) DESC LIMIT 3
""", q=q, v={"embed": q})
for row in r:
    print(row["id"], row["title"])
```

```text output
b03 Dragons of the Salt Coast
b12 Tides of Glass
b04 The Last Lighthouse
```

When one side should count more, normalise each score to 0..1 and weigh them: `ORDER BY 0.3 * minmax(bm25(description, :q)) + 0.7 * minmax(similarity(emb, :v)) DESC`. The same blend can boost by a column, `* (1 + 0.1 * rating)`; [ranking expressions](../chloedb/query-language.md#ranking-expressions) has the rest.

For the common case, `db.search(text)` writes this query itself: BM25 over the text fields fused with the vector field's similarity.

```python
import chloedb

db = chloedb.open("books.chloedb", read_only=True)
for row in db.search("lonely robots", limit=2, columns=["id", "title"]):
    print(row["id"], row["title"])
```

```text output
b11 Quiet Machines
b05 Machine Hearts
```

## Spread results out: `MAX k PER`

A ranking where one author fills the page reads badly. `MAX 1 PER author` keeps each author's best row and the order of the rest:

```sql db=books.chloedb
SELECT id, author, title FROM books ORDER BY rating DESC MAX 1 PER author
```

```text output
id  | author      | title
----+-------------+--------------------------
b11 | Mei Chen    | Quiet Machines
b09 | Lena Novak  | Bread and Salt
b03 | Ana Ruiz    | Dragons of the Salt Coast
b06 | Jonas Abara | Stars Over Lagos
b08 | Tomas Berg  | Murder at the Observatory
```

## What the planner did

Each answer says how it was found. In Python, `stats["plan"]` holds a line for each segment the query read: which conditions came from an index, which were tested row by row, and how the rows were ranked. [Reading a plan](../chloedb/query-language.md#reading-a-plan) explains the lines, and a server's `POST /v1/explain` takes the same `SELECT` and answers its plan as a tree too.

```python
import chloedb

db = chloedb.open("books.chloedb", read_only=True)
r = db.query("SELECT id FROM books WHERE MATCH(description, 'dragon') AND year > 2000 "
             "ORDER BY bm25(description, 'dragon') DESC LIMIT 3")
print(r.keys)
for line in r.stats["plan"]:
    print(line)
```

```text output
['b10', 'b03']
seg 0: bm25 MaxScore, filter Probe
```

## Next

- [Build a search page](search-page.md): this page's queries behind a search box in a Python web app, with highlights, did you mean and more like this.
- [Retrieval for RAG](rag.md): the hybrid query above, feeding passages to a model.
- [The SQL reference](../chloedb/query-language.md): joins, window functions, graphs, rank profiles, rerankers.
- [Python](../chloedb/python.md): queries built in Python rather than SQL text, Arrow results, writes.

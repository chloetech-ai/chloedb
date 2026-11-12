# Concepts

ChloeDB is a search engine and a database in one library. This page names the parts you meet when you use it: the file, tables and their fields, the three kinds of index every row gets, scores, the planner, and the server that serves the same database to many clients or many machines. [How ChloeDB works](../chloedb/index.md) goes deeper into each.

## One database, one file

A database is one file plus a write-ahead log beside it (`books.chloedb` and `books.chloedb-wal`), the way SQLite keeps one in WAL mode, and a small directory (`-readers`) where processes that read it keep their place. Your program opens it as a library, from Rust, [Python](../chloedb/python.md), [JavaScript](../chloedb/javascript.md) or [Java](../chloedb/java.md), with no server and no configuration. The same file can be served by [chloedb-server](../chloedb/server.md) when many clients, or many machines, need it.

```python
import os
import chloedb

db = chloedb.open("notes.chloedb", schema={
    "table": "notes", "key": "id",
    "fields": [{"name": "id", "type": "keyword"}, {"name": "body", "type": "text"}],
})
db.insert({"id": "n1", "body": "Water the tomatoes before nine."})
print(sorted(f for f in os.listdir(".") if f.startswith("notes")))
db.close()
print(sorted(f for f in os.listdir(".") if f.startswith("notes")))
```

```text output
['notes.chloedb', 'notes.chloedb-readers', 'notes.chloedb-wal']
['notes.chloedb', 'notes.chloedb-readers', 'notes.chloedb-wal']
```

Writes are transactions with snapshot isolation: a query sees the database as it was when it started, readers never wait for the writer, and a crash leaves the last committed state. One process writes a database at a time; other processes can open it read-only and follow its commits. Back up a database that is in use with `checkpoint`, not by copying its files ([backups](../chloedb/python.md#backing-up-and-damaged-files)).

## Tables, fields and keys

A database holds tables, and a table has typed fields and, usually, a primary key. A table is created from a schema, a JSON object as above or SQL:

```sql norun
CREATE TABLE books (
  id KEYWORD PRIMARY KEY,
  title TEXT,
  author KEYWORD,
  genres KEYWORD[],
  year INT,
  rating FLOAT,
  emb VECTOR OPTIONS(embed = 'title', model = 'potion-retrieval-32M')
);
```

| type | holds | searched with |
|---|---|---|
| `TEXT` | words, analysed (lowercased, stemmed, stopwords dropped) | `MATCH`, `bm25` |
| `KEYWORD`, `KEYWORD[]` | exact values: ids, tags, names | `=`, `IN`, `LIKE 'p%'`, `GROUP BY` |
| `INT`, `FLOAT`, `BOOL`, `TIMESTAMP` | numbers, flags and times | ranges, `ORDER BY`, aggregates |
| `JSON` | documents whose keys no schema names | `attrs.color = 'red'` |
| `VECTOR` | embeddings, given or computed by a model | `similarity`, `distance` |
| `SPARSE` | learned term weights (SPLADE and the like) | `similarity` |
| `EDGES` | typed, weighted links to other rows | `REACHABLE`, `PATH`, `ppr` |
| `MEDIA` | images and sounds, by URI | the vectors and text made from them |

A vector field with `embed` is filled by an embedding model from other fields when a row is written, so you insert text and search by meaning. The model runs inside ChloeDB (`potion-retrieval-32M`, a fast static model, or BERT models when built with them) or behind an API (OpenAI, Cohere, Voyage, Gemini, Ollama). The [SQL reference](../chloedb/query-language.md#columns-and-types) lists every type and option.

## Every row, indexed three ways

Each row is indexed by every kind of index its fields call for, in the same place, so one query can use them all:

- **Full text**: posting lists of each word's rows, compressed, and scored with BM25.
- **Columns**: each field's values stored by column, for filters, sorting, facets and aggregates.
- **Vectors**: an HNSW graph (or a flat or IVF index) over each vector field, for nearest neighbours.

Rows can also hold **edges** to other rows, which turn a table into a graph that the same queries walk ([graphs](../chloedb/graphs.md)).

Rows are written into immutable **segments**. A new row goes to the write-ahead log and a memory table first; full memory tables are written as segments, and segments are merged in the background, as Lucene and SQLite's FTS5 do. You do not manage segments, but `EXPLAIN` shows a plan for each.

## Queries, scores and the planner

Everything is SQL. Search is a set of functions inside it: `MATCH` selects rows with words, `bm25`, `similarity` and others score them, `rrf` and weighted sums blend scores, and `score()` is the score a row was ranked by. A ranking can multiply in a column (`* (1 + 0.1 * rating)`), decay with age, run a learned model, or rerank its top rows with a cross-encoder. The [search guide](search.md) shows each in use.

A cost-based **planner** decides, for each segment and from exact statistics, how to run each part of a statement: which condition to read from an index, which to test row by row, whether to search vectors before a filter or within it. You write what you want, and `EXPLAIN` says what it chose.

## From a file to a cluster

The same database, and the same SQL, run three ways:

| | how | when |
|---|---|---|
| **In your process** | `chloedb.open("books.chloedb")` | one program owns the data: an app, a notebook, an agent |
| **One server** | `chloedb-server --db books.chloedb` | many clients, other languages, a browser UI |
| **A cluster** | a coordinator over shards, each with replicas | more data or traffic than one machine holds |

[chloedb-server](../chloedb/server.md) serves a database over HTTP JSON and gRPC, with a command center in the browser for queries, health and performance, and an agent that answers questions by writing SQL. Several servers run as shards behind a coordinator that answers like one database, and a primary ships its writes to replicas. The Python, JavaScript and Java clients have the same methods for a file and a server, so code moves from one to the other unchanged.

## Apps on a cluster

Some programs built on ChloeDB run beside a server and appear in its command center: the [crawler](../chloedb/crawler.md), which turns a site into a table, [Sheets](../chloedb/sheets.md), spreadsheets whose cells are rows, and [ChloeCtx](../chloectx/index.md), long-term memory for Claude Code. [Apps on a cluster](apps.md) says how they attach.

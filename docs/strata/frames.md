---
title: DataFrames
parent: Strata
nav_order: 5.2
---

# DataFrames in Python
{: .no_toc }

`store["table"]` is a `strata.Frame`: a lazy dataframe written the way Polars writes one. Nothing is read until you ask for rows. Then the whole chain compiles to one SQL statement, which runs where the data is (a file in your process, or a strata-server), and the answer comes back as an Arrow table, which pandas, Polars, NumPy, DuckDB and PyTorch read without a copy. The same code runs on a file and on a server.
{: .fs-5 .fw-300 }

1. TOC
{:toc}

## Install

Frames are part of the `strata` package ([Python](python.md#install)) and need pyarrow 14 or later; pandas and Polars are optional, for `to_pandas()` and `to_polars()`:

```sh
pip install "strata-db[frame]" polars pandas
```

## A first frame

```python
import polars as pl
import strata as st
from strata import col

shop = st.connect("shop.strata")              # a file; st.connect("https://...") for a server
orders = shop.create_table("orders", pl.read_parquet("orders.parquet"), key="id", text=["note"])
# or, once the table exists: orders = shop["orders"]

top = (orders
       .filter(col("status") == "paid", country="cl")
       .group_by(month=col("ts").dt.truncate("month"))
       .agg(revenue=col("amount").sum(), p90=col("amount").quantile(0.9), n=st.len())
       .sort("month"))

top.to_polars()     # or .to_pandas(), .to_arrow(), .to_numpy(), .to_dicts()
print(top.sql())    # the statement it runs; its values are parameters, top.params()
```

`st.connect(path)` opens a database file, or makes it with its first `create_table`; `st.connect(url)` is a [`Client`](python.md#a-server-from-python) of a server or a coordinator of shards, with `token=` when the server asks for one. Both give frames and `create_table` the same way.

## Making tables

`create_table(name, data, ...)` makes a table from rows and returns its frame. `data` is anything that streams Arrow (a pyarrow Table, a Polars DataFrame, a DuckDB relation, another frame), a pandas DataFrame, or a list of dicts. The schema comes from the Arrow types as the [Parquet import](parquet.md) makes one: strings are KEYWORD (or TEXT when they read as prose), lists of numbers of one length are vectors, timestamps are TIMESTAMP.

```python
shop.create_table("users", users_df)                                  # key: the column named id
shop.create_table("docs", rows, key="doc_id", text=["title", "body"],  # TEXT: indexed for BM25
                  embed={"emb": {"from": ["title", "body"], "model": "potion-retrieval-32M"}})
shop.create_table("scores", preds, key="user", if_exists="append")    # upsert into it if it exists
```

| argument | |
|---|---|
| `key` | the primary key column (default a column named `id`; `""` for none) |
| `text`, `keyword` | columns to make TEXT (indexed for `search`) or KEYWORD |
| `embed` | vector fields a model fills from other fields, where the rows are written |
| `schema` | the whole [schema](python.md#a-database-in-your-process) instead |
| `if_exists` | `"fail"` (the default), `"append"` (upsert by key) or `"replace"` |

`frame.write_to(store, "table")` runs a frame and writes its answer to a table of any store: a server's rows to a local file for a notebook, or features computed on a file to a server. The key is the source table's when the answer has its column.

## Building a frame

Each method returns a new frame, and none of them runs anything.

| method | |
|---|---|
| `filter(cond, ..., name=value)` | the rows where every condition holds; conditions on indexed fields are answered by the indexes |
| `select(*cols, name=expr)`, `with_columns(...)` | the answer's columns; every column with these added or replaced. Aggregates alone give one row: `select(n=st.len(), total=col("x").sum())` |
| `drop(*cols)`, `rename({old: new})` | |
| `sort(by, ..., descending=False, nulls_last=True)` | rows without a value come last |
| `head(n=5)`, `slice(offset, length)` | the first rows; an unsorted, unranked frame's are any rows, as in SQL |
| `group_by(*keys, name=expr).agg(...)`, `.len()` | a row per group; a key may be an expression (`month=col("ts").dt.truncate("month")`). A `filter` after it is `HAVING` |
| `join(other, on=, left_on=, right_on=, how="inner", suffix="_right")` | `how` is `"inner"` or `"left"`, and both frames are of one store |
| `unique()`, `unique(subset, keep="first")` | each row once; the first row of each value in the frame's order (`DISTINCT ON`) |
| `union(*frames)`, `st.concat(frames)` | the rows of each (`UNION ALL`; `distinct=True` for `UNION`) |

## Expressions

| | |
|---|---|
| `col(name)`, `st.lit(value)`, `+ - * / % **`, `== != < <= > >=`, `& \| ~` | columns, values, arithmetic, comparisons, and, or, not |
| `.is_in(values)`, `.is_between(lo, hi)`, `.is_null()`, `.is_not_null()`, `.fill_null(value)`, `.cast("int")` | |
| `st.when(cond).then(a).when(...).then(b).otherwise(c)` | `CASE WHEN` |
| `.str.lower()`, `.upper()`, `.len_chars()`, `.contains(s)`, `.starts_with(s)`, `.ends_with(s)`, `.like(p)`, `.ilike(p)` | strings |
| `.dt.truncate("month")`, `.year()`, `.quarter()`, `.month()`, `.week()`, `.day()`, `.weekday()`, `.hour()`, `.epoch()`, `.strftime("%Y-%m")` | timestamps |
| `.sum()`, `.mean()`, `.median()`, `.quantile(q, interpolation="linear")`, `.min()`, `.max()`, `.count()`, `.n_unique()`, `st.len()` | aggregates, for `agg` or a `select` of totals |
| `.over(*partition_by, order_by=, descending=)` with `.rank(method="min")`, `.cum_sum()`, `.shift(n)` or an aggregate | windows: `col("amount").rank(descending=True).over("user")`, `col("x").shift(1).over("user", order_by="ts")` |

Every function of [queries built in Python](python.md#queries-built-in-python) is an expression of a frame too: `st.bm25`, `st.similarity`, `st.rrf`, `st.minmax`, `st.zscore`, `st.gbdt`, `st.coalesce`, and the rest.

## Search and ranking

Search is a verb of the frame. Each of these keeps the `k` best rows (100 by default), best first, and adds a `score` column:

```python
docs = shop["docs"]
docs.search("refund damaged", on="body", k=20)                  # BM25 over a TEXT field
docs.near("parcel arrived broken", on="emb", k=20)              # a text the field's model embeds, or a vector
docs.rank_by(0.6 * st.minmax(st.bm25("body", q))                # any ranking expression
             + 0.4 * st.minmax(st.similarity("emb", q)), k=100)

(docs.filter(col("lang") == "en")                               # filters first narrow what is ranked
     .search(q, on="body", k=200)
     .rerank(st.gbdt("docs_ltr", st.bm25("body", q), col("rating")), k=20)   # rank the 200 again
     .max_per(2, "site")                                         # at most two rows of a site
     .filter(col("rating") > 3))                                 # filters after narrow the hits
```

`search` reads [weights, required and excluded words](query-language.md#weights-required-and-excluded-words) (`"+mrna vaccine^2 -animal"`) unless `syntax="plain"`. `rerank` is a [rerank phase](query-language.md#rerank-phases), for a costlier score over the rows of the search before it.

## Getting the rows

| | |
|---|---|
| `to_arrow()`, `collect()` | a pyarrow Table |
| `to_polars()`, `pl.DataFrame(frame)` | a Polars DataFrame, sharing the Arrow memory |
| `to_pandas(arrow_dtypes=True)` | a pandas DataFrame backed by the Arrow columns (`False` copies into NumPy) |
| `to_numpy()` | a 2-D array of the columns, or of the one vector column, `(rows, dimensions)` float32: embeddings for a model |
| `to_dicts()`, `iter_batches(batch_size)` | a dict a row; RecordBatches |
| `count()`, `item()` | the number of rows (`COUNT(*)`, run where the data is); the one value of a one-row, one-column answer |

A frame streams Arrow (`__arrow_c_stream__`), so DuckDB reads it by name (`duckdb.sql("SELECT ... FROM frame")`) and `pa.table(frame)` runs it. In a notebook a frame shows its first ten rows.

## Writing

Writes are explicit; a frame never changes a table as a side effect.

```python
orders.get("o5")                                               # a row by key
orders.upsert(new_rows)                                        # rows in, replacing those of their keys
orders.filter(col("id") == "o5").update(status="void", amount=col("amount") * 0.9)
orders.filter(col("ts") < "2025-01-01").delete()               # delete() of a whole table takes all=True
```

## What runs where

A frame is one SQL statement, so it runs where the data is and only the answer moves: a group-by over a server's ten million rows sends back a row per group. `frame.sql()` shows the statement and `frame.explain()` the plan the store runs.

- A filter on an indexed field is answered by its index. A group-by on indexed KEYWORD, number, BOOL or TIMESTAMP columns (or parts of a timestamp) folds into the table's own query; any other key first computes its rows in a `WITH` step, with only the columns it reads.
- Joins are inner or left, between tables of one store. To join a server's table with a local one, copy one side with `write_to` first.
- From a server, the answer comes back as an Arrow stream, its columns typed as a file's are, so the same frame gives the same table from either. A server too old for Arrow answers JSON, which the client types from the table's schema.
- Not there yet: right, full and cross joins, `pivot` and `melt`, and a seeded `sample` or train/test split.

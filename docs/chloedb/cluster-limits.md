# What a cluster does differently

A coordinator over shards ([sharding](server.md#sharding-one-coordinator-many-shards)) answers most requests as one server does: the tests run every query kind against three shards and one database and compare the answers. This page lists what it refuses, and what it does in another way, so that nobody finds out in production. A table placed by a column (`OPTIONS(shard_by = '...')`, [tables on shards](server.md#tables-on-shards)) has a few differences of its own, listed after.

`POST /v1/sql/check` with `{"sql": ..., "params": ...}` says whether a statement will run on the deployment it is sent to, without running it: `{"runs": true, "deployment": "cluster", "kind": "write", "notes": [...]}`, or `runs: false` with the `error` it would get. A pipeline's CI can send its statements to the cluster it deploys to (an admin or full key; a key limited to tables is refused, as for `/v1/plan`).

## Refused on a cluster

| what | on one server | on a cluster | instead |
|---|---|---|---|
| Graph queries: `REACHABLE`, `PATH`, `hops()`, `ppr()` | run | refused: a row's edges point at rows other shards hold | run them on a server that holds the whole table, or walk from the application a round of reads by key per hop ([graphs](graphs.md#on-a-cluster)) |
| `ALTER TABLE ... DROP COLUMN` | runs | refused: the coordinator's plans name columns by place | add a column and stop reading the old one |
| `INSERT ... ON CONFLICT DO UPDATE` whose rows go to several shards | runs | refused (400) | one statement per row or per shard, or `INSERT OR REPLACE` |
| The relevance autopilot (`POST /v1/relevance/autopilot`) | runs | refused: it replays queries on one snapshot | run it on a server with a copy of the tables |
| `/v1/export` of the database as SQLite (`format=sqlite`) | the admin's | refused: a coordinator holds no files | `format=parquet`, `jsonl`, `csv` or `arrow` export one table's rows from a cluster |
| `EXPLAIN` through `/v1/sql` | refused on both | | `POST /v1/explain` |

## Different on a cluster

- **A write across shards is one transaction a shard, not one over all.** The coordinator checks each shard's part first and writes nothing when one refuses; what can still fail partway, and how to finish it with the idempotency key, is in [when a write fails](server.md#when-a-write-fails).
- **`UPDATE ... LIMIT n` and `DELETE ... [ORDER BY ...] LIMIT n`** are two steps: the coordinator reads the keys of the rows the statement's `WHERE`, `ORDER BY` and `LIMIT` pick (over every shard), then runs the statement without its `LIMIT` over those keys alone (`WHERE (...) AND key IN (...)`). A row changed in between so that it no longer matches is left alone, so fewer than n rows may change. The table needs a primary key, and the `LIMIT` is at most 10,000.
- **`INSERT ... ON CONFLICT DO NOTHING`** goes as an `insert_if_absent` operation (protocol 21: a shard of an older version refuses it as an unknown operation). **`ON CONFLICT DO UPDATE`** runs as written on the one shard its rows go to.
- **DDL** (`CREATE TABLE`, `ALTER TABLE`, `DROP TABLE`, rank profiles, functions) runs on every shard, one change a shard: a shard that fails leaves the others changed, and the statement is sent again with `IF NOT EXISTS`, `IF EXISTS` or `OR REPLACE`. `ALTER TABLE ... ADD COLUMN c ... NOT NULL` without a default is refused by each shard that has rows, and the error counts every such shard's rows.
- **A SQLite or Parquet import that creates its table** creates it on every shard by `CREATE TABLE` through the coordinator, as its plan (`plan=true`) says.
- **Exports**: `GET /v1/export?format=jsonl|csv|arrow|parquet&tables=T` reads the table by a scroll over every shard, with read on the table. Each page is read from the shards as they are then, not one snapshot of all of them: a write during the export may or may not be in it. Arrow is one batch built in memory (at most 1,000,000 rows); Parquet is written a page at a time and keeps the table's schema.
- **Rank-log export and interleaving credit** (`/v1/rank_log/export`, `/v1/rank_log/interleaving`) run their queries over every shard, each read whole by a scroll, rather than from one snapshot: a request logged during the export may be left out or come with part of its events.
- **Conditional writes** (`if_seq_no`, `seq_no()`) need every shard to number its writes alike (one replica, or a primary its replicas follow), and answers carry each shard's number (`seq_nos`), not one.
- **Write receipts** count per shard (`receipt.per_shard`); a shard from before protocol 21 does not count, and its counts are `null`. A strict write goes only to shards of protocol 21.

## A table placed by a column (`shard_by`)

A row goes to the shard of its `shard_by` value, not of its key. So:

- **A primary key is unique across the shards for `INSERT`.** Before it writes, the coordinator asks every shard's primary whether it holds the keys being inserted (one read by key a shard, for the whole request), and refuses a key another shard holds (409, naming the shard and the row's value of the column). Two inserts of one key through one coordinator wait for each other. Two coordinators inserting one key at the same moment with different `shard_by` values are not ordered by this check, and can both succeed: send the inserts of one key through one coordinator, or give the table a `shard_by` equal to the key's own placement.
- **`INSERT OR REPLACE` (upsert) replaces a row only on the shard of its `shard_by` value.** An upsert that gives an existing key another value of the column writes a second row there and leaves the old one (as Elasticsearch's custom routing does): change the column by deleting the row first.
- **`ON CONFLICT DO NOTHING`** passes over a key that another shard holds; **`DO UPDATE`** of a key that another shard holds under another value of the column is refused (409), since it would insert a second row.
- **Writes by key give the column's value as `"routing"`.** A `set`, `add` or `delete` whose routing places it on a shard that has no row with the key is refused (404, naming the routing and the column) rather than answered `written: 0`; `"if_missing": "skip"` passes over it as before. `DELETE /v1/kv/T/K` without `routing` on such a table is refused (400).
- **The column cannot change:** `UPDATE` of it, and `set` or `add` on it, are refused. A row that moves is a delete and an insert.
- **Queries ask every shard,** as for any table.

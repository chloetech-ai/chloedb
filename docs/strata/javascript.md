---
title: JavaScript
parent: Strata
nav_order: 6
---

# Strata from JavaScript
{: .no_toc }

The `strata-db` package opens a Strata file inside a Node, Bun or Deno process, the way better-sqlite3 opens a SQLite file: no server, no configuration. It can also talk to a running [strata-server](server.md) from any JavaScript runtime with `fetch` (a browser, a worker, Node, Bun, Deno). Both have the same methods, so code written against a local file works against a server.
{: .fs-5 .fw-300 }

1. TOC
{:toc}

## Install

The package is in [`strata-js/`](https://github.com/chloetech-ai/strata/tree/main/strata-js): a Node-API addon written in Rust ([napi-rs](https://napi.rs)) and the JavaScript around it, an ES module, for Node 18 or later, Bun and Deno.

### Prebuilt

Each `strata-js-v<version>` release on [GitHub](https://github.com/chloetech-ai/strata/releases) has a tarball per platform with the addon built in, which installs with no Rust and no compiler:

```sh
npm install https://github.com/chloetech-ai/strata/releases/download/strata-js-v0.1.0/strata-db-0.1.0-darwin-arm64.tgz
```

| platform | tarball |
|---|---|
| macOS 11+ on Apple silicon | `strata-db-<version>-darwin-arm64.tgz` |
| macOS 11+ on Intel | `strata-db-<version>-darwin-x64.tgz` |
| Linux x86-64, glibc 2.35+ (Ubuntu 22.04, Debian 12 and later) | `strata-db-<version>-linux-x64-gnu.tgz` |
| Linux arm64, glibc 2.35+ | `strata-db-<version>-linux-arm64-gnu.tgz` |

The workflow [`js-release.yml`](https://github.com/chloetech-ai/strata/blob/main/.github/workflows/js-release.yml) builds them on a tag `strata-js-v<version>` (the version in `strata-js/package.json`), tests each one, installs it into an empty app and runs it there, then attaches them to the release; `node scripts/pack.mjs <platform>` packs one by hand. The package is not on npm yet. Windows and Alpine (musl) have no tarball: build from source there (Windows through WSL 2).

### From source

It needs Rust (stable) and a C compiler.

```sh
cd strata-js
npm run build                    # cargo build --release, then copies the addon to strata.node
cd ../my-app
npm install ../strata/strata-js  # or "strata-db": "file:../strata/strata-js" in package.json
```

`npm install` of the directory builds the addon itself when `strata.node` is missing. `npm run build -- --features bert` builds more of Strata's features in (`bert`, `onnx`, `vision`, `audio`; `datafusion`, `parquet` and `s3` are on by default). `$STRATA_NODE_ADDON` loads an addon from another place.

The client alone (`strata-db/client`) is plain JavaScript with no addon: a bundler takes it into a page, and the package's `browser` and `workerd` conditions give it for `strata-db` too.

## A database in the process

```js
import { open } from "strata-db";

const db = await open("books.strata", {
  schema: {
    table: "books",
    key: "id",
    fields: [
      { name: "id", type: "keyword" },
      { name: "title", type: "text" },
      { name: "year", type: "int" },
      { name: "emb", type: "vector", embed: { from: ["title"], model: "potion-retrieval-32M" } },
    ],
  },
});

await db.upsert([{ id: "b1", title: "The river and the dragon", year: 1999 }]);
for (const row of await db.search("dragon")) console.log(row.id, row.title);
const r = await db.query("SELECT year, COUNT(*) FROM books GROUP BY year");
r.rows;          // [[1999, 1]]
await db.close();
```

`open(path, {schema})` opens the file, or creates it with the schema when it does not exist; `Database.create(path, schema)` and `Database.open(path)` do one or the other. A schema is an object, JSON text or the path of a JSON file, in the [short form or the long one](server.md#schemas). `directory: true` keeps a database as a directory of segment files, `models` is where embedded fields' models are (`$STRATA_MODELS`, else `~/.local/share/strata/models`; `strata-server fetch-model` fetches them), and `threads` how many cores index builds and each query use (every core by default).

Every call answers a promise and runs on a worker thread of Node's pool, so queries and writes leave the event loop free and many run at once. Writes made at once wait for each other and share commits (one disk flush for all those waiting), so none fails as busy. One process at a time may have a database open to write it (a second one gets "busy: the database is open elsewhere"). Others can open it with `Database.open(path, { readOnly: true })` to read it while it is written, the workers of a cluster, say: each query sees the writer's commits so far, every write fails, and `refresh()` looks for new commits at once ([how readers follow the writer](index.md#readers-in-other-processes)). For writes from several processes, serve it with strata-server.

### Querying

| call | what |
|---|---|
| `db.query(sql, params)` | a SELECT: a `Result` |
| `db.sql(sql, params)` | any statement: a SELECT's `Result`, or the rows an INSERT, UPDATE or DELETE wrote; `CREATE TABLE`, `ALTER TABLE` and `DROP TABLE` too |
| `db.search(text, {limit, where, columns, mode, textFields, vectorField, params, table})` | the rows that best match a text: BM25 over the text fields (`mode: "text"`), the similarity of an embedded vector field (`"vector"`), or both fused by reciprocal rank (`"hybrid"`, what `"auto"` picks when a model is open) |
| `db.plan(sql, params)`, `db.run(plan)` | a statement's [plan](query-language.md) as JSON, without running it; running a plan |
| `db.embed(text, {field, image, audio})`, `db.embedTokens(text, field)` | a query vector by an embedded field's model (of an image's or a sound's bytes too), a multi-vector field's token vectors |
| `db.queryLogged(sql, params, {context, variant, ...})`, `db.exportRankLog({...})` | the [rank log](query-language.md#the-rank-log-what-each-query-served): a ranked query that logs what it served, and the log as records for training |
| `db.sidNext(prefixes, {field, where})`, `db.trainCodebook(field, name)` | semantic IDs: the codes that can follow prefixes, and training a codebook |

Parameters are `:name` in the SQL and an object of values: numbers, strings, booleans, null, `Date`s, arrays (an array of numbers or a `Float32Array` is a vector), an object of numbers (sparse term weights), `{embed: "some text"}` (the text's vector by an embedded field's model) or `{vector_of: key}` (a stored row's vector).

A `Result` has `columns`, `rows` (arrays in column order), `keys` (each row's primary key), `scores` (what it was ranked by) and `stats` (what the engine did). Iterating it gives each row as an object; `r.at(i)`, `r.objects()`, `r.column(name)` and `r.scalar()` (a count, a sum) read it other ways.

### Writing

| call | what |
|---|---|
| `db.insert(rows, {table})` | rows whose keys are new; all or none are written. 10,000 rows or more are bulk-loaded into new segments in one commit |
| `db.upsert(rows, {table})` | rows, replacing the rows with their keys, in transactions of 10,000 |
| `db.delete(keys, {table})` | the rows with these keys |
| `db.write(ops)` | [write operations](server.md#http-api) in one transaction: insert, upsert, delete, `set` of a field (a number in place, another by writing the row again) and `add` to a number in place |
| `db.get(key)`, `db.getMany(keys)` | rows by key without SQL, or null |
| `db.addColumn(name, type, options)`, `dropColumn`, `renameColumn` | `ALTER TABLE` |
| `db.expire()`, `db.optimize()`, `db.vacuum()`, `db.close()` | rows past their `expire_by`; a table that is done changing readied for queries (its segments merged into as few as can be, deleted rows dropped, the file shrunk); the space merges freed returned to the file system; the database closed, after merging small segments and returning the space merges freed when it is an eighth of the file or more (`await using db = ...` closes it too). Segments merge in the background as writes add them, one merge at a time |

A row is an object of field name to value. A vector may be a `Float32Array` (read from its bytes, without a number per element in between), a `Float64Array` or an array; a TIMESTAMP a `Date` or ISO 8601 text; a list field an array or a `Set`. Values come back as strata-server answers them: TIMESTAMPs as ISO 8601 text in UTC, vectors as arrays of numbers.

An INT past 2^53 (a 64-bit key, a hash) comes back as a `BigInt`, and a `BigInt` may be written or passed as a parameter anywhere a number may; every other integer is a number. (Reading them whole takes a runtime whose `JSON.parse` gives a reviver the number's text: Node 21 and later, Deno, recent browsers.)

### Files

| call | what |
|---|---|
| `db.importFile(path, {format, table})` | a JSON Lines, JSON, CSV or TSV file: bulk-loaded into an empty table, else upserted; returns `{rows, bulk, skippedFields}` |
| `fromSqlite(sqlitePath, path)`, `planSqlite`, `db.importSqlite`, `db.exportSqlite` | [SQLite](sqlite.md) in and out |
| `fromParquet(parquetPath, path)`, `planParquet`, `db.importParquet`, `db.exportParquet` | [Parquet](parquet.md) files and directories in and out, and `s3://` URLs |
| `upgrade(path)` | a database from before field ids, copied into this version's format |

Errors reject with a `StrataError`, whose message is the database's.

## A server from JavaScript

```js
import { connect } from "strata-db/client";   // or from "strata-db"

const client = connect("http://127.0.0.1:7700", { token: process.env.STRATA_TOKEN });
await client.upsert([{ id: "b2", title: "A second book", year: 2001 }]);
const r = await client.search("book", { limit: 5 });
```

`Client` has the methods of `Database` that a server offers, over its [HTTP API](server.md#http-api), and more of the server's own: `health()`, `stats()`, `explain(sql)`, `put(key, row, {ifSeqNo})`, `stream(events)` (an iterable or async iterable of [stream events](server.md#streams), applied once), `source(name)` and `sources()`, `ask(question)` (the server's agent), and the file calls, which upload a path's bytes (Node, Bun, Deno) or bytes given (a `Uint8Array`, a `Blob`). `exportSqlite(null)` and `exportParquet(null)` give the file's bytes instead of writing it. Rows are written 1,000 a request, keys read 10,000 a request. An error the server answers rejects with a `StrataError` whose `status` is the HTTP status and `answer` its JSON; one where the server cannot be reached rejects with `fetch`'s error. `timeout` (milliseconds, 30 seconds by default) bounds each request, and `fetch` takes another fetch. A request the server refuses for its load (429: [behind on flushing](server.md#streams) its writes, its write queue full, too many queries running) did nothing, so the client sends it again after the `Retry-After` the server gives, for up to `retryFor` milliseconds (10 seconds; 0 sends each request once), and rejects with the 429 after that. A write that failed partway on a cluster (an error with `"partial": true`) is not sent again, since the shards that took it keep it, unless it has an idempotency key.

`write(ops, {idempotencyKey})` and `sql(sql, params, {idempotencyKey})` send a write with an [idempotency key](server.md#when-a-write-fails), so that sending it again does not apply it twice. A write that failed partway through a coordinator rejects with a `StrataError` whose `answer` says so (`partial: true`) and names its key: `await client.write(ops, { idempotencyKey: e.answer.idempotency_key })` writes the rest.

In a browser, the client works from pages of the server's own origin (a page it serves, or behind the same reverse proxy): strata-server sends no CORS headers, and refuses writes from pages of other sites ([security](server.md#security)).

## Search on the device

`openNearby(url, table, dir)` holds a [slice](slices.md) of a strata-server's table near a user: the rows of the tiles around them in a database in `dir`, searched with the server's statistics. `moveTo(lat, lon)` fetches the tiles it lacks, and `search(q, { k, d0 })` answers on the device when it can prove the answer is the server's, and asks the server otherwise; the result's `where` is `"device"` or `"server"` and `why` the reason.

```js
import { openNearby } from "strata-db";

const near = await openNearby("http://127.0.0.1:7700", "places", "./slice");
await near.moveTo(40.7484, -73.9857);
const r = await near.search("pizza", { k: 10 });
console.log(r.where, r.why);   // device proved
```

## Bun and Deno

Bun loads the addon as Node does: `bun run app.js`. Deno loads it through its Node compatibility with `--allow-ffi` (and `--allow-read`, `--allow-write`, `--allow-env` for the files and `$STRATA_MODELS`); the client needs only `--allow-net`.

## Tests

```sh
cd strata-js
npm run build -- --profile ci             # optimised, built faster than release
npm test                                  # node --test
```

The client's tests start `strata-server/target/release/strata-server` (or `$STRATA_SERVER`) and are skipped when it is not built.

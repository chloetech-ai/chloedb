# ChloeDB from JavaScript

The `chloedb` package opens a ChloeDB file inside a Node, Bun or Deno process, the way better-sqlite3 opens a SQLite file: no server, no configuration. It can also talk to a running [chloedb-server](server.md) from any JavaScript runtime with `fetch` (a browser, a worker, Node, Bun, Deno). Both have the same methods, so code written against a local file works against a server.

## Install

The package is in [`chloedb-js/`](https://github.com/chloetech-ai/chloedb/tree/main/chloedb-js): a Node-API addon written in Rust ([napi-rs](https://napi.rs)) and the JavaScript around it, an ES module, for Node 18 or later, Bun and Deno.

### Prebuilt

```sh
npm install chloedb
```

`bun add chloedb`, `pnpm add chloedb` and `yarn add chloedb` work the same way. npm installs the JavaScript and, as an optional dependency, the addon built for your machine (`@chloedb/darwin-arm64`, `@chloedb/linux-x64-gnu`, ...), so nothing is compiled: no Rust, no C compiler. It imports as `"chloedb"`. Windows and Alpine (musl) have no prebuilt addon: build from source there (Windows through WSL 2). Release 0.1.0, from before the rename, was published as `strata-db` and imports as `"strata-db"`.

| platform | addon package | release tarball |
|---|---|---|
| macOS 11+ on Apple silicon | `@chloedb/darwin-arm64` | `chloedb-<version>-darwin-arm64.tgz` |
| macOS 11+ on Intel | `@chloedb/darwin-x64` | `chloedb-<version>-darwin-x64.tgz` |
| Linux x86-64, glibc 2.35+ (Ubuntu 22.04, Debian 12 and later) | `@chloedb/linux-x64-gnu` | `chloedb-<version>-linux-x64-gnu.tgz` |
| Linux arm64, glibc 2.35+ | `@chloedb/linux-arm64-gnu` | `chloedb-<version>-linux-arm64-gnu.tgz` |

If importing `chloedb` throws an error that calls it a placeholder, npm installed `chloedb` 0.0.x, which only held the name until ChloeDB's releases were published there (the first one, 0.0.1, still says Strata, ChloeDB's name before the rename). `npm install chloedb@latest` in that app replaces it and rewrites the range in `package.json` (a plain `npm install chloedb` keeps the `^0.0.x` range already there, and the placeholder with it).

Each `chloedb-js-v<version>` release on [GitHub](https://github.com/chloetech-ai/chloedb/releases) also has a tarball per platform with the addon built in, for a machine that cannot reach npm's registry. Install the tarball for your platform, or its URL:

```sh
npm install https://github.com/chloetech-ai/chloedb/releases/download/chloedb-js-v0.3.7/chloedb-0.3.7-darwin-arm64.tgz
```

The workflow [`js-release.yml`](https://github.com/chloetech-ai/chloedb/blob/main/.github/workflows/js-release.yml) builds both on a `chloedb-js-v<version>` tag, the way napi-rs packages are laid out (`chloedb` names each platform's package as an optional dependency, of which npm installs the one of your machine), and installs each into an empty app and runs it there before attaching the tarballs to the release and publishing the packages to npm (the workflow's header says what publishing needs). `node scripts/pack.mjs <platform>` packs a tarball by hand, and `--npm <platform>` and `--npm-main` the packages npm gets.

### From source

It needs Rust (stable) and a C compiler.

```sh
cd chloedb-js
npm run build                    # cargo build --release, then copies the addon to chloedb.node
cd /path/to/my-app
npm install /path/to/chloedb/chloedb-js  # or "chloedb": "file:/path/to/chloedb/chloedb-js" in package.json
```

`npm install` of the directory builds the addon itself when `chloedb.node` is missing. `npm run build -- --features bert` builds more of ChloeDB's features in (`bert`, `onnx`, `vision`, `audio`; `datafusion`, `parquet` and `s3` are on by default). `$CHLOEDB_NODE_ADDON` (or `$STRATA_NODE_ADDON`) loads an addon from another place.

The client alone (`chloedb/client`) is plain JavaScript with no addon: a bundler takes it into a page, and the package's `browser` and `workerd` conditions give it for `chloedb` too.

## A database in the process

```js
import { open } from "chloedb";

const db = await open("books.chloedb", {
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

`open(path, {schema})` opens the file, or creates it with the schema when it does not exist; `Database.create(path, schema)` and `Database.open(path)` do one or the other. A schema is an object, JSON text or the path of a JSON file, in the [short form or the long one](server.md#schemas). `directory: true` keeps a database as a directory of segment files, `models` is where embedded fields' models are (`$CHLOEDB_MODELS`, else `~/.local/share/chloedb/models` (or `~/.local/share/strata/models`, on a machine that has that folder from before the rename and no `~/.local/share/chloedb`; before 0.3.2, always); `chloedb-server fetch-model` fetches them), and `threads` how many cores index builds and each query use (every core by default).

Every call answers a promise and runs on a worker thread of Node's pool, so queries and writes leave the event loop free and many run at once. Writes made at once wait for each other and share commits (one disk flush for all those waiting), so none fails as busy. One process at a time may have a database open to write it, and only once: a second `open` of the file, by another process or by the same one, gets "busy: the database is open elsewhere", so a program opens it once and shares the `Database` (in a dev server that runs modules again, [on `globalThis`](#the-same-in-the-process)). Others can open it with `Database.open(path, { readOnly: true })` to read it while it is written, the workers of a cluster, say: each query sees the writer's commits so far, every write fails, and `refresh()` looks for new commits at once ([how readers follow the writer](index.md#readers-in-other-processes)). For writes from several processes, serve it with chloedb-server.

### Querying

| call | what |
|---|---|
| `db.query<T>(sql, params, {timeoutMs, memory, signal, pageSize, keepAliveMs})` | a SELECT: a `Result` ([limits](#limits-memory-deadlines-aborting)); with `pageSize`, its first page and a `cursor` ([past the window](#reading-past-the-window-cursors)) |
| `db.sql<T>(sql, params, {timeoutMs, memory, signal})` | any statement: a SELECT's `Result`, or the rows an INSERT, UPDATE or DELETE wrote; `CREATE TABLE`, `ALTER TABLE` and `DROP TABLE` too |
| `db.execute(sql, params)` | an INSERT, UPDATE or DELETE (or a `CREATE`, `ALTER` or `DROP`): the rows it wrote, a number; a SELECT is refused (`TypeError`) |
| `db.next<T>(cursor, {keepAliveMs})`, `db.closeCursor(cursor)`, `db.scroll<T>(sql, params, {pageSize})` | a cursor's next page (its `cursor` null after the last), a cursor ended (true if it was open), and every page of a SELECT as an async iterator |
| `db.highlight(query, texts, {field, size, pre, post, html})` | `texts` (one, or a list) with the words `query` matches marked, as TEXT field `field` reads them (stems, synonyms, typos, `so*`), cut to about `size` characters (160; 0: whole), HTML-escaped around `<mark>` tags |
| `db.correct(text, {field})` | "did you mean": `{text, changed, words}`, the words no row holds and that look like typos replaced by those meant, each word `{word, term, to, toWord, edits, chance}` ([did you mean](query-language.md#did-you-mean-correcting-a-querys-words)) |
| `db.search(text, {limit, where, columns, mode, textFields, vectorField, params, table})` | the rows that best match a text: BM25 over the text fields (`mode: "text"`), the similarity of an embedded vector field (`"vector"`), or both fused by reciprocal rank (`"hybrid"`, what `"auto"` picks when a model is open) |
| `db.plan(sql, params)`, `db.run(plan)` | a statement's [plan](query-language.md) as JSON, without running it; running a plan |
| `db.embed(text, {field, image, audio})`, `db.embedTokens(text, field)` | a query vector by an embedded field's model (of an image's or a sound's bytes too), a multi-vector field's token vectors |
| `db.queryLogged(sql, params, {context, variant, ...})`, `db.exportRankLog({...})` | the [rank log](query-language.md#the-rank-log-what-each-query-served): a ranked query that logs what it served, and the log as records for training |
| `db.sidNext(prefixes, {field, where})`, `db.trainCodebook(field, name, {balanced})` | semantic IDs: the codes that can follow prefixes, and training a codebook (`balanced: true` uses every level's numbers evenly) |

Parameters are `:name` in the SQL and an object of values: numbers, strings, booleans, null, `Date`s, arrays (an array of numbers or a `Float32Array` is a vector), an object of numbers (sparse term weights), `{embed: "some text"}` (the text's vector by an embedded field's model) or `{vector_of: key}` (a stored row's vector).

A `Result` has `columns`, `rows` (arrays in column order), `keys` (each row's primary key), `scores` (what it was ranked by), `stats` (what the engine did) and, a page of a cursor's, `cursor`. Iterating it gives each row as an object; `r.at(i)`, `r.objects()`, `r.column(name)` and `r.scalar()` (a count, a sum) read it other ways.

`field` may be left out of `highlight` and `correct` when the table has one indexed TEXT field. Both are chloedb-server's routes (`POST /v1/highlight` and `POST /v1/correct`) on a `Client`, and the same answers from the database in the process, so a search page's code runs on either.

### Reading past the window: cursors

A query answers at most 10,000 rows, counted to the end of its `OFFSET`, as chloedb-server's does (`--max-result-window`): `LIMIT 10 OFFSET 10000` is refused with a `BadRequestError` that says how to read on, and so is a query without `LIMIT` that matches more. (**changed:** a `Database` had no window before, and the same query worked in the process and failed on a server.) `{ maxResultWindow: 0 }` when it opens, or `db.setLimits({ maxResultWindow: 0 })`, lifts it for that database, as `--max-result-window 0` does for a server; another number moves it.

Every row of a query, past the window, comes a page at a time from a cursor. `pageSize` on `query` answers the first page and its `cursor`; `next(cursor)` answers the next one, until a page whose `cursor` is null. Every page reads the version of the database the first one read, so rows written or deleted meanwhile neither appear nor go missing, and a page holds at most the window's rows:

```js
let page = await db.query("SELECT id, name FROM products ORDER BY price", null, { pageSize: 1000 });
for (;;) {
  send(page.objects());
  if (!page.cursor) break;
  page = await db.next(page.cursor);
}

// The same, closing the cursor when the loop is left early:
for await (const page of db.scroll("SELECT id, name FROM products ORDER BY price", null, { pageSize: 1000 })) send(page.objects());
```

A cursor not read for `keepAliveMs` (60 seconds, at most an hour) is gone, and `next` rejects with `NotFoundError`; `closeCursor(cursor)` ends one sooner. A plain scan of one table (no `ORDER BY`, groups or counts) reads its pages as they are asked for; any other query runs whole when the cursor starts and holds what it answered (an ordered one, its rows' places and scores; a join or groups, the rows), up to a million rows, the most the cursors of one database hold at once, as a server's `--max-scroll-rows`. On a `Client` the cursor is the server's [scroll](server.md#reading-past-the-window-scrolls), with the same options.

### Limits: memory, deadlines, aborting

A query runs in your process, so what it builds up (its groups, the rows it orders and returns, a join's tables) is held by your program. The queries of every database a process opens share a memory budget, by default a quarter of the machine's memory (the cgroup's limit in a container, else RAM), or `CHLOEDB_QUERY_MEMORY_MB` MiB when that is set when the first database opens (`0`: no budget). A query that would hold more rejects with a `ChloeDBError` of code `memory`, naming the budget and how to raise it, and gives back what it held, instead of the process running out of memory.

A query has no deadline unless it is given one. `db.query`, `db.sql`, `db.run` and `db.search` take a last argument of options: `timeoutMs` (its deadline, as a `Client`'s `query` takes it: it rejects with `DeadlineError` naming it), `memory` (a budget of its own in bytes; `0`: none) and `signal`, an `AbortSignal` that stops it when it aborts (it rejects with the signal's reason, as an aborted `fetch` does). A database's own limits, for every query that does not give its own, are set when it opens or later; `db.interrupt()` stops every query of the database running now, and returns how many there were:

```js
const db = await open("books.chloedb", { timeoutMs: 5000, memory: 512 * 2 ** 20 });  // 5 s, and 512 MiB its queries share
await db.query("SELECT genres, COUNT(*) FROM books GROUP BY genres", null, { timeoutMs: 200 });
await db.query("SELECT * FROM books", null, { memory: 0, signal: AbortSignal.timeout(1000) });
db.setLimits({});                 // back to the defaults: no deadline, the process's budget, 10,000 rows
db.timeoutMs, db.memory, db.maxResultWindow;   // the limits now (null: none)
```

A query checks its deadline and its signal between segments and every 1,024 rows it groups, orders, walks or joins, so it stops within milliseconds; it holds a thread of libuv's pool (`UV_THREADPOOL_SIZE`, 4 by default, which `fs` and `dns` share) until then. A write is not cut short: `sql`'s limits apply to a SELECT.

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

A row is an object of field name to value. A vector may be a `Float32Array` (read from its bytes, without a number per element in between), a `Float64Array` or an array; a TIMESTAMP a `Date` or ISO 8601 text; a list field an array or a `Set`. Values come back as chloedb-server answers them: TIMESTAMPs as ISO 8601 text in UTC, vectors as arrays of numbers.

An INT past 2^53 (a 64-bit key, a hash) comes back as a `BigInt`, and a `BigInt` may be written or passed as a parameter anywhere a number may; every other integer is a number. (Reading them whole takes a runtime whose `JSON.parse` gives a reviver the number's text: Node 21 and later, Deno, recent browsers.)

### Files

| call | what |
|---|---|
| `db.importFile(path, {format, table})` | a JSON Lines, JSON, CSV or TSV file: bulk-loaded into an empty table, else upserted; returns `{rows, bulk, skippedFields}` |
| `fromSqlite(sqlitePath, path)`, `planSqlite`, `db.importSqlite`, `db.exportSqlite` | [SQLite](sqlite.md) in and out |
| `fromParquet(parquetPath, path)`, `planParquet`, `db.importParquet`, `db.exportParquet` | [Parquet](parquet.md) files and directories in and out, and `s3://` URLs |
| `upgrade(path)` | a database from before field ids, copied into this version's format |

Errors reject with a `ChloeDBError`, whose message is the database's (`StrataError`, its name before the rename, is the same class), and whose `code` is stable, the same here, from a server and in Python: the server's JSON `code` when it gives one, else the kind the message starts with (`busy: ...`), else the HTTP status. The common codes reject as subclasses of `ChloeDBError`, so `instanceof ChloeDBError` still holds for every one:

| code | class | when |
|---|---|---|
| `bad_request` | `BadRequestError` | a bad schema, query, value or request (400, 413, 422) |
| `unauthorized` | `UnauthorizedError` | no token, or one without the rights needed (401, 403) |
| `not_found` | `NotFoundError` | no such route, table or cursor (404) |
| `conflict` | `ConflictError` | refused by the state it met (409): an `ifSeqNo` that no longer holds, an idempotency key still running, a `minSeqNo` past the server's last write |
| `partial_write` | `PartialWriteError` | a coordinator wrote some shards and not others: send it again with the same idempotency key |
| `busy` | `BusyError` | the database is open to write elsewhere, in another process or by another open in this one (the message says which, and what to do instead), or another writer holds it |
| `overloaded` | `OverloadedError` | refused for the server's load (429), after the client's retries |
| `timeout` | `DeadlineError` | a deadline passed: `timeoutMs`, a coordinator's wait for a shard (504) |
| `unavailable` | `UnavailableError` | no copy can serve it now (503) |

A write through a coordinator that may or may not be applied (its shard's primary stopped answering, or was replaced while the write was being acknowledged) keeps its status's class, `UnavailableError` (503) or `ConflictError` (409), and says so: `err.inDoubt` is true and `err.idempotencyKey` is the key to send it again with (`{ idempotencyKey }`), which applies it once; without the key it could be applied twice ([Failover](server.md#failover)).

Other codes are `memory` (a query past its memory budget), `corrupt`, `io`, `internal` and `error` (which is also what `open` of a file that does not exist, without a schema, rejects with: "x.chloedb does not exist: pass a schema to create it"); `errorCode(message, status, answer)` gives the code of any message.

## A server from JavaScript

```js
import { connect } from "chloedb/client";   // or from "chloedb"

const client = connect("http://127.0.0.1:7700", { token: process.env.CHLOEDB_TOKEN });
await client.upsert([{ id: "b2", title: "A second book", year: 2001 }]);
const r = await client.search("book", { limit: 5 });
```

`Client` has the methods of `Database` that a server offers, over its [HTTP API](server.md#http-api), and more of the server's own: `health()`, `stats()`, `explain(sql)`, `put(key, row, {ifSeqNo})`, `stream(events)` (an iterable or async iterable of [stream events](server.md#streams), applied once), `source(name)` and `sources()`, `ask(question)` (the server's agent), and the file calls, which upload a path's bytes (Node, Bun, Deno) or bytes given (a `Uint8Array`, a `Blob`). `exportSqlite(null)` and `exportParquet(null)` give the file's bytes instead of writing it. Rows are written 1,000 a request, keys read 10,000 a request. An error the server answers rejects with a `ChloeDBError` whose `status` is the HTTP status and `answer` its JSON; one where the server cannot be reached rejects with `fetch`'s error. `count()` reads the server's `/v1/health`, whose `docs` counts its first table: on a server started without a schema, whose tables were made since by `CREATE TABLE`, that is an empty placeholder table, and `count()` answers 0; `(await client.query("SELECT COUNT(*) FROM products")).scalar()` counts a table on any server. `highlight`, `correct` and cursors (`pageSize`, `next`, `closeCursor`, `scroll`) are the server's `POST /v1/highlight`, `POST /v1/correct` and [scrolls](server.md#reading-past-the-window-scrolls). `timeout` (milliseconds, 30 seconds by default) bounds each request, and `fetch` takes another fetch. A request the server refuses for its load (429: [behind on flushing](server.md#streams) its writes, its write queue full, too many queries running) did nothing, so the client sends it again after the `Retry-After` the server gives, for up to `retryFor` milliseconds (10 seconds; 0 sends each request once), and rejects with the 429 after that. A write that failed partway on a cluster (an error with `"partial": true`) is not sent again, since the shards that took it keep it, unless it has an idempotency key.

`write(ops, {idempotencyKey})` and `sql(sql, params, {idempotencyKey})` send a write with an [idempotency key](server.md#when-a-write-fails), so that sending it again does not apply it twice. A write that failed partway through a coordinator rejects with a `ChloeDBError` whose `answer` says so (`partial: true`) and names its key: `await client.write(ops, { idempotencyKey: e.answer.idempotency_key })` writes the rest.

### Reading your own writes

A read from a [replica](server.md#replication-a-primary-and-its-replicas), or through a coordinator whose shards have replicas, can be answered by a copy that has not applied a write yet. Servers that number their writes (`--seq-no`) fix that with sequence numbers ([reads that see a write](server.md#reads-that-see-a-write)):

- `writeResult(ops, {idempotencyKey})` is `write` answering `{written, seqNo, seqNos, skipped}`: `seqNo` from a server, `seqNos` (one per shard) from a coordinator, neither from a server without `--seq-no`. `write`, `insert`, `upsert`, `delete`, `put` and `sql` still answer the number of rows.
- `query(sql, params, {minSeqNo, minSeqNos})`, `search(text, {minSeqNo, minSeqNos})`, `get(key, {minSeqNo, minSeqNos})`, `getMany`, `sql` (a SELECT), `run` (a read plan) and `sidNext` take them: the copy that answers holds those writes (a replica waits up to a second; a server refuses a number past its last write, 409).
- `client.session()` (or `new Session(url, options)`) is a client that does this itself: it keeps the latest numbers its own writes answered (`session.seqNo`, `session.seqNos`) and sends them with each of its reads, so reading your own writes is one line. `session.observe(answer)` adds another client's write.

```js
const s = connect("http://coordinator:7700").session();
await s.upsert({ id: "u1", name: "Ada" });
await s.get("u1"); // sees the upsert, whichever copy answers

const w = await client.writeResult([{ op: "upsert", doc: { id: "u2", name: "Grace" } }]);
await client.get("u2", { minSeqNo: w.seqNo, minSeqNos: w.seqNos });
```

The numbers go with reads only: a server refuses them on a write, so a session leaves them off an `INSERT`, `UPDATE` or `DELETE` by `sql`, judging a statement by its first word. A `Database` in the process ignores them: it reads every write it committed.

## A search box in a web app

A search box sends what was typed to your app's own server code, which queries ChloeDB and answers the page. A page cannot query chloedb-server itself unless chloedb-server serves the page's origin.

### Why a page cannot call chloedb-server

chloedb-server refuses every request but `GET` and `HEAD` that carries an `Origin` other than the server's own, reads included: a `POST /v1/query` with a `SELECT` from a page on `http://localhost:3000` to a server on port 7700 gets 403, "a page from http://localhost:3000 cannot change this server". Its `error_code`, `scope_refused`, and its hint to ask for a key with another scope do not apply: no key lets such a request through. The server sends no CORS headers and answers a preflight `OPTIONS` with 405, so a browser does not even send the client's `POST` (it sends JSON, which needs the preflight), and the call rejects with `fetch`'s network error, which the browser's console shows as a CORS error. There is no search-only key to put in a page, as Algolia has one.

So the page calls a route of your app, and the route queries ChloeDB: over HTTP from the server side, where `fetch` sends no `Origin`, or in the process, with the database open there. A page calls chloedb-server directly only from the server's own origin: a page it serves, or a page behind a reverse proxy that serves your app and ChloeDB under one host name, passes the `Host` header on, and starts chloedb-server with `--allow-host` and that name ([security](server.md#security)).

### Search as you type

`search()` ranks by `bm25` of its text as written, so a word half typed (`so`) finds nothing; `so*` and `lether~` are a prefix and a word with its typos there, as in a query, where `ORDER BY bm25(name, 'sof*')` ranks the rows `MATCH(name, 'sof*')` keeps ([weights, required and excluded words](query-language.md#weights-required-and-excluded-words); **changed:** `bm25` read `sof*` as the word *sof* before, and returned no rows). A search box writes its own query: it filters with `MATCH` by what was typed, its last word a prefix while it is being typed, and ranks by `bm25` of the words typed whole, or by a column while the first word is still being typed. `fuzzy => 'auto'` takes typos in the words (`lether`). These are the queries for a table `products` (`id`, a TEXT `name`, a KEYWORD `brand`, `price`, `rating` and `in_stock`), shared by the two routes below:

```ts
// lib/search.ts: the queries of a search box, for what was typed so far
export type Hit = { id: string; name: string; brand: string; price: number };

/** The words typed, lowercased and of letters and digits only, so that none is an operator of MATCH. */
export function words(typed: string) {
  const all = typed.toLowerCase().match(/[\p{L}\p{N}]+/gu) ?? [];
  const typing = all.length > 0 && !/\s$/.test(typed); // the last word is still being typed
  const whole = typing ? all.slice(0, -1) : all;
  return { text: all.join(" "), match: all.join(" ") + (typing ? "*" : ""), whole: whole.join(" ") };
}

const WHERE = "MATCH(name, :match, fuzzy => 'auto') AND in_stock";

/** The hits, best first: by the words typed whole, or by rating while there are none. */
export function hitsSql(whole: string) {
  const order = whole ? "bm25(name, :whole, fuzzy => 'auto') * (1 + 0.1 * rating) DESC" : "rating DESC, id";
  return `SELECT id, name, brand, price FROM products WHERE ${WHERE} ORDER BY ${order} LIMIT 10`;
}

/** The brands of all the rows that match, with how many each: a facet. */
export const BRANDS_SQL = `SELECT brand, COUNT(*) AS n FROM products WHERE ${WHERE} GROUP BY brand ORDER BY n DESC LIMIT 10`;
```

Typed `lether so`, the hits are filtered by `MATCH(name, 'lether so*', fuzzy => 'auto')` and ranked by `bm25(name, 'lether', fuzzy => 'auto')`; typed `so`, they are filtered by `MATCH(name, 'so*', fuzzy => 'auto')` and ranked by rating.

### A route over HTTP

A Next.js route handler (the App Router's `app/api/search/route.ts`) answers the hits, a facet of their brands, each hit's name with the words that matched marked (`highlight`, the server's [`POST /v1/highlight`](server.md#http-api)) and a "did you mean" (`correct`, [`POST /v1/correct`](query-language.md#did-you-mean-correcting-a-querys-words)). The client needs nothing but `fetch`, so it runs on Node and on the Edge runtime alike:

```ts
// app/api/search/route.ts: the search box's answers, from chloedb-server
import { connect } from "chloedb/client";
import { BRANDS_SQL, hitsSql, words, type Hit } from "../../../lib/search";

const chloedb = connect(process.env.CHLOEDB_URL ?? "http://127.0.0.1:7700", { token: process.env.CHLOEDB_TOKEN });

export async function GET(request: Request) {
  const { text, match, whole } = words(new URL(request.url).searchParams.get("q") ?? "");
  if (!text) return Response.json({ hits: [], brands: [], didYouMean: null });
  const [hits, brands, corrected] = await Promise.all([
    chloedb.query<Hit>(hitsSql(whole), { match, whole }),
    chloedb.query(BRANDS_SQL, { match }),
    chloedb.correct(text, { field: "name" }),
  ]);
  const marked = await chloedb.highlight(match, hits.column("name"), { field: "name" });
  return Response.json({
    hits: hits.objects().map((hit, i) => ({ ...hit, marked: marked[i] })),
    brands: brands.objects(),
    didYouMean: corrected.changed ? corrected.text : null,
  });
}
```

`GET /api/search?q=lether%20so` answers:

```json
{"hits": [{"id": "p1", "name": "Leather sofa", "brand": "acme", "price": 318.5, "marked": "<mark>Leather</mark> <mark>sofa</mark>"}, ...],
 "brands": [{"brand": "acme", "n": 1}, ...],
 "didYouMean": "leather so"}
```

`marked` is HTML-escaped around its `<mark>` tags, so the page can set it as HTML (React's `dangerouslySetInnerHTML`); every other value is plain text, for the page to escape as usual. `correct` changes a word only when no row holds it and a neighbour one edit away is in 5 rows or more, so a small table gets few corrections. A deep page of hits comes from a [cursor](#reading-past-the-window-cursors) (`pageSize`, then `next(cursor)`), as `OFFSET` past 10,000 rows is refused.

### The same in the process

With the database in the app's process, there is no server to run, and the route is the same: a `Database` has `highlight` and `correct` too, and answers them as the server does. Open the file once and share the `Database`:

```ts
// lib/chloedb.ts: the one Database of the process
import { open, type Database } from "chloedb";

// `next dev` runs a module again when it changes, and a second open of the
// file in one process is refused as busy: keep the first on globalThis.
const g = globalThis as typeof globalThis & { chloedb?: Promise<Database> };

export function db(): Promise<Database> {
  return (g.chloedb ??= open(process.env.CHLOEDB_PATH ?? "products.chloedb"));
}
```

```ts
// app/api/search/route.ts, with the database in the process
import { db } from "../../../lib/chloedb";
import { BRANDS_SQL, hitsSql, words, type Hit } from "../../../lib/search";

export async function GET(request: Request) {
  const { text, match, whole } = words(new URL(request.url).searchParams.get("q") ?? "");
  if (!text) return Response.json({ hits: [], brands: [], didYouMean: null });
  const products = await db();
  const [hits, brands, corrected] = await Promise.all([
    products.query<Hit>(hitsSql(whole), { match, whole }),
    products.query(BRANDS_SQL, { match }),
    products.correct(text, { field: "name" }),
  ]);
  const marked = await products.highlight(match, hits.column("name"), { field: "name" });
  return Response.json({
    hits: hits.objects().map((hit, i) => ({ ...hit, marked: marked[i] })),
    brands: brands.objects(),
    didYouMean: corrected.changed ? corrected.text : null,
  });
}
```

Next.js bundles server code, and its bundler cannot take the addon (`next build` stops with "chloedb.node (node addon) is not placeable in ESM chunks"), so leave the package out of the bundle:

```js
// next.config.mjs
export default { serverExternalPackages: ["chloedb"] };
```

The route runs on Node, Next.js's default for route handlers: on the Edge runtime (`export const runtime = "edge"`) `"chloedb"` is the client alone, which has no `open`. One process may open the file to write it. When another process writes it (chloedb-server, an import job), or the app runs as several processes, open it with `{ readOnly: true }` in the app: each query sees the writer's commits so far.

### What a route sends

`Response.json(result)` sends the whole `Result`: `columns`, the `rows` as arrays, `keys`, `scores` and `stats`, whose `plan` lists what the engine did (`"seg 0: bm25 MaxScore, filter Probe"`). Send `result.objects()`, the rows as objects, or what you make of them, as the routes above do. An INT past 2^53 comes back as a `BigInt`, which `JSON.stringify` and `Response.json` refuse ("Do not know how to serialize a BigInt"); write it as text first, `JSON.stringify(rows, (k, v) => (typeof v === "bigint" ? String(v) : v))`.

### TypeScript

`query()` answers a `Result` whose rows are `Record<string, Value>`; `query<Hit>()` (and `sql<Hit>()`, `next<Hit>()`, `scroll<Hit>()`, `get<Hit>()`, `getMany<Hit>()`) types its rows as `Hit`, a `type` or an `interface`. Nothing checks the type against the query's columns: it says what the query answers. `sql()` answers `Result | number`, a SELECT's rows or the rows a write wrote; `execute()` answers a number. A schema's field types are checked (`type: "keywrd"` is an error).

The declarations type-check as they are, with `skipLibCheck` off, any `target` from ES2020 and no Node types: they name TypeScript's `esnext.disposable` library themselves, for `Symbol.asyncDispose` (`await using db = await open(...)`), so code for a page or a worker needs neither `"types": ["node"]` nor `"lib": [..., "ESNext.Disposable"]`. (**changed:** before, with TypeScript 7, which includes no `@types` package that `types` does not name, `tsc` stopped with "Property 'asyncDispose' does not exist on type 'SymbolConstructor'", a row type was a cast, `as Result<Hit>`, and an `interface` was refused for one.)

## Search on the device

`openNearby(url, table, dir)` holds a [slice](slices.md) of a chloedb-server's table near a user: the rows of the tiles around them in a database in `dir`, searched with the server's statistics. `moveTo(lat, lon)` fetches the tiles it lacks, and `search(q, { k, d0 })` answers on the device when it can prove the answer is the server's, and asks the server otherwise; the result's `where` is `"device"` or `"server"` and `why` the reason. For a [vector slice](slices.md#vector-slices-more-like-this-on-the-device), `focus(taste, { rows })` holds the clusters nearest a taste vector and `similar(vector, { k })` answers a similarity search the same way, fetching the few clusters its codes name when that proves the answer.

```js
import { openNearby } from "chloedb";

const near = await openNearby("http://127.0.0.1:7700", "places", "./slice");
await near.moveTo(40.7484, -73.9857);
const r = await near.search("pizza", { k: 10 });
console.log(r.where, r.why);   // device proved
```

## Bun and Deno

Bun loads the addon as Node does: `bun run app.js`. Deno loads it through its Node compatibility with `--allow-ffi` (and `--allow-read`, `--allow-write`, `--allow-env` for the files and `$CHLOEDB_MODELS`); the client needs only `--allow-net`.

## Tests

### Your app's tests

A test opens a database of its own. `open(":memory:", { schema })` (or `Database.create(":memory:", schema)`) is a new, empty one each time, in a directory of its own under the system's temporary directory that `close()` removes (and the process's exit, for one left open); `open(":memory:")` without a schema, or `Database.open(":memory:")`, is refused, as there is nothing to open. (**changed:** `open(":memory:")` made a file named `:memory:` in the working directory before, which the next run opened again.)

```js
import { test } from "node:test";
import assert from "node:assert/strict";
import { open } from "chloedb";

const SCHEMA = {
  table: "products", key: "id",
  fields: [
    { name: "id", type: "keyword" }, { name: "name", type: "text" },
    { name: "rating", type: "float" }, { name: "in_stock", type: "bool" },
  ],
};

test("a word being typed finds the sofa", async () => {
  const db = await open(":memory:", { schema: SCHEMA });
  try {
    await db.upsert([
      { id: "p1", name: "Leather sofa", rating: 4.1, in_stock: true },
      { id: "p2", name: "Leather armchair", rating: 3.9, in_stock: true },
    ]);
    const r = await db.query(
      "SELECT id FROM products WHERE MATCH(name, :match, fuzzy => 'auto') AND in_stock ORDER BY bm25(name, :whole, fuzzy => 'auto') DESC LIMIT 10",
      { match: "lether so*", whole: "lether" },
    );
    assert.deepEqual(r.column("id"), ["p1"]);
  } finally {
    await db.close();
  }
});
```

A test of a file opens it in a directory of its own (`mkdtemp`) and removes the directory after: a database in a file is the file and two entries beside it, `<name>-wal` and the directory `<name>-readers`.

A test against chloedb-server starts a server of its own, on other ports than the app's server: `--listen` moves only the HTTP port, and a second server left on gRPC's 7701 stops with "the address is in use", so give it `--grpc` another port too, or `--grpc off`:

```sh
chloedb-server --db "$(mktemp -d)/test.chloedb" --listen 127.0.0.1:7710 --grpc off
```

### The package's tests

```sh
cd chloedb-js
npm run build -- --profile ci             # optimised, built faster than release
npm test                                  # node --test
```

The client's tests start `chloedb-server/target/release/chloedb-server` (or `$CHLOEDB_SERVER`) and are skipped when it is not built. `test/types.test.mjs` type-checks `test/types/example.ts`, a typed app, with TypeScript 7.0.2 (by `npx`, or `$CHLOEDB_TSC`) and no Node types, and is skipped only when `npx` cannot fetch it.

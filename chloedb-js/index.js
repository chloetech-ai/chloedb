// ChloeDB from JavaScript: the database in this process.
//
// `Database` opens a ChloeDB file (or a directory of segment files) in this
// process, with no server, as a SQLite library opens a SQLite file: Node,
// Bun and Deno load the native addon (chloedb.node) that runs it. `Client`
// (client.js) talks to a running chloedb-server over HTTP instead, from any
// JavaScript runtime with fetch. Both answer queries with the same
// `Result` and write rows the same way, so code can move from one to the
// other:
//
//   import { open } from "chloedb";
//
//   const db = await open("books.chloedb", { schema: {
//     table: "books", key: "id",
//     fields: [
//       { name: "id", type: "keyword" },
//       { name: "title", type: "text" },
//       { name: "year", type: "int" },
//     ],
//   } });
//   await db.upsert([{ id: "b1", title: "The river and the dragon", year: 1999 }]);
//   for (const row of await db.search("dragon")) console.log(row.id, row.title);
//   (await db.query("SELECT year, COUNT(*) FROM books GROUP BY year")).rows;
//   await db.close();
//
// Every call runs on a worker thread and answers a promise: queries and
// writes leave the event loop free, and many run at once.

import { Buffer } from "node:buffer";
import { createRequire } from "node:module";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

import { Nearby } from "./nearby.js";
import { ChloeDBError, Result, Searchable, decode, encode, given, makeError, orderedSchema, pickTable, resultOrWritten } from "./common.js";

export { ChloeDBError, StrataError, Result, errorCode, BadRequestError, UnauthorizedError, NotFoundError, ConflictError, PartialWriteError, BusyError, OverloadedError, DeadlineError, UnavailableError } from "./common.js";
export { Client, Session, connect } from "./client.js";
export { Nearby, NearbyResult } from "./nearby.js";

const here = dirname(fileURLToPath(import.meta.url));

/** This machine as the platform packages are named: linux-x64-gnu,
 * darwin-arm64 (scripts/pack.mjs). */
function platformName() {
  if (process.platform !== "linux") return `${process.platform}-${process.arch}`;
  let glibc = false;
  try {
    glibc = Boolean(process.report?.getReport?.()?.header?.glibcVersionRuntime);
  } catch {
    glibc = false;
  }
  return `linux-${process.arch}-${glibc ? "gnu" : "musl"}`;
}

/** The native addon: `$CHLOEDB_NODE_ADDON` (or `$STRATA_NODE_ADDON`), else
 * chloedb.node beside this file (`npm run build` makes it, and a release's
 * tarball has it), else the one of this platform's package,
 * `@chloedb/<platform>`, which `npm install chloedb` from npm's registry
 * installs beside it. */
function load() {
  const require = createRequire(import.meta.url);
  const given = process.env.CHLOEDB_NODE_ADDON || process.env.STRATA_NODE_ADDON;
  const beside = join(here, "chloedb.node");
  if (given || existsSync(beside)) {
    const path = given || beside;
    if (!existsSync(path)) throw new ChloeDBError(`the ChloeDB addon ${path} is missing`);
    return require(path);
  }
  const platform = platformName();
  let found;
  try {
    found = require.resolve(`@chloedb/${platform}/chloedb.node`);
  } catch {
    throw new ChloeDBError(
      `the ChloeDB addon is not installed: no ${beside}, and no package @chloedb/${platform} ` +
        `(npm installs the one of a platform with a prebuilt addon: linux-x64-gnu, linux-arm64-gnu, darwin-arm64, darwin-x64; ` +
        `elsewhere, run \`npm run build\` in chloedb-js, which needs Rust and a C compiler). chloedb/client works without it.`,
    );
  }
  return require(found);
}

const native = load();

/** A native error (a failed call) as a ChloeDBError. */
function failed(e) {
  if (e instanceof ChloeDBError) return e;
  return makeError(e?.message ?? String(e), { cause: e });
}

async function callStatic(op, args) {
  try {
    return decode(await native.callStatic(op, encode(given(args))));
  } catch (e) {
    throw failed(e);
  }
}

/** A writer's open refused (the engine says by whom and why), with the fixes
 * in JavaScript's words: a dev server's hot reload that runs a module again
 * in the same process (DEV-11 of the persona study of 17 November 2026), or
 * another program. */
function busy(e, path) {
  if (e?.code !== "busy" || !/open elsewhere/.test(e.message)) return e;
  const p = JSON.stringify(String(path));
  const hint = /this process has it open/.test(e.message)
    ? `In JavaScript: share one Database, kept on globalThis so that a hot reload finds it (globalThis.db ??= await open(${p})), or open(${p}, { readOnly: true })`
    : `In JavaScript: open(${p}, { readOnly: true }), or connect("http://127.0.0.1:7700") to chloedb-server --db ${String(path)}`;
  return makeError(`${e.message}. ${hint}`, { cause: e });
}

async function opened(op, args, limits = {}, temp) {
  let db;
  try {
    db = new Database(await native.NativeDatabase.open(op, encode(given(args))), temp);
  } catch (e) {
    if (temp) gone(temp);
    throw op === "open" ? busy(failed(e), args.path) : failed(e);
  }
  if (limits.timeoutMs != null || limits.memory != null || limits.maxResultWindow != null) db.setLimits(limits);
  return db;
}

// ---------------------------------------------------------------- :memory:

/** The path that names a database of its own each time, as SQLite's does:
 * one in a new temporary directory, removed when it closes or the process
 * exits (DEV-208 of the persona study of 17 November 2026: it was a file
 * named ":memory:" in the working directory, kept between runs). */
const MEMORY = ":memory:";

const temps = new Set();
process.on("exit", () => {
  for (const d of temps) rmSync(d, { recursive: true, force: true });
});

/** A new temporary directory for a database of `:memory:`. */
function tempHome() {
  const d = mkdtempSync(join(tmpdir(), "chloedb-memory-"));
  temps.add(d);
  return d;
}

function gone(dir) {
  temps.delete(dir);
  rmSync(dir, { recursive: true, force: true });
}

function noMemoryOpen() {
  return new ChloeDBError(`${MEMORY} is a new database each time, with nothing to open: create one with open("${MEMORY}", { schema }) or Database.create("${MEMORY}", schema). It lives in a temporary directory until it closes`);
}

/** A whole number at least 0, or null and undefined. */
function wholeOrNone(k, v) {
  if (v != null && !(Number.isInteger(v) && v >= 0)) throw new RangeError(`${k} is a whole number, at least 0`);
}

/** A query's `timeoutMs` and `memory` as the native call takes them. */
function limitArgs({ timeoutMs, memory } = {}) {
  for (const [k, v] of [["timeoutMs", timeoutMs], ["memory", memory]]) wholeOrNone(k, v);
  return { timeout_ms: timeoutMs ?? undefined, memory: memory ?? undefined };
}

/** A cursor's `pageSize` and `keepAliveMs` as the native call takes them. */
function pageArgs({ pageSize, keepAliveMs } = {}) {
  if (pageSize != null && !(Number.isInteger(pageSize) && pageSize >= 1)) throw new RangeError("pageSize is a whole number, at least 1");
  if (keepAliveMs != null && !(Number.isInteger(keepAliveMs) && keepAliveMs >= 1)) throw new RangeError("keepAliveMs is a whole number, at least 1");
  return { page_size: pageSize ?? undefined, keep_alive_ms: keepAliveMs ?? undefined };
}

/** A schema given as an object, JSON text or the path of a JSON file, as
 * JSON text. */
function schemaJson(schema) {
  if (schema == null) return undefined;
  if (typeof schema === "object") return encode(schema);
  const text = String(schema);
  if (/^\s*[{[]/.test(text)) return text;
  return readFileSync(text, "utf8");
}

function paths(p) {
  return p instanceof URL ? fileURLToPath(p) : p == null ? p : String(p);
}

function names(tables) {
  return tables == null ? undefined : typeof tables === "string" ? [tables] : [...tables];
}

function isRow(o) {
  return o !== null && typeof o === "object" && !Array.isArray(o) && !(Symbol.iterator in o);
}

/** Rows to write as `{rows, places, vectors}`: the rows without their
 * Float32Array and Float64Array values, which travel apart, read from
 * their bytes, each placed by [row, field]. */
function splitVectors(rows) {
  const list = isRow(rows) ? [rows] : Array.isArray(rows) ? rows : [...rows];
  let places;
  let vectors;
  const out = list.map((r, i) => {
    if (!isRow(r)) return r;
    let copy;
    for (const k of Object.keys(r)) {
      const v = r[k];
      if (v instanceof Float32Array || v instanceof Float64Array) {
        copy ??= { ...r };
        delete copy[k];
        (places ??= []).push([i, k]);
        (vectors ??= []).push(v instanceof Float32Array ? v : Float32Array.from(v));
      }
    }
    return copy ?? r;
  });
  return { rows: out, places, vectors };
}

function base64(bytes) {
  if (bytes == null) return undefined;
  if (typeof bytes === "string") return bytes;
  const u8 = bytes instanceof Uint8Array ? bytes : ArrayBuffer.isView(bytes) ? new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength) : new Uint8Array(bytes);
  return Buffer.from(u8.buffer, u8.byteOffset, u8.byteLength).toString("base64");
}

/**
 * A ChloeDB database opened in this process.
 *
 * Create one with `Database.create` (or `open` with a schema), open an
 * existing one with `Database.open`. Only one process may have a database
 * open: to share one, serve it with chloedb-server and use `Client`.
 */
export class Database extends Searchable {
  #n;
  #tag = 0;
  /** The temporary directory of a database of `:memory:`. */
  #temp;

  /** @internal Use `Database.create`, `Database.open` or `open`. */
  constructor(nativeDb, temp) {
    super();
    this.#n = nativeDb;
    this.#temp = temp;
    /** What the import that made it, or the last import into it, did
     * (`fromSqlite`, `importSqlite`, `fromParquet`, `importParquet`). */
    this.lastImport = nativeDb.lastImport == null ? null : decode(nativeDb.lastImport);
  }

  /**
   * Creates a database at `path` with `schema` (an object, JSON text, or
   * the path of a JSON file; the short form or the long one). `directory`
   * keeps it as a directory of segment files instead of one file. `models`
   * is where embedded fields' models are (default $CHLOEDB_MODELS or
   * ~/.local/share/chloedb/models). `threads` is how many cores vector
   * indexes build on and each query searches segments on (default: every
   * core). `timeoutMs`, `memory` and `maxResultWindow` limit its queries
   * (see `setLimits`). `path` ":memory:" makes a database of its own each
   * time, in a temporary directory removed when it closes (or the process
   * exits), as SQLite's ":memory:" is one of its own.
   */
  static create(path, schema, { directory = false, models, threads, timeoutMs, memory, maxResultWindow } = {}) {
    const temp = path === MEMORY ? tempHome() : undefined;
    const at = temp ? join(temp, "memory.chloedb") : paths(path);
    return opened("create", { path: at, schema: schemaJson(schema), directory, models: paths(models), threads }, { timeoutMs, memory, maxResultWindow }, temp);
  }

  /** Opens the database at `path`; `models`, `threads`, `timeoutMs`,
   * `memory` and `maxResultWindow` as `create` takes them. `readOnly`
   * opens it to read while another process writes it. */
  static open(path, { models, threads, readOnly = false, timeoutMs, memory, maxResultWindow } = {}) {
    if (path === MEMORY) return Promise.reject(noMemoryOpen());
    return opened("open", { path: paths(path), models: paths(models), threads, read_only: readOnly }, { timeoutMs, memory, maxResultWindow });
  }

  /**
   * Limits every query of this database from now on, unless a query gives
   * its own (`query(sql, params, { timeoutMs, memory })`): a deadline of
   * `timeoutMs` milliseconds (none or 0: none, the default), past which a
   * query stops and rejects with DeadlineError; and a memory budget of
   * `memory` bytes that its queries share (0: none; none, the default: the
   * budget every database of the process shares, a quarter of the
   * machine's memory or $CHLOEDB_QUERY_MEMORY_MB MiB), past which a query
   * stops and rejects with a ChloeDBError of code "memory". And at most
   * `maxResultWindow` rows a query answers (none, the default: 10,000,
   * chloedb-server's `--max-result-window`; 0: no window), past which a
   * query rejects with BadRequestError, as a server refuses it: a cursor
   * (`pageSize`, `scroll`) reads past it a page at a time.
   */
  setLimits({ timeoutMs, memory, maxResultWindow } = {}) {
    const a = limitArgs({ timeoutMs, memory });
    wholeOrNone("maxResultWindow", maxResultWindow);
    this.#n.setLimits(a.timeout_ms, a.memory, maxResultWindow ?? undefined);
  }

  /** The most rows a query answers at once (null: no window). */
  get maxResultWindow() {
    return this.#n.maxResultWindow ?? null;
  }

  /** The deadline every query gets unless it gives its own, in ms (null:
   * none). */
  get timeoutMs() {
    return this.#n.timeoutMs ?? null;
  }

  /** The memory budget its queries share, in bytes (null: none). */
  get memory() {
    return this.#n.memory ?? null;
  }

  /** Stops the queries of this database running now: each rejects with
   * DeadlineError within a few thousand rows. Returns how many there
   * were; queries that start after it run. */
  interrupt() {
    return this.#n.interrupt();
  }

  /** A query's call: `timeoutMs` and `memory`, and `signal` (an
   * AbortSignal) to stop it. */
  async #query(op, args, { timeoutMs, memory, signal } = {}) {
    const a = { ...args, ...limitArgs({ timeoutMs, memory }) };
    if (signal == null) return this.#call(op, a);
    signal.throwIfAborted();
    const tag = ++this.#tag;
    const stop = () => this.#n.interrupt(tag);
    signal.addEventListener("abort", stop, { once: true });
    try {
      return await this.#call(op, { ...a, tag });
    } catch (e) {
      // Stopped by its signal: rejected with the signal's reason, as an
      // aborted fetch is.
      if (signal.aborted && e?.code === "timeout") throw signal.reason;
      throw e;
    } finally {
      signal.removeEventListener("abort", stop);
    }
  }

  /** @internal Queries run in this process (`search` takes `memory`). */
  get _local() {
    return true;
  }

  async #call(op, args = {}, vectors) {
    try {
      return decode(await this.#n.call(op, encode(given(args)), vectors));
    } catch (e) {
      throw failed(e);
    }
  }

  /** The first table's schema, or `table`'s, in its short form: {table,
   * key, fields}. */
  async schema(table) {
    return orderedSchema(await this.#call("schema", { table }));
  }

  /** Every table's schema in its short form, the first first. */
  async tables() {
    return (await this.#call("tables")).map(orderedSchema);
  }

  /** Live rows, in every table. */
  async count() {
    return this.#call("count");
  }

  /**
   * Adds a column to the first table or `table`, as `ALTER TABLE ... ADD
   * COLUMN` does: `options` are the field's, in the short form of a schema
   * (`default`, `not_null`, `indexed`, `embed` and `model`, ...).
   */
  async addColumn(name, type, { table, ...options } = {}) {
    await this.#call("alter", { alters: [{ add: { name, type, ...options } }], table });
  }

  /** Drops a column of the first table or `table`, with its values. */
  async dropColumn(name, { table } = {}) {
    await this.#call("alter", { alters: [{ drop: name }], table });
  }

  /** Renames a column of the first table or `table`, keeping its values. */
  async renameColumn(from, to, { table } = {}) {
    await this.#call("alter", { alters: [{ rename: [from, to] }], table });
  }

  /** Embedded fields whose model did not open, with the reason: rows
   * written here must bring those fields' vectors. */
  get embedErrors() {
    return JSON.parse(this.#n.embedErrors);
  }

  /** @internal The embedded vector fields of `table` whose model is open. */
  async _embedded(table) {
    const broken = this.embedErrors;
    const s = await this.schema(table);
    return s.fields.filter((f) => f.embed && !(f.name in broken)).map((f) => f.name);
  }

  /** Runs a SELECT. `params` binds its `:name` placeholders.
   * `timeoutMs` is its deadline, as a `Client`'s query takes it: past it
   * the query stops and rejects with DeadlineError (none or 0: the
   * database's, see `setLimits`). `memory` is a budget of its own in bytes,
   * past which it rejects with a ChloeDBError of code "memory" (0: none;
   * none: the database's). `signal`, an AbortSignal, stops it when it
   * aborts: it rejects with the signal's reason, as an aborted fetch
   * does.
   *
   * With `pageSize`, it answers the first page of at most that many rows
   * (and at most `maxResultWindow`), past the window, and while rows are
   * left the result's `cursor`, for `next(cursor)`: every page reads the
   * version of the database the first read, as a server's scroll does.
   * The cursor is kept for `keepAliveMs` after each page (60 s; an hour at
   * most). */
  async query(sql, params, { timeoutMs, memory, signal, pageSize, keepAliveMs } = {}) {
    if (pageSize == null && keepAliveMs != null) throw new TypeError("keepAliveMs is a cursor's: it goes with pageSize");
    const paging = pageSize != null ? pageArgs({ pageSize, keepAliveMs }) : {};
    return new Result(await this.#query("query", { sql, params, ...paging }, { timeoutMs, memory, signal }));
  }

  /** The next page of a cursor (`query` with `pageSize`): a `Result`
   * whose `cursor` is the same while rows are left, and null after the
   * last page. `keepAliveMs` changes how long it is kept after this
   * page; `timeoutMs`, `memory` and `signal` as `query` takes them. */
  async next(cursor, { keepAliveMs, timeoutMs, memory, signal } = {}) {
    if (typeof cursor !== "string") throw new TypeError("next(cursor): the cursor of a page, a string");
    try {
      return new Result(await this.#query("next", { cursor, keep_alive_ms: pageArgs({ keepAliveMs }).keep_alive_ms }, { timeoutMs, memory, signal }));
    } catch (e) {
      // A cursor gone is not found, as a server's 404 says.
      if (e?.code === "bad_request" && /no such cursor/.test(e.message)) throw makeError(e.message.replace(/^query error: /, ""), { code: "not_found", cause: e });
      throw e;
    }
  }

  /** Ends a cursor before its last page, letting go of the version it
   * reads; whether it was open. */
  async closeCursor(cursor) {
    return this.#call("close_cursor", { cursor });
  }

  /** @internal */
  async _highlight(body) {
    return this.#call("highlight", body);
  }

  /** @internal */
  async _correct(body) {
    return this.#call("correct", body);
  }

  /** Runs any statement: a SELECT's `Result`, or the number of rows an
   * INSERT, UPDATE or DELETE wrote. `timeoutMs`, `memory` and `signal`
   * limit a SELECT as `query` takes them; a write is not cut short. */
  async sql(sql, params, { timeoutMs, memory, signal } = {}) {
    return resultOrWritten(await this.#query("sql", { sql, params }, { timeoutMs, memory, signal }));
  }

  /** @internal */
  async _queryLogged(sql, params, log) {
    return new Result(await this.#call("query_logged", { sql, params, log }));
  }

  /** @internal */
  async _exportRankLog(opts) {
    return this.#call("export_rank_log", { export: opts });
  }

  /** The plan of a SELECT, INSERT, UPDATE or DELETE, without running it:
   * the plan IR as JSON, which `run` takes. */
  async plan(sql, params) {
    return this.#call("plan", { sql, params });
  }

  /** Runs a plan (as `plan` or a server's /v1/explain gives it): a read's
   * `Result`, or the number of rows a write wrote; a read limited by
   * `timeoutMs`, `memory` and `signal` as `query` takes them. */
  async run(plan, { timeoutMs, memory, signal } = {}) {
    return resultOrWritten(await this.#query("run", { plan }, { timeoutMs, memory, signal }));
  }

  /** Inserts a row or rows, whose keys must be new, into the first table or
   * `table`; all or none are written. Returns how many. A vector may be a
   * Float32Array (read from its bytes), a Float64Array or an array. */
  async insert(rows, { table } = {}) {
    const { rows: r, places, vectors } = splitVectors(rows);
    return this.#call("insert", { rows: r, table, vectors: places }, vectors);
  }

  /** Inserts a row or rows, replacing the rows that have their keys, in
   * transactions of 10,000 rows. Returns how many. */
  async upsert(rows, { table } = {}) {
    const { rows: r, places, vectors } = splitVectors(rows);
    return this.#call("upsert", { rows: r, table, vectors: places }, vectors);
  }

  /** The row with this primary key in the first table or `table`, read
   * without SQL: an object of the columns `SELECT *` answers, or null when
   * no live row has the key (or its `expire_by` time has passed). */
  async get(key, { table } = {}) {
    return (await this.#call("get_many", { keys: [key], table }))[0];
  }

  /** `get` of several keys at once, in their order. */
  async getMany(keys, { table } = {}) {
    return this.#call("get_many", { keys: [...keys], table });
  }

  /** Deletes the rows with this key or these keys; returns how many there
   * were. */
  async delete(keys, { table } = {}) {
    const list = typeof keys === "string" || typeof keys === "number" || typeof keys === "bigint" ? [keys] : [...keys];
    return this.#call("delete", { keys: list, table });
  }

  /** Deletes the rows whose `expire_by` time has passed, in every table with
   * one; returns how many. Reads leave them out already. */
  async expire() {
    return this.#call("expire");
  }

  /** Write operations as `Client.write` and /v1/write take them ({op:
   * "insert", doc: {...}}, {op: "add", key, field: "n", value: 1}, ...),
   * in one transaction. Returns the rows written. */
  async write(ops) {
    return this.#call("write", { ops: [...ops] });
  }

  /** Loads a JSON Lines, JSON (an array), CSV or TSV file, in `format` or
   * the one its extension names, into the first table or `table`. Into an
   * empty table the rows are bulk-loaded in one commit; otherwise they are
   * upserted. Returns {rows, bulk, skippedFields}: the columns the schema
   * lacks. */
  async importFile(path, { format, table } = {}) {
    const r = await this.#call("import_file", { path: paths(path), format, table });
    return { rows: r.rows, bulk: r.bulk, skippedFields: r.skipped_fields };
  }

  /** Imports the tables of a SQLite database file (all, or `tables`), each
   * into the table of its name, created from the SQLite table or from the
   * schema `schemas` gives for it (see `planSqlite`). A table without a
   * key that has rows is refused, since importing the file again would
   * repeat its rows, unless `append`. Returns {tables, skipped}. */
  async importSqlite(path, { tables, schemas, blobsBase64 = false, append = false } = {}) {
    this.lastImport = await this.#call("import_sqlite", { path: paths(path), tables: names(tables), schemas: schemas == null ? undefined : schemaJson(schemas), blobs_base64: blobsBase64, append });
    return this.lastImport;
  }

  /** Writes the tables (all, or `tables`) to a new SQLite file, from one
   * snapshot, with B-tree `indexes`, an FTS5 index over the text (`fts`)
   * and the ChloeDB `schemas`, which an import restores. Returns {tables}. */
  async exportSqlite(path, { tables, fts = true, indexes = true, schemas = true } = {}) {
    return this.#call("export_sqlite", { path: paths(path), tables: names(tables), fts, indexes, schemas });
  }

  /** Imports a Parquet file or a directory of them (Spark's, DuckDB's), or
   * an `s3://` URL, into one table (see chloedb-py's `import_parquet`).
   * Returns {table, rows, files, created, bulk, skipped, notes}. */
  async importParquet(path, { table, schema, key, blobsBase64 = false } = {}) {
    this.lastImport = await this.#call("import_parquet", { path: paths(path), table, schema: schemaJson(schema), key, blobs_base64: blobsBase64 });
    return this.lastImport;
  }

  /** What `importParquet` would make of a Parquet file or directory into
   * this database, without writing. */
  async planParquet(path, { table, schema, key, blobsBase64 = false } = {}) {
    return this.#call("plan_parquet", { parquet_path: paths(path), table, schema: schemaJson(schema), key, blobs_base64: blobsBase64 });
  }

  /** Writes the tables (all, or `tables`) to Parquet, from one snapshot: to
   * the file `path` when it ends in .parquet (one table), else to a new
   * directory of `<table>.parquet` files; `path` may be an `s3://` URL.
   * Returns {tables: [{table, file, rows, bytes, notes}]}. */
  async exportParquet(path, { tables, schemas = true } = {}) {
    return this.#call("export_parquet", { path: paths(path), tables: names(tables), schemas });
  }

  /** The query vector of `text` by an embedded field's model (of the first
   * table, or of `table`), or of `image` or `audio`, the bytes of an image
   * or sound file (a Uint8Array), by a model of that. */
  async embed(text, { field, table, image, audio } = {}) {
    return this.#call("embed", { text, field, table, image: base64(image), audio: base64(audio) });
  }

  /** The vectors of `text`'s tokens as a query, by the model of
   * multi-vector field `field` (a ColBERT model), as `maxsim` takes them. */
  async embedTokens(text, field, { table } = {}) {
    return this.#call("embed_tokens", { text, field, table });
  }

  /** The codes that can follow each prefix of semantic-ID field `field`
   * ("" for the first number) among the rows SQL condition `where`
   * matches, or all rows: {prefix: {code: rows}}. */
  async sidNext(prefixes, { field, where, params, table } = {}) {
    const got = await this.#call("sid_next", { prefixes: [...prefixes], field, where, params, table });
    return Object.fromEntries(Object.entries(got).map(([p, cs]) => [p, Object.fromEntries(cs)]));
  }

  /** Trains a semantic-ID codebook on the vectors of `field` of the first
   * table or `table`, saves it as `name` under `models`, and returns its
   * SHA-256. */
  async trainCodebook(field, name, { table, models, levels, k, iterations, sample, seed, balanced } = {}) {
    return this.#call("train_codebook", { field, name, table, models: paths(models), levels, k, iterations, sample, seed, balanced });
  }

  /** Readies a table that is done changing for queries: writes the rows
   * still in memory into segments, merges its segments into as few as it
   * can (one, up to 2,000,000 rows; a query visits each), drops deleted
   * rows, and returns the space this frees to the file system. */
  async optimize() {
    await this.#call("optimize");
  }

  /** Returns the space merges freed to the file system (they reuse it
   * otherwise): the number of bytes the file shrank by. */
  async vacuum() {
    return this.#call("vacuum");
  }

  /** Opened `readOnly`: reads what the writer committed since the last
   * query, now (each query does so by itself); whether anything changed.
   * False for the writer. */
  async refresh() {
    return this.#call("refresh");
  }

  /** Merges small segments, returns the space merges freed to the file
   * system when it is an eighth of the file or more, and closes the
   * database. Calls after it fail. */
  async close() {
    try {
      await this.#call("close");
    } finally {
      if (this.#temp) gone(this.#temp);
    }
  }

  get closed() {
    return this.#n.closed;
  }

  async [Symbol.asyncDispose]() {
    await this.close();
  }
}

/**
 * Opens the database at `path`, creating it with `schema` if it does not
 * exist; `directory`, `models`, `threads` and the limits as
 * `Database.create` takes them. ":memory:" with a schema is a database of
 * its own, in a temporary directory removed when it closes.
 */
export async function open(path, { schema, directory = false, models, threads, readOnly = false, timeoutMs, memory, maxResultWindow } = {}) {
  if (path === MEMORY && (schema == null || readOnly)) throw noMemoryOpen();
  if (path !== MEMORY && (existsSync(paths(path)) || readOnly)) return Database.open(path, { models, threads, readOnly, timeoutMs, memory, maxResultWindow });
  if (schema == null) throw new ChloeDBError(`${paths(path)} does not exist: pass a schema to create it`);
  return Database.create(path, schema, { directory, models, threads, timeoutMs, memory, maxResultWindow });
}

/** Creates a database at `path` from the tables of a SQLite database file
 * (all, or `tables`), with schemas made from the SQLite tables or given in
 * `schemas` (see `planSqlite`). What the import did is its `lastImport`. */
export function fromSqlite(sqlitePath, path, { tables, schemas, blobsBase64 = false, directory = false, models, threads } = {}) {
  return opened("from_sqlite", { sqlite_path: paths(sqlitePath), path: paths(path), tables: names(tables), schemas: schemas == null ? undefined : schemaJson(schemas), blobs_base64: blobsBase64, directory, models: paths(models), threads });
}

/** What `fromSqlite` would make of a SQLite file, without writing: {tables,
 * skipped}, each table the schema of the ChloeDB table it becomes. Edit the
 * plan and pass it to `fromSqlite` as `schemas`. */
export function planSqlite(sqlitePath, { tables, schemas, blobsBase64 = false } = {}) {
  return callStatic("plan_sqlite", { sqlite_path: paths(sqlitePath), tables: names(tables), schemas: schemas == null ? undefined : schemaJson(schemas), blobs_base64: blobsBase64 });
}

/** Creates a database at `path` from a Parquet file or directory (or an
 * `s3://` URL), its one table made from the columns, kept by an export, or
 * given as `schema` (see `planParquet`). What the import did is its
 * `lastImport`. */
export function fromParquet(parquetPath, path, { table, schema, key, blobsBase64 = false, directory = false, models, threads } = {}) {
  return opened("from_parquet", { parquet_path: paths(parquetPath), path: paths(path), table, schema: schemaJson(schema), key, blobs_base64: blobsBase64, directory, models: paths(models), threads });
}

/** What `fromParquet` would make of a Parquet file or directory, without
 * writing: the table's schema, the files and rows, the columns read and
 * what is skipped. Edit it and pass it back as `schema`. */
export function planParquet(parquetPath, { table, schema, key, blobsBase64 = false } = {}) {
  return callStatic("plan_parquet", { parquet_path: paths(parquetPath), table, schema: schemaJson(schema), key, blobs_base64: blobsBase64 });
}

/** Copies the database at `path`, written by a version of ChloeDB from
 * before field ids, into this version's format in its place, keeping the
 * old one as `<path>.before-field-ids`. Returns null when it needs no
 * upgrade, else {old, tables, rows}. */
export function upgrade(path) {
  return callStatic("upgrade", { path: paths(path) });
}

/** Trains a semantic-ID codebook on `vectors` (arrays or Float32Arrays),
 * saves it as `name` under `models`, and returns its SHA-256. */
export function trainCodebook(vectors, name, { models, levels, k, iterations, sample, seed, balanced } = {}) {
  return callStatic("train_codebook", { vectors: [...vectors], name, models: paths(models), levels, k, iterations, sample, seed, balanced });
}

/** The addon's version. */
/** A device's slice of `table` on the chloedb-server at `url`, kept in
 * directory `dir`: its manifest fetched and its database opened (see
 * nearby.js). Searches near the user answer on the device when they can
 * prove the answer is the server's, else ask the server. */
export function openNearby(url, table, dir, options = {}) {
  return new Nearby(native, url, table, dir, options).load();
}

export const version = native.version();

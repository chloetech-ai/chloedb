// ChloeDB from JavaScript: the database in this process.
//
// `Database` opens a ChloeDB file (or a directory of segment files) in this
// process, with no server, as a SQLite library opens a SQLite file: Node,
// Bun and Deno load the native addon (strata.node) that runs it. `Client`
// (client.js) talks to a running chloedb-server over HTTP instead, from any
// JavaScript runtime with fetch. Both answer queries with the same
// `Result` and write rows the same way, so code can move from one to the
// other:
//
//   import { open } from "chloedb";
//
//   const db = await open("books.strata", { schema: {
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
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

import { Nearby } from "./nearby.js";
import { ChloeDBError, Result, Searchable, decode, encode, given, orderedSchema, pickTable, resultOrWritten } from "./common.js";

export { ChloeDBError, StrataError, Result } from "./common.js";
export { Client, connect } from "./client.js";
export { Nearby, NearbyResult } from "./nearby.js";

const here = dirname(fileURLToPath(import.meta.url));

/** The native addon: `$CHLOEDB_NODE_ADDON` (or `$STRATA_NODE_ADDON`), else strata.node beside this
 * file (`npm run build` makes it). */
function load() {
  const path = process.env.CHLOEDB_NODE_ADDON || process.env.STRATA_NODE_ADDON || join(here, "strata.node");
  if (!existsSync(path)) {
    throw new ChloeDBError(`the ChloeDB addon is not built: ${path} is missing (run \`npm run build\` in chloedb-js, which needs Rust and a C compiler)`);
  }
  return createRequire(import.meta.url)(path);
}

const native = load();

/** A native error (a failed call) as a ChloeDBError. */
function failed(e) {
  if (e instanceof ChloeDBError) return e;
  return new ChloeDBError(e?.message ?? String(e), { cause: e });
}

async function callStatic(op, args) {
  try {
    return decode(await native.callStatic(op, encode(given(args))));
  } catch (e) {
    throw failed(e);
  }
}

async function opened(op, args) {
  try {
    return new Database(await native.NativeDatabase.open(op, encode(given(args))));
  } catch (e) {
    throw failed(e);
  }
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

  /** @internal Use `Database.create`, `Database.open` or `open`. */
  constructor(nativeDb) {
    super();
    this.#n = nativeDb;
    /** What the import that made it, or the last import into it, did
     * (`fromSqlite`, `importSqlite`, `fromParquet`, `importParquet`). */
    this.lastImport = nativeDb.lastImport == null ? null : decode(nativeDb.lastImport);
  }

  /**
   * Creates a database at `path` with `schema` (an object, JSON text, or
   * the path of a JSON file; the short form or the long one). `directory`
   * keeps it as a directory of segment files instead of one file. `models`
   * is where embedded fields' models are (default $CHLOEDB_MODELS or
   * ~/.local/share/strata/models). `threads` is how many cores vector
   * indexes build on and each query searches segments on (default: every
   * core).
   */
  static create(path, schema, { directory = false, models, threads } = {}) {
    return opened("create", { path: paths(path), schema: schemaJson(schema), directory, models: paths(models), threads });
  }

  /** Opens the database at `path`; `models` and `threads` as `create` takes
   * them. `readOnly` opens it to read while another process writes it. */
  static open(path, { models, threads, readOnly = false } = {}) {
    return opened("open", { path: paths(path), models: paths(models), threads, read_only: readOnly });
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

  /** Runs a SELECT. `params` binds its `:name` placeholders. */
  async query(sql, params) {
    return new Result(await this.#call("query", { sql, params }));
  }

  /** Runs any statement: a SELECT's `Result`, or the number of rows an
   * INSERT, UPDATE or DELETE wrote. */
  async sql(sql, params) {
    return resultOrWritten(await this.#call("sql", { sql, params }));
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
   * `Result`, or the number of rows a write wrote. */
  async run(plan) {
    return resultOrWritten(await this.#call("run", { plan }));
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
   * schema `schemas` gives for it (see `planSqlite`). Returns {tables,
   * skipped}. */
  async importSqlite(path, { tables, schemas, blobsBase64 = false } = {}) {
    this.lastImport = await this.#call("import_sqlite", { path: paths(path), tables: names(tables), schemas: schemas == null ? undefined : schemaJson(schemas), blobs_base64: blobsBase64 });
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
  async trainCodebook(field, name, { table, models, levels, k, iterations, sample, seed } = {}) {
    return this.#call("train_codebook", { field, name, table, models: paths(models), levels, k, iterations, sample, seed });
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
    await this.#call("close");
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
 * exist; `directory`, `models` and `threads` as `Database.create` takes
 * them.
 */
export async function open(path, { schema, directory = false, models, threads, readOnly = false } = {}) {
  if (existsSync(paths(path)) || readOnly) return Database.open(path, { models, threads, readOnly });
  if (schema == null) throw new ChloeDBError(`${paths(path)} does not exist: pass a schema to create it`);
  return Database.create(path, schema, { directory, models, threads });
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
export function trainCodebook(vectors, name, { models, levels, k, iterations, sample, seed } = {}) {
  return callStatic("train_codebook", { vectors: [...vectors], name, models: paths(models), levels, k, iterations, sample, seed });
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

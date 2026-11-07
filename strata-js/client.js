// A client of a running strata-server, over its HTTP JSON API, with the
// same methods as `Database` (index.js) where the server offers them.
//
// It needs only `fetch`, so it runs in Node 18 and later, Bun, Deno,
// browsers and edge workers, with no native code: `import { connect } from
// "strata-db/client"`. In a browser, the server answers pages of its own
// origin (a page it serves, or one behind the same reverse proxy): it
// sends no CORS headers, and refuses writes from other origins.

import { StrataError, Result, Searchable, decode, encode, given, orderedSchema, pickTable, resultOrWritten } from "./common.js";

export { StrataError, Result } from "./common.js";

/** Rows a write request carries. */
export const WRITE_BATCH = 1000;
/** Keys a read by key sends in one request (the server takes 10,000). */
export const GET_BATCH = 10_000;

const FORMATS = { ".jsonl": "jsonl", ".ndjson": "jsonl", ".json": "json", ".csv": "csv", ".tsv": "tsv", ".tab": "tsv", ".sqlite": "sqlite", ".db": "sqlite", ".parquet": "parquet" };
// The content type an import is sent with: a server refuses text/plain.
const CONTENT_TYPES = {
  jsonl: "application/x-ndjson",
  json: "application/json",
  csv: "text/csv",
  tsv: "text/tab-separated-values",
  sqlite: "application/vnd.sqlite3",
  parquet: "application/vnd.apache.parquet",
};

function env(name) {
  try {
    return globalThis.process?.env?.[name] ?? globalThis.Deno?.env?.get(name);
  } catch {
    return undefined; // Deno without --allow-env
  }
}

// node:fs, loaded only when a path is given: a page never needs it.
let fsp;
async function fs() {
  const name = "node:fs/promises";
  return (fsp ??= await import(/* @vite-ignore */ /* webpackIgnore: true */ name));
}

function extension(path) {
  const m = /\.[^./\\]+$/.exec(String(path));
  return m ? m[0].toLowerCase() : "";
}

/** A file's bytes: a path (read with node:fs), or the bytes themselves
 * (a Uint8Array, an ArrayBuffer, a Blob). */
async function bytesOf(data) {
  if (typeof data === "string" || data instanceof URL) return (await fs()).readFile(data);
  if (data instanceof Uint8Array || data instanceof ArrayBuffer) return data;
  if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  if (typeof Blob !== "undefined" && data instanceof Blob) return data;
  throw new StrataError("a file is a path, a Uint8Array, an ArrayBuffer or a Blob");
}

/** Writes `bytes` to `path`, which must not exist. */
async function writeNew(path, bytes) {
  const f = await fs();
  const part = `${path}.part`;
  try {
    await f.writeFile(part, bytes, { flag: "wx" });
    try {
      await f.access(path);
      throw new StrataError(`${path} exists`);
    } catch (e) {
      if (e instanceof StrataError) throw e;
    }
    await f.rename(part, path);
  } finally {
    await f.rm(part, { force: true });
  }
}

function query(o) {
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(o)) {
    if (v === undefined || v === null) continue;
    q.set(k, typeof v === "boolean" ? String(v) : Array.isArray(v) ? v.join(",") : String(v));
  }
  const s = q.toString();
  return s ? `?${s}` : "";
}

function names(tables) {
  return tables == null ? undefined : typeof tables === "string" ? tables : [...tables].join(",");
}

function rowsOf(rows) {
  return rows !== null && typeof rows === "object" && !Array.isArray(rows) && !(Symbol.iterator in rows) ? [rows] : [...rows];
}

/**
 * A strata-server at `url` ("http://host:7700"). `token` is the server's
 * --token or an API key, if it needs one ($STRATA_TOKEN by default).
 * Errors the server reports reject with `StrataError` (its `status` and
 * `answer` set); one it cannot be reached rejects with fetch's error.
 */
export class Client extends Searchable {
  #schema;

  constructor(url = "http://127.0.0.1:7700", { token, timeout = 30_000, fetch: f } = {}) {
    super();
    this.url = String(url).replace(/\/+$/, "");
    this.token = token ?? env("STRATA_TOKEN");
    /** Milliseconds a request may take (0 for no limit). */
    this.timeout = timeout;
    this.fetch = f ?? globalThis.fetch.bind(globalThis);
  }

  /** The answer of a request: its JSON, or with `as` "text" its text, or
   * "response" the Response itself. */
  async _call(method, path, body, { data, headers = {}, as = "json" } = {}) {
    const h = { ...headers };
    if (body !== undefined) {
      data = encode(body);
      h["content-type"] = "application/json";
    }
    if (this.token) h.authorization = `Bearer ${this.token}`;
    const init = { method, headers: h, body: data };
    if (this.timeout) init.signal = AbortSignal.timeout(this.timeout);
    if (data !== undefined && typeof data?.getReader === "function") init.duplex = "half";
    const r = await this.fetch(this.url + path, init);
    if (!r.ok) {
      const raw = await r.text();
      let answer;
      let msg = raw || r.statusText;
      try {
        answer = decode(raw);
        if (answer && typeof answer === "object" && answer.error) msg = answer.error;
      } catch {
        answer = undefined;
      }
      throw new StrataError(`${msg} (HTTP ${r.status})`, { status: r.status, answer });
    }
    if (as === "response") return r;
    const text = await r.text();
    return as === "text" ? text : decode(text);
  }

  async #schemaAnswer() {
    return (this.#schema ??= await this._call("GET", "/v1/schema"));
  }

  /** The first table's schema, or `table`'s, in its short form: {table,
   * key, fields}. */
  async schema(table) {
    if (table == null) return orderedSchema((await this.#schemaAnswer()).short);
    return pickTable(await this.tables(), table);
  }

  /** Every table's schema in its short form, the first first. */
  async tables() {
    return (await this._call("GET", "/v1/tables")).tables.map((t) => orderedSchema(t.short));
  }

  /** @internal */
  async _embedded(table) {
    let a = await this.#schemaAnswer();
    if (table != null && a.table.toLowerCase() !== String(table).toLowerCase()) {
      a = (await this._call("GET", "/v1/tables")).tables.find((t) => t.table.toLowerCase() === String(table).toLowerCase()) ?? {};
    }
    return (a.embeddings ?? []).filter((e) => e.loaded).map((e) => e.field);
  }

  /** {status, docs, segments, role, ...}, also when the status is "down" or
   * "failing" (which the server answers with 503). */
  async health() {
    try {
      return await this._call("GET", "/v1/health");
    } catch (e) {
      if (e instanceof StrataError && e.status === 503 && e.answer?.status) return e.answer;
      throw e;
    }
  }

  async stats() {
    return this._call("GET", "/v1/stats");
  }

  /** Live rows, as the server's health counts them. */
  async count() {
    return Number((await this.health()).docs);
  }

  /** Runs a SELECT; `timeoutMs` bounds it on the server. */
  async query(sql, params, { timeoutMs } = {}) {
    return new Result(await this._call("POST", "/v1/query", given({ sql, params, timeout_ms: timeoutMs })));
  }

  /** Runs any statement: a SELECT's `Result`, or the number of rows an
   * INSERT, UPDATE or DELETE wrote. */
  async sql(sql, params, { idempotencyKey } = {}) {
    return resultOrWritten(await this._call("POST", "/v1/sql", given({ sql, params, idempotency_key: idempotencyKey })));
  }

  /** @internal */
  async _queryLogged(sql, params, log) {
    return new Result(await this._call("POST", "/v1/query", given({ sql, params, log })));
  }

  /** @internal NDJSON, a record a line; a failure partway is the last line. */
  async _exportRankLog(opts) {
    const text = await this._call("POST", "/v1/rank_log/export", opts, { as: "text" });
    const out = text.split("\n").filter((l) => l.trim()).map(decode);
    const last = out.at(-1);
    if (last && "error" in last && !("request" in last)) {
      throw new StrataError(`${last.error} (HTTP ${last.status ?? 500})`, { status: last.status ?? 500, answer: last });
    }
    return out;
  }

  /** Runs the query and returns its plan: the plan IR ("ir") and its lines
   * ("tree"), the executor's plan ("plan") and the bound query
   * ("logical"). */
  async explain(sql, params) {
    return this._call("POST", "/v1/explain", given({ sql, params }));
  }

  /** Runs a plan (the "ir" of `explain`, or `Database.plan`'s): a read's
   * `Result`, or the number of rows a write wrote. */
  async run(plan) {
    return resultOrWritten(await this._call("POST", "/v1/plan", { plan }));
  }

  /** The query vector of `text` by an embedded field's model, or of `image`
   * or `audio` (the bytes of a file), on the server. */
  async embed(text, { field, table, image, audio } = {}) {
    return (await this._call("POST", "/v1/embed", given({ text, field, table, image: await base64(image), audio: await base64(audio) }))).vector;
  }

  /** The vectors of `text`'s tokens as a query, by the model of
   * multi-vector field `field` (ColBERT), on the server. */
  async embedTokens(text, field, { table } = {}) {
    return (await this._call("POST", "/v1/embed", given({ text, field, table }))).vectors;
  }

  /** The codes that can follow each prefix of semantic-ID field `field`
   * among the rows `where` matches, summed over the shards on a
   * coordinator: {prefix: {code: rows}}. */
  async sidNext(prefixes, { field, where, params, table } = {}) {
    const got = await this._call("POST", "/v1/sid/next", given({ field, prefixes: [...prefixes], where, params, table }));
    return Object.fromEntries(got.next.map((n) => [n.prefix, Object.fromEntries(n.next.map((c) => [c.code, c.count]))]));
  }

  /** Write operations as /v1/write takes them ({op: "insert", doc}, {op:
   * "delete", key}, ...), in one request. Returns the rows written. */
  async write(ops, { idempotencyKey } = {}) {
    return Number((await this._call("POST", "/v1/write", given({ ops: [...ops], idempotency_key: idempotencyKey }))).written);
  }

  async #ops(op, rows, table) {
    let n = 0;
    for (let i = 0; i < rows.length; i += WRITE_BATCH) {
      n += await this.write(rows.slice(i, i + WRITE_BATCH).map((doc) => given({ op, table, doc })));
    }
    return n;
  }

  /** Inserts a row or rows, whose keys must be new, into the first table or
   * `table`, 1,000 rows a request. Returns how many. */
  async insert(rows, { table } = {}) {
    return this.#ops("insert", rowsOf(rows), table);
  }

  /** Inserts a row or rows, replacing the rows that have their keys, 1,000
   * rows a request. Returns how many. */
  async upsert(rows, { table } = {}) {
    return this.#ops("upsert", rowsOf(rows), table);
  }

  /** Deletes the rows with this key or these keys. Through a coordinator,
   * `routing` gives the `shard_by` value of a table placed by a column. */
  async delete(keys, { table, routing } = {}) {
    const list = typeof keys === "string" || typeof keys === "number" || typeof keys === "bigint" ? [keys] : [...keys];
    let n = 0;
    for (let i = 0; i < list.length; i += WRITE_BATCH) {
      n += await this.write(list.slice(i, i + WRITE_BATCH).map((key) => given({ op: "delete", table, routing, key })));
    }
    return n;
  }

  async #table(table) {
    return table ?? (await this.schema()).table;
  }

  async #kvPath(key, table) {
    return `/v1/kv/${encodeURIComponent(await this.#table(table))}/${encodeURIComponent(String(key))}`;
  }

  /** The row with this primary key in the first table or `table`, read
   * without SQL: an object of the columns `SELECT *` answers, or null. A
   * coordinator asks the key's shard. */
  async get(key, { table } = {}) {
    try {
      return (await this._call("GET", await this.#kvPath(key, table))).row;
    } catch (e) {
      if (e instanceof StrataError && e.status === 404 && e.answer?.found === false) return null;
      throw e;
    }
  }

  /**
   * `get` of several keys, 10,000 a request, in their order; with
   * `columns`, each row of those columns alone (a ranker's features, say),
   * which reads no other.
   */
  async getMany(keys, { table, columns } = {}) {
    const list = [...keys];
    const body = { table: await this.#table(table) };
    if (columns !== undefined) body.columns = [...columns];
    const out = [];
    for (let i = 0; i < list.length; i += GET_BATCH) {
      out.push(...(await this._call("POST", "/v1/kv/get", { ...body, keys: list.slice(i, i + GET_BATCH) })).rows);
    }
    return out;
  }

  /** Writes `row` as the row with this key, in place of any that has it;
   * with `ifSeqNo`, only while that row's sequence number is that. Returns
   * the rows written. */
  async put(key, row, { table, ifSeqNo } = {}) {
    const path = (await this.#kvPath(key, table)) + query({ if_seq_no: ifSeqNo });
    return Number((await this._call("PUT", path, { ...row })).written);
  }

  /**
   * Writes `events` (an iterable or async iterable) through /v1/stream as
   * they come, in batches the server makes: each an operation, or one
   * event's operations as {ops: [...], source: {name, offset, ts}}, which a
   * batch never splits. Offsets a source has reached already are skipped,
   * so a replay applies each event once. Returns the last answer, {done,
   * lines, written, skipped, batches}.
   */
  async stream(events) {
    const enc = new TextEncoder();
    const it = (events[Symbol.asyncIterator] ?? events[Symbol.iterator]).call(events);
    const body = new ReadableStream({
      async pull(c) {
        const chunk = [];
        while (chunk.length < 256) {
          const { value, done } = await it.next();
          if (done) break;
          chunk.push(encode(value));
        }
        if (chunk.length) c.enqueue(enc.encode(chunk.join("\n") + "\n"));
        else c.close();
      },
    });
    const text = await this._call("POST", "/v1/stream", undefined, { data: body, headers: { "content-type": "application/x-ndjson" }, as: "text" });
    let last = {};
    for (const line of text.split("\n")) {
      if (!line.trim()) continue;
      try {
        last = decode(line);
      } catch {
        last = {}; // cut short
        break;
      }
    }
    if ("error" in last || !last.done) {
      throw new StrataError(`${last.error ?? "the stream ended early"} (HTTP ${last.status ?? 500})`, { status: last.status ?? 500, answer: last });
    }
    return last;
  }

  /** The offset of the last event written from stream source `name`, or
   * null for a source not written from: resume from the next. */
  async source(name) {
    return (await this._call("GET", "/v1/sources/" + encodeURIComponent(name).replace(/%2F/g, "/"))).offset;
  }

  /** Every stream source written from: [{name, offset, updated}]. */
  async sources() {
    return (await this._call("GET", "/v1/sources")).sources;
  }

  async #upload(data, format, q) {
    const body = await bytesOf(data);
    return this._call("POST", "/v1/import" + query({ format, ...q }), undefined, { data: body, headers: { "content-type": CONTENT_TYPES[format] ?? "application/octet-stream" } });
  }

  /** Uploads a JSON Lines, JSON (an array), CSV or TSV file (a path, or its
   * bytes; up to the server's 256 MB), in `format` or the one its
   * extension names, into the first table or `table`; its rows are
   * upserted. Returns {rows, bulk, skippedFields}. */
  async importFile(file, { format, table } = {}) {
    format ??= FORMATS[extension(file)];
    if (!format) throw new StrataError(`name the format of ${typeof file === "string" ? file : "the file"}: jsonl, json, csv or tsv`);
    const r = await this.#upload(file, format, { table });
    return { rows: r.written, bulk: false, skippedFields: r.skipped_fields ?? [] };
  }

  /** Uploads a SQLite database file and imports its tables (all, or
   * `tables`), each into the table of its name; rows are upserted by key.
   * Returns {tables, skipped, written}. */
  async importSqlite(file, { tables, blobsBase64 = false } = {}) {
    return this.#upload(file, "sqlite", { tables: names(tables), blobs_base64: blobsBase64 });
  }

  /** What `importSqlite` would do, writing nothing. */
  async planSqlite(file, { tables, blobsBase64 = false } = {}) {
    return this.#upload(file, "sqlite", { tables: names(tables), blobs_base64: blobsBase64, plan: true });
  }

  /** Uploads a Parquet file and imports its rows into `table` (by default,
   * the table an export kept in the file), keyed by `key` when it creates
   * one. Returns {table, rows, created, skipped, notes, written}. */
  async importParquet(file, { table, key, blobsBase64 = false } = {}) {
    return this.#upload(file, "parquet", { table, key, blobs_base64: blobsBase64 });
  }

  /** What `importParquet` would do, writing nothing. */
  async planParquet(file, { table, key, blobsBase64 = false } = {}) {
    return this.#upload(file, "parquet", { table, key, blobs_base64: blobsBase64, plan: true });
  }

  async #download(path, q) {
    const r = await this._call("GET", "/v1/export" + query(q), undefined, { as: "response" });
    const data = new Uint8Array(await r.arrayBuffer());
    const tables = (r.headers.get("x-strata-tables") ?? "").split(",").filter(Boolean);
    if (path == null) return { tables, data };
    await writeNew(path instanceof URL ? path : String(path), data);
    return { tables };
  }

  /** Downloads the server's tables (all, or `tables`) as a SQLite file: to
   * `path`, which must not exist, or, when `path` is null, as `data` (a
   * Uint8Array). Returns {tables, data?}. */
  async exportSqlite(path, { tables, fts = true, indexes = true, schemas = true } = {}) {
    return this.#download(path, { tables: names(tables), fts: fts ? undefined : false, indexes: indexes ? undefined : false, schemas: schemas ? undefined : false });
  }

  /** Downloads one of the server's tables (`table`, which a server of one
   * table may leave out) as a Parquet file, to `path` or as `data`, as
   * `exportSqlite` does. Returns {tables, data?}. */
  async exportParquet(path, { table, schemas = true } = {}) {
    return this.#download(path, { format: "parquet", tables: table, schemas: schemas ? undefined : false });
  }

  /** Asks the server's agent a question about the data: {answer, steps,
   * ...}. Needs an Anthropic API key, the server's or `apiKey`. */
  async ask(question, { model, effort, apiKey, maxSteps } = {}) {
    return this._call("POST", "/v1/agent", given({ question, model, effort, api_key: apiKey, max_steps: maxSteps }));
  }

  async close() {}

  async [Symbol.asyncDispose]() {}
}

async function base64(bytes) {
  if (bytes == null) return undefined;
  if (typeof bytes === "string") return bytes;
  let u8;
  if (typeof Blob !== "undefined" && bytes instanceof Blob) u8 = new Uint8Array(await bytes.arrayBuffer());
  else if (bytes instanceof ArrayBuffer) u8 = new Uint8Array(bytes);
  else u8 = new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (typeof Uint8Array.prototype.toBase64 === "function") return u8.toBase64();
  let s = "";
  for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode.apply(null, u8.subarray(i, i + 0x8000));
  return btoa(s);
}

/** A `Client` of the strata-server at `url`. */
export function connect(url = "http://127.0.0.1:7700", options = {}) {
  return new Client(url, options);
}

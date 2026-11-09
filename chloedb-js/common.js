// What `Database` (index.js, the database in this process) and `Client`
// (client.js, chloedb-server over HTTP) share: errors, answers, JSON with
// 64-bit integers, and `search` and the rank log on top of `query`.
//
// This module runs anywhere JavaScript does: it imports nothing.

/** An error from the database: a bad schema, query, value or file, or an
 * error a server answered (`status`, the HTTP status, and `answer`, its
 * JSON, are set then). */
export class StrataError extends Error {
  constructor(message, { status, answer, cause } = {}) {
    super(message, cause === undefined ? undefined : { cause });
    this.name = "StrataError";
    if (status !== undefined) this.status = status;
    if (answer !== undefined) this.answer = answer;
  }
}

/** `StrataError` by the database's new name (it was called Strata): the
 * same class, so `instanceof` either one holds for every error. */
export const ChloeDBError = StrataError;

// ---------------------------------------------------------------- JSON

// A BigInt has no JSON form: it is written as this marker in a string,
// which `encode` then replaces with its digits.
const BIG = "\u0000strata-bigint:";
const BIG_TEXT = /"\\u0000strata-bigint:(-?\d+)"/g;

function replacer(_key, v) {
  if (typeof v === "bigint") return BIG + v.toString();
  if (ArrayBuffer.isView(v) && !(v instanceof DataView)) return Array.from(v);
  if (v instanceof Map) return Object.fromEntries(v);
  if (v instanceof Set) return Array.from(v);
  return v;
}

/** A value as JSON text, as ChloeDB reads it: typed arrays (`Float32Array`)
 * become arrays of numbers, a `Map` an object, a `Date` ISO 8601 text, and
 * a BigInt a JSON integer, whole (a 64-bit key). */
export function encode(value) {
  const s = JSON.stringify(value === undefined ? null : value, replacer);
  return s.includes("strata-bigint:") ? s.replace(BIG_TEXT, "$1") : s;
}

// Whether JSON.parse hands a reviver the number's text (V8 since Node 21,
// recent browsers): then integers past 2^53 can come back whole.
const SOURCE_TEXT = (() => {
  try {
    let ok = false;
    JSON.parse("1", (_k, v, c) => {
      ok = c?.source === "1";
      return v;
    });
    return ok;
  } catch {
    return false;
  }
})();

const LONG_DIGITS = /\d{16}/;
const INTEGER = /^-?\d+$/;

function reviver(_key, v, c) {
  return typeof v === "number" && !Number.isSafeInteger(v) && c?.source && INTEGER.test(c.source) ? BigInt(c.source) : v;
}

/** JSON text as values: an integer that a number cannot hold exactly (past
 * 2^53, a 64-bit key) as a BigInt, where the runtime tells its digits. */
export function decode(text) {
  if (text === "" || text === undefined || text === null) return null;
  return SOURCE_TEXT && LONG_DIGITS.test(text) ? JSON.parse(text, reviver) : JSON.parse(text);
}

// ---------------------------------------------------------------- answers

/** A query's answer: `columns`, `rows` (arrays of values in column order),
 * each row's primary key in `keys` and the score it was ranked by in
 * `scores`, and what the engine did in `stats`.
 *
 * Iterating gives each row as an object of column to value; `at(i)` is row
 * `i` as one, and `length` the number of rows. */
export class Result {
  constructor({ columns = [], rows = [], keys = [], scores = [], stats = {}, took_ms, request, teams } = {}) {
    this.columns = columns;
    this.rows = rows;
    this.keys = keys ?? [];
    this.scores = scores ?? [];
    this.stats = stats ?? {};
    /** The server's time for it, in milliseconds (`Client` only). */
    this.tookMs = took_ms;
    /** The request's id in the rank log (`queryLogged`). */
    this.request = request;
    /** An interleaved query's: per row, the ranking that placed it. */
    this.teams = teams;
  }

  get length() {
    return this.rows.length;
  }

  #object(r) {
    const o = {};
    for (let i = 0; i < this.columns.length; i++) o[this.columns[i]] = r[i];
    return o;
  }

  /** Row `i` (negative counts from the end) as an object, or undefined. */
  at(i) {
    const r = this.rows.at(i);
    return r === undefined ? undefined : this.#object(r);
  }

  *[Symbol.iterator]() {
    for (const r of this.rows) yield this.#object(r);
  }

  /** Every row as an object of column to value. */
  objects() {
    return this.rows.map((r) => this.#object(r));
  }

  /** One column's values. */
  column(name) {
    const i = this.columns.indexOf(name);
    if (i < 0) throw new StrataError(`no column ${name}: the columns are ${this.columns.join(", ")}`);
    return this.rows.map((r) => r[i]);
  }

  /** The first column of the first row (null if there is none): a count, a
   * sum. */
  scalar() {
    return this.rows[0]?.[0] ?? null;
  }

  toJSON() {
    return { columns: this.columns, rows: this.rows, keys: this.keys, scores: this.scores, stats: this.stats };
  }
}

/** A read's `Result`, or the number of rows a write wrote. */
export function resultOrWritten(a) {
  return a && "rows" in a ? new Result(a) : Number(a.written);
}

// ---------------------------------------------------------------- shared

/** The short schema form in reading order: table, key, the table's
 * options, fields, and each field's name and type first. */
export function orderedSchema(s) {
  const out = { table: s.table };
  for (const k of ["key", "shard_by", "cluster_by", "retain", "retain_by", "edges"]) {
    if (s[k] !== undefined && s[k] !== null) out[k] = s[k];
  }
  out.fields = s.fields.map((f) => ({ name: f.name, type: f.type, ...f }));
  return out;
}

/** A name as an SQL identifier, whatever it is. */
export function quote(name) {
  return '"' + String(name).replace(/"/g, '""') + '"';
}

/** A table's name in SQL: as it is when it is a plain name, else quoted. */
export function ident(name) {
  return /^[A-Za-z_][A-Za-z0-9_]*$/.test(name) ? name : quote(name);
}

/** Leaves out the entries whose value is undefined or null. */
export function given(o) {
  const out = {};
  for (const [k, v] of Object.entries(o)) if (v !== undefined && v !== null) out[k] = v;
  return out;
}

function time(t) {
  return t instanceof Date ? t.toISOString() : t;
}

/** `search` and the rank log, shared by `Database` and `Client`, which give
 * `schema()`, `tables()`, `_embedded()` (the embedded vector fields whose
 * model is open), `query`, `_queryLogged` and `_exportRankLog`. */
export class Searchable {
  /**
   * Runs a SELECT ranked by an ORDER BY and logs what it served to a log
   * table, for training on later: one row per row answered, with its
   * position, its score and the values its ranking read (`features()`).
   * `result.request` is the request's id, which labels its rows later.
   */
  async queryLogged(sql, params = null, { context, request, variant, candidates, ts, table = "rank_log", interleave, variants } = {}) {
    const il = interleave == null ? undefined : given({ sql: interleave, variants: variants?.length ? [...variants] : undefined });
    const log = given({ table, request, context, variant, candidates, ts: time(ts), interleave: il });
    return this._queryLogged(sql, params, log);
  }

  /** The log table's rows as records for training, from one snapshot, each
   * context's in the order served: the row's columns, `features` as
   * served, and `label` (see chloedb-py's `export_rank_log`). */
  async exportRankLog({ table = "rank_log", events, history = 0, window, grades, since, until } = {}) {
    return this._exportRankLog(given({ table, events, history, window, grades, since: time(since), until: time(until) }));
  }

  /**
   * The rows that best match `text`.
   *
   * `mode` "text" ranks by BM25 over the text fields, "vector" by the
   * similarity of an embedded vector field to the text's embedding, and
   * "hybrid" fuses both by reciprocal rank (RRF). "auto" is hybrid when the
   * table has an embedded field whose model is open here, and text
   * otherwise. `where` is an SQL condition, with `params` for its
   * placeholders.
   */
  async search(text, { limit = 10, where, columns = "*", mode = "auto", textFields, vectorField, params, table } = {}) {
    const s = await this.schema(table);
    if (textFields == null) {
      const texts = s.fields.filter((f) => f.type === "text" && (f.indexed ?? true));
      const generated = texts.filter((f) => f.from).map((f) => f.name);
      textFields = generated.length ? generated : texts.map((f) => f.name);
    }
    textFields = [...textFields];
    if (vectorField == null) {
      const embedded = await this._embedded(s.table);
      vectorField = embedded[0] ?? null;
    }
    if (mode === "auto") mode = vectorField && textFields.length ? "hybrid" : vectorField ? "vector" : "text";
    if (!["text", "vector", "hybrid"].includes(mode)) throw new StrataError('mode is "auto", "text", "vector" or "hybrid"');
    const scores = [];
    if (mode === "text" || mode === "hybrid") {
      if (!textFields.length) throw new StrataError(`${s.table} has no indexed text field to search`);
      for (const f of textFields) scores.push(`bm25(${quote(f)}, :_q)`);
    }
    if (mode === "vector" || mode === "hybrid") {
      if (!vectorField) throw new StrataError(`${s.table} has no embedded vector field with its model open`);
      scores.push(`similarity(${quote(vectorField)}, :_v)`);
    }
    const fields = new Set(s.fields.map((f) => f.name.toLowerCase()));
    const name = (c) => (fields.has(c.toLowerCase()) ? quote(c) : c);
    const cols = typeof columns === "string" ? columns : [...columns].map((c) => name(c.trim())).join(", ");
    const order = scores.length === 1 ? scores[0] : `rrf(${scores.join(", ")})`;
    let sql = `SELECT ${cols} FROM ${ident(s.table)}`;
    if (where != null && String(where).trim()) sql += ` WHERE ${where}`;
    sql += ` ORDER BY ${order} DESC LIMIT ${Math.trunc(Number(limit))}`;
    const p = { ...(params ?? {}) };
    if (order.includes("bm25")) p._q = text;
    if (order.includes("similarity")) p._v = { embed: text, field: vectorField, ...(table == null ? {} : { table: s.table }) };
    return this.query(sql, p);
  }
}

/** The schema of `table` (any case) among `tables`, or the first. */
export function pickTable(tables, table) {
  if (table == null) return tables[0];
  const t = tables.find((t) => t.table.toLowerCase() === String(table).toLowerCase());
  if (!t) throw new StrataError(`no such table: ${table}`);
  return t;
}

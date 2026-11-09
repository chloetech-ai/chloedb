/** A value ChloeDB stores or answers: an INT (a number, or a BigInt past
 * 2^53), FLOAT, text or keyword, BOOL, a TIMESTAMP (ISO 8601 text in UTC),
 * a vector (numbers), a list, or null. */
export type Value = null | boolean | number | bigint | string | Value[] | { [k: string]: Value };

/** A value written: what ChloeDB answers, and also `Date`s (TIMESTAMPs),
 * typed arrays (vectors) and `Map`s (sparse term weights). */
export type Input = Value | Date | Float32Array | Float64Array | Int32Array | Map<string, number> | Input[] | { [k: string]: Input };

/** A row: field name to value. */
export type Row = Record<string, Input>;

/** Query parameters, `:name` in the SQL: values, `{embed: text, field?}`
 * (the text's vector by an embedded field's model), `{embed: {image:
 * base64}}`, or `{vector_of: key, field?}` (a stored row's vector). */
export type Params = Record<string, Input | { embed: string | { image: string } | { audio: string }; field?: string; table?: string } | { vector_of: Input; field?: string; table?: string }>;

/** A field of a schema in its short form. */
export interface Field {
  name: string;
  type: string;
  [option: string]: unknown;
}

/** A table's schema in its short form. */
export interface Schema {
  table: string;
  key?: string;
  fields: Field[];
  [option: string]: unknown;
}

/** A plan of the plan IR, as `Database.plan` and `Client.explain` give it. */
export type Plan = Record<string, unknown>;

export declare class StrataError extends Error {
  constructor(message: string, options?: { status?: number; answer?: unknown; cause?: unknown });
  /** The HTTP status, for an error a server answered. */
  status?: number;
  /** The server's answer, its JSON. */
  answer?: unknown;
}

/** `StrataError` by the database's new name: the same class. */
export { StrataError as ChloeDBError };

/** A query's answer. Iterating gives each row as an object. */
export declare class Result<T extends Record<string, Value> = Record<string, Value>> implements Iterable<T> {
  columns: string[];
  rows: Value[][];
  /** Each row's primary key. */
  keys: Value[];
  /** The score each row was ranked by. */
  scores: (number | null)[];
  /** What the engine did: segments, candidates, time per stage. */
  stats: Record<string, unknown>;
  /** The server's time for it, in milliseconds (`Client` only). */
  tookMs?: number;
  /** The request's id in the rank log (`queryLogged`). */
  request?: string;
  /** An interleaved query's: per row, the ranking that placed it. */
  teams?: (string | null)[];
  readonly length: number;
  /** Row `i` (negative counts from the end) as an object. */
  at(i: number): T | undefined;
  /** Every row as an object. */
  objects(): T[];
  /** One column's values. */
  column(name: string): Value[];
  /** The first column of the first row, or null: a count, a sum. */
  scalar(): Value;
  [Symbol.iterator](): Iterator<T>;
}

export interface SearchOptions {
  limit?: number;
  /** An SQL condition, with `params` for its placeholders. */
  where?: string;
  columns?: string | string[];
  mode?: "auto" | "text" | "vector" | "hybrid";
  textFields?: string[];
  vectorField?: string;
  params?: Params;
  table?: string;
}

export interface LogOptions {
  /** Whom it served: a user, a session. */
  context?: string;
  request?: string;
  /** The ranking's name (an A/B arm). */
  variant?: string;
  /** Logs that many rows past the OFFSET. */
  candidates?: number;
  /** When it was served (a Date, or microseconds); now by default. */
  ts?: Date | number;
  table?: string;
  /** A second SELECT, interleaved with the first by team draft. */
  interleave?: string;
  variants?: string[];
}

export interface ExportOptions {
  table?: string;
  events?: { table: string; user: string; item: string; time: string; action?: string };
  history?: number;
  window?: string | number;
  grades?: Record<string, number>;
  since?: Date | number;
  until?: Date | number;
}

/** What `Database` and `Client` share. */
export declare abstract class Searchable {
  abstract schema(table?: string): Promise<Schema>;
  abstract tables(): Promise<Schema[]>;
  abstract query(sql: string, params?: Params | null): Promise<Result>;
  /** The rows that best match `text`: BM25 over the text fields, the
   * similarity of an embedded vector field, or both fused by RRF. */
  search(text: string, options?: SearchOptions): Promise<Result>;
  /** Runs a SELECT ranked by an ORDER BY and logs what it served. */
  queryLogged(sql: string, params?: Params | null, options?: LogOptions): Promise<Result>;
  /** The log table's rows as records for training. */
  exportRankLog(options?: ExportOptions): Promise<Record<string, Value>[]>;
}

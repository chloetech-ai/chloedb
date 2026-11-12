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

export declare class ChloeDBError extends Error {
  constructor(message: string, options?: { status?: number; answer?: unknown; cause?: unknown; code?: ErrorCode });
  /** The HTTP status, for an error a server answered. */
  status?: number;
  /** The server's answer, its JSON. */
  answer?: unknown;
  /** The error's stable code (see `errorCode`). */
  code: ErrorCode;
}

/** A bad schema, query, value or request (400, 413, 422). Code "bad_request". */
export declare class BadRequestError extends ChloeDBError {}
/** No token, or one without the rights needed (401, 403). Code "unauthorized". */
export declare class UnauthorizedError extends ChloeDBError {}
/** No such row, table, route or cursor (404). Code "not_found". */
export declare class NotFoundError extends ChloeDBError {}
/** Refused by the state it met (409). Code "conflict". */
export declare class ConflictError extends ChloeDBError {}
/** A coordinator wrote some shards and not others. Code "partial_write". */
export declare class PartialWriteError extends ChloeDBError {}
/** The database is open elsewhere, or another writer holds it. Code "busy". */
export declare class BusyError extends ChloeDBError {}
/** Refused for the server's load before any work (429). Code "overloaded". */
export declare class OverloadedError extends ChloeDBError {}
/** A deadline passed (504, a query's timeoutMs). Code "timeout". */
export declare class DeadlineError extends ChloeDBError {}
/** No copy can serve it now (503). Code "unavailable". */
export declare class UnavailableError extends ChloeDBError {}

/** `ChloeDBError` by its name when ChloeDB was called Strata: the same class. */
export { ChloeDBError as StrataError };

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
  /** More rows matched than this answer holds, by a cut the query did not
   * write: a ranked query without LIMIT, or a fused page past its candidates. */
  readonly truncated: boolean;
  /** The row count a ranked query without LIMIT was cut to (100). */
  readonly implicitLimit: number | undefined;
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
  /** Through a `Client`: a write the answer must see ({@link ReadAfter}). */
  minSeqNo?: number;
  minSeqNos?: number[];
  /** Its deadline in ms, as `query` takes it. */
  timeoutMs?: number;
  /** A `Database`'s only: a memory budget of its own, in bytes. */
  memory?: number;
  /** A `Database`'s only: stops it when it aborts. */
  signal?: AbortSignal;
}

/** The writes a read must see: `minSeqNo`, the `seqNo` a server's write
 * answered, or `minSeqNos`, the `seqNos` a coordinator's did (one per
 * shard). The copy that answers holds them; a replica waits up to a second.
 * A Database in this process ignores them: it reads every write. */
export interface ReadAfter {
  minSeqNo?: number;
  minSeqNos?: number[];
}

/** The stable code of an error, the same in this process, from a server
 * and in chloedb-py: the server's JSON `code` when it gives one, else the
 * kind the message starts with ("busy: ..."), else the HTTP status. */
export type ErrorCode = "bad_request" | "unauthorized" | "not_found" | "conflict" | "partial_write" | "busy" | "overloaded" | "timeout" | "memory" | "unavailable" | "corrupt" | "io" | "internal" | "error" | (string & {});

export declare function errorCode(message: string, status?: number, answer?: unknown): ErrorCode;

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

import { Input, Params, Plan, Result, Row, Schema, Searchable, Value } from "./common.js";

export { Field, Input, LogOptions, ExportOptions, Params, Plan, Result, Row, Schema, SearchOptions, ChloeDBError, StrataError, Value } from "./common.js";
export { Client, ClientOptions, connect } from "./client.js";

type Path = string | URL;
/** A schema: an object, JSON text, or the path of a JSON file. */
type SchemaInput = Schema | string;
type Tables = string | Iterable<string>;
type Bytes = Uint8Array | ArrayBuffer | ArrayBufferView;

export interface OpenOptions {
  /** Where embedded fields' models are ($STRATA_MODELS or
   * ~/.local/share/strata/models by default). */
  models?: Path;
  /** Cores vector indexes build on and each query searches segments on
   * (default: every core). */
  threads?: number;
}

export interface ReadOnlyOption {
  /** Open it to read while another process writes it: it sees each of
   * the writer's commits, and every write fails. Without it only one
   * process may have the database open. */
  readOnly?: boolean;
}

export interface CreateOptions extends OpenOptions {
  /** Keep it as a directory of segment files instead of one file. */
  directory?: boolean;
}

/** A ChloeDB database opened in this process, from its file: no server.
 * Only one process may have a database open to write it at a time; others
 * may open it with `readOnly`. */
export declare class Database extends Searchable {
  private constructor();
  /** Creates a database at `path` with `schema`. */
  static create(path: Path, schema: SchemaInput, options?: CreateOptions): Promise<Database>;
  /** Opens the database at `path`. */
  static open(path: Path, options?: OpenOptions & ReadOnlyOption): Promise<Database>;

  /** What the import that made it, or the last import into it, did. */
  lastImport: Record<string, unknown> | null;
  /** Embedded fields whose model did not open, with the reason. */
  readonly embedErrors: Record<string, string>;
  readonly closed: boolean;

  schema(table?: string): Promise<Schema>;
  tables(): Promise<Schema[]>;
  /** Live rows, in every table. */
  count(): Promise<number>;
  addColumn(name: string, type: string, options?: { table?: string; [option: string]: unknown }): Promise<void>;
  dropColumn(name: string, options?: { table?: string }): Promise<void>;
  renameColumn(from: string, to: string, options?: { table?: string }): Promise<void>;

  /** Runs a SELECT. */
  query(sql: string, params?: Params | null): Promise<Result>;
  /** Runs any statement: a SELECT's Result, or the rows a write wrote. */
  sql(sql: string, params?: Params | null): Promise<Result | number>;
  /** The plan of a statement, without running it. */
  plan(sql: string, params?: Params | null): Promise<Plan>;
  /** Runs a plan. */
  run(plan: Plan): Promise<Result | number>;

  /** Inserts rows whose keys are new; all or none. Returns how many. */
  insert(rows: Row | Iterable<Row>, options?: { table?: string }): Promise<number>;
  /** Inserts rows, replacing the rows with their keys. Returns how many. */
  upsert(rows: Row | Iterable<Row>, options?: { table?: string }): Promise<number>;
  /** The row with this key, or null. */
  get(key: Input, options?: { table?: string }): Promise<Record<string, Value> | null>;
  getMany(keys: Iterable<Input>, options?: { table?: string }): Promise<(Record<string, Value> | null)[]>;
  /** Deletes the rows with this key or these keys; returns how many. */
  delete(keys: Input | Iterable<Input>, options?: { table?: string }): Promise<number>;
  /** Deletes the rows whose `expire_by` time has passed. */
  expire(): Promise<number>;
  /** Write operations as /v1/write takes them, in one transaction. */
  write(ops: Iterable<Record<string, Input>>): Promise<number>;

  importFile(path: Path, options?: { format?: "jsonl" | "json" | "csv" | "tsv"; table?: string }): Promise<{ rows: number; bulk: boolean; skippedFields: string[] }>;
  importSqlite(path: Path, options?: { tables?: Tables; schemas?: unknown; blobsBase64?: boolean }): Promise<Record<string, unknown>>;
  exportSqlite(path: Path, options?: { tables?: Tables; fts?: boolean; indexes?: boolean; schemas?: boolean }): Promise<Record<string, unknown>>;
  importParquet(path: Path, options?: { table?: string; schema?: SchemaInput; key?: string; blobsBase64?: boolean }): Promise<Record<string, unknown>>;
  planParquet(path: Path, options?: { table?: string; schema?: SchemaInput; key?: string; blobsBase64?: boolean }): Promise<Record<string, unknown>>;
  exportParquet(path: Path, options?: { tables?: Tables; schemas?: boolean }): Promise<Record<string, unknown>>;

  /** The query vector of a text, or of an image's or a sound's bytes. */
  embed(text?: string | null, options?: { field?: string; table?: string; image?: Bytes; audio?: Bytes }): Promise<number[]>;
  embedTokens(text: string, field: string, options?: { table?: string }): Promise<number[][]>;
  sidNext(prefixes: Iterable<string>, options: { field: string; where?: string; params?: Params; table?: string }): Promise<Record<string, Record<string, number>>>;
  trainCodebook(field: string, name: string, options?: { table?: string; models?: Path; levels?: number; k?: number; iterations?: number; sample?: number; seed?: number }): Promise<string>;

  /** Writes the rows in memory into segments, merges them into as few as
   * it can, drops deleted rows and returns the freed space to the file
   * system. */
  optimize(): Promise<void>;
  /** Returns the space merges freed to the file system: the bytes the file
   * shrank by. */
  vacuum(): Promise<number>;
  /** Opened `readOnly`: reads what the writer committed since the last
   * query, now; whether anything changed. */
  refresh(): Promise<boolean>;
  /** Merges small segments, returns the space merges freed to the file system when it is an eighth of the file or more, and closes the database. */
  close(): Promise<void>;
  [Symbol.asyncDispose](): Promise<void>;
}

/** Opens the database at `path`, creating it with `schema` if it does not
 * exist. */
export declare function open(path: Path, options?: CreateOptions & ReadOnlyOption & { schema?: SchemaInput }): Promise<Database>;
export declare function fromSqlite(sqlitePath: Path, path: Path, options?: CreateOptions & { tables?: Tables; schemas?: unknown; blobsBase64?: boolean }): Promise<Database>;
export declare function planSqlite(sqlitePath: Path, options?: { tables?: Tables; schemas?: unknown; blobsBase64?: boolean }): Promise<Record<string, unknown>>;
export declare function fromParquet(parquetPath: Path, path: Path, options?: CreateOptions & { table?: string; schema?: SchemaInput; key?: string; blobsBase64?: boolean }): Promise<Database>;
export declare function planParquet(parquetPath: Path, options?: { table?: string; schema?: SchemaInput; key?: string; blobsBase64?: boolean }): Promise<Record<string, unknown>>;
export declare function upgrade(path: Path): Promise<{ old: string; tables: number; rows: number } | null>;
export declare function trainCodebook(vectors: Iterable<number[] | Float32Array>, name: string, options?: { models?: Path; levels?: number; k?: number; iterations?: number; sample?: number; seed?: number }): Promise<string>;
/** The addon's version. */
export declare const version: string;

export interface NearbyOptions {
  /** The server's --token or an API key ($CHLOEDB_TOKEN, or $STRATA_TOKEN, by default). */
  token?: string;
  /** Tiles held each way around the user (default 2: a 5 x 5 square). */
  reach?: number;
  /** Milliseconds a request may take (default 30000). */
  timeout?: number;
  /** Milliseconds between asking the server whether the slice is the
   * current one (default 30000; 0 never asks). */
  statusEvery?: number;
  /** Milliseconds a slice may lag a changed table and still answer
   * (default 0: a changed table sends every search to the server). */
  maxLag?: number;
  /** Searches between usage reports to the server (default 20; 0 never). */
  reportEvery?: number;
  /** This device's name in the server's counts (default a random one). */
  device?: string;
  /** A vector slice: fetch every row's codes (default true), which prove
   * far more answers on the device. */
  codes?: boolean;
  fetch?: typeof globalThis.fetch;
}

/** Why a search was answered where it was: "proved" and "no_match" on the
 * device; "short", "bound" and "stale" by the server. */
export type NearbyWhy = "proved" | "no_match" | "short" | "bound" | "stale";

export declare class NearbyResult extends Result {
  /** "device" when the device proved its answer is the server's. */
  where: "device" | "server";
  why: NearbyWhy;
  /** The body of the request (POST /v1/query) that asks the server. */
  ask: string;
  readonly onDevice: boolean;
  proved: boolean;
  stale: boolean;
  /** The most a row not held could score (null when all are held). */
  bound: number | null;
  /** Metres to the nearest tile with rows not held (null when all are
   * held, and for a vector slice). */
  edgeM: number | null;
  deviceMs: number;
  serverMs: number | null;
  /** Clusters a vector search fetched to prove its answer. */
  fetched: number;
}

/** A device's slice of a table on a strata-server. */
export declare class Nearby {
  readonly url: string;
  readonly table: string;
  readonly dir: string;
  readonly device: string;
  reach: number;
  position: [number, number] | null;
  /** The taste last focused on, and the rows held for it. */
  taste: [number[], { rows: number }] | null;
  /** The manifest's summary: kind ("geo" or "vector"), table, version,
   * built_ms, tiles, rows; a geo slice's grid, text, lat and lon; a vector
   * slice's field and dim. */
  info: Record<string, unknown>;
  stats: { searches: number; onDevice: number; fetchedTiles: number; fetchedBytes: number };
  load(): Promise<this>;
  /** Asks the server whether the slice is current; fetches a newer one. */
  check(): Promise<{ table: string; version: number; built_ms: number; fresh: boolean; changed_ms?: number }>;
  /** Moves the user: fetches the tiles around it, drops those far behind. */
  moveTo(lat: number, lon: number, options?: { reach?: number }): Promise<{ fetched: number; dropped: number; bytes: number }>;
  /** A vector slice: holds the clusters whose rows could score highest
   * for taste, as many as fit in rows rows (default 20,000). */
  focus(taste: ArrayLike<number>, options?: { rows?: number }): Promise<{ fetched: number; dropped: number; bytes: number }>;
  /** The tiles held, as [x, y, rows] (a vector slice's cluster is x). */
  held(): [number, number, number][];
  /** The k best places for q near the user: on the device when it can
   * prove the answer is the server's, else the server's. */
  search(q: string, options?: { k?: number; d0?: number; lat?: number; lon?: number }): Promise<NearbyResult>;
  /** A vector slice: the k rows most similar to vector, on the device when
   * it can prove the answer is the server's, else the server's (exact). */
  similar(vector: ArrayLike<number>, options?: { k?: number; refine?: number }): Promise<NearbyResult>;
  /** Sends the counts since the last report to the server. */
  report(): Promise<void>;
  close(): Promise<void>;
}

/** A device's slice of `table` on the strata-server at `url`, kept in
 * directory `dir`. */
export declare function openNearby(url: string, table: string, dir: string, options?: NearbyOptions): Promise<Nearby>;

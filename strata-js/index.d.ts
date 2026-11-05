import { Input, Params, Plan, Result, Row, Schema, Searchable, Value } from "./common.js";

export { Field, Input, LogOptions, ExportOptions, Params, Plan, Result, Row, Schema, SearchOptions, StrataError, Value } from "./common.js";
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

export interface CreateOptions extends OpenOptions {
  /** Keep it as a directory of segment files instead of one file. */
  directory?: boolean;
}

/** A Strata database opened in this process, from its file: no server.
 * Only one process may have a database open at a time. */
export declare class Database extends Searchable {
  private constructor();
  /** Creates a database at `path` with `schema`. */
  static create(path: Path, schema: SchemaInput, options?: CreateOptions): Promise<Database>;
  /** Opens the database at `path`. */
  static open(path: Path, options?: OpenOptions): Promise<Database>;

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
  /** Merges small segments, returns the space merges freed to the file system when it is an eighth of the file or more, and closes the database. */
  close(): Promise<void>;
  [Symbol.asyncDispose](): Promise<void>;
}

/** Opens the database at `path`, creating it with `schema` if it does not
 * exist. */
export declare function open(path: Path, options?: CreateOptions & { schema?: SchemaInput }): Promise<Database>;
export declare function fromSqlite(sqlitePath: Path, path: Path, options?: CreateOptions & { tables?: Tables; schemas?: unknown; blobsBase64?: boolean }): Promise<Database>;
export declare function planSqlite(sqlitePath: Path, options?: { tables?: Tables; schemas?: unknown; blobsBase64?: boolean }): Promise<Record<string, unknown>>;
export declare function fromParquet(parquetPath: Path, path: Path, options?: CreateOptions & { table?: string; schema?: SchemaInput; key?: string; blobsBase64?: boolean }): Promise<Database>;
export declare function planParquet(parquetPath: Path, options?: { table?: string; schema?: SchemaInput; key?: string; blobsBase64?: boolean }): Promise<Record<string, unknown>>;
export declare function upgrade(path: Path): Promise<{ old: string; tables: number; rows: number } | null>;
export declare function trainCodebook(vectors: Iterable<number[] | Float32Array>, name: string, options?: { models?: Path; levels?: number; k?: number; iterations?: number; sample?: number; seed?: number }): Promise<string>;
/** The addon's version. */
export declare const version: string;

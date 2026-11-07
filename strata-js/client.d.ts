import { Input, Params, Plan, Result, Row, Schema, Searchable, Value } from "./common.js";

export { Field, Input, LogOptions, ExportOptions, Params, Plan, Result, Row, Schema, SearchOptions, StrataError, Value } from "./common.js";

/** A file to upload: a path (read with node:fs) or its bytes. */
type File = string | URL | Uint8Array | ArrayBuffer | ArrayBufferView | Blob;
type Tables = string | Iterable<string>;

export interface ClientOptions {
  /** The server's --token or an API key ($STRATA_TOKEN by default). */
  token?: string;
  /** Milliseconds a request may take (0 for no limit); 30 s by default. */
  timeout?: number;
  /** Milliseconds a request the server refused for its load (429) is sent
   * again for, after its Retry-After; 10 s by default, 0 for never. A write
   * that failed partway is sent again only with an idempotency key. */
  retryFor?: number;
  /** The fetch to use (globalThis.fetch by default). */
  fetch?: typeof fetch;
}

export declare const WRITE_BATCH: number;
export declare const GET_BATCH: number;

/** A strata-server, over its HTTP JSON API, from any runtime with fetch. */
export declare class Client extends Searchable {
  constructor(url?: string, options?: ClientOptions);
  url: string;
  token?: string;
  timeout: number;
  retryFor: number;

  schema(table?: string): Promise<Schema>;
  tables(): Promise<Schema[]>;
  health(): Promise<Record<string, unknown>>;
  stats(): Promise<Record<string, unknown>>;
  count(): Promise<number>;

  query(sql: string, params?: Params | null, options?: { timeoutMs?: number }): Promise<Result>;
  sql(sql: string, params?: Params | null): Promise<Result | number>;
  explain(sql: string, params?: Params | null): Promise<Record<string, unknown>>;
  run(plan: Plan): Promise<Result | number>;
  embed(text?: string | null, options?: { field?: string; table?: string; image?: Uint8Array | ArrayBuffer | Blob; audio?: Uint8Array | ArrayBuffer | Blob }): Promise<number[]>;
  embedTokens(text: string, field: string, options?: { table?: string }): Promise<number[][]>;
  sidNext(prefixes: Iterable<string>, options: { field: string; where?: string; params?: Params; table?: string }): Promise<Record<string, Record<string, number>>>;

  write(ops: Iterable<Record<string, Input>>): Promise<number>;
  insert(rows: Row | Iterable<Row>, options?: { table?: string }): Promise<number>;
  upsert(rows: Row | Iterable<Row>, options?: { table?: string }): Promise<number>;
  delete(keys: Input | Iterable<Input>, options?: { table?: string; routing?: Input }): Promise<number>;
  get(key: Input, options?: { table?: string }): Promise<Record<string, Value> | null>;
  getMany(keys: Iterable<Input>, options?: { table?: string; columns?: Iterable<string> }): Promise<(Record<string, Value> | null)[]>;
  put(key: Input, row: Row, options?: { table?: string; ifSeqNo?: number }): Promise<number>;
  stream(events: Iterable<Record<string, Input>> | AsyncIterable<Record<string, Input>>): Promise<Record<string, unknown>>;
  source(name: string): Promise<number | null>;
  sources(): Promise<{ name: string; offset: number; updated: string }[]>;

  importFile(file: File, options?: { format?: "jsonl" | "json" | "csv" | "tsv"; table?: string }): Promise<{ rows: number; bulk: boolean; skippedFields: string[] }>;
  importSqlite(file: File, options?: { tables?: Tables; blobsBase64?: boolean }): Promise<Record<string, unknown>>;
  planSqlite(file: File, options?: { tables?: Tables; blobsBase64?: boolean }): Promise<Record<string, unknown>>;
  importParquet(file: File, options?: { table?: string; key?: string; blobsBase64?: boolean }): Promise<Record<string, unknown>>;
  planParquet(file: File, options?: { table?: string; key?: string; blobsBase64?: boolean }): Promise<Record<string, unknown>>;
  /** To `path`, which must not exist, or, with `path` null, as `data`. */
  exportSqlite(path: string | URL | null, options?: { tables?: Tables; fts?: boolean; indexes?: boolean; schemas?: boolean }): Promise<{ tables: string[]; data?: Uint8Array }>;
  exportParquet(path: string | URL | null, options?: { table?: string; schemas?: boolean }): Promise<{ tables: string[]; data?: Uint8Array }>;
  ask(question: string, options?: { model?: string; effort?: string; apiKey?: string; maxSteps?: number }): Promise<Record<string, unknown>>;

  close(): Promise<void>;
  [Symbol.asyncDispose](): Promise<void>;
}

/** A Client of the strata-server at `url`. */
export declare function connect(url?: string, options?: ClientOptions): Client;

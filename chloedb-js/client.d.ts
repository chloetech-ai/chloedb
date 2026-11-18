/// <reference lib="esnext.disposable" />
import { Input, PageOptions, Params, Plan, ReadAfter, Result, Row, Schema, Searchable, Value } from "./common.js";

export { Corrected, CorrectedWord, Field, FieldType, HighlightOptions, Input, LogOptions, ExportOptions, PageOptions, Params, Plan, Result, Row, Schema, ScrollOptions, SearchOptions, ChloeDBError, StrataError, ErrorCode, errorCode, BadRequestError, UnauthorizedError, NotFoundError, ConflictError, PartialWriteError, BusyError, OverloadedError, DeadlineError, UnavailableError, ReadAfter, Value } from "./common.js";

/** A write's answer: the rows written, and its sequence numbers when the
 * server numbers its writes (--seq-no): `seqNo` from a server, `seqNos`
 * from a coordinator (one per shard, 0 where it wrote nothing). */
export interface Written {
  written: number;
  seqNo?: number;
  seqNos?: number[];
  /** Stream events written already, skipped. */
  skipped?: number;
}

/** A file to upload: a path (read with node:fs) or its bytes. */
type File = string | URL | Uint8Array | ArrayBuffer | ArrayBufferView | Blob;
type Tables = string | Iterable<string>;

export interface ClientOptions {
  /** The server's --token or an API key ($CHLOEDB_TOKEN, or $STRATA_TOKEN, by default). */
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

/** A chloedb-server, over its HTTP JSON API, from any runtime with fetch. */
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

  /** Runs a SELECT; its rows are `T`s (unchecked). With `pageSize`, the
   * first page of the server's scroll, and its `cursor` while rows are
   * left. */
  query<T = Record<string, Value>>(sql: string, params?: Params | null, options?: { timeoutMs?: number } & ReadAfter & PageOptions): Promise<Result<T>>;
  /** The next page of a cursor; `cursor` is null after the last. */
  next<T = Record<string, Value>>(cursor: string, options?: { keepAliveMs?: number; timeoutMs?: number }): Promise<Result<T>>;
  /** Ends a cursor before its last page; whether it was open. */
  closeCursor(cursor: string): Promise<boolean>;
  /** With `idempotencyKey`, a write sent again (after a lost answer, or
   * the `idempotency_key` of an error that says it failed partway through
   * a coordinator) is not applied twice. */
  sql<T = Record<string, Value>>(sql: string, params?: Params | null, options?: { idempotencyKey?: string } & ReadAfter): Promise<Result<T> | number>;
  /** Runs an INSERT, UPDATE or DELETE (or a statement that makes or
   * changes a table): the rows it wrote. A SELECT is refused. */
  execute(sql: string, params?: Params | null, options?: { idempotencyKey?: string }): Promise<number>;
  explain(sql: string, params?: Params | null): Promise<Record<string, unknown>>;
  run<T = Record<string, Value>>(plan: Plan, options?: ReadAfter): Promise<Result<T> | number>;
  embed(text?: string | null, options?: { field?: string; table?: string; image?: Uint8Array | ArrayBuffer | Blob; audio?: Uint8Array | ArrayBuffer | Blob }): Promise<number[]>;
  embedTokens(text: string, field: string, options?: { table?: string }): Promise<number[][]>;
  sidNext(prefixes: Iterable<string>, options: { field: string; where?: string; params?: Params; table?: string } & ReadAfter): Promise<Record<string, Record<string, number>>>;

  write(ops: Iterable<Record<string, Input>>, options?: { idempotencyKey?: string }): Promise<number>;
  /** `write`, answering its sequence numbers too, for a read's `minSeqNo(s)`. */
  writeResult(ops: Iterable<Record<string, Input>>, options?: { idempotencyKey?: string }): Promise<Written>;
  /** A `Session` on this client's server, with its options. */
  session(): Session;
  insert(rows: Row | Iterable<Row>, options?: { table?: string }): Promise<number>;
  upsert(rows: Row | Iterable<Row>, options?: { table?: string }): Promise<number>;
  delete(keys: Input | Iterable<Input>, options?: { table?: string; routing?: Input }): Promise<number>;
  get<T = Record<string, Value>>(key: Input, options?: { table?: string } & ReadAfter): Promise<T | null>;
  getMany<T = Record<string, Value>>(keys: Iterable<Input>, options?: { table?: string; columns?: Iterable<string> } & ReadAfter): Promise<(T | null)[]>;
  put(key: Input, row: Row, options?: { table?: string; ifSeqNo?: number }): Promise<number>;
  stream(events: Iterable<Record<string, Input>> | AsyncIterable<Record<string, Input>>): Promise<Record<string, unknown>>;
  source(name: string): Promise<number | null>;
  sources(): Promise<{ name: string; offset: number; updated: string }[]>;

  importFile(file: File, options?: { format?: "jsonl" | "json" | "csv" | "tsv"; table?: string }): Promise<{ rows: number; bulk: boolean; skippedFields: string[] }>;
  importSqlite(file: File, options?: { tables?: Tables; blobsBase64?: boolean; append?: boolean }): Promise<Record<string, unknown>>;
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

/** A Client whose reads see its own writes, from any copy: it keeps the
 * latest sequence numbers its writes answered and sends them with every
 * read. The servers must number their writes (--seq-no). */
export declare class Session extends Client {
  /** The latest number a server's write answered this session. */
  seqNo?: number;
  /** The latest numbers a coordinator's writes answered, per shard. */
  seqNos?: number[];
  /** Makes later reads see this write (a `Written`, or a server's answer). */
  observe(answer: Partial<Written> | { seq_no?: number; seq_nos?: number[] }): void;
}

/** A Client of the chloedb-server at `url`. */
export declare function connect(url?: string, options?: ClientOptions): Client;

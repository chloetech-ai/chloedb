// A client of chloedb-server over HTTP, for the prototypes. One server holds every
// app's tables; each app names its tables with its own prefix (`blackbox_sessions`).
//
//   import { sql, query, write, schema } from '../../lib/db.mjs';
//   const r = await query('SELECT id, title FROM x_docs WHERE MATCH(title, :q) LIMIT 5', { q: 'tent' });
//   r.rows      -> [{ id, title }]   (objects)
//   r.took_ms   -> the server's own time; r.ms -> the round trip from here
//   r.plan      -> the server's plan lines (what the engine did), for "the SQL behind this view"
//
// CHLOEDB_URL (default http://127.0.0.1:7700) and CHLOEDB_TOKEN choose the server.

export const BASE = (process.env.CHLOEDB_URL || 'http://127.0.0.1:7700').replace(/\/$/, '');
const TOKEN = process.env.CHLOEDB_TOKEN || '';

export class DbError extends Error {
  constructor(message, status, data) { super(message); this.status = status; this.data = data; }
}

export async function call(method, path, body, { timeoutMs = 60000, headers = {}, raw = false } = {}) {
  const ctl = new AbortController();
  const timer = setTimeout(() => ctl.abort(), timeoutMs);
  const t0 = performance.now();
  try {
    const res = await fetch(BASE + path, {
      method,
      signal: ctl.signal,
      headers: {
        ...(body !== undefined && typeof body !== 'string' ? { 'content-type': 'application/json' } : {}),
        ...(TOKEN ? { authorization: `Bearer ${TOKEN}` } : {}),
        ...headers,
      },
      body: body === undefined ? undefined : (typeof body === 'string' ? body : JSON.stringify(body)),
    });
    const text = await res.text();
    const ms = performance.now() - t0;
    if (raw) return { status: res.status, text, ms };
    let data = null;
    try { data = text ? JSON.parse(text) : null; } catch { data = { raw: text }; }
    if (!res.ok) {
      const msg = (data && (data.error || data.message)) || text.slice(0, 400);
      throw new DbError(`chloedb ${method} ${path} -> ${res.status}: ${typeof msg === 'string' ? msg : JSON.stringify(msg)}`, res.status, data);
    }
    return { data, ms };
  } catch (e) {
    if (e.name === 'AbortError') throw new DbError(`chloedb ${method} ${path} timed out after ${timeoutMs} ms`, 504);
    throw e;
  } finally {
    clearTimeout(timer);
  }
}

function shape(data, ms, statement) {
  const columns = data?.columns || [];
  let rows = data?.rows || [];
  if (rows.length && Array.isArray(rows[0])) {
    rows = rows.map(r => Object.fromEntries(columns.map((c, i) => [c, r[i]])));
  }
  return {
    columns, rows, scores: data?.scores || [], keys: data?.keys || [],
    took_ms: data?.took_ms ?? null, ms, plan: data?.stats?.plan || [], stats: data?.stats || {},
    sql: statement, raw: data,
  };
}

// A SELECT (or any statement) through /v1/query, rows as objects.
export async function query(statement, params = {}, opts = {}) {
  const { data, ms } = await call('POST', '/v1/query', { sql: statement, params, objects: true, ...(opts.body || {}) }, opts);
  return shape(data, ms, statement);
}

// Any one statement through /v1/sql (DDL, SHOW, INSERT, SELECT).
export async function sql(statement, params, opts = {}) {
  const { data, ms } = await call('POST', '/v1/sql', params ? { sql: statement, params } : { sql: statement }, opts);
  return shape(data, ms, statement);
}

// Rows in one transaction: ops = [{op:'upsert', table, doc}, {op:'delete', table, key}, ...].
export async function write(ops, opts = {}) {
  const { data, ms } = await call('POST', '/v1/write', { ops }, opts);
  return { ...data, ms };
}

// Upserts many docs in batches of `size`, calling onProgress(n) after each.
export async function upsertAll(table, docs, { size = 500, onProgress } = {}) {
  let n = 0;
  for (let i = 0; i < docs.length; i += size) {
    await write(docs.slice(i, i + size).map(doc => ({ op: 'upsert', table, doc })), { timeoutMs: 300000 });
    n += Math.min(size, docs.length - i);
    if (onProgress) onProgress(n);
  }
  return n;
}

export async function tables() {
  const r = await sql('SHOW TABLES');
  return r.rows.map(row => Object.values(row)[0]);
}

export async function tableExists(name) {
  return (await tables()).includes(name);
}

// CREATE TABLE only when it is missing. Returns true when it created it.
export async function ensureTable(name, ddl) {
  if (await tableExists(name)) return false;
  await sql(ddl);
  return true;
}

export async function count(table, where = '', params = {}) {
  const r = await query(`SELECT COUNT(*) AS n FROM ${table}${where ? ' WHERE ' + where : ''}`, params);
  return r.rows[0]?.n ?? 0;
}

export async function schema(table) {
  const { data } = await call('GET', `/v1/schema${table ? '?table=' + encodeURIComponent(table) : ''}`);
  return data;
}

export async function health() {
  const { data, ms } = await call('GET', '/v1/health', undefined, { timeoutMs: 5000 });
  return { ...data, ms };
}

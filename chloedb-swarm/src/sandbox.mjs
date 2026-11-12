// A sandbox: a chloedb-server of its own, on a folder of its own, that one
// simulated session plays against and that is thrown away after it. The
// server starts with a random root token that only the runner holds; the
// agent gets an API key with the scope its task needs, which dies with the
// server. Nothing here can reach a real cluster's data.
//
//   const sb = await Sandbox.start({ bin, dir });
//   await sb.load(fixture, seed);
//   const key = await sb.key('swarm-agent', 'write');
//   ... await sb.stop();

import { spawn } from 'node:child_process';
import { createServer } from 'node:net';
import { mkdir, open } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { randomBytes } from 'node:crypto';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { rows as generate } from './fixtures.mjs';

const HERE = fileURLToPath(new URL('.', import.meta.url));

/** The chloedb-server binary: CHLOEDB_SERVER_BIN, else the repository's build. */
export function serverBin() {
  if (process.env.CHLOEDB_SERVER_BIN) return process.env.CHLOEDB_SERVER_BIN;
  for (const p of ['release', 'ci', 'debug']) {
    const b = resolve(HERE, '../../chloedb-server/target', p, 'chloedb-server');
    if (existsSync(b)) return b;
  }
  return 'chloedb-server';
}

/** A port nobody is listening on now. */
export function freePort() {
  return new Promise((ok, fail) => {
    const s = createServer();
    s.unref();
    s.on('error', fail);
    s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => ok(port)); });
  });
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** A small client of chloedb-server's HTTP API. */
export class Server {
  constructor(url, token, headers = {}) { this.url = url.replace(/\/$/, ''); this.token = token; this.headers = headers; }

  async call(method, path, body, { raw = false, type = 'application/json', timeout = 60_000 } = {}) {
    const headers = { ...this.headers };
    if (this.token) headers.authorization = `Bearer ${this.token}`;
    if (body !== undefined) headers['content-type'] = type;
    const res = await fetch(this.url + path, { method, headers, body: body === undefined ? undefined : (typeof body === 'string' ? body : JSON.stringify(body)), signal: AbortSignal.timeout(timeout) });
    const text = await res.text();
    let json = null;
    try { json = text ? JSON.parse(text) : null; } catch { /* not JSON */ }
    if (raw) return { status: res.status, body: json ?? text };
    if (!res.ok) throw Object.assign(new Error(`${method} ${path}: ${res.status} ${(json?.error || json?.message || text).toString().slice(0, 300)}`), { status: res.status });
    return json ?? text;
  }

  /** A statement through /v1/sql (`params` named, :name); rows come back as objects. */
  async sql(sql, params) {
    const r = await this.call('POST', '/v1/sql', params ? { sql, params } : { sql });
    if (!Array.isArray(r?.rows)) return r;
    const cols = r.columns || [];
    return r.rows.map((row) => (Array.isArray(row) ? Object.fromEntries(cols.map((c, i) => [c, row[i]])) : row));
  }

  /** Rows into a table, as JSON Lines. */
  async import(table, rows) {
    if (!rows.length) return { written: 0 };
    return this.call('POST', `/v1/import?format=jsonl&table=${encodeURIComponent(table)}`, rows.map((r) => JSON.stringify(r)).join('\n'), { type: 'application/x-ndjson' });
  }

  async health() { return this.call('GET', '/v1/health', undefined, { timeout: 2000 }); }
}

export class Sandbox extends Server {
  /**
   * Start one. `dir` is its folder (made if missing), `bin` the server.
   * Answers once the server is healthy, or fails with its log's path.
   */
  static async start({ bin = serverBin(), dir, args = [] } = {}) {
    await mkdir(dir, { recursive: true });
    const port = await freePort();
    const token = `swarm-${randomBytes(18).toString('hex')}`;
    const log = await open(join(dir, 'chloedb-server.log'), 'a');
    const child = spawn(bin, ['--db', join(dir, 'sandbox.strata'), '--listen', `127.0.0.1:${port}`, '--grpc', 'off', '--token', token, ...args], {
      stdio: ['ignore', log.fd, log.fd],
      env: { ...process.env, CHLOEDB_LOG: process.env.CHLOEDB_LOG || 'warn' },
    });
    let exited = null;
    child.on('exit', (code, sig) => { exited = code ?? sig; });
    child.on('error', (e) => { exited = e.message; });
    const sb = new Sandbox(`http://127.0.0.1:${port}`, token);
    Object.assign(sb, { child, dir, port, log });
    const t0 = Date.now();
    for (;;) {
      if (exited !== null) { await log.close(); throw new Error(`chloedb-server exited (${exited}): see ${join(dir, 'chloedb-server.log')}`); }
      try { await sb.health(); return sb; } catch { /* not yet */ }
      if (Date.now() - t0 > 60_000) { await sb.stop(); throw new Error(`chloedb-server did not answer in a minute at ${sb.url}`); }
      await sleep(100);
    }
  }

  /** An API key for the agent: `scope` is read, write or admin; it expires in a day. */
  async key(name, scope = 'write') {
    const r = await this.call('POST', '/v1/admin/keys', { name, scope, expires_days: 1, note: 'chloedb-swarm sandbox' });
    return r.secret;
  }

  /**
   * The scenario's starting data: `sql` statements run in order, then each
   * table's generated or given rows (see fixtures.mjs).
   */
  async load(fixture = {}, seed = 1) {
    for (const s of fixture.sql || []) await this.sql(s);
    for (const t of fixture.tables || []) {
      if (t.create) await this.sql(t.create);
      const rows = t.rows ? t.rows : generate(t.generate, seed);
      for (let i = 0; i < rows.length; i += 500) await this.import(t.name, rows.slice(i, i + 500));
    }
  }

  /** SIGTERM, then SIGKILL after five seconds. */
  async stop() {
    const c = this.child;
    if (c && c.exitCode === null && c.signalCode === null) {
      const gone = new Promise((r) => c.once('exit', r));
      c.kill('SIGTERM');
      const t = setTimeout(() => c.kill('SIGKILL'), 5000);
      await gone;
      clearTimeout(t);
    }
    await this.log?.close().catch(() => {});
  }
}

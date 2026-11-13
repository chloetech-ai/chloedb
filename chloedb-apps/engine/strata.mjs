// A small client of chloedb-server's HTTP API, and the process that runs one
// when the engine starts its own.

import { spawn } from 'node:child_process';
import { existsSync, mkdirSync } from 'node:fs';
import net from 'node:net';
import path from 'node:path';

export class Strata {
  constructor(url, token) {
    this.url = url.replace(/\/$/, '');
    this.token = token || null;
    // The credential the feed is read with (follow): the one given, when
    // the engine trades it for a key of its own (appkeys.mjs).
    this.feedToken = this.token;
  }

  // Sends `token` from now on, but for the feed.
  useToken(token) {
    this.token = token || null;
  }

  async req(method, route, body) {
    const headers = { 'content-type': 'application/json' };
    if (this.token) headers.authorization = `Bearer ${this.token}`;
    const res = await fetch(this.url + route, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
    const text = await res.text();
    let data;
    try { data = text ? JSON.parse(text) : {}; } catch { data = { error: text }; }
    if (!res.ok) {
      const msg = (data && (data.error || data.message)) || text || res.statusText;
      const err = new Error(`strata ${route}: ${typeof msg === 'string' ? msg : JSON.stringify(msg)}`);
      err.status = res.status;
      throw err;
    }
    return data;
  }

  sql(sql, params) { return this.req('POST', '/v1/sql', params ? { sql, params } : { sql }); }

  async query(sql, params) {
    const out = await this.req('POST', '/v1/query', { sql, params: params || {}, objects: true });
    return out.rows || [];
  }

  // The plan of a query (and its run's statistics, not its rows).
  explain(sql, params) { return this.req('POST', '/v1/explain', { sql, params: params || {} }); }

  write(ops) { return this.req('POST', '/v1/write', { ops }); }

  async get(table, keys) {
    const out = await this.req('POST', '/v1/kv/get', { table, keys });
    return out.rows || [];
  }

  async tables() {
    const out = await this.req('GET', '/v1/tables');
    return out.tables || [];
  }

  health() { return this.req('GET', '/v1/health'); }

  // Follows the server's feed of committed transactions (`--seq-no`), calling
  // onTxn({seq, ops}) for each, and reconnecting when it ends. Returns false
  // when the server serves no feed.
  async follow(onTxn, log = () => {}) {
    let from = null;
    const open = async (start) => {
      const headers = {};
      if (this.feedToken) headers.authorization = `Bearer ${this.feedToken}`;
      const res = await fetch(`${this.url}/v1/feed?from=${start}`, { headers });
      if (!res.ok || !res.body) throw new Error(`feed: ${res.status} ${await res.text().catch(() => '')}`);
      return res.body.getReader();
    };
    // The first line says where the feed is; start after it.
    try {
      const reader = await open(1);
      const first = await readLine(reader);
      reader.cancel().catch(() => {});
      const head = JSON.parse(first || '{}');
      if (head.feed) from = (head.feed.last || 0) + 1;
      else if (head.gone) from = head.gone.oldest;
      else throw new Error(`feed: ${first}`);
    } catch (e) {
      log(`no feed (${e.message}); changes are announced by the engine itself`);
      return false;
    }
    (async () => {
      for (;;) {
        try {
          const reader = await open(from);
          for (;;) {
            const line = await readLine(reader);
            if (line === null) break;
            if (!line.trim()) continue;
            const msg = JSON.parse(line);
            if (msg.gone) { from = msg.gone.oldest; break; }
            if (typeof msg.seq === 'number') {
              from = msg.seq + 1;
              if (msg.ops) {
                try { await onTxn(msg); } catch (e) { log(`feed handler: ${e.message}`); }
              }
            }
          }
        } catch (e) {
          log(`feed: ${e.message}; reconnecting`);
        }
        await new Promise((r) => setTimeout(r, 500));
      }
    })();
    return true;
  }
}

const decoders = new WeakMap();
async function readLine(reader) {
  let st = decoders.get(reader);
  if (!st) { st = { buf: '', dec: new TextDecoder(), done: false }; decoders.set(reader, st); }
  for (;;) {
    const i = st.buf.indexOf('\n');
    if (i >= 0) {
      const line = st.buf.slice(0, i);
      st.buf = st.buf.slice(i + 1);
      return line;
    }
    if (st.done) {
      if (st.buf) { const rest = st.buf; st.buf = ''; return rest; }
      return null;
    }
    const { value, done } = await reader.read();
    if (done) st.done = true;
    else st.buf += st.dec.decode(value, { stream: true });
  }
}

// Finds a chloedb-server binary: $STRATA_SERVER_BIN, a build in the checkout,
// or one on PATH.
export function findServerBinary(repoRoot) {
  const candidates = [
    process.env.STRATA_SERVER_BIN,
    path.join(repoRoot, 'chloedb-server/target/release/chloedb-server'),
    path.join(repoRoot, 'chloedb-server/target/ci/chloedb-server'),
    path.join(repoRoot, 'chloedb-apps/.bin/chloedb-server'),
  ].filter(Boolean);
  for (const c of candidates) if (existsSync(c)) return c;
  for (const dir of (process.env.PATH || '').split(path.delimiter)) {
    const c = path.join(dir, 'chloedb-server');
    if (dir && existsSync(c)) return c;
  }
  return null;
}

// Starts chloedb-server on `db` and waits until it answers. With `shards`
// (or $STRATA_APPS_TEST_SHARDS, for the tests), a cluster instead: that many
// shard servers next to `db` and a coordinator over them on `listen`, as the
// engine finds it beside a cluster; `child.kill()` stops them all.
// With `token`, one server that takes it (`--token`; the tests' servers
// with keys), its client sending it.
export async function startServer({ bin, db, listen, log, token, shards = Number(process.env.STRATA_APPS_TEST_SHARDS) || 0 }) {
  if (shards > 0 && !token) return startCluster({ bin, db, listen, log, shards });
  const auth = token ? ['--token', token] : [];
  const started = await startOne({ bin, args: ['--db', db, '--seq-no', '--listen', listen, '--grpc', 'off', ...auth], listen, log, db, token });
  return started;
}

// A port of 127.0.0.1 that nothing listens on now, as the system hands one out.
function freePort() {
  return new Promise((resolve, reject) => {
    const s = net.createServer();
    s.on('error', reject);
    s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => resolve(port)); });
  });
}

// A shard's ports are the system's free ones, not the ones after `listen`'s:
// those can be another process's (tests run side by side), and between
// asking and the server's bind one can still be taken, so a shard that
// finds its port in use is started again on new ones.
async function startShard({ bin, db, i, log }) {
  for (let attempt = 1; ; attempt++) {
    const http = `127.0.0.1:${await freePort()}`;
    const grpc = `127.0.0.1:${await freePort()}`;
    try {
      const { child } = await startOne({ bin, args: ['--db', `${db}.shard-${i}`, '--seq-no', '--listen', http, '--grpc', grpc], listen: http, log, db });
      return { child, grpc };
    } catch (e) {
      if (attempt >= 5 || !/Address already in use/.test(e.message)) throw e;
      log(`shard ${i}: a port was taken, starting it again`);
    }
  }
}

async function startCluster({ bin, db, listen, log, shards }) {
  const kids = [];
  const stop = () => { for (const k of kids) k.kill(); };
  try {
    const grpcs = [];
    for (let i = 0; i < shards; i++) {
      const { child, grpc } = await startShard({ bin, db, i, log });
      kids.push(child);
      grpcs.push(`http://${grpc}`);
    }
    const { child, client } = await startOne({ bin, args: ['--shards', grpcs.join(','), '--listen', listen, '--grpc', 'off'], listen, log, db });
    kids.push(child);
    return { child: { kill: stop, on: (ev, fn) => child.on(ev, fn) }, client };
  } catch (e) {
    stop();
    throw e;
  }
}

async function startOne({ bin, args, listen, log, db, token }) {
  mkdirSync(path.dirname(db), { recursive: true });
  log(`starting ${bin} ${args.join(' ')}`);
  const child = spawn(bin, args, { stdio: ['ignore', 'pipe', 'pipe'] });
  const tail = [];
  const keep = (d) => { for (const l of String(d).split('\n')) if (l.trim()) { tail.push(l); if (tail.length > 40) tail.shift(); } };
  child.stdout.on('data', keep);
  child.stderr.on('data', keep);
  let exited = null;
  child.on('exit', (code) => { exited = code; });
  const client = new Strata(`http://${listen}`, token);
  for (let i = 0; i < 300; i++) {
    if (exited !== null) throw new Error(`chloedb-server exited (${exited}):\n${tail.join('\n')}`);
    try { await client.health(); return { child, client }; } catch { /* not yet */ }
    await new Promise((r) => setTimeout(r, 100));
  }
  child.kill();
  throw new Error(`chloedb-server did not answer on ${listen}:\n${tail.join('\n')}`);
}

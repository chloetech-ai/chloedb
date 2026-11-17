// Small HTTP helpers shared by the hub server and the apps: JSON answers, bodies,
// server-sent events and static files. Standard library only.
import { createReadStream, promises as fsp } from 'node:fs';
import path from 'node:path';

const TYPES = {
  '.html': 'text/html; charset=utf-8', '.htm': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8', '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8', '.json': 'application/json; charset=utf-8',
  '.geojson': 'application/geo+json', '.svg': 'image/svg+xml', '.png': 'image/png',
  '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.webp': 'image/webp', '.gif': 'image/gif',
  '.ico': 'image/x-icon', '.woff2': 'font/woff2', '.woff': 'font/woff', '.txt': 'text/plain; charset=utf-8',
  '.md': 'text/markdown; charset=utf-8', '.csv': 'text/csv; charset=utf-8', '.wasm': 'application/wasm',
  '.mp4': 'video/mp4', '.webm': 'video/webm', '.map': 'application/json; charset=utf-8',
};

export function json(res, status, data, headers = {}) {
  const body = JSON.stringify(data);
  res.writeHead(status, { 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store', ...headers });
  res.end(body);
}

export function text(res, status, body, type = 'text/plain; charset=utf-8') {
  res.writeHead(status, { 'content-type': type, 'cache-control': 'no-store' });
  res.end(body);
}

export async function readBody(req, limit = 2_000_000) {
  const chunks = [];
  let size = 0;
  for await (const c of req) {
    size += c.length;
    if (size > limit) throw Object.assign(new Error('request body too large'), { status: 413 });
    chunks.push(c);
  }
  const raw = Buffer.concat(chunks).toString('utf8');
  if (!raw) return {};
  try { return JSON.parse(raw); } catch { throw Object.assign(new Error('body is not JSON'), { status: 400 }); }
}

// Server-sent events. `send(event, data)` writes one event; a comment every 15 s keeps
// proxies from closing a quiet stream. `closed()` says whether the client went away.
export function sse(res) {
  res.writeHead(200, {
    'content-type': 'text/event-stream; charset=utf-8', 'cache-control': 'no-cache, no-transform',
    connection: 'keep-alive', 'x-accel-buffering': 'no',
  });
  res.write(': open\n\n');
  let closed = false;
  const ping = setInterval(() => { if (!closed) res.write(': ping\n\n'); }, 15000);
  res.on('close', () => { closed = true; clearInterval(ping); });
  return {
    send(event, data) { if (!closed) res.write(`event: ${event}\ndata: ${JSON.stringify(data)}\n\n`); },
    end() { clearInterval(ping); if (!closed) res.end(); closed = true; },
    closed: () => closed,
  };
}

export function clientIp(req) {
  const xf = req.headers['x-forwarded-for'];
  if (typeof xf === 'string' && xf) return xf.split(',')[0].trim();
  return req.socket.remoteAddress || 'unknown';
}

// Serves `rel` (a URL path) from `root`, refusing anything outside it. Directories
// answer their index.html. Returns false when there is no such file.
export async function serveStatic(req, res, root, rel, { maxAge = 60 } = {}) {
  let p;
  try { p = decodeURIComponent(rel.split('?')[0]); } catch { return false; }
  const full = path.resolve(root, '.' + path.posix.normalize('/' + p));
  if (full !== root && !full.startsWith(root + path.sep)) return false;
  let st;
  try { st = await fsp.stat(full); } catch { return false; }
  let file = full;
  if (st.isDirectory()) {
    file = path.join(full, 'index.html');
    try { st = await fsp.stat(file); } catch { return false; }
  }
  const type = TYPES[path.extname(file).toLowerCase()] || 'application/octet-stream';
  const etag = `W/"${st.size.toString(16)}-${Math.floor(st.mtimeMs).toString(16)}"`;
  if (req.headers['if-none-match'] === etag) { res.writeHead(304); res.end(); return true; }
  res.writeHead(200, {
    'content-type': type, 'content-length': st.size, etag,
    'cache-control': type.startsWith('text/html') ? 'no-cache' : `public, max-age=${maxAge}`,
  });
  if (req.method === 'HEAD') { res.end(); return true; }
  createReadStream(file).pipe(res);
  return true;
}

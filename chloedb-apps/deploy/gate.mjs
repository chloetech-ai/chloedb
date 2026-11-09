// A gate in front of the studio, for a studio other people reach over the
// network (deploy/README.md). The studio has no login of its own, so the gate
// asks for one shared key: it comes once in the link (/?key=...), is kept in
// a cookie after that, and every request without it is refused. The studio
// itself listens on 127.0.0.1 only, behind the gate.
//
//   STRATA_APPS_GATE_KEY=<key> node deploy/gate.mjs [--port 8080] [--host 0.0.0.0] [--to http://127.0.0.1:7730]
//
// A script can send the key in an x-sa-access header instead.

import http from 'node:http';
import { createHash, timingSafeEqual } from 'node:crypto';

function arg(name, fallback) {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 && process.argv[i + 1] ? process.argv[i + 1] : fallback;
}

const KEY = process.env.STRATA_APPS_GATE_KEY || '';
if (KEY.length < 16) {
  console.error('deploy/gate.mjs: set STRATA_APPS_GATE_KEY to a key of 16 characters or more');
  process.exit(1);
}
const PORT = Number(arg('port', process.env.PORT || 8080));
const HOST = arg('host', '0.0.0.0');
const TO = new URL(arg('to', 'http://127.0.0.1:7730'));
const COOKIE = 'sa_access';
const DAYS = 30;
const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a);

const digest = (s) => createHash('sha256').update(String(s)).digest();
const WANT = digest(KEY);
const right = (s) => typeof s === 'string' && s.length > 0 && timingSafeEqual(digest(s), WANT);
const cookieOf = (req) => {
  const m = String(req.headers.cookie || '').match(/(?:^|;\s*)sa_access=([^;]+)/);
  return m ? decodeURIComponent(m[1]) : null;
};
// The cookie without ours, for the studio.
const otherCookies = (req) => String(req.headers.cookie || '').split(/;\s*/).filter((c) => c && !c.startsWith(`${COOKIE}=`)).join('; ');

// Pictures and files a view shows are loaded from inside its sandboxed frame,
// which sends no cookies. Files are named by ids nobody can guess, and map
// tiles are public, so these pass without the key (GET only).
const OPEN = [/^\/api\/tiles\/\d{1,2}\/\d{1,7}\/\d{1,7}\.png$/, /^\/api\/apps\/[a-z0-9]+\/files\/[a-z0-9]{6,20}$/];

const PAGE = `<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Private studio</title>
<style>body{font:16px/1.5 system-ui,sans-serif;margin:0;min-height:100vh;display:grid;place-items:center;background:#f4f5f7;color:#1d2433}
main{max-width:26rem;padding:2rem;background:#fff;border:1px solid #dfe2e8;border-radius:8px}h1{font-size:1.2rem;margin:0 0 .5rem}p{margin:0}
@media (prefers-color-scheme: dark){body{background:#14171c;color:#e6e8ec}main{background:#1c2027;border-color:#2c313a}}</style>
<main><h1>This studio is private</h1><p>Open it with the link you were given. The link holds a key; once you have opened it, this browser remembers it for ${DAYS} days.</p></main>`;

function refuse(req, res) {
  if (req.url.startsWith('/api/')) {
    res.writeHead(401, { 'content-type': 'application/json' });
    return res.end(JSON.stringify({ error: 'this studio needs its key: open the link you were given' }));
  }
  res.writeHead(401, { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store' });
  res.end(PAGE);
}

function forward(req, res, pathAndQuery) {
  const headers = { ...req.headers, 'x-forwarded-for': req.socket.remoteAddress || '' };
  delete headers['x-sa-access'];
  const cookies = otherCookies(req);
  if (cookies) headers.cookie = cookies; else delete headers.cookie;
  const up = http.request({ host: TO.hostname, port: TO.port, method: req.method, path: pathAndQuery, headers }, (r) => {
    res.writeHead(r.statusCode, r.headers);
    r.pipe(res);
  });
  up.on('error', (e) => {
    if (res.headersSent) return res.destroy();
    res.writeHead(502, { 'content-type': 'text/plain; charset=utf-8' });
    res.end(`The studio is not answering (${e.code || e.message}). It may be starting; try again in a minute.`);
  });
  // A stream (the studio's live feed) ends when the browser goes away.
  res.on('close', () => up.destroy());
  req.pipe(up);
}

const server = http.createServer((req, res) => {
  const url = new URL(req.url, 'http://gate');
  // For the platform's health checks: answers whether the studio does.
  if (url.pathname === '/healthz') {
    const probe = http.get({ host: TO.hostname, port: TO.port, path: '/api/status', timeout: 3000 }, (r) => {
      r.resume();
      res.writeHead(r.statusCode === 200 ? 200 : 503, { 'content-type': 'text/plain' });
      res.end(r.statusCode === 200 ? 'ok' : 'studio not ready');
    });
    probe.on('timeout', () => probe.destroy(new Error('timeout')));
    probe.on('error', () => { res.writeHead(503, { 'content-type': 'text/plain' }); res.end('studio not answering'); });
    return;
  }
  // The key in the link: kept in a cookie, then the same page without it.
  const given = url.searchParams.get('key');
  if (given !== null && req.method === 'GET') {
    if (!right(given)) { log(`wrong key from ${req.socket.remoteAddress}`); return refuse(req, res); }
    url.searchParams.delete('key');
    res.writeHead(302, {
      'set-cookie': `${COOKIE}=${encodeURIComponent(given)}; Path=/; HttpOnly; SameSite=Lax; Max-Age=${DAYS * 86400}`,
      location: url.pathname + url.search,
      'referrer-policy': 'no-referrer',
      'cache-control': 'no-store',
    });
    return res.end();
  }
  const open = req.method === 'GET' && OPEN.some((re) => re.test(url.pathname));
  if (!open && !right(cookieOf(req)) && !right(req.headers['x-sa-access'])) return refuse(req, res);
  forward(req, res, req.url);
});

server.listen(PORT, HOST, () => log(`gate on http://${HOST}:${PORT}/ in front of ${TO.origin}`));

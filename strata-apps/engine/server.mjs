// strata-apps: the engine's HTTP server and the studio it serves.
//
//   node engine/server.mjs [--port 7730] [--strata http://127.0.0.1:7720] [--data DIR]
//
// Without --strata it starts its own strata-server on DIR/apps.strata.

import http from 'node:http';
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { Strata, findServerBinary, startServer } from './strata.mjs';
import { Engine } from './engine.mjs';
import { forge, suggestFromUse, useSharedVersion, editScreen } from './forge.mjs';
import { MODEL } from './llm.mjs';
import { newId } from './store.mjs';
import { seedLibrary } from './library.mjs';
import { Services, SESSION_DAYS, MAX_FILE } from './services.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, '..');
const REPO = path.resolve(ROOT, '..');

function arg(name, fallback) {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 && process.argv[i + 1] ? process.argv[i + 1] : fallback;
}

const PORT = Number(arg('port', process.env.PORT || 7730));
const HOST = arg('host', process.env.HOST || '127.0.0.1');
const DATA = path.resolve(arg('data', process.env.STRATA_APPS_DATA || path.join(ROOT, 'data')));
const AUTOREPAIR = process.env.STRATA_APPS_AUTOREPAIR !== '0';
const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a);

let strataUrl = arg('strata', process.env.STRATA_URL || '');
let child = null;
if (!strataUrl) {
  const bin = findServerBinary(REPO);
  if (!bin) {
    console.error('No strata-server found. Build it (cd strata-server && cargo build --release), set STRATA_SERVER_BIN, or pass --strata URL.');
    process.exit(1);
  }
  const listen = arg('strata-listen', '127.0.0.1:7720');
  ({ child } = await startServer({ bin, db: path.join(DATA, 'apps.strata'), listen, log }));
  strataUrl = `http://${listen}`;
  const stop = () => { child?.kill('SIGTERM'); process.exit(0); };
  process.on('SIGINT', stop);
  process.on('SIGTERM', stop);
}
const db = new Strata(strataUrl, process.env.STRATA_TOKEN);
const engine = new Engine(db, { log });
await engine.init();
await seedLibrary(engine.store, process.env.STRATA_APPS_LIBRARY || path.join(ROOT, 'library'), log);
const services = new Services(engine, { filesDir: path.join(DATA, 'files'), log });
engine.services = services;
log(services.smtpUrl && services.smtpFrom ? `email goes out through ${new URL(services.smtpUrl).host}` : 'email is held in the outbox (set SMTP_URL and SMTP_FROM to send it)');

// ---- who is asking: a session cookie, the same for every app here

const COOKIE = 'sa_session';
function tokenOf(req) {
  const m = String(req.headers.cookie || '').match(/(?:^|;\s*)sa_session=([^;]+)/);
  return m ? decodeURIComponent(m[1]) : null;
}
function setSession(res, token) {
  res.setHeader('set-cookie', `${COOKIE}=${token ? encodeURIComponent(token) : ''}; Path=/; HttpOnly; SameSite=Lax; Max-Age=${token ? SESSION_DAYS * 86400 : 0}`);
}
// The person as this app sees them, or null.
async function userOf(req, appId) {
  const person = await services.personOf(tokenOf(req)).catch(() => null);
  return person ? services.user(appId, person) : null;
}

// ---- map tiles, fetched once and kept on disk (OpenStreetMap's policy:
// a real user agent, caching, and no heavy use)

const TILES = process.env.STRATA_APPS_TILES === 'off' ? null : process.env.STRATA_APPS_TILES || 'https://tile.openstreetmap.org/{z}/{x}/{y}.png';
const tileQueue = [];
let tileBusy = 0;
async function tile(z, x, y) {
  const file = path.join(DATA, 'tiles', String(z), String(x), `${y}.png`);
  try { return await readFile(file); } catch { /* not cached yet */ }
  if (!TILES) throw Object.assign(new Error('map tiles are switched off here (STRATA_APPS_TILES=off)'), { status: 503 });
  if (tileBusy >= 2) await new Promise((r) => tileQueue.push(r));
  tileBusy++;
  try {
    const url = TILES.replace('{z}', z).replace('{x}', x).replace('{y}', y);
    const res = await fetch(url, { headers: { 'user-agent': 'strata-apps/0.1 (prototype app engine on Strata)' }, signal: AbortSignal.timeout(10000) });
    if (!res.ok) throw Object.assign(new Error(`tile server answered ${res.status}`), { status: 502 });
    const bytes = Buffer.from(await res.arrayBuffer());
    await mkdir(path.dirname(file), { recursive: true });
    await writeFile(file, bytes);
    return bytes;
  } finally {
    tileBusy--;
    tileQueue.shift()?.();
  }
}

// File types shown in place; anything else (HTML and SVG included) is a download.
const INLINE = /^(image\/(png|jpeg|gif|webp|avif)|audio\/[a-z0-9.+-]+|video\/[a-z0-9.+-]+|application\/pdf|text\/plain)$/;
function sendFile(res, meta, bytes, download) {
  const inline = !download && INLINE.test(meta.type);
  const name = encodeURIComponent(meta.name).replace(/['()]/g, escape);
  res.writeHead(200, {
    'content-type': inline ? meta.type : 'application/octet-stream',
    'content-length': bytes.length,
    'content-disposition': `${inline ? 'inline' : 'attachment'}; filename*=UTF-8''${name}`,
    'content-security-policy': "sandbox; default-src 'none'; img-src 'self'; media-src 'self'; style-src 'unsafe-inline'",
    'x-content-type-options': 'nosniff',
    'cache-control': 'private, max-age=86400, immutable',
  });
  res.end(bytes);
}

async function rawBody(req, limit) {
  const chunks = [];
  let size = 0;
  for await (const c of req) {
    size += c.length;
    if (size > limit) throw Object.assign(new Error(`files are at most ${limit / 1024 / 1024} MB`), { status: 413 });
    chunks.push(c);
  }
  return Buffer.concat(chunks);
}

// ---- live updates: one stream per open studio page

const clients = new Set();
function broadcast(type, payload) {
  const line = `event: ${type}\ndata: ${JSON.stringify(payload)}\n\n`;
  for (const res of clients) res.write(line);
}
// Data changes come in bursts (a trigger's writes, a batch); send each app's at most every 150 ms.
const pending = new Map();
engine.on('data', ({ app, tables }) => {
  if (!pending.has(app)) {
    pending.set(app, new Set());
    setTimeout(() => { const t = pending.get(app); pending.delete(app); broadcast('data', { app, tables: [...t] }); }, 150);
  }
  for (const t of tables) pending.get(app).add(t);
});
engine.on('app', (p) => broadcast('app', p));
engine.on('suggestions', (p) => broadcast('suggestions', p));
engine.on('request', (p) => broadcast('request', p));

// ---- requests: one at a time per app

const queues = new Map();
const running = new Map(); // app -> {id, text, source, lines}
const repaired = new Set();

async function submit(appId, text, source = 'user', extra = {}) {
  const id = newId(10);
  const req = { id, app: appId, text: String(text).slice(0, 4000), source, status: 'queued', comp: 0, summary: '', log: '', error: '', ms: 0, cost: 0, created: new Date().toISOString() };
  await engine.store.putRequest(req);
  const prev = queues.get(appId) || Promise.resolve();
  const next = prev.then(() => runRequest(req, extra)).catch(() => {});
  queues.set(appId, next);
  return req;
}

async function runRequest(req, extra = {}) {
  const lines = [];
  const state = { id: req.id, text: req.text, source: req.source, lines };
  running.set(req.app, state);
  // A line sent with {replace: true} takes the place of the previous such
  // line (the model's running count), so the log does not fill with them.
  const progress = (line, { replace = false, preview = null } = {}) => {
    if (replace && lines.at(-1)?.replace) lines[lines.length - 1] = { t: Date.now(), line, replace };
    else { lines.push({ t: Date.now(), line, replace }); if (!replace) log(`[${req.app}] ${line}`); }
    broadcast('progress', { app: req.app, request: req.id, line, replace, source: req.source, text: req.text });
    if (preview) broadcast('preview', { app: req.app, request: req.id, ...preview });
  };
  await engine.store.putRequest({ ...req, status: 'running' });
  try {
    const out = req.source === 'component' ? await useSharedVersion(engine, req.app, extra.cap, progress) : await forge(engine, req.app, req, progress);
    await engine.store.putRequest({ ...req, status: 'done', comp: out.n, summary: out.summary, log: lines.map((l) => l.line).join('\n'), ms: out.ms, cost: out.cost });
    if (out.trace) await saveTrace(req, out.trace, lines, { status: 'done', n: out.n, cost: out.cost });
    broadcast('done', { app: req.app, request: req.id, n: out.n, summary: out.summary });
  } catch (e) {
    progress(`Failed: ${e.message}`);
    await engine.store.putRequest({ ...req, status: 'failed', error: e.message.slice(0, 2000), log: lines.map((l) => l.line).join('\n'), cost: e.cost || 0 });
    if (e.trace) await saveTrace(req, e.trace, lines, { status: 'failed', error: e.message.slice(0, 500), cost: e.cost || 0 });
    broadcast('failed', { app: req.app, request: req.id, error: e.message });
  } finally {
    running.delete(req.app);
  }
}

// A build's timeline, with the progress lines it showed (their times from the
// build's start), kept next to the request.
async function saveTrace(req, trace, lines, extra) {
  const shown = lines.map((l) => ({ t: Math.max(0, l.t - trace.t0), line: l.line }));
  await engine.store.putTrace(req.id, req.app, { ...trace, ...extra, text: req.text, source: req.source, log: shown }).catch((e) => log(`trace ${req.id}: ${e.message}`));
}

// A capability that fails at run time gets one repair, by the forge.
async function repair(appId, capId, message, where) {
  if (!AUTOREPAIR || repaired.has(capId)) return;
  repaired.add(capId);
  const view = await engine.store.resolve(appId).catch(() => null);
  const cap = view?.caps.find((c) => c.id === capId);
  if (!cap) return;
  broadcast('repair', { app: appId, cap: capId, message });
  await submit(appId, `The ${cap.kind} "${cap.title}" (${cap.slug}) fails (${where}): ${message}\nFind the cause and fix it, changing only what the fix needs.`, 'repair');
}
engine.on('error', ({ app, cap, message, where }) => { repair(app, cap, message, where).catch((e) => log(`repair: ${e.message}`)); });

// What people do feeds suggestions: after every 25 actions, at most every 3 minutes.
const activity = new Map();
async function noteActivity(appId) {
  const a = activity.get(appId) || { n: 0, last: 0, busy: false };
  a.n++;
  activity.set(appId, a);
  if (a.n < 25 || a.busy || Date.now() - a.last < 180000 || running.has(appId)) return;
  a.n = 0;
  a.busy = true;
  a.last = Date.now();
  try {
    const items = await suggestFromUse(engine, appId);
    log(`[${appId}] ${items.length} suggestions from use`);
    if (!engine.feed) broadcast('suggestions', { app: appId });
  } catch (e) { log(`suggest: ${e.message}`); } finally { a.busy = false; }
}

// ---- HTTP

const STATIC = {
  '/': ['web/index.html', 'text/html; charset=utf-8'],
  '/studio.js': ['web/studio.js', 'text/javascript; charset=utf-8'],
  '/studio.css': ['web/studio.css', 'text/css; charset=utf-8'],
  '/frame.js': ['web/frame.js', 'text/javascript; charset=utf-8'],
  '/screen.js': ['web/screen.js', 'text/javascript; charset=utf-8'],
  '/graph.js': ['web/graph.js', 'text/javascript; charset=utf-8'],
  '/kit.css': ['web/kit.css', 'text/css; charset=utf-8'],
};

async function body(req) {
  const chunks = [];
  let size = 0;
  for await (const c of req) {
    size += c.length;
    if (size > 2 * 1024 * 1024) throw Object.assign(new Error('request too large'), { status: 413 });
    chunks.push(c);
  }
  const text = Buffer.concat(chunks).toString('utf8');
  if (!text) return {};
  try { return JSON.parse(text); } catch { throw Object.assign(new Error('body is not JSON'), { status: 400 }); }
}

function send(res, status, data) {
  res.writeHead(status, { 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store' });
  res.end(JSON.stringify(data));
}

async function appState(appId) {
  const { store } = engine;
  const view = await store.resolve(appId);
  const tables = [];
  for (const t of view.tables) {
    tables.push({ id: t.id, name: t.spec.name, description: t.spec.description, columns: t.spec.columns, rows: await store.countRows(`app_${appId}_${t.spec.name}`) });
  }
  const cap = (c) => ({ id: c.id, slug: c.slug, version: Number(c.version), kind: c.kind, title: c.title, description: c.description, code: c.code, spec: c.spec, forked_from: c.forked_from || null, author: c.author, created: c.created });
  const run = running.get(appId);
  return {
    app: view.app,
    version: { n: view.comp.n, summary: view.comp.summary || '', request: view.comp.request || '' },
    views: view.views.map(cap).sort((a, b) => (a.spec.order ?? 50) - (b.spec.order ?? 50)),
    functions: view.functions.map(cap),
    ...(await (async () => {
      const newest = await store.shared();
      const users = await store.sharedUsers();
      const one = (c) => ({ ...cap(c), latest: newest.get(`${c.kind}/${c.slug}`)?.version || Number(c.version), users: (users.get(`${c.kind}/${c.slug}`) || []).length });
      return { components: view.components.map(one), helpers: view.helpers.map(one) };
    })()),
    tables,
    people: await services.people(appId).catch(() => []),
    outbox: await services.outbox(appId, 20).catch(() => []),
    suggestions: await store.suggestions(appId),
    versions: await store.compositions(appId),
    requests: await store.requests(appId, 30),
    running: run ? { id: run.id, text: run.text, source: run.source, lines: run.lines.map((l) => l.line) } : null,
  };
}

const routes = [
  ['GET', /^\/api\/status$/, async () => ({ model: MODEL, strata: strataUrl, feed: engine.feed, data: child ? DATA : null, autorepair: AUTOREPAIR })],
  ['GET', /^\/api\/apps$/, async () => ({ apps: await engine.store.listApps() })],
  ['POST', /^\/api\/apps$/, async (b) => {
    const description = String(b.description || '').trim();
    if (description.length < 5) throw Object.assign(new Error('describe the app in a sentence or more'), { status: 400 });
    const name = description.split(/[.\n]/)[0].slice(0, 40);
    const app = await engine.store.createApp({ name, description });
    const request = await submit(app, description, 'user');
    return { app, request: request.id };
  }],
  ['GET', /^\/api\/apps\/([a-z0-9]+)$/, async (b, [id]) => appState(id)],
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/traces\/([A-Za-z0-9_-]+)$/, async (b, [id, rid]) => {
    const trace = await engine.store.trace(rid);
    if (!trace) throw Object.assign(new Error('no timeline for this build (it ran before timelines were recorded)'), { status: 404 });
    return { trace };
  }],
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/traces$/, async (b, [id]) => ({ traces: (await engine.store.traces(id)).map(({ log, ...t }) => t) })],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/requests$/, async (b, [id]) => {
    const text = String(b.text || '').trim();
    if (!text) throw Object.assign(new Error('say what to change'), { status: 400 });
    const taken = b.suggestion ? await engine.store.setSuggestion(String(b.suggestion), 'taken') : null;
    // A newer shared component is taken without the model: its code is in Strata.
    if (taken?.source === 'component' && taken.cap) return { request: (await submit(id, text, 'component', { cap: taken.cap })).id };
    const source = ['user', 'suggestion', 'library'].includes(b.source) ? b.source : 'user';
    const r = await submit(id, text, source);
    return { request: r.id };
  }],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/sql$/, async (b, [id]) => ({ rows: await engine.sql(id, b.sql, b.params) })],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/insert$/, async (b, [id], url, cx) => { const row = await engine.insert(id, b.table, b.row || {}, { user: await cx.user(id) }); noteActivity(id); return { row }; }],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/update$/, async (b, [id], url, cx) => { const row = await engine.update(id, b.table, b.id, b.fields || {}, { user: await cx.user(id) }); noteActivity(id); return { row }; }],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/remove$/, async (b, [id], url, cx) => { const out = await engine.remove(id, b.table, b.id, { user: await cx.user(id) }); noteActivity(id); return out; }],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/call$/, async (b, [id], url, cx) => { const out = await engine.call(id, b.name, b.input, { user: await cx.user(id) }); noteActivity(id); return out; }],
  // People: one account for every app here; joining an app on first use.
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/me$/, async (b, [id], url, cx) => ({ user: await cx.user(id) })],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/signup$/, async (b, [id], url, cx) => { const out = await services.signUp(id, b); setSession(cx.res, out.token); return { user: out.user }; }],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/signin$/, async (b, [id], url, cx) => { const out = await services.signIn(id, b); setSession(cx.res, out.token); return { user: out.user }; }],
  ['POST', /^\/api\/signout$/, async (b, m, url, cx) => { await services.signOut(tokenOf(cx.req)); setSession(cx.res, null); return { ok: true }; }],
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/people$/, async (b, [id]) => ({ people: await services.people(id) })],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/files\/info$/, async (b, [id]) => ({ files: (await services.fileInfo(id, b.ids)).map((f) => ({ ...f, url: `/api/apps/${id}/files/${f.id}` })) })],
  // Email from a view needs someone signed in; functions send as the app.
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/email$/, async (b, [id], url, cx) => {
    const user = await cx.user(id);
    if (!user) throw Object.assign(new Error('email: sign in to send email from this app'), { status: 401 });
    return { email: await services.email(id, b, user.id) };
  }],
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/outbox$/, async (b, [id]) => ({ outbox: await services.outbox(id) })],
  ['GET', /^\/api\/geocode$/, async (b, m, url) => ({ places: await services.geocode(url.searchParams.get('q')) })],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/track$/, async (b, [id]) => {
    const kind = b.kind === 'open' ? 'open' : 'action';
    const detail = kind === 'open' ? String(b.detail || '') : `${String(b.action || '').slice(0, 60)}${b.detail ? ` ${String(b.detail).slice(0, 200)}` : ''}`;
    await engine.store.track(id, kind, String(b.cap || ''), detail);
    noteActivity(id);
    return { ok: true };
  }],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/error$/, async (b, [id]) => {
    const cap = String(b.cap || '');
    // An error from code the app no longer runs (a window still showing an
    // older version) is not sent to the forge.
    const live = await engine.store.resolve(id).catch(() => null);
    if (!live?.comp.caps.includes(cap)) return { ok: true, repairing: false, stale: true };
    const repairing = AUTOREPAIR && !repaired.has(cap);
    engine.reportError(id, cap, String(b.message || '').slice(0, 1500), 'in the browser');
    return { ok: true, repairing };
  }],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/suggest$/, async (b, [id]) => ({ suggestions: await suggestFromUse(engine, id) })],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/suggestions\/([a-z0-9]+)$/, async (b, [id, sid]) => {
    await engine.store.setSuggestion(sid, b.status === 'taken' ? 'taken' : 'dismissed');
    if (!engine.feed) broadcast('suggestions', { app: id });
    return { ok: true };
  }],
  // A person's own edit of a screen (from the studio): no model, no queue;
  // refused while a change is being built, which would not include it.
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/screens\/([a-z0-9-]+)$/, async (b, [id, slug]) => {
    if (running.has(id)) throw Object.assign(new Error('a change is being built: edit once it is live'), { status: 409 });
    const t0 = Date.now();
    const out = await editScreen(engine, id, slug, b);
    if (!out.unchanged) {
      const text = String(b.note || `Edited the screen ${slug} by hand`).slice(0, 300);
      await engine.store.putRequest({ id: newId(10), app: id, text, source: 'edit', status: 'done', comp: out.n, summary: text, log: '', error: '', ms: Date.now() - t0, cost: 0, created: new Date().toISOString() });
      broadcast('done', { app: id, request: null, n: out.n, summary: text });
    }
    return out;
  }],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/restore$/, async (b, [id]) => {
    const n = Number(b.n);
    const comp = await engine.store.composition(id, n);
    if (!comp.n) throw Object.assign(new Error(`no version ${b.n}`), { status: 404 });
    await engine.store.setHead(id, n);
    if (!engine.feed) broadcast('app', { app: id, head: n });
    return { head: n };
  }],
  ['GET', /^\/api\/library$/, async (b, m, url) => {
    const q = url.searchParams.get('q');
    const rows = q ? await engine.store.searchCapabilities(q, { limit: 40 }) : await engine.store.liveCapabilities();
    const apps = new Map((await engine.store.listApps()).map((a) => [a.id, a.name]));
    const users = await engine.store.sharedUsers();
    const isShared = (r) => r.kind === 'component' || r.kind === 'helper';
    return { capabilities: rows.map((r) => ({ ...r, app_name: isShared(r) ? 'shared' : r.app === 'lib' ? 'built-in starter' : apps.get(r.app) || r.app,
      used_by: isShared(r) ? (users.get(`${r.kind}/${r.slug}`) || []).map((u) => ({ app: u.app, name: u.name, version: u.version })) : undefined })) };
  }],
  // The graph of apps and pieces: all of it, or one app's part.
  ['GET', /^\/api\/graph$/, async (b, m, url) => ({ nodes: await engine.store.graph({ app: url.searchParams.get('app') || undefined }) })],
  ['GET', /^\/api\/graph\/node$/, async (b, m, url) => {
    const node = await engine.store.node(String(url.searchParams.get('id') || ''));
    if (!node) throw Object.assign(new Error('no such node'), { status: 404 });
    return node;
  }],
  ['GET', /^\/api\/capabilities\/(.+)$/, async (b, [id]) => {
    const [cap] = await engine.store.getCapabilities([decodeURIComponent(id)]);
    if (!cap) throw Object.assign(new Error('no such capability'), { status: 404 });
    return cap;
  }],
];

const server = http.createServer(async (req, res) => {
  const url = new URL(req.url, `http://${req.headers.host || 'localhost'}`);
  try {
    // An app on its own, without the studio around it (for phones and sharing).
    const page = req.method === 'GET' && /^\/a\/[a-z0-9]+\/?$/.test(url.pathname) ? STATIC['/'] : STATIC[url.pathname];
    if (req.method === 'GET' && page) {
      const [file, type] = page;
      const data = await readFile(path.join(ROOT, file));
      res.writeHead(200, { 'content-type': type, 'cache-control': 'no-cache' });
      return res.end(data);
    }
    let m;
    if (req.method === 'GET' && (m = url.pathname.match(/^\/api\/apps\/([a-z0-9]+)\/files\/([a-z0-9]{6,20})$/))) {
      const { meta, bytes } = await services.readFile(m[1], m[2]);
      return sendFile(res, meta, bytes, url.searchParams.has('download'));
    }
    if (req.method === 'GET' && (m = url.pathname.match(/^\/api\/tiles\/(\d{1,2})\/(\d{1,7})\/(\d{1,7})\.png$/))) {
      const [z, x, y] = m.slice(1).map(Number);
      if (z > 19 || x >= 2 ** z || y >= 2 ** z) return send(res, 404, { error: 'no such tile' });
      const bytes = await tile(z, x, y);
      res.writeHead(200, { 'content-type': 'image/png', 'content-length': bytes.length, 'cache-control': 'public, max-age=604800' });
      return res.end(bytes);
    }
    if (req.method === 'GET' && url.pathname === '/api/stream') {
      res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-store', connection: 'keep-alive' });
      res.write(`event: hello\ndata: ${JSON.stringify({ feed: engine.feed })}\n\n`);
      clients.add(res);
      const ping = setInterval(() => res.write(': ping\n\n'), 20000);
      req.on('close', () => { clearInterval(ping); clients.delete(res); });
      return;
    }
    // Only this page may call the API: a page on another site could otherwise
    // post to localhost.
    if (req.method === 'POST') {
      const origin = req.headers.origin;
      if (origin && origin !== `http://${req.headers.host}`) return send(res, 403, { error: 'cross-origin request refused' });
    }
    // An upload is the file's bytes as the body; its name and type in the query.
    if (req.method === 'POST' && (m = url.pathname.match(/^\/api\/apps\/([a-z0-9]+)\/files$/))) {
      const bytes = await rawBody(req, MAX_FILE);
      const user = await userOf(req, m[1]);
      const file = await services.saveFile(m[1], bytes, { name: url.searchParams.get('name'), type: url.searchParams.get('type') || req.headers['content-type'] }, user?.id);
      noteActivity(m[1]);
      return send(res, 200, { file: { ...file, url: `/api/apps/${m[1]}/files/${file.id}` } });
    }
    const cx = { req, res, user: (appId) => userOf(req, appId) };
    for (const [method, re, fn] of routes) {
      if (req.method !== method) continue;
      const mm = url.pathname.match(re);
      if (!mm) continue;
      const b = method === 'POST' ? await body(req) : {};
      return send(res, 200, await fn(b, mm.slice(1), url, cx));
    }
    send(res, 404, { error: 'not found' });
  } catch (e) {
    const passOn = e.status && (e.status < 500 || ((e.status === 502 || e.status === 503) && !e.message.startsWith('strata')));
    const status = passOn ? e.status : (/^(no |table |app\.sql|one statement|empty SQL|.* takes )/.test(e.message) ? 400 : (e.message.startsWith('strata') ? 400 : 500));
    if (status >= 500) log(`${req.method} ${url.pathname}: ${e.stack || e.message}`);
    send(res, status, { error: e.message });
  }
});

server.listen(PORT, HOST, () => {
  log(`strata-apps on http://${HOST}:${PORT}/ (Strata at ${strataUrl}, model ${MODEL})`);
});

// chloedb-apps: the engine's HTTP server and the studio it serves.
//
//   node engine/server.mjs [--port 7730] [--strata http://127.0.0.1:7720] [--data DIR]
//
// Without --strata it starts its own chloedb-server on DIR/apps.strata.

import { decide } from './access.mjs';
import http from 'node:http';
import { readFile, writeFile, mkdir, chmod, rm } from 'node:fs/promises';
import { randomBytes } from 'node:crypto';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { Strata, findServerBinary, startServer } from './strata.mjs';
import { Login } from './login.mjs';
import { callerOf, refusal, sees } from './callers.mjs';
import { useSelfCookie } from './checks.mjs';
import { Engine } from './engine.mjs';
import { forge, suggestFromUse, useSharedVersion, editScreen, SCREEN_MODEL, SPLIT, useSqlFunctions, useCluster } from './forge.mjs';
import { repairRequest, lookSuggestions } from './look.mjs';
import { readSqlLibrary, readSqlFile, install as installSqlFunctions } from './sqlfns.mjs';
import { Changes, joinAsks } from './changes.mjs';
import { useTableFunctions, useFunctionReads } from './sql.mjs';
import { MODEL, spend, useSpendFile, buildScope, checkProvider, withProvider } from './llm.mjs';
import { explainFailure } from './failures.mjs';
import { newId, shortName, firstBuildStatus } from './store.mjs';
import { seedLibrary } from './library.mjs';
import { appScript } from './source.mjs';
import { provenance } from './provenance.mjs';
import { STEP_DEFAULTS, expectedSteps } from './trace.mjs';
import { Publish, forVisitor, strangerOf } from './publish.mjs';
import { forVisitors } from './audience.mjs';
import { ask } from './ask.mjs';
import { Services, SESSION_DAYS, MAX_FILE } from './services.mjs';
import { Reach, ABILITIES, usesOf } from './reach.mjs';
import { Scheduler } from './schedule.mjs';
import { ensureDesigns, tuneDesign, tokenChoices } from './designs.mjs';
import { currencyOf } from './data.mjs';
import { Evolver, MODES } from './evolve.mjs';
import { Recommender, withHint } from './recommend.mjs';
import { ask as askModel } from './llm.mjs';
import { activity as activityOf } from './activity.mjs';
import { AppKeys } from './appkeys.mjs';
import { Comments } from './comments.mjs';
import { Presence } from './presence.mjs';
import { Builders, builderRefusal, keyOf as builderKeyOf, COOKIE as BUILDER_COOKIE } from './builders.mjs';

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
// What the model has cost here, against STRATA_APPS_SPEND_CAP_USD (llm.mjs).
if (spend.cap) await mkdir(DATA, { recursive: true }).then(() => useSpendFile(path.join(DATA, 'spend.json')));
const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a);

// The token of the chloedb-server the engine starts itself: DATA/strata.token,
// made on first start, readable by its owner only.
async function ownToken() {
  const file = path.join(DATA, 'strata.token');
  try { const t = (await readFile(file, 'utf8')).trim(); if (t) return t; } catch { /* made below */ }
  await mkdir(DATA, { recursive: true });
  await writeFile(file, `${randomBytes(24).toString('hex')}\n`, { mode: 0o600, flag: 'wx' }).catch(() => {});
  await chmod(file, 0o600).catch(() => {});
  return (await readFile(file, 'utf8')).trim();
}

let strataUrl = arg('strata', process.env.STRATA_URL || '');
let strataToken = process.env.STRATA_TOKEN;
let child = null;
if (!strataUrl) {
  const bin = findServerBinary(REPO);
  if (!bin) {
    console.error('No chloedb-server found. Build it (cd chloedb-server && cargo build --release), set STRATA_SERVER_BIN, or pass --strata URL.');
    process.exit(1);
  }
  const listen = arg('strata-listen', '127.0.0.1:7720');
  // The server of its own takes a token (kept in DATA/strata.token), so
  // that it makes each app a key of its own (appkeys.mjs) and is not open
  // to every process of the machine. (Over test shards: none, as before.)
  if (!Number(process.env.STRATA_APPS_TEST_SHARDS)) strataToken = await ownToken();
  ({ child } = await startServer({ bin, db: path.join(DATA, 'apps.strata'), listen, log, token: strataToken }));
  strataUrl = `http://${listen}`;
  const stop = () => { child?.kill('SIGTERM'); process.exit(0); };
  process.on('SIGINT', stop);
  process.on('SIGTERM', stop);
}
const db = new Strata(strataUrl, strataToken);
// A Strata of its own answers before startServer returns; one named by
// --strata may start after the engine does (a cluster's coordinators
// beside its pod): wait for it, up to STRATA_APPS_WAIT_S (default 300 s).
const strataHealth = await (async () => {
  const until = Date.now() + 1000 * (Number(process.env.STRATA_APPS_WAIT_S) || (child ? 0 : 300));
  for (let said = false; ; said = true) {
    try { return await db.health(); } catch (e) {
      if (Date.now() >= until) { console.error(`ChloeDB at ${strataUrl} does not answer: ${e.message}`); process.exit(1); }
      if (!said) log(`waiting for ChloeDB at ${strataUrl}: ${e.message}`);
      await new Promise((r) => setTimeout(r, 2000));
    }
  }
})();
// A cluster's coordinator walks no graphs: the engine walks its own, and
// tells the model that apps read their links a step at a time (walk.mjs).
const ON_CLUSTER = strataHealth.role === 'coordinator';
if (ON_CLUSTER) { useCluster(true); log(`ChloeDB at ${strataUrl} is a coordinator over ${strataHealth.shards?.length ?? '?'} shards`); }
const engine = new Engine(db, { log });
// Where the build opens the views it made (engine/checks.mjs).
engine.baseUrl = `http://127.0.0.1:${PORT}`;
await engine.init();
const LIBRARY_DIR = process.env.STRATA_APPS_LIBRARY || path.join(ROOT, 'library');
await seedLibrary(engine.store, LIBRARY_DIR, log);
// The library's SQL functions, defined in Strata (STRATA_APPS_SQL_FUNCTIONS=off
// leaves them out).
if (process.env.STRATA_APPS_SQL_FUNCTIONS !== 'off') {
  engine.sqlFunctions = await installSqlFunctions(engine.db, readSqlLibrary(path.join(LIBRARY_DIR, 'sql')), log);
  useSqlFunctions(engine.sqlFunctions);
  useTableFunctions(engine.sqlFunctions.filter((f) => f.returns === 'TABLE').map((f) => f.name));
  useFunctionReads(engine.sqlFunctions.map((f) => f.sql));
  if (engine.sqlFunctions.length) log(`${engine.sqlFunctions.length} SQL functions kept in ChloeDB`);
}
// The engine's own functions: the patterns in people's use, counted over
// sa_events (engine/signals.sql); without them the evolver counts in here.
{
  const sig = readSqlFile(path.join(ROOT, 'engine', 'signals.sql'));
  engine.signalSql = (await installSqlFunctions(engine.db, sig, log)).length === sig.length;
  if (!engine.signalSql) log('the patterns of use are counted in the engine, not in ChloeDB');
}
// What apps change next, learned across them (a graph in Strata, ranked by ppr).
engine.changes = new Changes(engine.store, log);
await engine.changes.init().catch((e) => { log(`changes: ${e.message}`); engine.changes = null; });
// Screens expand from the patterns stored in Strata (built in and added by apps).
await engine.store.usePatterns();
const services = new Services(engine, { filesDir: path.join(DATA, 'files'), log });
engine.services = services;
// Who builds each app beside its owner, and what their key lets them do.
const builders = new Builders(engine.db);
// Notes on an app's screens.
const comments = new Comments(engine.db);
// Who is working on each app right now.
const presence = new Presence();
// A model and the web for apps, each switched on per app (reach.mjs).
// Apps' secrets (API keys) are sealed with STRATA_APPS_SECRET_KEY, or with a
// key kept in DATA/secret.key, made on first start.
async function secretKey() {
  if (process.env.STRATA_APPS_SECRET_KEY) return process.env.STRATA_APPS_SECRET_KEY;
  const file = path.join(DATA, 'secret.key');
  try { return (await readFile(file, 'utf8')).trim(); } catch { /* made below */ }
  await mkdir(DATA, { recursive: true });
  const key = randomBytes(32).toString('base64');
  await writeFile(file, `${key}\n`, { mode: 0o600, flag: 'wx' }).catch(async () => {});
  await chmod(file, 0o600).catch(() => {});
  log(`apps' secrets are sealed with a new key in ${file} (set STRATA_APPS_SECRET_KEY to keep it elsewhere)`);
  return (await readFile(file, 'utf8')).trim();
}
const reach = new Reach({ store: engine.store, log, secretKey: await secretKey() });
engine.reach = reach;
// Each app's own key, which the server holds to the app's tables
// (appkeys.mjs); the engine trades an admin credential for a key of its own.
const appKeys = new AppKeys({ db, store: engine.store, box: reach.box, log, ownServer: !!child });
await appKeys.init();
engine.keys = appKeys;
// Rules and functions that run on a clock (schedule.mjs), once their app's
// owner switched "Schedules" on.
const scheduler = new Scheduler({ engine, reach, log });
if (process.env.STRATA_APPS_SCHEDULES !== 'off') scheduler.start();
// Beside a cluster, who may use the studio (login.mjs): chloedb-server with
// its app token, and browsers it hands a login to.
// An agent's MCP server (mcp.mjs) signs in with the agent token:
// STRATA_APPS_AGENT_TOKEN, or DATA/agent.token, made on first start and
// readable by its owner only (audit 4, X-G7).
async function agentToken() {
  if (process.env.STRATA_APPS_AGENT_TOKEN) return process.env.STRATA_APPS_AGENT_TOKEN.trim();
  const file = path.join(DATA, 'agent.token');
  try { const t = (await readFile(file, 'utf8')).trim(); if (t) return t; } catch { /* made below */ }
  await mkdir(DATA, { recursive: true });
  await writeFile(file, `${randomBytes(32).toString('base64url')}\n`, { mode: 0o600, flag: 'wx' }).catch(() => {});
  await chmod(file, 0o600).catch(() => {});
  return (await readFile(file, 'utf8').catch(() => '')).trim() || null;
}
const login = new Login({ store: engine.store, appToken: process.env.STRATA_APP_TOKEN, agentToken: await agentToken(), publicUrl: process.env.STRATA_APPS_PUBLIC_URL, log });
useSelfCookie(login.selfCookie);
if (login.on) log(`the studio needs a login, handed over by the command center${login.publicUrl ? `; it is reached at ${login.publicUrl}` : ' (set STRATA_APPS_PUBLIC_URL to say where browsers reach it)'}`);
// Apps open to the public (publish.mjs): /a/<id>, and <name>.<domain> with a domain.
const publish = new Publish({ store: engine.store, domain: process.env.STRATA_APPS_DOMAIN, scheme: process.env.STRATA_APPS_DOMAIN_SCHEME, log });
await publish.load(true);
if (publish.domain) log(`published apps answer at <name>.${publish.domain}`);
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
// Who writes a note: a builder by their key, else the studio's owner, who
// may give the name to show.
function authorOf(cx, b) {
  if (cx.builder) return { id: cx.builder.id, name: cx.builder.name || cx.builder.email || '' };
  return { id: 'owner', name: String(b?.authorName || '').trim().slice(0, 80) };
}
function setBuilderCookie(res, key) {
  res.setHeader('set-cookie', `${BUILDER_COOKIE}=${key}; Path=/; HttpOnly; SameSite=Lax; Max-Age=${365 * 86400}`);
}
// Where people reach this engine, for links handed out (invitations).
function baseOf(req) {
  if (process.env.STRATA_APPS_PUBLIC_URL) return process.env.STRATA_APPS_PUBLIC_URL;
  const host = String(req.headers.host || 'localhost').replace(/[^A-Za-z0-9.:[\]-]/g, '');
  return `http://${host}`;
}
// Someone not signed in is told apart by a cookie of their own, so what
// people do can be counted per person.
const VISITOR = 'sa_visitor';
function visitorOf(req) {
  const m = String(req.headers.cookie || '').match(/(?:^|;\s*)sa_visitor=([a-z0-9]{8,20})/);
  return m ? m[1] : null;
}
// The person as this app sees them, or null. A visitor of a published app
// (join false) is someone there only once they signed up or in on it: a
// session from another app on the same address does not make them a member.
async function userOf(req, appId, { join = true } = {}) {
  const person = await services.personOf(tokenOf(req)).catch(() => null);
  return person ? services.user(appId, person, { join }) : null;
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
    const res = await fetch(url, { headers: { 'user-agent': 'chloedb-apps/0.1 (prototype app engine on ChloeDB)' }, signal: AbortSignal.timeout(10000) });
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

const clients = new Map(); // the response -> null, or {app} for the public's
// The same events kept for a while, numbered, for a page that cannot hold a
// stream open: the command center reaches the engine through chloedb-server,
// which forwards whole answers, so it asks GET /api/events?after=N and the
// answer waits (up to ~20 s) for the next event.
const EVENTS_KEPT = 500;
const events = [];
let eventSeq = 0;
const eventWaiters = new Set();
// The public's streams get one app's changes to its data, look and version.
const PUBLIC_EVENTS = new Set(['data', 'app', 'design']);
function broadcast(type, payload) {
  const line = `event: ${type}\ndata: ${JSON.stringify(payload)}\n\n`;
  for (const [res, only] of clients) if (!only || (payload?.app === only.app && (only.builder || PUBLIC_EVENTS.has(type)))) res.write(line);
  events.push({ seq: ++eventSeq, type, data: payload });
  if (events.length > EVENTS_KEPT) events.splice(0, events.length - EVENTS_KEPT);
  for (const w of eventWaiters) w();
}
function eventsAfter(after, app) {
  const from = after > eventSeq ? 0 : after; // the engine restarted: start again
  return events.filter((e) => e.seq > from && (!app || !e.data || !e.data.app || e.data.app === app));
}
async function waitEvents(after, app, ms) {
  let list = eventsAfter(after, app);
  if (!list.length && ms > 0) {
    await new Promise((ok) => {
      const done = () => { clearTimeout(t); eventWaiters.delete(check); ok(); };
      const check = () => { if (eventsAfter(after, app).length) done(); };
      const t = setTimeout(done, ms);
      eventWaiters.add(check);
    });
    list = eventsAfter(after, app);
  }
  return { seq: eventSeq, feed: engine.feed, events: list };
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

// What a person asks while a change is being built waits as one request:
// the asks (and suggestions taken) that come before it starts join it, and
// are built together as one design rather than one change on top of another.
const waiting = new Map(); // app -> {req, asks}
async function submit(appId, text, source = 'user', extra = {}) {
  const joins = ['user', 'suggestion'].includes(source) && !extra.cap;
  const w = joins ? waiting.get(appId) : null;
  if (w) {
    w.asks.push(String(text));
    w.req.text = joinAsks(w.asks);
    await engine.store.putRequest(w.req);
    return w.req;
  }
  const id = newId(10);
  const req = { id, app: appId, text: String(text).slice(0, 4000), source, status: 'queued', comp: 0, summary: '', log: '', error: '', ms: 0, cost: 0, created: new Date().toISOString() };
  await engine.store.putRequest(req);
  const prev = queues.get(appId) || Promise.resolve();
  if (joins && running.has(appId)) waiting.set(appId, { req, asks: [String(text)] });
  const next = prev.then(() => { if (waiting.get(appId)?.req === req) waiting.delete(appId); return runRequest(req, extra); }).catch(() => {});
  queues.set(appId, next);
  return req;
}

// How long each step of a build usually takes, first versions apart from
// changes: medians of the last builds' timelines (sa_traces), so the studio's
// progress bar moves at the pace builds here really go.
let expected = structuredClone(STEP_DEFAULTS);
async function learnExpected() {
  let rows = [];
  try { rows = await engine.db.query('SELECT trace FROM sa_traces ORDER BY created DESC LIMIT 80'); } catch { return; }
  const traces = [];
  for (const r of rows) { try { traces.push(JSON.parse(r.trace)); } catch { /* a broken timeline counts for nothing */ } }
  expected = expectedSteps(traces);
}
learnExpected().catch(() => {});

async function runRequest(req, extra = {}) {
  const lines = [];
  const first = !Number((await engine.store.getApp(req.app).catch(() => null))?.head);
  // The build's steps as the studio draws them (read, plan, write, store),
  // with when each began, and the pieces being written at once.
  const split = SPLIT === 'always' || (SPLIT === 'first' && first);
  // stop: aborts every model call of this build (POST .../stop).
  const state = { id: req.id, text: req.text, source: req.source, lines, t0: Date.now(), first, split, steps: [], pieces: [], expect: expected[first ? 'first' : 'change'], stop: new AbortController() };
  running.set(req.app, state);
  // A line sent with {replace: true} takes the place of the previous such
  // line (the model's running count), so the log does not fill with them.
  const progress = (line, { replace = false, preview = null, step = null, pieces = null } = {}) => {
    if (replace && lines.at(-1)?.replace) lines[lines.length - 1] = { t: Date.now(), line, replace };
    else { lines.push({ t: Date.now(), line, replace }); if (!replace) log(`[${req.app}] ${line}`); }
    if (step && state.steps.at(-1)?.key !== step) state.steps.push({ key: step, t: Date.now() - state.t0 });
    if (pieces) state.pieces = pieces;
    broadcast('progress', { app: req.app, request: req.id, line, replace, source: req.source, text: req.text, ...buildState(state) });
    if (preview) broadcast('preview', { app: req.app, request: req.id, ...preview });
  };
  await engine.store.putRequest({ ...req, status: 'running' });
  try {
    // A build that names its provider (codex, agy) has its model calls go there.
    if (extra.provider) log(`[${req.app}] ${req.id} built with the ${extra.provider} provider`);
    const out = await buildScope.run({ signal: state.stop.signal }, () => (req.source === 'component' ? useSharedVersion(engine, req.app, extra.cap, progress)
      : withProvider(extra.provider, () => forge(engine, req.app, extra.candidate ? { ...req, candidate: true } : req, progress))));
    if (state.stop.signal.aborted) throw Object.assign(new Error('stopped by its owner'), { stopped: true });
    await engine.store.putRequest({ ...req, status: 'done', comp: out.n, summary: out.summary, log: lines.map((l) => l.line).join('\n'), ms: out.ms, cost: out.cost });
    if (out.trace) { await saveTrace(req, out.trace, lines, { status: 'done', n: out.n, cost: out.cost }); learnExpected().catch(() => {}); }
    if (extra.candidate) await evolver.built(extra.candidate, out).catch((e) => log(`candidate ${extra.candidate}: ${e.message}`));
    broadcast('done', { app: req.app, request: req.id, n: out.n, summary: out.summary, candidate: extra.candidate || null });
    if (!extra.candidate) recommender.soon(req.app, { delay: 20000, reason: 'build', force: true });
    engine.changes?.soon();
  } catch (err) {
    // Stopped by its owner: whatever the forge made of the stop, it is a stop.
    const stopped = state.stop.signal.aborted;
    const e = stopped ? Object.assign(new Error('stopped by its owner'), { cost: err.cost, trace: err.trace }) : err;
    const status = stopped ? 'stopped' : 'failed';
    progress(stopped ? 'Stopped' : `Failed: ${e.message}`);
    await engine.store.putRequest({ ...req, status, error: e.message.slice(0, 2000), log: lines.map((l) => l.line).join('\n'), cost: e.cost || 0 });
    if (e.trace) await saveTrace(req, e.trace, lines, { status, error: e.message.slice(0, 500), cost: e.cost || 0 });
    if (extra.candidate) await evolver.failed(extra.candidate, e.message).catch(() => {});
    broadcast('failed', { app: req.app, request: req.id, error: e.message, stopped, said: explainFailure(e.message, { stopped }), candidate: extra.candidate || null });
  } finally {
    running.delete(req.app);
    // A first build that gave no art direction: designs from the description.
    directions.get(req.app)?.give(null);
  }
}

// What the studio needs to draw a build's progress: its steps so far, the
// pieces being written, how long it has run and how long its steps usually take.
const buildState = (st) => ({ first: st.first, split: st.split, steps: st.steps, pieces: st.pieces, elapsed: Date.now() - st.t0, expect: st.expect });

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

// The drawn-page check of each live version (look.mjs): what it found is kept
// per app for the studio, and what stands in the way of the job goes to the
// repair as one request; not after a fix it asked for (one round), and at
// most once in ten minutes per app.
const drawnOf = new Map();
const lookedAt = new Map();
engine.on('drawn', (d) => {
  const high = d.found.filter((f) => f.severity === 'high' && f.by !== 'look');
  drawnOf.set(d.app, { n: d.n, at: new Date().toISOString(), found: d.found, views: d.views, cost: d.cost, looked: d.looked, grade: d.grade ?? null });
  const said = d.found.length ? `${d.found.length} problem${d.found.length === 1 ? '' : 's'}, ${high.length} in the way` : 'nothing in the way';
  const look = d.looked ? ` (look $${(d.cost || 0).toFixed(3)})` : d.lookError ? ` (no look: ${d.lookError.slice(0, 120)})` : '';
  log(`[${d.app}] ${d.static ? `checked version ${d.n} without a browser` : `drew version ${d.n} on a phone and a computer`}: ${said}${d.grade ? `, graded ${d.grade} of 5` : ''}${look}`);
  const text = repairRequest(d.found);
  // A newer change already being built is drawn and checked in turn.
  const fixing = !!text && AUTOREPAIR && d.source !== 'look' && !running.has(d.app) && Date.now() - (lookedAt.get(d.app) || 0) > 600000;
  // The reviewer's opinions replace the ones it gave an earlier version.
  const ideas = lookSuggestions(d.found);
  broadcast('drawn', { app: d.app, n: d.n, found: d.found.length, high: high.length, fixing, grade: d.grade ?? null, suggested: ideas.length });
  if (d.looked) {
    (async () => {
      for (const s of await engine.store.suggestions(d.app)) if (s.source === 'look') await engine.store.setSuggestion(s.id, 'replaced');
      await engine.store.putSuggestions(d.app, ideas);
      if (!engine.feed) broadcast('suggestions', { app: d.app });
    })().catch((e) => log(`look suggestions ${d.app}: ${e.message}`));
  }
  if (fixing) {
    lookedAt.set(d.app, Date.now());
    submit(d.app, text, 'look').catch((e) => log(`look ${d.app}: ${e.message}`));
  }
});

// What people do makes the app evolve (evolve.mjs): the engine counts
// patterns in it, suggests what would answer a new one, and builds the
// strongest as a version to try. It looks after every 25 actions, at most
// every 3 minutes, and calls the model only when a new pattern shows.
const evolver = new Evolver({ engine, submit, broadcast, log, propose: suggestFromUse });
// The next edit, recommended from use and, before there is any, from what
// the app is (recommend.mjs): in the background, never during a build.
const recommender = new Recommender({ engine, ask: askModel, signalsOf: (a, v, e) => evolver.signalsOf(a, v, e), busy: (a) => running.has(a), log,
  emit: (k, p) => { if (!engine.feed) broadcast(k, p); } });
const activity = new Map();
async function noteActivity(appId) {
  const a = activity.get(appId) || { n: 0, last: 0, busy: false };
  a.n++;
  activity.set(appId, a);
  recommender.soon(appId, { reason: 'use' });
  if (a.n < 25 || a.busy || Date.now() - a.last < 180000 || running.has(appId)) return;
  a.n = 0;
  a.busy = true;
  a.last = Date.now();
  try {
    const out = await evolver.tick(appId);
    if (out.suggestions?.length && !engine.feed) broadcast('suggestions', { app: appId });
  } catch (e) { log(`evolve: ${e.message}`); } finally { a.busy = false; }
}

// ---- HTTP

const STATIC = {
  '/': ['web/index.html', 'text/html; charset=utf-8'],
  '/studio.js': ['web/studio.js', 'text/javascript; charset=utf-8'],
  '/studio.css': ['web/studio.css', 'text/css; charset=utf-8'],
  '/frame.js': ['web/frame.js', 'text/javascript; charset=utf-8'],
  '/screen.js': ['web/screen.js', 'text/javascript; charset=utf-8'],
  '/front.js': ['web/front.js', 'text/javascript; charset=utf-8'],
  '/front.css': ['web/front.css', 'text/css; charset=utf-8'],
  '/graph.js': ['web/graph.js', 'text/javascript; charset=utf-8'],
  '/kit.css': ['web/kit.css', 'text/css; charset=utf-8'],
  '/design.js': ['web/design.js', 'text/javascript; charset=utf-8'],
  '/policy.js': ['web/policy.js', 'text/javascript; charset=utf-8'],
  '/activity.js': ['web/activity.js', 'text/javascript; charset=utf-8'],
  '/activity.css': ['web/activity.css', 'text/css; charset=utf-8'],
  // The app creator, the engine's page in a cluster's command center (Apps -> App creator).
  '/cc/view.js': ['web/cc-view.js', 'text/javascript; charset=utf-8'],
};

// What chloedb-server shows of the engine (its apps, GET /v1/apps).
const MANIFEST = {
  name: 'apps',
  title: 'App creator',
  description: 'Apps built from a description: written by a model, checked, kept in this cluster as rows, and changed by asking',
  icon_svg: '<rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><path d="M17.5 14v7M14 17.5h7"/>',
  view: 'cc/view.js',
  // It refuses what x-chloedb-scope and x-chloedb-tables do not allow
  // (callers.mjs): chloedb-server forwards it keys limited to tables.
  checks_callers: true,
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

// The hosted payment page: the amount, what it is for, and in test mode a
// plain statement that no card is charged and none is asked for.
const html = (x) => String(x ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
function payPage(p, shop, { token = '', problem = '', provider = '', locale = null } = {}) {
  let amount = `${p.amount} ${p.currency}`;
  // the amount as the app writes money (its locale), else as in Britain
  try { amount = Number(p.amount).toLocaleString(locale || 'en-GB', { style: 'currency', currency: p.currency }); } catch { /* as it is */ }
  const paid = p.status === 'paid' || p.status === 'paid_test';
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Pay ${html(shop)}</title>
<style>:root{color-scheme:light dark}body{margin:0;font:16px/1.5 -apple-system,system-ui,sans-serif;background:#f4f4f6;color:#16161a;display:grid;place-items:center;min-height:100vh;padding:16px;box-sizing:border-box}
.card{background:#fff;border-radius:18px;box-shadow:0 20px 60px -30px rgba(0,0,0,.35);padding:28px;max-width:420px;width:100%}h1{font-size:15px;margin:0;color:#666;font-weight:600}
.amt{font-size:40px;font-weight:800;letter-spacing:-.02em;margin:6px 0 4px}.what{color:#444;margin:0 0 18px}button{width:100%;min-height:52px;border:0;border-radius:14px;background:#16161a;color:#fff;font:inherit;font-weight:700;font-size:17px;cursor:pointer}
.test{margin:16px 0 0;padding:12px 14px;border-radius:12px;background:#fff7e0;color:#6b4a00;font-size:14px}.ok{font-size:20px;font-weight:700;color:#18794e;margin:8px 0}.lock{color:#777;font-size:13px;margin-top:14px}.bad{color:#b42318;font-weight:600}
@media (prefers-color-scheme:dark){body{background:#0e0e11;color:#eee}.card{background:#1a1a1f}.what{color:#bbb}button{background:#fff;color:#111}.test{background:#3a2e0a;color:#ffd98a}}</style></head>
<body><main class="card"><h1>${html(shop)}</h1><div class="amt">${html(amount)}</div><p class="what">${html(p.description || 'Your order')}</p>
${problem ? `<p class="bad" role="alert">${html(problem)}</p>` : ''}
${paid ? `<p class="ok" role="status">${p.status === 'paid_test' ? 'Paid in test mode. No money moved.' : 'Paid. Thank you.'}</p><p class="what">You can close this page and go back to ${html(shop)}.</p>`
    : p.mode === 'live' ? `<p class="what">The card payment is taken and confirmed by ${html(provider || 'the card provider')}. This page does not mark it paid.</p>`
    : token ? `<form method="post"><input type="hidden" name="t" value="${html(token)}"><button type="submit">Pay ${html(amount)} by card</button></form>`
    : `<p class="bad" role="alert">This payment link is not valid. Go back to ${html(shop)} and pay from your order.</p>`}
${p.mode === 'test' ? `<p class="test"><b>Test mode.</b> No card is charged and no card details are asked for here. The shop's owner connects a card payment provider to take real payments.</p>` : ''}
<p class="lock">A secure payment page. ${html(shop)} never sees your card details.</p></main></body></html>`;
}

function send(res, status, data) {
  res.writeHead(status, { 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store' });
  res.end(JSON.stringify(data));
}

function designsOf(appId, fresh = false, direction = null) {
  // A new app's designs wait for its art direction (the first plan writes it);
  // an ask in the meantime gets them when they are made.
  const waiting = !fresh && !direction && directions.get(appId);
  if (waiting) return waiting.made;
  return ensureDesigns(engine.store, appId, { fresh, direction, onMade: (saved, out) => {
    console.log(`designs ${appId}: ${saved.designs.map((d) => `${d.name} (${d.look})`).join(', ')} (${out.source}${out.directed ? ', from the art direction' : ''}, ${(out.ms / 1000).toFixed(1)} s${out.error ? `; ${out.error.slice(0, 120)}` : ''})`);
    broadcast('design', { app: appId, chosen: saved.chosen });
  } });
}
// New apps waiting for their art direction: the first plan gives it
// (forge.mjs calls engine.onDirection once its checks pass); a first build
// that ends without one has its designs made from the description.
const directions = new Map();
function awaitDirection(appId, provider = null) {
  let give;
  const given = new Promise((ok) => { give = ok; });
  const made = given.then((direction) => withProvider(provider, () => designsOf(appId, false, direction)));
  made.catch((e) => console.error(`designs ${appId}: ${e.message}`));
  // Kept until they are made: the art direction comes early (as the plan
  // streams in) and again once the build is stored, and a later hook waits
  // on the same designs instead of making them twice.
  made.finally(() => directions.delete(appId)).catch(() => {});
  directions.set(appId, { give, made });
}
engine.onDirection = (appId, direction) => {
  const w = directions.get(appId);
  if (!w) return null;
  w.give(direction);
  return w.made;
};

// A members' screen carries the example rows the first build kept for it
// (gate.sample, forge.mjs gateSamples), so a visitor sees it drawn with
// them (web/screen.js). Answers the code a page gets for a capability.
async function samplesFor(appId) {
  let samples = {};
  try { samples = JSON.parse((await engine.store.setting(appId, 'samples').catch(() => null)) || '{}') || {}; } catch { samples = {}; }
  return (c) => {
    if (c.kind !== 'view' || !c.spec?.screen || !samples[c.slug] || typeof c.code !== 'string' || !c.code.includes('"gate"')) return c.code;
    try { const sc = JSON.parse(c.code); if (!sc.gate) return c.code; sc.gate = { ...sc.gate, sample: samples[c.slug] }; return JSON.stringify(sc, null, 1); } catch { return c.code; }
  };
}

async function appState(appId) {
  const { store } = engine;
  const view = await engine.view(appId);
  const head = Number(view.app.head);
  const evolve = await evolver.state(appId, view).catch((e) => ({ mode: 'off', signals: [], candidates: [], error: e.message }));
  recommender.soon(appId, { reason: 'open' });
  // A candidate's own version is listed only when it went live as built.
  const unkept = new Set(evolve.candidates.filter((c) => c.n && c.kept !== c.n).map((c) => c.n));
  const tables = [];
  for (const t of view.tables) {
    tables.push({ id: t.id, name: t.spec.name, description: t.spec.description, columns: t.spec.columns, rows: await store.countRows(`app_${appId}_${t.spec.name}`) });
  }
  const withSample = await samplesFor(appId);
  const cap = (c) => ({ id: c.id, slug: c.slug, version: Number(c.version), kind: c.kind, title: c.title, description: c.description, code: withSample(c), sql: c.sql, spec: c.spec, forked_from: c.forked_from || null, author: c.author, created: c.created });
  const run = running.get(appId);
  return {
    app: { ...view.app, name: nameOf(view.app), currency: (await store.setting(appId, 'currency').catch(() => null)) || currencyOf([], view.app?.description), locale: (await store.setting(appId, 'locale').catch(() => null)) || null },
    design: await store.designs(appId),
    version: { n: view.comp.n, summary: view.comp.summary || '', request: view.comp.request || '', trying: view.comp.n !== head ? view.comp.n : null, live: head },
    evolve,
    views: view.views.map(cap).sort((a, b) => (a.spec.order ?? 50) - (b.spec.order ?? 50)),
    functions: view.functions.map(cap),
    ...(await (async () => {
      const newest = await store.shared();
      const users = await store.sharedUsers();
      const one = (c) => ({ ...cap(c), latest: newest.get(`${c.kind}/${c.slug}`)?.version || Number(c.version), users: (users.get(`${c.kind}/${c.slug}`) || []).length });
      return { components: view.components.map(one), helpers: view.helpers.map(one) };
    })()),
    tables,
    labels: labelsOf(view),
    people: await services.people(appId).catch(() => []),
    outbox: await services.outbox(appId, 20).catch(() => []),
    suggestions: await store.suggestions(appId),
    // (versions built to try and not kept are listed with their candidates)
    versions: (await store.compositions(appId)).filter((c) => !unkept.has(Number(c.n)) || Number(c.n) === head),
    // A failed or stopped request says why in a plain sentence (said), its error kept for the details.
    requests: (await store.requests(appId, 30)).map((r) => (r.status === 'failed' || r.status === 'stopped' ? { ...r, said: explainFailure(r.error, { stopped: r.status === 'stopped' }) } : r)),
    running: run ? { id: run.id, text: run.text, source: run.source, lines: run.lines.map((l) => l.line), ...buildState(run) } : null,
    drawn: drawnOf.get(appId) || null,
    reach: await reachOf(appId, view),
    // Where its code came from: CREATE statements, the agent's own code, the library (provenance.mjs).
    made: provenance(view, appId),
    publish: publish.of(appId),
    // What its builds cost, all of them.
    cost: (await store.costs(appId)).get(appId) || { usd: 0, builds: 0 },
  };
}

// The options of app.ask a visitor may pass: the answer's shape only, never
// instructions of their own, the web or a fresh (uncached) call.
const publicAsk = (o = {}) => ({ ...(o.json ? { json: true } : {}), ...(o.kept ? { kept: true } : {}) });

// A published app's state for the public: what its page needs to run its
// screens (their code, its design, the live version), and nothing of how it
// is made (requests, versions, suggestions), its people or its outbox.
// Each table's naming column (name, title, label, else its first text
// column): the frame shows a row's name wherever its id would show
// (web/frame.js), so a reference reads "Ava", never "kid-1".
const LABEL_COLS = ['name', 'title', 'label', 'full_name', 'display_name', 'first_name', 'subject', 'company'];
function labelsOf(view) {
  const out = [];
  for (const t of view.tables || []) {
    const cols = (t.spec?.columns || []).filter((c) => c && c.name);
    const col = LABEL_COLS.find((n) => cols.some((c) => c.name === n)) || cols.find((c) => c.type === 'text')?.name;
    if (col && t.spec?.name && !t.spec.links) out.push({ table: t.spec.name, col });
  }
  return out;
}

// Whether someone of this role may read a table at all (a page reads names
// only from the tables its visitor may: no refusals for its labels).
function mayRead(view, table, role) {
  const t = view.tables.find((x) => x.spec?.name === table);
  if (!t) return false;
  try { decide(t.spec, 'read', { role: role || 'visitor', user: role ? { id: 'someone' } : null, company: role ? 'some' : null }); return true; } catch { return false; }
}

// An app's name as shown. One with no version yet that is still called by
// the first 40 characters of its description (as apps were named before
// shortName) is shown by a short name instead.
const nameOf = (a) => (!Number(a.head) && a.name && String(a.description || '').startsWith(String(a.name).trim()) && String(a.name).trim().length >= 30 ? shortName(a.description) : a.name);

async function publicState(appId, access, role = null) {
  const view = await engine.view(appId);
  const head = Number(view.app.head);
  const withSample = await samplesFor(appId);
  const cap = (c) => ({ id: c.id, slug: c.slug, version: Number(c.version), kind: c.kind, title: c.title, code: withSample(c), spec: c.spec });
  const reachNow = await reachOf(appId, view).catch(() => null);
  return {
    public: true,
    // What visitors may do with its data (publish.mjs): the page offers no
    // write a visitor who may only look would be refused.
    access: access || 'look',
    app: { id: appId, name: view.app.name, currency: (await engine.store.setting(appId, 'currency').catch(() => null)) || currencyOf([], view.app?.description), locale: (await engine.store.setting(appId, 'locale').catch(() => null)) || null },
    design: await engine.store.designs(appId),
    version: { n: view.comp.n, summary: '', request: '', trying: null, live: head },
    evolve: { mode: 'off', signals: [], candidates: [] },
    // A visitor never gets the owner's views (engine/audience.mjs).
    // ...but its owner and admins, signed in on its own page, get theirs.
    views: (role === 'owner' || role === 'admin' ? view.views : forVisitors(view.views)).map(cap).sort((a, b) => (a.spec.order ?? 50) - (b.spec.order ?? 50)),
    functions: view.functions.map((c) => ({ id: c.id, slug: c.slug, kind: c.kind, title: c.title, callers: c.spec?.callers || null })),
    components: view.components.map(cap),
    helpers: view.helpers.map(cap),
    tables: [], labels: labelsOf(view).filter((l) => mayRead(view, l.table, role)), people: [], outbox: [], suggestions: [], versions: [], requests: [], running: null, drawn: null,
    reach: { on: { fetch: !!reachNow?.on?.fetch, hosts: (reachNow?.on?.hosts || []).map((x) => ({ host: x.host })) } },
  };
}

// What the app's code uses of the model and the web, what its owner has
// switched on, and today's use.
async function reachOf(appId, view) {
  return { abilities: ABILITIES, uses: usesOf(view), on: await reach.switches(appId), today: await reach.usage(appId),
    wants: await reach.wants(appId, view), asked: await reach.asked(appId), secrets: await reach.secrets(appId),
    schedules: await scheduler.list(appId, view).catch(() => []) };
}

const routes = [
  ['GET', /^\/app\.json$/, async () => MANIFEST],
  // A one-time link into the studio, for the command center (login.mjs).
  ['POST', /^\/api\/handoff$/, async (b, m, url, cx) => login.handoff(cx.req, String(b.next || ''))],
  ['GET', /^\/api\/status$/, async () => ({ model: MODEL, strata: strataUrl, cluster: ON_CLUSTER, feed: engine.feed, keys: appKeys.status(), data: child ? DATA : null, autorepair: AUTOREPAIR,
    login: login.on ? { publicUrl: login.publicUrl } : null,
    spend: spend.cap ? { usd: Math.round(spend.usd * 100) / 100, cap: spend.cap, asksUsd: Math.round(spend.asksUsd * 100) / 100, asksToday: Math.round(spend.asksDayUsd * 100) / 100, asksCap: spend.asksCap, calls: spend.calls, refused: spend.refused, since: spend.since } : null })],
  ['GET', /^\/api\/apps$/, async (b, m, url, cx) => {
    const costs = await engine.store.costs();
    // A key limited to tables lists the apps whose tables it reads.
    const apps = (await engine.store.listApps()).filter((a) => sees(cx.caller, a.id) && (!cx.builder || cx.builder.app === a.id));
    // Each app's state: live (a version), building, or failed (its first
    // build did not go through and none is running): the gallery said
    // "building" for ever of an app whose first build failed.
    const status = async (a) => {
      if (Number(a.head)) return 'live';
      if (running.has(a.id) || waiting.has(a.id)) return 'building';
      const [last] = await engine.store.requests(a.id, 1).catch(() => []);
      return firstBuildStatus(last, Date.now());
    };
    return { apps: await Promise.all(apps.map(async (a) => ({ ...a, name: nameOf(a), status: await status(a), cost: costs.get(a.id) || { usd: 0, builds: 0 } }))) };
  }],
  // The events since ?after=N (see broadcast), for one app with ?app=.
  ['GET', /^\/api\/events$/, async (b, m, url) => waitEvents(Number(url.searchParams.get('after')) || 0, url.searchParams.get('app') || null,
    Math.min(25000, Math.max(0, Number(url.searchParams.get('wait') ?? 20000) || 0)))],
  // What a view's frame is made of, for a page that builds frames itself
  // (the command center's app creator): the stylesheet, the runtime, the
  // screen renderer, the design tokens, the frame's policy and the Activity
  // view as the modules the studio imports (web/design.js, web/policy.js,
  // web/activity.js, with its stylesheet).
  ['GET', /^\/api\/kit$/, async () => {
    const read = (f) => readFile(path.join(ROOT, 'web', f), 'utf8');
    const [kit, frontCss, frame, screen, front, design, policy, activityJs, activityCss] = await Promise.all(['kit.css', 'front.css', 'frame.js', 'screen.js', 'front.js', 'design.js', 'policy.js', 'activity.js', 'activity.css'].map(read));
    return { kit: `${kit}\n${frontCss}`, frame, screen, front, design, policy, activity: activityJs, activityCss, publicUrl: login.publicUrl || null, login: login.on };
  }],
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/made$/, async (b, [id]) => provenance(await engine.view(id), id)],
  ['POST', /^\/api\/apps$/, async (b) => {
    const description = String(b.description || '').trim();
    if (description.length < 5) throw Object.assign(new Error('describe the app in a sentence or more'), { status: 400 });
    // provider (claude, codex, agy): the CLI of this first build and its designs only.
    const provider = checkProvider(b.provider);
    // A name given is the app's for good (the plan's name does not replace it).
    const given = String(b.name || '').trim().slice(0, 60);
    const name = given || shortName(description);
    const app = await engine.store.createApp({ name, description });
    if (given) await engine.store.putSetting(app, 'named', '1');
    // Its three designs are made beside the first build, from the art
    // direction its plan writes (or the description, when it has none).
    awaitDirection(app, provider);
    const request = await submit(app, description, 'user', provider ? { provider } : {});
    return { app, request: request.id };
  }],
  ['GET', /^\/api\/apps\/([a-z0-9]+)$/, async (b, [id], url, cx) => (cx.public ? publicState(id, cx.access, (await cx.user(id))?.role || null) : appState(id))],
  // Renamed by its owner: the name stays (a later plan does not rename it).
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/rename$/, async (b, [id]) => {
    const name = String(b.name || '').trim().replace(/\s+/g, ' ');
    if (!name || name.length > 60) throw Object.assign(new Error('a name is 1 to 60 characters'), { status: 400 });
    if (!(await engine.store.getApp(id))) throw Object.assign(new Error(`no app ${id}`), { status: 404 });
    await engine.store.updateApp(id, { name });
    await engine.store.putSetting(id, 'named', '1');
    broadcast('app', { app: id });
    return { name };
  }],
  // A copy, as a new app (its rows too, unless {rows: false}).
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/duplicate$/, async (b, [id]) => {
    if (!(await engine.store.getApp(id))) throw Object.assign(new Error(`no app ${id}`), { status: 404 });
    const copy = await engine.store.duplicateApp(id, { name: b.name, rows: b.rows !== false });
    log(`[${id}] copied as ${copy}`);
    broadcast('app', { app: copy });
    return { app: copy };
  }],
  // Its rows, every table, as JSON (the app creator makes CSV of a table from it).
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/export$/, async (b, [id]) => engine.store.exportApp(id)],
  // Deleted, with its own tables (app_<id>_*), versions, pieces, use and
  // files; not while a change of it is being built.
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/delete$/, async (b, [id]) => {
    if (!(await engine.store.getApp(id))) throw Object.assign(new Error(`no app ${id}`), { status: 404 });
    if (running.has(id) || waiting.has(id)) throw Object.assign(new Error('a change of this app is being built: delete it once that ends'), { status: 409 });
    const out = await engine.store.deleteApp(id);
    await rm(path.join(DATA, 'files', id), { recursive: true, force: true }).catch(() => {});
    publish.forget(id);
    for (const m of [drawnOf, lookedAt, activity, queues]) m.delete(id);
    log(`[${id}] deleted: ${out.tables.length} table${out.tables.length === 1 ? '' : 's'}, ${out.rows} rows of the engine's`);
    broadcast('app', { app: id, deleted: true });
    return { deleted: id, ...out };
  }],
  // Open to the public, or not (publish.mjs); only from the studio and the command center.
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/publish$/, async (b, [id]) => publish.of(id)],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/publish$/, async (b, [id]) => {
    const app = await engine.store.getApp(id);
    if (!app) throw Object.assign(new Error(`no app ${id}`), { status: 404 });
    const out = await publish.set(id, app.name, b || {});
    broadcast('app', { app: id });
    return out;
  }],
  // Designs: three per app, made on first ask; choosing one is instant.
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/designs$/, async (b, [id]) => designsOf(id)],
  // The whole app as one SQL script (every piece is stored as its SQL).
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/source$/, async (b, [id]) => { const view = await engine.view(id); return { name: view.app.name, sql: appScript(view) }; }],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/designs$/, async (b, [id]) => {
    const cur = await designsOf(id);
    const chosen = Number(b.chosen);
    if (!Number.isInteger(chosen) || chosen < 0 || chosen >= cur.designs.length) throw Object.assign(new Error(`chosen is 0 to ${cur.designs.length - 1}`), { status: 400 });
    const next = { ...cur, chosen };
    await engine.store.putDesigns(id, next);
    broadcast('design', { app: id, chosen });
    return next;
  }],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/designs\/new$/, async (b, [id]) => designsOf(id, true)],
  // The design in use, its tokens changed by hand; and what to choose from.
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/designs\/tune$/, async (b, [id]) => {
    const cur = await designsOf(id);
    const designs = cur.designs.slice();
    designs[cur.chosen] = tuneDesign(designs[cur.chosen], b || {});
    const next = { ...cur, designs };
    await engine.store.putDesigns(id, next);
    broadcast('design', { app: id, chosen: cur.chosen });
    return next;
  }],
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/designs\/choices$/, async (b, [id], url) => {
    const cur = await designsOf(id);
    return { choices: tokenChoices(cur.designs, cur.chosen, url.searchParams.get('scheme') === 'dark' ? 'dark' : 'light') };
  }],
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/traces\/([A-Za-z0-9_-]+)$/, async (b, [id, rid]) => {
    const trace = await engine.store.trace(rid);
    if (!trace) throw Object.assign(new Error('no timeline for this build (it ran before timelines were recorded)'), { status: 404 });
    return { trace };
  }],
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/traces$/, async (b, [id]) => ({ traces: (await engine.store.traces(id)).map(({ log, ...t }) => t) })],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/requests$/, async (b, [id]) => {
    const text = String(b.text || '').trim();
    if (!text) throw Object.assign(new Error('say what to change'), { status: 400 });
    // provider (claude, codex, agy): the CLI of this build only; an ask that
    // joins a waiting request is built with that request's.
    const provider = checkProvider(b.provider);
    const taken = b.suggestion ? await engine.store.setSuggestion(String(b.suggestion), 'taken') : null;
    // A newer shared component is taken without the model: its code is in Strata.
    if (taken?.source === 'component' && taken.cap) return { request: (await submit(id, text, 'component', { cap: taken.cap })).id };
    const source = ['user', 'suggestion', 'library'].includes(b.source) ? b.source : 'user';
    // A recommended edit says what it implies: it goes with the request.
    const r = await submit(id, taken ? withHint(text, taken) : text, source, provider ? { provider } : {});
    return { request: r.id };
  }],
  // Stop the build running for this app: its model calls are killed at once
  // and the request ends "stopped" (requests queued after it go on).
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/stop$/, async (b, [id]) => {
    const run = running.get(id);
    if (!run || (b.request && b.request !== run.id)) return { stopped: false };
    log(`[${id}] stopped by its owner`);
    run.stop.abort();
    return { stopped: true, request: run.id };
  }],
  // Reads and writes: for the app's people (the public path) under the
  // tables' declared access (access.mjs, ctx.as); the studio has every row.
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/sql$/, async (b, [id], url, cx) => ({ rows: await engine.sql(id, b.sql, b.params, { user: await cx.user(id), as: await cx.as(id), company: await cx.company(id) }) })],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/insert$/, async (b, [id], url, cx) => { const row = await engine.insert(id, b.table, b.row || {}, await cx.writer(id)); noteActivity(id); return { row }; }],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/update$/, async (b, [id], url, cx) => { const row = await engine.update(id, b.table, b.id, b.fields || {}, await cx.writer(id)); noteActivity(id); return { row }; }],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/remove$/, async (b, [id], url, cx) => { const out = await engine.remove(id, b.table, b.id, await cx.writer(id)); noteActivity(id); return out; }],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/call$/, async (b, [id], url, cx) => { const out = await engine.call(id, b.name, b.input, await cx.writer(id)); noteActivity(id); return out; }],
  // Companies (services.mjs): the ones the signed-in person works in and the
  // one they work in now (x-sa-company, else their first); made by a member,
  // who becomes its admin; its admins add people who have an account.
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/companies$/, async (b, [id], url, cx) => { if (!cx.public && url.searchParams.get('all') === '1') return { companies: await services.allCompanies(id) }; const user = await cx.user(id); return { companies: await services.companies(id, user?.id), current: await cx.company(id) }; }],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/companies$/, async (b, [id], url, cx) => ({ company: await services.createCompany(id, (await cx.user(id))?.id, b) })],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/companies\/members$/, async (b, [id], url, cx) => {
    const user = await cx.user(id);
    if (cx.public && !user) throw Object.assign(new Error('Sign in first'), { status: 401 });
    return { member: await services.addToCompany(id, b.company, cx.public ? user.id : null, b) };
  }],
  // A company that invited the signed-in person (an admin added them on the
  // app's page): they join it themselves.
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/companies\/join$/, async (b, [id], url, cx) => ({ member: await services.joinCompany(id, b.company, (await cx.user(id))?.id) })],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/companies\/members\/remove$/, async (b, [id], url, cx) => {
    const user = await cx.user(id);
    if (cx.public && !user) throw Object.assign(new Error('Sign in first'), { status: 401 });
    return services.removeFromCompany(id, b.company, cx.public ? user.id : null, String(b.person || ''));
  }],
  // A member made an admin, or a member again: by the app's owner (or the studio).
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/people\/role$/, async (b, [id], url, cx) => {
    if (cx.public) {
      const user = await cx.user(id);
      if (!user) throw Object.assign(new Error('Sign in first'), { status: 401 });
      if (user.role !== 'owner') throw Object.assign(new Error("Only the app's owner can change who is an admin"), { status: 403 });
    }
    return { person: await services.setRole(id, b.person, String(b.role || '')) };
  }],
  // The app's builders (builders.mjs): invite by address with a role, a
  // fresh link for an invitation, a role changed, someone removed.
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/builders$/, async (b, [id], url, cx) => { await engine.view(id); return { builders: await builders.list(id), me: cx.builder ? { id: cx.builder.id, role: cx.builder.role } : { role: 'owner' } }; }],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/builders$/, async (b, [id], url, cx) => { await engine.view(id); return builders.invite(id, b.emails, b.role, { base: baseOf(cx.req) }); }],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/builders\/([a-z0-9]+)\/link$/, async (b, [id, bid], url, cx) => builders.link(id, bid, { base: baseOf(cx.req) })],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/builders\/([a-z0-9]+)\/remove$/, async (b, [id, bid], url, cx) => {
    if (cx.builder && cx.builder.id === bid) throw Object.assign(new Error('to leave, ask the owner or an admin to remove you'), { status: 400 });
    return builders.remove(id, bid);
  }],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/builders\/([a-z0-9]+)$/, async (b, [id, bid], url, cx) => {
    if (cx.builder && cx.builder.id === bid) throw Object.assign(new Error('a builder does not change their own role'), { status: 400 });
    return { builder: await builders.setRole(id, bid, b.role) };
  }],
  // Notes on the app's screens (comments.mjs). The author is the builder
  // whose key it is; the owner names themselves (the studio has no account).
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/comments$/, async (b, [id], url) => { await engine.view(id); const s = url.searchParams.get('status'); return { comments: await comments.list(id, { status: ['open', 'resolved'].includes(s) ? s : null }) }; }],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/comments$/, async (b, [id], url, cx) => { await engine.view(id); const out = await comments.add(id, b, authorOf(cx, b)); broadcast('comments', { app: id }); return { comment: out }; }],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/comments\/([a-z0-9]+)\/reply$/, async (b, [id, cid], url, cx) => { const out = await comments.reply(id, cid, b.body, authorOf(cx, b)); broadcast('comments', { app: id }); return { comment: out }; }],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/comments\/([a-z0-9]+)\/resolve$/, async (b, [id, cid]) => { const out = await comments.resolve(id, cid, { reopen: !!b.reopen }); broadcast('comments', { app: id }); return { comment: out }; }],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/comments\/([a-z0-9]+)\/delete$/, async (b, [id, cid]) => { const out = await comments.remove(id, cid); broadcast('comments', { app: id }); return out; }],
  // Who is working on the app now: a heartbeat every few seconds while it is
  // open, and leaving; everyone working on it hears of arrivals and leavings.
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/presence$/, async (b, [id]) => ({ here: presence.here(id) })],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/presence$/, async (b, [id], url, cx) => {
    await engine.view(id);
    const who = cx.builder ? { id: cx.builder.id, name: cx.builder.name || cx.builder.email || '', role: cx.builder.role } : { id: 'owner', name: String(b?.name || '').trim().slice(0, 80), role: 'owner' };
    const changed = b?.leave ? presence.leave(id, who.id) : presence.beat(id, { ...who, screen: b?.screen });
    if (changed) broadcast('presence', { app: id });
    return { here: presence.here(id), me: who.id };
  }],
  // An invitation taken, from the studio or another client: the key to use.
  ['POST', /^\/api\/builders\/join$/, async (b, m, url, cx) => { const out = await builders.join(b.token, { name: b.name }); setBuilderCookie(cx.res, out.key); return out; }],
  // People: one account for every app here; joining an app on first use.
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/me$/, async (b, [id], url, cx) => ({ user: await cx.user(id) })],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/signup$/, async (b, [id], url, cx) => { const out = await services.signUp(id, b, { pub: !!cx.public }); setSession(cx.res, out.token); return { user: out.user }; }],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/signin$/, async (b, [id], url, cx) => { const out = await services.signIn(id, b, { pub: !!cx.public }); setSession(cx.res, out.token); return { user: out.user }; }],
  ['POST', /^\/api\/signout$/, async (b, m, url, cx) => { await services.signOut(tokenOf(cx.req)); setSession(cx.res, null); return { ok: true }; }],
  // A published app's people are for its signed-in members only, and their
  // emails for its owner: never for someone passing by.
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/people$/, async (b, [id], url, cx) => {
    if (!cx.public) return { people: await services.people(id) };
    const user = await cx.user(id);
    if (!user) throw Object.assign(new Error('sign in to this app to see who uses it'), { status: 401 });
    const people = await services.people(id);
    return { people: user.role === 'owner' ? people : people.map(({ id: pid, name, role }) => ({ id: pid, name, role })) };
  }],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/files\/info$/, async (b, [id]) => ({ files: (await services.fileInfo(id, b.ids)).map((f) => ({ ...f, url: `/api/apps/${id}/files/${f.id}` })) })],
  // Email from a view needs someone signed in; functions send as the app.
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/email$/, async (b, [id], url, cx) => {
    const user = await cx.user(id);
    if (!user) throw Object.assign(new Error('email: sign in to send email from this app'), { status: 401 });
    return { email: await services.email(id, b, user.id, { sender: cx.public ? await cx.as(id) : null }) };
  }],
  // What people did in the app, in its own words, for the Activity view.
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/activity$/, async (b, [id], url, cx) => {
    const days = Math.max(0, Math.min(3650, Number(url.searchParams.get('days') ?? 30) || 0));
    const out = await activityOf(engine, id, { days });
    const view = await engine.store.resolve(id);
    return { ...out, me: await cx.who(id), people: (await services.people(id).catch(() => [])).map((p) => ({ id: p.id, name: p.name })),
      signals: (await evolver.state(id, view).catch(() => ({ signals: [] }))).signals };
  }],
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/outbox$/, async (b, [id]) => ({ outbox: await services.outbox(id) })],
  // A card payment for a row (an order): anyone using the app may ask, the
  // amount is the row's; the payer pays on the hosted page (/pay/<id>).
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/pay$/, async (b, [id], url, cx) => {
    const user = await cx.user(id);
    return { payment: await services.pay(id, b, { who: await cx.who(id), owner: !cx.public || user?.role === 'owner' }) };
  }],
  // The app's payments, for its owner (the studio, or the owner signed in on its page).
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/payments$/, async (b, [id], url, cx) => {
    if (cx.public) {
      const user = await cx.user(id);
      if (!user) throw Object.assign(new Error('Sign in first: only the app\'s owner sees its payments'), { status: 401 });
      if (user.role !== 'owner') throw Object.assign(new Error('Only the app\'s owner sees its payments'), { status: 403 });
    }
    const live = !!services.payProvider;
    return { payments: await services.payments(id), mode: live ? 'live' : 'test',
      note: live ? `Card payments go through ${services.payProvider.name}; a payment is paid only on its signed notice.` : 'Card payments are in test mode: no card is charged and orders paid this way say "paid (test)". Connect a card payment provider to take real payments.' };
  }],
  // A model and the web (reach.mjs): views ask through these, functions
  // through the same calls in the engine.
  // A published app's visitors name no secret, header or method of their own
  // (reach.fetch), add no host to the owner's list of hosts asked for, and
  // give app.ask no instructions, web search or fresh call of their own:
  // they get the plain call, under the app's switches and limits.
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/ask$/, async (b, [id], url, cx) => ({ answer: await reach.ask(id, b.prompt, cx.public ? publicAsk(b.opts) : b.opts || {}) })],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/fetch$/, async (b, [id], url, cx) => ({ page: await reach.fetch(id, b.url, cx.public ? {} : b.opts || {}, { pub: !!cx.public }) })],
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/reach$/, async (b, [id]) => reachOf(id, await engine.view(id))],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/reach$/, async (b, [id]) => {
    const on = await reach.setSwitches(id, b || {});
    log(`[${id}] reach: ${Object.keys(ABILITIES).filter((k) => on[k]).join(', ') || 'nothing'} switched on, ${on.perDay} model calls a day`);
    return reachOf(id, await engine.view(id));
  }],
  // Secrets (API keys): set, changed (hosts, where it goes) and removed by
  // the owner; their values are never sent back.
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/secrets$/, async (b, [id]) => ({ secrets: await reach.secrets(id) })],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/secrets$/, async (b, [id]) => {
    const secret = await reach.putSecret(id, b || {});
    log(`[${id}] secret ${secret.name} set, sent only to ${secret.hosts.join(', ')}`);
    return { secret, reach: await reachOf(id, await engine.view(id)) };
  }],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/secrets\/([A-Za-z0-9_-]+)\/delete$/, async (b, [id, name]) => {
    const out = await reach.deleteSecret(id, name);
    log(`[${id}] secret ${name} removed`);
    return { ...out, reach: await reachOf(id, await engine.view(id)) };
  }],
  // Schedules: when each runs, how it went; run one now, pause or start it again.
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/schedules$/, async (b, [id]) => ({ schedules: await scheduler.list(id) })],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/schedules\/([a-z0-9_-]+)\/run$/, async (b, [id, slug]) => ({ run: await scheduler.runNow(id, slug), schedules: await scheduler.list(id) })],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/schedules\/([a-z0-9_-]+)\/(pause|resume)$/, async (b, [id, slug, what]) => { await scheduler.setPaused(id, slug, what === 'pause'); return { schedules: await scheduler.list(id) }; }],
  ['GET', /^\/api\/geocode$/, async (b, m, url) => ({ places: await services.geocode(url.searchParams.get('q')) })],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/track$/, async (b, [id], url, cx) => {
    // open and leave name a screen (leave with the time spent on it); abandon
    // is a form typed into and closed unsaved; an action is anything else an
    // app records (a search with the words and how many rows they found).
    // A beacon sends no headers: the studio's says it is the owner's in its body.
    const kind = ['open', 'leave', 'abandon'].includes(b.kind) ? b.kind : 'action';
    const detail = kind === 'action' ? `${String(b.action || '').slice(0, 60)}${b.detail ? ` ${String(b.detail).slice(0, 200)}` : ''}` : String(b.detail || '');
    const where = cx.where(b.session);
    const screen = kind === 'open' || kind === 'leave' ? String(b.detail || '') : where.screen || b.screen;
    const q = kind === 'action' && /^(search|filter)$/.test(String(b.action || '')) ? (b.q ?? b.detail) : null;
    // The device: as the page says, else as its browser does.
    const device = ['phone', 'tablet', 'computer'].includes(b.device) ? b.device : /iPad|Tablet/i.test(cx.req.headers['user-agent'] || '') ? 'tablet' : /Mobi|Android|iPhone/i.test(cx.req.headers['user-agent'] || '') ? 'phone' : cx.req.headers['user-agent'] ? 'computer' : null;
    await engine.store.track(id, kind, String(b.cap || ''), detail, { person: await cx.who(id), v: (await engine.view(id)).comp.n, ...where, taps: null, screen, tbl: b.table, q, results: b.results, ms: b.ms, device, src: cx.owner || (!cx.public && b.owner === true) ? 'owner' : null });
    noteActivity(id);
    return { ok: true };
  }],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/error$/, async (b, [id], url, cx) => {
    // A published app's visitors start no build: their text would go into
    // the model's prompt, and a repair becomes the live version. Their
    // errors are logged for the owner to see in the studio.
    if (cx.public) { log(`[${id}] error from a visitor (no repair): ${String(b.cap || '').slice(0, 40)} ${String(b.message || '').slice(0, 200)}`); return { ok: true, repairing: false }; }
    const cap = String(b.cap || '');
    // An error from code the app no longer runs (a window still showing an
    // older version) is not sent to the forge; nor is a visitor's (a
    // published app's public), which is only recorded for the owner.
    const repairing = !cx.public && AUTOREPAIR && !repaired.has(cap);
    const out = await engine.browserError(id, cap, b.message, { visitor: !!cx.public });
    return out.stale ? { ok: true, repairing: false, stale: true } : { ok: true, repairing: repairing && out.sent };
  }],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/suggest$/, async (b, [id]) => {
    const out = await evolver.tick(id, { force: true });
    if (out.busy) throw Object.assign(new Error('the engine is already reading this app\'s use'), { status: 409 });
    const rec = await recommender.refresh(id, { reason: 'asked' }).catch((e) => { log(`[${id}] recommend: ${e.message}`); return { added: [] }; });
    if (!engine.feed) broadcast('suggestions', { app: id });
    return { suggestions: [...(out.suggestions || []), ...rec.added], building: out.building };
  }],
  // Evolving from use: the mode (off, suggest, build), and the candidates.
  // Every candidate the recommender finds now, scored, with its source and evidence (nothing stored).
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/recommend$/, async (b, [id]) => recommender.preview(id)],
  ['GET', /^\/api\/apps\/([a-z0-9]+)\/evolve$/, async (b, [id]) => evolver.state(id, await engine.store.resolve(id))],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/evolve$/, async (b, [id]) => ({ mode: await evolver.setMode(id, String(b.mode || '')), modes: MODES })],
  ['POST', /^\/api\/apps\/([a-z0-9]+)\/candidates\/([a-z0-9]+)$/, async (b, [id, cid]) => {
    if (b.action === 'keep') return evolver.keep(id, cid);
    if (b.action === 'drop') return evolver.drop(id, cid);
    throw Object.assign(new Error('action is keep or drop'), { status: 400 });
  }],
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
      engine.changes?.soon();
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
    // A question about how pieces fit together is answered from the graph.
    const question = q ? await ask(engine.store, q).catch(() => null) : null;
    if (question) return { question };
    const rows = q ? await engine.store.searchCapabilities(q, { limit: 40 }) : await engine.store.liveCapabilities();
    const apps = new Map((await engine.store.listApps()).map((a) => [a.id, a.name]));
    const users = await engine.store.sharedUsers();
    const isShared = (r) => r.kind === 'component' || r.kind === 'helper';
    const patternBy = (r) => (r.author === 'builtin' ? 'built-in pattern' : `pattern from ${apps.get(r.author) || r.author}`);
    return { capabilities: rows.map((r) => ({ ...r, app_name: isShared(r) ? 'shared' : r.kind === 'pattern' ? patternBy(r) : r.app === 'lib' ? 'built-in starter' : apps.get(r.app) || r.app,
      used_by: isShared(r) ? (users.get(`${r.kind}/${r.slug}`) || []).map((u) => ({ app: u.app, name: u.name, version: u.version })) : undefined })) };
  }],
  // A question about the graph ("which screens use the calendar").
  ['GET', /^\/api\/graph\/ask$/, async (b, m, url) => (await ask(engine.store, String(url.searchParams.get('q') || ''))) || { understood: null }],
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
  let visiting = false;
  try {
    if (url.pathname === '/healthz') return send(res, 200, { ok: true });
    // The kit's typefaces (web/fonts, SIL Open Font License): open to any
    // page, a view's sandboxed frame included (it has no origin of its own).
    let fm;
    if (req.method === 'GET' && (fm = url.pathname.match(/^\/fonts\/([a-z-]+)\.woff2$/))) {
      const bytes = await readFile(path.join(ROOT, 'web', 'fonts', `${fm[1]}.woff2`)).catch(() => null);
      if (!bytes) return send(res, 404, { error: 'no such font' });
      res.writeHead(200, { 'content-type': 'font/woff2', 'content-length': bytes.length, 'cache-control': 'public, max-age=604800', 'access-control-allow-origin': '*' });
      return res.end(bytes);
    }
    // Caddy asks before it gets a certificate for a host (the operator's
    // spec.apps.publish): only for published apps' hosts.
    if (req.method === 'GET' && url.pathname === '/api/tls/ask') {
      await publish.load();
      const ok = !!publish.appForHost({ headers: { host: String(url.searchParams.get('domain') || '') } });
      return send(res, ok ? 200 : 404, { ok });
    }
    if (req.method === 'GET' && url.pathname === '/login' && login.on) return await login.redeem(url, res);
    // Someone not signed in to the studio may still open a published app.
    let refused = await login.check(req, url);
    // Through chloedb-server: what the caller's key may do (callers.mjs).
    const caller = login.fromServer(req) ? callerOf(req) : null;
    const no = refusal(caller, req.method, url);
    if (no) return send(res, 403, { error: no });
    // The studio's "See it as a stranger": someone who may use the studio
    // asks to be treated as a visitor (never the other way round).
    const stranger = !refused ? strangerOf(req) : null;
    const seen = stranger ? await publish.allows(req, url, stranger) : null;
    // A refusal, not an error: the page says it and records nothing (refused).
    if (seen?.denied) return send(res, 403, { error: seen.denied, refused: true });
    const open = seen || (login.on && refused ? await publish.allows(req, url) : !login.on ? await publish.allows(req, url) : null);
    if (refused && open?.denied) return send(res, 403, { error: open.denied, refused: true });
    if (refused && open) {
      refused = null;
      if (!open.file) {
        const wait = publish.limit(req, open.dear);
        if (wait) { res.setHeader('retry-after', String(wait)); return send(res, 429, { error: `too many requests; try again in ${wait} s` }); }
      }
    }
    // What the request is, for the routes: the published app it may use.
    const visitorApp = seen ? seen.app : login.on && open && !open.file ? open.app : null;
    const cx0 = { public: visitorApp, access: visitorApp ? open.access || null : null, stranger: !!seen, caller };
    visiting = !!cx0.public;
    if (refused) {
      if (refused.json) return send(res, refused.status, refused.json);
      res.writeHead(refused.status, { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store' });
      return res.end(refused.html);
    }
    // A builder's key holds them to their app and role (builders.mjs); the
    // studio without one is its owner, as before. Not on a published app's
    // public path, which publish.mjs judges.
    let builder = null;
    if (!cx0.public) {
      const key = builderKeyOf(req);
      if (key) {
        builder = await builders.ofKey(key);
        if (!builder) return send(res, 401, { error: 'this builder key is not valid any more; ask the app\'s owner for a new invitation' });
        const no = builderRefusal(builder, req.method, url.pathname);
        if (no) return send(res, 403, { error: no });
      }
    }
    cx0.builder = builder;
    // An invitation link opened in a browser: taken, then the app in the studio.
    if (req.method === 'GET' && url.pathname === '/join') {
      try {
        const out = await builders.join(url.searchParams.get('token'));
        setBuilderCookie(res, out.key);
        res.writeHead(302, { location: `/#/app/${out.app}`, 'cache-control': 'no-store' });
        return res.end();
      } catch (e) { return send(res, e.status || 400, { error: e.message }); }
    }
    // An app on its own, without the studio around it (for phones and sharing).
    // On a published app's own host, / is the app.
    const hostApp = req.method === 'GET' && url.pathname === '/' ? publish.appForHost(req) : null;
    const page = req.method === 'GET' && (hostApp || /^\/a\/[a-z0-9]+\/?$/.test(url.pathname)) ? STATIC['/'] : STATIC[url.pathname];
    if (req.method === 'GET' && page) {
      const [file, type] = page;
      let data = await readFile(path.join(ROOT, file));
      if (hostApp) data = Buffer.from(String(data).replace('<head>', `<head>\n<meta name="sa-app" content="${hostApp}">`));
      const headers = { 'content-type': type, 'cache-control': 'no-cache' };
      if (page === STATIC['/'] && !visitorOf(req)) headers['set-cookie'] = `${VISITOR}=${newId(12)}; Path=/; HttpOnly; SameSite=Lax; Max-Age=${365 * 86400}`;
      res.writeHead(200, headers);
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
      clients.set(res, cx0.public ? { app: cx0.public } : cx0.builder ? { app: cx0.builder.app, builder: true } : null);
      const ping = setInterval(() => res.write(': ping\n\n'), 20000);
      req.on('close', () => { clearInterval(ping); clients.delete(res); });
      return;
    }
    // A card provider's signed notice that a payment went through
    // (services.providerNotice): from the provider's servers, so before the
    // same-origin check; its signature is what is trusted, nothing else.
    if (req.method === 'POST' && url.pathname === '/api/payments/notice') {
      const raw = await rawBody(req, 64 * 1024);
      try { return send(res, 200, await services.providerNotice(raw, req.headers['x-payment-signature'])); } catch (e) { return send(res, e.status || 500, { error: e.message }); }
    }
    // Only this page may call the API: a page on another site could otherwise
    // post to localhost.
    if (req.method === 'POST' && !login.sameOrigin(req)) return send(res, 403, { error: 'cross-origin request refused' });
    // A payment's hosted page (services.pay): the payer pays here, never in
    // the app, so the app never sees a card. In test mode no card is asked.
    if ((m = url.pathname.match(/^\/pay\/([a-z0-9]{8,32})$/)) && (req.method === 'GET' || req.method === 'POST')) {
      let p = await services.payment(m[1]);
      if (!p) { res.writeHead(404, { 'content-type': 'text/plain; charset=utf-8' }); return res.end('No such payment.'); }
      let token = url.searchParams.get('t') || '';
      let problem = '';
      if (req.method === 'POST') {
        token = new URLSearchParams(String(await rawBody(req, 8 * 1024))).get('t') || '';
        try { p = await services.payTest(m[1], token); } catch (e) { problem = e.message; }
      }
      const appRow = await engine.store.getApp(p.app).catch(() => null);
      // same-origin: the page's own Pay posts with its origin (no-referrer
      // would send "null" and be refused above); other sites never see its address
      res.writeHead(problem ? 403 : 200, { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store', 'referrer-policy': 'same-origin' });
      return res.end(payPage(p, appRow?.name || 'This shop', { token: p.status === 'open' && services.tokenOk(p, token) ? token : '', problem, provider: services.payProvider?.name || '', locale: await engine.store.setting(p.app, 'locale').catch(() => null) }));
    }
    // An upload is the file's bytes as the body; its name and type in the query.
    if (req.method === 'POST' && (m = url.pathname.match(/^\/api\/apps\/([a-z0-9]+)\/files$/))) {
      const bytes = await rawBody(req, MAX_FILE);
      const user = cx0.stranger ? null : await userOf(req, m[1], { join: !cx0.public });
      const file = await services.saveFile(m[1], bytes, { name: url.searchParams.get('name'), type: url.searchParams.get('type') || req.headers['content-type'] }, user?.id);
      noteActivity(m[1]);
      return send(res, 200, { file: { ...file, url: `/api/apps/${m[1]}/files/${file.id}` } });
    }
    // Where a request comes from: the studio's tab (its session) and the
    // screen it was made on, as the studio's headers say (a beacon, which
    // sends no headers, names its session in the body).
    const tag = (x) => (typeof x === 'string' && /^[A-Za-z0-9_.:-]{1,80}$/.test(x) ? x : null);
    // A write or a run also says how many taps led to it (x-sa-taps).
    const taps = Number(req.headers['x-sa-taps']);
    const where = (session) => ({ session: tag(req.headers['x-sa-session']) || tag(session), screen: tag(req.headers['x-sa-view']), ...(Number.isInteger(taps) && taps >= 0 && taps < 10000 ? { taps } : {}) });
    const join = !cx0.public;
    // A stranger is nobody signed in, whatever the studio's session.
    const people = new Map(); // app -> the person, once per request
    const personOf = (appId) => {
      if (cx0.stranger) return Promise.resolve(null);
      if (!people.has(appId)) people.set(appId, userOf(req, appId, { join }));
      return people.get(appId);
    };
    // The company the person works in now (services.companyOf).
    const companyOf = async (appId) => { const u = await personOf(appId); return u ? services.companyOf(appId, u.id, tag(req.headers['x-sa-company'])) : null; };
    // Who a read or write is for, against the tables' declared access
    // (access.mjs): one of the app's people on the public path (a visitor
    // when nobody is signed in), or null for the studio, which has every row.
    const asOf = async (appId) => {
      if (!cx0.public) return null;
      const user = await personOf(appId);
      return { role: user ? user.role || 'member' : 'visitor', user, company: user ? await companyOf(appId) : null };
    };
    const who = async (appId) => (await personOf(appId))?.id || visitorOf(req);
    // The app's owner trying it: the studio (which says so, x-sa-owner) or
    // the command center (through chloedb-server), never a published app's
    // visitor. Usage tells their events apart from people's (src "owner").
    const owner = !cx0.public && (login.fromServer(req) || req.headers['x-sa-owner'] === '1');
    const writer = async (appId) => ({ user: await personOf(appId), who: await who(appId), visitor: !!cx0.public, ownRows: cx0.access === 'add', ...where(), pub: !!cx0.public, as: await asOf(appId), company: await companyOf(appId), owner });
    const cx = { ...cx0, req, res, where, user: personOf, who, as: asOf, company: companyOf, writer, owner };
    // ?v=N runs a request on version N of the app (a version built to try).
    const v = Number(url.searchParams.get('v')) || null;
    for (const [method, re, fn] of routes) {
      if (req.method !== method) continue;
      const mm = url.pathname.match(re);
      if (!mm) continue;
      const b = method === 'POST' ? await body(req) : {};
      const run = () => fn(b, mm.slice(1), url, cx);
      return send(res, 200, await (v && url.pathname.startsWith('/api/apps/') ? engine.atVersion(mm[1], v, run) : run()));
    }
    send(res, 404, { error: 'not found' });
  } catch (e) {
    const passOn = e.status && (e.status < 500 || ((e.status === 502 || e.status === 503) && !e.message.startsWith('strata')));
    const status = passOn ? e.status : (/^(no |table |app\.sql|one statement|empty SQL|.* takes )/.test(e.message) ? 400 : (e.message.startsWith('strata') ? 400 : 500));
    if (status >= 500) log(`${req.method} ${url.pathname}: ${e.stack || e.message}`);
    send(res, status, { error: visiting ? forVisitor(status, e.message) : e.message, ...(e.refused ? { refused: true } : {}) });
  }
});

server.listen(PORT, HOST, () => {
  log(`chloedb-apps on http://${HOST}:${PORT}/ (ChloeDB at ${strataUrl}, model ${MODEL}${SCREEN_MODEL !== MODEL ? `, screens ${SCREEN_MODEL}` : ''})`);
});

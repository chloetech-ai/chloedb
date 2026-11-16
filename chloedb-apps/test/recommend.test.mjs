// The next-edit recommender (engine/recommend.mjs, engine/recsources.mjs):
// its sources over synthetic use, its ranking and learning, and a refresh
// over a store kept in memory; then, with a chloedb-server and a copy of
// built apps (STRATA_APPS_RECOMMEND_DATA; skipped without it), one app
// with use given to it and one without, over the real engine.

import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { cpSync, existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { signals } from '../engine/evolve.mjs';
import { usePatterns, useUnopened, usePhone, useErrors, startAsks, startLibrary, asksOf, wordsOf, builtWords, SOURCES } from '../engine/recsources.mjs';
import { Recommender, rank, learnWeights, withHint } from '../engine/recommend.mjs';

const T0 = Date.parse('2026-11-01T09:00:00Z');
const at = (s) => new Date(T0 + s * 1000).toISOString();
// Events as the store gives them: newest first.
const newestFirst = (evs) => evs.slice().sort((a, b) => b.ts.localeCompare(a.ts));

const cap = (kind, slug, extra = {}) => ({ id: `app1/${kind}/${slug}@1`, app: 'app1', kind, slug, title: extra.title || slug, description: extra.description || '', code: extra.code || '', spec: extra.spec || {}, created: at(-86400) });
function viewOf({ views = [], tables = [], functions = [], shared = [] } = {}) {
  const caps = [...tables, ...views, ...functions, ...shared];
  return { app: { id: 'app1', name: 'Shop', description: 'A small shop', head: 3 }, comp: { n: 3, caps: caps.map((c) => c.id) }, caps, tables, views, functions, components: shared, helpers: [], shared };
}
const VIEW = viewOf({
  tables: [cap('table', 'orders', { spec: { name: 'orders', columns: [{ name: 'customer' }, { name: 'status' }, { name: 'total' }] } })],
  views: [cap('view', 'orders', { title: 'Orders', description: 'Every order with its status' }), cap('view', 'stock', { title: 'Stock', description: 'What is left of each product' }), cap('view', 'reports', { title: 'Reports', description: 'Sales per month' })],
  functions: [cap('function', 'reserve', { description: 'Reserves stock for an order' })],
});
const ctxOf = (events, extra = {}) => ({ appId: 'app1', view: VIEW, events: newestFirst(events), signals: signals(newestFirst(events), { views: VIEW.views }), history: [], requests: [], versions: [], design: null, built: builtWords(VIEW), now: T0 + 86400000, appText: 'shop orders stock', appName: (id) => id, search: async () => [], near: async () => ({ best: 0, covered: false }), usersOf: () => [], example: () => null, ...extra });

test('use: a path between two screens and a field changed row after row become candidates with the change they imply', async () => {
  const evs = [];
  for (let i = 0; i < 6; i++) {
    evs.push({ kind: 'open', detail: 'orders', session: `s${i}`, person: `p${i % 2}`, ts: at(i * 1000) });
    evs.push({ kind: 'open', detail: 'stock', session: `s${i}`, person: `p${i % 2}`, ts: at(i * 1000 + 5) });
  }
  for (let r = 0; r < 7; r++) evs.push({ kind: 'update', detail: 'orders: status', session: 'edit', person: 'p0', ts: at(20000 + r * 20) });
  const out = await usePatterns.run(ctxOf(evs));
  const path = out.find((c) => c.basis === 'use.path');
  assert.ok(path, JSON.stringify(out));
  assert.equal(path.key, 'path:orders>stock');
  assert.match(path.text, /On Orders.*Stock/);
  assert.match(path.why, /went from Orders straight to Stock 6 times/);
  assert.deepEqual([path.change.kind, path.change.from, path.change.to], ['link', 'orders', 'stock']);
  const edits = out.find((c) => c.basis === 'use.edits');
  assert.ok(edits);
  assert.equal(edits.change.kind, 'inline-edit');
  assert.deepEqual(edits.change.fields, ['status']);
  assert.ok(edits.confidence > 0 && edits.confidence < 1);
});

test('use: a screen nobody opens, a screen used on phones, an error and a refusal that repeat', async () => {
  const evs = [];
  for (let i = 0; i < 14; i++) {
    evs.push({ kind: 'open', detail: i % 2 ? 'orders' : 'stock', session: `s${i % 4}`, device: 'phone', ts: at(i * 60) });
    evs.push({ kind: 'leave', detail: i % 2 ? 'orders' : 'stock', session: `s${i % 4}`, device: 'phone', ms: 40000, ts: at(i * 60 + 30) });
  }
  for (let i = 0; i < 4; i++) evs.push({ kind: 'open', detail: 'orders', session: 'desk', device: 'computer', ts: at(2000 + i) }, { kind: 'leave', detail: 'orders', session: 'desk', device: 'computer', ms: 10000, ts: at(2000 + i) });
  evs.push({ kind: 'error', cap: 'app1/function/reserve@1', detail: 'call: stock is not defined', ts: at(3000) }, { kind: 'error', cap: 'app1/function/reserve@1', detail: 'call: stock is not defined', ts: at(3100) });
  evs.push({ kind: 'error', cap: 'app1/function/reserve@0', detail: 'call: an old version', ts: at(3000) }, { kind: 'error', cap: 'app1/function/reserve@0', detail: 'call: an old version', ts: at(3001) });
  for (let i = 0; i < 3; i++) evs.push({ kind: 'refused', cap: 'app1/function/reserve@1', detail: 'reserve: Only 2 left', person: `p${i}`, ts: at(4000 + i) });
  const ctx = ctxOf(evs);
  const unopened = await useUnopened.run(ctx);
  assert.deepEqual(unopened.map((c) => c.key), ['unopened:reports']);
  assert.match(unopened[0].why, /Nobody opened Reports in \d+ visits/);
  const phone = await usePhone.run(ctx);
  assert.ok(phone.some((c) => c.key === 'phone:stock'), JSON.stringify(phone));
  const orders = phone.find((c) => c.key === 'phone:orders');
  assert.match(orders.why, /4\.0 times as long/);
  const errs = await useErrors.run(ctx);
  assert.equal(errs.filter((c) => c.basis === 'use.errors').length, 1, 'only the error of the live version');
  const refused = errs.find((c) => c.basis === 'use.refused');
  assert.match(refused.text, /Only 2 left/);
  assert.match(refused.why, /turned away 3 times by reserve, 3 of them/);
});

test('cold start: the asks of a request without its pasted data, and the ones nothing built mentions', async () => {
  const text = 'Make me a shop for my candles.\nSheet "Stock":\nName,Price,Left\nVanilla,12,4\nLavender,14,0\nLet customers leave a review with stars on each candle. Show the orders too.';
  assert.deepEqual(asksOf(text), ['Make me a shop for my candles.', 'Let customers leave a review with stars on each candle.', 'Show the orders too.']);
  const ctx = ctxOf([], { requests: [{ id: 'r1', text, source: 'user', status: 'done', comp: 1 }] });
  const out = await startAsks.run(ctx);
  assert.equal(out.length, 1, JSON.stringify(out));
  assert.match(out[0].text, /review with stars/);
  assert.match(out[0].why, /"review/);
  assert.equal(out[0].change.kind, 'ask');
});

test('cold start: a library pattern that fits and is not used, with the block that adds it; one used or far is not offered', async () => {
  const search = async () => [
    { id: 'lib/pattern/board@1', kind: 'pattern', slug: 'board', title: 'A board of rows by a status column', description: 'kanban', found: 'both', cosine: 0.41 },
    { id: 'lib/pattern/crud@1', kind: 'pattern', slug: 'crud', title: 'Rows of one table', description: '', found: 'both', cosine: 0.5 },
    { id: 'lib/component/gantt@1', kind: 'component', slug: 'gantt', title: 'Gantt chart', description: '', found: 'meaning', cosine: 0.2 },
  ];
  const view = viewOf({ ...VIEW, views: [cap('view', 'orders', { title: 'Orders', spec: { patterns: ['crud'] } })], tables: VIEW.tables });
  const out = await startLibrary.run(ctxOf([], { view, search, usersOf: () => [{ app: 'b', name: 'Bakery' }, { app: 'c', name: 'Cafe' }], example: (s) => ({ use: s, table: 'orders' }) }));
  assert.deepEqual(out.map((c) => c.key), ['lib:pattern/board']);
  assert.equal(out[0].change.kind, 'pattern');
  assert.match(out[0].change.hint, /"use":"board"/);
  assert.match(out[0].why, /2 other apps here already have it \(Bakery, Cafe\)/);
  // the reason is in words: no score, no internal name
  assert.doesNotMatch(out[0].why, /\d\.\d|pattern|component|meaning/);
});

test('ranking: one per key and per meaning, nothing offered or dismissed again, two of a source at most', () => {
  const c = (key, basis, text, confidence) => ({ key, basis, source: 'use', text, why: '', confidence });
  const cands = [
    c('a', 'use.path', 'Show stock on the orders screen', 0.9),
    c('a', 'use.path', 'Show stock on the orders screen again', 0.8),
    c('b', 'use.path', 'Show the stock on the orders screen please', 0.7),
    c('c', 'use.path', 'Link reports from stock', 0.6),
    c('d', 'use.path', 'Link customers from reports', 0.6),
    c('e', 'start.asks', 'Let customers leave reviews', 0.5),
    c('f', 'start.asks', 'Add a dark theme', 0.5),
    c('g', 'start.peers', 'Something weak', 0.05),
  ];
  const out = rank(cands, { history: [{ signal: 'f', status: 'dismissed', text: 'Add a dark theme', created: new Date().toISOString() }] });
  assert.deepEqual(out.map((x) => x.key), ['a', 'c', 'e']);
  // A stale one may come back; a dismissal long ago no longer blocks.
  assert.ok(rank(cands, { history: [{ signal: 'f', status: 'dismissed', created: '2020-01-01T00:00:00Z', text: 'x' }] }).some((x) => x.key === 'f'));
});

test('learning: a source people take weighs more, one they dismiss less, and this app\'s own decisions count most', () => {
  const rows = [
    ...Array(6).fill({ app: 'x', basis: 'use.path', status: 'taken' }),
    ...Array(6).fill({ app: 'x', basis: 'start.peers', status: 'dismissed' }),
    ...Array(4).fill({ app: 'me', basis: 'use.path', status: 'dismissed' }),
  ];
  const g = learnWeights(rows, 'other');
  assert.ok(g('use.path') > 1 && g('start.peers') < 1 && g('start.asks') === 1);
  const mine = learnWeights(rows, 'me');
  assert.ok(mine('use.path') < g('use.path'), 'my own dismissals pull it down');
});

test('taking a suggestion sends the change it implies with the request', () => {
  assert.equal(withHint('Add a board', { implies: JSON.stringify({ kind: 'pattern', hint: 'Use the board pattern.' }) }), 'Add a board\n\n(Use the board pattern.)');
  assert.equal(withHint('Add a board', { change: null }), 'Add a board');
});

// A store in memory with what the recommender reads and writes.
function memoryStore(view, events) {
  const rows = [];
  const store = {
    meaning: false,
    resolve: async () => view, getApp: async () => view.app, listApps: async () => [{ id: 'app1', name: 'Shop', head: 3 }],
    recentEvents: async () => newestFirst(events), requests: async () => [], compositions: async () => [], designs: async () => null,
    patterns: async () => new Map(), searchCapabilities: async () => [],
    suggestionHistory: async () => rows.slice().reverse().map((r) => ({ ...r, change: r.implies })),
    putSuggestions: async (app, items) => { const docs = items.map((s, i) => ({ id: `s${rows.length + i}`, app, status: 'open', created: new Date().toISOString(), ...s })); rows.push(...docs); return docs; },
    db: {
      query: async (sql) => (/FROM sa_suggestions/.test(sql) ? rows.filter((r) => r.basis && ['taken', 'dismissed', 'stale'].includes(r.status)) : []),
      write: async (ops) => { for (const o of ops) { const r = rows.find((x) => x.id === o.key); if (r && o.op === 'set') r[o.field] = o.value; } },
    },
  };
  return { store, rows };
}

test('a refresh stores the best few with source, confidence and change; again, it adds nothing twice; a dismissed one stays away', async () => {
  const evs = [];
  for (let i = 0; i < 6; i++) evs.push({ kind: 'open', detail: 'orders', session: `s${i}`, ts: at(i * 1000) }, { kind: 'open', detail: 'stock', session: `s${i}`, ts: at(i * 1000 + 5) });
  const { store, rows } = memoryStore(VIEW, evs);
  const emitted = [];
  const rec = new Recommender({ engine: { store }, signalsOf: async (a, v, e) => signals(e, { views: v.views }), emit: (k, p) => emitted.push([k, p]) });
  const first = await rec.refresh('app1');
  assert.ok(first.added.length >= 1);
  const path = rows.find((r) => r.signal === 'path:orders>stock');
  assert.ok(path, JSON.stringify(rows));
  assert.equal(path.basis, 'use.path');
  assert.equal(path.source, 'use');
  assert.ok(path.confidence > 0);
  assert.equal(JSON.parse(path.implies).kind, 'link');
  assert.deepEqual(emitted[0], ['suggestions', { app: 'app1' }]);
  const again = await rec.refresh('app1');
  assert.equal(again.added.length, 0);
  assert.equal(rows.filter((r) => r.signal === 'path:orders>stock').length, 1);
  path.status = 'dismissed';
  await rec.refresh('app1');
  assert.equal(rows.filter((r) => r.signal === 'path:orders>stock').length, 1);
});

test('the model only rewords and orders, within its budget; when it fails the sources\' words stand', async () => {
  const evs = [];
  for (let i = 0; i < 6; i++) evs.push({ kind: 'open', detail: 'orders', session: `s${i}`, ts: at(i * 1000) }, { kind: 'open', detail: 'stock', session: `s${i}`, ts: at(i * 1000 + 5) });
  const asked = [];
  const ask = async (o) => { asked.push(o); return { text: '{"items": [{"id": "c0", "text": "Show what is left of each product on Orders"}]}' }; };
  const a = memoryStore(VIEW, evs);
  const rec = new Recommender({ engine: { store: a.store }, ask, model: 'small', signalsOf: async (x, v, e) => signals(e, { views: v.views }) });
  const out = await rec.refresh('app1');
  assert.equal(asked.length, 1);
  assert.match(asked[0].prompt, /went from Orders straight to Stock/);
  assert.equal(out.added[0].text, 'Show what is left of each product on Orders');
  const b = memoryStore(VIEW, evs);
  const failing = new Recommender({ engine: { store: b.store }, ask: async () => { throw new Error('no model'); }, model: 'small', signalsOf: async (x, v, e) => signals(e, { views: v.views }) });
  const kept = await failing.refresh('app1');
  assert.match(kept.added[0].text, /^On Orders/);
});

test('every source is general: none of them names a kind of app', () => {
  const src = readFileSync(join(import.meta.dirname, '..', 'engine', 'recsources.mjs'), 'utf8');
  assert.equal(SOURCES.length, 8);
  for (const w of ['invoice', 'recipe', 'booking', 'candle', 'salon', 'workout']) assert.ok(!new RegExp(w, 'i').test(src), w);
  assert.ok(wordsOf('Café orders').has('cafe'));
});

// ---- over the real engine, on a copy of built apps

const DATA = process.env.STRATA_APPS_RECOMMEND_DATA || '';

test('live: an app with use gets a suggestion from it, one without gets cold-start ones, and a dismissal is learned', async (t) => {
  const { findServerBinary } = await import('../engine/strata.mjs');
  const bin = findServerBinary(join(import.meta.dirname, '..', '..'));
  if (!bin || !DATA || !existsSync(join(DATA, 'apps.strata'))) return t.skip('needs chloedb-server and a copy of built apps (STRATA_APPS_RECOMMEND_DATA)');
  const dir = mkdtempSync('/tmp/sa-recommend-');
  cpSync(DATA, join(dir, 'data'), { recursive: true });
  const env = { ...process.env, STRATA_SERVER_BIN: bin, STRATA_APPS_SCHEDULES: 'off', STRATA_APPS_AUTOREPAIR: '0', STRATA_APPS_EVOLVE: 'off', STRATA_APPS_RECOMMEND_MODEL: 'off', STRATA_APPS_RECOMMEND_GAP_S: '0', STRATA_APPS_LOOK: 'off', STRATA_APPS_TILES: 'off', STRATA_APPS_GEOCODER: 'off' };
  const proc = spawn(process.execPath, [join(import.meta.dirname, '..', 'engine', 'server.mjs'), '--port', '8991', '--strata-listen', '127.0.0.1:7991', '--data', join(dir, 'data')], { env, stdio: ['ignore', 'pipe', 'pipe'] });
  let log = '';
  proc.stdout.on('data', (d) => { log += d; });
  proc.stderr.on('data', (d) => { log += d; });
  const base = 'http://127.0.0.1:8991';
  try {
    for (let i = 0; ; i++) {
      try { if ((await fetch(`${base}/healthz`)).ok) break; } catch { /* starting */ }
      if (i > 900 || proc.exitCode !== null) throw new Error(`the engine did not start:\n${log.slice(-2000)}`);
      await new Promise((r) => setTimeout(r, 100));
    }
    const call = async (path, body) => {
      const res = await fetch(`${base}${path}`, { method: body === undefined ? 'GET' : 'POST', headers: { 'content-type': 'application/json', origin: base, 'user-agent': 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) Mobile' }, body: body === undefined ? undefined : JSON.stringify(body) });
      const out = await res.json();
      assert.equal(res.status, 200, JSON.stringify(out));
      return out;
    };
    const apps = (await call('/api/apps')).apps;
    // The app with the most screens gets use; another with screens stays without.
    const withScreens = [];
    for (const a of apps.filter((x) => Number(x.head))) { const s = await call(`/api/apps/${a.id}`); if (s.views.length >= 2) withScreens.push(s); }
    assert.ok(withScreens.length >= 2, 'two built apps with two screens or more');
    const used = withScreens[0];
    const [a, b] = used.views.map((v) => v.slug);
    for (let i = 0; i < 6; i++) {
      await call(`/api/apps/${used.app.id}/track`, { kind: 'open', detail: a, session: `live${i}` });
      await call(`/api/apps/${used.app.id}/track`, { kind: 'open', detail: b, session: `live${i}` });
    }
    const p = await call(`/api/apps/${used.app.id}/recommend`);
    const fromUse = p.next.find((c) => c.key === `path:${a}>${b}`);
    assert.ok(fromUse, JSON.stringify({ next: p.next, open: (await call(`/api/apps/${used.app.id}`)).suggestions }));
    assert.equal(fromUse.source, 'use');
    // The visits came from a phone (its browser says so).
    assert.ok(p.candidates.some((c) => c.basis === 'use.phone'), 'phone visits counted');
    // Opening the app recommends in the background; the suggestions API
    // gives the same fields as before, and the new ones.
    let stored = null;
    for (let i = 0; i < 100 && !stored; i++) {
      stored = (await call(`/api/apps/${used.app.id}`)).suggestions.find((s) => s.signal === `path:${a}>${b}` && s.basis);
      if (!stored) await new Promise((r) => setTimeout(r, 200));
    }
    assert.ok(stored, 'the recommendation is stored');
    for (const k of ['id', 'text', 'why', 'source', 'status', 'signal', 'created', 'basis', 'confidence', 'change']) assert.ok(k in stored, k);
    assert.equal(stored.change.kind, 'link');
    const cold = withScreens.find((s) => s.app.id !== used.app.id);
    const c = await call(`/api/apps/${cold.app.id}/recommend`);
    assert.ok(c.next.some((x) => x.source === 'start'), JSON.stringify(c.candidates.slice(0, 6)));
    // Dismissing it is learned: its source weighs less for the next.
    await call(`/api/apps/${used.app.id}/suggestions/${stored.id}`, { status: 'dismissed' });
    const after = await call(`/api/apps/${used.app.id}/recommend`);
    assert.ok(!after.next.some((x) => x.key === stored.signal), 'a dismissed one is not offered again');
    const w = after.candidates.find((x) => x.basis === 'use.path');
    if (w) assert.ok(w.weight < 1, `use.path weighs ${w.weight} after a dismissal`);
    t.diagnostic(`with use (${used.app.name}): ${fromUse.text} | ${fromUse.why}`);
    t.diagnostic(`cold start (${cold.app.name}): ${c.next.map((x) => `[${x.basis}] ${x.text}`).join(' | ')}`);
  } finally {
    proc.kill('SIGTERM');
    await new Promise((r) => setTimeout(r, 500));
    rmSync(dir, { recursive: true, force: true });
  }
});

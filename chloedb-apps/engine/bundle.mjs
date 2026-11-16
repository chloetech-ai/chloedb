// An app carried from one engine to another: the engine where it is built
// (a laptop) packs its live version whole into a bundle (bundleApp), and the
// engine where it is served (a cluster) unpacks it (importBundle), as a new
// app the first time and as a new version of that app after that.
//
// A bundle holds what the app is made of and what it starts with:
//   - its pieces as they are stored (screens, functions, rules, tables as
//     SQL), and the shared pieces its version pins (components, helpers);
//   - its tables' rows, which seed a table the engine makes for it; a table
//     it already has keeps its own rows (they are the people's: their
//     sign-ups and orders), and gets only the columns it lacks, never fewer;
//   - its look (the three designs and the one chosen), the settings that
//     shape it (currency, locale, its members' preview, its name), and the
//     files its rows and screens name (their bytes).
// Not in it: who uses it (people, members, companies, sessions), its use,
// its builds and their cost, its secrets (sealed with the laptop's key: the
// owner gives them again where it is served), its publish choice and its key.

import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { physical } from './store.mjs';

export const BUNDLE = 1;
// The settings carried with an app (the rest stay with the engine).
const CARRIED = ['currency', 'locale', 'samples', 'named', 'reach'];
// At most this many rows a table and files an app, as duplicateApp copies.
const MAX_ROWS = 10000;
const MAX_FILES = 500;

const fail = (message, status = 400) => Object.assign(new Error(message), { status });
const CAP_FIELDS = ['slug', 'version', 'kind', 'app', 'title', 'description', 'code', 'spec', 'forked_from', 'author'];
const capOf = (r) => Object.fromEntries(CAP_FIELDS.filter((k) => r[k] !== undefined && r[k] !== null).map((k) => [k, r[k]]).concat([['id', r.id]]));

// The app's live version as a bundle. rows: false leaves its rows out; files:
// false its files' bytes (their names stay).
export async function bundleApp(engine, appId, { rows = true, files = true } = {}) {
  const { store } = engine;
  const view = await store.resolve(appId);
  if (!view.comp.n) throw fail('the app has no version yet: build it first');
  const caps = (await store.db.get('sa_capabilities', view.comp.caps)).filter(Boolean).map(capOf);
  const tables = {};
  if (rows) {
    for (const t of view.tables) {
      const vec = new Set((t.spec.columns || []).filter((c) => c.type === 'meaning').map((c) => c.name));
      const got = await store.db.query(`SELECT * FROM ${physical(appId, t.spec.name)} LIMIT ${MAX_ROWS}`).catch(() => []);
      tables[t.spec.name] = got.map((r) => Object.fromEntries(Object.entries(r).filter(([k, v]) => !vec.has(k) && k !== 'score' && v !== null)));
    }
  }
  const settings = {};
  for (const name of CARRIED) {
    const v = await store.setting(appId, name).catch(() => null);
    if (v !== null && v !== undefined) settings[name] = v;
  }
  const fileRows = await store.db.query('SELECT id, name, type, size, author FROM sa_files WHERE app = :a LIMIT :n', { a: appId, n: MAX_FILES }).catch(() => []);
  const dir = engine.services?.filesDir;
  const out = [];
  for (const f of fileRows) {
    const item = { id: f.id, name: f.name, type: f.type, size: Number(f.size) || 0, author: f.author || '' };
    if (files && dir) {
      const bytes = await readFile(path.join(dir, appId, f.id)).catch(() => null);
      if (bytes) item.data = bytes.toString('base64');
    }
    out.push(item);
  }
  return {
    bundle: BUNDLE,
    made: new Date().toISOString(),
    app: { id: appId, name: view.app.name, description: view.app.description || '', ...(view.app.ux ? { ux: view.app.ux } : {}) },
    version: { n: view.comp.n, request: view.comp.request || '', summary: view.comp.summary || '' },
    caps,
    tables,
    settings,
    designs: await store.designs(appId).catch(() => null),
    files: out,
  };
}

// What a bundle must hold before anything of it is written.
export function checkBundle(b) {
  if (!b || typeof b !== 'object' || b.bundle !== BUNDLE) throw fail(`not an app bundle (version ${BUNDLE})`);
  if (!b.app || typeof b.app.id !== 'string' || typeof b.app.name !== 'string') throw fail('the bundle names no app');
  if (!Array.isArray(b.caps) || !b.caps.length) throw fail('the bundle has no pieces');
  for (const c of b.caps) {
    if (!c || typeof c.id !== 'string' || typeof c.kind !== 'string' || typeof c.slug !== 'string' || typeof c.app !== 'string') throw fail('a piece of the bundle has no id, kind, slug or app');
    if (!/^[a-z][a-z0-9_-]{0,60}$/i.test(c.slug) || !/^[a-z]+$/.test(c.kind)) throw fail(`the piece ${String(c.id).slice(0, 80)} has an odd kind or slug`);
  }
  if (b.tables && (typeof b.tables !== 'object' || Array.isArray(b.tables))) throw fail('the bundle\'s rows are not by table');
  if (b.files && !Array.isArray(b.files)) throw fail('the bundle\'s files are not a list');
}

// Unpacks a bundle on this engine: a new app (into: null), or a new version
// of the app `into`. Its own pieces are written under the app here (one the
// live version already has, the same, is kept as it is); a shared piece
// this engine has with the same code is used as it is, one it lacks is
// added, one it has under that id with other code is added as the next
// version. Then its tables are made or given the columns they lack, a table
// made now gets the bundle's rows, the look, settings and files follow, and
// the version goes live. Returns {app, n, created, added, rows, files}.
export async function importBundle(engine, b, { into = null } = {}) {
  checkBundle(b);
  const { store } = engine;
  let target = into;
  if (target) {
    const app = await store.getApp(target);
    if (!app) throw fail(`no app ${target}`, 404);
    await store.updateApp(target, { name: String(b.app.name).slice(0, 60), description: String(b.app.description || '') });
  } else {
    target = await store.createApp({ name: String(b.app.name).slice(0, 60), description: String(b.app.description || '') });
  }
  if (b.app.ux) await store.updateApp(target, { ux: typeof b.app.ux === 'string' ? b.app.ux : JSON.stringify(b.app.ux) });

  // The pieces of the version live here now, by kind/slug.
  const now = await store.resolve(target);
  const live = new Map();
  if (now.comp.n) for (const r of (await store.db.get('sa_capabilities', now.comp.caps)).filter(Boolean)) live.set(`${r.kind}/${r.slug}`, r);
  const same = (x, y) => x.code === y.code && String(x.spec ?? '') === String(y.spec ?? '') && String(x.title ?? '') === String(y.title ?? '') && String(x.description ?? '') === String(y.description ?? '');
  const caps = [];
  const ops = [];
  const nextVersion = new Map();
  const next = async (app, kind, slug) => {
    const key = `${app}/${kind}/${slug}`;
    const v = nextVersion.get(key) || await store.nextVersion(app, kind, slug);
    nextVersion.set(key, v + 1);
    return v;
  };
  const doc = (r, id, app, version) => {
    const d = { ...capOf(r), id, app, version };
    if (typeof d.spec !== 'string') d.spec = JSON.stringify(d.spec ?? {});
    if (!d.forked_from) delete d.forked_from;
    return d;
  };
  for (const r of b.caps) {
    if (r.app === b.app.id) {
      const cur = live.get(`${r.kind}/${r.slug}`);
      if (cur && cur.app === target && same(cur, r)) { caps.push(cur.id); continue; }
      const version = await next(target, r.kind, r.slug);
      const id = `${target}/${r.kind}/${r.slug}@${version}`;
      ops.push({ op: 'insert', table: 'sa_capabilities', doc: doc(r, id, target, version) });
      caps.push(id);
    } else {
      const [mine] = await store.db.get('sa_capabilities', [r.id]);
      if (mine && same(mine, r)) { caps.push(r.id); continue; }
      if (!mine) { ops.push({ op: 'insert', table: 'sa_capabilities', doc: doc(r, r.id, r.app, Number(r.version) || 1) }); caps.push(r.id); continue; }
      // the same piece carried before under another version number (the
      // live version pins it, or another version of it says the same)
      const pinned = live.get(`${r.kind}/${r.slug}`);
      if (pinned && same(pinned, r)) { caps.push(pinned.id); continue; }
      const others = await store.db.query('SELECT id, code, spec, title, description FROM sa_capabilities WHERE app = :a AND kind = :k AND slug = :s ORDER BY version DESC LIMIT 50', { a: r.app, k: r.kind, s: r.slug }).catch(() => []);
      const twin = others.find((o) => same(o, r));
      if (twin) { caps.push(twin.id); continue; }
      const version = await next(r.app, r.kind, r.slug);
      const id = `${r.app}/${r.kind}/${r.slug}@${version}`;
      ops.push({ op: 'insert', table: 'sa_capabilities', doc: doc(r, id, r.app, version) });
      caps.push(id);
    }
  }
  for (let i = 0; i < ops.length; i += 200) await store.db.write(ops.slice(i, i + 200));

  // The tables of the version, as its pieces say: made, or given the
  // columns they lack (a links table after the table it links).
  const tables = (await store.getCapabilities(caps)).filter((c) => c && c.kind === 'table' && c.spec?.name);
  const existing = new Set(await store.tableNames());
  const created = [];
  const added = {};
  for (const t of [...tables].sort((x, y) => Number(!!x.spec.links) - Number(!!y.spec.links))) {
    const r = await store.ensureTable(target, t.spec, existing);
    if (r.created) created.push(t.spec.name);
    if (r.added.length) added[t.spec.name] = r.added;
  }
  // A table made now starts with the bundle's rows; one already here keeps its own.
  let rows = 0;
  for (const name of created) {
    const docs = Array.isArray(b.tables?.[name]) ? b.tables[name].slice(0, MAX_ROWS) : [];
    for (let i = 0; i < docs.length; i += 500) await store.db.write(docs.slice(i, i + 500).map((d) => ({ op: 'insert', table: physical(target, name), doc: d })));
    rows += docs.length;
  }

  if (b.designs && Array.isArray(b.designs.designs) && b.designs.designs.length) await store.putDesigns(target, b.designs);
  for (const [name, value] of Object.entries(b.settings || {})) if (CARRIED.includes(name)) await store.putSetting(target, name, value);

  // Its files, under the same ids (rows and screens name them), once.
  let files = 0;
  const dir = engine.services?.filesDir;
  if (dir) {
    for (const f of (b.files || []).slice(0, MAX_FILES)) {
      if (!f || typeof f.data !== 'string' || !/^[a-z0-9]{6,20}$/.test(String(f.id))) continue;
      const [had] = await store.db.get('sa_files', [f.id]);
      if (had) continue;
      const bytes = Buffer.from(f.data, 'base64');
      await mkdir(path.join(dir, target), { recursive: true });
      await writeFile(path.join(dir, target, f.id), bytes);
      const type = /^[a-z]+\/[a-z0-9.+-]+$/i.test(f.type || '') ? String(f.type).toLowerCase() : 'application/octet-stream';
      await store.db.write([{ op: 'insert', table: 'sa_files', doc: { id: f.id, app: target, name: String(f.name || 'file').slice(0, 200), type, size: bytes.length, author: String(f.author || '') } }]);
      files++;
    }
  }

  const n = await store.writeComposition(target, caps, `Published version ${b.version?.n ?? '?'}`, b.version?.summary || '');
  return { app: target, n, created, added, rows, files };
}

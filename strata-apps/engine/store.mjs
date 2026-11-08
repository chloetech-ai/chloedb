// Everything the engine knows lives in Strata: apps, the capabilities they are
// made of (code included), each app's versions (compositions), the requests
// that changed them, what people did, and the suggestions made to them. Each
// app's own data lives in tables named app_<app>_<table>.

import { randomBytes } from 'node:crypto';

export const TYPES = {
  text: 'TEXT',            // prose people search by words
  keyword: 'KEYWORD',      // short labels: status, owner, category, email; filter, group and sort by them
  'keyword[]': 'KEYWORD[]',// tags
  int: 'INT',
  float: 'FLOAT',
  bool: 'BOOL',
  timestamp: 'TIMESTAMP',  // ISO 8601 text in and out
};

const SYSTEM_TABLES = {
  sa_apps: `CREATE TABLE sa_apps (id KEYWORD PRIMARY KEY, name TEXT, description TEXT, head INT DEFAULT 0, created TIMESTAMP DEFAULT CURRENT_TIMESTAMP)`,
  sa_capabilities: `CREATE TABLE sa_capabilities (id KEYWORD PRIMARY KEY, slug KEYWORD NOT NULL, version INT NOT NULL,
    kind KEYWORD NOT NULL, app KEYWORD, title TEXT, description TEXT, code TEXT, spec TEXT,
    forked_from KEYWORD, author KEYWORD, created TIMESTAMP DEFAULT CURRENT_TIMESTAMP)`,
  sa_compositions: `CREATE TABLE sa_compositions (id KEYWORD PRIMARY KEY, app KEYWORD NOT NULL, n INT NOT NULL,
    caps KEYWORD[], request TEXT, summary TEXT, created TIMESTAMP DEFAULT CURRENT_TIMESTAMP)`,
  sa_requests: `CREATE TABLE sa_requests (id KEYWORD PRIMARY KEY, app KEYWORD NOT NULL, text TEXT, source KEYWORD,
    status KEYWORD, comp INT, summary TEXT, log TEXT, error TEXT, ms INT, cost FLOAT, created TIMESTAMP DEFAULT CURRENT_TIMESTAMP)`,
  sa_events: `CREATE TABLE sa_events (id KEYWORD PRIMARY KEY, app KEYWORD NOT NULL, kind KEYWORD, cap KEYWORD,
    detail TEXT, ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP)`,
  sa_suggestions: `CREATE TABLE sa_suggestions (id KEYWORD PRIMARY KEY, app KEYWORD NOT NULL, text TEXT, why TEXT,
    source KEYWORD, cap KEYWORD, status KEYWORD, created TIMESTAMP DEFAULT CURRENT_TIMESTAMP)`,
};

export const newId = (n = 8) => randomBytes(n).toString('base64url').replace(/[^a-zA-Z0-9]/g, '').slice(0, n).toLowerCase().padEnd(n, '0');
export const physical = (app, table) => `app_${app}_${table}`;
export const NAME_RE = /^[a-z][a-z0-9_]{0,30}$/;
export const SLUG_RE = /^[a-z][a-z0-9-]{0,40}$/;

const tableName = (t) => t.table || t.schema?.table || t.name;
const parseSpec = (s) => { try { return s ? JSON.parse(s) : {}; } catch { return {}; } };

export class Store {
  constructor(strata, log = () => {}) {
    this.db = strata;
    this.log = log;
    this.resolved = new Map(); // app -> {head, view}
    this.capCache = new Map();  // capability id -> row (capabilities never change once written)
  }

  async init() {
    const have = new Set((await this.db.tables()).map(tableName).filter(Boolean));
    for (const [name, sql] of Object.entries(SYSTEM_TABLES)) {
      if (!have.has(name)) {
        await this.db.sql(sql);
        this.log(`created ${name}`);
      }
    }
  }

  async tableNames() {
    const tables = await this.db.tables();
    return tables.map(tableName).filter(Boolean);
  }

  // ---- apps

  async listApps() {
    return this.db.query('SELECT id, name, description, head, created FROM sa_apps ORDER BY created DESC LIMIT 200');
  }

  async getApp(id) {
    const [row] = await this.db.get('sa_apps', [id]);
    return row || null;
  }

  async createApp({ name, description }) {
    const id = newId(6);
    await this.db.write([{ op: 'insert', table: 'sa_apps', doc: { id, name: name || 'New app', description: description || '', head: 0 } }]);
    return id;
  }

  async updateApp(id, fields) {
    const app = await this.getApp(id);
    if (!app) throw new Error(`no app ${id}`);
    const doc = { ...app, ...fields };
    delete doc.created;
    await this.db.write([{ op: 'upsert', table: 'sa_apps', doc: { ...doc, created: app.created } }]);
    this.resolved.delete(id);
  }

  // ---- capabilities

  async getCapabilities(ids) {
    const missing = ids.filter((id) => !this.capCache.has(id));
    if (missing.length) {
      const rows = await this.db.get('sa_capabilities', missing);
      rows.forEach((r, i) => { if (r) this.capCache.set(missing[i], { ...r, spec: parseSpec(r.spec) }); });
    }
    return ids.map((id) => this.capCache.get(id)).filter(Boolean);
  }

  async nextVersion(app, kind, slug) {
    const rows = await this.db.query('SELECT version FROM sa_capabilities WHERE app = :a AND kind = :k AND slug = :s ORDER BY version DESC LIMIT 1', { a: app, k: kind, s: slug });
    return rows.length ? Number(rows[0].version) + 1 : 1;
  }

  // A capability is never changed once written: a change is a new version,
  // with its own id, `app/kind/slug@version`.
  async putCapability({ app, slug, kind, title, description, code, spec, forked_from, author }) {
    const version = await this.nextVersion(app, kind, slug);
    const id = `${app}/${kind}/${slug}@${version}`;
    const doc = { id, slug, version, kind, app, title: title || slug, description: description || '', code: code || '',
      spec: JSON.stringify(spec || {}), forked_from: forked_from || null, author: author || 'forge' };
    if (!doc.forked_from) delete doc.forked_from;
    await this.db.write([{ op: 'insert', table: 'sa_capabilities', doc }]);
    this.capCache.set(id, { ...doc, spec: spec || {} });
    return id;
  }

  // BM25 over what capabilities are and do, and over their code.
  async searchCapabilities(text, { excludeApp, kinds, limit = 6 } = {}) {
    const words = String(text || '').replace(/[^\p{L}\p{N}\s]/gu, ' ').trim();
    if (!words) return [];
    const where = [];
    const params = { t: words };
    if (excludeApp) { where.push('app != :x'); params.x = excludeApp; }
    if (kinds?.length) { where.push(`kind IN (${kinds.map((k, i) => `:k${i}`).join(', ')})`); kinds.forEach((k, i) => { params[`k${i}`] = k; }); }
    const sql = `SELECT id, slug, version, kind, app, title, description, spec, score() AS score FROM sa_capabilities
      ${where.length ? 'WHERE ' + where.join(' AND ') : ''}
      ORDER BY bm25(title, :t) * 2 + bm25(description, :t) + 0.3 * bm25(code, :t) DESC LIMIT ${limit * 4}`;
    let rows;
    try { rows = await this.db.query(sql, params); } catch (e) { this.log(`search: ${e.message}`); return []; }
    // Keep the newest version of each app's slug.
    const seen = new Set();
    const out = [];
    for (const r of rows) {
      if (!(Number(r.score) > 0)) continue;
      const key = `${r.app}/${r.kind}/${r.slug}`;
      if (seen.has(key)) continue;
      seen.add(key);
      out.push({ ...r, spec: parseSpec(r.spec) });
      if (out.length >= limit) break;
    }
    return out;
  }

  async allCapabilities(limit = 300) {
    const rows = await this.db.query(`SELECT id, slug, version, kind, app, title, description, forked_from, author, created FROM sa_capabilities ORDER BY created DESC LIMIT ${limit}`);
    return rows;
  }

  // ---- compositions: an app's versions

  async compositions(app) {
    return this.db.query('SELECT id, n, caps, request, summary, created FROM sa_compositions WHERE app = :a ORDER BY n DESC LIMIT 100', { a: app });
  }

  async composition(app, n) {
    if (!n) return { n: 0, caps: [] };
    const [row] = await this.db.get('sa_compositions', [`${app}#${n}`]);
    return row ? { ...row, caps: row.caps || [] } : { n: 0, caps: [] };
  }

  async writeComposition(app, caps, request, summary) {
    const rows = await this.db.query('SELECT n FROM sa_compositions WHERE app = :a ORDER BY n DESC LIMIT 1', { a: app });
    const n = rows.length ? Number(rows[0].n) + 1 : 1;
    await this.db.write([
      { op: 'insert', table: 'sa_compositions', doc: { id: `${app}#${n}`, app, n, caps, request: request || '', summary: summary || '' } },
    ]);
    await this.updateApp(app, { head: n });
    return n;
  }

  async setHead(app, n) {
    await this.composition(app, n); // exists?
    await this.updateApp(app, { head: n });
  }

  // The app as it runs now: its head composition, with every capability.
  async resolve(appId) {
    const app = await this.getApp(appId);
    if (!app) throw new Error(`no app ${appId}`);
    const cached = this.resolved.get(appId);
    if (cached && cached.head === app.head) return cached.view;
    const comp = await this.composition(appId, app.head);
    const caps = await this.getCapabilities(comp.caps);
    const view = {
      app, comp, caps,
      tables: caps.filter((c) => c.kind === 'table'),
      views: caps.filter((c) => c.kind === 'view'),
      functions: caps.filter((c) => c.kind === 'function'),
    };
    this.resolved.set(appId, { head: app.head, view });
    return view;
  }

  forget(appId) { this.resolved.delete(appId); }

  // ---- each app's data tables

  async ensureTable(appId, spec, existing) {
    const name = physical(appId, spec.name);
    const cols = spec.columns || [];
    if (!existing.has(name)) {
      const defs = ['id KEYWORD PRIMARY KEY'];
      for (const c of cols) defs.push(`${c.name} ${TYPES[c.type]}`);
      defs.push('created TIMESTAMP DEFAULT CURRENT_TIMESTAMP');
      await this.db.sql(`CREATE TABLE ${name} (${defs.join(', ')})`);
      existing.add(name);
      return { created: name, added: [] };
    }
    const tables = await this.db.tables();
    const t = tables.find((x) => tableName(x) === name);
    const have = new Set((t?.schema?.fields || []).map((f) => f.name));
    const added = [];
    for (const c of cols) {
      if (!have.has(c.name)) {
        await this.db.sql(`ALTER TABLE ${name} ADD COLUMN ${c.name} ${TYPES[c.type]}`);
        added.push(c.name);
      }
    }
    return { created: null, added };
  }

  async countRows(table) {
    try {
      const rows = await this.db.query(`SELECT COUNT(*) AS n FROM ${table}`);
      return Number(rows[0]?.n || 0);
    } catch { return 0; }
  }

  // ---- requests, events, suggestions

  async putRequest(doc) {
    await this.db.write([{ op: 'upsert', table: 'sa_requests', doc }]);
  }

  async requests(app, limit = 50) {
    return this.db.query(`SELECT id, text, source, status, comp, summary, error, ms, cost, created FROM sa_requests WHERE app = :a ORDER BY created DESC LIMIT ${limit}`, { a: app });
  }

  async track(app, kind, cap, detail) {
    await this.db.write([{ op: 'insert', table: 'sa_events', doc: { id: newId(12), app, kind, cap: cap || '', detail: detail ? String(detail).slice(0, 500) : '' } }]);
  }

  async recentEvents(app, limit = 200) {
    return this.db.query(`SELECT kind, cap, detail, ts FROM sa_events WHERE app = :a ORDER BY ts DESC LIMIT ${limit}`, { a: app });
  }

  async putSuggestions(app, items) {
    if (!items.length) return;
    await this.db.write(items.map((s) => ({ op: 'insert', table: 'sa_suggestions', doc: { id: newId(10), app, status: 'open', ...s } })));
  }

  async suggestions(app) {
    return this.db.query(`SELECT id, text, why, source, cap, status, created FROM sa_suggestions WHERE app = :a AND status = 'open' ORDER BY created DESC LIMIT 12`, { a: app });
  }

  async setSuggestion(id, status) {
    const [row] = await this.db.get('sa_suggestions', [id]);
    if (row) await this.db.write([{ op: 'upsert', table: 'sa_suggestions', doc: { ...row, status } }]);
    return row;
  }
}

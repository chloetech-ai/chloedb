// The runtime: what views and functions can do to an app's data, the
// triggers that run functions after writes, what people did (events), and
// the announcements that keep every open window live.

import { EventEmitter } from 'node:events';
import { Store, physical, newId } from './store.mjs';
import { rewriteSql } from './sql.mjs';
import { runFunction } from './sandbox.mjs';

const MAX_ROWS = 1000;
const MAX_DEPTH = 3;

function coerce(type, v, col) {
  if (v === null || v === undefined || v === '') return null;
  switch (type) {
    case 'int': {
      const n = typeof v === 'boolean' ? Number(v) : Number(v);
      if (!Number.isFinite(n)) throw new Error(`${col} takes a whole number, not ${JSON.stringify(v)}`);
      return Math.round(n);
    }
    case 'float': {
      const n = Number(v);
      if (!Number.isFinite(n)) throw new Error(`${col} takes a number, not ${JSON.stringify(v)}`);
      return n;
    }
    case 'bool':
      if (typeof v === 'boolean') return v;
      if (v === 1 || v === '1' || v === 'true' || v === 'yes' || v === 'on') return true;
      if (v === 0 || v === '0' || v === 'false' || v === 'no' || v === 'off') return false;
      throw new Error(`${col} takes true or false, not ${JSON.stringify(v)}`);
    case 'timestamp': {
      const d = typeof v === 'number' ? new Date(v) : new Date(String(v));
      if (Number.isNaN(d.getTime())) throw new Error(`${col} takes a time (ISO 8601), not ${JSON.stringify(v)}`);
      return d.toISOString();
    }
    case 'keyword[]': {
      const list = Array.isArray(v) ? v : String(v).split(',');
      return list.map((x) => String(x).trim()).filter(Boolean);
    }
    case 'keyword':
      return String(v).slice(0, 1000);
    default:
      return typeof v === 'string' ? v : (typeof v === 'object' ? JSON.stringify(v) : String(v));
  }
}

export class Engine extends EventEmitter {
  constructor(strata, { log = console.log } = {}) {
    super();
    this.setMaxListeners(1000);
    this.db = strata;
    this.store = new Store(strata, log);
    this.log = log;
    this.feed = false;
  }

  async init() {
    await this.store.init();
    // Strata's feed of committed transactions is how every window learns of
    // a change, whoever made it: this engine, another engine on the same
    // database, or a client writing to Strata directly.
    this.feed = await this.db.follow((txn) => this.onTxn(txn), this.log);
    this.log(this.feed ? 'following Strata\'s feed of changes' : 'Strata serves no feed here: the engine announces its own writes');
  }

  onTxn(txn) {
    const data = new Map();
    for (const op of txn.ops || []) {
      const t = op.table || '';
      const m = t.match(/^app_([a-z0-9]{6})_(.+)$/);
      if (m) {
        if (!data.has(m[1])) data.set(m[1], new Set());
        data.get(m[1]).add(m[2]);
      } else if (t === 'sa_apps' && op.doc?.id) {
        this.store.forget(op.doc.id);
        this.emit('app', { app: op.doc.id, head: op.doc.head });
      } else if (t === 'sa_suggestions' && op.doc?.app) {
        this.emit('suggestions', { app: op.doc.app });
      } else if (t === 'sa_requests' && op.doc?.app) {
        this.emit('request', { app: op.doc.app, id: op.doc.id, status: op.doc.status });
      }
    }
    for (const [app, tables] of data) this.emit('data', { app, tables: [...tables] });
  }

  // Without a feed, writes made here announce themselves.
  announce(kind, payload) {
    if (!this.feed) this.emit(kind, payload);
  }

  async tableSpec(appId, table) {
    const view = await this.store.resolve(appId);
    const cap = view.tables.find((t) => t.spec.name === table);
    if (!cap) {
      const names = view.tables.map((t) => t.spec.name).join(', ') || 'none yet';
      throw new Error(`no table ${table} in this app (its tables: ${names})`);
    }
    return cap.spec;
  }

  makeDoc(spec, fields, { partial = false } = {}) {
    const types = new Map((spec.columns || []).map((c) => [c.name, c.type]));
    const doc = {};
    for (const [k, v] of Object.entries(fields || {})) {
      if (k === 'id' || k === 'created') continue;
      if (!types.has(k)) {
        throw new Error(`table ${spec.name} has no column ${k} (its columns: id, created, ${[...types.keys()].join(', ')})`);
      }
      doc[k] = coerce(types.get(k), v, `${spec.name}.${k}`);
    }
    if (!partial) for (const k of types.keys()) if (!(k in doc)) doc[k] = null;
    return doc;
  }

  // ---- what views and functions call

  async sql(appId, sql, params) {
    const view = await this.store.resolve(appId);
    const names = view.tables.map((t) => t.spec.name);
    const { sql: phys } = rewriteSql(sql, names, (t) => physical(appId, t));
    let rows = await this.db.query(phys, params || {});
    if (rows.length > MAX_ROWS) rows = rows.slice(0, MAX_ROWS);
    // Strata names a qualified column as written (`t.title`); code expects `title`.
    return rows.map((r) => {
      let out = r;
      for (const k of Object.keys(r)) {
        const i = k.lastIndexOf('.');
        if (i > 0) {
          const short = k.slice(i + 1);
          if (!(short in r)) {
            if (out === r) out = { ...r };
            out[short] = r[k];
            delete out[k];
          }
        }
      }
      return out;
    });
  }

  async insert(appId, table, fields, ctx = {}) {
    const spec = await this.tableSpec(appId, table);
    const doc = this.makeDoc(spec, fields);
    for (const k of Object.keys(doc)) if (doc[k] === null) delete doc[k];
    doc.id = fields?.id ? String(fields.id) : newId(10);
    if (fields?.created) doc.created = coerce('timestamp', fields.created, 'created');
    await this.db.write([{ op: 'insert', table: physical(appId, table), doc }]);
    const [row] = await this.db.get(physical(appId, table), [doc.id]);
    this.announce('data', { app: appId, tables: [table] });
    await this.after(appId, table, 'insert', row || doc, ctx);
    return row || doc;
  }

  async update(appId, table, id, fields, ctx = {}) {
    const spec = await this.tableSpec(appId, table);
    const [old] = await this.db.get(physical(appId, table), [String(id)]);
    if (!old) throw new Error(`no row ${id} in ${table}`);
    const changes = this.makeDoc(spec, fields, { partial: true });
    const doc = { ...old, ...changes, id: old.id };
    for (const k of Object.keys(doc)) if (doc[k] === null) delete doc[k];
    await this.db.write([{ op: 'upsert', table: physical(appId, table), doc }]);
    this.announce('data', { app: appId, tables: [table] });
    await this.after(appId, table, 'update', doc, { ...ctx, changed: Object.keys(changes) });
    return doc;
  }

  async remove(appId, table, id, ctx = {}) {
    await this.tableSpec(appId, table);
    const [old] = await this.db.get(physical(appId, table), [String(id)]);
    await this.db.write([{ op: 'delete', table: physical(appId, table), key: String(id) }]);
    this.announce('data', { app: appId, tables: [table] });
    if (old) await this.after(appId, table, 'delete', old, ctx);
    return { removed: old ? 1 : 0 };
  }

  async call(appId, slug, input, ctx = {}) {
    const view = await this.store.resolve(appId);
    const fn = view.functions.find((f) => f.slug === slug);
    if (!fn) {
      const names = view.functions.map((f) => f.slug).join(', ') || 'none';
      throw new Error(`no function ${slug} in this app (its functions: ${names})`);
    }
    const depth = (ctx.depth || 0) + 1;
    if (depth > MAX_DEPTH + 1) throw new Error(`functions called each other more than ${MAX_DEPTH} deep`);
    const inner = { depth, by: fn.id };
    const host = async (method, args) => {
      switch (method) {
        case 'sql': return this.sql(appId, args[0], args[1]);
        case 'insert': return this.insert(appId, args[0], args[1], inner);
        case 'update': return this.update(appId, args[0], args[1], args[2], inner);
        case 'remove': return this.remove(appId, args[0], args[1], inner);
        case 'call': return this.call(appId, args[0], args[1], inner);
        default: throw new Error(`no app.${method}`);
      }
    };
    try {
      const out = await runFunction(fn.code, input, host);
      if (!ctx.depth) this.store.track(appId, 'call', fn.id, slug).catch(() => {});
      return out;
    } catch (e) {
      this.reportError(appId, fn.id, e.message, ctx.trigger ? `trigger on ${ctx.trigger}` : 'call');
      throw e;
    }
  }

  // Tracks the write and runs the functions that wait for it.
  async after(appId, table, event, row, ctx) {
    if (!ctx.depth) {
      const detail = event === 'update' && ctx.changed ? `${table}: ${ctx.changed.join(', ')}` : table;
      this.store.track(appId, event, '', detail).catch(() => {});
    }
    if ((ctx.depth || 0) >= MAX_DEPTH) return;
    const view = await this.store.resolve(appId);
    for (const fn of view.functions) {
      const on = fn.spec.on;
      if (!on || on.table !== table || (on.event || 'insert') !== event) continue;
      if (fn.id === ctx.by) continue;
      // A trigger's failure is the function's, not the write's: the write stands.
      this.call(appId, fn.slug, { row, table, event }, { depth: ctx.depth || 0, trigger: `${event} ${table}` })
        .catch((e) => this.log(`trigger ${fn.id}: ${e.message}`));
    }
  }

  reportError(appId, capId, message, where) {
    this.store.track(appId, 'error', capId, `${where}: ${message}`).catch(() => {});
    this.emit('error', { app: appId, cap: capId, message, where });
  }
}

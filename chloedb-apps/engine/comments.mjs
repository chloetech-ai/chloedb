// Notes on an app's screens: the people building it leave a note on a spot
// of a screen (a route, an element's selector, the words it shows and where
// on it), reply in a thread, and resolve it. Kept in sa_comments, one row
// per note or reply; the anchor is the client's to read back.

import { newId } from './store.mjs';

const err = (message, status = 400) => Object.assign(new Error(message), { status });
const ANCHOR_KEYS = { route: 300, selector: 2048, tagName: 40, textSnippet: 200, frame: 10, screen: 80 };

// What a note's anchor may hold: short strings and two fractions, nothing else.
export function cleanAnchor(a = {}) {
  const out = {};
  for (const [k, n] of Object.entries(ANCHOR_KEYS)) if (a[k] !== undefined && a[k] !== null && a[k] !== '') out[k] = String(a[k]).slice(0, n);
  for (const k of ['nx', 'ny']) if (Number.isFinite(Number(a[k]))) out[k] = Math.max(0, Math.min(1, Number(a[k])));
  return out;
}

export function cleanBody(body) {
  const b = String(body ?? '').trim();
  if (!b) throw err('write the note');
  if (b.length > 4000) throw err('a note is at most 4000 characters');
  return b;
}

// A row as clients read it.
export function shapeComment(r) {
  let anchor = {};
  try { anchor = r.anchor ? JSON.parse(r.anchor) : {}; } catch { anchor = {}; }
  return { id: r.id, parentId: r.parent || null, body: r.body, status: r.status || 'open', author: { name: r.author_name || '', id: r.author || null }, createdAt: r.created || null, resolvedAt: r.resolved || null, ...anchor };
}

export class Comments {
  constructor(db) { this.db = db; }

  async list(appId, { status = null } = {}) {
    const rows = await this.db.query('SELECT * FROM sa_comments WHERE app = :a ORDER BY created LIMIT 2000', { a: appId });
    const shaped = rows.map(shapeComment);
    if (!status) return shaped;
    // A root's status decides its thread: replies of an open note are listed with it.
    const roots = new Map(shaped.filter((c) => !c.parentId).map((c) => [c.id, c.status]));
    return shaped.filter((c) => (c.parentId ? roots.get(c.parentId) : c.status) === status);
  }

  async row(appId, id) {
    const [r] = await this.db.get('sa_comments', [String(id || '')]);
    if (!r || r.app !== appId) throw err('no such note on this app', 404);
    return r;
  }

  async add(appId, { body, anchor, parent = null } = {}, author = {}) {
    const doc = { id: newId(12), app: appId, parent: parent || '', body: cleanBody(body), anchor: parent ? '' : JSON.stringify(cleanAnchor(anchor || {})), status: 'open',
      author: String(author.id || '').slice(0, 40), author_name: String(author.name || '').slice(0, 80), created: new Date().toISOString() };
    await this.db.write([{ op: 'insert', table: 'sa_comments', doc }]);
    return shapeComment(doc);
  }

  async reply(appId, id, body, author) {
    const root = await this.row(appId, id);
    if (root.parent) throw err('reply to the note, not to a reply');
    return this.add(appId, { body, parent: root.id }, author);
  }

  async resolve(appId, id, { reopen = false } = {}) {
    const r = await this.row(appId, id);
    if (r.parent) throw err('resolve the note, not a reply');
    const doc = { ...r, status: reopen ? 'open' : 'resolved', resolved: reopen ? null : new Date().toISOString() };
    await this.db.write([{ op: 'upsert', table: 'sa_comments', doc }]);
    return shapeComment(doc);
  }

  // A note goes with its replies; a reply alone.
  async remove(appId, id) {
    const r = await this.row(appId, id);
    const replies = r.parent ? [] : await this.db.query('SELECT id FROM sa_comments WHERE app = :a AND parent = :p LIMIT 2000', { a: appId, p: r.id });
    await this.db.write([r, ...replies].map((x) => ({ op: 'delete', table: 'sa_comments', key: x.id })));
    return { removed: r.id, replies: replies.length };
  }
}

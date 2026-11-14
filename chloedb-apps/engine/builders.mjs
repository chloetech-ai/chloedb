// An app's builders: the people who work on the app in the studio beside its
// owner, each with a role. Not the app's own people (services.mjs,
// sa_members), who use the app; these change it.
//
// - viewer: opens the app in the studio and reads it (screens, data,
//   versions), changes nothing;
// - editor: changes it as the owner does (requests, screens, designs, data,
//   versions, publishing), but does not decide who builds it or delete it;
// - admin: an editor who also invites, changes and removes builders.
//
// The owner is whoever reaches the studio without a builder key (the studio
// on its own machine, or behind login.mjs/callers.mjs as before): nothing
// changes for them. Someone invited gets a link with a single-use token; the
// link, redeemed, gives them a builder key of their own (x-sa-builder header
// or the sa_builder cookie), and every request with that key is held to the
// key's app and role here. Only hashes of tokens and keys are kept.

import { randomBytes, createHash } from 'node:crypto';
import { newId } from './store.mjs';

export const BUILDER_ROLES = ['viewer', 'editor', 'admin'];
const RANK = { viewer: 1, editor: 2, admin: 3, owner: 4 };
export const INVITE_DAYS = 14;
export const MAX_INVITES = 50;
const EMAIL_RE = /^[^\s@<>()",;]+@[^\s@<>()",;]+\.[^\s@<>()",;]+$/;
const DAY = 86400000;
const err = (message, status = 400) => Object.assign(new Error(message), { status });
const hash = (s) => createHash('sha256').update(String(s)).digest('hex');
const secret = () => randomBytes(24).toString('base64url');

export const COOKIE = 'sa_builder';

// The builder key a request carries, or null.
export function keyOf(req) {
  const h = req.headers['x-sa-builder'];
  if (typeof h === 'string' && /^[A-Za-z0-9_-]{20,80}$/.test(h)) return h;
  const m = String(req.headers.cookie || '').match(/(?:^|;\s*)sa_builder=([A-Za-z0-9_-]{20,80})/);
  return m ? m[1] : null;
}

// What the app's people did and what it holds for them (their activity, the
// mail sent them, their payments, the app's keys): for its owner and admins.
const ADMIN_READS = new Set(['activity', 'outbox', 'payments', 'secrets']);
// Studio POSTs that only read (a query, file names, usage counted).
const READS = new Set(['sql', 'files/info', 'track', 'error']);
// Routes of no one app that a builder key may call.
const OPEN = [/^\/api\/status$/, /^\/api\/kit$/, /^\/api\/library$/, /^\/api\/geocode$/, /^\/api\/apps$/, /^\/api\/events$/, /^\/api\/stream$/, /^\/app\.json$/, /^\/api\/tiles\//, /^\/fonts\//];

// Why a builder may not make this request (a 403's message), or null. `b` is
// { app, role } of the key; null is the owner, who may do anything.
export function builderRefusal(b, method, pathname) {
  if (!b) return null;
  const get = method === 'GET' || method === 'HEAD';
  if (pathname === '/api/builders/join' || pathname === '/api/signout') return null;
  const m = pathname.match(/^\/api\/apps\/([a-z0-9]+)(?:\/(.*))?$/);
  if (m) {
    if (m[1] !== b.app) return 'this builder key is for another app';
    const rest = m[2] || '';
    if (ADMIN_READS.has(rest) && RANK[b.role] < RANK.admin) return 'only the app\'s owner and admins see this';
    if (get) return null;
    if (READS.has(rest)) return null;
    if (b.role === 'viewer') return 'a viewer can look at this app but not change it; ask its owner to make you an editor';
    if (/^builders(\/|$)/.test(rest) && RANK[b.role] < RANK.admin) return 'only the app\'s owner and admins decide who builds it';
    if (rest === 'delete' || rest === 'duplicate') return 'only the app\'s owner can delete or copy it';
    return null;
  }
  if (get && (pathname === '/' || OPEN.some((re) => re.test(pathname)) || !pathname.startsWith('/api/'))) return null;
  return 'a builder key is for one app; building a new app needs the studio\'s owner';
}

// The invite input: addresses (one string or a list), deduplicated, and the
// ones that are not addresses, apart.
export function inviteList(emails) {
  const raw = (Array.isArray(emails) ? emails : String(emails || '').split(/[\s,;]+/)).map((e) => String(e || '').trim().toLowerCase()).filter(Boolean);
  const valid = [];
  const invalid = [];
  for (const e of raw) {
    if (!EMAIL_RE.test(e) || e.length > 200) { if (!invalid.includes(e)) invalid.push(e); }
    else if (!valid.includes(e)) valid.push(e);
  }
  return { valid: valid.slice(0, MAX_INVITES), invalid, unprocessed: valid.slice(MAX_INVITES) };
}

export function assertRole(role) {
  const r = String(role || '').toLowerCase();
  if (!BUILDER_ROLES.includes(r)) throw err('a builder\'s role is viewer, editor or admin');
  return r;
}

// What a builder row shows (never its hashes).
export function shapeBuilder(row, now = Date.now()) {
  const joined = row.status === 'joined';
  const expires = row.expires ? new Date(row.expires).getTime() : null;
  return {
    id: row.id, email: row.email || '', name: row.name || '', role: row.role,
    status: joined ? 'joined' : (expires && expires < now ? 'expired' : 'invited'),
    invited: row.invited || null, joined: row.joined || null, expires: joined ? null : (row.expires || null),
  };
}

export class Builders {
  constructor(db) { this.db = db; }

  async list(appId) {
    const rows = await this.db.query('SELECT * FROM sa_builders WHERE app = :a ORDER BY invited LIMIT 500', { a: appId });
    return rows.map((r) => shapeBuilder(r));
  }

  async row(appId, id) {
    const [r] = await this.db.get('sa_builders', [String(id || '')]);
    if (!r || r.app !== appId) throw err('no such builder on this app', 404);
    return r;
  }

  // Invite addresses with one role. Someone already invited or joined keeps
  // their row: their role changes, and an invitation not yet taken gets a
  // fresh link. Each result carries the link to hand over (the engine sends
  // no email for the studio).
  async invite(appId, emails, role, { base = '' } = {}) {
    const r = assertRole(role);
    const { valid, invalid, unprocessed } = inviteList(emails);
    if (!valid.length && !invalid.length) throw err('say who to invite: one or more email addresses');
    const existing = valid.length ? await this.db.query('SELECT * FROM sa_builders WHERE app = :a LIMIT 500', { a: appId }) : [];
    const writes = [];
    const results = [];
    const expires = new Date(Date.now() + INVITE_DAYS * DAY).toISOString();
    for (const email of valid) {
      const had = existing.find((x) => x.email === email);
      if (had && had.status === 'joined') {
        writes.push({ op: 'upsert', table: 'sa_builders', doc: { ...had, role: r } });
        results.push({ email, status: 'already', id: had.id, role: r });
        continue;
      }
      const token = secret();
      const doc = { ...(had || {}), id: had?.id || newId(12), app: appId, email, name: had?.name || '', role: r, status: 'invited', token_hash: hash(token), key_hash: '', expires, invited: had?.invited || new Date().toISOString() };
      writes.push({ op: 'upsert', table: 'sa_builders', doc });
      results.push({ email, status: 'invited', id: doc.id, role: r, link: joinLink(base, token), expires });
    }
    if (writes.length) await this.db.write(writes);
    return { results, invalid, unprocessed };
  }

  // A fresh link for an invitation not yet taken; the old one stops working.
  async link(appId, id, { base = '' } = {}) {
    const row = await this.row(appId, id);
    if (row.status === 'joined') throw err('this builder has already joined');
    const token = secret();
    const expires = new Date(Date.now() + INVITE_DAYS * DAY).toISOString();
    await this.db.write([{ op: 'upsert', table: 'sa_builders', doc: { ...row, token_hash: hash(token), expires } }]);
    return { link: joinLink(base, token), email: row.email, expires };
  }

  async setRole(appId, id, role) {
    const r = assertRole(role);
    const row = await this.row(appId, id);
    await this.db.write([{ op: 'upsert', table: 'sa_builders', doc: { ...row, role: r } }]);
    return shapeBuilder({ ...row, role: r });
  }

  // Revoking an invitation or removing someone who joined: their link and
  // their key stop working at once.
  async remove(appId, id) {
    const row = await this.row(appId, id);
    await this.db.write([{ op: 'delete', table: 'sa_builders', key: row.id }]);
    return { removed: row.id };
  }

  // An invitation taken: the token is spent, and the builder gets a key.
  async join(token, { name = '' } = {}) {
    if (typeof token !== 'string' || !/^[A-Za-z0-9_-]{20,80}$/.test(token)) throw err('this invitation link is not valid', 404);
    const [row] = await this.db.query('SELECT * FROM sa_builders WHERE token_hash = :h LIMIT 1', { h: hash(token) });
    if (!row) throw err('this invitation link is not valid any more; ask for a new one', 404);
    if (row.expires && new Date(row.expires).getTime() < Date.now()) throw err('this invitation link has expired; ask for a new one', 410);
    const key = secret();
    const doc = { ...row, status: 'joined', token_hash: '', key_hash: hash(key), expires: null, joined: new Date().toISOString(), name: String(name || row.name || '').trim().slice(0, 80) };
    await this.db.write([{ op: 'upsert', table: 'sa_builders', doc }]);
    return { key, app: row.app, role: row.role, builder: shapeBuilder(doc) };
  }

  // The builder a key belongs to ({ id, app, role, email }), or null.
  async ofKey(key) {
    if (!key) return null;
    const [row] = await this.db.query('SELECT * FROM sa_builders WHERE key_hash = :h LIMIT 1', { h: hash(key) });
    return row && row.status === 'joined' ? { id: row.id, app: row.app, role: row.role, email: row.email, name: row.name || '' } : null;
  }
}

export function joinLink(base, token) {
  return `${String(base || '').replace(/\/+$/, '')}/join?token=${token}`;
}

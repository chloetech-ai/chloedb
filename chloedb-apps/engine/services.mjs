// What every app gets besides its own tables: people who sign in, files they
// upload, email the app sends, and places found by address. Views reach
// them through app.me(), app.signIn(), app.upload(), app.email(),
// app.geocode() and the rest; server functions through the same app calls.
// People, sessions, file metadata and the outbox are rows in Strata; file
// bytes sit next to the database.

import { scrypt as scryptCb, randomBytes, timingSafeEqual, createHash } from 'node:crypto';
import { mkdir, writeFile, readFile } from 'node:fs/promises';
import { promisify } from 'node:util';
import net from 'node:net';
import tls from 'node:tls';
import os from 'node:os';
import path from 'node:path';
import { newId } from './store.mjs';

const scrypt = promisify(scryptCb);
const DAY = 86400000;
export const SESSION_DAYS = 30;
export const MAX_FILE = 15 * 1024 * 1024;
const EMAIL_RE = /^[^\s@<>()",;]+@[^\s@<>()",;]+\.[^\s@<>()",;]+$/;
const err = (message, status = 400) => Object.assign(new Error(message), { status });

export class Services {
  constructor(engine, { filesDir, log = () => {}, smtpUrl = process.env.SMTP_URL, smtpFrom = process.env.SMTP_FROM, geocoder = process.env.STRATA_APPS_GEOCODER } = {}) {
    this.engine = engine;
    this.db = engine.db;
    this.store = engine.store;
    this.filesDir = filesDir;
    this.log = log;
    this.smtpUrl = smtpUrl || null;
    this.smtpFrom = smtpFrom || null;
    this.geocoder = geocoder === 'off' ? null : geocoder || 'https://nominatim.openstreetmap.org/search';
    this.sessions = new Map(); // token hash -> {person, expires}
    this.places = new Map();
    this.lastGeocode = 0;
  }

  // ---- people

  static async hashPassword(password, salt = randomBytes(16)) {
    const key = await scrypt(String(password), salt, 32);
    return `scrypt$${salt.toString('hex')}$${key.toString('hex')}`;
  }

  static async checkPassword(password, stored) {
    const [kind, salt, hex] = String(stored || '').split('$');
    if (kind !== 'scrypt' || !salt || !hex) return false;
    const key = await scrypt(String(password), Buffer.from(salt, 'hex'), 32);
    const want = Buffer.from(hex, 'hex');
    return want.length === key.length && timingSafeEqual(want, key);
  }

  async personByEmail(email) {
    const rows = await this.db.query('SELECT id, email, name, hash FROM sa_people WHERE email = :e LIMIT 1', { e: String(email).trim().toLowerCase() });
    return rows[0] || null;
  }

  async member(appId, personId) {
    const [m] = await this.db.get('sa_members', [`${appId}#${personId}`]);
    return m || null;
  }

  // Joins the app, as its owner when nobody has yet.
  async join(appId, personId) {
    const had = await this.member(appId, personId);
    if (had) return had;
    const any = await this.db.query('SELECT id FROM sa_members WHERE app = :a LIMIT 1', { a: appId });
    const doc = { id: `${appId}#${personId}`, app: appId, person: personId, role: any.length ? 'member' : 'owner' };
    await this.db.write([{ op: 'insert', table: 'sa_members', doc }]);
    return doc;
  }

  async newSession(personId) {
    const token = randomBytes(24).toString('base64url');
    const id = createHash('sha256').update(token).digest('hex');
    const expires = new Date(Date.now() + SESSION_DAYS * DAY).toISOString();
    await this.db.write([{ op: 'insert', table: 'sa_sessions', doc: { id, person: personId, expires } }]);
    this.sessions.set(id, { person: personId, expires });
    return token;
  }

  async signUp(appId, { email, name, password } = {}) {
    const e = String(email || '').trim().toLowerCase();
    if (!EMAIL_RE.test(e)) throw err('Enter an email address');
    if (String(password || '').length < 8) throw err('Choose a password of at least 8 characters');
    if (await this.personByEmail(e)) throw err('That email already has an account: sign in instead');
    const person = { id: newId(10), email: e, name: String(name || '').trim().slice(0, 80) || e.split('@')[0], hash: await Services.hashPassword(password) };
    await this.db.write([{ op: 'insert', table: 'sa_people', doc: person }]);
    await this.join(appId, person.id);
    this.store.track(appId, 'action', '', 'signed up').catch(() => {});
    return { token: await this.newSession(person.id), user: await this.user(appId, person.id) };
  }

  async signIn(appId, { email, password } = {}) {
    const p = await this.personByEmail(email || '');
    if (!p || !(await Services.checkPassword(password || '', p.hash))) throw err('That email and password do not match', 401);
    await this.join(appId, p.id);
    return { token: await this.newSession(p.id), user: await this.user(appId, p.id) };
  }

  async signOut(token) {
    if (!token) return;
    const id = createHash('sha256').update(token).digest('hex');
    this.sessions.delete(id);
    await this.db.write([{ op: 'delete', table: 'sa_sessions', key: id }]).catch(() => {});
  }

  // The person a session token belongs to, or null.
  async personOf(token) {
    if (!token) return null;
    const id = createHash('sha256').update(token).digest('hex');
    let s = this.sessions.get(id);
    if (!s) {
      const [row] = await this.db.get('sa_sessions', [id]);
      if (!row) return null;
      s = { person: row.person, expires: row.expires };
      this.sessions.set(id, s);
    }
    if (Date.parse(s.expires) < Date.now()) { this.sessions.delete(id); return null; }
    return s.person;
  }

  // The signed-in person as an app sees them: {id, name, email, role}, or
  // null. A person signed in through another app joins this one on first use.
  async user(appId, personId) {
    if (!personId) return null;
    const [p] = await this.db.get('sa_people', [personId]);
    if (!p) return null;
    const m = await this.join(appId, personId);
    return { id: p.id, name: p.name, email: p.email, role: m.role };
  }

  async people(appId) {
    const members = await this.db.query('SELECT person, role, created FROM sa_members WHERE app = :a ORDER BY created LIMIT 1000', { a: appId });
    if (!members.length) return [];
    const rows = await this.db.get('sa_people', members.map((m) => m.person));
    return members.map((m, i) => (rows[i] ? { id: rows[i].id, name: rows[i].name, email: rows[i].email, role: m.role, joined: m.created } : null)).filter(Boolean);
  }

  // ---- files

  async saveFile(appId, bytes, { name, type } = {}, by = null) {
    if (!bytes || !bytes.length) throw err('the file is empty');
    if (bytes.length > MAX_FILE) throw err(`files are at most ${MAX_FILE / 1024 / 1024} MB`, 413);
    const clean = String(name || 'file').replace(/[\\/\0\r\n]/g, '_').slice(0, 200) || 'file';
    const mime = /^[a-z]+\/[a-z0-9.+-]+$/i.test(type || '') ? String(type).toLowerCase() : 'application/octet-stream';
    const id = newId(14);
    await mkdir(path.join(this.filesDir, appId), { recursive: true });
    await writeFile(path.join(this.filesDir, appId, id), bytes);
    const doc = { id, app: appId, name: clean, type: mime, size: bytes.length, author: by || '' };
    await this.db.write([{ op: 'insert', table: 'sa_files', doc }]);
    this.store.track(appId, 'action', '', `uploaded ${mime}`).catch(() => {});
    return { id, name: clean, type: mime, size: bytes.length, by: by || null, created: new Date().toISOString() };
  }

  async fileInfo(appId, ids) {
    const list = [].concat(ids || []).map(String).filter((x) => /^[a-z0-9]{6,20}$/.test(x)).slice(0, 200);
    if (!list.length) return [];
    const rows = await this.db.get('sa_files', list);
    return rows.filter((r) => r && r.app === appId).map(({ app, ...r }) => ({ ...r, size: Number(r.size) }));
  }

  async readFile(appId, id) {
    const [meta] = await this.fileInfo(appId, [id]);
    if (!meta) throw err('no such file', 404);
    return { meta, bytes: await readFile(path.join(this.filesDir, appId, meta.id)) };
  }

  // ---- email

  async email(appId, msg = {}, by = null) {
    const to = [].concat(msg.to || []).flatMap((x) => String(x).split(',')).map((x) => x.trim().toLowerCase()).filter(Boolean);
    if (!to.length) throw err('email: say who it goes to (to)');
    if (to.length > 10) throw err('email: at most 10 recipients');
    const bad = to.find((x) => !EMAIL_RE.test(x));
    if (bad) throw err(`email: ${bad} is not an email address`);
    const subject = String(msg.subject || '').replace(/[\r\n]+/g, ' ').trim().slice(0, 200);
    const body = String(msg.text ?? msg.body ?? '').slice(0, 20000);
    if (!subject && !body) throw err('email: give it a subject or a text');
    const since = new Date(Date.now() - 3600000).toISOString();
    const recent = await this.db.query('SELECT COUNT(*) AS n FROM sa_outbox WHERE app = :a AND created > :t', { a: appId, t: since }).catch(() => [{ n: 0 }]);
    if (Number(recent[0]?.n || 0) >= 30) throw err('email: this app sent 30 emails in the last hour; try again later', 429);
    const id = newId(12);
    const sending = !!(this.smtpUrl && this.smtpFrom);
    const doc = { id, app: appId, recipients: to, subject, body, status: sending ? 'sending' : 'held', author: by || '' };
    await this.db.write([{ op: 'insert', table: 'sa_outbox', doc }]);
    this.store.track(appId, 'action', '', `email to ${to.length}`).catch(() => {});
    if (sending) {
      const appName = (await this.store.getApp(appId))?.name || 'Strata Apps';
      smtpSend(this.smtpUrl, { from: this.smtpFrom, fromName: appName, to, subject, text: body })
        .then(() => this.db.write([{ op: 'set', table: 'sa_outbox', key: id, field: 'status', value: 'sent' }]))
        .catch((e) => {
          this.log(`email ${id}: ${e.message}`);
          return this.db.write([{ op: 'upsert', table: 'sa_outbox', doc: { ...doc, status: 'failed', error: e.message.slice(0, 500) } }]);
        }).catch(() => {});
    }
    return { id, status: doc.status, to, subject };
  }

  async outbox(appId, limit = 50) {
    return this.db.query(`SELECT id, recipients, subject, body, status, error, created FROM sa_outbox WHERE app = :a ORDER BY created DESC LIMIT ${Number(limit)}`, { a: appId });
  }

  // ---- places

  async geocode(query) {
    const q = String(query || '').trim().replace(/\s+/g, ' ').slice(0, 200);
    if (q.length < 3) return [];
    const key = q.toLowerCase();
    if (this.places.has(key)) return this.places.get(key);
    if (!this.geocoder) throw err('address search is switched off here (STRATA_APPS_GEOCODER=off)', 503);
    // Nominatim's policy: at most one request a second, with a name.
    const wait = this.lastGeocode + 1100 - Date.now();
    if (wait > 0) await new Promise((r) => setTimeout(r, wait));
    this.lastGeocode = Date.now();
    let rows;
    try {
      const url = `${this.geocoder}?format=jsonv2&limit=5&q=${encodeURIComponent(q)}`;
      const res = await fetch(url, { headers: { 'user-agent': 'chloedb-apps/0.1 (prototype app engine on Strata)', accept: 'application/json' }, signal: AbortSignal.timeout(8000) });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      rows = await res.json();
    } catch (e) {
      throw err(`address search did not answer (${e.message})`, 502);
    }
    const out = (Array.isArray(rows) ? rows : []).map((r) => ({ label: r.display_name, lat: Number(r.lat), lng: Number(r.lon), kind: r.type || r.category || '' }))
      .filter((r) => Number.isFinite(r.lat) && Number.isFinite(r.lng));
    if (this.places.size > 2000) this.places.clear();
    this.places.set(key, out);
    return out;
  }
}

// ---- a small SMTP client: implicit TLS (smtps://, port 465) or STARTTLS
// when the server offers it, AUTH PLAIN, one message.

const b64 = (s) => Buffer.from(s, 'utf8').toString('base64');
const header = (s) => (/^[\x20-\x7e]*$/.test(s) ? s : `=?UTF-8?B?${b64(s)}?=`);

export function buildMessage({ from, fromName, to, subject, text, date = new Date(), id = `${randomBytes(8).toString('hex')}@chloedb-apps` }) {
  const body = String(text || '').replace(/\r?\n/g, '\r\n').split('\r\n').map((l) => (l.startsWith('.') ? `.${l}` : l)).join('\r\n');
  return [
    `From: ${fromName ? `${header(fromName.replace(/["\r\n]/g, ''))} <${from}>` : from}`,
    `To: ${to.join(', ')}`,
    `Subject: ${header(subject || '')}`,
    `Date: ${date.toUTCString()}`,
    `Message-ID: <${id}>`,
    'MIME-Version: 1.0',
    'Content-Type: text/plain; charset=utf-8',
    'Content-Transfer-Encoding: 8bit',
    '',
    body,
  ].join('\r\n');
}

export async function smtpSend(url, msg, { timeoutMs = 20000 } = {}) {
  const u = new URL(url);
  const secure = u.protocol === 'smtps:';
  const port = Number(u.port) || (secure ? 465 : 587);
  const host = u.hostname;
  let sock = secure ? tls.connect({ host, port, servername: host }) : net.connect({ host, port });
  let buf = '';
  let waiting = null;
  const onData = (d) => {
    buf += d.toString('utf8');
    const lines = buf.split('\r\n');
    for (let i = 0; i < lines.length - 1; i++) {
      if (/^\d{3} /.test(lines[i]) && waiting) {
        const reply = lines.slice(0, i + 1).join('\n');
        buf = lines.slice(i + 1).join('\r\n');
        const w = waiting; waiting = null; w.resolve(reply);
        return;
      }
    }
  };
  const attach = (s) => { s.on('data', onData); s.on('error', (e) => { if (waiting) { const w = waiting; waiting = null; w.reject(e); } }); };
  attach(sock);
  const timer = setTimeout(() => sock.destroy(new Error('the mail server did not answer in time')), timeoutMs);
  const read = () => new Promise((resolve, reject) => { waiting = { resolve, reject }; onData(Buffer.alloc(0)); });
  const expect = async (codes, line) => {
    if (line !== undefined) sock.write(`${line}\r\n`);
    const reply = await read();
    const last = reply.split('\n').pop();
    if (!codes.includes(Number(last.slice(0, 3)))) throw new Error(`mail server: ${last}${line ? ` (after ${line.split(' ')[0]})` : ''}`);
    return reply;
  };
  try {
    if (!secure) await new Promise((resolve, reject) => { sock.once('connect', resolve); sock.once('error', reject); });
    await expect([220]);
    const me = os.hostname().replace(/[^a-zA-Z0-9.-]/g, '') || 'localhost';
    let ehlo = await expect([250], `EHLO ${me}`);
    if (!secure && /STARTTLS/i.test(ehlo)) {
      await expect([220], 'STARTTLS');
      sock.removeListener('data', onData);
      sock = tls.connect({ socket: sock, servername: host });
      await new Promise((resolve, reject) => { sock.once('secureConnect', resolve); sock.once('error', reject); });
      attach(sock);
      ehlo = await expect([250], `EHLO ${me}`);
    }
    if (u.username) await expect([235], `AUTH PLAIN ${b64(`\0${decodeURIComponent(u.username)}\0${decodeURIComponent(u.password)}`)}`);
    await expect([250], `MAIL FROM:<${msg.from}>`);
    for (const r of msg.to) await expect([250, 251], `RCPT TO:<${r}>`);
    await expect([354], 'DATA');
    await expect([250], `${buildMessage(msg)}\r\n.`);
    sock.write('QUIT\r\n');
  } finally {
    clearTimeout(timer);
    setTimeout(() => sock.destroy(), 200);
  }
}

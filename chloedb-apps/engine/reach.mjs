// What an app may reach beyond its own rows: a model (app.ask), the web
// through that model (app.ask(..., {web: true})), and public pages and APIs
// (app.fetch). Each is off until the app's owner switches it on in the
// studio, so an app built to use one does nothing costly or outward until
// someone chose it.
//
// app.ask runs through the engine's own sign-in (llm.mjs, `claude -p`), a
// small model by default. Its answers are kept in Strata (sa_asks) and the
// same question gets the kept answer (web answers for six hours), each app
// has a daily number of model calls, and every call counts in the studio's
// spending limit. app.fetch reaches http and https addresses on the public
// internet only: an address on this machine or its networks is refused,
// checked on every address the name resolves to, at connect time and after
// every redirect.
//
// Two finer controls sit under the switch for pages and APIs:
//   hosts: the owner allows each host an app reads from (GET, HEAD), and
//     separately each host it may send to (POST). A host the code reaches
//     that is not allowed is refused and listed for the owner to allow, so a
//     change (asked for, built from use, or a repair) never widens what an
//     app reaches by itself. A host covers its subdomains. Apps switched on
//     before hosts existed keep "*": any public host, until the owner
//     replaces it with a list.
//   secrets: API keys the owner pastes in, kept encrypted (AES-256-GCM) in
//     sa_settings, outside the app's code. Code names one,
//     app.fetch(url, {secret: 'weather'}), and the engine puts it in the
//     header or query parameter the owner chose, only on a request to one of
//     the hosts the secret is bound to (a redirect elsewhere goes without
//     it). Code, the model and the studio never see the value again, and an
//     answer that echoes it back has it replaced by [secret].

import { createHash, createCipheriv, createDecipheriv, randomBytes } from 'node:crypto';
import dns from 'node:dns';
import http from 'node:http';
import https from 'node:https';
import net from 'node:net';
import zlib from 'node:zlib';
import { ask as llmAsk, extractJson } from './llm.mjs';

export const ASK_MODEL = process.env.STRATA_APPS_ASK_MODEL || 'claude-haiku-5-5';
// Model calls per app per day (answers from the cache are free), and web
// fetches per app per day.
export const ASKS_PER_DAY = Number(process.env.STRATA_APPS_ASKS_PER_DAY) || 200;
export const FETCHES_PER_DAY = Number(process.env.STRATA_APPS_FETCHES_PER_DAY) || 1000;
// Bursts: at most this many in any minute, per app.
const ASKS_PER_MINUTE = 20;
const FETCHES_PER_MINUTE = 60;
const WEB_KEEP_MS = 6 * 3600_000;
const MAX_PROMPT = 20_000;
export const MAX_FETCH_BYTES = 2 * 1024 * 1024;
const FETCH_TIMEOUT_MS = 10_000;
// A web ask may search and read pages for a while, and costs more.
const WEB_BUDGET_USD = 0.25;

// The three, as the studio names them.
export const ABILITIES = {
  ask: { title: 'A model', what: 'app.ask: the app asks a small model to summarize, sort, extract or draft' },
  web: { title: 'Web search', what: 'app.ask(..., {web: true}): the model searches the web for current facts' },
  fetch: { title: 'Web pages and APIs', what: 'app.fetch and PULL: the app reads public pages, feeds and APIs, from the hosts allowed below' },
  schedule: { title: 'Schedules', what: 'rules and functions that run on their own, on a clock (EVERY ...), with nobody watching' },
};

const err = (message, status = 400, extra = {}) => Object.assign(new Error(message), { status, ...extra });
const day = () => new Date().toISOString().slice(0, 10);

// What an app's code uses, read from the code (not from what the plan said):
// views, functions and components the app runs. hosts are those its code
// names: every address in a function or rule (they run on the server), and
// the literal address of a view's app.fetch; sends says whether any of it
// POSTs; secrets are the names it asks for.
export function usesOf(view) {
  const fns = view?.functions || [];
  const code = [...(view?.views || []), ...fns, ...(view?.components || [])].map((c) => String(c.code || '')).join('\n');
  const asks = /\bapp\.ask\s*\(/.test(code);
  const pulls = fns.flatMap((f) => pullsOf(f));
  const hosts = new Set();
  const add = (u) => { try { const h = new URL(u).hostname.toLowerCase().replace(/\.$/, ''); if (h.includes('.')) hosts.add(h); } catch { /* not an address */ } };
  for (const f of fns) if (!f.spec?.rule) for (const m of String(f.code || '').matchAll(/https?:\/\/[^\s'"`)<>\\]+/g)) add(m[0]);
  for (const v of [...(view?.views || []), ...(view?.components || [])]) for (const m of String(v.code || '').matchAll(/\bapp\.fetch\s*\(\s*['"`](https?:\/\/[^'"`]+)/g)) add(m[1]);
  for (const p of pulls) if (/^https?:/i.test(p.url)) add(p.url);
  const secrets = new Set([...code.matchAll(/\bsecret\s*:\s*['"]([A-Za-z0-9_-]{1,40})['"]/g)].map((m) => m[1]));
  for (const p of pulls) if (p.secret) secrets.add(p.secret);
  return {
    ask: asks,
    web: asks && /\bweb\s*:\s*true\b/.test(code),
    fetch: /\bapp\.fetch\s*\(/.test(code) || pulls.length > 0,
    schedule: fns.some((f) => f.spec?.every),
    hosts: [...hosts].sort(),
    sends: /\bmethod\s*:\s*['"`]post['"`]/i.test(code),
    secrets: [...secrets].sort(),
  };
}

// The PULL steps of a rule (its code is the rule's JSON).
function pullsOf(fn) {
  if (!fn?.spec?.rule) return [];
  let rule;
  try { rule = typeof fn.code === 'string' ? JSON.parse(fn.code) : fn.code; } catch { return []; }
  const out = [];
  const walk = (steps) => { for (const st of steps || []) { if (st?.pull) out.push(st.pull); if (st?.do) walk(st.do); } };
  walk(rule?.steps);
  return out;
}

// ---- hosts

const HOST_RE = /^(\*|(\*\.)?[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?(\.[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?)+)$/;
// A host as the owner may allow it: a name on the public internet (its
// subdomains come with it), or "*" for any.
export function cleanHost(raw) {
  let h = String(raw || '').trim().toLowerCase();
  if (/^https?:\/\//.test(h)) { try { h = new URL(h).hostname; } catch { /* below */ } }
  h = h.replace(/\.$/, '').replace(/^\*\./, '');
  if (h === '*') return h;
  if (!HOST_RE.test(h) || net.isIP(h) || /\.(localhost|local|internal|lan|home|arpa)$/.test(h)) throw err(`${String(raw).slice(0, 80)} is not a host on the public internet (give a name such as api.example.com)`);
  return h;
}
export const hostMatches = (entry, host) => entry === '*' || host === entry || host.endsWith(`.${entry}`);

// ---- secrets

const SECRET_NAME = /^[A-Za-z0-9_-]{1,40}$/;
export function sealer(key) {
  const k = createHash('sha256').update(key || randomBytes(32)).digest();
  return {
    seal: (text) => { const iv = randomBytes(12); const c = createCipheriv('aes-256-gcm', k, iv); const data = Buffer.concat([c.update(String(text), 'utf8'), c.final()]); return { iv: iv.toString('base64'), tag: c.getAuthTag().toString('base64'), data: data.toString('base64') }; },
    open: (box) => { const d = createDecipheriv('aes-256-gcm', k, Buffer.from(box.iv, 'base64')); d.setAuthTag(Buffer.from(box.tag, 'base64')); return Buffer.concat([d.update(Buffer.from(box.data, 'base64')), d.final()]).toString('utf8'); },
  };
}

// The value replaced wherever an answer carries it back.
function redact(v, secret) {
  if (!secret || secret.length < 4) return v;
  if (typeof v === 'string') return v.split(secret).join('[secret]');
  if (Array.isArray(v)) return v.map((x) => redact(x, secret));
  if (v && typeof v === 'object') return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, redact(x, secret)]));
  return v;
}

// ---- addresses that are not the public internet

const V4_PRIVATE = [
  ['0.0.0.0', 8], ['10.0.0.0', 8], ['100.64.0.0', 10], ['127.0.0.0', 8], ['169.254.0.0', 16], ['172.16.0.0', 12],
  ['192.0.0.0', 24], ['192.0.2.0', 24], ['192.88.99.0', 24], ['192.168.0.0', 16], ['198.18.0.0', 15], ['198.51.100.0', 24],
  ['203.0.113.0', 24], ['224.0.0.0', 4], ['240.0.0.0', 4],
];
const v4num = (ip) => ip.split('.').reduce((n, x) => n * 256 + Number(x), 0);
function privateV4(ip) {
  const n = v4num(ip);
  return V4_PRIVATE.some(([base, bits]) => Math.floor(n / 2 ** (32 - bits)) === Math.floor(v4num(base) / 2 ** (32 - bits)));
}
function v6words(ip) {
  let s = ip.toLowerCase().replace(/%.*$/, '');
  // a dotted tail (::ffff:1.2.3.4) as two words
  const dotted = s.match(/(\d+\.\d+\.\d+\.\d+)$/);
  if (dotted) { const n = v4num(dotted[1]); s = s.slice(0, -dotted[1].length) + `${(n >>> 16).toString(16)}:${(n & 0xffff).toString(16)}`; }
  const [head, tail] = s.split('::');
  const a = head ? head.split(':') : [];
  const b = tail !== undefined ? (tail ? tail.split(':') : []) : [];
  const words = tail !== undefined ? [...a, ...Array(8 - a.length - b.length).fill('0'), ...b] : a;
  return words.map((w) => parseInt(w || '0', 16));
}
export function privateAddress(ip) {
  const kind = net.isIP(String(ip));
  if (kind === 4) return privateV4(ip);
  if (kind !== 6) return true;
  const w = v6words(ip);
  if (w.length !== 8 || w.some((x) => !Number.isFinite(x))) return true;
  const v4 = () => `${w[6] >> 8}.${w[6] & 255}.${w[7] >> 8}.${w[7] & 255}`;
  if (w.slice(0, 7).every((x) => x === 0)) return true; // :: and ::1 (and the old ::a.b.c.d)
  if (w.slice(0, 5).every((x) => x === 0) && w[5] === 0xffff) return privateV4(v4()); // ::ffff:a.b.c.d
  if (w[0] === 0x64 && w[1] === 0xff9b) return privateV4(v4()); // NAT64
  if ((w[0] & 0xfe00) === 0xfc00) return true; // fc00::/7, unique local
  if ((w[0] & 0xffc0) === 0xfe80) return true; // fe80::/10, link local
  if ((w[0] & 0xff00) === 0xff00) return true; // multicast
  if (w[0] === 0x2001 && w[1] === 0x0db8) return true; // documentation
  if (w[0] === 0x2002) return privateV4(`${w[1] >> 8}.${w[1] & 255}.${w[2] >> 8}.${w[2] & 255}`); // 6to4
  if (w[0] === 0x0100 && w[1] === 0 && w[2] === 0 && w[3] === 0) return true; // discard
  return false;
}

// A URL an app may fetch, or the reason it may not.
export function checkUrl(raw) {
  let u;
  try { u = new URL(String(raw || '')); } catch { throw err(`app.fetch: "${String(raw).slice(0, 100)}" is not a URL`); }
  if (u.protocol !== 'http:' && u.protocol !== 'https:') throw err(`app.fetch: only http and https addresses, not ${u.protocol}`);
  if (u.username || u.password) throw err('app.fetch: a URL may not carry a name or password');
  const host = u.hostname.replace(/^\[|\]$/g, '').replace(/\.$/, '').toLowerCase();
  if (!host || host === 'localhost' || /\.(localhost|local|internal|lan|home|arpa)$/.test(host) || !host.includes('.') && !net.isIP(host)) throw err(`app.fetch: ${host || 'that address'} is not on the public internet`, 403);
  if (net.isIP(host) && privateAddress(host)) throw err(`app.fetch: ${host} is not on the public internet`, 403);
  const port = Number(u.port || (u.protocol === 'https:' ? 443 : 80));
  if (![80, 443, 8080, 8443].includes(port)) throw err(`app.fetch: port ${port} is not open to apps (80, 443, 8080 and 8443 are)`, 403);
  return u;
}

// dns.lookup for the request: every address the name has must be public.
export function publicLookup(hostname, options, cb) {
  dns.lookup(hostname, { ...options, all: true }, (e, addrs) => {
    if (e) return cb(e);
    const bad = addrs.find((a) => privateAddress(a.address));
    if (bad) return cb(err(`app.fetch: ${hostname} leads to ${bad.address}, which is not on the public internet`, 403, { code: 'EPRIVATE' }));
    if (options?.all) return cb(null, addrs);
    return cb(null, addrs[0].address, addrs[0].family);
  });
}

// Readable text of an HTML page, and its title.
export function pageText(html) {
  const title = (html.match(/<title[^>]*>([\s\S]*?)<\/title>/i)?.[1] || '').trim();
  const text = html
    .replace(/<!--[\s\S]*?-->/g, ' ')
    .replace(/<(script|style|noscript|svg|template|iframe|head)\b[\s\S]*?<\/\1>/gi, ' ')
    .replace(/<(br|\/p|\/div|\/li|\/h[1-6]|\/tr|\/section|\/article|\/header|\/footer|hr)\b[^>]*>/gi, '\n')
    .replace(/<[^>]+>/g, ' ');
  return { title: decode(title), text: decode(text).replace(/[ \t\f\v ]+/g, ' ').replace(/ *\n */g, '\n').replace(/\n{3,}/g, '\n\n').trim() };
}
const ENT = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ', mdash: '—', ndash: '–', hellip: '…', rsquo: '’', lsquo: '‘', rdquo: '”', ldquo: '“', copy: '©' };
const decode = (s) => s.replace(/&(#x[0-9a-f]+|#\d+|[a-z]+);/gi, (m, e) => {
  if (e[0] === '#') { const n = e[1].toLowerCase() === 'x' ? parseInt(e.slice(2), 16) : Number(e.slice(1)); return n > 0 && n < 0x110000 ? String.fromCodePoint(n) : m; }
  return ENT[e.toLowerCase()] ?? m;
});

// One request, with the address checked at connect time and the body cut at
// MAX_FETCH_BYTES.
function request(u, { method = 'GET', headers = {}, body = null, timeoutMs = FETCH_TIMEOUT_MS }) {
  return new Promise((resolve, reject) => {
    const lib = u.protocol === 'https:' ? https : http;
    let done = false;
    const parts = [];
    let size = 0;
    let cut = false;
    const fail = (e) => { if (done) return; done = true; clearTimeout(timer); reject(e.status ? e : err(`app.fetch: ${u.host}: ${e.code || e.message}`, 502)); };
    const req = lib.request(u, { method, headers, lookup: publicLookup, timeout: timeoutMs, agent: false }, (res) => {
      const enc = String(res.headers['content-encoding'] || '').toLowerCase();
      const stream = enc === 'gzip' ? res.pipe(zlib.createGunzip()) : enc === 'br' ? res.pipe(zlib.createBrotliDecompress()) : enc === 'deflate' ? res.pipe(zlib.createInflate()) : res;
      const finish = () => { if (done) return; done = true; clearTimeout(timer); resolve({ status: res.statusCode, headers: res.headers, body: Buffer.concat(parts), cut }); };
      stream.on('data', (d) => {
        if (done) return;
        size += d.length;
        if (size > MAX_FETCH_BYTES) { parts.push(d.subarray(0, d.length - (size - MAX_FETCH_BYTES))); cut = true; finish(); res.destroy(); return; }
        parts.push(d);
      });
      stream.on('end', finish);
      stream.on('error', fail);
      res.on('error', fail);
    });
    const timer = setTimeout(() => req.destroy(err(`app.fetch: ${u.host} did not answer in ${Math.round(timeoutMs / 1000)} s`, 504)), timeoutMs);
    req.on('timeout', () => req.destroy(err(`app.fetch: ${u.host} did not answer in ${Math.round(timeoutMs / 1000)} s`, 504)));
    req.on('error', fail);
    if (body !== null) req.write(body);
    req.end();
  });
}

const SENDABLE = new Set(['accept', 'accept-language', 'content-type']);

// Fetches a public URL: {ok, status, url, type, json, text, title, body, truncated}.
// JSON is parsed; HTML gives its readable text and title (body keeps the
// HTML); other text comes as it is; anything else only its type and size.
// allow(url) may refuse a hop (a redirect to a host the app may not reach);
// secret {value, hosts, header, prefix, param} goes only on a hop to one of
// its hosts.
export async function fetchPublic(raw, opts = {}, { requestFn = request, allow = null, secret = null } = {}) {
  let u = checkUrl(raw);
  let method = String(opts.method || 'GET').toUpperCase();
  if (!['GET', 'POST', 'HEAD'].includes(method)) throw err(`app.fetch: only GET, POST and HEAD, not ${method}`);
  const headers = { 'user-agent': 'chloedb-apps/0.1 (an app on ChloeDB)', accept: '*/*', 'accept-encoding': 'gzip, deflate, br' };
  for (const [k, v] of Object.entries(opts.headers || {})) if (SENDABLE.has(k.toLowerCase())) headers[k.toLowerCase()] = String(v).slice(0, 200);
  let body = null;
  if (method === 'POST') {
    if (opts.json !== undefined) { body = JSON.stringify(opts.json); headers['content-type'] ||= 'application/json'; }
    else if (opts.body !== undefined) body = String(opts.body);
    if (body && body.length > 256 * 1024) throw err('app.fetch: a body of at most 256 KB');
  }
  for (let hop = 0; ; hop++) {
    if (allow) allow(u);
    let target = u;
    const sent = body !== null ? { ...headers, 'content-length': Buffer.byteLength(body) } : { ...headers };
    if (secret && (secret.hosts || []).some((h) => hostMatches(h, u.hostname.toLowerCase()))) {
      if (secret.param) { target = new URL(u.href); target.searchParams.set(secret.param, secret.value); }
      else sent[(secret.header || 'authorization').toLowerCase()] = `${secret.prefix || ''}${secret.value}`;
    }
    const res = await requestFn(target, { method, headers: sent, body });
    if (res.status >= 300 && res.status < 400 && res.headers.location) {
      if (hop >= 4) throw err('app.fetch: more than 4 redirects', 508);
      u = checkUrl(new URL(res.headers.location, u).href);
      if (res.status === 303 || ((res.status === 301 || res.status === 302) && method === 'POST')) { method = 'GET'; body = null; }
      continue;
    }
    const type = String(res.headers['content-type'] || '').split(';')[0].trim().toLowerCase();
    const out = { ok: res.status >= 200 && res.status < 300, status: res.status, url: u.href, type, size: res.body.length, truncated: !!res.cut };
    const isText = !type || type.startsWith('text/') || /json|xml|javascript|csv|yaml/.test(type);
    if (!isText || method === 'HEAD') return out;
    const s = res.body.toString('utf8');
    if (/json/.test(type) || (!type && /^\s*[[{]/.test(s))) {
      try { out.json = JSON.parse(s); } catch { /* not JSON after all */ }
    }
    if (/html/.test(type) || (!type && /^\s*<(!doctype|html)/i.test(s))) {
      const { title, text } = pageText(s);
      Object.assign(out, { title, text: text.slice(0, 200_000), body: s.slice(0, 1_000_000) });
    } else if (out.json === undefined) {
      out.text = s.slice(0, 1_000_000);
    }
    return out;
  }
}

// ---- the switches, the cache and the counts

export class Reach {
  // store: the engine's store (settings, sa_asks); ask: llm.mjs ask;
  // fetchFn: fetchPublic, both replaceable in tests.
  // secretKey: what secrets are sealed with (STRATA_APPS_SECRET_KEY, or a
  // key file the server keeps under DATA); without one, a key for this run.
  constructor({ store, ask = llmAsk, fetchFn = fetchPublic, log = () => {}, model = ASK_MODEL, secretKey = null } = {}) {
    this.store = store;
    this.box = sealer(secretKey);
    this.db = store?.db;
    this.llm = ask;
    this.fetchFn = fetchFn;
    this.log = log;
    this.model = model;
    this.counts = new Map(); // app -> {day, asks, fetches, cached, usd}
    this.recent = new Map(); // `${app}:${kind}` -> [times in the last minute]
    this.pages = new Map(); // url -> {at, out}, GETs for 10 minutes
    this.pending = new Map(); // cache key -> promise: the same question at once is asked once
  }

  async switches(appId) {
    const raw = await this.store.setting(appId, 'reach').catch(() => null);
    let s = {};
    try { s = raw ? JSON.parse(raw) : {}; } catch { /* none */ }
    // Switched on before host lists: any public host, reading and sending.
    const hosts = Array.isArray(s.hosts) ? s.hosts : s.fetch ? [{ host: '*', send: true }] : [];
    return { ask: !!s.ask, web: !!s.web, fetch: !!s.fetch, schedule: !!s.schedule, perDay: Number(s.perDay) > 0 ? Math.min(5000, Number(s.perDay)) : ASKS_PER_DAY, hosts };
  }

  // patch: any switch by name, perDay, hosts (the whole list, each a name or
  // {host, send}), allow {host, send} (adds or changes one), forget (a host).
  async setSwitches(appId, patch = {}) {
    const now = await this.switches(appId);
    const next = { ...now, hosts: [...now.hosts] };
    for (const k of Object.keys(ABILITIES)) if (k in patch) next[k] = !!patch[k];
    if ('perDay' in patch) next.perDay = Math.max(1, Math.min(5000, Number(patch.perDay) || ASKS_PER_DAY));
    const entry = (x) => (typeof x === 'string' ? { host: cleanHost(x), send: false } : { host: cleanHost(x?.host), send: !!x?.send });
    if (Array.isArray(patch.hosts)) next.hosts = patch.hosts.map(entry);
    if (patch.allow) { const e = entry(patch.allow); next.hosts = [...next.hosts.filter((h) => h.host !== e.host), e]; }
    if (patch.forget) { const h = cleanHost(patch.forget); next.hosts = next.hosts.filter((x) => x.host !== h); }
    const seen = new Map();
    for (const h of next.hosts) seen.set(h.host, { host: h.host, send: !!(seen.get(h.host)?.send || h.send) });
    next.hosts = [...seen.values()].sort((a, b) => a.host.localeCompare(b.host));
    if (next.hosts.length > 100) throw err('at most 100 hosts per app');
    await this.store.putSetting(appId, 'reach', JSON.stringify(next));
    // a host allowed is no longer waiting for the owner
    const asked = await this.asked(appId);
    let changed = false;
    for (const h of Object.keys(asked)) if (this.allowed(next, h, asked[h].send)) { delete asked[h]; changed = true; }
    if (changed) await this.store.putSetting(appId, 'reach-asked', JSON.stringify(asked));
    return next;
  }

  allowed(on, host, send) {
    return (on.hosts || []).some((e) => hostMatches(e.host, host) && (!send || e.send));
  }

  // Hosts the app's code tried to reach and was refused, for the owner to
  // allow: {host: {send, count, last}}.
  async asked(appId) {
    const raw = await this.store.setting(appId, 'reach-asked').catch(() => null);
    try { return raw ? JSON.parse(raw) : {}; } catch { return {}; }
  }

  async noteAsked(appId, host, send) {
    const a = await this.asked(appId);
    if (!a[host] && Object.keys(a).length >= 50) return;
    a[host] = { send: !!(a[host]?.send || send), count: (a[host]?.count || 0) + 1, last: new Date().toISOString() };
    await this.store.putSetting(appId, 'reach-asked', JSON.stringify(a)).catch(() => {});
  }

  // ---- secrets: {name: {hosts, header, prefix, param, created, used, ...sealed}}

  async secretRows(appId) {
    const raw = await this.store.setting(appId, 'secrets').catch(() => null);
    try { return raw ? JSON.parse(raw) : {}; } catch { return {}; }
  }

  // What the studio shows: never the value.
  async secrets(appId) {
    const rows = await this.secretRows(appId);
    return Object.entries(rows).map(([name, r]) => ({ name, hosts: r.hosts, header: r.param ? null : r.header || 'authorization', prefix: r.param ? '' : r.prefix || '', param: r.param || null, created: r.created, used: r.used || null, hint: r.hint || '' }))
      .sort((a, b) => a.name.localeCompare(b.name));
  }

  // {name, value, hosts, header?, prefix?, param?}: a header (authorization
  // by default, with a prefix such as "Bearer ") or a query parameter.
  async putSecret(appId, { name, value, hosts, header, prefix, param } = {}) {
    if (!SECRET_NAME.test(String(name || ''))) throw err('a secret\'s name is 1 to 40 letters, digits, - and _');
    const rows = await this.secretRows(appId);
    const old = rows[name];
    const list = (Array.isArray(hosts) ? hosts : String(hosts || '').split(/[\s,]+/)).filter(Boolean).map(cleanHost);
    if (!list.length && !old) throw err('bind the secret to the hosts it may be sent to (such as api.example.com)');
    if (list.includes('*')) throw err('a secret is bound to named hosts, not to "*"');
    if (value === undefined && !old) throw err('give the secret\'s value');
    if (value !== undefined && (typeof value !== 'string' || !value.trim() || value.length > 4000)) throw err('a secret\'s value is text of up to 4000 characters');
    const h = header !== undefined ? String(header || '').trim().toLowerCase() : old?.header;
    if (h && !/^[a-z0-9-]{1,60}$/.test(h)) throw err('a header name is letters, digits and -');
    if (h && ['host', 'cookie', 'content-length', 'transfer-encoding', 'connection'].includes(h)) throw err(`the secret cannot go in the ${h} header`);
    const p = param !== undefined ? String(param || '').trim() : old?.param;
    if (p && !/^[A-Za-z0-9_.-]{1,60}$/.test(p)) throw err('a query parameter name is letters, digits, _, . and -');
    const sealed = value !== undefined ? { ...this.box.seal(value.trim()), hint: value.trim().length > 12 ? `…${value.trim().slice(-4)}` : '' } : { iv: old.iv, tag: old.tag, data: old.data, hint: old.hint };
    rows[name] = { ...sealed, hosts: list.length ? list : old.hosts, header: p ? null : h || 'authorization', prefix: p ? '' : prefix !== undefined ? String(prefix).slice(0, 40) : old?.prefix ?? ((h || 'authorization') === 'authorization' ? 'Bearer ' : ''), param: p || null, created: old?.created || new Date().toISOString(), used: old?.used || null };
    await this.store.putSetting(appId, 'secrets', JSON.stringify(rows));
    this.pages.clear();
    return (await this.secrets(appId)).find((x) => x.name === name);
  }

  async deleteSecret(appId, name) {
    const rows = await this.secretRows(appId);
    const had = !!rows[name];
    delete rows[name];
    await this.store.putSetting(appId, 'secrets', JSON.stringify(rows));
    this.pages.clear();
    return { deleted: had };
  }

  // What the code uses that its owner has not allowed yet: hosts, secrets
  // not set, and switches off.
  async wants(appId, view) {
    const uses = usesOf(view);
    const on = await this.switches(appId);
    const have = await this.secretRows(appId);
    return {
      switches: Object.keys(ABILITIES).filter((k) => uses[k] && !on[k]),
      hosts: uses.hosts.filter((h) => !this.allowed(on, h, false)),
      secrets: uses.secrets.filter((n) => !have[n]),
    };
  }

  async usage(appId) {
    const c = await this.count(appId);
    return { day: c.day, asks: c.asks, cached: c.cached, fetches: c.fetches, usd: Math.round(c.usd * 10000) / 10000 };
  }

  async count(appId) {
    let c = this.counts.get(appId);
    if (!c || c.day !== day()) {
      const raw = await this.store.setting(appId, 'reach-used').catch(() => null);
      let saved = null;
      try { saved = raw ? JSON.parse(raw) : null; } catch { /* none */ }
      c = saved && saved.day === day() ? { asks: 0, fetches: 0, cached: 0, usd: 0, ...saved } : { day: day(), asks: 0, fetches: 0, cached: 0, usd: 0 };
      this.counts.set(appId, c);
    }
    return c;
  }

  saveCount(appId, c) {
    this.store.putSetting(appId, 'reach-used', JSON.stringify(c)).catch(() => {});
  }

  burst(appId, kind, max) {
    const k = `${appId}:${kind}`;
    const now = Date.now();
    const times = (this.recent.get(k) || []).filter((t) => now - t < 60_000);
    if (times.length >= max) throw err(`app.${kind}: more than ${max} in a minute from this app; wait a little`, 429, { refused: true });
    times.push(now);
    this.recent.set(k, times);
  }

  // app.ask(prompt, {json, web, system, fresh}) -> text, or the JSON value
  // when json is set. In a build's tests it answers "" (null with json)
  // without calling anything.
  async ask(appId, prompt, opts = {}, { test = false } = {}) {
    if (typeof prompt !== 'string') prompt = prompt == null ? '' : JSON.stringify(prompt);
    prompt = prompt.trim();
    if (!prompt) throw err('app.ask: give it a question or an instruction');
    if (prompt.length > MAX_PROMPT) throw err(`app.ask: at most ${MAX_PROMPT} characters (this was ${prompt.length})`);
    const web = !!opts.web;
    const json = !!opts.json;
    const system = typeof opts.system === 'string' ? opts.system.slice(0, 4000) : '';
    if (test) return json ? null : '';
    const on = await this.switches(appId);
    if (!on.ask) throw err('app.ask is switched off for this app: its owner can switch on "A model" in the studio', 403, { off: 'ask', refused: true });
    if (web && !on.web) throw err('web search is switched off for this app: its owner can switch on "Web search" in the studio', 403, { off: 'web', refused: true });
    const key = createHash('sha256').update(JSON.stringify([this.model, system, prompt, json, web])).digest('hex').slice(0, 40);
    const c = await this.count(appId);
    if (!opts.fresh) {
      const kept = await this.kept(appId, key, web);
      if (kept !== null) { c.cached++; this.saveCount(appId, c); return json ? this.parse(kept) : kept; }
    }
    if (this.pending.has(key)) return this.pending.get(key);
    if (c.asks >= on.perDay) throw err(`app.ask: this app has asked the model ${c.asks} times today, its limit (${on.perDay}); it can ask again tomorrow, or its owner can raise the limit`, 429, { refused: true });
    this.burst(appId, 'ask', ASKS_PER_MINUTE);
    c.asks++;
    this.saveCount(appId, c);
    const p = (async () => {
      const sys = `You answer requests that an app's code sends you; the app shows or stores your answer as it is. Answer only what is asked, with no preamble and no sign-off. Plain text, no Markdown, unless the request asks for a format.${web ? ' Search the web when the answer depends on current facts, and name your sources in one short line at the end.' : ' You have no web access: answer from what you know and what the request gives, and say so when you do not know.'}${json ? ' Answer with one JSON value and nothing else: no code fences, no words around it.' : ''}${system ? `\n\nThe app's instructions:\n${system}` : ''}`;
      const t0 = Date.now();
      const r = await this.llm({ system: sys, prompt, model: this.model, timeoutMs: web ? 150_000 : 60_000, idleMs: web ? 90_000 : 45_000, tools: web ? ['WebSearch', 'WebFetch'] : [], maxBudgetUsd: web ? WEB_BUDGET_USD : undefined });
      c.usd += r.cost || 0;
      this.saveCount(appId, c);
      const text = String(r.text || '').trim();
      const value = json ? this.parse(text) : text;
      this.keep(appId, key, { prompt, text, web, cost: r.cost || 0, ms: Date.now() - t0 }).catch((e) => this.log(`ask cache ${appId}: ${e.message}`));
      return value;
    })().finally(() => this.pending.delete(key));
    this.pending.set(key, p);
    return p;
  }

  parse(text) {
    try { return JSON.parse(text); } catch { /* look inside */ }
    try { return extractJson(text); } catch { /* an array? */ }
    const a = text.indexOf('[');
    const b = text.lastIndexOf(']');
    if (a >= 0 && b > a) { try { return JSON.parse(text.slice(a, b + 1)); } catch { /* no */ } }
    throw err(`app.ask: the model's answer was not JSON: ${text.slice(0, 200)}`, 502);
  }

  async kept(appId, key, web) {
    if (!this.db) return null;
    const [row] = await this.db.get('sa_asks', [`${appId}:${key}`]).catch(() => []);
    if (!row) return null;
    if (web && Date.now() - Date.parse(row.created) > WEB_KEEP_MS) return null;
    return row.answer ?? null;
  }

  async keep(appId, key, { prompt, text, web, cost, ms }) {
    if (!this.db) return;
    await this.db.write([{ op: 'upsert', table: 'sa_asks', doc: { id: `${appId}:${key}`, app: appId, kind: web ? 'web' : 'model', prompt: prompt.slice(0, 4000), answer: text, cost, ms, created: new Date().toISOString() } }]);
  }

  // app.fetch(url, {method, json, body, headers}).
  async fetch(appId, url, opts = {}, { test = false } = {}) {
    if (test) return { ok: false, status: 0, url: String(url || ''), type: '', size: 0, truncated: false };
    opts = opts && typeof opts === 'object' ? opts : {};
    const on = await this.switches(appId);
    if (!on.fetch) throw err('app.fetch is switched off for this app: its owner can switch on "Web pages and APIs" in the studio', 403, { off: 'fetch', refused: true });
    const u = checkUrl(url);
    const method = String(opts.method || 'GET').toUpperCase();
    const send = method !== 'GET' && method !== 'HEAD';
    const refuse = async (host) => {
      await this.noteAsked(appId, host, send);
      return err(`app.fetch: ${host} is not among the hosts this app may ${send ? 'send to' : 'read from'}; its owner can allow it in the studio, under "Model and web"`, 403, { off: 'host', host, send, refused: true });
    };
    const host = u.hostname.toLowerCase().replace(/\.$/, '');
    if (!this.allowed(on, host, send)) throw await refuse(host);
    let secret = null;
    if (opts.secret !== undefined) {
      const name = String(opts.secret);
      const row = (await this.secretRows(appId))[name];
      if (!row) throw err(`app.fetch: this app has no secret "${name}"; its owner can add it in the studio, under "Model and web"`, 403, { off: 'secret', secret: name, refused: true });
      if (!row.hosts.some((h) => hostMatches(h, host))) throw err(`app.fetch: the secret "${name}" is only sent to ${row.hosts.join(', ')}, not to ${host}`, 403, { refused: true });
      let value;
      try { value = this.box.open(row); } catch { throw err(`app.fetch: the secret "${name}" cannot be read with this engine's key; its owner can enter it again`, 403, { off: 'secret', secret: name, refused: true }); }
      secret = { value, hosts: row.hosts, header: row.header, prefix: row.prefix, param: row.param };
      row.used = new Date().toISOString();
      this.secretRows(appId).then((rows) => { if (rows[name]) { rows[name].used = row.used; return this.store.putSetting(appId, 'secrets', JSON.stringify(rows)); } return null; }).catch(() => {});
    }
    const cacheable = method === 'GET' && !opts.fresh;
    const key = `${appId}\u0000${opts.secret ?? ''}\u0000${u.href}`;
    const hit = cacheable ? this.pages.get(key) : null;
    if (hit && Date.now() - hit.at < 600_000) return hit.out;
    const c = await this.count(appId);
    if (c.fetches >= FETCHES_PER_DAY) throw err(`app.fetch: this app has fetched ${c.fetches} times today, its limit`, 429, { refused: true });
    this.burst(appId, 'fetch', FETCHES_PER_MINUTE);
    c.fetches++;
    this.saveCount(appId, c);
    // every hop of a redirect must be allowed too
    let refusedHop = null;
    const allow = (hop) => { const h = hop.hostname.toLowerCase().replace(/\.$/, ''); if (!this.allowed(on, h, send)) { refusedHop = h; throw err(`app.fetch: ${host} sent it on to ${h}, which is not among the hosts this app may ${send ? 'send to' : 'read from'}; its owner can allow it in the studio`, 403, { off: 'host', host: h, send, refused: true }); } };
    const { secret: _s, fresh: _f, ...rest } = opts;
    let out;
    try { out = await this.fetchFn(u.href, rest, { allow, secret }); } catch (e) {
      if (refusedHop) await this.noteAsked(appId, refusedHop, send);
      throw secret ? Object.assign(e, { message: redact(e.message, secret.value) }) : e;
    }
    if (secret) out = redact(out, secret.value);
    if (cacheable && out.ok) {
      if (this.pages.size > 500) this.pages.clear();
      this.pages.set(key, { at: Date.now(), out });
    }
    return out;
  }
}

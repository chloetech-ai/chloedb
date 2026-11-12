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

import { createHash } from 'node:crypto';
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
  fetch: { title: 'Web pages and APIs', what: 'app.fetch: the app reads public pages and APIs, and its screens show images from any web address' },
};

const err = (message, status = 400, extra = {}) => Object.assign(new Error(message), { status, ...extra });
const day = () => new Date().toISOString().slice(0, 10);

// What an app's code uses, read from the code (not from what the plan said):
// views, functions and components the app runs.
export function usesOf(view) {
  const code = [...(view?.views || []), ...(view?.functions || []), ...(view?.components || [])].map((c) => String(c.code || '')).join('\n');
  const asks = /\bapp\.ask\s*\(/.test(code);
  return {
    ask: asks,
    web: asks && /\bweb\s*:\s*true\b/.test(code),
    fetch: /\bapp\.fetch\s*\(/.test(code),
  };
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
export async function fetchPublic(raw, opts = {}, { requestFn = request } = {}) {
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
    const res = await requestFn(u, { method, headers: body !== null ? { ...headers, 'content-length': Buffer.byteLength(body) } : headers, body });
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
  constructor({ store, ask = llmAsk, fetchFn = fetchPublic, log = () => {}, model = ASK_MODEL } = {}) {
    this.store = store;
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
    return { ask: !!s.ask, web: !!s.web, fetch: !!s.fetch, perDay: Number(s.perDay) > 0 ? Math.min(5000, Number(s.perDay)) : ASKS_PER_DAY };
  }

  async setSwitches(appId, patch = {}) {
    const now = await this.switches(appId);
    const next = { ...now };
    for (const k of Object.keys(ABILITIES)) if (k in patch) next[k] = !!patch[k];
    if ('perDay' in patch) next.perDay = Math.max(1, Math.min(5000, Number(patch.perDay) || ASKS_PER_DAY));
    await this.store.putSetting(appId, 'reach', JSON.stringify(next));
    return next;
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
    const on = await this.switches(appId);
    if (!on.fetch) throw err('app.fetch is switched off for this app: its owner can switch on "Web pages and APIs" in the studio', 403, { off: 'fetch', refused: true });
    const u = checkUrl(url);
    const method = String(opts?.method || 'GET').toUpperCase();
    const cacheable = method === 'GET' && !opts?.fresh;
    const hit = cacheable ? this.pages.get(u.href) : null;
    if (hit && Date.now() - hit.at < 600_000) return hit.out;
    const c = await this.count(appId);
    if (c.fetches >= FETCHES_PER_DAY) throw err(`app.fetch: this app has fetched ${c.fetches} times today, its limit`, 429, { refused: true });
    this.burst(appId, 'fetch', FETCHES_PER_MINUTE);
    c.fetches++;
    this.saveCount(appId, c);
    const out = await this.fetchFn(u.href, opts || {});
    if (cacheable && out.ok) {
      if (this.pages.size > 500) this.pages.clear();
      this.pages.set(u.href, { at: Date.now(), out });
    }
    return out;
  }
}

// The next edit to an app, recommended from how people use it and, before
// anyone has, from what the app is: pluggable sources (recsources.mjs) each
// propose candidates from one kind of evidence; the recommender scores them
// (a source's own confidence times the weight learned for that source),
// drops what was offered, taken or dismissed already, keeps the best few,
// and stores them as suggestions (sa_suggestions), where the studio and
// GET /api/apps/:id show them and POST takes or dismisses them as before.
//
// Learning: every suggestion it stores names its source (basis). What people
// do with them (taken, dismissed, or left while the app moved on: stale)
// gives each source a take rate, across all apps and for this one (shrunk
// toward the global rate until the app has decided a few), and the rate is
// the source's weight.
//
// The model comes last and is optional: one small call rewrites the new
// candidates as the person would ask for them and orders them; without it
// (off, over its hourly budget, or failing) the sources' own words stand.
// Nothing here runs inside a build: refreshes run one at a time in the
// background, after a build is done or when use comes in.

import { SOURCES, appText, builtWords, wordsOf, jaccard } from './recsources.mjs';

const DAY = 86400000;
export const MAX_NEW = Number(process.env.STRATA_APPS_RECOMMEND_MAX || 4);
const MAX_OPEN = 6;
const MIN_SCORE = 0.12;
const MIN_GAP = 5 * 60 * 1000;
const PRIOR = 3; // how many decisions of an app weigh as much as the global rate
export const RECOMMEND_MODEL = process.env.STRATA_APPS_RECOMMEND_MODEL === 'off' ? null
  : (process.env.STRATA_APPS_RECOMMEND_MODEL || process.env.STRATA_APPS_SUGGEST_MODEL || 'claude-haiku-5-5');
const MODEL_PER_HOUR = Number(process.env.STRATA_APPS_RECOMMEND_MODEL_PER_HOUR || 6);
const NEAR_MIN = 0.6; // a cosine above this: the app has something that means the same

const parse = (s) => { if (!s || typeof s !== 'string') return s || null; try { return JSON.parse(s); } catch { return null; } };
const tsOf = (s) => { const t = Date.parse(String(s || '').replace(' ', 'T')); return Number.isFinite(t) ? t : 0; };

// Each source's weight from what people did with its suggestions:
// rows [{app, basis, status}] -> (basis, app) -> weight, about 1 for a
// source taken half the time, from 0.3 to 2.
export function learnWeights(rows, appId) {
  const tally = () => ({ taken: 0, no: 0 });
  const global = new Map();
  const mine = new Map();
  for (const r of rows) {
    if (!r.basis) continue;
    const add = (m) => { const t = m.get(r.basis) || tally(); if (r.status === 'taken') t.taken++; else if (r.status === 'dismissed') t.no++; else if (r.status === 'stale') t.no += 0.25; m.set(r.basis, t); };
    add(global);
    if (r.app === appId) add(mine);
  }
  return (basis) => {
    const g = global.get(basis) || tally();
    const rate = (g.taken + 1) / (g.taken + g.no + 2);
    const a = mine.get(basis);
    const r = a ? (a.taken + PRIOR * rate) / (a.taken + a.no + PRIOR) : rate;
    return Math.min(2, Math.max(0.3, r / 0.5));
  };
}

// The candidates people should see, best first: scored, one per key and
// per meaning, none offered before (open now, taken, or dismissed in the
// last 30 days; a stale or replaced one may come back), at most two of a
// source, at most `max`.
export function rank(cands, { weight = () => 1, history = [], now = Date.now(), max = MAX_NEW } = {}) {
  const blocked = new Set();
  const blockedWords = [];
  for (const h of history) {
    const hard = h.status === 'open' || h.status === 'taken' || h.status === 'building' || (h.status === 'dismissed' && now - tsOf(h.created) < 30 * DAY);
    if (!hard) continue;
    if (h.signal) blocked.add(h.signal);
    blockedWords.push(wordsOf(h.text));
  }
  const scored = cands.filter((c) => c && c.text && c.key)
    .map((c) => ({ ...c, score: Math.round(Math.max(0, Math.min(1, c.confidence)) * weight(c.basis) * 1000) / 1000 }))
    .filter((c) => c.score >= MIN_SCORE && !blocked.has(c.key))
    .sort((a, b) => b.score - a.score);
  const out = [];
  const per = new Map();
  for (const c of scored) {
    const w = wordsOf(c.text);
    if (out.some((o) => o.key === c.key || jaccard(wordsOf(o.text), w) >= 0.5)) continue;
    if (blockedWords.some((b) => jaccard(b, w) >= 0.5)) continue;
    if ((per.get(c.basis) || 0) >= 2) continue;
    per.set(c.basis, (per.get(c.basis) || 0) + 1);
    out.push(c);
    if (out.length >= max) break;
  }
  return out;
}

// What goes with the request when a suggestion is taken: the change it
// implies, so the build starts from it rather than from the words alone.
export function withHint(text, suggestion) {
  const change = parse(suggestion?.change ?? suggestion?.implies);
  const hint = change?.hint ? String(change.hint).slice(0, 600) : '';
  return hint && !String(text).includes(hint) ? `${text}\n\n(${hint})` : text;
}

const PHRASE_SYSTEM = `You help a person improve a small app they built. You get the app and changes an engine found it may want next, each with its evidence. Rewrite each change as the person would ask for it, in one short sentence in the language of the app's request, specific to this app (its own screens, tables and words), keeping what the change does and nothing more; and order them, the most useful first. Never claim evidence that is not given. Leave out a change that makes no sense for this app.

Answer with JSON only: {"items": [{"id": "the change's id", "text": "the change, as the person would ask for it"}]}.`;

export class Recommender {
  // ask: the model (llm.mjs ask) or null; signalsOf(appId, view, events):
  // the evolver's patterns; busy(appId): a build is running there.
  constructor({ engine, ask = null, model = RECOMMEND_MODEL, signalsOf = null, busy = () => false, log = () => {}, emit = () => {}, sources = SOURCES }) {
    this.engine = engine;
    this.store = engine.store;
    this.ask = ask;
    this.model = model;
    this.signalsOf = signalsOf;
    this.busy = busy;
    this.log = log;
    this.emit = emit;
    this.sources = sources;
    this.last = new Map(); // app -> {at, head, events}
    this.queue = Promise.resolve();
    this.pending = new Set();
    this.modelCalls = [];
    this.profiles = null;
  }

  // ---- what the sources read

  async context(appId) {
    const { store } = this;
    const view = await store.resolve(appId);
    const [events, history, requests, versions, design] = await Promise.all([
      store.recentEvents(appId, 1500).catch(() => []),
      store.suggestionHistory(appId, 200).catch(() => []),
      store.requests(appId, 40).catch(() => []),
      store.compositions(appId).catch(() => []),
      store.designs?.(appId).then((d) => d?.designs?.[d.chosen ?? 0] || null).catch(() => null),
    ]);
    let signals = [];
    if (this.signalsOf) { try { signals = await this.signalsOf(appId, view, events); } catch (e) { this.log(`[${appId}] recommend: patterns: ${e.message}`); } }
    const apps = await this.appNames();
    const live = new Set(view.comp.caps || []);
    const nearCache = new Map();
    const built = builtWords(view, versions.filter((c) => Number(c.n) <= Number(view.comp.n)));
    const ctx = {
      appId, view, events, signals, history, requests, versions, design, built, now: Date.now(),
      appText: appText(view),
      appName: (id) => apps.get(id) || id,
      search: (text, opts) => store.searchCapabilities(text, opts).catch(() => []),
      // Does the app have something that means the same as `text`? By meaning
      // over its own live pieces when ChloeDB embeds, else by words.
      near: async (text) => {
        if (nearCache.has(text)) return nearCache.get(text);
        let best = 0;
        if (store.meaning) {
          const rows = await store.db.query(`SELECT id, score() AS score FROM sa_capabilities WHERE app = :me ORDER BY similarity(meaning, :q) DESC LIMIT 40`,
            { me: appId, q: { embed: String(text).slice(0, 1000), field: 'meaning', table: 'sa_capabilities' } }).catch(() => []);
          for (const r of rows) if (live.has(r.id)) best = Math.max(best, Number(r.score) || 0);
        }
        const w = wordsOf(text);
        const own = view.caps.filter((c) => c.kind !== 'table').map((c) => wordsOf(`${c.title || ''} ${c.description || ''}`));
        const words = Math.max(0, ...own.map((o) => jaccard(w, o)));
        const out = { best: Math.max(best, Math.min(1, words * 1.5)), covered: best >= NEAR_MIN || words >= 0.45 };
        nearCache.set(text, out);
        return out;
      },
      usersOf: (h) => (this.users?.get(`${h.kind}/${h.slug}`) || []),
      example: (slug) => { const p = this.patternCode?.get(slug); return p?.example || null; },
    };
    await this.libraryUse();
    return ctx;
  }

  async appNames() {
    if (this.names && Date.now() - this.names.at < 60000) return this.names.map;
    const map = new Map((await this.store.listApps().catch(() => [])).map((a) => [a.id, a.name]));
    this.names = { at: Date.now(), map };
    return map;
  }

  // Which apps use each pattern and shared piece (5 minutes old at most).
  async libraryUse() {
    if (this.usedAt && Date.now() - this.usedAt < 300000) return;
    this.usedAt = Date.now();
    const users = new Map();
    const push = (k, u) => { if (!users.has(k)) users.set(k, []); if (!users.get(k).some((x) => x.app === u.app)) users.get(k).push(u); };
    for (const a of await this.store.listApps().catch(() => [])) {
      if (!Number(a.head)) continue;
      const v = await this.store.resolve(a.id).catch(() => null);
      if (!v) continue;
      for (const c of v.shared) push(`${c.kind}/${c.slug}`, { app: a.id, name: a.name });
      for (const p of new Set(v.views.flatMap((x) => x.spec?.patterns || []))) push(`pattern/${p}`, { app: a.id, name: a.name });
    }
    this.users = users;
    const code = new Map();
    for (const [slug, p] of await this.store.patterns().catch(() => new Map())) { const j = parse(p.code); if (j) code.set(slug, j); }
    this.patternCode = code;
  }

  // Every source's candidates; a source that fails is left out.
  async candidates(ctx) {
    const out = [];
    for (const s of this.sources) {
      try { out.push(...(await s.run(ctx)).map((c) => ({ basis: s.id, source: s.source, ...c }))); } catch (e) { this.log(`[${ctx.appId}] recommend: ${s.id}: ${e.message}`); }
    }
    return out;
  }

  async weights(appId) {
    const rows = await this.store.db.query(`SELECT app, basis, status FROM sa_suggestions WHERE status IN ('taken', 'dismissed', 'stale') LIMIT 20000`).catch(() => []);
    return learnWeights(rows, appId);
  }

  // The model, last: rewrites the new candidates and orders them. At most
  // MODEL_PER_HOUR calls an hour across apps; any failure keeps the sources' words.
  async phrase(ctx, items) {
    if (!this.ask || !this.model || items.length < 1) return items;
    const now = Date.now();
    this.modelCalls = this.modelCalls.filter((t) => now - t < 3600000);
    if (this.modelCalls.length >= MODEL_PER_HOUR) return items;
    this.modelCalls.push(now);
    const v = ctx.view;
    const first = ctx.requests.at(-1)?.text || v.app.description || '';
    const prompt = [`# The app: ${v.app.name}\nAsked for: ${String(first).slice(0, 600)}`,
      `## Screens\n${v.views.map((x) => `- ${x.slug}: ${x.title}. ${x.description || ''}`).join('\n') || '(none)'}`,
      `## Tables\n${v.tables.map((t) => `- ${t.spec.name}: ${(t.spec.columns || []).map((c) => c.name).join(', ')}`).join('\n') || '(none)'}`,
      `## Changes found (id: change. evidence)\n${items.map((c, i) => `- c${i}: ${c.text}. ${c.why}`).join('\n')}`].join('\n\n');
    try {
      const answer = await this.ask({ system: PHRASE_SYSTEM, prompt, model: this.model, effort: 'low', timeoutMs: 90000, idleMs: 60000, bucket: 'asks' });
      const m = String(answer.text || '').match(/\{[\s\S]*\}/);
      const out = JSON.parse(m ? m[0] : answer.text);
      const got = (out.items || []).map((x) => ({ i: Number(String(x.id).replace(/^c/, '')), text: String(x.text || '').trim() })).filter((x) => items[x.i] && x.text);
      if (!got.length) return items;
      return got.map((x) => ({ ...items[x.i], text: x.text.slice(0, 300), phrased: true }));
    } catch (e) {
      this.log(`[${ctx.appId}] recommend: the model's wording: ${e.message}`);
      return items;
    }
  }

  // Recommends for one app now, and stores what is new. Returns
  // {added, candidates} (candidates: every one scored, for inspection).
  async refresh(appId, { reason = 'now' } = {}) {
    const ctx = await this.context(appId);
    if (!ctx.view.comp.n) return { added: [], candidates: [] };
    const all = await this.candidates(ctx);
    const weight = await this.weights(appId);
    const openOthers = ctx.history.filter((h) => h.status === 'open' && !h.basis).length;
    const mineOpen = ctx.history.filter((h) => h.status === 'open' && h.basis);
    // Ours still open are offered again when still found (kept as they are),
    // else replaced; the others' open ones block their keys and meanings.
    const fresh = rank(all, { weight, history: ctx.history.filter((h) => !(h.status === 'open' && h.basis)), now: ctx.now, max: MAX_NEW });
    const keep = new Set(fresh.map((c) => c.key));
    const room = Math.max(2, MAX_OPEN - openOthers);
    const kept = mineOpen.filter((h) => keep.has(h.signal));
    const keptKeys = new Set(kept.map((h) => h.signal));
    let add = fresh.filter((c) => !keptKeys.has(c.key)).slice(0, Math.max(0, room - kept.length));
    const head = Number(ctx.view.comp.n);
    const retire = mineOpen.filter((h) => !keep.has(h.signal));
    if (retire.length) {
      await this.store.db.write(retire.map((h) => ({ op: 'set', table: 'sa_suggestions', key: h.id, field: 'status', value: Number(parse(h.change)?.at) !== head ? 'stale' : 'replaced' }))).catch(() => {});
    }
    if (add.length) add = await this.phrase(ctx, add);
    const docs = add.map((c) => ({ text: c.text, why: c.why || '', source: c.source, cap: c.cap || '', signal: c.key, basis: c.basis, confidence: c.score,
      implies: JSON.stringify({ ...(c.change || {}), at: head }) }));
    const added = docs.length ? await this.store.putSuggestions(appId, docs) : [];
    this.last.set(appId, { at: Date.now(), head, events: ctx.events.length });
    if (added.length || retire.length) this.emit('suggestions', { app: appId });
    if (added.length) this.log(`[${appId}] ${added.length} recommended (${reason}): ${added.map((a) => `${a.basis} ${a.confidence}`).join(', ')}`);
    return { added, candidates: rank(all, { weight, history: [], now: ctx.now, max: 50 }) };
  }

  // What it would recommend now, and every candidate scored, stored nowhere.
  async preview(appId) {
    const ctx = await this.context(appId);
    if (!ctx.view.comp.n) return { next: [], candidates: [] };
    const all = await this.candidates(ctx);
    const weight = await this.weights(appId);
    const shape = (c) => ({ key: c.key, basis: c.basis, source: c.source, text: c.text, why: c.why, confidence: Math.round(c.confidence * 1000) / 1000, weight: Math.round(weight(c.basis) * 100) / 100, score: c.score, change: c.change || null });
    return {
      next: rank(all, { weight, history: ctx.history.filter((h) => !(h.status === 'open' && h.basis)), now: ctx.now }).map(shape),
      candidates: rank(all, { weight, history: [], now: ctx.now, max: 50 }).map(shape),
      signals: ctx.signals.length, events: ctx.events.length,
    };
  }

  // In the background, one app at a time, never while the app builds:
  // after a build (delay), or when use comes in or the app is opened
  // (at most every few minutes, and only when its version or its use moved).
  soon(appId, { delay = 0, reason = 'soon', force = false } = {}) {
    if (this.pending.has(appId)) return;
    const l = this.last.get(appId);
    if (!force && l && Date.now() - l.at < MIN_GAP) return;
    this.pending.add(appId);
    const run = async () => {
      if (delay) await new Promise((r) => { const t = setTimeout(r, delay); t.unref?.(); });
      try {
        if (this.busy(appId)) return;
        if (!force && l) {
          const app = await this.store.getApp(appId);
          const n = (await this.store.recentEvents(appId, 1500).catch(() => [])).length;
          if (Number(app?.head) === l.head && n === l.events) { l.at = Date.now(); return; }
        }
        await this.refresh(appId, { reason });
      } catch (e) { this.log(`[${appId}] recommend: ${e.message}`); } finally { this.pending.delete(appId); }
    };
    this.queue = this.queue.then(run, run);
    return this.queue;
  }
}

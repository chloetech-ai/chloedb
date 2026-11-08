// Apps that evolve from use. The engine counts patterns in what people do
// (a search typed again and again, a filter set every visit, the same field
// changed on row after row by hand, two views always opened one after the
// other), asks the model what small feature would answer the strongest new
// one, and builds it on its own as a version beside the live one: a
// candidate. The owner tries it on the real data, then keeps it (it goes
// live, merged over whatever changed meanwhile) or drops it. Each candidate
// names what should change in people's use if it works (its measure); after
// it is kept, the same count before and after says whether it did. Every
// event records the version it happened on, so the same measure can later
// compare two versions served side by side (an A/B test).

import { newId } from './store.mjs';

export const DEFAULT_MODE = ['off', 'suggest', 'build'].includes(process.env.STRATA_APPS_EVOLVE) ? process.env.STRATA_APPS_EVOLVE : 'build';
export const MODES = { off: 'off', suggest: 'suggest only', build: 'build to try' };
const PER_DAY = Number(process.env.STRATA_APPS_EVOLVE_PER_DAY || 2);
const MAX_WAITING = 2;
const DAY = 86400000;

// How often a pattern must show before it counts.
export const THRESHOLDS = { search: 3, filter: 4, edits: 5, adds: 6, deletes: 5, path: 4, call: 5 };
const SITTING = 10 * 60 * 1000; // a gap longer than this starts a new sitting
const TYPING = 20 * 1000;       // searches closer than this, one extending the other, are one search being typed
const NEXT = 30 * 1000;         // a view opened within this of another follows it

const when = (e, i) => {
  const t = Date.parse(String(e.ts || '').replace(' ', 'T'));
  return Number.isFinite(t) ? t : i * 1000;
};
const actionOf = (e, verb) => (e.kind === 'action' && String(e.detail || '').startsWith(`${verb} `) ? String(e.detail).slice(verb.length + 1).trim() : null);

// Patterns in an app's events (newest first, as the store gives them):
// [{key, kind, text, count, people, last}], strongest first. `views` (the
// app's view capabilities) name views by their titles.
export function signals(events, { views = [] } = {}) {
  const title = (slug) => views.find((v) => v.slug === slug)?.title || slug;
  const evs = events.slice().reverse().map((e, i) => ({ ...e, t: when(e, i), who: e.person || 'someone' }));
  const found = new Map();
  // A repetition in one sitting: rows changed, added or deleted one by one.
  const add = (key, kind, who, t, describe, n) => {
    const s = found.get(key) || { key, kind, count: 0, people: new Set(), last: 0, sittings: 0, most: 0, describe };
    s.count += n;
    s.sittings++;
    s.most = Math.max(s.most, n);
    s.people.add(who);
    s.last = Math.max(s.last, t);
    found.set(key, s);
  };
  const byWho = new Map();
  for (const e of evs) {
    if (!byWho.has(e.who)) byWho.set(e.who, []);
    byWho.get(e.who).push(e);
  }
  const counts = { search: new Map(), filter: new Map(), call: new Map(), path: new Map() };
  const bump = (m, k, who, t) => { const c = m.get(k) || { n: 0, people: new Set(), last: 0 }; c.n++; c.people.add(who); c.last = Math.max(c.last, t); m.set(k, c); };
  for (const [who, list] of byWho) {
    // Searches: the query someone stopped at, not each letter typed.
    const searches = list.map((e) => ({ e, q: actionOf(e, 'search') })).filter((x) => x.q !== null);
    searches.forEach((x, i) => {
      const next = searches[i + 1];
      const q = x.q.toLowerCase();
      if (next && next.e.t - x.e.t < TYPING) {
        const nq = next.q.toLowerCase();
        if (nq !== q && (nq.startsWith(q) || q.startsWith(nq))) return;
      }
      if (q.length >= 2) bump(counts.search, q, who, x.e.t);
    });
    for (const e of list) {
      const f = actionOf(e, 'filter');
      if (f && !/=(|null|all|undefined)$/i.test(f)) bump(counts.filter, f, who, e.t);
      if (e.kind === 'call' && e.detail) bump(counts.call, String(e.detail), who, e.t);
    }
    // Views opened one right after the other.
    const opens = list.filter((e) => e.kind === 'open' && e.detail);
    for (let i = 1; i < opens.length; i++) {
      const a = opens[i - 1];
      const b = opens[i];
      if (a.detail !== b.detail && b.t - a.t < NEXT) bump(counts.path, `${a.detail}>${b.detail}`, who, b.t);
    }
    // Sittings: the same change, add or delete again and again in one go.
    let sitting = [];
    const close = () => {
      const by = new Map();
      for (const e of sitting) {
        if (!['update', 'insert', 'delete'].includes(e.kind)) continue;
        const k = `${e.kind}|${e.detail}`;
        by.set(k, (by.get(k) || 0) + 1);
      }
      const last = sitting.at(-1)?.t || 0;
      for (const [k, n] of by) {
        const [kind, detail] = k.split('|');
        if (kind === 'update' && n >= THRESHOLDS.edits) {
          const [table, fields] = detail.split(': ');
          add(`edits:${detail}`, 'edits', who, last, `changed ${fields || 'rows'} of ${table} by hand, row after row`, n);
        }
        if (kind === 'insert' && n >= THRESHOLDS.adds) add(`adds:${detail}`, 'adds', who, last, `added rows to ${detail} one by one`, n);
        if (kind === 'delete' && n >= THRESHOLDS.deletes) add(`deletes:${detail}`, 'deletes', who, last, `deleted rows of ${detail} one by one`, n);
      }
      sitting = [];
    };
    for (const e of list) {
      if (sitting.length && e.t - sitting.at(-1).t > SITTING) close();
      sitting.push(e);
    }
    close();
  }
  const people = (c) => (c.people.size > 1 ? ` (${c.people.size} people)` : '');
  for (const s of found.values()) {
    s.text = `${s.describe}: ${s.count} times${s.sittings > 1 ? ` over ${s.sittings} sittings, up to ${s.most} in one` : ' in one sitting'}${s.people.size > 1 ? `, ${s.people.size} people` : ''}`;
  }
  for (const [q, c] of counts.search) if (c.n >= THRESHOLDS.search) found.set(`search:${q}`, { key: `search:${q}`, kind: 'search', count: c.n, people: c.people, last: c.last, text: `searched for "${q}" ${c.n} times${people(c)}` });
  for (const [f, c] of counts.filter) if (c.n >= THRESHOLDS.filter) found.set(`filter:${f}`, { key: `filter:${f}`, kind: 'filter', count: c.n, people: c.people, last: c.last, text: `set the filter ${f} ${c.n} times${people(c)}` });
  for (const [slug, c] of counts.call) if (c.n >= THRESHOLDS.call) found.set(`call:${slug}`, { key: `call:${slug}`, kind: 'call', count: c.n, people: c.people, last: c.last, text: `ran the function ${slug} by hand ${c.n} times${people(c)}` });
  for (const [p, c] of counts.path) {
    if (c.n < THRESHOLDS.path) continue;
    const [a, b] = p.split('>');
    found.set(`path:${p}`, { key: `path:${p}`, kind: 'path', count: c.n, people: c.people, last: c.last, text: `went from ${title(a)} straight to ${title(b)} ${c.n} times${people(c)}` });
  }
  return [...found.values()]
    .map((s) => ({ key: s.key.slice(0, 200), kind: s.kind, text: s.text, count: s.count, people: s.people.size, last: new Date(s.last).toISOString(), strength: s.count / THRESHOLDS[s.kind] * (1 + 0.5 * (s.people.size - 1)) }))
    .sort((a, b) => b.strength - a.strength)
    .slice(0, 10);
}

// What should change in people's use if a feature answers the pattern: the
// events to count, and which way they should go.
export function measureFor(signal) {
  const [kind, ...rest] = String(signal.key || signal).split(':');
  const what = rest.join(':');
  switch (kind) {
    case 'search': return { event: 'action', match: `search ${what}`, goal: 'fewer', says: `searches for "${what}"` };
    case 'filter': return { event: 'action', match: `filter ${what}`, goal: 'fewer', says: `times the filter ${what} is set by hand` };
    case 'edits': return { event: 'update', match: what, goal: 'fewer', says: `changes of ${what} made one row at a time` };
    case 'adds': return { event: 'insert', match: what, goal: 'fewer', says: `rows added to ${what} one at a time` };
    case 'deletes': return { event: 'delete', match: what, goal: 'fewer', says: `rows of ${what} deleted one at a time` };
    case 'path': return { event: 'open', match: what.split('>')[1], goal: 'fewer', says: `times ${what.split('>')[1]} is opened` };
    case 'call': return { event: 'call', match: what, goal: 'fewer', says: `runs of ${what} by hand` };
    default: return null;
  }
}

// The measure's events a day, before and after a moment (when a candidate
// went live). Events are newest first.
export function measureEffect(events, measure, since) {
  if (!measure || !since) return null;
  const t0 = Date.parse(String(since).replace(' ', 'T'));
  if (!Number.isFinite(t0)) return null;
  const match = String(measure.match || '').toLowerCase();
  let before = 0;
  let after = 0;
  let first = t0;
  let last = t0;
  events.forEach((e, i) => {
    const t = when(e, -i);
    if (e.kind !== measure.event) return;
    if (match && !String(e.detail || '').toLowerCase().includes(match)) return;
    if (t < t0) { before++; first = Math.min(first, t); } else { after++; last = Math.max(last, t); }
  });
  const days = (ms) => Math.max(ms / DAY, 1 / 24);
  const now = Date.now();
  return { before, after, beforeDays: days(t0 - first), afterDays: days(Math.max(now, last) - t0), perDayBefore: before / days(t0 - first), perDayAfter: after / days(Math.max(now, last) - t0) };
}

// Keeping a candidate after the live version moved on: its own changes (what
// it changed or removed from the version it was built on) applied over the
// live version, unless the live version changed the same pieces too.
const keyOf = (id) => id.slice(0, id.lastIndexOf('@'));
export function mergeCaps(base, cand, head) {
  const map = (ids) => new Map(ids.map((id) => [keyOf(id), id]));
  const B = map(base);
  const C = map(cand);
  const H = map(head);
  const changed = [...C].filter(([k, id]) => B.get(k) !== id);
  const removed = [...B.keys()].filter((k) => !C.has(k));
  const conflicts = [...changed.map(([k]) => k), ...removed].filter((k) => H.get(k) !== B.get(k));
  if (conflicts.length) return { conflicts };
  const M = new Map(H);
  for (const [k, id] of changed) M.set(k, id);
  for (const k of removed) M.delete(k);
  return { caps: [...M.values()], changed: changed.map(([k]) => k), removed };
}

export class Evolver {
  constructor({ engine, submit, broadcast = () => {}, log = () => {}, propose }) {
    this.engine = engine;
    this.store = engine.store;
    this.submit = submit;
    this.broadcast = broadcast;
    this.log = log;
    this.propose = propose; // (engine, appId, {signals, fresh}) -> stored suggestions
    this.busy = new Set();
  }

  async mode(appId) {
    const m = await this.store.setting(appId, 'evolve').catch(() => null);
    return MODES[m] ? m : DEFAULT_MODE;
  }

  async setMode(appId, mode) {
    if (!MODES[mode]) throw Object.assign(new Error(`evolve is one of ${Object.keys(MODES).join(', ')}`), { status: 400 });
    await this.store.putSetting(appId, 'evolve', mode);
    this.broadcast('candidates', { app: appId });
    return mode;
  }

  // What the studio shows: the mode, the patterns seen, and the candidates
  // (a kept one with its measure before and after).
  async state(appId, view) {
    const [mode, events, cands] = await Promise.all([this.mode(appId), this.store.recentEvents(appId, 1500), this.store.candidates(appId, 12)]);
    const answered = new Set(cands.map((c) => c.signal));
    return {
      mode,
      signals: signals(events, view).map((s) => ({ ...s, answered: answered.has(s.key) })),
      candidates: cands.map((c) => {
        const measure = parse(c.measure);
        return { id: c.id, text: c.text, why: c.why, signal: c.signal, status: c.status, n: Number(c.n) || null, base: Number(c.base) || null, kept: Number(c.kept) || null,
          summary: c.summary || '', problems: parse(c.problems) || [], cost: Number(c.cost) || 0, created: c.created, decided: c.decided || null,
          measure, effect: c.status === 'kept' && c.decided ? measureEffect(events, measure, c.decided) : null };
      }),
    };
  }

  // Looks at the app's use: new patterns get suggestions (one model call), and
  // in "build" mode the strongest answered one is built as a candidate.
  // `force` asks the model even with no new pattern (the studio's button).
  async tick(appId, { force = false } = {}) {
    if (this.busy.has(appId)) return { suggestions: [], building: null, busy: true };
    this.busy.add(appId);
    try {
      const mode = await this.mode(appId);
      if (mode === 'off' && !force) return { suggestions: [], building: null };
      const view = await this.store.resolve(appId);
      if (!view.comp.n) return { suggestions: [], building: null };
      const [events, cands, history] = await Promise.all([this.store.recentEvents(appId, 1500), this.store.candidates(appId, 50), this.store.suggestionHistory(appId, 80)]);
      const sigs = signals(events, view);
      const answered = new Set([...cands.map((c) => c.signal), ...history.map((h) => h.signal)].filter(Boolean));
      const fresh = sigs.filter((s) => !answered.has(s.key));
      if (!fresh.length && !force) return { suggestions: [], building: null, signals: sigs };
      const suggestions = await this.propose(this.engine, appId, { signals: sigs, fresh });
      this.log(`[${appId}] ${suggestions.length} suggestions from use (${fresh.length} new pattern${fresh.length === 1 ? '' : 's'}: ${fresh.map((s) => s.key).join(', ') || 'none'})`);
      let building = null;
      if (mode === 'build') {
        const now = Date.now();
        const waiting = cands.filter((c) => c.status === 'building' || c.status === 'ready').length;
        const today = cands.filter((c) => now - Date.parse(String(c.created).replace(' ', 'T')) < DAY).length;
        const pick = suggestions.find((s) => s.signal && fresh.some((f) => f.key === s.signal));
        if (pick && waiting < MAX_WAITING && today < PER_DAY) building = await this.build(appId, pick, fresh.find((f) => f.key === pick.signal));
        else if (pick) this.log(`[${appId}] not building "${pick.text}": ${waiting >= MAX_WAITING ? `${waiting} candidates wait` : `${today} built today`}`);
      }
      return { suggestions, building, signals: sigs };
    } finally {
      this.busy.delete(appId);
    }
  }

  async build(appId, suggestion, signal) {
    const app = await this.store.getApp(appId);
    const cand = { id: newId(10), app: appId, text: suggestion.text, why: suggestion.why || signal.text, signal: signal.key,
      measure: JSON.stringify(measureFor(signal)), suggestion: suggestion.id, base: Number(app.head), status: 'building' };
    await this.store.putCandidate(cand);
    await this.store.setSuggestion(suggestion.id, 'building');
    const req = await this.submit(appId, suggestion.text, 'evolve', { candidate: cand.id });
    await this.store.putCandidate({ ...cand, request: req.id });
    this.log(`[${appId}] building a candidate from use: "${suggestion.text}" (${signal.text})`);
    this.broadcast('candidates', { app: appId });
    return { ...cand, request: req.id };
  }

  // The forge finished a candidate's build (or failed it).
  async built(candId, out) {
    const c = await this.store.candidate(candId);
    if (!c) return;
    await this.store.putCandidate({ ...c, status: 'ready', n: out.n, base: out.base ?? c.base, summary: out.summary || '', problems: JSON.stringify(out.problems || []), cost: out.cost || 0, ms: out.ms || 0 });
    this.broadcast('candidates', { app: c.app });
  }

  async failed(candId, error) {
    const c = await this.store.candidate(candId);
    if (!c) return;
    await this.store.putCandidate({ ...c, status: 'failed', problems: JSON.stringify([String(error).slice(0, 500)]) });
    if (c.suggestion) await this.store.setSuggestion(c.suggestion, 'open');
    this.broadcast('candidates', { app: c.app });
  }

  // Keeps a candidate: it becomes the live version, merged over what changed
  // since it was built; when the same pieces changed on both sides it is
  // built again on the live version, as a request.
  async keep(appId, candId) {
    const c = await this.store.candidate(candId);
    if (!c || c.app !== appId) throw Object.assign(new Error('no such candidate'), { status: 404 });
    if (c.status !== 'ready') throw Object.assign(new Error(`this candidate is ${c.status}, not ready`), { status: 409 });
    const app = await this.store.getApp(appId);
    const decided = new Date().toISOString();
    let head;
    let how;
    if (Number(app.head) === Number(c.base)) {
      await this.store.setHead(appId, Number(c.n));
      head = Number(c.n);
      how = 'as built';
    } else {
      const [B, C, H] = await Promise.all([c.base, c.n, app.head].map((n) => this.store.composition(appId, Number(n))));
      const m = mergeCaps(B.caps, C.caps, H.caps);
      if (m.conflicts) {
        const req = await this.submit(appId, c.text, 'suggestion');
        await this.store.putCandidate({ ...c, status: 'kept', decided, problems: JSON.stringify([`built again on version ${app.head}, which changed ${m.conflicts.map((k) => k.split('/').slice(1).join(' ')).join(', ')} too`]) });
        if (c.suggestion) await this.store.setSuggestion(c.suggestion, 'taken');
        this.broadcast('candidates', { app: appId });
        return { how: 'rebuilt', request: req.id };
      }
      head = await this.store.writeComposition(appId, m.caps, `Keep: ${c.text}`, c.summary || c.text);
      how = `merged over version ${app.head}`;
    }
    await this.store.putCandidate({ ...c, status: 'kept', kept: head, decided });
    if (c.suggestion) await this.store.setSuggestion(c.suggestion, 'taken');
    this.log(`[${appId}] kept candidate ${c.id} (${how}): version ${head} is live`);
    this.broadcast('app', { app: appId, head });
    this.broadcast('candidates', { app: appId });
    return { how, head };
  }

  async drop(appId, candId) {
    const c = await this.store.candidate(candId);
    if (!c || c.app !== appId) throw Object.assign(new Error('no such candidate'), { status: 404 });
    if (c.status === 'kept') throw Object.assign(new Error('this candidate is live already: go back to an earlier version instead'), { status: 409 });
    await this.store.putCandidate({ ...c, status: 'dropped', decided: new Date().toISOString() });
    if (c.suggestion) await this.store.setSuggestion(c.suggestion, 'dismissed');
    this.broadcast('candidates', { app: appId });
    return { dropped: c.id };
  }
}

function parse(s) {
  if (!s) return null;
  try { return JSON.parse(s); } catch { return null; }
}

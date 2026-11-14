// The sources of the next-edit recommender (recommend.mjs). Each source
// reads one kind of evidence and proposes candidates; none knows what kind
// of app it reads, nor the words a request uses:
//
//   use.*    what people did (sa_events): the evolver's patterns (a path
//            between two screens, a search or filter set again and again,
//            a field changed row after row, forms given up...), screens
//            nobody opens, screens used mostly on a phone, errors that keep
//            coming back, actions a rule refused
//   start.*  what holds with no use yet: the app's own checks, the library's
//            patterns and shared components that fit it and it does not use,
//            what apps like it have (found by ChloeDB's search over every
//            app's pieces), and what its requests asked that no screen does
//
// A candidate: {key, basis, source ('use' | 'start'), text, why, confidence
// (0..1, the source's own), change: {kind, ..., hint} | null, cap}. `key`
// says what it answers, so the same thing is offered once (an evolver
// pattern's key for a pattern); `basis` names the source, whose weight is
// learned from what people take and dismiss; `change` is the edit it implies
// when known, and its hint goes with the request when it is taken, so the
// build starts from it.

import { checkStatic } from './look.mjs';

const DAY = 86400000;
// Below this cosine, a library piece or another app's is too far from this app to offer.
export const MEANING_FIT = Number(process.env.STRATA_APPS_RECOMMEND_FIT || 0.28);
// A piece only one other app has is offered only this close.
const PEER_ALONE = 0.38;
const tsOf = (e) => { const t = Date.parse(String(e?.ts || '').replace(' ', 'T')); return Number.isFinite(t) ? t : 0; };
const clip = (s, n) => { const t = String(s || '').replace(/\s+/g, ' ').trim(); return t.length <= n ? t : `${t.slice(0, n - 1).replace(/\s+\S*$/, '')}…`; };
const plural = (n, one, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;
// A source's own confidence from how strong its evidence is (1 = just
// enough to count), lower the older the evidence is.
export const strengthToConfidence = (strength, ageDays = 0) => (1 - Math.exp(-Math.max(0, strength) / 2)) * Math.exp(-Math.max(0, ageDays) / 30);

// ---- words

// The words of a text that carry its meaning: letters and digits, lowered,
// four characters or more (short words are mostly glue in any language),
// without the commonest glue of longer ones, a plural cut to its singular.
const GLUE = new Set('make made with each that this have from them they their there what when then also more some please want like need just into your about only sure look looks would could should every other than them will been were which where while very much many able'.split(' '));
export function wordsOf(text) {
  return new Set(String(text || '').toLowerCase().normalize('NFKD').replace(/[\u0300-\u036f]/g, '')
    .split(/[^\p{L}\p{N}]+/u).filter((w) => w.length >= 4 && !GLUE.has(w))
    .map((w) => (w.length > 4 && w.endsWith('ies') ? `${w.slice(0, -3)}y` : w.length > 4 && w.endsWith('s') && !w.endsWith('ss') ? w.slice(0, -1) : w)));
}
// How much of `a` is in `b` (sets of words).
export const covered = (a, b) => (a.size ? [...a].filter((w) => b.has(w)).length / a.size : 1);
export const jaccard = (a, b) => { const n = [...a].filter((w) => b.has(w)).length; return n / ((a.size + b.size - n) || 1); };

// The asks in a request: its sentences, without the data pasted with it
// (lines of numbers and commas, headings ending in a colon) and without
// ones too short or too long to be one ask.
export function asksOf(text) {
  const out = [];
  for (const line of String(text || '').split(/\n+/)) {
    const l = line.trim();
    if (!l || /:\s*$/.test(l)) continue;
    const punct = (l.match(/[,;|\t]/g) || []).length;
    const digits = (l.match(/\d/g) || []).length;
    if (punct >= 3 || digits / l.length > 0.15) continue;
    for (const s of l.split(/(?<=[.!?])\s+/)) {
      const t = s.trim().replace(/^[-*•\d.)\s]+/, '');
      const n = t.split(/\s+/).length;
      if (n >= 4 && n <= 45) out.push(t);
    }
  }
  return out;
}

// ---- what the app is

const slugTitle = (ctx, slug) => ctx.view.views.find((v) => v.slug === slug)?.title || slug;
const screenOf = (v) => { if (v.spec?.screen && typeof v.spec.screen === 'object') return v.spec.screen; try { return JSON.parse(v.code); } catch { return null; } };

// What the app is about, in words: for searching the library with.
export function appText(view) {
  const desc = asksOf(view.app.description || '').join(' ');
  return clip([view.app.name, desc, ...view.tables.map((t) => `${t.spec.name} ${(t.spec.columns || []).map((c) => c.name).join(' ')}`),
    ...view.views.map((v) => `${v.title}. ${v.description || ''}`), ...view.functions.map((f) => f.description || f.slug)].join('. ').replace(/_/g, ' '), 1800);
}

// Everything the app says it does, as words: its pieces, its screens' code,
// its versions' summaries.
export function builtWords(view, versions = []) {
  return wordsOf([view.app.name, ...view.caps.map((c) => `${c.title || ''} ${c.description || ''} ${c.slug}`),
    ...view.views.map((v) => v.code || ''), ...view.functions.map((f) => f.code || ''), ...versions.map((c) => `${c.summary || ''}`)].join(' ').replace(/_/g, ' '));
}

// ---- use

// What answers each kind of pattern the evolver counts (evolve.mjs
// signals): the remedy for the shape of the use, written as the person
// would ask, and the change it implies.
function remedy(sig, ctx) {
  const [kind, ...rest] = String(sig.key).split(':');
  const what = rest.join(':');
  const t = (s) => slugTitle(ctx, s);
  switch (kind) {
    case 'path': {
      const [a, b] = what.split('>');
      return { text: `On ${t(a)}, show what I go to ${t(b)} for, or a link straight to it`, change: { kind: 'link', from: a, to: b, hint: `Change ${a} only: put what people open ${b} for on it (a summary of it, or a link to it).` } };
    }
    case 'search': return { text: `Keep "${what}" one tap away (a saved view or a tab), so I do not search for it every time`, change: { kind: 'saved-view', q: what, hint: `A saved view or tab that shows the rows people find by searching "${what}".` } };
    case 'filter': return { text: `Open with ${what} already set, or give it a tab of its own`, change: { kind: 'default-filter', filter: what, hint: `Make ${what} the default filter (or a tab) where people set it.` } };
    case 'nothing': {
      const q = what.split(':').slice(1).join(':');
      return { text: `Make looking for "${q}" find something: search more columns or by meaning, or say what to try instead`, change: { kind: 'search', q, hint: `People look for "${q}" and find nothing: widen the search (more columns, by meaning) or show what is there.` } };
    }
    case 'edits': {
      const [tbl, fields] = what.split(': ');
      const f = String(fields || '').split(',').filter(Boolean).join(', ') || 'rows';
      return { text: `Let me change ${f} of ${tbl.replace(/_/g, ' ')} right in the list, or on many rows at once`, change: { kind: 'inline-edit', table: tbl, fields: String(fields || '').split(',').filter(Boolean), hint: `Make ${f} of ${tbl} editable in place in its list (or a bulk action), instead of one form per row.` } };
    }
    case 'adds': return { text: `Let me add several ${what.replace(/_/g, ' ')} at once`, change: { kind: 'bulk-add', table: what, hint: `A quick way to add many ${what} rows at once (a quick-add row, or paste a list).` } };
    case 'deletes': return { text: `Let me select several ${what.replace(/_/g, ' ')} and remove them together`, change: { kind: 'bulk-delete', table: what, hint: `Select many ${what} rows and remove them in one action.` } };
    case 'undo': return { text: `Make the ${what.replace(/_/g, ' ')} form harder to get wrong`, change: { kind: 'form', table: what, hint: `Rows of ${what} are often removed minutes after they are added: make its form clearer (defaults, a check before saving).` } };
    case 'abandon': {
      const [screen, form] = [what.slice(0, what.indexOf('/')), what.slice(what.indexOf('/') + 1)];
      const tbl = form.split(' ')[0];
      return { text: `Make the ${tbl.replace(/_/g, ' ')} form${screen ? ` on ${t(screen)}` : ''} shorter, with defaults, so it gets finished`, change: { kind: 'form', table: tbl, screen, hint: `People give up the ${form} form${screen ? ` on ${screen}` : ''}: ask fewer fields, fill defaults.` } };
    }
    case 'call': return { text: `Run ${what} on its own (when rows change, or on a schedule) instead of by hand`, change: { kind: 'trigger', fn: what, hint: `Run the function ${what} as a trigger or on a schedule, where people now run it by hand.` } };
    case 'ux': return { text: `Make it quicker to do what the app is for: ${sig.text}`, change: { kind: 'ux', measure: what, hint: `Make the main job quicker (fewer taps, on the first screen), without adding to it: ${sig.text}.` } };
    default: return null;
  }
}

// The evolver's patterns, each with its remedy.
export const usePatterns = {
  id: 'use.patterns',
  source: 'use',
  async run(ctx) {
    const out = [];
    for (const s of ctx.signals || []) {
      const r = remedy(s, ctx);
      if (!r) continue;
      const kind = String(s.key).split(':')[0];
      const age = s.last ? (ctx.now - Date.parse(s.last)) / DAY : 0;
      out.push({ key: s.key, basis: `use.${kind}`, source: 'use', text: r.text, why: `People ${s.text}.`, confidence: strengthToConfidence(s.strength ?? 1, age), change: r.change, signal: s.key });
    }
    return out;
  },
};

// Screens nobody opened while the others were opened again and again.
export const useUnopened = {
  id: 'use.unopened',
  source: 'use',
  async run(ctx) {
    const opens = ctx.events.filter((e) => e.kind === 'open' && e.detail);
    if (opens.length < 12 || new Set(opens.map((e) => e.session || e.person)).size < 3) return [];
    const out = [];
    for (const v of ctx.view.views) {
      const since = tsOf({ ts: v.created });
      const after = opens.filter((e) => tsOf(e) >= since);
      const mine = after.filter((e) => e.detail === v.slug).length;
      if (mine || after.length < 12) continue;
      const by = new Map();
      for (const e of after) by.set(e.detail, (by.get(e.detail) || 0) + 1);
      const [top] = [...by.entries()].sort((a, b) => b[1] - a[1]);
      if (!top) continue;
      out.push({ key: `unopened:${v.slug}`, basis: 'use.unopened', source: 'use', text: `Bring ${v.title} where people are: link it from ${slugTitle(ctx, top[0])}, or fold it in there`,
        why: `Nobody opened ${v.title} in ${plural(after.length, 'visit')} to the app; ${slugTitle(ctx, top[0])} was opened ${plural(top[1], 'time')}.`,
        confidence: Math.min(0.6, 0.25 + after.length / 200), change: { kind: 'link', from: top[0], to: v.slug, hint: `People never reach ${v.slug}: link it from ${top[0]} or fold it into it.` } });
    }
    return out;
  },
};

// Screens used mostly on a phone (and slower there).
export const usePhone = {
  id: 'use.phone',
  source: 'use',
  async run(ctx) {
    const by = new Map();
    for (const e of ctx.events) {
      if (!e.device || !['open', 'leave'].includes(e.kind) || !e.detail) continue;
      const s = by.get(e.detail) || { phone: 0, all: 0, phoneMs: [], otherMs: [] };
      if (e.kind === 'open') { s.all++; if (e.device === 'phone') s.phone++; }
      if (e.kind === 'leave' && Number(e.ms) > 0) (e.device === 'phone' ? s.phoneMs : s.otherMs).push(Number(e.ms));
      by.set(e.detail, s);
    }
    const avg = (a) => (a.length ? a.reduce((x, y) => x + y, 0) / a.length : 0);
    const out = [];
    for (const [slug, s] of by) {
      if (s.phone < 5 || s.phone / s.all < 0.5 || !ctx.view.views.some((v) => v.slug === slug)) continue;
      const slow = s.phoneMs.length >= 3 && s.otherMs.length >= 3 ? avg(s.phoneMs) / avg(s.otherMs) : 0;
      out.push({ key: `phone:${slug}`, basis: 'use.phone', source: 'use', text: `Make ${slugTitle(ctx, slug)} work best on a phone: the main action first, big targets, nothing to scroll sideways`,
        why: `${s.phone} of ${plural(s.all, 'visit')} to ${slugTitle(ctx, slug)} are from a phone${slow >= 1.5 ? `, and they take ${slow.toFixed(1)} times as long there` : ''}.`,
        confidence: Math.min(0.85, 0.3 + 0.35 * (s.phone / s.all) + (slow >= 1.5 ? 0.2 : 0)), change: { kind: 'layout', screen: slug, device: 'phone', hint: `Most visits to ${slug} are on a phone: lay it out for one (its main action on the first screenful).` } });
    }
    return out;
  },
};

// The same error, again and again, in code the app runs now; and actions a
// rule refused again and again (the person tried something the app says no to).
export const useErrors = {
  id: 'use.errors',
  source: 'use',
  async run(ctx) {
    const live = new Set(ctx.view.comp.caps || []);
    const groups = new Map();
    for (const e of ctx.events) {
      if (e.kind !== 'error' && e.kind !== 'refused') continue;
      if (e.kind === 'error' && e.cap && !live.has(e.cap)) continue;
      const msg = String(e.detail || '').replace(/^for a visitor, /, '');
      const k = `${e.kind}|${e.cap || ''}|${msg.slice(0, 80)}`;
      const g = groups.get(k) || { kind: e.kind, cap: e.cap, msg, n: 0, last: 0, people: new Set() };
      g.n++; g.last = Math.max(g.last, tsOf(e)); g.people.add(e.person || e.session || 'someone');
      groups.set(k, g);
    }
    const piece = (cap) => ctx.view.caps.find((c) => c.id === cap);
    const out = [];
    for (const g of groups.values()) {
      if (g.n < (g.kind === 'error' ? 2 : 3)) continue;
      const p = piece(g.cap);
      const where = p ? `${p.title || p.slug}` : 'the app';
      const age = (ctx.now - g.last) / DAY;
      if (g.kind === 'error') {
        out.push({ key: `error:${g.cap}:${g.msg.slice(0, 40)}`, basis: 'use.errors', source: 'use', text: `Fix ${where}: ${clip(g.msg, 140)}`, why: `It failed ${plural(g.n, 'time')}${g.people.size > 1 ? ` for ${g.people.size} people` : ''}, most lately ${Math.round(age) < 1 ? 'today' : `${Math.round(age)} days ago`}.`,
          confidence: strengthToConfidence(1 + g.n / 2, age), change: { kind: 'fix', cap: g.cap, hint: `Fix the cause of this error in ${p?.slug || 'the app'}: ${clip(g.msg, 300)}` } });
      } else {
        const [fn, ...m] = g.msg.split(': ');
        out.push({ key: `refused:${fn}:${m.join(': ').slice(0, 40)}`, basis: 'use.refused', source: 'use', text: `Say before people try it: "${clip(m.join(': ') || g.msg, 100)}" (show what is left, or keep the action off until it can work)`,
          why: `People were turned away ${plural(g.n, 'time')} by ${fn}${g.people.size > 1 ? `, ${g.people.size} of them` : ''}.`,
          confidence: strengthToConfidence(g.n / 3, age), change: { kind: 'rule-hint', fn, hint: `${fn} refuses this often ("${clip(m.join(': '), 120)}"): show the limit where people act, before they try.` } });
      }
    }
    return out;
  },
};

// ---- with no use yet

// The app's own checks that need no browser and no model (quality.mjs):
// labels that are column names, no main action, unreadable colors.
export const startChecks = {
  id: 'start.checks',
  source: 'start',
  async run(ctx) {
    let found = [];
    try { found = checkStatic(ctx.view, ctx.design); } catch { return []; }
    return found.slice(0, 4).map((f) => ({ key: `check:${f.view}:${clip(f.problem, 60)}`, basis: 'start.checks', source: 'start',
      text: `${f.view ? `On ${f.view}: ` : ''}${clip(f.fix || f.problem, 200)}`, why: `The app's own check found it: ${clip(f.problem, 200)}.`,
      confidence: f.severity === 'high' ? 0.7 : 0.4, change: { kind: 'fix', screen: f.view || null, hint: clip(`${f.problem}${f.fix ? `; ${f.fix}` : ''}`, 300) } }));
  },
};

// The library's patterns and shared components that fit what the app is
// about, which it does not use; the more apps here use one, the surer.
export const startLibrary = {
  id: 'start.library',
  source: 'start',
  async run(ctx) {
    const usedPatterns = new Set(ctx.view.views.flatMap((v) => v.spec?.patterns || []));
    const usedShared = new Set(ctx.view.shared.map((c) => `${c.kind}/${c.slug}`));
    const hits = await ctx.search(ctx.appText, { kinds: ['pattern', 'component'], limit: 12 });
    const out = [];
    for (const [i, h] of hits.entries()) {
      if (h.kind === 'pattern' ? usedPatterns.has(h.slug) : usedShared.has(`${h.kind}/${h.slug}`)) continue;
      // Close enough in meaning (MEANING_FIT), or found by its words alone.
      if (h.cosine !== undefined && Number(h.cosine) < MEANING_FIT) continue;
      const users = (ctx.usersOf(h) || []).filter((u) => u.app !== ctx.appId);
      const near = await ctx.near(`${h.title}. ${h.description || ''}`);
      if (near.covered) continue;
      const title = h.title.split(/[:;(]/)[0].trim();
      const cos = Number(h.cosine) || 0;
      const usedBy = users.length ? ` ${users.length === 1 ? 'Another app' : `${users.length} other apps`} here already ${users.length === 1 ? 'has' : 'have'} it (${users.slice(0, 3).map((u) => u.name).join(', ')}).` : '';
      out.push({ key: `lib:${h.kind}/${h.slug}`, basis: 'start.library', source: 'start', cap: h.id,
        text: `Add ${title.charAt(0).toLowerCase()}${title.slice(1)}`,
        // (in plain words: how close it came is the confidence, not the reason)
        why: clip(`It fits what this app is for, and the app has nothing like it yet.${usedBy}`, 240),
        confidence: Math.min(0.75, 0.2 + 0.5 * cos + 0.04 * Math.min(users.length, 5) + (i < 3 ? 0.05 : 0)) * (1 - 0.5 * near.best),
        change: h.kind === 'pattern'
          ? { kind: 'pattern', name: h.slug, ref: h.id, example: ctx.example(h.slug), hint: `The library's "${h.slug}" pattern does this: a block ${JSON.stringify(ctx.example(h.slug) || { use: h.slug })} on the screen where it fits, with this app's own tables and columns.` }
          : { kind: 'component', name: h.slug, ref: h.id, hint: `Use the shared component ui.${h.slug} (${clip(h.description, 160)}).` } });
    }
    return out;
  },
};

// What apps like this one have and it does not: the screens and functions
// of other apps here that fit what it is about (ChloeDB's search, by words
// and by meaning, over every app's pieces), each counted for every
// neighbour app that has one like it (collaborative filtering over apps).
export const startPeers = {
  id: 'start.peers',
  source: 'start',
  async run(ctx) {
    const hits = (await ctx.search(ctx.appText, { excludeApp: ctx.appId, kinds: ['view', 'function'], limit: 16 }))
      .filter((h) => h.app && h.app !== 'lib' && (h.cosine === undefined || Number(h.cosine) >= MEANING_FIT));
    if (!hits.length) return [];
    // A neighbour is an app with pieces that fit: the more and the better, the closer.
    const close = new Map();
    hits.forEach((h, i) => close.set(h.app, (close.get(h.app) || 0) + 1 / (1 + i)));
    const out = [];
    for (const h of hits.slice(0, 8)) {
      const near = await ctx.near(`${h.title}. ${h.description || ''}`);
      if (near.covered) continue;
      const appName = ctx.appName(h.app);
      // Others like it, in other neighbour apps: the same feature, more apps.
      const alike = hits.filter((o) => o.app !== h.app && jaccard(wordsOf(`${o.title} ${o.description}`), wordsOf(`${h.title} ${h.description}`)) >= 0.3).map((o) => o.app);
      const apps = [...new Set([h.app, ...alike])];
      const cos = Number(h.cosine) || 0;
      // One app's piece, not close: chance more than a pattern of apps like this.
      if (apps.length < 2 && cos < PEER_ALONE) continue;
      out.push({ key: `peer:${h.kind}/${String(h.title || h.slug).toLowerCase().slice(0, 60)}`, basis: 'start.peers', source: 'start', cap: h.id,
        text: `Add ${h.kind === 'function' ? 'a function' : 'a screen'} like ${appName}'s ${h.title}: ${clip(h.description, 120)}`,
        why: clip(`Apps like this one have it: ${apps.slice(0, 3).map((a) => ctx.appName(a)).join(', ')}${apps.length > 3 ? ` and ${apps.length - 3} more` : ''}; this app has nothing like it yet.`, 240),
        confidence: Math.min(0.7, 0.15 + 0.4 * cos + 0.1 * Math.min(apps.length - 1, 3) + 0.15 * Math.min(1, close.get(h.app))) * (1 - 0.5 * near.best),
        change: { kind: h.kind, ref: h.id, from: h.app, hint: `${appName} (another app here) has ${h.kind === 'function' ? 'a function' : 'a screen'} that does this: ${h.id}, "${h.title}". Build this app's own, on its own tables.` } });
    }
    return out;
  },
};

// What the app's requests asked that nothing it built says it does: each
// ask (a sentence of a request) against the app's pieces, screens and
// versions, by words and, when ChloeDB embeds, by meaning.
export const startAsks = {
  id: 'start.asks',
  source: 'start',
  async run(ctx) {
    const built = ctx.built;
    const out = [];
    const seen = new Set();
    const reqs = ctx.requests.filter((r) => r.status === 'done' && ['user', 'suggestion'].includes(r.source || 'user')).slice(0, 12);
    for (const r of reqs) {
      for (const a of asksOf(r.text).slice(0, 6)) {
        const w = wordsOf(a);
        if (w.size < 2 || seen.has([...w].sort().join(' '))) continue;
        seen.add([...w].sort().join(' '));
        const cov = covered(w, built);
        if (cov >= 0.5) continue;
        const near = await ctx.near(a);
        if (near.covered || near.best >= 0.5) continue;
        const missing = [...w].filter((x) => !built.has(x)).slice(0, 4);
        out.push({ key: `ask:${[...w].sort().join(' ').slice(0, 80)}`, basis: 'start.asks', source: 'start', text: clip(a.replace(/[.!]+$/, ''), 220),
          why: clip(`You asked for it${r.comp ? ` (version ${r.comp})` : ''}, and nothing the app has mentions ${missing.map((m) => `"${m}"`).join(', ')}.`, 240),
          confidence: Math.min(0.8, 0.3 + 0.5 * (1 - cov)) * (1 - near.best), change: { kind: 'ask', request: r.id, hint: `This was asked before and is not built yet: "${clip(a, 300)}".` } });
        if (out.length >= 4) return out;
      }
    }
    return out;
  },
};

export const SOURCES = [usePatterns, useUnopened, usePhone, useErrors, startChecks, startAsks, startLibrary, startPeers];

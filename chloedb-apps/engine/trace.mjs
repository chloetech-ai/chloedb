// A build's timeline: what the engine did and when (reading Strata, each
// model call, checking, storing), and what it decided for every piece of the
// app (use one from the library as it is, copy a starter, fork one, write a
// new one, write a new version of one, keep one). Stored per request in
// sa_traces and drawn by the studio.

export class Trace {
  constructor() {
    this.t0 = Date.now();
    this.spans = [];
    this.decisions = [];
  }

  now() { return Date.now() - this.t0; }

  // A span of work. kind: strata (reading or writing Strata), plan (the
  // model planning), model (one answer with the code), write (one piece's
  // writer), check (the forge's checks), fix (a writer again, for problems).
  start(kind, label, extra = {}) {
    const span = { kind, label, start: this.now(), end: null, ...extra };
    this.spans.push(span);
    return span;
  }

  end(span, extra = {}) {
    span.end = this.now();
    Object.assign(span, extra);
    return span;
  }

  async span(kind, label, fn, extra = {}) {
    const s = this.start(kind, label, extra);
    try { return await fn(s); } finally { if (s.end === null) this.end(s); }
  }

  // What a model call cost and how big it was.
  call(span, answer, promptChars) {
    const u = answer?.usage || {};
    return this.end(span, {
      cost: answer?.cost || 0,
      chars: answer?.text?.length || 0,
      prompt: promptChars || 0,
      tokens: { in: u.input_tokens || 0, cached: (u.cache_read_input_tokens || 0), cacheWrite: u.cache_creation_input_tokens || 0, out: u.output_tokens || 0 },
    });
  }

  decide(d) { this.decisions.push({ t: this.now(), ...d }); }

  toJSON() {
    const end = this.now();
    for (const s of this.spans) if (s.end === null) s.end = end;
    return { t0: this.t0, ms: end, spans: this.spans, decisions: this.decisions };
  }
}

// What a plan decided for each piece, against the app as it was (view), the
// library offered (library: whole views and functions; shared: components and
// helpers by kind/name) and the shared pieces the new version pins.
export function planDecisions({ plan, view, library, shared, pinned, written, code = {}, at }) {
  const out = [];
  const had = new Map([...view.views, ...view.functions].map((c) => [`${c.kind}/${c.slug}`, c]));
  const hadShared = new Map(view.shared.map((c) => [`${c.kind}/${c.slug}`, c]));
  const tables = new Map(view.tables.map((t) => [t.spec.name, t.spec]));
  for (const t of plan.tables || []) {
    const old = tables.get(t.name);
    const added = (t.columns || []).filter((c) => !old?.columns?.find((x) => x.name === c.name)).map((c) => c.name);
    if (!old) out.push({ t: at, decision: 'new', kind: 'table', name: t.name, title: t.name, detail: `${(t.columns || []).length} columns: ${(t.columns || []).map((c) => c.name).join(', ')}` });
    else if (added.length) out.push({ t: at, decision: 'change', kind: 'table', name: t.name, title: t.name, detail: `added ${added.join(', ')}` });
  }
  for (const [kind, list] of [['view', plan.views], ['function', plan.functions]]) {
    for (const item of list || []) {
      const before = had.get(`${kind}/${item.slug}`);
      const src = item.forked_from ? library.find((c) => c.id === item.forked_from) : null;
      const decision = item.forked_from ? 'fork' : before ? 'change' : 'new';
      out.push({ t: at, decision, kind, name: item.slug, title: item.title || item.slug, detail: item.description || item.spec || '',
        from: item.forked_from ? { id: item.forked_from, title: src?.title || item.forked_from, starter: src?.app === 'lib' } : before ? { id: before.id, title: before.title } : null,
        piece: item.slug, chars: (code[item.slug] || '').length, ...(item.screen ? { screen: true } : {}), ...(item.patched ? { patched: item.patched } : {}), ...(item.patterns ? { patterns: item.patterns } : {}), ...(item.statement ? { statement: true } : {}) });
    }
  }
  for (const r of plan.reuse || []) {
    const src = library.find((c) => c.id === r.id);
    out.push({ t: at, decision: 'copy', kind: src?.kind || 'view', name: r.slug || src?.slug, title: r.title || src?.title || r.id, detail: src?.description || '',
      from: { id: r.id, title: src?.title || r.id, starter: src?.app === 'lib', author: src?.author } });
  }
  for (const plural of ['components', 'helpers']) {
    const kind = plural === 'components' ? 'component' : 'helper';
    const ns = kind === 'component' ? 'ui' : 'lib';
    for (const c of plan[plural] || []) {
      const prev = shared.get(`${kind}/${c.name}`);
      out.push({ t: at, decision: prev ? 'version' : 'new', kind, name: c.name, title: c.title || c.name, detail: prev ? (c.change || c.description || '') : (c.description || ''),
        from: prev ? { id: prev.id, title: prev.title, author: prev.author } : null, piece: `${ns}:${c.name}`, shared: true, chars: (code[`${ns}:${c.name}`] || '').length });
    }
  }
  for (const [key, id] of pinned || []) {
    if (written?.has(key)) continue;
    const [kind, name] = key.split('/');
    const before = hadShared.get(key);
    if (before && before.id === id) continue; // kept, counted below
    const c = [...shared.values()].find((x) => x.id === id) || before;
    out.push({ t: at, decision: before ? 'upgrade' : 'use', kind, name, title: c?.title || name, detail: c?.description || '',
      from: { id, title: c?.title || name, author: c?.author, builtin: c?.author === 'builtin' }, shared: true });
  }
  const planned = new Set([...(plan.views || []).map((v) => `view/${v.slug}`), ...(plan.functions || []).map((f) => `function/${f.slug}`)]);
  for (const s of plan.remove || []) {
    for (const kind of ['view', 'function']) if (had.has(`${kind}/${s}`)) { out.push({ t: at, decision: 'remove', kind, name: s, title: had.get(`${kind}/${s}`).title }); planned.add(`${kind}/${s}`); }
  }
  for (const [key, c] of had) if (!planned.has(key)) out.push({ t: at, decision: 'keep', kind: c.kind, name: c.slug, title: c.title });
  for (const [key, c] of hadShared) if (pinned?.get(key) === c.id && !written?.has(key)) out.push({ t: at, decision: 'keep', kind: c.kind, name: c.slug, title: c.title, shared: true });
  return out;
}

// How long each step of a build usually takes, first versions apart from
// changes, from the last builds' timelines (sa_traces): the studio's
// progress bar moves at the pace builds here really go.
// Before any build here, a minute or so (what builds took on a cluster in
// November 2026); after one, the steps are scaled to what builds here took
// in all (the median of their real durations), so the bar does not say
// "20 s left" of a build that takes a minute.
export const STEP_DEFAULTS = { first: { read: 300, plan: 25000, write: 25000, store: 8000 }, change: { read: 300, plan: 30000, write: 20000, store: 8000 } };
export function expectedSteps(traces, defaults = STEP_DEFAULTS) {
  const out = structuredClone(defaults);
  const seen = { first: {}, change: {} };
  const totals = { first: [], change: [] };
  for (const t of traces) {
    if (t.status !== 'done' || !Array.isArray(t.spans)) continue;
    const kind = t.spans.some((x) => /first version/.test(x.label || '')) ? 'first' : 'change';
    const span = (k) => { const ss = t.spans.filter((x) => k.includes(x.kind)); return ss.length ? Math.max(...ss.map((x) => x.end ?? x.start)) - Math.min(...ss.map((x) => x.start)) : 0; };
    const plan = span(['plan', 'model']);
    const write = span(['write', 'fix']);
    const read = t.spans.find((x) => x.kind === 'strata')?.end || 0;
    const store = Math.max(0, (t.ms || 0) - read - plan - write);
    for (const [k, v] of Object.entries({ read, plan, write, store })) if (v > 0 || k !== 'write') (seen[kind][k] ||= []).push(v);
    if (t.ms > 0) totals[kind].push(t.ms);
  }
  const median = (a) => { const s = [...a].sort((x, y) => x - y); return s[Math.floor(s.length / 2)]; };
  for (const kind of ['first', 'change']) {
    for (const [k, list] of Object.entries(seen[kind])) if (list.length >= 3) out[kind][k] = Math.round(median(list));
    // A change with no write step (a small one) does not count write in the sum.
    if (totals[kind].length) {
      const want = median(totals[kind]);
      const keys = Object.keys(out[kind]).filter((k) => k !== 'write' || seen[kind].write?.length);
      const sum = keys.reduce((a, k) => a + out[kind][k], 0);
      if (sum > 0) for (const k of keys) out[kind][k] = Math.round(out[kind][k] * want / sum);
    }
  }
  return out;
}

// How much it takes people to do what they came for, measured from use (the
// use log, sa_events) and judged against the app's brief: the taps and the
// seconds from opening the app to doing something, how much of it is done on
// the first screen, forms given up, rows added then removed, Undo used. The
// frame counts taps (any tap in the app, and the app's tabs) and each write
// carries the taps since the one before it.
//
// uxMeasures(events, {screens, brief}) -> {sittings, measures, findings, versions}
//   events: the store's events (any order), screens: [{slug, title, order}]

const WRITES = new Set(['insert', 'update', 'delete', 'call']);
const GAP = 30 * 60000; // a sitting ends after half an hour of nothing
const UNDO = 20000; // a row put back this soon after its delete: Undo
const WRONG = 10 * 60000; // a row removed this soon after it was added went wrong
export const LEAST = 5; // sittings (or writes, or forms) before a measure is judged

const median = (xs) => {
  if (!xs.length) return null;
  const s = [...xs].sort((a, b) => a - b);
  const m = s.length >> 1;
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
};
const share = (n, of) => (of ? n / of : null);
const pct = (x) => `${Math.round(x * 100)}%`;
const tsOf = (e) => Date.parse(String(e.ts || '').replace(' ', 'T'));

// What the brief asks of the job: done several times a day or daily, it is
// a tap or three from opening the app, in seconds; done less often, a little
// more. Most of what is done is done on the first screen (where the brief
// puts the job), few forms are given up, few rows are added by mistake.
export function targetsOf(brief) {
  const frequent = /several|daily/.test(String(brief?.often || '')) || !brief;
  return { taps: frequent ? 3 : 5, secs: frequent ? 15 : 30, first: 0.5, gaveUp: 0.25, wrong: 0.15, undo: 0.3 };
}

// Sittings: one person's (or tab's) events with no half-hour gap, oldest first.
export function sittingsOf(events) {
  const asc = events.map((e) => ({ ...e, t: tsOf(e) })).filter((e) => Number.isFinite(e.t)).sort((a, b) => a.t - b.t);
  const open = new Map();
  const out = [];
  for (const e of asc) {
    const who = e.session || e.person || 'someone';
    let s = open.get(who);
    if (!s || e.t - s.last > GAP) { s = { who, start: e.t, last: e.t, events: [] }; open.set(who, s); out.push(s); }
    s.events.push(e);
    s.last = e.t;
  }
  return out;
}

// Marks the Undos (a row put back right after its delete) and the rows added
// by mistake (removed within ten minutes, not by an Undo) across all events.
function mark(sittings) {
  const all = sittings.flatMap((s) => s.events).sort((a, b) => a.t - b.t);
  const deleted = new Map();
  const added = new Map();
  for (const e of all) {
    const k = e.tbl && e.rid ? `${e.tbl}/${e.rid}` : null;
    if (!k) continue;
    if (e.kind === 'delete') {
      deleted.set(k, e);
      const a = added.get(k);
      if (a && e.t - a.t < WRONG) a.wrong = e;
    } else if (e.kind === 'insert') {
      const d = deleted.get(k);
      if (d && e.t - d.t < UNDO) { e.undo = true; d.undone = true; }
      else added.set(k, e);
    }
  }
  // An add removed and put straight back was not a mistake.
  for (const e of all) if (e.kind === 'insert' && e.wrong && e.wrong.undone) e.wrong = null;
}

function measure(sittings, first) {
  const acts = (s) => s.events.filter((e) => WRITES.has(e.kind) && !e.undo);
  const acting = sittings.filter((s) => acts(s).length);
  const firstActs = acting.map((s) => ({ s, e: acts(s)[0] }));
  const taps = firstActs.filter((x) => x.e.taps !== null && x.e.taps !== undefined).map((x) => Number(x.e.taps));
  const writes = sittings.flatMap(acts);
  const located = writes.filter((e) => e.screen);
  const onFirst = located.filter((e) => e.screen === first).length;
  const where = new Map();
  for (const e of located) where.set(e.screen, (where.get(e.screen) || 0) + 1);
  const inserts = writes.filter((e) => e.kind === 'insert');
  const deletes = writes.filter((e) => e.kind === 'delete');
  const abandons = sittings.flatMap((s) => s.events.filter((e) => e.kind === 'abandon'));
  const byTable = (list) => { const m = new Map(); for (const e of list) if (e.tbl) m.set(e.tbl, (m.get(e.tbl) || 0) + 1); return [...m].sort((a, b) => b[1] - a[1])[0]?.[0] || null; };
  return {
    sittings: sittings.length,
    acted: share(acting.length, sittings.length),
    tapsToAct: median(taps),
    tapsMeasured: taps.length,
    secsToAct: median(firstActs.map((x) => (x.e.t - x.s.start) / 1000)),
    screensToAct: median(firstActs.map((x) => new Set(x.s.events.filter((e) => e.kind === 'open' && e.t <= x.e.t).map((e) => e.detail)).size)),
    tapsPerWrite: median(writes.filter((e) => e.taps !== null && e.taps !== undefined).map((e) => Number(e.taps))),
    writes: writes.length,
    onFirst: share(onFirst, located.length),
    located: located.length,
    mostOn: [...where].sort((a, b) => b[1] - a[1])[0]?.[0] || null,
    gaveUp: share(abandons.length, abandons.length + inserts.length),
    forms: abandons.length + inserts.length,
    gaveUpOn: byTable(abandons),
    wrong: share(inserts.filter((e) => e.wrong).length, inserts.length),
    inserts: inserts.length,
    wrongOn: byTable(inserts.filter((e) => e.wrong)),
    undone: share(deletes.filter((e) => e.undone).length, deletes.length),
    deletes: deletes.length,
  };
}

export function uxMeasures(events, { screens = [], brief = null } = {}) {
  const sittings = sittingsOf(events || []).filter((s) => s.events.some((e) => e.kind !== 'error'));
  mark(sittings);
  const order = [...screens].sort((a, b) => (a.order ?? 50) - (b.order ?? 50));
  const first = order[0]?.slug || null;
  const title = (slug) => order.find((x) => x.slug === slug)?.title || slug;
  const targets = targetsOf(brief);
  const m = measure(sittings, first);
  const job = brief?.job ? `the job, “${String(brief.job).replace(/^to /i, '')}”,` : 'the main job';
  const findings = [];
  const say = (key, value, target, text, severity = 'medium') => findings.push({ key, value, target, text, severity });
  const acting = Math.round((m.acted || 0) * m.sittings);
  if (m.tapsMeasured >= LEAST && m.tapsToAct > targets.taps) say('taps', m.tapsToAct, targets.taps, `It takes ${m.tapsToAct} taps from opening the app to doing anything (the median of ${m.tapsMeasured} visits); ${job} should take ${targets.taps} or fewer`, m.tapsToAct > 2 * targets.taps ? 'high' : 'medium');
  if (acting >= LEAST && m.secsToAct > targets.secs) say('secs', Math.round(m.secsToAct), targets.secs, `People take ${Math.round(m.secsToAct)} s from opening the app to doing anything (the median of ${acting} visits); ${job} should take under ${targets.secs} s`, m.secsToAct > 3 * targets.secs ? 'high' : 'medium');
  if (m.located >= LEAST && first && m.onFirst < targets.first && m.mostOn && m.mostOn !== first) say('first', m.onFirst, targets.first, `Only ${pct(m.onFirst)} of what people do is done on the first screen, ${title(first)}; most is done on ${title(m.mostOn)}: put what they do there on the first screen, or open on it`, m.onFirst < 0.25 ? 'high' : 'medium');
  if (m.forms >= LEAST && m.gaveUp > targets.gaveUp) say('gaveUp', m.gaveUp, targets.gaveUp, `${pct(m.gaveUp)} of the forms people start${m.gaveUpOn ? ` on ${m.gaveUpOn}` : ''} are given up: ask only for what is needed now, with defaults`);
  if (m.inserts >= LEAST && m.wrong > targets.wrong) say('wrong', m.wrong, targets.wrong, `${pct(m.wrong)} of the rows added${m.wrongOn ? ` to ${m.wrongOn}` : ''} are removed within ten minutes: adding one by mistake is easy, or what is added comes out wrong`);
  if (m.deletes >= 3 && m.undone > targets.undo) say('undo', m.undone, targets.undo, `Undo is used on ${pct(m.undone)} of the removes: a remove is easy to hit by mistake; keep it away from what people tap often`);
  // Each version's measures, so a change can be judged by what it did.
  const byV = new Map();
  for (const s of sittings) { const v = s.events.find((e) => e.v)?.v; if (v) { if (!byV.has(v)) byV.set(v, []); byV.get(v).push(s); } }
  const versions = [...byV].sort((a, b) => a[0] - b[0]).map(([v, ss]) => {
    const x = measure(ss, first);
    return { v: Number(v), sittings: x.sittings, tapsToAct: x.tapsToAct, secsToAct: x.secsToAct, onFirst: x.onFirst, gaveUp: x.gaveUp, acted: x.acted };
  });
  return { sittings: m.sittings, first, targets, measures: m, findings, versions };
}

// The findings as use signals (engine/evolve.mjs): they go to the suggestions
// from use, and a change kept for one is judged by its measure afterwards.
export function uxSignals(ux) {
  const last = new Date().toISOString();
  const weight = { high: 3, medium: 2, low: 1 };
  return (ux?.findings || []).map((f) => ({ key: `ux:${f.key}`, kind: 'ux', text: f.text.replace(/^./, (c) => c.toLowerCase()), count: ux.sittings, people: 0, last, strength: weight[f.severity] || 1 }));
}

// A measure before and after a moment (a change kept), for `ux:<key>`.
export function uxEffect(events, key, since, opts) {
  const t0 = Date.parse(String(since || '').replace(' ', 'T'));
  if (!Number.isFinite(t0)) return null;
  const field = { taps: 'tapsToAct', secs: 'secsToAct', first: 'onFirst', gaveUp: 'gaveUp', wrong: 'wrong', undo: 'undone' }[key];
  if (!field) return null;
  const before = uxMeasures(events.filter((e) => tsOf(e) < t0), opts).measures;
  const after = uxMeasures(events.filter((e) => tsOf(e) >= t0), opts).measures;
  return { ux: key, before: before[field], after: after[field], sittingsBefore: before.sittings, sittingsAfter: after.sittings };
}

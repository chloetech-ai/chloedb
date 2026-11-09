// Questions about how the pieces of every app fit together, answered from
// the graph in Strata (sa_graph, graph.mjs) with no model call:
//
//   which screens use the calendar        which rules write votes
//   which apps use lib.money              what calls checkout
//   what does the cart screen use         what runs when a vote is added
//   which screens read invoices           which screens use the leaderboard pattern
//
// A question is a kind of piece (screens, rules, functions, triggers, apps,
// components...), a relation (use, read, write, call, open, run on, depend
// on, fork) and a piece, named as people name it ("the calendar", "ui.calendar",
// "the invoices table", "cast special vote"). The piece is found by its
// name, else by words and meaning (the library's search). The relation is
// a walk of the graph's labelled edges (REACHABLE ... dir => 'in' for what
// points at the piece, 'out' for what it points at); writes are told from
// reads by the code.

import { capNode } from './graph.mjs';

const KINDS = [
  [/^(screens?|views?|pages?|tabs?)$/, { kind: 'view', name: 'screens', one: 'screen' }],
  [/^rules?$/, { kind: 'function', rule: true, name: 'rules', one: 'rule' }],
  [/^triggers?$/, { kind: 'function', trigger: true, name: 'triggers', one: 'trigger' }],
  [/^functions?$/, { kind: 'function', name: 'functions', one: 'function' }],
  [/^apps?$/, { kind: 'app', name: 'apps', one: 'app' }],
  [/^components?$/, { kind: 'component', name: 'components', one: 'component' }],
  [/^helpers?$/, { kind: 'helper', name: 'helpers', one: 'helper' }],
  [/^patterns?$/, { kind: 'pattern', name: 'patterns', one: 'pattern' }],
  [/^tables?$/, { kind: 'table', name: 'tables', one: 'table' }],
  [/^(pieces?|things?|parts?|capabilities)$/, { name: 'pieces', one: 'piece' }],
];
const kindOf = (w) => (w ? KINDS.find(([re]) => re.test(w))?.[1] || null : null);

// Relations, by the verbs that name them. `writes` keeps only the pieces
// whose code writes the table; `events` the triggers on those writes.
const VERBS = [
  [/^(depends? on|relies on|rely on|needs?)$/, { rel: 'depends', phrase: 'depend on' }],
  [/^(runs? on|triggers? on|fires? on|runs? when)$/, { rel: 'on', phrase: 'run on writes to' }],
  [/^((?:are |were )?(?:forked|copied) from)$/, { rel: 'forked', phrase: 'were forked from' }],
  [/^(links? to|opens?)$/, { rel: 'opens', phrase: 'open' }],
  [/^(writes?(?: to)?|changes?|updates?|adds? to|deletes? from|fills?)$/, { rel: 'writes', phrase: 'write' }],
  [/^(reads?|shows?|lists?|displays?|queries|query)$/, { rel: 'reads', phrase: 'read' }],
  [/^(calls?|runs?)$/, { rel: 'calls', phrase: 'call' }],
  [/^(uses?|used|using|includes?|contains?)$/, { rel: 'uses', phrase: 'use' }],
];
const VERB_RE = '(depends? on|relies on|rely on|needs?|runs? on|triggers? on|fires? on|runs? when|(?:are |were )?(?:forked|copied) from|links? to|opens?|writes?(?: to)?|changes?|updates?|adds? to|deletes? from|fills?|reads?|shows?|lists?|displays?|queries|query|calls?|runs?|uses?|used|using|includes?|contains?)';
const verbOf = (v) => VERBS.find(([re]) => re.test(v))?.[1] || null;

const EVENTS = { added: 'insert', created: 'insert', inserted: 'insert', made: 'insert', changed: 'update', updated: 'update', edited: 'update', deleted: 'delete', removed: 'delete' };

// The parts of a question, or null when it is not one this answers.
export function parseQuestion(q) {
  const s = String(q || '').toLowerCase().replace(/[?!.]+\s*$/, '').replace(/\s+/g, ' ').trim()
    .replace(/^(?:please |can you |tell me |show me |list |find )+/, '');
  if (!s) return null;
  let m = s.match(/^what (?:runs|happens|fires|is run|gets run) (?:when|after|on) (?:a |an |the |some )?(.+?) (?:is|are|gets|get) (added|created|inserted|made|changed|updated|edited|deleted|removed)$/);
  if (m) return { what: { kind: 'function', trigger: true, name: 'triggers', one: 'trigger' }, rel: 'on', phrase: `run when ${m[1]} is ${m[2]}`, target: m[1], hint: 'table', event: EVENTS[m[2]], dir: 'in' };
  m = s.match(new RegExp(`^(?:which|what|who|where)(?: (?:of the )?([a-z]+))?(?: (?:that|which|are|is|do|does))? ${VERB_RE} (.+)$`));
  if (m) {
    const what = kindOf(m[1]);
    if (m[1] && !what) return null;
    const v = verbOf(m[2]);
    if (!v) return null;
    return { what: what || { name: 'pieces', one: 'piece' }, ...v, target: m[3], dir: 'in' };
  }
  m = s.match(new RegExp(`^what(?: ([a-z]+))? (?:does|do|did) (.+?) ${VERB_RE}$`));
  if (m) {
    const what = kindOf(m[1]);
    if (m[1] && !what) return null;
    const v = verbOf(m[3]);
    if (!v) return null;
    return { what: what || { name: 'pieces', one: 'piece' }, ...v, target: m[2], dir: 'out' };
  }
  m = s.match(/^(?:where|in which ([a-z]+)) (?:is|are) (.+?) used$/);
  if (m) {
    const what = kindOf(m[1]) || { name: 'pieces', one: 'piece' };
    return { what, rel: 'uses', phrase: 'use', target: m[2], dir: 'in' };
  }
  return null;
}

const norm = (x) => String(x || '').toLowerCase().replace(/[^a-z0-9]/g, '');
const singular = (w) => (w.endsWith('ies') ? `${w.slice(0, -3)}y` : w.endsWith('ses') || w.endsWith('xes') ? w.slice(0, -2) : w.endsWith('s') && !w.endsWith('ss') ? w.slice(0, -1) : w);
const sameName = (a, b) => { const x = norm(a); const y = norm(b); return !!x && (x === y || singular(x) === singular(y)); };

const TYPE_WORDS = { component: 'component', helper: 'helper', pattern: 'pattern', table: 'table', screen: 'view', view: 'view', page: 'view', tab: 'view', rule: 'function', function: 'function', trigger: 'function', app: 'app' };

// The nodes a name stands for: "ui.calendar", "the calendar component",
// "the invoices table", "the cart screen of Corner Shop", "checkout".
export function resolveName(text, nodes, { hint = null, apps = new Map() } = {}) {
  let t = String(text || '').trim().replace(/^(?:the|a|an|our|my) /, '');
  let kind = hint;
  let app = null;
  // "... in/of/from <app>" when that is an app's name.
  const inApp = t.match(/^(.+?) (?:in|of|from) (?:the )?(.+?)(?: app)?$/);
  if (inApp) {
    const a = [...apps.entries()].find(([id, name]) => sameName(name, inApp[2]) || norm(name).startsWith(norm(inApp[2])) || id === inApp[2]);
    if (a) { app = a[0]; t = inApp[1].replace(/^(?:the|a|an) /, ''); }
  }
  let m = t.match(/^(ui|lib)\s*\.\s*([a-z][a-z0-9_-]*)$/i);
  if (m) { kind = m[1].toLowerCase() === 'ui' ? 'component' : 'helper'; t = m[2]; }
  m = t.match(/^(.+?) (component|helper|pattern|table|screen|view|page|tab|rule|function|trigger|app)s?$/);
  if (m && !kind) { kind = TYPE_WORDS[m[2]]; t = m[1]; }
  m = t.match(/^(component|helper|pattern|table|screen|view|page|rule|function|trigger|app) (.+)$/);
  if (m && !kind) { kind = TYPE_WORDS[m[1]]; t = m[2]; }
  t = t.replace(/^(?:the|a|an) /, '');
  const pool = nodes.filter((n) => (!kind || n.kind === kind) && (!app || n.app === app || n.app === 'lib'));
  const by = (f) => pool.filter((n) => (n.kind === 'app' ? f(n.title) : f(n.slug) || f(n.title)));
  let hits = by((x) => sameName(x, t));
  if (!hits.length && norm(t).length >= 4) hits = by((x) => norm(x).length >= 4 && (norm(x).includes(norm(t)) || norm(t).includes(norm(x))));
  return { hits, kind, app, name: t };
}

// Labels and depth of a relation's walk, by the kind of the piece asked about.
function walkOf(rel, targetKind, what, dir) {
  const toApps = what.kind === 'app';
  if (dir === 'out') {
    if (rel === 'depends') return { label: 'uses,data,calls,on', hops: 8 };
    if (rel === 'uses') return { label: 'uses,data,calls,on,opens', hops: 1 };
    if (rel === 'reads' || rel === 'writes') return { label: 'data', hops: 1 };
    return { label: { calls: 'calls', opens: 'opens', on: 'on', forked: 'forked' }[rel] || 'uses', hops: 1 };
  }
  const extra = toApps ? ',has' : '';
  if (rel === 'depends') return { label: `uses,data,calls,on${extra}`, hops: 8 };
  if (rel === 'on') return { label: 'on', hops: 1 };
  if (rel === 'forked') return { label: 'forked', hops: 1 };
  if (rel === 'reads' || rel === 'writes') return { label: `data,on${extra}`, hops: toApps ? 2 : 1 };
  if (rel === 'calls') return { label: `calls${extra}`, hops: toApps ? 2 : 1 };
  if (rel === 'opens') return { label: `opens${extra}`, hops: toApps ? 2 : 1 };
  // use: a table is used by what reads or writes it, a function by what
  // calls it, a screen by what opens it; a shared piece or pattern by what
  // calls it, directly or through other shared pieces.
  if (targetKind === 'table') return { label: `data,on${extra}`, hops: toApps ? 2 : 1 };
  if (targetKind === 'function') return { label: `calls${extra}`, hops: toApps ? 3 : 2 };
  if (targetKind === 'view') return { label: `opens${extra}`, hops: toApps ? 2 : 1 };
  return { label: `uses,calls${extra}`, hops: toApps ? 5 : 4 };
}

const esc = (s) => String(s).replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
// Whether a piece's code writes a table: SQL, app.insert/update/remove,
// a rule's steps, a screen's form.
export function writesTable(code, table) {
  const t = esc(table);
  return new RegExp(`\\b(?:insert\\s+into|update|delete\\s+from)\\s+${t}\\b|\\bapp\\s*\\.\\s*(?:insert|update|remove|upsert)\\s*\\(\\s*['"\`]${t}['"\`]|"(?:insert|update|upsert|remove|form)"\\s*:\\s*"${t}"`, 'i').test(String(code || ''));
}

const label = (n) => (n.kind === 'component' ? `ui.${n.slug}` : n.kind === 'helper' ? `lib.${n.slug}` : n.kind === 'pattern' ? `the ${n.slug} pattern`
  : n.kind === 'table' ? `the table ${n.slug}` : n.kind === 'app' ? n.title : `${n.kind === 'view' ? 'the screen' : 'the function'} ${n.slug}`);

// Answers a question: what it understood, the pieces it was about, and the
// answer, nearest first, each with its app and, for one found through
// another piece, that piece. Null when the text is not such a question.
export async function ask(store, question) {
  const p = parseQuestion(question);
  if (!p) return null;
  const nodes = await store.graph();
  const apps = new Map(nodes.filter((n) => n.kind === 'app').map((n) => [n.app, n.title]));
  const hint = p.hint || null;
  let { hits, name } = resolveName(p.target, nodes, { hint, apps });
  let foundBy = 'name';
  // No piece has that name: the library's search, by words and meaning.
  if (!hits.length) {
    const found = await store.searchCapabilities(name, { limit: 1, ...(hint ? { kinds: [hint] } : {}) }).catch(() => []);
    const node = found[0] && nodes.find((n) => n.id === capNode(found[0]));
    if (node) { hits = [node]; foundBy = found[0].found === 'meaning' ? 'meaning' : 'words'; }
  }
  if (!hits.length) return { question, understood: `${p.what.name} that ${p.phrase} "${name}"`, about: [], answer: [], note: `Nothing in the graph is called "${name}".` };
  // When a name means several kinds of piece, a use is of the shared ones
  // (ui.calendar and the calendar pattern rather than one app's screen named
  // calendar), and a read or write is of tables.
  if (p.dir === 'in' && hits.length > 1) {
    const prefer = p.rel === 'reads' || p.rel === 'writes' || p.rel === 'on' ? ['table'] : p.rel === 'calls' ? ['function'] : p.rel === 'opens' ? ['view'] : ['component', 'helper', 'pattern', 'table'];
    const some = hits.filter((n) => prefer.includes(n.kind));
    if (some.length) hits = some;
  }
  const kinds = [...new Set(hits.map((n) => n.kind))];
  const walk = walkOf(p.rel, kinds.length === 1 ? kinds[0] : 'component', p.what, p.dir);
  const seeds = hits.map((n) => n.id);
  let rows = await store.reach(seeds, { dir: p.dir, label: walk.label, hops: walk.hops });
  const seedSet = new Set(seeds);
  rows = rows.filter((r) => !seedSet.has(r.id));
  const byId = new Map(rows.map((r) => [r.id, r]));
  // A piece found through others: the pieces between it and the one asked
  // about (an app -> its screen menu -> ui.cart -> lib.money).
  const step = (r) => (r.links || []).map((l) => byId.get(l.to)).find((x) => x && Number(x.hops) === Number(r.hops) - 1);
  for (const r of rows) {
    if (p.dir !== 'in' || Number(r.hops) <= 1) continue;
    const via = [];
    for (let x = step(r); x && via.length < 8; x = Number(x.hops) > 1 ? step(x) : null) via.push(x);
    if (via.length) r.via = via;
  }
  if (p.what.kind) rows = rows.filter((r) => r.kind === p.what.kind);
  else rows = rows.filter((r) => r.kind !== 'app');
  // Rules, triggers, writes and events need the pieces' code and spec.
  const needCaps = p.what.rule || p.what.trigger || p.rel === 'writes' || p.rel === 'reads' || p.event;
  const caps = needCaps ? new Map((await store.getCapabilities(rows.map((r) => r.live).filter(Boolean))).map((c) => [c.id, c])) : new Map();
  const capOf = (r) => caps.get(r.live);
  const tables = hits.filter((n) => n.kind === 'table');
  if (p.what.rule) rows = rows.filter((r) => capOf(r)?.spec?.rule);
  if (p.what.trigger) rows = rows.filter((r) => capOf(r)?.spec?.on?.table);
  if (p.rel === 'writes' && p.dir === 'in' && tables.length) rows = rows.filter((r) => r.kind === 'app' || tables.some((t) => t.app === r.app && writesTable(capOf(r)?.code, t.slug)));
  // A read is a data edge (a trigger on the table that never reads it is not).
  if (p.rel === 'reads' && p.dir === 'in' && tables.length) rows = rows.filter((r) => r.kind === 'app' || (r.links || []).some((l) => l.label === 'data' && seedSet.has(l.to)));
  if (p.rel === 'writes' && p.dir === 'out') {
    const [own] = await store.getCapabilities(hits.map((n) => n.live).filter(Boolean));
    rows = rows.filter((r) => r.kind !== 'table' || writesTable(own?.code, r.slug));
  }
  if (p.event) rows = rows.filter((r) => [].concat(capOf(r)?.spec?.on?.event || 'insert').includes(p.event));
  const answer = rows.map((r) => ({ id: r.id, live: r.live || null, kind: r.kind, app: r.app, app_name: r.app === 'lib' ? 'shared' : apps.get(r.app) || r.app, slug: r.slug, title: r.title,
    hops: Number(r.hops) || 0, ...(r.via ? { via: r.via.map((x) => ({ id: x.id, label: label(x) })) } : {}), ...(capOf(r)?.spec?.rule ? { rule: true } : {}) }));
  const about = hits.map((n) => ({ id: n.id, kind: n.kind, app: n.app, app_name: n.app === 'lib' ? 'shared' : apps.get(n.app) || n.app, slug: n.slug, title: n.title, label: label(n) }));
  const tableApps = new Set(tables.map((t) => t.app));
  const targetText = tables.length > 1 && tables.length === hits.length ? `the table ${tables[0].slug} (in ${tableApps.size} apps)` : about.map((a) => a.label).join(' or ');
  const understood = p.dir === 'in'
    ? `${p.what.name[0].toUpperCase()}${p.what.name.slice(1)} that ${p.event ? `run when a row of ${targetText.replace(/^the table /, '')} is ${{ insert: 'added', update: 'changed', delete: 'deleted' }[p.event]}` : `${p.phrase} ${targetText}`}`
    : `${p.what.name[0].toUpperCase()}${p.what.name.slice(1)} ${targetText} ${{ uses: 'uses', reads: 'reads', writes: 'writes', calls: 'calls', opens: 'opens', depends: 'depends on', on: 'runs on', forked: 'was forked from' }[p.rel]}`;
  return { question, understood, about, found_by: foundBy, answer };
}

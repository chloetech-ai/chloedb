// The pieces of every app, and the shared library, as a graph in Strata.
//
// A row of sa_graph is a node: an app, one of its tables, views or
// functions, or a shared component or helper, by name (the version that is
// live now is in `live`). Its `links` field, of Strata's EDGES type, holds
// what it points at, each edge labelled:
//
//   has     app -> its tables, views and functions
//   uses    view, function, component or helper -> a shared piece it calls
//   data    view or function -> a table of its app it reads or writes
//   on      function -> the table whose writes run it
//   calls   view or function -> a function of its app it calls
//   opens   view -> a view of its app it opens
//   forked  capability -> the one it was copied from
//
// Edges point at names, not versions: apps pin versions of shared pieces in
// their compositions, and a node's edges are those of its live version. The
// engine asks the graph what depends on a piece (REACHABLE ... dir => 'in'),
// what a piece needs, and which pieces go together (personalized PageRank
// from the pieces an app uses and from the best text matches of a request).

export const GRAPH_SQL = `CREATE TABLE sa_graph (id KEYWORD PRIMARY KEY, kind KEYWORD NOT NULL, app KEYWORD, slug KEYWORD,
    title TEXT, description TEXT, live KEYWORD, version INT, links EDGES, updated TIMESTAMP DEFAULT CURRENT_TIMESTAMP)`;

export const LABELS = ['has', 'uses', 'data', 'on', 'calls', 'opens', 'forked'];

// Shared pieces: components are called as ui.name(props), helpers as
// lib.name.fn(...). Code blocks for them are named ui:NAME and lib:NAME.
export const SHARED = {
  component: { ns: 'ui', plural: 'components', define: /\bfunction\s+component\s*\(/, defines: 'function component(props)' },
  helper: { ns: 'lib', plural: 'helpers', define: /\bfunction\s+helper\s*\(/, defines: 'function helper()' },
};

// The shared pieces a piece of code calls, as kind/name.
export function sharedRefs(code) {
  const out = new Set();
  for (const [kind, { ns }] of Object.entries(SHARED)) {
    const re = new RegExp(`(?<![\\w$.])${ns}\\s*(?:\\.\\s*([a-z][a-zA-Z0-9]*)|\\[\\s*['"]([a-z][a-zA-Z0-9]*)['"]\\s*\\])`, 'g');
    for (const m of String(code || '').matchAll(re)) out.add(`${kind}/${m[1] || m[2]}`);
  }
  // A screen (a view written as JSON) names its components as "ui": "name".
  for (const m of String(code || '').matchAll(/"ui"\s*:\s*"([a-z][a-zA-Z0-9]*)"/g)) out.add(`component/${m[1]}`);
  return out;
}

export const appNode = (appId) => `app/${appId}`;
// A capability's node: `<app>/<kind>/<slug>`, shared pieces under `lib`.
export const capNode = (c) => `${c.app}/${c.kind}/${c.slug}`;
export const nameOf = (capId) => String(capId || '').replace(/@\d+$/, '');

const esc = (s) => String(s).replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

// What a capability points at, read from its code.
export function linksOf(cap, { tables = [], functions = [], views = [], lib = 'lib' } = {}) {
  const code = String(cap.code || '');
  const out = [];
  const add = (label, to) => { if (to !== capNode(cap) && !out.some(([l, t]) => l === label && t === to)) out.push([label, to]); };
  for (const key of sharedRefs(code)) add('uses', `${lib}/${key}`);
  if (cap.kind === 'view' || cap.kind === 'function') {
    for (const t of tables) {
      const n = esc(t);
      if (new RegExp(`(['"\`])\\s*${n}\\s*\\1|\\b(?:from|join|into|update)\\s+${n}\\b`, 'i').test(code)) add('data', `${cap.app}/table/${t}`);
    }
    for (const f of functions) if (new RegExp(`\\bapp\\s*\\.\\s*call\\s*\\(\\s*['"\`]${esc(f)}['"\`]`).test(code)) add('calls', `${cap.app}/function/${f}`);
    for (const v of views) if (new RegExp(`\\bapp\\s*\\.\\s*open\\s*\\(\\s*['"\`]${esc(v)}['"\`]`).test(code)) add('opens', `${cap.app}/view/${v}`);
  }
  const on = cap.spec?.on?.table;
  if (cap.kind === 'function' && on) add('on', `${cap.app}/table/${on}`);
  if (cap.forked_from) add('forked', nameOf(cap.forked_from));
  return out;
}

// A capability's node row.
export function capDoc(cap, names) {
  return { id: capNode(cap), kind: cap.kind, app: cap.app, slug: cap.slug, title: cap.title || cap.slug, description: cap.description || '',
    live: cap.id, version: Number(cap.version) || 0, links: linksOf(cap, names) };
}

// An app's node rows: the app and its own tables, views and functions.
export function appDocs(view, sharedKinds) {
  const appId = view.app.id;
  const own = view.caps.filter((c) => !sharedKinds.includes(c.kind));
  const names = { tables: view.tables.map((t) => t.spec.name || t.slug), functions: view.functions.map((f) => f.slug), views: view.views.map((v) => v.slug) };
  const docs = [{ id: appNode(appId), kind: 'app', app: appId, slug: appId, title: view.app.name || appId, description: view.app.description || '',
    live: '', version: Number(view.app.head) || 0, links: own.map((c) => ['has', capNode(c)]) }];
  for (const c of own) docs.push(capDoc(c, names));
  return docs;
}

export const parseLinks = (v) => {
  let list = v;
  if (typeof v === 'string') { try { list = JSON.parse(v); } catch { list = []; } }
  return (Array.isArray(list) ? list : []).map((e) => (Array.isArray(e) ? (e.length === 1 ? { to: e[0] } : { label: e[0], to: e[1] }) : typeof e === 'string' ? { to: e } : { label: e.label, to: e.to }));
};

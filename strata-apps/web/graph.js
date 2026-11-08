// The graph of apps and shared pieces, as Strata holds it in sa_graph: drawn
// in columns (apps, their views and functions, tables, components and
// patterns, helpers),
// with a panel that asks Strata what a node needs, what depends on it and
// what goes with it.

const COLS = ['Apps', 'Views and functions', 'Tables', 'Components and patterns', 'Helpers'];
const COL = { app: 0, view: 1, function: 1, table: 2, component: 3, pattern: 3, helper: 4 };
const EDGE = {
  uses: { c: 'var(--g-uses)', t: 'uses' }, data: { c: 'var(--g-data)', t: 'reads or writes' }, has: { c: 'var(--g-has)', t: 'has' },
  calls: { c: 'var(--g-calls)', t: 'calls' }, opens: { c: 'var(--g-calls)', t: 'opens' }, on: { c: 'var(--g-on)', t: 'runs on writes to' },
  forked: { c: 'var(--g-forked)', t: 'copied from', dash: '4 3' },
};
const NS = 'http://www.w3.org/2000/svg';
function s(tag, attrs, ...children) {
  const el = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs || {})) if (v !== undefined && v !== null) el.setAttribute(k, v);
  for (const c of children.flat()) if (c !== null && c !== undefined) el.append(c instanceof Node ? c : document.createTextNode(String(c)));
  return el;
}

// Columns, each ordered by where its neighbours in the columns before it
// sit, so that edges mostly run straight.
function layout(nodes) {
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const edges = [];
  for (const n of nodes) for (const l of n.links || []) if (byId.has(l.to)) edges.push({ from: n.id, to: l.to, label: l.label || '' });
  const nbrs = new Map(nodes.map((n) => [n.id, []]));
  for (const e of edges) { nbrs.get(e.from).push(e.to); nbrs.get(e.to).push(e.from); }
  const cols = COLS.map(() => []);
  for (const n of nodes) cols[COL[n.kind] ?? 1].push(n);
  const pos = new Map();
  const place = (c) => cols[c].forEach((n, i) => pos.set(n.id, { c, i }));
  cols[0].sort((a, b) => String(a.title).localeCompare(String(b.title)));
  place(0);
  for (let c = 1; c < cols.length; c++) {
    const key = new Map(cols[c].map((n) => {
      const before = nbrs.get(n.id).map((m) => pos.get(m)).filter((p) => p && p.c < c);
      return [n.id, before.length ? before.reduce((a, p) => a + p.c * 1000 + p.i, 0) / before.length : Infinity];
    }));
    cols[c].sort((a, b) => key.get(a.id) - key.get(b.id) || String(a.title).localeCompare(String(b.title)));
    place(c);
  }
  return { byId, edges, cols, pos };
}

// Everything reachable from a node along edges (out: what it needs) or
// against them (in: what depends on it), with the labels the server's walks
// follow: a view that opens another does not depend on its pieces.
const WALK = { out: new Set(['has', 'uses', 'data', 'calls', 'on']), in: new Set(['uses', 'has', 'calls']) };
function reach(id, edges, dir) {
  const seen = new Set([id]);
  const queue = [id];
  while (queue.length) {
    const x = queue.shift();
    for (const e of edges) {
      if (!WALK[dir].has(e.label)) continue;
      const [a, b] = dir === 'out' ? [e.from, e.to] : [e.to, e.from];
      if (a === x && !seen.has(b)) { seen.add(b); queue.push(b); }
    }
  }
  return seen;
}

export function drawGraph(nodes, { onSelect, selected, compact = false } = {}) {
  const W = compact ? 168 : 184; const GAP = compact ? 40 : 46; const RH = compact ? 30 : 34; const BH = compact ? 24 : 26; const TOP = 34;
  const { byId, edges, cols, pos } = layout(nodes);
  const used = cols.map((c, i) => (c.length ? i : -1)).filter((i) => i >= 0);
  const x = new Map(used.map((c, k) => [c, 8 + k * (W + GAP)]));
  const width = 16 + used.length * (W + GAP) - GAP;
  const height = TOP + Math.max(1, ...cols.map((c) => c.length)) * RH + 10;
  const at = (id) => { const p = pos.get(id); return { x: x.get(p.c), y: TOP + p.i * RH }; };
  const root = s('svg', { class: 'graph', width, height, viewBox: `0 0 ${width} ${height}`, role: 'img', 'aria-label': 'Graph of apps and shared pieces' });
  root.append(s('defs', null, ...Object.entries(EDGE).map(([k, v]) => s('marker', { id: `arrow-${k}`, viewBox: '0 0 8 8', refX: 7, refY: 4, markerWidth: 6, markerHeight: 6, orient: 'auto-start-reverse' },
    s('path', { d: 'M0,0 L8,4 L0,8 z', fill: v.c })))));
  for (const c of used) root.append(s('text', { x: x.get(c), y: 16, class: 'ghead' }, `${COLS[c]} (${cols[c].length})`));
  const edgeEls = [];
  const lines = s('g', { class: 'gedges' });
  for (const e of edges) {
    const a = at(e.from); const b = at(e.to);
    const y1 = a.y + BH / 2; const y2 = b.y + BH / 2;
    let d;
    if (b.x > a.x) { const x1 = a.x + W; const x2 = b.x - 2; const m = (x2 - x1) / 2; d = `M${x1},${y1} C${x1 + m},${y1} ${x2 - m},${y2} ${x2},${y2}`; } else { const x1 = a.x + W; const x2 = b.x + W + 2; const bulge = 18 + Math.min(40, Math.abs(y2 - y1) / 4); d = `M${x1},${y1} C${x1 + bulge},${y1} ${x2 + bulge},${y2} ${x2},${y2}`; }
    const st = EDGE[e.label] || EDGE.has;
    const el = s('path', { d, fill: 'none', stroke: st.c, 'stroke-width': 1.3, 'stroke-dasharray': st.dash, 'marker-end': `url(#arrow-${EDGE[e.label] ? e.label : 'has'})`, class: 'gedge' },
      s('title', null, `${byId.get(e.from).title} ${st.t} ${byId.get(e.to).title}`));
    edgeEls.push({ e, el });
    lines.append(el);
  }
  root.append(lines);
  const nodeEls = new Map();
  const boxes = s('g', null);
  for (const n of nodes) {
    const p = at(n.id);
    const label = String(n.kind === 'component' || n.kind === 'helper' ? `${n.kind === 'helper' ? 'lib' : 'ui'}.${n.slug}` : n.kind === 'pattern' ? `pattern ${n.slug}` : n.title || n.slug);
    const max = Math.floor((W - 22) / 6.6);
    const g = s('g', { class: `gnode k-${n.kind}`, transform: `translate(${p.x},${p.y})`, tabindex: 0 },
      s('rect', { width: W, height: BH, rx: 7 }), s('circle', { cx: 11, cy: BH / 2, r: 4 }),
      s('text', { x: 21, y: BH / 2 + 4 }, label.length > max ? `${label.slice(0, max - 1)}…` : label),
      s('title', null, `${n.kind}: ${n.title}${n.description ? ` — ${n.description}` : ''}`));
    g.addEventListener('click', () => select(n.id));
    g.addEventListener('keydown', (ev) => { if (ev.key === 'Enter') select(n.id); });
    nodeEls.set(n.id, g);
    boxes.append(g);
  }
  root.append(boxes);
  function select(id) {
    const out = id ? reach(id, edges, 'out') : null;
    const inn = id ? reach(id, edges, 'in') : null;
    for (const [nid, g] of nodeEls) {
      g.classList.toggle('sel', nid === id);
      g.classList.toggle('dim', !!id && !out.has(nid) && !inn.has(nid));
    }
    for (const { e, el } of edgeEls) {
      const on = id && ((out.has(e.from) && out.has(e.to)) || (inn.has(e.from) && inn.has(e.to)));
      el.classList.toggle('dim', !!id && !on);
      el.classList.toggle('hot', !!on);
    }
    if (onSelect) onSelect(id);
  }
  if (selected && byId.has(selected)) setTimeout(() => select(selected), 0);
  const legend = Object.entries(EDGE).filter(([k]) => edges.some((e) => e.label === k));
  return { svg: root, select, legend: legend.map(([k, v]) => ({ label: k, text: v.t, color: v.c, dash: v.dash })) };
}

// What Strata answers about one node, by graph queries over sa_graph.
export async function nodePanel(id, { h, api, showCode, onPick }) {
  let n;
  try { n = await api('GET', `/api/graph/node?id=${encodeURIComponent(id)}`); } catch (e) { return h('div', { class: 'err' }, e.message); }
  const name = (r) => (r.kind === 'component' || r.kind === 'helper' ? `${r.kind === 'helper' ? 'lib' : 'ui'}.${r.slug}` : r.kind === 'pattern' ? `pattern ${r.slug}` : r.title || r.slug);
  const item = (r, extra) => h('button', { class: 'gitem', onclick: () => onPick && onPick(r.id) }, h('span', { class: `pill ${r.kind}` }, r.kind), h('span', { class: 't' }, name(r)), extra ? h('span', { class: 'v' }, extra) : null);
  const list = (title, rows, sql, extra) => h('div', { class: 'gsec' }, h('div', { class: 'gsec-h' }, h('b', title), h('span', { class: 'mono gsql' }, sql)),
    rows.length ? h('div', { class: 'glist' }, rows.map((r) => item(r, extra && extra(r)))) : h('div', { class: 'hint' }, 'None'));
  const apps = n.dependents.filter((r) => r.kind === 'app');
  const deps = n.dependents.filter((r) => r.kind !== 'app');
  const key = `'${n.id}'`;
  return h('div', { class: 'gpanel' },
    h('div', { class: 'gpanel-h' }, h('span', { class: `pill ${n.kind}` }, n.kind), h('b', name(n)), n.version ? h('span', { class: 'muted' }, `v${n.version}`) : null,
      n.live ? h('button', { class: 'btn small', style: 'margin-left:auto', onclick: () => showCode(n.live) }, 'Code') : null),
    n.description ? h('div', { class: 'muted small' }, n.description) : null,
    n.kind !== 'app' ? list('Apps that depend on it', apps, `REACHABLE(links, ${key}, dir => 'in', label => 'uses,has,calls') AND kind = 'app'`, (r) => (Number(r.hops) > 1 ? `${r.hops} hops` : null)) : null,
    list(n.kind === 'app' ? 'What it is made of' : 'What depends on it', n.kind === 'app' ? n.needs : deps, n.kind === 'app' ? `REACHABLE(links, ${key}, hops => 8)` : `REACHABLE(links, ${key}, dir => 'in', label => 'uses,has,calls')`, (r) => `${r.hops} hop${Number(r.hops) === 1 ? '' : 's'}`),
    n.kind !== 'app' ? list('What it needs', n.needs, `REACHABLE(links, ${key}, hops => 8)`, (r) => `${r.hops} hop${Number(r.hops) === 1 ? '' : 's'}`) : null,
    list('Goes with', n.related, `ORDER BY ppr(links, ${key}) DESC`, (r) => r.score.toFixed(3)));
}

export const GRAPH_HELP = 'Each app, view, function, table and shared piece is a row of sa_graph in Strata, and what it points at is its links field (Strata’s EDGES type). Click a node: Strata walks the graph from it (REACHABLE, both ways) and ranks what goes with it by personalized PageRank. The forge ranks the library the same way, fused with BM25 over the request, before it writes anything new.';

// Shared front-end helpers for the prototypes (ES module, no build step).
//
//   import { topbar, api, countUp, fmt, sparkline, highlightSQL, sqlDrawer } from '/shared/ds.js';
//   topbar({ app: 'Leak Lab' });           // the shared top bar, with the live-model indicator
//   const r = await api('/leaklab/api/matrix');   // JSON; any `queries` in the answer go to the SQL drawer
//
// Convention: an app's API answers carry `queries: [{sql, params, took_ms, rows, plan}]`
// for the ChloeDB statements behind the view, so "the SQL behind this" is always one
// click away, with the server's own timing.

const esc = s => String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
export { esc };

export const fmt = {
  n(v, d = 0) { return v == null || Number.isNaN(v) ? '–' : Number(v).toLocaleString('en-US', { maximumFractionDigits: d, minimumFractionDigits: d }); },
  compact(v) {
    if (v == null) return '–';
    const a = Math.abs(v);
    if (a >= 1e9) return (v / 1e9).toFixed(a >= 1e10 ? 0 : 1).replace(/\.0$/, '') + 'B';
    if (a >= 1e6) return (v / 1e6).toFixed(a >= 1e7 ? 0 : 1).replace(/\.0$/, '') + 'M';
    if (a >= 1e3) return (v / 1e3).toFixed(a >= 1e4 ? 0 : 1).replace(/\.0$/, '') + 'k';
    return String(Math.round(v));
  },
  pct(v, d = 0) { return v == null ? '–' : (v * 100).toFixed(d) + '%'; },
  ms(v) { return v == null ? '–' : v < 1 ? v.toFixed(2) + ' ms' : v < 100 ? v.toFixed(1) + ' ms' : Math.round(v) + ' ms'; },
  usd(v) { return v == null ? '–' : '$' + (v < 1 ? v.toFixed(3) : v.toFixed(2)); },
};

// ---- the SQL drawer ------------------------------------------------------------------
let drawer = null, drawerList = null, drawerCount = null, total = 0;
export function sqlDrawer() {
  if (drawer) return drawer;
  drawer = document.createElement('aside');
  drawer.className = 'sql-drawer';
  drawer.innerHTML = `<header><span style="color:var(--indigo)">&#9670;</span> The SQL behind this view <span class="count">0 queries</span></header><div class="list"></div>`;
  document.body.appendChild(drawer);
  drawerList = drawer.querySelector('.list');
  drawerCount = drawer.querySelector('.count');
  drawer.querySelector('header').addEventListener('click', () => drawer.classList.toggle('open'));
  return drawer;
}

export function logQueries(queries = [], label = '') {
  if (!queries.length) return;
  sqlDrawer();
  for (const q of queries) {
    total += 1;
    const el = document.createElement('div');
    el.className = 'q fade-up';
    const params = q.params && Object.keys(q.params).length ? `<span>params ${esc(JSON.stringify(q.params)).slice(0, 120)}</span>` : '';
    el.innerHTML = `<div class="meta"><span style="color:var(--green)">${q.took_ms != null ? fmt.ms(q.took_ms) : ''}</span>${q.rows != null ? `<span>${fmt.n(q.rows)} rows</span>` : ''}${label ? `<span>${esc(label)}</span>` : ''}${params}</div><pre class="code">${highlightSQL(q.sql || '')}</pre>${q.plan && q.plan.length ? `<div class="plan">${q.plan.slice(0, 6).map(esc).join('<br>')}</div>` : ''}`;
    drawerList.prepend(el);
  }
  while (drawerList.children.length > 40) drawerList.lastChild.remove();
  drawerCount.textContent = `${total} ${total === 1 ? 'query' : 'queries'}`;
}

// ---- fetch -------------------------------------------------------------------------------
export async function api(path, { method, body, label, signal } = {}) {
  const res = await fetch(path, {
    method: method || (body ? 'POST' : 'GET'), signal,
    headers: body ? { 'content-type': 'application/json' } : {},
    body: body ? JSON.stringify(body) : undefined,
  });
  let data = null;
  const t = await res.text();
  try { data = t ? JSON.parse(t) : null; } catch { data = { error: t.slice(0, 300) }; }
  if (data && Array.isArray(data.queries)) logQueries(data.queries, label || path.split('/api/')[1]);
  if (!res.ok) throw Object.assign(new Error(data?.error || `HTTP ${res.status}`), { status: res.status, data });
  return data;
}

// Server-sent events: handlers = { eventName: data => ..., error: e => ... }. Returns close().
export function stream(path, handlers = {}) {
  const es = new EventSource(path);
  for (const [k, fn] of Object.entries(handlers)) {
    if (k === 'error') continue;
    es.addEventListener(k, ev => { let d = null; try { d = JSON.parse(ev.data); } catch { d = ev.data; } if (d && d.queries) logQueries(d.queries, k); fn(d); });
  }
  es.onerror = e => { if (handlers.error) handlers.error(e); es.close(); };
  return () => es.close();
}

// ---- the top bar --------------------------------------------------------------------------
export function topbar({ app = '', links = [] } = {}) {
  const el = document.createElement('header');
  el.className = 'topbar';
  el.innerHTML = `<a class="brand" href="/"><span class="mark"></span>ChloeDB <span class="sep">×</span> Nodwyn</a>${app ? `<span class="sep">/</span><span class="app">${esc(app)}</span>` : ''}<nav>${links.map(l => `<a href="${esc(l.href)}">${esc(l.label)}</a>`).join('')}<a href="/">All prototypes</a><span class="live-dot off" title="live model calls"><i></i><span>model</span></span></nav>`;
  document.body.prepend(el);
  fetch('/api/hub').then(r => r.json()).then(h => {
    const dot = el.querySelector('.live-dot');
    const s = h.spend || {};
    dot.classList.toggle('off', !s.live);
    dot.querySelector('span').textContent = s.live ? 'live model on' : 'recorded results';
    dot.title = s.live ? `live model calls on; $${(s.daily_cap_usd - s.usd).toFixed(2)} of today's $${s.daily_cap_usd} budget left` : 'live model calls are off or over budget: the page shows recorded results';
  }).catch(() => {});
  return el;
}

// ---- numbers that count up when they come into view ----------------------------------------
export function countUp(el, to, { duration = 1100, format = v => fmt.n(v), from = 0 } = {}) {
  const start = performance.now();
  const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
  if (reduce) { el.textContent = format(to); return; }
  const step = now => {
    const t = Math.min(1, (now - start) / duration);
    const e = 1 - Math.pow(1 - t, 3);
    el.textContent = format(from + (to - from) * e);
    if (t < 1) requestAnimationFrame(step);
  };
  requestAnimationFrame(step);
}

export function onVisible(el, fn, { once = true, threshold = 0.3 } = {}) {
  const io = new IntersectionObserver(entries => {
    for (const e of entries) if (e.isIntersecting) { fn(e.target); if (once) io.unobserve(e.target); }
  }, { threshold });
  io.observe(el);
  return () => io.disconnect();
}

// ---- small charts (SVG strings) ------------------------------------------------------------
export function sparkline(values, { w = 120, h = 32, color = 'var(--indigo)', fill = true } = {}) {
  if (!values.length) return '';
  const max = Math.max(...values), min = Math.min(...values, 0);
  const x = i => (i / Math.max(1, values.length - 1)) * w;
  const y = v => h - 2 - ((v - min) / Math.max(1e-9, max - min)) * (h - 4);
  const d = values.map((v, i) => `${i ? 'L' : 'M'}${x(i).toFixed(1)} ${y(v).toFixed(1)}`).join('');
  return `<svg viewBox="0 0 ${w} ${h}" width="${w}" height="${h}" aria-hidden="true">${fill ? `<path d="${d}L${w} ${h}L0 ${h}Z" fill="${color}" opacity=".12"/>` : ''}<path d="${d}" fill="none" stroke="${color}" stroke-width="1.6" stroke-linejoin="round"/></svg>`;
}

// Horizontal bars: items = [{label, value, color?, note?}]
export function hbars(items, { max, fmtv = v => fmt.n(v), height = 22 } = {}) {
  const m = max ?? Math.max(...items.map(i => i.value), 1);
  return `<div style="display:flex;flex-direction:column;gap:8px">${items.map(i => `<div style="display:grid;grid-template-columns:minmax(120px,32%) 1fr auto;gap:10px;align-items:center;font-size:13px"><span class="muted" style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap" title="${esc(i.label)}">${esc(i.label)}</span><span style="background:var(--panel-2);border-radius:6px;height:${height}px;overflow:hidden"><span style="display:block;height:100%;width:${(100 * i.value / m).toFixed(2)}%;background:${i.color || 'var(--indigo)'};border-radius:6px;transition:width .8s ease"></span></span><span class="num mono" style="min-width:56px;text-align:right">${fmtv(i.value)}</span></div>`).join('')}</div>`;
}

// ---- SQL highlighting ------------------------------------------------------------------------
const KW = /\b(SELECT|FROM|WHERE|AND|OR|NOT|IN|IS|NULL|AS|ON|JOIN|LEFT|INNER|GROUP|BY|ORDER|LIMIT|OFFSET|DESC|ASC|HAVING|WITH|UNION|ALL|DISTINCT|CASE|WHEN|THEN|ELSE|END|INSERT|INTO|VALUES|UPDATE|SET|DELETE|CREATE|TABLE|PRIMARY|KEY|OPTIONS|DEFAULT|EXPLAIN|SHOW|TABLES|BETWEEN|LIKE|ILIKE|EXISTS|OVER|PARTITION|EDGES|REACHABLE|PACK|TOKENS|RECOMMENDER|TEXT|KEYWORD|INT|FLOAT|VECTOR|TIMESTAMP|BOOL)\b/gi;
const FN = /\b(MATCH|bm25|rrf|similarity|embed|llm|ppr|score|COUNT|SUM|AVG|MIN|MAX|coalesce|lower|upper|length|substr|date_trunc|now|highlight|snippet|features|uuid|abs|round)\s*(?=\()/gi;
export function highlightSQL(src) {
  const out = [];
  const re = /(--[^\n]*)|('(?:[^']|'')*')|(:[a-zA-Z_][\w]*)|(\b\d+(?:\.\d+)?\b)|([A-Za-z_][\w]*)|(\s+)|(.)/g;
  let m;
  while ((m = re.exec(src))) {
    const [t, c, s, p, n, w] = m;
    if (c) out.push(`<span class="sql-c">${esc(c)}</span>`);
    else if (s) out.push(`<span class="sql-s">${esc(s)}</span>`);
    else if (p) out.push(`<span class="sql-p">${esc(p)}</span>`);
    else if (n) out.push(`<span class="sql-n">${esc(n)}</span>`);
    else if (w) {
      KW.lastIndex = 0; FN.lastIndex = 0;
      const after = src.slice(re.lastIndex).match(/^\s*\(/);
      if (after && /^(MATCH|bm25|rrf|similarity|embed|llm|ppr|score|COUNT|SUM|AVG|MIN|MAX|coalesce|lower|upper|length|substr|date_trunc|now|highlight|snippet|features|uuid|abs|round|REACHABLE)$/i.test(w)) out.push(`<span class="sql-f">${esc(w)}</span>`);
      else if (new RegExp(`^(${KW.source.slice(3, -3)})$`, 'i').test(w)) out.push(`<span class="sql-k">${esc(w.toUpperCase())}</span>`);
      else out.push(esc(w));
    } else out.push(esc(t));
  }
  return out.join('');
}

export function toast(msg, ms = 2600) {
  const t = document.createElement('div');
  t.className = 'toast';
  t.textContent = msg;
  document.body.appendChild(t);
  setTimeout(() => t.remove(), ms);
}

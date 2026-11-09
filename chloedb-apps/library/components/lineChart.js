// title: Line chart
// description: Lines over time or any number (weight per day, balance per month, minutes per week), one or several series, with dots, an optional filled area, and the values of the nearest point on hover.
// props: series: [{name, color?, points: [{x, y}]}] or data: [{x, y}] for one line; x is a 'YYYY-MM-DD' day, an ISO time or a number;
//   format(y): how values read (default a plain number); formatX(x): how x reads (default the day as "7 Nov");
//   area: true to fill under a single line; height: pixels (default 220); yFrom0: start the axis at 0 (default true when every y >= 0);
//   onSelect(point, series) when a point is tapped
function component(props) {
  css('sa-lines', `
.sa-lines{width:100%;position:relative;font-size:12px;color:var(--muted)}
.sa-lines svg{display:block;max-width:100%;overflow:visible}
.sa-lines-tip{position:absolute;pointer-events:none;background:var(--fg);color:var(--bg);border-radius:6px;padding:4px 8px;font-size:12px;white-space:nowrap;transform:translate(-50%,-110%);display:none}
.sa-lines-legend{display:flex;flex-wrap:wrap;gap:12px;margin-top:6px}
.sa-lines-legend span{display:inline-flex;align-items:center;gap:5px}
.sa-lines-legend i{width:12px;height:3px;border-radius:2px;display:inline-block}
`);
  const PALETTE = ['var(--accent)', '#e8590c', '#2f9e44', '#ae3ec9', '#1098ad', '#f08c00'];
  const raw = props.series || [{ name: '', points: props.data || [] }];
  const isTime = (v) => typeof v === 'string' && /^\d{4}-\d{2}-\d{2}/.test(v);
  const toX = (v) => (isTime(v) ? Date.parse(v.length === 10 ? `${v}T00:00:00Z` : v) : Number(v));
  const series = raw.map((s, i) => ({ name: s.name || '', color: s.color || PALETTE[i % PALETTE.length],
    points: (s.points || []).filter((p) => p && p.y !== null && p.y !== undefined && Number.isFinite(Number(p.y))).map((p) => ({ ...p, _x: toX(p.x), _y: Number(p.y) })).filter((p) => Number.isFinite(p._x)).sort((a, b) => a._x - b._x) }));
  const all = series.flatMap((s) => s.points);
  const root = h('div', { class: 'sa-lines' });
  if (!all.length) { root.append(h('div', { class: 'empty' }, 'No data yet')); return root; }
  const timeAxis = all.some((p) => isTime(p.x));
  const fmt = props.format || ((v) => Number(v).toLocaleString(undefined, { maximumFractionDigits: 2 }));
  const fmtX = props.formatX || ((x) => (isTime(x) ? new Date(toX(x)).toLocaleDateString(undefined, { day: 'numeric', month: 'short', timeZone: 'UTC' }) : String(x)));
  const H = Number(props.height) || 220;
  const L = 46; const R = 10; const T = 10; const B = 24;
  let lo = Math.min(...all.map((p) => p._y)); let hi = Math.max(...all.map((p) => p._y));
  if (props.yFrom0 !== false && lo >= 0) lo = 0;
  if (hi === lo) { hi += 1; lo -= lo === 0 ? 0 : 1; }
  const span = hi - lo; const step = 10 ** Math.floor(Math.log10(span / 4)); const tick = [1, 2, 5, 10].map((f) => f * step).find((t) => span / t <= 5) || step * 10;
  lo = Math.floor(lo / tick) * tick; hi = Math.ceil(hi / tick) * tick;
  const x0 = Math.min(...all.map((p) => p._x)); const x1 = Math.max(...all.map((p) => p._x));
  const tip = h('div', { class: 'sa-lines-tip' });
  // Drawn at the width it gets, so that text is never stretched; drawn again when that changes.
  const plot = (W) => {
  const sx = (x) => (x1 === x0 ? (L + W - R) / 2 : L + ((x - x0) * (W - L - R)) / (x1 - x0));
  const sy = (y) => T + ((hi - y) * (H - T - B)) / (hi - lo);
  const g = svg('svg', { viewBox: `0 0 ${W} ${H}`, width: W, height: H, role: 'img', 'aria-label': props.label || 'line chart' });
  for (let v = lo; v <= hi + tick / 2; v += tick) {
    g.append(svg('line', { x1: L, x2: W - R, y1: sy(v), y2: sy(v), stroke: 'var(--line)' }));
    g.append(svg('text', { x: L - 6, y: sy(v) + 4, 'text-anchor': 'end', fill: 'var(--muted)', 'font-size': 11 }, fmt(Math.round(v / tick) * tick)));
  }
  const xs = [...new Set(all.map((p) => p._x))].sort((a, b) => a - b);
  const every = Math.max(1, Math.ceil(xs.length / Math.max(2, Math.floor(W / 90))));
  xs.forEach((x, i) => { if (i % every === 0 || i === xs.length - 1 && xs.length < 8) { const p = all.find((q) => q._x === x); g.append(svg('text', { x: sx(x), y: H - 6, 'text-anchor': 'middle', fill: 'var(--muted)', 'font-size': 11 }, fmtX(p.x))); } });
  for (const s of series) {
    if (!s.points.length) continue;
    const d = s.points.map((p, i) => `${i ? 'L' : 'M'}${sx(p._x).toFixed(1)},${sy(p._y).toFixed(1)}`).join(' ');
    if (props.area && series.length === 1) g.append(svg('path', { d: `${d} L${sx(s.points[s.points.length - 1]._x).toFixed(1)},${sy(lo)} L${sx(s.points[0]._x).toFixed(1)},${sy(lo)} Z`, fill: s.color, opacity: 0.12 }));
    g.append(svg('path', { d, fill: 'none', stroke: s.color, 'stroke-width': 2, 'stroke-linejoin': 'round' }));
    if (s.points.length <= 60) for (const p of s.points) g.append(svg('circle', { cx: sx(p._x), cy: sy(p._y), r: 3, fill: s.color, style: props.onSelect ? 'cursor:pointer' : null, onclick: props.onSelect ? () => props.onSelect(p, s) : null }, svg('title', null, `${s.name ? `${s.name} · ` : ''}${fmtX(p.x)}: ${fmt(p._y)}`)));
  }
  const guide = svg('line', { y1: T, y2: H - B, stroke: 'var(--muted)', 'stroke-dasharray': '3 3', visibility: 'hidden' });
  g.append(guide);
  g.addEventListener('mousemove', (e) => {
    const r = g.getBoundingClientRect();
    const mx = ((e.clientX - r.left) / r.width) * W;
    let best = xs[0];
    for (const x of xs) if (Math.abs(sx(x) - mx) < Math.abs(sx(best) - mx)) best = x;
    const here = series.map((s) => [s, s.points.find((p) => p._x === best)]).filter(([, p]) => p);
    guide.setAttribute('x1', sx(best)); guide.setAttribute('x2', sx(best)); guide.setAttribute('visibility', 'visible');
    tip.textContent = `${fmtX(here[0][1].x)}: ${here.map(([s, p]) => `${s.name ? `${s.name} ` : ''}${fmt(p._y)}`).join(' · ')}`;
    tip.style.display = 'block';
    tip.style.left = `${(sx(best) / W) * r.width}px`;
    tip.style.top = `${(sy(Math.max(...here.map(([, p]) => p._y))) / H) * r.height}px`;
  });
  g.addEventListener('mouseleave', () => { tip.style.display = 'none'; guide.setAttribute('visibility', 'hidden'); });
  return g;
  };
  const holder = h('div');
  let drawn = 0;
  const draw = (w) => { drawn = w; holder.replaceChildren(plot(w)); };
  draw(560);
  if (typeof ResizeObserver === 'function') new ResizeObserver(() => { const w = Math.floor(holder.clientWidth); if (w > 120 && Math.abs(w - drawn) > 3) draw(w); }).observe(holder);
  root.append(holder, tip, series.length > 1 ? h('div', { class: 'sa-lines-legend' }, series.map((s) => h('span', null, h('i', { style: `background:${s.color}` }), s.name))) : null);
  return root;
}

// title: Bar chart
// description: Bars for comparing amounts across categories or periods (spending by category, sessions per week), vertical or horizontal, one series or several side by side or stacked, with values on hover.
// props: data: [{label, value} or {label, values: [numbers, one per series]}]; series: [{name, color?}] when values has several;
//   stacked: true to stack the series; horizontal: true for bars across (good for long labels);
//   format(value): how values read (default a plain number); color: css color for one series (default the accent);
//   height: pixels of the plot (default 220); onSelect(item, seriesIndex) when a bar is tapped
function component(props) {
  css('sa-bars', `
.sa-bars{width:100%;font-size:12px;color:var(--muted)}
.sa-bars svg{display:block;max-width:100%;overflow:visible}
.sa-bars .bar{cursor:default;transition:opacity .1s}
.sa-bars.clickable .bar{cursor:pointer}
.sa-bars .bar:hover{opacity:.8}
.sa-bars-legend{display:flex;flex-wrap:wrap;gap:12px;margin-top:6px}
.sa-bars-legend span{display:inline-flex;align-items:center;gap:5px}
.sa-bars-legend i{width:10px;height:10px;border-radius:3px;display:inline-block}
.sa-bars-h{display:flex;flex-direction:column;gap:6px}
.sa-bars-h .row{display:grid;grid-template-columns:minmax(60px,30%) 1fr auto;gap:8px;align-items:center}
.sa-bars-h .lab{color:var(--fg);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.sa-bars-h .track{display:flex;height:16px;border-radius:4px;overflow:hidden;background:var(--soft)}
.sa-bars-h .val{font-variant-numeric:tabular-nums;color:var(--fg)}
`);
  const PALETTE = ['var(--accent)', '#e8590c', '#2f9e44', '#ae3ec9', '#1098ad', '#f08c00', '#d6336c', '#5c7cfa'];
  const data = (props.data || []).map((d) => ({ ...d, vals: Array.isArray(d.values) ? d.values.map((v) => Number(v) || 0) : [Number(d.value) || 0] }));
  const nSeries = Math.max(1, ...data.map((d) => d.vals.length));
  const series = Array.from({ length: nSeries }, (_, i) => ({ name: props.series?.[i]?.name || '', color: props.series?.[i]?.color || (nSeries === 1 ? props.color || PALETTE[0] : PALETTE[i % PALETTE.length]) }));
  const fmt = props.format || ((v) => Number(v).toLocaleString(undefined, { maximumFractionDigits: 2 }));
  const stacked = !!props.stacked && nSeries > 1;
  const total = (d) => d.vals.reduce((a, b) => a + b, 0);
  const max = Math.max(0, ...data.map((d) => (stacked ? total(d) : Math.max(...d.vals))));
  const root = h('div', { class: `sa-bars${props.onSelect ? ' clickable' : ''}` });
  const legend = nSeries > 1 ? h('div', { class: 'sa-bars-legend' }, series.map((s) => h('span', null, h('i', { style: `background:${s.color}` }), s.name))) : null;
  if (!data.length) { root.append(h('div', { class: 'empty' }, 'No data yet')); return root; }
  if (props.horizontal) {
    root.append(h('div', { class: 'sa-bars-h' }, data.map((d) => h('div', { class: 'row' },
      h('span', { class: 'lab', title: d.label }, String(d.label ?? '')),
      h('div', { class: 'track' }, d.vals.map((v, i) => h('div', { class: 'bar', title: `${d.label}${series[i].name ? ` · ${series[i].name}` : ''}: ${fmt(v)}`,
        style: `width:${max ? (100 * v) / max / (stacked ? 1 : nSeries) : 0}%;background:${series[i].color}${!stacked && i ? ';margin-left:2px' : ''}`, onclick: props.onSelect ? () => props.onSelect(d, i) : null }))),
      h('span', { class: 'val' }, fmt(stacked ? total(d) : d.vals[0]))))), legend);
    return root;
  }
  const H = Number(props.height) || 220;
  const L = 44; const B = 22; const T = 8;
  const nice = (m) => { if (m <= 0) return 1; const p = 10 ** Math.floor(Math.log10(m)); const f = m / p; return (f <= 1 ? 1 : f <= 2 ? 2 : f <= 5 ? 5 : 10) * p; };
  const top = nice(max);
  const y = (v) => T + (H - T - B) * (1 - v / top);
  // Drawn at the width it gets, so that text is never stretched; drawn again when that changes.
  const plot = (W) => {
  const g = svg('svg', { viewBox: `0 0 ${W} ${H}`, width: W, height: H, role: 'img', 'aria-label': props.label || 'bar chart' });
  for (let k = 0; k <= 4; k++) {
    const v = (top * k) / 4;
    g.append(svg('line', { x1: L, x2: W, y1: y(v), y2: y(v), stroke: 'var(--line)', 'stroke-width': 1 }));
    g.append(svg('text', { x: L - 6, y: y(v) + 4, 'text-anchor': 'end', fill: 'var(--muted)', 'font-size': 11 }, fmt(v)));
  }
  const slot = (W - L) / data.length;
  const every = Math.max(1, Math.ceil(data.length / Math.floor((W - L) / 46)));
  data.forEach((d, j) => {
    const x0 = L + j * slot + slot * 0.15;
    const bw = slot * 0.7;
    let acc = 0;
    d.vals.forEach((v, i) => {
      const w = stacked ? bw : bw / nSeries;
      const x = stacked ? x0 : x0 + i * w;
      const y1 = y(stacked ? acc + v : v);
      const y0 = y(stacked ? acc : 0);
      acc += v;
      g.append(svg('rect', { class: 'bar', x: x + 1, y: Math.min(y1, y0), width: Math.max(1, w - 2), height: Math.max(0, Math.abs(y0 - y1)), rx: 3, fill: series[i].color,
        onclick: props.onSelect ? () => props.onSelect(d, i) : null }, svg('title', null, `${d.label}${series[i].name ? ` · ${series[i].name}` : ''}: ${fmt(v)}`)));
    });
    if (j % every === 0) g.append(svg('text', { x: x0 + bw / 2, y: H - 6, 'text-anchor': 'middle', fill: 'var(--muted)', 'font-size': 11 }, String(d.label ?? '').slice(0, 12)));
  });
  return g;
  };
  const holder = h('div');
  let drawn = 0;
  const draw = (w) => { drawn = w; holder.replaceChildren(plot(w)); };
  draw(560);
  if (typeof ResizeObserver === 'function') new ResizeObserver(() => { const w = Math.floor(holder.clientWidth); if (w > 100 && Math.abs(w - drawn) > 3) draw(w); }).observe(holder);
  root.append(holder, legend);
  return root;
}

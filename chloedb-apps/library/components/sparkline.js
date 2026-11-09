// title: Sparkline
// description: A tiny line of how a number moved (last 30 days, last 12 weeks), small enough to sit inline next to a total or in a table cell; the last point is dotted.
// props: values: [numbers] oldest first (nulls are gaps); width (default 120); height (default 32);
//   color: css color (default the accent); area: true to fill under the line (default true); label: accessible description
function component(props) {
  const W = Number(props.width) || 120;
  const H = Number(props.height) || 32;
  const vals = (props.values || []).map((v) => (v === null || v === undefined || v === '' ? null : Number(v)));
  const nums = vals.filter((v) => v !== null && Number.isFinite(v));
  const color = props.color || 'var(--accent)';
  const root = svg('svg', { width: W, height: H, viewBox: `0 0 ${W} ${H}`, role: 'img', 'aria-label': props.label || `trend of ${nums.length} values`, style: 'display:inline-block;vertical-align:middle;overflow:visible' });
  if (nums.length < 2) { root.append(svg('line', { x1: 0, x2: W, y1: H / 2, y2: H / 2, stroke: 'var(--line)', 'stroke-width': 1.5 })); return root; }
  const lo = Math.min(...nums);
  const hi = Math.max(...nums);
  const pad = 3;
  const x = (i) => pad + (i * (W - 2 * pad)) / (vals.length - 1);
  const y = (v) => (hi === lo ? H / 2 : H - pad - ((v - lo) * (H - 2 * pad)) / (hi - lo));
  let d = '';
  let pen = false;
  vals.forEach((v, i) => { if (v === null || !Number.isFinite(v)) { pen = false; return; } d += `${pen ? 'L' : 'M'}${x(i).toFixed(1)},${y(v).toFixed(1)} `; pen = true; });
  if (props.area !== false) {
    const idx = vals.map((v, i) => (v === null || !Number.isFinite(v) ? -1 : i)).filter((i) => i >= 0);
    const fill = `M${x(idx[0]).toFixed(1)},${H} ${idx.map((i) => `L${x(i).toFixed(1)},${y(vals[i]).toFixed(1)}`).join(' ')} L${x(idx[idx.length - 1]).toFixed(1)},${H} Z`;
    root.append(svg('path', { d: fill, fill: color, opacity: 0.12 }));
  }
  root.append(svg('path', { d: d.trim(), fill: 'none', stroke: color, 'stroke-width': 1.6, 'stroke-linejoin': 'round', 'stroke-linecap': 'round' }));
  let last = vals.length - 1;
  while (last >= 0 && (vals[last] === null || !Number.isFinite(vals[last]))) last--;
  if (last >= 0) root.append(svg('circle', { cx: x(last), cy: y(vals[last]), r: 2.4, fill: color }));
  return root;
}

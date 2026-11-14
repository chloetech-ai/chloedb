// title: Donut chart
// description: Parts of a whole as a ring (spending by category, time by project), with the total or a label in the middle and a legend with each part's value and share.
// props: data: [{label, value, color?}] (parts at or below 0 are left out); format(value): how values read (default a plain number);
//   center: text in the middle (default the total, formatted); centerLabel: small text under it; size: pixels (default 180);
//   legend: false to hide it; onSelect(item) when a part is tapped
function component(props) {
  css('sa-donut', `
.sa-donut{display:flex;flex-wrap:wrap;align-items:center;gap:18px}
.sa-donut-ring{position:relative;flex:none}
.sa-donut-ring svg{display:block;transform:rotate(-90deg)}
.sa-donut-ring circle.part{transition:opacity .1s}
.sa-donut-ring circle.part:hover{opacity:.8}
.sa-donut-mid{position:absolute;inset:0;display:flex;flex-direction:column;align-items:center;justify-content:center;text-align:center;pointer-events:none}
.sa-donut-total{font-weight:650;font-size:20px;letter-spacing:-.02em}
.sa-donut-sub{font-size:11.5px;color:var(--muted)}
.sa-donut-legend{display:flex;flex-direction:column;gap:5px;min-width:min(100%,220px);max-width:440px;flex:1;font-size:13px}
.sa-donut-legend .it{display:grid;grid-template-columns:10px 1fr auto auto;gap:8px;align-items:center;background:none;border:0;padding:0;box-shadow:none;text-align:left;font:inherit;color:inherit}
.sa-donut-legend button.it{cursor:pointer}
.sa-donut-legend i{width:10px;height:10px;border-radius:3px}
.sa-donut-legend .v{font-variant-numeric:tabular-nums}
.sa-donut-legend .p{color:var(--muted);font-size:12px;min-width:36px;text-align:right}
`);
  // tones of the app's own accent, so a chart is in its design (a part may still give its color)
  const PALETTE = ['var(--accent)', 'color-mix(in srgb, var(--accent) 55%, var(--card))', 'color-mix(in srgb, var(--accent) 62%, var(--fg))', 'color-mix(in srgb, var(--accent) 30%, var(--card))', 'color-mix(in srgb, var(--accent) 35%, var(--fg))', 'color-mix(in srgb, var(--accent) 16%, var(--card))', 'color-mix(in srgb, var(--fg) 45%, var(--card))', 'color-mix(in srgb, var(--fg) 25%, var(--card))'];
  const parts = (props.data || []).map((d, i) => ({ ...d, v: Number(d.value) || 0, color: d.color || PALETTE[i % PALETTE.length] })).filter((d) => d.v > 0);
  const total = parts.reduce((a, d) => a + d.v, 0);
  const fmt = props.format || ((v) => Number(v).toLocaleString(undefined, { maximumFractionDigits: 2 }));
  const size = Number(props.size) || 180;
  const stroke = Math.round(size * 0.16);
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  const ring = svg('svg', { width: size, height: size, viewBox: `0 0 ${size} ${size}`, role: 'img', 'aria-label': props.label || 'donut chart' },
    svg('circle', { cx: size / 2, cy: size / 2, r, fill: 'none', stroke: 'var(--soft)', 'stroke-width': stroke }));
  let at = 0;
  for (const d of parts) {
    const len = (c * d.v) / total;
    const gap = parts.length > 1 ? Math.min(2, len / 3) : 0;
    ring.append(svg('circle', { class: 'part', cx: size / 2, cy: size / 2, r, fill: 'none', stroke: d.color, 'stroke-width': stroke, 'stroke-dasharray': `${Math.max(0, len - gap).toFixed(2)} ${c.toFixed(2)}`, 'stroke-dashoffset': (-at).toFixed(2),
      style: props.onSelect ? 'cursor:pointer' : null, onclick: props.onSelect ? () => props.onSelect(d) : null }, svg('title', null, `${d.label}: ${fmt(d.v)} (${Math.round((100 * d.v) / total)}%)`)));
    at += len;
  }
  const pct = (v) => (total ? `${Math.round((100 * v) / total)}%` : '');
  return h('div', { class: 'sa-donut' },
    h('div', { class: 'sa-donut-ring', style: `width:${size}px;height:${size}px` }, ring,
      h('div', { class: 'sa-donut-mid' }, h('div', { class: 'sa-donut-total' }, props.center !== undefined ? String(props.center) : total ? fmt(total) : '–'), props.centerLabel ? h('div', { class: 'sa-donut-sub' }, props.centerLabel) : null)),
    props.legend === false ? null : h('div', { class: 'sa-donut-legend' }, parts.length ? parts.map((d) => {
      const kids = [h('i', { style: `background:${d.color}` }), h('span', { class: 'clip' }, d.label), h('span', { class: 'v' }, fmt(d.v)), h('span', { class: 'p' }, pct(d.v))];
      return props.onSelect ? h('button', { class: 'it', type: 'button', onclick: () => props.onSelect(d) }, kids) : h('div', { class: 'it' }, kids);
    }) : h('span', { class: 'muted' }, 'Nothing yet')));
}

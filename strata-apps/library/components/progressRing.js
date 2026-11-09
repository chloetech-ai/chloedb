// title: Progress ring
// description: A ring that fills toward a goal (glasses of water, pages read, budget spent) with the number in the middle; turns to the warning color past the goal when asked.
// props: value: number; max: the goal (default 100); size: pixels (default 96); label: small text under the number;
//   text: what the middle says (default value/max, or a percent when max is 100); color: css color (default the accent);
//   overColor: css color once value passes max (default the ring's color)
function component(props) {
  css('sa-ring', `
.sa-ring{display:inline-grid;place-items:center;position:relative}
.sa-ring svg{display:block;transform:rotate(-90deg)}
.sa-ring-mid{position:absolute;inset:0;display:flex;flex-direction:column;align-items:center;justify-content:center;text-align:center;line-height:1.1}
.sa-ring-val{font-weight:650;letter-spacing:-.02em}
.sa-ring-label{font-size:11px;color:var(--muted);max-width:80%}
`);
  const size = Number(props.size) || 96;
  const max = Number(props.max) > 0 ? Number(props.max) : 100;
  const value = Number(props.value) || 0;
  const frac = Math.max(0, Math.min(1, value / max));
  const over = value > max;
  const color = over && props.overColor ? props.overColor : props.color || 'var(--accent)';
  const stroke = Math.max(4, Math.round(size / 10));
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  const round = (n) => (Math.abs(n - Math.round(n)) < 1e-9 ? String(Math.round(n)) : n.toFixed(1));
  const text = props.text !== undefined ? String(props.text) : max === 100 ? `${round(value)}%` : `${round(value)}/${round(max)}`;
  return h('div', { class: 'sa-ring', role: 'img', 'aria-label': `${text}${props.label ? ` ${props.label}` : ''}`, style: `width:${size}px;height:${size}px` },
    svg('svg', { width: size, height: size, viewBox: `0 0 ${size} ${size}` },
      svg('circle', { cx: size / 2, cy: size / 2, r, fill: 'none', stroke: 'var(--soft)', 'stroke-width': stroke }),
      svg('circle', { cx: size / 2, cy: size / 2, r, fill: 'none', stroke: color, 'stroke-width': stroke, 'stroke-linecap': 'round', 'stroke-dasharray': `${(c * frac).toFixed(2)} ${c.toFixed(2)}` })),
    h('div', { class: 'sa-ring-mid' }, h('div', { class: 'sa-ring-val', style: `font-size:${Math.round(size / 4.6)}px` }, text), props.label ? h('div', { class: 'sa-ring-label' }, props.label) : null));
}

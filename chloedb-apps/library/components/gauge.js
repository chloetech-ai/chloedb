// title: Gauge
// description: A half-circle gauge for one number against its range, coloured by the band it falls in (green, amber, red by default), with the value in the middle; for scores, budget used, battery, speed, a target reached.
// props: value: number (null or missing shows a dash and an empty gauge; values outside min..max pin to the end but show as they are);
//   min: number (default 0); max: number (default 100; when not above min it becomes min + 1);
//   label: text under the gauge and its accessible name (optional, default "Gauge" for the name); format(value) -> text: how the value, min and max read
//   (default a plain number with at most one decimal); bands: [{to: number, color: css color}] each band running from the previous one's end
//   (or min) to its to, sorted by to (default green to 60% of the range, amber to 85%, red to max); the range past the last band is the line colour;
//   size: width in pixels (default 160; it shrinks to fit narrower space)
function component(props) {
  css('sa-gauge', `
.sa-gauge{display:inline-flex;flex-direction:column;align-items:center;max-width:100%;line-height:1.2}
.sa-gauge svg{display:block;max-width:100%;height:auto;overflow:visible}
.sa-gauge .v{fill:var(--fg);font-weight:600}
.sa-gauge .m{fill:var(--muted)}
.sa-gauge .l{color:var(--muted);font-size:.9em;text-align:center;margin-top:2px;max-width:100%;overflow-wrap:anywhere}
`);
  const num = (x) => (x === null || x === undefined || x === '' || !isFinite(Number(x)) ? null : Number(x));
  const min = num(props.min) ?? 0;
  let max = num(props.max) ?? 100;
  if (max <= min) max = min + 1;
  const span = max - min;
  const value = num(props.value);
  const size = Math.max(60, num(props.size) || 160);
  const plain = (v) => Number(v).toLocaleString(undefined, { maximumFractionDigits: 1 });
  const fmt = (v) => {
    if (typeof props.format !== 'function') return plain(v);
    try { const t = props.format(v); return t === null || t === undefined ? '' : String(t); } catch (e) { return plain(v); }
  };

  let bands = Array.isArray(props.bands)
    ? props.bands.filter((b) => b && num(b.to) !== null).map((b) => ({ to: Number(b.to), color: b.color || 'var(--accent)' })).sort((a, b) => a.to - b.to)
    : null;
  if (!bands || !bands.length) bands = [
    { to: min + span * 0.6, color: 'var(--ok)' },
    { to: min + span * 0.85, color: 'var(--warn)' },
    { to: max, color: 'var(--bad)' },
  ];

  const frac = (v) => Math.max(0, Math.min(1, (v - min) / span));
  const sw = size * 0.11;
  const r = (size - sw) / 2 - 1;
  const cx = size / 2;
  const cy = r + sw / 2 + 1;
  const fs = Math.max(10, size * 0.075);
  const height = cy + fs * 1.5;
  const pt = (f) => {
    const a = Math.PI * (1 - f);
    return `${(cx + r * Math.cos(a)).toFixed(2)} ${(cy - r * Math.sin(a)).toFixed(2)}`;
  };
  const arc = (f0, f1, color, opacity) => f1 - f0 <= 0.0005 ? null : svg('path', {
    d: `M ${pt(f0)} A ${r} ${r} 0 0 1 ${pt(f1)}`,
    fill: 'none', stroke: color, 'stroke-width': sw, 'stroke-linecap': 'butt', opacity: opacity,
  });

  const track = [arc(0, 1, 'var(--line)', 1)];
  let from = 0;
  for (const b of bands) {
    const to = frac(b.to);
    if (to > from) { track.push(arc(from, to, b.color, 0.28)); from = to; }
  }

  let color = 'var(--line)';
  if (value !== null) {
    const band = bands.find((b) => value <= b.to) || bands[bands.length - 1];
    color = band.color;
  }
  const f = value === null ? 0 : frac(value);
  const text = value === null ? '–' : fmt(value);

  const graphic = svg('svg', { viewBox: `0 0 ${size} ${height.toFixed(2)}`, width: size, height: height.toFixed(2), 'aria-hidden': 'true' },
    track,
    value !== null ? arc(0, f, color, 1) : null,
    value !== null && f > 0 ? svg('line', {
      x1: cx + (r - sw / 2) * Math.cos(Math.PI * (1 - f)), y1: cy - (r - sw / 2) * Math.sin(Math.PI * (1 - f)),
      x2: cx + (r + sw / 2) * Math.cos(Math.PI * (1 - f)), y2: cy - (r + sw / 2) * Math.sin(Math.PI * (1 - f)),
      stroke: 'var(--fg)', 'stroke-width': Math.max(2, size * 0.015),
    }) : null,
    svg('text', { class: 'v', x: cx, y: cy - r * 0.12, 'text-anchor': 'middle', 'font-size': (size * 0.2).toFixed(1) }, text),
    svg('text', { class: 'm', x: cx - r, y: cy + fs * 1.25, 'text-anchor': 'middle', 'font-size': fs.toFixed(1) }, fmt(min)),
    svg('text', { class: 'm', x: cx + r, y: cy + fs * 1.25, 'text-anchor': 'middle', 'font-size': fs.toFixed(1) }, fmt(max)),
  );

  const name = props.label ? String(props.label) : 'Gauge';
  const attrs = {
    class: 'sa-gauge', role: 'meter', style: `width:${size}px`,
    'aria-label': name, 'aria-valuemin': min, 'aria-valuemax': max,
    'aria-valuetext': value === null ? 'No value' : `${text} (${fmt(min)} to ${fmt(max)})`,
  };
  if (value !== null) attrs['aria-valuenow'] = Math.max(min, Math.min(max, value));
  return h('div', attrs, graphic, props.label ? h('div', { class: 'l' }, String(props.label)) : null);
}

// title: Stacked bar chart
// description: Bars split into coloured parts to compare totals and what makes them up (spending per month by category, hours per week by project, tasks per person by status), upright or across, as amounts or as shares of 100%, with a legend and the parts of a bar on hover or focus.
// props: data: [{label, values: {seriesKey: number}}] one bar each, in order (missing, negative or non-numeric values count as 0);
//   series: [{key, label?, color?}] the parts, bottom (or left) first (default: every key found in data's values, in order of appearance);
//   horizontal: true for bars across, label on the left and total on the right (default false);
//   percent: true to draw every bar to 100% and show each part's share (default false);
//   legend: false to hide the legend (default true); format(n): how amounts read (default a plain number);
//   height: pixels of the upright plot (default 220; across, the height follows the number of bars)
function component(props) {
  css('sa-stackedBar', `
.sa-stackedBar{position:relative;display:flex;flex-direction:column;gap:10px;width:100%;min-width:0;font-size:14px}
.sa-stackedBar .v{display:grid;grid-template-columns:auto minmax(0,1fr);column-gap:6px;row-gap:4px}
.sa-stackedBar .ya{position:relative;font-size:11px;color:var(--muted)}
.sa-stackedBar .ya span{position:absolute;right:0;transform:translateY(50%);line-height:1;white-space:nowrap;font-variant-numeric:tabular-nums}
.sa-stackedBar .area{position:relative;border-bottom:1px solid var(--line)}
.sa-stackedBar .gl{position:absolute;left:0;right:0;border-top:1px dashed var(--line);opacity:.7}
.sa-stackedBar .bars{position:absolute;inset:0;display:flex;gap:4px;align-items:flex-end}
.sa-stackedBar .slot{flex:1 1 0;min-width:0;height:100%;display:flex;align-items:flex-end;justify-content:center}
.sa-stackedBar .v .bar{width:100%;max-width:56px;display:flex;flex-direction:column-reverse;border-radius:4px 4px 0 0;overflow:hidden}
.sa-stackedBar .seg{flex:none;min-width:0;min-height:0}
.sa-stackedBar .bar{cursor:default;outline:none}
.sa-stackedBar .bar:focus-visible{box-shadow:0 0 0 2px var(--accent)}
.sa-stackedBar .xl{display:flex;gap:4px}
.sa-stackedBar .xl span{flex:1 1 0;min-width:0;text-align:center;font-size:11px;color:var(--muted);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.sa-stackedBar .hz{display:grid;grid-template-columns:fit-content(35%) minmax(0,1fr) auto;gap:6px 8px;align-items:center}
.sa-stackedBar .hz .lb{font-size:12px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.sa-stackedBar .hz .track{height:22px;display:flex;min-width:0}
.sa-stackedBar .hz .bar{height:100%;display:flex;flex-direction:row;border-radius:0 4px 4px 0;overflow:hidden}
.sa-stackedBar .hz .seg{height:100%}
.sa-stackedBar .hz .tot{font-size:12px;color:var(--muted);white-space:nowrap;font-variant-numeric:tabular-nums}
.sa-stackedBar .tip{position:absolute;z-index:5;pointer-events:none;display:none;background:var(--card);color:var(--fg);border:1px solid var(--line);border-radius:var(--radius,8px);box-shadow:0 4px 14px rgba(0,0,0,.18);padding:6px 8px;font-size:12px;min-width:130px;max-width:240px}
.sa-stackedBar .tip .th{font-weight:600;margin-bottom:4px;overflow-wrap:anywhere}
.sa-stackedBar .tip .tr{display:flex;gap:6px;align-items:center}
.sa-stackedBar .tip .tr span{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.sa-stackedBar .tip .tr.on{font-weight:600}
.sa-stackedBar .tip b{font-weight:inherit;font-variant-numeric:tabular-nums;white-space:nowrap}
.sa-stackedBar .tip .tt{display:flex;justify-content:space-between;gap:8px;border-top:1px solid var(--line);margin-top:4px;padding-top:4px;font-weight:600}
.sa-stackedBar i.sw{width:10px;height:10px;border-radius:2px;flex:none;display:inline-block}
.sa-stackedBar .legend{display:flex;flex-wrap:wrap;gap:4px 12px;font-size:12px;color:var(--muted)}
.sa-stackedBar .legend .li{display:inline-flex;align-items:center;gap:6px}
`);
  const PALETTE = ['var(--accent)', '#f59e0b', '#10b981', '#ef4444', '#8b5cf6', '#06b6d4', '#ec4899', '#84cc16', '#f97316', '#64748b'];
  const horizontal = !!props.horizontal;
  const percent = !!props.percent;
  const height = Math.max(80, Number(props.height) || 220);
  const fmt = typeof props.format === 'function' ? props.format
    : (n) => (typeof app !== 'undefined' && app.fmt && app.fmt.number) ? app.fmt.number(n) : String(Math.round(n * 100) / 100);
  const num = (v) => { const n = Number(v); return Number.isFinite(n) && n > 0 ? n : 0; };
  const data = Array.isArray(props.data) ? props.data.filter((d) => d && typeof d === 'object') : [];

  let series = [];
  const seen = new Set();
  if (Array.isArray(props.series) && props.series.length) {
    for (const s of props.series) {
      if (!s || s.key == null || seen.has(String(s.key))) continue;
      seen.add(String(s.key));
      series.push({ key: String(s.key), label: s.label != null ? String(s.label) : String(s.key), color: s.color });
    }
  } else {
    for (const d of data) for (const k of Object.keys(d.values || {})) {
      if (!seen.has(k)) { seen.add(k); series.push({ key: k, label: k }); }
    }
  }
  series = series.map((s, i) => ({ ...s, color: s.color || PALETTE[i % PALETTE.length] }));

  const root = h('div', { class: 'sa-stackedBar', role: 'group', 'aria-label': percent ? 'Stacked bar chart, shares of 100%' : 'Stacked bar chart' });
  if (!data.length || !series.length) {
    root.append(h('div', { class: 'empty' }, 'No data to show yet'));
    return root;
  }

  const bars = data.map((d) => {
    const v = d.values || {};
    const parts = series.map((s) => ({ s, value: num(v[s.key]) }));
    return { label: d.label == null ? '' : String(d.label), parts, total: parts.reduce((a, p) => a + p.value, 0) };
  });
  const share = (p, b) => (b.total > 0 ? p.value / b.total * 100 : 0);
  const pctText = (x) => `${Math.round(x * 10) / 10}%`;

  // Scale: a round top and evenly spaced ticks.
  const maxTotal = Math.max(0, ...bars.map((b) => b.total));
  let top, ticks;
  if (percent) { top = 100; ticks = [0, 25, 50, 75, 100]; }
  else if (maxTotal <= 0) { top = 1; ticks = [0]; }
  else {
    const raw = maxTotal / 4, p = Math.pow(10, Math.floor(Math.log10(raw))), f = raw / p;
    const step = (f <= 1 ? 1 : f <= 2 ? 2 : f <= 2.5 ? 2.5 : f <= 5 ? 5 : 10) * p;
    const n = Math.max(1, Math.ceil(maxTotal / step - 1e-9));
    top = step * n;
    ticks = Array.from({ length: n + 1 }, (_, i) => Math.round(i * step * 1e9) / 1e9);
  }
  const barSize = (b) => (b.total <= 0 ? 0 : percent ? 100 : b.total / top * 100);
  const tickText = (t) => (percent ? `${t}%` : String(fmt(t)));

  const describe = (b) => {
    const parts = b.parts.filter((p) => p.value > 0).map((p) => `${p.s.label} ${fmt(p.value)}${percent ? ` (${pctText(share(p, b))})` : ''}`);
    return `${b.label || 'Bar'}: ${parts.length ? parts.join(', ') : 'nothing'}; total ${fmt(b.total)}`;
  };

  // Tooltip
  const tip = h('div', { class: 'tip', role: 'tooltip' });
  let cur = null, curKey = null;
  const fill = (b, key) => {
    const rows = b.parts.filter((p) => p.value > 0).map((p) => h('div', { class: `tr${p.s.key === key ? ' on' : ''}` },
      h('i', { class: 'sw', style: `background:${p.s.color}` }),
      h('span', null, p.s.label),
      h('b', null, String(fmt(p.value)) + (percent ? ` (${pctText(share(p, b))})` : ''))));
    tip.replaceChildren(
      b.label ? h('div', { class: 'th' }, b.label) : null,
      rows.length ? rows : h('div', { class: 'muted' }, 'Nothing'),
      h('div', { class: 'tt' }, h('span', null, 'Total'), h('b', null, String(fmt(b.total)))));
  };
  const show = (b, key, x, y) => {
    if (b !== cur || key !== curKey) { cur = b; curKey = key; fill(b, key); }
    tip.style.display = 'block';
    const r = root.getBoundingClientRect(), tw = tip.offsetWidth, th = tip.offsetHeight;
    let left = x - r.left + 12;
    if (left + tw > r.width - 4) left = x - r.left - tw - 12;
    left = Math.max(4, Math.min(left, r.width - tw - 4));
    let topPx = y - r.top - th - 10;
    if (topPx < 0) topPx = y - r.top + 16;
    tip.style.left = `${left}px`;
    tip.style.top = `${topPx}px`;
  };
  const hide = () => { tip.style.display = 'none'; cur = null; curKey = null; };
  const keyOf = (e) => { const t = e.target && e.target.closest ? e.target.closest('[data-key]') : null; return t ? t.getAttribute('data-key') : null; };

  const barEl = (b) => h('div', {
    class: 'bar', tabindex: '0', role: 'img', 'aria-label': describe(b),
    style: horizontal ? `width:${barSize(b)}%` : `height:${barSize(b)}%`,
    onpointerenter: (e) => show(b, keyOf(e), e.clientX, e.clientY),
    onpointermove: (e) => show(b, keyOf(e), e.clientX, e.clientY),
    onpointerleave: hide,
    onfocus: (e) => { const r = e.currentTarget.getBoundingClientRect(); show(b, null, r.left + r.width / 2, r.top); },
    onblur: hide,
  }, b.parts.filter((p) => p.value > 0).map((p) => h('div', {
    class: 'seg', 'data-key': p.s.key,
    style: `${horizontal ? 'width' : 'height'}:${share(p, b)}%;background:${p.s.color}`,
  })));

  if (horizontal) {
    root.append(h('div', { class: 'hz' }, bars.map((b) => [
      h('div', { class: 'lb', title: b.label }, b.label),
      h('div', { class: 'track' }, barEl(b)),
      h('div', { class: 'tot' }, String(fmt(b.total))),
    ])));
  } else {
    const labels = ticks.map(tickText);
    const axisW = `${Math.max(1, ...labels.map((t) => t.length)) * 0.6 + 0.2}em`;
    root.append(h('div', { class: 'v' },
      h('div', { class: 'ya', 'aria-hidden': 'true', style: `height:${height}px;min-width:${axisW}` },
        ticks.map((t, i) => h('span', { style: `bottom:${t / top * 100}%` }, labels[i]))),
      h('div', { class: 'area', style: `height:${height}px` },
        ticks.slice(1).map((t) => h('div', { class: 'gl', style: `bottom:${t / top * 100}%` })),
        h('div', { class: 'bars' }, bars.map((b) => h('div', { class: 'slot' }, barEl(b))))),
      h('div', null),
      h('div', { class: 'xl', 'aria-hidden': 'true' }, bars.map((b) => h('span', { title: b.label }, b.label)))));
  }

  if (props.legend !== false) {
    root.append(h('div', { class: 'legend', role: 'list', 'aria-label': 'Legend' }, series.map((s) =>
      h('span', { class: 'li', role: 'listitem' }, h('i', { class: 'sw', style: `background:${s.color}` }), s.label))));
  }
  root.append(tip);
  return root;
}

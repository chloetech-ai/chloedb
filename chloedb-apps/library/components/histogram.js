// title: Histogram
// description: How a set of numbers is spread, as bars per range of values with the mean marked (session lengths, prices, test scores, response times, weights); hovering or tapping a bar tells its range and count.
// props: values: [numbers] to sort into bins (non-numbers are left out); bins: how many bins to make from values (default 10),
//   or [{from, to, count}] already counted (then values is only used for the mean); step: the width of each bin instead of a count;
//   label: a title above the chart (optional); format(n): how values and bin edges read (default a plain number, up to 2 decimals);
//   color: css color of the bars (default the accent); mean: false to hide the mean line (default true; from bin midpoints when only bins are given);
//   height: pixels of the plot (default 200)
function component(props) {
  css('sa-histogram', `
.sa-histogram{display:flex;flex-direction:column;gap:6px;min-width:0;width:100%}
.sa-histogram .sa-h-title{font-weight:600}
.sa-histogram .sa-h-wrap{display:flex;gap:6px;min-width:0}
.sa-histogram .sa-h-y{position:relative;width:32px;flex:none;font-size:11px;color:var(--muted)}
.sa-histogram .sa-h-y span{position:absolute;right:0;transform:translateY(-50%);white-space:nowrap}
.sa-histogram .sa-h-plot{position:relative;flex:1;min-width:0;border-bottom:1px solid var(--line)}
.sa-histogram .sa-h-grid{position:absolute;left:0;right:0;border-top:1px dashed var(--line);pointer-events:none}
.sa-histogram .sa-h-col{position:absolute;top:0;bottom:0;cursor:default;outline:none;border-radius:3px 3px 0 0}
.sa-histogram .sa-h-col:hover,.sa-histogram .sa-h-col.on{background:color-mix(in srgb,var(--c) 10%,transparent)}
.sa-histogram .sa-h-col:focus-visible{box-shadow:inset 0 0 0 2px var(--fg)}
.sa-histogram .sa-h-fill{position:absolute;bottom:0;left:1px;right:1px;background:var(--c);opacity:.8;border-radius:3px 3px 0 0;min-height:0}
.sa-histogram .sa-h-col:hover .sa-h-fill,.sa-histogram .sa-h-col.on .sa-h-fill{opacity:1}
.sa-histogram .sa-h-mean{position:absolute;top:0;bottom:0;border-left:2px dashed var(--fg);pointer-events:none;opacity:.7}
.sa-histogram .sa-h-meanlabel{position:absolute;top:0;font-size:11px;background:var(--card);color:var(--fg);padding:0 4px;border-radius:4px;white-space:nowrap;pointer-events:none}
.sa-histogram .sa-h-x{position:relative;height:16px;margin-left:38px;font-size:11px;color:var(--muted)}
.sa-histogram .sa-h-x span{position:absolute;top:0;white-space:nowrap}
.sa-histogram .sa-h-read{font-size:12px;color:var(--muted);min-height:1.3em}
`);
  const fmt = typeof props.format === 'function' ? props.format
    : (n) => (Math.round(n * 100) / 100).toLocaleString(undefined, { maximumFractionDigits: 2 });
  const color = props.color || 'var(--accent)';
  const height = Math.max(80, Number(props.height) || 200);
  const values = Array.isArray(props.values)
    ? props.values.filter((v) => v !== null && v !== '' && typeof v !== 'boolean').map(Number).filter(Number.isFinite)
    : [];

  let bins = [];
  if (Array.isArray(props.bins)) {
    bins = props.bins
      .filter((b) => b && Number.isFinite(Number(b.from)) && Number.isFinite(Number(b.to)))
      .map((b) => ({ from: Number(b.from), to: Number(b.to), count: Math.max(0, Number(b.count) || 0) }))
      .sort((a, b) => a.from - b.from);
  } else if (values.length) {
    const step = Number(props.step);
    const opts = step > 0 ? { step } : { buckets: Math.max(1, Math.min(200, Math.round(Number(props.bins) || 10))) };
    bins = (lib.stats.histogram(values, opts) || []).map((b) => ({ from: Number(b.from), to: Number(b.to), count: Number(b.count) || 0 }));
    if (!bins.length) {
      const v = values[0];
      bins = [{ from: v, to: v, count: values.length }];
    }
  }

  const root = h('div', { class: 'sa-histogram', style: `--c:${color}` });
  if (props.label) root.append(h('div', { class: 'sa-h-title' }, props.label));
  const total = bins.reduce((s, b) => s + b.count, 0);
  if (!bins.length) {
    root.append(h('div', { class: 'empty' }, 'No values to show yet'));
    return root;
  }

  let mean = null;
  if (props.mean !== false) {
    if (values.length) mean = lib.stats.mean(values);
    else if (total > 0) mean = bins.reduce((s, b) => s + b.count * (b.from + b.to) / 2, 0) / total;
    if (!Number.isFinite(mean)) mean = null;
  }

  let lo = bins[0].from;
  let hi = Math.max(...bins.map((b) => b.to));
  const degenerate = hi <= lo;
  if (degenerate) { lo -= 0.5; hi += 0.5; }
  const span = hi - lo;
  const pct = (v) => Math.max(0, Math.min(100, ((v - lo) / span) * 100));
  const maxCount = Math.max(1, ...bins.map((b) => b.count));

  const summary = `${total.toLocaleString()} value${total === 1 ? '' : 's'}` + (mean !== null ? ` · mean ${fmt(mean)}` : '');
  const read = h('div', { class: 'sa-h-read', 'aria-live': 'polite' }, summary);

  const plot = h('div', {
    class: 'sa-h-plot', style: `height:${height}px`, role: 'group',
    'aria-label': `${props.label || 'Histogram'}: ${summary}, from ${fmt(bins[0].from)} to ${fmt(Math.max(...bins.map((b) => b.to)))}`,
  });
  const yAxis = h('div', { class: 'sa-h-y', style: `height:${height}px`, 'aria-hidden': 'true' });
  const ticks = maxCount >= 2 ? [0, Math.round(maxCount / 2), maxCount] : [0, maxCount];
  [...new Set(ticks)].forEach((t) => {
    const top = (1 - t / maxCount) * 100;
    yAxis.append(h('span', { style: `top:${top}%` }, t.toLocaleString()));
    if (t > 0) plot.append(h('div', { class: 'sa-h-grid', style: `top:${top}%` }));
  });

  let active = null;
  const show = (col, b) => {
    if (active && active !== col) active.classList.remove('on');
    active = col;
    col.classList.add('on');
    const share = total > 0 ? Math.round((b.count / total) * 100) : 0;
    const range = b.from === b.to ? fmt(b.from) : `${fmt(b.from)} – ${fmt(b.to)}`;
    read.textContent = `${range}: ${b.count.toLocaleString()} (${share}%)`;
  };
  const hide = (col) => {
    col.classList.remove('on');
    if (active === col) active = null;
    read.textContent = summary;
  };

  bins.forEach((b, i) => {
    let left, width;
    if (degenerate) { left = (i / bins.length) * 100; width = 100 / bins.length; }
    else { left = pct(b.from); width = Math.max(0.5, pct(b.to) - left); }
    const range = b.from === b.to ? fmt(b.from) : `${fmt(b.from)} to ${fmt(b.to)}`;
    const col = h('div', {
      class: 'sa-h-col', tabindex: '0', role: 'img',
      style: `left:${left}%;width:${width}%`,
      'aria-label': `${range}: ${b.count}`, title: `${range}: ${b.count}`,
    }, h('div', { class: 'sa-h-fill', style: `height:${(b.count / maxCount) * 100}%` }));
    col.onpointerenter = () => show(col, b);
    col.onpointerleave = () => hide(col);
    col.onfocus = () => show(col, b);
    col.onblur = () => hide(col);
    col.onclick = () => show(col, b);
    plot.append(col);
  });

  if (mean !== null) {
    const m = pct(mean);
    const shift = m < 15 ? 'translateX(4px)' : m > 85 ? 'translateX(calc(-100% - 4px))' : 'translateX(-50%)';
    plot.append(
      h('div', { class: 'sa-h-mean', style: `left:${m}%`, 'aria-hidden': 'true' }),
      h('div', { class: 'sa-h-meanlabel', style: `left:${m}%;transform:${shift}`, 'aria-hidden': 'true' }, `mean ${fmt(mean)}`),
    );
  }

  const xAxis = h('div', { class: 'sa-h-x', 'aria-hidden': 'true' });
  const edges = [...new Set([...bins.map((b) => b.from), Math.max(...bins.map((b) => b.to))])].sort((a, b) => a - b);
  const every = Math.max(1, Math.ceil((edges.length - 1) / 4));
  const picked = [];
  edges.forEach((e, i) => {
    if (i === edges.length - 1) {
      if (picked.length && i - picked[picked.length - 1].i < every / 2 && picked.length > 1) picked.pop();
      picked.push({ e, i });
    } else if (i % every === 0) picked.push({ e, i });
  });
  picked.forEach(({ e }, k) => {
    const p = degenerate ? 50 : pct(e);
    const t = picked.length === 1 ? 'translateX(-50%)' : k === 0 ? 'none' : k === picked.length - 1 ? 'translateX(-100%)' : 'translateX(-50%)';
    xAxis.append(h('span', { style: `left:${p}%;transform:${t}` }, fmt(e)));
  });

  root.append(h('div', { class: 'sa-h-wrap' }, yAxis, plot), xAxis, read);
  return root;
}

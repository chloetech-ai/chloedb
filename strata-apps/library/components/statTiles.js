// title: Stat tiles
// description: A row of key numbers (totals, counts, averages), each with a label, an optional change against before (green up, red down, or the other way round) and a tiny trend line.
// props: items: [{label, value (number or text), format? ('number' | 'money' | 'percent' | a function of the value), currency? (for money, default 'USD'),
//   delta? (the change, a number: shown as +12% when deltaFormat is 'percent', the default, or +12 with 'number'), deltaFormat?, goodWhen? ('up' default | 'down'),
//   hint? (small text under it), trend? ([numbers] drawn as a sparkline), onClick? }]
function component(props) {
  css('sa-stats', `
.sa-stats{display:grid;grid-template-columns:repeat(auto-fit,minmax(150px,1fr));gap:12px}
.sa-stat{background:var(--card);border:1px solid var(--line);border-radius:var(--radius);padding:12px 14px;display:flex;flex-direction:column;gap:2px;text-align:left;box-shadow:var(--shadow);font:inherit;color:inherit}
button.sa-stat{cursor:pointer;white-space:normal}
button.sa-stat:hover{border-color:var(--fg)}
.sa-stat-label{color:var(--muted);font-size:13px}
.sa-stat-row{display:flex;align-items:flex-end;justify-content:space-between;gap:8px}
.sa-stat-value{font-size:26px;font-weight:650;letter-spacing:-.02em;line-height:1.15}
.sa-stat-delta{font-size:12.5px;font-weight:600}
.sa-stat-delta.good{color:var(--ok)} .sa-stat-delta.bad{color:var(--bad)} .sa-stat-delta.flat{color:var(--muted)}
.sa-stat-hint{color:var(--muted);font-size:12px}
`);
  const fmtValue = (it) => {
    const v = it.value;
    if (v === null || v === undefined || v === '') return '–';
    if (typeof it.format === 'function') return String(it.format(v));
    if (it.format === 'money') return lib.money.format(Number(v), it.currency || 'USD');
    if (it.format === 'percent') return `${Math.round(Number(v) * 10) / 10}%`;
    if (typeof v === 'number') return v.toLocaleString(undefined, { maximumFractionDigits: 2 });
    return String(v);
  };
  const delta = (it) => {
    if (it.delta === null || it.delta === undefined || !Number.isFinite(Number(it.delta))) return null;
    const d = Number(it.delta);
    const tone = d === 0 ? 'flat' : (d > 0) === (it.goodWhen !== 'down') ? 'good' : 'bad';
    const n = Math.round(Math.abs(d) * 10) / 10;
    const text = `${d > 0 ? '▲' : d < 0 ? '▼' : ''} ${it.deltaFormat === 'number' ? n.toLocaleString() : `${n}%`}`.trim();
    return h('span', { class: `sa-stat-delta ${tone}` }, text);
  };
  return h('div', { class: 'sa-stats' }, (props.items || []).map((it) => {
    const kids = [
      h('span', { class: 'sa-stat-label' }, it.label || ''),
      h('div', { class: 'sa-stat-row' }, h('span', { class: 'sa-stat-value' }, fmtValue(it)), Array.isArray(it.trend) && it.trend.length > 1 ? ui.sparkline({ values: it.trend, width: 70, height: 26 }) : null),
      delta(it),
      it.hint ? h('span', { class: 'sa-stat-hint' }, it.hint) : null,
    ];
    return it.onClick ? h('button', { class: 'sa-stat', type: 'button', onclick: () => it.onClick(it) }, kids) : h('div', { class: 'sa-stat' }, kids);
  }));
}

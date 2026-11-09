// title: Activity heatmap
// description: A grid of days, one column per week, shaded by how much happened each day (like a contribution graph), with month labels; for streaks, workouts, spending or anything counted per day.
// props: values: [{date: 'YYYY-MM-DD', value: number}] or {date: number} (several entries on a day add up);
//   weeks: how many weeks to show (default 26); end: the last day (default today); weekStart: 1 Monday (default) or 0 Sunday;
//   color: css color of the busiest day (default the accent); format(value, date): tooltip text (default "3 on 2026-11-07");
//   onSelect(date) when a day is tapped
function component(props) {
  css('sa-heatmap', `
.sa-heat{display:flex;flex-direction:column;gap:4px;max-width:100%;overflow-x:auto}
.sa-heat-grid{display:grid;grid-auto-flow:column;grid-template-rows:repeat(7,12px);gap:3px}
.sa-heat-months{display:grid;grid-auto-flow:column;gap:3px;font-size:10.5px;color:var(--muted);height:14px}
.sa-heat-months span{white-space:nowrap;overflow:visible;width:12px}
.sa-heat-cell{width:12px;height:12px;border-radius:3px;background:var(--soft);border:0;padding:0;box-shadow:none}
button.sa-heat-cell{cursor:pointer}
.sa-heat-cell.lvl{background:color-mix(in srgb,var(--c) var(--p),var(--soft))}
.sa-heat-cell.today{outline:1.5px solid var(--fg);outline-offset:0}
.sa-heat-cell.pad{visibility:hidden}
.sa-heat-legend{display:flex;align-items:center;gap:3px;font-size:10.5px;color:var(--muted);align-self:flex-end}
`);
  const D = lib.dates;
  const weeks = Math.max(1, Math.min(106, Number(props.weeks) || 26));
  const weekStart = props.weekStart === 0 ? 0 : 1;
  const end = D.isDay(props.end || '') ? props.end : app.today();
  const today = app.today();
  const sums = new Map();
  const add = (d, v) => { const day = String(d).length > 10 ? D.fromTime(d) : String(d); sums.set(day, (sums.get(day) || 0) + (Number(v) || 0)); };
  if (Array.isArray(props.values)) for (const r of props.values) { if (r && r.date) add(r.date, r.value === undefined ? 1 : r.value); } else for (const [d, v] of Object.entries(props.values || {})) add(d, v);
  const first = D.addDays(D.startOfWeek(end, weekStart), -7 * (weeks - 1));
  const max = Math.max(0, ...[...sums.entries()].filter(([d]) => d >= first && d <= end).map(([, v]) => v));
  const color = props.color || 'var(--accent)';
  const tip = props.format || ((v, d) => `${v} on ${d}`);
  const cells = [];
  const months = [];
  for (let w = 0; w < weeks; w++) {
    const start = D.addDays(first, 7 * w);
    // A week is labelled with the month that starts in it; the first week
    // with its own month unless one starts within the next two weeks.
    const startsMonth = (s0) => [0, 1, 2, 3, 4, 5, 6].some((i) => D.addDays(s0, i).endsWith('-01'));
    const label = startsMonth(start) || (w === 0 && !startsMonth(D.addDays(start, 7)) && !startsMonth(D.addDays(start, 14))) ? D.monthName(D.addDays(start, 6), true) : '';
    months.push(h('span', null, label));
    for (let i = 0; i < 7; i++) {
      const d = D.addDays(start, i);
      if (d > end) { cells.push(h('span', { class: 'sa-heat-cell pad' })); continue; }
      const v = sums.get(d) || 0;
      const p = v > 0 && max > 0 ? Math.round(25 + 75 * Math.min(1, v / max)) : 0;
      const attrs = { class: `sa-heat-cell${p ? ' lvl' : ''}${d === today ? ' today' : ''}`, style: p ? `--c:${color};--p:${p}%` : null, title: tip(v, d) };
      cells.push(props.onSelect ? h('button', { ...attrs, type: 'button', 'aria-label': tip(v, d), onclick: () => props.onSelect(d) }) : h('span', attrs));
    }
  }
  const legend = [0, 25, 50, 75, 100].map((p) => h('span', { class: `sa-heat-cell${p ? ' lvl' : ''}`, style: p ? `--c:${color};--p:${p}%` : null }));
  return h('div', { class: 'sa-heat' }, h('div', { class: 'sa-heat-months' }, months), h('div', { class: 'sa-heat-grid' }, cells),
    h('div', { class: 'sa-heat-legend' }, 'less', legend, 'more'));
}

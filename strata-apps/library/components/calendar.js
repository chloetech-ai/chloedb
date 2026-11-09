// title: Month calendar
// description: A month grid with previous and next buttons. Days with items are filled with their color and can show a small badge (a count, a note); today is outlined; tapping a day reports it.
// props: items: [{date: 'YYYY-MM-DD' (or an ISO time), color? (css color, default the accent), label? (tooltip), badge? (small text under the number)}];
//   month: 'YYYY-MM' to show (default this month); onMonth(month) when the person moves to another;
//   onSelect(date) when a day is tapped (days are buttons only then); selected: 'YYYY-MM-DD' to ring;
//   weekStart: 1 Monday (default) or 0 Sunday; minDate, maxDate: 'YYYY-MM-DD', days outside are disabled
function component(props) {
  css('sa-calendar', `
.sa-cal{width:100%;max-width:440px}
.sa-cal-head{display:flex;align-items:center;gap:8px;margin-bottom:8px}
.sa-cal-title{flex:1;text-align:center;font-weight:600}
.sa-cal-grid{display:grid;grid-template-columns:repeat(7,minmax(0,1fr));gap:4px}
.sa-cal-dow{text-align:center;font-size:11px;color:var(--muted);text-transform:uppercase;letter-spacing:.04em;padding:2px 0}
.sa-cal-day{aspect-ratio:1;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:1px;border-radius:8px;border:1px solid var(--line);background:var(--card);color:var(--fg);font-size:14px;line-height:1.1;padding:0;margin:0;min-width:0;box-shadow:none}
button.sa-cal-day{cursor:pointer}
button.sa-cal-day:hover{border-color:var(--fg)}
.sa-cal-day.on{background:var(--c);border-color:var(--c);color:var(--bg);font-weight:600}
.sa-cal-day.today{outline:2px solid var(--accent);outline-offset:1px}
.sa-cal-day.sel{box-shadow:0 0 0 3px color-mix(in srgb,var(--fg) 35%,transparent)}
.sa-cal-day.blank{visibility:hidden}
.sa-cal-day:disabled{opacity:.35;cursor:default}
.sa-cal-badge{font-size:10px;font-weight:600;max-width:100%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;padding:0 2px}
`);
  const D = lib.dates;
  const today = app.today();
  let month = /^\d{4}-\d{2}$/.test(props.month || '') ? props.month : today.slice(0, 7);
  const byDate = new Map();
  for (const it of props.items || []) {
    if (!it || !it.date) continue;
    const d = String(it.date).length > 10 ? D.fromTime(it.date) : String(it.date);
    if (!byDate.has(d)) byDate.set(d, []);
    byDate.get(d).push(it);
  }
  const root = h('div', { class: 'sa-cal' });
  const go = (n) => { month = D.month(D.addMonths(`${month}-01`, n)); draw(); if (props.onMonth) props.onMonth(month); };
  function draw() {
    const weekStart = props.weekStart === 0 ? 0 : 1;
    const dows = Array.from({ length: 7 }, (_, i) => h('div', { class: 'sa-cal-dow' }, D.weekdayName((weekStart + i) % 7, true).slice(0, 2)));
    const cells = [];
    for (const week of D.monthGrid(month, weekStart)) {
      for (const date of week) {
        if (!date) { cells.push(h('div', { class: 'sa-cal-day blank' })); continue; }
        const its = byDate.get(date) || [];
        const off = (props.minDate && date < props.minDate) || (props.maxDate && date > props.maxDate);
        const cls = ['sa-cal-day', its.length ? 'on' : '', date === today ? 'today' : '', date === props.selected ? 'sel' : ''].filter(Boolean).join(' ');
        const tip = its.map((x) => x.label).filter(Boolean).join(', ');
        const badge = its.find((x) => x.badge !== undefined && x.badge !== null && x.badge !== '');
        const kids = [h('span', null, String(Number(date.slice(8)))), badge ? h('span', { class: 'sa-cal-badge' }, String(badge.badge)) : null];
        const attrs = { class: cls, style: its.length ? `--c:${its[0].color || 'var(--accent)'}` : null, title: tip || null };
        cells.push(props.onSelect
          ? h('button', { ...attrs, type: 'button', disabled: !!off, 'aria-label': `${date}${tip ? `, ${tip}` : ''}`, onclick: () => props.onSelect(date) }, kids)
          : h('div', attrs, kids));
      }
    }
    root.replaceChildren(
      h('div', { class: 'sa-cal-head' },
        h('button', { class: 'icon ghost', type: 'button', 'aria-label': 'Previous month', onclick: () => go(-1) }, '‹'),
        h('div', { class: 'sa-cal-title' }, `${D.monthName(month)} ${month.slice(0, 4)}`),
        h('button', { class: 'icon ghost', type: 'button', 'aria-label': 'Next month', onclick: () => go(1) }, '›')),
      h('div', { class: 'sa-cal-grid' }, dows, cells));
  }
  draw();
  return root;
}

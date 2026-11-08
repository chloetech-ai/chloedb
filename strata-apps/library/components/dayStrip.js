// title: Day strip
// description: A row of the last few days (a week by default) as small round buttons, each filled when done, for checking off a habit, a workout or a chore per day.
// props: marks: ['YYYY-MM-DD', ...] or {date: true | color | count} for the days that are done;
//   days: how many days to show (default 7); end: the last day shown (default today);
//   onToggle(date, done) when a day is tapped, done being its new state (days are buttons only then);
//   color: css color of a done day (default the accent); label: text before the days (optional)
function component(props) {
  css('sa-daystrip', `
.sa-days{display:flex;align-items:center;gap:6px;flex-wrap:wrap}
.sa-days-label{font-size:13px;color:var(--muted);margin-right:4px}
.sa-day{display:flex;flex-direction:column;align-items:center;gap:2px;background:none;border:0;padding:0;box-shadow:none;color:var(--muted);font-size:10.5px;min-width:30px}
button.sa-day{cursor:pointer}
.sa-day i{width:28px;height:28px;border-radius:50%;border:1.5px solid var(--line);display:grid;place-items:center;font-style:normal;font-size:11px;font-weight:600;color:var(--fg);background:var(--card)}
.sa-day.on i{background:var(--c);border-color:var(--c);color:var(--bg)}
.sa-day.today{color:var(--fg);font-weight:600}
.sa-day.today i{outline:2px solid var(--accent);outline-offset:1px}
button.sa-day:hover i{border-color:var(--fg)}
`);
  const D = lib.dates;
  const n = Math.max(1, Math.min(31, Number(props.days) || 7));
  const end = D.isDay(props.end || '') ? props.end : app.today();
  const marks = new Map();
  if (Array.isArray(props.marks)) for (const d of props.marks) marks.set(String(d).slice(0, 10), true);
  else for (const [d, v] of Object.entries(props.marks || {})) if (v) marks.set(d, v);
  const today = app.today();
  const days = D.range(D.addDays(end, -(n - 1)), end);
  return h('div', { class: 'sa-days' },
    props.label ? h('span', { class: 'sa-days-label' }, props.label) : null,
    days.map((d) => {
      const v = marks.get(d);
      const on = !!v;
      const color = typeof v === 'string' ? v : props.color || 'var(--accent)';
      const inner = h('i', null, typeof v === 'number' && v > 1 ? String(v) : '');
      const attrs = { class: `sa-day${on ? ' on' : ''}${d === today ? ' today' : ''}`, style: `--c:${color}`, title: `${D.weekdayName(d)} ${d}${on ? ', done' : ''}` };
      const kids = [inner, D.weekdayName(d, true).slice(0, 2)];
      return props.onToggle
        ? h('button', { ...attrs, type: 'button', 'aria-pressed': on ? 'true' : 'false', onclick: () => props.onToggle(d, !on) }, kids)
        : h('span', attrs, kids);
    }));
}

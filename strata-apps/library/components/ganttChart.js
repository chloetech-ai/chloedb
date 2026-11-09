// title: Gantt chart
// description: Tasks as bars on a timeline of days, with progress, groups and a line for today, scrolling sideways; for project plans, trips, renovations and study schedules.
// props: tasks: [{id, label, start: 'YYYY-MM-DD', end: 'YYYY-MM-DD' (default start; inclusive), progress?: 0..1, color?: css color (default the accent), group?: text}];
//   tasks with no valid start are left out, and an end before the start is swapped;
//   from, to: 'YYYY-MM-DD', the days shown (default: fit the tasks, widened to whole weeks on the week scale; this week and the next three when there are none);
//   scale: 'day' | 'week' | 'auto' (default 'auto': days up to 62 days shown, weeks beyond); today: 'YYYY-MM-DD' where the today line goes (default app.today(); false hides it);
//   onSelect(id) when a bar or a task's name is tapped (they are buttons only then); selected: a task id to ring;
//   label: accessible name (default 'Timeline'); empty: text when there are no tasks (default 'No tasks yet')
function component(props) {
  css('sa-gantt', `
.sa-gantt{--lw:150px;--rh:34px;overflow-x:auto;border:1px solid var(--line);border-radius:var(--radius);background:var(--card);-webkit-overflow-scrolling:touch}
.sa-gantt-in{position:relative;min-width:100%}
.sa-gantt-row{display:flex;height:var(--rh);border-top:1px solid var(--line)}
.sa-gantt-head{height:44px;border-top:0}
.sa-gantt-lab{position:sticky;left:0;z-index:3;flex:0 0 var(--lw);width:var(--lw);box-sizing:border-box;padding:0 8px;display:flex;align-items:center;background:var(--card);border-right:1px solid var(--line);overflow:hidden;font-size:.9em}
.sa-gantt-lab span,.sa-gantt-lab button{white-space:nowrap;overflow:hidden;text-overflow:ellipsis;min-width:0}
.sa-gantt-lab button{border:0;background:none;box-shadow:none;padding:0;margin:0;color:var(--fg);font:inherit;text-align:left;cursor:pointer}
.sa-gantt-lab button:hover{text-decoration:underline}
.sa-gantt-grp .sa-gantt-lab{font-weight:600}
.sa-gantt-grp{background:color-mix(in srgb,var(--line) 30%,transparent)}
.sa-gantt-head .sa-gantt-lab{color:var(--muted);font-size:.8em}
.sa-gantt-track{position:relative;flex:0 0 auto}
.sa-gantt-mon,.sa-gantt-tick{position:absolute;box-sizing:border-box;font-size:.72em;color:var(--muted);white-space:nowrap;overflow:hidden;padding:0 4px;line-height:22px;height:22px}
.sa-gantt-mon{top:0;border-left:1px solid var(--line);font-weight:600;color:var(--fg)}
.sa-gantt-tick{top:22px;text-align:center;padding:0}
.sa-gantt-tick.we{opacity:.6}
.sa-gantt-tick.now{color:var(--accent);font-weight:700}
.sa-gantt-bar{position:absolute;top:7px;height:calc(var(--rh) - 15px);box-sizing:border-box;border:0;padding:0;margin:0;box-shadow:none;border-radius:5px;overflow:hidden;background:color-mix(in srgb,var(--c) 32%,transparent);min-width:4px}
button.sa-gantt-bar{cursor:pointer}
button.sa-gantt-bar:hover{filter:brightness(1.08)}
.sa-gantt-bar i{position:absolute;left:0;top:0;bottom:0;background:var(--c)}
.sa-gantt-bar.cl{border-top-left-radius:0;border-bottom-left-radius:0}
.sa-gantt-bar.cr{border-top-right-radius:0;border-bottom-right-radius:0}
.sa-gantt-bar.sel{outline:2px solid var(--fg);outline-offset:1px}
.sa-gantt-today{position:absolute;top:22px;bottom:0;width:2px;margin-left:-1px;background:var(--bad);z-index:2;pointer-events:none;opacity:.8}
@media (max-width:480px){.sa-gantt{--lw:100px}}
`);
  const D = lib.dates;
  const fmt = (d) => `${Number(d.slice(8))} ${D.monthName(d, true)}`;
  const tasks = (Array.isArray(props.tasks) ? props.tasks : [])
    .filter((t) => t && D.isDay(t.start))
    .map((t) => {
      let s = t.start, e = D.isDay(t.end) ? t.end : t.start;
      if (e < s) [s, e] = [e, s];
      return { ...t, start: s, end: e };
    });
  const label = props.label || 'Timeline';
  if (!tasks.length) {
    return h('div', { class: 'sa-gantt', role: 'figure', 'aria-label': label },
      h('p', { class: 'empty' }, props.empty || 'No tasks yet'));
  }
  const today = props.today === false ? null : (D.isDay(props.today) ? props.today : app.today());

  let from = D.isDay(props.from) ? props.from : tasks.reduce((m, t) => (t.start < m ? t.start : m), tasks[0].start);
  let to = D.isDay(props.to) ? props.to : tasks.reduce((m, t) => (t.end > m ? t.end : m), tasks[0].end);
  if (to < from) [from, to] = [to, from];
  if (D.diffDays(from, to) > 1099) to = D.addDays(from, 1099);
  const scale = props.scale === 'day' || props.scale === 'week' ? props.scale : (D.diffDays(from, to) < 62 ? 'day' : 'week');
  if (scale === 'week') {
    if (!D.isDay(props.from)) from = D.startOfWeek(from, 1);
    if (!D.isDay(props.to)) to = D.endOfWeek(to, 1);
  }
  const dw = scale === 'day' ? 28 : 10;
  const days = D.range(from, to);
  const W = days.length * dw;
  const x = (d) => D.diffDays(from, d) * dw;

  // header: months above, days or weeks below
  const head = [];
  for (let d = from; d <= to;) {
    const end = D.endOfMonth(d) < to ? D.endOfMonth(d) : to;
    const w = (D.diffDays(d, end) + 1) * dw;
    const showYear = d === from || d.slice(5, 7) === '01';
    head.push(h('div', { class: 'sa-gantt-mon', style: `left:${x(d)}px;width:${w}px` },
      w >= 28 ? D.monthName(d, true) + (showYear && w >= 64 ? ' ' + d.slice(0, 4) : '') : ''));
    d = D.addDays(end, 1);
  }
  for (const d of days) {
    const wd = D.weekday(d);
    if (scale === 'day') {
      head.push(h('div', { class: `sa-gantt-tick${wd === 0 || wd === 6 ? ' we' : ''}${d === today ? ' now' : ''}`, style: `left:${x(d)}px;width:${dw}px`, title: D.weekdayName(d, true) + ' ' + fmt(d) }, String(Number(d.slice(8)))));
    } else if (wd === 1) {
      head.push(h('div', { class: 'sa-gantt-tick', style: `left:${x(d)}px;width:${7 * dw}px;text-align:left;padding-left:3px`, title: 'Week of ' + fmt(d) }, String(Number(d.slice(8)))));
    }
  }

  const step = scale === 'day' ? dw : 7 * dw;
  const off = scale === 'day' ? 0 : ((8 - D.weekday(from)) % 7) * dw;
  const grid = `background:linear-gradient(to right,color-mix(in srgb,var(--line) 70%,transparent) 1px,transparent 1px) ${off}px 0/${step}px 100% repeat-x`;
  const select = typeof props.onSelect === 'function' ? props.onSelect : null;

  const taskRow = (t) => {
    const name = String(t.label == null ? '' : t.label);
    const p = Math.max(0, Math.min(1, Number(t.progress) || 0));
    const desc = `${name}, ${fmt(t.start)}${t.end !== t.start ? ' to ' + fmt(t.end) : ''}${t.progress != null ? `, ${Math.round(p * 100)}% done` : ''}`;
    let bar = null;
    if (t.end >= from && t.start <= to) {
      const s = t.start < from ? from : t.start, e = t.end > to ? to : t.end;
      const cls = `sa-gantt-bar${t.start < from ? ' cl' : ''}${t.end > to ? ' cr' : ''}${props.selected != null && props.selected === t.id ? ' sel' : ''}`;
      const attrs = { class: cls, style: `left:${x(s)}px;width:${(D.diffDays(s, e) + 1) * dw}px;--c:${t.color || 'var(--accent)'}`, title: desc };
      const fill = p > 0 ? h('i', { style: `width:${p * 100}%` }) : null;
      bar = select
        ? h('button', { ...attrs, type: 'button', 'aria-label': desc, onclick: () => select(t.id) }, fill)
        : h('div', { ...attrs, role: 'img', 'aria-label': desc }, fill);
    }
    return h('div', { class: 'sa-gantt-row', role: 'listitem' },
      h('div', { class: 'sa-gantt-lab' }, select
        ? h('button', { type: 'button', title: desc, tabindex: '-1', onclick: () => select(t.id) }, name)
        : h('span', { title: desc }, name)),
      h('div', { class: 'sa-gantt-track', style: `width:${W}px;${grid}` }, bar));
  };

  const rows = [];
  const groups = [];
  const byGroup = new Map();
  for (const t of tasks) {
    const g = t.group == null || t.group === '' ? '' : String(t.group);
    if (!byGroup.has(g)) { byGroup.set(g, []); groups.push(g); }
    byGroup.get(g).push(t);
  }
  if (byGroup.has('')) byGroup.get('').forEach((t) => rows.push(taskRow(t)));
  for (const g of groups) {
    if (g === '') continue;
    rows.push(h('div', { class: 'sa-gantt-row sa-gantt-grp', role: 'listitem' },
      h('div', { class: 'sa-gantt-lab' }, h('span', { title: g }, g)),
      h('div', { class: 'sa-gantt-track', style: `width:${W}px` })));
    byGroup.get(g).forEach((t) => rows.push(taskRow(t)));
  }

  const showToday = today && today >= from && today <= to;
  const inner = h('div', { class: 'sa-gantt-in', style: `width:calc(var(--lw) + ${W}px)` },
    h('div', { class: 'sa-gantt-row sa-gantt-head', 'aria-hidden': 'true' },
      h('div', { class: 'sa-gantt-lab' }, 'Task'),
      h('div', { class: 'sa-gantt-track', style: `width:${W}px` }, head)),
    h('div', { role: 'list' }, rows),
    showToday ? h('div', { class: 'sa-gantt-today', style: `left:calc(var(--lw) + ${x(today) + dw / 2}px)`, title: 'Today', 'aria-hidden': 'true' }) : null);

  const root = h('div', { class: 'sa-gantt', role: 'figure', 'aria-label': `${label}: ${fmt(from)} to ${fmt(to)}` }, inner);
  if (showToday) {
    requestAnimationFrame(() => {
      if (!root.isConnected) return;
      const lab = root.querySelector('.sa-gantt-lab');
      const visible = root.clientWidth - (lab ? lab.offsetWidth : 0);
      const pos = x(today);
      if (pos > visible * 0.7) root.scrollLeft = Math.max(0, pos - visible / 3);
    });
  }
  return root;
}

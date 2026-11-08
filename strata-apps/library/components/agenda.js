// title: Agenda
// description: Upcoming events listed day by day under headings like 'Today', 'Tomorrow' and 'Thu 12 Nov', all-day events first and then by time, with place and colour, tapping one to open it; for appointments, classes, bookings and plans.
// props: events: [{id, title, start ('YYYY-MM-DD' for an all-day event, or an ISO time), end? (a 'YYYY-MM-DD' last day for all-day events,
//   inclusive, or an ISO time), place? (text), color? (css color, default the accent)}]; an event spanning several days shows on each of them;
//   from: the first day shown, 'YYYY-MM-DD' (default today); days: how many days to show (default 14, 1 to 366);
//   onSelect(id) when an event is tapped (events are buttons only then); showEmptyDays: true to list days without events too (default false);
//   empty: text when no event falls in the days shown (default 'Nothing coming up')
function component(props) {
  css('sa-agenda', `
.sa-agenda{display:flex;flex-direction:column;gap:14px;min-width:0}
.sa-agenda-head{display:flex;align-items:baseline;gap:8px;flex-wrap:wrap;font-weight:600;padding-bottom:4px;margin-bottom:4px;border-bottom:1px solid var(--line)}
.sa-agenda-head small{font-weight:400;color:var(--muted)}
.sa-agenda-day.today .sa-agenda-name{color:var(--accent)}
.sa-agenda-list{list-style:none;margin:0;padding:0;display:flex;flex-direction:column;gap:2px}
.sa-agenda-list li{margin:0;padding:0}
.sa-agenda-ev{display:flex;gap:10px;align-items:flex-start;width:100%;box-sizing:border-box;text-align:left;margin:0;padding:6px 8px;border:0;border-left:3px solid var(--c,var(--accent));border-radius:0 var(--radius) var(--radius) 0;background:none;box-shadow:none;color:inherit;font:inherit;line-height:1.35}
button.sa-agenda-ev{cursor:pointer}
button.sa-agenda-ev:hover,button.sa-agenda-ev:focus-visible{background:color-mix(in srgb,var(--c,var(--accent)) 12%,transparent)}
.sa-agenda-when{flex:0 0 5.2em;display:flex;flex-direction:column;color:var(--muted);font-size:.9em;font-variant-numeric:tabular-nums;white-space:nowrap}
.sa-agenda-when small{font-size:.85em}
.sa-agenda-what{flex:1;min-width:0;display:flex;flex-direction:column}
.sa-agenda-title{overflow-wrap:anywhere}
.sa-agenda-place{color:var(--muted);font-size:.85em;overflow-wrap:anywhere}
.sa-agenda .past{opacity:.55}
.sa-agenda-none{margin:2px 0 0 11px}
`);
  const D = lib.dates;
  const today = app.today();
  const from = D.isDay(props.from) ? props.from : today;
  const n = Math.max(1, Math.min(366, Math.round(Number(props.days)) || 14));
  const to = D.addDays(from, n - 1);
  const days = D.range(from, to);
  const byDay = {};
  days.forEach((d) => { byDay[d] = []; });
  const now = Date.now();
  const selectable = typeof props.onSelect === 'function';

  // The calendar day of a value; an ISO end is exclusive, so 00:00 the next day stays on the day before.
  const dayOf = (v, isEnd) => {
    if (D.isDay(v)) return v;
    const t = Date.parse(v);
    if (isNaN(t)) return null;
    return D.fromTime(new Date(isEnd ? t - 1 : t).toISOString());
  };
  const shortDay = (d) => `${D.weekdayName(d, true)} ${Number(d.slice(8))} ${D.monthName(d, true)}`;

  for (const e of Array.isArray(props.events) ? props.events : []) {
    if (!e || e.start == null || e.start === '') continue;
    const start = String(e.start);
    const allDay = D.isDay(start);
    const sd = dayOf(start, false);
    if (!sd) continue;
    let ed = e.end ? dayOf(String(e.end), !D.isDay(String(e.end))) : sd;
    if (!ed || ed < sd) ed = sd;
    if (ed < from || sd > to) continue;
    const sMs = allDay ? 0 : Date.parse(start);
    const eRaw = !allDay && e.end && !D.isDay(String(e.end)) ? Date.parse(e.end) : NaN;
    const eMs = !isNaN(eRaw) && eRaw > sMs ? eRaw : null;
    const past = !allDay && (eMs != null ? eMs : sMs) < now;
    const first = sd < from ? from : sd;
    const last = ed > to ? to : ed;
    for (const d of D.range(first, last)) {
      let main, sub = null, rank, key = 0;
      if (allDay || (d > sd && d < ed)) { main = 'All day'; rank = 0; }
      else if (d === sd) {
        main = app.fmt.time(start);
        sub = ed === sd ? (eMs != null ? '– ' + app.fmt.time(e.end) : null) : '→ ' + D.weekdayName(ed, true);
        rank = 2; key = sMs;
      } else { main = 'Until'; sub = eMs != null ? app.fmt.time(e.end) : null; rank = 1; }
      byDay[d].push({ e, main, sub, rank, key, past });
    }
  }

  const root = h('div', { class: 'sa-agenda' });
  if (!props.showEmptyDays && !days.some((d) => byDay[d].length)) {
    root.append(h('div', { class: 'empty' }, props.empty || 'Nothing coming up'));
    return root;
  }

  const named = { [today]: 'Today', [D.addDays(today, 1)]: 'Tomorrow', [D.addDays(today, -1)]: 'Yesterday' };
  const yearOf = (d) => (d.slice(0, 4) !== today.slice(0, 4) ? ' ' + d.slice(0, 4) : '');
  const safeColor = (c) => String(c).replace(/[;{}<>"'\\]/g, '');

  for (const d of days) {
    const items = byDay[d];
    if (!items.length && !props.showEmptyDays) continue;
    items.sort((a, b) => a.rank - b.rank || a.key - b.key ||
      String(a.e.title || '').localeCompare(String(b.e.title || '')));
    const dateText = shortDay(d) + yearOf(d);
    const name = named[d] || dateText;
    const head = h('div', { class: 'sa-agenda-head', role: 'heading', 'aria-level': '3' },
      h('span', { class: 'sa-agenda-name' }, name),
      named[d] ? h('small', null, dateText) : null);
    const list = items.length
      ? h('ul', { class: 'sa-agenda-list' }, items.map((it) => {
          const title = it.e.title ? String(it.e.title) : 'Untitled';
          const place = it.e.place ? String(it.e.place) : '';
          const inner = [
            h('span', { class: 'sa-agenda-when' }, h('span', null, it.main), it.sub ? h('small', null, it.sub) : null),
            h('span', { class: 'sa-agenda-what' },
              h('span', { class: 'sa-agenda-title' }, title),
              place ? h('span', { class: 'sa-agenda-place' }, place) : null),
          ];
          const cls = 'sa-agenda-ev' + (it.past ? ' past' : '');
          const style = it.e.color ? `--c:${safeColor(it.e.color)}` : null;
          const label = [title, it.main + (it.sub ? ' ' + it.sub : ''), place].filter(Boolean).join(', ');
          return h('li', null, selectable
            ? h('button', { type: 'button', class: cls, style, 'aria-label': label, onclick: () => props.onSelect(it.e.id) }, inner)
            : h('div', { class: cls, style }, inner));
        }))
      : h('p', { class: 'sa-agenda-none muted small' }, 'Nothing planned');
    root.append(h('section', { class: 'sa-agenda-day' + (d === today ? ' today' : ''), 'aria-label': name === dateText ? name : `${name}, ${dateText}` }, head, list));
  }
  return root;
}

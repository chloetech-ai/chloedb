// title: Booking slots
// description: Pick a day and a free time to book: a strip of days, then the day's slots between opening and closing time, with taken, past and closed slots greyed out.
// props: onBook({day, time, start, end}) when a free slot is chosen (start and end are local 'YYYY-MM-DDTHH:MM'); taken: booked times, each a start
//   ('YYYY-MM-DDTHH:MM' or an ISO time) or {start, end}; from: the first day (default today); days: how many days to offer (default 14);
//   open (default '09:00') and close (default '17:00'); step: minutes between slot starts (default 30); length: a booking's minutes (default step);
//   closed: weekdays with no slots (0 Sunday .. 6 Saturday, default [0, 6]); notice: minutes ahead a booking must be (default 60);
//   day and onDay(day) to keep the chosen day; selected: a start to show as chosen
function component(props) {
  css('sa-book', `
.sa-book{display:flex;flex-direction:column;gap:10px}
.sa-book .days{display:flex;gap:6px;overflow-x:auto;padding-bottom:4px;scrollbar-width:thin}
.sa-book .days button{flex:none;display:flex;flex-direction:column;align-items:center;gap:1px;min-width:54px;padding:6px 8px;border-radius:10px}
.sa-book .days button small{font-size:11px;color:var(--muted)}
.sa-book .days button.on{background:var(--accent);color:var(--accent-fg,#fff);border-color:var(--accent)}
.sa-book .days button.on small{color:inherit;opacity:.85}
.sa-book .slots{display:grid;grid-template-columns:repeat(auto-fill,minmax(76px,1fr));gap:6px}
.sa-book .slots button{padding:7px 4px;font-variant-numeric:tabular-nums}
.sa-book .slots button.on{background:var(--accent);color:var(--accent-fg,#fff);border-color:var(--accent)}
.sa-book .slots button:disabled{text-decoration:line-through;opacity:.45}
`);
  const D = lib.dates;
  const pad = (n) => String(n).padStart(2, '0');
  const local = (d) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
  const norm = (v) => { const s = String(v || ''); if (/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/.test(s)) return s; const d = new Date(s); return Number.isNaN(d.getTime()) ? null : local(d); };
  const mins = (t) => { const [hh, mm] = String(t).split(':').map(Number); return hh * 60 + (mm || 0); };
  const toMs = (s) => new Date(`${s}:00`).getTime();
  const step = Number(props.step) || 30;
  const length = Number(props.length) || step;
  const closed = new Set(props.closed || [0, 6]);
  const notice = props.notice === undefined ? 60 : Number(props.notice);
  const busy = (props.taken || []).map((t) => {
    const start = norm(typeof t === 'object' && t ? t.start : t);
    if (!start) return null;
    const end = typeof t === 'object' && t && t.end ? norm(t.end) : null;
    const a = toMs(start);
    return [a, end ? toMs(end) : a + length * 60000];
  }).filter(Boolean);
  const first = props.from || D.today();
  const days = D.range(first, D.addDays(first, (Number(props.days) || 14) - 1)).filter((d) => !closed.has(D.weekday(d)));
  let day = props.day && days.includes(props.day) ? props.day : null;
  const root = h('div', { class: 'sa-book' });
  const slotsOf = (d) => {
    const out = [];
    for (let m = mins(props.open || '09:00'); m + length <= mins(props.close || '17:00'); m += step) {
      const time = `${pad(Math.floor(m / 60))}:${pad(m % 60)}`;
      const start = `${d}T${time}`;
      const a = toMs(start); const b = a + length * 60000;
      const free = a >= Date.now() + notice * 60000 && !busy.some(([x, y]) => a < y && b > x);
      out.push({ day: d, time, start, end: local(new Date(b)), free });
    }
    return out;
  };
  if (!day) day = days.find((d) => slotsOf(d).some((s) => s.free)) || days[0];
  function draw() {
    const slots = day ? slotsOf(day) : [];
    root.replaceChildren(
      h('div', { class: 'days' }, days.map((d) => h('button', { type: 'button', class: d === day ? 'on' : null, onclick: () => { day = d; if (props.onDay) props.onDay(d); draw(); } },
        h('small', null, D.weekdayName(d, true)), h('b', null, Number(d.slice(8))), h('small', null, D.monthName(d, true))))),
      slots.some((s) => s.free) || slots.length ? h('div', { class: 'slots' }, slots.map((s) => h('button', { type: 'button', disabled: !s.free, class: props.selected && norm(props.selected) === s.start ? 'on' : null,
        title: s.free ? `Book ${s.time}` : 'Not available', onclick: () => props.onBook && props.onBook({ day: s.day, time: s.time, start: s.start, end: s.end }) }, s.time))) : null,
      !slots.some((s) => s.free) ? h('div', { class: 'small muted' }, days.length ? 'No free times this day.' : 'No days open for booking.') : null);
  }
  draw();
  return root;
}

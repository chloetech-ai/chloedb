// title: Opening hours
// description: A week's opening hours with whether the place is open now and when it next opens or closes ("Open now · Closes at 17:00", "Closed · Opens tomorrow at 9:00"), with special days and today highlighted; for shops, clinics, gyms, offices and booking pages.
// props: hours: {mon, tue, wed, thu, fri, sat, sun}, each a list of ranges [['09:00','17:00'], ['18:00','22:00']] ('HH:MM', '24:00' allowed;
//   a close earlier than the open runs past midnight, ['00:00','24:00'] is open all day; a missing or empty day is closed);
//   now: ISO time to judge "open now" by (default app.now()); exceptions: [{date: 'YYYY-MM-DD', ranges: [[open, close]] ([] or none = closed), note?}]
//   for holidays and special days, used instead of the weekly hours on that date;
//   compact: true for one line with the status and today's hours only (default false); highlightToday: false to not mark today's row (default true);
//   label: accessible name (default "Opening hours")
function component(props) {
  css('sa-hours', `
.sa-hours{display:flex;flex-direction:column;gap:8px;min-width:0}
.sa-hours .sa-hours-st{display:flex;flex-wrap:wrap;gap:6px;align-items:center}
.sa-hours table{width:100%;border-collapse:collapse;margin:0}
.sa-hours th,.sa-hours td{padding:6px 8px;border-bottom:1px solid var(--line);vertical-align:top;text-align:left;font-weight:normal}
.sa-hours tr:last-child th,.sa-hours tr:last-child td{border-bottom:0}
.sa-hours td{text-align:right;font-variant-numeric:tabular-nums}
.sa-hours tr.today th,.sa-hours tr.today td{font-weight:600;background:color-mix(in srgb,var(--accent) 10%,transparent)}
.sa-hours .closed{color:var(--muted)}
.sa-hours .note{display:block;font-size:.8em;color:var(--warn);font-weight:normal}
.sa-hours.compact{flex-direction:row;flex-wrap:wrap;align-items:center;gap:6px}
`);
  const KEYS = ['sun', 'mon', 'tue', 'wed', 'thu', 'fri', 'sat'];
  const ORDER = [1, 2, 3, 4, 5, 6, 0];
  const hours = props.hours && typeof props.hours === 'object' ? props.hours : {};
  const highlight = props.highlightToday !== false;

  let at = new Date(props.now || app.now());
  if (isNaN(at.getTime())) at = new Date();
  const today = lib.dates.fromTime(at.toISOString());
  const nowMin = at.getHours() * 60 + at.getMinutes();

  const exc = {};
  for (const e of Array.isArray(props.exceptions) ? props.exceptions : []) {
    if (e && lib.dates.isDay(e.date)) exc[e.date] = e;
  }

  const toMin = (t) => {
    const m = /^(\d{1,2}):(\d{2})$/.exec(String(t || '').trim());
    if (!m) return null;
    const hh = +m[1], mm = +m[2];
    if (mm > 59 || hh > 24 || (hh === 24 && mm > 0)) return null;
    return hh * 60 + mm;
  };
  const clean = (list) => {
    if (!Array.isArray(list)) return [];
    if (list.length === 2 && typeof list[0] === 'string') list = [list];
    const out = [];
    for (const r of list) {
      if (!Array.isArray(r)) continue;
      const a = toMin(r[0]), b = toMin(r[1]);
      if (a === null || b === null || a === 1440) continue;
      out.push({ a, b: b <= a ? b + 1440 : b });
    }
    return out.sort((x, y) => x.a - y.a);
  };
  const rangesFor = (day) => exc[day] ? clean(exc[day].ranges || []) : clean(hours[KEYS[lib.dates.weekday(day)]]);

  // Every opening from yesterday to a week ahead, in minutes from today's midnight, merged where they touch.
  const raw = [];
  for (let d = -1; d <= 7; d++) {
    for (const r of rangesFor(lib.dates.addDays(today, d))) raw.push([d * 1440 + r.a, d * 1440 + r.b]);
  }
  raw.sort((x, y) => x[0] - y[0]);
  const spans = [];
  for (const s of raw) {
    const last = spans[spans.length - 1];
    if (last && s[0] <= last[1]) last[1] = Math.max(last[1], s[1]);
    else spans.push(s.slice());
  }

  const clock = (m) => `${Math.floor(m / 60)}:${String(m % 60).padStart(2, '0')}`;
  const spoken = (m) => {
    const x = ((m % 1440) + 1440) % 1440;
    return x === 0 ? 'midnight' : clock(x);
  };
  const dayWord = (m, from) => {
    const off = Math.floor((m - (m % 1440 === 0 && m > from ? 1 : 0)) / 1440);
    if (m % 1440 === 0 && m > from) return off === 0 ? '' : off === 1 ? 'tomorrow ' : `${lib.dates.weekdayName(lib.dates.addDays(today, off), true)} `;
    return off === 0 ? '' : off === 1 ? 'tomorrow ' : `${lib.dates.weekdayName(lib.dates.addDays(today, off), true)} `;
  };

  const current = spans.find((s) => s[0] <= nowMin && nowMin < s[1]);
  let open = false, text = '';
  if (current) {
    open = true;
    if (current[1] < 8 * 1440) {
      text = current[1] - nowMin < 1440 ? `Closes at ${spoken(current[1])}` : `Closes ${dayWord(current[1], nowMin)}at ${spoken(current[1])}`;
    }
  } else {
    const next = spans.find((s) => s[0] > nowMin);
    if (next) {
      const off = Math.floor(next[0] / 1440);
      const when = off === 0 ? '' : off === 1 ? 'tomorrow ' : `${lib.dates.weekdayName(lib.dates.addDays(today, off), true)} `;
      text = `Opens ${when}at ${spoken(next[0])}`;
    }
  }

  const rangeText = (list) => {
    if (!list.length) return 'Closed';
    if (list.length === 1 && list[0].a === 0 && list[0].b >= 1440) return 'Open 24 hours';
    return list.map((r) => `${clock(r.a)}–${clock(r.b >= 1440 ? (r.b === 1440 ? 1440 : r.b - 1440) : r.b)}`).join(', ');
  };

  const status = h('div', { class: 'sa-hours-st' },
    h('span', { class: `badge ${open ? 'ok' : 'bad'}` }, open ? 'Open now' : 'Closed'),
    text ? h('span', null, text) : null);

  const label = props.label || 'Opening hours';

  if (props.compact) {
    const list = rangesFor(today);
    const note = exc[today] && exc[today].note;
    return h('div', { class: 'sa-hours compact', role: 'group', 'aria-label': label },
      status,
      h('span', { class: list.length ? 'muted' : 'closed' }, `· Today ${rangeText(list)}`),
      note ? h('span', { class: 'note' }, note) : null);
  }

  const wdToday = lib.dates.weekday(today);
  const rows = ORDER.map((wd) => {
    const off = (wd - wdToday + 7) % 7;
    const date = lib.dates.addDays(today, off);
    const ex = exc[date];
    const list = rangesFor(date);
    const isToday = off === 0;
    return h('tr', { class: isToday && highlight ? 'today' : null, 'aria-current': isToday ? 'date' : null },
      h('th', { scope: 'row' }, lib.dates.weekdayName(wd), isToday ? h('span', { class: 'muted small' }, ' (today)') : null),
      h('td', { class: list.length ? null : 'closed' },
        rangeText(list),
        ex ? h('span', { class: 'note' }, ex.note || 'Special hours') : null));
  });

  return h('div', { class: 'sa-hours', role: 'group', 'aria-label': label },
    status,
    h('table', { 'aria-label': label }, h('tbody', null, rows)));
}

// title: Dates
// description: Calendar-day arithmetic on "YYYY-MM-DD" text: today, adding days and months, differences, weeks, months, ranges and relative labels. Days never shift with time zones.
// api: today() -> 'YYYY-MM-DD' where the code runs;
//   fromTime(iso) -> the local day of a timestamp;
//   addDays(day, n), addMonths(day, n) (the 31st becomes the month's last day) -> day;
//   diffDays(a, b) -> whole days from a to b (b later is positive);
//   weekday(day) -> 0 Sunday .. 6 Saturday; weekdayName(day or 0-6, short = false), monthName(day or 'YYYY-MM' or 1-12, short = false);
//   startOfWeek(day, weekStart = 1 Monday), endOfWeek(day, weekStart = 1), startOfMonth(day), endOfMonth(day) -> day;
//   month(day) -> 'YYYY-MM'; daysInMonth('YYYY-MM') -> 28..31; isoWeek(day) -> 1..53;
//   range(from, to) -> every day from..to inclusive (at most 3700);
//   monthGrid('YYYY-MM', weekStart = 1) -> array of weeks, each 7 days ('YYYY-MM-DD' or null outside the month);
//   relative(day, today = today()) -> 'today', 'tomorrow', 'yesterday', 'in 3 days', '2 weeks ago', ...;
//   isDay(text) -> true for valid 'YYYY-MM-DD'
function helper() {
  const pad = (n) => String(n).padStart(2, '0');
  const DAY = 86400000;
  const isDay = (s) => typeof s === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(s) && !Number.isNaN(Date.parse(s + 'T00:00:00Z'));
  const toUTC = (day) => {
    if (!isDay(day)) throw new Error(`not a day ("YYYY-MM-DD"): ${JSON.stringify(day)}`);
    return Date.UTC(Number(day.slice(0, 4)), Number(day.slice(5, 7)) - 1, Number(day.slice(8, 10)));
  };
  const fromUTC = (ms) => { const d = new Date(ms); return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`; };
  const WEEKDAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
  const MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];

  function today() { const d = new Date(); return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`; }
  function fromTime(iso) { const d = new Date(iso); if (Number.isNaN(d.getTime())) throw new Error(`not a time: ${JSON.stringify(iso)}`); return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`; }
  function addDays(day, n) { return fromUTC(toUTC(day) + Math.round(n) * DAY); }
  function daysInMonth(ym) { const y = Number(ym.slice(0, 4)); const m = Number(ym.slice(5, 7)); return new Date(Date.UTC(y, m, 0)).getUTCDate(); }
  function addMonths(day, n) {
    const y = Number(day.slice(0, 4)); const m = Number(day.slice(5, 7)) - 1 + Math.round(n);
    const ty = y + Math.floor(m / 12); const tm = ((m % 12) + 12) % 12;
    const ym = `${ty}-${pad(tm + 1)}`;
    return `${ym}-${pad(Math.min(Number(day.slice(8, 10)), daysInMonth(ym)))}`;
  }
  function diffDays(a, b) { return Math.round((toUTC(b) - toUTC(a)) / DAY); }
  function weekday(day) { return new Date(toUTC(day)).getUTCDay(); }
  function startOfWeek(day, weekStart = 1) { return addDays(day, -((weekday(day) - weekStart + 7) % 7)); }
  function endOfWeek(day, weekStart = 1) { return addDays(startOfWeek(day, weekStart), 6); }
  function month(day) { return day.slice(0, 7); }
  function startOfMonth(day) { return `${day.slice(0, 7)}-01`; }
  function endOfMonth(day) { return `${day.slice(0, 7)}-${pad(daysInMonth(day))}`; }
  function isoWeek(day) {
    const d = new Date(toUTC(day));
    const dow = d.getUTCDay() || 7;
    d.setUTCDate(d.getUTCDate() + 4 - dow);
    const yearStart = Date.UTC(d.getUTCFullYear(), 0, 1);
    return Math.ceil(((d.getTime() - yearStart) / DAY + 1) / 7);
  }
  function range(from, to) {
    const n = diffDays(from, to);
    if (n < 0) return [];
    if (n > 3700) throw new Error('range: more than 3700 days');
    const out = [];
    for (let i = 0; i <= n; i++) out.push(addDays(from, i));
    return out;
  }
  function monthGrid(ym, weekStart = 1) {
    const first = `${ym.slice(0, 7)}-01`;
    const lead = (weekday(first) - weekStart + 7) % 7;
    const n = daysInMonth(first);
    const cells = [];
    for (let i = 0; i < lead; i++) cells.push(null);
    for (let d = 1; d <= n; d++) cells.push(`${ym.slice(0, 7)}-${pad(d)}`);
    while (cells.length % 7) cells.push(null);
    const weeks = [];
    for (let i = 0; i < cells.length; i += 7) weeks.push(cells.slice(i, i + 7));
    return weeks;
  }
  function weekdayName(x, short = false) { const i = typeof x === 'number' ? x : weekday(x); const n = WEEKDAYS[((i % 7) + 7) % 7]; return short ? n.slice(0, 3) : n; }
  function monthName(x, short = false) {
    const i = typeof x === 'number' ? x - 1 : Number(String(x).slice(5, 7)) - 1;
    const n = MONTHS[i];
    if (!n) throw new Error(`monthName: no month in ${JSON.stringify(x)}`);
    return short ? n.slice(0, 3) : n;
  }
  function relative(day, now = today()) {
    const n = diffDays(now, day);
    if (n === 0) return 'today';
    if (n === 1) return 'tomorrow';
    if (n === -1) return 'yesterday';
    const a = Math.abs(n);
    const unit = a < 14 ? [a, 'day'] : a < 60 ? [Math.round(a / 7), 'week'] : a < 730 ? [Math.round(a / 30.44), 'month'] : [Math.round(a / 365.25), 'year'];
    const text = `${unit[0]} ${unit[1]}${unit[0] === 1 ? '' : 's'}`;
    return n > 0 ? `in ${text}` : `${text} ago`;
  }
  return { today, fromTime, addDays, addMonths, diffDays, weekday, weekdayName, monthName, startOfWeek, endOfWeek, startOfMonth, endOfMonth, month, daysInMonth, isoWeek, range, monthGrid, relative, isDay };
}

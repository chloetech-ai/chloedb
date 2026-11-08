// title: Deadlines
// description: Due dates and business days: whether something is overdue, due today, due soon or later, how many days are left, a plain label, and adding or counting working days around weekends and holidays (tasks, bills, invoices, assignments).
// api: status(due, {today, soonDays = 3, done}) -> 'done' | 'overdue' | 'today' | 'soon' (due within soonDays) | 'later', or null when due is missing or not a date;
//   due is 'YYYY-MM-DD' or a full ISO time (taken as its local day); today: 'YYYY-MM-DD', default the local date; done: true gives 'done' whatever the date;
//   daysLeft(due, {today}) -> whole days from today to due (0 today, negative when overdue), or null;
//   label(due, {today, done}) -> 'done', 'due today', 'due tomorrow', 'due in 3 days', 'overdue by 1 day', 'overdue by 4 days' ('' when due is not a date);
//   business days take opts {weekend = [0, 6] (0 Sunday .. 6 Saturday, or names 'sat', 'sun'), holidays = ['YYYY-MM-DD', ...]}:
//   isBusinessDay(day, opts) -> bool (null when day is not a date);
//   addBusinessDays(day, n, opts) -> the day n business days later (n negative goes back; n = 0 gives day itself), or null;
//   nextBusinessDay(day, opts) -> the first business day strictly after day, or null;
//   adjust(day, opts) -> day itself when it is a business day, else the next one (a due date on a Saturday moves to Monday), or null;
//   businessDaysBetween(a, b, opts) -> business days after a up to and including b (negative when b is before a), or null;
//   with no business day in the week (every weekday a weekend) the business-day functions return null
function helper() {
  const DAY = 86400000;
  const LIMIT = 40000;
  const pad = (n) => String(n).padStart(2, '0');
  const toUTC = (d) => Date.UTC(Number(d.slice(0, 4)), Number(d.slice(5, 7)) - 1, Number(d.slice(8, 10)));
  const fromUTC = (ms) => { const d = new Date(ms); return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`; };
  const local = (d) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  const isDay = (s) => typeof s === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(s) && fromUTC(toUTC(s)) === s;
  const NAMES = { sun: 0, mon: 1, tue: 2, wed: 3, thu: 4, fri: 5, sat: 6 };

  function toDay(v) {
    if (v instanceof Date) return isNaN(v.getTime()) ? null : local(v);
    if (typeof v !== 'string') return null;
    const s = v.trim();
    if (s.length === 10) return isDay(s) ? s : null;
    if (/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}/.test(s)) {
      const d = new Date(s);
      return isNaN(d.getTime()) ? null : local(d);
    }
    return null;
  }
  const todayOf = (opts) => toDay(opts && opts.today) || local(new Date());

  function daysLeft(due, opts = {}) {
    const d = toDay(due);
    if (!d) return null;
    return Math.round((toUTC(d) - toUTC(todayOf(opts || {}))) / DAY);
  }
  function status(due, opts = {}) {
    const o = opts || {};
    if (o.done) return 'done';
    const n = daysLeft(due, o);
    if (n === null) return null;
    const soon = typeof o.soonDays === 'number' && isFinite(o.soonDays) ? o.soonDays : 3;
    if (n < 0) return 'overdue';
    if (n === 0) return 'today';
    if (n <= soon) return 'soon';
    return 'later';
  }
  function label(due, opts = {}) {
    const o = opts || {};
    if (o.done) return 'done';
    const n = daysLeft(due, o);
    if (n === null) return '';
    if (n === 0) return 'due today';
    if (n === 1) return 'due tomorrow';
    if (n > 1) return `due in ${n} days`;
    return n === -1 ? 'overdue by 1 day' : `overdue by ${-n} days`;
  }

  function calendar(opts) {
    const o = opts || {};
    const list = Array.isArray(o.weekend) ? o.weekend : [0, 6];
    const weekend = new Set();
    for (const w of list) {
      const n = typeof w === 'number' ? w : NAMES[String(w).slice(0, 3).toLowerCase()];
      if (Number.isInteger(n) && n >= 0 && n <= 6) weekend.add(n);
    }
    const holidays = new Set((Array.isArray(o.holidays) ? o.holidays : []).map(toDay).filter(Boolean));
    const ok = (ms) => !weekend.has(new Date(ms).getUTCDay()) && !holidays.has(fromUTC(ms));
    return { ok, none: weekend.size >= 7 };
  }

  function isBusinessDay(day, opts) {
    const d = toDay(day);
    if (!d) return null;
    return calendar(opts).ok(toUTC(d));
  }
  function addBusinessDays(day, n, opts) {
    const d = toDay(day);
    if (!d || typeof n !== 'number' || !isFinite(n)) return null;
    const cal = calendar(opts);
    if (cal.none) return null;
    let left = Math.abs(Math.trunc(n));
    const step = n < 0 ? -DAY : DAY;
    let ms = toUTC(d);
    for (let guard = 0; left > 0; guard++) {
      if (guard > LIMIT) return null;
      ms += step;
      if (cal.ok(ms)) left--;
    }
    return fromUTC(ms);
  }
  function nextBusinessDay(day, opts) { return addBusinessDays(day, 1, opts); }
  function adjust(day, opts) {
    const d = toDay(day);
    if (!d) return null;
    const cal = calendar(opts);
    if (cal.none) return null;
    return cal.ok(toUTC(d)) ? d : addBusinessDays(d, 1, opts);
  }
  function businessDaysBetween(a, b, opts) {
    const x = toDay(a);
    const y = toDay(b);
    if (!x || !y) return null;
    const cal = calendar(opts);
    if (cal.none) return null;
    let from = toUTC(x);
    let to = toUTC(y);
    const sign = to < from ? -1 : 1;
    if (sign < 0) { const t = from; from = to; to = t; }
    if ((to - from) / DAY > LIMIT) return null;
    let n = 0;
    for (let ms = from + DAY; ms <= to; ms += DAY) if (cal.ok(ms)) n++;
    return sign * n;
  }

  return { status, daysLeft, label, isBusinessDay, addBusinessDays, nextBusinessDay, adjust, businessDaysBetween };
}

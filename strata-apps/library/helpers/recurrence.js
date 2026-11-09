// title: Repeating schedules
// description: Rules for things that repeat (chores, bills, reminders, classes, medication): the next date, the dates in a range, whether a day is due, and a plain description.
// api: a rule is {every: 'day' | 'week' | 'month' | 'year', interval = 1, start: 'YYYY-MM-DD', weekdays: [0-6 or 'mon'..] (weeks; default start's weekday), day: day of month (months; default start's, -1 = the last day), until: 'YYYY-MM-DD' (optional), count: n (optional)};
//   next(rule, after) -> the first date strictly after 'after' (default: the day before start), or null when the rule has ended;
//   between(rule, from, to, limit = 500) -> dates from..to inclusive;
//   isDue(rule, day) -> true when the rule falls on day;
//   describe(rule) -> 'Every 2 weeks on Mon and Thu', 'Every month on the last day', ...;
//   check(rule) -> a list of problems with the rule ([] when it is fine)
function helper() {
  const DAY = 86400000;
  const pad = (n) => String(n).padStart(2, '0');
  const toUTC = (d) => Date.UTC(Number(d.slice(0, 4)), Number(d.slice(5, 7)) - 1, Number(d.slice(8, 10)));
  const fromUTC = (ms) => { const d = new Date(ms); return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`; };
  const isDay = (s) => typeof s === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(s);
  const NAMES = { sun: 0, mon: 1, tue: 2, wed: 3, thu: 4, fri: 5, sat: 6 };
  const SHORT = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
  const dim = (y, m) => new Date(Date.UTC(y, m + 1, 0)).getUTCDate();
  const ord = (n) => `${n}${n % 100 >= 11 && n % 100 <= 13 ? 'th' : ['th', 'st', 'nd', 'rd'][n % 10] || 'th'}`;

  function norm(rule) {
    const problems = check(rule);
    if (problems.length) throw new Error(`recurrence: ${problems.join('; ')}`);
    const start = rule.start;
    const every = rule.every;
    const interval = Math.max(1, Math.round(Number(rule.interval) || 1));
    const weekdays = (rule.weekdays && rule.weekdays.length ? rule.weekdays : [new Date(toUTC(start)).getUTCDay()])
      .map((w) => (typeof w === 'number' ? w : NAMES[String(w).slice(0, 3).toLowerCase()])).sort();
    const day = rule.day === undefined || rule.day === null ? Number(start.slice(8, 10)) : Number(rule.day);
    return { start, every, interval, weekdays, day, until: rule.until || null, count: rule.count ? Number(rule.count) : null };
  }
  function check(rule) {
    const p = [];
    if (!rule || typeof rule !== 'object') return ['a rule is an object'];
    if (!['day', 'week', 'month', 'year'].includes(rule.every)) p.push('every is day, week, month or year');
    if (!isDay(rule.start)) p.push('start is a day, "YYYY-MM-DD"');
    if (rule.until && !isDay(rule.until)) p.push('until is a day, "YYYY-MM-DD"');
    if (rule.weekdays && rule.weekdays.some((w) => (typeof w === 'number' ? w < 0 || w > 6 : NAMES[String(w).slice(0, 3).toLowerCase()] === undefined))) p.push('weekdays are 0-6 or mon..sun');
    if (rule.day !== undefined && rule.day !== null && !(Number(rule.day) === -1 || (Number(rule.day) >= 1 && Number(rule.day) <= 31))) p.push('day is 1-31 or -1 for the last day');
    return p;
  }
  // does the rule (ignoring until and count) fall on ms?
  function falls(r, ms) {
    const s = toUTC(r.start);
    if (ms < s) return false;
    const d = new Date(ms);
    if (r.every === 'day') return Math.round((ms - s) / DAY) % r.interval === 0;
    if (r.every === 'week') {
      const weekOf = (x) => x - ((new Date(x).getUTCDay() + 6) % 7) * DAY; // Monday of x's week
      const weeks = Math.round((weekOf(ms) - weekOf(s)) / (7 * DAY));
      return weeks % r.interval === 0 && r.weekdays.includes(d.getUTCDay());
    }
    const sd = new Date(s);
    const months = (d.getUTCFullYear() - sd.getUTCFullYear()) * 12 + d.getUTCMonth() - sd.getUTCMonth();
    const last = dim(d.getUTCFullYear(), d.getUTCMonth());
    const want = r.day === -1 ? last : Math.min(r.day, last);
    if (r.every === 'month') return months % r.interval === 0 && d.getUTCDate() === want;
    // year: the start's month and day (29 Feb falls on 28 Feb in other years)
    const wantYear = Math.min(sd.getUTCDate(), last);
    return d.getUTCMonth() === sd.getUTCMonth() && (months / 12) % r.interval === 0 && d.getUTCDate() === wantYear;
  }
  function* walk(r, fromMs) {
    // count needs every occurrence from the start
    let ms = r.count ? toUTC(r.start) : Math.max(fromMs, toUTC(r.start));
    let seen = 0;
    const end = r.until ? toUTC(r.until) : Infinity;
    for (let guard = 0; guard < 40000 && ms <= end; guard++, ms += DAY) {
      if (!falls(r, ms)) continue;
      seen++;
      if (r.count && seen > r.count) return;
      if (ms >= fromMs) yield ms;
    }
  }
  function next(rule, after) {
    const r = norm(rule);
    const from = after ? toUTC(after) + DAY : toUTC(r.start);
    for (const ms of walk(r, from)) return fromUTC(ms);
    return null;
  }
  function between(rule, from, to, limit = 500) {
    const r = norm(rule);
    const out = [];
    const last = toUTC(to);
    for (const ms of walk(r, toUTC(from))) {
      if (ms > last || out.length >= limit) break;
      out.push(fromUTC(ms));
    }
    return out;
  }
  function isDue(rule, day) { return between(rule, day, day).length === 1; }
  function describe(rule) {
    const r = norm(rule);
    const n = r.interval;
    const unit = n === 1 ? (r.every === 'day' ? 'Every day' : `Every ${r.every}`) : `Every ${n} ${r.every}s`;
    let text = unit;
    if (r.every === 'week') {
      const names = r.weekdays.map((w) => SHORT[w]);
      text += ` on ${names.length > 1 ? `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}` : names[0]}`;
    } else if (r.every === 'month') {
      text += r.day === -1 ? ' on the last day' : ` on the ${ord(r.day)}`;
    } else if (r.every === 'year') {
      const d = new Date(toUTC(r.start));
      text += ` on ${d.getUTCDate()} ${['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'][d.getUTCMonth()]}`;
    }
    if (r.until) text += `, until ${r.until}`;
    if (r.count) text += `, ${r.count} times`;
    return text;
  }
  return { next, between, isDue, describe, check };
}

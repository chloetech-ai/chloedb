// title: Daily digest email
// description: Emails a plain-text list of the rows of any table that fall due today or within the next few days, and sends nothing when none do.
// input: {table, titleColumn, dateColumn (a 'YYYY-MM-DD' keyword or a timestamp), to: an email or [emails] (at most 10), days = 0 (0 today only, 7 today and the week ahead), subject? (default 'Due today (3)')} -> {sent: bool, count: rows due, status: the email's status ('held' when queued) or 'none' when nothing was due}, or {error}
function run(input, app) {
  input = input || {};
  const NAME = /^[a-z][a-z0-9_]*$/;
  const table = input.table, tc = input.titleColumn, dc = input.dateColumn;
  for (const [what, name] of [['table', table], ['titleColumn', tc], ['dateColumn', dc]]) {
    if (typeof name !== 'string' || !NAME.test(name)) {
      return { error: what + ' must be a name of lowercase letters, digits and _' };
    }
  }

  const days = input.days == null || input.days === '' ? 0 : Number(input.days);
  if (!Number.isInteger(days) || days < 0 || days > 366) {
    return { error: 'days must be a whole number from 0 to 366' };
  }

  const list = Array.isArray(input.to) ? input.to : String(input.to || '').split(/[,;]/);
  const to = [];
  for (const raw of list) {
    const e = String(raw || '').trim().toLowerCase();
    if (e && to.indexOf(e) < 0) to.push(e);
  }
  if (!to.length) return { error: 'to needs at least one email address' };
  if (to.length > 10) return { error: 'to may hold at most 10 addresses' };
  const bad = to.filter((e) => !lib.validate.isEmail(e));
  if (bad.length) return { error: 'Not an email address: ' + bad.join(', ') };

  const today = lib.dates.fromTime(app.now());
  const last = lib.dates.addDays(today, days);

  // Checks that the table and both columns exist, and tells a day keyword from a timestamp.
  let sample;
  try {
    sample = app.sql(`SELECT ${tc} AS t, ${dc} AS d FROM ${table} WHERE ${dc} IS NOT NULL LIMIT 1`);
  } catch (e) {
    return { error: `Cannot read ${tc} and ${dc} from ${table}: ${(e && e.message) || e}` };
  }
  if (!sample.length) return { sent: false, count: 0, status: 'none' };
  const isDay = typeof sample[0].d === 'string' && lib.dates.isDay(sample[0].d);

  const midnight = (day) => {
    const [y, m, d] = day.split('-').map(Number);
    return new Date(y, m - 1, d).toISOString().replace('.000Z', 'Z');
  };
  const params = {};
  let where;
  if (isDay) {
    const window = lib.dates.range(today, last);
    window.forEach((d, i) => { params['d' + i] = d; });
    where = `${dc} IN (${window.map((d, i) => ':d' + i).join(', ')})`;
  } else {
    params.from = midnight(today);
    params.to = midnight(lib.dates.addDays(last, 1));
    where = `${dc} >= :from AND ${dc} < :to`;
  }

  let rows, count;
  try {
    rows = app.sql(`SELECT ${tc} AS t, ${dc} AS d FROM ${table} WHERE ${where} ORDER BY ${dc} ASC LIMIT 500`, params);
    count = Number(app.sql(`SELECT COUNT(*) AS n FROM ${table} WHERE ${where}`, params)[0].n) || 0;
  } catch (e) {
    return { error: `${dc} must be a timestamp or a 'YYYY-MM-DD' keyword: ${(e && e.message) || e}` };
  }
  if (!rows.length) return { sent: false, count: 0, status: 'none' };

  const pad = (n) => String(n).padStart(2, '0');
  const dayLabel = (day) => {
    const base = `${lib.dates.weekdayName(day, true)} ${Number(day.slice(8))} ${lib.dates.monthName(day, true)}`;
    const rel = lib.dates.diffDays(today, day);
    return rel === 0 ? base + ' (today)' : rel === 1 ? base + ' (tomorrow)' : base;
  };

  const lines = [];
  let current = null;
  for (const r of rows) {
    const day = isDay ? r.d : lib.dates.fromTime(r.d);
    if (days > 0 && day !== current) {
      if (current !== null) lines.push('');
      lines.push(dayLabel(day));
      current = day;
    }
    let title = String(r.t == null ? '' : r.t).replace(/\s+/g, ' ').trim() || '(untitled)';
    title = lib.text.truncate(title, 200);
    let time = '';
    if (!isDay) {
      const t = new Date(r.d);
      if (!isNaN(t)) time = `${pad(t.getHours())}:${pad(t.getMinutes())}  `;
    }
    lines.push(`- ${time}${title}`);
  }
  if (count > rows.length) lines.push('', `...and ${count - rows.length} more.`);

  const what = lib.text.plural(count, 'item');
  const span = days === 0 ? 'today' : days === 1 ? 'today and tomorrow' : `in the next ${days} days`;
  const intro = days === 0 ? `${what} due today, ${dayLabel(today).replace(' (today)', '')}:` : `${what} due ${span}:`;
  const subject = (typeof input.subject === 'string' && input.subject.trim())
    || `Due ${span} (${count})`;

  let res;
  try {
    res = app.email({ to, subject, text: intro + '\n\n' + lines.join('\n') + '\n' });
  } catch (e) {
    return { error: 'Could not send the email: ' + ((e && e.message) || e), count };
  }
  return { sent: true, count, status: (res && res.status) || 'queued' };
}

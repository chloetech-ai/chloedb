// title: Streaks
// description: Streaks and hit rates over the days something was done (habits, workouts, practice, journaling), optionally only counting scheduled weekdays.
// api: current(days, {today, weekdays}) -> the run of done days ending today, or ending yesterday when today is not done yet (it is still alive);
//   best(days, {weekdays}) -> the longest run; runs(days, {weekdays}) -> [{start, end, length}] oldest first;
//   rate(days, {from, to, weekdays}) -> {due, done, rate} where rate is done/due from 0 to 1 (null when nothing was due);
//   days: an array of 'YYYY-MM-DD' (duplicates and order do not matter); today: 'YYYY-MM-DD', default the local date;
//   weekdays: the days that count, as numbers 0 Sunday .. 6 Saturday or names ('mon', 'tue', ...); days not scheduled neither break nor extend a run
function helper() {
  const DAY = 86400000;
  const pad = (n) => String(n).padStart(2, '0');
  const toUTC = (d) => Date.UTC(Number(d.slice(0, 4)), Number(d.slice(5, 7)) - 1, Number(d.slice(8, 10)));
  const fromUTC = (ms) => { const d = new Date(ms); return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`; };
  const localToday = () => { const d = new Date(); return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`; };
  const NAMES = { sun: 0, mon: 1, tue: 2, wed: 3, thu: 4, fri: 5, sat: 6 };
  function schedule(weekdays) {
    if (!Array.isArray(weekdays) || !weekdays.length) return () => true;
    const set = new Set(weekdays.map((w) => (typeof w === 'number' ? w : NAMES[String(w).slice(0, 3).toLowerCase()])));
    return (ms) => set.has(new Date(ms).getUTCDay());
  }
  const clean = (days) => new Set((days || []).filter((d) => typeof d === 'string' && /^\d{4}-\d{2}-\d{2}/.test(d)).map((d) => d.slice(0, 10)));

  function current(days, { today, weekdays } = {}) {
    const done = clean(days);
    const due = schedule(weekdays);
    let ms = toUTC(today || localToday());
    // today not done yet: the streak is still alive, count from yesterday
    if (!done.has(fromUTC(ms))) ms -= DAY;
    let n = 0;
    for (let guard = 0; guard < 4000; guard++, ms -= DAY) {
      const d = fromUTC(ms);
      if (done.has(d)) { n++; continue; }
      if (!due(ms)) continue;
      break;
    }
    return n;
  }
  function runs(days, { weekdays } = {}) {
    const done = [...clean(days)].sort();
    if (!done.length) return [];
    const due = schedule(weekdays);
    const out = [];
    let start = done[0];
    let prev = done[0];
    let length = 1;
    for (const d of done.slice(1)) {
      let broken = false;
      for (let ms = toUTC(prev) + DAY; ms < toUTC(d); ms += DAY) if (due(ms)) { broken = true; break; }
      if (broken) { out.push({ start, end: prev, length }); start = d; length = 0; }
      length++;
      prev = d;
    }
    out.push({ start, end: prev, length });
    return out;
  }
  function best(days, opts = {}) { return runs(days, opts).reduce((m, r) => Math.max(m, r.length), 0); }
  function rate(days, { from, to, weekdays } = {}) {
    const done = clean(days);
    const due = schedule(weekdays);
    const end = to || localToday();
    const begin = from || [...done].sort()[0] || end;
    let n = 0;
    let hit = 0;
    for (let ms = toUTC(begin), last = toUTC(end), guard = 0; ms <= last && guard < 4000; ms += DAY, guard++) {
      if (!due(ms)) continue;
      n++;
      if (done.has(fromUTC(ms))) hit++;
    }
    return { due: n, done: hit, rate: n ? hit / n : null };
  }
  return { current, best, runs, rate };
}

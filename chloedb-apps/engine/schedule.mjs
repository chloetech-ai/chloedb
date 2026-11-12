// Schedules: a rule or function that runs on its own, on a clock.
//
//   CREATE RULE morning EVERY DAY AT '07:00' TZ 'Europe/Madrid' AS SEND EMAIL ...
//   CREATE SOURCE news FROM FEED 'https://example.com/rss' EVERY 1 HOUR INTO stories ...
//   a JavaScript function's plan entry: "every": "1 hour" (or "day at 07:00")
//
// A schedule runs with nobody watching, so it is where spending piles up
// unseen: it runs only once the app's owner has switched "Schedules" on (in
// reach.mjs, with the app's other switches), never more often than every 15
// minutes, its model calls and fetches count against the app's daily limits,
// and it stops itself after 5 failures in a row until the owner starts it
// again. A run missed while the engine was down runs once when it is back,
// not once per missed time. Each run is an event in the app's Activity, by
// "schedule", and its state (next, last, result) is kept in sa_settings.
//
// One engine runs the schedules of the apps in its ChloeDB: several engines
// on one database would each run them (a lease between them is not built).

export const MIN_MINUTES = 15;
export const MAX_FAILURES = 5;
const DAY_MS = 86_400_000;
const DAYS = ['sunday', 'monday', 'tuesday', 'wednesday', 'thursday', 'friday', 'saturday'];
const UNITS = { minute: 'minute', minutes: 'minute', min: 'minute', mins: 'minute', hour: 'hour', hours: 'hour', day: 'day', days: 'day', week: 'week', weeks: 'week' };

export class ScheduleError extends Error {}
const fail = (m) => { throw new ScheduleError(m); };

function checkTz(tz) {
  try { new Intl.DateTimeFormat('en-US', { timeZone: tz }).format(0); } catch { fail(`TZ ${JSON.stringify(tz)} is not a time zone (say one such as 'Europe/Madrid' or 'America/New_York')`); }
  return tz;
}

// "EVERY ..." (the keyword optional) -> {unit, n, at, days, tz}.
//   EVERY 30 MINUTES | EVERY HOUR | EVERY 2 HOURS
//   EVERY DAY [AT 'HH:MM'] | EVERY 3 DAYS AT '06:00'
//   EVERY MONDAY [AT ...] | EVERY WEEKDAY [AT ...] | EVERY WEEK [ON FRIDAY] [AT ...]
//   any of them [TZ 'Area/City'] (UTC when not given)
export function parseEvery(text) {
  let s = String(text ?? '').trim().replace(/;$/, '').trim();
  if (!s) fail('give when it runs, such as EVERY DAY AT \'07:00\'');
  s = s.replace(/^every\s+/i, '');
  let tz = 'UTC';
  const tzm = s.match(/\s*\bTZ\s+'([^']+)'\s*$/i);
  if (tzm) { tz = checkTz(tzm[1]); s = s.slice(0, tzm.index).trim(); }
  let at = null;
  const atm = s.match(/\s*\bAT\s+'?(\d{1,2})(?::(\d{2}))?'?\s*$/i);
  if (atm) {
    const h = Number(atm[1]); const m = Number(atm[2] || 0);
    if (h > 23 || m > 59) fail(`AT '${atm[1]}:${atm[2] || '00'}' is not a time of day (00:00 to 23:59)`);
    at = `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}`;
    s = s.slice(0, atm.index).trim();
  }
  let days = null;
  const onm = s.match(/\s*\bON\s+([a-z, ]+)$/i);
  if (onm) { days = dayList(onm[1]); s = s.slice(0, onm.index).trim(); }
  const words = s.toLowerCase().split(/\s+/).filter(Boolean);
  let n = 1;
  let unit;
  const isDay = (w) => DAYS.includes(w.replace(/s$/, '')) || /^week(day|end)s?$/.test(w) || w === 'and';
  if (words.length && words.every((w) => isDay(w.replace(/,$/, ''))) && words[0] !== 'and') {
    unit = 'week';
    days = dayList(words.join(' '));
  } else {
    if (words.length === 2) { n = Number(words[0]); if (!Number.isInteger(n) || n < 1) fail(`"${words[0]}" is not a whole number`); words.shift(); }
    if (words.length !== 1 || !UNITS[words[0]]) fail(`EVERY ${s}: say EVERY 30 MINUTES, EVERY HOUR, EVERY 2 HOURS, EVERY DAY AT '07:00', EVERY MONDAY AT '09:00' or EVERY WEEKDAY AT '08:00'`);
    unit = UNITS[words[0]];
  }
  if (unit === 'minute' && n < MIN_MINUTES) fail(`a schedule runs at most every ${MIN_MINUTES} minutes (not every ${n})`);
  if (unit === 'minute' && n % 60 === 0) { unit = 'hour'; n /= 60; }
  if ((unit === 'minute' || unit === 'hour') && (at || days)) fail(`EVERY ${n} ${unit}${n === 1 ? '' : 's'} runs on its own count; AT and ON are for days and weeks`);
  if (unit === 'week') { if (n !== 1) fail('weeks are EVERY WEEK or EVERY MONDAY (not every few weeks)'); days ||= [1]; }
  if (unit === 'day' && days) fail('ON names days of the week: say EVERY MONDAY or EVERY WEEKDAY');
  if (unit === 'day' || unit === 'week') at ||= '00:00';
  return { unit, n, at, days, tz };
}

function dayList(words) {
  const out = new Set();
  for (const w of String(words).toLowerCase().split(/[\s,]+|\band\b/).filter(Boolean)) {
    if (w === 'weekday' || w === 'weekdays') [1, 2, 3, 4, 5].forEach((d) => out.add(d));
    else if (w === 'weekend' || w === 'weekends') [0, 6].forEach((d) => out.add(d));
    else { const i = DAYS.indexOf(w.replace(/s$/, '')); if (i < 0) fail(`${w} is not a day of the week`); out.add(i); }
  }
  if (!out.size) fail('ON names days of the week');
  return [...out].sort();
}

// The clause again, as a statement says it: parseEvery(everyText(x)) is x.
export function everyText(e) {
  if (!e) return '';
  const tz = e.tz && e.tz !== 'UTC' ? ` TZ '${e.tz}'` : '';
  if (e.unit === 'minute' || e.unit === 'hour') return `EVERY ${e.n === 1 ? '' : `${e.n} `}${e.unit.toUpperCase()}${e.n === 1 ? '' : 'S'}${tz}`;
  const at = ` AT '${e.at || '00:00'}'`;
  if (e.unit === 'day') return `EVERY ${e.n === 1 ? 'DAY' : `${e.n} DAYS`}${at}${tz}`;
  const d = e.days || [1];
  const name = d.join() === '1,2,3,4,5' ? 'WEEKDAY' : d.length === 1 ? DAYS[d[0]].toUpperCase() : `WEEK ON ${d.map((x) => DAYS[x].toUpperCase()).join(', ')}`;
  return `EVERY ${name}${at}${tz}`;
}

// In words, for people: "every day at 07:00 (Europe/Madrid)".
export function everyWords(e) {
  return everyText(e).toLowerCase().replace(/ tz '([^']+)'/, (m, tz) => ` (${tz.replace(/(^|\/)([a-z])/g, (x, a, b) => a + b.toUpperCase())})`).replace(/'/g, '');
}

// ---- the next time

function parts(ms, tz) {
  const f = new Intl.DateTimeFormat('en-US', { timeZone: tz, hourCycle: 'h23', year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', second: '2-digit' });
  const o = {};
  for (const p of f.formatToParts(new Date(ms))) o[p.type] = p.value;
  return { y: Number(o.year), m: Number(o.month), d: Number(o.day), hh: Number(o.hour) % 24, mm: Number(o.minute), ss: Number(o.second) };
}
// The instant that reads y-m-d hh:mm on a clock in tz.
export function zoned(y, m, d, hh, mm, tz) {
  const guess = Date.UTC(y, m - 1, d, hh, mm);
  const off = (t) => { const p = parts(t, tz); return Date.UTC(p.y, p.m - 1, p.d, p.hh, p.mm, p.ss) - t; };
  let t = guess - off(guess);
  t = guess - off(t);
  return t;
}

// The first run after `after` (ms). For a count of minutes or hours, the
// last run plus the count (or `after` itself when it never ran: it runs on
// the next tick); for days and weeks, the next time the clock in tz reads
// AT on a day that is one of them (n days after the last run, for every n days).
export function nextRun(e, after, last = null) {
  if (e.unit === 'minute' || e.unit === 'hour') {
    const step = e.n * (e.unit === 'minute' ? 60_000 : 3_600_000);
    if (last == null) return after;
    let t = last + step;
    if (t <= after) t = after; // missed while away: once now, not once per missed time
    return t;
  }
  const [hh, mm] = (e.at || '00:00').split(':').map(Number);
  const p = parts(after, e.tz || 'UTC');
  let first = 0;
  if (e.unit === 'day' && e.n > 1 && last != null) {
    const l = parts(last, e.tz || 'UTC');
    first = Math.max(0, Math.round((Date.UTC(l.y, l.m - 1, l.d) - Date.UTC(p.y, p.m - 1, p.d)) / DAY_MS) + e.n);
  }
  for (let i = first; i < first + 15; i++) {
    const day = new Date(Date.UTC(p.y, p.m - 1, p.d + i));
    if (e.days && !e.days.includes(day.getUTCDay())) continue;
    const t = zoned(day.getUTCFullYear(), day.getUTCMonth() + 1, day.getUTCDate(), hh, mm, e.tz || 'UTC');
    if (t > after) return t;
  }
  return after + DAY_MS;
}

// ---- running them

export class Scheduler {
  // engine: runs the calls; reach: the owner's switch for schedules.
  constructor({ engine, reach, log = () => {}, now = () => Date.now(), tickMs = 30_000 } = {}) {
    this.engine = engine;
    this.store = engine.store;
    this.reach = reach;
    this.log = log;
    this.now = now;
    this.tickMs = tickMs;
    this.busy = false;
    this.running = new Set(); // `${app}/${slug}` while it runs
  }

  start() {
    if (this.timer) return;
    this.timer = setInterval(() => this.tick().catch((e) => this.log(`schedules: ${e.message}`)), this.tickMs);
    this.timer.unref?.();
    this.first = setTimeout(() => this.tick().catch((e) => this.log(`schedules: ${e.message}`)), 3000);
    this.first.unref?.();
  }

  stop() { clearInterval(this.timer); clearTimeout(this.first); this.timer = null; }

  async state(appId, slug) {
    const raw = await this.store.setting(appId, `schedule:${slug}`).catch(() => null);
    try { return raw ? JSON.parse(raw) : {}; } catch { return {}; }
  }

  async save(appId, slug, st) { await this.store.putSetting(appId, `schedule:${slug}`, JSON.stringify(st)); }

  // The live version's scheduled pieces, each with when it runs and how its
  // last run went.
  async list(appId, view = null) {
    view ||= await this.store.resolve(appId);
    const on = this.reach ? (await this.reach.switches(appId)).schedule : true;
    const out = [];
    for (const fn of view.functions || []) {
      if (!fn.spec?.every) continue;
      const st = await this.state(appId, fn.slug);
      out.push({ slug: fn.slug, title: fn.title || fn.slug, every: fn.spec.every, when: everyWords(fn.spec.every), source: !!fn.spec.source,
        next: on && !st.paused ? st.next || null : null, last: st.last || null, status: st.status || 'never run', error: st.error || null,
        result: st.result ?? null, runs: st.runs || 0, failures: st.failures || 0, paused: !!st.paused, on });
    }
    return out;
  }

  async tick() {
    if (this.busy) return;
    this.busy = true;
    try {
      for (const app of await this.store.listApps()) {
        if (!Number(app.head)) continue;
        await this.app(app.id).catch((e) => this.log(`[${app.id}] schedules: ${e.message}`));
      }
    } finally { this.busy = false; }
  }

  async app(appId) {
    const view = await this.store.resolve(appId);
    const fns = (view.functions || []).filter((f) => f.spec?.every);
    if (!fns.length) return;
    const on = this.reach ? (await this.reach.switches(appId)).schedule : true;
    const now = this.now();
    for (const fn of fns) {
      const st = await this.state(appId, fn.slug);
      const text = everyText(fn.spec.every);
      // a new schedule, or one a change moved: its next time from now
      if (!st.next || st.when !== text) {
        st.when = text;
        st.next = nextRun(fn.spec.every, now, st.last ? Date.parse(st.last) : null);
        await this.save(appId, fn.slug, st);
      }
      if (!on || st.paused || st.next > now) continue;
      await this.run(appId, fn, st);
    }
  }

  // Runs one scheduled piece now: on its time, or when its owner asks.
  async run(appId, fn, st = null, by = 'schedule') {
    const key = `${appId}/${fn.slug}`;
    if (this.running.has(key)) return null;
    this.running.add(key);
    st ||= await this.state(appId, fn.slug);
    const started = this.now();
    try {
      const p = parts(started, fn.spec.every.tz || 'UTC');
      const today = `${p.y}-${String(p.m).padStart(2, '0')}-${String(p.d).padStart(2, '0')}`;
      const out = await this.engine.call(appId, fn.slug, { today }, { who: by, trigger: `${by === 'schedule' ? 'schedule' : 'run by hand'} ${fn.slug}` });
      Object.assign(st, { status: 'ok', error: null, failures: 0, result: short(out?.result) });
    } catch (e) {
      // A refusal (a switch off, a limit reached, a REQUIRE) is not the
      // code's failure: it is said, not counted towards stopping.
      st.status = e.refused ? 'refused' : 'failed';
      st.error = String(e.message || e).slice(0, 500);
      if (!e.refused) st.failures = (st.failures || 0) + 1;
      if (st.failures >= MAX_FAILURES) { st.paused = true; this.log(`[${appId}] schedule ${fn.slug} stopped after ${st.failures} failures in a row: ${st.error}`); }
    } finally { this.running.delete(key); }
    st.last = new Date(started).toISOString();
    st.ms = this.now() - started;
    st.runs = (st.runs || 0) + 1;
    st.when = everyText(fn.spec.every);
    st.next = nextRun(fn.spec.every, Math.max(this.now(), started + 1), started);
    await this.save(appId, fn.slug, st);
    return st;
  }

  async runNow(appId, slug) {
    const view = await this.store.resolve(appId);
    const fn = (view.functions || []).find((f) => f.slug === slug && f.spec?.every);
    if (!fn) throw Object.assign(new Error(`no scheduled rule or function ${slug} in this app`), { status: 404 });
    return this.run(appId, fn, null, 'owner');
  }

  // Pause or start again (which clears the count of failures).
  async setPaused(appId, slug, paused) {
    const st = await this.state(appId, slug);
    st.paused = !!paused;
    if (!paused) { st.failures = 0; st.next = null; }
    await this.save(appId, slug, st);
    return st;
  }
}

function short(v) {
  if (v === undefined || v === null) return null;
  const s = typeof v === 'string' ? v : JSON.stringify(v);
  return s.length > 300 ? `${s.slice(0, 300)}…` : s;
}

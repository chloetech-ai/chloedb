// title: Durations
// description: Lengths of time as people write them: read "1h 30m", "90 min", "1:30", "1.5 hours" or "PT1H30M", print them short, long or as a clock, add them to a time, and total them.
// api: parse(text) -> seconds or null ('1h 30m', '90m', '1:30' (hours:minutes), '0:45:10', '1.5h', '2 days', 'PT1H30M', a plain number is minutes);
//   format(seconds, style = 'short' | 'long' | 'clock') -> '1h 30m' | '1 hour 30 minutes' | '1:30:00' ('' for null);
//   between(a, b) -> seconds from time a to time b (ISO text or Date); add(time, seconds) -> ISO text;
//   sum(list, by?) -> seconds (list of seconds, texts, or rows with by(row) or row[by]); minutes(text) -> whole minutes or null
function helper() {
  const UNITS = { w: 604800, week: 604800, weeks: 604800, d: 86400, day: 86400, days: 86400, h: 3600, hr: 3600, hrs: 3600, hour: 3600, hours: 3600,
    m: 60, min: 60, mins: 60, minute: 60, minutes: 60, s: 1, sec: 1, secs: 1, second: 1, seconds: 1 };
  function parse(text) {
    if (typeof text === 'number') return Number.isFinite(text) ? Math.round(text * 60) : null;
    const t = String(text || '').trim().toLowerCase();
    if (!t) return null;
    let m = t.match(/^p(?:(\d+)d)?(?:t(?:(\d+(?:\.\d+)?)h)?(?:(\d+(?:\.\d+)?)m)?(?:(\d+(?:\.\d+)?)s)?)?$/);
    if (m && t !== 'p') return Math.round((+(m[1] || 0)) * 86400 + (+(m[2] || 0)) * 3600 + (+(m[3] || 0)) * 60 + (+(m[4] || 0)));
    m = t.match(/^(\d+):([0-5]\d)(?::([0-5]\d))?$/);
    if (m) return +m[1] * 3600 + +m[2] * 60 + +(m[3] || 0);
    if (/^\d+(\.\d+)?$/.test(t)) return Math.round(Number(t) * 60);
    let total = 0; let found = false;
    const re = /(\d+(?:[.,]\d+)?)\s*([a-z]+)/g;
    let x;
    while ((x = re.exec(t))) {
      const u = UNITS[x[2]] || UNITS[x[2].replace(/s$/, '')];
      if (!u) return null;
      total += Number(x[1].replace(',', '.')) * u;
      found = true;
    }
    const rest = t.replace(re, '').replace(/\band\b|[,\s]/g, '');
    return found && !rest ? Math.round(total) : null;
  }
  function format(seconds, style = 'short') {
    if (seconds === null || seconds === undefined || !Number.isFinite(Number(seconds))) return '';
    let s = Math.round(Math.abs(Number(seconds)));
    const sign = Number(seconds) < 0 ? '-' : '';
    if (style === 'clock') {
      const hh = Math.floor(s / 3600); const mm = Math.floor((s % 3600) / 60); const ss = s % 60;
      return `${sign}${hh}:${String(mm).padStart(2, '0')}:${String(ss).padStart(2, '0')}`;
    }
    const parts = [];
    const d = Math.floor(s / 86400); s %= 86400;
    const h = Math.floor(s / 3600); s %= 3600;
    const m = Math.floor(s / 60); const sec = s % 60;
    const long = style === 'long';
    const put = (n, short, word) => { if (n) parts.push(long ? `${n} ${word}${n === 1 ? '' : 's'}` : `${n}${short}`); };
    put(d, 'd', 'day'); put(h, 'h', 'hour'); put(m, 'm', 'minute');
    if (!d && !h && (sec || !parts.length)) put(sec || 0, 's', 'second');
    if (!parts.length) parts.push(long ? '0 seconds' : '0s');
    return sign + parts.join(' ');
  }
  const asTime = (v) => (v instanceof Date ? v.getTime() : Date.parse(String(v)));
  const between = (a, b) => Math.round((asTime(b) - asTime(a)) / 1000);
  const add = (time, seconds) => new Date(asTime(time) + Number(seconds) * 1000).toISOString();
  function sum(list, by) {
    let total = 0;
    for (const x of list || []) {
      const v = typeof by === 'function' ? by(x) : by ? x[by] : x;
      const s = typeof v === 'string' ? parse(v) : Number(v);
      if (Number.isFinite(s)) total += s;
    }
    return total;
  }
  const minutes = (text) => { const s = parse(text); return s === null ? null : Math.round(s / 60); };
  return { parse, format, between, add, sum, minutes };
}

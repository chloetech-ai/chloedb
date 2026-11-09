// title: Times of day
// description: Times of day as 'HH:MM': read what people type ('9:30pm', '21h', 'noon'), print them in 12 or 24 hours, minute arithmetic that wraps past midnight, slot lists and overlap checks for schedules, opening hours and bookings.
// api: parse(text) -> 'HH:MM' or null ('9', '09:30', '9:30pm', '9.30', '930', '9 p.m.', '12am' = '00:00', '21h', '21h30', '9:30:15' (seconds dropped), 'noon', 'midnight'; a number is an hour);
//   format(hhmm, style = '24h' | '12h') -> '21:30' | '9:30 PM' ('' when the time cannot be read);
//   minutes(hhmm) -> minutes since midnight, 0..1439, or null; fromMinutes(m) -> 'HH:MM' (rounded, wrapped into one day; null when not a number);
//   add(hhmm, minutes) -> 'HH:MM', wrapping past midnight either way (null when unreadable);
//   between(a, b) -> minutes from a to b, 0..1439, across midnight when b is earlier than a (null when unreadable);
//   slots(from, to, step = 30) -> slot starts from 'from' up to but not including 'to' (['09:00', '09:30', ...]), across midnight when to < from; [] when from = to or unreadable;
//   overlaps([a1, a2], [b1, b2]) -> true when the two ranges share any minute (ends are exclusive, so touching ranges do not overlap; a range whose end is earlier than its start runs past midnight; empty ranges never overlap);
//   round(hhmm, step = 15) -> 'HH:MM' rounded to the nearest step minutes (halves round up, wraps: '23:53' -> '00:00'); null when unreadable;
//   every function also accepts any time parse reads
function helper() {
  const DAY = 1440;
  const pad = (n) => String(n).padStart(2, '0');
  const wrap = (m) => ((m % DAY) + DAY) % DAY;
  const out = (m) => `${pad(Math.floor(m / 60))}:${pad(m % 60)}`;

  function toMin(text) {
    if (text === null || text === undefined) return null;
    if (typeof text === 'number') {
      if (!Number.isInteger(text) || text < 0 || text > 23) return null;
      return text * 60;
    }
    if (typeof text !== 'string') return null;
    let s = text.trim().toLowerCase().replace(/a\.m\.?/g, 'am').replace(/p\.m\.?/g, 'pm').replace(/\s+/g, '');
    if (!s) return null;
    if (s === 'noon' || s === 'midday') return 720;
    if (s === 'midnight') return 0;
    let h;
    let m = 0;
    let mer;
    let r = s.match(/^(\d{1,2})(?:[:.h](\d{2})(?::\d{2}(?:\.\d+)?)?)?h?(am|pm|a|p)?$/);
    if (r) { h = Number(r[1]); m = r[2] ? Number(r[2]) : 0; mer = r[3]; }
    else {
      r = s.match(/^(\d{3,4})(am|pm|a|p)?$/);
      if (!r) return null;
      h = Number(r[1].slice(0, -2)); m = Number(r[1].slice(-2)); mer = r[2];
    }
    if (m > 59) return null;
    if (mer) {
      if (h < 1 || h > 12) return null;
      h = h % 12 + (mer[0] === 'p' ? 12 : 0);
    } else if (h > 23) return null;
    return h * 60 + m;
  }

  function parse(text) { const m = toMin(text); return m === null ? null : out(m); }

  function format(hhmm, style = '24h') {
    const m = toMin(hhmm);
    if (m === null) return '';
    if (style !== '12h') return out(m);
    const h = Math.floor(m / 60);
    return `${h % 12 || 12}:${pad(m % 60)} ${h < 12 ? 'AM' : 'PM'}`;
  }

  function minutes(hhmm) { return toMin(hhmm); }

  function fromMinutes(m) {
    if (typeof m !== 'number' || !isFinite(m)) return null;
    return out(wrap(Math.round(m)));
  }

  function add(hhmm, mins) {
    const m = toMin(hhmm);
    if (m === null || typeof mins !== 'number' || !isFinite(mins)) return null;
    return out(wrap(m + Math.round(mins)));
  }

  function between(a, b) {
    const ma = toMin(a);
    const mb = toMin(b);
    if (ma === null || mb === null) return null;
    return wrap(mb - ma);
  }

  function slots(from, to, step = 30) {
    const start = toMin(from);
    const span = between(from, to);
    const st = typeof step === 'number' && isFinite(step) && step >= 1 ? Math.round(step) : 30;
    if (start === null || !span) return [];
    const list = [];
    for (let t = 0; t < span; t += st) list.push(out(wrap(start + t)));
    return list;
  }

  function range(r) {
    if (!Array.isArray(r) || r.length < 2) return null;
    const s = toMin(r[0]);
    const len = between(r[0], r[1]);
    if (s === null || !len) return null;
    return [s, s + len];
  }

  function overlaps(a, b) {
    const ra = range(a);
    const rb = range(b);
    if (!ra || !rb) return false;
    for (const shift of [-DAY, 0, DAY]) {
      if (ra[0] < rb[1] + shift && rb[0] + shift < ra[1]) return true;
    }
    return false;
  }

  function round(hhmm, step = 15) {
    const m = toMin(hhmm);
    if (m === null) return null;
    const st = typeof step === 'number' && isFinite(step) && step > 0 ? step : 15;
    return out(wrap(Math.round(Math.floor(m / st + 0.5) * st)));
  }

  return { parse, format, minutes, fromMinutes, add, between, slots, overlaps, round };
}

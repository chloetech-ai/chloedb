// title: Scheduling
// description: Finding times in local time: free slots between busy events within working hours, overlapping events, merged busy blocks, whether a slot fits and the next free start; for meetings, bookings, appointments and planning a day.
// api: free(busy, {from = now, to = from + 7 days, minutes = 30, hours = ['09:00', '17:00'], days = [1, 2, 3, 4, 5], step = 15}) -> [{start, end}] oldest first:
//   the free stretches inside the working hours of the working days between from and to, each at least minutes long, its start moved up to the step grid
//   (minutes from local midnight); stretches that touch across days are joined; hours ['00:00', '24:00'] is the whole day, an end before the start runs past midnight;
//   days: weekdays 0 Sunday .. 6 Saturday or names ('mon', ...);
//   conflicts(events) -> [[a, b], ...] the pairs of events that overlap (touching is not overlapping), a starting first, as the objects given;
//   merge(intervals) -> [{start, end}] sorted, with overlapping and touching intervals joined;
//   fits(slot, busy) -> true when slot {start, end} overlaps none of busy (false for a missing or empty slot);
//   nextFree(busy, {after = now, minutes = 30, hours, days, step = 15}) -> the first free start at or after after (looking a year ahead), or null;
//   busy and events: [{start, end}] (extra fields kept), times as full ISO text (with Z or an offset, or none for local time), 'YYYY-MM-DD' (local midnight),
//   a Date or milliseconds; entries that cannot be read or end before they start are skipped; times come back as ISO text in UTC ('2026-11-09T09:00:00Z')
function helper() {
  const MIN = 60000;
  const DAY = 86400000;
  const NAMES = { sun: 0, mon: 1, tue: 2, wed: 3, thu: 4, fri: 5, sat: 6 };
  const RE = /^(\d{4})-(\d{2})-(\d{2})(?:[T ](\d{2}):(\d{2})(?::(\d{2})(?:\.(\d{1,9}))?)?)?\s*(Z|z|[+-]\d{2}(?::?\d{2})?)?$/;

  function toMs(v) {
    if (v instanceof Date) { const t = v.getTime(); return isNaN(t) ? null : t; }
    if (typeof v === 'number') return isFinite(v) ? v : null;
    if (typeof v !== 'string') return null;
    const m = RE.exec(v.trim());
    if (!m) return null;
    const y = +m[1], mo = +m[2], d = +m[3], h = +(m[4] || 0), mi = +(m[5] || 0), s = +(m[6] || 0);
    const ms = m[7] ? Math.round(Number('0.' + m[7]) * 1000) : 0;
    if (mo < 1 || mo > 12 || d < 1 || d > 31 || h > 24 || mi > 59 || s > 60) return null;
    if (m[8]) {
      let off = 0;
      if (!/z/i.test(m[8])) {
        const sign = m[8][0] === '-' ? -1 : 1;
        const digits = m[8].slice(1).replace(':', '');
        off = sign * (Number(digits.slice(0, 2)) * 60 + Number(digits.slice(2, 4) || 0));
      }
      return Date.UTC(y, mo - 1, d, h, mi, s, ms) - off * MIN;
    }
    return new Date(y, mo - 1, d, h, mi, s, ms).getTime();
  }
  const iso = (ms) => new Date(ms).toISOString().replace('.000Z', 'Z');

  function intervals(list) {
    const out = [];
    (Array.isArray(list) ? list : []).forEach((item, i) => {
      if (!item || typeof item !== 'object') return;
      const s = toMs(item.start);
      const e = toMs(item.end);
      if (s == null || e == null || e <= s) return;
      out.push({ s, e, item, i });
    });
    return out.sort((a, b) => a.s - b.s || a.i - b.i);
  }
  function mergeMs(list) {
    const out = [];
    for (const iv of list) {
      const last = out[out.length - 1];
      if (last && iv.s <= last.e) last.e = Math.max(last.e, iv.e);
      else out.push({ s: iv.s, e: iv.e });
    }
    return out;
  }

  function merge(list) { return mergeMs(intervals(list)).map((iv) => ({ start: iso(iv.s), end: iso(iv.e) })); }

  function conflicts(events) {
    const list = intervals(events);
    const out = [];
    for (let i = 0; i < list.length; i++) {
      for (let j = i + 1; j < list.length && list[j].s < list[i].e; j++) out.push([list[i].item, list[j].item]);
    }
    return out;
  }

  function fits(slot, busy) {
    if (!slot || typeof slot !== 'object') return false;
    const s = toMs(slot.start);
    const e = toMs(slot.end);
    if (s == null || e == null || e <= s) return false;
    return !intervals(busy).some((iv) => iv.s < e && iv.e > s);
  }

  const hm = (t) => {
    const m = /^(\d{1,2}):(\d{2})$/.exec(String(t == null ? '' : t).trim());
    if (!m || +m[2] > 59) return null;
    const v = +m[1] * 60 + +m[2];
    return v <= 1440 ? v : null;
  };
  function daySet(days) {
    const set = new Set();
    if (Array.isArray(days)) {
      for (const d of days) {
        const n = typeof d === 'number' ? d : NAMES[String(d).slice(0, 3).toLowerCase()];
        if (Number.isInteger(n) && n >= 0 && n <= 6) set.add(n);
      }
    }
    return set.size ? set : new Set([1, 2, 3, 4, 5]);
  }
  function ceilStep(ms, step) {
    const d = new Date(ms);
    const mid = new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
    const q = step * MIN;
    return mid + Math.ceil((ms - mid) / q) * q;
  }

  function free(busy, opts) {
    const o = opts && typeof opts === 'object' ? opts : {};
    const from = o.from == null ? Date.now() : toMs(o.from);
    if (from == null) return [];
    const to = o.to == null ? from + 7 * DAY : toMs(o.to);
    if (to == null || to <= from) return [];
    const minutes = Number(o.minutes) > 0 ? Number(o.minutes) : 30;
    const step = Number(o.step) >= 1 ? Number(o.step) : 15;
    let h1 = Array.isArray(o.hours) ? hm(o.hours[0]) : null;
    let h2 = Array.isArray(o.hours) ? hm(o.hours[1]) : null;
    if (h1 == null || h2 == null) { h1 = 540; h2 = 1020; }
    const days = daySet(o.days);
    const blocks = mergeMs(intervals(busy));

    const raw = [];
    const f = new Date(from);
    const y = f.getFullYear(), mo = f.getMonth();
    let p = 0;
    // start a day early so working hours running past midnight into from are seen
    for (let d = f.getDate() - 1, guard = 0; guard < 4000; d++, guard++) {
      const ws = new Date(y, mo, d, 0, h1).getTime();
      if (ws >= to) break;
      const we = new Date(y, mo, d + (h2 <= h1 ? 1 : 0), 0, h2).getTime();
      if (!days.has(new Date(y, mo, d).getDay()) || we <= from) continue;
      const a = Math.max(ws, from);
      const end = Math.min(we, to);
      if (end <= a) continue;
      while (p < blocks.length && blocks[p].e <= a) p++;
      let cur = a;
      for (let k = p; k < blocks.length && cur < end; k++) {
        const iv = blocks[k];
        if (iv.s >= end) break;
        if (iv.s > cur) raw.push([cur, iv.s]);
        cur = Math.max(cur, iv.e);
      }
      if (cur < end) raw.push([cur, end]);
    }

    const joined = [];
    for (const r of raw) {
      const last = joined[joined.length - 1];
      if (last && r[0] <= last[1]) last[1] = Math.max(last[1], r[1]);
      else joined.push([r[0], r[1]]);
    }
    const out = [];
    for (const [s0, e] of joined) {
      const s = ceilStep(s0, step);
      if (e - s >= minutes * MIN) out.push({ start: iso(s), end: iso(e) });
    }
    return out;
  }

  function nextFree(busy, opts) {
    const o = opts && typeof opts === 'object' ? opts : {};
    const after = o.after == null ? Date.now() : toMs(o.after);
    if (after == null) return null;
    const slots = free(busy, { minutes: o.minutes, hours: o.hours, days: o.days, step: o.step, from: after, to: after + 366 * DAY });
    return slots.length ? slots[0].start : null;
  }

  return { free, conflicts, merge, fits, nextFree };
}

// title: Inventory
// description: Stock arithmetic for pantries, stockrooms, shops and supplies: whether an item is out, low, fine or over stocked, how many to reorder in whole packs, stock levels from a log of movements, how fast it is used, when it runs out, what it is worth and what expires soon.
// api: status(qty, {reorderAt, max}) -> 'out' (qty <= 0) | 'low' (qty <= reorderAt) | 'over' (qty > max) | 'ok', or null when qty is not a number;
//   reorderQty(qty, {reorderAt = 0, max, packSize = 1, minOrder = 0}) -> how many to order: 0 while qty is above reorderAt, else enough to reach max
//   (or to rise just above reorderAt when there is no max), rounded up to whole packs and at least minOrder; null when qty is not a number;
//   level(movements, {start = 0}) -> the quantity after applying movements in the order given; a movement is {qty, type?}:
//   type 'in' adds |qty|, 'out' takes |qty|, 'count' or 'set' sets the level to qty, no type adds qty as a signed number; odd entries are skipped;
//   levels(movements, {by = 'item', start = {}}) -> {key: quantity} per item (by: a field name or a function of the movement; start: starting levels by key);
//   usage(movements, {from, to, today, date = 'date'}) -> average quantity taken out per day over from..to inclusive ('YYYY-MM-DD'; to defaults to today,
//   from to 29 days before to), counting 'out' movements and untyped negative ones dated in the range (a day or an ISO time), to 3 decimals; null when the range is empty;
//   daysLeft(qty, perDay) -> whole days the stock lasts (0 when qty <= 0), null when perDay is not above 0;
//   runOut(qty, perDay, today) -> the 'YYYY-MM-DD' day it runs out (today + daysLeft), null when it never does;
//   value(items, {qty = 'qty', cost = 'cost'}) -> the stock's worth to the cent, sum of qty × cost (negative quantities count as 0, odd rows skipped);
//   expiring(items, {today, within = 7, field = 'expires'}) -> [{item, days}] of items whose 'YYYY-MM-DD' (or ISO) field is within that many days,
//   expired ones included with negative days, soonest first; items without a valid date are left out
function helper() {
  const DAY = 86400000;
  const pad = (n) => String(n).padStart(2, '0');
  const isDay = (d) => typeof d === 'string' && /^\d{4}-\d{2}-\d{2}/.test(d);
  const toUTC = (d) => Date.UTC(Number(d.slice(0, 4)), Number(d.slice(5, 7)) - 1, Number(d.slice(8, 10)));
  const fromUTC = (ms) => { const d = new Date(ms); return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`; };
  const localToday = () => { const d = new Date(); return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`; };
  const addDays = (d, n) => fromUTC(toUTC(d) + n * DAY);
  const tidy = (x) => Math.round(x * 1e6) / 1e6;
  function num(v) {
    if (typeof v === 'number') return isFinite(v) ? v : null;
    if (typeof v === 'string' && v.trim() !== '' && isFinite(Number(v))) return Number(v);
    return null;
  }
  const getter = (by, dflt) => (typeof by === 'function' ? by : (row) => (row == null ? undefined : row[by == null ? dflt : by]));

  function status(qty, { reorderAt, max } = {}) {
    const q = num(qty);
    if (q === null) return null;
    if (q <= 0) return 'out';
    const r = num(reorderAt);
    if (r !== null && q <= r) return 'low';
    const m = num(max);
    if (m !== null && q > m) return 'over';
    return 'ok';
  }

  function reorderQty(qty, { reorderAt, max, packSize = 1, minOrder = 0 } = {}) {
    const q = num(qty);
    if (q === null) return null;
    const r = num(reorderAt) ?? 0;
    if (q > r) return 0;
    const m = num(max);
    const need = m !== null ? m - q : r - q + 1;
    if (need <= 0) return 0;
    const p = num(packSize) > 0 ? num(packSize) : 1;
    const packs = (n) => Math.ceil(tidy(n / p)) * p;
    const min = num(minOrder) > 0 ? packs(num(minOrder)) : 0;
    return tidy(Math.max(packs(need), min));
  }

  function step(q, mv) {
    if (!mv || typeof mv !== 'object') return q;
    const v = num(mv.qty);
    if (v === null) return q;
    const t = String(mv.type || '').toLowerCase();
    if (t === 'in') return q + Math.abs(v);
    if (t === 'out') return q - Math.abs(v);
    if (t === 'count' || t === 'set') return v;
    return q + v;
  }

  function level(movements, { start = 0 } = {}) {
    let q = num(start) ?? 0;
    for (const mv of Array.isArray(movements) ? movements : []) q = step(q, mv);
    return tidy(q);
  }

  function levels(movements, { by = 'item', start = {} } = {}) {
    const key = getter(by, 'item');
    const out = {};
    const base = start && typeof start === 'object' ? start : {};
    for (const k of Object.keys(base)) out[k] = num(base[k]) ?? 0;
    for (const mv of Array.isArray(movements) ? movements : []) {
      if (!mv || typeof mv !== 'object') continue;
      const k = key(mv);
      if (k == null || k === '') continue;
      out[k] = tidy(step(out[k] ?? 0, mv));
    }
    return out;
  }

  function usage(movements, { from, to, today, date = 'date' } = {}) {
    const end = isDay(to) ? to.slice(0, 10) : isDay(today) ? today.slice(0, 10) : localToday();
    const begin = isDay(from) ? from.slice(0, 10) : addDays(end, -29);
    const days = Math.round((toUTC(end) - toUTC(begin)) / DAY) + 1;
    if (!(days > 0)) return null;
    const when = getter(date, 'date');
    let total = 0;
    for (const mv of Array.isArray(movements) ? movements : []) {
      if (!mv || typeof mv !== 'object') continue;
      const d = when(mv);
      if (!isDay(d)) continue;
      const day = d.slice(0, 10);
      if (day < begin || day > end) continue;
      const v = num(mv.qty);
      if (v === null) continue;
      const t = String(mv.type || '').toLowerCase();
      if (t === 'out') total += Math.abs(v);
      else if (!t && v < 0) total += -v;
    }
    return Math.round((total / days) * 1000) / 1000;
  }

  function daysLeft(qty, perDay) {
    const q = num(qty);
    const r = num(perDay);
    if (q === null || r === null || r <= 0) return null;
    if (q <= 0) return 0;
    return Math.floor(tidy(q / r));
  }

  function runOut(qty, perDay, today) {
    const n = daysLeft(qty, perDay);
    if (n === null) return null;
    return addDays(isDay(today) ? today.slice(0, 10) : localToday(), n);
  }

  function value(items, { qty = 'qty', cost = 'cost' } = {}) {
    const getQ = getter(qty, 'qty');
    const getC = getter(cost, 'cost');
    let cents = 0;
    for (const it of Array.isArray(items) ? items : []) {
      if (!it || typeof it !== 'object') continue;
      const q = num(getQ(it));
      const c = num(getC(it));
      if (q === null || c === null) continue;
      cents += Math.round(Math.max(0, q) * c * 100);
    }
    return cents / 100;
  }

  function expiring(items, { today, within = 7, field = 'expires' } = {}) {
    const now = toUTC(isDay(today) ? today.slice(0, 10) : localToday());
    const limit = num(within) ?? 7;
    const get = getter(field, 'expires');
    const out = [];
    for (const it of Array.isArray(items) ? items : []) {
      const d = get(it);
      if (!isDay(d)) continue;
      const ms = toUTC(d.slice(0, 10));
      if (!isFinite(ms)) continue;
      const days = Math.round((ms - now) / DAY);
      if (days <= limit) out.push({ item: it, days });
    }
    return out.sort((a, b) => a.days - b.days);
  }

  return { status, reorderQty, level, levels, usage, daysLeft, runOut, value, expiring };
}

// title: Statistics
// description: Sums, averages and spreads over numbers and rows, counts and totals by group, histograms, trends, and zero-filled series by day, week or month for charts.
// api: by is a field name or a function of the row, everywhere;
//   sum(values, by?), mean(values, by?), median(values, by?), min(values, by?), max(values, by?), stdev(values, by?), percentile(values, p 0..100, by?) -> number (null for none);
//   countBy(rows, by) -> [{key, count}] most first; sumBy(rows, by, value) -> [{key, total, count}] largest first; groupBy(rows, by) -> {key: rows};
//   topN(rows, n, by) -> the n rows with the largest by;
//   histogram(values, {buckets = 10 | step, by}) -> [{from, to, count}];
//   trend(values or [{x, y}]) -> {slope, intercept, direction: 'up' | 'down' | 'flat'};
//   change(before, after) -> {diff, pct} (pct null when before is 0);
//   series(rows, {date: field or fn (a 'YYYY-MM-DD' day or ISO time), value: field or fn (default counts rows), period: 'day' | 'week' | 'month', from, to, weekStart = 1}) -> [{period: first day 'YYYY-MM-DD', value, count}] with every period from..to, empty ones 0
function helper() {
  const get = (by) => (by === undefined || by === null ? (x) => x : typeof by === 'function' ? by : (x) => (x == null ? undefined : x[by]));
  const nums = (values, by) => (values || []).map(get(by)).map(Number).filter((x) => Number.isFinite(x));
  const sum = (v, by) => nums(v, by).reduce((a, b) => a + b, 0);
  const mean = (v, by) => { const xs = nums(v, by); return xs.length ? sum(xs) / xs.length : null; };
  const percentile = (v, p, by) => {
    const xs = nums(v, by).sort((a, b) => a - b);
    if (!xs.length) return null;
    const i = (Math.min(100, Math.max(0, p)) / 100) * (xs.length - 1);
    const lo = Math.floor(i);
    return xs[lo] + (xs[Math.min(lo + 1, xs.length - 1)] - xs[lo]) * (i - lo);
  };
  const median = (v, by) => percentile(v, 50, by);
  const min = (v, by) => { const xs = nums(v, by); return xs.length ? Math.min(...xs) : null; };
  const max = (v, by) => { const xs = nums(v, by); return xs.length ? Math.max(...xs) : null; };
  const stdev = (v, by) => { const xs = nums(v, by); if (xs.length < 2) return null; const m = sum(xs) / xs.length; return Math.sqrt(xs.reduce((a, x) => a + (x - m) ** 2, 0) / (xs.length - 1)); };
  const keyOf = (k) => (k === undefined || k === null || k === '' ? '(none)' : Array.isArray(k) ? k : String(k));
  function groupBy(rows, by) {
    const f = get(by);
    const out = {};
    for (const r of rows || []) {
      const k = keyOf(f(r));
      for (const key of Array.isArray(k) ? (k.length ? k : ['(none)']) : [k]) (out[key] ||= []).push(r);
    }
    return out;
  }
  const countBy = (rows, by) => Object.entries(groupBy(rows, by)).map(([key, rs]) => ({ key, count: rs.length })).sort((a, b) => b.count - a.count || a.key.localeCompare(b.key));
  const sumBy = (rows, by, value) => Object.entries(groupBy(rows, by)).map(([key, rs]) => ({ key, total: sum(rs, value), count: rs.length })).sort((a, b) => b.total - a.total || a.key.localeCompare(b.key));
  const topN = (rows, n, by) => { const f = get(by); return (rows || []).slice().sort((a, b) => (Number(f(b)) || 0) - (Number(f(a)) || 0)).slice(0, n); };
  function histogram(values, { buckets = 10, step, by } = {}) {
    const xs = nums(values, by);
    if (!xs.length) return [];
    const lo = Math.min(...xs);
    const hi = Math.max(...xs);
    const width = step || (hi === lo ? 1 : (hi - lo) / buckets);
    const n = Math.max(1, Math.ceil((hi - lo) / width) || 1);
    const out = Array.from({ length: n }, (_, i) => ({ from: lo + i * width, to: lo + (i + 1) * width, count: 0 }));
    for (const x of xs) out[Math.min(n - 1, Math.floor((x - lo) / width))].count++;
    return out;
  }
  function trend(points) {
    const ps = (points || []).map((p, i) => (typeof p === 'object' && p ? { x: Number(p.x), y: Number(p.y) } : { x: i, y: Number(p) })).filter((p) => Number.isFinite(p.x) && Number.isFinite(p.y));
    if (ps.length < 2) return { slope: 0, intercept: ps[0] ? ps[0].y : 0, direction: 'flat' };
    const mx = ps.reduce((a, p) => a + p.x, 0) / ps.length;
    const my = ps.reduce((a, p) => a + p.y, 0) / ps.length;
    const den = ps.reduce((a, p) => a + (p.x - mx) ** 2, 0);
    const slope = den ? ps.reduce((a, p) => a + (p.x - mx) * (p.y - my), 0) / den : 0;
    const scale = Math.max(1e-9, Math.abs(my));
    return { slope, intercept: my - slope * mx, direction: Math.abs(slope) / scale < 0.01 ? 'flat' : slope > 0 ? 'up' : 'down' };
  }
  const change = (before, after) => ({ diff: after - before, pct: before ? ((after - before) / Math.abs(before)) * 100 : null });

  const pad = (n) => String(n).padStart(2, '0');
  const DAY = 86400000;
  const dayOf = (v) => {
    if (typeof v === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(v)) return v;
    const d = new Date(v);
    if (Number.isNaN(d.getTime())) return null;
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  };
  const toUTC = (d) => Date.UTC(Number(d.slice(0, 4)), Number(d.slice(5, 7)) - 1, Number(d.slice(8, 10)));
  const fromUTC = (ms) => { const d = new Date(ms); return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`; };
  function periodOf(day, period, weekStart) {
    if (period === 'month') return `${day.slice(0, 7)}-01`;
    if (period === 'week') { const ms = toUTC(day); return fromUTC(ms - ((new Date(ms).getUTCDay() - weekStart + 7) % 7) * DAY); }
    return day;
  }
  function stepPeriod(p, period) {
    if (period === 'month') { const y = Number(p.slice(0, 4)); const m = Number(p.slice(5, 7)); return m === 12 ? `${y + 1}-01-01` : `${y}-${pad(m + 1)}-01`; }
    return fromUTC(toUTC(p) + (period === 'week' ? 7 : 1) * DAY);
  }
  function series(rows, { date = 'created', value, period = 'day', from, to, weekStart = 1 } = {}) {
    const fd = get(date);
    const fv = value === undefined ? null : get(value);
    const acc = new Map();
    let first = null;
    let last = null;
    for (const r of rows || []) {
      const day = dayOf(fd(r));
      if (!day) continue;
      const p = periodOf(day, period, weekStart);
      const a = acc.get(p) || { value: 0, count: 0 };
      a.count++;
      a.value += fv ? Number(fv(r)) || 0 : 1;
      acc.set(p, a);
      if (!first || p < first) first = p;
      if (!last || p > last) last = p;
    }
    const start = from ? periodOf(dayOf(from), period, weekStart) : first;
    const end = to ? periodOf(dayOf(to), period, weekStart) : last;
    if (!start || !end) return [];
    const out = [];
    for (let p = start, guard = 0; p <= end && guard < 3700; p = stepPeriod(p, period), guard++) {
      const a = acc.get(p) || { value: 0, count: 0 };
      out.push({ period: p, value: a.value, count: a.count });
    }
    return out;
  }
  return { sum, mean, median, min, max, stdev, percentile, countBy, sumBy, groupBy, topN, histogram, trend, change, series };
}

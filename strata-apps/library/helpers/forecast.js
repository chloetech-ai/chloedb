// title: Forecasts
// description: Simple forecasts from a series of numbers (daily spending, weekly sessions, monthly sales): moving averages, exponential smoothing, a straight-line trend with the next values, the total a period will reach at the current rate, weekday-style seasonal patterns and period-over-period growth.
// api: values everywhere: an array of numbers oldest first; entries that are not numbers (null, '', NaN) are gaps, skipped where noted;
//   results are rounded to 10 decimal places so 0.1 + 0.2 reads 0.3;
//   movingAverage(values, span = 7) -> an array as long as values: each entry the mean of the numbers among the last span entries up to it (fewer at the start), null when there are none;
//   exponential(values, alpha = 0.3) -> an array as long as values: s = alpha * value + (1 - alpha) * previous s, starting at the first number; a gap repeats the previous s (null before the first number); alpha outside (0, 1] becomes 0.3;
//   linear(values) -> {slope, intercept, next(n = 1)} or null when there are no numbers: the least-squares line through (index, value) of the numbers (one number gives slope 0);
//   next(n) -> the n values the line gives after the last entry (at indexes values.length .. values.length + n - 1);
//   projectTotal(valuesSoFar, periods) -> the total by the end when the remaining periods go at the average so far: sum + mean * (periods - entries so far);
//   every entry is an elapsed period, gaps count as 0; the plain sum when periods are already over; null for no entries or periods not a positive number;
//   seasonal(values, season = 7) -> an array of season averages, one per position (index % season, so values[0] is position 0), null for a position with no numbers;
//   growth(values) -> an array as long as values of the change from the entry before as a fraction ((v - prev) / |prev|, 0.1 = +10%); null for the first entry,
//   when either is a gap, or when prev is 0
function helper() {
  const num = (v) => {
    if (typeof v === 'number') return Number.isFinite(v) ? v : null;
    if (typeof v === 'string' && v.trim() !== '') { const n = Number(v); return Number.isFinite(n) ? n : null; }
    return null;
  };
  const clean = (x) => (x === null ? null : Math.round(x * 1e10) / 1e10 + 0);
  const list = (values) => (Array.isArray(values) ? values.map(num) : []);
  const posInt = (n, dflt) => { const v = Math.floor(Number(n)); return Number.isFinite(v) && v >= 1 ? v : dflt; };

  function movingAverage(values, span = 7) {
    const xs = list(values);
    const w = posInt(span, 7);
    return xs.map((_, i) => {
      let s = 0;
      let c = 0;
      for (let j = Math.max(0, i - w + 1); j <= i; j++) if (xs[j] !== null) { s += xs[j]; c++; }
      return c ? clean(s / c) : null;
    });
  }

  function exponential(values, alpha = 0.3) {
    const xs = list(values);
    let a = Number(alpha);
    if (!Number.isFinite(a) || a <= 0 || a > 1) a = 0.3;
    let s = null;
    return xs.map((x) => {
      if (x !== null) s = s === null ? x : a * x + (1 - a) * s;
      return clean(s);
    });
  }

  function linear(values) {
    const xs = list(values);
    const pts = [];
    xs.forEach((y, x) => { if (y !== null) pts.push([x, y]); });
    if (!pts.length) return null;
    const n = pts.length;
    const mx = pts.reduce((s, p) => s + p[0], 0) / n;
    const my = pts.reduce((s, p) => s + p[1], 0) / n;
    let sxy = 0;
    let sxx = 0;
    for (const [x, y] of pts) { sxy += (x - mx) * (y - my); sxx += (x - mx) * (x - mx); }
    const slope = sxx ? sxy / sxx : 0;
    const intercept = my - slope * mx;
    const len = xs.length;
    const next = (count = 1) => {
      const k = Math.floor(Number(count));
      if (!Number.isFinite(k) || k < 1) return [];
      const out = [];
      for (let i = 0; i < Math.min(k, 10000); i++) out.push(clean(intercept + slope * (len + i)));
      return out;
    };
    return { slope: clean(slope), intercept: clean(intercept), next };
  }

  function projectTotal(valuesSoFar, periods) {
    const xs = list(valuesSoFar);
    const p = Number(periods);
    if (!xs.length || !Number.isFinite(p) || p <= 0) return null;
    const sum = xs.reduce((s, x) => s + (x === null ? 0 : x), 0);
    if (xs.length >= p) return clean(sum);
    return clean(sum + (sum / xs.length) * (p - xs.length));
  }

  function seasonal(values, season = 7) {
    const xs = list(values);
    const k = posInt(season, 7);
    const sums = new Array(k).fill(0);
    const counts = new Array(k).fill(0);
    xs.forEach((x, i) => { if (x !== null) { sums[i % k] += x; counts[i % k]++; } });
    return sums.map((s, i) => (counts[i] ? clean(s / counts[i]) : null));
  }

  function growth(values) {
    const xs = list(values);
    return xs.map((x, i) => {
      if (i === 0) return null;
      const prev = xs[i - 1];
      if (x === null || prev === null || prev === 0) return null;
      return clean((x - prev) / Math.abs(prev));
    });
  }

  return { movingAverage, exponential, linear, projectTotal, seasonal, growth };
}

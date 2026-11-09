// title: Budgets
// description: Budgets against spending: how much is left and whether a budget is fine, nearly used up or overspent, per category, and whether the pace of spending will last the period (monthly budgets, project costs, allowances).
// api: status(budget, spent, {near = 0.8}) -> {budget, spent, left, pct, state: 'ok' | 'near' | 'over'} or null when budget is not a number >= 0;
//   left is budget - spent to the cent (negative when over); pct is spent as a percent of budget, one decimal (null when budget is 0);
//   state is 'over' when spent > budget, 'near' when spent >= near * budget, else 'ok'; a missing spent counts as 0;
//   compare(budgets, rows, {by = 'category', value = 'amount', near = 0.8}) -> [{category, budget, spent, left, pct, state}]:
//   budgets is {category: amount} or [{category, amount}]; rows are spending, by and value are a field name or a function of the row;
//   budgeted categories come first in the order given, then categories with spending but no budget (budget 0, so 'over'), most spent first;
//   rows without a category count as 'uncategorized';
//   pace(budget, spent, {from, to, today, near = 0.8}) -> {budget, spent, left, days, daysLeft, elapsed, expected, projected, perDay, state} or null:
//   from and to are the period's first and last 'YYYY-MM-DD' (inclusive), today defaults to the local date and counts as passed;
//   elapsed is the share of the period passed (0..1, three decimals), expected what would be spent by now at an even pace,
//   projected the total at the end at the pace so far, daysLeft the days from today to the end (today included),
//   perDay what may still be spent each of those days (0 when nothing is left), state the status of the projected total
function helper() {
  const pad = (n) => String(n).padStart(2, '0');
  const isDay = (d) => typeof d === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(d);
  const toUTC = (d) => Date.UTC(Number(d.slice(0, 4)), Number(d.slice(5, 7)) - 1, Number(d.slice(8, 10)));
  const localToday = () => { const d = new Date(); return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`; };
  const diffDays = (a, b) => Math.round((toUTC(b) - toUTC(a)) / 86400000);
  const num = (v) => {
    if (typeof v === 'number') return isFinite(v) ? v : null;
    if (typeof v === 'string' && v.trim() !== '' && isFinite(Number(v))) return Number(v);
    return null;
  };
  const round = (n, p = 2) => { const f = Math.pow(10, p); return (Math.sign(n) * Math.round(Math.abs(n) * f + 1e-9) / f) || 0; };
  const getter = (by, fallback) => (typeof by === 'function' ? by : (row) => (row == null ? undefined : row[by == null ? fallback : by]));
  const nearOf = (near) => { const n = num(near); return n === null || n < 0 ? 0.8 : n; };

  function status(budget, spent, { near } = {}) {
    const b = num(budget);
    if (b === null || b < 0) return null;
    const s = num(spent) === null ? 0 : num(spent);
    const limit = nearOf(near);
    const state = s > b ? 'over' : b > 0 && s >= b * limit ? 'near' : 'ok';
    return { budget: round(b), spent: round(s), left: round(b - s), pct: b > 0 ? round((s / b) * 100, 1) : null, state };
  }

  function compare(budgets, rows, { by = 'category', value = 'amount', near } = {}) {
    const planned = new Map();
    if (Array.isArray(budgets)) {
      for (const item of budgets) {
        if (!item || item.category == null) continue;
        const a = num(item.amount);
        if (a !== null && a >= 0) planned.set(String(item.category), a);
      }
    } else if (budgets && typeof budgets === 'object') {
      for (const k of Object.keys(budgets)) {
        const a = num(budgets[k]);
        if (a !== null && a >= 0) planned.set(k, a);
      }
    }
    const keyOf = getter(by, 'category');
    const valOf = getter(value, 'amount');
    const spent = new Map();
    for (const row of Array.isArray(rows) ? rows : []) {
      const v = num(valOf(row));
      if (v === null) continue;
      const k = keyOf(row);
      const key = k == null || k === '' ? 'uncategorized' : String(k);
      spent.set(key, (spent.get(key) || 0) + v);
    }
    const out = [];
    for (const [category, b] of planned) out.push({ category, ...status(b, spent.get(category) || 0, { near }) });
    const extra = [...spent.keys()].filter((k) => !planned.has(k)).sort((a, b) => spent.get(b) - spent.get(a));
    for (const category of extra) out.push({ category, ...status(0, spent.get(category), { near }) });
    return out;
  }

  function pace(budget, spent, { from, to, today, near } = {}) {
    const base = status(budget, spent, { near });
    if (!base || !isDay(from) || !isDay(to)) return null;
    const day = isDay(today) ? today : localToday();
    const days = diffDays(from, to) + 1;
    if (days < 1) return null;
    const d = diffDays(from, day);
    const passed = Math.min(Math.max(d + 1, 0), days);
    const daysLeft = Math.min(Math.max(days - d, 0), days);
    const share = passed / days;
    const b = num(budget);
    const s = base.spent;
    const projected = share > 0 ? s / share : s;
    return {
      budget: base.budget,
      spent: s,
      left: base.left,
      days,
      daysLeft,
      elapsed: round(share, 3),
      expected: round(b * share),
      projected: round(projected),
      perDay: daysLeft > 0 ? round(Math.max(b - s, 0) / daysLeft) : 0,
      state: status(b, projected, { near }).state,
    };
  }

  return { status, compare, pace };
}

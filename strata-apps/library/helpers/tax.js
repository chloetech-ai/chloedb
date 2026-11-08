// title: Tax and tips
// description: Prices with tax, discounts and tips, to the cent: adding or taking out sales tax or VAT, a percent or amount off, tip choices for a bill, and a receipt's totals with tax per rate.
// api: addTax(net, ratePct = 0) -> {net, tax, gross} with the tax added on top of net; null when net or the rate is not a number (or the rate is negative);
//   removeTax(gross, ratePct = 0) -> {net, tax, gross} for a price that already includes the tax (net + tax is exactly gross); null as above;
//   discount(amount, {pct = 0, off = 0}) -> {amount, saved}: pct percent of the amount and off taken together, never more than the amount
//   (pct is held to 0..100, off below 0 counts as 0, nothing is saved on an amount at or below 0); null when amount is not a number;
//   tip(amount, pct) -> the tip as a number; null when either is not a number or is negative;
//   tipOptions(amount, pcts = [15, 18, 20]) -> [{pct, tip, total}] in the order given, odd percents left out; [] when amount is not a number or is negative;
//   total(lines, {discount}) -> {subtotal, discount, tax, total, byRate: [{rate, base, tax}]}: lines are [{amount (before tax), taxRate (percent, default 0)}],
//   lines without a numeric amount are left out; discount is {pct, off} as for discount(), or a number for an amount off; it comes off the subtotal
//   before tax, shared over the rates in proportion to their amounts; tax is worked out once per rate on its discounted base; byRate is lowest rate first;
//   amounts may be numbers or numeric text; every amount returned is rounded to the cent (halves away from zero)
function helper() {
  const num = (x) => {
    if (typeof x === 'number') return isFinite(x) ? x : null;
    if (typeof x === 'string' && x.trim() !== '') { const n = Number(x.trim()); return isFinite(n) ? n : null; }
    return null;
  };
  // round to a whole number, halves away from zero, ignoring float noise (1.005 * 100 = 100.49999...)
  const ri = (v) => { const s = v < 0 ? -1 : 1; return (s * Math.round(Number(Math.abs(v).toFixed(6)))) || 0; };
  const cents = (x) => ri(x * 100);
  const money = (c) => (c || 0) / 100;
  const rateOf = (r) => {
    if (r === undefined || r === null || r === '') return 0;
    const n = num(r);
    return n === null || n < 0 ? null : n;
  };

  function addTax(net, ratePct) {
    const n = num(net);
    const r = rateOf(ratePct);
    if (n === null || r === null) return null;
    const nc = cents(n);
    const tc = ri((nc * r) / 100);
    return { net: money(nc), tax: money(tc), gross: money(nc + tc) };
  }

  function removeTax(gross, ratePct) {
    const g = num(gross);
    const r = rateOf(ratePct);
    if (g === null || r === null) return null;
    const gc = cents(g);
    const nc = ri((gc * 100) / (100 + r));
    return { net: money(nc), tax: money(gc - nc), gross: money(gc) };
  }

  function savedCents(ac, opts) {
    const o = opts && typeof opts === 'object' ? opts : {};
    if (ac <= 0) return 0;
    const pct = Math.min(100, Math.max(0, num(o.pct) || 0));
    const off = Math.max(0, num(o.off) || 0);
    return Math.min(ac, ri((ac * pct) / 100) + cents(off));
  }

  function discount(amount, opts) {
    const a = num(amount);
    if (a === null) return null;
    const ac = cents(a);
    const sc = savedCents(ac, opts);
    return { amount: money(ac - sc), saved: money(sc) };
  }

  function tip(amount, pct) {
    const a = num(amount);
    const p = num(pct);
    if (a === null || p === null || a < 0 || p < 0) return null;
    return money(ri((cents(a) * p) / 100));
  }

  function tipOptions(amount, pcts) {
    const a = num(amount);
    if (a === null || a < 0) return [];
    const list = Array.isArray(pcts) ? pcts : [15, 18, 20];
    const ac = cents(a);
    const out = [];
    for (const raw of list) {
      const p = num(raw);
      if (p === null || p < 0) continue;
      const tc = ri((ac * p) / 100);
      out.push({ pct: p, tip: money(tc), total: money(ac + tc) });
    }
    return out;
  }

  function total(lines, { discount: disc } = {}) {
    const groups = new Map();
    let sub = 0;
    for (const line of Array.isArray(lines) ? lines : []) {
      if (!line || typeof line !== 'object') continue;
      const a = num(line.amount);
      const r = rateOf(line.taxRate);
      if (a === null || r === null) continue;
      const c = cents(a);
      sub += c;
      groups.set(r, (groups.get(r) || 0) + c);
    }
    const opts = typeof disc === 'number' || typeof disc === 'string' ? { off: disc } : disc;
    const sc = disc ? savedCents(sub, opts) : 0;
    const rates = [...groups.keys()].sort((a, b) => a - b);
    const weights = rates.map((r) => Math.max(0, groups.get(r)));
    const W = weights.reduce((s, w) => s + w, 0);
    const raw = weights.map((w) => (W ? (sc * w) / W : 0));
    const share = raw.map(Math.floor);
    let left = sc - share.reduce((s, x) => s + x, 0);
    const order = raw.map((v, i) => [v - share[i], i]).sort((a, b) => b[0] - a[0] || a[1] - b[1]);
    for (let k = 0; left > 0 && order.length; k = (k + 1) % order.length, left--) share[order[k][1]]++;
    let taxSum = 0;
    const byRate = rates.map((r, i) => {
      const base = groups.get(r) - share[i];
      const tc = ri((base * r) / 100);
      taxSum += tc;
      return { rate: r, base: money(base), tax: money(tc) };
    });
    return { subtotal: money(sub), discount: money(sc), tax: money(taxSum), total: money(sub - sc + taxSum), byRate };
  }

  return { addTax, removeTax, discount, tip, tipOptions, total };
}

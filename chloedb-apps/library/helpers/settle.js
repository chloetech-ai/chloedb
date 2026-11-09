// title: Settling up
// description: Shared expenses between people (trips, roommates, group dinners): each person's balance, what each paid and owes, and the fewest payments that make everyone even, all to the cent.
// api: balances(expenses) -> {person: net} where a positive net means the others owe them money, a negative net means they owe;
//   perPerson(expenses) -> {person: {paid, owes, net}} (net = paid - owes);
//   settle(balances) -> the fewest payments [{from, to, amount}] that make everyone even, largest first (from pays to);
//   balances may be balances()'s answer or perPerson()'s (each value's net is used);
//   expenses: [{paidBy, amount, split}], split: [people] for equal shares, or {person: share} with shares as weights ({Ann: 2, Ben: 1} is two thirds and a third),
//   or missing for everyone named anywhere in the expenses; a negative amount is a refund, shared the same way;
//   amounts are kept in whole cents: each expense's shares add up to it exactly, and the odd cents go to whoever has had the fewest odd cents so far;
//   expenses with no payer, no amount or nobody to share it are left out; people come out in the order they first appear
function helper() {
  const EXACT = 15; // up to this many people with a balance, settle searches for the true fewest payments
  const key = (p) => {
    if (p == null || typeof p === 'object') return null;
    const s = String(p).trim();
    return s || null;
  };
  const cents = (v) => {
    const n = typeof v === 'number' ? v : typeof v === 'string' && v.trim() !== '' ? Number(v) : NaN;
    if (!Number.isFinite(n)) return null;
    const c = Math.round(Math.abs(n) * 100 + 1e-7);
    return n < 0 ? -c : c;
  };
  const units = (c) => (c ? c / 100 : 0);
  const isMap = (x) => x && typeof x === 'object' && !Array.isArray(x);

  function parts(split) {
    const out = new Map();
    if (Array.isArray(split)) {
      for (const p of split) { const k = key(p); if (k) out.set(k, 1); }
    } else if (isMap(split)) {
      for (const p of Object.keys(split)) {
        const k = key(p);
        const w = Number(split[p]);
        if (k && Number.isFinite(w) && w > 0) out.set(k, (out.get(k) || 0) + w);
      }
    }
    return [...out];
  }

  // split c cents by weight; leftover cents go to the largest fractions, then to whoever has had the fewest extra cents
  function allocate(c, ps, extra) {
    const sign = c < 0 ? -1 : 1;
    const abs = Math.abs(c);
    const total = ps.reduce((s, p) => s + p[1], 0);
    const shares = ps.map(([, w]) => { const x = (abs * w) / total; const b = Math.floor(x); return { b, f: x - b }; });
    let left = abs - shares.reduce((s, x) => s + x.b, 0);
    const burden = (i) => extra.get(ps[i][0]) || 0;
    const order = ps.map((_, i) => i).sort((i, j) => {
      const d = shares[j].f - shares[i].f;
      return (Math.abs(d) > 1e-9 ? d : 0) || (burden(i) - burden(j)) * sign || i - j;
    });
    for (let k = 0; left > 0; k = (k + 1) % order.length, left--) {
      const i = order[k];
      shares[i].b++;
      extra.set(ps[i][0], burden(i) + sign);
    }
    return shares.map((s) => s.b * sign);
  }

  function tally(expenses) {
    const list = (Array.isArray(expenses) ? expenses : []).filter((e) => isMap(e) && key(e.paidBy) && cents(e.amount));
    const everyone = [];
    const seen = new Set();
    const meet = (p) => { const k = key(p); if (k && !seen.has(k)) { seen.add(k); everyone.push(k); } };
    for (const e of list) {
      meet(e.paidBy);
      if (Array.isArray(e.split)) e.split.forEach(meet);
      else if (isMap(e.split)) Object.keys(e.split).forEach(meet);
    }
    const rows = new Map();
    const extra = new Map();
    const row = (k) => { if (!rows.has(k)) rows.set(k, { paid: 0, owes: 0 }); return rows.get(k); };
    for (const e of list) {
      const c = cents(e.amount);
      const ps = e.split == null ? everyone.map((p) => [p, 1]) : parts(e.split);
      if (!ps.length) continue;
      row(key(e.paidBy)).paid += c;
      allocate(c, ps, extra).forEach((share, i) => { row(ps[i][0]).owes += share; });
    }
    return everyone.filter((p) => rows.has(p)).map((p) => [p, rows.get(p)]);
  }

  function perPerson(expenses) {
    const out = {};
    for (const [p, r] of tally(expenses)) out[p] = { paid: units(r.paid), owes: units(r.owes), net: units(r.paid - r.owes) };
    return out;
  }
  function balances(expenses) {
    const out = {};
    for (const [p, r] of tally(expenses)) out[p] = units(r.paid - r.owes);
    return out;
  }

  // largest debtor pays largest creditor until one of them is even
  function greedy(list, pays) {
    const cr = list.filter((x) => x[1] > 0).map((x) => [x[0], x[1]]).sort((a, b) => b[1] - a[1]);
    const db = list.filter((x) => x[1] < 0).map((x) => [x[0], -x[1]]).sort((a, b) => b[1] - a[1]);
    let i = 0;
    let j = 0;
    while (i < cr.length && j < db.length) {
      const amount = Math.min(cr[i][1], db[j][1]);
      pays.push({ from: db[j][0], to: cr[i][0], amount });
      cr[i][1] -= amount;
      db[j][1] -= amount;
      if (!cr[i][1]) i++;
      if (!db[j][1]) j++;
    }
  }

  // split the people into as many groups that sum to zero as possible: each group of k settles in k - 1 payments
  function groups(list) {
    const n = list.length;
    const size = 1 << n;
    const sum = new Float64Array(size);
    const dp = new Uint8Array(size);
    for (let m = 1; m < size; m++) {
      const low = m & -m;
      sum[m] = sum[m ^ low] + list[31 - Math.clz32(low)][1];
      let best = 0;
      for (let i = 0; i < n; i++) if (m & (1 << i) && dp[m ^ (1 << i)] > best) best = dp[m ^ (1 << i)];
      dp[m] = best + (sum[m] === 0 ? 1 : 0);
    }
    const order = [];
    for (let m = size - 1; m; ) {
      const z = sum[m] === 0 ? 1 : 0;
      for (let i = 0; i < n; i++) {
        const bit = 1 << i;
        if (m & bit && dp[m ^ bit] + z === dp[m]) { order.push(i); m ^= bit; break; }
      }
    }
    order.reverse();
    const out = [];
    let group = [];
    let run = 0;
    for (const i of order) {
      group.push(list[i]);
      run += list[i][1];
      if (run === 0) { out.push(group); group = []; }
    }
    if (group.length) out.push(group);
    return out;
  }

  function settle(bal) {
    const list = [];
    if (isMap(bal)) {
      for (const k of Object.keys(bal)) {
        const v = bal[k];
        const c = cents(isMap(v) ? v.net : v);
        const name = key(k);
        if (name && c) list.push([name, c]);
      }
    }
    const total = list.reduce((s, x) => s + x[1], 0);
    const pays = [];
    if (total !== 0 || list.length > EXACT) greedy(list, pays);
    else for (const g of groups(list)) greedy(g, pays);
    const cmp = (a, b) => (a < b ? -1 : a > b ? 1 : 0);
    return pays
      .sort((a, b) => b.amount - a.amount || cmp(a.from, b.from) || cmp(a.to, b.to))
      .map((p) => ({ from: p.from, to: p.to, amount: units(p.amount) }));
  }

  return { balances, settle, perPerson };
}

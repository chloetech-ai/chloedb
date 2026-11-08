// title: Ranking
// description: Ranks and ratings: places in a leaderboard with ties, Elo ratings for head-to-head games, and fair scores for votes and star ratings with few reviews (scoreboards, tournaments, best-rated lists).
// api: rank(rows, by, {desc = true, ties = 'competition' | 'dense' | 'ordinal'}) -> copies of the rows sorted, each with rank
//   (competition 1, 2, 2, 4; dense 1, 2, 2, 3; ordinal 1, 2, 3, 4); by is a field name or a function of the row;
//   rows whose value is missing or not a number come last with rank null; equal values keep their input order;
//   expected(ratingA, ratingB) -> the chance from 0 to 1 that A beats B under Elo, or null;
//   elo(ratingA, ratingB, result, {k = 32}) -> {a, b} the new ratings, where result is 1 when A won, 0.5 a draw, 0 when B won
//   (any score from 0 to 1); null when a rating or the result is not valid;
//   wilson(positive, total, {z = 1.96}) -> the lower bound of the share of positive votes, 0 to 1 (0 when total is 0), to sort up/down votes fairly;
//   bayes(average, count, prior, weight = 5) -> the average pulled toward prior as if weight more ratings of prior had been given
//   (prior when count is 0; null when nothing is valid), to sort star ratings with few reviews fairly
function helper() {
  const num = (x) => (typeof x === 'number' ? x : typeof x === 'string' && x.trim() !== '' ? Number(x) : NaN);
  const ok = (x) => typeof x === 'number' && isFinite(x);

  function rank(rows, by, { desc = true, ties = 'competition' } = {}) {
    if (!Array.isArray(rows)) return [];
    const get = typeof by === 'function' ? by : (r) => (r == null ? undefined : r[by]);
    const valid = [];
    const rest = [];
    rows.forEach((row, i) => {
      let v;
      try { v = num(get(row)); } catch (e) { v = NaN; }
      if (ok(v)) valid.push({ row, v, i });
      else rest.push(row);
    });
    valid.sort((x, y) => (x.v === y.v ? x.i - y.i : desc ? y.v - x.v : x.v - y.v));
    const out = [];
    let place = 0;
    let prev;
    valid.forEach((e, idx) => {
      if (ties === 'ordinal') place = idx + 1;
      else if (idx === 0 || e.v !== prev) place = ties === 'dense' ? place + 1 : idx + 1;
      prev = e.v;
      out.push(Object.assign({}, e.row, { rank: place }));
    });
    rest.forEach((row) => out.push(Object.assign({}, row, { rank: null })));
    return out;
  }

  function expected(a, b) {
    const ra = num(a);
    const rb = num(b);
    if (!ok(ra) || !ok(rb)) return null;
    return 1 / (1 + Math.pow(10, (rb - ra) / 400));
  }

  function elo(a, b, result, { k = 32 } = {}) {
    const ra = num(a);
    const rb = num(b);
    const s = num(result);
    const kk = num(k);
    if (!ok(ra) || !ok(rb) || !ok(s) || s < 0 || s > 1 || !ok(kk)) return null;
    const ea = expected(ra, rb);
    return { a: ra + kk * (s - ea), b: rb + kk * ((1 - s) - (1 - ea)) };
  }

  function wilson(positive, total, { z = 1.96 } = {}) {
    const p0 = num(positive);
    const n = num(total);
    const zz = num(z);
    if (!ok(n) || n <= 0 || !ok(p0) || !ok(zz)) return 0;
    const p = Math.min(Math.max(p0, 0), n) / n;
    const z2 = zz * zz;
    const v = (p + z2 / (2 * n) - zz * Math.sqrt((p * (1 - p) + z2 / (4 * n)) / n)) / (1 + z2 / n);
    return Math.min(1, Math.max(0, v));
  }

  function bayes(average, count, prior, weight = 5) {
    const m = num(average);
    const n = num(count);
    const pr = num(prior);
    const w = num(weight);
    const hasPrior = ok(pr) && ok(w) && w >= 0;
    if (!ok(n) || n <= 0 || !ok(m)) return hasPrior ? pr : null;
    if (!hasPrior) return m;
    return (pr * w + m * n) / (w + n);
  }

  return { rank, expected, elo, wilson, bayes };
}

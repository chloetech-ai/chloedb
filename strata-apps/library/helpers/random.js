// title: Random
// description: Randomness that can repeat when seeded: random numbers, picks, weighted picks, shuffles, samples, balanced teams, dice rolls and a daily pick that is the same for everyone (games, raffles, quotes of the day, splitting a group).
// api: rng(seed) -> a function giving a number from 0 (included) to 1 (excluded); the same seed (a number or text) always gives the same sequence; Math.random when seed is missing;
//   every rng? below is such a function, or a seed (number or text) to make one, or missing for Math.random;
//   int(min, max, rng?) -> a whole number from min to max, both included (swapped when min > max; null when either is not a number);
//   pick(items, rng?) -> one item, or null when items is empty or not an array;
//   pickWeighted(items, weightBy = 'weight', rng?) -> one item, chosen in proportion to its weight (weightBy: a field name or a function of the item;
//   weights that are not positive numbers are never picked); null when no item has a weight;
//   shuffle(items, rng?) -> a new array with the same items in random order (the input is left as it is; [] when not an array);
//   sample(items, n, rng?) -> n different items in random order (all of them, shuffled, when n is more than there are; [] for n <= 0);
//   teams(people, n, rng?) -> n groups whose sizes differ by at most one, people shuffled then dealt in turn ([] when n < 1);
//   dice(notation, rng?) -> {total, rolls} for '2d6+1', 'd20', '3d8-2', '1d6+1d4', 'd%' (rolls: each die's value in order;
//   dice after a minus count against the total); null when the notation cannot be read or asks for more than 1000 dice;
//   daily(seed, today = the local date, items?) -> with items, the day's pick from them, the same for everyone on that day and seed;
//   without items, the day's number from 0 to 1
function helper() {
  const pad = (n) => String(n).padStart(2, '0');
  const localToday = () => { const d = new Date(); return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`; };

  // text -> 32-bit seed (xmur3-style mixing)
  function hash(text) {
    let h = 1779033703 ^ text.length;
    for (let i = 0; i < text.length; i++) {
      h = Math.imul(h ^ text.charCodeAt(i), 3432918353);
      h = (h << 13) | (h >>> 19);
    }
    h = Math.imul(h ^ (h >>> 16), 2246822507);
    h = Math.imul(h ^ (h >>> 13), 3266489909);
    return (h ^ (h >>> 16)) >>> 0;
  }
  // mulberry32: small, fast, good enough for games and picks
  function mulberry(a) {
    return function next() {
      a = (a + 0x6d2b79f5) | 0;
      let t = Math.imul(a ^ (a >>> 15), 1 | a);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }

  function rng(seed) {
    if (seed === undefined || seed === null || seed === '') return Math.random;
    if (typeof seed === 'number' && !Number.isFinite(seed)) return Math.random;
    return mulberry(hash(String(seed)));
  }
  const use = (r) => (typeof r === 'function' ? r : rng(r));
  const below = (n, r) => Math.min(n - 1, Math.floor(r() * n));

  function int(min, max, r) {
    let a = Number(min);
    let b = Number(max);
    if (min === null || max === null || min === undefined || max === undefined) return null;
    if (!Number.isFinite(a) || !Number.isFinite(b)) return null;
    if (a > b) [a, b] = [b, a];
    a = Math.ceil(a);
    b = Math.floor(b);
    if (a > b) return null;
    return a + below(b - a + 1, use(r));
  }

  function pick(items, r) {
    if (!Array.isArray(items) || !items.length) return null;
    return items[below(items.length, use(r))];
  }

  function pickWeighted(items, weightBy, r) {
    if (!Array.isArray(items) || !items.length) return null;
    const by = weightBy === undefined || weightBy === null ? 'weight' : weightBy;
    const weightOf = (item) => {
      const w = Number(typeof by === 'function' ? by(item) : item && item[by]);
      return Number.isFinite(w) && w > 0 ? w : 0;
    };
    const weights = items.map(weightOf);
    const total = weights.reduce((s, w) => s + w, 0);
    if (!(total > 0)) return null;
    let x = use(r)() * total;
    let last = null;
    for (let i = 0; i < items.length; i++) {
      if (!weights[i]) continue;
      last = items[i];
      if (x < weights[i]) return items[i];
      x -= weights[i];
    }
    return last; // rounding at the very top end
  }

  function shuffle(items, r) {
    if (!Array.isArray(items)) return [];
    const out = items.slice();
    const f = use(r);
    for (let i = out.length - 1; i > 0; i--) {
      const j = below(i + 1, f);
      [out[i], out[j]] = [out[j], out[i]];
    }
    return out;
  }

  function sample(items, n, r) {
    if (!Array.isArray(items)) return [];
    const k = Math.min(items.length, Math.floor(Number(n)));
    if (!(k > 0)) return [];
    const pool = items.slice();
    const f = use(r);
    for (let i = 0; i < k; i++) {
      const j = i + below(pool.length - i, f);
      [pool[i], pool[j]] = [pool[j], pool[i]];
    }
    return pool.slice(0, k);
  }

  function teams(people, n, r) {
    const count = Math.floor(Number(n));
    if (!(count >= 1) || count > 10000) return [];
    const groups = Array.from({ length: count }, () => []);
    shuffle(Array.isArray(people) ? people : [], r).forEach((p, i) => groups[i % count].push(p));
    return groups;
  }

  function dice(notation, r) {
    if (typeof notation !== 'string' && typeof notation !== 'number') return null;
    const s = String(notation).replace(/\s+/g, '').toLowerCase();
    const TERM = '(?:\\d*d(?:\\d+|%)|\\d+)';
    if (!s || !new RegExp(`^[+-]?${TERM}(?:[+-]${TERM})*$`).test(s)) return null;
    const terms = [];
    let dieCount = 0;
    const re = /([+-]?)(?:(\d*)d(\d+|%)|(\d+))/g;
    let m;
    while ((m = re.exec(s))) {
      const sign = m[1] === '-' ? -1 : 1;
      if (m[4] !== undefined) { terms.push({ sign, flat: Number(m[4]) }); continue; }
      const times = m[2] === '' ? 1 : Number(m[2]);
      const sides = m[3] === '%' ? 100 : Number(m[3]);
      if (times < 1 || sides < 1 || sides > 1e9) return null;
      dieCount += times;
      if (dieCount > 1000) return null;
      terms.push({ sign, times, sides });
    }
    const f = use(r);
    const rolls = [];
    let total = 0;
    for (const t of terms) {
      if (t.flat !== undefined) { total += t.sign * t.flat; continue; }
      for (let i = 0; i < t.times; i++) {
        const v = 1 + below(t.sides, f);
        rolls.push(v);
        total += t.sign * v;
      }
    }
    return { total, rolls };
  }

  function daily(seed, today, items) {
    const day = typeof today === 'string' && /^\d{4}-\d{2}-\d{2}/.test(today) ? today.slice(0, 10) : localToday();
    const f = mulberry(hash(`${seed === undefined || seed === null ? '' : String(seed)}|${day}`));
    if (items === undefined) return f();
    return pick(items, f);
  }

  return { rng, int, pick, pickWeighted, shuffle, sample, teams, dice, daily };
}

// title: Lists
// description: Everyday array tools for rows and values: sorting by several keys (stable, nulls last), natural order of names with numbers, removing duplicates, indexing, splitting, chunking, moving an item, number ranges, zipping and compacting.
// api: by (in sortBy, uniqBy, indexBy, partition) is a field name or a function of the row (row, index); missing means the value itself;
//   sortBy(rows, keys) -> a new array, stable; keys is one key or a list: 'field' ascending, '-field' descending, or a function (row) -> value, ascending;
//   values compare as numbers, text (plain character order), booleans (false first); null, undefined and NaN always sort last, in either direction;
//   naturalCompare(a, b) -> -1 | 0 | 1, for array.sort: digits compare as numbers ('file2' before 'file10'), letters ignore case, null and undefined last;
//   uniqBy(rows, by?) -> the rows whose key was not seen before, the first of each kept, in order (1 and '1' are different keys);
//   indexBy(rows, by) -> {key: row}, the last row wins for a key; rows whose key is null or undefined are left out;
//   partition(rows, test) -> [rows where test is truthy, the rest], test being a function (row, index) or a field name;
//   chunk(rows, n) -> [[...n rows], ...], the last one shorter ([] when n < 1);
//   move(rows, from, to) -> a new array with the item at from moved to to (negative indexes count from the end; to is clamped; an unchanged copy when from is out of range);
//   range(n) -> [0, 1, ..., n - 1]; range(from, to, step = 1, or -1 when to < from) -> from up to but not including to (at most 100000 numbers; [] when step is 0 or points away);
//   zip(a, b, ...) -> [[a0, b0], [a1, b1], ...] as long as the shortest array;
//   compact(rows) -> the rows without null, undefined, '' and false (0 and NaN stay)
function helper() {
  const list = (rows) => (Array.isArray(rows) ? rows : []);
  const getter = (by) => {
    if (typeof by === 'function') return by;
    if (by == null || by === '') return (row) => row;
    const key = String(by);
    return (row) => (row == null ? undefined : row[key]);
  };
  const missing = (v) => v == null || (typeof v === 'number' && isNaN(v));
  const RANK = { number: 0, string: 1, object: 2 };

  // compares two present values, ascending
  function compareValues(a, b) {
    if (typeof a === 'boolean') a = a ? 1 : 0;
    if (typeof b === 'boolean') b = b ? 1 : 0;
    if (a instanceof Date) a = a.getTime();
    if (b instanceof Date) b = b.getTime();
    const ta = typeof a;
    const tb = typeof b;
    if (ta !== tb) {
      const ra = ta in RANK ? RANK[ta] : 3;
      const rb = tb in RANK ? RANK[tb] : 3;
      if (ra !== rb) return ra < rb ? -1 : 1;
      a = String(a);
      b = String(b);
    } else if (ta !== 'number' && ta !== 'string') {
      a = String(a);
      b = String(b);
    }
    return a < b ? -1 : a > b ? 1 : 0;
  }

  function sortBy(rows, keys) {
    const items = list(rows);
    const specs = (Array.isArray(keys) ? keys : keys == null ? [null] : [keys]).map((k) => {
      if (typeof k === 'string' && (k[0] === '-' || k[0] === '+')) return { get: getter(k.slice(1)), dir: k[0] === '-' ? -1 : 1 };
      return { get: getter(k), dir: 1 };
    });
    if (!specs.length) specs.push({ get: getter(null), dir: 1 });
    const decorated = items.map((row, i) => ({ row, i, vals: specs.map((s) => s.get(row, i)) }));
    decorated.sort((x, y) => {
      for (let k = 0; k < specs.length; k++) {
        const a = x.vals[k];
        const b = y.vals[k];
        const ma = missing(a);
        const mb = missing(b);
        if (ma || mb) {
          if (ma && mb) continue;
          return ma ? 1 : -1;
        }
        const c = compareValues(a, b);
        if (c) return c * specs[k].dir;
      }
      return x.i - y.i;
    });
    return decorated.map((d) => d.row);
  }

  function naturalCompare(a, b) {
    const ma = a == null;
    const mb = b == null;
    if (ma || mb) return ma === mb ? 0 : ma ? 1 : -1;
    const x = String(a);
    const y = String(b);
    const ta = x.match(/\d+|\D+/g) || [];
    const tb = y.match(/\d+|\D+/g) || [];
    const n = Math.min(ta.length, tb.length);
    for (let i = 0; i < n; i++) {
      const p = ta[i];
      const q = tb[i];
      const dp = p.charCodeAt(0) >= 48 && p.charCodeAt(0) <= 57;
      const dq = q.charCodeAt(0) >= 48 && q.charCodeAt(0) <= 57;
      if (dp && dq) {
        // compare as numbers without losing precision on long digit runs
        const p2 = p.replace(/^0+/, '');
        const q2 = q.replace(/^0+/, '');
        if (p2.length !== q2.length) return p2.length < q2.length ? -1 : 1;
        if (p2 !== q2) return p2 < q2 ? -1 : 1;
      } else if (dp !== dq) {
        return dp ? -1 : 1;
      } else {
        const pl = p.toLowerCase();
        const ql = q.toLowerCase();
        if (pl !== ql) return pl < ql ? -1 : 1;
      }
    }
    if (ta.length !== tb.length) return ta.length < tb.length ? -1 : 1;
    return x < y ? -1 : x > y ? 1 : 0;
  }

  function uniqBy(rows, by) {
    const get = getter(by);
    const seen = new Map();
    const out = [];
    list(rows).forEach((row, i) => {
      const k = get(row, i);
      if (seen.has(k)) return;
      seen.set(k, true);
      out.push(row);
    });
    return out;
  }

  function indexBy(rows, by) {
    const get = getter(by);
    const out = {};
    list(rows).forEach((row, i) => {
      const k = get(row, i);
      if (k == null) return;
      const key = String(k);
      if (key === '__proto__') return;
      out[key] = row;
    });
    return out;
  }

  function partition(rows, test) {
    const get = getter(test);
    const yes = [];
    const no = [];
    list(rows).forEach((row, i) => (get(row, i) ? yes : no).push(row));
    return [yes, no];
  }

  function chunk(rows, n) {
    const size = Math.floor(Number(n));
    if (!(size >= 1)) return [];
    const items = list(rows);
    const out = [];
    for (let i = 0; i < items.length; i += size) out.push(items.slice(i, i + size));
    return out;
  }

  function move(rows, from, to) {
    const out = list(rows).slice();
    const len = out.length;
    let f = Math.trunc(Number(from));
    let t = Math.trunc(Number(to));
    if (isNaN(f) || isNaN(t) || !len) return out;
    if (f < 0) f += len;
    if (f < 0 || f >= len) return out;
    if (t < 0) t += len;
    t = Math.max(0, Math.min(len - 1, t));
    const [item] = out.splice(f, 1);
    out.splice(t, 0, item);
    return out;
  }

  function range(from, to, step) {
    if (to === undefined) { to = from; from = 0; }
    from = Number(from);
    to = Number(to);
    if (!isFinite(from) || !isFinite(to)) return [];
    step = step === undefined ? (to < from ? -1 : 1) : Number(step);
    if (!isFinite(step) || step === 0) return [];
    const count = Math.ceil((to - from) / step);
    if (!(count > 0)) return [];
    const out = [];
    for (let i = 0; i < Math.min(count, 100000); i++) out.push(from + i * step);
    return out;
  }

  function zip(...arrays) {
    if (!arrays.length || !arrays.every(Array.isArray)) return [];
    const n = Math.min(...arrays.map((a) => a.length));
    const out = [];
    for (let i = 0; i < n; i++) out.push(arrays.map((a) => a[i]));
    return out;
  }

  function compact(rows) {
    return list(rows).filter((v) => v != null && v !== '' && v !== false);
  }

  return { sortBy, naturalCompare, uniqBy, indexBy, partition, chunk, move, range, zip, compact };
}

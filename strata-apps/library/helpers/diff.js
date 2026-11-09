// title: Differences
// description: What changed between two versions of a text, a list or a record, line by line, word by word or field by field, for edit histories, "what changed" notes, reviewing imports and highlighting edits.
// api: lines(a, b, {ignoreCase = false, ignoreWhitespace = false}) -> [{type: 'same' | 'add' | 'remove', text}], one entry per line in reading order,
//   removals before additions where lines were replaced; a final line break does not make an extra empty line; same lines carry b's text;
//   words(a, b, {ignoreCase = false}) -> [{type, text}] runs of words, spaces and punctuation, adjacent pieces of one type joined
//   (the 'same' and 'remove' texts joined give a, the 'same' and 'add' texts give b);
//   chars(a, b, {ignoreCase = false}) -> [{type, text}] runs of characters, joined the same way;
//   arrays(a, b, by?) -> [{type, value}] one entry per item; by: a field name or a function giving an item's identity (default the whole item, compared deeply);
//   fields(before, after, {ignore = []}) -> [{field, type: 'add' | 'remove' | 'change', from, to}] for the fields that differ
//   (before's order, then fields new in after; a missing side is null; undefined counts as missing; values compared deeply);
//   stats(changes) -> {same, added, removed, changed}: how many entries of each type a result above holds;
//   equal(x, y) -> true when two values are deeply the same (numbers, text, arrays, plain objects);
//   a and b that are not text count as empty (numbers and booleans are read as text); very large inputs fall back to removing and adding the changed middle whole
function helper() {
  const LIMIT = 1000000;
  const str = (x) => (typeof x === 'string' ? x : typeof x === 'number' || typeof x === 'boolean' ? String(x) : '');

  function equal(x, y) {
    if (x === y) return true;
    if (typeof x === 'number' && typeof y === 'number') return x !== x && y !== y;
    if (!x || !y || typeof x !== 'object' || typeof y !== 'object') return false;
    if (x instanceof Date || y instanceof Date) return x instanceof Date && y instanceof Date && x.getTime() === y.getTime();
    if (Array.isArray(x) !== Array.isArray(y)) return false;
    if (Array.isArray(x)) {
      if (x.length !== y.length) return false;
      for (let i = 0; i < x.length; i++) if (!equal(x[i], y[i])) return false;
      return true;
    }
    const kx = Object.keys(x).filter((k) => x[k] !== undefined);
    const ky = Object.keys(y).filter((k) => y[k] !== undefined);
    if (kx.length !== ky.length) return false;
    for (const k of kx) if (!equal(x[k], y[k])) return false;
    return true;
  }

  // core: entries [type, itemOfA, itemOfB]
  function core(a, b, eq) {
    const out = [];
    const n0 = a.length;
    const m0 = b.length;
    let s = 0;
    while (s < n0 && s < m0 && eq(a[s], b[s])) s++;
    let ea = n0;
    let eb = m0;
    while (ea > s && eb > s && eq(a[ea - 1], b[eb - 1])) { ea--; eb--; }
    for (let i = 0; i < s; i++) out.push(['same', a[i], b[i]]);
    const n = ea - s;
    const m = eb - s;
    const mid = [];
    if (n && m && n * m <= LIMIT) {
      const L = [];
      for (let i = 0; i <= n; i++) L.push(new Int32Array(m + 1));
      for (let i = n - 1; i >= 0; i--) {
        for (let j = m - 1; j >= 0; j--) {
          L[i][j] = eq(a[s + i], b[s + j]) ? L[i + 1][j + 1] + 1 : Math.max(L[i + 1][j], L[i][j + 1]);
        }
      }
      let i = 0;
      let j = 0;
      while (i < n && j < m) {
        if (eq(a[s + i], b[s + j])) { mid.push(['same', a[s + i], b[s + j]]); i++; j++; }
        else if (L[i + 1][j] >= L[i][j + 1]) { mid.push(['remove', a[s + i], null]); i++; }
        else { mid.push(['add', null, b[s + j]]); j++; }
      }
      for (; i < n; i++) mid.push(['remove', a[s + i], null]);
      for (; j < m; j++) mid.push(['add', null, b[s + j]]);
    } else {
      for (let i = 0; i < n; i++) mid.push(['remove', a[s + i], null]);
      for (let j = 0; j < m; j++) mid.push(['add', null, b[s + j]]);
    }
    // within each changed block, removals come before additions
    let rem = [];
    let add = [];
    const flush = () => {
      for (const e of rem) out.push(e);
      for (const e of add) out.push(e);
      rem = [];
      add = [];
    };
    for (const e of mid) {
      if (e[0] === 'same') { flush(); out.push(e); }
      else (e[0] === 'remove' ? rem : add).push(e);
    }
    flush();
    for (let i = ea; i < n0; i++) out.push(['same', a[i], b[i - ea + eb]]);
    return out;
  }

  function textDiff(ta, tb, key, merge) {
    const A = ta.map((t) => ({ t, k: key(t) }));
    const B = tb.map((t) => ({ t, k: key(t) }));
    const out = [];
    for (const [type, x, y] of core(A, B, (p, q) => p.k === q.k)) {
      const text = type === 'remove' ? x.t : y.t;
      const last = out[out.length - 1];
      if (merge && last && last.type === type) last.text += text;
      else out.push({ type, text });
    }
    return out;
  }

  function splitLines(t) {
    t = str(t);
    if (!t) return [];
    const ls = t.split(/\r\n|\r|\n/);
    if (ls[ls.length - 1] === '') ls.pop();
    return ls;
  }

  function lines(a, b, { ignoreCase = false, ignoreWhitespace = false } = {}) {
    const key = (s) => {
      if (ignoreWhitespace) s = s.trim().replace(/\s+/g, ' ');
      return ignoreCase ? s.toLowerCase() : s;
    };
    return textDiff(splitLines(a), splitLines(b), key, false);
  }

  const TOKEN = /\s+|[^\s!-\/:-@\[-`{-~]+|[^\s]/g;
  function words(a, b, { ignoreCase = false } = {}) {
    const tok = (t) => str(t).match(TOKEN) || [];
    return textDiff(tok(a), tok(b), ignoreCase ? (s) => s.toLowerCase() : (s) => s, true);
  }

  function chars(a, b, { ignoreCase = false } = {}) {
    return textDiff(Array.from(str(a)), Array.from(str(b)), ignoreCase ? (s) => s.toLowerCase() : (s) => s, true);
  }

  function arrays(a, b, by) {
    const A = Array.isArray(a) ? a : [];
    const B = Array.isArray(b) ? b : [];
    const key = typeof by === 'function' ? by
      : typeof by === 'string' ? (x) => (x == null ? x : x[by])
        : (x) => x;
    return core(A, B, (x, y) => equal(key(x), key(y)))
      .map(([type, x, y]) => ({ type, value: type === 'remove' ? x : y }));
  }

  function fields(before, after, { ignore = [] } = {}) {
    const obj = (x) => (x && typeof x === 'object' && !Array.isArray(x) ? x : {});
    const A = obj(before);
    const B = obj(after);
    const skip = new Set(Array.isArray(ignore) ? ignore : []);
    const keys = Object.keys(A);
    for (const k of Object.keys(B)) if (!(k in A)) keys.push(k);
    const out = [];
    for (const field of keys) {
      if (skip.has(field)) continue;
      const ha = A[field] !== undefined;
      const hb = B[field] !== undefined;
      if (!ha && !hb) continue;
      if (!ha) out.push({ field, type: 'add', from: null, to: B[field] });
      else if (!hb) out.push({ field, type: 'remove', from: A[field], to: null });
      else if (!equal(A[field], B[field])) out.push({ field, type: 'change', from: A[field], to: B[field] });
    }
    return out;
  }

  function stats(changes) {
    const r = { same: 0, added: 0, removed: 0, changed: 0 };
    for (const c of Array.isArray(changes) ? changes : []) {
      const t = c && c.type;
      if (t === 'same') r.same++;
      else if (t === 'add') r.added++;
      else if (t === 'remove') r.removed++;
      else if (t === 'change') r.changed++;
    }
    return r;
  }

  return { lines, words, chars, arrays, fields, stats, equal };
}

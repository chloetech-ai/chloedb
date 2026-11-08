// title: Objects
// description: Work with plain objects and JSON data: read and write deep paths, deep merge, copy and compare, pick and omit keys, parse JSON safely, and flatten nested settings into dotted keys and back.
// api: get(obj, path, fallback = undefined) -> the value at path, or fallback when any step is missing (a null found at the end is returned as null);
//   path is 'a.b[0].c' (also a['key with.dots']), a number, or an array of keys ['a', 'b', 0, 'c']; an empty path is obj itself; only own properties are followed;
//   set(obj, path, value) -> a new object with value at path; the original is untouched (only the objects along the path are copied, the rest is shared);
//   missing steps are created as arrays when the next key is a number, else as objects; a '__proto__' step is refused (a shallow copy comes back);
//   merge(a, b) -> a new object: b deep-merged over a (plain objects merge key by key; arrays and every other value from b replace; undefined in b is skipped;
//   when either side is not a plain object the result is a copy of b, or of a when b is undefined);
//   pick(obj, keys) -> {} with only those own keys (keys: an array, or a single key); omit(obj, keys) -> {} with every own key but those;
//   equal(a, b) -> true when deeply equal (arrays, plain objects, Dates by time, RegExps, Maps, Sets, NaN equals NaN; cycles are safe);
//   clone(value) -> a deep copy (arrays, plain objects, Dates, Maps, Sets; other objects such as functions are kept by reference; cycles are kept);
//   safeParse(text, fallback = null) -> the parsed JSON, or fallback when text is not a string or not valid JSON;
//   flatten(obj) -> {path: leaf} with object keys joined by '.' and array indexes as [i] ({a: {b: [1]}} -> {'a.b[0]': 1}); empty objects and arrays stay as leaves;
//   unflatten(flat) -> the nested object (or array when the first path starts with an index) rebuilt from such paths; unflatten(flatten(x)) equals x
//   when no key holds '.', '[' or ']';
//   isEmpty(value) -> true for null, undefined, '', [], {}, an empty Map or Set; false for numbers, booleans and anything else
function helper() {
  const own = (o, k) => Object.prototype.hasOwnProperty.call(o, k);
  const tag = (v) => Object.prototype.toString.call(v);
  // works for objects made in another frame too: their Object.prototype differs from ours
  const isPlain = (v) => {
    if (v === null || typeof v !== 'object' || tag(v) !== '[object Object]') return false;
    const p = Object.getPrototypeOf(v);
    return p === null || Object.getPrototypeOf(p) === null;
  };
  const isDate = (v) => tag(v) === '[object Date]';
  const isRegExp = (v) => tag(v) === '[object RegExp]';
  const isMap = (v) => tag(v) === '[object Map]';
  const isSet = (v) => tag(v) === '[object Set]';
  const blank = (v) => (Object.getPrototypeOf(v) === null ? Object.create(null) : {});

  function parsePath(path) {
    if (Array.isArray(path)) return path;
    if (typeof path === 'number') return [path];
    if (typeof path !== 'string' || path === '') return [];
    const out = [];
    const re = /\[(\d+)\]|\[(["'])(.*?)\2\]|([^.[\]]+)/g;
    let m;
    while ((m = re.exec(path))) out.push(m[1] !== undefined ? Number(m[1]) : m[3] !== undefined ? m[3] : m[4]);
    return out;
  }

  function get(obj, path, fallback) {
    const keys = parsePath(path);
    let cur = obj;
    for (const k of keys) {
      if (cur === null || cur === undefined) return fallback;
      if (typeof cur !== 'object' && typeof cur !== 'string') return fallback;
      if (!own(cur, k)) return fallback;
      cur = cur[k];
    }
    return cur === undefined ? fallback : cur;
  }

  function setAt(node, keys, i, value) {
    const k = keys[i];
    let copy;
    if (Array.isArray(node)) copy = node.slice();
    else if (isPlain(node)) copy = Object.assign(blank(node), node);
    else copy = typeof k === 'number' ? [] : {};
    copy[k] = i === keys.length - 1 ? value : setAt(copy[k], keys, i + 1, value);
    return copy;
  }
  function set(obj, path, value) {
    const keys = parsePath(path);
    if (!keys.length) return value;
    if (keys.some((k) => String(k) === '__proto__')) {
      return Array.isArray(obj) ? obj.slice() : isPlain(obj) ? Object.assign({}, obj) : {};
    }
    return setAt(obj, keys, 0, value);
  }

  function cloneIn(v, seen) {
    if (v === null || typeof v !== 'object') return v;
    if (seen.has(v)) return seen.get(v);
    if (isDate(v)) return new Date(v.getTime());
    if (Array.isArray(v)) {
      const out = new Array(v.length);
      seen.set(v, out);
      for (let i = 0; i < v.length; i++) if (i in v) out[i] = cloneIn(v[i], seen);
      return out;
    }
    if (isMap(v)) {
      const out = new Map();
      seen.set(v, out);
      v.forEach((val, key) => out.set(key, cloneIn(val, seen)));
      return out;
    }
    if (isSet(v)) {
      const out = new Set();
      seen.set(v, out);
      v.forEach((val) => out.add(cloneIn(val, seen)));
      return out;
    }
    if (!isPlain(v)) return v;
    const out = blank(v);
    seen.set(v, out);
    for (const k of Object.keys(v)) if (k !== '__proto__') out[k] = cloneIn(v[k], seen);
    return out;
  }
  const clone = (value) => cloneIn(value, new Map());

  function merge(a, b) {
    if (b === undefined) return clone(a);
    if (!isPlain(a) || !isPlain(b)) return clone(b);
    const out = clone(a);
    for (const k of Object.keys(b)) {
      if (k === '__proto__' || b[k] === undefined) continue;
      out[k] = isPlain(out[k]) && isPlain(b[k]) ? merge(out[k], b[k]) : clone(b[k]);
    }
    return out;
  }

  const keyList = (keys) => (Array.isArray(keys) ? keys : keys === undefined || keys === null ? [] : [keys]).map(String);
  function pick(obj, keys) {
    const out = {};
    if (obj === null || typeof obj !== 'object') return out;
    for (const k of keyList(keys)) if (k !== '__proto__' && own(obj, k)) out[k] = obj[k];
    return out;
  }
  function omit(obj, keys) {
    const out = {};
    if (obj === null || typeof obj !== 'object') return out;
    const drop = new Set(keyList(keys));
    for (const k of Object.keys(obj)) if (k !== '__proto__' && !drop.has(k)) out[k] = obj[k];
    return out;
  }

  function eq(a, b, seen) {
    if (a === b) return true;
    if (typeof a === 'number' && typeof b === 'number') return a !== a && b !== b;
    if (!a || !b || typeof a !== 'object' || typeof b !== 'object') return false;
    if (tag(a) !== tag(b)) return false;
    if (Array.isArray(a) !== Array.isArray(b)) return false;
    const pa = isPlain(a);
    if (pa !== isPlain(b)) return false;
    if (!pa && !Array.isArray(a) && tag(a) === '[object Object]' && Object.getPrototypeOf(a) !== Object.getPrototypeOf(b)) return false;
    if (isDate(a)) {
      const x = a.getTime();
      const y = b.getTime();
      return x === y || (x !== x && y !== y);
    }
    if (isRegExp(a)) return String(a) === String(b);
    for (const [x, y] of seen) if (x === a && y === b) return true;
    seen.push([a, b]);
    if (Array.isArray(a)) {
      if (a.length !== b.length) return false;
      for (let i = 0; i < a.length; i++) if (!eq(a[i], b[i], seen)) return false;
      return true;
    }
    if (isMap(a)) {
      if (a.size !== b.size) return false;
      let ok = true;
      a.forEach((v, k) => { if (ok && (!b.has(k) || !eq(v, b.get(k), seen))) ok = false; });
      return ok;
    }
    if (isSet(a)) {
      if (a.size !== b.size) return false;
      let ok = true;
      a.forEach((v) => { if (ok && !b.has(v)) ok = false; });
      return ok;
    }
    const ka = Object.keys(a);
    const kb = Object.keys(b);
    if (ka.length !== kb.length) return false;
    for (const k of ka) if (!own(b, k) || !eq(a[k], b[k], seen)) return false;
    return true;
  }
  const equal = (a, b) => eq(a, b, []);

  function safeParse(text, fallback = null) {
    if (typeof text !== 'string') return fallback;
    try { return JSON.parse(text); } catch (e) { return fallback; }
  }

  function flatten(obj) {
    const out = {};
    if (obj === null || typeof obj !== 'object') return out;
    const walk = (v, prefix) => {
      const arr = Array.isArray(v);
      if (!arr && !isPlain(v)) { out[prefix] = v; return; }
      const keys = arr ? Array.from(v, (_, i) => i) : Object.keys(v).filter((k) => k !== '__proto__');
      if (!keys.length) { if (prefix) out[prefix] = arr ? [] : {}; return; }
      for (const k of keys) walk(v[k], arr ? `${prefix}[${k}]` : prefix ? `${prefix}.${k}` : k);
    };
    walk(obj, '');
    return out;
  }

  function unflatten(flat) {
    if (!isPlain(flat)) return {};
    let root = null;
    for (const key of Object.keys(flat)) {
      const keys = parsePath(key);
      if (!keys.length || keys.some((k) => String(k) === '__proto__')) continue;
      if (root === null) root = typeof keys[0] === 'number' ? [] : {};
      let node = root;
      for (let i = 0; i < keys.length - 1; i++) {
        const k = keys[i];
        if (node[k] === null || typeof node[k] !== 'object') node[k] = typeof keys[i + 1] === 'number' ? [] : {};
        node = node[k];
      }
      node[keys[keys.length - 1]] = clone(flat[key]);
    }
    return root || {};
  }

  function isEmpty(value) {
    if (value === null || value === undefined) return true;
    if (typeof value === 'string' || Array.isArray(value)) return value.length === 0;
    if (isMap(value) || isSet(value)) return value.size === 0;
    if (isPlain(value)) return Object.keys(value).length === 0;
    return false;
  }

  return { get, set, merge, pick, omit, equal, clone, safeParse, flatten, unflatten, isEmpty };
}

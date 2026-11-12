// Starting data drawn from a seed, so a scenario can say "200 orders over
// the last three weeks" and every session of the same seed gets the same
// rows. A spec is { count, fields: { name: kind } }, each kind one of:
//
//   "seq"                     1, 2, 3, ...
//   { "seq": "o-" }           o-1, o-2, ...
//   { "pick": [...], "weights": [...]? }
//   { "int": [lo, hi] }       { "float": [lo, hi], "digits": 2 }
//   { "daysAgo": [lo, hi] }   a time that many days before now, ISO 8601
//   { "const": value }
//
// Times are relative to the moment the rows are drawn, so a check that asks
// "in the last 7 days" reads the truth from the same sandbox (see checks.mjs).

/** mulberry32: small, fast, and the same everywhere. */
export function rng(seed) {
  let a = (seed >>> 0) || 1;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function pick(r, values, weights) {
  if (!weights) return values[Math.floor(r() * values.length)];
  const total = weights.reduce((a, b) => a + b, 0);
  let x = r() * total;
  for (let i = 0; i < values.length; i++) { x -= weights[i]; if (x < 0) return values[i]; }
  return values.at(-1);
}

function value(kind, i, r, now) {
  if (kind === 'seq') return i + 1;
  if ('seq' in kind) return `${kind.seq}${i + 1}`;
  if ('pick' in kind) return pick(r, kind.pick, kind.weights);
  if ('int' in kind) return kind.int[0] + Math.floor(r() * (kind.int[1] - kind.int[0] + 1));
  if ('float' in kind) return Number((kind.float[0] + r() * (kind.float[1] - kind.float[0])).toFixed(kind.digits ?? 2));
  if ('daysAgo' in kind) {
    const d = kind.daysAgo[0] + r() * (kind.daysAgo[1] - kind.daysAgo[0]);
    return new Date(now - d * 86400000).toISOString();
  }
  if ('const' in kind) return kind.const;
  throw new Error(`unknown fixture field kind: ${JSON.stringify(kind)}`);
}

/** The rows of a spec for a seed. */
export function rows(spec, seed = 1, now = Date.now()) {
  if (!spec) return [];
  const r = rng(seed * 7919 + (spec.salt || 0));
  const out = [];
  for (let i = 0; i < spec.count; i++) {
    const row = {};
    for (const [name, kind] of Object.entries(spec.fields)) row[name] = value(kind, i, r, now);
    out.push(row);
  }
  return out;
}

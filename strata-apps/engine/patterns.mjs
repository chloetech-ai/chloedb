// Screen patterns: whole parts of a screen with parameters (a table of rows
// with its add and edit form, a board over a status column, one record with
// its related rows, a breakdown by group, an agenda). They ship in
// library/patterns/ as JSON. In a screen, the block
//
//   {"use": "crud", "table": "guests", "fields": [...], "columns": [...]}
//
// is replaced by the pattern's blocks, and the pattern's state, data and lets
// join the screen's, before the screen is checked and stored. The stored
// screen holds the expanded blocks, so later changes patch them like any
// other part.
//
// A pattern file: {title, about, params: {name: {about, need?, default?}},
// example, screen: {state, data, let, body}}. In the screen template:
// - "{{name}}" alone is the parameter's value, of any type (missing: the
//   entry is left out); "{{a.b}}" reads into an object;
// - inside a longer string a text value goes in as it is and any other value
//   as JSON; "{{name|json}}" is always JSON, and "{{name|pre:TEXT}}" is TEXT
//   and the value, or nothing when the value is empty;
// - "{{...name}}" as an item of a list splices the list in;
// - an object with "$when": "name" (or "name=value", "name!=value", "!name",
//   several joined by "&") is left out unless the parameter is given (or
//   equals the value, differs from it, is not given); {"$when", "$value"}
//   stands for its $value.
// Defaults may name parameters declared before them.

import { readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const DIR = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'library', 'patterns');
const KEY_RE = /^[A-Za-z_][A-Za-z0-9_]*$/;

export function loadPatterns(dir = process.env.STRATA_APPS_PATTERNS || DIR) {
  const out = new Map();
  let files = [];
  try { files = readdirSync(dir).filter((f) => f.endsWith('.json')).sort(); } catch { return out; }
  for (const f of files) {
    const p = JSON.parse(readFileSync(path.join(dir, f), 'utf8'));
    out.set(f.slice(0, -5), p);
  }
  return out;
}

export const PATTERNS = loadPatterns();

const empty = (v) => v === undefined || v === null || v === '' || v === false || (Array.isArray(v) && !v.length) || (typeof v === 'object' && !Array.isArray(v) && !Object.keys(v).length);
const get = (P, name) => name.split('.').reduce((o, k) => (o == null ? undefined : o[k]), P);

function interpolate(s, P) {
  const whole = s.match(/^\{\{\s*([\w.]+)\s*\}\}$/);
  if (whole) return get(P, whole[1]);
  return s.replace(/\{\{\s*([\w.]+)\s*(?:\|\s*(json|pre:[^}]*))?\}\}/g, (_, name, filter) => {
    const v = get(P, name);
    if (filter && filter.startsWith('pre:')) return empty(v) ? '' : `${filter.slice(4)}${typeof v === 'string' ? v : JSON.stringify(v)}`;
    if (filter === 'json') return JSON.stringify(v ?? null);
    if (v === undefined || v === null) return '';
    return typeof v === 'string' ? v : JSON.stringify(v);
  });
}

function when(cond, P) {
  return cond.split('&').map((c) => c.trim()).every((c) => {
    const ne = c.indexOf('!=');
    if (ne > 0) return String(get(P, c.slice(0, ne)) ?? '') !== c.slice(ne + 2);
    if (c.startsWith('!')) return empty(get(P, c.slice(1)));
    const eq = c.indexOf('=');
    if (eq > 0) return String(get(P, c.slice(0, eq)) ?? '') === c.slice(eq + 1);
    return !empty(get(P, c));
  });
}

// The template v with the parameters P put in.
export function fill(v, P) {
  if (typeof v === 'string') return interpolate(v, P);
  if (Array.isArray(v)) {
    const out = [];
    for (const x of v) {
      const spread = typeof x === 'string' && x.match(/^\{\{\s*\.\.\.([\w.]+)\s*\}\}$/);
      if (spread) { const list = get(P, spread[1]); if (Array.isArray(list)) out.push(...list); else if (list !== undefined && list !== null) out.push(list); continue; }
      const y = fill(x, P);
      if (y !== undefined) out.push(y);
    }
    return out;
  }
  if (v && typeof v === 'object') {
    if (typeof v.$when === 'string' && !when(v.$when, P)) return undefined;
    if (v.$value !== undefined) return fill(v.$value, P);
    const out = {};
    for (const [k, x] of Object.entries(v)) {
      if (k === '$when') continue;
      const y = fill(x, P);
      if (y !== undefined) out[interpolate(k, P)] = y;
    }
    return out;
  }
  return v;
}

// A form's fields as a table's columns, for a pattern given no columns.
const FORMATS = { date: 'date', datetime: 'datetime', number: 'number', checkbox: 'bool', rating: 'stars' };
const fieldsAsColumns = (fields) => (Array.isArray(fields) ? fields : [])
  .map((f) => (typeof f === 'string' ? { name: f } : f))
  .filter((f) => f && f.name && !['textarea', 'file'].includes(f.type))
  .slice(0, 6)
  .map((f) => ({ key: f.name, ...(f.label ? { label: f.label } : {}), ...(FORMATS[f.type] ? { format: FORMATS[f.type] } : {}), ...(f.type === 'select' ? { badge: true } : {}) }));

// One "use" block expanded: {blocks, state, data, let} or throws.
function expandOne(b, patterns) {
  const { use, ...given } = b;
  const p = patterns.get(use);
  if (!p) throw new Error(`no screen pattern ${JSON.stringify(use)}; the patterns are ${[...patterns.keys()].join(', ')}`);
  const unknown = Object.keys(given).filter((k) => !p.params[k]);
  if (unknown.length) throw new Error(`the pattern ${use} takes ${Object.keys(p.params).join(', ')}, not ${unknown.join(', ')}`);
  const P = {};
  for (const [name, d] of Object.entries(p.params)) {
    let v = given[name];
    if (v === undefined && d.default !== undefined) v = fill(d.default, P);
    if (d.need && empty(v)) throw new Error(`the pattern ${use} needs "${name}": ${d.about}`);
    if (v !== undefined) P[name] = v;
  }
  // Columns made from the fields, once every parameter is known.
  for (const [name, d] of Object.entries(p.params)) if (P[name] === undefined && d.from === 'fields') P[name] = fieldsAsColumns(P.fields).filter((c) => c.key !== P.toggle);
  if (P.key !== undefined && !KEY_RE.test(String(P.key))) throw new Error(`the pattern ${use}: "key" ${JSON.stringify(P.key)} must be a name of letters, digits and _ (set "key" when the table name is not one)`);
  const s = fill(p.screen, P);
  return { blocks: s.body || [], state: s.state || {}, data: s.data || {}, let: s.let || {} };
}

const HOLDERS = ['row', 'col', 'grid', 'split', 'card', 'section', 'then', 'else', 'body'];

// Expands every "use" block of a screen (at any depth). Returns
// {screen, used: [pattern names], problems}.
export function expandScreen(screen, patterns = PATTERNS) {
  const problems = [];
  const used = [];
  if (!screen || typeof screen !== 'object' || !JSON.stringify(screen).includes('"use"')) return { screen, used, problems };
  const out = { ...screen, state: { ...(screen.state || {}) }, data: { ...(screen.data || {}) } };
  const lets = {};
  const join = (into, add, what, where) => {
    for (const [k, v] of Object.entries(add)) {
      if (into[k] !== undefined && JSON.stringify(into[k]) !== JSON.stringify(v)) problems.push(`${where}: the screen has ${what}.${k} already; give the pattern another "key"`);
      else into[k] = v;
    }
  };
  const walk = (list, path) => {
    if (!Array.isArray(list)) return list;
    const res = [];
    list.forEach((b, i) => {
      const where = `${path}[${i}]`;
      if (b && typeof b === 'object' && !Array.isArray(b) && typeof b.use === 'string') {
        try {
          const x = expandOne(b, patterns);
          used.push(b.use);
          join(out.state, x.state, 'state', where);
          join(out.data, x.data, 'data', where);
          join(lets, x.let, 'let', where);
          res.push(...walk(x.blocks, where));
        } catch (e) { problems.push(`${where}: ${e.message}`); }
        return;
      }
      if (b && typeof b === 'object' && !Array.isArray(b)) {
        const c = { ...b };
        for (const k of HOLDERS) if (Array.isArray(c[k])) c[k] = walk(c[k], `${where}.${k}`);
        res.push(c);
      } else res.push(b);
    });
    return res;
  };
  out.body = walk(screen.body, 'body');
  // The patterns' lets come first: the screen's own lets may read them.
  out.let = { ...lets, ...(screen.let || {}) };
  for (const k of Object.keys(lets)) if (screen.let && screen.let[k] !== undefined && JSON.stringify(screen.let[k]) !== JSON.stringify(lets[k])) problems.push(`the screen has let.${k} already; give the pattern another "key"`);
  if (!Object.keys(out.state).length) delete out.state;
  if (!Object.keys(out.let).length) delete out.let;
  return { screen: out, used, problems };
}

// Expands the "use" blocks of every screen in a plan; records the patterns
// each view used. Returns problems.
export function expandPlan(plan, patterns = PATTERNS) {
  const problems = [];
  for (const v of plan?.views || []) {
    if (!v || !v.screen || typeof v.screen !== 'object') continue;
    const r = expandScreen(v.screen, patterns);
    v.screen = r.screen;
    if (r.used.length) v.patterns = [...new Set(r.used)];
    problems.push(...r.problems.map((p) => `view ${v.slug}: ${p}`));
  }
  return problems;
}

// The patterns, for the system prompt.
export function patternsDoc(patterns = PATTERNS) {
  if (!patterns.size) return '';
  const parts = [...patterns.entries()].map(([name, p]) => {
    const params = Object.entries(p.params).map(([k, d]) => `  - ${k}${d.need ? ' (needed)' : ''}: ${d.about}${d.default !== undefined && typeof d.default !== 'object' && d.default !== '' ? ` (default ${JSON.stringify(d.default)})` : ''}`).join('\n');
    return `## ${name}: ${p.title}\n${p.about}\n${params}\nExample: ${JSON.stringify(p.example)}`;
  });
  return `# Screen patterns

A pattern is a whole part of a screen with parameters. Put {"use": "name", ...parameters} in a screen's body, alone or among other blocks; the forge replaces it with the pattern's blocks, adds the pattern's state, data and lets to the screen, and the app shows the expanded screen. Prefer a pattern for the parts it covers: it is short to write and already right; add your own blocks around it for the rest. Names the pattern adds start with its "key" (the table, unless you set "key"): data <key> holds its rows (as the query gives them), let <key>_shown the rows it shows, and state <key>_q, <key>_f and <key>_editing its search, filter and the row being edited; your blocks and expressions may read them. Use "key" to put one pattern twice in a screen. A parameter that is a block list (card, extra) holds ordinary blocks, and inside them the row is "row".

${parts.join('\n\n')}`;
}

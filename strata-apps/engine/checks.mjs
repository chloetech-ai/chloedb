// Checks a build runs once its version is live (idea 7: tests as data).
//
// - The app's tests, written as data in the plan, run against its new tables
//   while they are still empty, before the example rows go in; then those
//   tables are emptied again. Triggers and functions run as they will for
//   people, so a test checks the logic, not only the shape.
// - Every view is opened in a headless browser (when Playwright is
//   installed): an error there reaches the engine the way an error a person
//   saw does, and is repaired the same way.

import { createRequire } from 'node:module';
import { runRule, paramsOf, sqlOf, used, checkRule } from './rules.mjs';
import { physical } from './store.mjs';

export const TESTS_DOC = `# Tests
A new app, or a change that adds tables, may give "tests" in its plan: 1 to 3 checks of the logic most likely to go wrong (a total, a count a trigger keeps right, what a function returns). Each runs when the version is built, on the new tables while they are still empty (before the seed rows), and the tables are emptied after:
  "tests": [{"name": "a vote raises the feature's count",
    "do": [{"insert": "features", "values": {"title": "Dark mode"}, "as": "f"}, {"insert": "votes", "values": {"feature": ":f.id"}}],
    "expect": [{"query": "SELECT votes FROM features WHERE id = :f.id", "is": 1}]}]
- "do": steps as in rules (insert, update, remove, call, let); "as" names an inserted row or a call's result for later steps and for "expect" (:f.id).
- "expect": a SELECT and the value its first column of its first row must be ("is": a literal, or ":f.id" for a value a step named), or how many rows it returns ("rows"). Names are ":name", never "=expressions".
- A test writes only to tables this version creates. A failing test is reported and repaired like an error. A screen that only lists and edits rows needs no test.`;

// The same, with the steps written as statements of a CREATE RULE body.
export const TESTS_SQL_DOC = `# Tests
A new app, or a change that adds tables, may give "tests" in its plan: 1 to 3 checks of the logic most likely to go wrong (a total, a count a trigger keeps right, what a function returns). Each runs when the version is built, on the new tables while they are still empty (before the seed rows), and the tables are emptied after:
  "tests": [{"name": "a vote raises the feature's count",
    "do": "INSERT INTO features (title) VALUES ('Dark mode') AS f; INSERT INTO votes (feature) VALUES (:f.id)",
    "expect": [{"query": "SELECT votes FROM features WHERE id = :f.id", "is": 1}]}]
- "do": statements as in a rule's body (INSERT, UPDATE, DELETE, CALL fn(...) AS name, LET), each ending with ";"; AS names an inserted row or a call's result for later statements and for "expect" (:f.id).
- "expect": a SELECT and the value its first column of its first row must be ("is": a literal, or ":f.id" for a value a statement named), or how many rows it returns ("rows"). Names are ":name", never "=expressions".
- A test writes only to tables this version creates. A failing test is reported and repaired like an error. A screen that only lists and edits rows needs no test.`;

// Checks the tests' shape. columnsOf(table) -> Set | null.
export function checkTests(tests, columnsOf) {
  if (tests === undefined) return [];
  if (!Array.isArray(tests)) return ['"tests" is a list'];
  const problems = [];
  tests.forEach((t, i) => {
    const where = `tests[${i}]${t && t.name ? ` (${t.name})` : ''}`;
    if (!t || typeof t !== 'object') { problems.push(`${where}: a test is an object with name, do and expect`); return; }
    if (!Array.isArray(t.expect) || !t.expect.length) problems.push(`${where}: give "expect": [{"query": "SELECT ...", "is": value}]`);
    for (const [j, e] of (t.expect || []).entries()) {
      if (!e || typeof e.query !== 'string' || !/^\s*(select|with)\b/i.test(e.query)) problems.push(`${where}.expect[${j}]: give "query", a SELECT`);
      else if (e.is === undefined && e.rows === undefined) problems.push(`${where}.expect[${j}]: give "is" (the first value) or "rows" (how many)`);
      else if (typeof e.is === 'string' && e.is.startsWith('=')) problems.push(`${where}.expect[${j}].is: a value, or ":name.col" for something a step named with "as"; not an =expression`);
    }
    if (t.do !== undefined) for (const p of checkRule({ steps: t.do }, columnsOf)) problems.push(`${where}.do: ${p.replace(/^steps/, '')}`);
  });
  return problems;
}

// ":f.id" in "is": a value a step named with "as".
const expected = (v, lets) => (typeof v === 'string' && /^:[A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)?$/.test(v) ? paramsOf({}, lets)[v.slice(1).replace('.', '_')] ?? null : v);

const same = (a, b) => {
  if (a === b) return true;
  if (a == null || b == null) return a == b; // eslint-disable-line eqeqeq
  const na = Number(a), nb = Number(b);
  if (typeof a !== 'object' && typeof b !== 'object' && !Number.isNaN(na) && !Number.isNaN(nb) && String(a).trim() !== '' && String(b).trim() !== '') return Math.abs(na - nb) < 1e-6;
  return JSON.stringify(a) === JSON.stringify(b);
};

// The tables a test writes, directly.
const writes = (t) => (t.do || []).map((s) => s && (s.insert || s.update || s.remove)).filter(Boolean);

// Runs the tests on the tables in `fresh` (created by this version, still
// empty), then empties them. Returns [{name, ok, problem?, cap?}]: cap is the
// function the failure most likely belongs to (one it calls, or a trigger on
// a table it writes).
export async function runTests(engine, appId, tests, fresh) {
  const out = [];
  const view = await engine.store.resolve(appId);
  for (const t of tests || []) {
    const name = String(t.name || 'a test');
    const touched = writes(t);
    const calls = (t.do || []).filter((s) => s && s.call).map((s) => s.call);
    const cap = view.functions.find((f) => calls.includes(f.slug))
      || view.functions.find((f) => f.spec.on && touched.includes(f.spec.on.table));
    if (touched.some((x) => !fresh.has(x))) { out.push({ name, ok: null, problem: `skipped: it writes ${touched.filter((x) => !fresh.has(x)).join(', ')}, which already holds rows` }); continue; }
    if (calls.length && !fresh.size) { out.push({ name, ok: null, problem: 'skipped: it calls a function on tables that hold rows' }); continue; }
    const host = async (method, args) => {
      switch (method) {
        case 'sql': return engine.sql(appId, args[0], args[1]);
        case 'insert': return engine.insert(appId, args[0], args[1], {});
        case 'update': return engine.update(appId, args[0], args[1], args[2], {});
        case 'remove': return engine.remove(appId, args[0], args[1], {});
        case 'call': return (await engine.call(appId, args[0], args[1], { depth: 0 })).result;
        default: throw new Error(`no ${method} in a test`);
      }
    };
    try {
      const { lets } = await runRule({ steps: t.do || [] }, {}, host);
      await engine.settle();
      const bad = [];
      for (const e of t.expect || []) {
        const q = sqlOf(e.query);
        const rows = await engine.sql(appId, q, used(q, paramsOf({}, lets)));
        if (e.rows !== undefined) { if (rows.length !== Number(e.rows)) bad.push(`${e.query} gave ${rows.length} rows, not ${e.rows}`); continue; }
        const got = rows[0] ? Object.values(rows[0])[0] ?? null : null;
        const want = expected(e.is, lets);
        if (!same(got, want)) bad.push(`${e.query} gave ${JSON.stringify(got)}, not ${JSON.stringify(want)}${want !== e.is ? ` (${e.is})` : ''}`);
      }
      out.push(bad.length ? { name, ok: false, problem: bad.join('; '), cap: cap?.id } : { name, ok: true });
    } catch (e) {
      await engine.settle().catch(() => {});
      out.push({ name, ok: false, problem: e.message, cap: cap?.id });
    }
  }
  await engine.settle().catch(() => {});
  await emptyTables(engine, appId, fresh);
  return out;
}

// Deletes every row of the given tables, without running triggers.
async function emptyTables(engine, appId, tables) {
  for (const t of tables) {
    const name = physical(appId, t);
    for (;;) {
      const rows = await engine.db.query(`SELECT id FROM ${name} LIMIT 1000`).catch(() => []);
      if (!rows.length) break;
      await engine.db.write(rows.map((r) => ({ op: 'delete', table: name, key: String(r.id) })));
      if (rows.length < 1000) break;
    }
  }
  if (tables.size) engine.announce('data', { app: appId, tables: [...tables] });
}

let playwright;
function loadPlaywright() {
  if (playwright !== undefined) return playwright;
  try { playwright = createRequire(import.meta.url)('playwright'); } catch { playwright = null; }
  return playwright;
}

// Opens the app and each of its views in a headless browser and returns the
// errors seen, per view: {ran, views: [{tab, errors}]}. The page itself
// reports its errors to the engine, which repairs them.
export async function openViews(baseUrl, appId, { wait = 1500, v = null } = {}) {
  const pw = process.env.STRATA_APPS_OPEN_VIEWS === 'off' ? null : loadPlaywright();
  if (!pw || !baseUrl) return { ran: false, views: [] };
  const browser = await pw.chromium.launch();
  const views = [];
  try {
    const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
    let errors = [];
    page.on('pageerror', (e) => errors.push(e.message.split('\n')[0].slice(0, 300)));
    page.on('console', (m) => { if (m.type() === 'error' && !/favicon/.test(m.text())) errors.push(m.text().split('\n')[0].slice(0, 300)); });
    page.on('dialog', (d) => d.dismiss());
    await page.goto(`${baseUrl}/a/${appId}${v ? `?v=${v}` : ''}`, { waitUntil: 'load', timeout: 15000 });
    await page.waitForTimeout(wait);
    const tabs = page.locator('#tabs button:visible');
    const n = Math.max(await tabs.count(), 1);
    for (let i = 0; i < n; i++) {
      if (i > 0 || errors.length === 0) errors = i === 0 ? errors : [];
      let tab = '';
      if (await tabs.count()) { tab = (await tabs.nth(i).innerText()).trim(); await tabs.nth(i).click(); await page.waitForTimeout(wait); }
      views.push({ tab, errors: [...new Set(errors)] });
      errors = [];
    }
  } finally { await browser.close(); }
  return { ran: true, views };
}

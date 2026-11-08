// Tests that need neither Strata nor the model: the SQL rewrite, the answer
// parser, the checks on a plan, and the function sandbox.

import test from 'node:test';
import assert from 'node:assert/strict';
import { rewriteSql } from '../engine/sql.mjs';
import { parseAnswer } from '../engine/llm.mjs';
import { validate, uiRefs, pinShared, checkShared, sharedRefs } from '../engine/forge.mjs';
import { readLibrary, parsePiece } from '../engine/library.mjs';
import { linksOf, appDocs, parseLinks } from '../engine/graph.mjs';
import { checkFunction, runFunction } from '../engine/sandbox.mjs';

const P = (t) => `app_abc123_${t}`;
const T = ['tasks', 'tags', 'people'];

test('table names become physical names, keeping the declared name as alias', () => {
  assert.equal(rewriteSql("SELECT * FROM tasks WHERE status = 'from tasks' LIMIT 5", T, P).sql,
    "SELECT * FROM app_abc123_tasks AS tasks WHERE status = 'from tasks' LIMIT 5");
  assert.equal(rewriteSql('SELECT t.title, p.name FROM tasks t JOIN people p ON t.owner = p.id', T, P).sql,
    'SELECT t.title, p.name FROM app_abc123_tasks t JOIN app_abc123_people p ON t.owner = p.id');
  assert.equal(rewriteSql('SELECT tasks.title FROM tasks JOIN tags ON tags.task = tasks.id', T, P).sql,
    'SELECT tasks.title FROM app_abc123_tasks AS tasks JOIN app_abc123_tags AS tags ON tags.task = tasks.id');
  assert.equal(rewriteSql('SELECT EXTRACT(dow FROM created) AS d, COUNT(*) AS n FROM tasks GROUP BY d', T, P).sql,
    'SELECT EXTRACT(dow FROM created) AS d, COUNT(*) AS n FROM app_abc123_tasks AS tasks GROUP BY d');
  assert.equal(rewriteSql('WITH r AS (SELECT * FROM tasks LIMIT 10) SELECT * FROM r', T, P).sql,
    'WITH r AS (SELECT * FROM app_abc123_tasks AS tasks LIMIT 10) SELECT * FROM r');
});

test('only SELECTs over the app\'s own tables', () => {
  assert.throws(() => rewriteSql('SELECT * FROM sa_apps', T, P), /no table sa_apps/);
  assert.throws(() => rewriteSql('SELECT * FROM app_zzz999_tasks', T, P), /no table/);
  assert.throws(() => rewriteSql('DELETE FROM tasks', T, P), /SELECT queries only/);
  assert.throws(() => rewriteSql('SELECT 1; DROP TABLE tasks', T, P), /one statement/);
});

test('the answer parser keeps code verbatim', () => {
  const text = '<plan>{"views":[{"slug":"v1"}]}</plan>\n<code slug="v1">\nconst x = "</div>";\nroot.append(x)\n</code>';
  const { plan, code } = parseAnswer(text);
  assert.deepEqual(plan, { views: [{ slug: 'v1' }] });
  assert.equal(code.v1, 'const x = "</div>";\nroot.append(x)');
});

const empty = { app: {}, comp: { n: 0, caps: [] }, caps: [], tables: [], views: [], functions: [], components: [], helpers: [], shared: [] };

test('a plan is checked before anything is stored', async () => {
  const good = { tables: [{ name: 'notes', columns: [{ name: 'body', type: 'text' }] }], views: [{ slug: 'home', title: 'Home' }] };
  assert.deepEqual(await validate(good, { home: 'async function render(root, app) { root.append("hi") }' }, empty, []), []);
  const bad = {
    tables: [{ name: 'Notes', columns: [] }, { name: 'n2', columns: [{ name: 'id', type: 'text' }, { name: 'x', type: 'string' }] }],
    views: [{ slug: 'home' }, { slug: 'other' }],
    functions: [{ slug: 'f', on: { table: 'nope', event: 'insert' } }],
  };
  const problems = await validate(bad, { home: 'async function render(root, app) { fetch("x") }', other: 'function render( {', f: 'async function run() { await 1 }' }, empty, []);
  const text = problems.join('\n');
  for (const want of ['table name "Notes"', 'n2.id', 'type "string"', 'no fetch', 'view other: syntax', 'do not use await', 'names table nope']) {
    assert.ok(text.includes(want), `missing ${want} in:\n${text}`);
  }
});

test('types never change', async () => {
  const view = { ...empty, comp: { n: 1, caps: [] }, tables: [{ spec: { name: 'notes', columns: [{ name: 'body', type: 'text' }] } }], views: [{ slug: 'home' }] };
  const problems = await validate({ tables: [{ name: 'notes', columns: [{ name: 'body', type: 'keyword' }] }] }, {}, view, []);
  assert.match(problems.join('\n'), /types never change/);
});

test('functions run in the sandbox, with app calls answered by the host', async () => {
  assert.deepEqual(await checkFunction('function run(input, app) { return 1 }'), []);
  const calls = [];
  const host = async (method, args) => { calls.push([method, args]); return method === 'sql' ? [{ n: 2 }] : { id: 'r1' }; };
  const out = await runFunction('function run(input, app) { const [r] = app.sql("SELECT COUNT(*) AS n FROM tasks"); app.insert("log", { n: r.n + input.add }); return r.n * 10; }', { add: 1 }, host);
  assert.equal(out.result, 20);
  assert.deepEqual(calls, [['sql', ['SELECT COUNT(*) AS n FROM tasks', {}]], ['insert', ['log', { n: 3 }]]]);
});

test('the sandbox stops runaway code and has no way out', async () => {
  await assert.rejects(runFunction('function run() { for (;;) {} }', null, async () => null, { timeoutMs: 300 }), /interrupted/);
  const out = await runFunction('function run() { return [typeof require, typeof process, typeof fetch, typeof globalThis.std] }', null, async () => null);
  assert.deepEqual(out.result, ['undefined', 'undefined', 'undefined', 'undefined']);
  await assert.rejects(runFunction('function run(i, app) { return app.sql("x") }', null, async () => { throw new Error('refused') }), /refused/);
});

test('views call shared components; components take data only through props', async () => {
  assert.deepEqual([...uiRefs("root.append(ui.calendar({ month }), ui['barChart']({}), app.ui, myui.x)")], ['calendar', 'barChart']);
  const shared = new Map([['component/calendar', { id: 'lib/component/calendar@1', kind: 'component', slug: 'calendar', version: 1 }]]);
  const plan = { tables: [{ name: 'checks', columns: [{ name: 'day', type: 'keyword' }] }], views: [{ slug: 'month' }],
    components: [{ name: 'streak', props: 'days: string[]' }, { name: 'Bad-Name' }, { name: 'peek' }] };
  const code = {
    month: 'async function render(root, app) { root.append(ui.calendar({}), ui.streak({}), ui.heatmap({})) }',
    'ui:streak': 'function component(props) { return h("b", null, props.days.length) }',
    'ui:peek': 'function component(props) { return app.sql("SELECT 1") }',
  };
  const text = (await validate(plan, code, empty, [], shared)).join('\n');
  for (const want of ['component name "Bad-Name"', 'component peek: uses app.sql', 'calls ui.heatmap, which is not a shared component']) {
    assert.ok(text.includes(want), `missing ${want} in:\n${text}`);
  }
  assert.ok(!text.includes('ui.calendar') && !text.includes('ui.streak'), text);
});

test('an app pins the shared pieces its code calls, following pieces that call others, and keeps its versions until it upgrades', async () => {
  const caps = new Map([
    ['a/view/home@1', { code: 'ui.calendar({}); ui.chart({})' }],
    ['a/function/total@1', { code: 'async function run(input, app) { return lib.money.sum([1]) }' }],
    ['lib/component/calendar@1', { code: 'function component() { return ui.dayCell({ d: lib.dates.today() }) }' }],
    ['lib/component/calendar@2', { code: 'function component() { return ui.dayCell({}) }' }],
    ['lib/component/dayCell@1', { code: 'function component() {}' }],
    ['lib/component/chart@3', { code: 'function component() {}' }],
    ['lib/helper/dates@1', { code: 'function helper() { return {} }' }],
    ['lib/helper/money@2', { code: 'function helper() { return {} }' }],
  ]);
  const store = { getCapabilities: async (ids) => ids.map((id) => caps.get(id)).filter(Boolean) };
  const shared = new Map([['component/calendar', { id: 'lib/component/calendar@2' }], ['component/dayCell', { id: 'lib/component/dayCell@1' }], ['component/chart', { id: 'lib/component/chart@3' }],
    ['component/unused', { id: 'lib/component/unused@1' }], ['helper/dates', { id: 'lib/helper/dates@1' }], ['helper/money', { id: 'lib/helper/money@2' }]]);
  const had = new Map([['component/calendar', 'lib/component/calendar@1']]);
  const kept = await pinShared(store, ['a/view/home@1', 'a/function/total@1'], { had, shared });
  assert.deepEqual(Object.fromEntries(kept), { 'component/calendar': 'lib/component/calendar@1', 'component/chart': 'lib/component/chart@3', 'component/dayCell': 'lib/component/dayCell@1',
    'helper/dates': 'lib/helper/dates@1', 'helper/money': 'lib/helper/money@2' });
  const moved = await pinShared(store, ['a/view/home@1'], { had, shared, upgrade: new Set(['calendar']) });
  assert.equal(moved.get('component/calendar'), 'lib/component/calendar@2');
  assert.ok(!moved.has('helper/dates'), 'calendar@2 no longer calls lib.dates');
});

test('helpers run in functions, inside the sandbox', async () => {
  const pieces = await readLibrary(new URL('../library', import.meta.url).pathname);
  const helpers = pieces.filter((p) => p.kind === 'helper').map((p) => ({ name: p.name, code: p.code }));
  const out = await runFunction('function run(input, app) { return { split: lib.money.split(input.total, 3), next: lib.dates.addDays("2026-02-27", 2), streak: lib.streak.current(["2026-11-06", "2026-11-07"], { today: "2026-11-08" }), csv: lib.csv.parse("a,b\\n1,2").length } }',
    { total: 100 }, async () => null, { helpers });
  assert.deepEqual(out.result, { split: [33.34, 33.33, 33.33], next: '2026-03-01', streak: 2, csv: 1 });
});

test('the built-in library: every piece has a header, passes the checks a written piece must pass, and calls only pieces that exist', async () => {
  const pieces = await readLibrary(new URL('../library', import.meta.url).pathname);
  assert.ok(pieces.length >= 25, `${pieces.length} pieces`);
  const names = new Set(pieces.map((p) => `${p.kind}/${p.name}`));
  for (const p of pieces) {
    assert.ok(p.meta.title && p.meta.description && (p.kind === 'helper' ? p.meta.api : p.meta.props), `${p.name}: header`);
    assert.deepEqual(checkShared(p.kind, p.code), [], p.name);
    for (const ref of sharedRefs(p.code)) assert.ok(names.has(ref), `${p.name} calls ${ref}, which the library lacks`);
    if (p.kind === 'helper') {
      const api = new Function(`${p.code}\n;return helper();`)();
      assert.ok(Object.values(api).every((f) => typeof f === 'function'), `${p.name}: helper() returns functions`);
    }
  }
  const { meta, code } = parsePiece('// title: T\n// description: one\n//   two\n// api: f()\nfunction helper() { return {} }\n');
  assert.deepEqual(meta, { title: 'T', description: 'one\ntwo', api: 'f()' });
  assert.equal(code, 'function helper() { return {} }\n');
});

test('the graph links what code calls: shared pieces, tables, functions and views', () => {
  const view = { app: 'a1', kind: 'view', slug: 'home', id: 'a1/view/home@3', code: `
    const rows = await app.sql('SELECT * FROM habits WHERE id = :x');
    await app.insert("checkins", {}); app.call('streaks', {}); app.open('history');
    root.append(ui.calendar({}), lib.dates.today());` };
  assert.deepEqual(linksOf(view, { tables: ['habits', 'checkins', 'notes'], functions: ['streaks'], views: ['home', 'history'] }), [
    ['uses', 'lib/component/calendar'], ['uses', 'lib/helper/dates'], ['data', 'a1/table/habits'], ['data', 'a1/table/checkins'], ['calls', 'a1/function/streaks'], ['opens', 'a1/view/history']]);
  const fn = { app: 'a1', kind: 'function', slug: 'streaks', code: 'async function run(input, app) { return 1 }', spec: { on: { table: 'checkins' } }, forked_from: 'b2/function/streaks@4' };
  assert.deepEqual(linksOf(fn, { tables: ['checkins'] }), [['on', 'a1/table/checkins'], ['forked', 'b2/function/streaks']]);
  const docs = appDocs({ app: { id: 'a1', name: 'Habits', head: 3 }, caps: [view, { app: 'a1', kind: 'table', slug: 'habits', spec: { name: 'habits' } }, { app: 'lib', kind: 'component', slug: 'calendar' }],
    tables: [{ slug: 'habits', spec: { name: 'habits' } }], views: [view], functions: [] }, ['component', 'helper']);
  assert.deepEqual(docs.map((d) => d.id), ['app/a1', 'a1/view/home', 'a1/table/habits']);
  assert.deepEqual(docs[0].links, [['has', 'a1/view/home'], ['has', 'a1/table/habits']]);
  assert.deepEqual(parseLinks('[["uses","x"],"y",{"to":"z","label":"has"}]'), [{ label: 'uses', to: 'x' }, { to: 'y' }, { label: 'has', to: 'z' }]);
});

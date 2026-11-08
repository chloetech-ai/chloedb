// Tests that need neither Strata nor the model: the SQL rewrite, the answer
// parser, the checks on a plan, and the function sandbox.

import test from 'node:test';
import assert from 'node:assert/strict';
import { rewriteSql } from '../engine/sql.mjs';
import { parseAnswer } from '../engine/llm.mjs';
import { validate } from '../engine/forge.mjs';
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

const empty = { app: {}, comp: { n: 0, caps: [] }, tables: [], views: [], functions: [] };

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

// Tests that need neither Strata nor the model: the SQL rewrite, the answer
// parser, the checks on a plan, and the function sandbox.

import test from 'node:test';
import assert from 'node:assert/strict';
import { rewriteSql } from '../engine/sql.mjs';
import { parseAnswer, ask } from '../engine/llm.mjs';
import { writeFileSync, chmodSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { validate, uiRefs, pinShared, checkShared, sharedRefs, checkView, completePlan, checkScreen, screensToCode, applyPatch, patchScreens, screenLines, rulesToCode, prepareScreens, editCode } from '../engine/forge.mjs';
import { runRule, checkRule } from '../engine/rules.mjs';
import { readLibrary, parsePiece, parseTables } from '../engine/library.mjs';
import { linksOf, appDocs, parseLinks } from '../engine/graph.mjs';
import { checkFunction, runFunction } from '../engine/sandbox.mjs';
import { Services, buildMessage, smtpSend } from '../engine/services.mjs';
import { Trace, planDecisions } from '../engine/trace.mjs';
import net from 'node:net';

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
    const third = { component: p.meta.props, helper: p.meta.api, view: p.meta.tables, function: p.meta.input }[p.kind];
    assert.ok(p.meta.title && p.meta.description && third, `${p.name}: header`);
    if (p.kind === 'view') {
      assert.deepEqual(checkView(p.code), [], p.name);
      assert.ok(p.tables.length, `${p.name}: names its tables`);
      for (const t of p.tables) assert.deepEqual(await validate({ tables: [t] }, {}, { tables: [], views: [], functions: [], shared: [], comp: { n: 1 } }, []), ['the app would have no view'], `${p.name}: table ${t.name}`);
    } else if (p.kind === 'function') {
      assert.deepEqual(await checkFunction(p.code), [], p.name);
    } else {
      assert.deepEqual(checkShared(p.kind, p.code), [], p.name);
    }
    for (const ref of sharedRefs(p.code)) assert.ok(names.has(ref), `${p.name} calls ${ref}, which the library lacks`);
    if (p.kind === 'helper') {
      const api = new Function(`${p.code}\n;return helper();`)();
      assert.ok(Object.values(api).every((f) => typeof f === 'function'), `${p.name}: helper() returns functions`);
    }
  }
  assert.deepEqual(parseTables('notes(title text, tags keyword[]); links(note keyword, n int)'),
    [{ name: 'notes', columns: [{ name: 'title', type: 'text' }, { name: 'tags', type: 'keyword[]' }] }, { name: 'links', columns: [{ name: 'note', type: 'keyword' }, { name: 'n', type: 'int' }] }]);
  assert.throws(() => parseTables('notes(title string)'), /not "name type"/);
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

test('a function sees who is signed in, reaches the services through the host, and uses the new helpers', async () => {
  const pieces = await readLibrary(new URL('../library', import.meta.url).pathname);
  const helpers = pieces.filter((p) => p.kind === 'helper').map((p) => ({ name: p.name, code: p.code }));
  const calls = [];
  const host = async (method, args) => { calls.push(method); return method === 'people' ? [{ id: 'p1' }, { id: 'p2' }] : method === 'email' ? { id: 'm1', status: 'held' } : null; };
  const code = `function run(input, app) {
    const row = { id: 'r1', author: 'p1' };
    return {
      me: app.user && app.user.name, same: app.me() === app.user,
      mayEdit: lib.access.can(app.user, 'edit', row), mayEditOther: lib.access.can(app.user, 'edit', { author: 'p9' }),
      people: app.people().length, mail: app.email({ to: 'a@example.com', subject: 'hi' }).status,
      km: Math.round(lib.geo.distance({ lat: 52.52, lng: 13.405 }, { lat: 48.8566, lng: 2.3522 })),
      mins: lib.duration.minutes('1h 30m'), lb: Math.round(lib.units.convert(10, 'kg', 'lb') * 10) / 10,
      ok: lib.validate.check({ email: 'x' }, { email: 'required|email' }).ok, tasks: lib.markdown.tasks('- [ ] a\\n- [x] b').length,
      ink: lib.color.readable('#1d4ed8'),
    };
  }`;
  const out = await runFunction(code, null, host, { helpers, user: { id: 'p1', name: 'Ana', email: 'ana@example.com', role: 'member' } });
  assert.deepEqual(out.result, { me: 'Ana', same: true, mayEdit: true, mayEditOther: false, people: 2, mail: 'held', km: 877, mins: 90, lb: 22, ok: false, tasks: 2, ink: '#ffffff' });
  assert.deepEqual(calls, ['people', 'email']);
  const nobody = await runFunction('function run(input, app) { return [app.user, lib.access.why(app.user, "add")] }', null, host, { helpers });
  assert.deepEqual(nobody.result, [null, 'Sign in to add this']);
});

test('helpers get no app at all; components may call services but not data', () => {
  assert.match(checkShared('helper', 'function helper() { return { who: () => app.me() } }').join(' '), /a helper gets no app/);
  assert.deepEqual(checkShared('component', 'function component(props) { return h("img", { src: app.fileUrl(props.file) }) }'), []);
  assert.match(checkShared('component', 'function component(props) { app.sql("SELECT 1"); return h("div") }').join(' '), /uses app.sql/);
});

test('passwords are hashed with scrypt and checked in constant time', async () => {
  const stored = await Services.hashPassword('correct horse');
  assert.match(stored, /^scrypt\$[0-9a-f]{32}\$[0-9a-f]{64}$/);
  assert.equal(await Services.checkPassword('correct horse', stored), true);
  assert.equal(await Services.checkPassword('wrong horse', stored), false);
  assert.equal(await Services.checkPassword('x', 'plain'), false);
});

test('email goes out over SMTP: AUTH PLAIN, one recipient line each, dot-stuffed body, encoded headers', async () => {
  const msg = buildMessage({ from: 'apps@example.test', fromName: 'Café list', to: ['a@example.com'], subject: 'Grüße', text: 'line one\n.starts with a dot' });
  assert.match(msg, /^From: =\?UTF-8\?B\?[A-Za-z0-9+/=]+\?= <apps@example.test>\r\n/);
  assert.match(msg, /\r\nSubject: =\?UTF-8\?B\?/);
  assert.match(msg, /\r\n\r\nline one\r\n\.\.starts with a dot$/);
  const seen = { lines: [], data: '' };
  const server = net.createServer((sock) => {
    let buf = ''; let inData = false;
    sock.write('220 test ESMTP\r\n');
    sock.on('data', (d) => {
      buf += d.toString();
      for (;;) {
        if (inData) { const end = buf.indexOf('\r\n.\r\n'); if (end < 0) return; seen.data = buf.slice(0, end); buf = buf.slice(end + 5); inData = false; sock.write('250 queued\r\n'); continue; }
        const i = buf.indexOf('\r\n'); if (i < 0) return;
        const line = buf.slice(0, i); buf = buf.slice(i + 2); seen.lines.push(line);
        const cmd = line.split(' ')[0];
        if (cmd === 'EHLO') sock.write('250-test\r\n250 AUTH PLAIN\r\n');
        else if (cmd === 'AUTH') sock.write('235 ok\r\n');
        else if (cmd === 'DATA') { inData = true; sock.write('354 go\r\n'); }
        else if (cmd === 'QUIT') { sock.end('221 bye\r\n'); }
        else sock.write('250 ok\r\n');
      }
    });
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  const { port } = server.address();
  try {
    await smtpSend(`smtp://me%40x:p%40ss@127.0.0.1:${port}`, { from: 'apps@example.test', to: ['a@example.com', 'b@example.com'], subject: 'Hi', text: 'Hello\n.dot' });
    assert.equal(seen.lines.find((l) => l.startsWith('AUTH')), `AUTH PLAIN ${Buffer.from('\0me@x\0p@ss').toString('base64')}`);
    assert.deepEqual(seen.lines.filter((l) => l.startsWith('RCPT')), ['RCPT TO:<a@example.com>', 'RCPT TO:<b@example.com>']);
    assert.match(seen.data, /\r\nSubject: Hi\r\n/);
    assert.match(seen.data, /Hello\r\n\.\.dot$/);
  } finally { server.close(); }
});

// A stand-in for `claude -p --output-format stream-json`: it streams the
// answer in pieces (after `pause` ms of silence), then the result line.
function fakeClaude(answer, pause = 0) {
  const dir = mkdtempSync(join(tmpdir(), 'sa-fake-'));
  const bin = join(dir, 'claude');
  writeFileSync(bin, `#!/usr/bin/env node
const answer = ${JSON.stringify(answer)};
const out = (o) => process.stdout.write(JSON.stringify(o) + '\\n');
process.stdin.resume();
process.stdin.on('end', async () => {
  out({ type: 'system', subtype: 'init' });
  await new Promise((r) => setTimeout(r, ${pause}));
  out({ type: 'stream_event', event: { type: 'content_block_delta', index: 0, delta: { type: 'thinking_delta', thinking: '' } } });
  for (let i = 0; i < answer.length; i += 7) out({ type: 'stream_event', event: { type: 'content_block_delta', index: 1, delta: { type: 'text_delta', text: answer.slice(i, i + 7) } } });
  out({ type: 'result', subtype: 'success', is_error: false, result: answer, total_cost_usd: 0.01, duration_ms: 5, usage: { output_tokens: 3 } });
});
`);
  chmodSync(bin, 0o755);
  return bin;
}

test('the model\'s answer streams in: progress as it is written, and a stop only when it goes quiet', async () => {
  const answer = '<plan>{"views":[]}</plan>\n<code slug="a">\nlet x = 1;\n</code>';
  const seen = [];
  const res = await ask({ system: 's', prompt: 'p', bin: fakeClaude(answer), onWrite: (w) => seen.push({ ...w }) });
  assert.equal(res.text, answer);
  assert.equal(res.cost, 0.01);
  assert.ok(seen.length > 3, 'onWrite runs as text arrives');
  assert.equal(seen.at(-1).chars, answer.length);
  // Quiet longer than the idle limit: stopped, with what was written.
  await assert.rejects(ask({ system: 's', prompt: 'p', bin: fakeClaude(answer, 1500), idleMs: 300 }), /wrote nothing for 0 s|wrote nothing for/);
  // A slow start within the idle limit is fine, however long the whole answer takes.
  const slow = await ask({ system: 's', prompt: 'p', bin: fakeClaude(answer, 200), idleMs: 1000, timeoutMs: 5000 });
  assert.equal(slow.text, answer);
});

test('the word import in a lesson is fine; loading code is not', () => {
  const lesson = "function component(props) { const pre = h('pre', {}, 'import random\\nfrom typing import Optional'); return pre; }";
  assert.deepEqual(checkShared('component', lesson).filter((p) => /import/.test(p)), []);
  assert.ok(checkShared('component', "function component(props) { const m = require('fs'); return h('div'); }").some((p) => /import or require/.test(p)));
  assert.ok(checkShared('component', "function component(props) { import('x'); return h('div'); }").some((p) => /import or require/.test(p)));
});

test('a reused starter view brings its tables, adding only what the app lacks', () => {
  const library = [{ id: 'lib/view/notes@1', app: 'lib', kind: 'view', title: 'Notes', spec: { tables: [{ name: 'notes', columns: [{ name: 'title', type: 'text' }, { name: 'pinned', type: 'bool' }] }] } },
    { id: 'abc123/view/board@2', app: 'abc123', kind: 'view', title: 'Board', spec: { tables: [{ name: 'cards', columns: [{ name: 'x', type: 'text' }] }] } }];
  const fresh = { tables: [] };
  const p1 = completePlan({ reuse: [{ id: 'lib/view/notes@1', slug: 'notes' }, { id: 'abc123/view/board@2' }] }, fresh, library);
  assert.deepEqual(p1.tables.map((t) => [t.name, t.columns.map((c) => c.name)]), [['notes', ['title', 'pinned']]]);
  const has = { tables: [{ spec: { name: 'notes', columns: [{ name: 'title', type: 'text' }] } }] };
  const p2 = completePlan({ reuse: [{ id: 'lib/view/notes@1' }], tables: [] }, has, library);
  assert.deepEqual(p2.tables.map((t) => [t.name, t.columns.map((c) => c.name)]), [['notes', ['pinned']]]);
  const p3 = completePlan({ reuse: [{ id: 'lib/view/notes@1' }], tables: [{ name: 'notes', columns: [{ name: 'pinned', type: 'bool' }] }] }, has, library);
  assert.deepEqual(p3.tables.map((t) => [t.name, t.columns.map((c) => c.name)]), [['notes', ['pinned']]]);
});

// Each built-in helper's example calls, from library/examples/helpers, run
// where views run (here, in Node) and where functions run (QuickJS).
test('the built-in helpers give the results their examples promise, in the browser and in the server sandbox', async () => {
  const { readdirSync, readFileSync } = await import('node:fs');
  const lib = new URL('../library', import.meta.url).pathname;
  const pieces = (await readLibrary(lib)).filter((p) => p.kind === 'helper');
  const helpers = pieces.map((p) => ({ name: p.name, code: p.code }));
  const near = (a, b) => {
    if (typeof a === 'number' && typeof b === 'number') return Math.abs(a - b) <= 1e-6 * Math.max(1, Math.abs(a), Math.abs(b));
    if (Array.isArray(a) && Array.isArray(b)) return a.length === b.length && a.every((x, i) => near(x, b[i]));
    if (a && b && typeof a === 'object' && typeof b === 'object') {
      const ka = Object.keys(a).filter((k) => a[k] !== undefined).sort();
      const kb = Object.keys(b).filter((k) => b[k] !== undefined).sort();
      return ka.length === kb.length && ka.every((k, i) => k === kb[i] && near(a[k], b[k]));
    }
    return a === b;
  };
  const libs = {};
  for (const h of helpers) libs[h.name] = new Function('lib', `${h.code}\n;return helper();`)(libs);
  let n = 0;
  for (const f of readdirSync(`${lib}/examples/helpers`).filter((x) => x.endsWith('.json'))) {
    const name = f.slice(0, -5);
    const { cases } = JSON.parse(readFileSync(`${lib}/examples/helpers/${f}`, 'utf8'));
    for (const c of cases) {
      const got = JSON.parse(JSON.stringify(libs[name][c.call](...c.args) ?? null));
      assert.ok(near(got, c.expect ?? null), `${name}.${c.call}(${JSON.stringify(c.args)}) -> ${JSON.stringify(got)}, expected ${JSON.stringify(c.expect)}`);
      n++;
    }
    const res = await runFunction(`function run(input, app) { return input.cases.map(function (c) { var v = lib[input.name][c.call].apply(null, c.args); return v === undefined ? null : v; }); }`,
      { name, cases }, async () => null, { helpers });
    res.result.forEach((got, i) => assert.ok(near(JSON.parse(JSON.stringify(got)), cases[i].expect ?? null), `in QuickJS, ${name}.${cases[i].call}(${JSON.stringify(cases[i].args)}) -> ${JSON.stringify(got)}`));
  }
  assert.ok(n >= 150, `${n} cases`);
});

test('a build\'s timeline names what was used, copied, forked, rewritten, written new and kept', async () => {
  const tr = new Trace();
  const plan = tr.start('plan', 'The model plans');
  tr.call(plan, { text: 'x'.repeat(10), cost: 0.2, usage: { input_tokens: 5, cache_read_input_tokens: 100, output_tokens: 7 } }, 1234);
  await tr.span('write', 'view board', async () => {}, { piece: 'board' });
  const json = tr.toJSON();
  assert.equal(json.spans.length, 2);
  assert.deepEqual(json.spans[0].tokens, { in: 5, cached: 100, cacheWrite: 0, out: 7 });
  assert.ok(json.spans.every((s) => s.end >= s.start));

  const view = {
    views: [{ kind: 'view', slug: 'list', id: 'a1/view/list@1', title: 'List' }, { kind: 'view', slug: 'old', id: 'a1/view/old@1', title: 'Old' }],
    functions: [], tables: [{ spec: { name: 'tasks', columns: [{ name: 'title', type: 'text' }] } }],
    shared: [{ kind: 'component', slug: 'dialog', id: 'lib/component/dialog@1', title: 'Dialog' }],
  };
  const shared = new Map([
    ['component/dialog', { id: 'lib/component/dialog@1', title: 'Dialog', author: 'builtin' }],
    ['component/calendar', { id: 'lib/component/calendar@2', title: 'Calendar', author: 'builtin' }],
    ['helper/dates', { id: 'lib/helper/dates@1', title: 'Dates', author: 'builtin' }],
  ]);
  const library = [{ id: 'lib/view/notes@1', app: 'lib', kind: 'view', slug: 'notes', title: 'Notes' }, { id: 'zz/view/plan@3', app: 'zz', kind: 'view', slug: 'plan', title: 'Plan' }];
  const decisions = planDecisions({
    plan: {
      tables: [{ name: 'tasks', columns: [{ name: 'title', type: 'text' }, { name: 'due', type: 'keyword' }] }, { name: 'notes', columns: [{ name: 'body', type: 'text' }] }],
      views: [{ slug: 'list', title: 'List' }, { slug: 'board', title: 'Board' }, { slug: 'week', title: 'Week', forked_from: 'zz/view/plan@3' }],
      reuse: [{ id: 'lib/view/notes@1', slug: 'notes' }],
      components: [{ name: 'calendar', title: 'Calendar', change: 'adds week starts' }, { name: 'kanban', title: 'Kanban' }],
      remove: ['old'],
    },
    view, library, shared,
    pinned: new Map([['component/dialog', 'lib/component/dialog@1'], ['component/calendar', 'lib/component/calendar@3'], ['component/kanban', 'lib/component/kanban@1'], ['helper/dates', 'lib/helper/dates@1']]),
    written: new Map([['component/calendar', 'lib/component/calendar@3'], ['component/kanban', 'lib/component/kanban@1']]),
    code: { board: 'abc', 'ui:kanban': 'abcdef' },
    at: 42,
  });
  const by = (d, name) => decisions.find((x) => x.decision === d && x.name === name);
  assert.ok(by('change', 'tasks'), 'a table gains a column');
  assert.ok(by('new', 'notes') && by('new', 'notes').kind === 'table');
  assert.ok(by('change', 'list'), 'a view the app had is rewritten');
  assert.equal(by('new', 'board').chars, 3, 'a new view, with its size');
  assert.equal(by('fork', 'week').from.title, 'Plan');
  assert.ok(by('copy', 'notes').from.starter, 'a starter copied as it is');
  assert.ok(by('version', 'calendar'), 'a new version of a shared piece');
  assert.equal(by('new', 'kanban').chars, 6);
  assert.ok(by('use', 'dates').from.builtin, 'a built-in helper used as it is');
  assert.ok(by('remove', 'old'));
  assert.ok(by('keep', 'dialog'), 'a shared piece the app had, kept');
  assert.ok(decisions.every((d) => d.t === 42));
});

test('a screen view is data in the plan: checked for shape, expressions, SQL and form columns', async () => {
  const screen = {
    state: { filter: 'all' },
    data: { notes: 'SELECT * FROM notes LIMIT 50' },
    let: { shown: "=notes.filter(n => state.filter === 'all' || n.tag === state.filter)" },
    body: [{ h: 'Notes' }, { form: 'notes', fields: [{ name: 'body', type: 'textarea' }] }, { table: '=shown', columns: [{ key: 'body' }] }, { ui: 'barChart', props: { items: '=[]' } }],
  };
  const plan = { tables: [{ name: 'notes', columns: [{ name: 'body', type: 'text' }, { name: 'tag', type: 'keyword' }] }], views: [{ slug: 'home', title: 'Home', screen }] };
  const code = screensToCode(plan, {});
  assert.equal(JSON.parse(code.home).body[0].h, 'Notes');
  assert.deepEqual(await validate(plan, code, empty, [], new Map([['component/barChart', {}]])), []);
  assert.deepEqual([...sharedRefs(code.home)], ['component/barChart']);
  // A JSON code block for a view is taken as its screen.
  const plan2 = { views: [{ slug: 'home' }] };
  const code2 = screensToCode(plan2, { home: JSON.stringify(screen) });
  assert.ok(plan2.views[0].screen && code2.home.includes('"Notes"'));
  const cols = (t) => (t === 'notes' ? new Set(['id', 'created', 'body']) : null);
  const p = checkScreen({ data: { a: 'DELETE FROM notes', b: '=app.call(' }, body: [{ wat: 1 }, { form: 'tasks', fields: ['x'] }, { form: 'notes', fields: ['body', 'nope'] }, { if: '=a.length >', then: ['ok'] }, { code: 'root.append(1)' }] }, cols);
  for (const want of ['only SELECT', 'data.b', 'unknown block', 'table "tasks"', 'nope is not a column', 'body[3].if', 'render(root, ctx)']) assert.ok(p.some((x) => x.includes(want)), `${want}: ${p.join(' | ')}`);
  assert.deepEqual(checkScreen([]), ['the screen must be a JSON object with "body"']);
});

test('a parameter named like a keyword (:from, :to) is not taken for one', () => {
  const { sql } = rewriteSql('SELECT * FROM events WHERE at >= :from AND at < :to ORDER BY at LIMIT 50', ['events'], P);
  assert.equal(sql, 'SELECT * FROM app_abc123_events AS events WHERE at >= :from AND at < :to ORDER BY at LIMIT 50');
});

test('a change to a screen can be a patch on the paths the model is shown', () => {
  const screen = { data: { notes: 'SELECT * FROM notes LIMIT 50' }, body: [{ h: 'Notes' }, { table: '=notes', columns: [{ key: 'body' }] }] };
  assert.equal(screenLines(JSON.stringify(screen)), '/data/notes: "SELECT * FROM notes LIMIT 50"\n/body/0: {"h":"Notes"}\n/body/1: {"table":"=notes","columns":[{"key":"body"}]}');
  const out = applyPatch(screen, [{ op: 'add', path: '/body/1', value: { text: 'hi' } }, { op: 'add', path: '/body/2/columns/-', value: { key: 'tag' } }, { op: 'add', path: '/state/q', value: '' }, { op: 'move', from: '/body/0', path: '/body/-' }]);
  assert.deepEqual(out, { data: screen.data, body: [{ text: 'hi' }, { table: '=notes', columns: [{ key: 'body' }, { key: 'tag' }] }, { h: 'Notes' }], state: { q: '' } });
  assert.equal(screen.body.length, 2, 'the screen the app has is left as it is');
  assert.throws(() => applyPatch(screen, [{ op: 'remove', path: '/body/5' }]), /patch\[0\].*outside the list/);
  const view = { ...empty, views: [{ slug: 'home', title: 'Home', description: 'd', spec: { order: 2, screen: true }, code: JSON.stringify(screen) }, { slug: 'old', spec: { order: 3 }, code: 'async function render() {}' }] };
  const plan = { views: [{ slug: 'home', patch: [{ op: 'replace', path: '/body/0/h', value: 'My notes' }] }, { slug: 'old', patch: [] }] };
  const problems = patchScreens(plan, view);
  assert.deepEqual(plan.views[0], { slug: 'home', screen: { ...screen, body: [{ h: 'My notes' }, screen.body[1]] }, patched: 1, title: 'Home', order: 2, description: 'd' });
  assert.equal(problems.length, 1);
  assert.match(problems[0], /view old: "patch" changes a screen the app has/);
});

test('a screen pattern expands into blocks, state, data and lets that pass the screen check', async () => {
  const { PATTERNS, expandScreen, fill } = await import('../engine/patterns.mjs');
  assert.ok(PATTERNS.size >= 4);
  const cols = { guests: ['name', 'rsvp', 'table_no'], orders: ['customer', 'items', 'total', 'status'], clients: ['name', 'email', 'rate'], invoices: ['number', 'total', 'status', 'client'], expenses: ['amount', 'category', 'day'], chores: ['title', 'kid', 'points'], chore_done: ['chore', 'day', 'points', 'kid'], events: ['title', 'day', 'at', 'who'], cards: ['question', 'answer', 'deck', 'due', 'interval_days'], questions: ['question', 'options', 'correct', 'explanation', 'deck'], quiz_results: ['score', 'total', 'deck'], notes: ['title', 'body', 'meaning', 'paper', 'authors', 'year', 'tags'], hours: ['day', 'client', 'hours'] };
  const columnsOf = (t) => (cols[t] ? new Set(['id', 'created', ...cols[t]]) : null);
  for (const [name, p] of PATTERNS) {
    const r = expandScreen({ body: [{ h: 'Top' }, p.example] });
    assert.deepEqual(r.problems, [], name);
    assert.deepEqual(r.used, [name]);
    assert.ok(!JSON.stringify(r.screen).includes('{{'), `${name}: a placeholder is left`);
    assert.ok(!JSON.stringify(r.screen).includes('$when'), `${name}: a condition is left`);
    assert.deepEqual(checkScreen(r.screen, columnsOf), [], name);
    assert.equal(r.screen.body[0].h, 'Top');
  }
  // crud: the form goes beside the list; no search box when no search columns;
  // columns come from the fields; the toggle column is not shown twice
  const c = expandScreen({ body: [{ use: 'crud', table: 'tasks', form: 'side', toggle: 'done', fields: [{ name: 'title', required: true }, { name: 'due', type: 'date' }, { name: 'done', type: 'checkbox' }] }] });
  assert.deepEqual(c.problems, []);
  assert.ok(c.screen.body.some((b) => b.split), 'a split');
  const text = JSON.stringify(c.screen);
  assert.ok(!text.includes('"input"'), 'no search box');
  const table = c.screen.body.find((b) => b.split).split[0].col.find((b) => b.table);
  assert.deepEqual(table.columns.map((x) => x.key), ['done', 'title', 'due']);
  assert.equal(table.columns[2].format, 'date');
  assert.equal(c.screen.data.tasks.sql, 'SELECT * FROM tasks ORDER BY created DESC LIMIT 500');
  // mistakes come back as problems the model can fix
  assert.match(expandScreen({ body: [{ use: 'crud', table: 'x' }] }).problems[0], /needs "fields"/);
  assert.match(expandScreen({ body: [{ use: 'crud', table: 'x', fields: ['a'], colums: [] }] }).problems[0], /takes table, .* not colums/);
  assert.match(expandScreen({ body: [{ use: 'grid-of-things' }] }).problems[0], /no screen pattern/);
  assert.match(expandScreen({ data: { guests: 'SELECT 1' }, body: [{ use: 'crud', table: 'guests', fields: ['name'] }] }).problems[0], /has data.guests already/);
  // two of one pattern in a screen, apart by key
  assert.deepEqual(expandScreen({ body: [{ use: 'crud', table: 'a', fields: ['n'] }, { use: 'crud', table: 'a', key: 'a2', fields: ['n'], where: 'n > 1' }] }).problems, []);
  // the template language
  assert.equal(fill('x{{a|pre: WHERE }}y', { a: '' }), 'xy');
  assert.equal(fill('x{{a|pre: WHERE }}y', { a: 'b = 1' }), 'x WHERE b = 1y');
  assert.equal(fill('{{a|json}}.map(k => r[k])', { a: ['p', 'q'] }), '["p","q"].map(k => r[k])');
  assert.deepEqual(fill([1, '{{...a}}', { $when: 'b=2', z: 1 }, { $when: 'b!=2', z: 2 }], { a: [7, 8], b: 2 }), [1, 7, 8, { z: 1 }]);
  assert.deepEqual(fill({ k: { $when: 'm', $value: '{{m}}' } }, {}), {});
});

test('a CREATE SCREEN statement compiles into a screen of patterns that passes the screen check', async () => {
  const { compileStatement, compileStatements } = await import('../engine/statement.mjs');
  const { expandScreen } = await import('../engine/patterns.mjs');
  const src = `CREATE SCREEN guests TITLE 'Guests'
LIST guests
  TITLE 'Guests'  NOUN 'guest'
  ORDER BY name  SEARCH name
  FILTER rsvp IN ('yes', 'no', 'waiting')
  FORM (name REQUIRED, rsvp 'RSVP' SELECT ('yes', 'no', 'waiting') DEFAULT 'waiting',
        table_no 'Table' NUMBER)
  COLUMNS (name, rsvp 'RSVP' BADGE, table_no 'Table')
  KPI 'Coming' = COUNT(*) WHERE rsvp = 'yes'
  ACTION 'Confirm' SET rsvp = 'yes'
BOARD guests AS by_rsvp
  BY rsvp ('yes', 'no', 'waiting')
  NAME name
SUMMARY guests AS tables
  GROUP BY table_no  WHERE rsvp = 'yes'  -- a comment
BLOCKS [{"text": "=guests.length + ' guests'"}]`;
  const c = compileStatement(src);
  assert.equal(c.slug, 'guests');
  assert.equal(c.title, 'Guests');
  assert.deepEqual(c.screen.body.map((b) => b.use || Object.keys(b)[0]), ['crud', 'board', 'summary', 'text']);
  const list = c.screen.body[0];
  assert.equal(list.order, 'name');
  assert.deepEqual(list.search, ['name']);
  assert.deepEqual(list.fields[1], { name: 'rsvp', label: 'RSVP', type: 'select', options: ['yes', 'no', 'waiting'], value: 'waiting' });
  assert.equal(c.screen.data.guests_k0, "SELECT COUNT(*) AS v FROM guests WHERE (rsvp = 'yes')");
  assert.equal(c.screen.body[2].group, 'table_no');
  assert.equal(c.screen.body[2].where, "rsvp = 'yes'");
  const e = expandScreen(c.screen);
  assert.deepEqual(e.problems, []);
  const cols = new Set(['id', 'created', 'name', 'rsvp', 'table_no']);
  assert.deepEqual(checkScreen(e.screen, (t) => (t === 'guests' ? cols : null)), []);
  // mistakes name their line and what to write
  assert.throws(() => compileStatement("CREATE SCREEN x\nLIST t\n  FORM (a WIDGET)"), /line 3: FORM a: cannot read "WIDGET"/);
  assert.throws(() => compileStatement('CREATE SCREEN x\nLIST t\n  SEARCH a'), /needs FORM/);
  // in a plan: the view's code block becomes its screen; a bad one stays as code with a problem
  const plan = { views: [{ slug: 'guests' }, { slug: 'bad' }] };
  const code = { guests: src, bad: 'CREATE SCREEN bad\nLIST t\n  NOPE' };
  const problems = compileStatements(plan, code);
  assert.ok(plan.views[0].screen && plan.views[0].statement && !code.guests);
  assert.match(problems[0], /^view bad: CREATE SCREEN line 3/);
  assert.ok(code.bad.startsWith('CREATE SCREEN'));
});

test('a rule is a function as data: queries return rows, steps write, and :row.col reads the trigger row', async () => {
  const calls = [];
  const votes = { f1: 3 };
  const host = async (method, args) => {
    calls.push([method, ...args]);
    if (method === 'sql') {
      if (/COUNT/.test(args[0])) return [{ n: votes[args[1].row_feature] }];
      if (/stock/.test(args[0])) return [{ stock: 2 }];
      if (/AS no/.test(args[0])) return [{ no: 0 }];
      return [{ total: 42, n: 2 }, { total: 7, n: 1 }];
    }
    if (method === 'insert') return { id: 'm1', ...args[1] };
    return {};
  };
  assert.deepEqual(await runRule({ query: 'SELECT SUM(amount) AS total FROM expenses WHERE day >= :from' }, { from: '2026-11-01', unused: 1 }, host), [{ total: 42, n: 2 }, { total: 7, n: 1 }]);
  assert.deepEqual(calls.at(-1), ['sql', 'SELECT SUM(amount) AS total FROM expenses WHERE day >= :from', { from: '2026-11-01' }]);
  assert.deepEqual(await runRule({ query: 'SELECT 1', one: true }, {}, host), { total: 42, n: 2 });
  assert.equal(await runRule({ query: 'SELECT 1', value: true }, {}, host), 42);
  const out = await runRule({ steps: [
    { update: 'features', id: ':row.feature', set: { votes: 'SELECT COUNT(*) AS n FROM votes WHERE feature = :row.feature', touched: true } },
    { let: 'left', value: 'SELECT stock FROM items WHERE id = :row.item' },
    { insert: 'moves', values: { item: ':row.item', qty: ':left', by: ':user.id', note: "it's :row.feature" } },
    { remove: 'holds', id: ':row.hold', if: 'SELECT 0 AS no' },
  ] }, { row: { feature: 'f1', item: 'i9', hold: 'h1' }, user: { id: 'u7' } }, host);
  assert.deepEqual(out.done, ['updated features f1', 'added to moves m1']);
  assert.ok(calls.some((c) => c[0] === 'sql' && c[1] === 'SELECT COUNT(*) AS n FROM votes WHERE feature = :row_feature' && c[2].row_feature === 'f1'));
  assert.deepEqual(calls.find((c) => c[0] === 'update'), ['update', 'features', 'f1', { votes: 3, touched: true }]);
  assert.deepEqual(calls.find((c) => c[0] === 'insert'), ['insert', 'moves', { item: 'i9', qty: 2, by: 'u7', note: "it's :row.feature" }]);
  assert.ok(!calls.some((c) => c[0] === 'remove'), 'the step whose if is 0 does not run');
});

test('a rule is checked against the tables it writes, and needs no writer', async () => {
  const cols = (t) => (t === 'features' ? new Set(['id', 'created', 'votes']) : null);
  assert.deepEqual(checkRule({ query: 'SELECT 1' }), []);
  const p = checkRule({ steps: [
    { update: 'features', set: { votes: 1, nope: 2 } },
    { insert: 'ghosts', values: { a: 1 } },
    { update: 'features', id: ':row.feature', set: { votes: 'DELETE FROM features' } },
    { wat: 1 },
  ] }, cols).join('\n');
  for (const want of ['give "id"', 'nope is not a column of features', 'no table "ghosts"', 'only SELECT here', 'a step is update, insert, remove, call or let']) assert.ok(p.includes(want), `missing ${want} in:\n${p}`);
  assert.match(checkRule({ query: 'UPDATE x SET a = 1' }).join(), /give a SELECT/);
  // In a plan: the rule becomes the function's code, is checked, and gets no writer.
  const plan = { tables: [{ name: 'features', columns: [{ name: 'votes', type: 'int' }] }, { name: 'votes', columns: [{ name: 'feature', type: 'keyword' }] }],
    views: [{ slug: 'home', title: 'Home', screen: { body: [{ h: 'Hi' }] } }], functions: [{ slug: 'count-votes', on: { table: 'votes', event: 'insert' }, rule: { steps: [{ update: 'features', id: ':row.feature', set: { votes: 'SELECT COUNT(*) FROM votes WHERE feature = :row.feature' } }] } },
      { slug: 'bad', rule: { steps: [{ insert: 'features', values: { stars: 5 } }] } }] };
  const code = {};
  assert.deepEqual(prepareScreens(plan, code, empty), []);
  assert.equal(JSON.parse(code['count-votes']).steps[0].update, 'features');
  const problems = await validate(plan, code, empty, []);
  assert.deepEqual(problems, ['function bad: steps[0]: stars is not a column of features']);
  // A rule given as JSON in a code block is a rule too; JavaScript stays JavaScript.
  const p2 = { functions: [{ slug: 'a' }, { slug: 'b' }] };
  rulesToCode(p2, { a: '{"query": "SELECT 1"}', b: 'function run(input, app) { return 1 }' });
  assert.deepEqual(p2.functions[0].rule, { query: 'SELECT 1' });
  assert.equal(p2.functions[1].rule, undefined);
});

test('edits change a JavaScript view or function by find and replace, and refuse what does not match once', () => {
  const src = "function render(root) {\n  root.append(h('h2', { style: 'color: #1d4ed8' }, 'Notes'));\n  root.append(h('p', {}, 'Notes'));\n}";
  const view = {
    views: [{ slug: 'notes', title: 'Notes', description: 'd', code: src, spec: { order: 2 } }, { slug: 'board', title: 'Board', code: '{}', spec: { screen: true } }],
    functions: [{ slug: 'total', code: '{"query": "SELECT 1"}', spec: { rule: true } }],
  };
  const plan = { views: [{ slug: 'notes', edits: [{ old: 'color: #1d4ed8', new: 'color: #db2777' }] }] };
  const code = {};
  assert.deepEqual(editCode(plan, code, view), []);
  assert.ok(code.notes.includes('color: #db2777') && !code.notes.includes('#1d4ed8'));
  assert.equal(plan.views[0].title, 'Notes');
  assert.equal(plan.views[0].order, 2);
  assert.equal(plan.views[0].edits, undefined);
  // "Notes" occurs twice, "purple" not at all: both are refused with how to fix it
  assert.match(editCode({ views: [{ slug: 'notes', edits: [{ old: "'Notes'", new: "'Diary'" }] }] }, {}, view)[0], /found 2 times/);
  assert.match(editCode({ views: [{ slug: 'notes', edits: [{ old: 'purple', new: 'red' }] }] }, {}, view)[0], /found 0 times/);
  // a replacement with $ signs is taken as it is
  const c2 = {};
  editCode({ views: [{ slug: 'notes', edits: [{ old: 'color: #1d4ed8', new: "content: '$&'" }] }] }, c2, view);
  assert.ok(c2.notes.includes("content: '$&'"));
  // screens take patches, rules are given whole, and a code block with edits is one too many
  assert.match(editCode({ views: [{ slug: 'board', edits: [{ old: '{}', new: '[]' }] }] }, {}, view)[0], /a screen takes "patch"/);
  assert.match(editCode({ functions: [{ slug: 'total', edits: [{ old: '1', new: '2' }] }] }, {}, view)[0], /a rule is given whole/);
  assert.match(editCode({ views: [{ slug: 'notes', edits: [{ old: 'Notes', new: 'x' }] }] }, { notes: 'x' }, view)[0], /not both/);
});

test('tests and rules name values as :name, and an =expression is refused at build time', async () => {
  const { checkTests } = await import('../engine/checks.mjs');
  const cols = (t) => ({ hours: new Set(['id', 'client', 'project']), projects: new Set(['id', 'client']) })[t] || null;
  const good = [{ name: 'fills the client', do: [{ insert: 'projects', values: { client: 'c1' }, as: 'p' }, { insert: 'hours', values: { project: ':p.id' }, as: 'h' }], expect: [{ query: 'SELECT client FROM hours WHERE id = :h.id', is: ':p.client' }] }];
  assert.deepEqual(checkTests(good, cols), []);
  const bad = [{ name: 'x', do: [{ insert: 'hours', values: { project: '=p.id' } }], expect: [{ query: 'SELECT client FROM hours', is: '=c.id' }] }];
  const problems = checkTests(bad, cols);
  assert.equal(problems.length, 2);
  assert.match(problems.join('\n'), /expect\[0\]\.is: a value, or ":name.col"/);
  assert.match(problems.join('\n'), /project: a value here is a SELECT, ":name" or a literal; not an =expression/);
  assert.match(checkRule({ steps: [{ update: 'hours', id: '=row.id', set: { client: 'x' } }] }, cols)[0], /steps\[0\]\.id: .* not an =expression/);
});

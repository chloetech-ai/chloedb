// Tests that need neither Strata nor the model: the SQL rewrite, the answer
// parser, the checks on a plan, and the function sandbox.

import test from 'node:test';
import assert from 'node:assert/strict';
import { rewriteSql } from '../engine/sql.mjs';
import { parseAnswer, ask } from '../engine/llm.mjs';
import { writeFileSync, chmodSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { checkBrief, orderByBrief, normalizeBrief, briefOf, validate, uiRefs, pinShared, checkShared, sharedRefs, checkView, completePlan, checkScreen, screensToCode, applyPatch, patchScreens, screenLines, rulesToCode, prepareScreens, editCode, smallChange, SYSTEM, SUGGEST_AFTER } from '../engine/forge.mjs';
import { expandScreen as expandWith } from '../engine/patterns.mjs';
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
// A new app's name and UX brief, which every first plan gives.
const brief = { name: 'Notes', ux: { who: 'me, on my phone', job: 'jot a note', often: 'daily', device: 'phone', first: 'home', glance: 'the newest notes' } };

test('a plan is checked before anything is stored', async () => {
  const good = { ...brief, tables: [{ name: 'notes', columns: [{ name: 'body', type: 'text' }] }], views: [{ slug: 'home', title: 'Home' }] };
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
    if (p.kind === 'pattern') {
      // A pattern's example expands into a screen that passes the checks.
      const pat = JSON.parse(p.code);
      assert.ok(p.meta.title && p.meta.description, `${p.name}: title and about`);
      const { use, ...given } = pat.example;
      const r = expandWith({ body: [{ use: p.name, ...given }] }, new Map([[p.name, pat]]));
      assert.deepEqual(r.problems, [], p.name);
      assert.deepEqual(checkScreen(r.screen, () => ({ has: () => true })), [], p.name);
      continue;
    }
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
  const plan = { ...brief, tables: [{ name: 'notes', columns: [{ name: 'body', type: 'text' }, { name: 'tag', type: 'keyword' }] }], views: [{ slug: 'home', title: 'Home', screen }] };
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
  const cols = { guests: ['name', 'rsvp', 'table_no'], orders: ['customer', 'items', 'total', 'status'], clients: ['name', 'email', 'rate'], invoices: ['number', 'total', 'status', 'client'], expenses: ['amount', 'category', 'day'], chores: ['title', 'kid', 'points'], chore_done: ['chore', 'day', 'points', 'kid'], events: ['title', 'day', 'at', 'who'], cards: ['question', 'answer', 'deck', 'due', 'interval_days'], questions: ['question', 'options', 'correct', 'explanation', 'deck'], quiz_results: ['score', 'total', 'deck'], notes: ['title', 'body', 'meaning', 'paper', 'authors', 'year', 'tags'], hours: ['day', 'client', 'hours'], habits: ['name'], checkins: ['habit', 'day', 'skipped'] };
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
  // A column's value is an expression of row; a function of the row is taken as written, not wrapped again.
  const valued = compileStatement("CREATE SCREEN x\nLIST t\n  FORM (a NUMBER)\n  COLUMNS (a 'A' = row.a * 2, b 'B' = r => r.a + 1, c 'C' = (row) => row.a)").screen.body[0].columns;
  assert.deepEqual(valued.map((x) => x.value), ['=row => row.a * 2', '=r => r.a + 1', '=(row) => row.a']);
  // Formatting is said with words, not written as code: a KPI over any table, a value per
  // period (TREND), how a column or a KPI reads (DAYS, DURATION, MONEY...).
  const w = compileStatement(`CREATE SCREEN runs TITLE 'Runs'
KPI 'This month' = SUM(distance_km) FROM runs WHERE date_trunc('month', day) = date_trunc('month', :today)
KPI 'Owed' = SUM(amount) FROM bills WHERE paid = false MONEY
LIST runs
  FORM (day DATE, distance_km NUMBER, minutes NUMBER)
  COLUMNS (day 'Day' DAYS, minutes 'Time' DURATION)
  KPI 'Total' = SUM(minutes) DURATION
  ACTION 'Remove' DELETE
TREND runs AS weekly
  DATE day  VALUE SUM(distance_km)  PER week  LAST 8  LINE`);
  assert.deepEqual(w.screen.body.map((b) => b.use || Object.keys(b)[0]), ['kpis', 'crud', 'trend']);
  assert.deepEqual(w.screen.body[0].kpis, [{ label: 'This month', of: 'kpi_0', format: 'number' }, { label: 'Owed', of: 'kpi_1', format: 'money' }]);
  assert.deepEqual(w.screen.data.kpi_0.params, { today: '=today' });
  assert.equal(w.screen.data.kpi_1, 'SELECT SUM(amount) AS v FROM bills WHERE paid = false');
  assert.deepEqual(w.screen.body[1].columns.map((x) => x.format), ['days', 'duration']);
  assert.equal(w.screen.body[1].deleteLabel, 'Remove', 'ACTION DELETE names the delete button every list has');
  assert.equal(w.screen.body[1].actions, undefined);
  assert.deepEqual(w.screen.body[2], { use: 'trend', table: 'runs', key: 'weekly', date: 'day', value: 'SUM(distance_km)', per: 'week', last: 8, line: true });
  const we = expandScreen(w.screen);
  assert.deepEqual(we.problems, []);
  assert.match(JSON.stringify(we.screen.data), /date_trunc\('week', day\)/);
  assert.equal((JSON.stringify(we.screen).match(/"Remove"/g) || []).length, 1);
  assert.deepEqual(checkScreen(we.screen, (t) => ({ runs: new Set(['id', 'created', 'day', 'distance_km', 'minutes']), bills: new Set(['id', 'amount', 'paid']) })[t] || null), []);
  assert.throws(() => compileStatement("CREATE SCREEN x\nTREND t\n  PER year"), /PER/);
  // A field FROM a table picks among its rows (the choices people add), not a fixed list.
  const pick = compileStatement("CREATE SCREEN s\nLIST spending\n  FORM (category 'Category' FROM categories (name) REQUIRED, client FROM clients, amount NUMBER)").screen;
  assert.equal(pick.data.categories_name_choices, 'SELECT name AS value, name AS label FROM categories ORDER BY name LIMIT 500');
  assert.equal(pick.data.clients_id_name_choices, 'SELECT id AS value, name AS label FROM clients ORDER BY name LIMIT 500');
  assert.deepEqual(pick.body[0].fields.map((f) => [f.name, f.type, String(f.options).includes('_choices')]), [['category', 'select', true], ['client', 'select', true], ['amount', 'number', false]]);
  assert.ok(!pick.body[0].fields.some((f) => f.from));
  // the same said as a subquery in the options (a round 14 app showed the SQL as its one choice)
  const sub = compileStatement("CREATE SCREEN s\nLIST requests\n  FORM (flat 'Flat' SELECT ((SELECT id, name FROM flats ORDER BY name)), title)").screen;
  assert.equal(sub.data.flats_id_name_choices, 'SELECT id AS value, name AS label FROM flats ORDER BY name LIMIT 500');
  assert.throws(() => compileStatement("CREATE SCREEN s\nLIST r\n  FORM (flat (SELECT id FROM flats WHERE x = 1))"), /flat FROM <table>/);
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

test('CREATE SCREEN says the week, calendar, flashcards, quiz, search and dashboard patterns too', async () => {
  const { compileStatement } = await import('../engine/statement.mjs');
  const { expandScreen } = await import('../engine/patterns.mjs');
  const src = `CREATE SCREEN home TITLE 'Home'
STATE deck = "Spanish"
WEEK chore_done AS chart
  ITEMS chores  ITEM chore  DAY day  NAME title  SUB kid  LABEL 'Done'  EXTRA {points: it.points, kid: it.kid}
CALENDAR events
  DATE day  NAME title  TIME at  COLOR who  NOUN 'event'
  FORM (title REQUIRED, day DATE, at 'Time', who)
FLASHCARDS cards
  FRONT question  BACK answer  INTERVAL interval_days  MASTERED 21
  WHERE deck = :deck  PARAM deck = state.deck
QUIZ questions
  QUESTION question  CHOICES options  ANSWER correct  EXPLANATION explanation  COUNT 5
  RESULTS quiz_results  EXTRA {deck: state.deck}
FIND notes
  TEXT body  MEANING meaning  NAME title  SUB paper, year  SHOW (paper, authors 'By', tags)
  PLACEHOLDER 'Search notes'  LIMIT 10
DASHBOARD hours
  DAY day  VALUE SUM(hours)  LABEL 'Hours'  FORMAT hours  DAYS 30  BY client  RECENT (day, client, hours)`;
  const c = compileStatement(src);
  assert.deepEqual(c.screen.body.map((b) => b.use), ['week', 'calendar', 'flashcards', 'quiz', 'search', 'dashboard']);
  const [week, cal, cards, quiz, find, dash] = c.screen.body;
  assert.deepEqual({ table: week.table, key: week.key, items: week.items, item: week.item, name: week.name, sub: week.sub, label: week.label, extra: week.extra }, { table: 'chore_done', key: 'chart', items: 'chores', item: 'chore', name: 'title', sub: 'kid', label: 'Done', extra: '{points: it.points, kid: it.kid}' });
  assert.equal(cal.fields.length, 4);
  assert.deepEqual(cal.fields[1], { name: 'day', type: 'date' });
  assert.equal(cards.where, 'deck = :deck');
  assert.deepEqual(cards.params, { deck: '=state.deck' });
  assert.equal(cards.mastered, 21);
  assert.equal(quiz.count, 5);
  assert.equal(quiz.resultExtra, '{deck: state.deck}');
  assert.deepEqual(find.sub, ['paper', 'year']);
  assert.deepEqual(find.show.map((x) => x.key || x), ['paper', 'authors', 'tags']);
  assert.equal(find.limit, 10);
  assert.deepEqual({ date: dash.date, day: dash.day, value: dash.value, format: dash.format, days: dash.days, group: dash.group }, { date: 'day', day: true, value: 'SUM(hours)', format: 'hours', days: 30, group: 'client' });
  const e = expandScreen(c.screen);
  assert.deepEqual(e.problems, []);
  const cols = { chores: ['title', 'kid', 'points'], chore_done: ['chore', 'day', 'points', 'kid'], events: ['title', 'day', 'at', 'who'], cards: ['question', 'answer', 'deck', 'due', 'interval_days'], questions: ['question', 'options', 'correct', 'explanation', 'deck'], quiz_results: ['score', 'total', 'deck'], notes: ['title', 'body', 'meaning', 'paper', 'authors', 'year', 'tags'], hours: ['day', 'client', 'hours'] };
  assert.deepEqual(checkScreen(e.screen, (t) => (cols[t] ? new Set(['id', 'created', ...cols[t]]) : null)), []);
  // what a part needs, and a clause it does not take, name the line
  assert.throws(() => compileStatement('CREATE SCREEN x\nWEEK marks\n  ITEMS things'), /ITEM/);
  assert.throws(() => compileStatement('CREATE SCREEN x\nFIND notes\n  NAME title'), /TEXT/);
  assert.throws(() => compileStatement('CREATE SCREEN x\nQUIZ q\n  COUNT many'), /line 3/);
});

test('a screen written as a statement keeps it as its source: changes edit the statement, patches are refused', async () => {
  const { compileStatement } = await import('../engine/statement.mjs');
  const statement = "CREATE SCREEN guests TITLE 'Guests'\nLIST guests\n  FORM (name REQUIRED, rsvp SELECT ('yes', 'no'))\n  COLUMNS (name, rsvp BADGE)";
  const screen = compileStatement(statement).screen;
  const view = { ...empty, views: [{ slug: 'guests', title: 'Guests', description: 'd', spec: { order: 1, screen: true, statement }, code: JSON.stringify(screen) }] };
  // edits change the statement; it is compiled again into the screen
  const plan = { views: [{ slug: 'guests', edits: [{ old: "rsvp SELECT ('yes', 'no')", new: "rsvp SELECT ('yes', 'no', 'maybe')" }] }] };
  const code = {};
  assert.deepEqual(prepareScreens(plan, code, view), []);
  assert.ok(plan.views[0].screen && code.guests.startsWith('{'), 'the view\'s code is the compiled screen');
  assert.match(plan.views[0].statement, /'maybe'/);
  assert.ok(code.guests.includes('"maybe"'), 'the compiled screen has the new option');
  // a new statement as the view's code replaces it the same way
  const plan2 = { views: [{ slug: 'guests', title: 'Guests' }] };
  const code2 = { guests: statement.replace("COLUMNS (name, rsvp BADGE)", "COLUMNS (name)") };
  assert.deepEqual(prepareScreens(plan2, code2, view), []);
  assert.ok(plan2.views[0].statement.endsWith('COLUMNS (name)') && !code2.guests.includes('"badge"'));
  // a JSON patch would leave the statement behind: refused, with what to give
  const plan3 = { views: [{ slug: 'guests', patch: [{ op: 'replace', path: '/body/0/title', value: 'x' }] }] };
  const problems = prepareScreens(plan3, {}, view);
  assert.equal(problems.length, 1);
  assert.match(problems[0], /CREATE SCREEN statement/);
  // a statement that does not compile stays a problem naming its line
  const bad = prepareScreens({ views: [{ slug: 'guests', edits: [{ old: 'COLUMNS', new: 'COLUMNZ' }] }] }, {}, view);
  assert.match(bad[0], /line 4/);
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
  for (const want of ['give "id"', 'nope is not a column of features', 'no table "ghosts"', 'only SELECT here', 'a step is update, insert, upsert, remove, call, email, let, each, require or return']) assert.ok(p.includes(want), `missing ${want} in:\n${p}`);
  assert.match(checkRule({ query: 'UPDATE x SET a = 1' }).join(), /give a SELECT/);
  // In a plan: the rule becomes the function's code, is checked, and gets no writer.
  const plan = { ...brief, tables: [{ name: 'features', columns: [{ name: 'votes', type: 'int' }] }, { name: 'votes', columns: [{ name: 'feature', type: 'keyword' }] }],
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
  assert.match(editCode({ views: [{ slug: 'board', edits: [{ old: '{}', new: '[]' }] }] }, {}, view)[0], /a screen shown as JSON takes "patch"/);
  assert.match(editCode({ functions: [{ slug: 'total', edits: [{ old: '1', new: '2' }] }] }, {}, view)[0], /a rule shown as JSON is given whole/);
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

test('a rule sends its email once its writes are stored, and none when it stops', async () => {
  const { Engine } = await import('../engine/engine.mjs');
  const { compileRule } = await import('../engine/rulesql.mjs');
  const order = [];
  const engine = new Engine({ write: async (ops) => { order.push(`write ${ops.map((o) => `${o.op} ${o.table}`).join(', ')}`); }, get: async () => [] }, { log: () => {} });
  engine.store = { track: async () => {} };
  engine.services = { email: async (app, msg) => { order.push(`email ${msg.to.join(', ')}: ${msg.subject}`); return { status: 'held' }; } };
  engine.sql = async (app, sql, params) => [{ v: params.ok ?? true }];
  const fn = (slug, src) => ({ id: `a/function/${slug}`, slug, code: JSON.stringify(compileRule(src).rule), spec: { rule: true } });
  const view = { comp: { n: 1 }, helpers: [], tables: [{ spec: { name: 'requests', columns: [{ name: 'title', type: 'text' }] } }], functions: [
    fn('add', "CREATE RULE add AS INSERT INTO requests (title) VALUES (:title); SEND EMAIL TO :to SUBJECT 'New: :title';"),
    fn('add-checked', "CREATE RULE add_checked AS SEND EMAIL TO :to SUBJECT 'New'; INSERT INTO requests (title) VALUES (:title); REQUIRE :ok ELSE 'Not now';"),
  ] };
  engine.view = async () => view;
  await engine.call('a', 'add', { title: 'Dark mode', to: 'a@b.co' });
  assert.deepEqual(order, ['write insert app_a_requests', 'email a@b.co: New: Dark mode']);
  await assert.rejects(engine.call('a', 'add-checked', { title: 'x', to: 'a@b.co', ok: false }), /Not now/);
  assert.equal(order.length, 2, 'a rule that stops writes nothing and sends nothing');
});

test('rule steps loop over rows, refuse with a message, insert or update, and return a value', async () => {
  const { Refused } = await import('../engine/rules.mjs');
  const writes = [];
  const stock = { p1: 5, p2: 1 };
  let big = false;
  const host = async (method, args) => {
    if (method === 'sql') {
      const [q, p] = args;
      if (/FROM cart/.test(q) && /SELECT product/.test(q)) return [{ product: 'p1', qty: 2 }, { product: 'p2', qty: big ? 3 : 1 }];
      if (/stock >= :line_qty/.test(q)) return [{ ok: stock[p.line_product] >= p.line_qty }];
      if (/SELECT stock - :line_qty/.test(q)) return [{ s: stock[p.line_product] - p.line_qty }];
      if (/SELECT stock FROM products/.test(q)) return [{ stock: stock[p.line_product] }];
      if (/SELECT id FROM cart WHERE product = :product/.test(q)) return p.product === 'p1' ? [{ id: 'c1' }] : [];
      return [];
    }
    writes.push([method, ...args]);
    return method === 'insert' ? { id: `new-${writes.length}`, ...args[1] } : { id: args[1], ...args[2] };
  };
  const checkout = { steps: [
    { insert: 'orders', values: { status: 'new' }, as: 'order' },
    { each: 'SELECT product, qty FROM cart WHERE owner = :user.id', as: 'line', do: [
      { let: 'stock', value: 'SELECT stock FROM products WHERE id = :line.product' },
      { require: 'SELECT stock >= :line.qty AS ok FROM products WHERE id = :line.product', else: 'Only :stock left of :line.product' },
      { insert: 'order_lines', values: { order: ':order.id', product: ':line.product', qty: ':line.qty' } },
      { update: 'products', id: ':line.product', set: { stock: 'SELECT stock - :line.qty FROM products WHERE id = :line.product' } }] },
    { return: ':order.id' }] };
  assert.equal(await runRule(checkout, { user: { id: 'u1' } }, host), 'new-1');
  assert.deepEqual(writes.map((w) => w[0] + ' ' + w[1]), ['insert orders', 'insert order_lines', 'update products', 'insert order_lines', 'update products']);
  assert.deepEqual(writes[2][3], { stock: 3 });
  // a refusal names what is short; the engine writes nothing of it (its batch is never committed)
  big = true;
  await assert.rejects(runRule(checkout, { user: { id: 'u1' } }, host), (e) => e instanceof Refused && e.refused && e.message === 'Only 1 left of p2');
  // upsert: the row found is updated, otherwise one is added
  writes.length = 0;
  const add = { steps: [{ upsert: 'cart', find: 'SELECT id FROM cart WHERE product = :product', values: { product: ':product', qty: 1 } }] };
  await runRule(add, { product: 'p1' }, host);
  await runRule(add, { product: 'p3' }, host);
  assert.deepEqual(writes.map((w) => `${w[0]} ${w[1]} ${w[0] === 'update' ? w[2] : w[2].product}`), ['update cart c1', 'insert cart p3']);
  // the checks know the new steps
  const cols = (t) => ({ orders: new Set(['id', 'status']), order_lines: new Set(['id', 'order', 'product', 'qty']), products: new Set(['id', 'stock']), cart: new Set(['id', 'product', 'qty']) })[t] || null;
  assert.deepEqual(checkRule(checkout, cols), []);
  assert.deepEqual(checkRule(add, cols), []);
  assert.match(checkRule({ steps: [{ each: 'SELECT 1', as: 'x' }] }, cols)[0], /give "do"/);
  assert.match(checkRule({ steps: [{ require: 'stock > 1' }] }, cols)[0], /require: give a SELECT/);
  assert.match(checkRule({ steps: [{ upsert: 'cart', values: { qty: 1 } }] }, cols)[0], /find: give a SELECT/);
  assert.match(checkRule({ steps: [{ each: 'SELECT 1', do: [{ insert: 'cart', values: { color: 'red' } }] }] }, cols)[0], /steps\[0\]\.do\[0\]: color is not a column of cart/);
  // SEND EMAIL: a statement of its own, with a line per row; a bad address refuses before anything is sent
  const { compileRule, ruleStatement } = await import('../engine/rulesql.mjs');
  const digest = compileRule(`CREATE RULE digest AS
    SEND EMAIL TO :to SUBJECT 'New for :team' TEXT 'This week:' ROWS (SELECT title, votes FROM requests ORDER BY votes DESC) NONE 'Nothing new.';`);
  assert.deepEqual(digest.rule.steps, [{ email: { to: ':to', subject: 'New for :team', text: 'This week:', rows: 'SELECT title, votes FROM requests ORDER BY votes DESC', none: 'Nothing new.' } }]);
  assert.ok(ruleStatement('digest', digest.rule, null), 'it reads back as the same statement');
  assert.deepEqual(checkRule(digest.rule, cols), []);
  const sent = [];
  let rows = [{ title: 'Dark mode', votes: 4 }, { title: 'CSV', votes: 2 }];
  const mail = async (method, args) => { if (method === 'sql') return /FROM people/.test(args[0]) ? [{ email: 'o@x.co' }, { email: 'p@x.co' }] : rows; if (method === 'email') sent.push(args[0]); return {}; };
  await runRule(digest.rule, { to: 'a@b.co, c@d.co', team: 'Ops' }, mail);
  rows = [];
  await runRule(digest.rule, { to: 'a@b.co', team: 'Ops' }, mail);
  assert.deepEqual(sent, [
    { to: ['a@b.co', 'c@d.co'], subject: 'New for Ops', text: 'This week:\n\n- Dark mode · 4\n- CSV · 2' },
    { to: ['a@b.co'], subject: 'New for Ops', text: 'This week:\n\nNothing new.' }]);
  await assert.rejects(runRule(digest.rule, { to: 'nope' }, mail), (e) => e.refused && /nope is not an email address/.test(e.message));
  const owners = compileRule("CREATE RULE popular AFTER INSERT ON requests AS IF :row.votes > 3 THEN SEND EMAIL TO (SELECT email FROM people WHERE role = 'owner') SUBJECT 'Popular: :row.title'; END;");
  sent.length = 0;
  await runRule(owners.rule, { row: { votes: 5, title: 'Dark mode' } }, async (m, a) => (m === 'sql' && /^SELECT :row_votes > 3/.test(a[0]) ? [{ x: true }] : mail(m, a)));
  assert.deepEqual(sent, [{ to: ['o@x.co', 'p@x.co'], subject: 'Popular: Dark mode', text: '' }]);
  assert.throws(() => compileRule("CREATE RULE x AS SEND EMAIL SUBJECT 'Hi';"), /say who it goes to/);
});

test('a design is a few tokens made readable, drawn as CSS variables for light and dark', async () => {
  const { checkDesign, designCss, presetDesigns, contrast, PRESETS } = await import('../web/design.js');
  const d = checkDesign({ name: 'Pale', light: { bg: '#ffffff', card: '#ffffff', fg: '#bbbbbb', accent: '#ffee00' }, font: 'comic', nav: 'sidebar' });
  assert.ok(contrast(d.light.fg, d.light.bg) >= 7, 'text reads on the page');
  assert.ok(contrast(d.light.accent, d.light.card) >= 4.5, 'the accent reads on cards');
  assert.equal(d.font, PRESETS[0].font, 'an unknown token falls back');
  assert.equal(d.nav, 'sidebar');
  const css = designCss(d, '.stage');
  assert.match(css, /^\.stage \{ --bg: #ffffff;/);
  assert.match(css, /@media \(prefers-color-scheme: dark\) \{ \.stage \{/);
  assert.match(css, /--card-border: var\(--line\)/, 'a card the color of its page gets a hairline');
  assert.ok(!designCss({ ...d, scheme: 'dark' }).includes('@media'), 'a dark design is dark in both');
  const three = presetDesigns('abc123');
  assert.deepEqual(three.map((x) => x.nav), ['segmented', 'tabs', 'sidebar']);
  assert.notDeepEqual(presetDesigns('abc123').map((x) => x.name), presetDesigns('zz9').map((x) => x.name).reverse());
});

test('the bold look: a deep color white text reads on, a light one that reads on it, a page tinted by the accent', async () => {
  const { checkDesign, designVars, boldColors, contrast, PRESETS } = await import('../web/design.js');
  assert.equal(checkDesign(PRESETS[0]).look, 'plain', 'plain unless a design says otherwise');
  assert.equal(checkDesign({ look: 'loud' }).look, 'plain', 'an unknown look falls back');
  for (const p of PRESETS) {
    const d = checkDesign({ ...p, look: 'bold' });
    for (const [which, dark] of [['light', false], ['dark', true]]) {
      const b = boldColors(d[which], dark);
      assert.ok(contrast(b.deep, b.deepFg) >= 7, `${p.name} ${which}: words on the band read`);
      assert.ok(contrast(b.pop, b.deep) >= 4.5, `${p.name} ${which}: the big word on the band reads`);
    }
    const v = designVars(d, 'light');
    assert.equal(v['--look'], 'bold');
    assert.equal(v['--bg'], v['--wash'], 'the page is the wash');
    assert.equal(designVars(p, 'light')['--bg'], checkDesign(p).light.bg, 'a plain design keeps its page');
  }
});

test('an app gets three designs from one model call, presets when the model fails, and new ones keep the one in use', async () => {
  const { makeDesigns, ensureDesigns } = await import('../engine/designs.mjs');
  const app = { id: 'a1', name: 'Guests', description: 'A wedding guest list', head: 0 };
  const answer = JSON.stringify([{ name: 'Linen', light: { bg: '#faf6f5', card: '#ffffff', fg: '#221a1c', accent: '#a8485f' } }, { name: 'Paper', titles: 'serif' }, { name: 'Gilt', scheme: 'dark', dark: { bg: '#0d0b08', card: '#19160f', fg: '#f5efe2', accent: '#e0b45a' } }]);
  let prompt = '';
  const out = await makeDesigns(app, null, { ask: async (a) => { prompt = a.prompt; return { text: `Here: ${answer}`, cost: 0.002 }; } });
  assert.deepEqual(out.designs.map((x) => x.name), ['Linen', 'Paper', 'Gilt']);
  assert.equal(out.designs[2].light.bg, '#0d0b08', 'a dark design is dark in both schemes');
  assert.match(prompt, /A wedding guest list/);
  const bad = await makeDesigns(app, null, { ask: async () => ({ text: 'no idea' }) });
  assert.equal(bad.source, 'presets');
  assert.equal(bad.designs.length, 3);
  // stored once; asking again returns what is stored; fresh ones keep the chosen slot
  const rows = new Map();
  const store = { designs: async (id) => rows.get(id) || null, putDesigns: async (id, v) => rows.set(id, v), getApp: async () => app, resolve: async () => null };
  let calls = 0;
  const make = async () => { calls++; return { designs: [{ name: `N${calls}a` }, { name: `N${calls}b` }, { name: `N${calls}c` }], source: 'm' }; };
  await Promise.all([ensureDesigns(store, 'a1', { make }), ensureDesigns(store, 'a1', { make })]);
  assert.equal(calls, 1, 'one generation at a time');
  rows.set('a1', { ...rows.get('a1'), chosen: 1 });
  const fresh = await ensureDesigns(store, 'a1', { make, fresh: true });
  assert.deepEqual(fresh.designs.map((x) => x.name), ['N2a', 'N1b', 'N2b']);
  assert.equal(fresh.chosen, 1);
});

// ---- evolving from use

import { signals, measureFor, measureEffect, mergeCaps, Evolver } from '../engine/evolve.mjs';

// Events as the store gives them: newest first.
const at = (min, sec = 0) => new Date(Date.UTC(2026, 10, 8, 9, min, sec)).toISOString();
const evs = (list) => list.map(([t, kind, detail, person = 'p1']) => ({ kind, detail, person, ts: t, cap: '' })).reverse();

test('patterns in use: searches as typed once, filters, edits row after row, views opened together, functions run by hand', () => {
  const list = [];
  // "overdue" typed letter by letter, three times, by two people; a one-off search too.
  for (const [m, who] of [[0, 'p1'], [5, 'p2'], [9, 'p1']]) for (const [i, q] of ['o', 'ov', 'ove', 'overdue'].entries()) list.push([at(m, i), 'action', `search ${q}`, who]);
  list.push([at(12), 'action', 'search invoices from march']);
  // A filter set four times; "all" is no filter.
  for (let i = 0; i < 4; i++) list.push([at(14 + i), 'action', 'filter status=waiting']);
  for (let i = 0; i < 6; i++) list.push([at(19, i), 'action', 'filter status=']);
  // The same field changed on six rows in one sitting.
  for (let i = 0; i < 6; i++) list.push([at(20, i * 5), 'update', 'guests: rsvp']);
  // Guests then Budget, within seconds, four times.
  for (let i = 0; i < 4; i++) { list.push([at(30 + i * 2), 'open', 'guests']); list.push([at(30 + i * 2, 10), 'open', 'budget']); }
  for (let i = 0; i < 5; i++) list.push([at(45 + i), 'call', 'send-reminders']);
  const sigs = signals(evs(list), { views: [{ slug: 'guests', title: 'Guests' }, { slug: 'budget', title: 'Budget' }] });
  const keys = sigs.map((s) => s.key);
  assert.ok(keys.includes('search:overdue'), keys.join(', '));
  assert.ok(!keys.some((k) => k === 'search:o' || k === 'search:ov'), 'letters typed are not searches');
  assert.ok(!keys.includes('search:invoices from march'), 'one search is no pattern');
  const s = sigs.find((x) => x.key === 'search:overdue');
  assert.equal(s.count, 3);
  assert.equal(s.people, 2);
  assert.match(s.text, /searched for "overdue" 3 times \(2 people\)/);
  assert.ok(keys.includes('filter:status=waiting'));
  assert.ok(!keys.some((k) => k === 'filter:status='));
  assert.ok(keys.includes('edits:guests: rsvp'));
  assert.match(sigs.find((x) => x.key === "edits:guests: rsvp").text, /changed rsvp of guests by hand, row after row: 6 times in one sitting/);
  assert.ok(keys.includes('path:guests>budget'));
  assert.match(sigs.find((x) => x.key === 'path:guests>budget').text, /went from Guests straight to Budget 4 times/);
  assert.ok(keys.includes('call:send-reminders'));
  // Edits spread over sittings an hour apart are not one pattern.
  const spread = evs([0, 61, 122, 183, 244].map((m) => [new Date(Date.UTC(2026, 10, 8, 0, m)).toISOString(), 'update', 'guests: rsvp']));
  assert.deepEqual(signals(spread).map((x) => x.key), []);
});

test('a pattern names what should change if a feature answers it, counted before and after', () => {
  assert.deepEqual(measureFor({ key: 'search:overdue' }), { event: 'action', match: 'search overdue', q: 'overdue', goal: 'fewer', says: 'searches for "overdue"' });
  assert.deepEqual(measureFor('edits:guests: rsvp, table').fields, 'rsvp,table');
  assert.equal(measureFor('nothing:search:caterer').empty, true);
  assert.equal(measureFor('abandon:guests/guests new').screen, 'guests');
  assert.equal(measureFor('path:guests>budget').match, 'budget');
  const list = [];
  for (let d = 1; d <= 10; d++) list.push([new Date(Date.UTC(2026, 10, d, 12)).toISOString(), 'action', 'search overdue']);
  list.push([new Date(Date.UTC(2026, 10, 11, 12)).toISOString(), 'action', 'search overdue']);
  list.push([new Date(Date.UTC(2026, 10, 11, 13)).toISOString(), 'action', 'search other']);
  const e = measureEffect(evs(list), measureFor('search:overdue'), new Date(Date.UTC(2026, 10, 10, 18)).toISOString());
  assert.equal(e.before, 10);
  assert.equal(e.after, 1);
  assert.ok(e.perDayBefore > e.perDayAfter || e.afterDays < 1);
});

test('keeping a candidate merges its own changes over a live version that moved on, unless both changed a piece', () => {
  const base = ['a/view/home@1', 'a/view/list@1', 'a/table/t@1', 'lib/component/card@2'];
  const cand = ['a/view/home@2', 'a/view/list@1', 'a/table/t@2', 'a/function/bulk@1', 'lib/component/card@2'];
  const head = ['a/view/home@1', 'a/view/list@2', 'a/table/t@1', 'lib/component/card@3'];
  const m = mergeCaps(base, cand, head);
  assert.deepEqual(m.caps.sort(), ['a/function/bulk@1', 'a/table/t@2', 'a/view/home@2', 'a/view/list@2', 'lib/component/card@3']);
  const clash = mergeCaps(base, cand, ['a/view/home@3', 'a/view/list@1', 'a/table/t@1', 'lib/component/card@2']);
  assert.deepEqual(clash.conflicts, ['a/view/home']);
  // A piece the candidate removed is removed from the live version too.
  assert.deepEqual(mergeCaps(['a/view/x@1', 'a/view/y@1'], ['a/view/x@1'], ['a/view/x@2', 'a/view/y@1']).caps, ['a/view/x@2']);
});

class FakeStore {
  constructor() {
    this.app = { id: 'a', name: 'Wedding', head: 3 };
    this.comps = new Map([[3, { n: 3, caps: ['a/view/guests@1'] }]]);
    this.events = [];
    this.cands = new Map();
    this.sugs = new Map();
    this.settings = new Map();
  }
  async getApp() { return { ...this.app }; }
  async resolve() { return { app: this.app, comp: this.comps.get(this.app.head), views: [{ slug: 'guests', title: 'Guests' }] }; }
  async recentEvents() { return this.events; }
  async candidates() { return [...this.cands.values()].reverse(); }
  async candidate(id) { return this.cands.get(id) || null; }
  async putCandidate(d) { this.cands.set(d.id, { created: new Date().toISOString(), ...this.cands.get(d.id), ...d }); return d; }
  async suggestionHistory() { return [...this.sugs.values()]; }
  async setSuggestion(id, status) { const s = this.sugs.get(id); if (s) s.status = status; return s; }
  async setting(app, name) { return this.settings.get(name) || null; }
  async putSetting(app, name, v) { this.settings.set(name, v); }
  async composition(app, n) { return this.comps.get(n) || { n: 0, caps: [] }; }
  async setHead(app, n) { this.app.head = n; }
  async writeComposition(app, caps) { const n = Math.max(...this.comps.keys()) + 1; this.comps.set(n, { n, caps }); this.app.head = n; return n; }
}

test('the engine builds the strongest new pattern beside the live version, once, then keeps or drops it', async () => {
  const store = new FakeStore();
  for (let i = 0; i < 4; i++) store.events.unshift({ kind: 'action', detail: 'search overdue', person: 'p1', ts: at(i) });
  const submitted = [];
  let asked = 0;
  const propose = async (engine, appId, { signals: sigs, fresh }) => {
    asked++;
    assert.deepEqual(fresh.map((f) => f.key), ['search:overdue']);
    const docs = [{ id: 's1', text: 'Add an Overdue filter', why: 'You searched "overdue" 4 times', signal: 'search:overdue', status: 'open' }, { id: 's2', text: 'Something else', signal: null, status: 'open' }];
    for (const d of docs) store.sugs.set(d.id, d);
    return docs;
  };
  const ev = new Evolver({ engine: { store }, submit: async (app, text, source, extra) => { submitted.push({ text, source, extra }); return { id: `r${submitted.length}` }; }, propose });
  const out = await ev.tick('a');
  assert.equal(asked, 1);
  assert.equal(submitted.length, 1);
  assert.deepEqual([submitted[0].text, submitted[0].source], ['Add an Overdue filter', 'evolve']);
  const cid = submitted[0].extra.candidate;
  assert.equal(out.building.id, cid);
  assert.equal(store.cands.get(cid).status, 'building');
  assert.equal(store.cands.get(cid).base, 3);
  assert.equal(store.sugs.get('s1').status, 'building');
  // The same pattern again: no model call, nothing built.
  await ev.tick('a');
  assert.equal(asked, 1);
  assert.equal(submitted.length, 1);
  // Built as version 4 beside the live version 3; kept as built.
  store.comps.set(4, { n: 4, caps: ['a/view/guests@2'] });
  await ev.built(cid, { n: 4, base: 3, summary: 'An Overdue filter', problems: [] });
  assert.equal(store.cands.get(cid).status, 'ready');
  assert.equal(store.app.head, 3);
  const kept = await ev.keep('a', cid);
  assert.deepEqual(kept, { how: 'as built', head: 4 });
  assert.equal(store.cands.get(cid).status, 'kept');
  assert.equal(store.sugs.get('s1').status, 'taken');
  await assert.rejects(ev.drop('a', cid), /live already/);
  // Off: nothing is looked at.
  await ev.setMode('a', 'off');
  for (let i = 0; i < 5; i++) store.events.unshift({ kind: 'action', detail: 'filter rsvp=no', person: 'p1', ts: at(30 + i) });
  assert.deepEqual((await ev.tick('a')).suggestions, []);
  assert.equal(asked, 1);
  // Suggest only: the model answers the new pattern; nothing is built.
  await ev.setMode('a', 'suggest');
  const propose2 = async () => { asked++; return [{ id: 's3', text: 'Make "no" a tab', signal: 'filter:rsvp=no' }]; };
  ev.propose = propose2;
  const out2 = await ev.tick('a');
  assert.equal(out2.suggestions.length, 1);
  assert.equal(out2.building, null);
  assert.equal(submitted.length, 1);
  await assert.rejects(ev.setMode('a', 'sometimes'), /evolve is one of/);
});

test('a photo named by what it shows is found in the built-in set by its subject, and kept apart from the others on the page', async () => {
  const { matchBuiltin, placeScreenPhotos, placePhotos, builtinPhotos, WIDTHS } = await import('../engine/photos.mjs');
  assert.ok(builtinPhotos().length > 200, 'the built-in set ships with the engine');
  // The subject (the first word naming a thing), two words or most of them
  // must fit; a look word ("dark", "minimal") never makes a photo fit alone.
  assert.match(matchBuiltin('coffee shop interior')[0].about, /cafe/);
  assert.match(matchBuiltin('cozy living room')[0].about, /living room/);
  assert.deepEqual(matchBuiltin('kayaking river'), [], 'a city on a river is not a kayak');
  assert.deepEqual(matchBuiltin('snowmobile trail'), []);
  assert.match(matchBuiltin('dark roast coffee beans')[0].about, /coffee beans/, 'not a dark abstract');
  assert.match(matchBuiltin('house blend coffee bag')[0].about, /bag of coffee/, 'not a house');
  assert.match(matchBuiltin('close-up of latte art')[0].about, /latte/);
  assert.deepEqual(matchBuiltin('Sumatra Mandheling dark roast'), []);
  // Another photo stands in for a used one only when it fits nearly as well.
  const first = matchBuiltin('latte art coffee')[0];
  const second = matchBuiltin('latte art coffee', { used: new Set([first.id]) })[0];
  assert.ok(second && second.id !== first.id && /latte/.test(second.about));

  // Placing: each block that names a photo gets src, sized for where it goes.
  const asked = [];
  const find = async (q, { use, used }) => { asked.push([q, use]); const id = `p${asked.length}`; return { id, src: `https://x/${id}?w=${WIDTHS[use]}`, source: 'builtin', credit: q === 'tram' ? 'Tram by A, CC BY 2.0' : null }; };
  const screen = { page: 'front', body: [
    { hero: 'Hi', photo: 'coffee' },
    { carousel: [{ title: 'a', photo: 'bread' }, { title: 'b', photo: 'cake', src: 'https://kept' }], look: 'slides' },
    { quotes: [{ text: 'x', photo: 'portrait man' }] },
    { gallery: [{ photo: 'tram' }] },
    { band: [{ media: ['x'], photo: 'farm' }], photo: '=row.image' },
  ] };
  const placed = await placeScreenPhotos(screen, { find });
  assert.equal(placed.length, 5);
  assert.deepEqual(asked.map(([q, u]) => `${q}:${u}`), ['coffee:hero', 'bread:slide', 'portrait man:avatar', 'tram:gallery', 'farm:media']);
  assert.equal(screen.body[1].carousel[1].src, 'https://kept', 'a photo with a src is left as it is');
  assert.equal(screen.body[3].gallery[0].credit, 'Tram by A, CC BY 2.0');
  assert.equal(screen.body[4].src, undefined, 'an expression is the data\'s photo, not a query');

  // Example rows: "photo:..." becomes a URL from the built-in set; a row the
  // set has nothing of gets a photo of what its table holds, else nothing.
  const plan = { views: [], seed: {
    coffees: [{ name: 'Huila', image: 'photo:latte art coffee' }, { name: 'Sumatra', image: 'photo:Sumatra Mandheling dark roast' }],
    sleds: [{ name: 'Polar', image: 'photo:snowmobile trail' }] } };
  await placePhotos(plan, {});
  assert.match(plan.seed.coffees[0].image, /^https:\/\/images\.unsplash\.com\/photo-/);
  assert.match(plan.seed.coffees[1].image, /^https:\/\/images\.unsplash\.com\/photo-/);
  assert.notEqual(plan.seed.coffees[1].image, plan.seed.coffees[0].image);
  assert.equal(plan.seed.sleds[0].image, null);
  // A product the page already shows gets that photo again; one the set has
  // nothing of gets its kind's photo, never a photo of another thing.
  const croissant = matchBuiltin('butter croissant')[0].id;
  const shop = { views: [{ slug: 'home', screen: { page: 'front', body: [{ hero: 'Hi', photo: 'butter croissant' }] } }], seed: { products: [
    { name: 'Butter croissant', image: 'photo:butter croissant' },
    { name: 'Pumpkin pie', category: 'Pies', image: 'photo:pumpkin pie' },
    { name: 'Cable', image: 'photo:usb charging cable' }] } };
  const code = {};
  await placePhotos(shop, code);
  assert.match(code.home, new RegExp(croissant));
  assert.match(shop.seed.products[0].image, /croissant|photo-/);
  assert.match(builtinPhotos().find((p) => shop.seed.products[1].image.includes(p.id)).tags, /\bpie\b/);
  assert.equal(shop.seed.products[2].image, null, '"products" says nothing of what a row shows');
});

test('front-page blocks pass the screen checks, their nested blocks checked like any others', () => {
  const ok = { page: 'front', body: [
    { nav: 'Brand', links: [{ label: 'Menu', href: '#menu' }], action: { label: 'Book', open: 'bookings' } },
    { hero: 'Hello *there*', photo: 'cafe', actions: [{ label: 'Go', do: '=() => app.toast("hi")' }] },
    { band: [{ h: 'Visit' }, 'Open daily'], look: 'dark' },
    { media: ['Our story'], photo: 'garden', side: 'right' },
    { carousel: '=rows', item: { title: '=item.name' } },
    { features: [{ icon: 'leaf', title: 'A', text: 'B' }] },
    { footer: 'Brand', text: 'Made here' },
  ] };
  assert.deepEqual(checkScreen(ok), []);
  const bad = { page: 'front', body: [{ band: [{ nope: 1 }] }, { media: [{ h: '=(' }] }, { carousel: '=rows.map(', item: {} }] };
  const p = checkScreen(bad);
  assert.ok(p.some((x) => /body\[0\]\.band\[0\]: unknown block/.test(x)), p.join('\n'));
  assert.ok(p.some((x) => /body\[1\]\.media\[0\]/.test(x) && /does not parse/.test(x)), p.join('\n'));
  assert.ok(p.some((x) => /body\[2\]\.carousel/.test(x) && /does not parse/.test(x)), p.join('\n'));
});

test('CREATE RULE compiles SQL into the steps the engine runs: checkout, upsert, IF, set-wide writes and triggers on several events', async () => {
  const { compileRule, compileRuleStatements } = await import('../engine/rulesql.mjs');
  const { rule } = compileRule(`CREATE RULE checkout AS
    REQUIRE (SELECT COUNT(*) FROM cart WHERE owner = :user.id) > 0 ELSE 'Your cart is empty';
    INSERT INTO orders (status) VALUES ('new') AS order; -- the order; its lines next
    FOR line IN (SELECT product, qty FROM cart WHERE owner = :user.id) DO
      LET stock = (SELECT stock FROM products WHERE id = :line.product);
      REQUIRE :stock >= :line.qty ELSE 'Only :stock left of :line.product';
      INSERT INTO order_lines (order, product, qty) VALUES (:order.id, :line.product, :line.qty);
      UPDATE products SET stock = stock - :line.qty WHERE id = :line.product;
    END;
    RETURN :order.id;`);
  assert.deepEqual(rule.steps.map((s) => Object.keys(s)[0]), ['require', 'insert', 'each', 'return']);
  assert.equal(rule.steps[0].require, 'SELECT (SELECT COUNT(*) FROM cart WHERE owner = :user.id) > 0');
  assert.deepEqual(rule.steps[2].do[3], { update: 'products', id: ':line.product', set: { stock: 'SELECT stock - :line.qty FROM products WHERE id = :line.product' } });
  // It runs like the JSON rule it compiles to.
  const writes = [];
  const stock = { p1: 5, p2: 1 };
  const host = async (method, args) => {
    if (method === 'sql') {
      const [q, p] = args;
      if (/COUNT\(\*\) FROM cart/.test(q)) return [{ ok: true }];
      if (/SELECT product, qty FROM cart/.test(q)) return [{ product: 'p1', qty: 2 }, { product: 'p2', qty: 1 }];
      if (/^SELECT stock FROM products/.test(q)) return [{ stock: stock[p.line_product] }];
      if (/^SELECT :stock >= :line_qty$/.test(q)) return [{ ok: p.stock >= p.line_qty }];
      if (/SELECT stock - :line_qty/.test(q)) return [{ s: stock[p.line_product] - p.line_qty }];
      return [];
    }
    writes.push([method, ...args]);
    return method === 'insert' ? { id: `new-${writes.length}`, ...args[1] } : { id: args[1], ...args[2] };
  };
  assert.equal(await runRule(rule, { user: { id: 'u1' } }, host), 'new-1');
  assert.deepEqual(writes.map((w) => `${w[0]} ${w[1]}`), ['insert orders', 'insert order_lines', 'update products', 'insert order_lines', 'update products']);
  const cols = (t) => ({ orders: new Set(['id', 'status']), order_lines: new Set(['id', 'order', 'product', 'qty']), products: new Set(['id', 'stock']), cart: new Set(['id', 'owner', 'product', 'qty']) })[t] || null;
  assert.deepEqual(checkRule(rule, cols), []);

  // ON CONFLICT: find the row, add it or update it (excluded.col is the new value).
  const up = compileRule("CREATE RULE add AS INSERT INTO cart (owner, product, qty) VALUES (:user.id, :product, 1) ON CONFLICT (owner, product) DO UPDATE SET qty = qty + excluded.qty AS line; RETURN :line.id").rule.steps;
  assert.deepEqual(up.map((s) => Object.keys(s)[0]), ['let', 'insert', 'update', 'return']);
  assert.equal(up[0].value, 'SELECT id FROM cart WHERE owner = :user.id AND product = :product');
  assert.equal(up[2].set.qty, `SELECT qty + 1 FROM cart WHERE id = :${up[0].let}`);
  assert.deepEqual(checkRule({ steps: up }, cols), []);
  // IF / ELSE: each statement runs when its condition holds.
  const branch = compileRule("CREATE RULE b AS IF :total > 100 THEN UPDATE orders SET status = 'review' WHERE id = :order; ELSE DELETE FROM cart WHERE owner = :user.id; END IF;").rule.steps;
  assert.equal(branch[0].if, 'SELECT :total > 100');
  assert.equal(branch[1].if, 'SELECT NOT ((SELECT :total > 100))');
  assert.deepEqual(branch[1].each && [branch[1].each, branch[1].do[0].remove], ['SELECT id FROM cart WHERE owner = :user.id', 'cart'], 'WHERE other than id = goes through each row it selects');
  // A query rule, and a trigger on two events.
  assert.deepEqual(compileRule('CREATE RULE spent RETURNS VALUE AS SELECT SUM(amount) FROM expenses WHERE day >= :from;').rule, { query: 'SELECT SUM(amount) FROM expenses WHERE day >= :from', value: true });
  const t = compileRule('CREATE RULE count_votes AFTER INSERT OR DELETE ON votes AS UPDATE features SET votes = (SELECT COUNT(*) FROM votes WHERE feature = :row.feature) WHERE id = :row.feature');
  assert.deepEqual(t.on, { table: 'votes', event: ['insert', 'delete'] });
  assert.equal(t.rule.steps[0].set.votes, 'SELECT COUNT(*) FROM votes WHERE feature = :row.feature');
  // Mistakes say what to write.
  for (const [bad, want] of [
    ['CREATE RULE x AS SELECT 1; DELETE FROM a WHERE id = 1', /a SELECT among other statements/],
    ['CREATE RULE x AS FOR r IN (SELECT id FROM a) DO DELETE FROM a WHERE id = :r.id;', /has no END/],
    ['CREATE RULE x AS INSERT INTO a (b, c) VALUES (1)', /2 columns but 1 values/],
    ['CREATE RULE x AS UPDATE a SET b = 1', /give WHERE/],
    ["CREATE RULE x AS REQUIRE :n > 0 ELSE oops", /single quotes/],
    ['CREATE RULE x AS MERGE INTO a', /a rule's statements are/],
    ['CREATE RULE x BEFORE INSERT ON a AS DELETE FROM a WHERE id = 1', /is not understood/],
  ]) assert.throws(() => compileRule(bad), want, bad);
  // In a plan: the statement is compiled, keeps its trigger, and a test's "do" may be statements too.
  const plan = { functions: [{ slug: 'count-votes' }, { slug: 'broken' }], tests: [{ name: 'v', do: "INSERT INTO features (title) VALUES ('A') AS f; INSERT INTO votes (feature) VALUES (:f.id)", expect: [] }] };
  const code = { 'count-votes': 'CREATE RULE count_votes AFTER INSERT ON votes AS UPDATE features SET votes = 1 WHERE id = :row.feature;', broken: 'CREATE RULE broken AS UPDATE a SET b = 1' };
  const problems = compileRuleStatements(plan, code);
  assert.equal(problems.length, 1);
  assert.match(problems[0], /^function broken: CREATE RULE: UPDATE a: give WHERE/);
  assert.deepEqual(plan.functions[0].on, { table: 'votes', event: 'insert' });
  assert.match(plan.functions[0].statement, /^CREATE RULE count_votes/);
  assert.deepEqual(plan.tests[0].do, [{ insert: 'features', values: { title: 'A' }, as: 'f' }, { insert: 'votes', values: { feature: ':f.id' } }]);
});

test('patterns are rows: seeded from library/patterns, versioned, used from Strata, and a plan may add one that its screens use at once', async () => {
  const { seedLibrary } = await import('../engine/library.mjs');
  const { PATTERNS, BUILTIN, usePatterns, checkPattern } = await import('../engine/patterns.mjs');
  const builtins = new Map(PATTERNS);
  // A store that keeps capabilities in memory.
  const rows = [];
  const store = {
    async shared() { return new Map(); },
    async starters() { return new Map(); },
    async patterns() { const out = new Map(); for (const r of [...rows].reverse()) if (r.kind === 'pattern' && !out.has(r.slug)) out.set(r.slug, r); return out; },
    async getCapabilities(ids) { return rows.filter((r) => ids.includes(r.id)); },
    async putCapability(c) { const version = rows.filter((r) => r.kind === c.kind && r.slug === c.slug).length + 1; const id = `${c.app}/${c.kind}/${c.slug}@${version}`; rows.push({ ...c, id, version }); return id; },
  };
  const lib = new URL('../library', import.meta.url).pathname;
  await seedLibrary(store, lib);
  const patternRows = () => rows.filter((r) => r.kind === 'pattern').length;
  assert.ok(rows.some((r) => r.id === 'lib/pattern/crud@1' && JSON.parse(r.code).params.table));
  assert.equal(patternRows(), BUILTIN.size);
  // Seeding again adds no pattern; a changed file would be version 2.
  await seedLibrary(store, lib);
  assert.equal(patternRows(), BUILTIN.size);
  try {
    assert.equal(usePatterns(await store.patterns()), BUILTIN.size);
    assert.equal(PATTERNS.get('crud').author, 'builtin');
    // A pattern an app adds: checked by expanding its example.
    const board = { name: 'leaderboard', title: 'Top rows by a score', about: 'Kids by points, players by wins.',
      params: { table: { about: 'the table', need: true }, score: { about: 'the column to rank by', default: 'points' }, key: { about: 'names its data', default: '{{table}}' } },
      example: { use: 'leaderboard', table: 'kids', score: 'points' },
      screen: { data: { '{{key}}': 'SELECT * FROM {{table}} ORDER BY {{score}} DESC LIMIT 10' }, body: [{ table: '=data.{{key}}', columns: [{ key: 'name' }, { key: '{{score}}' }] }] } };
    assert.deepEqual(checkPattern(board, checkScreen), []);
    assert.match(checkPattern({ ...board, name: 'crud' }, checkScreen)[0], /built-in pattern/);
    assert.match(checkPattern({ ...board, example: { use: 'leaderboard' } }, checkScreen)[0], /needs "table"/);
    // In a plan: its screens use the pattern it adds, before it is stored.
    const plan = { patterns: [board], views: [{ slug: 'top', screen: { body: [{ use: 'leaderboard', table: 'players', score: 'wins' }] } }] };
    const problems = prepareScreens(plan, {}, { views: [], functions: [], tables: [], shared: [] });
    assert.deepEqual(problems, []);
    assert.equal(plan.views[0].screen.data.players, 'SELECT * FROM players ORDER BY wins DESC LIMIT 10');
    // Stored, it is a row like the built-in ones, and every build expands from it.
    await store.putCapability({ app: 'lib', slug: 'leaderboard', kind: 'pattern', title: board.title, description: board.about, code: JSON.stringify(board), author: 'a1' });
    usePatterns(await store.patterns());
    assert.equal(PATTERNS.get('leaderboard').author, 'a1');
    assert.ok(!BUILTIN.has('leaderboard'));
  } finally {
    PATTERNS.clear();
    for (const [k, v] of builtins) PATTERNS.set(k, v);
  }
});

test('the prompt gives the patterns that fit in full and the others by name', async () => {
  const { addedPatternsPart } = await import('../engine/forge.mjs');
  const p = (title) => ({ title, about: `${title}.`, params: { table: { about: 'the table', need: true } }, example: { use: 'x', table: 't' } });
  const added = new Map([['leaderboard', p('Ranked rows')], ['gallery', p('Photo grid')], ['timeline', p('Events in order')]]);
  const part = addedPatternsPart(added, ['gallery', 'missing']);
  assert.match(part, /The ones that fit this request best/);
  assert.match(part, /## gallery: Photo grid\nPhoto grid\.\n {2}- table \(needed\): the table/);
  assert.doesNotMatch(part, /## leaderboard/);
  assert.match(part, /The others, by name only[^\n]*\n- leaderboard: Ranked rows\n- timeline: Events in order/);
  // When every pattern fits, there is no list of others.
  const all = addedPatternsPart(added, ['leaderboard', 'gallery', 'timeline']);
  assert.doesNotMatch(all, /The others|fit this request best/);
});

// Against a real strata-server (skipped without one): pieces are found by
// what they mean, not only by their words, and each says how it was found.
test('pieces are searched by words and by meaning', async (t) => {
  const { findServerBinary, startServer } = await import('../engine/strata.mjs');
  const { Store } = await import('../engine/store.mjs');
  const bin = findServerBinary(join(import.meta.dirname, '..', '..'));
  if (!bin) return t.skip('no strata-server binary');
  const dir = mkdtempSync(join(tmpdir(), 'sa-meaning-'));
  const port = await new Promise((r) => { const s = net.createServer().listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => r(p)); }); });
  const { child, client } = await startServer({ bin, db: join(dir, 'apps.strata'), listen: `127.0.0.1:${port}`, log: () => {} });
  try {
    const store = new Store(client, () => {});
    await store.init();
    if (!store.meaning) return t.skip('this strata-server cannot load the embedding model');
    const put = (slug, kind, title, description) => store.putCapability({ app: 'lib', slug, kind, title, description, code: '{}', spec: {}, author: 'builtin' });
    await put('checkout', 'function', 'Checkout', 'Turns the shopping cart into an order and charges the card on file.');
    await put('vote-board', 'view', 'Vote board', 'People suggest ideas and upvote the ones they like; the most voted rise to the top.');
    await put('gallery', 'view', 'Gallery', 'A grid of photos that open full screen.');
    await put('chore-chart', 'view', 'Chore chart', 'Chores worth points for each kid, ticked off through the week.');
    // No word of the request is in the piece: found by meaning alone.
    const [pay] = await store.searchCapabilities('purchase the items in my basket', { limit: 2 });
    assert.equal(pay?.slug, 'checkout');
    assert.equal(pay.found, 'meaning');
    assert.ok(pay.cosine > 0);
    // Words and meaning agree.
    const [vote] = await store.searchCapabilities('let people vote on ideas', { limit: 2 });
    assert.equal(vote?.slug, 'vote-board');
    assert.equal(vote.found, 'both');
    // Kinds still filter, and the vector is not handed out with a piece.
    const views = await store.searchCapabilities('photos', { kinds: ['view'], limit: 3 });
    assert.ok(views.every((c) => c.kind === 'view'));
    assert.equal(views[0]?.slug, 'gallery');
    assert.ok(!('meaning' in views[0]));
  } finally {
    child.kill();
  }
});

test('graph questions are parsed into a kind, a relation and a piece', async () => {
  const { parseQuestion, resolveName, writesTable } = await import('../engine/ask.mjs');
  const q = (s) => { const p = parseQuestion(s); return p && [p.what.name, p.rel, p.target, p.dir, p.event || null]; };
  assert.deepEqual(q('Which screens use the calendar?'), ['screens', 'uses', 'the calendar', 'in', null]);
  assert.deepEqual(q('which rules write votes'), ['rules', 'writes', 'votes', 'in', null]);
  assert.deepEqual(q('which apps use lib.money'), ['apps', 'uses', 'lib.money', 'in', null]);
  assert.deepEqual(q('what calls checkout'), ['pieces', 'calls', 'checkout', 'in', null]);
  assert.deepEqual(q('what tables does the board screen read'), ['tables', 'reads', 'the board screen', 'out', null]);
  assert.deepEqual(q('what runs when a vote is added'), ['triggers', 'on', 'vote', 'in', 'insert']);
  assert.deepEqual(q('where is the shopping cart used'), ['pieces', 'uses', 'the shopping cart', 'in', null]);
  assert.deepEqual(q('which screens depend on the invoices table'), ['screens', 'depends', 'the invoices table', 'in', null]);
  // Not questions about the graph: searched as words and meaning instead.
  for (const s of ['let people pay for their basket', 'checkout', 'which colors look best', 'how do I make a cake']) assert.equal(parseQuestion(s), null, s);
  const nodes = [
    { id: 'lib/component/progressBar', kind: 'component', app: 'lib', slug: 'progressBar', title: 'Progress bar' },
    { id: 'lib/pattern/calendar', kind: 'pattern', app: 'lib', slug: 'calendar', title: 'Calendar' },
    { id: 'a/table/votes', kind: 'table', app: 'a', slug: 'votes', title: 'votes' },
    { id: 'b/table/votes', kind: 'table', app: 'b', slug: 'votes', title: 'votes' },
    { id: 'a/view/board', kind: 'view', app: 'a', slug: 'board', title: 'Features' },
    { id: 'b/function/cast-special-vote', kind: 'function', app: 'b', slug: 'cast-special-vote', title: 'cast-special-vote' },
    { id: 'app/b', kind: 'app', app: 'b', slug: 'b', title: 'Bakery' },
  ];
  const ids = (t, o) => resolveName(t, nodes, o).hits.map((n) => n.id);
  assert.deepEqual(ids('the progress bar'), ['lib/component/progressBar']);
  assert.deepEqual(ids('ui.progressBar'), ['lib/component/progressBar']);
  assert.deepEqual(ids('vote', { hint: 'table' }), ['a/table/votes', 'b/table/votes']);
  assert.deepEqual(ids('the votes table in the bakery', { apps: new Map([['a', 'Voting'], ['b', 'Bakery']]) }), ['b/table/votes']);
  assert.deepEqual(ids('cast special vote'), ['b/function/cast-special-vote']);
  assert.deepEqual(ids('the board screen'), ['a/view/board']);
  assert.deepEqual(ids('the calendar pattern'), ['lib/pattern/calendar']);
  // Writes, told from reads by the code.
  assert.ok(writesTable('{"steps": [{"insert": "votes", "values": {}}]}', 'votes'));
  assert.ok(writesTable("=() => app.update('features', id, {done: true})", 'features'));
  assert.ok(writesTable('{"form": "guests", "fields": []}', 'guests'));
  assert.ok(writesTable('DELETE FROM votes WHERE id = :id', 'votes'));
  assert.ok(!writesTable('SELECT COUNT(*) FROM votes WHERE feature = :f', 'votes'));
  assert.ok(!writesTable('{"update": "features", "set": {"votes": "SELECT COUNT(*) FROM votes"}}', 'votes'));
});

test('graph questions are answered from Strata', async (t) => {
  const { findServerBinary, startServer } = await import('../engine/strata.mjs');
  const { Store } = await import('../engine/store.mjs');
  const { ask } = await import('../engine/ask.mjs');
  const bin = findServerBinary(join(import.meta.dirname, '..', '..'));
  if (!bin) return t.skip('no strata-server binary');
  const dir = mkdtempSync(join(tmpdir(), 'sa-ask-'));
  const port = await new Promise((r) => { const s = net.createServer().listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => r(p)); }); });
  const { child, client } = await startServer({ bin, db: join(dir, 'apps.strata'), listen: `127.0.0.1:${port}`, log: () => {} });
  try {
    const store = new Store(client, () => {});
    await store.init();
    const put = async (c) => { const id = await store.putCapability({ author: 'forge', description: '', ...c }); return { ...c, id, version: 1, spec: c.spec || {} }; };
    await put({ app: 'lib', slug: 'money', kind: 'helper', title: 'Money', code: 'function helper() { return {}; }' });
    await put({ app: 'lib', slug: 'progressBar', kind: 'component', title: 'Progress bar', code: 'function component(p) { return lib.money.format(p.v); }' });
    await put({ app: 'lib', slug: 'leaderboard', kind: 'pattern', title: 'Leaderboard', code: '{"screen": {"body": [{"ui": "progressBar"}]}}' });
    const caps = [
      await put({ app: 'v', slug: 'board', kind: 'view', title: 'Features', code: '{"body": [{"ui": "progressBar"}, {"table": "features"}, {"button": "Vote", "do": "=() => app.call(\'toggle-vote\', {})"}]}', spec: { screen: true, patterns: ['leaderboard'] } }),
      await put({ app: 'v', slug: 'toggle-vote', kind: 'function', title: 'toggle-vote', code: '{"steps": [{"insert": "votes", "values": {"feature": ":feature"}}]}', spec: { rule: true } }),
      await put({ app: 'v', slug: 'count-votes', kind: 'function', title: 'count-votes', code: '{"steps": [{"update": "features", "id": ":row.feature", "set": {"votes": "SELECT COUNT(*) FROM votes"}}]}', spec: { rule: true, on: { table: 'votes', event: ['insert', 'delete'] } } }),
      await put({ app: 'v', slug: 'features', kind: 'table', title: 'features', code: '', spec: { name: 'features' } }),
      await put({ app: 'v', slug: 'votes', kind: 'table', title: 'votes', code: '', spec: { name: 'votes' } }),
    ];
    const view = { app: { id: 'v', name: 'Voting board', head: 1 }, caps, tables: caps.filter((c) => c.kind === 'table').map((c) => ({ ...c, spec: c.spec })), functions: caps.filter((c) => c.kind === 'function'), views: caps.filter((c) => c.kind === 'view') };
    await store.writeGraph(appDocs(view, ['component', 'helper']));
    const a = async (q) => { const r = await ask(store, q); return r.answer.map((x) => `${x.id}${x.via ? ` via ${x.via.map((v) => v.label).join(' > ')}` : ''}`); };
    assert.deepEqual(await a('which screens use the progress bar'), ['v/view/board']);
    assert.deepEqual(await a('which screens use lib.money'), ['v/view/board via ui.progressBar']);
    assert.deepEqual(await a('which apps use lib.money'), ['app/v via the screen board > ui.progressBar']);
    assert.deepEqual(await a('which screens use the leaderboard pattern'), ['v/view/board']);
    assert.deepEqual(await a('which rules write votes'), ['v/function/toggle-vote']);
    assert.deepEqual(await a('which rules write features'), ['v/function/count-votes']);
    assert.deepEqual(await a('what runs when a vote is added'), ['v/function/count-votes']);
    assert.deepEqual(await a('what runs when a vote is changed'), []);
    assert.deepEqual(await a('what calls toggle vote'), ['v/view/board']);
    assert.deepEqual((await a('what tables does the board screen read')).sort(), ['v/table/features']);
    const r = await ask(store, 'which screens use the zzqxv');
    assert.deepEqual(r.answer, []);
    assert.match(r.note, /Nothing in the graph is called "zzqxv"/);
    assert.equal(await ask(store, 'a board for votes'), null);
  } finally {
    child.kill();
  }
});

// Every piece is stored as its SQL and compiled when read: the statement
// gives back exactly what the engine runs.
test('pieces are stored as SQL: each kind gives back what it runs', async () => {
  const { toSource, fromSource, storedSpec, withoutPhotos, appScript } = await import('../engine/source.mjs');
  const { compileStatement } = await import('../engine/statement.mjs');
  const { expandScreen } = await import('../engine/patterns.mjs');
  const round = (piece) => {
    const sql = toSource(piece);
    const spec = storedSpec(piece.kind, piece.spec, sql);
    return { sql, spec, back: fromSource({ ...piece, code: sql, spec }) };
  };
  // A table: its CREATE TABLE.
  const t = round({ kind: 'table', slug: 'dishes', title: 'dishes', code: '', spec: { name: 'dishes', description: 'What the kitchen serves', columns: [{ name: 'name', type: 'text' }, { name: 'price', type: 'float' }, { name: 'about', type: 'meaning', of: ['name'] }] } });
  assert.match(t.sql, /^-- What the kitchen serves\nCREATE TABLE dishes \(\n {2}id KEYWORD PRIMARY KEY,\n {2}name TEXT,\n {2}price FLOAT,\n {2}about VECTOR OPTIONS\(embed = 'name'/);
  assert.equal(t.back.code, '');
  // A screen written as a statement, with a photo found for it after it was
  // expanded: the statement and a PHOTOS part.
  const statement = "CREATE SCREEN home TITLE 'Home'\nBLOCKS [{\"hero\": \"Fresh sushi\", \"photo\": \"sushi counter\"}]\nLIST dishes\n  TITLE 'Dishes'  NOUN 'dish'\n  FORM (name REQUIRED, price NUMBER)";
  const ex = expandScreen(compileStatement(statement).screen);
  assert.deepEqual(ex.problems, []);
  ex.screen.body[0].src = 'https://images.unsplash.com/photo-1-abc?w=1600';
  ex.screen.body[0].credit = 'A. Person';
  const s = round({ kind: 'view', slug: 'home', title: 'Home', code: JSON.stringify(ex.screen, null, 1), spec: { screen: true, order: 10, statement } });
  assert.ok(s.sql.startsWith(statement));
  assert.match(s.sql, /\nPHOTOS \[\n {2}\{"photo":"sushi counter","src":"https:\/\/images\.unsplash\.com\/photo-1-abc\?w=1600","credit":"A\. Person"\}\n\]$/);
  assert.deepEqual(JSON.parse(s.back.code), ex.screen);
  assert.equal(s.back.statement, statement);
  assert.equal(withoutPhotos(s.sql), statement);
  assert.ok(!s.spec.statement && !s.spec.derived);
  // A screen written as JSON: a statement made from it, marked derived (it is
  // still shown and changed as JSON).
  const json = { data: { n: 'SELECT COUNT(*) AS n FROM dishes' }, body: [{ h: 'Menu' }, { table: '=data.dishes', columns: [{ key: 'name' }] }] };
  const j = round({ kind: 'view', slug: 'menu', title: 'Menu', code: JSON.stringify(json), spec: { screen: true } });
  assert.match(j.sql, /^CREATE SCREEN menu TITLE 'Menu'\nDATA n AS \(SELECT COUNT\(\*\) AS n FROM dishes\)\nHEADING 'Menu'\nBLOCKS \[/);
  assert.deepEqual(JSON.parse(j.back.code), json);
  assert.equal(j.spec.derived, true);
  assert.equal(j.back.statement, null);
  // A stale statement (the JSON was patched since) is not stored: the JSON's own is.
  const patched = { ...ex.screen, body: ex.screen.body.slice(1) };
  const p = round({ kind: 'view', slug: 'home', title: 'Home', code: JSON.stringify(patched), spec: { screen: true, statement } });
  assert.ok(!p.sql.includes('LIST dishes'));
  assert.equal(p.spec.derived, true);
  assert.deepEqual(JSON.parse(p.back.code), patched);
  // A rule written as CREATE RULE, and one written as JSON.
  const ruleSql = "CREATE RULE count_votes AFTER INSERT OR DELETE ON votes AS\n  UPDATE features SET votes = (SELECT COUNT(*) FROM votes WHERE feature = :row.feature) WHERE id = :row.feature;";
  const { compileRule } = await import('../engine/rulesql.mjs');
  const compiled = compileRule(ruleSql);
  const r = round({ kind: 'function', slug: 'count_votes', title: 'count_votes', code: JSON.stringify(compiled.rule), spec: { rule: true, on: compiled.on, statement: ruleSql } });
  assert.equal(r.sql, ruleSql);
  assert.deepEqual(JSON.parse(r.back.code), compiled.rule);
  assert.equal(r.back.statement, ruleSql);
  const rj = round({ kind: 'function', slug: 'add-guest', title: 'add-guest', code: JSON.stringify({ steps: [{ insert: 'guests', values: { name: ':name' } }] }), spec: { rule: true, on: null } });
  assert.match(rj.sql, /^CREATE RULE add-guest AS\n {2}INSERT INTO guests \(name\) VALUES \(:name\);$/);
  assert.equal(rj.spec.derived, true);
  assert.deepEqual(JSON.parse(rj.back.code), { steps: [{ insert: 'guests', values: { name: ':name' } }] });
  // JavaScript and JSON inside a CREATE ... AS $$ ... $$, exactly, even
  // with $$ in the body.
  for (const [kind, slug, code, spec, head] of [
    ['function', 'send-digest', "async function run(input, app) {\n  return '$$' + input.x;\n}\n", { on: { table: 'orders', event: 'insert' } }, 'CREATE FUNCTION send-digest AFTER INSERT ON orders LANGUAGE javascript AS $js$'],
    ['view', 'game', 'function view(root, app) { root.append("hi"); }', { order: 5 }, "CREATE SCREEN game TITLE 'Snake' LANGUAGE javascript AS $$"],
    ['component', 'stars', 'function component(props) { return h("span", {}, "*"); }', {}, 'CREATE COMPONENT stars LANGUAGE javascript AS $$'],
    ['helper', 'money', 'function helper() { return { f: (n) => n }; }', {}, 'CREATE HELPER money LANGUAGE javascript AS $$'],
    ['pattern', 'leaderboard', '{\n "params": {}\n}\n', {}, 'CREATE PATTERN leaderboard LANGUAGE json AS $$'],
  ]) {
    const w = round({ kind, slug, title: slug === 'game' ? 'Snake' : slug, code, spec });
    assert.ok(w.sql.startsWith(head), `${kind}: ${w.sql.split('\n')[0]}`);
    assert.equal(w.back.code, code, kind);
  }
  // The whole app as one script.
  const sql = appScript({ app: { id: 'abc123', name: 'Sushi', description: 'A sushi bar' }, comp: { n: 3, summary: 'Added the menu' },
    tables: [{ kind: 'table', sql: t.sql }], views: [{ kind: 'view', sql: j.sql, spec: { order: 20 } }, { kind: 'view', sql: s.sql, spec: { order: 10 } }],
    functions: [{ kind: 'function', sql: r.sql, spec: { rule: true }, description: 'Counts the votes' }], components: [], helpers: [] });
  assert.match(sql, /^-- Sushi\n-- A sushi bar\n-- Version 3: Added the menu\n/);
  assert.ok(sql.indexOf('CREATE SCREEN home') < sql.indexOf('CREATE SCREEN menu'));
  assert.match(sql, /-- Counts the votes\nCREATE RULE count_votes [\s\S]*WHERE id = :row\.feature;\n/);
  assert.ok(!/;;/.test(sql));
});

// Against a real strata-server: the row holds the SQL, a read compiles it,
// and pieces stored before are rewritten as SQL in place on start.
test('the store keeps SQL in Strata and compiles it when read', async (t) => {
  const { findServerBinary, startServer } = await import('../engine/strata.mjs');
  const { Store } = await import('../engine/store.mjs');
  const bin = findServerBinary(join(import.meta.dirname, '..', '..'));
  if (!bin) return t.skip('no strata-server binary');
  const dir = mkdtempSync(join(tmpdir(), 'sa-sql-'));
  const port = await new Promise((r) => { const s = net.createServer().listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => r(p)); }); });
  const { child, client } = await startServer({ bin, db: join(dir, 'apps.strata'), listen: `127.0.0.1:${port}`, log: () => {} });
  try {
    const store = new Store(client, () => {});
    await store.init();
    const statement = "CREATE SCREEN guests TITLE 'Guests'\nLIST guests\n  NOUN 'guest'\n  FORM (name REQUIRED, rsvp SELECT ('yes', 'no'))";
    const { compileStatement } = await import('../engine/statement.mjs');
    const { expandScreen } = await import('../engine/patterns.mjs');
    const screen = expandScreen(compileStatement(statement).screen).screen;
    const id = await store.putCapability({ app: 'a1', slug: 'guests', kind: 'view', title: 'Guests', code: JSON.stringify(screen, null, 1), spec: { screen: true, statement } });
    const [row] = await client.get('sa_capabilities', [id]);
    assert.equal(row.code, statement);
    assert.ok(!JSON.parse(row.spec).statement);
    // Read back by a store that has not seen it: compiled from the SQL.
    const fresh = new Store(client, () => {});
    const [cap] = await fresh.getCapabilities([id]);
    assert.deepEqual(JSON.parse(cap.code), screen);
    assert.equal(cap.sql, statement);
    assert.equal(cap.spec.statement, statement);
    // A row an older engine stored (its compiled JSON) is rewritten as SQL
    // on start, with the same id.
    const oldRule = { steps: [{ insert: 'guests', values: { name: ':name' } }] };
    await client.write([
      { op: 'insert', table: 'sa_capabilities', doc: { id: 'a1/function/add@1', slug: 'add', version: 1, kind: 'function', app: 'a1', title: 'add', description: '', code: JSON.stringify(oldRule), spec: JSON.stringify({ rule: true, on: null }), author: 'forge' } },
      { op: 'insert', table: 'sa_capabilities', doc: { id: 'a1/view/old@1', slug: 'old', version: 1, kind: 'view', app: 'a1', title: 'Old', description: '', code: JSON.stringify(screen), spec: JSON.stringify({ screen: true, statement }), author: 'forge' } },
      { op: 'insert', table: 'sa_capabilities', doc: { id: 'lib/helper/fmt@1', slug: 'fmt', version: 1, kind: 'helper', app: 'lib', title: 'fmt', description: '', code: 'function helper() { return {}; }', spec: '{}', author: 'builtin' } },
    ]);
    await client.write([{ op: 'delete', table: 'sa_settings', key: '_engine:stored-as-sql' }]);
    const again = new Store(client, () => {});
    await again.init();
    const rows = await client.get('sa_capabilities', ['a1/function/add@1', 'a1/view/old@1', 'lib/helper/fmt@1']);
    assert.match(rows[0].code, /^CREATE RULE add AS\n {2}INSERT INTO guests/);
    assert.equal(JSON.parse(rows[0].spec).derived, true);
    assert.equal(rows[1].code, statement.replace('guests TITLE', 'guests TITLE'));
    assert.match(rows[2].code, /^CREATE HELPER fmt LANGUAGE javascript AS \$\$\nfunction helper\(\) \{ return \{\}; \}\n\$\$$/);
    const caps = await again.getCapabilities(['a1/function/add@1', 'a1/view/old@1', 'lib/helper/fmt@1']);
    assert.deepEqual(JSON.parse(caps[0].code), oldRule);
    assert.deepEqual(JSON.parse(caps[1].code), screen);
    assert.equal(caps[2].code, 'function helper() { return {}; }');
  } finally {
    child.kill();
  }
});

test('small changes go to the small model; the build does not write suggestions', () => {
  const app = { comp: { n: 2 } };
  for (const t of ['Rename the Today page to Needs water', 'Sort the books by rating, best first.', 'Make the done tasks grey and move them to the bottom.', 'Hide the notes column in the list']) assert.ok(smallChange(t, app), t);
  for (const t of ['Add a genre to each book and let me filter the list by genre.', 'Email me a weekly summary', "Show our opening hours with 'open now', and today's specials at the top of the menu.", 'Show the number of books per genre as a small bar chart above the list.']) assert.ok(!smallChange(t, app), t);
  assert.ok(!smallChange('Rename the app', { comp: { n: 0 } }), 'a first version is never small');
  if (SUGGEST_AFTER) assert.ok(!/"suggestions"/.test(SYSTEM) && !/- suggestions:/.test(SYSTEM));
});

test('the library SQL functions: read, described, and their table functions kept as written', async () => {
  const { readSqlLibrary, statements, sqlFunctionsDoc } = await import('../engine/sqlfns.mjs');
  const { useTableFunctions } = await import('../engine/sql.mjs');
  assert.deepEqual(statements("CREATE FUNCTION a() RETURNS INT AS $$ 1; $$; -- c;\nCREATE FUNCTION b() RETURNS TEXT AS 'x;y'"), ["CREATE FUNCTION a() RETURNS INT AS $$ 1; $$", "CREATE FUNCTION b() RETURNS TEXT AS 'x;y'"]);
  const fns = readSqlLibrary(join(import.meta.dirname, '..', 'library', 'sql'));
  assert.ok(fns.length >= 20);
  const due = fns.find((f) => f.name === 'due_state');
  assert.equal(due.returns, 'KEYWORD');
  assert.match(due.description, /overdue/);
  assert.match(sqlFunctionsDoc(fns), /amortization\(principal FLOAT, annual_rate_pct FLOAT, months INT\) -> rows, called in FROM/);
  useTableFunctions(fns.filter((f) => f.returns === 'TABLE').map((f) => f.name));
  const out = rewriteSql('SELECT d.day AS day, SUM(r.km) AS km FROM days_from(:s, 7) AS d LEFT JOIN runs r ON r.day = d.day GROUP BY d.day', ['runs'], (t) => `app_x_${t}`);
  assert.equal(out.sql, 'SELECT d.day AS day, SUM(r.km) AS km FROM days_from(:s, 7) AS d LEFT JOIN app_x_runs r ON r.day = d.day GROUP BY d.day');
  assert.throws(() => rewriteSql('SELECT * FROM days_from', ['runs'], (t) => t), /no table days_from/);
  useTableFunctions([]);
});

test('the library SQL functions run in Strata', async (t) => {
  const { findServerBinary, startServer } = await import('../engine/strata.mjs');
  const { readSqlLibrary, install } = await import('../engine/sqlfns.mjs');
  const bin = findServerBinary(join(import.meta.dirname, '..', '..'));
  if (!bin) return t.skip('no strata-server binary');
  const dir = mkdtempSync(join(tmpdir(), 'sa-fns-'));
  const port = await new Promise((r) => { const s = net.createServer().listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => r(p)); }); });
  const { child, client } = await startServer({ bin, db: join(dir, 'apps.strata'), listen: `127.0.0.1:${port}`, log: () => {} });
  try {
    const fns = readSqlLibrary(join(import.meta.dirname, '..', 'library', 'sql'));
    const logs = [];
    const ok = await install(client, fns, (m) => logs.push(m));
    if (!ok.length && logs.some((m) => /no stored functions/.test(m))) return t.skip('this strata-server keeps no stored functions');
    assert.equal(ok.length, fns.length, logs.join('\n'));
    // Defining them again writes no new version.
    assert.equal((await install(client, fns)).length, fns.length);
    await client.sql('CREATE TABLE t (id KEYWORD PRIMARY KEY)');
    await client.write([{ op: 'upsert', table: 't', doc: { id: 'a' } }]);
    const one = async (expr) => (await client.query(`SELECT ${expr} AS v FROM t LIMIT 1`))[0].v;
    assert.equal(await one('loan_payment(300000, 6, 360)'), 1798.65);
    assert.equal(await one("due_state('2026-11-01', '2026-11-08')"), 'overdue');
    assert.equal(await one("days_left('2026-11-10', '2026-11-08')"), 2);
    assert.equal(await one("add_days('2026-11-28', 5)"), '2026-12-03');
    assert.equal(await one('with_tax(100, 8.5)'), 108.5);
    assert.equal(await one('stock_state(2, 5)'), 'low');
    assert.equal(await one('reorder_qty(2, 5, 20, 6)'), 18);
    const sched = await client.query('SELECT * FROM amortization(20000, 4.5, 48)');
    assert.equal(sched.length, 48);
    assert.equal(sched.at(-1).balance, 0);
    const days = await client.query("SELECT * FROM days_from('2026-11-28', 3)");
    assert.deepEqual(days.map((d) => d.day), ['2026-11-28', '2026-11-29', '2026-11-30']);
    // Over the rows: dates as text, as keywords, and as timestamps.
    await client.sql('CREATE TABLE tasks (id KEYWORD PRIMARY KEY, due TEXT, day KEYWORD, at TIMESTAMP)');
    await client.write([{ op: 'upsert', table: 'tasks', doc: { id: 'a', due: '2026-11-05', day: '2026-11-08', at: '2026-11-09T15:00:00Z' } },
      { op: 'upsert', table: 'tasks', doc: { id: 'b', due: '2026-11-10', day: '2026-11-30', at: '2026-12-01T08:00:00Z' } }]);
    const rows = await client.query('SELECT id, due_state(due, :t) AS s, days_left(due, :t) AS n, add_days(day, 3) AS later, days_between(day, at) AS gap FROM tasks ORDER BY id', { t: '2026-11-08' });
    assert.deepEqual(rows.map((r) => [r.id, r.s, r.n, r.later, r.gap]), [['a', 'overdue', -3, '2026-11-11', 1], ['b', 'soon', 2, '2026-12-03', 1]]);
  } finally {
    child.kill();
  }
});

// Against a real strata-server: what people do is stored with its columns
// (session, screen, table, row, fields, words, results, time), and Strata
// counts the patterns in it with the engine's table functions.
test('the patterns of use are counted in Strata', async (t) => {
  const { findServerBinary, startServer } = await import('../engine/strata.mjs');
  const { Store } = await import('../engine/store.mjs');
  const { readSqlFile, install } = await import('../engine/sqlfns.mjs');
  const { signalsFromSql } = await import('../engine/evolve.mjs');
  const bin = findServerBinary(join(import.meta.dirname, '..', '..'));
  if (!bin) return t.skip('no strata-server binary');
  const dir = mkdtempSync(join(tmpdir(), 'sa-sig-'));
  const port = await new Promise((r) => { const s = net.createServer().listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => r(p)); }); });
  const { child, client } = await startServer({ bin, db: join(dir, 'apps.strata'), listen: `127.0.0.1:${port}`, log: () => {} });
  try {
    const store = new Store(client, () => {});
    await store.init();
    const fns = readSqlFile(join(import.meta.dirname, '..', 'engine', 'signals.sql'));
    const logs = [];
    const ok = await install(client, fns, (m) => logs.push(m));
    if (!ok.length && logs.some((m) => /no stored functions/.test(m))) return t.skip('this strata-server keeps no stored functions');
    assert.equal(ok.length, fns.length, logs.join('\n'));
    // The store keeps each column, words in lower case and fields sorted.
    await store.track('a1', 'update', '', 'guests: table, rsvp', { person: 'p1', session: 's9', screen: 'guests', tbl: 'guests', rid: 'g1', fields: ['table', 'rsvp'] });
    await store.track('a1', 'action', '', 'search Big  Party', { session: 's9', q: 'Big  Party', results: 0 });
    const [kept] = (await store.recentEvents('a1', 5)).filter((e) => e.kind === 'update');
    assert.equal(kept.fields, 'rsvp,table');
    assert.equal(kept.screen, 'guests');
    assert.equal(kept.rid, 'g1');
    assert.equal((await store.recentEvents('a1', 5)).find((e) => e.kind === 'action').q, 'big party');
    // A write keeps the name of its row; events() reads a period's events.
    await store.track('a1', 'delete', '', 'guests', { session: 's9', tbl: 'guests', rid: 'g2', label: 'Ana Lima' });
    const since = await store.events('a1', { since: new Date(Date.now() - 60000).toISOString() });
    assert.equal(since.length, 3);
    assert.equal(since.find((e) => e.kind === 'delete').label, 'Ana Lima');
    assert.equal((await store.events('a1', { since: new Date(Date.now() + 60000).toISOString() })).length, 0);
    // Then a day of use, with times, written as the engine writes events.
    const docs = [];
    const ev = (min, sec, o) => docs.push({ op: 'insert', table: 'sa_events', doc: { id: `e${docs.length}`, app: 'a2', ts: at(min, sec), ...o } });
    // "overdue" typed in pauses, three times by two people: three searches.
    for (const [m, who] of [[0, 'p1'], [5, 'p2'], [9, 'p1']]) for (const [i, q] of ['ov', 'over', 'overdue'].entries()) ev(m, i * 2, { kind: 'action', detail: `search ${q}`, q, results: 2, person: who, session: `${who}-${m}` });
    // A caterer looked for twice and never found.
    for (const m of [12, 13]) ev(m, 0, { kind: 'action', detail: 'search caterer', q: 'caterer', results: 0, person: 'p1', session: 's1' });
    for (let i = 0; i < 4; i++) ev(14 + i, 0, { kind: 'action', detail: 'filter status=waiting', q: 'status=waiting', results: 3, person: 'p1', session: 's1' });
    for (let i = 0; i < 6; i++) ev(20, i * 5, { kind: 'update', detail: 'guests: rsvp', tbl: 'guests', rid: `g${i}`, fields: 'rsvp', person: 'p1', session: 's1' });
    for (let i = 0; i < 4; i++) { ev(30 + i * 2, 0, { kind: 'open', detail: 'guests', person: 'p1', session: 's1' }); ev(30 + i * 2, 10, { kind: 'open', detail: 'budget', person: 'p1', session: 's1' }); }
    for (let i = 0; i < 5; i++) ev(45 + i, 0, { kind: 'call', detail: 'send-reminders', person: 'p1', session: 's1' });
    // Three guests added and deleted again a minute later.
    for (let i = 0; i < 3; i++) { ev(50 + i, 0, { kind: 'insert', detail: 'guests', tbl: 'guests', rid: `x${i}`, person: 'p1', session: 's1' }); ev(51 + i, 0, { kind: 'delete', detail: 'guests', tbl: 'guests', rid: `x${i}`, person: 'p1', session: 's1' }); }
    // The new guest form typed into and closed unsaved, three times.
    for (let i = 0; i < 3; i++) ev(55 + i, 0, { kind: 'abandon', detail: 'guests new', screen: 'guests', tbl: 'guests', ms: 8000, person: `p${i}`, session: `t${i}` });
    await client.write(docs);
    const sigs = await signalsFromSql(client, 'a2', { views: [{ slug: 'guests', title: 'Guests' }, { slug: 'budget', title: 'Budget' }] });
    const by = Object.fromEntries(sigs.map((x) => [x.key, x]));
    assert.equal(by['search:overdue']?.count, 3, Object.keys(by).join(', '));
    assert.equal(by['search:overdue'].people, 2);
    assert.ok(!by['search:over'] && !by['search:ov'], 'pauses while typing are not searches');
    assert.match(by['nothing:search:caterer'].text, /searched for "caterer" and found nothing, 2 times/);
    assert.ok(by['filter:status=waiting']);
    assert.match(by['edits:guests: rsvp'].text, /changed rsvp of guests by hand, row after row: 6 times in one sitting/);
    assert.match(by['path:guests>budget'].text, /went from Guests straight to Budget 4 times/);
    assert.ok(by['call:send-reminders']);
    assert.match(by['undo:guests'].text, /deleted rows of guests within minutes of adding them, 3 times/);
    assert.match(by['abandon:guests/guests new'].text, /typed into the new guests form on Guests and closed it unsaved, 3 times \(3 people\) \(8 s of typing on average\)/);
    // A kept change is measured by the same columns.
    const events = await store.recentEvents('a2', 500);
    const e = measureEffect(events, measureFor('edits:guests: rsvp'), at(25));
    assert.equal(e.before, 6);
    assert.equal(measureEffect(events, measureFor('nothing:search:caterer'), at(0)).after, 2);
    assert.equal(measureEffect(events, measureFor('abandon:guests/guests new'), at(0)).after, 3);
    const screens = await client.query('SELECT * FROM sa_use_screens(:a)', { a: 'a2' });
    assert.deepEqual(screens, []);
  } finally {
    child.kill();
  }
});

test('a request is sorted into kinds of change', async () => {
  const { kindsOf, changeDocs } = await import('../engine/changes.mjs');
  assert.deepEqual(kindsOf('Sort the runs newest first.'), ['sort']);
  assert.deepEqual(kindsOf('Add a weekly digest email of new requests'), ['notify']);
  assert.deepEqual(kindsOf('Show the number of books per genre as a small bar chart above the list.'), ['totals', 'chart']);
  assert.deepEqual(kindsOf('hello there'), []);
  // An app did each kind once per change; a kind points at the kinds that followed it.
  const docs = changeDocs(new Map([['a', [['search'], ['chart', 'totals']]]]));
  assert.deepEqual(docs.find((d) => d.id === 'app:a').links, [['did', 'change:search'], ['did', 'change:chart'], ['did', 'change:totals']]);
  assert.deepEqual(docs.find((d) => d.id === 'change:search').links, [['then', 'change:chart'], ['then', 'change:totals']]);
});

// Against a real strata-server: the next change ranked by ppr over the graph
// of what apps did, and suggestions ordered by how often their kind is taken.
test('what apps change next is learned across apps, in Strata', async (t) => {
  const { findServerBinary, startServer } = await import('../engine/strata.mjs');
  const { Store } = await import('../engine/store.mjs');
  const { Changes } = await import('../engine/changes.mjs');
  const bin = findServerBinary(join(import.meta.dirname, '..', '..'));
  if (!bin) return t.skip('no strata-server binary');
  const dir = mkdtempSync(join(tmpdir(), 'sa-next-'));
  const port = await new Promise((r) => { const s = net.createServer().listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => r(p)); }); });
  const { child, client } = await startServer({ bin, db: join(dir, 'apps.strata'), listen: `127.0.0.1:${port}`, log: () => {} });
  try {
    const store = new Store(client, () => {});
    await store.init();
    // Four apps added a search and then a chart; one added a search and then
    // an export; the new app has added a search.
    const asks = { a1: ['Add a search box', 'Add a bar chart of the totals'], a2: ['Let me search my notes', 'Show a chart of notes per week'], a3: ['Search by title', 'A chart of spending'],
      a4: ['Find by name', 'Add a chart'], a5: ['Search by tag', 'Export as CSV'], me: ['Add a search'] };
    let t0 = Date.parse('2026-11-08T10:00:00Z');
    const docs = [];
    for (const [app, list] of Object.entries(asks)) for (const text of [`A ${app} app`, ...list]) docs.push({ op: 'insert', table: 'sa_requests', doc: { id: `r${docs.length}`, app, text, source: 'user', status: 'done', created: new Date(t0 += 1000).toISOString() } });
    await client.write(docs);
    const changes = new Changes(store, () => {});
    await changes.init();
    assert.deepEqual(await changes.next('me'), [], 'fewer than ten other apps: nothing yet');
    const next = await changes.next('me', { least: 3 });
    assert.equal(next[0]?.kind, 'chart', JSON.stringify(next));
    assert.equal(next[0].apps, 4);
    assert.ok(!next.some((n) => n.kind === 'search'), 'a kind the app made already is not offered');
    // Suggestions: exports were never taken, charts were.
    const sug = [];
    for (let i = 0; i < 6; i++) sug.push({ op: 'insert', table: 'sa_suggestions', doc: { id: `s${i}`, app: 'a1', text: 'Export the rows as CSV', status: i ? 'stale' : 'dismissed' } });
    for (let i = 0; i < 5; i++) sug.push({ op: 'insert', table: 'sa_suggestions', doc: { id: `c${i}`, app: 'a2', text: 'Add a chart of the week', status: i < 3 ? 'taken' : 'stale' } });
    await client.write(sug);
    const rates = await changes.takeRates();
    assert.equal(rates.export.offered, 6);
    assert.equal(rates.export.taken, 0);
    assert.equal(rates.chart.rate, 0.6);
    const ranked = await changes.rank([{ text: 'Download a CSV export' }, { text: 'A chart of runs per week' }]);
    assert.deepEqual(ranked.map((s) => s.text), ['A chart of runs per week', 'Download a CSV export']);
    const part = await changes.promptPart('me', { least: 3 });
    assert.match(part, /What apps like this one changed next \(learned from 6 apps here\)\n- a chart or a dashboard \(4 other apps added one\)/);
    assert.match(part, /Taken most: a chart or a dashboard: 3 of 5/);
    assert.match(part, /Rarely taken: an export, a document or an invoice: 0 of 6/);
  } finally {
    child.kill();
  }
});

test('an event is told by the name of the row it wrote', async () => {
  const { singular, rowLabel, activity } = await import('../engine/activity.mjs');
  for (const [many, one] of [['plants', 'plant'], ['categories', 'category'], ['boxes', 'box'], ['people', 'person'], ['status', 'status'], ['glass', 'glass'], ['check_ins', 'check in']]) assert.equal(singular(many), one);
  const columns = { habits: [{ name: 'name', type: 'text' }, { name: 'goal', type: 'int' }], checkins: [{ name: 'habit', type: 'keyword' }, { name: 'day', type: 'keyword' }, { name: 'note', type: 'text' }],
    waterings: [{ name: 'plant_name', type: 'keyword' }, { name: 'day', type: 'keyword' }], notes: [{ name: 'body', type: 'text' }] };
  const tables = Object.entries(columns).map(([name, cols]) => ({ name, columns: cols }));
  const rows = { app_a1_habits: { h1: { id: 'h1', name: 'Read 20 pages' } }, app_a1_checkins: { c1: { id: 'c1', habit: 'h1', day: '2026-11-08' } } };
  const db = { get: async (t, ids) => ids.map((id) => rows[t]?.[id] || null) };
  // its own name; the name of what it points to, with its day; another
  // table's name kept in a column; its first short text
  assert.equal(await rowLabel(db, 'a1', 'habits', rows.app_a1_habits.h1, tables), 'Read 20 pages');
  assert.equal(await rowLabel(db, 'a1', 'checkins', rows.app_a1_checkins.c1, tables), 'Read 20 pages · 2026-11-08');
  assert.equal(await rowLabel(db, 'a1', 'waterings', { id: 'w1', plant_name: 'Monstera', day: '2026-11-08' }, tables), 'Monstera · 2026-11-08');
  assert.equal(await rowLabel(db, 'a1', 'notes', { id: 'n1', body: 'Call   the vet' }, tables), 'Call the vet');
  assert.equal(await rowLabel(db, 'a1', 'notes', { id: 'n2', body: 'https://example.com/a' }, tables), null);
  assert.equal(await rowLabel(db, 'a1', 'gone', { id: 'x' }, tables), null);
  // The view's events: a label kept with the event wins (a removed row keeps
  // its name), else the row as it is now; empty fields are left out.
  let asked = null;
  const engine = {
    view: async () => ({ app: { id: 'a1', name: 'Habits' }, tables: tables.map((t) => ({ spec: { name: t.name, columns: t.columns } })),
      views: [{ id: 'a1/view/manage@1', slug: 'manage', title: 'Manage', spec: { order: 60 } }, { id: 'a1/view/today@2', slug: 'today', title: 'Today', spec: { order: 10 } }], functions: [{ id: 'a1/function/streaks@1', slug: 'streaks', title: 'streaks' }] }),
    store: {
      events: async (app, o) => { asked = o; return [
        { kind: 'insert', tbl: 'checkins', rid: 'c1', label: null, person: null, ts: '2026-11-08T10:00:00Z' },
        { kind: 'delete', tbl: 'habits', rid: 'h9', label: 'Meditate', ts: '2026-11-08T09:00:00Z' },
        { kind: 'update', tbl: 'habits', rid: 'h1', fields: 'goal', ts: '2026-11-08T08:00:00Z' },
        { kind: 'open', detail: 'today', screen: 'today', session: 's1', ts: '2026-11-08T07:00:00Z' }]; },
      countRows: async (t) => Object.keys(rows[t] || {}).length,
    },
    db,
  };
  const out = await activity(engine, 'a1', { days: 7 });
  assert.equal(asked.limit, 5000);
  assert.ok(Math.abs(Date.parse(asked.since) - (Date.now() - 7 * 86400000)) < 5000);
  assert.deepEqual(out.events.map((e) => e.label || null), ['Read 20 pages · 2026-11-08', 'Meditate', 'Read 20 pages', null]);
  assert.ok(!('person' in out.events[0]), 'empty fields are left out');
  assert.deepEqual(out.screens.map((x) => x.slug), ['today', 'manage']);
  assert.deepEqual(out.tables.map((t) => [t.name, t.noun, t.rows]), [['habits', 'habit', 1], ['checkins', 'checkin', 1], ['waterings', 'watering', 0], ['notes', 'note', 0]]);
  assert.equal((await activity(engine, 'a1', { days: 0 })).since, null);
  assert.equal(asked.since, null);
});

test('a layout class on a table cell leaves it a cell', async () => {
  const { readFileSync } = await import('node:fs');
  const kit = readFileSync(join(import.meta.dirname, '..', 'web', 'kit.css'), 'utf8');
  // .center is a grid; on a td it stacked a week's days one under another.
  assert.match(kit, /:is\(td, th\):is\([^)]*\.center[^)]*\) \{ display: table-cell; \}/);
  assert.match(kit, /tr:is\([^)]*\.row[^)]*\) \{ display: table-row; \}/);
});

test('a thing done each day is a CHECKS part: one tap per item per day, its table and columns checked', async () => {
  const { compileStatement } = await import('../engine/statement.mjs');
  const { expandScreen } = await import('../engine/patterns.mjs');
  const c = compileStatement(`CREATE SCREEN today TITLE 'Today'
CHECKS checkins
  ITEMS habits  ITEM habit  DAY day  NAME name  DAYS 5  ADD  NOUN 'habit'`);
  const [part] = c.screen.body;
  assert.deepEqual({ use: part.use, table: part.table, items: part.items, item: part.item, day: part.day, name: part.name, days: part.days, add: part.add, noun: part.noun },
    { use: 'checks', table: 'checkins', items: 'habits', item: 'habit', day: 'day', name: 'name', days: 5, add: true, noun: 'habit' });
  const e = expandScreen(c.screen);
  assert.deepEqual(e.problems, []);
  const cols = { habits: ['name'], checkins: ['habit', 'day'] };
  const known = (over) => (t) => ((over || cols)[t] ? new Set(['id', 'created', ...(over || cols)[t]]) : null);
  assert.deepEqual(checkScreen(e.screen, known()), []);
  // a log table without the day column, or a table the app lacks, is named
  assert.match(checkScreen(e.screen, known({ habits: ['name'], checkins: ['habit', 'when'] })).join('\n'), /day/);
  assert.match(checkScreen(e.screen, known({ habits: ['name'] })).join('\n'), /checkins/);
  assert.throws(() => compileStatement('CREATE SCREEN x\nCHECKS log\n  DAYS many'), /line 3/);
});

test('every design keeps grey text readable, on the page and on cards', async () => {
  const { PRESETS, mutedOf, contrast, checkDesign } = await import('../web/design.js');
  for (const p of PRESETS) for (const mode of ['light', 'dark']) {
    const d = checkDesign(p);
    const m = mutedOf(d[mode], mode === 'dark' ? 58 : 55);
    assert.ok(contrast(m, d[mode].bg) >= 4.5 && contrast(m, d[mode].card) >= 4.5, `${p.name} ${mode}`);
  }
  // a pale grey mixes toward the text until it reads
  const pale = { bg: '#ffffff', card: '#f6f6f6', fg: '#222222' };
  assert.ok(contrast(mutedOf(pale, 10), pale.card) >= 4.5);
});

test('every app gets the same touch rules: 44 px taps, tables as cards on a phone, Undo instead of a question', async () => {
  const { readFileSync } = await import('node:fs');
  const web = (f) => readFileSync(join(import.meta.dirname, '..', 'web', f), 'utf8');
  const kit = web('kit.css');
  // a small button is still 44 px tall under a finger
  assert.match(kit, /@media \(pointer: coarse\) \{\s*button:is\(\.small, \*\)[^{]*\{ min-height: 44px; \}/);
  assert.match(kit, /table\.stack thead \{ display: none; \}/);
  // a tick in a row is a 44 px target; a form above a list folds on a phone; big numbers are one row
  assert.match(kit, /@media \(pointer: coarse\) \{ td\.tick label\.tap \{ min-width: 44px; min-height: 44px; \}/);
  assert.match(kit, /\.fold\.phone:not\(\.open\) > form \{ display: none; \}/);
  assert.match(kit, /\.kpis \{ grid-template-columns: repeat\(auto-fit, minmax\(96px, 1fr\)\)/);
  assert.match(web('screen.js'), /b\.fold === false \|\| !\(b\.fold === true \|\| b\.fold === 'phone' \|\| \(listing && n >= 3\)\)/);
  // a remove offers Undo, and a delete asks nothing first
  assert.match(web('frame.js'), /remove: \(table, id, opts\) => rpc\('remove', \[table, id\]\)\.then/);
  assert.match(web('screen.js'), /const asks = \(q, fn\) => !!q && !\/\\bapp\\\.remove\\\(\/\.test\(String\(fn\)\)/);
  for (const f of ['crud', 'board', 'calendar', 'detail']) assert.ok(!/"confirm": "Delete/.test(readFileSync(join(import.meta.dirname, '..', 'library', 'patterns', `${f}.json`), 'utf8')), f);
  // the engine hands back the removed row in a form insert takes back
  const { Engine } = await import('../engine/engine.mjs');
  const rows = new Map([['p1', { id: 'p1', created: '2026-11-08T10:00:00Z', name: 'Monstera', every: 7, _score: 1 }]]);
  const engine = new Engine({
    get: async (t, ids) => ids.map((id) => rows.get(id) || null),
    write: async (ops) => { for (const o of ops) { if (o.op === 'delete') rows.delete(o.key); else rows.set(o.doc.id, o.doc); } },
  }, { log: () => {} });
  engine.store = { track: async () => {} };
  engine.view = async () => ({ app: { id: 'a' }, comp: { n: 1 }, helpers: [], views: [], functions: [], tables: [{ spec: { name: 'plants', columns: [{ name: 'name', type: 'text' }, { name: 'every', type: 'int' }] } }] });
  const out = await engine.remove('a', 'plants', 'p1');
  assert.deepEqual(out, { removed: 1, row: { id: 'p1', created: '2026-11-08T10:00:00Z', name: 'Monstera', every: 7 } });
  assert.equal(rows.size, 0);
  await engine.insert('a', 'plants', out.row);
  assert.deepEqual(rows.get('p1'), { id: 'p1', created: '2026-11-08T10:00:00.000Z', name: 'Monstera', every: 7 });
  assert.deepEqual(await engine.remove('a', 'plants', 'nope'), { removed: 0, row: null });
});

test('a new app is planned from a UX brief: a short name, who, the job, how often, the device, the first screen and what it shows', () => {
  const views = [{ slug: 'manage', title: 'Habits', order: 1 }, { slug: 'today', title: 'Today', order: 2 }, { slug: 'history', title: 'History', order: 3 }];
  const ux = { who: 'me, on my phone', job: 'tick off my habits', often: 'daily', device: 'phone', first: 'today', glance: 'how many are done today' };
  assert.deepEqual(checkBrief({ name: 'Habits', ux, views }, empty), []);
  const said = (plan) => checkBrief(plan, empty).join('\n');
  assert.match(said({ ux, views }), /"name": a new app needs a short name/);
  assert.match(said({ name: 'A habit tracker: my daily habits, a chec', ux, views }), /is long: 2 to 4 words/);
  assert.match(said({ name: 'Habits', views }), /"ux": a new app gives its brief/);
  assert.match(said({ name: 'Habits', ux: { ...ux, often: 'a lot', device: 'watch', job: '' }, views }), /ux\.job[\s\S]*ux\.often "a lot"[\s\S]*ux\.device "watch"/);
  assert.match(said({ name: 'Habits', ux: { ...ux, first: 'home' }, views }), /ux\.first "home": .*one of manage, today, history/);
  // a change needs neither, and may restate the brief
  const app = { ...empty, comp: { n: 3, caps: [] }, views: [{ slug: 'today', spec: { order: 1 } }] };
  assert.deepEqual(checkBrief({ views: [] }, app), []);
  assert.deepEqual(checkBrief({ ux: { ...ux, device: 'both' } }, app), []);
  // the screen where the job is done opens first
  const plan = { ux, views: views.map((v) => ({ ...v })) };
  orderByBrief(plan, empty);
  assert.deepEqual(plan.views.sort((a, b) => a.order - b.order).map((v) => v.slug), ['today', 'manage', 'history']);
  // kept with the app as JSON, read back for later changes
  assert.equal(briefOf({ ux: JSON.stringify(ux) }).first, 'today');
  assert.equal(briefOf({ ux: 'not json' }), null);
  assert.equal(briefOf({}), null);
  // a brief in other words is taken in the nearest, not refused
  const near = (often, device = 'phone') => { const p = { ux: { ...ux, often, device } }; normalizeBrief(p); return [p.ux.often, p.ux.device]; };
  assert.deepEqual(near('several times a week'), ['several times a week', 'phone']);
  assert.deepEqual(near('3 times a week', 'mobile'), ['several times a week', 'phone']);
  assert.deepEqual(near('every day', 'laptop'), ['daily', 'desktop']);
  assert.deepEqual(near('a few times a day', 'phone and computer'), ['several times a day', 'both']);
  assert.deepEqual(near('once a month'), ['monthly', 'phone']);
  assert.deepEqual(near('a lot', 'watch'), ['now and then', 'watch']);
  const asked = { name: 'Runs', ux: { ...ux, often: 'a few times a week' }, views };
  completePlan(asked, empty, []);
  assert.deepEqual(checkBrief(asked, empty), []);
});

test('every version is drawn on a phone and a computer, and the rules find what stands in the way', async (t) => {
  const { rules, repairRequest, measure } = await import('../engine/look.mjs');
  const ok = { width: 390, scroll: 390, height: 900, screen: 740, wide: null, small: [], smallCount: 0, repeats: [], faint: [], faintCount: 0, ids: [], fieldsOnFirstScreen: 1, text: 200, controls: 6 };
  assert.deepEqual(rules(ok, 'phone'), []);
  const bad = { ...ok, scroll: 520, wide: { right: 520, what: 'table' }, small: ['Edit (44×26 px)'], smallCount: 8, faint: [{ text: 'No streak yet', ratio: 3.1 }], faintCount: 3, ids: ['ai7nozxh2o'], repeats: ['"Mark done" ×28'] };
  const said = rules(bad, 'phone');
  assert.deepEqual(said.map((r) => r.severity), ['high', 'high', 'high', 'high', 'low']);
  assert.match(said[0].problem, /130 px wider than the phone and scrolls sideways \(table reaches 520 px\)/);
  assert.match(said[1].problem, /8 things to tap are smaller than a finger \(44 px\): Edit/);
  assert.match(said[2].problem, /"No streak yet" at 3.1:1/);
  assert.match(said[3].problem, /raw ids are shown: ai7nozxh2o/);
  // small targets are a phone's problem; a computer has a pointer
  assert.ok(!rules(bad, 'computer').some((r) => /finger/.test(r.problem)));
  assert.equal(repairRequest([{ view: 'Today', device: 'phone', problem: 'x', severity: 'low' }]), null);
  assert.match(repairRequest([{ view: 'Books', device: 'phone', problem: 'The list is below the form.', fix: 'Put the list first', severity: 'high' }]), /- Books \(phone\): The list is below the form\. Fix: Put the list first$/);
  // measured in a browser, when one is installed
  const { loadPlaywright } = await import('../engine/checks.mjs');
  const pw = loadPlaywright();
  if (!pw) { t.diagnostic('no playwright: the measuring is not run'); return; }
  const browser = await pw.chromium.launch();
  try {
    const page = await browser.newPage({ viewport: { width: 390, height: 844 } });
    await page.setContent(`<body style="margin:0;background:#fff;font:16px sans-serif">
      <button style="height:24px">Edit</button><button style="height:48px;width:120px">Save</button>
      <p style="color:#bbb">faint words</p><p style="color:#222">plain words</p><p>row ai7nozxh2o</p>
      <div style="width:600px;height:10px"></div><input><input><select></select></body>`);
    const m = await page.evaluate(measure, 844);
    assert.equal(m.scroll, 600);
    assert.equal(m.smallCount, 4, m.small.join(', ')); // Edit, two inputs and a select under 40 px
    assert.deepEqual(m.faint.map((f) => f.text), ['faint words']);
    assert.deepEqual(m.ids, ['ai7nozxh2o']);
    assert.equal(m.fieldsOnFirstScreen, 3);
  } finally { await browser.close(); }
});

test('a screen has a budget: a change consolidates rather than piling up, and keeps its parts', async () => {
  const { budgetOf, overBudget, isBudget, BUDGET } = await import('../engine/forge.mjs');
  const { compileStatement } = await import('../engine/statement.mjs');
  const { joinAsks } = await import('../engine/changes.mjs');
  const part = compileStatement(`CREATE SCREEN today TITLE 'Today'
CHECKS checkins
  ITEMS habits  ITEM habit  SKIP skipped`).screen;
  const before = budgetOf(part);
  assert.deepEqual(before, { parts: 1, kpis: 0, fields: 0, code: 0, uses: ['checks'] });
  // the same screen said with code instead of the part
  const code = { let: { rows: '=habits.map(h => ({h, days: [6,5,4,3,2,1,0].map(i => lib.dates.addDays(today, -i))}))' },
    body: [{ h: 'Today' }, { each: '=rows', as: 'v', body: [{ text: '=v.h.name' }, { each: '=v.days', as: 'd', body: [{ button: '=d.slice(8)', do: "=() => app.insert('checkins', {habit: v.h.id, day: d})" }] }] }] };
  const now = budgetOf(code);
  assert.ok(now.code > 100 && now.uses.length === 0);
  const said = overBudget('today', now, before);
  assert.equal(said.length, 1);
  assert.match(said[0], /^over budget: screen today gives up its CHECKS part for code: keep it/);
  assert.ok(said.every(isBudget));
  // code grows by at most BUDGET.grow per change; parts, numbers and fields have ceilings
  const big = { parts: 9, kpis: 5, fields: 10, code: 300 + BUDGET.grow + 1, uses: [] };
  const msgs = overBudget('home', big, { parts: 3, kpis: 2, fields: 4, code: 300, uses: [] }).join('\n');
  for (const want of ['has 9 parts', 'has 5 big numbers', 'has 10 fields', `grows by ${(BUDGET.grow + 1).toLocaleString('en-US')} characters`]) assert.ok(msgs.includes(want), want);
  // a screen already over a ceiling may stay there, but not grow past it
  assert.deepEqual(overBudget('home', { parts: 9, kpis: 0, fields: 0, code: 0, uses: [] }, { parts: 9, kpis: 0, fields: 0, code: 0, uses: [] }), []);
  // asks made while a build runs are built as one change
  assert.equal(joinAsks(['Add a note']), 'Add a note');
  assert.match(joinAsks(['Let me skip a day', 'Show a weekly count']), /^Make these 2 changes together, as one design: .*\n1\. Let me skip a day\n2\. Show a weekly count$/);
});

test('UX measures from use: taps and seconds to do something, the first screen, forms given up, rows added by mistake, Undo; judged against the brief, version by version', async () => {
  const { uxMeasures, uxSignals, uxEffect, targetsOf, sittingsOf } = await import('../engine/measures.mjs');
  const { measureFor } = await import('../engine/evolve.mjs');
  const { Store } = await import('../engine/store.mjs');
  const t0 = Date.parse('2026-11-01T08:00:00Z');
  const at = (s) => new Date(t0 + s * 1000).toISOString();
  const ev = [];
  // Version 1: six visits that open on Today, go to Habits and add one there
  // after 20 s and 6 taps.
  for (let i = 0; i < 6; i++) {
    const b = i * 3600;
    ev.push({ kind: 'open', detail: 'today', session: `a${i}`, v: 1, ts: at(b) },
      { kind: 'open', detail: 'habits', session: `a${i}`, v: 1, ts: at(b + 5) },
      { kind: 'insert', tbl: 'habits', rid: `h${i}`, screen: 'habits', taps: 6, session: `a${i}`, v: 1, ts: at(b + 20) });
  }
  // A form given up, a row removed two minutes after it was added, a remove undone.
  ev.push({ kind: 'abandon', detail: 'habits new', tbl: 'habits', session: 'a0', v: 1, ts: at(30) },
    { kind: 'delete', tbl: 'habits', rid: 'h1', screen: 'habits', taps: 1, session: 'a1', v: 1, ts: at(3600 + 140) },
    { kind: 'delete', tbl: 'habits', rid: 'h2', screen: 'habits', taps: 1, session: 'a2', v: 1, ts: at(7200 + 60) },
    { kind: 'insert', tbl: 'habits', rid: 'h2', screen: 'habits', taps: 1, session: 'a2', v: 1, ts: at(7200 + 64) },
    { kind: 'delete', tbl: 'habits', rid: 'h3', screen: 'habits', taps: 1, session: 'a3', v: 1, ts: at(10800 + 60) },
    { kind: 'insert', tbl: 'habits', rid: 'h3', screen: 'habits', taps: 1, session: 'a3', v: 1, ts: at(10800 + 66) });
  // Version 2: the job on Today, in one tap and 4 s.
  for (let i = 0; i < 6; i++) {
    const b = 86400 + i * 3600;
    ev.push({ kind: 'open', detail: 'today', session: `b${i}`, v: 2, ts: at(b) },
      { kind: 'insert', tbl: 'checkins', rid: `c${i}`, screen: 'today', taps: 1, session: `b${i}`, v: 2, ts: at(b + 4) });
  }
  // A tab left open: an hour later is another visit.
  assert.equal(sittingsOf([{ session: 'x', ts: at(0) }, { session: 'x', ts: at(600) }, { session: 'x', ts: at(4000) }]).length, 2);
  const screens = [{ slug: 'habits', title: 'Habits', order: 60 }, { slug: 'today', title: 'Today', order: 10 }];
  const brief = { who: 'someone building habits', job: 'check off today\'s habits', often: 'daily', device: 'phone' };
  assert.deepEqual([targetsOf(brief).taps, targetsOf({ ...brief, often: 'weekly' }).taps], [3, 5]);
  const v1 = uxMeasures(ev.filter((e) => e.v === 1), { screens, brief });
  assert.equal(v1.first, 'today');
  assert.equal(v1.sittings, 6);
  assert.equal(v1.measures.tapsToAct, 6);
  assert.equal(v1.measures.secsToAct, 20);
  assert.equal(v1.measures.screensToAct, 2);
  assert.equal(v1.measures.onFirst, 0);
  assert.ok(Math.abs(v1.measures.undone - 2 / 3) < 1e-9, 'two removes of three were undone');
  assert.equal(v1.measures.inserts, 6, 'the row put back is not counted as added');
  assert.ok(Math.abs(v1.measures.wrong - 1 / 6) < 1e-9, 'h1 was removed two minutes after it was added; h2 and h3 were put back');
  assert.ok(Math.abs(v1.measures.gaveUp - 1 / 7) < 1e-9);
  assert.deepEqual(v1.findings.map((f) => [f.key, f.severity]), [['taps', 'medium'], ['secs', 'medium'], ['first', 'high'], ['wrong', 'medium'], ['undo', 'medium']]);
  assert.match(v1.findings[0].text, /6 taps .*check off today's habits.* 3 or fewer/);
  assert.match(v1.findings[2].text, /first screen, Today; most is done on Habits/);
  const all = uxMeasures(ev, { screens, brief });
  assert.deepEqual(all.versions.map((x) => [x.v, x.sittings, x.tapsToAct, x.secsToAct, x.onFirst]), [[1, 6, 6, 20, 0], [2, 6, 1, 4, 1]]);
  // Too few visits to judge: measured, not judged.
  assert.deepEqual(uxMeasures(ev.slice(0, 9), { screens, brief }).findings, []);
  // The findings are use signals, and a change kept for one is judged by it.
  assert.deepEqual(uxSignals(v1).map((s) => s.key), ['ux:taps', 'ux:secs', 'ux:first', 'ux:wrong', 'ux:undo']);
  assert.deepEqual(measureFor({ key: 'ux:taps' }), { ux: 'taps', goal: 'fewer', says: 'taps to do something' });
  assert.equal(measureFor({ key: 'ux:first' }).goal, 'more');
  assert.deepEqual(uxEffect(ev, 'taps', at(86000), { screens, brief }), { ux: 'taps', before: 6, after: 1, sittingsBefore: 6, sittingsAfter: 6 });
  // The store keeps the taps with the event.
  const written = [];
  await Store.prototype.track.call({ db: { write: async (ops) => written.push(...ops) } }, 'a1', 'insert', '', 'habits', { taps: 4, session: 's1' });
  assert.equal(written[0].doc.taps, 4);
});

// A function with declared callers (ALLOW CALL, access.mjs) on a published
// app over a real chloedb-server (ports 7974 and 7976): the call is checked
// against who may make it, then it runs with the app's rights, so a
// visitor's order lowers a stock no visitor may change; a function with no
// callers runs as the person and is held to the tables' statements.

import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { join } from 'node:path';
import { parseAllow, allowText, mayCall, checkAccess } from '../engine/access.mjs';

test('ALLOW CALL: parsed, said back, checked against a role', () => {
  const r = parseAllow('ALLOW CALL ON take-place TO everyone');
  assert.deepEqual(r, { table: 'take-place', actions: ['call'], who: 'everyone', scope: [], call: true });
  assert.equal(allowText(r), 'ALLOW CALL ON take-place TO everyone');
  assert.doesNotThrow(() => mayCall('everyone', 'take a place', { role: 'visitor', user: null }));
  assert.throws(() => mayCall('members', 'take a place', { role: 'visitor', user: null }), (e) => e.status === 401 && /Sign in first: only the app's signed-in members can take a place/.test(e.message));
  assert.throws(() => mayCall('owner', 'reset', { role: 'member', user: { id: 'p' } }), (e) => e.status === 403);
  assert.deepEqual(checkAccess(['ALLOW CALL ON nope TO everyone'], new Map(), new Set(['take-place'])), ['access: ALLOW CALL ON nope TO everyone names the function nope, which the app does not have']);
});

test('a function with callers runs with the app\'s rights; one without runs as the person', async (t) => {
  const { findServerBinary, startServer } = await import('../engine/strata.mjs');
  const { Store } = await import('../engine/store.mjs');
  const bin = findServerBinary(join(import.meta.dirname, '..', '..'));
  if (!bin) {
    if (process.env.STRATA_APPS_NEED_SERVER === '1') throw new Error('no chloedb-server binary');
    return t.skip('no chloedb-server binary');
  }
  const dir = mkdtempSync('/tmp/sa-definer-');
  const { child: strata, client } = await startServer({ bin, db: join(dir, 'apps.strata'), listen: '127.0.0.1:7974', log: () => {} });
  let engineProc = null;
  try {
    const store = new Store(client, () => {});
    await store.init();
    const app = await store.createApp({ name: 'Places' });
    const tables = [
      { name: 'slots', columns: [{ name: 'name', type: 'text' }, { name: 'spots', type: 'int' }], access: ['ALLOW READ ON slots TO everyone', 'ALLOW WRITE ON slots TO owner'] },
      { name: 'claims', columns: [{ name: 'slot', type: 'keyword' }, { name: 'who', type: 'text' }], access: ['ALLOW ADD ON claims TO everyone', 'ALLOW READ, CHANGE, REMOVE ON claims TO owner'] },
    ];
    const caps = [];
    const have = new Set(await store.tableNames());
    for (const spec of tables) {
      await store.ensureTable(app, spec, have);
      caps.push(await store.putCapability({ app, slug: spec.name, kind: 'table', title: spec.name, spec, code: '' }));
    }
    const steps = JSON.stringify({ steps: [
      { let: 'free', value: 'SELECT spots FROM slots WHERE id = :slot' },
      { require: 'SELECT :free >= 1', else: 'None left' },
      { insert: 'claims', values: { slot: ':slot', who: ':who' } },
      { update: 'slots', id: ':slot', set: { spots: 'SELECT spots - 1 FROM slots WHERE id = :slot' } },
    ] });
    caps.push(await store.putCapability({ app, slug: 'take-place', kind: 'function', title: 'Take a place', spec: { on: null, rule: true, callers: 'everyone' }, code: steps }));
    caps.push(await store.putCapability({ app, slug: 'take-place-as-me', kind: 'function', title: 'Take a place as me', spec: { on: null, rule: true }, code: steps }));
    caps.push(await store.putCapability({ app, slug: 'members-take', kind: 'function', title: 'Take a place for members', spec: { on: null, rule: true, callers: 'members' }, code: steps }));
    await store.writeComposition(app, caps, 'first', 'places');
    await client.write([{ op: 'insert', table: `app_${app}_slots`, doc: { id: 's1', name: 'Morning', spots: 2 } }]);
    await store.putSetting(app, 'publish', JSON.stringify({ on: true, name: 'places', access: 'change' }));

    const TOKEN = 'definer-test-token';
    engineProc = spawn(process.execPath, [join(import.meta.dirname, '..', 'engine', 'server.mjs'), '--port', '7976', '--strata', 'http://127.0.0.1:7974', '--data', join(dir, 'engine')],
      { env: { ...process.env, STRATA_APP_TOKEN: TOKEN, STRATA_APPS_SCHEDULES: 'off', STRATA_APPS_AUTOREPAIR: '0', STRATA_APPS_TILES: 'off', STRATA_APPS_GEOCODER: 'off' }, stdio: ['ignore', 'pipe', 'pipe'] });
    let out = '';
    engineProc.stdout.on('data', (d) => { out += d; });
    engineProc.stderr.on('data', (d) => { out += d; });
    const base = 'http://127.0.0.1:7976';
    for (let i = 0; ; i++) {
      try { if ((await fetch(`${base}/healthz`)).ok) break; } catch { /* starting */ }
      if (i > 600 || engineProc.exitCode !== null) throw new Error(`the engine did not start:\n${out.slice(-2000)}`);
      await new Promise((r) => setTimeout(r, 100));
    }
    const call = async (path, body, { cookie, studio } = {}) => {
      const headers = { 'content-type': 'application/json', origin: base, ...(cookie ? { cookie } : {}), ...(studio ? { authorization: `Bearer ${TOKEN}` } : {}) };
      const res = await fetch(`${base}/api/apps/${app}${path}`, { method: body === undefined ? 'GET' : 'POST', headers, body: body === undefined ? undefined : JSON.stringify(body) });
      return { status: res.status, body: await res.json().catch(() => ({})), cookie: (res.headers.get('set-cookie') || '').split(';')[0] };
    };
    const left = async () => (await call('/sql', { sql: "SELECT spots FROM slots WHERE id = 's1'" }, { studio: true })).body.rows[0].spots;

    // A visitor may not change the slots themselves...
    assert.equal((await call('/update', { table: 'slots', id: 's1', fields: { spots: 9 } })).status, 401);
    // ...but the function that declares everyone may call it does it for them.
    const ok = await call('/call', { name: 'take-place', input: { slot: 's1', who: 'Ana' } });
    assert.equal(ok.status, 200, JSON.stringify(ok.body));
    assert.equal(await left(), 1);
    // The same steps with no callers declared run as the visitor: refused.
    const asMe = await call('/call', { name: 'take-place-as-me', input: { slot: 's1', who: 'Ben' } });
    assert.equal(asMe.status, 401, JSON.stringify(asMe.body));
    assert.equal(await left(), 1);
    // A members' function: a visitor is told to sign in; a member's call goes through.
    const v = await call('/call', { name: 'members-take', input: { slot: 's1', who: 'Cy' } });
    assert.equal(v.status, 401);
    assert.match(v.body.error, /Sign in first: only the app's signed-in members can take a place for members/);
    const m = await call('/signup', { email: 'cy@example.com', name: 'Cy', password: 'correct horse' });
    assert.equal((await call('/call', { name: 'members-take', input: { slot: 's1', who: 'Cy' } }, { cookie: m.cookie })).status, 200);
    assert.equal(await left(), 0);
    // A refusal of the function's own (REQUIRE) is said to the person.
    const none = await call('/call', { name: 'take-place', input: { slot: 's1', who: 'Di' } });
    assert.equal(none.status, 409, JSON.stringify(none.body));
    assert.match(none.body.error, /None left/);
    // The visitor still reads none of the claims.
    assert.equal((await call('/sql', { sql: 'SELECT who FROM claims' })).status, 401);
  } finally {
    if (engineProc) { engineProc.kill('SIGTERM'); await new Promise((r) => { if (engineProc.exitCode !== null) r(); else engineProc.on('exit', r); }); }
    strata.kill();
  }
});

// An app's builders (engine/builders.mjs): who works on an app beside its
// owner, with a role. The pure decisions, then a real chloedb-server (ports
// 7943 and 7947): invitations by address, a link taken once, a key held to
// its app and role on every studio route, roles changed, someone removed.

import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { join } from 'node:path';
import { builderRefusal, inviteList, assertRole, shapeBuilder, joinLink } from '../engine/builders.mjs';

test('builders: what each role may do in the studio', () => {
  const viewer = { app: 'abc123', role: 'viewer' };
  const editor = { app: 'abc123', role: 'editor' };
  const admin = { app: 'abc123', role: 'admin' };
  assert.equal(builderRefusal(null, 'POST', '/api/apps/abc123/delete'), null, 'the owner');
  assert.equal(builderRefusal(viewer, 'GET', '/api/apps/abc123'), null);
  assert.equal(builderRefusal(viewer, 'GET', '/api/apps/abc123/builders'), null);
  for (const rest of ['activity', 'outbox', 'payments', 'secrets']) {
    assert.match(builderRefusal(editor, 'GET', `/api/apps/abc123/${rest}`), /owner and admins/, rest);
    assert.equal(builderRefusal(admin, 'GET', `/api/apps/abc123/${rest}`), null, rest);
  }
  assert.equal(builderRefusal(viewer, 'POST', '/api/apps/abc123/sql'), null, 'a query only reads');
  for (const rest of ['requests', 'screens/home', 'designs', 'restore', 'insert', 'publish', 'rename', 'schedules/x/run']) {
    assert.match(builderRefusal(viewer, 'POST', `/api/apps/abc123/${rest}`), /viewer can look/, rest);
    assert.equal(builderRefusal(editor, 'POST', `/api/apps/abc123/${rest}`), null, rest);
  }
  assert.equal(builderRefusal(viewer, 'POST', '/api/apps/abc123/comments'), null, 'a viewer leaves notes');
  assert.equal(builderRefusal(viewer, 'POST', '/api/apps/abc123/comments/c1/reply'), null);
  assert.equal(builderRefusal(viewer, 'POST', '/api/apps/abc123/comments/c1/resolve'), null);
  assert.match(builderRefusal(viewer, 'POST', '/api/apps/abc123/comments/c1/delete'), /viewer can look/);
  assert.match(builderRefusal(editor, 'POST', '/api/apps/abc123/builders'), /owner and admins/);
  assert.match(builderRefusal(editor, 'POST', '/api/apps/abc123/builders/x/remove'), /owner and admins/);
  assert.equal(builderRefusal(admin, 'POST', '/api/apps/abc123/builders'), null);
  assert.match(builderRefusal(admin, 'POST', '/api/apps/abc123/delete'), /only the app's owner/);
  assert.match(builderRefusal(editor, 'GET', '/api/apps/zzz999'), /another app/);
  assert.match(builderRefusal(editor, 'POST', '/api/apps'), /one app/);
  assert.equal(builderRefusal(editor, 'GET', '/api/apps'), null);
  assert.equal(builderRefusal(editor, 'GET', '/'), null);
  assert.match(builderRefusal(editor, 'GET', '/api/graph'), /one app/);
});

test('builders: invite lists, roles, rows and links', () => {
  assert.deepEqual(inviteList('Ana@x.com, bo@y.org;ana@x.com nope'), { valid: ['ana@x.com', 'bo@y.org'], invalid: ['nope'], unprocessed: [] });
  assert.equal(inviteList(Array.from({ length: 52 }, (_, i) => `p${i}@x.com`)).unprocessed.length, 2);
  assert.equal(assertRole('Editor'), 'editor');
  assert.throws(() => assertRole('owner'), /viewer, editor or admin/);
  const row = { id: 'b1', app: 'a', email: 'e@x.com', role: 'viewer', status: 'invited', token_hash: 'h', key_hash: '', expires: '2020-01-01T00:00:00Z', invited: '2019-12-01T00:00:00Z' };
  const shaped = shapeBuilder(row, Date.parse('2021-01-01'));
  assert.equal(shaped.status, 'expired');
  assert.ok(!('token_hash' in shaped) && !('key_hash' in shaped));
  assert.equal(joinLink('http://h:1/', 'tok'), 'http://h:1/join?token=tok');
});

test('builders over a real server: invited, joined once, held to their app and role, removed', async (t) => {
  const { findServerBinary, startServer } = await import('../engine/strata.mjs');
  const { Store } = await import('../engine/store.mjs');
  const bin = findServerBinary(join(import.meta.dirname, '..', '..'));
  if (!bin) {
    if (process.env.STRATA_APPS_NEED_SERVER === '1') throw new Error('no chloedb-server binary');
    return t.skip('no chloedb-server binary');
  }
  const dir = mkdtempSync('/tmp/sa-builders-');
  const { child: strata, client } = await startServer({ bin, db: join(dir, 'apps.strata'), listen: '127.0.0.1:7943', log: () => {} });
  let engineProc = null;
  try {
    const store = new Store(client, () => {});
    await store.init();
    const app = await store.createApp({ name: 'Notes', description: 'Team notes' });
    const other = await store.createApp({ name: 'Other', description: 'Another app' });
    const env = { ...process.env, STRATA_APPS_SCHEDULES: 'off', STRATA_APPS_AUTOREPAIR: '0', STRATA_APPS_TILES: 'off', STRATA_APPS_GEOCODER: 'off' };
    delete env.STRATA_APP_TOKEN;
    delete env.STRATA_APPS_PUBLIC_URL;
    engineProc = spawn(process.execPath, [join(import.meta.dirname, '..', 'engine', 'server.mjs'), '--port', '7947', '--strata', 'http://127.0.0.1:7943', '--data', join(dir, 'engine')],
      { env, stdio: ['ignore', 'pipe', 'pipe'] });
    let out = '';
    engineProc.stdout.on('data', (d) => { out += d; });
    engineProc.stderr.on('data', (d) => { out += d; });
    const base = 'http://127.0.0.1:7947';
    for (let i = 0; ; i++) {
      try { if ((await fetch(`${base}/healthz`)).ok) break; } catch { /* starting */ }
      if (i > 600 || engineProc.exitCode !== null) throw new Error(`the engine did not start:\n${out.slice(-2000)}`);
      await new Promise((r) => setTimeout(r, 100));
    }
    const call = async (path, body, key) => {
      const headers = { 'content-type': 'application/json', origin: base, ...(key ? { 'x-sa-builder': key } : {}) };
      const res = await fetch(`${base}${path}`, { method: body === undefined ? 'GET' : 'POST', headers, body: body === undefined ? undefined : JSON.stringify(body), redirect: 'manual' });
      return { status: res.status, body: await res.json().catch(() => ({})), headers: res.headers };
    };

    // The owner (no key) invites two people, one address twice and one not an address.
    const inv = await call(`/api/apps/${app}/builders`, { emails: ['vic@example.com', 'eve@example.com', 'VIC@example.com', 'nope'], role: 'viewer' });
    assert.equal(inv.status, 200, JSON.stringify(inv.body));
    assert.deepEqual(inv.body.invalid, ['nope']);
    assert.equal(inv.body.results.length, 2);
    const vic = inv.body.results.find((r) => r.email === 'vic@example.com');
    assert.match(vic.link, /^http:\/\/127\.0\.0\.1:7947\/join\?token=[A-Za-z0-9_-]{20,}$/);
    let list = await call(`/api/apps/${app}/builders`);
    assert.deepEqual(list.body.builders.map((b) => [b.email, b.role, b.status]), [['vic@example.com', 'viewer', 'invited'], ['eve@example.com', 'viewer', 'invited']]);
    assert.equal(list.body.me.role, 'owner');
    assert.ok(!JSON.stringify(list.body).includes('hash'));

    // Eve becomes an editor before joining; a fresh link replaces her first one.
    const eveId = inv.body.results.find((r) => r.email === 'eve@example.com').id;
    assert.equal((await call(`/api/apps/${app}/builders/${eveId}`, { role: 'editor' })).body.builder.role, 'editor');
    const firstEve = inv.body.results.find((r) => r.email === 'eve@example.com').link;
    const fresh = await call(`/api/apps/${app}/builders/${eveId}/link`, {});
    assert.notEqual(fresh.body.link, firstEve);
    const tokenOf = (link) => new URL(link).searchParams.get('token');
    assert.equal((await call('/api/builders/join', { token: tokenOf(firstEve) })).status, 404, 'the replaced link');

    // Vic opens his link in a browser: a cookie, then the app in the studio; the link works once.
    const opened = await fetch(`${base}/join?token=${tokenOf(vic.link)}`, { redirect: 'manual' });
    assert.equal(opened.status, 302);
    assert.equal(opened.headers.get('location'), `/#/app/${app}`);
    const vicCookie = opened.headers.get('set-cookie').split(';')[0];
    assert.match(vicCookie, /^sa_builder=/);
    assert.equal((await fetch(`${base}/join?token=${tokenOf(vic.link)}`, { redirect: 'manual' })).status, 404);
    const vicKey = vicCookie.split('=')[1];

    // Eve takes hers through the API.
    const joined = await call('/api/builders/join', { token: tokenOf(fresh.body.link), name: 'Eve' });
    assert.equal(joined.status, 200, JSON.stringify(joined.body));
    assert.equal(joined.body.app, app);
    assert.equal(joined.body.role, 'editor');
    const eveKey = joined.body.key;

    // A viewer reads, and changes nothing; the cookie works as the header does.
    const asCookie = await fetch(`${base}/api/apps/${app}`, { headers: { cookie: vicCookie } });
    assert.equal(asCookie.status, 200);
    assert.equal((await call(`/api/apps/${app}/sql`, { sql: 'SELECT 1 AS one' }, vicKey)).status !== 403, true, 'a query is a read');
    const refused = await call(`/api/apps/${app}/rename`, { name: 'Mine' }, vicKey);
    assert.equal(refused.status, 403);
    assert.match(refused.body.error, /viewer can look/);
    assert.equal((await call(`/api/apps/${app}/requests`, { text: 'add a page' }, vicKey)).status, 403);
    assert.equal((await call(`/api/apps/${app}/builders`, { emails: ['x@example.com'], role: 'admin' }, vicKey)).status, 403);

    // An editor changes the app but not who builds it, nor deletes it, nor reaches another app.
    const renamed = await call(`/api/apps/${app}/rename`, { name: 'Team notes' }, eveKey);
    assert.equal(renamed.status, 200, JSON.stringify(renamed.body));
    assert.equal((await call(`/api/apps/${app}/builders`, { emails: ['x@example.com'], role: 'editor' }, eveKey)).status, 403);
    assert.equal((await call(`/api/apps/${app}/delete`, {}, eveKey)).status, 403);
    assert.equal((await call(`/api/apps/${other}`, undefined, eveKey)).status, 403);
    assert.equal((await call('/api/apps', { description: 'a new app' }, eveKey)).status, 403);
    const listed = await call('/api/apps', undefined, eveKey);
    assert.deepEqual(listed.body.apps.map((a) => a.id), [app], 'a builder sees their app only');
    assert.equal((await call(`/api/apps/${app}/builders`, undefined, eveKey)).body.me.role, 'editor');

    // The owner makes Vic an admin: now he may invite.
    const vicId = vic.id;
    await call(`/api/apps/${app}/builders/${vicId}`, { role: 'admin' });
    const byAdmin = await call(`/api/apps/${app}/builders`, { emails: ['zed@example.com'], role: 'viewer' }, vicKey);
    assert.equal(byAdmin.status, 200, JSON.stringify(byAdmin.body));
    assert.equal((await call(`/api/apps/${app}/builders/${vicId}`, { role: 'viewer' }, vicKey)).status, 400, 'not their own role');

    // Removed: Eve's key stops working at once; a pending invitation is withdrawn the same way.
    assert.equal((await call(`/api/apps/${app}/builders/${eveId}/remove`, {})).status, 200);
    const gone = await call(`/api/apps/${app}`, undefined, eveKey);
    assert.equal(gone.status, 401);
    const zedId = byAdmin.body.results[0].id;
    assert.equal((await call(`/api/apps/${app}/builders/${zedId}/remove`, {})).status, 200);
    list = await call(`/api/apps/${app}/builders`);
    assert.deepEqual(list.body.builders.map((b) => [b.email, b.role, b.status]), [['vic@example.com', 'admin', 'joined']]);
    assert.equal((await call(`/api/apps/${other}/builders/${vicId}/remove`, {})).status, 404, 'a builder of another app');

    // Notes on the app's screens: the owner names themselves, a builder is named by their key.
    const note = await call(`/api/apps/${app}/comments`, { body: 'The headline is too long', authorName: 'Ana', anchor: { route: '/home', selector: 'h1', textSnippet: 'Welcome', nx: 2, junk: 'x' } });
    assert.equal(note.status, 200, JSON.stringify(note.body));
    assert.deepEqual([note.body.comment.author.name, note.body.comment.route, note.body.comment.nx, note.body.comment.junk], ['Ana', '/home', 1, undefined]);
    const reply = await call(`/api/apps/${app}/comments/${note.body.comment.id}/reply`, { body: 'Shorter now' }, vicKey);
    assert.equal(reply.body.comment.author.name, 'vic@example.com');
    let notes = await call(`/api/apps/${app}/comments?status=open`, undefined, vicKey);
    assert.deepEqual(notes.body.comments.map((c) => [c.body, c.parentId === null]), [['The headline is too long', true], ['Shorter now', false]]);
    await call(`/api/apps/${app}/comments/${note.body.comment.id}/resolve`, {});
    assert.equal((await call(`/api/apps/${app}/comments?status=open`)).body.comments.length, 0, 'a resolved note takes its replies with it');
    assert.equal((await call(`/api/apps/${app}/comments/${note.body.comment.id}/delete`, {})).body.replies, 1);
    assert.equal((await call(`/api/apps/${app}/comments`, { body: '  ' })).status, 400);

    // The studio sees every company of the app with who works in it.
    const signed = await fetch(`${base}/api/apps/${app}/signup`, { method: 'POST', headers: { 'content-type': 'application/json', origin: base }, body: JSON.stringify({ email: 'olive@example.com', name: 'Olive', password: 'correct horse' }) });
    assert.equal(signed.status, 200);
    const olive = signed.headers.get('set-cookie').split(';')[0];
    const made = await fetch(`${base}/api/apps/${app}/companies`, { method: 'POST', headers: { 'content-type': 'application/json', origin: base, cookie: olive }, body: JSON.stringify({ name: 'Acme' }) });
    assert.equal(made.status, 200, await made.clone().text());
    const all = await call(`/api/apps/${app}/companies?all=1`);
    assert.deepEqual(all.body.companies.map((c) => [c.name, c.members.map((m) => m.role)]), [['Acme', ['admin']]]);

    // Deleting the app removes its builders too.
    assert.equal((await call(`/api/apps/${app}/delete`, {})).status, 200);
    assert.equal((await call(`/api/apps/${other}`, undefined, vicKey)).status, 401);
  } finally {
    engineProc?.kill('SIGTERM');
    strata.kill('SIGTERM');
  }
});

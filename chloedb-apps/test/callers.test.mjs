// The engine's callers through chloedb-server (callers.mjs): a key limited to
// tables reads, uses and changes an app by its tables (app_<id>_*), builds
// one only with admin on every app's (app_*), and never gets the studio's
// login; a key of every table keeps read to look and admin to change.

import test from 'node:test';
import assert from 'node:assert/strict';
import { callerOf, refusal, sees } from '../engine/callers.mjs';

const req = (h) => ({ headers: Object.fromEntries(Object.entries(h).map(([k, v]) => [k.toLowerCase(), v])) });
const may = (c, method, path) => refusal(c, method, new URL(path, 'http://x'));

test('an app owner key builds and changes apps by their tables', () => {
  const owner = callerOf(req({ 'x-chloedb-principal': 'groomer', 'x-chloedb-scope': 'admin', 'x-chloedb-tables': 'app_*=admin' }));
  assert.equal(may(owner, 'POST', '/api/apps'), null, 'builds a new app');
  assert.equal(may(owner, 'POST', '/api/apps/btk06c/publish'), null);
  assert.equal(may(owner, 'POST', '/api/apps/btk06c/requests'), null);
  assert.equal(may(owner, 'GET', '/api/apps/btk06c'), null);
  assert.match(may(owner, 'POST', '/api/handoff'), /not for a key limited to tables/, 'the studio login is everything');
  assert.match(may(owner, 'GET', '/api/graph'), /not for a key limited to tables/);
  assert.equal(sees(owner, 'btk06c'), true);
});

test('a key of one app uses it, and changes it only as its admin', () => {
  const one = callerOf(req({ 'x-chloedb-principal': 'shop', 'x-chloedb-scope': 'write', 'x-chloedb-tables': 'app_btk06c_*=write' }));
  assert.equal(may(one, 'POST', '/api/apps/btk06c/insert'), null);
  assert.equal(may(one, 'GET', '/api/apps/btk06c'), null);
  assert.match(may(one, 'POST', '/api/apps/btk06c/publish'), /admin scope on the app's tables/);
  assert.match(may(one, 'GET', '/api/apps/other1'), /read scope/);
  assert.match(may(one, 'POST', '/api/apps'), /every app's tables/);
  assert.match(may(one, 'POST', '/api/apps/btk06c/duplicate'), /admin/);
  assert.equal(may(one, 'GET', '/api/events?app=btk06c'), null);
  assert.match(may(one, 'GET', '/api/events'), /one app's events/);
  assert.equal(sees(one, 'btk06c'), true);
  assert.equal(sees(one, 'other1'), false);
  // An exact table is not the app's every table.
  const exact = callerOf(req({ 'x-chloedb-scope': 'write', 'x-chloedb-tables': 'app_btk06c_bookings=write' }));
  assert.notEqual(may(exact, 'GET', '/api/apps/btk06c'), null);
});

test('a key of every table keeps read to look and admin to change', () => {
  const reader = callerOf(req({ 'x-chloedb-principal': 'r', 'x-chloedb-scope': 'read' }));
  const writer = callerOf(req({ 'x-chloedb-principal': 'w', 'x-chloedb-scope': 'write' }));
  const admin = callerOf(req({ 'x-chloedb-principal': 'token', 'x-chloedb-scope': 'admin' }));
  assert.equal(may(reader, 'GET', '/api/graph'), null);
  assert.match(may(writer, 'POST', '/api/apps/btk06c/publish'), /admin scope/);
  assert.equal(may(admin, 'POST', '/api/handoff'), null);
  // Not through the server: no caller, judged as before.
  assert.equal(callerOf(req({})), null);
  assert.equal(may(null, 'POST', '/api/apps'), null);
});

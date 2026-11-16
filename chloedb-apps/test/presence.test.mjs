// Who is working on an app now (engine/presence.mjs): heartbeats, a short
// time to live, leaving, and change detection for the event stream.
import test from 'node:test';
import assert from 'node:assert/strict';
import { Presence } from '../engine/presence.mjs';

test('presence: here while beating, gone after the time to live or on leaving', () => {
  let t = 1000;
  const p = new Presence({ ttl: 30000, now: () => t });
  assert.equal(p.beat('a1', { id: 'owner', name: 'Ana', role: 'owner', screen: 'home' }), true, 'arriving is a change');
  assert.equal(p.beat('a1', { id: 'b1', name: 'Bea', role: 'editor', screen: 'home' }), true);
  t += 10000;
  assert.equal(p.beat('a1', { id: 'b1', name: 'Bea', role: 'editor', screen: 'home' }), false, 'a beat that changes nothing');
  assert.deepEqual(p.here('a1').map((x) => [x.id, x.role]), [['owner', 'owner'], ['b1', 'editor']]);
  assert.equal(p.beat('a1', { id: 'b1', name: 'Bea', role: 'editor', screen: 'books' }), true, 'another screen is a change');
  t += 25000;
  assert.deepEqual(p.here('a1').map((x) => x.id), ['b1'], 'the owner stopped beating 35 s ago');
  assert.equal(p.leave('a1', 'b1'), true);
  assert.deepEqual(p.here('a1'), []);
  assert.deepEqual(p.here('nobody'), []);
});

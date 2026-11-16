// A design's tokens changed by hand (engine/designs.mjs tuneDesign) and the
// colors offered for a token (tokenChoices).
import test from 'node:test';
import assert from 'node:assert/strict';
import { tuneDesign, tokenChoices } from '../engine/designs.mjs';
import { checkDesign, PRESETS } from '../web/design.js';

test('tuneDesign: a token changed, the rest kept, text kept readable, nonsense refused', () => {
  const d = checkDesign(PRESETS[0]);
  const t = tuneDesign(d, { colors: { accent: '#B4237A' }, corners: 'round' });
  assert.equal(t.light.accent, '#b4237a');
  assert.equal(t.corners, 'round');
  assert.equal(t.name, d.name);
  assert.deepEqual(t.dark, d.dark, 'the other scheme is untouched');
  const dark = tuneDesign(d, { scheme: 'dark', colors: { bg: '#101820' } });
  assert.equal(dark.dark.bg, '#101820');
  assert.equal(dark.light.bg, d.light.bg);
  const pale = tuneDesign(d, { colors: { fg: '#f0f0f0' } });
  assert.notEqual(pale.light.fg, '#f0f0f0', 'text too pale on its page is darkened to be read');
  assert.throws(() => tuneDesign(d, { colors: { shadow: '#000000' } }), /accent, bg, card, fg/);
  assert.throws(() => tuneDesign(d, { colors: { accent: 'red' } }), /like #2f5bd3/);
  assert.throws(() => tuneDesign(d, { corners: 'wavy' }), /corners is one of/);
  assert.throws(() => tuneDesign(d, { margin: '4px' }), /not a design token/);
  assert.throws(() => tuneDesign(d, { scheme: 'sepia' }), /light or dark/);
});

test('tokenChoices: the one in use first, then the other designs and the presets, no repeats', () => {
  const designs = [checkDesign(PRESETS[1]), checkDesign(PRESETS[2]), checkDesign(PRESETS[1])];
  const c = tokenChoices(designs, 1);
  assert.equal(c.accent[0], designs[1].light.accent);
  assert.equal(new Set(c.accent).size, c.accent.length);
  assert.ok(c.accent.length > 3 && c.accent.length <= 12);
  for (const k of ['bg', 'card', 'fg']) assert.ok(c[k].every((v) => /^#[0-9a-f]{6}$/.test(v)), k);
});

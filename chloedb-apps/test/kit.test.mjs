// The kit's pure parts: how a table's numbers read (web/screen.js
// window.__sa_numbers), the type pairings and the native palette
// (web/design.js), and the frame's font policy (web/policy.js).
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import { TYPES, typeOf, checkDesign, designVars, contrast, PRESETS } from '../web/design.js';
import { frameCsp } from '../web/policy.js';
import { shapeDesigns } from '../engine/designs.mjs';

const screenSrc = readFileSync(new URL('../web/screen.js', import.meta.url), 'utf8');
const numbers = () => { const window = {}; vm.runInNewContext(screenSrc, { window }); return window.__sa_numbers; };

test('numbers: figures read from data and from what a column shows', () => {
  const { parseNum } = numbers();
  assert.equal(parseNum(1200), 1200);
  assert.equal(parseNum('1,200'), 1200);
  assert.equal(parseNum('$42,150.00'), 42150);
  assert.equal(parseNum('-$2,350.00'), -2350);
  assert.equal(parseNum('+5.7%'), 5.7);
  assert.equal(parseNum('(300)'), -300);
  assert.equal(parseNum('−42 €'), -42);
  assert.equal(parseNum('January 2026'), null);
  assert.equal(parseNum('1-30 days'), null);
  assert.equal(parseNum(''), null);
});

test('numbers: which columns are figures, changes and amounts that add up', () => {
  const { kinds, tone, barColumn, totals } = numbers();
  const cols = [{ key: 'month', label: 'Month' }, { key: 'revenue', label: 'Revenue', format: 'money' }, { key: 'change', label: 'Change' }, { key: 'pct', label: 'Change %' }, { key: 'price', label: 'Unit price', format: 'money' }, { key: 'year', label: 'Year' }];
  const values = [['Jan', 'Feb', 'Mar'], [42150, 39800, 45200], ['', '-$2,350.00', '+$5,400.00'], ['', '-5.6%', '+13.6%'], [10, 12, 9], [2026, 2026, 2026]];
  const ks = kinds(cols, values);
  assert.deepEqual([...ks].map((k) => k.numeric), [false, true, true, true, true, false], 'a year is no figure');
  assert.deepEqual([...ks].map((k) => k.change), [false, false, true, true, false, false]);
  assert.deepEqual([...ks].map((k) => k.adds), [false, true, false, false, false, false], 'a price does not add up, a change is not totalled');
  assert.equal(tone(ks[2], '-$2,350.00'), 'down');
  assert.equal(tone(ks[2], '+$5,400.00'), 'up');
  assert.equal(tone(ks[1], 42150), '', 'an amount is only marked below zero');
  assert.equal(tone(ks[1], -10), 'down');
  assert.equal(barColumn(ks, 3), 2, 'the change gets the bar');
  assert.equal(barColumn(ks, 2), -1, 'too few rows for bars');
  assert.deepEqual([...totals(cols, ks, values, undefined)], [null, 127150, null, null, null, null]);
  assert.equal(totals(cols, ks, values, false), null, 'the screen can say no total');
  assert.deepEqual([...totals(cols, ks, values, ['price'])], [null, null, null, null, 31, null], 'or name the columns');
  assert.equal(totals(cols, ks, values.map((v) => v.slice(0, 2)), undefined), null, 'two rows are not added up');
  const plain = kinds([{ key: 'name' }, { key: 'stock', label: 'In stock' }], [['Fig', 'Basil'], [3, 0]]);
  assert.equal(totals([{ key: 'name' }, { key: 'stock' }], plain, [['Fig', 'Basil', 'Mint'], [3, 0, 2]], undefined), null, 'counts of things are no money total');
});

test('numbers: a short figure for a chart and a month for a phone row', () => {
  const { short, monthOf } = numbers();
  assert.equal(short(42150), '42.2K');
  assert.equal(short(1250000), '1.3M');
  assert.equal(short(980), '980');
  assert.deepEqual({ ...monthOf('January 2026') }, { mon: 'jan', year: '2026' });
  assert.deepEqual({ ...monthOf('2026-03') }, { mon: 'mar', year: '2026' });
  assert.equal(monthOf('1-30 days'), null);
  assert.equal(monthOf('Bramble & Co'), null);
});

test('type pairings: each design gets one that fits its titles and its subject', () => {
  for (const [name, t] of Object.entries(TYPES)) assert.ok(t.title && t.weight, `${name} has a title face and weight`);
  assert.equal(typeOf({ titles: 'serif' }, 'A board report of revenue variance'), 'ledger');
  assert.equal(typeOf({ titles: 'serif' }, 'Peluquería Lucía, a hair salon'), 'classic');
  assert.equal(typeOf({ titles: 'serif' }, 'An architecture studio portfolio'), 'elegant');
  assert.equal(typeOf({ titles: 'system' }, 'Riverside Runners club races'), 'bricolage');
  assert.equal(typeOf({ titles: 'system' }, 'A sales dashboard'), 'grotesk');
  assert.equal(typeOf({ titles: 'system' }, 'My reading list'), 'system');
  assert.equal(typeOf({ titles: 'serif', type: 'elegant' }, 'a ledger'), 'elegant', 'a fitting choice is kept');
  assert.equal(typeOf({ titles: 'system', type: 'ledger' }, ''), 'system', 'a pairing of another family is not');
  const d = checkDesign({ titles: 'serif', type: 'classic' });
  const v = designVars(d, 'light');
  assert.match(v['--font-title'], /Playfair Display/);
  assert.equal(v['--title-weight'], '650');
  // The first design takes the app's subject.
  const designs = [checkDesign({ look: 'native', titles: 'serif' }), checkDesign({ look: 'bold' }), checkDesign({ look: 'plain' })];
  shapeDesigns(designs, { art: { type: 'serif', concept: 'a quiet ledger for the board' } }, { name: 'Northwind', description: 'revenue variance and aging' });
  assert.equal(designs[0].type, 'ledger');
  // and a serif design is set on paper: a cool grey page turns warm white
  const grey = [checkDesign({ look: 'native', titles: 'serif', light: { bg: '#f5f6f8', card: '#ffffff', fg: '#111111', accent: '#1d4e89' } }), checkDesign({ look: 'bold' }), checkDesign({ look: 'plain' })];
  shapeDesigns(grey, { art: { type: 'serif' } }, { name: 'Ledger', description: 'board report' });
  assert.equal(grey[0].light.bg, '#f8f6f1');
  assert.equal(grey[0].light.card, '#ffffff');
});

test('the native look keeps the design\'s own page and cards, readable', () => {
  const warm = checkDesign({ look: 'native', light: { bg: '#faf7f2', card: '#ffffff', fg: '#1f1a14', accent: '#a0461b' } });
  const v = designVars(warm, 'light');
  assert.equal(v['--bg'], '#faf7f2', 'a warm paper page stays warm');
  assert.equal(v['--card'], '#ffffff');
  for (const p of PRESETS) {
    for (const which of ['light', 'dark']) {
      const n = designVars({ ...p, look: 'native' }, which);
      assert.ok(contrast(n['--fg'], n['--card']) >= 7, `${p.name} ${which}: text reads on cards`);
      assert.ok(contrast(n['--muted'], n['--bg']) >= 4.5, `${p.name} ${which}: secondary text reads`);
      assert.match(n['--fill-solid'], /^#[0-9a-f]{6}$/, 'a solid fill for the chrome around the app');
    }
  }
});

test('a frame may load the kit\'s typefaces from the engine and nothing else', () => {
  const csp = frameCsp('http://127.0.0.1:7860');
  assert.match(csp, /font-src data: http:\/\/127\.0\.0\.1:7860\/fonts\/;/);
  const kit = readFileSync(new URL('../web/kit.css', import.meta.url), 'utf8');
  for (const face of ['Fraunces', 'Playfair Display', 'Instrument Serif', 'Space Grotesk', 'Bricolage Grotesque', 'Nunito', 'JetBrains Mono']) {
    assert.match(kit, new RegExp(`font-family: "${face}"; src: url\\("/fonts/[a-z-]+\\.woff2"\\)`), `${face} is declared`);
  }
  for (const f of kit.match(/\/fonts\/[a-z-]+\.woff2/g)) assert.ok(readFileSync(new URL(`../web${f}`, import.meta.url)).length > 10000, `${f} ships`);
});

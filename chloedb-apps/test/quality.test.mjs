// The checks that are numbers (engine/quality.mjs), the drawn-page rules
// that use them (engine/look.mjs), and the LAYOUT clause of CREATE SCREEN.
import test from 'node:test';
import assert from 'node:assert/strict';
import { Q, QUALITY_SRC, parseLayout, LAYOUT_NAMES, labelProblems, designProblems, mainAction, staticChecks, screenLabels } from '../engine/quality.mjs';
import { rules, repairRequest, checkStatic, namesOf } from '../engine/look.mjs';
import { compileStatement, screenStatement, STATEMENT_DOC } from '../engine/statement.mjs';
import { cleanArt, SYSTEM, planWatcher, tablesWatcher, kpiParts, signInFirst, takeExamplesInto } from '../engine/forge.mjs';
import { shapeDesigns, designPrompt } from '../engine/designs.mjs';
import { checkDesign } from '../web/design.js';

const near = (a, b, d = 0.01) => assert.ok(Math.abs(a - b) < d, `${a} is not near ${b}`);

test('contrast: the WCAG ratio, with translucent text over its background', () => {
  near(Q.contrastRatio('#000000', '#ffffff'), 21);
  near(Q.contrastRatio('#ffffff', '#ffffff'), 1);
  near(Q.contrastRatio('rgb(118, 118, 118)', '#fff'), 4.54);
  // White on a light blue: the "Start learning" button that did not read.
  assert.ok(Q.contrastRatio('#ffffff', '#7aa2ff') < 3);
  // Half-transparent black on white is a mid grey.
  near(Q.contrastRatio('rgba(0, 0, 0, 0.5)', '#ffffff'), 3.98);
  near(Q.contrastRatio('#0008', '#fff'), Q.contrastRatio('rgba(0,0,0,0.533)', '#fff'), 0.02);
  assert.equal(Q.contrastRatio('nonsense', '#fff'), null);
  // Large text needs 3:1, other text 4.5:1.
  assert.equal(Q.neededContrast(16, 400), 4.5);
  assert.equal(Q.neededContrast(24, 400), 3);
  assert.equal(Q.neededContrast(19, 700), 3);
  assert.equal(Q.neededContrast(19, 600), 4.5);
});

test('contrast: the colors of a gradient, never of a picture', () => {
  assert.deepEqual(Q.gradientColors('linear-gradient(125deg, rgb(47, 91, 211), rgba(0, 0, 0, 0.5) 55%, #ffffff)'), ['rgb(47, 91, 211)', 'rgba(0, 0, 0, 0.5)', '#ffffff']);
  assert.deepEqual(Q.gradientColors('url("a.jpg")'), []);
  assert.deepEqual(Q.gradientColors('linear-gradient(red, blue), url(a.jpg)'), []);
  assert.deepEqual(Q.gradientColors('none'), []);
});

test('target size: 44 by 44 px, half a pixel of rounding allowed', () => {
  assert.equal(Q.smallTarget(44, 44), false);
  assert.equal(Q.smallTarget(43.6, 120), false);
  assert.equal(Q.smallTarget(120, 32), true);
  assert.equal(Q.smallTarget(40, 44), true);
  assert.equal(Q.smallTarget(30, 30, 24), false);
});

test('labels: raw column names, placeholders and a generic word beside a number', () => {
  assert.match(Q.rawLabel('Groups 4'), /generic word/);
  assert.match(Q.rawLabel('Items: 12'), /generic word/);
  assert.match(Q.rawLabel('Groups', { value: '4' }), /generic word/);
  assert.match(Q.rawLabel('lesson_count'), /snake_case/);
  assert.match(Q.rawLabel('quizScore'), /camelCase/);
  assert.match(Q.rawLabel('Item 1'), /placeholder/);
  assert.match(Q.rawLabel('Lorem'), /placeholder/);
  assert.match(Q.rawLabel('enrollments', { names: ['enrollments'] }), /table's name/);
  assert.match(Q.rawLabel('Enrollments 3', { names: ['enrollment'] }), /generic word/);
  // Words for people pass.
  for (const ok of ['Lessons done', 'Groups meeting this week', 'Coming', 'Owed', 'Start lesson 1', 'Carnitas', 'RSVP', '']) assert.equal(Q.rawLabel(ok, { value: 3 }), null, ok);
  assert.equal(Q.rawLabel('Groups'), null); // a heading, no number beside it
});

test('the kit runs in a page as it does here', () => {
  const g = {};
  new Function('window', QUALITY_SRC)(g);
  near(g.__q.contrastRatio('#000', '#fff'), 21);
  assert.equal(g.__q.smallTarget(10, 10), true);
  assert.match(g.__q.rawLabel('Groups 4'), /generic/);
});

test('a screen\'s labels and its main action, read without drawing it', () => {
  const screen = { body: [
    { h: 'Your groups' },
    { kpis: [{ label: 'Groups', of: 'n' }, { label: 'Meeting this week', value: '=3' }] },
    { table: '=rows', columns: [{ key: 'member_count', label: 'member_count' }, { key: 'name', label: 'Group' }] },
    { form: 'groups', fields: [{ name: 'name' }], submit: 'Add group' },
  ] };
  const labels = screenLabels(screen).map((l) => l.text);
  assert.deepEqual(labels, ['Your groups', 'Groups', 'Meeting this week', 'member_count', 'Group']);
  const p = labelProblems(screen, ['groups', 'member_count']);
  assert.equal(p.length, 2);
  assert.match(p[0], /big number label "Groups"/);
  assert.match(p[1], /column label "member_count"/);
  assert.deepEqual(mainAction(screen), { kind: 'form', label: 'Add group' });
  assert.deepEqual(mainAction({ body: [{ nav: 'X' }, { hero: 'Tacos', actions: [{ label: 'See the menu', open: 'menu' }] }] }), { kind: 'hero', label: 'See the menu' });
  assert.equal(mainAction({ body: [{ h: 'Hi' }, 'Words'] }), null);
});

test('a design\'s own colors are measured: the words on its buttons read', () => {
  assert.deepEqual(designProblems(checkDesign({ light: { bg: '#ffffff', card: '#ffffff', fg: '#111111', accent: '#2f5bd3' } })), []);
  // checkDesign makes every pair readable, so a design as stored passes; a
  // raw design that skipped it (a hand-made one) is caught.
  assert.ok(designProblems(checkDesign({ light: { bg: '#ffffff', card: '#ffffff', fg: '#bbbbbb', accent: '#9ec5ff' } })).length === 0);
  const found = staticChecks({ screens: [{ slug: 'home', title: 'Home', screen: { body: [{ kpis: [{ label: 'lesson_count', value: 3 }] }] } }], names: [] });
  assert.ok(found.some((f) => f.by === 'static' && f.severity === 'high' && /lesson_count/.test(f.problem)));
  assert.ok(found.some((f) => /no screen has a main action/.test(f.problem) && f.severity === 'low'));
  assert.match(repairRequest(found), /checked without drawing them/);
});

test('the drawn-page rules: button words, the main action below the fold, raw labels', () => {
  const m = { text: 100, controls: 4, width: 390, scroll: 390, screen: 844, front: true, firstTop: null, small: [], smallCount: 0, repeats: [], faint: [{ text: 'Start learning', ratio: 2.1, need: 4.5 }, { text: 'Small print', ratio: 3.9, need: 4.5 }], faintCount: 2, faintButtons: [{ text: 'Start learning', ratio: 2.1, need: 4.5 }], ids: [], tinyFields: 0, numbers: 0, zeros: 0, rowsShown: 0, mainTop: 1200, mainName: 'Order tacos', raw: ['"Groups 4" (one generic word beside a number)'] };
  const out = rules(m, 'phone');
  const said = out.map((r) => r.problem).join('\n');
  assert.match(said, /button words that are hard to read: "Start learning" at 2.1:1/);
  assert.match(said, /text that is hard to read: "Small print"/);
  assert.doesNotMatch(said, /text that is hard to read: "Start learning"/);
  assert.match(said, /main action \("Order tacos"\) starts 1200 px down/);
  assert.match(said, /labels that are not words for people: "Groups 4"/);
  assert.ok(out.every((r) => r.severity === 'high'));
  // On a computer the fold is not a phone's: no main-action rule.
  assert.doesNotMatch(rules(m, 'computer').map((r) => r.problem).join('\n'), /main action/);
  // A small target on a phone.
  assert.match(rules({ ...m, faint: [], faintCount: 0, faintButtons: [], raw: [], mainTop: 300, small: ['Menu (40×40 px)'], smallCount: 1 }, 'phone')[0].problem, /smaller than a finger \(44 px\): Menu/);
});

test('the static fallback reads screens stored as JSON in a view\'s code', () => {
  const view = { tables: [{ spec: { name: 'lessons', columns: [{ name: 'title' }, { name: 'done_count' }] } }],
    views: [{ slug: 'home', title: 'Home', spec: { screen: true }, code: JSON.stringify({ body: [{ kpis: [{ label: 'Lessons', of: 'k' }] }, { form: 'lessons', fields: ['title'], submit: 'Add lesson' }] }) }] };
  assert.deepEqual(namesOf(view), ['lessons', 'title', 'done_count']);
  const found = checkStatic(view);
  assert.equal(found.length, 1);
  assert.match(found[0].problem, /"Lessons"/);
});

test('layouts: parsed from what a statement or a plan says', () => {
  assert.deepEqual(LAYOUT_NAMES, ['hero', 'split', 'editorial', 'dashboard', 'gallery', 'tool', 'report']);
  assert.equal(parseLayout('a monthly briefing'), 'report');
  assert.equal(parseLayout('split'), 'split');
  assert.equal(parseLayout('Editorial, the menu as a chalkboard list'), 'editorial');
  assert.equal(parseLayout('single-tool'), 'tool');
  assert.equal(parseLayout('landing'), 'hero');
  assert.equal(parseLayout('a gallery of the dishes'), 'gallery');
  assert.equal(parseLayout('cards'), 'gallery');
  assert.equal(parseLayout('whatever'), null);
  assert.equal(parseLayout(3), null);
});

test('CREATE SCREEN: LAYOUT and PAGE front LAYOUT, and back to a statement', () => {
  const a = compileStatement("CREATE SCREEN lessons TITLE 'Lessons'\nLAYOUT split\nHEADING 'Lessons'");
  assert.equal(a.screen.layout, 'split');
  const b = compileStatement("CREATE SCREEN home TITLE 'Home'\nPAGE front LAYOUT editorial\nHEADING 'Tacos'");
  assert.equal(b.screen.page, 'front');
  assert.equal(b.screen.layout, 'editorial');
  // An unknown layout is left out, never a failed build.
  assert.equal(compileStatement("CREATE SCREEN x TITLE 'X'\nLAYOUT bento\nHEADING 'X'").screen.layout, undefined);
  assert.equal(compileStatement("CREATE SCREEN x TITLE 'X'\nLAYOUT 'Single-tool'\nHEADING 'X'").screen.layout, 'tool');
  const back = screenStatement('lessons', 'Lessons', a.screen);
  assert.match(back, /^LAYOUT split$/m);
  assert.deepEqual(compileStatement(back).screen, a.screen);
  assert.equal(screenStatement('x', 'X', { body: [], layout: 'bento' }), null);
  assert.match(STATEMENT_DOC, /LAYOUT <hero \| split \| editorial \| dashboard \| gallery \| tool \| report>/);
});

test('the art direction: layout, type, accent, imagery and the one main action', () => {
  const art = cleanArt({ concept: 'A chalkboard menu by the window', mood: 'warm, quick, local', layout: 'Editorial: the menu as a chalkboard list', type: 'serif titles over plain text', accent: 'salsa red', imagery: 'the tacos on the plate', action: 'See the menu' });
  assert.equal(art.structure, 'editorial');
  assert.equal(art.type, 'serif');
  assert.equal(art.action, 'See the menu');
  assert.equal(cleanArt({ concept: 'x' }).structure, '');
  // The forge sees the rules, compactly.
  for (const rule of [/44 by 44 px/, /4\.5:1/, /first phone screen/, /"Groups 4"/, /never a generic one/, /real product itself/, /"layout": the first screen's structure/]) assert.match(SYSTEM, rule);
  // The designs follow it: an editorial page is a reading column.
  const designs = [checkDesign({ look: 'native', width: 'wide' }), checkDesign({ look: 'bold' }), checkDesign({ look: 'plain', nav: 'sidebar' })];
  shapeDesigns(designs, { art });
  assert.equal(designs[0].width, 'narrow');
  assert.equal(designs[0].titles, 'serif');
  assert.match(designPrompt({ name: 'Tacos', description: 'A taco truck' }, null, { art }), /main action: See the menu/);
});

test('the plan is handed on once its </plan> has streamed in, before the statements after it', () => {
  const got = [];
  const w = planWatcher((p) => got.push(p));
  w({}, 'Some words <plan>{"name": "Shop", "art": {"concept": "a qu');
  assert.equal(got.length, 0);
  w({}, 'Some words <plan>{"name": "Shop", "art": {"concept": "a quiet shelf"}}</plan>\n<code slug="home">CREATE SCR');
  w({}, 'Some words <plan>{"name": "Shop", "art": {"concept": "a quiet shelf"}}</plan>\n<code slug="home">CREATE SCREEN home</code>');
  assert.equal(got.length, 1);
  assert.equal(got[0].art.concept, 'a quiet shelf');
  // A plan that does not parse is not handed on, and a missing hook is fine.
  const bad = [];
  planWatcher((p) => bad.push(p))({}, '<plan>{"name": </plan>');
  assert.equal(bad.length, 0);
  planWatcher(null)({}, '<plan>{}</plan>');
});

test('the plan up to its tables is handed on once that list has streamed in, for the example rows to be written beside the rest', () => {
  const got = [];
  const w = tablesWatcher((p) => got.push(p));
  const head = '<plan>\n{"name": "Kiln", "ux": {"job": "book a [class] {soon}"}, "tables": [{"name": "pieces", "columns": [{"name": "title"}], "examples": "5 pieces, one \\"sold out\\""}';
  w({}, head);
  assert.equal(got.length, 0);
  w({}, `${head}, {"name": "classes"}], "views": [{"slug": "ho`);
  w({}, `${head}, {"name": "classes"}], "views": [{"slug": "home"}]}</plan>`);
  assert.equal(got.length, 1);
  assert.deepEqual(got[0].tables.map((t) => t.name), ['pieces', 'classes']);
  assert.equal(got[0].ux.job, 'book a [class] {soon}');
  assert.equal(got[0].views, undefined);
  // no tables, no rows to write
  const none = [];
  tablesWatcher((p) => none.push(p))({}, '<plan>{"name": "Journal", "tables": [], "views": []}</plan>');
  assert.equal(none.length, 0);
});


test('the tests writer\'s answer never takes away the example rows their own writer wrote, whichever was done first', () => {
  const rows = { pieces: [{ title: 'Bowl' }] };
  // the rows' writer was done first: the tests come after and the rows stay
  const plan = { seed: rows };
  takeExamplesInto(plan, '{"tests": [{"name": "a piece keeps its title"}]}', { seeded: true });
  assert.deepEqual(plan.seed, rows);
  assert.equal(plan.tests.length, 1);
  // a seed the tests writer gave anyway does not replace the rows' writer's
  takeExamplesInto(plan, '{"seed": {"pieces": []}, "tests": []}', { seeded: true });
  assert.deepEqual(plan.seed, rows);
  // one writer for both: its answer is the rows and the tests
  const one = { seed: rows };
  takeExamplesInto(one, '```json\n{"seed": {"pieces": [{"title": "Cup"}]}, "tests": []}\n```');
  assert.equal(one.seed.pieces[0].title, 'Cup');
  takeExamplesInto(one, '{"tests": []}');
  assert.equal(one.seed, undefined);
  // not JSON: nothing changes
  takeExamplesInto(plan, 'no JSON here', { seeded: true });
  assert.deepEqual(plan.seed, rows);
});

test('big numbers where one is the other with more conditions are found, for the figure check to ask about', () => {
  const screen = { data: { a: "SELECT SUM(x) AS v FROM t WHERE kind != 'z' AND d > 0", b: "SELECT SUM(x) AS v FROM t WHERE kind != 'z'", c: 'SELECT COUNT(*) AS v FROM u' },
    body: [{ kpis: [{ label: 'Narrow', of: 'a' }, { label: 'Wide', of: 'b' }, { label: 'Other', of: 'c' }] }] };
  assert.deepEqual(kpiParts(screen).map((x) => [x.part.label, x.whole.label]), [['Narrow', 'Wide']]);
  assert.deepEqual(kpiParts({ data: { a: 'SELECT 1 AS v FROM t WHERE p OR q', b: 'SELECT 1 AS v FROM t' }, body: [{ kpis: [{ label: 'A', of: 'a' }, { label: 'B', of: 'b' }] }] }), [], 'an OR is no part');
});

test('a screen that draws a sign-in form before its main part is found', () => {
  assert.equal(signInFirst({ body: [{ h: 'Notes' }, { if: '=!me', then: [{ ui: 'signIn' }] }, { table: '=rows' }] }), true);
  assert.equal(signInFirst({ body: [{ hero: 'Hi' }, { table: '=rows' }, { band: [{ ui: 'signIn' }] }] }), false);
  assert.equal(signInFirst({ body: [{ h: 'Rows' }, { table: '=rows' }] }), false);
  assert.equal(signInFirst({ body: [{ code: 'async function render(root) { root.append(ui.signIn({})); }' }, { carousel: '=rows' }] }), true);
});

test('three big numbers that narrow one another (over 90 days, past due, unpaid) are found as parts of the widest', () => {
  const w = "coalesce(lower(status), '') != 'paid'";
  const screen = { data: { k0: `SELECT SUM(amount) AS v FROM r WHERE ${w} AND days_between(due, :today) > 90`, k1: `SELECT SUM(amount) AS v FROM r WHERE ${w} AND days_between(due, :today) > 0`, k2: `SELECT SUM(amount) AS v FROM r WHERE ${w}` },
    body: [{ kpis: [{ label: 'Over 90', of: 'k0' }, { label: 'Past due', of: 'k1' }, { label: 'Unpaid', of: 'k2' }] }] };
  assert.deepEqual(kpiParts(screen).map((x) => [x.part.label, x.whole.label]), [['Over 90', 'Unpaid'], ['Past due', 'Unpaid']]);
});

test('a screen its readers use that reads a table only the owner may read is a problem; the owner\'s own screens are not', async () => {
  const { readerProblems } = await import('../engine/forge.mjs');
  const plan = { access: ['ALLOW READ ON lessons TO members', 'ALLOW WRITE ON lessons TO owner', 'ALLOW ALL ON questions TO owner'], views: [
    { slug: 'lesson', for: 'members', screen: { data: { q: 'SELECT * FROM questions WHERE lesson = :id LIMIT 20', l: 'SELECT * FROM lessons LIMIT 5' }, body: [] } },
    { slug: 'manage', for: 'owner', screen: { data: { q: 'SELECT * FROM questions LIMIT 20' }, body: [] } },
    { slug: 'home', screen: { data: { l: 'SELECT * FROM lessons LIMIT 5' }, body: [] } }] };
  const p = readerProblems(plan, { tables: [] });
  assert.equal(p.length, 1);
  assert.match(p[0], /^view lesson .* reads questions, which only the owner may read/);
  // members may read the questions: nothing to say
  plan.access.push('ALLOW READ ON questions TO members');
  assert.deepEqual(readerProblems(plan, { tables: [] }), []);
});

test('a screen its users use that writes a table only the owner may write is a problem; a rule they call, or the owner\'s screen, is not', async () => {
  const { writerProblems } = await import('../engine/forge.mjs');
  const plan = { access: ['ALLOW READ ON items TO everyone', 'ALLOW ALL ON requests TO owner', 'ALLOW CALL ON send-request TO everyone'], views: [
    { slug: 'front', screen: { data: { p: 'SELECT * FROM items LIMIT 20' }, body: [{ form: 'requests', fields: ['name'] }] } },
    { slug: 'ask', screen: { body: [{ button: 'Send', do: "=() => app.call('send-request', {id: 1})" }] } },
    { slug: 'quick', for: 'members', screen: { body: [{ button: 'Add', do: "=() => app.insert('requests', {a: 1})" }] } },
    { slug: 'manage', for: 'owner', screen: { body: [{ form: 'requests', fields: ['name'] }] } }] };
  const p = writerProblems(plan, { tables: [] });
  assert.deepEqual(p.map((x) => x.split(' ')[1]), ['front', 'quick']);
  assert.match(p[0], /^view front is for its visitors and members, but adds to requests, which only the owner may do/);
  assert.match(p[1], /^view quick is for its signed-in members/);
  // members may add their own: nothing to say about the members' screen
  plan.access.push('ALLOW ADD ON requests TO members WHERE owner_id = :user_id');
  assert.deepEqual(writerProblems(plan, { tables: [] }).map((x) => x.split(' ')[1]), []);
});

test('access opened to members while every screen stays the owner\'s is a problem; a screen of theirs, or access kept to the owner, is not', async () => {
  const { audienceProblems } = await import('../engine/forge.mjs');
  const view = { views: [{ slug: 'dashboard', spec: { for: 'owner' } }, { slug: 'invoices', spec: { for: 'owner' } }] };
  const p = audienceProblems({ access: ['ALLOW ALL ON invoices TO members'] }, view);
  assert.equal(p.length, 1);
  assert.match(p[0], /every screen is still for the owner \(dashboard, invoices\)/);
  assert.deepEqual(audienceProblems({ access: ['ALLOW ALL ON invoices TO members'], views: [{ slug: 'invoices', for: 'members' }] }, view), []);
  assert.deepEqual(audienceProblems({ access: ['ALLOW ALL ON invoices TO owner'] }, view), []);
  assert.deepEqual(audienceProblems({}, view), []);
});

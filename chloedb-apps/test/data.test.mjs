// What an app holds when it opens: attached sheets, example rows, and the
// figures a build checks (engine/data.mjs). No server and no model.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseCsv, parseAttached, inferKind, toValue, planImports, importRows, alignSeed, computeFigure, compareFigure, numbersIn, figureNotShown, prepareData, gateOf, visitorsRead, tablesIn } from '../engine/data.mjs';

const REQUEST = `Here is my sales workbook.

My data, from the attached workbook northwind-sales.xlsx:

Sheet "Monthly Sales":
Month,Revenue,Cost of Goods
Jan,42150,18300
Feb,39800,17650

Sheet "Accounts Receivable":
Customer,Invoice,Amount,Due Date,Status
Bramble & Co,INV-1042,3200,2026-06-15,Open
Fenwick Supplies,INV-1055,1875,2026-06-28,Open
Bramble & Co,INV-1070,2100,2026-07-12,Paid
Harlow Print Shop,INV-1038,980,2026-05-30,Overdue

Sheet "Pieces":
Name,Description,Price,Stock
Moon Bowl,"Hand-thrown stoneware bowl, matte white glaze",£45,8
Coastal Platter,"Large serving platter, sea-blue glaze",£85,0
`;

test('CSV: quotes, commas inside quotes, doubled quotes, blank lines', () => {
  assert.deepEqual(parseCsv('a,b\n"x, y","say ""hi"""\n\n1,2\n'), [['a', 'b'], ['x, y', 'say "hi"'], ['1', '2']]);
});

test('attached sheets: every row of every sheet, with typed columns', () => {
  const sheets = parseAttached(REQUEST);
  assert.deepEqual(sheets.map((s) => [s.name, s.rows.length]), [['Monthly Sales', 2], ['Accounts Receivable', 4], ['Pieces', 2]]);
  const ar = sheets[1];
  assert.deepEqual(ar.columns.map((c) => [c.name, c.kind, c.type]), [['customer', 'label', 'keyword'], ['invoice', 'label', 'keyword'], ['amount', 'int', 'int'], ['due_date', 'date', 'keyword'], ['status', 'label', 'keyword']]);
  const price = sheets[2].columns.find((c) => c.name === 'price');
  assert.equal(price.kind, 'money');
  assert.equal(price.currency, 'GBP');
  assert.equal(sheets[2].rows[0].Description, 'Hand-thrown stoneware bowl, matte white glaze');
  assert.equal(parseAttached('no sheets here').length, 0);
});

test('types: phone numbers stay labels, money and decimals become numbers', () => {
  assert.equal(inferKind(['07700 900311', '07700 900428']).type, 'keyword');
  assert.equal(inferKind(['0123', '0456']).type, 'keyword');
  assert.equal(inferKind(['4.50', '4']).type, 'float');
  assert.equal(inferKind(['28 €', '16 €']).kind, 'money');
  assert.equal(inferKind(['22:41', '19:58']).type, 'keyword');
  assert.equal(toValue('£1,250.50', 'float'), 1250.5);
  assert.equal(toValue('28 €', 'float'), 28);
  assert.equal(toValue('-$3', 'float'), -3);
  assert.equal(toValue('', 'float'), null);
  assert.equal(toValue('15/06/2026', 'keyword'), '2026-06-15');
});

test('imports: sheets go to the plan tables by name, missing columns are added and money columns made numbers', () => {
  const sheets = parseAttached(REQUEST);
  const plan = { tables: [
    { name: 'monthly_sales', columns: [{ name: 'month', type: 'keyword' }, { name: 'revenue', type: 'keyword' }] },
    { name: 'invoices', columns: [{ name: 'customer', type: 'keyword' }, { name: 'amount', type: 'float' }, { name: 'due', type: 'keyword' }, { name: 'status', type: 'keyword' }] },
    { name: 'pieces', columns: [{ name: 'name', type: 'keyword' }] },
  ], import: { 'Accounts Receivable': { table: 'invoices', columns: { 'Due Date': 'due' } } } };
  const r = planImports(plan, sheets);
  assert.deepEqual(r.problems, []);
  assert.deepEqual(r.imports.map((i) => [i.sheet, i.table]), [['Monthly Sales', 'monthly_sales'], ['Accounts Receivable', 'invoices'], ['Pieces', 'pieces']]);
  assert.equal(plan.tables[0].columns.find((c) => c.name === 'revenue').type, 'int', 'a number kept as keyword is made a number');
  assert.ok(plan.tables[0].columns.find((c) => c.name === 'cost_of_goods'), 'a column the plan forgot is added');
  assert.equal(plan.tables[2].columns.find((c) => c.name === 'price').type, 'float');
  const rows = importRows(r.imports[1]);
  assert.deepEqual(rows[0], { customer: 'Bramble & Co', invoice: 'INV-1042', amount: 3200, due: '2026-06-15', status: 'Open' });
  assert.equal(importRows(r.imports[2])[1].price, 85);
});

test('prepareData: a sheet with no table gets one, seed rows for imported tables are dropped, bad figures are left out', () => {
  const sheets = parseAttached(REQUEST);
  const plan = {
    tables: [{ name: 'sales', columns: [{ name: 'month', type: 'keyword' }, { name: 'revenue', type: 'float' }, { name: 'cost_of_goods', type: 'float' }] }],
    import: { 'Monthly Sales': 'sales' },
    seed: { sales: [{ month: 'Mar', revenue: 1 }] },
    figures: [{ label: 'Revenue', table: 'sales', sum: 'revenue' }, { label: 'Bad', table: 'sales', sum: 'nope' }],
  };
  const d = prepareData(plan, { tables: [], comp: { n: 0 } }, sheets, '2026-11-13');
  assert.deepEqual(d.imports.map((i) => [i.table, i.rows.length]), [['sales', 2], ['accounts_receivable', 4], ['pieces', 2]]);
  assert.equal(plan.seed.sales, undefined);
  assert.deepEqual(d.figures.map((f) => f.label), ['Revenue']);
});

test('example rows line up: references by id, names turned into ids, dates near today', () => {
  const seed = {
    projects: [{ name: 'Brand refresh' }, { name: 'Website' }],
    tasks: [{ title: 'Logo', project: 'Website', due: '2024-03-01' }, { title: 'Copy', project: 'zzz', due: '2024-03-05' }, { title: 'QA', project: 'project-1', due: '2024-03-03' }],
  };
  const tables = [{ name: 'projects', columns: [{ name: 'name', type: 'keyword' }] }, { name: 'tasks', columns: [{ name: 'title', type: 'text' }, { name: 'project', type: 'keyword' }, { name: 'due', type: 'keyword' }] }];
  const notes = alignSeed(seed, tables, '2026-11-13');
  assert.deepEqual(seed.projects.map((p) => p.id), ['project-1', 'project-2']);
  assert.equal(seed.tasks[0].project, 'project-2');
  assert.ok(['project-1', 'project-2'].includes(seed.tasks[1].project));
  assert.equal(seed.tasks[2].project, 'project-1');
  assert.equal(seed.tasks[2].due, '2026-11-13');
  assert.equal(seed.tasks[0].due, '2026-11-11');
  assert.ok(notes.length >= 2);
});

const INVOICES = [
  { customer: 'Bramble & Co', amount: 3200, due: '2026-11-20', status: 'Open' },
  { customer: 'Fenwick', amount: 1875, due: '2026-11-03', status: 'Open' },
  { customer: 'Bramble & Co', amount: 2100, due: '2026-09-30', status: 'Open' },
  { customer: 'Harlow', amount: 980, due: '2026-07-30', status: 'Overdue' },
  { customer: 'Corner', amount: 4500, due: '2026-10-01', status: 'Paid' },
];

test('figures: a total, a top N by group and aging buckets, worked out from rows', () => {
  assert.deepEqual(computeFigure({ table: 'i', sum: 'amount' }, INVOICES), { kind: 'one', value: 12655 });
  assert.deepEqual(computeFigure({ table: 'i', count: true, where: { status: { not: ['Paid'] } } }, INVOICES), { kind: 'one', value: 4 });
  const top = computeFigure({ table: 'i', sum: 'amount', by: 'customer', top: 2 }, INVOICES);
  assert.deepEqual(top.groups, [{ key: 'Bramble & Co', value: 5300 }, { key: 'Corner', value: 4500 }]);
  const aging = computeFigure({ table: 'i', sum: 'amount', where: { status: { not: 'Paid' } }, age: { of: 'due', buckets: [[null, 0], [1, 30], [31, 60], [61, 90], [91, null]] } }, INVOICES, '2026-11-13');
  assert.deepEqual(aging.buckets.map((b) => b.value), [3200, 1875, 2100, 0, 980]);
});

test('figures: a query that agrees passes; a wrong top N, a wrong order and double-counted buckets are reported', () => {
  const fig = { label: 'Top 2', table: 'i', sum: 'amount', by: 'customer', top: 2 };
  const exp = computeFigure(fig, INVOICES);
  assert.equal(compareFigure(fig, exp, [{ customer: 'Bramble & Co', total: 5300 }, { customer: 'Corner', total: 4500 }]), null);
  assert.match(compareFigure(fig, exp, [{ customer: 'Bramble & Co', total: 3200 }, { customer: 'Corner', total: 4500 }]), /lacks Bramble & Co 5,300/);
  assert.match(compareFigure(fig, exp, [{ customer: 'Corner', total: 4500 }, { customer: 'Bramble & Co', total: 5300 }]), /biggest first/);
  assert.match(compareFigure(fig, exp, [{ customer: 'Bramble & Co', total: 5300 }, { customer: 'Corner', total: 4500 }, { customer: 'Harlow', total: 980 }]), /not the top 2/);
  const one = { label: 'Total', table: 'i', sum: 'amount' };
  assert.equal(compareFigure(one, computeFigure(one, INVOICES), [{ v: 12655 }]), null);
  assert.match(compareFigure(one, computeFigure(one, INVOICES), [{ v: 8155 }]), /should be 12,655/);
  const ag = { label: 'Aging', table: 'i', sum: 'amount', where: { status: { not: ['Paid'] } }, age: { of: 'due', buckets: [[null, 0], [1, 30], [31, 60], [61, 90], [91, null]] } };
  const e = computeFigure(ag, INVOICES, '2026-11-13');
  assert.equal(compareFigure(ag, e, [{ current: 3200, d1_30: 1875, d31_60: 2100, d61_90: 0, d90: 980 }]), null);
  assert.equal(compareFigure(ag, e, [{ bucket: 'Current', amount: 3200 }, { bucket: '1-30', amount: 1875 }, { bucket: '31-60', amount: 2100 }, { bucket: '90+', amount: 980 }]), null);
  assert.match(compareFigure(ag, e, [{ current: 3200, d1_30: 3975, d31_60: 2100, d61_90: 0, d90: 980 }]), /exactly one range/);
});

test('figures on a page: numbers in any common format, and what a page fails to show', () => {
  assert.ok(numbersIn('Total $12,655.00 today').includes(12655));
  assert.ok(numbersIn('Pagado 5.300,00 €').includes(5300));
  assert.ok(numbersIn('28 € el corte').includes(28));
  assert.ok(numbersIn('Revenue 266.8k').some((n) => Math.abs(n - 266800) < 1));
  const top = computeFigure({ table: 'i', sum: 'amount', by: 'customer', top: 2 }, INVOICES);
  assert.deepEqual(figureNotShown(top, 'Top customers\nBramble & Co $5,300.00\nCorner $4,500.00'), []);
  assert.deepEqual(figureNotShown(top, 'Top 5 customers by amount invoiced'), ['Bramble & Co 5,300', 'Corner 4,500']);
});

test('sign-in entry: a screen whose data visitors may not read gets one; open and front screens do not', async () => {
  assert.equal(visitorsRead(['ALLOW READ ON races TO everyone', 'ALLOW WRITE ON races TO admins']), true);
  assert.equal(visitorsRead(['ALLOW ALL ON tasks TO members WHERE owner_id = :user_id']), false);
  assert.equal(visitorsRead(null), true);
  assert.deepEqual(tablesIn('SELECT t.*, p.name FROM tasks t JOIN projects p ON p.id = t.project'), ['tasks', 'projects']);
  const access = { tasks: ['ALLOW ALL ON tasks TO members'], races: ['ALLOW READ ON races TO everyone'] };
  const of = (t) => access[t] || null;
  const tasks = { data: { tasks: 'SELECT * FROM tasks LIMIT 50' }, body: [] };
  const gate = gateOf(tasks, of, { name: 'Agency projects', welcome: { text: 'Projects and tasks for the studio.', points: ['Your tasks', 'Your projects'] }, title: 'Tasks' });
  assert.equal(gate.title, 'Agency projects');
  assert.deepEqual(gate.points, ['Your tasks', 'Your projects']);
  const { sharedRefs } = await import('../engine/graph.mjs');
  assert.ok(sharedRefs(JSON.stringify({ ...tasks, gate })).has('component/signIn'), 'the app pins the sign-in form the entry draws');
  assert.equal(gateOf({ data: { races: 'SELECT * FROM races LIMIT 9' }, body: [] }, of, {}), null);
  assert.equal(gateOf({ page: 'front', data: { tasks: 'SELECT * FROM tasks LIMIT 9' }, body: [] }, of, {}), null);
  // A view in JavaScript: the SQL in its code.
  assert.ok(gateOf({ data: { code: "async function render(root, app) { const rows = await app.sql('SELECT * FROM tasks ORDER BY due LIMIT 50'); }" } }, of, { name: 'Agency' }));
});

test('the owner takes the example rows of tables that show each person their own rows', async () => {
  const { Services } = await import('../engine/services.mjs');
  const writes = [];
  const db = {
    query: async (sql) => (/WHERE owner_id IS NULL/.test(sql) ? [{ id: 'r1' }, { id: 'r2' }] : []),
    write: async (ops) => { writes.push(...ops); },
  };
  const store = { resolve: async () => ({ tables: [
    { spec: { name: 'tasks', access: ['ALLOW ALL ON tasks TO members WHERE owner_id = :user_id'] } },
    { spec: { name: 'projects', access: ['ALLOW READ ON projects TO members'] } },
  ] }) };
  const s = new Services({ db, store }, { filesDir: '/tmp' });
  await s.claimExamples('app1', 'p9');
  assert.deepEqual(writes.map((w) => [w.key, w.field, w.value]), [['r1', 'owner_id', 'p9'], ['r2', 'owner_id', 'p9']]);
  assert.match(writes[0].table, /tasks/);
});

test('the owner takes no example rows of a table they read whole already', async () => {
  const { Services } = await import('../engine/services.mjs');
  const writes = [];
  const db = { query: async () => [{ id: 'r1' }], write: async (ops) => { writes.push(...ops); } };
  const store = { resolve: async () => ({ tables: [{ spec: { name: 'tasks', access: ['ALLOW ALL ON tasks TO admins', 'ALLOW READ ON tasks TO members WHERE assignee_id = :user_id OR lead_id = :user_id'] } }] }) };
  await new Services({ db, store }, { filesDir: '/tmp' }).claimExamples('app1', 'p9');
  assert.deepEqual(writes, []);
});

test('a request that renames the main heading names the app', async () => {
  const { renameIn } = await import('../engine/data.mjs');
  assert.equal(renameIn('Rename the main heading to Northwind at a glance.'), 'Northwind at a glance');
  assert.equal(renameIn('Change the title to "Bean Counter"'), 'Bean Counter');
  assert.equal(renameIn('call the app Plant care'), 'Plant care');
  assert.equal(renameIn('Add a column for the due date'), null);
  assert.equal(renameIn('Rename the status "done" to "shipped"'), null);
});

// Against a real chloedb-server (skipped without one): a screen's queries
// are run on imported rows and compared with the figures worked out from
// them; a wrong top N and double-counted aging buckets are caught.
test('figures: screen queries checked against imported rows in ChloeDB', async (t) => {
  const net = await import('node:net');
  const { mkdtempSync } = await import('node:fs');
  const { tmpdir } = await import('node:os');
  const { join } = await import('node:path');
  const { findServerBinary, startServer } = await import('../engine/strata.mjs');
  const { Store } = await import('../engine/store.mjs');
  const { figureProblems } = await import('../engine/forge.mjs');
  const bin = findServerBinary(join(import.meta.dirname, '..', '..'));
  if (!bin) {
    if (process.env.STRATA_APPS_NEED_SERVER === '1') throw new Error('no chloedb-server binary');
    return t.skip('no chloedb-server binary');
  }
  const dir = mkdtempSync(join(tmpdir(), 'sa-figures-'));
  const port = await new Promise((r) => { const s = net.createServer().listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => r(p)); }); });
  const { child, client } = await startServer({ bin, db: join(dir, 'apps.strata'), listen: `127.0.0.1:${port}`, log: () => {} });
  try {
    const store = new Store(client, () => {});
    await store.init();
    const spec = { name: 'invoices', columns: [{ name: 'customer', type: 'keyword' }, { name: 'amount', type: 'float' }, { name: 'due', type: 'keyword' }, { name: 'status', type: 'keyword' }] };
    await store.ensureTable('figs', spec, new Set(await store.tableNames()));
    await client.write(INVOICES.map((r, i) => ({ op: 'insert', table: 'app_figs_invoices', doc: { id: `i${i}`, ...r } })));
    const buckets = [[null, 0], [1, 30], [31, 60], [61, 90], [91, null]];
    const figures = [
      { label: 'Top 2', view: 'dash', data: 'top', table: 'invoices', sum: 'amount', by: 'customer', top: 2 },
      { label: 'Aging', view: 'dash', data: 'aging', table: 'invoices', sum: 'amount', where: { status: { not: ['Paid'] } }, age: { of: 'due', buckets } },
    ];
    const plan = (top, aging) => ({ views: [{ slug: 'dash', screen: { data: { top, aging }, body: [] } }] });
    const good = plan('SELECT customer, SUM(amount) AS total FROM invoices GROUP BY customer ORDER BY total DESC LIMIT 2',
      { sql: "SELECT SUM(CASE WHEN due >= :today THEN amount ELSE 0 END) AS cur, SUM(CASE WHEN due < :today AND due >= '2026-10-14' THEN amount ELSE 0 END) AS d30, SUM(CASE WHEN due < '2026-10-14' AND due >= '2026-09-14' THEN amount ELSE 0 END) AS d60, SUM(CASE WHEN due < '2026-09-14' AND due >= '2026-08-15' THEN amount ELSE 0 END) AS d90, SUM(CASE WHEN due < '2026-08-15' THEN amount ELSE 0 END) AS over FROM invoices WHERE status != 'Paid'", params: { today: '=today' } });
    assert.deepEqual(await figureProblems(client, 'figs', ['invoices'], good, figures, '2026-11-13'), []);
    // Ungrouped "top" and buckets that count paid invoices too.
    const bad = plan('SELECT customer, amount AS total FROM invoices ORDER BY amount DESC LIMIT 2',
      { sql: "SELECT SUM(CASE WHEN due >= :today THEN amount ELSE 0 END) AS cur, SUM(CASE WHEN due < :today AND due >= '2026-10-14' THEN amount ELSE 0 END) AS d30, SUM(CASE WHEN due < '2026-10-14' AND due >= '2026-09-14' THEN amount ELSE 0 END) AS d60, SUM(CASE WHEN due < '2026-09-14' THEN amount ELSE 0 END) AS over FROM invoices", params: { today: '=today' } });
    const problems = await figureProblems(client, 'figs', ['invoices'], bad, figures, '2026-11-13');
    assert.equal(problems.length, 2, problems.join('\n'));
    assert.match(problems[0], /data\.top gives the wrong figure: "Top 2" should give Bramble & Co: 5,300/);
    assert.match(problems[1], /data\.aging gives the wrong figure/);
  } finally {
    child.kill();
  }
});

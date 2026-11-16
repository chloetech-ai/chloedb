import { spawn } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';
import assert from 'node:assert/strict';
const A = join(import.meta.dirname, '..');
const { startServer } = await import(`${A}/engine/strata.mjs`);
const { Store } = await import(`${A}/engine/store.mjs`);
const { compileStatement } = await import(`${A}/engine/statement.mjs`);
const { expandScreen } = await import(`${A}/engine/patterns.mjs`);
test('the patterns of what comes next, what falls due, lending, a week plan, a numbered report, the team and prices draw with their rows, on a phone and a computer', async (t) => {
  const { findServerBinary } = await import(`${A}/engine/strata.mjs`);
  const { loadPlaywright } = await import(`${A}/engine/checks.mjs`);
  const bin = process.env.STRATA_SERVER_BIN || findServerBinary(join(A, '..'));
  const pw = loadPlaywright();
  if (!bin || !pw) return t.skip('needs chloedb-server and playwright');
  const dir = mkdtempSync('/tmp/sa-smoke-');
  const { child: strata, client } = await startServer({ bin, db: join(dir, 'apps.strata'), listen: '127.0.0.1:7991', log: () => {} });
  let eng;
  const seen = [];
  const today = new Date().toISOString().slice(0, 10);
  const day = (n) => new Date(Date.now() + n * 86400000).toISOString().slice(0, 10);
  try {
    const TOKEN = 'smoke-token';
    eng = spawn(process.execPath, [join(A, 'engine', 'server.mjs'), '--port', '7992', '--strata', 'http://127.0.0.1:7991', '--data', join(dir, 'engine')],
      { env: { ...process.env, STRATA_APPS_PUBLIC_RATE: '5000', STRATA_APP_TOKEN: TOKEN, STRATA_APPS_SCHEDULES: 'off', STRATA_APPS_AUTOREPAIR: '0', STRATA_APPS_TILES: 'off', STRATA_APPS_GEOCODER: 'off' }, stdio: ['ignore', 'pipe', 'pipe'] });
    let log = ''; eng.stdout.on('data', (d) => { log += d; }); eng.stderr.on('data', (d) => { log += d; });
    const base = 'http://127.0.0.1:7992';
    for (let i = 0; ; i++) { try { if ((await fetch(`${base}/healthz`)).ok) break; } catch {} if (i > 600) throw new Error('engine did not start\n' + log.slice(-1500)); await new Promise((r) => setTimeout(r, 100)); }
    const store = new Store(client, () => {});
    await store.init();
    for (let i = 0; i < 100; i++) { const sh = await store.shared(); if (sh.size > 100) break; await new Promise((r) => setTimeout(r, 200)); }
    const app = await store.createApp({ name: 'Smoke' });
    const T = (name, cols, access) => ({ name, columns: Object.entries(cols).map(([n, type]) => ({ name: n, type })), ...(access ? { access } : {}) });
    const tables = [
      T('sessions', { title: 'text', day: 'keyword', venue: 'text', summary: 'text' }),
      T('permits', { title: 'text', expires: 'keyword', holder: 'text', kind: 'keyword', status: 'keyword' }),
      T('tools', { name: 'text', serial: 'keyword' }),
      T('loans', { tool: 'keyword', holder: 'text', out_day: 'keyword', due_day: 'keyword', back_day: 'keyword' }),
      T('jobs', { name: 'text' }), T('rota', { day: 'keyword', job: 'keyword' }), T('job_parts', { job: 'keyword', material: 'text', qty: 'float', unit: 'keyword' }),
      T('traffic', { month: 'keyword', visits: 'float' }), T('tickets', { opened: 'keyword', hours: 'float', status: 'keyword' }),
      T('accounts', { name: 'text' }), T('members_of', { person: 'keyword', account: 'keyword', role: 'keyword' }),
      T('services', { name: 'text', price: 'float', length: 'text', kind: 'keyword' }),
      T('ledger', { title: 'text', amount: 'float' }, ['ALLOW ALL ON ledger TO members']),
      T('weekly', { name: 'text', weekday: 'keyword', start_time: 'keyword', minutes: 'int', leader: 'text', hall: 'text', spaces: 'int' }), T('places', { class: 'keyword', date: 'keyword' }),
      T('appointments', { starts: 'keyword', ends: 'keyword', staff: 'keyword' }), T('staff_members', { name: 'text' }),
      T('requests', { name: 'text', email: 'keyword', message: 'text', status: 'keyword', topic: 'keyword' }),
      T('fixtures', { home: 'keyword', away: 'keyword', hg: 'int', ag: 'int' }),
      T('bills', { number: 'keyword', issued: 'keyword', client_name: 'text', terms: 'text' }), T('bill_lines', { bill: 'keyword', description: 'text', qty: 'float', price: 'float' }),
      T('pictures', { file: 'text', caption: 'text', album: 'keyword' }),
    ];
    const have = new Set(await store.tableNames());
    const caps = [];
    for (const spec of tables) { await store.ensureTable(app, spec, have); caps.push(await store.putCapability({ app, slug: spec.name, kind: 'table', title: spec.name, spec, code: '' })); }
    const S = {
      next: `CREATE SCREEN next TITLE 'Next'\nNEXT sessions\n  DATE day  NAME title  PLACE venue  TEXT summary  EYEBROW 'Next session'  ACTION 'Save a place'  MORE 4`,
      due: `CREATE SCREEN due TITLE 'Expiring'\nDUE permits\n  DUE expires  NAME title  SUB holder, kind  SOON 30  LABEL 'Permit'\n  WHERE status != 'retired'\n  ACTION 'Mark renewed'\n  SET {expires: lib.dates.addDays(today, 365)}\n  NOUN 'permit'`,
      lend: `CREATE SCREEN lend TITLE 'Tools'\nLEND loans\n  ITEMS tools  ITEM tool  NAME name  SUB serial  HOLDER holder  DUE due_day  NOUN 'tool'  LABEL 'Tool'`,
      plan: `CREATE SCREEN plan TITLE 'Week'\nPLAN rota\n  ITEMS jobs  ITEM job  DAY day  PARTS job_parts  OF job  PART material  QTY qty  UNIT unit\n  TITLE 'This week'  PICK 'Pick a job'  TOTALS 'Materials to buy'`,
      report: `CREATE SCREEN report TITLE 'Report'\nREPORT 'Month-end report'\n  SUB '=''As of '' + app.fmt.date(today)'\n  SECTION 'Visits by month' (SELECT month, SUM(visits) AS visits FROM traffic GROUP BY month ORDER BY month) COLUMNS (month 'Month', visits 'Visits' NUMBER) TOTAL (visits) NOTE 'Counted in the month made.'\n  SECTION 'Open tickets by days open' (SELECT b.band AS band, COUNT(x.hours) AS n, COALESCE(SUM(x.hours), 0) AS hours FROM bands() AS b LEFT JOIN (SELECT band_of(days_between(opened, :today)) AS band, hours FROM tickets WHERE status = 'open') AS x ON x.band = b.band GROUP BY b.band, b.r ORDER BY b.r) COLUMNS (band 'Days open', n 'Tickets' NUMBER, hours 'Hours' NUMBER) TOTAL (n, hours)`,
      team: `CREATE SCREEN team TITLE 'Team'\nTEAM members_of\n  THINGS accounts  PERSON person  THING account  ROLE role  ROLES ('lead', 'member', 'client')  LABEL 'Account'`,
      tt: `CREATE SCREEN tt TITLE 'Timetable'\nTIMETABLE weekly\n  DAY weekday  TIME start_time  MINUTES minutes  NAME name  SUB leader  PLACE hall  SPACES spaces\n  BOOKINGS places  OF class  DATE date  ACTION 'Book'`,
      sl: `CREATE SCREEN sl TITLE 'Book'\nSLOTS appointments\n  START starts  END ends  OPEN '09:00'  CLOSE '12:00'  STEP 30  LENGTH 30  CLOSED (0)\n  RESOURCE staff  RESOURCES staff_members  LABEL name  TITLE 'Book a time'`,
      ib: `CREATE SCREEN ib TITLE 'Inbox'\nINBOX requests\n  NAME name  CONTACT email  TEXT message  STATUS status  SUB topic  NOUN 'request'`,
      st: `CREATE SCREEN st TITLE 'Table'\nSTANDINGS fixtures\n  QUERY (SELECT side AS name, SUM(pts) AS value, COUNT(*) AS sub FROM (SELECT home AS side, CASE WHEN hg > ag THEN 3 WHEN hg = ag THEN 1 ELSE 0 END AS pts FROM fixtures UNION ALL SELECT away AS side, CASE WHEN ag > hg THEN 3 WHEN hg = ag THEN 1 ELSE 0 END AS pts FROM fixtures) AS x GROUP BY side ORDER BY value DESC)\n  UNIT 'played'  TITLE 'Table'`,
      doc: `CREATE SCREEN doc TITLE 'Invoice'\nDOCUMENT bills\n  LABEL 'Invoice'  NUMBER number  DATE issued  TO client_name  BY 'Studio North'  LINES bill_lines  OF bill  ITEM description  QTY qty  PRICE price  TAX 20  NOTE terms`,
      ga: `CREATE SCREEN ga TITLE 'Work'\nGALLERY pictures\n  FILE file  CAPTION caption  ALBUM album  TITLE 'Our work'`,
      prices: `CREATE SCREEN prices TITLE 'Prices'\nPRICES services\n  NAME name  PRICE price  NOTE length  GROUP kind  TITLE 'Services and prices'\nPRICES services AS rates\n  COLUMNS (name 'Service', price 'Price' MONEY, length 'Takes')`,
    };
    let order = 0;
    for (const [slug, text] of Object.entries(S)) {
      const c = compileStatement(text);
      const r = expandScreen(c.screen);
      if (r.problems.length) throw new Error(`${slug}: ${r.problems.join('; ')}`);
      caps.push(await store.putCapability({ app, slug, kind: 'view', title: c.title, spec: { order: order++, screen: true, statement: text }, code: JSON.stringify(r.screen, null, 1) }));
    }
    for (const c of (await store.shared()).values()) caps.push(c.id);
    // a members' screen: a visitor sees it with the example rows kept for it
    caps.push(await store.putCapability({ app, slug: 'ledger', kind: 'view', title: 'Ledger', spec: { order: 99, screen: true },
      code: JSON.stringify({ data: { ledger: 'SELECT * FROM ledger ORDER BY title LIMIT 50' }, body: [{ h: 'Ledger' }, { table: '=ledger', columns: [{ key: 'title', label: 'Entry' }, { key: 'amount', label: 'Amount', format: 'money' }] }],
        gate: { title: 'Smoke ledger', text: 'Sign in to see the ledger.', ui: 'signIn' } }) }));
    await store.putSetting(app, 'samples', JSON.stringify({ ledger: { ledger: [{ id: 'x1', title: 'Example entry', amount: 12 }] } }));
    await store.writeComposition(app, caps, 'smoke', 'patterns smoke test');
    const P = (t) => `app_${app}_${t}`;
    const ins = (t, rows) => client.write(rows.map((doc, i) => ({ op: 'insert', table: P(t), doc: { id: `${t}${i + 1}`, ...doc } })));
    await ins('sessions', [{ title: 'Open evening', day: day(5), venue: 'Main hall', summary: 'Tours every half hour.' }, { title: 'Spring fair', day: day(19), venue: 'Yard' }, { title: 'Late talk', day: day(33), venue: 'Studio 2' }, { title: 'Past one', day: day(-3) }]);
    await ins('permits', [{ title: 'Parking A', expires: day(-4), holder: 'Ana', kind: 'car', status: 'active' }, { title: 'Dock pass', expires: day(12), holder: 'Ben', kind: 'site', status: 'active' }, { title: 'Roof access', expires: day(90), holder: 'Cy', kind: 'site', status: 'active' }, { title: 'Old badge', expires: day(-40), status: 'retired' }]);
    await ins('tools', [{ name: 'Drill', serial: 'D-12' }, { name: 'Ladder', serial: 'L-3' }, { name: 'Saw', serial: 'S-7' }]);
    await ins('loans', [{ tool: 'tools1', holder: 'Ana', out_day: day(-10), due_day: day(-2) }, { tool: 'tools2', holder: 'Ben', out_day: day(-1), due_day: day(6) }, { tool: 'tools3', holder: 'Cy', out_day: day(-20), due_day: day(-15), back_day: day(-14) }]);
    await ins('jobs', [{ name: 'Fence repair' }, { name: 'Deck stain' }, { name: 'Gutter clean' }]);
    await ins('job_parts', [{ job: 'jobs1', material: 'screws', qty: 200, unit: 'pc' }, { job: 'jobs2', material: 'stain', qty: 4, unit: 'l' }, { job: 'jobs2', material: 'screws', qty: 100, unit: 'pc' }, { job: 'jobs3', material: 'bags', qty: 3, unit: 'pc' }]);
    const mon = (() => { const d = new Date(); const k = (d.getDay() + 6) % 7; return new Date(d.getTime() - k * 86400000); })();
    const wd = (i) => { const d = new Date(mon.getTime() + i * 86400000); return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`; };
    await ins('rota', [{ day: wd(0), job: 'jobs1' }, { day: wd(2), job: 'jobs2' }, { day: wd(3), job: 'jobs2' }]);
    await ins('traffic', [{ month: '2026-09', visits: 1200 }, { month: '2026-10', visits: 1500 }, { month: '2026-10', visits: 300 }, { month: '2026-11', visits: 900 }]);
    await ins('tickets', [{ opened: day(-5), hours: 5, status: 'open' }, { opened: day(-45), hours: 8, status: 'open' }, { opened: day(-100), hours: 2, status: 'open' }, { opened: day(-2), hours: 1, status: 'done' }]);
    await ins('accounts', [{ name: 'Northside' }, { name: 'Harbour' }, { name: 'Archive' }]);
    await ins('services', [{ name: 'Tuning', price: 30, length: '1 h', kind: 'Pianos' }, { name: 'Regulation', price: 75, length: '3 h', kind: 'Pianos' }, { name: 'Restring', price: 20, length: '40 min', kind: 'Guitars' }]);
    const pub = await fetch(`${base}/api/apps/${app}/publish`, { method: 'POST', headers: { 'content-type': 'application/json', authorization: `Bearer ${TOKEN}` }, body: JSON.stringify({ on: true, access: 'change', name: 'smoke' }) });
    assert.equal(pub.status, 200);
    const su = await fetch(`${base}/api/apps/${app}/signup`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ email: 'owner@local.test', name: 'Olive Owner', password: 'owner-pass-123' }) });
    const cookie = (su.headers.get('set-cookie') || '').split(';')[0];
    const sj = await su.json(); assert.equal(su.status, 200, JSON.stringify(sj)); const me = sj.user;
    await client.write([{ op: 'insert', table: P('members_of'), doc: { id: 'm1', person: me.id, account: 'accounts1', role: 'lead' } }]);
    const dow = ['sun', 'mon', 'tue', 'wed', 'thu', 'fri', 'sat'];
    await ins('weekly', [{ name: 'Early circuit', weekday: dow[new Date().getDay()], start_time: '07:00', minutes: 45, leader: 'Mo', hall: 'Hall A', spaces: 2 }, { name: 'Stretch', weekday: 'wed', start_time: '18:30', minutes: 60, leader: 'Lu', hall: 'Hall B', spaces: 10 }]);
    await ins('places', [{ class: 'weekly1', date: wd((new Date().getDay() + 6) % 7) }, { class: 'weekly1', date: wd((new Date().getDay() + 6) % 7) }]);
    await ins('staff_members', [{ name: 'Rae' }, { name: 'Sol' }]);
    await ins('appointments', [{ starts: `${day(1)}T09:30`, ends: `${day(1)}T10:00`, staff: 'staff_members1' }]);
    await ins('requests', [{ name: 'Kai', email: 'kai@example.com', message: 'Can you call me back?', status: 'new', topic: 'quote' }, { name: 'Lee', email: 'lee@example.com', message: 'Thanks!', status: 'done', topic: 'other' }]);
    await ins('fixtures', [{ home: 'Reds', away: 'Blues', hg: 2, ag: 1 }, { home: 'Greens', away: 'Reds', hg: 0, ag: 0 }, { home: 'Blues', away: 'Greens', hg: 3, ag: 3 }]);
    await ins('bills', [{ number: 'B-101', issued: day(-1), client_name: 'Harbour Ltd', terms: 'Due in 14 days.' }]);
    await ins('bill_lines', [{ bill: 'bills1', description: 'Design', qty: 2, price: 100 }, { bill: 'bills1', description: 'Print run', qty: 1, price: 50 }]);
    const dot = 'data:image/svg+xml,' + encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30"><rect width="40" height="30" fill="#5a7"/></svg>');
    await ins('pictures', [{ file: dot, caption: 'Window frame', album: 'Wood' }, { file: dot, caption: 'Garden wall', album: 'Stone' }]);
    await client.write([{ op: 'insert', table: P('ledger'), doc: { id: 'l1', title: 'Real entry', amount: 99 } }]);
    const browser = await pw.chromium.launch();
    {
      // a visitor: the members' screen with its example rows and a sign-in bar, never the real rows
      const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 } });
      const page = await ctx.newPage();
      await page.goto(`${base}/a/${app}#ledger`, { waitUntil: 'load' });
      await page.waitForTimeout(1200);
      const tab = page.locator('#tabs button.tab[data-slug="ledger"]');
      if (await tab.count()) { await tab.first().evaluate((b) => b.click()); }
      let text = '';
      for (let k = 0; k < 40 && !/Example entry/.test(text); k++) { await page.waitForTimeout(250); const f = page.frames().filter((x) => x !== page.mainFrame()).pop(); text = f ? await f.evaluate(() => document.body.innerText).catch(() => '') : ''; }
      seen.push({ tag: 'visitor', slug: 'ledger', bad: /hit an error/.test(text), text });
      await ctx.close();
    }
    for (const [vw, vh, tag] of [[1280, 900, 'desktop'], [390, 844, 'phone']]) {
      const ctx = await browser.newContext({ viewport: { width: vw, height: vh } });
      const [n, v] = cookie.split('=');
      await ctx.addCookies([{ name: n, value: v, url: base }]);
      const page = await ctx.newPage();
      const errs = []; page.on('pageerror', (e) => errs.push(e.message)); page.on('console', (m) => { if (m.type() === 'error') errs.push(m.text()); });
      for (const slug of [...Object.keys(S), 'ledger']) {
        await page.goto(`${base}/a/${app}#${slug}`, { waitUntil: 'load' });
        await page.waitForTimeout(1200);
        const tabs = page.locator(`#tabs button.tab[data-slug="${slug}"]`);
        if (await tabs.count()) { await tabs.first().evaluate((b) => b.click()); await page.waitForTimeout(1500); }
        // (the screen is drawn once its rows are in: wait for its words to settle)
        let text = '';
        for (let k = 0, last = null; k < 40; k++) {
          const f = page.frames().filter((x) => x !== page.mainFrame()).pop();
          text = f ? await f.evaluate(() => document.body.innerText).catch(() => '') : '';
          if (text.length > 40 && text === last) break;
          last = text;
          await page.waitForTimeout(250);
        }
        const bad = /hit an error|is not defined|undefined|NaN|\[object Object\]/.test(text);
        seen.push({ tag, slug, bad, text });
      }
      seen.push({ tag, errors: errs.filter((e) => !/status of 401/.test(e)) });
      await ctx.close();
    }
    await browser.close();
    if (process.env.SA_LIVE_LOG) console.log(log.split('\n').filter((l) => /error|refus|strata/i.test(l)).slice(-30).join('\n'));
  } finally {
    if (eng) eng.kill('SIGTERM');
    strata.kill();
  }

  const shown = (tag, slug) => seen.find((x) => x.tag === tag && x.slug === slug)?.text || '';
  assert.match(shown('visitor', 'ledger'), /Smoke ledger[\s\S]*example rows[\s\S]*Example entry/, 'a visitor sees the screen with its example rows, marked');
  assert.doesNotMatch(shown('visitor', 'ledger'), /Real entry/, 'and never the real rows');
  for (const tag of ['desktop', 'phone']) if (shown(tag, 'ledger')) assert.match(shown(tag, 'ledger'), /Real entry/, 'a member sees the real rows');
  for (const x of seen) if (x.slug) assert.equal(x.bad, false, `${x.tag} ${x.slug}: ${x.text.slice(0, 300)}`);
  for (const x of seen) if (x.errors) assert.deepEqual(x.errors, [], x.tag);
  for (const tag of ['desktop', 'phone']) {
    assert.match(shown(tag, 'next'), /Open evening[\s\S]*In \d+ days[\s\S]*Spring fair/, 'the next one leads, the ones after it follow');
    assert.doesNotMatch(shown(tag, 'next'), /Past one/);
    assert.match(shown(tag, 'due'), /Overdue[\s\S]*Parking A[\s\S]*overdue/);
    assert.doesNotMatch(shown(tag, 'due'), /Old badge/, 'a retired one is not watched');
    for (const w of [/Drill/, /overdue/i, /Ana/, /Check out/]) assert.match(shown(tag, 'lend'), w);
    assert.match(shown(tag, 'plan'), /Monday[\s\S]*Sunday/);
    assert.match(shown(tag, 'report'), /1\. Visits by month[\s\S]*2\. Open tickets by days open[\s\S]*current[\s\S]*31-60[\s\S]*61-90/i, 'every band shows, the empty ones too');
    assert.match(shown(tag, 'team'), /Northside[\s\S]*Olive Owner/);
    assert.match(shown(tag, 'prices'), /Tuning[\s\S]*\$30/);
  }
  assert.match(shown('desktop', 'report'), /Total\s+3,900/, 'a total row under the section');
  for (const tag of ['desktop', 'phone']) {
    assert.match(shown(tag, 'tt'), /Early circuit/); assert.match(shown(tag, 'tt'), /\bFull\b/i, 'a weekly row with no places left is Full');
    assert.match(shown(tag, 'tt'), /Previous week[\s\S]*Next week/);
    assert.match(shown(tag, 'tt'), /Stretch/);
    assert.match(shown(tag, 'sl'), /Book a time[\s\S]*Rae[\s\S]*Sol/);
    assert.match(shown(tag, 'ib'), /Kai[\s\S]*call me back/);
    assert.doesNotMatch(shown(tag, 'ib'), /Thanks!/, 'done ones are out of the open list');
    assert.match(shown(tag, 'st'), /Reds[\s\S]*4[\s\S]*Greens|Reds[\s\S]*Blues/);
    for (const w of [/Invoice/, /B-101/, /For Harbour Ltd/, /Design/, /Print run/, /Studio North/]) assert.match(shown(tag, 'doc'), w);
    assert.match(shown(tag, 'doc'), /\$300|300\.00/, 'the total with tax (250 + 20%)');
    assert.match(shown(tag, 'ga'), /Our work/);
  }
  assert.match(shown('desktop', 'plan'), /screws[\s\S]*400/i, 'the week adds up what its picks need');
});

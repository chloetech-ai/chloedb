// Card payments (services.pay) on a published app over a real
// chloedb-server (ports 7973 and 7977): a payment is bound to the order its
// payer placed, its hosted page pays only with the server's single-use
// token, test mode marks "paid (test)" and never "paid", only a provider's
// signed notice could mark "paid", and visitors and members do not read the
// payments or the orders.

import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { join } from 'node:path';

test('payments: bound to the order placed, a single-use token, test mode said, owners only read them', async (t) => {
  const { findServerBinary, startServer } = await import('../engine/strata.mjs');
  const { Store } = await import('../engine/store.mjs');
  const bin = findServerBinary(join(import.meta.dirname, '..', '..'));
  if (!bin) {
    if (process.env.STRATA_APPS_NEED_SERVER === '1') throw new Error('no chloedb-server binary');
    return t.skip('no chloedb-server binary');
  }
  const dir = mkdtempSync('/tmp/sa-pay-');
  const { child: strata, client } = await startServer({ bin, db: join(dir, 'apps.strata'), listen: '127.0.0.1:7973', log: () => {} });
  let engineProc = null;
  try {
    const store = new Store(client, () => {});
    await store.init();
    const app = await store.createApp({ name: 'Candles', description: 'A candle shop, prices in £' });
    const tables = [
      { name: 'orders', columns: [{ name: 'product', type: 'keyword' }, { name: 'qty', type: 'int' }, { name: 'total', type: 'float' }, { name: 'status', type: 'keyword' }],
        access: ['ALLOW ADD ON orders TO everyone', 'ALLOW READ, CHANGE, REMOVE ON orders TO owner'] },
    ];
    const caps = [];
    const have = new Set(await store.tableNames());
    for (const spec of tables) {
      await store.ensureTable(app, spec, have);
      caps.push(await store.putCapability({ app, slug: spec.name, kind: 'table', title: spec.name, spec, code: '' }));
    }
    await store.writeComposition(app, caps, 'first', 'a shop');
    await store.putSetting(app, 'publish', JSON.stringify({ on: true, name: 'candles', access: 'change' }));

    const TOKEN = 'pay-test-token';
    const env = { ...process.env, STRATA_APP_TOKEN: TOKEN, STRATA_APPS_SCHEDULES: 'off', STRATA_APPS_AUTOREPAIR: '0', STRATA_APPS_TILES: 'off', STRATA_APPS_GEOCODER: 'off' };
    delete env.STRATA_APPS_PAYMENT_PROVIDER;
    delete env.STRATA_APPS_PAYMENT_SECRET;
    engineProc = spawn(process.execPath, [join(import.meta.dirname, '..', 'engine', 'server.mjs'), '--port', '7977', '--strata', 'http://127.0.0.1:7973', '--data', join(dir, 'engine')],
      { env, stdio: ['ignore', 'pipe', 'pipe'] });
    let out = '';
    engineProc.stdout.on('data', (d) => { out += d; });
    engineProc.stderr.on('data', (d) => { out += d; });
    const base = 'http://127.0.0.1:7977';
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
    // A visitor's browser: the app's page gives it its visitor cookie.
    const visitor = async () => ({ cookie: (await fetch(`${base}/a/${app}`)).headers.get('set-cookie').split(';')[0] });
    const page = async (path, form) => {
      const res = await fetch(`${base}${path}`, form === undefined ? {} : { method: 'POST', headers: { 'content-type': 'application/x-www-form-urlencoded', origin: base }, body: new URLSearchParams(form).toString() });
      return { status: res.status, text: await res.text() };
    };

    // The owner joins from the studio (a stranger signing up first on the
    // public page never owns it), then a member signs up on the page.
    const owner = await call('/signup', { email: 'olive@example.com', name: 'Olive', password: 'correct horse' }, { studio: true });
    assert.equal(owner.status, 200, JSON.stringify(owner.body));
    const O = { cookie: owner.cookie };
    const member = await call('/signup', { email: 'bob@example.com', name: 'Bob', password: 'battery staple' });
    const B = { cookie: member.cookie };

    // A visitor places an order; another visitor, and nobody, may not pay for it.
    const V1 = await visitor();
    const V2 = await visitor();
    assert.notEqual(V1.cookie, V2.cookie);
    const placed = await call('/insert', { table: 'orders', row: { product: 'ember', qty: 1, total: 26, status: 'awaiting payment' } }, V1);
    assert.equal(placed.status, 200, JSON.stringify(placed.body));
    const orderId = placed.body.row.id;
    assert.equal((await call('/pay', { table: 'orders', id: orderId }, V2)).status, 403, 'another visitor');
    assert.equal((await call('/pay', { table: 'orders', id: orderId }, {})).status, 403, 'nobody');
    assert.equal((await call('/pay', { table: 'orders', id: orderId }, B)).status, 403, 'a member who did not place it');
    assert.equal((await call('/pay', { amount: 5, description: 'anything' }, V1)).status, 400, 'a visitor pays only for an order');

    // Its payer asks: the amount is the order's own, the url carries a token.
    const asked = await call('/pay', { table: 'orders', id: orderId, amount: 1, description: '1 x Ember Spice' }, V1);
    assert.equal(asked.status, 200, JSON.stringify(asked.body));
    const pmt = asked.body.payment;
    assert.equal(pmt.amount, 26, 'the row\'s total, not the page\'s amount');
    assert.equal(pmt.currency, 'GBP');
    assert.equal(pmt.mode, 'test');
    const [path, token] = pmt.url.split('?t=');
    assert.ok(token && token.length >= 24);

    // The page without the token, or with another, pays nothing.
    const bare = await page(path);
    assert.doesNotMatch(bare.text, /<form/);
    assert.match(bare.text, /not valid/);
    assert.match(bare.text, /Test mode/);
    assert.equal((await page(path, { t: 'x'.repeat(32) })).status, 403);
    const withToken = await page(`${path}?t=${token}`);
    assert.match(withToken.text, /<form method="post">/);
    // The token pays once, in test mode: "paid (test)", never "paid".
    const paid = await page(path, { t: token });
    assert.equal(paid.status, 200);
    assert.match(paid.text, /Paid in test mode\. No money moved\./);
    const after = await call('/sql', { sql: 'SELECT status FROM orders WHERE id = :id', params: { id: orderId } }, { studio: true });
    assert.deepEqual(after.body.rows, [{ status: 'paid (test)' }]);
    assert.equal((await page(path, { t: token })).text.match(/Paid in test mode/g).length, 1, 'a spent token pays nothing again');
    assert.equal((await call('/pay', { table: 'orders', id: orderId }, V1)).status, 409, 'an order paid already');

    // Only a provider's signed notice could mark "paid": none is configured.
    const notice = await fetch(`${base}/api/payments/notice`, { method: 'POST', headers: { 'x-payment-signature': 'sha256=00' }, body: JSON.stringify({ payment: pmt.id, status: 'paid' }) });
    const noticeBody = await notice.clone().json().catch(() => ({}));
    assert.equal(notice.status, 501, JSON.stringify(noticeBody));
    assert.match((await notice.json()).error, /no card payment provider is configured/);

    // The payments and the orders: the owner reads them, visitors and members do not.
    assert.equal((await call('/payments', undefined, V1)).status, 401);
    assert.equal((await call('/payments', undefined, B)).status, 403);
    const listed = await call('/payments', undefined, O);
    assert.equal(listed.status, 200, JSON.stringify(listed.body));
    assert.equal(listed.body.mode, 'test');
    assert.deepEqual(listed.body.payments.map((p) => [p.ref_id, p.status, p.mode]), [[orderId, 'paid_test', 'test']]);
    assert.ok(!('token_hash' in listed.body.payments[0]), 'no token in the list');
    assert.equal((await call('/payments', undefined, { studio: true })).status, 200);
    const sql = (opts) => call('/sql', { sql: 'SELECT id, status FROM orders' }, opts);
    assert.equal((await sql(V1)).status, 401);
    assert.equal((await sql(B)).status, 403);
    assert.deepEqual((await sql(O)).body.rows, [{ id: orderId, status: 'paid (test)' }]);
  } finally {
    if (engineProc) { engineProc.kill('SIGTERM'); await new Promise((r) => { if (engineProc.exitCode !== null) r(); else engineProc.on('exit', r); }); }
    strata.kill();
  }
});

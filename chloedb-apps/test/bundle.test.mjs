// An app carried from one engine to another (engine/bundle.mjs): a laptop's
// engine packs it, a second engine unpacks it as a new app, people use it
// there, and a later version carried again keeps their rows.
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import net from 'node:net';
import { bundleApp, importBundle, checkBundle } from '../engine/bundle.mjs';

function noServer(t) {
  if (process.env.STRATA_APPS_NEED_SERVER === '1') throw new Error('no chloedb-server binary: build chloedb-server or set STRATA_SERVER_BIN');
  return t.skip('no chloedb-server binary');
}
const freePort = () => new Promise((r) => { const s = net.createServer().listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => r(p)); }); });

test('a bundle without pieces, an app or its version number is refused before anything is written', () => {
  assert.throws(() => checkBundle(null), /not an app bundle/);
  assert.throws(() => checkBundle({ bundle: 2, app: { id: 'a', name: 'A' }, caps: [{}] }), /not an app bundle/);
  assert.throws(() => checkBundle({ bundle: 1, app: { id: 'a', name: 'A' }, caps: [] }), /no pieces/);
  assert.throws(() => checkBundle({ bundle: 1, app: { id: 'a', name: 'A' }, caps: [{ id: 'x', kind: 'view', slug: '../x', app: 'a' }] }), /odd kind or slug/);
  assert.doesNotThrow(() => checkBundle({ bundle: 1, app: { id: 'a', name: 'A' }, caps: [{ id: 'a/view/home@1', kind: 'view', slug: 'home', app: 'a' }] }));
});

test('an app carried to another engine: made there with its rows, look and files; a later version adds columns and keeps the rows people wrote', async (t) => {
  const { findServerBinary, startServer } = await import('../engine/strata.mjs');
  const { Store } = await import('../engine/store.mjs');
  const bin = findServerBinary(join(import.meta.dirname, '..', '..'));
  if (!bin) return noServer(t);
  const engines = [];
  for (const name of ['laptop', 'cloud']) {
    const dir = mkdtempSync(join(tmpdir(), `sa-bundle-${name}-`));
    const { child, client } = await startServer({ bin, db: join(dir, 'apps.strata'), listen: `127.0.0.1:${await freePort()}`, log: () => {} });
    t.after(() => child.kill());
    const store = new Store(client, () => {});
    await store.init();
    const filesDir = join(dir, 'files');
    mkdirSync(filesDir, { recursive: true });
    engines.push({ store, client, services: { filesDir } });
  }
  const [laptop, cloud] = engines;

  // On the laptop: an app with a table, a screen, two rows, its currency, a design and a photo.
  const a = await laptop.store.createApp({ name: 'Dog walks', description: 'Book a walk' });
  const spec1 = { name: 'bookings', columns: [{ name: 'dog', type: 'text' }, { name: 'status', type: 'keyword' }] };
  await laptop.store.ensureTable(a, spec1, new Set(await laptop.store.tableNames()));
  const tbl1 = await laptop.store.putCapability({ app: a, slug: 'bookings', kind: 'table', title: 'bookings', spec: spec1, code: '' });
  const screen = { body: [{ h: 'Walks' }, { table: '=bookings', columns: [{ key: 'dog' }] }], data: { bookings: 'SELECT * FROM bookings LIMIT 50' } };
  const view1 = await laptop.store.putCapability({ app: a, slug: 'home', kind: 'view', title: 'Walks', spec: { screen: true }, code: JSON.stringify(screen) });
  // a shared component it pins, written on the laptop; the cloud has another under the same id
  const comp = await laptop.store.putCapability({ app: 'lib', slug: 'walkBadge', kind: 'component', title: 'Walk badge', spec: { props: 'dog' }, code: 'function component(p) { return h("b", null, p.dog); }' });
  const clash = await cloud.store.putCapability({ app: 'lib', slug: 'walkBadge', kind: 'component', title: 'Walk badge', spec: { props: 'dog' }, code: 'function component(p) { return h("i", null, "other"); }' });
  assert.equal(comp, clash, 'the same id on both engines');
  await laptop.store.writeComposition(a, [tbl1, view1, comp], 'first', 'walks');
  await laptop.client.write([
    { op: 'insert', table: `app_${a}_bookings`, doc: { id: 'r1', dog: 'Rex', status: 'booked' } },
    { op: 'insert', table: `app_${a}_bookings`, doc: { id: 'r2', dog: 'Ada', status: 'done' } },
  ]);
  await laptop.store.putSetting(a, 'currency', 'EUR');
  await laptop.store.putSetting(a, 'secrets', 'sealed-on-the-laptop');
  await laptop.store.putSetting(a, 'publish', JSON.stringify({ on: true, name: 'walks' }));
  await laptop.store.putDesigns(a, { designs: [{ name: 'Calm', light: { accent: '#225' }, dark: { bg: '#111' } }], chosen: 0, source: 'test' });
  mkdirSync(join(laptop.services.filesDir, a), { recursive: true });
  writeFileSync(join(laptop.services.filesDir, a, 'photo0000001'), Buffer.from('a photo'));
  await laptop.client.write([{ op: 'insert', table: 'sa_files', doc: { id: 'photo0000001', app: a, name: 'rex.jpg', type: 'image/jpeg', size: 7, author: '' } }]);

  const b1 = JSON.parse(JSON.stringify(await bundleApp(laptop, a)));
  assert.equal(b1.app.name, 'Dog walks');
  assert.deepEqual(b1.tables.bookings.map((r) => r.dog).sort(), ['Ada', 'Rex']);
  assert.equal(b1.settings.currency, 'EUR');
  assert.equal(b1.settings.secrets, undefined, 'secrets stay on the laptop');
  assert.equal(b1.settings.publish, undefined, 'so does the publish choice');
  assert.equal(Buffer.from(b1.files[0].data, 'base64').toString(), 'a photo');

  // In the cloud: a new app of its own, the same rows, look and photo.
  const first = await importBundle(cloud, b1);
  const c = first.app;
  assert.notEqual(c, '');
  assert.equal(first.n, 1);
  assert.deepEqual(first.created, ['bookings']);
  assert.equal(first.rows, 2);
  const v1 = await cloud.store.resolve(c);
  assert.equal(v1.app.name, 'Dog walks');
  assert.ok(v1.comp.caps.filter((id) => !id.startsWith('lib/')).every((id) => id.startsWith(`${c}/`)), `its own pieces under its own id: ${v1.comp.caps}`);
  const badge = v1.components.find((x) => x.slug === 'walkBadge');
  assert.ok(badge && badge.id !== clash && /h\("b"/.test(badge.code), `the shared piece it pins, as written, beside the other one: ${badge?.id}`);
  assert.deepEqual(v1.views.map((v) => v.slug), ['home']);
  assert.deepEqual((await cloud.client.query(`SELECT dog FROM app_${c}_bookings ORDER BY dog`)).map((r) => r.dog), ['Ada', 'Rex']);
  assert.equal(await cloud.store.setting(c, 'currency'), 'EUR');
  assert.equal(await cloud.store.setting(c, 'secrets'), null);
  assert.equal((await cloud.store.designs(c)).designs[0].name, 'Calm');
  assert.equal(readFileSync(join(cloud.services.filesDir, c, 'photo0000001'), 'utf8'), 'a photo');

  // People use it in the cloud.
  await cloud.client.write([{ op: 'insert', table: `app_${c}_bookings`, doc: { id: 'r3', dog: 'Fido', status: 'booked' } }]);

  // On the laptop, a next version: the table gets a column, the screen changes, an example row is added.
  const spec2 = { name: 'bookings', columns: [...spec1.columns, { name: 'minutes', type: 'int' }] };
  await laptop.store.ensureTable(a, spec2, new Set(await laptop.store.tableNames()));
  const tbl2 = await laptop.store.putCapability({ app: a, slug: 'bookings', kind: 'table', title: 'bookings', spec: spec2, code: '' });
  await laptop.client.write([{ op: 'insert', table: `app_${a}_bookings`, doc: { id: 'r9', dog: 'Example', status: 'booked', minutes: 30 } }]);
  await laptop.store.writeComposition(a, [tbl2, view1, comp], 'minutes', 'how long');
  const b2 = JSON.parse(JSON.stringify(await bundleApp(laptop, a)));

  const second = await importBundle(cloud, b2, { into: c });
  assert.equal(second.app, c);
  assert.equal(second.n, 2);
  assert.deepEqual(second.created, []);
  assert.deepEqual(second.added, { bookings: ['minutes'] });
  assert.equal(second.rows, 0, 'a table already there keeps its own rows');
  assert.equal(second.files, 0, 'a file already there is not written again');
  const v2 = await cloud.store.resolve(c);
  assert.ok(v2.comp.caps.includes(v1.views[0].id), 'the screen that did not change is the same piece');
  assert.ok(!v2.comp.caps.includes(v1.tables[0].id), 'the table that changed is a new version of it');
  assert.ok(v2.comp.caps.includes(badge.id), 'and the shared piece it carried before is used again');
  assert.deepEqual((await cloud.client.query(`SELECT dog FROM app_${c}_bookings ORDER BY dog`)).map((r) => r.dog), ['Ada', 'Fido', 'Rex'], 'people\'s rows kept, no example row added');
  await cloud.client.write([{ op: 'insert', table: `app_${c}_bookings`, doc: { id: 'r4', dog: 'Bo', status: 'booked', minutes: 45 } }]);
  assert.equal((await cloud.client.query(`SELECT minutes FROM app_${c}_bookings WHERE id = 'r4'`))[0].minutes, 45);
});

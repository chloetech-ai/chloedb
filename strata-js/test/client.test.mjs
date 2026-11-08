// The client of strata-server, against a running server, and against the
// database in-process: the two answer alike. Skipped when strata-server is
// not built (cargo build --release in strata-server, or STRATA_SERVER).

import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { after, before, describe, test } from "node:test";

import { Client, connect, StrataError } from "../client.js";
import { Database } from "../index.js";
import { BOOKS, book, books, serve, serverBinary, tempDir } from "./helpers.mjs";

describe("Client", { skip: serverBinary() ? false : "strata-server is not built" }, () => {
  let server;
  let client;
  let local;

  before(async () => {
    const dir = tempDir();
    const path = join(dir, "books.strata");
    const db = await Database.create(path, BOOKS);
    await db.insert(books(200));
    await db.close();
    server = await serve(path);
    client = connect(server.url);
    // The same rows in this process, to compare answers with.
    local = await Database.create(join(dir, "local.strata"), BOOKS);
    await local.insert(books(200));
  });

  after(async () => {
    await local?.close();
    await server?.stop();
  });

  test("reads like the database in-process", async () => {
    assert.equal(await client.count(), 200);
    assert.deepEqual(await client.schema(), await local.schema());
    const sql = "SELECT id, title, year, genres, rating FROM books WHERE year >= :y ORDER BY year DESC, id LIMIT 5";
    const [a, b] = [await client.query(sql, { y: 2000 }), await local.query(sql, { y: 2000 })];
    assert.deepEqual(a.columns, b.columns);
    assert.deepEqual(a.rows, b.rows);
    assert.deepEqual(a.keys, b.keys);
    assert.equal(typeof a.tookMs, "number");
    const [s, t] = [await client.search("dragon", { limit: 5, columns: "id" }), await local.search("dragon", { limit: 5, columns: "id" })];
    assert.deepEqual(s.rows, t.rows);
    assert.deepEqual(await client.embed("dragon"), await local.embed("dragon"));
    assert.deepEqual(await client.get("b0003"), await local.get("b0003"));
    assert.equal(await client.get("nope"), null);
    const keys = ["b0001", "nope", "b0002"];
    assert.deepEqual(await client.getMany(keys), await local.getMany(keys));
    const year = (await local.get("b0001")).year;
    assert.deepEqual(await client.getMany(keys, { columns: ["year"] }), [{ year }, null, { year: (await local.get("b0002")).year }]);
  });

  test("writes", async () => {
    assert.equal(await client.upsert([{ ...book(1), title: "new title" }]), 1);
    assert.equal((await client.get("b0001")).title, "new title");
    assert.equal(await client.insert([{ ...book(500) }]), 1);
    await assert.rejects(client.insert(book(500)), (e) => e instanceof StrataError && e.status >= 400);
    assert.equal(await client.sql("UPDATE books SET rating = 4.5 WHERE id = 'b0500'"), 1);
    assert.equal(await client.put("b0501", { ...book(501), id: "b0501" }), 1);
    assert.equal(await client.write([{ op: "add", key: "b0501", field: "year", value: 1 }]), 1);
    assert.equal((await client.get("b0501")).year, book(501).year + 1);
    assert.equal(await client.delete(["b0500", "b0501"]), 2);
    // Many rows: sent in batches.
    const rows = Array.from({ length: 2500 }, (_, i) => ({ ...book(i), id: `m${i}` }));
    assert.equal(await client.upsert(rows), 2500);
    assert.equal(await client.delete(rows.map((r) => r.id)), 2500);
  });

  test("plans from the database run on the server", async () => {
    const plan = await local.plan("SELECT id FROM books WHERE year < 1903 ORDER BY id");
    const r = await client.run(plan);
    assert.deepEqual(r.rows, (await local.run(plan)).rows);
    const e = await client.explain("SELECT id FROM books LIMIT 1");
    assert.ok("ir" in e);
  });

  test("files in and out", async () => {
    const dir = tempDir();
    const jsonl = join(dir, "rows.jsonl");
    writeFileSync(jsonl, [{ ...book(900), id: "f1" }, { ...book(901), id: "f2", extra: true }].map((r) => JSON.stringify(r)).join("\n"));
    const r = await client.importFile(jsonl);
    assert.equal(r.rows, 2);
    assert.deepEqual(r.skippedFields, ["extra"]);
    // Bytes rather than a path, as a page would send them.
    const csv = new TextEncoder().encode("id,title,year\nf3,from bytes,2003\n");
    assert.equal((await client.importFile(csv, { format: "csv" })).rows, 1);
    assert.equal((await client.get("f3")).year, 2003);

    const sqlite = join(dir, "out.sqlite");
    const out = await client.exportSqlite(sqlite);
    assert.deepEqual(out.tables, ["books"]);
    assert.ok(readFileSync(sqlite).subarray(0, 15).toString() === "SQLite format 3");
    await assert.rejects(client.exportSqlite(sqlite), /exists/);
    const { data } = await client.exportSqlite(null);
    assert.ok(data instanceof Uint8Array && data.length > 0);
    const plan = await client.planSqlite(sqlite);
    assert.equal(plan.tables[0].table, "books");

    const parquet = join(dir, "out.parquet");
    await client.exportParquet(parquet);
    assert.ok(existsSync(parquet));
    const back = await client.importParquet(parquet);
    assert.ok(back.rows > 0);
    assert.equal(await client.delete(["f1", "f2", "f3"]), 3);
  });

  test("streams events once", async () => {
    async function* events(from, to) {
      for (let i = from; i < to; i++) yield { ops: [{ op: "upsert", doc: { ...book(i), id: `s${i}` } }], source: { name: "js-test", offset: i } };
    }
    const first = await client.stream(events(0, 300));
    assert.equal(first.done, true);
    assert.equal(first.written, 300);
    assert.equal(await client.source("js-test"), 299);
    // A replay from before where it got to applies only what is new.
    const again = await client.stream(events(250, 320));
    assert.equal(again.written, 20);
    assert.ok((await client.sources()).some((s) => s.name === "js-test"));
    const ids = Array.from({ length: 320 }, (_, i) => `s${i}`);
    assert.equal(await client.delete(ids), 320);
  });

  test("the rank log", async () => {
    const r = await client.queryLogged("SELECT id FROM books ORDER BY bm25(title, :q) DESC LIMIT 2", { q: "river" }, { context: "u1" });
    assert.equal(typeof r.request, "string");
    const records = await client.exportRankLog();
    assert.ok(records.length >= 2);
  });

  test("errors carry the server's status and answer", async () => {
    await assert.rejects(client.query("SELEC"), (e) => e instanceof StrataError && e.status === 400 && typeof e.answer === "object");
    const nobody = new Client("http://127.0.0.1:1", { timeout: 2000 });
    await assert.rejects(nobody.health(), (e) => !(e instanceof StrataError));
  });

  test("a server behind a token", async () => {
    const dir = tempDir();
    const path = join(dir, "t.strata");
    await (await Database.create(path, BOOKS)).close();
    const s = await serve(path, "--token", "s3cret");
    try {
      await assert.rejects(connect(s.url, { token: "wrong" }).query("SELECT COUNT(*) FROM books"), (e) => e.status === 401);
      assert.equal((await connect(s.url, { token: "s3cret" }).query("SELECT COUNT(*) FROM books")).scalar(), 0);
    } finally {
      await s.stop();
    }
  });
});

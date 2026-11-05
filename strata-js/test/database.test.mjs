// The database in this process: no server.

import assert from "node:assert/strict";
import { existsSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

import { Database, Result, StrataError, fromParquet, fromSqlite, open, planParquet, planSqlite, upgrade, version } from "../index.js";
import { BOOKS, book, books, tempDir } from "./helpers.mjs";

async function booksDb(n = 200) {
  const db = await Database.create(join(tempDir(), "books.strata"), BOOKS);
  await db.insert(books(n));
  return db;
}

test("the addon loads", () => {
  assert.match(version, /^\d+\.\d+\.\d+/);
});

test("create, write, query, close and open again", async () => {
  const path = join(tempDir(), "books.strata");
  const db = await open(path, { schema: BOOKS });
  assert.equal(await db.insert(books(200)), 200);
  assert.equal(await db.count(), 200);
  const s = await db.schema();
  assert.equal(s.table, "books");
  assert.equal(s.key, "id");
  assert.deepEqual(s.fields.map((f) => f.name), ["id", "title", "description", "genres", "year", "rating", "emb"]);
  assert.deepEqual(db.embedErrors, {});

  const r = await db.query("SELECT id, year FROM books WHERE year >= :y ORDER BY year DESC LIMIT 3", { y: 2010 });
  assert.ok(r instanceof Result);
  assert.deepEqual(r.columns, ["id", "year"]);
  assert.equal(r.length, 3);
  assert.deepEqual(r.column("year"), [2019, 2018, 2017]);
  assert.deepEqual([...r], [{ id: "b0119", year: 2019 }, { id: "b0118", year: 2018 }, { id: "b0117", year: 2017 }]);
  assert.deepEqual(r.at(0), { id: "b0119", year: 2019 });
  assert.deepEqual(r.keys, ["b0119", "b0118", "b0117"]);
  assert.equal(r.at(-1).year, 2017);
  assert.equal((await db.query("SELECT COUNT(*) FROM books")).scalar(), 200);
  assert.equal(typeof r.stats, "object");

  assert.equal(await db.sql("UPDATE books SET rating = 5.0 WHERE id = 'b0001'"), 1);
  assert.equal((await db.get("b0001")).rating, 5);
  await db.close();
  assert.ok(db.closed);
  await assert.rejects(db.count(), (e) => e instanceof StrataError && /closed/.test(e.message));

  // It reopens from its file, with its schema and rows.
  const again = await open(path);
  assert.equal(await again.count(), 200);
  assert.equal((await again.get("b0001")).rating, 5);
  await again.close();
});

test("one process at a time has a database open", async () => {
  const path = join(tempDir(), "busy.strata");
  const db = await Database.create(path, BOOKS);
  await assert.rejects(Database.open(path), StrataError);
  await db.close();
  await (await Database.open(path)).close();
});

test("open without a schema needs the file", async () => {
  await assert.rejects(open(join(tempDir(), "missing.strata")), /pass a schema/);
});

test("search: text, vector and hybrid", async () => {
  const db = await booksDb();
  const text = await db.search("dragon", { mode: "text", limit: 5 });
  assert.equal(text.length, 5);
  assert.ok(text.objects().every((o) => `${o.title} ${o.description}`.includes("dragon")));
  assert.ok(text.scores[0] >= text.scores[4]);
  const vector = await db.search("dragon river", { mode: "vector", limit: 3, columns: ["id", "title"] });
  assert.deepEqual(vector.columns, ["id", "title"]);
  assert.equal(vector.length, 3);
  const hybrid = await db.search("dragon", { where: "year > :y", params: { y: 2000 }, limit: 50 });
  assert.ok(hybrid.length > 0);
  for (const o of hybrid) assert.ok(o.year > 2000);
  await db.close();
});

test("upsert, get, getMany, delete and write", async () => {
  const db = await booksDb(10);
  assert.equal(await db.upsert({ ...book(3), title: "changed" }), 1);
  assert.equal((await db.get("b0003")).title, "changed");
  assert.equal(await db.get("nope"), null);
  const many = await db.getMany(["b0001", "nope", "b0002"]);
  assert.deepEqual(many.map((r) => r?.id ?? null), ["b0001", null, "b0002"]);
  await assert.rejects(db.insert(book(1)), StrataError);
  assert.equal(await db.delete("b0001"), 1);
  assert.equal(await db.delete(["b0002", "nope"]), 1);
  assert.equal(await db.count(), 8);
  const n = await db.write([
    { op: "insert", doc: { ...book(100) } },
    { op: "add", key: "b0004", field: "year", value: 1 },
    { op: "set", key: "b0005", field: "rating", value: 1.5 },
    { op: "delete", key: "b0006" },
  ]);
  assert.equal(n, 4);
  assert.equal((await db.get("b0004")).year, book(4).year + 1);
  assert.equal((await db.get("b0005")).rating, 1.5);
  assert.equal(await db.get("b0006"), null);
  await db.close();
});

test("vectors as Float32Array, Float64Array and arrays", async () => {
  const schema = { table: "items", key: "id", fields: [{ name: "id", type: "int" }, { name: "v", type: "vector", dim: 4 }, { name: "tags", type: "keyword", multi: true }] };
  const db = await Database.create(join(tempDir(), "v.strata"), schema);
  const big = new Float32Array([9, 9, 9, 9, 1, 0, 0, 0, 9]);
  const rows = [
    { id: 1, v: new Float32Array([1, 0, 0, 0]), tags: ["a"] },
    { id: 2, v: new Float64Array([0, 1, 0, 0]), tags: ["b"] },
    { id: 3, v: [0, 0, 1, 0] },
    // A view into a larger buffer: its own four numbers, not the buffer's.
    { id: 4, v: big.subarray(4, 8) },
  ];
  assert.equal(await db.insert(rows), 4);
  assert.ok(rows[0].v instanceof Float32Array, "the caller's rows are left as they were");
  const v = await db.query("SELECT id, v FROM items ORDER BY id");
  assert.deepEqual(v.column("v"), [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [1, 0, 0, 0]]);
  assert.deepEqual((await db.get(1)).tags, ["a"]);
  const r = await db.query("SELECT id FROM items ORDER BY similarity(v, :q) DESC LIMIT 2", { q: new Float32Array([0, 1, 0, 0]) });
  assert.equal(r.rows[0][0], 2);
  const like = await db.query("SELECT id FROM items ORDER BY similarity(v, :q) DESC LIMIT 1", { q: { vector_of: 3 } });
  assert.equal(like.rows[0][0], 3);
  await assert.rejects(db.insert({ id: 5, v: new Float32Array([1, 2]) }), StrataError);
  await db.close();
});

test("64-bit integers stay whole", async () => {
  const schema = { table: "t", key: "id", fields: [{ name: "id", type: "int" }, { name: "n", type: "int" }] };
  const db = await Database.create(join(tempDir(), "big.strata"), schema);
  const big = 9007199254740993n; // 2^53 + 1: no number holds it
  await db.insert([{ id: big, n: -(2n ** 62n) }, { id: 1, n: 2 }]);
  const row = await db.get(big);
  assert.equal(row.id, big);
  assert.equal(row.n, -(2n ** 62n));
  const r = await db.query("SELECT id, n FROM t WHERE id IN (:ids) ORDER BY id", { ids: [1, big] });
  assert.deepEqual(r.rows, [[1, 2], [big, -(2n ** 62n)]]);
  assert.equal(await db.delete(big), 1);
  await db.close();
});

test("timestamps, bools and lists", async () => {
  const schema = {
    table: "events",
    key: "id",
    fields: [
      { name: "id", type: "keyword" },
      { name: "at", type: "timestamp" },
      { name: "ok", type: "bool" },
      { name: "tags", type: "keyword", multi: true },
    ],
  };
  const db = await Database.create(join(tempDir(), "e.strata"), schema);
  await db.insert({ id: "e1", at: new Date("2026-10-01T12:30:00Z"), ok: true, tags: new Set(["x", "y"]) });
  const row = await db.get("e1");
  assert.equal(row.at, "2026-10-01T12:30:00Z");
  assert.equal(row.ok, true);
  assert.deepEqual(row.tags, ["x", "y"]);
  const r = await db.query("SELECT id FROM events WHERE at >= :t", { t: new Date("2026-01-01T00:00:00Z") });
  assert.deepEqual(r.rows, [["e1"]]);
  await db.close();
});

test("many calls at once", async () => {
  const db = await booksDb();
  const answers = await Promise.all(Array.from({ length: 64 }, (_, i) => db.query("SELECT COUNT(*) FROM books WHERE year >= :y", { y: 1900 + i })));
  answers.forEach((r, i) => assert.equal(r.scalar(), books(200).filter((b) => b.year >= 1900 + i).length));
  await db.close();
});

test("many writes at once wait for each other and share commits", async () => {
  const db = await booksDb(4);
  // Calls run on the thread pool: none fails as busy, no increment is lost.
  const writes = Array.from({ length: 64 }, (_, i) => [
    db.insert({ ...book(1000 + i) }),
    db.write([{ op: "add", key: `b000${i % 4}`, field: "year", value: 1 }]),
    db.sql("UPDATE books SET rating = rating + 1 WHERE id = 'b0000'"),
  ]).flat();
  await Promise.all(writes);
  assert.equal(await db.count(), 4 + 64);
  const years = await db.query("SELECT SUM(year) FROM books WHERE id IN ('b0000', 'b0001', 'b0002', 'b0003')");
  assert.equal(years.scalar(), [0, 1, 2, 3].reduce((s, k) => s + book(k).year, 0) + 64);
  assert.equal((await db.get("b0000")).rating, book(0).rating + 64);
  await db.close();
});

test("errors are StrataErrors with the database's message", async () => {
  const db = await booksDb(1);
  await assert.rejects(db.query("SELEC nothing"), (e) => e instanceof StrataError && e.message.length > 0);
  await assert.rejects(db.query("SELECT nope FROM books"), StrataError);
  await assert.rejects(db.insert({ id: "x", year: "not a number" }), StrataError);
  await assert.rejects(Database.create(join(tempDir(), "bad.strata"), { table: "t", fields: [{ name: "a", type: "nope" }] }), StrataError);
  await db.close();
});

test("plan and run", async () => {
  const db = await booksDb(20);
  const plan = await db.plan("SELECT id FROM books WHERE year < :y ORDER BY year LIMIT 2", { y: 1905 });
  assert.equal(typeof plan, "object");
  const r = await db.run(plan);
  assert.deepEqual(r.rows, [["b0000"], ["b0001"]]);
  assert.equal(await db.run(await db.plan("DELETE FROM books WHERE year < 1903")), 3);
  await db.close();
});

test("columns change with alter", async () => {
  const db = await booksDb(5);
  await db.addColumn("price", "float", { default: 1.5 });
  assert.equal((await db.get("b0001")).price, 1.5);
  await db.renameColumn("price", "cost");
  assert.equal((await db.get("b0001")).cost, 1.5);
  await db.dropColumn("cost");
  assert.ok(!("cost" in (await db.get("b0001"))));
  await db.close();
});

test("several tables", async () => {
  const db = await booksDb(3);
  await db.sql("CREATE TABLE authors (id KEYWORD PRIMARY KEY, name TEXT)");
  await db.insert([{ id: "a1", name: "Ursula" }], { table: "authors" });
  assert.deepEqual((await db.tables()).map((t) => t.table), ["books", "authors"]);
  assert.equal((await db.schema("authors")).table, "authors");
  assert.equal((await db.get("a1", { table: "authors" })).name, "Ursula");
  const r = await db.search("Ursula", { table: "authors" });
  assert.deepEqual(r.column("id"), ["a1"]);
  await db.close();
});

test("embed a query and read embedErrors", async () => {
  const db = await booksDb(1);
  const v = await db.embed("dragon");
  assert.equal(v.length, 32);
  const missing = await Database.create(join(tempDir(), "m.strata"), { table: "t", key: "id", fields: [{ name: "id", type: "keyword" }, { name: "body", type: "text" }, { name: "emb", type: "vector", dim: 8, embed: { from: ["body"], model: "no-such-model" } }] }, { models: tempDir() });
  assert.ok("emb" in missing.embedErrors);
  await missing.close();
  await db.close();
});

test("files: JSON Lines and CSV in", async () => {
  const dir = tempDir();
  const db = await Database.create(join(dir, "f.strata"), BOOKS);
  const jsonl = join(dir, "rows.jsonl");
  writeFileSync(jsonl, books(30).map((b) => JSON.stringify({ ...b, extra: 1 })).join("\n"));
  const r = await db.importFile(jsonl);
  assert.equal(r.rows, 30);
  assert.equal(r.bulk, true);
  assert.deepEqual(r.skippedFields, ["extra"]);
  const csv = join(dir, "more.csv");
  writeFileSync(csv, "id,title,year\nc1,a csv book,2001\n");
  assert.equal((await db.importFile(csv)).rows, 1);
  assert.equal((await db.get("c1")).year, 2001);
  await db.close();
});

test("SQLite out and in", async () => {
  const dir = tempDir();
  const db = await booksDb(25);
  const sqlite = join(dir, "books.sqlite");
  const out = await db.exportSqlite(sqlite);
  assert.equal(out.tables[0].rows, 25);
  await db.close();
  const plan = await planSqlite(sqlite);
  assert.equal(plan.tables[0].table, "books");
  const copy = await fromSqlite(sqlite, join(dir, "copy.strata"));
  assert.equal(copy.lastImport.tables[0].rows, 25);
  assert.equal(await copy.count(), 25);
  assert.equal((await copy.get("b0007")).title, book(7).title);
  await copy.close();
});

test("Parquet out and in", async () => {
  const dir = tempDir();
  const db = await booksDb(40);
  const file = join(dir, "books.parquet");
  const out = await db.exportParquet(file);
  assert.equal(out.tables[0].rows, 40);
  assert.ok(existsSync(file));
  const plan = await planParquet(file);
  assert.equal(plan.rows, 40);
  const copy = await fromParquet(file, join(dir, "copy.strata"));
  assert.equal(await copy.count(), 40);
  assert.deepEqual((await copy.get("b0009")).genres, book(9).genres);
  // Into a database that has the table: upserted by key.
  const into = await copy.importParquet(file);
  assert.equal(into.rows, 40);
  await copy.close();
  await db.close();
});

test("the rank log", async () => {
  const db = await booksDb(50);
  const r = await db.queryLogged("SELECT id FROM books ORDER BY bm25(title, :q) DESC LIMIT 3", { q: "dragon" }, { context: "u1", variant: "a" });
  assert.equal(r.length, 3);
  assert.equal(typeof r.request, "string");
  await db.sql("UPDATE rank_log SET label = 1 WHERE request = :r AND row_key = :k", { r: r.request, k: r.keys[0] });
  const records = await db.exportRankLog();
  assert.equal(records.length, 3);
  assert.deepEqual(records.map((x) => x.row_key), r.keys);
  assert.deepEqual(records.map((x) => x.label), [1, null, null]);
  assert.deepEqual(records.map((x) => [x.context, x.variant, x.position]), [["u1", "a", 0], ["u1", "a", 1], ["u1", "a", 2]]);
  assert.equal(typeof records[0].features["bm25(title, :q)"], "number");
  await db.close();
});

test("upgrade leaves a current database alone", async () => {
  const path = join(tempDir(), "u.strata");
  await (await Database.create(path, BOOKS)).close();
  assert.equal(await upgrade(path), null);
});

test("await using closes it", async () => {
  const path = join(tempDir(), "using.strata");
  const db = await Database.create(path, BOOKS);
  await db[Symbol.asyncDispose]();
  assert.ok(db.closed);
  await (await Database.open(path)).close();
});

test("optimize leaves one segment and a compact file", async () => {
  // Several bulk segments and two tables merge into one segment each;
  // deleted rows are dropped and the file shrinks to about its live data.
  const path = join(tempDir(), "o.strata");
  const schema = { table: "t", key: "id", fields: [{ name: "id", type: "keyword" }, { name: "body", type: "text" }] };
  const db = await open(path, { schema });
  await db.sql("CREATE TABLE u (id KEYWORD PRIMARY KEY, n INT)");
  for (let b = 0; b < 4; b++) {
    await db.insert(Array.from({ length: 10_000 }, (_, i) => ({ id: `r${b}-${i}`, body: `batch ${b} row ${i} `.repeat(4) })));
    await db.insert(Array.from({ length: 10_000 }, (_, i) => ({ id: `u${b}-${i}`, n: i })), { table: "u" });
  }
  await db.sql("DELETE FROM t WHERE id LIKE 'r2-%' OR id LIKE 'r3-%'");
  const before = statSync(path).size;
  await db.optimize();
  assert.ok(statSync(path).size < before);
  assert.equal(await db.vacuum(), 0);
  assert.equal(await db.count(), 20_000);
  assert.equal((await db.query("SELECT count(*) FROM u")).scalar(), 40_000);
  const { stats } = await db.query("SELECT id FROM t WHERE MATCH(body, 'batch') ORDER BY bm25(body, 'batch') DESC LIMIT 1");
  assert.ok((stats.steps ?? []).every((s) => (s.seg ?? 0) === 0));
  await db.close();
  const again = await Database.open(path);
  assert.equal(await again.count(), 20_000);
  assert.notEqual(await again.get("r1-9999"), null);
  assert.equal(await again.get("r3-0"), null);
  await again.close();
});

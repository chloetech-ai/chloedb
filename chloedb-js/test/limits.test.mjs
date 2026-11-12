// A Database's limits on its queries: a memory budget (the process's by
// default, the database's or a query's own), a deadline (`timeoutMs`, on
// the database or the query), an AbortSignal, and `interrupt()`.

import assert from "node:assert/strict";
import { join } from "node:path";
import process from "node:process";
import { test } from "node:test";
import { setTimeout as sleep } from "node:timers/promises";

import { ChloeDBError, Client, Database, DeadlineError, open } from "../index.js";
import { tempDir } from "./helpers.mjs";

// DataFusion answers SLOW's join at once; ChloeDB's own operators take
// seconds. Read when a database opens.
process.env.CHLOEDB_ENGINE = "operators";

const SCHEMA = {
  table: "t",
  key: "id",
  fields: [
    { name: "id", type: "int" },
    { name: "a", type: "int" },
    { name: "c", type: "int" },
    { name: "note", type: "text" },
  ],
};

// A group per row, and every row with its text: each holds a few MiB.
const HEAVY = ["SELECT a, COUNT(*) FROM t GROUP BY a LIMIT 5", "SELECT * FROM t"];

// Every row meets the 8,000 rows of its `c` and none passes the rest of
// the condition: seconds of comparisons, holding next to nothing.
const SLOW = "SELECT x.id, y.id FROM t x JOIN t y ON x.c = y.c AND x.a < y.a - 1000000";

function rows(n) {
  return Array.from({ length: n }, (_, i) => ({ id: i, a: (i * 7919) % n, c: i % 3, note: `${String(i).padStart(8, "0")} `.repeat(20) }));
}

async function made(n, options = {}) {
  const db = await Database.create(join(tempDir(), "t.strata"), SCHEMA, options);
  await db.insert(rows(n));
  return db;
}

test("queries share the process's budget by default, and a query past its own fails naming it", async () => {
  const db = await made(50_000);
  assert.ok(db.memory >= 1 << 20);
  assert.equal(db.timeoutMs, null);
  for (const sql of HEAVY) {
    await db.query(sql);
    for (const run of [() => db.query(sql, null, { memory: 256 << 10 }), () => db.sql(sql, null, { memory: 256 << 10 })]) {
      await assert.rejects(run, (e) => e instanceof ChloeDBError && e.code === "memory" && /this query may hold \(its memory option\)/.test(e.message));
    }
    assert.ok((await db.query(sql, null, { memory: 0 })).length > 0);
  }
  await assert.rejects(db.run(await db.plan(HEAVY[0]), { memory: 256 << 10 }), (e) => e.code === "memory");
  await assert.rejects(db.search("00000001", { memory: 1 }), (e) => e.code === "memory");
  await assert.rejects(db.query("SELECT 1 FROM t", null, { memory: -1 }), RangeError);
  await db.close();
});

test("a database's budget, which its queries share", async () => {
  const db = await made(50_000, { memory: 256 << 10 });
  assert.equal(db.memory, 256 << 10);
  await assert.rejects(db.query(HEAVY[0]), (e) => e.code === "memory" && /this database's queries may hold/.test(e.message));
  assert.equal((await db.query(HEAVY[0], null, { memory: 0 })).length, 5);
  db.setLimits({ memory: 0 });
  assert.equal(db.memory, null);
  assert.equal((await db.query(HEAVY[0])).length, 5);
  db.setLimits();
  assert.ok(db.memory > 256 << 10);
  await db.close();
});

test("a deadline stops a slow query", async () => {
  const db = await made(24_000, { memory: 0 });
  for (const run of [() => db.query(SLOW, null, { timeoutMs: 100 }), () => db.sql(SLOW, null, { timeoutMs: 100 }), async () => db.run(await db.plan(SLOW), { timeoutMs: 100 })]) {
    const t0 = performance.now();
    await assert.rejects(run, (e) => e instanceof DeadlineError && e.code === "timeout" && /timeoutMs=100/.test(e.message));
    assert.ok(performance.now() - t0 < 5000);
  }
  db.setLimits({ timeoutMs: 100, memory: 0 });
  assert.equal(db.timeoutMs, 100);
  await assert.rejects(db.query(SLOW), /timeoutMs=100/);
  await assert.rejects(db.query(SLOW, null, { timeoutMs: 150 }), /timeoutMs=150/);
  assert.deepEqual((await db.query("SELECT COUNT(*) FROM t")).rows, [[24_000]]);
  assert.ok((await db.search("00000001", { timeoutMs: 60_000 })).length > 0);
  await db.close();
  const again = await open(join(tempDir(), "o.strata"), { schema: SCHEMA, timeoutMs: 1234, memory: 1 << 30 });
  assert.deepEqual([again.timeoutMs, again.memory], [1234, 1 << 30]);
  await again.close();
});

test("an AbortSignal and interrupt() stop the queries running", async () => {
  const db = await made(24_000, { memory: 0 });
  // A signal aborted while the query runs: rejected with its reason.
  const c = new AbortController();
  const t0 = performance.now();
  const q = db.query(SLOW, null, { signal: c.signal });
  await sleep(100);
  c.abort();
  await assert.rejects(q, (e) => e.name === "AbortError");
  assert.ok(performance.now() - t0 < 5000);
  // One already aborted does not run.
  await assert.rejects(db.query("SELECT COUNT(*) FROM t", null, { signal: AbortSignal.abort() }), (e) => e.name === "AbortError");
  // A signal that never aborts changes nothing.
  assert.deepEqual((await db.query("SELECT COUNT(*) FROM t", null, { signal: new AbortController().signal })).rows, [[24_000]]);

  // interrupt() stops every query running now.
  assert.equal(db.interrupt(), 0);
  const running = [db.query(SLOW), db.sql(SLOW)].map((p) => p.then(() => null, (e) => e));
  let n = 0;
  const t1 = performance.now();
  while (n < 2 && performance.now() - t1 < 10_000) {
    await sleep(50);
    n += db.interrupt();
  }
  assert.equal(n, 2);
  for (const e of await Promise.all(running)) {
    assert.ok(e instanceof DeadlineError && /interrupt/.test(e.message), String(e));
  }
  assert.deepEqual((await db.query("SELECT COUNT(*) FROM t")).rows, [[24_000]]);
  await db.close();
});

test("a client's search takes no memory", async () => {
  await assert.rejects(new Client("http://127.0.0.1:9").search("x", { memory: 1 }), TypeError);
});

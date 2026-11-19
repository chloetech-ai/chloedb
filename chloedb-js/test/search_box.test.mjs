// The persona study's fourth round (19 November 2026), from JavaScript.
//
// MAYA-403: a call that reads or writes one table and names none, on a
// database or a server of several tables, used the first one. It now throws
// and names the tables; a database of one table needs no name.
//
// MAYA-405: better-sqlite3's `?` and node-postgres's `$1` with an array of
// values throw a TypeError that shows the call with names.
//
// The server's tests are skipped when chloedb-server is not built (cargo
// build --release in chloedb-server, or CHLOEDB_SERVER).

import assert from "node:assert/strict";
import { join } from "node:path";
import { describe, test } from "node:test";

import { ChloeDBError, connect } from "../client.js";
import { Database } from "../index.js";
import { serve, serverBinary, tempDir } from "./helpers.mjs";

const RECIPES = { table: "recipes", key: "id", fields: [{ name: "id", type: "keyword" }, { name: "name", type: "text" }] };
const ROWS = [
  { id: "r1", name: "lasagne al forno" },
  { id: "r2", name: "lasagna with spinach" },
  { id: "r5", name: "tomato soup" },
];

async function oneTable(path) {
  const db = await Database.create(path, RECIPES);
  await db.insert(ROWS);
  return db;
}

async function twoTables(path) {
  const db = await oneTable(path);
  await db.sql("CREATE TABLE notes (id KEYWORD PRIMARY KEY, body TEXT)");
  await db.insert([{ id: "n1", body: "buy lasagna sheets" }], { table: "notes" });
  return db;
}

async function refuses(db, holder) {
  const calls = {
    search: () => db.search("lasagna"),
    insert: () => db.insert({ id: "x", name: "x" }),
    upsert: () => db.upsert({ id: "x", name: "x" }),
    delete: () => db.delete(["r1"]),
    get: () => db.get("r1"),
    getMany: () => db.getMany(["r1"]),
  };
  for (const [call, run] of Object.entries(calls)) {
    await assert.rejects(run(), (e) => {
      assert.ok(e instanceof ChloeDBError, `${call}: ${e}`);
      assert.match(e.message, new RegExp(`^${call} names no table, and this ${holder} has 2: recipes, notes\\. Pass table, as in ${call}\\(\\.\\.\\., \\{ table: "notes" \\}\\)$`));
      return true;
    });
  }
  // Named, each reads its own.
  assert.deepEqual((await db.search("lasagna", { table: "notes", mode: "text" })).keys, ["n1"]);
  assert.equal((await db.get("r1", { table: "recipes" })).id, "r1");
}

async function teaches(db) {
  for (const [sql, given] of [
    ["SELECT id FROM recipes WHERE MATCH(name, ?)", ["soup"]],
    ["SELECT id FROM recipes WHERE id = $1 OR id = $2", ["r1", "r5"]],
  ]) {
    await assert.rejects(db.query(sql, given), (e) => {
      assert.ok(e instanceof TypeError, String(e));
      assert.match(e.message, /named in ChloeDB's SQL \(:name\)/);
      assert.match(e.message, /better-sqlite3's \? and node-postgres's \$1/);
      assert.match(e.message, /:p1/);
      assert.ok(e.message.includes(`p1: ${JSON.stringify(given[0])}`), e.message);
      return true;
    });
  }
  await assert.rejects(db.sql("SELECT id FROM recipes WHERE id = ?", ["r1"]), /TypeError: .* sql\("SELECT id FROM recipes WHERE id = :p1"/);
  assert.deepEqual((await db.query("SELECT id FROM recipes WHERE MATCH(name, :q)", { q: "soup" })).keys, ["r5"]);
  // No values: the engine's error says to name it.
  await assert.rejects(db.query("SELECT id FROM recipes WHERE MATCH(name, ?)"), (e) => {
    assert.match(e.message, /by position/);
    assert.match(e.message, /:q/);
    return true;
  });
}

test("one table needs no name, in this process", async () => {
  const db = await oneTable(join(tempDir(), "one.chloedb"));
  try {
    assert.equal((await db.get("r1")).id, "r1");
    assert.ok((await db.search("tomato", { mode: "text" })).keys.includes("r5"));
  } finally {
    await db.close();
  }
});

test("several tables and none named throw, in this process", async () => {
  const db = await twoTables(join(tempDir(), "two.chloedb"));
  try {
    await refuses(db, "database");
    await assert.rejects(db.addColumn("n", "int"), /addColumn names no table/);
  } finally {
    await db.close();
  }
});

test("a placeholder by position teaches, in this process", async () => {
  const db = await oneTable(join(tempDir(), "one.chloedb"));
  try {
    await teaches(db);
  } finally {
    await db.close();
  }
});

describe("through a server", { skip: serverBinary() ? false : "chloedb-server is not built" }, () => {
  test("several tables and none named throw", async () => {
    const path = join(tempDir(), "two.chloedb");
    await (await twoTables(path)).close();
    const s = await serve(path);
    try {
      const c = connect(s.url);
      await refuses(c, "server");
      // A table made through the client is seen at once.
      await c.sql("CREATE TABLE more (id KEYWORD PRIMARY KEY, body TEXT)");
      await assert.rejects(c.search("soup"), /has 3: recipes, notes, more/);
    } finally {
      await s.stop();
    }
  });

  test("one table needs no name, and a placeholder by position teaches", async () => {
    const path = join(tempDir(), "one.chloedb");
    await (await oneTable(path)).close();
    const s = await serve(path);
    try {
      const c = connect(s.url);
      assert.equal((await c.get("r1")).id, "r1");
      await teaches(c);
    } finally {
      await s.stop();
    }
  });
});

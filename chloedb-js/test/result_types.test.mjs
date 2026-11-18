// Round 3 of the persona study (18 November 2026): an answer gives each
// column's type from its values, whatever the column is named (SAM-301),
// a UNION of an INT and a FLOAT is FLOAT (SAM-308), and a vector
// parameter is the field the query ranks it against (PRIYA-303), in this
// process and through a server.

import assert from "node:assert/strict";
import { join } from "node:path";
import { after, before, describe, test } from "node:test";

import { connect } from "../client.js";
import { Database } from "../index.js";
import { serve, serverBinary, tempDir } from "./helpers.mjs";

const INV = { table: "inv", key: "id", fields: [{ name: "id", type: "int" }, { name: "cust", type: "int" }, { name: "ts", type: "timestamp" }] };
const DOCS = {
  table: "docs",
  key: "id",
  fields: [
    { name: "id", type: "keyword" },
    { name: "title", type: "text" },
    { name: "emb", type: "vector", embed: { from: ["title"], model: "hash:32" } },
    { name: "bge", type: "vector", dim: 4 },
  ],
};

async function fill(dir) {
  const db = await Database.create(join(dir, "t.chloedb"), INV);
  await db.sql("CREATE TABLE cust (id INT PRIMARY KEY, name KEYWORD)");
  await db.sql("INSERT INTO inv (id, cust, ts) VALUES (1, 10, '2009-01-01T00:00:00Z'), (2, 11, NULL)");
  await db.sql("INSERT INTO cust (id, name) VALUES (10, 'Ana'), (11, 'Sam')");
  await db.sql("CREATE TABLE docs (id KEYWORD PRIMARY KEY, title TEXT, emb VECTOR(32) OPTIONS(embed = 'title', model = 'hash:32'), bge VECTOR(4))");
  await db.sql("INSERT INTO docs (id, title, bge) VALUES ('a', 'vitamin d', '[1,0,0,0]'), ('b', 'masks', '[0,1,0,0]')");
  return db;
}

async function answersAsTyped(db) {
  for (const sql of [
    "SELECT ts AS d FROM inv ORDER BY id LIMIT 1",
    "SELECT MAX(ts) FROM inv",
    "SELECT i.ts FROM inv i JOIN cust c ON c.id = i.cust ORDER BY i.id LIMIT 1",
    "SELECT date_trunc('month', ts) AS m FROM inv ORDER BY id LIMIT 1",
  ]) {
    assert.deepEqual((await db.query(sql)).types, ["timestamp"], sql);
  }
  assert.deepEqual((await db.query("SELECT id, ts FROM inv WHERE id = 2")).types, ["int", null]);
  const u = await db.query("SELECT 1 AS v UNION ALL SELECT 2.5");
  assert.deepEqual([u.rows, u.types], [[[1], [2.5]], ["float"]]);
  const q = "SELECT id FROM docs ORDER BY similarity(bge, :v) DESC LIMIT 2";
  assert.deepEqual((await db.query(q, { v: { vector_of: "b" } })).rows, [["b"], ["a"]]);
  await assert.rejects(db.query(q, { v: { embed: "vitamin d" } }), /bge (has no model|is embedded by no model)/);
}

test("an answer types each column by its values, in this process", async () => {
  const db = await fill(tempDir());
  try {
    await answersAsTyped(db);
  } finally {
    await db.close();
  }
});

describe("through a server", { skip: serverBinary() ? false : "chloedb-server is not built" }, () => {
  let server;
  before(async () => {
    const dir = tempDir();
    await (await fill(dir)).close();
    server = await serve(join(dir, "t.chloedb"));
  });
  after(async () => {
    await server?.stop();
  });
  test("an answer types each column by its values", async () => {
    await answersAsTyped(connect(server.url));
  });
});

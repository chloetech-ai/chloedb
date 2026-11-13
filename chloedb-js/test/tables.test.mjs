// A table named is read alone: `search(text, {table})` and `schema(table)`
// never read the first table (a key limited to other tables may not), and a
// statement's `{vector_of}` and `{embed}` parameters read its FROM table.
// The client's tests are skipped when chloedb-server is not built (cargo
// build --release in chloedb-server, or CHLOEDB_SERVER).

import assert from "node:assert/strict";
import { join } from "node:path";
import { describe, test } from "node:test";

import { connect } from "../client.js";
import { Database } from "../index.js";
import { serve, serverBinary, tempDir } from "./helpers.mjs";

const FIRST = { table: "products", key: "id", fields: [{ name: "id", type: "keyword" }, { name: "body", type: "text" }] };
const NOTES = [
  { id: "a", body: "rivers and mountains", vec: [1, 0, 0] },
  { id: "b", body: "the open sea", vec: [0, 1, 0] },
  { id: "c", body: "a mountain lake", vec: [0.9, 0.1, 0] },
];

async function twoTables(path) {
  const db = await Database.create(path, FIRST);
  await db.insert([{ id: "p1", body: "a waterproof jacket" }]);
  await db.sql("CREATE TABLE mine_notes (id KEYWORD PRIMARY KEY, body TEXT, vec VECTOR(3), emb VECTOR OPTIONS(embed = 'body', model = 'hash:16'))");
  await db.insert(NOTES, { table: "mine_notes" });
  return db;
}

test("vector_of and embed read the statement's FROM table", async () => {
  const db = await twoTables(join(tempDir(), "two.chloedb"));
  try {
    const r = await db.query("SELECT id FROM mine_notes ORDER BY similarity(vec, :v) DESC LIMIT 2", { v: { vector_of: "a" } });
    assert.deepEqual(r.keys, ["a", "c"]);
    const e = await db.query("SELECT id FROM mine_notes ORDER BY similarity(emb, :v) DESC LIMIT 1", { v: { embed: "the open sea" } });
    assert.deepEqual(e.keys, ["b"]);
  } finally {
    await db.close();
  }
});

describe("a key limited to a table", { skip: serverBinary() ? false : "chloedb-server is not built" }, () => {
  test("searches it without reading the first table", async () => {
    const path = join(tempDir(), "served.chloedb");
    await (await twoTables(path)).close();
    const s = await serve(path, "--token", "root");
    try {
      const r = await fetch(s.url + "/v1/admin/keys", {
        method: "POST",
        headers: { authorization: "Bearer root", "content-type": "application/json" },
        body: JSON.stringify({ name: "mine", tables: { "mine_*": "admin" } }),
      });
      const { secret } = await r.json();
      const c = connect(s.url, { token: secret });
      await assert.rejects(c.query("SELECT id FROM products"), (e) => e.status === 403);
      const hits = await c.search("mountain", { table: "mine_notes", mode: "text" });
      assert.deepEqual([...hits.keys].sort(), ["a", "c"]);
      assert.equal((await c.schema("mine_notes")).table, "mine_notes");
      // The hash model ranks no meaning; the call embeds by that table's model.
      const v = await c.search("sea", { table: "mine_notes", mode: "vector" });
      assert.ok(v.keys.length > 0 && v.keys.every((k) => ["a", "b", "c"].includes(k)));
    } finally {
      await s.stop();
    }
  });
});

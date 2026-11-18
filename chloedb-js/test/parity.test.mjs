// Highlighting, "did you mean", cursors and the result window answer
// alike from the database in this process and from chloedb-server, so a
// search page's code tested on one runs on the other (DEV-3, DEV-205 and
// DEV-206 of the persona study of 17 November 2026: highlighting and
// "did you mean" were server routes only, cursors had no method, and an
// OFFSET past 10,000 rows worked in the process and failed on a server).
// The server's half is skipped when chloedb-server is not built.

import assert from "node:assert/strict";
import { existsSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { after, before, describe, test } from "node:test";

import { BadRequestError, ChloeDBError, Database, NotFoundError, connect, open } from "../index.js";
import { serve, serverBinary, tempDir } from "./helpers.mjs";

const PRODUCTS = {
  table: "products",
  key: "id",
  fields: [
    { name: "id", type: "keyword" },
    { name: "name", type: "text" },
    { name: "description", type: "text" },
    { name: "price", type: "float" },
  ],
};

const KINDS = ["Leather sofa", "Leather armchair", "Oak table", "Wool rug", "Velvet sofa bed", "Glass lamp"];

/** More rows than a query answers at once (10,000). */
const N = 12_050;

function product(i) {
  return {
    id: `p${String(i).padStart(5, "0")}`,
    name: `${KINDS[i % KINDS.length]} ${i}`,
    description: `a ${KINDS[(i * 7) % KINDS.length].toLowerCase()} for the living room`,
    price: Math.round(((i * 37) % 900) + 10) + 0.5,
  };
}

const ROWS = Array.from({ length: N }, (_, i) => product(i));

/** Every page of a cursor, read with `next`. */
async function pages(db, sql, pageSize) {
  const first = await db.query(sql, null, { pageSize });
  const out = [first];
  let cursor = first.cursor;
  while (cursor) {
    const p = await db.next(cursor);
    out.push(p);
    assert.ok(p.cursor === null || p.cursor === cursor, "a cursor keeps its name");
    cursor = p.cursor;
  }
  return out;
}

const ORDERED = "SELECT id, price FROM products ORDER BY price, id";

/** The checks both sides pass, `db` a Database or a Client. */
function parity(name, getDb) {
  describe(name, () => {
    test("highlight marks what a query matches, as the field reads it", async () => {
      const db = getDb();
      const texts = ["Leather sofa 1", "Oak table 2", "A leather sofas corner"];
      const marked = await db.highlight("lether so*", texts, { field: "name" });
      assert.equal(marked[0], "<mark>Leather</mark> <mark>sofa</mark> 1");
      assert.equal(marked[1], "Oak table 2");
      assert.match(marked[2], /<mark>leather<\/mark> <mark>sofas<\/mark>/);
      assert.equal(await db.highlight("oak", "Oak table <b>", { field: "name" }), "<mark>Oak</mark> table &lt;b&gt;");
      assert.equal(await db.highlight("oak", "Oak table", { field: "name", pre: "[", post: "]" }), "[Oak] table");
      assert.equal(await db.highlight("sofa", [null], { field: "name" }).then((t) => t[0]), "");
      // Two text fields: the field is named.
      await assert.rejects(() => db.highlight("sofa", ["x"]), (e) => e instanceof ChloeDBError && /name the field/.test(e.message) && /name, description/.test(e.message));
    });

    test("correct answers \"did you mean\"", async () => {
      const db = getDb();
      const c = await db.correct("lether sofa", { field: "name" });
      assert.equal(c.changed, true);
      assert.equal(c.text, "leather sofa");
      assert.equal(c.words.length, 1);
      assert.deepEqual([c.words[0].word, c.words[0].toWord, c.words[0].edits], ["lether", "leather", 1]);
      assert.equal(typeof c.words[0].chance, "number");
      assert.equal("to_word" in c.words[0], false);
      assert.deepEqual(await db.correct("oak table", { field: "name" }), { text: "oak table", changed: false, words: [] });
    });

    test("a cursor pages past the window from one version", async () => {
      const db = getDb();
      // The whole answer at once is past the window, as on a server.
      await assert.rejects(() => db.query(ORDERED), (e) => e instanceof BadRequestError && /the most a query returns/.test(e.message) && /page/.test(e.message));
      await assert.rejects(() => db.query("SELECT id FROM products ORDER BY price LIMIT 10 OFFSET 10000"), (e) => e instanceof BadRequestError && /a query returns at most 10000 rows/.test(e.message));
      const got = await pages(db, ORDERED, 5000);
      assert.deepEqual(got.map((p) => p.length), [5000, 5000, 2050]);
      assert.equal(got.at(-1).cursor, null);
      const ids = got.flatMap((p) => p.column("id"));
      assert.equal(new Set(ids).size, N);
      const want = [...ROWS].sort((a, b) => a.price - b.price || (a.id < b.id ? -1 : 1)).map((r) => r.id);
      assert.deepEqual(ids, want);
      // A plain scan pages too, and a page is cut to the window.
      const scan = await db.query("SELECT id FROM products", null, { pageSize: 50_000 });
      assert.equal(scan.length, 10_000);
      assert.equal(typeof scan.cursor, "string");
      assert.equal(await db.closeCursor(scan.cursor), true);
      assert.equal(await db.closeCursor(scan.cursor), false);
      await assert.rejects(() => db.next(scan.cursor), (e) => e instanceof NotFoundError && /no such cursor/.test(e.message));
    });

    test("scroll reads every page, and closes its cursor when left early", async () => {
      const db = getDb();
      const ids = [];
      for await (const page of db.scroll("SELECT id FROM products WHERE price < :p", { p: 500 }, { pageSize: 1000 })) ids.push(...page.column("id"));
      assert.equal(ids.length, ROWS.filter((r) => r.price < 500).length);
      let seen = 0;
      for await (const page of db.scroll(ORDERED, null, { pageSize: 100 })) {
        seen += page.length;
        if (seen >= 300) break;
      }
      assert.equal(seen, 300);
    });

    test("execute answers the rows a write wrote", async () => {
      const db = getDb();
      assert.equal(await db.execute("UPDATE products SET price = price WHERE id = :id", { id: "p00001" }), 1);
      await assert.rejects(() => db.execute("SELECT id FROM products LIMIT 1"), TypeError);
    });
  });
}

describe("in the process", () => {
  let db;
  before(async () => {
    db = await Database.create(join(tempDir(), "products.chloedb"), PRODUCTS);
    await db.insert(ROWS);
  });
  after(() => db?.close());

  parity("Database", () => db);

  test("a cursor reads the version it started on", async () => {
    const first = await db.query("SELECT id FROM products WHERE price > 900", null, { pageSize: 10 });
    const want = (await db.query("SELECT COUNT(*) FROM products WHERE price > 900")).scalar();
    const gone = ROWS.filter((r) => r.price > 900);
    assert.ok(want > 10 && want === gone.length);
    await db.delete(gone.map((r) => r.id));
    assert.equal((await db.query("SELECT COUNT(*) FROM products WHERE price > 900")).scalar(), 0);
    let n = first.length;
    for (let c = first.cursor; c; ) {
      const p = await db.next(c);
      n += p.length;
      c = p.cursor;
    }
    assert.equal(n, want);
    await db.upsert(gone);
  });

  test("the window names the remedies in the process, and maxResultWindow moves it", async () => {
    await assert.rejects(() => db.query("SELECT id FROM products LIMIT 10 OFFSET 10000"), (e) => /db\.scroll/.test(e.message) && /maxResultWindow: 0/.test(e.message));
    assert.equal(db.maxResultWindow, 10_000);
    db.setLimits({ maxResultWindow: 0 });
    assert.equal(db.maxResultWindow, null);
    assert.equal((await db.query("SELECT id FROM products LIMIT 10 OFFSET 10000")).length, 10);
    assert.equal((await db.query(ORDERED)).length, N);
    db.setLimits({ maxResultWindow: 20 });
    await assert.rejects(() => db.query("SELECT id FROM products LIMIT 21"), BadRequestError);
    // A cursor's page is cut to the window.
    assert.equal((await db.query("SELECT id FROM products", null, { pageSize: 100 })).length, 20);
    db.setLimits({});
    assert.equal(db.maxResultWindow, 10_000);
    const lifted = await open(join(tempDir(), "lifted.chloedb"), { schema: PRODUCTS, maxResultWindow: 0 });
    try {
      assert.equal(lifted.maxResultWindow, null);
    } finally {
      await lifted.close();
    }
  });

  test("synonyms => 1 without a list names the remedy in the process", async () => {
    await assert.rejects(() => db.query("SELECT COUNT(*) FROM products WHERE MATCH(name, 'couch', synonyms => 1)"), (e) => /CREATE SYNONYMS ON products AS/.test(e.message));
  });
});

describe(":memory:", () => {
  test("is a database of its own each time, gone when it closes", async () => {
    const cwd = process.cwd();
    const before = new Set(readdirSync(cwd));
    const a = await open(":memory:", { schema: PRODUCTS });
    const b = await Database.create(":memory:", PRODUCTS);
    try {
      await a.insert(ROWS.slice(0, 3));
      assert.equal(await a.count(), 3);
      assert.equal(await b.count(), 0);
    } finally {
      await a.close();
      await b.close();
    }
    const c = await open(":memory:", { schema: PRODUCTS });
    assert.equal(await c.count(), 0, "nothing kept from before");
    await c.close();
    assert.deepEqual(readdirSync(cwd).filter((f) => !before.has(f)), [], "no files in the working directory");
    assert.equal(existsSync(join(cwd, ":memory:")), false);
    await assert.rejects(() => open(":memory:"), /a new database each time/);
    await assert.rejects(() => Database.open(":memory:"), /a new database each time/);
  });
});

describe("from chloedb-server", { skip: serverBinary() ? false : "chloedb-server is not built" }, () => {
  let server;
  let client;
  before(async () => {
    const path = join(tempDir(), "products.chloedb");
    const db = await Database.create(path, PRODUCTS);
    await db.insert(ROWS);
    await db.close();
    server = await serve(path);
    client = connect(server.url);
  });
  after(() => server?.stop());

  parity("Client", () => client);

  test("a statement refused is told the route that checks it", async () => {
    await assert.rejects(
      () => client.query("SELECT id FROM products WHERE price > 'cheap'"),
      (e) => e instanceof BadRequestError && e.answer?.error_code === "query_invalid" && /POST \/v1\/sql\/check/.test(e.answer.hint) && !/search_docs/.test(e.answer.hint),
    );
    await assert.rejects(() => client.query("SELECT id FROM products LIMIT 10 OFFSET 10000"), (e) => /page_size/.test(e.answer?.hint ?? ""));
  });

  test("the same pages as the database in the process", async () => {
    const local = await Database.create(join(tempDir(), "same.chloedb"), PRODUCTS);
    try {
      await local.insert(ROWS);
      const [a, b] = [await pages(client, ORDERED, 3000), await pages(local, ORDERED, 3000)];
      assert.deepEqual(a.map((p) => p.rows), b.map((p) => p.rows));
      const names = ROWS.slice(0, 20).map((r) => r.name);
      assert.deepEqual(await client.highlight("leather sof*", names, { field: "name", size: 0 }), await local.highlight("leather sof*", names, { field: "name", size: 0 }));
      assert.deepEqual(await client.correct("lether armchiar", { field: "name" }), await local.correct("lether armchiar", { field: "name" }));
    } finally {
      await local.close();
    }
  });
});

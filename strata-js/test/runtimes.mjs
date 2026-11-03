// One script for every runtime that loads the addon: Node, Bun and Deno
// (`node test/runtimes.mjs`, `bun test/runtimes.mjs`, `deno run
// --allow-ffi --allow-read --allow-write --allow-env --allow-net
// --allow-run --allow-sys test/runtimes.mjs`). It opens a file, writes,
// searches, closes and opens it again, then serves it with strata-server
// (when it is built) and reads it through the client. It fails by
// throwing; `node --test` runs the thorough tests.

import assert from "node:assert/strict";
import { join } from "node:path";

import { Database, connect, open, version } from "../index.js";
import { BOOKS, books, serve, serverBinary, tempDir } from "./helpers.mjs";

const runtime = globalThis.Bun ? `Bun ${Bun.version}` : globalThis.Deno ? `Deno ${Deno.version.deno}` : `Node ${process.version}`;
const path = join(tempDir(), "books.strata");

const db = await open(path, { schema: BOOKS });
assert.equal(await db.insert(books(100)), 100);
await db.upsert({ id: "big", title: "a big key", year: 2000, rating: 1, genres: [], description: "" });
const hits = await db.search("dragon", { limit: 3 });
assert.equal(hits.length, 3);
const v = new Float32Array(await db.embed("dragon"));
const near = await db.query("SELECT id FROM books ORDER BY similarity(emb, :v) DESC LIMIT 1", { v });
assert.equal(near.length, 1);
const counts = await Promise.all([1900, 1950, 2000].map((y) => db.query("SELECT COUNT(*) FROM books WHERE year >= :y", { y })));
assert.deepEqual(counts.map((r) => r.scalar()), [101, 51, 1]);
await db.close();

const again = await Database.open(path);
assert.equal(await again.count(), 101);
await again.close();

let served = "skipped (strata-server is not built)";
if (serverBinary()) {
  const s = await serve(path);
  try {
    const client = connect(s.url);
    assert.equal(await client.count(), 101);
    assert.equal((await client.search("dragon", { limit: 3 })).length, 3);
    served = "read through the client";
  } finally {
    await s.stop();
  }
}

console.log(`${runtime}: addon ${version}, a file written, searched and reopened; the server ${served}`);

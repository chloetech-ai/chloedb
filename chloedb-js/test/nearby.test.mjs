// A device's slice of a table on chloedb-server (`openNearby`): answers on
// the device what the server would, asks the server when it cannot prove
// that or the table changed, and reports its counts. Skipped when
// chloedb-server is not built (cargo build --release in chloedb-server, or
// STRATA_SERVER).

import assert from "node:assert/strict";
import { join } from "node:path";
import { after, before, describe, test } from "node:test";

import { connect } from "../client.js";
import { Database, StrataError, openNearby } from "../index.js";
import { serve, serverBinary, tempDir } from "./helpers.mjs";

const PLACES = {
  table: "places",
  key: "id",
  fields: [
    { name: "id", type: "int" },
    { name: "body", type: "text" },
    { name: "lat", type: "float" },
    { name: "lon", type: "float" },
  ],
};
const WORDS = ["pizza", "coffee", "bagel", "pharmacy", "park", "deli", "sushi", "bar"];
const place = (i) => ({
  id: i,
  body: `${WORDS[i % 8]} ${WORDS[(i * 7 + 3) % 8]}`,
  lat: 40.7 + ((i * 37) % 200) * 0.0003 - 0.03,
  lon: -74.0 + ((i * 53) % 200) * 0.0004 - 0.04,
});

async function call(url, path, body) {
  const r = await fetch(url + path, body === undefined ? {} : { method: "POST", headers: { "content-type": "application/json" }, body: typeof body === "string" ? body : JSON.stringify(body) });
  assert.ok(r.ok, `${path}: ${r.status} ${await r.clone().text()}`);
  return r.json();
}

describe("openNearby", { skip: serverBinary() ? false : "chloedb-server is not built" }, () => {
  let server;
  let dir;

  before(async () => {
    dir = tempDir();
    const path = join(dir, "places.strata");
    const db = await Database.create(path, PLACES);
    await db.insert(Array.from({ length: 2000 }, (_, i) => place(i)));
    await db.close();
    server = await serve(path, "--slices", join(dir, "slices"));
    const built = await call(server.url, "/v1/admin/slices", { text: "body", lat: "lat", lon: "lon", lat0: 40.7, lon0: -74.0 });
    assert.equal(built.rows, 2000);
  });

  after(async () => {
    await server?.stop();
  });

  test("answers on the device what the server would", async () => {
    const near = await openNearby(server.url, "places", join(dir, "device-a"), { reach: 1, reportEvery: 0, device: "test-phone" });
    const moved = await near.moveTo(40.7, -74.0);
    assert.equal(moved.fetched, 9);
    assert.ok(near.held().reduce((n, t) => n + t[2], 0) > 0);
    let onDevice = 0;
    for (const q of ["pizza", "coffee bar", "sushi deli"]) {
      const r = await near.search(q, { k: 5, d0: 300 });
      assert.equal(r.length, 5);
      if (r.where !== "device") continue;
      onDevice += 1;
      assert.ok(r.onDevice && r.proved && r.serverMs === null);
      // The server's answer to the same query, row for row.
      const cloud = await call(server.url, "/v1/query", r.ask);
      assert.deepEqual([r.keys, r.scores], [cloud.keys, cloud.scores]);
    }
    assert.ok(onDevice >= 2, `on device ${onDevice}`);
    let r = await near.search("nothing");
    assert.deepEqual([r.where, r.why, r.length], ["device", "no_match", 0]);
    r = await near.search("pizza", { k: 1000 });
    assert.deepEqual([r.where, r.why, r.length], ["server", "short", 500]);
    assert.ok("body" in r.at(0));
    await near.close();
    const usage = (await call(server.url, "/v1/admin/slices")).slices[0].usage;
    assert.ok(usage.searches >= 5 && usage.devices >= 1, JSON.stringify(usage));
  });

  test("asks the server while the table has changed", async () => {
    const near = await openNearby(server.url, "places", join(dir, "device-d"), { reach: 1, reportEvery: 0 });
    await near.moveTo(40.7, -74.0);
    assert.equal((await near.check()).fresh, true);
    const version = near.info.version;
    await connect(server.url).delete([7]);
    assert.equal((await near.check()).fresh, false);
    let r = await near.search("nothing");
    assert.deepEqual([r.where, r.why], ["server", "stale"]);
    await call(server.url, "/v1/admin/slices", { table: "places" });
    assert.equal((await near.check()).fresh, true);
    assert.ok(near.info.version > version);
    assert.equal(near.held().length, 9);
    r = await near.search("nothing");
    assert.deepEqual([r.where, r.why], ["device", "no_match"]);
    await near.close();
    await assert.rejects(near.search("pizza"), StrataError);
  });
});

const TRACKS = {
  table: "tracks",
  key: "id",
  fields: [
    { name: "id", type: "int" },
    { name: "emb", type: "vector", dim: 8, index: "flat" },
  ],
};
// Unit vectors around 12 topics, each a few dimensions.
const track = (i) => {
  const t = i % 12;
  const v = Array.from({ length: 8 }, (_, j) => (0.05 * (((i * 31 + j * 17) % 11) - 5)) / 5);
  v[t % 8] += 1.0;
  v[(t * 3 + 1) % 8] += 0.5 * (Math.floor(t / 8) + 1);
  const n = Math.hypot(...v);
  return { id: i, emb: v.map((x) => x / n) };
};

describe("openNearby on a vector slice", { skip: serverBinary() ? false : "chloedb-server is not built" }, () => {
  let server;
  let dir;

  before(async () => {
    dir = tempDir();
    const path = join(dir, "tracks.strata");
    const db = await Database.create(path, TRACKS);
    await db.insert(Array.from({ length: 1200 }, (_, i) => track(i)));
    await db.close();
    server = await serve(path, "--slices", join(dir, "slices"));
    const built = await call(server.url, "/v1/admin/slices", { table: "tracks", vector: "emb", clusters: 12 });
    assert.deepEqual([built.kind, built.rows, built.clusters], ["vector", 1200, 12]);
  });

  after(async () => {
    await server?.stop();
  });

  test("answers on the device what the server would", async () => {
    const near = await openNearby(server.url, "tracks", join(dir, "device-v"), { reportEvery: 0 });
    assert.deepEqual([near.info.kind, near.info.dim], ["vector", 8]);
    const taste = track(5).emb;
    const got = await near.focus(taste, { rows: 300 });
    const held = near.held().reduce((n, t) => n + t[2], 0);
    assert.ok(got.fetched >= 1 && held > 0 && held <= 450, `${JSON.stringify(got)}, ${held} rows`);
    let r = await near.similar(taste, { k: 5 });
    assert.deepEqual([r.where, r.why, r.edgeM], ["device", "proved", null]);
    const cloud = await call(server.url, "/v1/query", r.ask);
    assert.deepEqual([r.keys, r.scores], [cloud.keys, cloud.scores]);
    // More rows than it holds: the server answers, exactly.
    r = await near.similar(taste, { k: 1000 });
    assert.deepEqual([r.where, r.why, r.length], ["server", "short", 1000]);
    // Away from the taste: the codes name the clusters to fetch, and the
    // device proves its answer once it holds them.
    const far = track(7).emb;
    r = await near.similar(far, { k: 5, refine: 0 });
    assert.deepEqual([r.where, r.why, r.fetched], ["server", "bound", 0]);
    r = await near.similar(far, { k: 5 });
    assert.deepEqual([r.where, r.why], ["device", "proved"]);
    assert.ok(r.fetched >= 1);
    const again = await call(server.url, "/v1/query", r.ask);
    assert.deepEqual([r.keys, r.scores], [again.keys, again.scores]);
    await assert.rejects(near.search("pizza", { lat: 40.7, lon: -74.0 }), /search it with a vector/);
    await near.close();
  });
});

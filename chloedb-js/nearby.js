// Search near a device's user on the device, with a chloedb-server as the
// fallback: `openNearby(url, table, dir)`.
//
// The server builds a table's slice (`POST /v1/admin/slices`, or the
// command center's Slices view): a manifest and the rows of each map tile,
// or of each vector cluster. `Nearby` fetches the manifest, then the tiles
// around its user as the user moves (`moveTo`), or the clusters nearest
// the user's taste (`focus`), into a local database (`dir`). A search runs
// there and scores every row held exactly as the server would. When the
// k-th local score beats the most a row not held could score, the local
// answer is the server's and no request is made; otherwise the same query
// goes to the server (`POST /v1/query`). Each answer says which, and why.
//
// Before it trusts its slice it asks the server whether the slice is the
// current one (`GET /v1/slices/<table>/status`, every `statusEvery` ms): it
// fetches a newer one, and while the table has changed since the build it
// asks the server for every search (unless the slice is under `maxLag` ms
// old). It reports its counts to the server every `reportEvery` searches
// (`POST /v1/slices/<table>/usage`), which the command center shows.
//
//   import { openNearby } from "chloedb";
//
//   const near = await openNearby("http://127.0.0.1:7700", "places", "./slice");
//   await near.moveTo(40.7484, -73.9857);
//   const r = await near.search("pizza", { k: 10 });
//   r.where, r.why;            // "device" or "server", and why
//   for (const row of r) console.log(row.name);
//   await near.close();
//
//   const tracks = await openNearby("http://127.0.0.1:7700", "tracks", "./tracks");
//   await tracks.focus(taste, { rows: 20000 });   // the user's taste, a vector
//   const s = await tracks.similar(taste, { k: 10 });
//
// A vector slice's device also fetches every row's codes (`codes.bin.gz`,
// one byte a dimension), which bound the rows it lacks tightly; when they
// leave its answer in doubt it fetches the few clusters they name, up to
// `refine` bytes, and proves it on the device.

import { Buffer } from "node:buffer";
import process from "node:process";
import { randomUUID } from "node:crypto";

import { Result, ChloeDBError, decode } from "./common.js";

const REASONS = ["proved", "no_match", "short", "bound", "stale"];

function empty() {
  return { searches: 0, why: Object.fromEntries(REASONS.map((w) => [w, 0])), device_ms: 0, cloud_ms: 0 };
}

/** A search's answer, the device's or the server's, and where it was
 * answered: `where` is "device" or "server"; `why` is "proved" or
 * "no_match" (answered on the device), or "short" (the device held fewer
 * than k matching rows), "bound" (a row beyond the tiles or clusters held
 * could have scored higher) or "stale" (the table changed since the slice
 * was built). `ask` is the body of the request (`POST /v1/query`) that
 * asks the server for it. */
export class NearbyResult extends Result {
  get onDevice() {
    return this.where === "device";
  }
}

/** A device's slice of a table on a chloedb-server; see the module. Make
 * one with `openNearby`. */
export class Nearby {
  #native;
  #dev = null;
  #pending = empty();
  #statusAt = 0;

  constructor(native, url, table, dir, { token, reach = 2, timeout = 30_000, statusEvery = 30_000, maxLag = 0, reportEvery = 20, device, codes = true, fetch: f } = {}) {
    this.#native = native;
    this.url = url.replace(/\/+$/, "");
    this.table = table;
    this.dir = dir;
    this.token = token ?? process.env.CHLOEDB_TOKEN ?? process.env.STRATA_TOKEN;
    this.reach = reach;
    this.timeout = timeout;
    this.statusEvery = statusEvery;
    this.maxLag = maxLag;
    this.reportEvery = reportEvery;
    this.codes = codes;
    this.device = device ?? `js-${randomUUID().slice(0, 12)}`;
    this.fetch = f ?? globalThis.fetch;
    this.position = null;
    this.taste = null;
    this.info = null;
    this.stats = { searches: 0, onDevice: 0, fetchedTiles: 0, fetchedBytes: 0 };
  }

  // -------------------------------------------------------------- HTTP

  async #request(method, path, body) {
    const headers = body === undefined ? {} : { "content-type": "application/json" };
    if (this.token) headers.authorization = `Bearer ${this.token}`;
    const r = await this.fetch(this.url + path, { method, headers, body, signal: AbortSignal.timeout(this.timeout) });
    const bytes = Buffer.from(await r.arrayBuffer());
    if (!r.ok) {
      let text = bytes.toString();
      try {
        text = JSON.parse(text).error ?? text;
      } catch {}
      throw new ChloeDBError(`${method} ${path}: ${r.status}: ${text}`, { status: r.status });
    }
    return bytes;
  }

  get #d() {
    if (!this.#dev) throw new ChloeDBError("the slice is closed");
    return this.#dev;
  }

  #slice(rel) {
    return this.#request("GET", `/v1/slices/${encodeURIComponent(this.table)}/${rel}`);
  }

  // -------------------------------------------------------------- the slice

  /** Fetches the manifest and opens the device's database (`openNearby`
   * calls it). */
  async load() {
    const text = (await this.#slice("manifest.json")).toString();
    this.stats.fetchedBytes += text.length;
    this.#dev?.close();
    this.#dev = await this.#native.NativeSlice.create(this.dir, text);
    this.info = JSON.parse(this.#dev.info());
    if (this.codes && this.info.codes_bytes) {
      const data = await this.#slice("codes.bin.gz");
      await this.#dev.addCodes(data);
      this.stats.fetchedBytes += data.length;
    }
    this.#statusAt = Date.now();
    if (this.position) await this.moveTo(...this.position);
    if (this.taste) await this.focus(...this.taste);
    return this;
  }

  /** Asks the server whether the slice is the current one: fetches a newer
   * one (and the tiles around the user again), and marks the device stale
   * while the table has changed since the build. Answers the server's
   * status. */
  async check() {
    let st = JSON.parse((await this.#slice("status")).toString());
    this.#statusAt = Date.now();
    if (st.version !== this.info.version) {
      await this.load();
      st = JSON.parse((await this.#slice("status")).toString());
    }
    const lag = Date.now() - (this.info.built_ms ?? 0);
    this.#d.setStale(!st.fresh && lag > this.maxLag);
    return st;
  }

  /** Moves the user: fetches the tiles of the square around it the device
   * lacks (nearest first) and drops those far behind. Answers what it did:
   * {fetched, dropped, bytes}. */
  async moveTo(lat, lon, { reach } = {}) {
    if (reach !== undefined) this.reach = reach;
    this.position = [lat, lon];
    return this.#apply(JSON.parse(this.#d.plan(lat, lon, this.reach)));
  }

  /** For a vector slice: holds the clusters whose rows could score highest
   * for `taste` (a vector, such as the user's history summed), as many as
   * fit in `rows` rows, fetching those it lacks (best first) and dropping
   * those that fell well behind. Answers what it did: {fetched, dropped,
   * bytes}. */
  async focus(taste, { rows = 20_000 } = {}) {
    taste = Array.from(taste, Number);
    this.taste = [taste, { rows }];
    return this.#apply(JSON.parse(this.#d.planVector(taste, rows)));
  }

  async #apply(plan) {
    for (const [x, y] of plan.drop) await this.#d.dropTile(x, y);
    let bytes = 0;
    for (const [x, y, path] of plan.fetch) {
      const data = await this.#slice(path);
      await this.#d.addTile(x, y, data);
      bytes += data.length;
    }
    this.stats.fetchedTiles += plan.fetch.length;
    this.stats.fetchedBytes += bytes;
    return { fetched: plan.fetch.length, dropped: plan.drop.length, bytes };
  }

  /** The tiles held, as [x, y, rows]. */
  held() {
    return JSON.parse(this.#d.held());
  }

  // -------------------------------------------------------------- searching

  /** The `k` best places for `q` near the user (or near `lat`, `lon`),
   * ranked by BM25 faded by distance (a place `d0` metres away counts
   * half): the device's answer when it can prove it is the server's, else
   * the server's. */
  async search(q, { k = 10, d0 = 1000, lat, lon } = {}) {
    if (lat === undefined || lon === undefined) {
      if (!this.position) throw new ChloeDBError("moveTo(lat, lon) first, or pass lat and lon");
      [lat, lon] = this.position;
    }
    return this.#answer(() => this.#d.search(q, lat, lon, k, d0));
  }

  /** For a vector slice: the `k` rows most similar to `vector` (by dot
   * product, as `similarity` ranks them): the device's answer when it can
   * prove it is the server's, else the server's, which re-scores every row
   * (an exact search). When the device's codes name clusters that could
   * still beat its k-th, it fetches them if they come to at most `refine`
   * bytes (0: never), and then proves its answer. */
  async similar(vector, { k = 10, refine = 2_000_000 } = {}) {
    vector = Array.from(vector, Number);
    return this.#answer(() => this.#d.searchVector(vector, k), refine);
  }

  async #answer(local, refine = 0) {
    if (this.statusEvery && Date.now() - this.#statusAt >= this.statusEvery) await this.check();
    let t0 = performance.now();
    let a = decode(await local());
    let deviceMs = performance.now() - t0;
    const need = a.need ?? [];
    let fetched = 0;
    if (!a.on_device && a.why === "bound" && need.length && need.reduce((n, t) => n + t[2], 0) <= refine) {
      await this.#apply({ fetch: need.map(([x, y, b]) => [x, y, `tiles/${x}_${y}.jsonl.gz`, b]), drop: [] });
      fetched = need.length;
      t0 = performance.now();
      a = decode(await local());
      deviceMs += performance.now() - t0;
    }
    let r;
    let serverMs = null;
    if (a.on_device) {
      r = new NearbyResult(a);
      r.where = "device";
    } else {
      t0 = performance.now();
      // The body as the device wrote it: its floats stay floats.
      r = new NearbyResult(decode((await this.#request("POST", "/v1/query", a.ask)).toString()));
      serverMs = performance.now() - t0;
      r.where = "server";
    }
    Object.assign(r, { why: a.why, ask: a.ask, proved: a.proved, stale: a.stale, bound: a.bound, edgeM: a.edge_m, deviceMs, serverMs, fetched });
    this.#note(r);
    return r;
  }

  // -------------------------------------------------------------- reporting

  #note(r) {
    this.stats.searches += 1;
    if (r.where === "device") this.stats.onDevice += 1;
    const p = this.#pending;
    p.searches += 1;
    p.why[r.why] += 1;
    p.device_ms += r.deviceMs;
    p.cloud_ms += r.serverMs ?? 0;
    if (this.reportEvery && p.searches >= this.reportEvery) this.report().catch(() => {});
  }

  /** Sends the counts since the last report to the server; they are best
   * effort, so a failure is ignored. */
  async report() {
    const p = this.#pending;
    this.#pending = empty();
    if (!p.searches) return;
    try {
      await this.#request("POST", `/v1/slices/${encodeURIComponent(this.table)}/usage`, JSON.stringify({ device: this.device, ...p }));
    } catch {}
  }

  /** Reports what is left and closes the device's database. */
  async close() {
    await this.report();
    this.#dev?.close();
    this.#dev = null;
  }
}

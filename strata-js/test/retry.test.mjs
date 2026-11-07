// The client sends a request the server refused for its load (429) again,
// after its Retry-After; not a write that failed partway without its key,
// nor once retryFor has passed. No server needed: a fetch that answers.

import assert from "node:assert/strict";
import { test } from "node:test";

import { Client, StrataError } from "../client.js";

function answering(answers, seen) {
  return async (url, init) => {
    seen.push(JSON.parse(init.body));
    const [status, body] = answers.shift();
    const headers = { "content-type": "application/json" };
    if (status === 429) headers["retry-after"] = "0";
    return new Response(JSON.stringify(body), { status, headers });
  };
}

const busy = [429, { error: "the database is behind on flushing writes to disk, so the write was not applied; send it again in a second" }];

test("a request refused for the server's load is sent again", async () => {
  const seen = [];
  const c = new Client("http://strata.invalid", { fetch: answering([busy, busy, [200, { written: 1, took_ms: 1 }]], seen) });
  assert.equal(await c.write([{ op: "upsert", doc: { id: "a" } }]), 1);
  assert.equal(seen.length, 3);
  assert.deepEqual(seen[0], seen[2]);
});

test("a write that failed partway is sent again only with its key", async () => {
  const partial = [429, { error: "busy. shard 0 (1 rows) took the write before it failed, and keep it", partial: true, shards: [] }];
  let seen = [];
  let c = new Client("http://strata.invalid", { fetch: answering([partial], seen) });
  await assert.rejects(c.write([{ op: "upsert", doc: { id: "a" } }]), (e) => e instanceof StrataError && e.status === 429 && e.answer.partial === true);
  assert.equal(seen.length, 1);
  // Under a key the shards that took it do not apply it again.
  seen = [];
  c = new Client("http://strata.invalid", { fetch: answering([partial, [200, { written: 2, took_ms: 1 }]], seen) });
  assert.equal((await c._call("POST", "/v1/write", { ops: [], idempotency_key: "k1" })).written, 2);
  assert.equal(seen.length, 2);
});

test("retryFor 0 sends a request once", async () => {
  const seen = [];
  const c = new Client("http://strata.invalid", { retryFor: 0, fetch: answering([busy], seen) });
  await assert.rejects(c.write([{ op: "upsert", doc: { id: "a" } }]), (e) => e.status === 429);
  assert.equal(seen.length, 1);
});

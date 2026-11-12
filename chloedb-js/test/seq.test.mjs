// Reads that see a write (writeResult, minSeqNo(s), Session) and the error
// codes and classes, against a server and a coordinator over two shards.
// Skipped when chloedb-server is not built. Needs no addon.

import assert from "node:assert/strict";
import { join } from "node:path";
import { after, before, describe, test } from "node:test";

import { BadRequestError, ChloeDBError, ConflictError, NotFoundError, Session, connect, errorCode } from "../client.js";
import { freePort, launch, serve, serverBinary, tempDir } from "./helpers.mjs";

const TABLE = "CREATE TABLE t (id KEYWORD PRIMARY KEY, n INT, title TEXT)";

describe("reads that see a write", { skip: serverBinary() ? false : "chloedb-server is not built" }, () => {
  const servers = [];
  let one;
  let coord;

  before(async () => {
    const dir = tempDir();
    const s = await serve(join(dir, "one.strata"), "--seq-no");
    servers.push(s);
    one = s.url;
    await connect(one).sql(TABLE);
    const grpcs = [];
    for (const i of [0, 1]) {
      const http = await freePort();
      const grpc = await freePort();
      servers.push(await launch(["--db", join(dir, `shard${i}.strata`), "--listen", `127.0.0.1:${http}`, "--grpc", `127.0.0.1:${grpc}`, "--seq-no"], http));
      grpcs.push(`http://127.0.0.1:${grpc}`);
    }
    const port = await freePort();
    const c = await launch(["--shards", grpcs.join(","), "--listen", `127.0.0.1:${port}`, "--grpc", "off"], port);
    servers.push(c);
    coord = c.url;
    for (let i = 0; ; i++) {
      try {
        await connect(coord).sql(TABLE);
        break;
      } catch (e) {
        if (i > 50) throw e;
        await new Promise((ok) => setTimeout(ok, 200));
      }
    }
  });

  after(async () => {
    await Promise.all(servers.map((s) => s.stop()));
  });

  test("writes answer their numbers, and write still answers a number", async () => {
    const c = connect(one);
    const w = await c.writeResult([{ op: "upsert", doc: { id: "w1", n: 1 } }]);
    assert.equal(w.written, 1);
    assert.ok(w.seqNo >= 1);
    assert.equal(w.seqNos, undefined);
    assert.equal(await c.write([{ op: "upsert", doc: { id: "w2", n: 2 } }]), 1);
    assert.equal(await c.sql("UPDATE t SET n = 3 WHERE id = 'w1'"), 1);
  });

  test("reads take minSeqNo", async () => {
    const c = connect(one);
    const w = await c.writeResult([{ op: "upsert", doc: { id: "r1", n: 1, title: "river" } }]);
    assert.deepEqual((await c.query("SELECT n FROM t WHERE id = 'r1'", null, { minSeqNo: w.seqNo })).rows, [[1]]);
    assert.equal((await c.get("r1", { minSeqNo: w.seqNo })).n, 1);
    assert.equal((await c.getMany(["r1"], { minSeqNo: w.seqNo }))[0].n, 1);
    assert.deepEqual((await c.search("river", { textFields: ["title"], minSeqNo: w.seqNo })).keys, ["r1"]);
    const far = w.seqNo + 1000;
    for (const read of [
      () => c.query("SELECT n FROM t", null, { minSeqNo: far }),
      () => c.get("r1", { minSeqNo: far }),
      () => c.getMany(["r1"], { minSeqNo: far }),
      () => c.search("river", { textFields: ["title"], minSeqNo: far }),
      () => c.sql("SELECT n FROM t", null, { minSeqNo: far }),
    ]) {
      await assert.rejects(read, (e) => e instanceof ConflictError && e instanceof ChloeDBError && e.code === "conflict" && e.status === 409);
    }
  });

  test("a session reads its own writes", async () => {
    const s = connect(one).session();
    assert.ok(s instanceof Session);
    assert.equal(s.seqNo, undefined);
    await s.upsert({ id: "s1", n: 1, title: "garden" });
    assert.ok(s.seqNo >= 1);
    assert.equal((await s.get("s1")).n, 1);
    const before = s.seqNo;
    assert.equal(await s.sql("UPDATE t SET n = 2 WHERE id = 's1'"), 1);
    assert.ok(s.seqNo > before);
    assert.deepEqual((await s.sql("SELECT n FROM t WHERE id = 's1'")).rows, [[2]]);
    s.observe({ seqNo: s.seqNo + 1000 });
    for (const read of [() => s.query("SELECT n FROM t"), () => s.get("s1"), () => s.getMany(["s1"]), () => s.search("garden", { textFields: ["title"] })]) {
      await assert.rejects(read, ConflictError);
    }
    assert.equal((await connect(one).get("s1")).n, 2);
  });

  test("a session through a coordinator", async () => {
    const s = connect(coord).session();
    const rows = Array.from({ length: 20 }, (_, i) => ({ id: `c${i}`, n: i }));
    assert.equal(await s.upsert(rows), 20);
    assert.equal(s.seqNo, undefined);
    assert.equal(s.seqNos.length, 2);
    assert.ok(s.seqNos.some(Boolean));
    assert.deepEqual((await s.query("SELECT COUNT(*) AS n FROM t")).rows, [[20]]);
    assert.equal((await s.get("c3")).n, 3);
    s.observe({ seqNos: s.seqNos.map((n) => n + 1000) });
    await assert.rejects(() => s.query("SELECT COUNT(*) FROM t"), (e) => e.code === "conflict");
  });

  test("errors have codes and classes", async () => {
    const c = connect(one);
    await assert.rejects(() => c.query("SELEC nothing"), (e) => e instanceof BadRequestError && e.code === "bad_request" && e.status === 400);
    const w = await c.writeResult([{ op: "upsert", doc: { id: "e1", n: 1 } }]);
    await assert.rejects(() => c.put("e1", { n: 2 }, { ifSeqNo: w.seqNo + 1000 }), ConflictError);
    await assert.rejects(() => c._call("GET", "/v1/no/such/route"), (e) => e instanceof NotFoundError && e.code === "not_found");
  });
});

test("errorCode", () => {
  assert.equal(errorCode("busy: the database is open elsewhere"), "busy");
  assert.equal(errorCode("timeout: past its deadline", 504), "timeout");
  assert.equal(errorCode("x", 429), "overloaded");
  assert.equal(errorCode("x", 409, { partial: true }), "partial_write");
  assert.equal(errorCode("x", 500, { code: "custom" }), "custom");
  assert.equal(errorCode("x", 500), "internal");
  assert.equal(errorCode("x"), "error");
  assert.equal(new ChloeDBError("query error: no").code, "bad_request");
});

// ChloeDB was called Strata: the old names and the new ones both work.

import assert from "node:assert/strict";
import { test } from "node:test";

import * as client from "../client.js";
import * as common from "../common.js";
import * as local from "../index.js";

test("ChloeDBError is StrataError, from every entry point", () => {
  for (const m of [common, client, local]) {
    assert.equal(m.ChloeDBError, common.StrataError);
    assert.equal(m.StrataError, common.StrataError);
  }
  const e = new common.ChloeDBError("x", { status: 400 });
  assert.ok(e instanceof common.StrataError);
  assert.equal(e.status, 400);
});

test("the token is read from $CHLOEDB_TOKEN first, then $STRATA_TOKEN", () => {
  const saved = { CHLOEDB_TOKEN: process.env.CHLOEDB_TOKEN, STRATA_TOKEN: process.env.STRATA_TOKEN };
  try {
    delete process.env.CHLOEDB_TOKEN;
    process.env.STRATA_TOKEN = "old";
    assert.equal(client.connect("http://127.0.0.1:1").token, "old");
    process.env.CHLOEDB_TOKEN = "new";
    assert.equal(client.connect("http://127.0.0.1:1").token, "new");
    assert.equal(client.connect("http://127.0.0.1:1", { token: "given" }).token, "given");
  } finally {
    for (const [k, v] of Object.entries(saved)) {
      if (v === undefined) delete process.env[k];
      else process.env[k] = v;
    }
  }
});

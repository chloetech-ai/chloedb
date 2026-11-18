// The declarations type-check as an app's code checks them: tsc on a
// small typed example (test/types/example.ts) with strict options,
// skipLibCheck off and no Node types, as TypeScript 7 leaves them out
// unless `types` names them (DEV-7, DEV-8 and DEV-207 of the persona study
// of 17 November 2026: every row a cast, `sql()` a union, and
// "Property 'asyncDispose' does not exist" without `"types": ["node"]`).
//
// tsc is $CHLOEDB_TSC, else TypeScript 7.0.2 by npx (fetched from npm's
// registry the first time). The test is skipped only when npx cannot fetch
// it; a type error fails it.

import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const TYPESCRIPT = "typescript@7.0.2";

test("the declarations type-check a typed app without Node's types", (t) => {
  const project = join(here, "types");
  const given = process.env.CHLOEDB_TSC;
  const [cmd, args] = given ? [given, ["-p", project]] : ["npx", ["--yes", "-p", TYPESCRIPT, "tsc", "-p", project]];
  const r = spawnSync(cmd, args, { encoding: "utf8", shell: process.platform === "win32", timeout: 300_000 });
  const out = `${r.stdout ?? ""}${r.stderr ?? ""}`;
  if (r.status !== 0 && !/error TS\d+/.test(out)) {
    if (!given && (r.error || /npm (ERR|error)/.test(out))) {
      t.skip(`npx could not fetch ${TYPESCRIPT}: ${(r.error?.message ?? out).trim().split("\n").at(-1)}`);
      return;
    }
  }
  assert.equal(r.status, 0, `tsc -p test/types:\n${out}`);
});

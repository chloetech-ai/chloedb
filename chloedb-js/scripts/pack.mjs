// Packs the package with its built addon (strata.node) for one platform,
// as a tarball that `npm install` takes with no Rust and no build:
//
//   node scripts/pack.mjs linux-x64-gnu     # dist/chloedb-<version>-linux-x64-gnu.tgz
//
// The platform is <os>-<cpu>[-<libc>], as Node names them: linux-x64-gnu,
// linux-arm64-gnu, darwin-arm64, darwin-x64. The tarball says so (`os`,
// `cpu`, `libc`), so npm refuses it on another machine. It holds the
// JavaScript, the declarations and the addon; not the Rust sources, and no
// install script.

import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const platform = process.argv[2];
const m = /^(linux|darwin|win32)-(x64|arm64)(?:-(gnu|musl))?$/.exec(platform ?? "");
if (!m) {
  console.error("usage: node scripts/pack.mjs <platform>, e.g. linux-x64-gnu, linux-arm64-gnu, darwin-arm64, darwin-x64");
  process.exit(2);
}
const [, os, cpu, libc] = m;
if (!existsSync(join(root, "strata.node"))) {
  console.error("chloedb-js: strata.node is missing: build it first (node scripts/build.mjs)");
  process.exit(1);
}

const files = ["index.js", "index.d.ts", "client.js", "client.d.ts", "common.js", "common.d.ts", "nearby.js", "README.md", "strata.node"];
const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
const stage = mkdtempSync(join(tmpdir(), "strata-js-pack-"));
try {
  for (const f of files) copyFileSync(join(root, f), join(stage, f));
  copyFileSync(join(root, "..", "LICENSE"), join(stage, "LICENSE"));
  const out = { ...pkg, files: [...files, "LICENSE"], os: [os], cpu: [cpu], ...(libc ? { libc: [libc === "gnu" ? "glibc" : "musl"] } : {}) };
  // No build at install: the addon is in the package.
  out.scripts = {};
  writeFileSync(join(stage, "package.json"), JSON.stringify(out, null, 2) + "\n");
  const dist = join(root, "dist");
  mkdirSync(dist, { recursive: true });
  const r = spawnSync("npm", ["pack", "--pack-destination", dist], { cwd: stage, stdio: ["ignore", "pipe", "inherit"], encoding: "utf8" });
  if (r.status !== 0) process.exit(r.status ?? 1);
  const made = r.stdout.trim().split("\n").at(-1);
  const name = `${pkg.name}-${pkg.version}-${platform}.tgz`;
  renameSync(join(dist, made), join(dist, name));
  console.log(join(dist, name));
} finally {
  rmSync(stage, { recursive: true, force: true });
}

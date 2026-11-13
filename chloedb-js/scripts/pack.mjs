// Packs the package for installing with no Rust and no build. Three kinds:
//
//   node scripts/pack.mjs linux-x64-gnu          # dist/chloedb-<version>-linux-x64-gnu.tgz
//   node scripts/pack.mjs --npm linux-x64-gnu    # dist/npm/chloedb-<platform>-<version>.tgz
//   node scripts/pack.mjs --npm-main             # dist/npm/chloedb-<version>.tgz
//
// The first is the whole package with its built addon (chloedb.node) for
// one platform, which a GitHub release carries and `npm install <tarball or
// its URL>` takes. The other two are what npm's registry gets, as napi-rs
// lays it out: a package per platform, `@chloedb/<platform>`, holding the
// addon alone, and the package `chloedb`, the JavaScript with every
// platform's package as an optional dependency, of which npm installs the
// one whose `os`, `cpu` and `libc` match (index.js loads its addon). The
// npm organization `chloedb` must exist for the scope to be published.
//
// The platform is <os>-<cpu>[-<libc>], as Node names them: linux-x64-gnu,
// linux-arm64-gnu, darwin-arm64, darwin-x64. A package of one says so
// (`os`, `cpu`, `libc`), so npm refuses it on another machine. None holds
// the Rust sources or an install script.

import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
/** The platforms npm's registry gets a package of (js-release.yml builds each). */
export const PLATFORMS = ["linux-x64-gnu", "linux-arm64-gnu", "darwin-arm64", "darwin-x64"];
// index.js loads the addon from this scope.
const scope = "@chloedb";
const JS = ["index.js", "index.d.ts", "client.js", "client.d.ts", "common.js", "common.d.ts", "nearby.js", "README.md"];

const args = process.argv.slice(2);
const kind = args.includes("--npm-main") ? "main" : args.includes("--npm") ? "platform" : "release";
const platform = args.find((a) => !a.startsWith("--"));
const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));

function target(p) {
  const m = /^(linux|darwin|win32)-(x64|arm64)(?:-(gnu|musl))?$/.exec(p ?? "");
  if (!m) {
    console.error("usage: node scripts/pack.mjs [--npm] <platform>, e.g. linux-x64-gnu, linux-arm64-gnu, darwin-arm64, darwin-x64; or --npm-main");
    process.exit(2);
  }
  const [, os, cpu, libc] = m;
  return { os: [os], cpu: [cpu], ...(libc ? { libc: [libc === "gnu" ? "glibc" : "musl"] } : {}) };
}

function needAddon() {
  if (!existsSync(join(root, "chloedb.node"))) {
    console.error("chloedb-js: chloedb.node is missing: build it first (node scripts/build.mjs)");
    process.exit(1);
  }
}

/** `npm pack` of `files` (from here) with this package.json, as dist/<out>/<name>. */
function pack(files, manifest, outDir, name, extra = {}) {
  const stage = mkdtempSync(join(tmpdir(), "chloedb-js-pack-"));
  try {
    for (const f of files) copyFileSync(join(root, f), join(stage, f));
    copyFileSync(join(root, "..", "LICENSE"), join(stage, "LICENSE"));
    for (const [f, text] of Object.entries(extra)) writeFileSync(join(stage, f), text);
    writeFileSync(join(stage, "package.json"), JSON.stringify({ ...manifest, files: [...files, ...Object.keys(extra), "LICENSE"] }, null, 2) + "\n");
    const dist = join(root, ...outDir);
    mkdirSync(dist, { recursive: true });
    const r = spawnSync("npm", ["pack", "--pack-destination", dist], { cwd: stage, stdio: ["ignore", "pipe", "inherit"], encoding: "utf8" });
    if (r.status !== 0) process.exit(r.status ?? 1);
    const made = r.stdout.trim().split("\n").at(-1);
    renameSync(join(dist, made), join(dist, name));
    console.log(join(dist, name));
  } finally {
    rmSync(stage, { recursive: true, force: true });
  }
}

const common = { version: pkg.version, license: pkg.license, repository: pkg.repository, homepage: "https://chloetech-ai.github.io/chloedb/site/docs/javascript.html" };
const { private: _private, scripts: _scripts, ...published } = pkg;

if (kind === "release") {
  const t = target(platform);
  needAddon();
  // No build at install: the addon is in the package.
  pack([...JS, "chloedb.node"], { ...pkg, ...t, scripts: {} }, ["dist"], `${pkg.name}-${pkg.version}-${platform}.tgz`);
} else if (kind === "platform") {
  const t = target(platform);
  needAddon();
  const manifest = { name: `${scope}/${platform}`, ...common, description: `ChloeDB's native addon for ${platform}, which the package chloedb loads`, main: "chloedb.node", ...t, publishConfig: { access: "public" } };
  // Its README says what it is; the package's own is chloedb's.
  const readme = `# ${scope}/${platform}\n\nThe native addon of [chloedb](https://www.npmjs.com/package/chloedb) for ${platform}. Install \`chloedb\`, which depends on it; npm picks the package of your platform.\n`;
  pack(["chloedb.node"], manifest, ["dist", "npm"], `chloedb-${platform}-${pkg.version}.tgz`, { "README.md": readme });
} else {
  const optionalDependencies = Object.fromEntries(PLATFORMS.map((p) => [`${scope}/${p}`, pkg.version]));
  pack(JS, { ...published, scripts: {}, optionalDependencies, publishConfig: { access: "public" } }, ["dist", "npm"], `chloedb-${pkg.version}.tgz`);
}

// Builds the native addon with cargo and puts it beside index.js as
// strata.node, which index.js loads.
//
//   node scripts/build.mjs                 # release (link-time optimised)
//   node scripts/build.mjs --profile ci    # CI's: optimised, built faster
//   node scripts/build.mjs --features bert # more of Strata's features
//   node scripts/build.mjs --target aarch64-apple-darwin   # for another target
//   node scripts/build.mjs --if-missing    # only when strata.node is missing (npm install)
//
// It needs Rust (https://rustup.rs) and a C compiler; cargo's target
// directory is $CARGO_TARGET_DIR, else target/ here.

import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const out = join(root, "strata.node");
const args = process.argv.slice(2);

function option(name, fallback) {
  const i = args.indexOf(name);
  return i >= 0 && i + 1 < args.length ? args[i + 1] : fallback;
}

if (args.includes("--if-missing") && existsSync(out)) {
  process.exit(0);
}

const profile = option("--profile", "release");
const features = option("--features", "");
const triple = option("--target", "");
const cargo = process.env.CARGO ?? "cargo";
const build = ["build", "--profile", profile, ...(features ? ["--features", features] : []), ...(triple ? ["--target", triple] : [])];
console.error(`strata-js: cargo ${build.join(" ")}`);
const r = spawnSync(cargo, build, { cwd: root, stdio: "inherit" });
if (r.error) {
  console.error(`strata-js: cannot run cargo (${r.error.message}); install Rust from https://rustup.rs`);
  process.exit(1);
}
if (r.status !== 0) process.exit(r.status ?? 1);

const target = process.env.CARGO_TARGET_DIR ? resolve(process.env.CARGO_TARGET_DIR) : join(root, "target");
const dir = join(target, ...(triple ? [triple] : []), profile === "dev" ? "debug" : profile);
const os = triple ? (triple.includes("apple") ? "darwin" : triple.includes("windows") ? "win32" : "linux") : process.platform;
const lib = { darwin: "libstrata_node.dylib", win32: "strata_node.dll" }[os] ?? "libstrata_node.so";
copyFileSync(join(dir, lib), out);
console.error(`strata-js: built ${out}`);

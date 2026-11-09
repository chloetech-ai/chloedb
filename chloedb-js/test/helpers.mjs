// What the tests share: a schema of books with an embedded vector field
// (the `hash:32` model, which needs no download), rows of it, temporary
// directories, and a chloedb-server to run against.

import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

export const BOOKS = {
  table: "books",
  key: "id",
  fields: [
    { name: "id", type: "keyword" },
    { name: "title", type: "text" },
    { name: "description", type: "text" },
    { name: "genres", type: "keyword", multi: true },
    { name: "year", type: "int" },
    { name: "rating", type: "float" },
    { name: "emb", type: "vector", embed: { from: ["title", "description"], model: "hash:32" } },
  ],
};

const WORDS = "river mountain dragon detective ship garden war letter city winter star secret".split(" ");

export function book(i) {
  const w = WORDS;
  return {
    id: `b${String(i).padStart(4, "0")}`,
    title: `${w[i % 12]} ${w[(i * 7) % 12]}`,
    description: Array.from({ length: 8 }, (_, j) => w[(i + j) % 12]).join(" "),
    genres: [i % 2 ? "fantasy" : "mystery", i % 5 === 0 ? "classic" : "new"],
    year: 1900 + (i % 120),
    rating: Math.round((1 + (i % 40) / 10) * 10) / 10,
  };
}

export function books(n) {
  return Array.from({ length: n }, (_, i) => book(i));
}

const made = [];
process.on("exit", () => made.forEach((d) => rmSync(d, { recursive: true, force: true })));

/** A new temporary directory, removed when the process exits. */
export function tempDir() {
  const d = mkdtempSync(join(tmpdir(), "strata-js-"));
  made.push(d);
  return d;
}

const here = dirname(fileURLToPath(import.meta.url));

/** chloedb-server: $CHLOEDB_SERVER, else the release build of this checkout,
 * else null (the client tests are skipped). */
export function serverBinary() {
  const p = process.env.CHLOEDB_SERVER || join(here, "..", "..", "chloedb-server", "target", "release", "chloedb-server");
  return existsSync(p) ? p : null;
}

function freePort() {
  return new Promise((ok, fail) => {
    const s = createServer();
    s.listen(0, "127.0.0.1", () => {
      const { port } = s.address();
      s.close(() => ok(port));
    });
    s.on("error", fail);
  });
}

/** Starts chloedb-server on the database at `db`; resolves to {url, stop}. */
export async function serve(db, ...args) {
  const binary = serverBinary();
  const port = await freePort();
  const p = spawn(binary, ["--db", db, "--listen", `127.0.0.1:${port}`, "--grpc", "off", ...args], { stdio: ["ignore", "ignore", "pipe"] });
  let stderr = "";
  p.stderr.on("data", (d) => (stderr += d));
  const url = `http://127.0.0.1:${port}`;
  const stop = () =>
    new Promise((ok) => {
      if (p.exitCode !== null) return ok();
      p.once("exit", ok);
      p.kill("SIGTERM");
    });
  for (let i = 0; i < 300; i++) {
    if (p.exitCode !== null) throw new Error(`chloedb-server exited: ${stderr}`);
    try {
      await fetch(url + "/v1/health");
      return { url, stop };
    } catch {
      await new Promise((r) => setTimeout(r, 100));
    }
  }
  await stop();
  throw new Error(`chloedb-server did not start: ${stderr}`);
}

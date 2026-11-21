// The extension end to end in Chromium (Playwright): `chloectx me serve` on
// a store of its own, the extension loaded and set up through its options
// page, and a copy of each site's page (served in place of the real site)
// whose conversation must arrive in the store, once.
//
//   CHLOECTX_BIN=../target/ci/chloectx node test.mjs
import { createRequire } from "node:module";
import { spawn, execFileSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
// Playwright from this folder, or the one installed for every project.
const { chromium } = createRequire(join(here, "x.js"))("playwright");
const bin = process.env.CHLOECTX_BIN || join(here, "../target/release/chloectx");
const home = mkdtempSync(join(tmpdir(), "chloectx-ext-"));
const env = { ...process.env, CHLOECTX_HOME: home, CHLOECTX_EMBEDDER: "hash:64" };
const port = 7700 + Math.floor(Math.random() * 90);

const pages = {
  "https://chatgpt.com/c/6a1f0e2c-aaaa-bbbb": `<title>Weekly meal plan - ChatGPT</title><main>
    <div data-message-author-role="user"><div>I'm vegetarian and I hate mushrooms.</div></div>
    <div data-message-author-role="assistant"><div><p>Lentil curry on Monday.</p></div></div></main>`,
  "https://claude.ai/chat/0c9b8e5a-cccc-dddd": `<title>Running plan - Claude</title><div>
    <div data-testid="user-message"><p>I run 30 km a week.</p></div>
    <div class="font-claude-response"><div class="font-claude-response"><p>Here is a 16-week plan.</p></div></div></div>`,
  "https://gemini.google.com/app/abc123def456": `<title>Gemini</title><div>
    <user-query><div class="query-text">What should I pack for Iceland in March?</div></user-query>
    <model-response><message-content>Layers and a waterproof shell.</message-content></model-response></div>`,
};

function chloectx(...args) {
  return execFileSync(bin, args, { env, encoding: "utf8" });
}

const server = spawn(bin, ["me", "serve", "--port", String(port)], { env });
let out = "";
const key = await new Promise((resolve, reject) => {
  const t = setTimeout(() => reject(new Error("chloectx me serve did not start: " + out)), 30000);
  server.stdout.on("data", (d) => {
    out += d;
    const m = out.match(/Capture key[^:]*: (ctxc_\S+)/);
    if (m && out.includes("API proxy")) {
      clearTimeout(t);
      resolve(m[1]);
    }
  });
});

const profile = mkdtempSync(join(tmpdir(), "chloectx-ext-profile-"));
const ctx = await chromium.launchPersistentContext(profile, {
  headless: true,
  // The new headless mode, which loads extensions.
  channel: "chromium",
  args: [`--disable-extensions-except=${here}`, `--load-extension=${here}`],
});
let failed = false;
try {
  let [sw] = ctx.serviceWorkers();
  if (!sw) sw = await ctx.waitForEvent("serviceworker");
  const id = new URL(sw.url()).host;
  const opt = await ctx.newPage();
  await opt.goto(`chrome-extension://${id}/options.html`);
  await opt.fill("#endpoint", `http://127.0.0.1:${port}`);
  await opt.fill("#key", "wrong");
  await opt.click("#save");
  await opt.click("#test");
  await opt.waitForFunction(() => document.getElementById("msg").textContent.includes("capture key is wrong"));
  await opt.fill("#key", key);
  for (const s of ["chatgpt", "claude", "gemini"]) await opt.check("#site-" + s);
  await opt.click("#save");
  await opt.click("#test");
  await opt.waitForFunction(() => document.getElementById("msg").textContent === "Connected.");

  await ctx.route(/https:\/\/(chatgpt\.com|claude\.ai|gemini\.google\.com)\/.*/, (route) => {
    const url = route.request().url().split("?")[0];
    const body = pages[url];
    return body ? route.fulfill({ status: 200, contentType: "text/html", body: `<!doctype html><html><body>${body}</body></html>` }) : route.fulfill({ status: 404, body: "" });
  });
  for (const url of Object.keys(pages)) {
    const p = await ctx.newPage();
    await p.goto(url);
    await p.waitForTimeout(4500);
    // Settling again sends nothing new.
    await p.evaluate(() => document.body.appendChild(document.createElement("span")));
    await p.waitForTimeout(4000);
    await p.close();
  }
  await opt.reload();
  const status = await opt.evaluate(() => chrome.storage.local.get({ status: {} }).then((o) => o.status));
  console.log(JSON.stringify(status));
  const me = JSON.parse(chloectx("me", "--json"));
  const by = Object.fromEntries(me.by_source.map((s) => [s.source, s.messages]));
  console.log(JSON.stringify(by));
  for (const s of ["chatgpt", "claude", "gemini"]) {
    if (by[s] !== 2) {
      console.error(`FAIL: ${s} kept ${by[s]} messages, not 2`);
      failed = true;
    }
  }
  const found = chloectx("--me", "search", "Iceland waterproof");
  if (!found.includes("Layers and a waterproof shell")) {
    console.error("FAIL: the Gemini reply is not found:\n" + found);
    failed = true;
  }
  // A site that is off sends nothing.
  await opt.uncheck("#site-chatgpt");
  await opt.click("#save");
  pages["https://chatgpt.com/c/ffff0000-1111"] = `<title>Off</title><div data-message-author-role="user">This must not be kept.</div>`;
  const p = await ctx.newPage();
  await p.goto("https://chatgpt.com/c/ffff0000-1111");
  await p.waitForTimeout(4500);
  if (chloectx("--me", "search", "must not be kept").includes("must not be kept")) {
    console.error("FAIL: a site turned off was captured");
    failed = true;
  }
} finally {
  await ctx.close();
  server.kill();
  try {
    chloectx("stop", "--all");
  } catch {}
  rmSync(profile, { recursive: true, force: true });
  rmSync(home, { recursive: true, force: true });
}
console.log(failed ? "FAILED" : "ok: the extension's captures arrive, once, from the sites turned on only");
process.exit(failed ? 1 : 0);

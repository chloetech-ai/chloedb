// Grace's Monday numbers by clicking, in ChloeDB's command center, driven in
// Chromium by Playwright as she would: load the Zendesk export on the Import
// page, then build each number in the Query builder, read it off the Answer
// table, save the weekly chart as a team view and open it at a phone's width.
// Every click, pick and typed value is counted. run.py starts the server and
// compares what this reads with SQL.
//
//   node click.mjs URL tickets.csv OUT_DIR
//
// Writes OUT_DIR/clicks.json and screenshots beside it.

import { createRequire } from "node:module";
import fs from "node:fs";
import path from "node:path";

const { chromium, devices } = createRequire(import.meta.url)("playwright");
const [base, csv, outDir] = process.argv.slice(2);
const out = { steps: [], numbers: {}, errors: [] };
let actions = 0;
const shot = (p, name) => p.screenshot({ path: path.join(outDir, name) });

const browser = await chromium.launch();
const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
const p = await ctx.newPage();
p.on("pageerror", (e) => out.errors.push(e.message));

// What a person does, counted: a click, a pick from a list, a value typed.
async function act(what, fn) { actions += 1; await fn(); await p.waitForTimeout(150); return what; }
const click = (loc) => act("click", () => loc.click());
const pick = (loc, label) => act("pick", () => loc.selectOption({ label }));
const type = (loc, text) => act("type", () => loc.fill(text));
const field = async (name, item) => {
  await click(p.locator(`.bld-fields .fi:has(span.grow:text-is("${name}"))`).first());
  await click(p.locator(".menu").getByText(item, { exact: true }).first());
};
async function answer() {
  // the builder runs as you build: wait for the answer to settle
  await p.waitForTimeout(800);
  await p.locator(".bld-out:not(.loading) table").first().waitFor({ timeout: 30000 });
  await p.waitForTimeout(400);
  return p.locator(".bld-out table").first().evaluate((t) => {
    const head = [...t.querySelectorAll("thead th")].map((th) => th.innerText.trim().toLowerCase()).filter((x) => x !== "#");
    const rows = [...t.querySelectorAll("tbody tr")].map((tr) => [...tr.querySelectorAll("td")].map((td) => td.innerText.trim()));
    return { head, rows: rows.map((r) => r.slice(r.length - head.length)) };
  });
}
const sql = () => p.locator(".bld-sql").innerText();
function step(name, started, extra = {}) {
  out.steps.push({ name, actions: actions - started, ...extra });
}

// ------------------------------------------------------------ the import
await p.goto(`${base}/#/import`);
await p.waitForLoadState("networkidle");
let a0 = actions;
await act("pick a file", () => p.locator("input[type=file]").first().setInputFiles(csv));
await p.getByText("split at").first().waitFor();
const tagsSplit = await p.locator("select[aria-label='Split tags at']").evaluate((s) => s.options[s.selectedIndex].text).catch(() => null);
await click(p.getByRole("button", { name: "Load the rows" }));
await p.getByText(/rows written to tickets/).waitFor({ timeout: 120000 });
step("Load the export (tags split at spaces, as the page offered)", a0, { tags_split_at: tagsSplit, said: (await p.getByText(/rows written to tickets/).innerText()) });

// ------------------------------------------------------------ the builder
await p.goto(`${base}/#/builder`);
await p.waitForLoadState("networkidle");
await click(p.locator(".bld-tables").getByRole("button", { name: /tickets/ }).first());

// A: tickets each week by priority, its chart a line per priority, saved for the team
a0 = actions;
await field("created_at", "Group by week");
await field("priority", "Group by it");
out.numbers.per_week = await answer();
out.numbers.per_week.sql = await sql();
await click(p.getByRole("button", { name: "Chart", exact: true }));
await p.waitForTimeout(500);
const lines = await p.locator(".bld-out select[aria-label='A line or bar for each value of']").evaluate((s) => s.value).catch(() => null);
await shot(p, "builder-chart.png");
await click(p.getByRole("button", { name: "Save as a view" }));
await type(p.locator(".modal input").first(), "Monday numbers");
await click(p.getByRole("button", { name: /Save the view|Replace the view/ }));
await p.waitForFunction(() => location.hash.includes("view="), null, { timeout: 15000 });
const viewHash = await p.evaluate(() => location.hash);
// the link the page copies when it saves (views.js viewLink): the view's own page, which a phone opens
const viewId = decodeURIComponent(viewHash.split("view=")[1]);
const viewUrl = `${base}/#/views/${encodeURIComponent(viewId)}`;
step("Tickets each week by priority, as a chart, saved as a team view", a0, { chart_lines_per: lines, view: viewId, link: viewUrl.replace(base, "") });

// B: median hours to solve, each week
a0 = actions;
await click(p.getByRole("button", { name: "Clear", exact: true }));
await field("created_at", "Group by week");
await pick(p.locator(".stage[data-stage=group] .item select").first(), "MEDIAN");
await pick(p.locator(".stage[data-stage=group] select[aria-label='Of']").first(), "hours from created_at to solved_at");
await click(p.getByRole("button", { name: "Table", exact: true }));
out.numbers.median_hours = await answer();
out.numbers.median_hours.sql = await sql();
step("Median hours from created to solved, each week", a0);

// C: the ten most used tags
a0 = actions;
await click(p.getByRole("button", { name: "Clear", exact: true }));
await field("tags", "Group by it");
await type(p.locator(".stage[data-stage=order] input.num").first(), "10");
out.numbers.top_tags = await answer();
out.numbers.top_tags.sql = await sql();
step("The ten most used tags", a0);

// D: satisfaction per agent: of the ratings, the share that are good
a0 = actions;
await click(p.getByRole("button", { name: "Clear", exact: true }));
await field("satisfaction", "Filter on it");
await pick(p.locator(".stage[data-stage=filter] .item select").nth(1), "is one of");
await click(p.locator(".stage[data-stage=filter] .vals .chip", { hasText: /^good/ }).first());
await click(p.locator(".stage[data-stage=filter] .vals .chip", { hasText: /^bad/ }).first());
await field("assignee", "Group by it");
await field("satisfaction", "Group by it");
await pick(p.locator(".stage[data-stage=group] .item select").first(), "share of rows (%)");
await pick(p.locator(".stage[data-stage=group] select[aria-label='A share of']").first(), "within each assignee");
out.numbers.satisfaction = await answer();
out.numbers.satisfaction.sql = await sql();
step("Satisfaction per agent: the share of ratings that are good", a0);

// ------------------------------------------------------------ on a phone
const phone = await browser.newContext({ ...devices["iPhone 13"] });
const q = await phone.newPage();
q.on("pageerror", (e) => out.errors.push("phone: " + e.message));
await q.goto(viewUrl);
await q.waitForLoadState("networkidle");
await q.waitForTimeout(2500);
await q.screenshot({ path: path.join(outDir, "view-phone.png") });
out.phone = {
  device: "iPhone 13",
  shows_chart: await q.locator("svg").count() > 0,
  wider_than_screen: await q.evaluate(() => document.documentElement.scrollWidth > window.innerWidth + 1),
};
// the address the builder's page shows after saving, opened on the same phone: is the chart on the first screen?
await q.goto(`${base}/${viewHash}`);
await q.waitForLoadState("networkidle");
await q.waitForTimeout(2500);
await q.screenshot({ path: path.join(outDir, "builder-address-phone.png") });
const firstScreen = async (pg) => pg.evaluate(() => [...document.querySelectorAll("svg")].some((s) => { const r = s.getBoundingClientRect(); return r.width > 150 && r.top < window.innerHeight && r.bottom > 0; }));
out.phone.builder_address = { link: viewHash, chart_on_first_screen: await firstScreen(q) };
await q.goto(viewUrl);
await q.waitForLoadState("networkidle");
await q.waitForTimeout(2500);
out.phone.chart_on_first_screen = await firstScreen(q);
await phone.close();

out.actions = actions;
fs.writeFileSync(path.join(outDir, "clicks.json"), JSON.stringify(out, null, 1) + "\n");
await browser.close();
console.log(`clicked: ${actions} actions, ${out.errors.length} page errors`);

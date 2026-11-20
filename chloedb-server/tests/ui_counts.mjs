// The counts the command center shows, against SQL and arithmetic on the
// same rows (GRACE-401 to 407, 417): tests/ui.rs starts a server, and this
// writes made-up tickets to it, builds each statement as the views do
// (the Search tab's count and order, the Data tab's CSV of every match,
// the builder's weeks, median and share, NOT IN of two picks, a split
// list, a team view) and checks what the server answers. Run by hand:
// `node tests/ui_counts.mjs http://127.0.0.1:7700` against an empty server.

import assert from "node:assert/strict";

const base = process.argv[2] || process.env.CHLOEDB_URL;
if (!base) { console.log("no server: skipped"); process.exit(0); }
const noop = () => {};
const el = () => ({ style: {}, dataset: {}, classList: { add: noop, remove: noop, toggle: noop }, append: noop, setAttribute: noop, addEventListener: noop, replaceChildren: noop, remove: noop, click: noop, children: [] });
const written = [];
const realFetch = globalThis.fetch;
Object.assign(globalThis, {
  window: globalThis,
  location: { host: "x", origin: "http://x", pathname: "/", hash: "", search: "", href: "http://x/" },
  matchMedia: () => ({ matches: false, addEventListener: noop }),
  sessionStorage: { getItem: () => null, setItem: noop, removeItem: noop },
  localStorage: { getItem: () => null, setItem: noop, removeItem: noop },
  document: { addEventListener: noop, querySelector: () => null, querySelectorAll: () => [], documentElement: el(), body: el(), createElement: el, createElementNS: el, createTextNode: el },
  addEventListener: noop,
  // The page's requests go to the server under test.
  fetch: (u, init) => realFetch(new URL(u, base), init),
  // Export all: the file the browser would save.
  showSaveFilePicker: async () => ({ createWritable: async () => { const parts = []; written.push(parts); return { write: async (t) => { parts.push(t); }, close: async () => {}, abort: noop }; } }),
});
const app = new URL("../ui/app/", import.meta.url);
const { api } = await import(new URL("core.js", app));
const { tables } = await import(new URL("catalog.js", app));
const tableView = await import(new URL("views/table.js", app));
const builder = await import(new URL("views/builder.js", app));
const big = await import(new URL("bigresult.js", app));
const imp = await import(new URL("views/import.js", app));
const views = await import(new URL("views/views.js", app));
// Who the page is: an open server's root, which may make the views' table.
await (await import(new URL("data.js", app))).refreshWho();

const q = (sql, params = {}) => api("/v1/query", { body: { sql, params } });
const one = async (sql, params) => (await q(sql, params)).rows[0][0];

// ---------------------------------------------------------------- the rows
// 600 made-up tickets from a seeded generator: a subject of a few words,
// a status, a priority, a time created over twelve weeks and solved some
// hours after, tags as Zendesk writes them (one value, spaces between),
// and a small vector.
let seed = 7;
const rand = () => ((seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648);
const pick = (xs) => xs[Math.floor(rand() * xs.length)];
const WORDS = ["billing", "refund", "login", "password", "invoice", "shipping", "delay", "account", "upgrade", "export"];
const FILL = ["help", "with", "my", "please", "today", "again", "still", "issue", "question", "about"];
const T0 = Date.UTC(2026, 7, 3); // a Monday
const rows = Array.from({ length: 600 }, (_, i) => {
  const words = Array.from({ length: 2 + Math.floor(rand() * 3) }, () => (rand() < 0.5 ? pick(WORDS) : pick(FILL)));
  const created = T0 + Math.floor(rand() * 84 * 24) * 3600e3 + Math.floor(rand() * 60) * 60e3;
  const hours = Math.floor(rand() * 96 * 60) / 60;
  return {
    id: `t${String(i).padStart(4, "0")}`, subject: words.join(" "), status: pick(["new", "open", "pending", "solved", "closed"]), priority: pick(["low", "normal", "high", "urgent"]),
    created: new Date(created).toISOString(), solved: new Date(created + hours * 3600e3).toISOString(),
    tags: [...new Set(Array.from({ length: 1 + Math.floor(rand() * 3) }, () => pick(["billing", "refund", "web_form", "email", "us", "how_to"])))].join(" "),
    emb: [rand(), rand(), rand(), rand()],
  };
});
// The ticket that says both words, and a vector far from the query's;
// another close to it in meaning that has neither word.
rows[0] = { ...rows[0], subject: "charged twice for billing and no refund", emb: [0, 0, 0, 1] };
rows[1] = { ...rows[1], subject: "money back for a payment", emb: [1, 0, 0, 0] };
const has = (r, w) => r.subject.split(/\s+/).includes(w);

await api("/v1/sql", { body: { sql: "CREATE TABLE tk (id KEYWORD PRIMARY KEY, subject TEXT, status KEYWORD, priority KEYWORD, tags KEYWORD[], created TIMESTAMP, solved TIMESTAMP, emb VECTOR(4))" } });
// GRACE-405: the tags column is seen as a list split at spaces, and the
// CSV rewritten as the server reads a list.
assert.equal(imp.listSeparator(rows.slice(0, 200).map((r) => r.tags)), " ");
const csv = imp.splitLists(`id,tags\n${rows.map((r) => `${r.id},${r.tags}`).join("\n")}\n`, { tags: " " });
const lists = new Map(csv.trim().split("\n").slice(1).map((l) => { const [id, t] = l.split(","); return [id, t ? t.split("|") : []]; }));
await api("/v1/write", { body: { table: "tk", ops: rows.map((r) => ({ op: "upsert", doc: { ...r, tags: lists.get(r.id) } })) } });
await tables(true);
const t = (await tables()).find((x) => x.table === "tk");
assert.ok(t, "the catalog lists tk");
const F = (n) => t.short.fields.find((f) => f.name === n);

// GRACE-405: each tag is counted once a ticket, as the file has it.
{
  const j = await q("SELECT tags, COUNT(*) AS n FROM tk GROUP BY tags ORDER BY n DESC, tags");
  const want = new Map();
  for (const r of rows) for (const g of new Set(r.tags.split(" "))) want.set(g, (want.get(g) || 0) + 1);
  assert.deepEqual(new Map(j.rows.map(([g, n]) => [g, n])), want);
}

// GRACE-401: the count over the words is the rows that have one of them.
{
  const s = tableView.searchSql(t, { q: "billing refund", fields: [F("subject")], vec: F("emb"), mode: "both" });
  const want = rows.filter((r) => has(r, "billing") || has(r, "refund")).length;
  assert.ok(want > 20, `${want} rows have a word`);
  assert.equal(await one(s.exact.sql, s.exact.params), want, "rows with the words as typed");
  assert.ok(await one(s.count.sql, s.count.params) >= want);
  // Pages: the second page starts where the first ended, and none repeats.
  const params = { ...s.params, v: [0.9, 0.1, 0, 0] };
  const p1 = await q(s.sql, params);
  const p2 = await q(tableView.searchSql(t, { q: "billing refund", fields: [F("subject")], vec: F("emb"), mode: "both", offset: 20 }).sql, params);
  const ids = [...p1.rows, ...p2.rows].map((r) => r[0]);
  assert.equal(new Set(ids).size, 40, "two pages of 20, none twice");
  // GRACE-402: in Both, a row with every word comes before one close in
  // meaning that has none; the ticket far in meaning is still first.
  const every = new Set(rows.filter((r) => has(r, "billing") && has(r, "refund")).map((r) => r.id));
  assert.ok(every.has("t0000") && every.size < 20, `${every.size} rows have both words`);
  const firsts = ids.slice(0, every.size);
  assert.deepEqual(new Set(firsts), every, "the rows with every word come first, t0000 among them though its vector is far");
  assert.ok(!ids.includes("t0001") || ids.indexOf("t0001") >= every.size, "the row near in meaning with neither word comes after");
}

// GRACE-403: Export all writes every row the Data tab's filter keeps, a
// page of the scroll at a time, not the 50 on screen.
{
  const where = "status IN ('open', 'new')";
  const want = rows.filter((r) => ["open", "new"].includes(r.status)).length;
  assert.ok(want > 50);
  const n = await big.exportRows(tableView.dataSql(t, [F("id"), F("status")], { where, by: "id" }), {}, "tk.csv", { format: "csv" });
  assert.equal(n, want);
  const lines = written.at(-1).join("").trim().split("\n");
  assert.equal(lines.length, want + 1, "the header and every row");
  assert.equal(lines[0], "id,status");
  const nj = await big.exportRows(tableView.dataSql(t, [F("id")], { where }), {}, "tk.jsonl", { format: "jsonl" });
  assert.equal(nj, want);
  assert.equal(written.at(-1).join("").trim().split("\n").length, want);
}

// GRACE-404: tickets by week and priority, the median hours to solve and
// each priority's share of its week, as arithmetic over the rows.
{
  const st = { table: "tk", signals: [], fusion: "weighted", filters: [], any: false, boosts: [], sort: null, columns: [], score: true, limit: 1000,
    group: { by: ["created", "priority"], bins: { created: "week" }, measures: [{ fn: "count" }, { fn: "median", field: "hours:created:solved" }, { fn: "share", within: "created" }], min: 0 } };
  const j = await q(builder.toSql(st).sql);
  assert.deepEqual(j.columns, ["created_week", "priority", "n", "median_hours_to_solved", "pct_of_created"]);
  const monday = (iso) => { const d = Date.parse(iso), day = Math.floor(d / 86400e3); return (day - ((day + 3) % 7)) * 86400e3; };
  const groups = new Map(), weeks = new Map();
  for (const r of rows) {
    const k = `${monday(r.created)} ${r.priority}`;
    (groups.get(k) || groups.set(k, []).get(k)).push((Date.parse(r.solved) - Date.parse(r.created)) / 3600e3);
    weeks.set(monday(r.created), (weeks.get(monday(r.created)) || 0) + 1);
  }
  const median = (xs) => { const s = [...xs].sort((a, b) => a - b), m = s.length >> 1; return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2; };
  assert.equal(j.rows.length, groups.size);
  let last = -Infinity;
  for (const [w, p, n, med, pct] of j.rows) {
    const at = typeof w === "number" ? w : Date.parse(w);
    assert.ok(at >= last, "in time order"); last = at;
    const hs = groups.get(`${at} ${p}`);
    assert.ok(hs, `a week of ${new Date(at).toISOString()} starts on a Monday, as the rows' weeks do`);
    assert.equal(n, hs.length);
    assert.ok(Math.abs(med - median(hs)) < 1e-6, `${med} ${median(hs)}`);
    assert.equal(pct, Math.round(1000 * n / weeks.get(at)) / 10);
  }
}

// GRACE-417: two picks turned from "is one of" to "is not" keep the rows
// with neither, as NOT IN written by hand does.
{
  const c = builder.changeOp({ field: "status", op: "in", value: "open, new" }, "!=");
  const st = { table: "tk", signals: [], fusion: "weighted", filters: [c], any: false, boosts: [], group: { by: [], measures: [{ fn: "count" }], min: 0 }, sort: null, columns: [], score: true, limit: 20 };
  const n = await one(builder.toSql(st).sql);
  assert.equal(n, rows.filter((r) => !["open", "new"].includes(r.status)).length);
  assert.equal(n, await one("SELECT COUNT(*) FROM tk WHERE status NOT IN ('open', 'new')"));
}

// GRACE-407: a team view saved, listed, opened by its id and removed; its
// answer is its SQL's.
{
  assert.deepEqual(await views.listViews(), [], "no views before the first save");
  const st = { table: "tk", signals: [], fusion: "weighted", filters: [], any: false, boosts: [], sort: null, columns: [], score: true, limit: 1000, group: { by: ["priority"], measures: [{ fn: "count" }], min: 0 } };
  const { sql, params } = builder.toSql(st);
  const doc = views.viewDoc({ name: "Monday numbers", about: "tickets by priority", kind: "builder", sql, params, chart: { x: "priority", ys: ["n"], split: null, kind: "bar" }, state: st, by: "test" });
  assert.equal(await views.putView(doc), "monday_numbers");
  const list = await views.listViews();
  assert.deepEqual(list.map((v) => [v.id, v.name, v.kind]), [["monday_numbers", "Monday numbers", "builder"]]);
  const v = await views.getView("monday_numbers");
  assert.deepEqual([v.sql, v.chart, v.state], [sql, doc.spec.chart, st]);
  const j = await q(v.sql, v.params);
  assert.equal(j.rows.reduce((a, r) => a + r[1], 0), rows.length);
  await api("/v1/write", { body: { table: views.TABLE, ops: [{ op: "delete", key: "monday_numbers" }] } });
  assert.equal(await views.getView("monday_numbers"), null);
}

console.log("ok");
process.exit(0);

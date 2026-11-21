// The command center's number format, big answers and column health, and
// the pure parts of its views (what a search shows, the SQL splitter, the
// permission model), in Node (tests/ui.rs runs it when node is on the
// PATH): the modules load with a little of a browser's globals.

import assert from "node:assert/strict";

const noop = () => {};
// What the modules listen for on the document (core.js: whether anybody is at the page).
const docListeners = {};
const el = () => ({ style: {}, dataset: {}, classList: { add: noop, remove: noop, toggle: noop }, append: noop, setAttribute: noop, addEventListener: noop, replaceChildren: noop, children: [] });
Object.assign(globalThis, {
  window: globalThis,
  location: { host: "x", pathname: "/", hash: "", search: "", href: "http://x/" },
  matchMedia: () => ({ matches: false, addEventListener: noop }),
  sessionStorage: { getItem: () => null, setItem: noop, removeItem: noop },
  localStorage: { getItem: () => null, setItem: noop, removeItem: noop },
  document: { addEventListener: (ev, f) => (docListeners[ev] ||= []).push(f), querySelector: () => null, querySelectorAll: () => [], documentElement: el(), body: el(), createElement: el, createElementNS: el, createTextNode: el },
  addEventListener: noop,
});
const app = new URL("../ui/app/", import.meta.url);
const { fmt, ApiError } = await import(new URL("core.js", app));
const big = await import(new URL("bigresult.js", app));
const { flags } = await import(new URL("health.js", app));

// One format for a value of the data: the grid, a chart's tip, Sheets.
assert.equal(fmt.num(226159459.83), (226159459.83).toLocaleString(undefined, { maximumFractionDigits: 2 }));
assert.equal(fmt.num(4.1271186440678), (4.127119).toLocaleString(undefined, { maximumFractionDigits: 6 }));
assert.equal(fmt.num(0.00001234), "1.234e-5");
assert.equal(fmt.num(null), "–");
// Round ticks, labelled short: not 991.7M, 1.5B.
assert.deepEqual(fmt.ticks(0, 1983364195.78, 4), [0, 5e8, 1e9, 1.5e9, 2e9]);
assert.deepEqual(fmt.ticks(0, 1983364195.78, 4).map(fmt.tick), ["0", "500M", "1B", "1.5B", "2B"]);
assert.deepEqual(fmt.ticks(-3, 7.3, 4), [-5, 0, 5, 10]);

// The server's refusal of an answer for its size, in its two wordings.
assert.ok(big.tooBig(new ApiError(400, "query error: the query matches more than 10000 rows, the most a query returns; add a LIMIT (or read every row a page at a time: a scroll, page_size)")));
assert.ok(big.tooBig(new ApiError(400, "query error: LIMIT and OFFSET reach row 20000; a query returns at most 10000 rows")));
assert.ok(!big.tooBig(new ApiError(400, "unknown column x")));
assert.equal(big.SHOWN, 10000);
assert.equal(big.wholeTable("SELECT * FROM trips"), "trips");
assert.equal(big.wholeTable('select * from "my trips";'), "my trips");
assert.equal(big.wholeTable("SELECT * FROM trips WHERE fare > 3"), null);

// A column's flags: always NULL, mostly NULL, one value, a sum a few rows make.
assert.deepEqual(flags({ n: 50, nonNull: 0 }), [["bad", "always NULL"]]);
assert.deepEqual(flags({ n: 100, nonNull: 100, distinct: 1 }), [["warn", "one value"]]);
assert.deepEqual(flags({ n: 1000, nonNull: 1000, distinct: 900, sum: 100, top: 10, topSum: 80 }).map(([, t]) => t), ["10 rows make 80% of the sum"]);
assert.deepEqual(flags({ n: 1000, nonNull: 1000, distinct: 900, sum: 100, top: 10, topSum: 20 }), []);
// Tables and storage: a table's bytes on every copy are the copies' own
// where a coordinator asked them, else one copy's times the copies; every
// table adds up, a table that failed counted apart and a copy that did not
// answer said.
{
  const { onCopies, storageTotals } = await import(new URL("views/tables.js", app));
  const a = { table: "a", docs: 10, bytes: 1000, segments: 2, copies_bytes: 2900, copies_measured: 3, copies_missing: 0, shards: [{}] };
  const b = { table: "b", docs: 5, bytes: 4000, segments: 1, copies_bytes: 8000, copies_measured: 2, copies_missing: 1, shards: [{}] };
  assert.deepEqual(onCopies(a, 3), { bytes: 2900, measured: true, copies: 3, missing: 0 });
  assert.deepEqual(onCopies({ bytes: 1000 }, 3), { bytes: 3000, measured: false, copies: null, missing: 0 });
  assert.equal(onCopies({ table: "x" }, 3), null);
  const t = storageTotals([a, b, { table: "c", error: "shard 1 does not answer" }], 3);
  assert.deepEqual([t.tables, t.failed, t.docs, t.segments, t.bytes, t.all, t.measured, t.incomplete, t.copies], [2, 1, 15, 3, 5000, 10900, true, 1, 3]);
  assert.deepEqual(t.largest, { table: "b", bytes: 8000 });
  // One server: nothing measured beyond its own copy.
  const one = storageTotals([{ table: "a", docs: 1, bytes: 10, segments: 1 }], 1);
  assert.deepEqual([one.bytes, one.all, one.measured, one.largest.table], [10, 10, false, "a"]);
  assert.equal(storageTotals([], 1).largest, null);
}

// CCB-1: a search over a TEXT made from others (not kept, so it reads
// NULL) shows the stored fields it is made from, the title first; the
// saved search's SELECT is the same.
const tableView = await import(new URL("views/table.js", app));
const products = { table: "products", short: { key: "id", table: "products", fields: [
  { name: "id", type: "keyword" }, { name: "title", type: "text", indexed: false, positions: false }, { name: "description", type: "text", indexed: false, positions: false },
  { name: "body", type: "text", from: ["title", "description"] }, { name: "brand", type: "keyword" }, { name: "price", type: "float" },
  { name: "emb", type: "vector", embed: { from: ["title", "description"], model: "hash:64" } }] } };
const body = products.short.fields[3], emb = products.short.fields[6];
const shown = tableView.shownFields(products, [body], emb);
assert.deepEqual([shown.title, shown.text], ["title", "description"]);
assert.ok(!shown.cols.includes("body") && !shown.cols.includes("emb"), shown.cols.join());
const s = tableView.searchSql(products, { q: "rain jacket", fields: [body], vec: emb, mode: "both" });
assert.match(s.body, /^SELECT id, title, description, brand, price, score\(\)\nFROM products/);
// MAYA-202: a typo in the box is forgiven, as on a shop's search page.
assert.match(s.body, /rrf\(bm25\(body, :q, fuzzy => 'auto'\), similarity\(emb, :v\)\)/);
const sw = tableView.searchSql(products, { q: "jaket", fields: [body], vec: null, mode: "words" });
assert.match(sw.body, /bm25\(body, :q, fuzzy => 'auto', min => 1, min_rows => 20\)/);
// A stored searched field shows itself.
const docs = { table: "docs", short: { key: "id", fields: [{ name: "id", type: "keyword" }, { name: "title", type: "text" }, { name: "text", type: "text" }, { name: "n", type: "int" }] } };
assert.deepEqual(tableView.shownFields(docs, [docs.short.fields[2]]), { cols: ["id", "title", "text", "n"], title: "title", text: "text" });
// LIV-5, PRF-5: the Quality tab's profile of a wide table is a few
// statements of side-by-side aggregates and one top-3 a keyword field,
// run a few at a time; the key's distinct count is not asked.
const wideFields = [{ name: "id", type: "keyword" }, ...Array.from({ length: 30 }, (_, i) => ({ name: `k${i}`, type: "keyword" })),
  ...Array.from({ length: 30 }, (_, i) => ({ name: `n${i}`, type: "float" })), ...Array.from({ length: 30 }, (_, i) => ({ name: `t${i}`, type: "text" }))];
const wide = { table: "wide", short: { key: "id", fields: wideFields } };
const prof = tableView.profileSql(wide, wideFields);
assert.equal(prof.tops.length, 30);
assert.ok(prof.batches.length <= 14, `${prof.batches.length} statements of aggregates`);
assert.ok(prof.batches.every((b) => b.parts.length <= 26));
assert.ok(!prof.batches.some((b) => /COUNT\(DISTINCT id\)|COUNT\(id\)/.test(b.sql)));
assert.ok(!prof.batches.some((b) => /IS NOT NULL/.test(b.sql)), "a single field is counted with COUNT(field), which works unindexed");
let live = 0, peak = 0;
await tableView.bounded(Array.from({ length: 20 }, () => async () => { live++; peak = Math.max(peak, live); await new Promise((r) => setTimeout(r, 2)); live--; }), 4);
assert.equal(peak, 4);
// CCB-3: the workbench's statements: an apostrophe in a comment is not
// a string, and a ';' in a string, a quoted name or a comment ends nothing.
const sqlView = await import(new URL("views/sql.js", app));
const three = "-- the shop's first query\nSELECT 1 AS one;\nSELECT 2 AS two;\n";
assert.equal(sqlView.statements(three).length, 2);
assert.equal(sqlView.statementAt(three, [three.length - 3, three.length - 3]).sql.trim(), "SELECT 2 AS two");
assert.equal(sqlView.statementAt(three, [30, 30]).sql.trim(), "-- the shop's first query\nSELECT 1 AS one");
const tricky = `SELECT 'a;b', "x;y" FROM t /* ; it's */ WHERE s = 'it''s;'; -- done; isn't it\nSELECT 3`;
assert.deepEqual(sqlView.statements(tricky).map((x) => x.sql.trim()), [`SELECT 'a;b', "x;y" FROM t /* ; it's */ WHERE s = 'it''s;'`, "-- done; isn't it\nSELECT 3"]);
assert.equal(sqlView.statements("SELECT 1; -- only a comment\n").length, 1);
// CCB-8, CCB-9: a .json of one object a line is JSON Lines; a CSV
// separated by ';' is read as such, decimal commas as numbers.
const imp = await import(new URL("views/import.js", app));
assert.equal(await imp.sniff(new File(['{"a":1}\n{"a":2}\n'], "events.json")), "jsonl");
assert.equal(await imp.sniff(new File(['[{"a":1}]'], "rows.json")), "json");
assert.equal(await imp.sniff(new File(['{\n  "a": 1\n}'], "one.json")), "json");
assert.equal(imp.otherSeparator("id;name;amount"), ";");
assert.equal(imp.otherSeparator("id,name,amount"), null);
assert.equal(imp.otherSeparator("id|name|tags"), "|");
assert.equal(imp.toCommaCsv('id;name;amount\n1;Anna;3,5\n2;"Bo; Jr";4\n', ";"), 'id,name,amount\n1,Anna,3.5\n2,Bo; Jr,4\n');
assert.equal(imp.toCommaCsv("id;city\n1;Paris, France\n", ";"), 'id,city\n1,"Paris, France"\n');
const { isStored } = await import(new URL("catalog.js", app));
assert.deepEqual(products.short.fields.map(isStored), [true, true, true, false, true, true, false]);
assert.equal(isStored({ name: "b", type: "text", from: ["a"], stored: true }), true);

// CCW-1: the ops and propose roles. A role key reads as a read key does,
// and may take what the catalog says its role may (an ops key the bounded,
// tier 1 actions); the tooltip names the key that would do.
const core = await import(new URL("core.js", app));
const ACK = "POST /v1/alerts/:rule/ack", NOTE = "POST /v1/events/notes", APPROVE = "POST /v1/admin/oncall/proposals/:id/approve";
core.provideRoutes([
  { method: "POST", path: "/v1/alerts/:rule/ack", scope: "write", tier: 1, roles: { ops: true, propose: false } },
  { method: "POST", path: "/v1/events/notes", scope: "admin", tier: 1, roles: { ops: true, propose: false } },
  { method: "GET", path: "/v1/admin/audit", scope: "admin", tier: 0, roles: { ops: true, propose: true } },
  { method: "POST", path: "/v1/admin/oncall/proposals/:id/approve", scope: "admin", tier: 2, roles: { ops: false, propose: false } },
]);
const role = (scope) => ({ name: "bot", scope, may: { data: "read", server_views: true, act_alone_up_to_tier: scope === "ops" ? 1 : 0, reason_required: true } });
core.provide({ whoami: role("ops") });
assert.equal(core.roleOf(), "ops");
assert.ok(core.can("read") && !core.can("write") && !core.can("admin"));
assert.ok(core.can("write", ACK) && core.can("admin", NOTE) && core.can("admin", "GET /v1/admin/audit"));
assert.ok(!core.can("admin", APPROVE));
assert.equal(core.needs("admin", APPROVE), "needs an admin key (this key is ops: it files a change request instead)");
assert.ok(core.reasonNeeded());
core.provide({ whoami: role("propose") });
assert.ok(core.can("read") && !core.can("write", ACK) && core.can("admin", "GET /v1/admin/audit"));
assert.equal(core.needs("write", ACK), "needs a write or an ops key (this key is propose: it files change requests)");
core.provide({ whoami: { name: "w", scope: "write", may: { data: "write", server_views: false } } });
assert.ok(core.can("write", ACK) && !core.can("admin", NOTE) && core.can("write"));
assert.equal(core.needs("admin", NOTE), "needs an admin or an ops key");
assert.equal(core.roleOf(), null);
// MARCO-301: a key limited to tables calls the server's routes whoami lists
// for it (the oncall preset's: the alert's actions), and the page offers
// no other, with a tooltip that says why; a server from before the list
// is read by the key's scope, as before.
const RULES = "PUT /v1/alerts/rules";
const oncall = { name: "netops", scope: "write", tables: { "logs_*": "write", ctx_incidents: "read" }, preset: "oncall", may: { data: "write", server_views: false, routes: ["GET /v1/alerts", "GET /v1/alerts/:rule", "POST /v1/alerts/:rule/ack", "POST /v1/alerts/:rule/silence", "DELETE /v1/alerts/:rule/silence", "POST /v1/alerts/:rule/notes"] } };
core.provide({ whoami: oncall });
assert.ok(core.can("write", ACK) && core.can("write", "POST /v1/alerts/:rule/notes"));
assert.ok(!core.can("admin", RULES) && !core.can("admin", NOTE) && !core.can("admin"));
assert.equal(core.needs("admin", RULES), "needs an admin key of the server (this key is limited to tables)");
core.provide({ whoami: { ...oncall, name: "shipper", tables: { "logs_*": "write" }, preset: undefined, may: { data: "write", routes: ["GET /v1/alerts"] } } });
assert.ok(!core.can("write", ACK));
assert.equal(core.needs("write", ACK), "needs a key given POST /v1/alerts/:rule/ack (this key is limited to tables: logs_* write)");
core.provide({ whoami: { name: "viewer", scope: "read", tables: { "logs_*": "read" }, may: { data: "read", routes: ["GET /v1/alerts", "GET /v1/alerts/:rule"] } } });
assert.ok(!core.can("write", ACK));
assert.equal(core.needs("write", ACK), "needs a write key given POST /v1/alerts/:rule/ack (this key is limited to tables: logs_* read)");
core.provide({ whoami: { name: "old", scope: "write", tables: { "logs_*": "write" }, may: { data: "write" } } });
assert.ok(core.can("write", ACK), "a server that lists no routes: by the key's scope, as before");
// MARCO-403: a key whose grants only append reads nothing, the feed included.
core.provide({ whoami: { name: "collector", scope: "append", tables: { "logs_*": "append" }, may: { data: "append", server_views: false, routes: ["POST /v1/logs", "POST /v1/import", "POST /v1/write", "POST /v1/sql", "GET /v1/health", "GET /v1/whoami"] } } });
assert.ok(!core.can("feed") && !core.can("read") && !core.can("write") && !core.can("read", "GET /v1/alerts") && core.can("write", "POST /v1/logs"));
core.provide({ whoami: null });
core.provideRoutes(null);

// CCW-3: after a 401 the polls hold (a poll runs once, then waits) until
// another key is given.
// (Another key makes data.js poll again: its answers here are empty.)
console.warn = noop;
let refuse = true;
globalThis.fetch = async () => (refuse ? { status: 401, ok: false, text: async () => '{"error":"a key is needed"}' } : { status: 200, ok: true, text: async () => "{}" });
await core.api("/v1/alerts").catch(() => {});
assert.ok(core.signInNeeded());
const ticks = [];
const tick = () => new Promise((r) => setTimeout(r, 30));
const stop = core.every(10, () => ticks.push(1));
await tick(); await tick();
assert.equal(ticks.length, 1, "a poll waits while the key is refused");
refuse = false;
core.setToken("another");
assert.ok(!core.signInNeeded());
await tick();
assert.ok(ticks.length > 1, "it goes on once another key is given");
stop();

// LIV-14: cards that ask the same thing at once share one request, each
// with its own copy of the answer; a change is never shared.
let fetches = 0;
globalThis.fetch = async () => { fetches++; await tick(); return { status: 200, ok: true, text: async () => '{"tables":[{"name":"t"}]}' }; };
const [a1, a2] = await Promise.all([core.api("/v1/index/tables"), core.api("/v1/index/tables")]);
assert.equal(fetches, 1, "one request for two cards");
assert.deepEqual(a1, a2); assert.notEqual(a1, a2);
await core.api("/v1/index/tables");
assert.equal(fetches, 2, "a later read asks again");
await Promise.all([core.api("/v1/x", { method: "POST", body: {} }), core.api("/v1/x", { method: "POST", body: {} })]);
assert.equal(fetches, 4, "writes are each sent");

// LIV-14: a page nobody touched for IDLE_MS polls IDLE_SLOW times less
// often, and asks what is due as soon as someone is back.
globalThis.fetch = async () => ({ status: 200, ok: true, text: async () => "{}" });
const realNow = Date.now;
const count = async (opts) => { let n = 0; const st = core.every(10, () => { n++; }, true, opts); await new Promise((r) => setTimeout(r, 200)); st(); return n; };
const busy = await count();
Date.now = () => realNow() + core.IDLE_MS + 1000;
assert.ok(core.isIdle());
const resting = await count();
// Idle, a poll waits at least 10 * IDLE_SLOW - 5 ms after the one before,
// so 200 ms hold the first and at most 200 / 45 more, however late the
// timers fire. (Bounding it by the polls counted while used failed on a
// loaded macOS runner, whose late timers left 15 of the 20 there.)
assert.ok(busy >= 8 && resting <= 1 + Math.floor(200 / (10 * core.IDLE_SLOW - 5)), `polls while used ${busy}, while idle ${resting}`);
assert.ok(await count({ always: true }) >= 8, "a change under way is followed at the full rate");
let woke = 0;
const st = core.every(10, () => { woke++; });
await new Promise((r) => setTimeout(r, 30));
const before = woke;
for (const f of docListeners.pointermove || []) f();
assert.ok(!core.isIdle());
assert.equal(woke, before + 1, "someone back: what is due is asked at once");
st();
Date.now = realNow;

// LIV-9: an alert firing now is a problem on Home, though the fleet's last
// answer did not count it yet; an info alert tells but is no problem.
const data = await import(new URL("data.js", app));
data.state.fleet = { status: "ok", problems: [], checks: [] };
data.state.alerts = { rules: [{ id: "p99-jump", name: "Query p99 doubled", severity: "warning" }], states: [{ rule: "p99-jump", state: "firing", since_ms: 1000, where: [] }] };
assert.deepEqual(data.homeProblems().map((p) => [p.id, p.severity, p.title, p.next.view]), [["alerts", "warning", "1 alert is firing: Query p99 doubled", "alert/p99-jump"]]);
assert.equal(data.homeStatus(), "warning");
data.state.fleet = { status: "warning", problems: [{ id: "alerts", severity: "warning", title: "1 alert is firing: Query p99 doubled" }] };
assert.equal(data.homeProblems().length, 1, "counted once when the fleet has it");
data.state.fleet = { status: "ok", problems: [] };
data.state.alerts.rules[0].severity = "info";
assert.deepEqual([data.homeProblems(), data.homeStatus()], [[], "ok"]);
data.state.fleet = null; data.state.alerts = null;

// LIV-12: the apps' tables are the apps', not the owner's own: netops's and
// ChloeCtx's fold away, and netops's syslog stays a source for Logs, named
// as the app's.
const owners = await import(new URL("owners.js", app));
assert.deepEqual(["netops_sites", "chloectx_memory", "ctx_spaces", "fs_items", "products"].map((t) => owners.ownerOf(t)?.owner ?? null), ["Network operations", "ChloeCtx", "ChloeCtx", "Feature store", null]);
assert.deepEqual(owners.byOwner(["products", "netops_sites", "netops_links"]).mine, ["products"]);
const logsView = await import(new URL("views/logs.js", app));
const syslog = { table: "netops_syslog", short: { fields: [{ name: "ts", type: "timestamp" }, { name: "body", type: "text" }] } };
assert.deepEqual(logsView.logTables([syslog]).map((x) => x.table), ["netops_syslog"]);

// LIV-11: a memory request is not a limit: no restart is foretold without
// one, but a request under what a copy holds is said; a limit under it
// restarts the copy.
const { SETTINGS } = await import(new URL("spec.js", app));
const memory = SETTINGS.find((x) => x.label === "Memory per server");
const L = { rss: 608e6, file: 1e8 };
const noLimit = { L, d: { resources: { requests: { memory: "512Mi" } } } };
assert.match(memory.warn("512Mi", noLimit), /scheduler may put more on a node/);
assert.doesNotMatch(memory.warn("512Mi", noLimit) + memory.effect("512Mi", noLimit), /restart(ed)? (it|by)/);
const limited = { L, d: { resources: { requests: { memory: "512Mi" }, limits: { memory: "512Mi" } } } };
assert.match(memory.warn("512Mi", limited), /a limit of 512Mi would restart it/);
assert.equal(memory.warn("1Gi", { L, d: { resources: { requests: { memory: "1Gi" } } } }), null);

// Routing settings: least-cost and prefer-caught-up are chloedb-server's
// defaults, so leaving them as they are writes no flag, and turning
// prefer-caught-up off writes --prefer-caught-up=false.
const spec = await import(new URL("spec.js", app));
const routeSt = SETTINGS.find((x) => x.at === "cargs:--route"), preferSt = SETTINGS.find((x) => x.at === "cargs:--prefer-caught-up"), adaptiveSt = SETTINGS.find((x) => x.at === "cargs:--hedge-adaptive");
const cs = { coordinatorArgs: ["--hedge-ms", "50"] };
assert.deepEqual([spec.value(cs, routeSt), spec.value(cs, preferSt), spec.value(cs, adaptiveSt)], ["least-cost", true, false]);
assert.equal(spec.shown(routeSt, spec.value(cs, routeSt)), "the one that costs least");
spec.assign(cs, preferSt, false);
spec.assign(cs, routeSt, "round-robin");
spec.assign(cs, adaptiveSt, true);
assert.deepEqual(cs.coordinatorArgs, ["--hedge-ms", "50", "--prefer-caught-up=false", "--route", "round-robin", "--hedge-adaptive"]);
assert.deepEqual([spec.value(cs, routeSt), spec.value(cs, preferSt), spec.value(cs, adaptiveSt)], ["round-robin", false, true]);
assert.match(preferSt.warn(true, { d: cs }), /each in turn ignores it/);
spec.assign(cs, preferSt, true);
spec.assign(cs, routeSt, "least-cost");
spec.assign(cs, adaptiveSt, false);
assert.deepEqual(cs.coordinatorArgs, ["--hedge-ms", "50"]);
assert.equal(spec.value({ coordinatorArgs: ["--prefer-caught-up"] }, preferSt), true);
assert.equal(spec.value({ coordinatorArgs: ["--partial"] }, SETTINGS.find((x) => x.at === "cargs:--partial")), true);
assert.equal(spec.value({}, SETTINGS.find((x) => x.at === "cargs:--partial")), false);

// LIV-10: advice names the spec's field when an operator runs the cluster, the flag otherwise.
assert.equal(data.howToSet({ flag: "--environment", field: "spec.environment", value: "production" }), "start the server with --environment production");
data.state.kube = { managed: true, namespace: "chloedb", name: "e2e" };
assert.equal(data.howToSet({ flag: "--environment", field: "spec.environment", value: "production", or: "staging" }), "set spec.environment: production (or staging) in the StrataCluster chloedb/e2e");
data.state.kube = null;

// CCB-17: a save of the definition there now makes no version, and the
// dialog knows it; another body, or another description, does.
const savedView = await import(new URL("views/saved.js", app));
const v1 = { version: 3, signature: "jacket_search(q TEXT, k INT DEFAULT 10)", description: "Finds jackets.", body: "SELECT id, score() FROM products ORDER BY bm25(body, :q) DESC LIMIT :k" };
const jacket = { name: "jacket_search", params: [{ name: "q", ty: "TEXT" }, { name: "k", ty: "INT", default: 10 }], body: "SELECT id, score()\nFROM products\nORDER BY bm25(body, :q) DESC\nLIMIT :k", description: " Finds jackets. " };
assert.ok(savedView.sameDefinition(v1, jacket));
assert.ok(!savedView.sameDefinition(v1, { ...jacket, body: jacket.body.replace("LIMIT", "WHERE price < 9 LIMIT") }));
assert.ok(!savedView.sameDefinition(v1, { ...jacket, description: "Finds coats." }));
assert.ok(!savedView.sameDefinition(undefined, jacket));
// CCB-18: two versions, each kept on one line, compare a clause a line:
// what both share is not marked.
const oldDef = "CREATE FUNCTION f(q TEXT) RETURNS TABLE AS SELECT id, title, score() FROM products ORDER BY bm25(body, :q) DESC LIMIT 10";
const newDef = "CREATE FUNCTION f(q TEXT) RETURNS TABLE AS SELECT id, title, score() FROM products WHERE price < 200 ORDER BY bm25(body, :q) DESC LIMIT 10";
const lines = savedView.versionDiff(oldDef, newDef, sqlView.formatSql);
assert.deepEqual(lines.filter(([k]) => k !== " ").map(([k, l]) => k + l), ["+WHERE price < 200"]);
assert.ok(lines.length >= 5, JSON.stringify(lines));

// CCB-24: the tiles are in the List's order, largest first.
assert.deepEqual(tableView.tilesOrder([{ table: "products" }, { table: "events" }, { table: "notes" }, { table: "a" }], { products: 1200, events: 20000, notes: 3 }).map((t) => t.table), ["events", "products", "notes", "a"]);

// CCW-10: the shell knows each view before its code loads: what views/index.js
// says of a view (its title, icon, layout, the words the palette finds it by)
// is what the view's module says, and each view's code loads.
const { VIEWS, NAV } = await import(new URL("views/index.js", app));
for (const v of VIEWS) {
  const m = (await v.load()).default;
  assert.equal(typeof m.render, "function", v.id);
  for (const k of ["id", "title", "icon", "group", "parent", "nav", "hidden", "full", "range", "cluster", "keywords"]) assert.deepEqual(v[k], m[k], `views/index.js says ${v.id}.${k} = ${JSON.stringify(v[k])}, its module ${JSON.stringify(m[k])}`);
}
for (const [, list] of NAV) for (const e of list) assert.ok(VIEWS.some((v) => v.id === e.id || v.id === e.fallback), e.id);

// APC-27: a request of an app's page that is lost on its way says nothing of
// the server (it forwards the app's requests); the server's own does.
{
  const { api, connection } = await import(new URL("core.js", app));
  globalThis.fetch = async () => ({ status: 200, ok: true, text: async () => "{}" });
  await api("/v1/health");
  assert.equal(connection().ok, true);
  globalThis.fetch = async () => { throw new TypeError("Failed to fetch"); };
  await assert.rejects(api("/v1/apps/netops/api/status"), (e) => e.status === 0 && e.answered === false);
  await assert.rejects(api("/v1/apps/apps/api/apps/a1/sql", { method: "POST", body: {} }));
  assert.equal(connection().ok, true, "an app's lost request is the app page's to say");
  await assert.rejects(api("/v1/health"));
  assert.equal(connection().ok, false, "the server's own lost request says it is not answering");
  globalThis.fetch = async () => ({ status: 200, ok: true, text: async () => "{}" });
  await api("/v1/health");
  assert.equal(connection().ok, true);
}

// An operator older than the servers it runs (views/rollout.js): the
// upgrader moves the servers, never the operator, and a 0.3.5 server
// refuses the copies an older operator adds (audit CLU-3).
{
  const { cmpRelease, operatorBehind } = await import(new URL("views/rollout.js", app));
  assert.deepEqual(["0.3.5", "0.10.1", "v0.3.10", "0.3.4"].sort(cmpRelease), ["0.3.4", "0.3.5", "v0.3.10", "0.10.1"]);
  assert.equal(cmpRelease("v0.3.5", "0.3.5"), 0);
  const v = (release, digest) => ({ release, digest });
  assert.equal(operatorBehind({ version: "0.3.5" }, [v("0.3.5"), v("0.3.5")], ""), null);
  assert.equal(operatorBehind({ version: "0.3.6" }, [v("0.3.5"), null], ""), null, "an operator newer than its servers is fine");
  assert.equal(operatorBehind({ version: "0.3.6" }, [v("0.3.6"), v("0.3.7")], ""), "The operator runs 0.3.6 and the servers 0.3.7");
  // Builds of main between two releases say one version: by when they were built.
  const built = (d) => ({ "sha256:old": Date.UTC(2026, 10, 15, 10), "sha256:new": Date.UTC(2026, 10, 15, 20) })[d] ?? null;
  assert.match(operatorBehind({ version: "0.3.5", digest: "sha256:old" }, [v("0.3.5", "sha256:new")], "", built), /older build than the servers/);
  assert.equal(operatorBehind({ version: "0.3.5", digest: "sha256:new" }, [v("0.3.5", "sha256:old")], "", built), null);
  assert.equal(operatorBehind({ version: "0.3.5", digest: "sha256:else" }, [v("0.3.5", "sha256:new")], "", built), null, "a build the registry does not date is not judged");
  const refused = "POST /v1/admin/shards/0/replicas: 403 Forbidden: adding s0-2 to the cluster takes the cluster's token: the coordinator sends it to the server it adds";
  assert.match(operatorBehind(undefined, [v("0.3.5")], refused), /older than 0\.3\.5/);
  assert.equal(operatorBehind(undefined, [v("0.3.5")], "Ready"), null, "an operator that says no version may be 0.3.5");
  assert.match(operatorBehind(undefined, [v("0.3.6")], "Ready"), /older than 0\.3\.6/);
}

// The Log explorer: which column a line's text is, its fields beside it, and the search on a table with no text column.
{
  const lx = await import(new URL("views/explorer.js", app));
  // HN's users: one text column, mostly empty. The table is not a log, so its lines show their fields.
  const users = [{ name: "id", type: "keyword" }, { name: "stories", type: "int" }, { name: "first", type: "timestamp" }, { name: "last", type: "timestamp" }, { name: "about", type: "text" }, { name: "karma", type: "int" }, { name: "emb", type: "vector", dim: 8 }];
  const u = lx.logColumns(users, { table: "hn_users", key: "id" });
  assert.deepEqual([u.ts, u.msg, u.log, u.key, u.times, u.texts], ["first", "about", false, "id", ["first", "last"], ["about"]]);
  const rows = Array.from({ length: 10 }, (_, i) => ({ id: "u" + i, stories: i, first: "2026-11-16T20:15:22.000Z", last: null, about: i < 2 ? "I build things" : "", karma: 5 }));
  assert.deepEqual(lx.pickText(rows, u), { msg: "about", n: 10, empty: 8 }, "no fuller text column: the lines show their fields");
  assert.deepEqual(lx.rowFields({ ...rows[3], emb: [0.1, 0.2] }, u), [["stories", "3"], ["karma", "5"]], "not the time, the text, the key, empty values or a vector");
  assert.equal(lx.lineText(rows[3], u), "stories=3 karma=5");
  assert.equal(lx.lineText(rows[0], u), "I build things");
  // HN's stories: not a log, so a title says more in a line than a body (which most stories lack); a text made from another column (urlwords) comes last.
  const stories = [{ name: "id", type: "int" }, { name: "title", type: "text", substring: true }, { name: "body", type: "text" }, { name: "url", type: "keyword", substring: true }, { name: "urlwords", type: "text", from: "url" }, { name: "author", type: "keyword" }, { name: "created", type: "timestamp" }];
  const st = lx.logColumns(stories, { table: "hn_stories", key: "id" });
  assert.deepEqual([st.ts, st.msg, st.texts], ["created", "title", ["title", "body", "url", "urlwords"]]);
  assert.deepEqual(lx.searchSql("rust", st).conds, ["title ILIKE '%rust%'"]);
  // The newest lines move it to a fuller column when it is empty in most of them.
  const srows = Array.from({ length: 10 }, (_, i) => ({ id: i, title: i === 0 ? "Ask HN" : "", body: "words " + i, url: "https://x/" + i, created: "2026-11-16T20:00:00Z" }));
  const d = lx.pickText(srows, st);
  assert.deepEqual(d, { msg: "body", from: "title", n: 10, empty: 9 });
  const st2 = lx.withText(st, d.msg);
  assert.deepEqual([st2.msg, st2.substring, st2.indexed], ["body", false, true]);
  assert.deepEqual(lx.searchSql("rust", st2).conds, ["MATCH(body, 'rust')"]);
  assert.deepEqual(lx.pickText(srows.slice(0, 4).map((r) => ({ ...r, body: r.id === 1 ? r.body : "", url: null })), st), { msg: "title", n: 4, empty: 3 }, "none full in half the lines: it stays");
  // A log: its level and host make it one, and its text is the body.
  const syslog = [{ name: "ts", type: "timestamp" }, { name: "severity", type: "keyword" }, { name: "host", type: "keyword", substring: true }, { name: "service", type: "keyword" }, { name: "body", type: "text", substring: true }, { name: "attributes", type: "json" }];
  const sy = lx.logColumns(syslog, { table: "logs_syslog" });
  assert.deepEqual([sy.ts, sy.msg, sy.host, sy.sev, sy.log, sy.substring, sy.texts], ["ts", "body", "host", "severity", true, true, ["body"]], "the host is not a text to move to");
  assert.deepEqual(lx.searchSql("Gi0/1 -debug", sy).conds, ["body ILIKE '%Gi0/1%'", "NOT (body ILIKE '%debug%')"]);
  // OTLP logs: the level by severity_number, which a collector sets whatever its text says (nginx's "502" is ERROR 17; MARCO-502).
  const otlp = [{ name: "ts", type: "timestamp" }, { name: "severity", type: "keyword" }, { name: "severity_number", type: "int" }, { name: "service", type: "keyword" }, { name: "body", type: "text" }];
  const ot = lx.logColumns(otlp, { table: "logs_app" });
  assert.deepEqual([ot.sev, ot.sevNum], ["severity", "severity_number"]);
  assert.deepEqual(lx.searchSql("level>=error", ot).conds, ["severity_number >= 17"]);
  assert.deepEqual(lx.searchSql("level>warn", ot).conds, ["severity_number >= 17"]);
  assert.deepEqual(lx.searchSql("severity<info", ot).conds, ["severity_number < 9"]);
  assert.deepEqual(lx.searchSql("level>=error", sy).conds, ["severity IN ('error', 'ERROR', 'err', 'ERR', 'critical', 'CRITICAL', 'crit', 'CRIT', 'fatal', 'FATAL', 'alert', 'ALERT', 'emerg', 'EMERG', 'emergency', 'EMERGENCY', 'panic', 'PANIC')"], "no number: the spellings");
  // BGP updates: a TIMESTAMP and no text column. Each line is its fields, and a word is a field's exact value.
  const updates = [{ name: "id", type: "int" }, { name: "ts", type: "timestamp" }, { name: "collector", type: "keyword" }, { name: "peer_as", type: "int" }, { name: "prefix", type: "keyword" }, { name: "route", type: "keyword", indexed: false }, { name: "comms", type: "keyword", multi: true }];
  const bu = lx.logColumns(updates, { table: "bb_updates", key: "id" });
  assert.deepEqual([bu.ts, bu.msg, bu.log, bu.texts], ["ts", null, false, []]);
  assert.deepEqual(lx.searchSql("rrc00", bu).conds, ["(collector = 'rrc00' OR prefix = 'rrc00')"]);
  assert.deepEqual(lx.searchSql("13335", bu).conds, ["(id = 13335 OR collector = '13335' OR peer_as = 13335 OR prefix = '13335')"]);
  assert.deepEqual(lx.searchSql("collector:rrc0*", bu).conds, ["collector LIKE 'rrc0%'"]);
  assert.equal(lx.lineText({ id: 7, ts: "x", collector: "rrc00", peer_as: 3333, comms: ["3333:100"], route: "" }, bu), "collector=rrc00 peer_as=3333 comms=3333:100");
  assert.equal(lx.pickText([{ id: 1 }], bu).msg, null);
  // Backbone's alerts: an unindexed KEYWORD title is the text, searched by ILIKE (MATCH needs a text index), not the indexed subject.
  const alerts = [{ name: "id", type: "keyword" }, { name: "severity", type: "keyword" }, { name: "subject", type: "keyword" }, { name: "title", type: "keyword", indexed: false }, { name: "detail", type: "keyword", indexed: false }, { name: "first_seen", type: "timestamp" }, { name: "last_seen", type: "timestamp" }];
  const al = lx.logColumns(alerts, { table: "bb_alerts", key: "id" });
  assert.deepEqual([al.msg, al.texts, al.sev, al.log], ["title", ["title", "subject", "detail"], "severity", true]);
  assert.deepEqual(lx.searchSql("RPKI", al, "words").conds, ["title ILIKE '%RPKI%'"]);
  // arXiv's issues: an unindexed TEXT is ranked after an indexed one, and a search of it reads it with ILIKE.
  const issues = [{ name: "id", type: "keyword" }, { name: "since", type: "timestamp" }, { name: "made", type: "timestamp" }, { name: "title", type: "text" }, { name: "body", type: "text", indexed: false }];
  const is = lx.logColumns(issues, { table: "ax_issues", key: "id" });
  assert.deepEqual([is.msg, is.texts], ["title", ["title", "body"]]);
  assert.deepEqual(lx.searchSql("graph body:graph", is).conds, ["MATCH(title, 'graph')", "body ILIKE '%graph%'"]);
  assert.equal(lx.logColumns([{ name: "id", type: "keyword" }, { name: "about", type: "text" }]), null, "no TIMESTAMP: not in the explorer");
  // MARCO-403: "Send logs here" gives the Collector a key that only appends to its table, not the oncall key.
  assert.deepEqual(lx.shipperKey("logs_app"), { name: "logs-app-collector", tables: { logs_app: "append" } });
  assert.ok(lx.collectorYaml("http://x", "logs_app").includes("${env:CHLOEDB_SHIP_KEY}") && lx.importCurl("http://x", "logs_app").includes("$CHLOEDB_SHIP_KEY"));
  // Values on one line: whitespace folded, cut, arrays joined, times to the second.
  assert.equal(lx.shortValue("a\n  b", "text"), "a b");
  assert.equal(lx.shortValue("x".repeat(100), "keyword").length, 80);
  assert.equal(lx.shortValue([1, 2, 3], "int"), "1,2,3");
  assert.equal(lx.shortValue({ a: 1 }, "json"), '{"a":1}');
  assert.equal(lx.shortValue("2026-11-16T20:15:22.123456Z", "timestamp"), "2026-11-16T20:15:22Z");
  assert.deepEqual(lx.rowFields({ a: 1, b: 2, c: 3 }, { fields: [{ name: "a", type: "int" }, { name: "b", type: "int" }, { name: "c", type: "int" }] }, 2), [["a", "1"], ["b", "2"], ["…", "+1 more"]]);
  // Patterns of field lines group by the fields' shape.
  const g = lx.groupPatterns([{ ts: "2026-11-16T20:00:00Z", collector: "rrc00", peer_as: 3333 }, { ts: "2026-11-16T20:01:00Z", collector: "rrc01", peer_as: 174 }], (r) => lx.lineText(r, bu), "ts", null);
  assert.deepEqual(g.map((x) => [x.pattern, x.n]), [["collector=rrc<n> peer_as=<n>", 2]]);
}

// A checkpoint's default name is one the server takes (OLU-10): a copy named by its address loses the ":".
{
  const at = new Date(Date.UTC(2026, 10, 17, 18, 52, 11));
  assert.equal(core.checkpointName("127.0.0.1:17744", at), "127.0.0.1-17744-2026-11-17-18-52-11");
  assert.equal(core.checkpointName("s1-0", at), "s1-0-2026-11-17-18-52-11");
  assert.equal(core.checkpointName(".hidden name", at), "hidden-name-2026-11-17-18-52-11");
  assert.equal(core.checkpointName("", at), "2026-11-17-18-52-11");
  for (const p of ["127.0.0.1:17744", "[::1]:7701", "pod/s0 a", "cp", "..x"]) assert.equal(core.checkpointNameProblem(core.checkpointName(p, at)), null, p);
  assert.ok(core.checkpointNameProblem("127.0.0.1:17744-2026"));
  assert.ok(core.checkpointNameProblem(".x"));
  assert.ok(core.checkpointNameProblem(""));
  assert.ok(core.checkpointNameProblem("x".repeat(201)));
}

// PRIYA-205: the relevance lab's judgments in and out, its gain and its statistics.
{
  const rel = await import(new URL("views/relevance.js", app));
  // TREC's four columns and BEIR's three with a header; grades into 0..3.
  const trec = rel.parseQrels("1 0 d1 2\n1 0 d2 0\n2 0 d3 4\n2 0 d4 -1\n");
  assert.deepEqual(trec.qrels, { 1: { d1: 2, d2: 0 }, 2: { d3: 3, d4: 0 } });
  assert.equal(trec.clamped, 2);
  const beir = rel.parseQrels("query-id\tcorpus-id\tscore\nq7\tdocA\t1\nq7\tdocB\t2\n");
  assert.deepEqual(beir.qrels, { q7: { docA: 1, docB: 2 } });
  assert.equal(beir.lines, 2);
  // Queries as TSV and as BEIR's JSON lines.
  assert.deepEqual(rel.parseQueries("1\tquiet charger\n2\tcovid 19 masks\n"), { 1: "quiet charger", 2: "covid 19 masks" });
  assert.deepEqual(rel.parseQueries('{"_id": "q7", "text": "Vitamin D and bones", "metadata": {}}\n'), { q7: "Vitamin D and bones" });
  // Which file is which: by name, else by its lines.
  assert.equal(rel.fileKind("test.tsv", "q7\tdocA\t1\nq7\tdocB\t2\n"), "qrels");
  assert.equal(rel.fileKind("queries.jsonl", ""), "queries");
  assert.equal(rel.fileKind("x.txt", '{"_id": "1", "text": "a"}'), "queries");
  assert.equal(rel.fileKind("x.txt", "1\tquiet charger for a phone\n2\twhy does the linker fail\n"), "queries");
  // Into the lab's grades by query text, lower-cased, with the qid kept.
  const into = rel.gradesFromQrels({ q7: { docA: 1 }, q9: { docC: 2 } }, { q7: "Vitamin D and bones" });
  assert.deepEqual(into.grades, { "vitamin d and bones": { docA: 1 }, q9: { docC: 2 } });
  assert.deepEqual(into.qids, { "vitamin d and bones": "q7", q9: "q9" });
  assert.equal(into.unnamed, 1);
  // Out as TREC files: the qid it came with, else a new one; and back in again.
  const out = rel.toTrec({ "vitamin d and bones": { docA: 1, "a b": 2 }, "quiet charger": { k2: 3, k1: 0 }, empty: {} }, { "vitamin d and bones": "q7" });
  assert.equal(out.qrels, "q1 0 k1 0\nq1 0 k2 3\nq7 0 docA 1\n");
  assert.equal(out.queries, "q1\tquiet charger\nq7\tvitamin d and bones\n");
  assert.equal(out.skipped, 1);
  const back = rel.gradesFromQrels(rel.parseQrels(out.qrels).qrels, rel.parseQueries(out.queries));
  assert.deepEqual(back.grades, { "quiet charger": { k1: 0, k2: 3 }, "vitamin d and bones": { docA: 1 } });
  // nDCG's gain: exponential by default, linear as trec_eval; the same on 0/1 grades.
  const g = { a: 3, b: 1, c: 0 };
  const exp = rel.measures(["b", "a"], g, 10).ndcg, lin = rel.measures(["b", "a"], g, 10, "linear").ndcg;
  assert.ok(Math.abs(exp - (1 + 7 / Math.log2(3)) / (7 + 1 / Math.log2(3))) < 1e-12, String(exp));
  assert.ok(Math.abs(lin - (1 + 3 / Math.log2(3)) / (3 + 1 / Math.log2(3))) < 1e-12, String(lin));
  assert.equal(rel.measures(["b", "a"], { a: 1, b: 0 }, 10).ndcg, rel.measures(["b", "a"], { a: 1, b: 0 }, 10, "linear").ndcg);
  // A held-out half: the same each time, half and half, nothing in both.
  const qs = Array.from({ length: 21 }, (_, i) => `query ${i}`);
  const h1 = rel.halves(qs), h2 = rel.halves([...qs].reverse());
  assert.deepEqual(h1, h2);
  assert.equal(h1.fit.length, 11); assert.equal(h1.held.length, 10);
  assert.equal(new Set([...h1.fit, ...h1.held]).size, 21);
  assert.deepEqual(rel.halves(["a", "b", "c"]), { fit: ["a", "b", "c"], held: [] });
  // The paired randomization test: exact for a few queries, seeded for more.
  const five = rel.pairedTest([0, 0, 0, 0, 0], [1, 1, 1, 1, 1]);
  assert.equal(five.p, 2 / 32); assert.equal(five.better, 5); assert.equal(five.mean, 1);
  assert.equal(rel.pairedTest([0.5, 0.5], [0.5, 0.5]).p, 1);
  assert.equal(rel.pairedTest([0.2, null, 0.4], [0.3, 0.9, 0.4]).n, 2);
  const a = Array.from({ length: 40 }, (_, i) => 0.5 + 0.01 * (i % 7)), b = a.map((x, i) => x + (i % 5 === 0 ? -0.01 : 0.03));
  const big = rel.pairedTest(a, b);
  assert.ok(big.p < 0.001, String(big.p));
  assert.equal(rel.pairedTest(a, b).p, big.p);
  const noise = rel.pairedTest(a, a.map((x, i) => x + (i % 2 ? 0.02 : -0.02)));
  assert.ok(noise.p > 0.5, String(noise.p));
  assert.equal(rel.fmtP(0.0001), "p < 0.001");
  assert.equal(rel.fmtP(0.0421), "p = 0.042");
}

// PRIYA-401: a binary qrels file (BEIR's 0 and 1) is read as relevant = good,
// so "good in top k" and MRR agree with trec_eval's success and recip_rank.
{
  const rel = await import(new URL("views/relevance.js", app));
  const bin = rel.parseQrels("q1\td1\t1\nq1\td2\t0\n");
  assert.equal(bin.binary, true);
  assert.deepEqual(bin.qrels, { q1: { d1: 2, d2: 0 } });
  assert.equal(rel.parseQrels("q1\td1\t1\nq1\td2\t2\n").binary, false);
  assert.deepEqual(rel.parseQrels("q1\td1\t1\nq1\td2\t2\n").qrels, { q1: { d1: 1, d2: 2 } });
  // The relevant doc at rank 2: reciprocal rank 0.5, a good row in the top 10, nDCG as with the raw 1.
  const m = rel.measures(["x", "d1"], bin.qrels.q1, 10, "linear");
  assert.equal(m.mrr, 0.5); assert.equal(m.precision, 0.1); assert.equal(m.success, 1);
  assert.equal(rel.measures(["x", "y"], bin.qrels.q1, 10, "linear").success, 0);
  assert.ok(Math.abs(m.ndcg - rel.measures(["x", "d1"], { d1: 1, d2: 0 }, 10, "linear").ndcg) < 1e-12);
  assert.ok(Math.abs(m.ndcg - 1 / Math.log2(3)) < 1e-12);
}

// NADIA-403: the recsys eval's nDCG counts every request, 0 for a miss.
{
  const rx = await import(new URL("views/recsys.js", app));
  const row = (key, position, label) => ({ row_key: key, position, label, shown: true });
  const miss = rx.requestMeasures([row("a", 0, 0), row("b", 1, 0)], 10);
  assert.equal(miss.ndcg, 0); assert.equal(miss.hit, 0);
  const hit = rx.requestMeasures([row("a", 0, 0), row("b", 1, 1)], 10);
  assert.ok(Math.abs(hit.ndcg - 1 / Math.log2(3)) < 1e-12);
  // A variant that hits more users cannot show a lower mean nDCG for it: 1 hit of 2 vs 2 hits of 2 at rank 2.
  const mean = (xs) => xs.reduce((a, b) => a + b, 0) / xs.length;
  assert.ok(mean([hit.ndcg, miss.ndcg]) < mean([hit.ndcg, hit.ndcg]));
  assert.ok(mean([hit.ndcg, miss.ndcg]) <= 0.5);
}

// GRACE-301: the answer's header says how many rows match only when it
// was counted (the builder's COUNT of the same WHERE); the executor's
// filter_docs counts the rows of the segments a LIMIT let it look at, so
// it is not shown as a count of matching rows anywhere.
{
  const { answerFigures } = await import(new URL("results.js", app));
  const j = { rows: Array.from({ length: 20 }, () => [1]), took_ms: 3.2, stats: { filter_docs: 2036, scored: 189 } };
  const text = (f) => f.map(([n, l]) => `${n} ${l}`).join(" · ");
  assert.ok(!/2,036|2036|passed the filter/.test(text(answerFigures(j))), text(answerFigures(j)));
  assert.match(text(answerFigures(j)), /^20 rows · .* · 189 scored$/);
  assert.match(text(answerFigures(j, { matched: 3694 })), /^20 rows · 3,694 match · /);
  assert.match(text(answerFigures({ rows: [[1]], took_ms: 1, stats: {} }, { matched: 1 })), /^1 row · 1 matches · /);
  // The builder counts what its filters keep with the WHERE it ranks by.
  globalThis.fetch = async () => ({ status: 200, ok: true, text: async () => JSON.stringify({ tables: [{ table: "tickets", short: { key: "id", table: "tickets", fields: [
    { name: "id", type: "keyword" }, { name: "domain", type: "keyword" }, { name: "subject", type: "text" }, { name: "created", type: "timestamp" }] } }] }) });
  const { tables } = await import(new URL("catalog.js", app));
  await tables(true);
  const { toSql } = await import(new URL("views/builder.js", app));
  const st = { table: "tickets", signals: [], fusion: "weighted", filters: [{ field: "domain", op: "=", value: "northwindtraders.com" }], any: false, boosts: [], group: null, sort: { field: "created", dir: "desc" }, columns: ["id", "subject"], score: true, limit: 20 };
  const q = toSql(st);
  assert.match(q.sql, /WHERE domain = 'northwindtraders\.com'\nORDER BY created DESC, id\nLIMIT 20;$/);
  assert.equal(q.count, "SELECT COUNT(*) FROM tickets\nWHERE domain = 'northwindtraders.com';");
  assert.equal(toSql({ ...st, filters: [] }).count, null, "no filter, nothing to count but the table");
  globalThis.fetch = async () => ({ status: 200, ok: true, text: async () => "{}" });
}

// NADIA-302: the Graph explorer's PageRank takes an eps (none: the walk
// lowers it from 1e-4 as it needs) and says when the walk stayed near the
// node it started from.
{
  const gx = await import(new URL("views/graph.js", app));
  const q = (eps, dir = "both") => gx.pprSql({ cols: ["id", "title"], nodes: "movies", graph: "ui", id: "'u1'", dir, eps });
  assert.equal(q(""), "SELECT id, title, score() FROM movies ORDER BY ppr(ui, 'u1') DESC LIMIT 15");
  assert.equal(q("1e-6", "out"), "SELECT id, title, score() FROM movies ORDER BY ppr(ui, 'u1', dir => 'out', eps => 0.000001) DESC LIMIT 15");
  assert.deepEqual(gx.EPS.map((e) => e[0]), ["", "1e-5", "1e-6", "1e-7"]);
  const lowered = gx.pprNote({ plan: ["graph 0: ppr over ui, 1 seeds, 3100 rows, 563336 edges followed, 9078 pushes, eps lowered to 3.1e-6, 72% settled, 9.15 ms"] });
  assert.deepEqual(lowered, { lowered: "3.1e-6", settled: 72, warn: null });
  const near = gx.pprNote({ ppr_unsettled: true, plan: ["graph 0: ppr over ui, 1 seeds, 3 rows, 260 edges followed, 3 pushes, 18% settled (its scores hold 18% of the walk, the rest stayed near the seeds: lower eps, or leave it out to let the walk lower it), 1.82 ms"] });
  assert.equal(near.settled, 18);
  assert.match(near.warn, /stayed near the node: its scores hold 18% of the walk; pick a lower eps/);
  assert.match(gx.pprNote({ max_pushes_reached: true, plan: [] }).warn, /stopped at max_pushes/);
}

// WEI-403: the audit banner called a key that never expires "expired": its
// `expires_ms` is null, which compares as 0.
{
  const { expiredBy } = await import(new URL("views/access.js", app));
  assert.equal(expiredBy({ expires_ms: null }, 1_700_000_000_000), false);
  assert.equal(expiredBy({}, 1_700_000_000_000), false);
  assert.equal(expiredBy(undefined, 1_700_000_000_000), false);
  assert.equal(expiredBy({ expires_ms: 1_600_000_000_000 }, 1_700_000_000_000), true);
  assert.equal(expiredBy({ expires_ms: 1_800_000_000_000 }, 1_700_000_000_000), false);
}

// OLU-403: signed out, every read but /v1/health's status is refused, and
// Home's readiness list says what it cannot tell as unknown: no "one
// server", "No alert rules", "Alerts go nowhere" or "No environment named".
{
  const data = await import(new URL("data.js", app));
  const home = await import(new URL("views/overview.js", app));
  Object.assign(data.state, { health: { status: "ok" }, whoami: null, alerts: null, cluster: null, system: null });
  assert.equal(data.roleKnown(), false);
  const items = home.readyItems();
  const titles = items.map((x) => x.title).join(" | ");
  assert.doesNotMatch(titles, /One server|No alert rules|Alerts go nowhere|No environment named/, titles);
  assert.deepEqual(items.filter((x) => x.ok === false), [], titles);
  assert.match(titles, /Alert rules: needs a key to tell/);
  assert.match(titles, /Copies of the data: needs a key to tell/);
  // Signed in to one server with no rules and no route: said as facts.
  Object.assign(data.state, { health: { status: "ok", role: "server" }, whoami: { name: "k", scope: "admin", auth: { open: false } }, alerts: { rules: [], routes: [] } });
  const said = home.readyItems().map((x) => x.title).join(" | ");
  assert.match(said, /One server: its disk holds the only copy/);
  assert.match(said, /Alerts go nowhere/);
  Object.assign(data.state, { health: null, whoami: null, alerts: null });
}

// OLU-401, OLU-402: during a failover the Cluster page says the copy just
// elected takes writes within seconds and not to restart it, offers no
// repair, says the window, and names copies by their --node-name (an
// operator's pod keeps s1-0).
{
  const { state } = await import(new URL("data.js", app));
  const cl = await import(new URL("views/cluster.js", app));
  const cap = (w) => ({ capability: { serves_reads: true, takes_writes: !w, no_writes: w || undefined, failover_after_ms: 2000 } });
  const rep = (port, name, o) => ({ url: `http://127.0.0.1:${port}`, short: `127.0.0.1:${port}`, name, term: 2, ...o });
  state.cluster = { shards: [{ shard: 1, state: "no_writes", failover: true, majority: true, electing: true, failover_window_ms: 2000, takes_writes: false, serves_reads: true, primary: "http://127.0.0.1:17756",
    why: "2 of 3 copies answer: s1c was just elected primary", replicas: [
      rep(17754, "s1a", { status: "down", role: "unknown", last_role: "primary", down_since_ms: 1 }),
      rep(17755, "s1b", { status: "ok", role: "replica", takes_writes: false, detail: cap("a replica takes no writes from clients") }),
      rep(17756, "s1c", { status: "ok", role: "primary", takes_writes: true, detail: cap("a replica takes no writes from clients") })] }] };
  const M = cl.model(state.cluster);
  const sh = M.shards[0];
  const a = cl.critAdvice(sh);
  assert.match(a.next, /^Wait: s1c was just elected primary .*Do not restart it$/);
  assert.doesNotMatch(a.next + a.cause + a.restore, /127\.0\.0\.1|restart it\.|Fix the cause/);
  assert.match(a.cause, /A silent primary is given 2 s \(--failover-after-ms\)/);
  const p = cl.problems(M).find((x) => x.shards.includes(1));
  assert.equal(p.label, "Shard 1 holds writes while it elects a primary");
  assert.match(p.impact, /held for seconds, then answered/);
  assert.equal(p.fix, null, "no repair while it elects");
  // Before the election ends, with no primary at all: a write is refused with a 503 to resend, not held (OLU-501).
  const s0 = state.cluster.shards[0];
  const keep = { primary: s0.primary, replicas: s0.replicas };
  s0.primary = null;
  s0.replicas = s0.replicas.map((r) => (r.role === "primary" ? { ...r, role: "replica", takes_writes: false } : r));
  const p0 = cl.problems(cl.model(state.cluster)).find((x) => x.shards.includes(1));
  assert.equal(p0.label, "Shard 1 refuses writes while it elects a primary");
  assert.match(p0.impact, /answered 503 \(retryable\).*same idempotency key/);
  Object.assign(s0, keep);
  // The same shard once the election is over and the primary cannot write: the old advice.
  state.cluster.shards[0].electing = false;
  assert.match(cl.critAdvice(cl.model(state.cluster).shards[0]).next, /on s1c and restart it$/);
  assert.equal(core.copyName("http://127.0.0.1:17755", state.cluster), "s1b");
  assert.equal(core.shownName({ short: "s1-0", name: "prod-s1-0" }), "s1-0");
  assert.equal(core.shownName({ short: "127.0.0.1:8004", name: "" }), "127.0.0.1:8004");
  assert.match(core.copyTitle("http://127.0.0.1:17755", state.cluster), /^s1b: shard 1, replica · copy 127\.0\.0\.1:17755/);
  state.cluster = null;
}

// Query templates (qtemplate.js): a design becomes the query the
// designer previews, and the CREATE statements it is saved as.
{
  const T = await import(new URL("qtemplate.js", app));
  const t = { table: "products", short: { key: "id", fields: [
    { name: "id", type: "keyword" }, { name: "title", type: "text", indexed: false }, { name: "body", type: "text" }, { name: "brand", type: "keyword" },
    { name: "price", type: "float" }, { name: "sold", type: "int" }, { name: "emb", type: "vector", embed: { model: "hash:64" } } ] } };
  const d = T.blank(t, "Shoe search!");
  assert.equal(d.name, "shoe_search");
  assert.deepEqual(d.words.fields, [{ field: "body", weight: 1 }]);
  // A row shows as a result: its title, a line of text and its facts.
  assert.deepEqual(d.output.columns, ["id", "title", "body", "brand", "price"]);
  d.output.columns = ["id", "title"];
  // One input: one phase, no WITH.
  assert.equal(T.querySql(d, t), "SELECT id, title, score()\nFROM products\nORDER BY bm25(body, :q, syntax => 'plain', fuzzy => 'auto') DESC\nLIMIT 10");
  assert.equal(T.profileSql(d, t), "CREATE OR REPLACE RANK PROFILE shoe_search ON products AS\n  SELECT * FROM products ORDER BY bm25(body, :q, syntax => 'plain', fuzzy => 'auto') DESC");
  // Words and meaning fused; a filter; a boost whose weight is a parameter.
  d.meaning.on = true; d.meaning.field = "emb";
  d.filters = [{ field: "price", op: "<=", value: "200" }];
  d.rank.boosts = [{ kind: "number", field: "sold", w: 0.2, log: true }];
  const sql = T.querySql(d, t);
  // One phase: the fused score times the boosts, the filter in WHERE.
  assert.equal(sql, "SELECT id, title, score()\nFROM products\nWHERE price <= 200\nORDER BY rrf(bm25(body, :q, syntax => 'plain', fuzzy => 'auto'), similarity(emb, :q)) * (1 + :w_sold * ln(1 + coalesce(sold, 0))) DESC\nLIMIT 10");
  d.candidates = 300;
  assert.match(T.firstScore(d, t), /similarity\(emb, :q\), depth => 300\)$/);
  d.fusion = "weighted";
  assert.equal(T.firstScore(d, t), "0.5 * tmm(bm25(body, :q, syntax => 'plain', fuzzy => 'auto', depth => 300)) + 0.5 * tmm(similarity(emb, :q, depth => 300))");
  assert.match(T.querySql(d, t), /ORDER BY \(0\.5 \* tmm\(.*\)\) \* \(1 \+ :w_sold/);
  d.fusion = "rrf"; d.candidates = 100;
  assert.deepEqual(T.params(d, t, "red shoes"), { q: "red shoes", w_sold: 0.2 });
  assert.equal(T.profileSql(d, t), "CREATE OR REPLACE RANK PROFILE shoe_search ON products AS\n  SELECT * FROM products ORDER BY rrf(bm25(body, :q, syntax => 'plain', fuzzy => 'auto'), similarity(emb, :q)) * (1 + :w_sold * ln(1 + coalesce(sold, 0))) DESC\nWITH (w_sold => 0.2)");
  // Retrieval alone: the first phase, cut to the page.
  assert.match(T.querySql(d, t, { upTo: "retrieval" }), /^SELECT id, title, score\(\)\nFROM products\nWHERE price <= 200\nORDER BY rrf\(/);
  // Understanding and expansions are options of the first words field's bm25; RM3 reads the query's syntax.
  d.understand = { values: true, mode: "filter", fields: ["brand", "price"], weight: 0.25 };
  d.words.synonyms = true; d.words.feedback = 5;
  assert.equal(T.inputs(d, t)[0].sql, "bm25(body, :q, fuzzy => 'auto', synonyms => 1, expand => 5, understand => 'filter', understand_fields => 'brand, price')");
  // All the words: a MATCH of the first field.
  d.words.match = "all";
  assert.ok(T.where(d, t).includes("MATCH(body, :q, syntax => 'plain', fuzzy => 'auto', synonyms => 1)"));
  // Saved: a profile, the function apps call, and the design; a blend when on.
  let st = T.saveStatements(d, t);
  assert.deepEqual(st.map((s) => s.what), ["ranking", "template", "design"]);
  assert.match(st[1].sql, /^CREATE OR REPLACE FUNCTION shoe_search\(q TEXT, k INT DEFAULT 10\) RETURNS TABLE OPTIONS \(description = '.*'\) AS\n  SELECT id, title, score\(\) FROM products WHERE MATCH\(body, :q.*\) AND price <= 200 ORDER BY rank_profile\('shoe_search', q => :q\) LIMIT :k$/);
  const body = st[2].sql.slice(st[2].sql.indexOf(" AS ") + 4);
  assert.deepEqual(T.normalize(T.ofBody(body), t).rank.boosts, d.rank.boosts);
  assert.equal(T.ofBody("'blend:{}'"), null);
  d.blend.on = true; d.blend.cap = { per: "brand", at_most: 2 }; d.blend.dedupe = { on: "emb", threshold: 0.95 };
  st = T.saveStatements(d, t);
  assert.deepEqual(st.map((s) => s.what), ["ranking", "blend", "template", "design"]);
  assert.match(st[1].sql, /^CREATE OR REPLACE BLEND shoe_search_blend AS\n  SOURCES \(\n    results AS \(SELECT id, title, brand, emb, score\(\) FROM products WHERE .* ORDER BY rank_profile\('shoe_search', q => :q\) LIMIT 50\)\n  \)\n  MAX 2 PER brand\n  DEDUPE 0.95 ON emb\n  WITH \(k => 10\)$/);
  assert.match(st[2].sql, /SELECT \* FROM blend\('shoe_search_blend', q => :q\) LIMIT :k$/);
  // Blending turned off drops the blend it made.
  d.blend.on = false;
  assert.equal(T.saveStatements(d, t, { hadBlend: true })[1].sql, "DROP BLEND IF EXISTS shoe_search_blend");
  assert.throws(() => T.saveStatements({ ...d, name: "" }, t), /Name the template/);
  assert.throws(() => T.firstScore({ ...d, words: { ...d.words, on: false }, meaning: { ...d.meaning, on: false } }, t), /needs words or meaning/);
  // Heads: the first phase's top rows scored again, each head weighted.
  d.rank.heads = { on: true, depth: 50, fuse: "weighted", norm: "minmax", list: [
    { kind: "words", field: "body", w: 1 }, { kind: "meaning", field: "emb", w: 0.5 }, { kind: "number", field: "sold", log: true, w: -0.2 },
    { kind: "model", model: "clicks", prob: true, features: [{ kind: "words", field: "body" }, { kind: "number", field: "price" }], w: 0.3 } ] };
  let ps = T.phases(d, t);
  assert.deepEqual(ps.map((p) => [p.name, p.limit]), [["candidates", 50], ["ranked", null]]);
  assert.equal(ps[1].by, "1 * minmax(bm25(body, :q, syntax => 'plain')) + 0.5 * minmax(similarity(emb, :q)) - 0.2 * minmax(ln(1 + coalesce(sold, 0))) + 0.3 * minmax(sigmoid(gbdt('clicks', bm25(body, :q, syntax => 'plain'), coalesce(price, 0))))");
  // The heads' phase reads the first's rows; the first keeps its text scores' options (and their rewrites).
  assert.match(T.querySql(d, t), /^WITH candidates AS \(SELECT \* FROM products WHERE MATCH\(body.*expand => 5.*LIMIT 50\)\nSELECT id, title, score\(\) FROM candidates\nORDER BY 1 \* minmax/);
  assert.doesNotMatch(T.querySql(d, t).split("\n").slice(1).join("\n"), /expand|understand/);
  d.rank.heads.fuse = "rrf";
  assert.equal(T.headsScore(d, t), "rrf(bm25(body, :q, syntax => 'plain'), similarity(emb, :q), ln(1 + coalesce(sold, 0)), sigmoid(gbdt('clicks', bm25(body, :q, syntax => 'plain'), coalesce(price, 0))), weights => [1, 0.5, -0.2, 0.3])");
  assert.throws(() => T.headsScore({ ...d, rank: { ...d.rank, heads: { ...d.rank.heads, list: [{ kind: "model", model: "m", features: [] }] } } }, t), /needs its features/);
  // A cross-encoder head over the top 20, the one phase after the first.
  d.rank.heads = { on: true, depth: 20, fuse: "weighted", norm: "minmax", list: [{ kind: "cross", model: "ms-marco-MiniLM-L-6-v2", fields: ["title"], w: 1 }] };
  ps = T.phases(d, t);
  assert.deepEqual(ps.map((p) => [p.name, p.limit]), [["candidates", 20], ["ranked", null]]);
  assert.equal(ps[1].by, "cross_encoder('ms-marco-MiniLM-L-6-v2', title, :q)");
  // A design of version 1 with a rerank opens as that head.
  const old = T.normalize({ ...d, rank: { boosts: [], rerank: { on: true, model: "bge-reranker-base", fields: [], depth: 15 } } }, t);
  assert.deepEqual([old.rank.heads.on, old.rank.heads.depth, old.rank.heads.list[0].kind, old.rank.rerank.on], [true, 15, "cross", false]);
  // A blend of several queries: this template's, another template's, a table's, any query.
  d.rank.heads.on = false;
  d.blend = { ...d.blend, on: true, boost: 0.1, sources: [
    { kind: "template", template: "news_search", depth: 15, boost: -0.1 }, { kind: "sql", name: "ads", sql: "SELECT *, score() FROM ads ORDER BY bm25(text, :q) DESC LIMIT 5;" }, { kind: "template", template: "" } ] };
  const def = T.blendDef(d, t, { saved: true });
  assert.deepEqual(def.sources.map((s) => [s.name, s.sql.slice(0, 40)]), [["results", "SELECT id, title, brand, emb, score() FR"], ["news_search", "SELECT * FROM news_search(q => :q, k => "], ["ads", "SELECT *, score() FROM ads ORDER BY bm25"]]);
  assert.equal(def.sources[2].sql, "SELECT *, score() FROM ads ORDER BY bm25(text, :q) DESC LIMIT 5");
  assert.deepEqual(def.boost, [{ source: "results", by: 0.1 }, { source: "news_search", by: -0.1 }]);
  assert.match(T.blendSql(def), /BOOST \(results 0\.1, news_search -0\.1\)/);
}

// GRACE-401: a search's head says how many rows match (a COUNT with the
// WHERE the words keep), of them how many have the words as typed, and
// pages by OFFSET; GRACE-402: in Both, a row with every word comes first
// (rrf's fused rank plus 1 for it), so the ticket that says "billing" and
// "refund" is not ranked under rows close in meaning only.
{
  const tk = { table: "tickets", short: { key: "id", fields: [{ name: "id", type: "keyword" }, { name: "subject", type: "text" }, { name: "emb", type: "vector", dim: 4 }] } };
  const [, subject, emb] = tk.short.fields;
  const both = tableView.searchSql(tk, { q: "billing refund", fields: [subject], vec: emb, mode: "both", offset: 20 });
  assert.equal(both.sql, "SELECT id, subject, score()\nFROM tickets\nORDER BY rrf(bm25(subject, :q, fuzzy => 'auto'), similarity(emb, :v))\n  + CASE WHEN MATCH(subject, :q, fuzzy => 'auto', min => '100%') THEN 1 ELSE 0 END DESC\nLIMIT 20 OFFSET 20");
  assert.deepEqual(both.count, { sql: "SELECT COUNT(*) FROM tickets\nWHERE MATCH(subject, :q, fuzzy => 'auto', min => 1, min_rows => 20)", params: { q: "billing refund" } });
  assert.equal(both.exact.sql, "SELECT COUNT(*) FROM tickets\nWHERE MATCH(subject, :q, min => 1, min_rows => 20)");
  assert.equal(both.all.sql, "SELECT id, subject\nFROM tickets\nWHERE MATCH(subject, :q, fuzzy => 'auto', min => 1, min_rows => 20)");
  assert.ok(!/OFFSET/.test(tableView.searchSql(tk, { q: "x", fields: [subject], vec: null, mode: "words" }).sql));
  assert.equal(tableView.searchSql(tk, { q: "x", fields: [], vec: emb, mode: "meaning" }).count, null, "by meaning every row is ranked: nothing to count");
  assert.equal(tableView.matchLine({ mode: "words", matched: 7436, exact: 5022, rows: 20000, n: 20, offset: 20 }), "21–40 of 7,436 rows that have the words (5,022 with the words as typed, the others with a word a letter or two from one)");
  assert.equal(tableView.matchLine({ mode: "both", matched: 1, exact: 1, rows: 9, n: 1 }), "1–1 of 1 row that has the words, those with every word first, then by words and meaning");
  assert.equal(tableView.matchLine({ mode: "both", matched: 4, exact: 4, rows: 30, n: 20 }), "1–20: the 4 rows that have the words first, then the closest by meaning");
  assert.match(tableView.matchLine({ mode: "meaning", rows: 20000, n: 20 }), /^1–20 · by meaning, every one of the 20,000 rows is ranked/);
  // GRACE-403: the Data tab's CSV of a filtered list reads every row its
  // WHERE keeps (no LIMIT), not the 50 on screen.
  assert.equal(tableView.dataSql(tk, tk.short.fields.slice(0, 2), { where: " status = 'open' ", by: "id", desc: true }), "SELECT id, subject FROM tickets WHERE status = 'open' ORDER BY id DESC");
  assert.equal(tableView.dataSql(tk, tk.short.fields.slice(0, 2), { limit: 50, offset: 100 }), "SELECT id, subject FROM tickets LIMIT 50 OFFSET 100");
  const { jsonLines } = big;
  assert.equal(jsonLines(["id", "tags"], [["t1", ["a", "b"]], ["t2", null]]), '{"id":"t1","tags":["a","b"]}\n{"id":"t2","tags":null}\n');
}

// GRACE-404: the builder groups a time by week (date_trunc, Monday
// weeks), measures the median hours between two times and each group's
// share of its week, in time order; GRACE-417: "is one of" with two picks
// turned to "is not" is NOT IN of both, not one value "open, new".
{
  globalThis.fetch = async () => ({ status: 200, ok: true, text: async () => JSON.stringify({ tables: [{ table: "tickets", short: { key: "id", table: "tickets", fields: [
    { name: "id", type: "keyword" }, { name: "status", type: "keyword" }, { name: "priority", type: "keyword" }, { name: "subject", type: "text" }, { name: "created", type: "timestamp" }, { name: "solved", type: "timestamp" }] } }] }) });
  const { tables } = await import(new URL("catalog.js", app));
  await tables(true);
  const b = await import(new URL("views/builder.js", app));
  const st = { table: "tickets", signals: [], fusion: "weighted", filters: [], any: false, boosts: [], sort: null, columns: [], score: true, limit: 1000,
    group: { by: ["created", "priority"], bins: { created: "week" }, measures: [{ fn: "count" }, { fn: "median", field: "hours:created:solved" }, { fn: "share", within: "created" }], min: 0 } };
  assert.equal(b.toSql(st).sql, "SELECT date_trunc('week', created) AS created_week, priority, COUNT(*) AS n, MEDIAN(date_diff('minute', created, solved) / 60.0) AS median_hours_to_solved, ROUND(100.0 * COUNT(*) / SUM(COUNT(*)) OVER (PARTITION BY date_trunc('week', created)), 1) AS pct_of_created\nFROM tickets\nGROUP BY date_trunc('week', created), priority\nORDER BY date_trunc('week', created), priority\nLIMIT 1000;");
  const month = b.toSql({ ...st, group: { by: ["created"], bins: { created: "month" }, measures: [{ fn: "share" }], min: 0 } }).sql;
  assert.match(month, /^SELECT date_trunc\('month', created\) AS created_month, ROUND\(100\.0 \* COUNT\(\*\) \/ SUM\(COUNT\(\*\)\) OVER \(\), 1\) AS pct\n/);
  assert.deepEqual(b.hoursOf("hours:created:solved"), { from: "created", to: "solved" });
  assert.equal(b.hoursOf("solved"), null);
  const picks = b.changeOp({ field: "status", op: "in", value: "open, new" }, "!=");
  assert.equal(picks.op, "notin");
  assert.match(b.toSql({ ...st, group: null, columns: ["id"], filters: [picks] }).sql, /\nWHERE status NOT IN \('open', 'new'\)\n/);
  assert.equal(b.changeOp({ field: "status", op: "notin", value: "open, new" }, "=").op, "in");
  assert.equal(b.changeOp({ field: "status", op: "in", value: "open" }, "!=").op, "!=", "one pick stays one value");
  assert.equal(b.changeOp({ field: "status", op: "in", value: "a, b" }, "prefix").value, "");
  const { operators } = await import(new URL("catalog.js", app));
  assert.ok(operators({ name: "status", type: "keyword" }).some(([op]) => op === "notin"));
  globalThis.fetch = async () => ({ status: 200, ok: true, text: async () => "{}" });
}

// GRACE-405: a column whose values list tags ("billing refund web_form")
// is found and split at import, the separator shown and changeable; names
// and sentences are not split.
{
  const tags = ["billing refund", "refund web_form", "billing", "email us", "billing email", "us", "web_form email", "refund us", "email", "billing us"];
  assert.equal(imp.listSeparator(tags), " ");
  assert.equal(imp.listSeparator(["a;b", "b;c", "a", "c;a"]), ";");
  assert.equal(imp.listSeparator(["Ana Lima", "Bo Chen", "Cy Diaz", "Di Eng", "Ed Fox"]), null);
  assert.equal(imp.listSeparator(["Please refund me.", "My card was charged twice!", "Where is it?", "Help now please"]), null);
  assert.deepEqual(imp.LIST_SEPS.map(([s]) => s), [" ", ",", ";", "|"]);
  assert.equal(imp.splitLists('id,tags,name\n1,billing refund,"Lima, Ana"\n2,email,Bo\n3,,Cy\n', { tags: " " }), 'id,tags,name\n1,billing|refund,"Lima, Ana"\n2,email,Bo\n3,,Cy\n');
  assert.equal(imp.splitLists("id,tags\n1,a|b;c\n", { tags: ";" }), 'id,tags\n1,"[""a|b"",""c""]"\n', "an item that holds a | is a JSON array");
}

// GRACE-406: a chart splits its series by a column, a line per value
// (the most rows first), a missing point NULL; whole days read "18 May".
{
  const r = await import(new URL("results.js", app));
  const cols = ["created_week", "priority", "n"];
  const rows = [["2026-11-02T00:00:00Z", "high", 3], ["2026-11-02T00:00:00Z", "low", 5], ["2026-11-09T00:00:00Z", "high", 4], ["2026-11-09T00:00:00Z", "urgent", 1], ["2026-11-09T00:00:00Z", "low", 2]];
  assert.deepEqual(r.chartSeries(cols, rows, { x: "created_week", ys: ["n"], split: "priority" }),
    { names: ["high", "low", "urgent"], pts: [{ x: "2026-11-02T00:00:00Z", ys: [3, 5, null] }, { x: "2026-11-09T00:00:00Z", ys: [4, 2, 1] }], left: 0 });
  assert.deepEqual(r.chartSeries(cols, rows, { x: "created_week", ys: ["n"] }).names, ["n"]);
  const many = Array.from({ length: r.MAX_SERIES + 3 }, (_, i) => ["w", `p${i}`, i]);
  assert.equal(r.chartSeries(cols, many, { x: "created_week", ys: ["n"], split: "priority" }).left, 3);
  assert.equal(r.timeLabel([Date.UTC(2026, 4, 18)])(Date.UTC(2026, 4, 18)), "18 May");
}

// GRACE-407: a team view is a row of cc_views, its id its name's slug
// (saving the same name replaces it), its chart and the builder's state
// in spec; a missing table reads as no views yet.
{
  const v = await import(new URL("views/views.js", app));
  const doc = v.viewDoc({ name: " Monday numbers ", kind: "builder", sql: "SELECT 1 ", params: { a: 1 }, chart: { x: "w", ys: ["n"], split: "p", kind: "line" }, by: "grace", at: Date.UTC(2026, 10, 20) });
  assert.deepEqual(doc, { id: "monday_numbers", name: "Monday numbers", about: "", kind: "builder", sql: "SELECT 1", spec: { params: { a: 1 }, chart: { x: "w", ys: ["n"], split: "p", kind: "line" }, state: null }, by: "grace", at: "2026-11-20T00:00:00.000Z" });
  assert.throws(() => v.viewDoc({ name: " ", sql: "SELECT 1" }), /needs a name/);
  assert.throws(() => v.viewDoc({ name: "x", sql: "" }), /needs a query/);
  assert.ok(v.noTable(new ApiError(404, "no such table: cc_views", { error_code: "table_not_found" })));
  assert.ok(!v.noTable(new ApiError(404, "no such table: tickets", { error_code: "table_not_found" })));
  assert.ok(!v.noTable(new Error("no such table: cc_views")));
  assert.match(v.TABLE_SQL, /^CREATE TABLE cc_views \(id KEYWORD PRIMARY KEY, .*spec JSON/);
  assert.equal(owners.ownerOf("cc_views")?.owner, "Command center", "the views table is the command center's, folded with the apps' tables");
}

console.log("ok");
process.exit(0);

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
assert.match(s.body, /bm25\(body, :q\)/);
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
assert.ok(busy >= 8 && resting <= Math.ceil(busy / core.IDLE_SLOW) + 1, `polls while used ${busy}, while idle ${resting}`);
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

console.log("ok");
process.exit(0);

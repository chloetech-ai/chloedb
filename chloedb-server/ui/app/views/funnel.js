// Query funnel: everything a query does between its text and its rows,
// stage by stage (`chloedb::query::funnel`). Two tabs. All queries lays
// the time of the queries this server ran (its query log, the latest 500)
// on the stages, together, by statement and the slowest one by one, with
// the rows the stages' steps held on average (GET /v1/queries/funnel).
// One query runs a query once (POST /v1/funnel) and shows its stages in
// order (understood words, the terms its words became with their typos
// and "did you mean", the texts embedded, the plan, the match, WHERE, the
// ranking, a rerank, a blend, the shards), the rows left after each, and
// every row it saw with where it left the funnel and why. A query of the
// log opens with its time as it ran (?id=), and runs again to trace its
// rows. Queries > a query's drawer > "Its funnel" opens it here.

import { h, icon, clear, fmt, api, card, empty, copy, seg, failure, table, store, tipOn, sqlBlock, link } from "../core.js";
import { paramsObj, paramsText, outcomePill, waterfall } from "../workload.js";
import { flameChart, stepTree, kpi } from "../profile.js";

export default { id: "funnel", title: "Query funnel", icon: "filter", group: "Operate", keywords: "query funnel stages pipeline matching selection candidates ranking rerank blending filtered dropped kept why where each row left latency time spent understanding spell correction did you mean typos terms synonyms embeddings embed plan shards", render, onParams, range: false };

/** The stages, in the order a query meets them: their names and colours. */
export const STAGES = [
  ["wait", "Wait", "var(--axis)"],
  ["understand", "Understand", "var(--s5)"],
  ["expand", "Expand terms", "var(--s6)"],
  ["embed", "Embed", "var(--s7)"],
  ["plan", "Plan", "var(--s3)"],
  ["match", "Match", "var(--s1)"],
  ["select", "Select", "var(--s2)"],
  ["rank", "Rank", "var(--s4)"],
  ["rerank", "Rerank", "var(--s8)"],
  ["blend", "Blend", "var(--s5)"],
  ["shards", "Shards", "var(--muted)"],
  ["fetch", "Fetch", "var(--line-2)"],
  ["other", "Other", "var(--grid)"],
];
const NAME = Object.fromEntries(STAGES.map(([k, n]) => [k, n]));
const COLOR = Object.fromEntries(STAGES.map(([k, , c]) => [k, c]));

/** What leaving the funnel at each fate means, and its tone. */
export const FATES = {
  shown: ["On the page", "ok"],
  offset: ["Before the page (OFFSET)", "info"],
  reranked: ["Reranked off the page", "warn"],
  cut: ["Ranked past the page", "warn"],
  not_candidate: ["Not a candidate", "info"],
  filtered: ["Dropped by WHERE", "bad"],
};

const WINDOWS = [["300000", "Last 5 min"], ["3600000", "Last hour"], ["0", "Everything kept"]];

let reroute = null;
function onParams(r) { reroute?.(r.params); }

/** A stacked bar of the stages' time: `ms` by stage key, on one scale. */
export function stageBar(ms, { height = 14, total } = {}) {
  const all = total ?? Object.values(ms).reduce((a, x) => a + (x || 0), 0);
  const bar = h("div", { class: "fn-stack", style: { height: height + "px" } });
  if (!all) return bar;
  for (const [k, name] of STAGES) {
    const v = ms[k] || 0;
    if (v <= 0) continue;
    const seg = h("div", { class: "fn-seg", style: { width: (v / all * 100) + "%", background: COLOR[k] } });
    tipOn(seg, [h("div", { class: "t" }, name), h("div", { class: "r" }, h("b", {}, fmt.ms(v)), h("span", { class: "n" }, fmt.pct(v / all)))]);
    bar.append(seg);
  }
  return bar;
}

/** The stages that took time, as a legend. */
function legend(ms) {
  return h("div", { class: "fn-legend" }, STAGES.filter(([k]) => (ms[k] || 0) > 0).map(([k, n]) => h("span", { class: "row nw" }, h("span", { class: "fn-dot", style: { background: COLOR[k] } }), n)));
}

const msOf = (stages) => Object.fromEntries((stages || []).map((s) => [s.stage, s.ms ?? s.mean_ms ?? 0]));

function render(page, r) {
  const tabs = h("div", { class: "tabs", role: "tablist" });
  const body = h("div");
  page.append(
    h("div", { class: "page-head" },
      h("div", {}, h("h1", {}, "Query funnel"),
        h("p", {}, "Everything a query does between its text and its rows: the time each stage takes, and the rows each stage keeps and drops, across the queries this server ran or for one query.")),
      h("div", { class: "tools" }, h("button", { class: "btn sm", title: "Copy a link to this view", onclick: () => copy(location.href, "Link copied") }, icon("link"), "Copy link"))),
    tabs, body);
  let tab = null, key = "", stop = null;
  function drawTabs() {
    clear(tabs, [["all", "All queries"], ["one", "One query"]].map(([k, v]) => h("button", { role: "tab", "aria-selected": String(k === tab), onclick: () => { location.hash = link("funnel", { tab: k }); } }, v)));
  }
  function show(params) {
    const t = params.get("tab") === "one" || params.get("id") || params.get("sql") ? "one" : "all";
    const k = t + "|" + [...params].filter(([a]) => !["from", "to", "range"].includes(a)).map((x) => x.join("=")).join("&");
    if (k === key) return;
    key = k; tab = t;
    stop?.(); stop = null;
    drawTabs();
    clear(body);
    stop = t === "one" ? oneQuery(body, params) : allQueries(body, params);
  }
  reroute = show;
  show(r?.params || new URLSearchParams());
  return () => { reroute = null; stop?.(); };
}

// ---------------------------------------------------------------- all queries

function allQueries(el, params) {
  let since = params.get("window") || store.get("funnel.window", "3600000");
  const fp = params.get("fp") || "";
  const tableName = params.get("table") || "";
  const out = h("div", { class: "stack" }, h("div", { class: "small muted" }, "Reading the query log…"));
  const go = (extra) => { location.hash = link("funnel", { tab: "all", window: since, fp, table: tableName, ...extra }); };
  const tableIn = h("input", { class: "input", style: { width: "160px" }, placeholder: "any table", value: tableName, "aria-label": "Only the queries of a table", onchange: () => go({ table: tableIn.value.trim() }) });
  el.append(
    h("div", { class: "row", style: { gap: "8px", flexWrap: "wrap", marginBottom: "12px" } },
      seg(WINDOWS, since, (v) => { since = v; store.set("funnel.window", v); go({ window: v }); }),
      tableIn,
      fp ? h("span", { class: "chip", "aria-pressed": "true", title: "Only this statement's queries", onclick: () => go({ fp: "" }) }, "one statement", h("span", { class: "x" }, "✕")) : null,
      h("span", { class: "grow" }),
      h("button", { class: "btn sm", onclick: load }, icon("refresh"), "Refresh")),
    out);
  async function load() {
    const q = new URLSearchParams();
    if (+since) q.set("since", String(Date.now() - +since));
    if (fp) q.set("fingerprint", fp);
    if (tableName) q.set("table", tableName);
    let j;
    try { j = await api("/v1/queries/funnel?" + q); } catch (e) { clear(out, failure(e, "The query funnel")); return; }
    if (!j.queries) {
      clear(out, card({ body: empty("filter", "No queries to show", `${+since ? "None in this window" : "The query log holds none"}${tableName ? ` of ${tableName}` : ""}. The funnel lays the time of the queries this server ran on its stages; run a few, or trace one in the One query tab.`, h("a", { class: "btn sm", href: link("funnel", { tab: "one" }) }, "Trace one query")) }));
      return;
    }
    const stages = j.stages.filter((s) => s.ran > 0);
    const mean = Object.fromEntries(stages.map((s) => [s.stage, s.mean_ms]));
    const top = stages.slice().sort((a, b) => b.share - a.share)[0];
    clear(out,
      h("div", { class: "grid g4" },
        kpi("Queries", fmt.n(j.queries), `of the latest ${fmt.n(j.kept)} the server keeps`),
        kpi("Mean", fmt.ms(j.mean_ms), `p50 ${fmt.ms(j.p50_ms)}`),
        kpi("p95", fmt.ms(j.p95_ms)),
        kpi("Most time in", top ? NAME[top.stage] : "–", top ? `${fmt.pct(top.share)} of all their time` : null)),
      card({ title: "Where the time goes", sub: "each stage's share of every query's time, its mean over all of them and its percentiles over those that ran it",
        body: [stageBar(mean, { height: 22 }), legend(mean), stageTable(stages, j.queries)] }),
      rowsCard(j.stage_rows),
      statementsCard(j.statements),
      slowestCard(j.slowest));
  }
  load();
  return null;
}

function stageTable(stages, n) {
  return table([
    { key: "name", label: "Stage", render: (s) => h("span", { class: "row nw" }, h("span", { class: "fn-dot", style: { background: COLOR[s.stage] } }), h("b", {}, s.name)) },
    { key: "says", label: "What it is", sortable: false, render: (s) => h("span", { class: "small dim" }, s.says) },
    { key: "ran", label: "Ran in", num: true, render: (s) => `${fmt.n(s.ran)} of ${fmt.n(n)}` },
    { key: "mean_ms", label: "Mean", num: true, render: (s) => fmt.ms(s.mean_ms) },
    { key: "p50_ms", label: "p50", num: true, render: (s) => fmt.ms(s.p50_ms) },
    { key: "p95_ms", label: "p95", num: true, render: (s) => fmt.ms(s.p95_ms) },
    { key: "max_ms", label: "Max", num: true, render: (s) => fmt.ms(s.max_ms) },
    { key: "share", label: "Share", num: true, render: (s) => h("div", { class: "row nw", style: { gap: "8px" } }, h("div", { class: "barbox" }, h("div", { class: "bar", style: { width: Math.min(100, s.share * 100) + "%", background: COLOR[s.stage] } })), h("span", { class: "small dim num" }, fmt.pct(s.share))) },
  ], { rows: stages, compact: true });
}

/** The rows each stage's steps held, on average: the funnel of many queries. */
function rowsCard(rows) {
  const order = ["match", "select", "rank", "rerank", "blend", "fetch"].filter((k) => rows?.[k]);
  if (!order.length) return null;
  const max = Math.max(...order.map((k) => rows[k].mean), 1);
  return card({ title: "Rows at each stage", sub: "on average, the most rows a step of each stage held: what the matching found, what WHERE kept, what the ranking ranked and what was answered",
    body: h("div", { class: "fn-funnel" }, order.map((k) => funnelRow(NAME[k], rows[k].mean, max, COLOR[k], `${fmt.plural(rows[k].queries, "query", "queries")}`))) });
}

/** One bar of a funnel, on a log scale so that 10 rows of a million show. */
function funnelRow(label, n, max, color, note, extra) {
  const w = n > 0 ? Math.max(1.5, Math.log10(n + 1) / Math.log10(max + 1) * 100) : 0;
  return h("div", { class: "fn-frow" },
    h("div", { class: "fn-flabel" }, label),
    h("div", { class: "fn-fbar" }, h("div", { style: { width: w + "%", background: color } })),
    h("div", { class: "fn-fn num" }, n == null ? "–" : fmt.n(n, n < 10 && n % 1 ? 1 : 0)),
    h("div", { class: "small dim fn-fnote" }, note || "", extra || null));
}

function statementsCard(list) {
  if (!list?.length) return null;
  return card({ title: "By statement", sub: "the queries of one shape together, the most time first · click one to see only its queries", flush: true,
    body: table([
      { key: "sql", label: "Statement", render: (x) => h("span", { class: "mono small ellipsis", style: { maxWidth: "420px", display: "inline-block" }, title: x.sql }, x.sql) },
      { key: "queries", label: "Queries", num: true },
      { key: "mean_ms", label: "Mean", num: true, render: (x) => fmt.ms(x.mean_ms) },
      { key: "p95_ms", label: "p95", num: true, render: (x) => fmt.ms(x.p95_ms) },
      { key: "stages", label: "Its stages", sortable: false, width: "220px", render: (x) => stageBar(x.stages) },
      { label: "", sortable: false, render: (x) => h("a", { class: "btn ghost sm", href: link("funnel", { tab: "one", id: x.last_id }), title: "Its latest query's stages, and its rows traced" }, "Its funnel", icon("right")) },
    ], { rows: list, onRow: (x) => { location.hash = link("funnel", { tab: "all", fp: x.fingerprint }); } }) });
}

function slowestCard(list) {
  if (!list?.length) return null;
  return card({ title: "Slowest queries", sub: "one by one, each with its stages · click one to see it, and to trace its rows", flush: true,
    body: table([
      { key: "ts_ms", label: "When", render: (x) => h("span", { class: "small nw" }, fmt.time(x.ts_ms)) },
      { key: "sql", label: "Query", render: (x) => h("span", { class: "mono small ellipsis", style: { maxWidth: "380px", display: "inline-block" }, title: x.sql + (x.params ? "\n" + paramsText(x.params, 400) : "") }, x.sql) },
      { key: "outcome", label: "", sortable: false, render: (x) => x.outcome === "ok" ? null : outcomePill(x.outcome) },
      { key: "rows", label: "Rows", num: true },
      { key: "ms", label: "Time", num: true, render: (x) => fmt.ms(x.ms) },
      { key: "stages", label: "Its stages", sortable: false, width: "220px", render: (x) => stageBar(x.stages, { total: x.ms }) },
    ], { rows: list, onRow: (x) => { location.hash = link("funnel", { tab: "one", id: x.id }); } }) });
}

// ---------------------------------------------------------------- one query

function oneQuery(el, params) {
  const id = params.get("id");
  const sql = h("textarea", { class: "input mono", rows: 4, style: { width: "100%" }, spellcheck: "false", "aria-label": "The query to trace", placeholder: "SELECT id, title FROM docs WHERE year >= 2020 ORDER BY rrf(bm25(body, :q), similarity(emb, :q)) DESC LIMIT 10" });
  const pin = h("input", { class: "input mono", style: { width: "100%" }, placeholder: '{"q": "red chair"}', "aria-label": "Its parameters, as JSON" });
  sql.value = params.get("sql") ?? store.get("funnel.sql", "");
  pin.value = params.get("p") ?? store.get("funnel.p", "");
  const runBtn = h("button", { class: "btn primary", onclick: () => run() }, icon("play"), "Trace it");
  const note = h("div", { class: "small muted" }, "Runs the query once, then the funnel's probes: counts and top rows of each stage, quiet (not in the query log, not in the time shown).");
  const out = h("div", { class: "stack" });
  sql.addEventListener("keydown", (e) => { if ((e.metaKey || e.ctrlKey) && e.key === "Enter") run(); });
  el.append(card({ title: "The query", body: h("div", { class: "stack", style: { gap: "8px" } }, sql, h("div", { class: "row", style: { gap: "8px" } }, h("div", { class: "grow" }, pin), runBtn), note) }), out);

  async function run() {
    const text = sql.value.trim();
    if (!text) { clear(out, h("div", { class: "notice" }, "Write a query to trace: a search of one table shows its rows stage by stage, any other its time.")); return; }
    let p = null;
    if (pin.value.trim()) { try { p = JSON.parse(pin.value); } catch { clear(out, h("div", { class: "error" }, "The parameters are JSON: {\"q\": \"red chair\"}")); return; } }
    store.set("funnel.sql", text); store.set("funnel.p", pin.value);
    runBtn.disabled = true;
    clear(out, h("div", { class: "small muted" }, "Running it, then its probes…"));
    const t0 = performance.now();
    try {
      const j = await api("/v1/funnel", { body: p ? { sql: text, params: p } : { sql: text } });
      clear(out, oneResult(j, performance.now() - t0, { rerun: (s, ps) => { sql.value = s; pin.value = ps ? JSON.stringify(ps) : ""; run(); } }));
    } catch (e) {
      clear(out, failure(e, "The funnel"));
    } finally { runBtn.disabled = false; }
  }

  if (id) {
    clear(out, h("div", { class: "small muted" }, `Reading query ${id} from the log…`));
    const given = params.get("sql");
    api("/v1/queries/funnel?id=" + encodeURIComponent(id)).then((j) => {
      const q = j.query || {};
      // A number of another log (the log store's history): not this query.
      if (given && q.sql !== given) throw Object.assign(new Error(`This server's query ${id} is another query`), { status: 404 });
      sql.value = q.sql || "";
      const ps = paramsObj(q.params);
      pin.value = ps ? JSON.stringify(ps) : "";
      const cut = ps && Object.values(ps).some((v) => typeof v === "string" && (/\.\.\.$/.test(v) || /^\[\d+ values\]$/.test(v)));
      clear(out,
        h("div", { class: "notice" }, `Query ${q.id}, as it ran ${fmt.datetime(q.ts_ms)}: its time by stage. `, h("b", {}, "Trace it"), " runs it again to follow its rows.",
          cut ? " Its log cut a long parameter: write it out before you run it." : ""),
        timeCard(j, null),
        stepsCard(j));
    }).catch((e) => clear(out, e.status === 404 ? h("div", { class: "notice" }, `${e.message.replace(/\.$/, "")}: its time as it ran is not kept. `, h("b", {}, "Trace it"), given ? " runs it again." : " runs a query you write above.") : failure(e, "The query")));
  } else if (params.get("sql")) run();
  return null;
}

/** Everything one funnel answered. */
function oneResult(j, clientMs, { rerun }) {
  const parts = [];
  const ms = msOf(j.stages);
  const rows = j.rows;
  const total = j.took_ms;
  const top = (j.stages || []).filter((s) => s.ms > 0 && s.stage !== "other").sort((a, b) => b.ms - a.ms)[0];
  // The sentence.
  const say = [`Took ${fmt.ms(total)}`];
  if (top) say.push(`, most of it in ${NAME[top.stage]} (${fmt.ms(top.ms)}, ${fmt.pct(top.share)})`);
  say.push(".");
  if (rows && rows.kind !== "other") {
    const chain = [rows.live != null ? `${fmt.n(rows.live)} rows in the table` : null, rows.matched != null && rows.matched !== rows.live ? `${fmt.n(rows.matched)} matched` : null,
      rows.selected != null && rows.conditions?.length ? `${fmt.n(rows.selected)} kept by WHERE` : null, rows.candidates != null && rows.fused ? `${fmt.n(rows.candidates)} candidates ranked` : null, `${fmt.n(rows.shown)} on the page`].filter(Boolean);
    say.push(" ", chain.join(" → "), ".");
  }
  if (j.ran && j.ran !== j.sql) say.push(" It ran as rewritten by its understood or expanded words (below).");
  parts.push(h("div", { class: "wl-story" }, say));
  parts.push(h("div", { class: "grid g4" },
    kpi("Server time", fmt.ms(total), `${fmt.ms(clientMs)} with the probes, from this page`),
    kpi("Rows answered", fmt.n(j.answer?.total_rows ?? 0)),
    rows ? kpi("Rows seen", fmt.n(rows.rows?.length ?? 0), "by the probes, each with where it left") : kpi("Kind", j.kind),
    kpi("Stages", fmt.n((j.stages || []).filter((s) => s.ms > 0).length), "that took any time")));
  parts.push(timeCard(j, rows));
  if (rows && rows.kind !== "other") parts.push(rowsFunnel(rows), fatesCard(rows));
  else if (j.kind === "other" || j.kind === "compound") parts.push(h("div", { class: "notice" }, j.kind === "compound" ? "A query of several tables (a join, WITH, a subquery, UNION): its time is shown, and its rows are its parts'. Trace each part on its own." : "A query of groups or counts: its time is shown; it ranks no rows to follow."));
  if (j.blend) parts.push(blendCard(j, rerun));
  for (const n of [...(rows?.notes || []), ...(j.notes || [])]) parts.push(h("div", { class: "small muted" }, n));
  parts.push(stepsCard(j));
  if (j.answer) parts.push(answerCard(j.answer));
  return parts;
}

/** The stages in order, each with its time and what it did. */
function timeCard(j, rows) {
  const ms = msOf(j.stages);
  const total = j.took_ms || Object.values(ms).reduce((a, b) => a + b, 0);
  const text = j.text || {};
  const detail = {
    wait: () => { const w = (j.spans || []).filter((s) => s.name === "admission" || s.name === "wait for writes"); return w.length ? w.map((s) => `${s.name} ${fmt.ms(s.dur_us / 1000)}`).join(" · ") : null; },
    understand: () => text.understood?.length ? h("div", {}, text.understood.map((l) => h("div", { class: "mono small" }, l))) : null,
    expand: () => [text.expanded?.length ? h("div", {}, text.expanded.map((l) => h("div", { class: "mono small" }, l))) : null, ...(text.searches || []).map(termsOf)],
    embed: () => text.embedded?.length ? h("div", {}, text.embedded.map((e) => h("div", { class: "small" }, h("b", {}, e.table ? `${e.table}.${e.field}` : e.model), e.model && e.table ? h("span", { class: "dim" }, ` by ${e.model}`) : null, ": ", h("span", { class: "mono" }, `"${e.text}"`), e.ms != null ? h("span", { class: "dim" }, ` · ${fmt.ms(e.ms)}`) : null))) : null,
    plan: () => j.ran && j.ran !== j.sql ? h("div", {}, h("div", { class: "small dim" }, "It ran as:"), sqlBlock(j.ran)) : null,
    match: () => rows?.inputs?.length ? h("div", {}, rows.inputs.map((i) => h("div", { class: "small" }, h("b", { class: "mono" }, i.label), i.text ? h("span", { class: "mono" }, ` "${i.text}"`) : null, ": ",
      i.matched != null ? `${fmt.plural(i.matched, "row")} match` : i.kind === "similarity" || i.kind === "vector" || !i.generates ? "ranks every row that has a value" : "–",
      i.generates && rows.fused ? h("span", { class: "dim" }, ` · its top ${fmt.n(i.depth)} after WHERE are candidates (${fmt.n(i.candidates)})`) : null))) : null,
    select: () => rows?.conditions?.length ? h("div", {}, rows.conditions.map((c, k) => h("div", { class: "small" }, h("span", { class: "mono" }, c.text), ": ", c.left == null ? "–" : `${fmt.plural(c.left, "row")} left`, k === 0 && rows.matched != null ? h("span", { class: "dim" }, ` of ${fmt.n(rows.matched)} matched`) : null))) : rows && rows.kind !== "other" ? h("span", { class: "small dim" }, "no WHERE: every matched row goes on") : null,
    rank: () => rows && rows.kind !== "other" ? h("div", { class: "small" }, rows.fused ? [h("span", { class: "mono" }, rows.fused), ": "] : null, rows.candidates != null ? `${fmt.plural(rows.candidates, "candidate")} ranked, ` : "", `the first ${fmt.n(rows.ranked)} up to the page's end`) : null,
    rerank: () => rows?.phases?.length ? h("div", {}, rows.phases.map((p) => h("div", { class: "small" }, h("span", { class: "mono" }, p.text), `: the top ${fmt.n(p.count)} ranked again`))) : null,
    blend: () => j.blend ? h("div", { class: "small" }, `${fmt.plural(j.blend.sources.length, "source")} filled ${fmt.plural(j.blend.rows.length, "slot")} in ${fmt.ms(j.blend.fill_ms)}`) : null,
    shards: () => { const calls = (j.spans || []).filter((s) => s.shard != null); return calls.length ? `${fmt.plural(calls.length, "call")} to ${fmt.plural(new Set(calls.map((c) => c.shard)).size, "shard")}` : null; },
    fetch: () => j.answer ? `${fmt.plural(j.answer.total_rows, "row")} answered` : null,
    other: () => h("span", { class: "small dim" }, "reading the request, writing the answer, and anything else no stage names"),
  };
  const lines = STAGES.map(([k, name]) => {
    const v = ms[k] || 0;
    const d = detail[k]?.();
    const ran = v > 0;
    if (!ran && (d == null || (Array.isArray(d) && !d.filter(Boolean).length))) return h("div", { class: "fn-stage off" }, h("span", { class: "fn-dot", style: { background: COLOR[k] } }), h("b", {}, name), h("span", { class: "small muted" }, "did not run"));
    return h("div", { class: "fn-stage" },
      h("span", { class: "fn-dot", style: { background: COLOR[k] } }), h("b", {}, name),
      h("div", { class: "fn-time" }, h("div", { class: "fn-tbar" }, h("div", { style: { width: Math.min(100, v / Math.max(total, 1e-9) * 100) + "%", background: COLOR[k] } })), h("span", { class: "num small" }, ran ? `${fmt.ms(v)} · ${fmt.pct(v / Math.max(total, 1e-9))}` : "–")),
      h("div", { class: "fn-detail" }, d));
  });
  return card({ title: "From its text to its rows", sub: "each stage in the order a query meets it, its time (summing to the whole) and what it did", body: [stageBar(ms, { height: 18, total }), h("div", { class: "fn-stages" }, lines)] });
}

/** A search's words: the terms they became, and "did you mean". */
function termsOf(s) {
  const kinds = { word: "", typo: "typo of", prefix: "prefix", synonym: "synonym" };
  return h("div", { class: "fn-terms" },
    h("div", { class: "small" }, h("b", { class: "mono" }, `bm25(${s.field})`), h("span", { class: "mono" }, ` "${s.text}"`), s.docs != null ? h("span", { class: "dim" }, ` · ${fmt.plural(s.docs, "row")} have ${s.field}`) : null),
    h("div", { class: "row", style: { gap: "4px", flexWrap: "wrap" } }, (s.terms || []).map((t) => h("span", { class: "fn-term " + t.kind + (t.df === 0 ? " none" : ""), title: t.kind === "word" ? `a word of the text, in ${fmt.plural(t.df, "row")}` : `${t.kind}${t.of ? " of " + t.of : ""}: in ${fmt.plural(t.df, "row")}` },
      t.term, h("span", { class: "n" }, fmt.si(t.df)), t.kind !== "word" ? h("span", { class: "k" }, kinds[t.kind] + (t.of ? " " + t.of : "")) : null))),
    s.corrected?.changed ? h("div", { class: "small" }, "Did you mean: ", h("b", {}, s.corrected.text), h("span", { class: "dim" }, ` (${s.corrected.words.map((w) => `${w.word} → ${w.to_word || w.to}`).join(", ")}: in no row as written)`)) : null);
}

/** The rows left after each stage of one query. */
function rowsFunnel(rows) {
  const steps = [];
  if (rows.live != null) steps.push(["In the table", rows.live, "var(--axis)", "its live rows"]);
  if (rows.matched != null) steps.push(["Matched", rows.matched, COLOR.match, rows.inputs.some((i) => i.generates && i.matched == null) ? "a search by meaning reaches every row with a vector" : rows.inputs.filter((i) => i.matched != null).map((i) => `${i.label} ${fmt.n(i.matched)}`).join(" · ")]);
  for (const c of rows.conditions || []) steps.push([h("span", { class: "mono", title: c.text }, "WHERE " + c.text), c.left, COLOR.select, ""]);
  if (rows.fused && rows.candidates != null) steps.push(["Candidates", rows.candidates, COLOR.rank, "each input's top rows among those WHERE kept, which the ranking scores"]);
  steps.push(["Ranked", rows.ranked, COLOR.rank, rows.offset ? `the page's end, OFFSET ${rows.offset} included` : "up to the page's end"]);
  if (rows.phases?.length) for (const p of rows.phases) steps.push([`Reranked: ${p.text}`, Math.min(p.count, rows.ranked), COLOR.rerank, ""]);
  steps.push(["Shown", rows.shown, "var(--good, var(--s3))", "on the page"]);
  const max = Math.max(...steps.map((s) => s[1] || 0), 1);
  return card({ title: "Rows, stage by stage", sub: "the rows left after each stage (bars on a log scale)", body: h("div", { class: "fn-funnel" }, steps.map(([l, n, c, note]) => funnelRow(l, n, max, c, note))) });
}

/** Every row the probes saw, with where it left and why. */
function fatesCard(rows) {
  const list = rows.rows || [];
  if (!list.length) return null;
  const counts = {};
  for (const r of list) counts[r.fate] = (counts[r.fate] || 0) + 1;
  let pick = "all";
  const t = table([
    { key: "key", label: rows.key || "key", render: (r) => h("span", { class: "mono" }, typeof r.key === "string" ? r.key : JSON.stringify(r.key)) },
    { key: "fate", label: "Fate", render: (r) => h("span", { class: "pill " + (FATES[r.fate]?.[1] || "info") }, FATES[r.fate]?.[0] || r.fate) },
    { key: "stage", label: "Left at", render: (r) => h("span", { class: "row nw" }, h("span", { class: "fn-dot", style: { background: COLOR[r.stage] } }), NAME[r.stage] || r.stage) },
    { key: "why", label: "Why", sortable: false, render: (r) => h("span", { class: "small" }, r.why) },
    { key: "rank", label: "Rank", num: true, render: (r) => r.rank ?? "–" },
    { key: "score", label: "Score", num: true, render: (r) => r.score == null ? "–" : fmt.num(+r.score.toPrecision(4)) },
    ...(rows.inputs || []).map((inp, i) => ({ key: "in" + i, label: inp.label, num: true, title: `its place in ${inp.label}'s top: before WHERE, and among the rows WHERE kept`, sort: (r) => r.selected[i] ?? r.matched[i]?.[0] ?? 1e9,
      render: (r) => { const m = r.matched[i], s = r.selected[i]; return m == null && s == null ? h("span", { class: "dim" }, "–") : h("span", { class: "small nw" }, m ? `#${m[0]}` : "–", h("span", { class: "dim" }, " · "), s ? `#${s}` : h("span", { class: "dim" }, "–")); } })),
  ], { rows: list, compact: true, maxHeight: "480px" });
  const filter = seg([["all", `All ${list.length}`], ...Object.keys(FATES).filter((f) => counts[f]).map((f) => [f, `${FATES[f][0]} ${counts[f]}`])], pick, (v) => { pick = v; t.update(v === "all" ? list : list.filter((r) => r.fate === v)); });
  return card({ title: "Where each row left", sub: "every row the probes saw: the page, the rows ranked just past it, each input's top rows, and those WHERE took from them", tools: filter, body: t, flush: true });
}

function blendCard(j, rerun) {
  const b = j.blend;
  const srcSql = Object.fromEntries((j.sources || []).map((s) => [s.name, s.sql]));
  const params = paramsObj(j.params);
  return card({ title: "Blend", sub: `${b.blend} v${b.version}: each source a query of its own, then the fill into ${fmt.plural(b.k, "slot")}`, body: [
    table([
      { key: "name", label: "Source", render: (s) => h("b", {}, s.name) },
      { key: "rows", label: "Rows", num: true },
      { key: "took_ms", label: "Took", num: true, render: (s) => fmt.ms(s.took_ms) },
      { key: "placed", label: "Placed", num: true, render: (s) => typeof s.placed === "number" ? fmt.pct(s.placed) : JSON.stringify(s.placed) },
      { label: "", sortable: false, render: (s) => srcSql[s.name] ? h("button", { class: "btn ghost sm", title: "Trace this source's own funnel", onclick: () => rerun(srcSql[s.name], params) }, "Its funnel", icon("right")) : null },
    ], { rows: b.sources, compact: true }),
    h("div", { class: "small", style: { fontWeight: 600, margin: "10px 0 4px" } }, "Placed"),
    table([
      { key: "slot", label: "Slot", num: true },
      { key: "source", label: "Source" },
      { key: "key", label: "Key", render: (r) => h("span", { class: "mono" }, JSON.stringify(r.key)) },
      { key: "kind", label: "By" },
      { key: "why", label: "Why", sortable: false, render: (r) => h("span", { class: "small" }, r.why) },
    ], { rows: b.rows, compact: true }),
    b.passed?.length ? [h("div", { class: "small", style: { fontWeight: 600, margin: "10px 0 4px" } }, "Passed over"),
      table([
        { key: "source", label: "Source" },
        { key: "key", label: "Key", render: (r) => h("span", { class: "mono" }, JSON.stringify(r.key)) },
        { key: "why", label: "Why", sortable: false, render: (r) => h("span", { class: "small" }, r.why) },
      ], { rows: b.passed, compact: true })] : null,
    h("a", { class: "btn sm", href: link("blend", { name: b.blend }), style: { marginTop: "8px" } }, icon("sliders"), "Open it in Blending"),
  ] });
}

/** The steps and spans, folded: the flame chart of every step, the trace's spans. */
function stepsCard(j) {
  const box = h("div");
  const steps = j.steps || [], spans = j.spans || [];
  if (!steps.length && !spans.length) return null;
  const btn = h("button", { class: "btn sm", onclick: () => {
    if (box.childElementCount) { clear(box); btn.lastChild.textContent = "Show every step ▸"; return; }
    clear(box, steps.length ? flameChart(stepTree(steps), spans) : null, spans.length ? [h("div", { class: "small", style: { fontWeight: 600, margin: "8px 0 4px" } }, "The request's spans"), waterfall(spans)] : null);
    btn.lastChild.textContent = "Hide the steps ▴";
  } }, h("span", {}, "Show every step ▸"));
  return card({ title: "Every step", sub: `${fmt.plural(steps.length, "step")} and ${fmt.plural(spans.length, "span")}, each on the stage it counts for`, body: [btn, box] });
}

function answerCard(a) {
  const cols = a.columns || [];
  return card({ title: "The answer", sub: a.total_rows > (a.rows || []).length ? `its first ${a.rows.length} of ${fmt.n(a.total_rows)} rows` : fmt.plural(a.total_rows ?? 0, "row"), flush: true,
    body: table(cols.map((c, i) => ({ key: String(i), label: c, render: (r) => { const v = r[i]; return v == null ? h("span", { class: "dim" }, "null") : h("span", { class: "small" }, typeof v === "object" ? JSON.stringify(v) : String(v)); } })),
      { rows: a.rows || [], compact: true }) });
}

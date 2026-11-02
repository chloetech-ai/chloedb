// Alerts across the fleet. The list shows what fires first, one row a rule
// with where it fires (each copy, a shard, this coordinator), then every
// rule with its scope, owner, runbook and noise, then when each fired over
// the top bar's time range. Each alert has a page, #/alert/<rule>: the
// incident hub, with the metric around it and the events that changed
// things, the blast radius, the failing statements, the log lines grouped
// by message, the affected copies with their links, and acknowledge,
// silence and notes. Rules are edited in a drawer with a backtest over up
// to 7 days; routes carry the messages, each with its delivery status.
// Against a server without the fleet alerting API (no states, routes,
// ack or backtest) the same pages show what that server has and say what
// it cannot do.

import { h, s, icon, clear, fmt, card, api, bus, drawer, toast, pill, seg, empty, menu, copy, showTip, hideTip, table, every, debounce, confirm, can, needs, copyName, findCopy, link, ago, when, whenTitle, ApiError, isNarrow } from "../core.js";
import { state, refreshAlerts, timeRange, isCoordinator } from "../data.js";
import { niceStep } from "../charts.js";
import { pattern } from "./logs.js";

const view = { id: "alerts", title: "Alerts", icon: "alerts", group: "Operate", keywords: "alerts rules firing incidents acknowledge silence runbook owner routes webhook notifications backtest", render, onParams, badge };
export default view;
/** The alert page, `#/alert/<rule>`: not in the navigation. */
export const alertPage = { id: "alert", title: "Alert", icon: "alerts", group: "Operate", hidden: true, nav: false, render, onParams };

// ---------------------------------------------------------------- helpers
const Needs = (scope) => needs(scope).replace(/^n/, "N");
/** A time without seconds: "19:42" today, "Nov 2 19:42" another day. */
const hm = (ms) => when(ms, { sec: false });
const dur = (ms) => fmt.dur(Math.max(0, ms) / 1000).replace(/^(\d+)m 0s$/, "$1 min").replace(/^(\d+)m (\d+)s$/, "$1 min").replace(/^(\d+)s$/, "$1 s");
const alertLink = (id, params) => link("alert/" + encodeURIComponent(id), params);
/** A copy in the cluster by its URL, short name or node name: {shard, i, r, sh}. */
function copyOf(key) {
  const f = findCopy(key);
  if (f) return { shard: f.shard, i: f.copy, r: f.rep, sh: f.sh };
  for (const sh of state.cluster?.shards || []) {
    const i = (sh.replicas || []).findIndex((r) => r.short === key || r.name === key || r.detail?.log_store?.node === key);
    if (i >= 0) return { shard: sh.shard, i, r: sh.replicas[i], sh };
  }
  return null;
}
/** A copy's name for an entry of an alert's `where`: {copy (url), short}. */
const whereName = (c) => findCopy(c.copy) ? copyName(c.copy) : c.short || copyName(c.copy);
/** A server's node name (as the log store names it) as the copy's name. */
function nodeShort(name) {
  const c = copyOf(name);
  return c ? copyName(c.r.url) : name;
}
const coordinator = () => isCoordinator() || !!state.cluster?.shards?.length;
const allCopies = () => (state.cluster?.shards || []).reduce((a, sh) => a + (sh.replicas?.length || 0), 0);
const range = () => timeRange();
/** Asks before a change, with a required reason: resolves {reason} or false. */
const ask = ({ title, body, ok, danger, typed, reason = true }) => confirm({ title, body, ok, danger, typed, reason });
const failMsg = (e, what) => e instanceof ApiError && e.status === 403 ? `${what} ${needs("write")}` : e instanceof ApiError && (e.status === 404 || e.status === 405) ? `${what}: this server cannot do that yet (it needs a newer strata-server)` : `${what}: ${e.message}`;
const missing = (e) => e instanceof ApiError && (e.status === 404 || e.status === 405);

// ---------------------------------------------------------------- metrics
// What a rule can watch: its name, unit and where the value exists (A10).
const METRICS = {
  error_ratio: ["Failed queries, share of all (server-side)", "pct", "both"],
  availability: ["Availability (1 - server failures / queries)", "pct", "both"],
  p99_ms: ["Query p99", "ms", "both"], p95_ms: ["Query p95", "ms", "both"], p50_ms: ["Query p50", "ms", "both"],
  qps: ["Queries a second", "/s", "both"],
  errors_s: ["Failed queries a second (server-side)", "/s", "both"],
  query_errors: ["Failed queries (server-side)", "n", "both"],
  client_errors_s: ["Client errors (400) a second", "/s", "both"],
  timeouts_s: ["Server timeouts a second", "/s", "both"],
  rejected_s: ["Queries refused (429) a second", "/s", "both"],
  partial_s: ["Partial answers a second", "/s", "coordinator"],
  slow_s: ["Slow queries a second", "/s", "both"],
  http_5xx_s: ["HTTP 5xx answers a second", "/s", "both"],
  http_4xx_s: ["HTTP 4xx answers a second", "/s", "both"],
  writes_s: ["Writes a second", "/s", "both"],
  write_errors_s: ["Failed writes a second", "/s", "both"],
  write_p99_ms: ["Write p99", "ms", "both"],
  cpu: ["CPU busy (cores)", "n", "both"],
  disk_used_ratio: ["Disk used", "pct", "copy"],
  failing: ["Database refuses writes (0 or 1)", "flag", "copy"],
  flush_failing: ["Flushes fail (0 or 1)", "flag", "copy"],
  merge_failing: ["Merges fail (0 or 1)", "flag", "copy"],
  replica_behind_max: ["Replica behind, in transactions", "n", "copy"],
  replica_behind_s: ["Replica behind, in seconds", "s", "copy"],
  replicas_out_of_sync: ["Replicas out of sync", "n", "copy"],
  serves_reads: ["Serves reads (0 or 1)", "flag", "copy"],
  query_memory_ratio: ["Query memory used", "pct", "both"],
  write_queue: ["Writes waiting", "n", "copy"],
  running: ["Queries running", "n", "both"],
  rss_bytes: ["Resident memory", "bytes", "both"],
  heap_bytes: ["Heap", "bytes", "both"],
  segments: ["Segments", "n", "copy"],
};
const unitOf = (m) => METRICS[m]?.[1] || (/_ms$/.test(m) ? "ms" : /_ratio$/.test(m) ? "pct" : /_s$/.test(m) ? "/s" : /bytes/.test(m) ? "bytes" : "n");
/** A metric's value with its unit. */
function fv(metric, v) {
  if (v == null || Number.isNaN(v)) return "–";
  switch (unitOf(metric)) {
    case "pct": return fmt.pct(v, Math.abs(v) < 0.1 ? 2 : 1);
    case "ms": return fmt.ms(v);
    case "/s": return fmt.si(v) + "/s";
    case "bytes": return fmt.bytes(v);
    case "s": return fmt.dur(v);
    case "flag": return v > 0 ? "yes" : "no";
    default: return fmt.si(v);
  }
}
const EVAL = { all: "each 2-s point", mean: "mean over", max: "max over", min: "min over", rate: "rate over" };
const OPS = [">", ">=", "<", "<="];
const SEV = ["critical", "warning", "info"];
const RANK = { firing: 0, pending: 1, no_data: 2, ok: 3, disabled: 4 };
const SEVR = { critical: 0, warning: 1, info: 2 };
const holds = (v, op, t) => v != null && (op === ">" ? v > t : op === ">=" ? v >= t : op === "<" ? v < t : v <= t);
/** The condition in words: "error_ratio, mean over 5 min > 2%, held 2 min". */
function cond(r, short) {
  const ev = r.eval || (r.window_s ? "mean" : "all");
  const how = ev === "all" ? (short ? "" : "each point ") : `${EVAL[ev]} ${dur((r.window_s || 300) * 1000)} `;
  const held = r.for_s ? ` for ${dur(r.for_s * 1000)}` : "";
  return `${short ? "" : r.metric + ", "}${how}${r.op} ${fv(r.metric, r.threshold)}${held}`.replace(/\s+/g, " ").trim();
}
const sevBadge = (sv) => h("span", { class: "al-sev " + (sv || "info") }, sv || "info");

// ---------------------------------------------------------------- the model
// Old servers answer {rules: [{..., status}], firing: [rules], events, webhook};
// new ones {rules, states, events, firing: n, routes}. Both become one shape.
const stateName = (x) => x === "no data" ? "no_data" : x || "ok";
function model(j) {
  if (!j) return null;
  const fleet = Array.isArray(j.states) || Array.isArray(j.routes);
  const rules = (j.rules || []).map((r) => ({ ...r, status: undefined }));
  const st = new Map();
  for (const x of j.states || []) st.set(x.rule, { ...x, state: stateName(x.state), where: x.where || [], notes: x.notes || [] });
  for (const r of j.rules || []) {
    if (st.has(r.id)) continue;
    const x = r.status || {};
    st.set(r.id, { rule: r.id, state: stateName(x.state), value: x.value, since_ms: x.since_ms || null, where: [], notes: [] });
  }
  for (const r of rules) {
    const x = st.get(r.id) || { rule: r.id, where: [], notes: [] };
    if (!r.enabled) x.state = "disabled";
    x.silent = !!(x.silenced && (!x.silenced.until_ms || x.silenced.until_ms > Date.now()));
    st.set(r.id, x);
  }
  const events = (j.events || []).map(normEvent).filter((e) => e.rule);
  const routes = Array.isArray(j.routes) ? j.routes : j.webhook ? [{ name: "default", legacy: true }] : [];
  return { rules, st, events, routes, fleet, file: j.file };
}
/** An event of /v1/alerts or /v1/events as {ts, rule, what, severity, value, by, text}. */
function normEvent(e) {
  const kind = e.kind || "";
  const what = e.state === "firing" || kind === "alert_fired" || e.what === "fired" ? "fired"
    : e.state === "resolved" || kind === "alert_resolved" || e.what === "resolved" ? "resolved"
    : kind === "alert_acked" || e.state === "acked" ? "acked" : kind === "alert_silenced" || e.state === "silenced" ? "silenced"
    : kind === "note" || e.state === "note" ? "note" : e.state || kind || "changed";
  return { ts: e.ts_ms || e.at_ms, rule: e.rule || e.where?.rule, what, severity: e.severity, value: e.value, by: e.who || e.by, text: e.detail?.text || e.text || e.note || e.reason || null, name: e.name, where: e.where, title: e.title };
}
function sorted(m) {
  return m.rules.slice().sort((a, b) => {
    const x = m.st.get(a.id), y = m.st.get(b.id);
    return (x.silent - y.silent) || (RANK[x.state] ?? 5) - (RANK[y.state] ?? 5) || (SEVR[a.severity] ?? 3) - (SEVR[b.severity] ?? 3) || String(a.name).localeCompare(b.name);
  });
}
/** Where a rule fires, in words, and the copies it fires on. */
function whereText(r, x) {
  const all = x.where || [];
  const firing = all.filter((c) => stateName(c.state) === "firing");
  const w = firing.length ? firing : all.filter((c) => stateName(c.state) === "pending");
  const pend = firing.length ? all.filter((c) => stateName(c.state) === "pending").length : 0;
  const names = w.map(whereName);
  const more = pend ? ` (+${pend} pending)` : "";
  if (w.length === 1) { const c = copyOf(w[0].copy || w[0].short); return `${names[0]}${c ? ` · shard ${c.shard}` : ""}${more}`; }
  if (r.scope === "each_copy") {
    const total = Math.max(all.length, allCopies());
    if (!w.length) return x.state === "no_data" ? "no copy reports it" : `each copy (${fmt.n(total)})`;
    return `${w.length} of ${fmt.n(total)} copies: ${names.slice(0, 3).join(", ")}${names.length > 3 ? ` +${names.length - 3}` : ""}${more}`;
  }
  if (names.length) return names.slice(0, 3).join(", ") + (names.length > 3 ? ` +${names.length - 3}` : "") + more;
  return r.builtin ? "the cluster" : scopeText(r)[0];
}
function scopeText(r) {
  if (r.scope === "each_copy") return ["each copy", `${fmt.n(allCopies())} copies`];
  if (r.builtin) return ["from the cluster state", "on this coordinator"];
  const coord = coordinator();
  const n = state.fleet?.coordinators?.length;
  return coord ? [n > 1 ? "coordinators" : "this coordinator", n > 1 ? `${n} coordinators` : "its own points"] : ["this server", "its own points"];
}

let current = null; // the list's model, for the badge
export function badge() {
  const m = model(state.alerts);
  if (!m) return null;
  const firing = m.rules.filter((r) => { const x = m.st.get(r.id); return x.state === "firing" && !x.silent; });
  if (!firing.length) return null;
  return { n: firing.length, tone: firing.some((r) => r.severity === "critical") ? "bad" : "warn" };
}

// ---------------------------------------------------------------- the list
let openRule = null, openRoutes = null, hubParams = null;
// The list (#/alerts) and an alert's page (#/alert/<rule>, which the router hands to this view
// with the rule in r.rest) share one view: going from one to the other only changes the params,
// so onParams mounts the other one in the same page.
let mount = null;
const hubOf = (r) => { const id = r.rest?.[0] ? decodeURIComponent(r.rest[0]) : null; return id || (r.id === "alert" ? r.params.get("rule") : null) || null; };
function mountInto(r) {
  try { mount.stop?.(); } catch (e) { console.error(e); }
  clear(mount.page);
  mount.id = hubOf(r);
  mount.stop = mount.id ? alertHub(mount.page, mount.id, r) : listView(mount.page, r);
}
function render(page, r) {
  const me = mount = { page, id: null, stop: null };
  mountInto(r);
  return () => { try { me.stop?.(); } catch (e) { console.error(e); } if (mount === me) mount = null; };
}
function onParams(r) {
  if (mount && hubOf(r) !== mount.id) { mountInto(r); mount.page.parentElement?.scrollTo?.(0, 0); return; }
  if (mount?.id) { hubParams?.(r); return; }
  if (r.params.get("rule")) openRule?.(r.params.get("rule"));
  else if (r.params.get("new")) openRule?.(null);
  if (r.params.get("routes")) openRoutes?.();
}

function listView(page, r) {
  const sub = h("p");
  const routesBtn = h("button", { class: "btn", onclick: () => routesDrawer() }, icon("route"), "Routes");
  const newBtn = h("button", { class: "btn primary", onclick: () => editRule(null) }, icon("plus"), "New rule");
  const sum = h("div");
  const firingBox = h("div");
  const silencedBox = h("div");
  const rulesBox = h("div");
  const histBox = h("div");
  const histSub = h("span", { class: "sub" });
  const rulesSub = h("span", { class: "sub" });
  let filter = "all", q = "";
  const search = h("input", { class: "input sm", type: "search", placeholder: "Filter rules, owners, metrics", style: { width: "220px" }, "aria-label": "Filter rules", oninput: debounce(() => { q = search.value.trim().toLowerCase(); drawRules(); }, 150) });
  const filters = seg([["all", "all"], ["firing", "firing"], ["no_data", "no data"], ["builtin", "built-in"], ["norunbook", "no runbook"], ["off", "off"]], filter, (v) => { filter = v; drawRules(); }, { "aria-label": "Show rules" });
  page.append(
    h("div", { class: "page-head" }, h("div", {}, h("h1", {}, "Alerts"), sub), h("div", { class: "tools" }, routesBtn, newBtn)),
    sum,
    card({ title: "Firing now", sub: "firing first, then pending · not silenced", body: [firingBox, silencedBox], flush: true, attrs: { style: { marginBottom: "14px" } } }),
    h("section", { class: "card", style: { marginBottom: "14px" } }, h("div", { class: "hd al-rhd" }, h("h3", {}, "Rules"), rulesSub, h("div", { class: "tools" }, filters, search)), h("div", { class: "bd flush" }, rulesBox)),
    h("section", { class: "card" }, h("div", { class: "hd" }, h("h3", {}, "When each rule fired"), histSub), h("div", { class: "bd" }, histBox)));

  let m = null, longEvents = null, longFor = null;
  async function loadLong() {
    // Fired and resolved over the top bar's range, from the event store when the server has one.
    const t = range();
    const key = `${Math.round(t.from / 60e3)}-${t.live ? "now" : Math.round(t.to / 60e3)}`;
    if (key === longFor) return;
    longFor = key;
    try {
      const j = await api(`/v1/events?from=${Math.round(t.from)}&to=${Math.round(t.to)}&kind=alert_fired,alert_resolved,alert_acked,alert_silenced&limit=5000`);
      longEvents = (j.events || []).map(normEvent).filter((e) => e.rule);
    } catch { longEvents = null; }
    draw();
  }
  const eventsIn = () => {
    const t = range();
    const all = [...(longEvents || []), ...(m?.events || [])];
    const seen = new Set();
    return all.filter((e) => { const k = `${e.rule}/${e.what}/${e.ts}`; if (seen.has(k)) return false; seen.add(k); return e.ts >= t.from && e.ts <= t.to + 5e3; }).sort((a, b) => a.ts - b.ts);
  };

  function draw() {
    if (state.paused && m) return;
    m = current = model(state.alerts);
    if (!m) { clear(firingBox, empty("alerts", "Loading the rules…")); return; }
    const adm = can("admin");
    newBtn.disabled = !adm; newBtn.title = adm ? "" : Needs("admin");
    clear(routesBtn, icon("route"), m.fleet ? `Routes: ${m.routes.length}` : m.routes.length ? "Route: one webhook" : "Routes: none");
    const coord = coordinator();
    clear(sub, m.fleet
      ? "Rules live on the coordinator, every change audited; rules of scope each copy run on every copy and the coordinator gathers them. What fires is grouped by rule, with where it fires."
      : `This server checks its rules on its own points every 2 seconds${coord ? ": a coordinator cannot see its copies' disks, flushes or replication, so those rules have no data here" : ""}.`);
    drawSummary(); drawFiring(); drawRules(); drawHistory();
  }

  function drawSummary() {
    const firing = m.rules.filter((r) => { const x = m.st.get(r.id); return x.state === "firing" && !x.silent; });
    const crit = firing.filter((r) => r.severity === "critical"), warn = firing.filter((r) => r.severity !== "critical");
    const unacked = crit.filter((r) => !m.st.get(r.id).ack);
    const noData = m.rules.filter((r) => m.st.get(r.id).state === "no_data");
    const bits = [];
    // Copies that stop reporting are themselves an alert (copy-silent); say how many report.
    const silentCopies = (state.cluster?.shards || []).flatMap((sh) => (sh.replicas || []).map((rp, i) => ({ rp, i, sh }))).filter(({ rp }) => rp.status === "down" || rp.role === "unknown");
    if (state.cluster?.shards) bits.push(`Copies reporting: ${fmt.n(allCopies() - silentCopies.length)} of ${fmt.n(allCopies())}${silentCopies.length ? ` (${silentCopies.slice(0, 3).map(({ rp }) => copyName(rp.url)).join(", ")}${silentCopies.length > 3 ? " …" : ""} silent)` : ""}`);
    for (const rt of m.routes.filter((x) => !x.legacy)) bits.push(rt.last_error && (!rt.last_ok_ms || rt.last_error_ms > rt.last_ok_ms) ? `route ${rt.name}: last send failed` : rt.last_ok_ms ? `route ${rt.name} delivered ${hm(rt.last_ok_ms)}` : `route ${rt.name}: nothing sent yet`);
    if (!m.routes.length) bits.push("no route: nobody is told when an alert fires");
    if (!firing.length) {
      clear(sum, h("div", { class: "banner ok" }, icon("okc", "lg"), h("div", { class: "grow" },
        h("div", { class: "big" }, "Nothing is firing"),
        h("div", { class: "small dim" }, `${fmt.plural(m.rules.length, "rule")} · ${fmt.n(m.rules.filter((r) => m.st.get(r.id).state === "ok").length)} ok${noData.length ? ` · ${fmt.n(noData.length)} without data (they cannot tell)` : ""}${bits.length ? " · " + bits.join(" · ") : ""}`))));
      return;
    }
    const oldest = unacked.reduce((a, r) => Math.min(a, m.st.get(r.id).since_ms || Date.now()), Date.now());
    clear(sum, h("div", { class: "banner " + (crit.length ? "bad" : "warn") }, icon("warn", "lg"), h("div", { class: "grow" },
      h("div", { class: "big" }, [crit.length ? `${crit.length} critical` : null, warn.length ? `${warn.length} warning` : null].filter(Boolean).join(", ") + " firing",
        m.fleet && unacked.length ? h("span", { style: { fontWeight: 500 } }, ` · ${fmt.plural(unacked.length, "critical alert")} not acknowledged for ${dur(Date.now() - oldest)}`) : null),
      h("div", { class: "small dim" }, bits.join(" · ")))));
  }

  function actions(r, x) {
    const out = [];
    if (m.fleet) {
      const w = can("write");
      out.push(h("button", { class: "btn xs", disabled: !w || !!x.ack, title: !w ? Needs("write") : x.ack ? `Acknowledged by ${x.ack.by}` : "Acknowledge: you are on it", onclick: () => ack(r) }, "Ack"));
      const sil = h("button", { class: "btn xs", disabled: !w, title: w ? "Silence it for a while, with a reason" : Needs("write") }, "Silence", icon("down"));
      sil.onclick = () => silenceMenu(sil, r, draw);
      out.push(sil);
    }
    out.push(h("a", { class: "btn xs", href: alertLink(r.id) }, "Open"));
    return h("div", { class: "row nw al-acts", style: { gap: "4px", justifyContent: "flex-end" } }, out);
  }
  async function ack(r) {
    try { await api(`/v1/alerts/${encodeURIComponent(r.id)}/ack`, { body: {} }); toast(`Acknowledged: ${r.name}`, "ok"); await refreshAlerts(); }
    catch (e) { toast(failMsg(e, "Acknowledging"), "bad"); }
  }

  function drawFiring() {
    const rows = sorted(m).filter((r) => { const x = m.st.get(r.id); return (x.state === "firing" || x.state === "pending") && !x.silent; });
    const silenced = m.rules.filter((r) => m.st.get(r.id).silent);
    clear(silencedBox, silenced.length ? h("div", { class: "al-silenced" }, icon("pause"),
      h("span", {}, `${silenced.length} silenced: `, silenced.map((r, i) => { const x = m.st.get(r.id); return [i ? "; " : "", h("a", { href: alertLink(r.id) }, r.name), ` (${x.state === "firing" ? "firing" : x.state}) by ${x.silenced.by || "someone"} until ${hm(x.silenced.until_ms)}${x.silenced.reason ? `, "${x.silenced.reason}"` : ""}`]; }))) : null);
    if (!rows.length) {
      clear(firingBox, h("div", { class: "al-quiet" }, icon("okc"), h("span", {}, "No rule is firing or pending."), m.rules.some((r) => m.st.get(r.id).state === "no_data") ? h("span", { class: "muted" }, "Rules without data are listed below: they cannot fire until their metric has a value.") : null));
      return;
    }
    clear(firingBox, table([
      { key: "sev", label: "", width: "24px", cls: "al-c-dot", render: (r) => h("span", { class: "dot " + (m.st.get(r.id).state === "firing" ? (r.severity === "critical" ? "critical pulse" : r.severity === "warning" ? "warn" : "") : "pending"), title: m.st.get(r.id).state } ) , sortable: false },
      { key: "name", label: "alert", cls: "al-c-name", render: (r) => h("div", {}, h("a", { class: "al-name", href: alertLink(r.id) }, r.name), h("div", { class: "tiny muted" }, r.builtin ? "built-in · " : "", cond(r, true))) },
      { key: "where", label: "where", cls: "al-c-where", render: (r) => h("span", { class: "small" }, whereText(r, m.st.get(r.id))) },
      { key: "since", label: "since", cls: "al-c-since", sort: (r) => m.st.get(r.id).since_ms, render: (r) => { const x = m.st.get(r.id); return h("span", { class: "small nowrap", title: whenTitle(x.since_ms) }, x.since_ms ? `${hm(x.since_ms)} · ${dur(Date.now() - x.since_ms)}` : "–"); } },
      { key: "now", label: "now", cls: "al-c-now", render: (r) => { const x = m.st.get(r.id); return h("span", { class: "small nowrap" }, h("span", { class: "al-narrow-only muted" }, "now "), x.value == null ? "no value" : fv(r.metric, x.value)); } },
      { key: "owner", label: "owner", cls: "al-c-owner", render: (r) => r.owner ? h("span", { class: "small" }, r.owner) : h("span", { class: "small muted" }, "no owner") },
      { key: "state", label: "state", cls: "al-c-state", sort: (r) => m.st.get(r.id).state, render: (r) => ackPill(r, m.st.get(r.id), m.fleet) },
      { key: "act", label: "", cls: "al-c-act", sortable: false, render: (r) => actions(r, m.st.get(r.id)) },
    ], { rows, compact: true, onRow: (r) => { location.hash = alertLink(r.id); } }));
    firingBox.firstElementChild?.classList.add("al-ftbl");
  }

  function drawRules() {
    if (!m) return;
    const evs = eventsIn();
    const t = range();
    const fires = {};
    for (const e of evs) if (e.what === "fired") (fires[e.rule] ||= []).push(e.ts);
    let rows = sorted(m);
    if (filter === "firing") rows = rows.filter((r) => ["firing", "pending"].includes(m.st.get(r.id).state));
    else if (filter === "no_data") rows = rows.filter((r) => m.st.get(r.id).state === "no_data");
    else if (filter === "builtin") rows = rows.filter((r) => r.builtin);
    else if (filter === "norunbook") rows = rows.filter((r) => !r.runbook);
    else if (filter === "off") rows = rows.filter((r) => !r.enabled);
    if (q) rows = rows.filter((r) => [r.name, r.id, r.metric, r.owner, r.description].some((v) => String(v || "").toLowerCase().includes(q)));
    const adm = can("admin");
    rulesSub.textContent = `${rows.length} of ${m.rules.length} · firing first · fires over ${t.label}`;
    clear(rulesBox, table([
      { key: "severity", label: "sev.", cls: "al-c-sev", sort: (r) => SEVR[r.severity], render: (r) => sevBadge(r.severity) },
      { key: "name", label: "rule", cls: "al-c-name", render: (r) => h("div", { class: "al-rname" }, h("div", { class: "row nw", style: { gap: "6px" } }, h("button", { class: "al-link", title: "Edit the rule", onclick: () => editRule(r) }, r.name), r.builtin ? h("span", { class: "badge accent", title: "Built in: watches the cluster state; only its severity, on/off and route change" }, "built-in") : null), h("div", { class: "tiny muted mono" }, cond(r))) },
      { key: "scope", label: "scope", cls: "al-c-scope", sort: (r) => r.scope || "", render: (r) => { const [a, b] = scopeText(r); return h("div", { class: "small" }, a, h("div", { class: "tiny muted" }, b)); } },
      { key: "owner", label: "owner", cls: "al-c-owner", render: (r) => r.owner ? h("span", { class: "small" }, r.owner) : h("span", { class: "small muted" }, "–") },
      { key: "runbook", label: "runbook", cls: "al-c-runbook", render: (r) => r.runbook ? h("a", { class: "small", href: r.runbook, target: "_blank", rel: "noopener", title: r.runbook }, runbookName(r.runbook), " ", icon("ext")) : h("span", { class: "small muted" }, "none") },
      { key: "fires", label: "fires", num: true, cls: "al-c-fires", title: `times it fired over ${t.label}`, sort: (r) => (fires[r.id] || []).length, render: (r) => h("div", { class: "row nw", style: { gap: "8px", justifyContent: "flex-end" } }, fireBars(fires[r.id] || [], t), h("b", { class: "num" }, fmt.n((fires[r.id] || []).length))) },
      { key: "state", label: "state", cls: "al-c-state", sort: (r) => RANK[m.st.get(r.id).state], render: (r) => statePill(r, m.st.get(r.id)) },
      { key: "on", label: "on", cls: "al-c-on", sortable: false, render: (r) => onSwitch(r, !adm, (on) => setEnabled(r, on)) },
      { key: "edit", label: "", cls: "al-c-edit", sortable: false, render: (r) => h("button", { class: "btn xs ghost", title: "Edit the rule", onclick: () => editRule(r) }, icon("edit")) },
    ], { rows, compact: true, onRow: (r) => editRule(r), empty: empty("alerts", "No rule matches", "Try another filter.") }));
    rulesBox.firstElementChild?.classList.add("al-rtbl");
  }

  async function setEnabled(r, on) {
    const x = m.st.get(r.id);
    let reason = "";
    if (!on) {
      const ok = await ask({ title: `Turn "${r.name}" off?`, body: x.state === "firing" ? "It is firing now: turning it off resolves it and sends a RESOLVED message. Nothing checks it until it is on again." : "Nothing checks it until it is turned on again. To quiet it for a while, silence it instead.", ok: "Turn off", danger: x.state === "firing" });
      if (!ok) { drawRules(); return; }
      reason = ok.reason || "";
    }
    try { await saveRules(m.rules.map((y) => y.id === r.id ? { ...y, enabled: on } : y), reason); toast(`${r.name}: ${on ? "on" : "off"}`, "ok"); }
    catch { drawRules(); }
  }

  function drawHistory() {
    const t = range();
    const evs = eventsIn();
    histSub.textContent = `${t.label}${longEvents ? "" : " · from the latest events this server keeps in memory"}`;
    const spans = {};
    for (const e of evs) {
      if (e.what !== "fired" && e.what !== "resolved") continue;
      const r = m.rules.find((x) => x.id === e.rule);
      const sp = spans[e.rule] ||= { name: r?.name || e.name || e.rule, sev: r?.severity || e.severity, list: [] };
      if (e.what === "fired") sp.list.push([e.ts, null]);
      else if (sp.list.length && sp.list[sp.list.length - 1][1] == null) sp.list[sp.list.length - 1][1] = e.ts;
      else sp.list.push([t.from, e.ts]);
    }
    for (const r of m.rules) { const x = m.st.get(r.id); if (x.state === "firing" && x.since_ms && !spans[r.id]) spans[r.id] = { name: r.name, sev: r.severity, list: [[x.since_ms, null]] }; }
    const rows = Object.entries(spans);
    if (!rows.length) { clear(histBox, h("div", { class: "al-quiet" }, icon("okc"), h("span", {}, `No rule fired over ${t.label}.`))); return; }
    const cw = histBox.clientWidth || 1000, small = cw < 600;
    const W = Math.max(small ? 280 : 500, cw), rh = small ? 26 : 22, LW = small ? 112 : 210, NL = small ? 16 : 30;
    const t0 = t.from, t1 = t.to;
    const X = (v) => LW + (Math.max(t0, Math.min(t1, v)) - t0) / (t1 - t0) * (W - LW - 8);
    const svgEl = s("svg", { viewBox: `0 0 ${W} ${rows.length * rh + 20}`, style: { width: "100%", height: rows.length * rh + 20 + "px" }, role: "img", "aria-label": "When each rule fired" });
    rows.forEach(([id, sp], i) => {
      const a = s("a", { href: alertLink(id) }, s("text", { x: 0, y: i * rh + 15, class: "al-tl-name" }, sp.name.length > NL ? sp.name.slice(0, NL - 1) + "…" : sp.name));
      svgEl.append(a, s("rect", { x: LW, y: i * rh + 5, width: W - LW - 8, height: rh - 10, rx: 3, class: "al-tl-bg" }));
      for (const [a0, a1] of sp.list) {
        const x0 = X(a0), x1 = X(a1 ?? t1);
        const bar = s("rect", { x: x0, y: i * rh + 5, width: Math.max(3, x1 - x0), height: rh - 10, rx: 3, class: "al-tl-bar " + (sp.sev || "info") });
        bar.addEventListener("pointermove", (e) => showTip(e.clientX, e.clientY, [h("div", { class: "t" }, sp.name), h("div", { class: "r" }, h("b", {}, a1 ? dur(a1 - a0) : "still firing"), h("span", { class: "n" }, `${when(a0)} – ${a1 ? when(a1) : "now"}`))]));
        bar.addEventListener("pointerleave", hideTip);
        svgEl.append(bar);
      }
    });
    const nt = small ? 2 : 5;
    for (let k = 0; k <= nt; k++) { const v = t0 + k / nt * (t1 - t0); svgEl.append(s("text", { class: "al-ax", x: X(v), y: rows.length * rh + 15, "text-anchor": k === 0 ? "start" : k === nt ? "end" : "middle" }, t1 - t0 > 86400e3 ? hm(v) : fmt.time(v, false))); }
    clear(histBox, svgEl);
  }

  openRule = (id) => { if (id == null) { editRule(null); return; } const x = (model(state.alerts)?.rules || []).find((y) => y.id === id); if (x) editRule(x); };
  openRoutes = () => routesDrawer();
  draw();
  refreshAlerts().then(draw);
  loadLong();
  if (r.params.get("rule")) setTimeout(() => openRule(r.params.get("rule")), 60);
  else if (r.params.get("new")) setTimeout(() => editRule(null), 60);
  if (r.params.get("routes")) setTimeout(() => routesDrawer(), 60);
  const offs = [bus.on("alerts", draw), bus.on("whoami", () => { m = null; draw(); }), bus.on("cluster", () => { if (!state.paused && m) drawSummary(); }), bus.on("range", () => { longFor = null; loadLong(); draw(); }), bus.on("narrow", () => { if (m) drawHistory(); })];
  const stop = every(30000, () => { if (!state.paused && range().live) { longFor = null; return loadLong(); } }, false);
  return () => { offs.forEach((f) => f()); stop(); openRule = openRoutes = null; };
}

const runbookName = (u) => { try { const p = new URL(u); const last = p.pathname.split("/").filter(Boolean).pop(); return last ? decodeURIComponent(last).slice(0, 24) : p.host; } catch { return u.slice(0, 24); } };
function fireBars(ts, t) {
  const n = 14, bins = new Array(n).fill(0);
  for (const v of ts) bins[Math.min(n - 1, Math.max(0, Math.floor((v - t.from) / (t.to - t.from) * n)))]++;
  const max = Math.max(1, ...bins);
  return s("svg", { viewBox: `0 0 ${n * 5} 16`, width: n * 5, height: 16, class: "al-fires", "aria-hidden": "true" }, bins.map((b, i) => b ? s("rect", { x: i * 5, y: 16 - Math.max(3, b / max * 16), width: 3.5, height: Math.max(3, b / max * 16) }) : null));
}
/** The state in words; no data is its own neutral pill, never "info" (A1). */
function statePill(r, x) {
  if (!r.enabled) return h("span", { class: "pill disabled" }, icon("minus"), "off");
  if (x.silent) return h("span", { class: "pill al-silent", title: `${x.silenced.reason || ""}` }, icon("pause"), `silenced to ${hm(x.silenced.until_ms)}`);
  if (x.state === "firing") return pill(r.severity === "info" ? "warn" : "firing", `firing ${x.since_ms ? dur(Date.now() - x.since_ms) : ""}`.trim());
  if (x.state === "pending") return pill("pending", "pending");
  if (x.state === "no_data") return h("span", { class: "pill al-nodata", title: "The metric has no value here, so the rule cannot tell. It keeps its last state and never resolves on its own." }, icon("minus"), `no data${x.no_data_since_ms || x.since_ms ? " " + dur(Date.now() - (x.no_data_since_ms || x.since_ms)) : ""}`);
  return pill("ok", "ok");
}
function ackPill(r, x, fleet) {
  if (x.state !== "firing") return pill("pending", "pending");
  if (x.ack) return h("span", { class: "pill ok", title: x.ack.note || "" }, icon("check"), `acked · ${x.ack.by || "someone"}`);
  if (!fleet) return h("span", { class: "pill " + (r.severity === "critical" ? "critical" : "warn") }, `firing ${x.since_ms ? dur(Date.now() - x.since_ms) : ""}`);
  return h("span", { class: "pill " + (r.severity === "critical" ? "critical" : "warn") }, `unacked ${x.since_ms ? dur(Date.now() - x.since_ms) : ""}`);
}
/** A switch with its state in words (A8: the old one rendered as a bare knob). */
function onSwitch(r, disabled, onChange) {
  const inp = h("input", { type: "checkbox", checked: r.enabled, disabled, "aria-label": `${r.name}: checked or not` });
  const lbl = h("span", { class: "al-sw-l" }, r.enabled ? "on" : "off");
  inp.onchange = () => { lbl.textContent = inp.checked ? "on" : "off"; onChange(inp.checked); };
  return h("label", { class: "al-switch" + (disabled ? " off" : ""), title: disabled ? Needs("admin") : r.enabled ? "On: the rule is checked. Turn it off" : "Off: nothing checks it. Turn it on", onclick: (e) => e.stopPropagation() }, inp, h("span", { class: "al-track" }), lbl);
}

function silenceMenu(anchor, r, after) {
  const until9 = () => { const d = new Date(); d.setDate(d.getDate() + (d.getHours() >= 9 ? 1 : 0)); d.setHours(9, 0, 0, 0); return Math.round((d - Date.now()) / 60e3); };
  menu(anchor, [[30, "for 30 minutes"], [60, "for 1 hour"], [240, "for 4 hours"], [until9(), "until 09:00"]].map(([min, label]) => ({ label: `Silence ${label}`, icon: "pause", onclick: () => silence(r, min, after) })));
  const el = [...document.querySelectorAll(".menu")].pop();
  if (el && !isNarrow()) requestAnimationFrame(() => { const b = el.getBoundingClientRect(); if (b.bottom > innerHeight - 8) el.style.top = Math.max(8, anchor.getBoundingClientRect().top - b.height - 4) + "px"; });
}
async function silence(r, minutes, after) {
  const until = Date.now() + minutes * 60e3;
  const ok = await ask({ title: `Silence "${r.name}" until ${hm(until)}?`, body: `It stays on this page, greyed, and keeps its state; no message is sent for it until ${when(until)}. The silence ends by itself.`, ok: "Silence" });
  if (!ok) return;
  try { await api(`/v1/alerts/${encodeURIComponent(r.id)}/silence`, { body: { minutes, reason: ok.reason || "" }, reason: ok.reason }); toast(`Silenced until ${hm(until)}`, "ok"); await refreshAlerts(); after?.(); }
  catch (e) { toast(failMsg(e, "Silencing"), "bad"); }
}

async function saveRules(next, reason) {
  try {
    await api("/v1/alerts/rules", { method: "PUT", body: { rules: next.map(cleanRule) }, reason: reason || undefined });
    await refreshAlerts();
  } catch (e) { toast(e instanceof ApiError && e.status === 403 ? `Changing rules ${needs("admin")}` : e.message, "bad"); throw e; }
}
function cleanRule(x) {
  const out = { id: x.id, name: x.name, metric: x.metric, op: x.op, threshold: Number(x.threshold), for_s: Number(x.for_s) || 0, severity: x.severity, enabled: !!x.enabled, description: x.description || "" };
  for (const k of ["eval", "window_s", "resolve", "scope", "owner", "runbook", "route", "builtin"]) if (x[k] != null && x[k] !== "") out[k] = x[k];
  if (out.window_s != null) out.window_s = Number(out.window_s);
  if (out.resolve != null) out.resolve = Number(out.resolve);
  return out;
}

// ---------------------------------------------------------------- charts
/**
 * A time chart for the alert page and the backtest: series of [t, v], a
 * threshold and a resolve line, shaded spans when the rule fired, and
 * labelled markers (events) along the top. o: {t0, t1, series: [{name, cls,
 * pts}], thr, resolve, spans: [[a, b]], markers: [{t, label, tone}], fmtv}.
 */
function alertChart(height = 240) {
  const root = h("div", { class: "chart al-chart" });
  const svgEl = s("svg", { height, role: "img", "aria-label": "the metric over time" });
  root.append(svgEl);
  let o = null, width = 0;
  function draw() {
    if (!o) return;
    width = root.clientWidth || 700;
    svgEl.setAttribute("viewBox", `0 0 ${width} ${height}`);
    svgEl.replaceChildren();
    const marks = (o.markers || []).filter((k) => k.t >= o.t0 && k.t <= o.t1).sort((a, b) => a.t - b.t);
    const span = Math.max(1, o.t1 - o.t0);
    const M = { l: 52, r: 14, b: 22, t: 8 };
    const W = width - M.l - M.r;
    const X = (t) => M.l + (t - o.t0) / span * W;
    // Marker labels in lanes along the top, so they do not overlap.
    const lanes = [];
    for (const k of marks) {
      const w = Math.min(190, 12 + k.label.length * 6.1), x = Math.min(width - M.r - w, Math.max(M.l, X(k.t) - 4));
      let lane = lanes.findIndex((end) => end < x - 4);
      if (lane < 0 && lanes.length < 4) { lanes.push(0); lane = lanes.length - 1; }
      if (lane < 0) { k.lane = -1; continue; }
      lanes[lane] = x + w; k.lane = lane; k.x = x; k.w = w;
    }
    M.t = 8 + lanes.length * 17;
    const H = height - M.t - M.b;
    let ymax = 0;
    for (const sr of o.series) for (const [, v] of sr.pts) if (v != null && v > ymax) ymax = v;
    if (o.thr != null) ymax = Math.max(ymax, o.thr * 1.2);
    if (o.resolve != null) ymax = Math.max(ymax, o.resolve * 1.1);
    if (!(ymax > 0)) ymax = 1;
    const step = niceStep(ymax, 4);
    ymax = Math.ceil(ymax / step) * step;
    const Y = (v) => M.t + H - v / ymax * H;
    const g = s("g", { class: "grid" }), ax = s("g", { class: "axis" });
    for (let v = 0; v <= ymax + 1e-12; v += step) { g.append(s("line", { x1: M.l, x2: width - M.r, y1: Y(v), y2: Y(v) })); ax.append(s("text", { x: M.l - 6, y: Y(v) + 3.5, "text-anchor": "end" }, o.fmtv(v))); }
    for (const t of ticks(o.t0, o.t1, W)) ax.append(s("text", { x: X(t), y: height - 6, "text-anchor": "middle" }, span > 86400e3 * 1.5 ? new Date(t).toLocaleDateString(undefined, { month: "short", day: "numeric" }) + (new Date(t).getHours() ? " " + fmt.time(t, false) : "") : fmt.time(t, false)));
    svgEl.append(g);
    for (const [a, b] of o.spans || []) { const x0 = X(Math.max(o.t0, a)), x1 = X(Math.min(o.t1, b ?? o.t1)); if (x1 > M.l && x0 < width - M.r) svgEl.append(s("rect", { class: "al-fired", x: x0, y: M.t, width: Math.max(2, x1 - x0), height: H })); }
    svgEl.append(ax, s("line", { class: "base", x1: M.l, x2: width - M.r, y1: Y(0), y2: Y(0) }));
    for (const sr of o.series) {
      let d = "", open = false;
      for (const [t, v] of sr.pts) { if (v == null || !Number.isFinite(v)) { open = false; continue; } d += (open ? "L" : "M") + X(t).toFixed(1) + "," + Y(Math.min(v, ymax)).toFixed(1); open = true; }
      svgEl.append(s("path", { class: "al-ln " + (sr.cls || ""), d }));
    }
    if (o.thr != null) svgEl.append(s("line", { class: "thr", x1: M.l, x2: width - M.r, y1: Y(o.thr), y2: Y(o.thr) }), s("text", { class: "thr-lbl", x: width - M.r - 4, y: Y(o.thr) - 5, "text-anchor": "end" }, `threshold ${o.fmtv(o.thr)}`));
    if (o.resolve != null && o.resolve !== o.thr) svgEl.append(s("line", { class: "al-resolve", x1: M.l, x2: width - M.r, y1: Y(o.resolve), y2: Y(o.resolve) }), s("text", { class: "al-resolve-l", x: M.l + 4, y: Y(o.resolve) - 4 }, `resolves at ${o.fmtv(o.resolve)}`));
    for (const k of marks) {
      const x = X(k.t);
      svgEl.append(s("line", { class: "al-mk " + (k.tone || ""), x1: x, x2: x, y1: k.lane >= 0 ? 8 + k.lane * 17 + 14 : M.t, y2: M.t + H }));
      if (k.lane >= 0) svgEl.append(s("g", { class: "al-mkl " + (k.tone || "") }, s("rect", { x: k.x, y: 8 + k.lane * 17, width: k.w, height: 14, rx: 3 }), s("text", { x: k.x + 5, y: 8 + k.lane * 17 + 10.5 }, k.label.length > 30 ? k.label.slice(0, 29) + "…" : k.label), s("title", {}, `${when(k.t)} ${k.label}`)));
    }
    // Hover: every series at the nearest point, and the markers near it.
    const xh = s("line", { class: "xhair", y1: M.t, y2: M.t + H, style: { display: "none" } });
    const over = s("rect", { x: M.l, y: M.t, width: W, height: H, fill: "transparent" });
    svgEl.append(xh, over);
    over.addEventListener("pointermove", (e) => {
      const r = svgEl.getBoundingClientRect();
      const t = o.t0 + ((e.clientX - r.left) * (width / r.width) - M.l) / W * span;
      xh.setAttribute("x1", X(t)); xh.setAttribute("x2", X(t)); xh.style.display = "";
      const rows = o.series.map((sr) => { const p = nearest(sr.pts, t); return h("div", { class: "r" }, h("span", { class: "k al-k " + (sr.cls || "") }), h("b", {}, p ? o.fmtv(p[1]) : "–"), h("span", { class: "n" }, sr.name)); });
      const near = marks.filter((k) => Math.abs(k.t - t) < span / W * 8).map((k) => h("div", { class: "r" }, h("span", { class: "n", style: { marginLeft: 0, paddingLeft: 0 } }, `${hm(k.t)} ${k.label}`)));
      showTip(e.clientX, e.clientY, [h("div", { class: "t" }, when(t)), ...rows, ...near]);
    });
    over.addEventListener("pointerleave", () => { xh.style.display = "none"; hideTip(); });
  }
  new ResizeObserver(() => { if (root.isConnected && Math.abs(root.clientWidth - width) > 2) draw(); }).observe(root);
  root.update = (x) => { o = x; draw(); };
  return root;
}
function nearest(pts, t) {
  let lo = 0, hi = pts.length - 1;
  if (hi < 0) return null;
  while (lo < hi) { const mid = (lo + hi) >> 1; if (pts[mid][0] < t) lo = mid + 1; else hi = mid; }
  if (lo > 0 && Math.abs(pts[lo - 1][0] - t) < Math.abs(pts[lo][0] - t)) lo--;
  return pts[lo][1] == null ? null : pts[lo];
}
const STEPS = [60e3, 120e3, 300e3, 600e3, 900e3, 1800e3, 3600e3, 7200e3, 10800e3, 21600e3, 43200e3, 86400e3];
function ticks(t0, t1, w) {
  const want = Math.max(2, Math.floor(w / 95));
  const st = STEPS.find((x) => (t1 - t0) / x <= want) || 86400e3;
  const off = st >= 86400e3 ? new Date(t0).getTimezoneOffset() * 60e3 : 0;
  const out = [];
  for (let t = Math.ceil((t0 - off) / st) * st + off; t <= t1; t += st) out.push(t);
  return out;
}

// ---------------------------------------------------------------- evaluation
/** The value a rule tests at each point, from raw [t, v] points (as the server does). */
function evaluate(pts, r) {
  const ev = r.eval || (r.window_s ? "mean" : "all");
  if (ev === "all") return pts;
  const w = (r.window_s || 300) * 1000, out = [];
  let j = 0;
  for (let i = 0; i < pts.length; i++) {
    while (pts[j][0] <= pts[i][0] - w) j++;
    const win = pts.slice(j, i + 1).filter((p) => p[1] != null);
    if (!win.length) { out.push([pts[i][0], null]); continue; }
    const vs = win.map((p) => p[1]);
    out.push([pts[i][0], ev === "max" ? Math.max(...vs) : ev === "min" ? Math.min(...vs) : vs.reduce((a, b) => a + b, 0) / vs.length]);
  }
  return out;
}
/** When a rule fires on evaluated points: [[from, to|null]], with for_s and the resolve threshold. */
function firings(pts, r) {
  const spans = [];
  let pend = null, on = false;
  const res = r.resolve != null && r.resolve !== "" ? Number(r.resolve) : Number(r.threshold);
  for (const [t, v] of pts) {
    if (v == null) continue; // no data keeps the state
    if (on) { if (!holds(v, r.op, res) && !(r.op.startsWith(">") ? v > res : v < res)) { spans[spans.length - 1][1] = t; on = false; pend = null; } continue; }
    if (holds(v, r.op, Number(r.threshold))) { pend ??= t; if (t - pend >= (Number(r.for_s) || 0) * 1000) { spans.push([pend, null]); on = true; } }
    else pend = null;
  }
  return spans;
}
const median = (xs) => { if (!xs.length) return null; const a = xs.slice().sort((x, y) => x - y); return a[Math.floor(a.length / 2)]; };

/** Raw points of a metric over [from, to): the server's series (per minute beyond its hour), else the browser's. */
async function seriesOf(metric, from, to) {
  let pts = null, source = "memory";
  try {
    const j = await api(`/v1/timeseries?from=${Math.round(from)}&to=${Math.round(to)}`);
    pts = (j.points || []).filter((p) => p.t >= from && p.t <= to);
    if (j.source) source = j.source;
  } catch { /* the browser's own */ }
  if (!pts?.length) pts = (state.points || []).filter((p) => p.t >= from && p.t <= to);
  return { pts: pts.map((p) => [p.t, metricOf(p, metric)]), source, has: pts.some((p) => metricOf(p, metric) != null) };
}
function metricOf(p, metric) {
  const v = p[metric];
  if (v != null) return v;
  if (metric === "error_ratio" && p.queries) return p.query_errors != null ? p.query_errors / p.queries : null;
  if (metric === "availability" && p.queries) return 1 - (p.query_errors || 0) / p.queries;
  return null;
}

// ---------------------------------------------------------------- the alert page
const TONE = { alert_fired: "bad", alert_resolved: "good", alert_acked: "good", alert_silenced: "muted", copy_down: "bad", majority_lost: "bad", copy_up: "good", majority_back: "good", primary_changed: "warn", rollout: "accent", upgrade: "accent", change: "accent", log_level: "accent", autoscale: "accent", contain: "warn", note: "muted", moves_started: "accent", moves_finished: "accent", shard_added: "accent", shard_removed: "accent", copy_added: "accent", copy_removed: "accent" };
const WINDOWS = [["15m", "±15 min", 15 * 60e3], ["1h", "±1 h", 3600e3], ["6h", "±6 h", 6 * 3600e3], ["7d", "7 days", 7 * 86400e3]];

function alertHub(page, id, r) {
  id = decodeURIComponent(id || "");
  if (!id) { location.hash = "#/alerts"; return; }
  const fixedAtOpen = range().fixed;
  let win = r?.params?.get("w") || (fixedAtOpen ? "range" : "1h"), m = null, one = null, rule = null, x = null, gen = 0, notesAt = null;
  const crumbs = h("div", { class: "row small muted al-crumbs" }, h("a", { href: link("alerts") }, "Alerts"), h("span", {}, "›"), h("span", { class: "mono" }, id));
  const title = h("h1", { class: "row nw", style: { gap: "10px" } });
  const line = h("p");
  const tools = h("div", { class: "tools al-hubbar" });
  const kpis = h("div", { class: "grid g4 al-kpis" });
  const chart = alertChart(250);
  const chartNote = h("div", { class: "small muted", style: { marginTop: "6px" } });
  const winSeg = seg([...(fixedAtOpen ? [["range", "the link's window"]] : []), ...WINDOWS.map(([k, l]) => [k, l])], win, (v) => { win = v; loadChart(); }, { "aria-label": "Time around the alert" });
  const chartSub = h("span", { class: "sub" });
  const blast = h("div");
  const changed = h("div");
  const stmts = h("div");
  const copies = h("div");
  const logsBox = h("div");
  const logsLink = h("a", { class: "btn sm ghost" }, "Open in Logs", icon("right"));
  const activity = h("div");
  const noteIn = h("input", { class: "input", placeholder: "Add a note: what you found, what you did", style: { flex: "1 1 auto" } });
  const noteBtn = h("button", { class: "btn" }, "Post");
  page.append(
    crumbs,
    h("div", { class: "page-head", style: { marginBottom: "12px" } }, h("div", {}, title, line), tools),
    kpis,
    h("div", { class: "grid g3 al-row" },
      h("section", { class: "card span2" }, h("div", { class: "hd" }, h("h3", {}, "Around the alert"), chartSub, h("div", { class: "tools" }, winSeg)), h("div", { class: "bd" }, chart, chartNote)),
      card({ title: "Blast radius", sub: "since it fired", body: blast })),
    h("div", { class: "grid g3 al-row" },
      card({ title: "What changed", sub: "events from 30 min before it fired", body: changed }),
      card({ title: "Failing statements", sub: "server failures, since it fired", tools: h("a", { class: "btn sm ghost", href: link("queries", { tab: "statements" }) }, "Queries", icon("right")), body: stmts, flush: true }),
      card({ title: "Affected copies", tools: h("a", { class: "btn sm ghost", href: link("cluster") }, "Cluster", icon("right")), body: copies, flush: true })),
    h("div", { class: "grid g3 al-row" },
      card({ title: "Log lines, grouped", sub: "warn and error, every server, since 5 min before it fired", tools: logsLink, body: logsBox, flush: true, cls: "span2" }),
      card({ title: "Activity", sub: "this alert's timeline, shared with the link", body: [activity, h("div", { class: "row nw", style: { marginTop: "10px" } }, noteIn, noteBtn)] })),
    h("div", { class: "al-hubspace", "aria-hidden": "true" }));

  const since = () => x?.since_ms && (x.state === "firing" || x.state === "pending") ? x.since_ms : lastFired() || Date.now() - 3600e3;
  const lastFired = () => { const f = (one?.events || m?.events || []).filter((e) => e.rule === id && e.what === "fired").map((e) => e.ts); return f.length ? Math.max(...f) : null; };

  async function load() {
    const my = ++gen;
    m = model(state.alerts) || model(await api("/v1/alerts").catch(() => null));
    try {
      const j = await api(`/v1/alerts/${encodeURIComponent(id)}`);
      one = { ...j, events: (j.events || []).map(normEvent), notes: j.notes || j.state?.notes || [] };
    } catch { one = null; }
    if (my !== gen) return;
    rule = m?.rules.find((y) => y.id === id) || (one?.rule && typeof one.rule === "object" ? one.rule : null);
    if (!rule) { clear(page, crumbs, empty("alerts", `No rule "${id}"`, "It may have been deleted or renamed.", h("a", { class: "btn sm", href: link("alerts") }, "All alerts"))); return; }
    const fromOne = one && (one.state && typeof one.state === "object" ? one.state : one);
    x = { ...(m.st.get(id) || {}), ...(fromOne?.state && typeof fromOne.state === "string" ? { ...fromOne, state: stateName(fromOne.state) } : {}) };
    x.silent = !!(x.silenced && (!x.silenced.until_ms || x.silenced.until_ms > Date.now()));
    if (!rule.enabled) x.state = "disabled";
    drawHead(); drawKpis(); drawCopies(); drawActivity();
  }

  function drawHead() {
    document.title = `${rule.name} · Alert · Strata`;
    const st = x.state;
    const tone = st === "firing" ? (rule.severity === "critical" ? "critical pulse" : "warn") : st === "pending" ? "pending" : st === "ok" ? "ok" : "";
    clear(title, h("span", { class: "dot " + tone, style: { width: "10px", height: "10px" } }), rule.name,
      st === "firing" ? h("span", { class: "pill " + (rule.severity === "critical" ? "critical" : "warn") }, `${rule.severity} · firing ${x.since_ms ? dur(Date.now() - x.since_ms) : ""}`) : statePill(rule, x),
      rule.builtin ? h("span", { class: "badge accent" }, "built-in") : null);
    const w = (x.where || []).filter((c) => c.state === "firing");
    clear(line, cond(rule), " · ", rule.scope === "each_copy" ? h("b", { class: "al-strong" }, w.length ? `on ${w.length} of ${fmt.n(Math.max((x.where || []).length, allCopies()))} copies` : `on each copy (${fmt.n(allCopies())})`) : h("b", { class: "al-strong" }, scopeText(rule)[0]),
      x.since_ms && st !== "ok" ? ` · since ${when(x.since_ms)}` : "", ` · now ${x.value == null ? "no value" : fv(rule.metric, x.value)}`, h("span", { class: "al-peak" }));
    const w8 = can("write"), fleet = m.fleet;
    const ackBtn = x.ack
      ? h("button", { class: "btn al-acked", disabled: true, title: x.ack.note || "" }, icon("check"), h("span", { class: "al-wide-only" }, `Acknowledged · ${x.ack.by || "someone"} ${hm(x.ack.at_ms)}`), h("span", { class: "al-narrow-only" }, "Acked"))
      : h("button", { class: "btn primary", disabled: !fleet || !w8 || st !== "firing", title: !fleet ? "This server cannot acknowledge alerts yet" : !w8 ? Needs("write") : st !== "firing" ? "Only a firing alert is acknowledged" : "Acknowledge: you are on it", onclick: async () => {
        try { await api(`/v1/alerts/${encodeURIComponent(id)}/ack`, { body: {} }); toast("Acknowledged", "ok"); await refreshAlerts(); load(); } catch (e) { toast(failMsg(e, "Acknowledging"), "bad"); }
      } }, icon("check"), h("span", { class: "al-wide-only" }, "Acknowledge"), h("span", { class: "al-narrow-only" }, "Ack"));
    const silBtn = x.silent
      ? h("button", { class: "btn", disabled: !w8, title: `Silenced by ${x.silenced.by || "someone"}: ${x.silenced.reason || ""}`, onclick: async () => {
        const ok = await ask({ title: "End the silence now?", body: "Messages for this alert are sent again from now on.", ok: "End the silence" });
        if (!ok) return;
        try { await api(`/v1/alerts/${encodeURIComponent(id)}/silence`, { method: "DELETE", reason: ok.reason }); toast("Silence ended", "ok"); await refreshAlerts(); load(); } catch (e) { toast(failMsg(e, "Ending the silence"), "bad"); }
      } }, icon("pause"), h("span", { class: "al-wide-only" }, `Silenced to ${hm(x.silenced.until_ms)} · end`), h("span", { class: "al-narrow-only" }, `To ${hm(x.silenced.until_ms)} · end`))
      : h("button", { class: "btn", disabled: !fleet || !w8, title: !fleet ? "This server cannot silence alerts yet" : !w8 ? Needs("write") : "Silence it for a while, with a reason" }, icon("pause"), "Silence", icon("down"));
    if (!x.silent) silBtn.onclick = () => silenceMenu(silBtn, rule, () => load());
    clear(tools, ackBtn, silBtn,
      h("button", { class: "btn", disabled: !fleet || !w8, title: !fleet ? "This server keeps no notes yet" : !w8 ? Needs("write") : "Add a note to the alert's timeline", onclick: () => { noteIn.scrollIntoView({ block: "center", behavior: "smooth" }); noteIn.focus({ preventScroll: true }); } }, icon("edit"), h("span", { class: "al-wide-only" }, "Add note"), h("span", { class: "al-narrow-only" }, "Note")),
      h("button", { class: "btn al-ico", title: "Copy a link to this alert at this time", onclick: () => { const t = since(); copy(location.origin + location.pathname + alertLink(id, { from: Math.round(t - 30 * 60e3), to: Math.round(x.state === "firing" ? Date.now() : t + 60 * 60e3) }), "Link copied: it opens this alert at this time"); } }, icon("link"), h("span", { class: "al-wide-only" }, "Copy link")),
      h("button", { class: "btn ghost al-ico", title: "Edit the rule", onclick: () => editRule(rule, { after: load }) }, icon("sliders"), h("span", { class: "al-wide-only" }, "Edit rule")));
    noteIn.disabled = noteBtn.disabled = !fleet || !w8;
    noteIn.placeholder = !fleet ? "Notes need a newer server" : !w8 ? Needs("write") : "Add a note: what you found, what you did";
  }

  function drawKpis() {
    const route = m.routes.find((rt) => rt.name === (rule.route || "default")) || (m.routes.length === 1 ? m.routes[0] : null);
    const evs = (one?.events || m.events).filter((e) => e.rule === id);
    const fired = evs.filter((e) => e.what === "fired").sort((a, b) => a.ts - b.ts);
    const resolved = evs.filter((e) => e.what === "resolved").sort((a, b) => a.ts - b.ts);
    const lens = fired.map((f) => { const r2 = resolved.find((e) => e.ts > f.ts); return r2 ? r2.ts - f.ts : null; }).filter((v) => v != null);
    const flaps = lens.filter((v) => v < 120e3).length;
    const kp = (label, val, sub, attrs = {}) => h("div", { class: "card kpi al-kpi", ...attrs }, h("div", { class: "lbl" }, label), h("div", { class: "val" }, val), sub ? h("div", { class: "delta" }, sub) : null);
    const ed = () => editRule(rule, { after: load });
    clear(kpis,
      kp("Owner", rule.owner || h("span", { class: "muted" }, "No owner"), rule.owner ? null : can("admin") ? h("button", { class: "al-link", onclick: ed }, "Set an owner") : "the rule names none"),
      kp("Runbook", rule.runbook ? h("a", { href: rule.runbook, target: "_blank", rel: "noopener", title: rule.runbook }, runbookName(rule.runbook), " ", icon("ext")) : h("span", { class: "muted" }, "No runbook"), rule.description ? h("span", { title: rule.description }, rule.description) : rule.runbook ? null : can("admin") ? h("button", { class: "al-link", onclick: ed }, "Add one") : null),
      kp("Notified", route ? (route.legacy ? "the webhook" : route.name) : h("span", { class: "muted" }, "Nobody"),
        !route ? "no route: add one in Routes" : route.legacy ? "--alert-webhook; delivery is not tracked by this server"
          : route.last_error && (!route.last_ok_ms || (route.last_error_ms || 0) > route.last_ok_ms) ? h("span", { class: "al-bad" }, `last send failed: ${route.last_error}`)
            : route.last_ok_ms ? h("span", { class: "al-good" }, `delivered ${when(route.last_ok_ms)} · ${fmt.n(route.sent || 0)} sent`) : "nothing sent yet"),
      kp(`Noise${one ? " (7 days)" : " (in memory)"}`, `${fmt.plural(fired.length, "fire")} · ${fmt.plural(flaps, "flap")}`, lens.length ? `median ${dur(median(lens))} firing` : fired.length ? "still firing" : "it did not fire"));
  }

  async function loadChart() {
    if (!rule) return;
    const now = Date.now(), at = since(), w = WINDOWS.find(([k]) => k === win)?.[2] || 3600e3;
    const rr = range();
    const from = win === "range" ? rr.from : win === "7d" ? now - w : at - w, to = win === "range" ? rr.to : win === "7d" ? now : Math.min(now, at + w);
    chartSub.textContent = `${rule.metric}${rule.eval && rule.eval !== "all" ? `, ${EVAL[rule.eval]} ${dur((rule.window_s || 300) * 1000)} (red) and raw points (grey)` : ""} · ${hm(from)} – ${to >= now - 5e3 ? "now" : hm(to)}`;
    const [raw, bt, evs] = await Promise.all([
      seriesOf(rule.metric, from, to),
      api("/v1/alerts/backtest", { body: { rule: cleanRule(rule), from: Math.round(from), to: Math.round(to) } }).catch(() => null),
      api(`/v1/events?from=${Math.round(from)}&to=${Math.round(to)}&limit=500`).then((j) => j.events || []).catch(() => null),
    ]);
    const ev = bt?.points ? bt.points.map((p) => [p.t, p.value]) : evaluate(raw.pts, rule);
    const spans = bt?.fired ? bt.fired.map((f) => [f.from_ms, f.to_ms]) : firings(ev, rule);
    const markers = (evs || []).map((e) => ({ t: e.ts_ms, label: e.title || e.kind, tone: TONE[e.kind] || "accent" }));
    if (!evs) for (const e of m.events.filter((e2) => e2.rule === id)) markers.push({ t: e.ts, label: e.what === "fired" ? "fired" : "resolved", tone: e.what === "fired" ? "bad" : "good" });
    const series = [];
    if (rule.eval && rule.eval !== "all" && raw.has) series.push({ name: "raw points", cls: "raw", pts: raw.pts });
    series.push({ name: rule.eval && rule.eval !== "all" ? `${EVAL[rule.eval]} ${dur((rule.window_s || 300) * 1000)}` : rule.metric, cls: "eval", pts: ev });
    chart.update({ t0: from, t1: to, series, thr: Number(rule.threshold), resolve: rule.resolve != null ? Number(rule.resolve) : null, spans, markers, fmtv: (v) => fv(rule.metric, v) });
    const vals = ev.filter((p) => p[1] != null && p[0] >= at);
    const peak = vals.reduce((a, p) => (!a || (rule.op.startsWith(">") ? p[1] > a[1] : p[1] < a[1]) ? p : a), null);
    const pk = line.querySelector(".al-peak");
    if (pk) pk.textContent = peak ? ` · ${rule.op.startsWith(">") ? "peak" : "lowest"} ${fv(rule.metric, peak[1])} at ${hm(peak[0])}` : "";
    const hasEval = ev.some((p) => p[1] != null);
    clear(chartNote, !hasEval
      ? (rule.scope === "each_copy" || METRICS[rule.metric]?.[2] === "copy" ? `This server's points have no ${rule.metric}: it lives on the copies. Their values are in Affected copies.` : `No ${rule.metric} values in this time.`)
      : [bt?.source ? `Evaluated by the server from ${bt.source === "load" ? "the per-minute load table" : "its memory"}.` : `Evaluated here from ${raw.source === "load" ? "per-minute points" : "the 2-s points"}.`, evs ? " Markers: events of the cluster (failovers, rollouts, changes, notes)." : " Markers: this rule's own events (the server keeps no event log)."]);
    drawBlast(raw, at);
    drawChanged(evs, at);
  }

  async function drawBlast(raw, at) {
    // Queries and failures since it fired, from the coordinator's own points.
    const all = (state.points || []).length && at > (state.points[0]?.t || Infinity) ? state.points.filter((p) => p.t >= at) : null;
    let q = 0, bad = 0, cli = 0, rej = 0;
    const src = all || (await api(`/v1/timeseries?from=${Math.round(at)}&to=${Date.now()}`).then((j) => j.points || []).catch(() => []));
    for (const p of src) {
      if (p.t < at) continue;
      const dt = p.secs || (p.interval_ms || 2000) / 1000;
      if (p.queries != null) { q += p.queries; bad += p.query_errors || 0; cli += p.client_errors || 0; }
      else { q += (p.qps || 0) * dt; bad += (p.errors_s || 0) * dt; cli += (p.client_errors_s ?? p.http_4xx_s ?? 0) * dt; }
      rej += (p.rejected_s || 0) * dt;
    }
    const shards = (state.cluster?.shards || []);
    const hurt = shards.filter((sh) => (sh.state && sh.state !== "ok") || (!sh.state && sh.status && sh.status !== "ok"));
    const row = (k, v, sub) => [h("dt", {}, k), h("dd", {}, v, sub ? h("div", { class: "tiny muted" }, sub) : null)];
    const kv = h("dl", { class: "kv al-blast" },
      row("Queries", q ? fmt.n(Math.round(q)) : "–", q ? `coordinator's own, since ${hm(at)}` : "no points for this time"),
      row("Failed (server)", q ? h("span", { class: bad / q > 0.01 ? "al-bad" : "" }, `${fmt.n(Math.round(bad))} (${fmt.pct(bad / q)})`) : "–", "errors and server timeouts"),
      row("Client errors", q ? `${fmt.n(Math.round(cli))}` : "–", "400s: bad SQL, the client's own deadline; not this incident's"),
      rej ? row("Refused (429)", fmt.n(Math.round(rej))) : null,
      shards.length ? row("Shards", hurt.length ? h("span", { class: "al-bad" }, `${hurt.length} of ${shards.length}: ${hurt.slice(0, 4).map((sh) => `shard ${sh.shard}${sh.state ? " " + sh.state.replace("_", " ") : ""}`).join(", ")}`) : `all ${shards.length} serve reads and writes`, hurt[0]?.why || null) : null);
    clear(blast, kv, h("div", { class: "small muted", style: { marginTop: "6px" } }, "Clients and statements: see Failing statements."));
  }

  function drawChanged(evs, at) {
    if (!evs) { clear(changed, h("div", { class: "small muted" }, "This server keeps no event log (failovers, rollouts, changes). A newer strata-server records them at /v1/events.")); return; }
    const list = evs.filter((e) => e.ts_ms >= at - 30 * 60e3 && !e.kind?.startsWith("alert_")).sort((a, b) => b.ts_ms - a.ts_ms).slice(0, 12);
    clear(changed, list.length ? h("div", { class: "al-changes" }, list.map((e) => h("div", { class: "al-chg" },
      h("span", { class: "al-time", title: whenTitle(e.ts_ms) }, hm(e.ts_ms)),
      h("div", {}, h("div", {}, h("span", { class: "al-dot " + (TONE[e.kind] || "") }), e.title || e.kind), h("div", { class: "tiny muted" }, [e.who ? `by ${e.who}` : null, e.where?.copy ? copyName(e.where.copy) : e.where?.shard != null ? `shard ${e.where.shard}` : null, e.until_ms ? `until ${hm(e.until_ms)}` : null].filter(Boolean).join(" · ")))))) : h("div", { class: "small muted" }, "Nothing changed in the 30 minutes before it fired, or since: no failover, rollout, setting or note."));
  }

  async function loadStatements() {
    const at = since();
    try {
      const j = await api(`/v1/queries/stats?from=${Math.round(at)}&to=${Date.now()}`);
      const list = Array.isArray(j) ? j : j.statements || [];
      const errs = (x2) => x2.outcomes ? (x2.outcomes.error || 0) + (x2.outcomes.timeout || 0) : (x2.errors || 0);
      const failing = list.filter((x2) => errs(x2) > 0).sort((a, b) => errs(b) - errs(a)).slice(0, 6);
      const windowed = Array.isArray(j) || j.from_ms != null;
      clear(stmts, failing.length ? table([
        { key: "sql", label: "statement", render: (x2) => h("a", { class: "mono small al-sql", href: link("statement/" + encodeURIComponent(x2.fingerprint)), title: x2.sql || x2.example || x2.fingerprint }, (x2.sql || x2.example || x2.fingerprint).slice(0, 140)) },
        { key: "f", label: "failed", num: true, sort: errs, render: (x2) => h("b", {}, fmt.si(errs(x2))) },
        { key: "p99_ms", label: "p99", num: true, render: (x2) => h("span", { class: "nowrap" }, fmt.ms(x2.p99_ms)) },
      ], { rows: failing, compact: true }) : h("div", { class: "small muted", style: { padding: "4px 14px 12px" } }, `No statement failed on the server's side${windowed ? " since it fired" : ""}.`),
      windowed ? null : h("div", { class: "tiny muted", style: { padding: "0 14px 10px" } }, "Counted since this server started, client errors (bad SQL) included: it keeps no per-window statements."));
    } catch (e) { clear(stmts, h("div", { class: "small muted", style: { padding: "4px 14px 12px" } }, e instanceof ApiError && e.status === 403 ? "Statements need an admin key." : "No statements from this server.")); }
  }

  function drawCopies() {
    let rows = (x.where || []).slice().sort((a, b) => (RANK[stateName(a.state)] ?? 5) - (RANK[stateName(b.state)] ?? 5));
    let cause = false;
    if (!rows.length && x.state !== "ok" && x.state !== "disabled") {
      // A rule of this server's own points has no copies; the copies of the shards that are not ok are the likely cause.
      const bad = (state.cluster?.shards || []).filter((sh) => (sh.state && sh.state !== "ok") || (!sh.state && sh.status && sh.status !== "ok")).slice(0, 4);
      rows = bad.flatMap((sh) => (sh.replicas || []).map((rp, i) => ({ copy: rp.url, short: copyName(rp.url), state: rp.status === "down" || rp.role === "unknown" ? "firing" : "ok", value: null, since_ms: rp.down_since_ms || null })));
      cause = rows.length > 0;
    }
    const shardsOf = new Set();
    const lines = rows.slice(0, 12).map((c) => {
      const url = c.copy, cp = copyOf(url || c.short), name = whereName(c);
      if (cp) shardsOf.add(cp.shard);
      const node = cp?.r?.name || cp?.r?.detail?.log_store?.node || null;
      const st = stateName(c.state);
      return h("tr", {},
        h("td", {}, h("span", { class: "row nw", style: { gap: "6px" } }, h("span", { class: "dot " + (st === "firing" ? "critical" : st === "pending" ? "pending" : st === "ok" ? "ok" : "") }), h("span", { class: "mono nowrap", title: url || "" }, name))),
        h("td", { class: "small" }, cp?.r?.role === "unknown" ? `was ${cp.r.last_role || "?"}` : cp?.r?.role || "–"),
        h("td", { class: "small nowrap" }, cause ? (st === "firing" ? h("span", { class: "al-bad" }, `down ${c.since_ms ? dur(Date.now() - c.since_ms) : ""}`) : "answers") : [fv(rule.metric, c.value), c.since_ms ? h("span", { class: "muted" }, ` · ${dur(Date.now() - c.since_ms)}`) : null]),
        h("td", {}, h("div", { class: "row nw", style: { gap: "2px", justifyContent: "flex-end" } },
          h("a", { class: "btn xs ghost", title: `${name}'s log lines`, href: link("logs", { q: node ? `server:${node}` : "", from: Math.round(since() - 5 * 60e3), to: Date.now() }) }, icon("logs")),
          url ? h("a", { class: "btn xs ghost", title: `Profile ${name}`, href: link("profile", { tab: "cpu", node: url }) }, icon("flame")) : null,
          cp ? h("a", { class: "btn xs ghost", title: `${name} in Cluster`, href: link("cluster", { shard: cp.shard, copy: name }) }, icon("box")) : null)));
    });
    const self = !rows.length;
    clear(copies, self
      ? h("div", { style: { padding: "2px 14px 12px" } }, h("div", { class: "small" }, rule.scope === "each_copy" ? "No copy reports it now." : `Evaluated on ${scopeText(rule)[0]}.`),
        h("div", { class: "row", style: { gap: "6px", marginTop: "8px" } },
          h("a", { class: "btn sm", href: link("logs", { q: "level>=warn", from: Math.round(since() - 5 * 60e3), to: Date.now() }) }, icon("logs"), "Its log lines"),
          h("a", { class: "btn sm", href: link("profile", { tab: "cpu" }) }, icon("flame"), "Profile it"),
          h("a", { class: "btn sm", href: link("queries") }, icon("queries"), "Running queries")))
      : [h("div", { class: "tbl-wrap" }, h("table", { class: "t compact" }, h("thead", {}, h("tr", {}, ["copy", "role", cause ? "state" : "value · for", ""].map((k) => h("th", {}, k)))), h("tbody", {}, lines))),
        cause ? h("div", { class: "tiny muted", style: { padding: "6px 14px 0" } }, `The rule reads ${scopeText(rule)[0]}'s own points; these are the copies of the shards that are not ok.`) : null,
        rows.length > 12 ? h("div", { class: "tiny muted", style: { padding: "4px 14px" } }, `and ${rows.length - 12} more`) : null,
        h("div", { class: "row al-mitigate" }, h("span", { class: "small muted" }, "Mitigate:"),
          [...shardsOf].slice(0, 2).map((sh) => h("a", { class: "btn sm", href: link("cluster", { shard: sh }) }, `Shard ${sh} in Cluster`)),
          h("a", { class: "btn sm", href: link("queries") }, "Running queries"))]);
  }

  async function loadLogs() {
    const at = since() - 5 * 60e3, to = Date.now();
    logsLink.href = link("logs", { q: "level>=warn", from: Math.round(at), to });
    const sql = `SELECT ts, node, level, message, fields FROM logs WHERE ts >= '${new Date(at).toISOString()}' AND level IN ('error', 'warn') ORDER BY ts DESC LIMIT 2000`;
    try {
      const j = await api("/v1/logs/query", { body: { sql } });
      const cols = j.columns || [];
      const rows = (j.rows || []).map((rw) => Object.fromEntries(cols.map((c, i) => [c, rw[i]])));
      const groups = new Map();
      for (const rw of rows) {
        const key = rw.level + " " + pattern(rw.message, rw.fields);
        const g = groups.get(key) || { level: rw.level, text: pattern(rw.message, rw.fields), n: 0, nodes: new Set(), last: 0 };
        g.n++; g.nodes.add(rw.node); g.last = Math.max(g.last, Date.parse(rw.ts) || 0);
        groups.set(key, g);
      }
      const list = [...groups.values()].sort((a, b) => b.n - a.n).slice(0, 8);
      const missed = (j.nodes || []).filter((n) => n.error);
      clear(logsBox, list.length ? table([
        { key: "n", label: "lines", num: true, render: (g) => h("b", {}, fmt.si(g.n)) },
        { key: "level", label: "level", render: (g) => h("span", { class: "al-lv al-lv-" + g.level }, g.level) },
        { key: "text", label: "message pattern", render: (g) => h("a", { class: "mono small al-pat", href: link("logs", { q: `level:${g.level} "${g.text.split(" <")[0].slice(0, 60)}"`, from: Math.round(at), to }), title: g.text }, g.text) },
        { key: "nodes", label: "where", render: (g) => { const ns = [...g.nodes].map(nodeShort).sort(); return h("span", { class: "small", title: [...g.nodes].join(", ") }, ns.slice(0, 3).join(", ") + (ns.length > 3 ? ` +${ns.length - 3}` : "")); } },
      ], { rows: list, compact: true }) : h("div", { class: "small muted", style: { padding: "4px 14px 12px" } }, "No warn or error lines since it fired."),
      h("div", { class: "tiny muted", style: { padding: "4px 14px 10px" } }, `${rows.length >= 2000 ? "Patterns of the newest 2,000 lines" : `${fmt.plural(rows.length, "line")}`} from ${fmt.plural((j.nodes || []).length - missed.length, "server")}${missed.length ? `; ${missed.length} did not answer (${missed.slice(0, 3).map((n) => n.url ? copyName(n.url) : nodeShort(n.node)).join(", ")})` : ""}.`));
    } catch (e) {
      clear(logsBox, h("div", { class: "small muted", style: { padding: "4px 14px 12px" } }, e instanceof ApiError && e.status === 403 ? "Log lines need an admin key." : "No log store on this server: its lines are not kept."));
    }
  }

  function drawActivity() {
    const evs = (one?.events || m.events.filter((e) => e.rule === id)).filter((e) => e.rule === id || !e.rule);
    const notes = (one?.notes || x.notes || []).map((n) => ({ ts: n.at_ms, what: "note", by: n.by, text: n.text }));
    const items = [...evs, ...notes];
    if (x.ack && !items.some((e) => e.what === "acked")) items.push({ ts: x.ack.at_ms, what: "acked", by: x.ack.by, text: x.ack.note });
    if (x.silenced && !items.some((e) => e.what === "silenced")) items.push({ ts: x.silenced.at_ms || x.silenced.until_ms, what: "silenced", by: x.silenced.by, text: `until ${hm(x.silenced.until_ms)}${x.silenced.reason ? `: "${x.silenced.reason}"` : ""}` });
    items.sort((a, b) => (b.ts || 0) - (a.ts || 0));
    for (let i = items.length - 1; i > 0; i--) if (items[i].what === items[i - 1].what && Math.abs((items[i].ts || 0) - (items[i - 1].ts || 0)) < 2000) { items[i - 1].text ||= items[i].text; items[i - 1].value ??= items[i].value; items[i - 1].by ||= items[i].by; items.splice(i, 1); }
    const label = { fired: "Fired", resolved: "Resolved", acked: "Acknowledged", silenced: "Silenced", note: "Note", changed: "Changed" };
    clear(activity, items.length ? h("div", { class: "timeline al-activity" }, items.slice(0, 30).map((e) => h("div", { class: "ev " + (e.what === "fired" ? "firing" : e.what === "resolved" || e.what === "acked" ? "ok" : "") },
      h("div", { class: "row nw", style: { gap: "8px" } }, h("b", {}, label[e.what] || e.what), e.by ? h("span", { class: "small dim" }, e.by) : null, h("span", { class: "sep" }), h("span", { class: "tiny muted nowrap", title: e.ts ? whenTitle(e.ts) : "" }, when(e.ts))),
      e.text ? h("div", { class: "small dim" }, e.text) : e.value != null ? h("div", { class: "small dim" }, `${rule.metric} ${fv(rule.metric, e.value)}`) : null)))
      : h("div", { class: "small muted" }, m.fleet ? "Nothing yet." : "This server keeps the last 500 changes of all rules in memory; none is this rule's."));
    notesAt = items.length;
  }
  void notesAt;

  async function post() {
    const text = noteIn.value.trim();
    if (!text) return;
    noteBtn.disabled = true;
    try { await api(`/v1/alerts/${encodeURIComponent(id)}/notes`, { body: { text } }); noteIn.value = ""; toast("Note added", "ok"); await load(); }
    catch (e) { toast(failMsg(e, "Adding a note"), "bad"); }
    noteBtn.disabled = false;
  }
  noteBtn.onclick = post;
  noteIn.onkeydown = (e) => { if (e.key === "Enter") post(); };

  hubParams = (r2) => { const w = r2.params.get("w"); if (w && w !== win) { win = w; winSeg.set(w); loadChart(); } };
  (async () => { await load(); if (!rule) return; loadChart(); loadStatements(); loadLogs(); })();
  const offs = [bus.on("alerts", () => { if (!state.paused && rule) { m = model(state.alerts); x = { ...x, ...(m.st.get(id) || {}) }; x.silent = !!(x.silenced && (!x.silenced.until_ms || x.silenced.until_ms > Date.now())); if (!rule.enabled) x.state = "disabled"; drawHead(); drawCopies(); } })];
  const stop = every(30000, async () => { if (state.paused || !rule) return; await load(); loadChart(); loadStatements(); loadLogs(); }, false);
  return () => { offs.forEach((f) => f()); stop(); hubParams = null; };
}

// ---------------------------------------------------------------- the rule editor
function editRule(rule, opts = {}) {
  const m = model(state.alerts);
  const isNew = !rule;
  const fleet = !!m?.fleet;
  const x = rule ? { ...rule } : { id: "rule-" + Math.random().toString(36).slice(2, 8), name: "", metric: "error_ratio", op: ">", threshold: 0.02, for_s: 0, severity: "warning", enabled: true, description: "", eval: "mean", window_s: 300, scope: "self" };
  if (!fleet && !rule) { x.eval = undefined; x.window_s = undefined; x.for_s = 60; }
  const saved = rule ? { ...rule } : null;
  const adm = can("admin");
  const locked = !!x.builtin; // built-in rules change only in severity, on/off and route
  const dis = (k) => !adm || (locked && !["severity", "enabled", "route"].includes(k)) || (!fleet && ["eval", "window_s", "resolve", "scope", "owner", "runbook", "route"].includes(k));
  const metrics = [...new Set([...Object.keys(METRICS), ...((state.points || []).slice(-3).flatMap((p) => Object.keys(p).filter((k) => typeof p[k] === "number" && k !== "t" && k !== "secs"))), x.metric])].sort();
  const name = h("input", { class: "input", value: x.name, placeholder: "Query errors over 2%", disabled: dis("name") });
  const metric = h("select", { class: "select", disabled: dis("metric"), "aria-label": "Metric" }, metrics.map((k) => h("option", { value: k, selected: k === x.metric }, METRICS[k] ? `${k} · ${METRICS[k][0]}${METRICS[k][2] === "copy" ? " (copies only)" : ""}` : k)));
  const scopeSeg = seg([["self", coordinator() ? "this coordinator" : "this server"], ["each_copy", `each copy (${fmt.n(allCopies())})`]], x.scope || "self", (v) => { x.scope = v; run(); }, { "aria-label": "Evaluated on" });
  const evalSeg = seg(Object.entries(EVAL).map(([k, l]) => [k, l]), x.eval || "all", (v) => { x.eval = v; run(); }, { "aria-label": "Value" });
  const win = h("input", { class: "input", type: "number", min: 1, step: 1, value: Math.round((x.window_s || 300) / 60), style: { width: "90px" }, disabled: dis("window_s"), "aria-label": "Window in minutes" });
  const opSeg = seg(OPS.map((o) => [o, o]), x.op, (v) => { x.op = v; run(); }, { "aria-label": "Fires when the value is" });
  const unit = () => unitOf(x.metric) === "pct" ? "%" : unitOf(x.metric) === "ms" ? "ms" : unitOf(x.metric) === "/s" ? "/s" : "";
  const toInput = (v) => v == null || v === "" ? "" : unitOf(x.metric) === "pct" ? +(v * 100).toPrecision(6) : v;
  const fromInput = (v) => v === "" ? null : unitOf(x.metric) === "pct" ? Number(v) / 100 : Number(v);
  const thr = h("input", { class: "input", type: "number", step: "any", value: toInput(x.threshold), style: { width: "120px" }, disabled: dis("threshold"), "aria-label": "Threshold" });
  const res = h("input", { class: "input", type: "number", step: "any", value: toInput(x.resolve), placeholder: "same", style: { width: "110px" }, disabled: dis("resolve"), "aria-label": "Resolves at" });
  const forS = h("input", { class: "input", type: "number", min: 0, value: x.for_s || 0, style: { width: "90px" }, disabled: dis("for_s"), "aria-label": "Held for, seconds" });
  const sevSeg = seg(SEV.map((v) => [v, v]), x.severity, (v) => { x.severity = v; }, { "aria-label": "Severity" });
  const owner = h("input", { class: "input", value: x.owner || "", placeholder: "search-platform on-call", disabled: dis("owner") });
  const runbook = h("input", { class: "input", value: x.runbook || "", placeholder: "https://wiki.example.com/runbooks/errors", disabled: dis("runbook") });
  const routeSel = h("select", { class: "select", disabled: dis("route"), "aria-label": "Route" }, h("option", { value: "" }, "default route"), (m?.routes || []).filter((rt) => !rt.legacy).map((rt) => h("option", { value: rt.name, selected: rt.name === x.route }, `${rt.name}${rt.severities?.length ? ` (${rt.severities.join(", ")})` : ""}`)));
  const desc = h("textarea", { class: "input", rows: 2, style: { fontFamily: "var(--sans)" }, disabled: dis("description") }); desc.value = x.description || "";
  const why = h("input", { class: "input sm", placeholder: "Why (kept in the audit log)", style: { width: "260px" } });
  const unitEls = [h("span", { class: "small muted" }), h("span", { class: "small muted" })];
  // The on/off switch, visible at the top (A8).
  const sw = onSwitch(x, !adm, (on) => { x.enabled = on; });
  for (const seg0 of [scopeSeg, evalSeg, opSeg]) for (const b of seg0.children) b.disabled = dis(seg0 === scopeSeg ? "scope" : seg0 === evalSeg ? "eval" : "op");
  for (const b of sevSeg.children) b.disabled = !adm;
  // The backtest.
  let span = fleet ? "7d" : "1h";
  const spanSeg = seg([["1h", "1 h"], ["24h", "24 h"], ["7d", "7 days"]], span, (v) => { span = v; run(); }, { "aria-label": "Backtest over" });
  const btChart = alertChart(220);
  const verdict = h("div", { class: "stack al-verdict", style: { gap: "4px" } });
  const btTitle = h("h3");
  const note = (t) => h("div", { class: "row nw small muted", style: { gap: "6px" } }, icon("info"), h("span", {}, t));
  const field = (label, el, hint) => h("label", { class: "f" }, label, el, hint ? h("span", { class: "tiny muted" }, hint) : null);

  let btGen = 0;
  async function backtest(r, from, to) {
    try {
      const j = await api("/v1/alerts/backtest", { body: { rule: cleanRule(r), from: Math.round(from), to: Math.round(to) } });
      return { pts: (j.points || []).map((p) => [p.t, p.value]), spans: (j.fired || []).map((f) => [f.from_ms, f.to_ms]), source: j.source, server: true, note: j.note || j.error };
    } catch (e) {
      if (!missing(e) && !(e instanceof ApiError && e.status === 400)) throw e;
      const raw = await seriesOf(r.metric, from, to);
      const ev = evaluate(raw.pts, r);
      return { pts: ev, spans: firings(ev, r), source: raw.source, server: false, has: raw.has };
    }
  }
  const run = debounce(async () => {
    const my = ++btGen;
    read();
    clear(unitEls[0], unit()); clear(unitEls[1], unit());
    const now = Date.now(), from = now - (span === "7d" ? 7 * 86400e3 : span === "24h" ? 86400e3 : 3600e3);
    const ev = x.eval || "all";
    btTitle.textContent = `${ev === "all" ? "Each point" : `The ${dur((x.window_s || 300) * 1000)} ${ev === "mean" ? "mean" : ev}`} of ${x.metric} over the last ${span === "7d" ? "7 days" : span === "24h" ? "24 hours" : "hour"}`;
    clear(verdict, h("div", { class: "small muted" }, "Backtesting…"));
    try {
      const changedRule = saved && JSON.stringify(cleanRule(saved)) !== JSON.stringify(cleanRule(x));
      const [a, b] = await Promise.all([backtest(x, from, now), changedRule ? backtest(saved, from, now).catch(() => null) : null]);
      if (my !== btGen) return;
      btChart.update({ t0: from, t1: now, series: [{ name: ev === "all" ? x.metric : `${EVAL[ev]} ${dur((x.window_s || 300) * 1000)}`, cls: "eval", pts: a.pts }], thr: Number(x.threshold), resolve: x.resolve != null ? Number(x.resolve) : null, spans: a.spans, markers: [], fmtv: (v) => fv(x.metric, v) });
      const lens = a.spans.map(([s0, s1]) => (s1 ?? now) - s0);
      const flaps = lens.filter((v) => v < 120e3).length;
      const last = [...a.pts].reverse().find((p) => p[1] != null);
      const label = span === "7d" ? "7 days" : span === "24h" ? "24 hours" : "hour";
      const out = [];
      if (!a.pts.some((p) => p[1] != null)) out.push(h("div", { class: "row nw", style: { gap: "8px" } }, icon("info"), h("span", {}, `No values of ${x.metric} over the last ${label}${METRICS[x.metric]?.[2] === "copy" && x.scope !== "each_copy" ? ": this metric lives on the copies; evaluate it on each copy" : ""}.`)));
      else out.push(h("div", { class: "row nw", style: { gap: "8px" } }, icon(a.spans.length ? "warn" : "okc"), h("span", {}, `Over the last ${label} this rule would have fired `, h("b", {}, fmt.plural(a.spans.length, "time")), a.spans.length ? ` (median ${dur(median(lens))} firing, ${fmt.plural(flaps, "flap")} under 2 min)` : "", `. Now: ${last ? fv(x.metric, last[1]) : "no value"}.`)));
      if (b) out.push(note(`The rule as saved (${cond(saved, true)}) would have fired ${fmt.plural(b.spans.length, "time")}.`));
      out.push(note(a.server ? `Evaluated by the server over ${a.source === "load" ? "the per-minute load table (only the metrics it holds)" : "its memory"}.` : `This server cannot backtest: evaluated here over ${a.source === "load" ? "its per-minute points" : "the points in this browser's memory (up to an hour)"}.`));
      if (a.note) out.push(note(a.note));
      clear(verdict, out);
    } catch (e) { if (my === btGen) clear(verdict, h("div", { class: "error" }, e.message)); }
  }, 250);
  function read() {
    x.name = name.value.trim();
    x.metric = metric.value;
    x.threshold = fromInput(thr.value) ?? x.threshold;
    const rv = fromInput(res.value);
    x.resolve = rv == null ? undefined : rv;
    x.for_s = Number(forS.value) || 0;
    x.window_s = Math.max(1, Number(win.value) || 5) * 60;
    x.owner = owner.value.trim() || undefined;
    x.runbook = runbook.value.trim() || undefined;
    x.route = routeSel.value || undefined;
    x.description = desc.value.trim();
    if (!fleet) { delete x.eval; delete x.window_s; delete x.resolve; delete x.scope; }
  }
  for (const el of [thr, res, forS, win]) el.oninput = run;
  metric.onchange = () => { const before = unitOf(x.metric); x.metric = metric.value; if (unitOf(x.metric) !== before) { thr.value = toInput(x.threshold); res.value = toInput(x.resolve); } run(); };

  const saveBtn = h("button", { class: "btn primary", disabled: !adm, title: adm ? "" : Needs("admin") }, icon("check"), isNew ? "Create rule" : "Save");
  const more = h("button", { class: "btn ghost", title: "More", "aria-label": "More actions" }, icon("dots"));
  const d = drawer({
    title: isNew ? "New alert rule" : x.name,
    sub: isNew ? "fires when the value crosses the threshold, and resolves when it comes back past the resolve line" : `${x.id} · ${m?.st.get(x.id)?.state?.replace("_", " ") || ""}${x.owner ? ` · owned by ${x.owner}` : ""}`,
    wide: true,
    body: [
      h("div", { class: "row between al-ed-top" }, sw, locked ? h("span", { class: "small muted" }, "Built in: only severity, on/off and route change.") : null, !fleet ? h("span", { class: "small muted" }, "This server checks each point: window, resolve, scope, owner, runbook and route need a newer strata-server.") : null),
      h("div", { class: "grid g2" }, field("Name", name), field("Metric", metric, METRICS[x.metric] ? `unit: ${unitOf(x.metric) === "pct" ? "%" : unitOf(x.metric)}` : null)),
      h("div", { class: "row al-ed-row" }, field("Evaluated on", scopeSeg), field("Value", evalSeg), field("Window, minutes", win)),
      h("div", { class: "row al-ed-row" }, field("Fires when the value is", opSeg), h("label", { class: "f" }, "Threshold", h("span", { class: "row nw", style: { gap: "4px" } }, thr, unitEls[0])), field("Held for, seconds", forS), field("Severity", sevSeg), h("label", { class: "f" }, "Resolves at", h("span", { class: "row nw", style: { gap: "4px" } }, res, unitEls[1]))),
      h("section", { class: "card", style: { marginTop: "4px" } }, h("div", { class: "hd" }, btTitle, h("span", { class: "sub" }, "shaded: when it would have fired"), h("div", { class: "tools" }, spanSeg)), h("div", { class: "bd" }, btChart, verdict)),
      h("div", { class: "grid g2" }, field("Owner", owner), field("Runbook", runbook)),
      h("div", { class: "grid g2" }, field("Route", routeSel, "who is told: routes are set in Routes"), field("Description (sent with the message)", desc)),
    ],
    foot: [more, h("span", { class: "sep" }), adm ? why : null, h("button", { class: "btn", onclick: () => d.close() }, "Cancel"), saveBtn],
  });
  run();
  more.onclick = () => menu(more, [
    { label: "Copy as JSON", icon: "copy", onclick: () => { read(); copy(JSON.stringify(cleanRule(x), null, 2), "Rule copied as JSON"); } },
    isNew ? null : { label: "Open its alert page", icon: "alerts", onclick: () => { d.close(); location.hash = alertLink(x.id); } },
    !isNew && !locked && adm ? "-" : null,
    !isNew && !locked && adm ? { label: "Delete this rule", icon: "trash", danger: true, onclick: del } : null,
  ].filter(Boolean));
  async function del() {
    const st = m?.st.get(x.id)?.state;
    const ok = await ask({ title: `Delete "${x.name}"?`, body: `The server stops checking it${st === "firing" ? " and sends a RESOLVED message (it is firing now)" : ""}. Its past events stay. To stop it for a while, turn it off or silence it instead.`, ok: "Delete", danger: true, typed: x.id });
    if (!ok) return;
    try { await saveRules((model(state.alerts)?.rules || []).filter((y) => y.id !== x.id), ok.reason); toast("Rule deleted", "ok"); d.close(); opts.after?.(); } catch { /* told */ }
  }
  saveBtn.onclick = async () => {
    read();
    if (!x.name) x.name = `${x.metric} ${x.op} ${fv(x.metric, x.threshold)}`;
    const cur = model(state.alerts)?.rules || [];
    const next = isNew ? [...cur, x] : cur.map((y) => y.id === x.id ? x : y);
    try { await saveRules(next, why.value.trim()); toast(isNew ? "Rule created" : "Rule saved", "ok"); d.close(); opts.after?.(); } catch { /* told */ }
  };
}

// ---------------------------------------------------------------- routes
function routesDrawer() {
  const m = model(state.alerts);
  const body = h("div", { class: "stack" });
  const d = drawer({ title: "Routes", sub: "where alert messages go, by severity; each with its delivery status", wide: true, body });
  if (!m?.fleet) {
    clear(body, h("div", { class: "notice" }, m?.routes.length ? "This server posts every alert that fires or resolves to one webhook (--alert-webhook). It does not track deliveries, route by severity or send a test: a newer strata-server does." : "No webhook: this server tells nobody when an alert fires. Start it with --alert-webhook URL, or run a newer strata-server to set routes here."),
      h("p", { class: "small muted" }, "A message reads: [strata FIRING critical] <rule> on <cluster>/<where>: <metric> = <value> (<op> <threshold>, <eval> over <window>). <description> Runbook: <url>, then a link to the alert's page."));
    return d;
  }
  let routes = m.routes.map((r) => ({ ...r, severities: r.severities || [] }));
  let editing = false;
  const draw = () => {
    const adm = can("admin");
    const rows = routes.map((r, i) => editing
      ? h("tr", {},
        h("td", {}, h("input", { class: "input sm", value: r.name, style: { width: "120px" }, oninput: (e) => { r.name = e.target.value.trim(); } })),
        h("td", {}, h("input", { class: "input sm mono", value: r.url || "", placeholder: "https://hooks.slack.com/…", style: { width: "100%" }, oninput: (e) => { r.url = e.target.value.trim(); } })),
        h("td", {}, h("div", { class: "row nw", style: { gap: "4px" } }, SEV.map((sv) => h("button", { class: "chip", "aria-pressed": String(!r.severities.length || r.severities.includes(sv)), onclick: (e) => { const on = e.currentTarget.getAttribute("aria-pressed") === "true"; const cur = r.severities.length ? r.severities : SEV.slice(); r.severities = on ? cur.filter((y) => y !== sv) : [...cur, sv]; if (r.severities.length === SEV.length) r.severities = []; draw(); } }, sv)))),
        h("td", { colspan: 2 }), h("td", {}, h("button", { class: "btn xs ghost", title: "Remove this route", onclick: () => { routes.splice(i, 1); draw(); } }, icon("x"))))
      : h("tr", {},
        h("td", { class: "al-ro-name" }, h("b", {}, r.name)),
        h("td", { class: "mono small ellipsis al-ro-url", style: { maxWidth: "280px" }, title: r.url || "" }, r.url ? r.url.replace(/^https?:\/\//, "").slice(0, 48) : "–"),
        h("td", { class: "small al-ro-sev" }, r.severities?.length ? r.severities.join(", ") : "every severity"),
        h("td", { class: "small al-ro-sent" }, `${fmt.n(r.sent || 0)} sent`, r.failed ? h("span", { class: "al-bad" }, ` · ${fmt.n(r.failed)} failed`) : null),
        h("td", { class: "small al-ro-st" }, r.last_error && (!r.last_ok_ms || (r.last_error_ms || 0) > r.last_ok_ms) ? h("span", { class: "al-bad", title: r.last_error }, `failing: ${r.last_error.slice(0, 60)}`) : r.last_ok_ms ? h("span", { class: "al-good" }, `delivered ${ago(r.last_ok_ms)}`) : h("span", { class: "muted" }, "nothing sent yet")),
        h("td", { class: "al-ro-test" }, h("button", { class: "btn xs", disabled: !can("write"), title: can("write") ? "Send a test message through this route now" : Needs("write"), onclick: async (e) => {
          const b = e.currentTarget; b.disabled = true;
          try { const j = await api(`/v1/alerts/routes/${encodeURIComponent(r.name)}/test`, { body: {} }); toast(j.ok === false ? `Test failed: ${j.error || j.status}` : `Test sent through ${r.name}${j.status ? ` (${j.status})` : ""}`, j.ok === false ? "bad" : "ok"); await refreshAlerts(); routes = model(state.alerts).routes.map((y) => ({ ...y, severities: y.severities || [] })); draw(); }
          catch (err) { toast(failMsg(err, "Testing the route"), "bad"); }
          b.disabled = false;
        } }, "Test"))));
    clear(body,
      h("div", { class: "tbl-wrap al-rotbl" }, h("table", { class: "t compact" }, h("thead", {}, h("tr", {}, ["name", "URL", "severities", "sent", "status", ""].map((k) => h("th", {}, k)))), h("tbody", {}, rows.length ? rows : h("tr", {}, h("td", { colspan: 6 }, empty("route", "No routes", "Nobody is told when an alert fires.")))))),
      h("div", { class: "row" },
        editing ? h("button", { class: "btn sm", onclick: () => { routes.push({ name: `route-${routes.length + 1}`, url: "", severities: [] }); draw(); } }, icon("plus"), "Add a route") : null,
        h("span", { class: "sep" }),
        editing ? h("button", { class: "btn sm", onclick: () => { editing = false; routes = model(state.alerts).routes.map((y) => ({ ...y, severities: y.severities || [] })); draw(); } }, "Cancel") : null,
        editing ? h("button", { class: "btn sm primary", onclick: save }, icon("check"), "Save routes")
          : h("button", { class: "btn sm", disabled: !adm, title: adm ? "" : Needs("admin"), onclick: () => { editing = true; draw(); } }, icon("edit"), "Edit routes")),
      h("p", { class: "small muted" }, "A rule names its route (or uses \"default\"). A route takes the severities ticked; a message reads: [strata FIRING critical] <rule> on <cluster>/<where>: <metric> = <value> (<op> <threshold>, <eval> over <window>). <description> Runbook: <url>, and a link to the alert's page. A RESOLVED message follows when it resolves, also when its rule is edited or deleted."));
  };
  async function save() {
    if (routes.some((r) => !r.name || !/^https?:\/\//.test(r.url || ""))) { toast("Each route needs a name and an http(s) URL", "bad"); return; }
    const ok = await ask({ title: "Save the routes?", body: `${fmt.plural(routes.length, "route")}. Messages go through them from now on.`, ok: "Save" });
    if (!ok) return;
    try { await api("/v1/alerts/routes", { method: "PUT", body: { routes: routes.map((r) => ({ name: r.name, url: r.url, severities: r.severities })) }, reason: ok.reason }); toast("Routes saved", "ok"); editing = false; await refreshAlerts(); routes = model(state.alerts).routes.map((y) => ({ ...y, severities: y.severities || [] })); draw(); }
    catch (e) { toast(failMsg(e, "Saving the routes"), "bad"); }
  }
  draw();
  return d;
}

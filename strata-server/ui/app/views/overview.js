// Home: is the fleet well, and if not what, where, since when, and what to
// do next. A banner from the fleet's health (never green while a headline
// card is red), the headline numbers each naming the worst copy, a Where
// strip with a cell per shard (or per copy, by node or zone), the problems
// ranked with their next step, error ratio and p99 with the changes of the
// window, capacity, the coordinators side by side, and who is affected.

import { h, icon, clear, fmt, card, every, api, empty, bus, seg, store, tipOn, drawer, link, when, whenTitle, span, copyName, copyTitle, copy, timeEl } from "../core.js";
import { timeChart, sparkline, SERIES } from "../charts.js";
import { state, visible, domain, setZoom, isCoordinator, timeRange, fresh, staleText, firingAlerts, clusterName, environment, rangeParams, shardState } from "../data.js";

export default { id: "home", title: "Home", icon: "overview", group: "Operate", keywords: "overview home fleet health status incident banner problems slo where capacity coordinators affected dashboard morning check", render, badge };

/** Problems on Home, for the navigation. */
function badge() {
  const f = state.fleet; if (!f) return null;
  const n = (f.problems || []).length;
  return n ? { n, tone: f.status === "critical" ? "crit" : "warn", title: `${fmt.plural(n, "problem")}, worst first on Home` } : null;
}

const SLO_ERR = 0.01, SLO_P99 = 250;
const check = (id) => (state.fleet?.checks || []).find((c) => c.id === id) || null;
const sloErr = () => check("error_ratio")?.slo ?? SLO_ERR;
const sloP99 = () => check("p99")?.slo_ms ?? SLO_P99;
const median = (xs) => { const v = xs.filter((x) => x != null && Number.isFinite(x)).sort((a, b) => a - b); return v.length ? v[Math.floor(v.length / 2)] : null; };
const pctTxt = (v) => v == null ? "–" : v < 0.001 && v > 0 ? "<0.1%" : fmt.pct(v, v < 0.1 ? 2 : 1);
const durTxt = (ms) => ms == null ? "" : fmt.dur(Math.max(0, (Date.now() - ms) / 1000)).replace(/ 0s$/, "");

// ---------------------------------------------------------------- the fleet, copy by copy
/** Every shard with its copies' numbers, from /v1/cluster. */
function model() {
  const c = state.cluster;
  const shards = [];
  const nodes = new Map(), zones = new Map();
  let total = 0, up = 0;
  for (const [i, sh] of (c?.shards || []).entries()) {
    const n = sh.shard ?? i;
    const reps = sh.replicas || [];
    const prim = reps.find((r) => r.role === "primary" && r.status === "ok") || reps.find((r) => r.url === sh.primary) || reps.find((r) => r.role === "primary");
    const behind = {};
    for (const r of reps) for (const x of r.detail?.health?.replication?.replicas || []) if (x.url) behind[x.url] = Math.max(behind[x.url] || 0, x.behind || 0);
    const copies = reps.map((r, j) => {
      const L = r.detail?.load || {};
      const sys = r.detail?.system || {};
      const ok = r.status === "ok" && r.role !== "unknown";
      const name = copyName(r.url, c);
      const x = {
        url: r.url, name, j, shard: n, up: ok, role: ok ? r.role : "down", was: r.last_role || r.role, primary: r === prim,
        node: r.node || null, zone: r.zone || null, host: hostOf(r.url), version: r.version?.version || null,
        p99: L.p99_ms ?? null, qps: L.qps ?? null, errs: L.errors_s ?? null, cpu: L.cpu != null && sys.cores ? L.cpu / sys.cores : null,
        mem: L.rss_bytes != null && sys.memory_limit_bytes ? L.rss_bytes / sys.memory_limit_bytes : L.query_memory_ratio ?? null,
        disk: L.disk_used_ratio ?? null, slots: L.slots_free != null && (sys.limits?.max_concurrent || state.system?.limits?.max_concurrent) ? Math.max(0, 1 - L.slots_free / (sys.limits?.max_concurrent || state.system.limits.max_concurrent)) : null,
        lag: behind[r.url] ?? null, alerts: r.detail?.alerts?.firing || [], down_since: r.down_since_ms || null, avoided: !!r.avoided, why: r.avoided_why || null,
      };
      x.err = x.qps ? (x.errs || 0) / x.qps : null;
      total++; if (ok) up++;
      const nk = x.node || null;
      if (nk) { if (!nodes.has(nk)) nodes.set(nk, []); nodes.get(nk).push(x); }
      if (x.zone) { if (!zones.has(x.zone)) zones.set(x.zone, []); zones.get(x.zone).push(x); }
      return x;
    });
    const st = shardState(sh);
    shards.push({ n, sh, st, why: sh.why || null, copies, primary: prim?.url || null });
  }
  return { shards, nodes, zones, total, up, at: c?.at_ms || null };
}
const hostOf = (u) => { try { return new URL(u).host; } catch { return String(u || ""); } };

/** A copy's tone: crit (down, or hurting queries now), warn (needs a look), or "". */
function copyTone(x) {
  if (!x.up) return "crit";
  if ((x.err != null && x.err > sloErr() && x.qps > 1) || (x.p99 != null && x.p99 > sloP99())) return "crit";
  if ((x.lag != null && x.lag > 1000) || (x.disk != null && x.disk > 0.85) || x.alerts.length || x.avoided) return "warn";
  return "";
}
/** A shard's cell under a lens: { tone, tag, value }. */
function shardCell(s, lens) {
  const worst = (f) => { let v = null; for (const x of s.copies) { const y = f(x); if (y != null && (v == null || y > v)) v = y; } return v; };
  if (lens === "p99") { const v = worst((x) => x.p99); return { tone: v == null ? "" : v > sloP99() ? "crit" : v > sloP99() * 0.6 ? "warn" : "", tag: v != null && v > sloP99() * 0.6 ? fmt.ms(v).replace(" ", "") : null, value: v != null ? `worst p99 ${fmt.ms(v)}` : "p99 not known" }; }
  if (lens === "errors") { const v = worst((x) => x.err); return { tone: v == null ? "" : v > sloErr() ? "crit" : v > sloErr() / 4 ? "warn" : "", tag: v != null && v > sloErr() / 4 ? pctTxt(v) : null, value: v != null ? `${pctTxt(v)} of its queries fail` : "errors not known" }; }
  if (lens === "disk") { const v = worst((x) => x.disk); return { tone: v == null ? "" : v > 0.9 ? "crit" : v > 0.8 ? "warn" : "", tag: v != null && v > 0.8 ? fmt.pct(v, 0) : null, value: v != null ? `fullest disk ${fmt.pct(v, 0)}` : "disk not known" }; }
  if (lens === "lag") { const v = worst((x) => x.lag); return { tone: v == null ? "" : v > 10000 ? "crit" : v > 1000 ? "warn" : "", tag: v != null && v > 1000 ? fmt.si(v) : null, value: v != null ? `a copy ${fmt.si(v)} transactions behind` : "lag not known" }; }
  // Health: what serves, then the worst copy.
  if (["no_reads", "no_writes", "down"].includes(s.st)) return { tone: "crit", tag: s.st === "down" ? "dark" : s.st === "no_reads" ? "no reads" : "no writes", value: s.why || stText(s.st) };
  const tones = s.copies.map(copyTone);
  const down = s.copies.filter((x) => !x.up).length;
  if (tones.includes("crit") && !down) return { tone: "crit", tag: "hot", value: "a copy hurts queries now" };
  if (down) return { tone: "warn", tag: "down", value: `${down} of ${s.copies.length} copies do not answer; the others serve` };
  if (tones.includes("warn")) { const x = s.copies.find((c) => copyTone(c) === "warn"); return { tone: "warn", tag: x.lag > 1000 ? "lag" : x.disk > 0.85 ? "disk" : x.alerts.length ? "alert" : "look", value: x.lag > 1000 ? `${x.name} is ${fmt.si(x.lag)} transactions behind` : x.disk > 0.85 ? `${x.name}'s disk is ${fmt.pct(x.disk, 0)} full` : x.alerts.length ? `${x.name}: ${x.alerts[0].name}` : `${x.name} is avoided after failures` }; }
  return { tone: "", tag: null, value: "fine" };
}
const stText = (st) => ({ ok: "serves reads and writes", degraded: "a copy is down; it still serves", no_reads: "serves no reads", no_writes: "takes no writes", down: "does not answer" })[st] || st;

// ---------------------------------------------------------------- the view
function render(page) {
  const head = h("div", { class: "page-head" });
  const banner = h("div", { class: "banner home-banner" });
  const kpiRow = h("div", { class: "grid g6 home-kpis" });
  page.append(head, banner, kpiRow);

  // Where, and the problems.
  let lens = store.get("home.lens", "health"), group = store.get("home.group", "shard");
  const whereBody = h("div", { class: "home-where" });
  const whereSub = h("span", { class: "sub" });
  const lensSeg = seg([["health", "Health"], ["p99", "p99"], ["errors", "Errors"], ["disk", "Disk"], ["lag", "Lag"]], lens, (v) => { lens = v; store.set("home.lens", v); drawWhere(); }, { "aria-label": "Colour by" });
  const groupSeg = seg([["shard", "Shard"], ["node", "Node"], ["zone", "Zone"]], group, (v) => { group = v; store.set("home.group", v); drawWhere(); }, { "aria-label": "Group by" });
  const whereCard = h("section", { class: "card span2 home-wherecard" },
    h("div", { class: "hd" }, h("h3", {}, "Where"), whereSub, h("div", { class: "tools" }, lensSeg, groupSeg, h("a", { class: "btn ghost sm", href: link("cluster") }, "Cluster", icon("right")))),
    h("div", { class: "bd" }, whereBody));
  const probBody = h("div", { class: "stack home-probs" });
  const probTitle = h("h3", {}, "Problems");
  const probSub = h("span", { class: "sub" });
  const probCard = h("section", { class: "card home-probcard" }, h("div", { class: "hd" }, probTitle, probSub), h("div", { class: "bd" }, probBody));
  page.append(h("div", { class: "grid g3 home-row" }, whereCard, probCard));

  // Since when, and what changed.
  const brush = (a, b) => setZoom(a == null ? null : [a, b]);
  const markers = () => events.filter((e) => e.severity !== "info" || MARK.has(e.kind)).map((e) => ({ t: e.ts_ms, label: `${when(e.ts_ms)} ${e.title}` }));
  const errChart = timeChart({ height: 150, label: "error ratio", fmt: (v) => fmt.pct(v, v < 0.01 ? 2 : 1), series: [{ name: "error ratio", get: (p) => p.error_ratio ?? (p.qps ? (p.errors_s || 0) / p.qps : null), color: SERIES[7], area: true }], domain, onBrush: brush, markers, threshold: { value: SLO_ERR, label: "SLO" }, legend: false });
  const p99Chart = timeChart({ height: 150, label: "p99 latency", fmt: (v) => fmt.ms(v), series: [{ name: "p99", get: (p) => p.p99_ms, color: SERIES[6] }, { name: "p50", get: (p) => p.p50_ms, color: SERIES[0], dash: "3 3" }], domain, onBrush: brush, markers, threshold: { value: SLO_P99, label: "SLO" }, legend: false });
  const lane = h("div", { class: "home-lane" });
  const evList = h("div", { class: "home-evlist" });
  const chartsSub = h("span", { class: "sub" });
  const zoomLinks = h("div", { class: "tools" });
  const sinceTitle = h("h3", {}, "Since when, and what changed");
  const chartsCard = h("section", { class: "card home-since" },
    h("div", { class: "hd" }, sinceTitle, chartsSub, zoomLinks),
    h("div", { class: "bd" }, h("div", { class: "home-since-grid" },
      h("div", { class: "home-charts" }, h("div", { class: "small muted home-clabel" }, "error ratio"), errChart, h("div", { class: "small muted home-clabel" }, "p99 latency (dashed: p50)"), p99Chart, lane),
      h("div", { class: "home-evside" }, h("div", { class: "row nw home-evhead" }, h("b", {}, "What happened"), h("span", { class: "small muted" }, "newest first"), h("span", { class: "sep" }), h("a", { class: "small", href: link("timeline") }, "Timeline ", icon("right"))), evList))));
  page.append(chartsCard);

  // Capacity, coordinators, who is affected.
  const capBody = h("div", { class: "stack home-cap" });
  const coordBody = h("div");
  const whoBody = h("div");
  const whoSub = h("span", { class: "sub" }, "failing queries by table and by API key, this window");
  page.append(h("div", { class: "grid g3 home-row" },
    h("section", { class: "card home-capcard" }, h("div", { class: "hd" }, h("h3", {}, "Capacity"), h("span", { class: "sub" }, "copies now: worst and median"), h("div", { class: "tools" }, h("a", { class: "btn ghost sm", href: link("cluster", { tab: "capacity", range: "7d" }) }, "Trend 7 d", icon("right")))), h("div", { class: "bd" }, capBody)),
    h("section", { class: "card home-coordcard" }, h("div", { class: "hd" }, h("h3", {}, "Coordinators"), h("span", { class: "sub" }, "each one's own numbers")), h("div", { class: "bd flush" }, coordBody)),
    h("section", { class: "card home-whocard" }, h("div", { class: "hd" }, h("h3", {}, "Who is affected"), whoSub, h("div", { class: "tools" }, h("a", { class: "btn ghost sm", href: link("queries", { tab: "statements" }) }, "Queries", icon("right")))), h("div", { class: "bd" }, whoBody))));

  // ------------------------------------------------------------ data the view reads itself
  let events = [], eventsFrom = "events", stats = null, statsKeys = false;
  const MARK = new Set(["copy_down", "copy_up", "primary_changed", "majority_lost", "majority_back", "rollout", "upgrade", "change", "alert_fired", "note", "moves_started", "contain"]);
  async function loadEvents() {
    const r = timeRange();
    try {
      const j = await api(`/v1/events?from=${Math.round(r.from)}&to=${Math.round(r.to)}&limit=500`, { timeout: 10000 });
      events = (j.events || []).slice().sort((a, b) => b.ts_ms - a.ts_ms);
      eventsFrom = "events";
    } catch (e) {
      if (e.status !== 404 && e.status !== 405) return;
      // An older server: the alerts' own changes are what it remembers.
      eventsFrom = "alerts";
      events = (state.alerts?.events || []).filter((x) => x.ts_ms >= r.from && x.ts_ms <= r.to).map((x) => ({ id: `${x.rule || x.name}-${x.ts_ms}`, ts_ms: x.ts_ms, kind: x.state === "firing" ? "alert_fired" : x.state === "ok" ? "alert_resolved" : "alert_" + x.state, severity: x.state === "firing" ? (x.severity || "warning") : "info", title: `${x.name || x.rule}: ${x.state}`, where: { rule: x.rule } }));
    }
    drawEvents(); drawCharts();
  }
  async function loadStats() {
    const r = timeRange();
    try {
      const j = await api(`/v1/queries/stats?from=${Math.round(r.from)}&to=${Math.round(r.to)}&limit=500`, { timeout: 10000 });
      stats = Array.isArray(j) ? j : j.statements || [];
      statsKeys = stats.some((x) => Array.isArray(x.keys));
    } catch (e) { stats = e.status === 403 ? "forbidden" : null; }
    drawWho();
  }

  // ------------------------------------------------------------ head and banner
  function drawHead() {
    const m = model();
    const r = timeRange();
    const name = clusterName();
    const coords = state.fleet?.coordinators?.length || 0;
    const reps = m.shards.length ? Math.round(m.total / m.shards.length) : 0;
    const parts = [
      name || location.host,
      m.shards.length ? `${fmt.plural(m.shards.length, "shard")} × ${reps} ${reps === 1 ? "copy" : "copies"}${m.nodes.size ? ` on ${fmt.plural(m.nodes.size, "node")}` : ""}${m.zones.size ? ` in ${fmt.plural(m.zones.size, "zone")}` : ""}` : state.health ? (isCoordinator() ? "no shards listed" : "one server, not a cluster") : null,
      coords > 1 ? fmt.plural(coords, "coordinator") : null,
      `numbers over ${r.fixed ? span(r.from, r.to) : r.label.replace("last ", "the last ")} unless marked`,
    ].filter(Boolean);
    const seen = m.total ? `${m.up} of ${m.total} copies answered at the last look${m.at ? ", " + fmt.ago(m.at) : ""}` : "";
    const fr = fresh("cluster");
    clear(head, h("div", {}, h("h1", {}, "Fleet health"), h("p", {}, parts.join(" · "))),
      h("div", { class: "tools" }, !fr.ok && !fr.loading ? h("span", { class: "stale-age" }, icon("clock"), staleText("cluster")) : null, seen ? h("span", { class: "small muted" }, seen) : null));
  }

  /** The tone of each headline card, so the banner is never greener than they are. */
  function cardTones() { return kpiDefs().map((d) => d.tone).filter(Boolean); }

  function drawBanner() {
    const f = state.fleet;
    const answering = !document.body.classList.contains("not-answering");
    if (!f) { banner.className = "banner home-banner"; clear(banner, h("div", { class: "bic" }, h("span", { class: "spin" })), h("div", { class: "grow" }, h("div", { class: "big" }, "Checking the fleet…"))); return; }
    const tones = cardTones();
    let kind = f.status === "critical" ? "bad" : f.status === "warning" ? "warn" : "ok";
    if (tones.includes("bad")) kind = "bad"; else if (kind === "ok" && tones.includes("warn")) kind = "warn";
    if (!answering) kind = "unknown";
    banner.className = "banner home-banner " + kind;
    const probs = f.problems || [];
    const top = probs[0];
    let headline = f.headline || (top ? top.title : "Every check is within its limit");
    // The fleet said ok, a card says otherwise: say what the card says.
    if (f.status === "ok" && kind !== "ok" && kind !== "unknown") headline = kpiDefs().filter((d) => d.tone === kind).map((d) => d.why).filter(Boolean)[0] || "A headline number is over its limit";
    if (!answering) headline = "Not known: the server does not answer";
    const since = f.since_ms || top?.since_ms || null;
    const sub = [];
    if (!answering) sub.push(`What shows is from ${when(f.at_ms)}. The last answer said: ${f.headline || "ok"}.`);
    else if (kind === "ok") {
      const m = model();
      sub.push([m.shards.length ? `${m.shards.filter((s) => s.st === "ok").length} of ${m.shards.length} shards serve reads and writes` : null, m.total ? `${m.up} of ${m.total} copies answer` : null,
        check("error_ratio")?.value != null ? `error ratio ${pctTxt(check("error_ratio").value)} (SLO ${pctTxt(sloErr())})` : null,
        check("p99")?.value_ms != null ? `p99 ${fmt.ms(check("p99").value_ms)} (SLO ${fmt.ms(sloP99())})` : null,
        firingAlerts().length ? null : "nothing firing", `checked ${when(f.at_ms)}`].filter(Boolean).join(" · "));
    } else {
      if (since) sub.push(`since ${when(since)} (${durTxt(since)})`);
      if (top?.impact) sub.push(top.impact);
      if (top?.where?.length) sub.push("where: " + top.where.join(", "));
      if (probs.length > 1) sub.push(`and ${fmt.plural(probs.length - 1, "other problem")}`);
    }
    const chips = [];
    if (kind !== "ok" && answering) {
      const fa = firingAlerts();
      if (since) chips.push(h("span", { class: "home-chip" }, `first signal ${when(since)}`));
      for (const a of fa.slice(0, 2)) chips.push(h("a", { class: "home-chip", href: link("alert/" + encodeURIComponent(a.id)) }, `alert "${a.name}" ${a.since_ms ? "since " + when(a.since_ms) : "firing"}`));
      const ack = fa.find((a) => a.ack);
      if (ack) chips.push(h("span", { class: "home-chip ack" }, `acked by ${ack.ack.by} ${when(ack.ack.at_ms)}`));
      else if (fa.length) chips.push(h("span", { class: "home-chip" }, "nobody has acked yet"));
    }
    const acts = [];
    if (top?.next && kind !== "ok" && answering) acts.push(h("a", { class: "btn sm primary", href: link(top.next.view, top.next.params || {}), title: top.next.text }, icon("right"), top.next.text.length > 42 ? nextShort(top.next) : top.next.text));
    if (kind !== "ok" && answering) acts.push(h("a", { class: "btn sm", href: link("logs", { level: "warn", ...(since ? { from: since - 60e3, to: Date.now() } : {}) }) }, icon("logs"), since ? `Logs ${when(since - 60e3, { sec: false })}–now` : "Logs"));
    acts.push(h("button", { class: "btn sm", title: "Copy a link to Home at this window (opens paused)", onclick: () => copy(location.origin + location.pathname + link("home", rangeParams({ absolute: true })), "Link to Home at this window copied") }, icon("share"), "Copy link"));
    clear(banner,
      h("div", { class: "bic" }, icon(kind === "ok" ? "okc" : kind === "unknown" ? "info" : "warn", "lg")),
      h("div", { class: "grow" }, h("div", { class: "big" }, headline), sub.length ? h("div", { class: "small dim" }, sub.join(" · ")) : null, chips.length ? h("div", { class: "row home-chips" }, chips) : null,
        f.derived ? h("div", { class: "tiny muted", title: "This server has no /v1/health/fleet: the command center works it out from /v1/cluster, /v1/health, /v1/alerts and this coordinator's points" }, "Worked out in the browser from this coordinator's view: an older server") : null),
      h("div", { class: "row home-bacts" }, acts));
  }
  const VIEWNAME = { cluster: "Cluster", rollout: "Rollouts", logs: "Logs", queries: "Queries", performance: "Performance", autoscale: "Autoscaling", access: "Access", timeline: "Timeline", profile: "Profiler", home: "Home" };
  const nextShort = (n) => n.view.startsWith("alert") ? "Open the alert" : n.params?.shard != null && n.view === "cluster" ? `Shard ${n.params.shard} in Cluster` : `Open in ${VIEWNAME[n.view] || n.view}`;

  // ------------------------------------------------------------ headline numbers
  function kpiDefs() {
    const m = model();
    const ps = visible();
    const lastP = ps[ps.length - 1] || {};
    const f = state.fleet;
    const er = check("error_ratio"), p9 = check("p99"), cp = check("copies"), lg = check("lag"), dk = check("disk"), shc = check("shards");
    const coords = f?.coordinators || [];
    const qpsFleet = coords.length > 1 ? coords.reduce((a, c) => a + (c.qps || 0), 0) : lastP.qps ?? coords[0]?.qps ?? null;
    const tone = (st) => st === "critical" ? "bad" : st === "warning" ? "warn" : "";
    const out = [];
    const dark = m.shards.filter((s) => ["no_reads", "no_writes", "down"].includes(s.st));
    if (dark.length) {
      out.push({ key: "shards", label: "Shards serving", icon: "shard", val: [String(m.shards.length - dark.length), h("small", {}, "/" + m.shards.length)], tone: "bad", sub: dark.slice(0, 3).map((s) => `shard ${s.n} ${s.st === "no_reads" ? "has no majority" : stText(s.st)}`).join(", "), why: `${fmt.plural(dark.length, "shard")} serve no reads or take no writes`, link: link("cluster", { shard: dark[0].n }) });
    } else {
      out.push({ key: "qps", label: coords.length > 1 ? "Queries, fleet" : "Queries", icon: "queries", val: [fmt.si(qpsFleet), h("small", {}, "/s")], spark: ps.map((p) => p.qps), color: SERIES[0], sub: coords.length > 1 ? `${coords.length} coordinators` : "this coordinator, now", link: link("performance") });
    }
    const p99v = p9?.value_ms ?? lastP.p99_ms ?? null;
    out.push({ key: "p99", label: "p99 latency", icon: "clock", val: msParts(p99v), tone: p9 ? tone(p9.status) : p99v > sloP99() ? "bad" : "", spark: ps.map((p) => p.p99_ms), color: SERIES[6], sub: `SLO ${fmt.ms(sloP99())}${p9?.window_s ? ` · over ${fmt.dur(p9.window_s)}` : ""}`, why: p99v != null ? `p99 ${fmt.ms(p99v)} is over the SLO (${fmt.ms(sloP99())})` : null, link: link("performance") });
    const erv = er?.value ?? null;
    const failing = erv != null && qpsFleet ? erv * qpsFleet : null;
    out.push({ key: "err", label: "Error ratio", icon: "warn", val: erv == null ? ["–"] : [pctTxt(erv).replace("%", ""), h("small", {}, "%")], tone: er ? tone(er.status) : "", spark: ps.map((p) => p.error_ratio ?? null), color: SERIES[7], sub: `SLO ${pctTxt(sloErr())}${failing != null && failing >= 0.1 ? ` · ${fmt.si(failing)} q/s failing` : erv == null ? " · no query ended" : ""}`, why: erv != null ? `${pctTxt(erv)} of queries fail (SLO ${pctTxt(sloErr())})` : null, link: link("queries", { tab: "statements", outcome: "error" }) });
    const downNames = (cp?.where || m.shards.flatMap((s) => s.copies.filter((x) => !x.up).map((x) => x.name)));
    const tot = cp?.total ?? m.total, upN = cp ? cp.total - cp.down : m.up;
    out.push({ key: "copies", label: "Copies answering", icon: "db", val: tot ? [String(upN), h("small", {}, "/" + tot)] : ["–"], tone: cp ? tone(cp.status) : upN < tot ? "warn" : "", sub: downNames.length ? `${downNames.slice(0, 3).join(", ")}${downNames.length > 3 ? ` +${downNames.length - 3}` : ""} down` : tot ? "every copy answers" : "no copies listed", why: downNames.length ? `${fmt.plural(downNames.length, "copy", "copies")} do not answer` : null, link: link("cluster") });
    const lagV = lg?.max_txn ?? null;
    out.push({ key: "lag", label: "Replication lag, max", icon: "layers", val: lagV == null ? ["–"] : [fmt.si(lagV), h("small", {}, "txn")], tone: lg ? tone(lg.status) : "", sub: lagV == null ? "no replica reports one" : lagV === 0 ? "every replica in step" : `${lg.where || ""}${lg.max_s != null ? ` · reads up to ${fmt.dur(lg.max_s)} old` : ""}`, why: lagV ? `${lg.where} is ${fmt.si(lagV)} transactions behind` : null, link: link("cluster", { lens: "lag" }) });
    const dkV = dk?.max ?? null;
    out.push({ key: "disk", label: "Fullest disk", icon: "disk", val: dkV == null ? ["–"] : [fmt.n(dkV * 100, 0), h("small", {}, "%")], tone: dk ? tone(dk.status) : "", sub: dkV == null ? "not known" : `${dk.where || ""}${dk.days_to_full != null ? ` · full in ~${fmt.n(dk.days_to_full, dk.days_to_full < 10 ? 1 : 0)} days` : ""}${dk.median != null ? ` · median ${fmt.pct(dk.median, 0)}` : ""}`, why: dkV != null ? `${dk.where}'s disk is ${fmt.pct(dkV, 0)} full` : null, link: link("cluster", { tab: "capacity" }) });
    if (shc && !dark.length && shc.status !== "ok") out[0].tone = tone(shc.status);
    return out;
  }
  function drawKpis() {
    const fr = fresh("fleet");
    clear(kpiRow, kpiDefs().map((d) => {
      const c = h("a", { class: "card kpi home-kpi " + (d.tone || "") + (!fr.ok && !fr.loading ? " stale" : ""), href: d.link, title: d.why || "" },
        h("div", { class: "lbl" }, icon(d.icon), d.label), h("div", { class: "val" }, d.val), h("div", { class: "delta" }, d.sub || " "));
      if (d.spark && d.spark.filter((x) => x != null).length > 1) c.append(sparkline(d.spark.slice(-180), { color: d.tone === "bad" ? "var(--crit)" : d.tone === "warn" ? "var(--warn)" : d.color }));
      return c;
    }));
  }

  // ------------------------------------------------------------ where
  let lastModel = null;
  function drawWhere() {
    const m = lastModel = model();
    whereCard.classList.toggle("stale", !fresh("cluster").ok && !fresh("cluster").loading);
    if (!m.shards.length) {
      clear(whereBody, state.cluster === null && !isCoordinator() ? empty("db", "One server", "This server is not a coordinator: it has no shards to show. Its health is in the numbers above.") : empty("db", state.cluster ? "No shards listed" : "Reading the cluster…"));
      whereSub.textContent = "";
      return;
    }
    whereSub.textContent = group === "shard" ? `${fmt.plural(m.shards.length, "shard")}, each its worst copy now · no colour = fine` : group === "node" ? (m.nodes.size ? `${fmt.plural(m.total, "copy", "copies")} on ${fmt.plural(m.nodes.size, "node")}` : "nodes not known: grouped by address") : (m.zones.size ? `${fmt.plural(m.zones.size, "zone")}` : "zones not known");
    const legend = h("div", { class: "row home-legend small muted" },
      h("span", {}, h("i", { class: "lg" }), " fine"), h("span", {}, h("i", { class: "lg warn" }), " needs a look"), h("span", {}, h("i", { class: "lg crit" }), " hurts queries now"), h("span", {}, h("i", { class: "pip down" }), " copy down"), h("span", {}, h("i", { class: "pip p" }), " primary"),
      h("span", { class: "sep" }), h("span", {}, "click a shard: its copies, logs and profile for this window"));
    if (group === "shard") {
      const big = m.shards.length <= 16;
      const grid = h("div", { class: "home-heat" + (big ? " big" : "") }, m.shards.map((s) => shardEl(s, big)));
      clear(whereBody, grid, legend, zoneCards(m));
    } else {
      const groups = group === "node" ? (m.nodes.size ? m.nodes : byHost(m)) : m.zones;
      if (!groups.size) { clear(whereBody, empty("cloud", "Zones not known", "Each copy says its zone when the server runs with STRATA_ZONE set (the operator sets it on Kubernetes)."), legend); return; }
      const list = [...groups.entries()].map(([k, xs]) => ({ k, xs, bad: xs.filter((x) => copyTone(x) === "crit").length, warn: xs.filter((x) => copyTone(x) === "warn").length }))
        .sort((a, b) => (b.bad - a.bad) || (b.warn - a.warn) || String(a.k).localeCompare(String(b.k), undefined, { numeric: true }));
      clear(whereBody, h("div", { class: "home-groups" + (group === "zone" ? " zones" : "") }, list.map((g) => h("div", { class: "home-group" + (g.bad ? " crit" : g.warn ? " warn" : "") },
        h("div", { class: "row nw small" }, h("b", { class: "ellipsis", title: g.k }, group === "zone" ? "zone " + g.k : g.k), h("span", { class: "sep" }), h("span", { class: "muted" }, `${g.xs.filter((x) => x.up).length}/${g.xs.length}`)),
        h("div", { class: "home-copies" }, g.xs.sort((a, b) => a.shard - b.shard || a.j - b.j).map((x) => copyEl(x)))))), legend);
    }
  }
  function byHost(m) { const g = new Map(); for (const s of m.shards) for (const x of s.copies) { const k = x.host.replace(/:\d+$/, ""); if (!g.has(k)) g.set(k, []); g.get(k).push(x); } return g; }
  function shardEl(s, big) {
    const c = shardCell(s, lens);
    const el = h("button", { class: "hc " + c.tone, "aria-label": `shard ${s.n}: ${c.value}` },
      h("span", { class: "n" }, String(s.n)), c.tag ? h("em", {}, c.tag) : null,
      big ? h("span", { class: "names" }, s.copies.map((x) => h("span", { class: copyTone(x) || "" }, x.name))) : null,
      h("span", { class: "pips" }, s.copies.map((x) => h("i", { class: "pip" + (x.primary ? " p" : "") + (!x.up ? " down" : copyTone(x) ? " " + copyTone(x) : "") }))));
    tipOn(el, () => [h("div", { class: "t" }, `Shard ${s.n} · ${stText(s.st)}`), h("div", { class: "small", style: { marginBottom: "4px" } }, c.value),
      ...s.copies.map((x) => h("div", { class: "r" }, h("span", { class: "k box", style: { background: !x.up ? "var(--crit)" : copyTone(x) === "warn" ? "var(--warn)" : copyTone(x) === "crit" ? "var(--crit)" : "var(--good)" } }), h("b", {}, x.name), h("span", { class: "n" }, [x.up ? x.role : "not answering", x.p99 != null ? "p99 " + fmt.ms(x.p99) : null, x.disk != null ? "disk " + fmt.pct(x.disk, 0) : null, x.lag ? fmt.si(x.lag) + " behind" : null].filter(Boolean).join(" · "))))]);
    el.onclick = () => openShard(s);
    return el;
  }
  function copyEl(x) {
    const t = copyTone(x);
    const el = h("button", { class: "hcopy " + t + (x.primary ? " p" : "") + (!x.up ? " down" : ""), title: `${copyTitle(x.url)}\n${x.up ? x.role : "not answering"}${x.p99 != null ? " · p99 " + fmt.ms(x.p99) : ""}${x.disk != null ? " · disk " + fmt.pct(x.disk, 0) : ""}${x.lag ? " · " + fmt.si(x.lag) + " behind" : ""}` }, x.name);
    el.onclick = () => openShard(lastModel.shards.find((s) => s.n === x.shard));
    return el;
  }
  function zoneCards(m) {
    if (!m.zones.size) return null;
    return h("div", { class: "home-zones" }, [...m.zones.entries()].sort().map(([z, xs]) => {
      const down = xs.filter((x) => !x.up);
      const prim = xs.filter((x) => x.primary && copyTone(x) === "crit");
      return h("a", { class: "home-zone" + (down.length ? " crit" : ""), href: link("cluster", { group: "zone", zone: z }) }, h("b", {}, "zone " + z),
        h("span", { class: "small dim" }, `${xs.length - down.length}/${xs.length} copies answer`),
        h("span", { class: "small muted ellipsis" }, down.length ? `${down.length} down · ${down.slice(0, 3).map((x) => x.name).join(", ")}` : "0 down" + (prim.length ? ` · ${prim[0].name} primary here` : "")));
    }));
  }
  function openShard(s) {
    if (!s) return;
    const r = timeRange();
    const prim = s.copies.find((x) => x.primary);
    const rows = s.copies.map((x) => h("tr", {},
      h("td", {}, h("span", { class: "dot " + (x.up ? copyTone(x) === "crit" ? "bad" : copyTone(x) === "warn" ? "warn" : "ok" : "bad") }), " ", h("b", { title: copyTitle(x.url) }, x.name)),
      h("td", {}, x.up ? x.role : `not answering${x.down_since ? " since " + when(x.down_since) : ""}`), h("td", { class: "small dim" }, [x.zone ? "zone " + x.zone : null, x.node].filter(Boolean).join(" · ") || x.host),
      h("td", { class: "num" }, x.p99 != null ? fmt.ms(x.p99) : "–"), h("td", { class: "num" }, x.disk != null ? fmt.pct(x.disk, 0) : "–"), h("td", { class: "num" }, x.lag != null ? fmt.si(x.lag) : "–"),
      h("td", {}, h("a", { class: "btn xs", href: link("logs", { node: x.url }) }, "Logs"), " ", x.up ? h("a", { class: "btn xs", href: link("profile", { tab: "cpu", node: x.url }) }, "Profile") : null)));
    const d = drawer({
      title: `Shard ${s.n}`, sub: `${stText(s.st)} · ${span(r.from, r.to)}`,
      body: [
        s.why || s.st !== "ok" ? h("div", { class: s.st === "ok" ? "notice" : ["no_reads", "no_writes", "down"].includes(s.st) ? "error" : "notice" }, s.why || `${s.copies.filter((x) => x.up).length} of ${s.copies.length} copies answer.`) : null,
        h("div", { class: "tbl-wrap" }, h("table", { class: "t compact" }, h("thead", {}, h("tr", {}, ["Copy", "Role", "Where", "p99", "Disk", "Behind", ""].map((x, i) => h("th", { class: i >= 3 && i <= 5 ? "num" : "" }, x)))), h("tbody", {}, rows))),
        h("div", { class: "row" },
          h("a", { class: "btn primary", href: link("cluster", { shard: s.n }) }, icon("cluster"), "Shard " + s.n + " in Cluster"),
          h("a", { class: "btn", href: link("logs", { shard: s.n, level: "warn" }) }, icon("logs"), "Its logs, this window"),
          prim ? h("a", { class: "btn", href: link("profile", { tab: "cpu", node: prim.url }) }, icon("flame"), `Profile ${prim.name}`) : null,
          h("a", { class: "btn", href: link("timeline", { shard: s.n }) }, icon("timeline"), "Its events")),
      ],
    });
    d.el.addEventListener("click", (e) => { if (e.target.closest("a[href^='#']")) d.close(); });
  }

  // ------------------------------------------------------------ problems
  function drawProblems() {
    const f = state.fleet;
    probCard.classList.toggle("stale", !fresh("fleet").ok && !fresh("fleet").loading);
    if (!f) { clear(probBody, h("div", { class: "muted small" }, "Checking…")); return; }
    const probs = f.problems || [];
    const restore = probs[0]?.severity === "critical" && /majority|dark|does not answer|no reads/i.test(probs[0].title);
    probTitle.textContent = restore ? "What would restore service" : "Problems";
    probSub.textContent = probs.length ? `${probs.length}, worst first · where, since when, next step` : "";
    if (!probs.length) {
      const m = model();
      const okRows = [
        m.shards.length ? `${m.shards.length} of ${m.shards.length} shards serve reads and writes` : null,
        m.total ? `${m.up} of ${m.total} copies answer` : null,
        check("error_ratio")?.value != null ? `${pctTxt(check("error_ratio").value)} of queries fail (SLO ${pctTxt(sloErr())})` : "no failed queries counted",
        check("p99")?.value_ms != null ? `p99 ${fmt.ms(check("p99").value_ms)} (SLO ${fmt.ms(sloP99())})` : null,
        check("lag") ? `replication lag at most ${fmt.si(check("lag").max_txn || 0)} transactions` : null,
        check("disk")?.max != null ? `fullest disk ${fmt.pct(check("disk").max, 0)}` : null,
        `${firingAlerts().length ? fmt.plural(firingAlerts().length, "alert") + " firing" : "no alert firing"}`,
      ].filter(Boolean);
      clear(probBody, h("div", { class: "row nw", style: { gap: "10px" } }, h("span", { style: { color: "var(--good)" } }, icon("okc", "lg")), h("b", {}, "No problems")),
        h("ul", { class: "home-oklist" }, okRows.map((t) => h("li", {}, icon("check"), t))),
        h("div", { class: "small muted" }, `As of ${when(f.at_ms)}. What changed recently is on the `, h("a", { href: link("timeline") }, "Timeline"), "."));
      return;
    }
    clear(probBody, probs.slice(0, 8).map((p) => {
      const actions = [];
      if (p.next) actions.push(h("a", { class: "btn xs primary", href: link(p.next.view, p.next.params || {}), title: p.next.text }, nextShort(p.next)));
      const copyUrl = (p.where || []).map((w) => findUrl(w)).find(Boolean);
      const shardW = (p.where || []).map((w) => /^shard (\d+)$/.exec(w)).find(Boolean);
      actions.push(h("a", { class: "btn xs", href: link("logs", { ...(copyUrl ? { node: copyUrl } : shardW ? { shard: shardW[1] } : {}), level: "warn", ...(p.since_ms ? { from: p.since_ms - 60e3, to: Date.now() } : {}) }) }, "Logs"));
      if (copyUrl && p.id !== "copies-down") actions.push(h("a", { class: "btn xs", href: link("profile", { tab: "cpu", node: copyUrl }) }, "Profile " + copyName(copyUrl)));
      return h("div", { class: "prob " + (p.severity === "critical" ? "crit" : "warn") },
        h("span", { class: "sv" }),
        h("div", { class: "grow" },
          h("div", { class: "pt" }, p.title),
          p.where?.length ? h("div", { class: "small dim" }, p.where.join(" · ")) : null,
          h("div", { class: "small dim" }, [p.since_ms ? h("span", { title: whenTitle(p.since_ms) }, `since ${when(p.since_ms)} (${durTxt(p.since_ms)})`) : null, p.impact].filter(Boolean).flatMap((x, i) => i ? [" · ", x] : [x])),
          p.next?.text ? h("div", { class: "small home-next" }, icon("right"), p.next.text) : null,
          h("div", { class: "row home-pacts" }, actions)));
    }), probs.length > 8 ? h("div", { class: "small muted" }, `and ${probs.length - 8} more`) : null);
  }
  function findUrl(name) {
    for (const s of model().shards) for (const x of s.copies) if (x.name === name || x.url === name) return x.url;
    return null;
  }

  // ------------------------------------------------------------ since when
  function drawCharts() {
    const ps = visible();
    const slo = sloErr(), slop = sloP99();
    errChart.setThreshold(slo); p99Chart.setThreshold(slop);
    errChart.update(ps); p99Chart.update(ps);
    chartsCard.classList.toggle("stale", !fresh("series").ok && !fresh("series").loading);
    const coords = state.fleet?.coordinators?.length || 1;
    const r = timeRange();
    const src = state.series;
    chartsSub.textContent = [coords > 1 ? "the queries this coordinator answered (each coordinator's own are below)" : "the queries this server answered",
      `with the fleet's changes in ${r.zoomed ? "the zoom" : "the window"}`, src.source === "load" ? "a point a minute" : `a point every ${fmt.dur(state.interval / 1000)}`,
      src.partial ? "this server keeps only its last hour" : null, "drag to zoom; the zoom goes into the links"].filter(Boolean).join(" · ");
    const q = rangeParams({ absolute: r.zoomed });
    clear(zoomLinks,
      h("a", { class: "btn ghost sm", href: link("logs", { level: "warn", ...q }) }, r.zoomed ? "Logs for the zoom" : "Logs for the window", icon("right")),
      h("a", { class: "btn ghost sm", href: link("queries", { tab: "statements", ...q }) }, r.zoomed ? "Queries for the zoom" : "Queries for the window", icon("right")));
    drawLane();
  }
  function drawLane() {
    const [t0, t1] = domain();
    const evs = events.filter((e) => e.ts_ms >= t0 && e.ts_ms <= t1 && (e.severity !== "info" || MARK.has(e.kind)));
    // The chart's plot starts 46 px in and ends 14 px before the edge.
    clear(lane, evs.slice(0, 40).reverse().map((e, i) => {
      const x = (e.ts_ms - t0) / Math.max(1, t1 - t0);
      const el = h("a", { class: "ev " + (e.severity === "critical" ? "crit" : e.severity === "warning" ? "warn" : MARK.has(e.kind) ? "accent" : ""), href: eventLink(e), title: `${whenTitle(e.ts_ms)}\n${e.title}${e.who ? "\nby " + e.who : ""}`,
        style: { left: `calc(46px + (100% - 60px) * ${x.toFixed(4)})`, top: (i % 3) * 15 + "px" } }, h("b", {}, when(e.ts_ms, { sec: false })), " ", e.title);
      return el;
    }));
    if (!evs.length) lane.append(h("div", { class: "small muted", style: { paddingLeft: "46px", paddingTop: "4px" } }, eventsFrom === "events" ? "No change in this window: no copy went down, no primary moved, nobody changed anything." : "This server keeps no events (it has no /v1/events): only alert changes show."));
  }
  function drawEvents() {
    const evs = events.slice(0, 12);
    if (!evs.length) { clear(evList, h("div", { class: "small muted" }, eventsFrom === "events" ? `Nothing happened in ${timeRange().label.replace("last", "the last")}.` : "This server keeps no events: an older server.")); return; }
    clear(evList, evs.map((e) => h("a", { class: "home-ev", href: eventLink(e), title: whenTitle(e.ts_ms) },
      h("span", { class: "mono small" }, when(e.ts_ms)), h("span", { class: "home-kind " + kindTone(e) }, kindName(e.kind)), h("span", { class: "small" }, e.title, e.who ? h("span", { class: "muted" }, " · " + e.who) : null))));
  }

  // ------------------------------------------------------------ capacity
  function drawCapacity() {
    const m = model();
    const xs = m.shards.flatMap((s) => s.copies).filter((x) => x.up);
    const dk = check("disk");
    const row = (label, f, extra) => {
      const vs = xs.map((x) => ({ x, v: f(x) })).filter((a) => a.v != null);
      if (!vs.length) return h("div", { class: "home-caprow" }, h("div", { class: "row nw" }, h("b", {}, label), h("span", { class: "sep" }), h("span", { class: "small muted" }, "not known")));
      const worst = vs.reduce((a, b) => (b.v > a.v ? b : a));
      const med = median(vs.map((a) => a.v));
      const tone = worst.v > 0.9 ? "crit" : worst.v > 0.75 ? "warn" : "";
      return h("div", { class: "home-caprow" },
        h("div", { class: "row nw small" }, h("b", { class: "lbl" }, label), h("span", {}, `worst ${fmt.pct(worst.v, 0)} · `, h("span", { title: copyTitle(worst.x.url) }, worst.x.name), extra ? " · " + extra : ""), h("span", { class: "sep" }), h("span", { class: "muted" }, `median ${fmt.pct(med, 0)}`)),
        h("div", { class: "home-meter " + tone }, h("i", { style: { width: Math.min(100, worst.v * 100) + "%" } }), h("b", { class: "med", style: { left: Math.min(100, med * 100) + "%" }, title: "median" })));
    };
    if (!xs.length) { clear(capBody, h("div", { class: "small muted" }, isCoordinator() || state.cluster ? "No copy reports its load." : "One server: its own numbers are on System.")); return; }
    clear(capBody,
      row("CPU", (x) => x.cpu),
      row("Disk", (x) => x.disk, dk?.days_to_full != null ? `full in ~${fmt.n(dk.days_to_full, 1)} days` : null),
      row("Memory", (x) => x.mem),
      row("Query slots", (x) => x.slots));
  }

  // ------------------------------------------------------------ coordinators
  function drawCoords() {
    const f = state.fleet;
    const cs = f?.coordinators || [];
    if (!cs.length) { clear(coordBody, h("div", { class: "small muted", style: { padding: "0 14px 14px" } }, f ? "No coordinator listed: this server is not one." : "Checking…")); return; }
    const t = h("table", { class: "t compact" }, h("thead", {}, h("tr", {}, ["Coordinator", "q/s", "p99", "Errors", "CPU"].map((x, i) => h("th", { class: i ? "num" : "" }, x)))),
      h("tbody", {}, cs.map((c) => h("tr", {},
        h("td", {}, h("span", { class: "dot " + ((c.error_ratio ?? 0) > sloErr() || (c.p99_ms ?? 0) > sloP99() ? "bad" : "ok") }), " ", c.url && !c.this ? h("a", { href: c.url.replace(/\/$/, "") + "/#/home", title: c.url }, c.name || hostOf(c.url)) : h("span", { title: c.url || "" }, c.name || hostOf(c.url)), c.this ? h("span", { class: "muted" }, " (this)") : null),
        h("td", { class: "num" }, fmt.si(c.qps)), h("td", { class: "num" }, c.p99_ms != null ? fmt.ms(c.p99_ms) : "–"),
        h("td", { class: "num " + ((c.error_ratio ?? 0) > sloErr() ? "home-bad" : "") }, pctTxt(c.error_ratio)), h("td", { class: "num" }, c.cpu != null ? (c.cpu > 1.5 ? fmt.si(c.cpu) + " cores" : fmt.pct(c.cpu, 0)) : "–")))));
    let note = null;
    const ers = cs.map((c) => c.error_ratio).filter((x) => x != null);
    if (cs.length === 1) note = f.derived ? "This server does not list the other coordinators (an older server)." : "One coordinator: its numbers are the fleet's.";
    else if (ers.length === cs.length && Math.max(...ers) > sloErr()) {
      const lo = Math.min(...ers), hi = Math.max(...ers);
      note = hi < lo * 1.5 ? `The same error ratio on all ${cs.length}: not a coordinator problem.` : `${cs.find((c) => c.error_ratio === hi).name || "One coordinator"} fails more than the others: look at it first.`;
    }
    clear(coordBody, t, note ? h("div", { class: "small muted", style: { padding: "8px 14px 12px" } }, note) : null);
  }

  // ------------------------------------------------------------ who is affected
  function drawWho() {
    if (stats === "forbidden") { clear(whoBody, empty("lock", "Needs an admin key", "Who sends which queries is for admin keys.")); return; }
    if (!stats) { clear(whoBody, h("div", { class: "small muted" }, "Reading the statements of this window…")); return; }
    const byTable = new Map(), byKey = new Map();
    let calls = 0, errors = 0;
    for (const st of stats) {
      const c = st.calls || 0;
      const e = st.errors ?? (st.outcomes ? (st.outcomes.error || 0) + (st.outcomes.timeout || 0) : 0);
      calls += c; errors += e;
      const tb = tableOf(st.sql || st.example || st.fingerprint || "");
      if (tb) { const a = byTable.get(tb) || { calls: 0, errors: 0 }; a.calls += c; a.errors += e; byTable.set(tb, a); }
      for (const k of st.keys || []) { const a = byKey.get(k.key) || { calls: 0, errors: 0 }; a.calls += k.calls || 0; a.errors += k.errors || 0; byKey.set(k.key, a); }
    }
    const failing = errors > 0;
    whoSub.textContent = failing ? `share of the failed queries, by table${statsKeys ? " and by API key" : ""}, this window` : `share of the queries by table${statsKeys ? " and by API key" : ""}, this window: none failed`;
    // A bar is the share of the window's failures (or, with none, of its queries).
    const bars = (m, kind) => [...m.entries()].map(([k, a]) => ({ k, ...a, share: failing ? a.errors / errors : a.calls / Math.max(1, calls) }))
      .sort((a, b) => failing ? (b.errors - a.errors) : (b.calls - a.calls)).slice(0, kind === "key" ? 4 : 3)
      .map((a) => h("a", { class: "home-who " + (failing && a.errors / Math.max(1, a.calls) > sloErr() ? "bad" : ""), href: kind === "key" ? link("queries", { key: a.k, tab: "statements" }) : link(byIdTables(), { table: a.k }), title: failing ? `${fmt.pct(a.share, 0)} of the failures: ${fmt.n(a.errors)} of its ${fmt.n(a.calls)} queries failed (${pctTxt(a.errors / Math.max(1, a.calls))})` : `${fmt.n(a.calls)} queries, ${fmt.pct(a.share, 0)} of all` },
        h("span", { class: "nm " + (kind === "key" ? "mono" : "") }, a.k), h("span", { class: "home-meter " + (failing ? "crit" : "") }, h("i", { style: { width: Math.max(1, a.share * 100) + "%" } })), h("span", { class: "num small" }, fmt.pct(a.share, a.share < 0.1 ? 1 : 0))));
    clear(whoBody,
      byTable.size ? h("div", { class: "stack", style: { gap: "6px" } }, bars(byTable, "table")) : null,
      statsKeys ? h("div", { class: "stack", style: { gap: "6px", marginTop: "10px" } }, bars(byKey, "key")) : h("div", { class: "small muted", style: { marginTop: "10px" } }, "This server does not record which API key sent a query (an older server)."),
      h("div", { class: "small muted", style: { marginTop: "8px" } }, calls ? `${fmt.n(errors)} of ${fmt.n(calls)} queries failed in ${timeRange().label.replace("last", "the last")}${stats.length && !statsKeys && !stats[0].prev ? " (an older server counts since it started)" : ""}.` : "No queries in this window."));
  }
  const byIdTables = () => "tables";
  const tableOf = (sql) => { const m = /\bFROM\s+("?)([A-Za-z_][\w.]*)\1/i.exec(sql) || /\bINTO\s+([A-Za-z_][\w.]*)/i.exec(sql); return m ? (m[2] || m[1]) : null; };

  // ------------------------------------------------------------ wiring
  function drawFleet() { drawHead(); drawKpis(); drawBanner(); drawProblems(); drawCoords(); }
  function drawCluster() { drawHead(); drawWhere(); drawCapacity(); drawKpis(); drawBanner(); }
  drawFleet(); drawCluster(); drawCharts(); drawEvents(); drawWho();
  const offs = [
    bus.on("fleet", drawFleet), bus.on("cluster", drawCluster), bus.on("alerts", drawBanner),
    bus.on("points", () => { drawCharts(); drawKpis(); }), bus.on("range", () => { drawCharts(); drawHead(); loadEvents(); loadStats(); }),
    bus.on("fresh", () => { drawKpis(); drawWhere(); drawProblems(); chartsCard.classList.toggle("stale", !fresh("series").ok && !fresh("series").loading); }),
    bus.on("answering", () => { drawBanner(); }), bus.on("theme", drawCharts),
    every(15000, loadEvents), every(30000, loadStats),
  ];
  return () => offs.forEach((f) => f());
}

// ---------------------------------------------------------------- events, shared with the timeline
/** The view an event opens, at its time (a fixed window around it, which opens paused). */
export function eventLink(e) {
  const t = e.ts_ms, from = t - 15 * 60e3, to = Math.min(Date.now(), (e.until_ms || t) + 15 * 60e3);
  const w = e.where || {};
  const at = { from, to };
  const k = e.kind || "";
  if (k.startsWith("alert")) return link("alert/" + encodeURIComponent(w.rule || e.detail?.rule || ""), at);
  if (k === "rollout" || k === "upgrade") return link("rollout", at);
  if (k === "autoscale") return link("autoscale", at);
  if (k === "change") return link("access", { tab: "changes", q: e.title, ...at });
  if (k === "log_level") return link("logs", at);
  if (k === "contain") return link("queries", { tab: "running", ...at });
  if (k === "note") return link("timeline", { ...at, event: e.id });
  if (w.copy || w.shard != null) return link("cluster", { shard: w.shard, copy: w.copy, ...at });
  return link("timeline", { ...at, event: e.id });
}
export const KINDS = {
  copy_down: "copy", copy_up: "copy", copy_added: "copy", copy_removed: "copy", primary_changed: "primary", majority_lost: "shard", majority_back: "shard",
  shard_added: "shard", shard_removed: "shard", moves_started: "moves", moves_finished: "moves", alert_fired: "alert", alert_resolved: "alert", alert_acked: "on-call",
  alert_silenced: "on-call", rollout: "rollout", autoscale: "autoscale", upgrade: "rollout", change: "change", note: "note", log_level: "config", contain: "contain",
};
export const kindName = (k) => KINDS[k] || (k || "event").replace(/_/g, " ");
export const kindTone = (e) => e.severity === "critical" ? "crit" : e.severity === "warning" ? "warn" : e.kind === "note" ? "note" : ["change", "rollout", "upgrade", "autoscale", "log_level"].includes(e.kind) ? "accent" : "";

function msParts(v) {
  if (v == null) return ["–"];
  const t = fmt.ms(v); const i = t.lastIndexOf(" ");
  return i < 0 ? [t] : [t.slice(0, i), h("small", {}, t.slice(i + 1))];
}

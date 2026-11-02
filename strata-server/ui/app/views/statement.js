// A statement's page (#/statement/<fingerprint>): one shape of SQL over the
// range in the top bar against the window before. Its figures, a trend
// with what happened around it marked (rollouts, alerts, changes), why it
// is slow worked out from those figures, where (by shard and copy) and
// who (by key) it runs, its slowest calls with their parameters (each can
// be profiled again or opened in the workbench), its errors, what else
// happened in the window, and the actions: contain it, cancel what runs.

import { h, icon, clear, fmt, api, card, empty, sqlBlock, copy, failure, table, bus } from "../core.js";
import { timeChart, SERIES } from "../charts.js";
import { firingAlerts } from "../data.js";
import {
  state, link, when, can, timeRange, rangeQuery, poll, isCoord, copyName, copyTitle, outcomeOf, outcomePill, outcomeBar, split, paramsText, paramsObj,
  shortFp, statementDetail, change, queryDrawer, askCancel, containDialog, workbenchLink, profileLink, nodeLink, copyLink, shardLink, ask, objects, msOf, parseJson, hasLogStore,
  narrow, cols, sheet,
} from "../workload.js";

export default { id: "statement", title: "Statement", icon: "queries", group: "Operate", parent: "queries", hidden: true, nav: false, keywords: "statement fingerprint shape", render, onParams };

let reroute = null;
function onParams(r) { reroute?.(r); }

const tableOf = (sql) => /\bFROM\s+("?)([A-Za-z_][\w.]*)\1/i.exec(sql || "")?.[2] || null;

/** The fingerprint in the URL, read from the hash itself so that it is decoded once whichever router passed it. */
function fpOf(r) {
  const m = /^#\/?statement\/([^?]*)/.exec(location.hash);
  if (m) return decode(m[1]);
  return r.params.get("fingerprint") || r.params.get("fp") || (r.rest || []).join("/");
}

export function render(page, r) {
  let fp = fpOf(r);
  let d = null, live = [], err = null, gen = 0;
  const head = h("div"), body = h("div");
  page.append(head, body);
  const offs = [];

  async function load() {
    const my = ++gen;
    const range = timeRange();
    try {
      const [x, lq] = await Promise.all([statementDetail(fp, range), api("/v1/queries?group=shape").catch(() => api("/v1/queries").catch(() => ({ queries: [] })))]);
      if (my !== gen) return;
      d = x; err = null;
      live = (lq.queries || []).filter((q) => (q.fingerprint || "") === fp);
    } catch (e) { if (my !== gen) return; err = e; }
    draw();
  }

  function draw() {
    const range = timeRange();
    const st = d?.statement;
    const sql = st?.sql || fp;
    const c = st ? change(st) : null;
    const sp = st ? split(st.outcomes, st.calls) : null;
    // An older server counts a statement since it started, not over the range.
    const sinceStart = d?.source === "fallback" && !d.windowed;
    const secs = Math.max(1, sinceStart && d.since_ms ? (Date.now() - d.since_ms) / 1000 : ((range.to ?? Date.now()) - range.from) / 1000);
    const slowSince = c?.p99X >= 2 ? firstAbove(d.trend, st.prev.p99_ms * 2) : null;
    const status = !st ? null : sp.server / Math.max(1, sp.calls) > 0.01 ? ["bad", "failing"] : sp.client / Math.max(1, sp.calls) > 0.05 ? ["warn", "client errors"] : st.new || (st.first_seen_ms && st.first_seen_ms >= range.from) ? ["info", `new since ${when(st.first_seen_ms)}`] : c?.p99X >= 2 ? ["bad", slowSince ? `slower since ${when(slowSince, { sec: false })}` : `slower ×${c.p99X.toFixed(1)}`] : null;
    const sample = d?.slowest?.find((x) => x.params) || (st?.sample?.params ? st.sample : null);
    const table_ = tableOf(sql);
    clear(head,
      h("div", { class: "row small muted", style: { marginBottom: "6px" } }, h("a", { class: "wl-link", href: link("queries", { tab: "live" }) }, "Queries"), " › ", h("a", { class: "wl-link", href: link("queries", { tab: "statements" }) }, "Statements"), ` › ${shortFp(fp)}`),
      h("div", { class: "page-head" },
        h("div", {}, h("h1", {}, `Statement ${shortFp(fp)} `, st?.kind ? h("span", { class: "badge mono", style: { verticalAlign: "middle" } }, st.kind) : null, " ", status ? h("span", { class: "pill " + status[0], style: { verticalAlign: "middle" } }, status[1]) : null),
          h("p", {}, [table_ ? `Table ${table_}` : null, st?.first_seen_ms ? `first seen ${fmt.ago(st.first_seen_ms)}` : null, isCoord() ? "what this coordinator ran" : "what this server ran",
            d?.source === "fallback" ? (d.windowed ? range.label : "since the server started (an older server)") : `${range.label}${st?.prev ? ", against the window before" : ""}`].filter(Boolean).join(" · "))),
        h("div", { class: "tools" },
          h("button", { class: "btn sm", onclick: () => copy(location.href, "Link copied") }, icon("link"), "Copy link"),
          h("a", { class: "btn sm", href: link("queries", { tab: "history", fingerprint: fp }), title: "Every query of this shape the log store kept" }, "History of this shape"),
          h("a", { class: "btn sm", href: workbenchLink(sample ? sqlWith(sql, sample) : sql, sample?.params), title: sample ? "With the parameters of its slowest call" : "It has no parameters kept" }, icon("sql"), sample ? "Open in the workbench, with params" : "Open in the workbench"),
          h("a", { class: "btn sm", href: profileLink(sample ? sqlWith(sql, sample) : sql, sample?.params) }, icon("flame"), "Profile it"),
          can("admin") ? h("button", { class: "btn sm", onclick: () => containDialog({ fingerprint: fp, sql, key: st?.keys?.length === 1 ? st.keys[0].key : null, keys: keyShares(st), onDone: load }) }, icon("shield"), "Contain…") : null,
          can("admin") && live.length ? h("button", { class: "btn sm danger", onclick: () => askCancel({ sql, fingerprint: fp, queries: live, key: null, onDone: load }) }, `Cancel ${fmt.n(live.length)} running…`) : null)),
      sqlBlock(fp || sql, { style: { marginBottom: "14px" }, title: "The statement's shape: its literals are ? and its parameters :name" }));
    if (err) { clear(body, failure(err, "This statement")); return; }
    if (!d) { clear(body, h("div", { class: "small muted" }, "Reading…")); return; }
    if (!st && !d.trend?.length && !d.slowest?.length) { clear(body, card({ body: empty("queries", "No call of this statement in this range", h("span", {}, "Try a longer range in the top bar, or ", h("a", { class: "wl-link", href: link("queries", { tab: "statements" }) }, "pick another statement"), ".")) })); return; }

    // The figures, against the window before.
    const prev = st?.prev;
    const was = (now, then, f = fmt.ms) => then == null ? null : `was ${f(then)}`;
    const kpis = h("div", { class: "grid g6 wl-kpis", style: { marginBottom: "14px" } },
      kpi("Calls", st ? [fmt.si(st.calls / secs), h("small", {}, "/s")] : "–", prev?.calls ? `${pctChange(st.calls, prev.calls)} vs before` : `${fmt.n(st?.calls)} ${sinceStart ? "since the server started" : "in the range"}`),
      kpi("p50", st ? fmtMs(st.p50_ms) : "–", was(st?.p50_ms, prev?.p50_ms)),
      kpi("p99", st ? fmtMs(st.p99_ms) : "–", was(st?.p99_ms, prev?.p99_ms), c?.p99X >= 2 ? "bad" : null),
      kpi("Failed on the server", sp ? [fmt.pct(sp.server / Math.max(1, sp.calls), 2).replace("%", ""), h("small", {}, "%")] : "–", sp ? failWords(sp, secs) : null, sp?.server / Math.max(1, sp?.calls) > 0.01 ? "bad" : null),
      kpi("Rows per call", st ? fmt.si(st.rows_per_call) : "–", prev?.rows != null && prev.calls ? `was ${fmt.si(prev.rows / prev.calls)}` : null),
      kpi("Share of query time", st?.share != null ? [fmt.pct(st.share, 0).replace("%", ""), h("small", {}, "%")] : "–", st?.total_ms != null ? `${fmt.ms(st.total_ms)} in all` : null));

    // The trend, with what happened around it.
    const evs = [];
    const markers = () => evs.map((e) => ({ t: e.ts_ms, label: `${when(e.ts_ms)} ${e.title}` }));
    const dom = () => [range.from, range.to ?? Date.now()];
    const hasP50 = (d.trend || []).some((p) => p.p50_ms != null);
    const lat = timeChart({ height: 170, domain: dom, fmt: fmt.ms, markers, label: "latency", series: [
      { name: d.trendNote ? "slowest call" : "p99", get: (p) => p.p99_ms, color: SERIES[6] },
      { name: hasP50 ? "p50" : "mean", get: (p) => p.p50_ms ?? p.mean_ms, color: SERIES[0] },
    ], threshold: prev?.p99_ms ? { value: prev.p99_ms, label: "p99 before" } : null });
    const step = d.step_ms || (d.trend.length > 1 ? d.trend[1].t - d.trend[0].t : 60e3);
    const callsC = timeChart({ height: 90, domain: dom, markers, label: "calls", fmt: fmt.si, series: [
      { name: `calls a ${step >= 3600e3 ? "hour" : step >= 60e3 * 2 ? fmt.dur(step / 1000).replace(" 0s", "") : "minute"}`, get: (p) => p.calls, area: true, color: SERIES[0] },
      { name: "failed", get: (p) => p.errors, color: "var(--crit)" },
    ] });
    lat.update(d.trend || []); callsC.update(d.trend || []);
    const trendCard = card({ title: "Latency and calls", sub: d.trendNote || `${fmt.dur(step / 1000)} points · events marked`, body: [lat, callsC, h("div", { class: "small muted wl-evline" })] });

    // Why it is slow, from the figures below.
    const why = whyItIs(st, d, c, slowSince);
    const help = helpers(st, d, sample, sql);
    const whyCard = card({ title: why.title, sub: "each line from the figures on this page", body: [
      h("div", { class: "wl-why" }, why.lines.map((l) => h("div", { class: "row nw wl-why-l" }, h("i", { class: "dot", style: { background: l.tone === "bad" ? "var(--crit)" : l.tone === "warn" ? "var(--warn)" : "var(--muted)" } }), h("div", { style: { minWidth: 0 } }, h("div", {}, l.text), l.sub ? h("div", { class: "small muted" }, l.sub) : null)))),
      help.length ? [h("div", { class: "small", style: { fontWeight: 600, margin: "12px 0 6px" } }, "What would help"), h("div", { class: "row", style: { gap: "6px" } }, help)] : null] });

    clear(body, kpis,
      h("div", { class: "wl-two", style: { marginBottom: "14px" } }, trendCard, whyCard),
      whereCard(st, d), h("div", { class: "wl-two-b", style: { margin: "14px 0" } }, whoCard(st, d, secs), slowestCard(d, sql)),
      d.errors?.length ? errorsCard(d) : null,
      aroundCard(range, evs, () => { lat.update(d.trend || []); callsC.update(d.trend || []); trendCard.querySelector(".wl-evline").textContent = evs.length ? `${fmt.plural(evs.length, "event")} in the range: the dotted lines` : ""; }));
  }
  reroute = (r2) => { const nfp = fpOf(r2); if (nfp && nfp !== fp) { fp = nfp; d = null; draw(); load(); } };
  draw();
  offs.push(poll(15000, load), bus.on("range", load), bus.on("narrow", draw));
  return () => { offs.forEach((f) => f()); reroute = null; };
}

function decode(x) { try { return decodeURIComponent(x); } catch { return x; } }
const fmtMs = (v) => { const t = fmt.ms(v); const m = /^([\d.]+) (\S+)$/.exec(t); return m ? [m[1], h("small", {}, m[2])] : t; };
function kpi(label, val, sub, tone) {
  return h("div", { class: "card kpi" + (tone === "bad" ? " wl-kpi-bad" : "") }, h("div", { class: "lbl" }, label), h("div", { class: "val" }, val), sub ? h("div", { class: "delta" + (tone === "bad" ? " up-bad" : ""), style: { maxWidth: "100%" } }, sub) : null);
}
const pctChange = (a, b) => { const x = (a - b) / Math.max(1, b); return (x >= 0 ? "+" : "−") + fmt.pct(Math.abs(x), 0); };
/** The failures that are not the server's, counted apart. */
function failWords(sp, secs) {
  const parts = [[sp.client, "client errors"], [sp.deadlines, "client deadlines"], [sp.refused, "refused"]].filter(([n]) => n);
  if (!parts.length) return sp.server ? `${fmt.si(sp.server / secs)}/s` : "none failed";
  return h("span", { title: parts.map(([n, w]) => `${w}: ${fmt.n(n)} calls, ${fmt.si(n / secs)}/s`).join("\n") }, "apart: " + parts.map(([n, w]) => `${w} ${fmt.si(n / secs)}/s`).join(", "));
}
function firstAbove(trend, v) { for (const p of trend || []) if (p.p99_ms > v) return p.t; return null; }
function keyShares(st) { const tot = (st?.keys || []).reduce((a, k) => a + k.calls, 0) || 1; return (st?.keys || []).map((k) => ({ key: k.key, share: k.calls / tot })); }
/** The SQL with a sample's literal values where the shape has `?` (LIMIT ?), so that it runs. */
function sqlWith(sql, sample) { return sample?.sql || sql.replace(/\bLIMIT \?/gi, "LIMIT 10").replace(/\bOFFSET \?/gi, "OFFSET 0"); }

/** The chain from symptom to cause, each line from a figure on the page. */
function whyItIs(st, d, c, slowSince) {
  const lines = [];
  if (!st) return { title: "Why", lines: [{ text: "The statement's figures are not in this range.", tone: "muted" }] };
  const sp = split(st.outcomes, st.calls);
  const prev = st.prev;
  const slow = c?.p99X >= 2;
  if (sp.server / Math.max(1, sp.calls) > 0.01) lines.push({ tone: "bad", text: `${fmt.pct(sp.server / sp.calls, 1)} of its calls fail on the server`, sub: d.errors?.[0] ? `mostly “${d.errors[0].message}”` : null });
  if (sp.client / Math.max(1, sp.calls) > 0.05) lines.push({ tone: "warn", text: `${fmt.pct(sp.client / sp.calls, 0)} of its calls are client errors (400)`, sub: d.errors?.[0] ? `“${d.errors[0].message}”: the client sends SQL the server cannot run; not a server fault` : "the client sends a request the server cannot run; not a server fault" });
  if (slow) lines.push({ tone: "bad", text: `p99 is ${fmt.ms(st.p99_ms)}${slowSince ? " since " + when(slowSince, { sec: false }) : ""} (${fmt.ms(prev.p99_ms)} before)`, sub: prev.p50_ms != null && st.p50_ms < prev.p50_ms * 1.5 ? "p50 has not moved: a few calls are slow, not all of them" : c.callsX < 1.5 ? "calls are flat: not more of it, each slower" : null });
  else if (prev?.p99_ms) lines.push({ tone: "muted", text: `p99 ${fmt.ms(st.p99_ms)}, ${fmt.ms(prev.p99_ms)} before`, sub: c ? `calls ×${c.callsX.toFixed(1)}` : null });
  if (c?.callsX >= 3) lines.push({ tone: "warn", text: `${c.callsX.toFixed(1)}× the calls of the window before`, sub: "more of it, rather than each slower" });
  // Where: a shard that holds it up, and its copy.
  const w = straggler(d), bs = d.by_shard || [], med = w?.med;
  if (w) {
      lines.push({ tone: "bad", text: `shard ${w.shard} answers it at p99 ${fmt.ms(w.p99_ms)}`, sub: `the other ${fmt.plural(bs.length - 1, "shard")} at ${fmt.ms(med)} (median)${w.last_share != null ? ` · last to answer in ${fmt.pct(w.last_share, 0)} of the calls` : ""}` });
      const cp = w.copy ? copyOf(w.copy) : null;
      if (w.copy) lines.push({ tone: "bad", text: [`on shard ${w.shard}, copy `, h("b", { class: "mono", title: copyTitle(w.copy) }, copyName(w.copy)), " is the slow one"], sub: cp ? siblings(w.shard, w.copy) : null });
      if (cp) {
        const ld = cp.detail?.load || {}, med2 = medianOf("segments");
        if (ld.segments != null && med2 && ld.segments > 3 * med2) lines.push({ tone: "warn", text: `${copyName(w.copy)} has ${fmt.n(ld.segments)} segments (the fleet's median is ${fmt.n(med2)})`, sub: "a search reads every segment" });
        if (ld.merge_failing) lines.push({ tone: "warn", text: `its merges fail`, sub: ld.disk_used_ratio != null ? `disk ${fmt.pct(ld.disk_used_ratio, 0)} used on ${copyName(w.copy)}` : null });
        else if (ld.disk_used_ratio > 0.9) lines.push({ tone: "warn", text: `disk ${fmt.pct(ld.disk_used_ratio, 0)} used on ${copyName(w.copy)}` });
      }
  }
  // Who: one key behind most of it.
  const ks = keyShares(st);
  if (ks.length && ks[0].share > 0.8 && st.calls > 10) lines.push({ tone: st.new ? "warn" : "muted", text: [`${fmt.pct(ks[0].share, 0)} of it comes from key `, h("b", { class: "mono" }, ks[0].key)], sub: st.new ? "a new statement from one client: contain it if it hurts the others" : null });
  if (st.rows_per_call > 1000) lines.push({ tone: "warn", text: `${fmt.si(st.rows_per_call)} rows a call`, sub: "the coordinator gathers and sorts them all: a smaller LIMIT, or a page by key, costs less" });
  if (sp.deadlines) lines.push({ tone: "muted", text: `${fmt.n(sp.deadlines)} calls passed the client's own deadline`, sub: "counted apart: the client asked for less time than the query takes" });
  if (!lines.length) lines.push({ tone: "muted", text: "Nothing stands out", sub: prev ? "its figures are as in the window before" : "and there is no window before to compare with" });
  return { title: slow ? "Why it is slow" : sp.server || sp.client ? "Why it fails" : "What stands out", lines };
}
function copyOf(url) { for (const sh of state.cluster?.shards || []) for (const r of sh.replicas || []) if (r.url === url) return r; return null; }
function medianOf(f) { const xs = []; for (const sh of state.cluster?.shards || []) for (const r of sh.replicas || []) { const v = r.detail?.load?.[f]; if (v != null) xs.push(v); } xs.sort((a, b) => a - b); return xs.length ? xs[Math.floor(xs.length / 2)] : null; }
function siblings(shard, url) {
  const sh = (state.cluster?.shards || []).find((x) => x.shard === shard);
  if (!sh) return null;
  const others = sh.replicas.filter((r) => r.url !== url);
  return others.length ? "its other copies, p99 over all queries: " + others.map((r) => `${copyName(r.url)} ${fmt.ms(r.detail?.load?.p99_ms ?? r.detail?.minute?.p99_ms)}`).join(", ") : null;
}
/** The shard that holds the statement up, if one does: {shard, p99_ms, copy, med}. */
function straggler(d) {
  const bs = (d.by_shard || []).slice().sort((a, b) => b.p99_ms - a.p99_ms);
  if (bs.length < 2) return null;
  const med = bs[Math.floor(bs.length / 2)].p99_ms, w = bs[0];
  return w.p99_ms > 3 * med && w.p99_ms > 20 ? { ...w, med } : null;
}
/** The next steps that fit what the figures say. */
function helpers(st, d, sample, sql) {
  const out = [];
  if (!st) return out;
  const sp = split(st.outcomes, st.calls), ks = keyShares(st), top = ks[0];
  const c = change(st), w = straggler(d);
  if (sp.client / Math.max(1, sp.calls) > 0.05) {
    if (can("admin")) out.push(h("button", { class: "btn sm", onclick: () => containDialog({ fingerprint: st.fingerprint, sql, keys: ks, key: top?.share > 0.8 ? top.key : null }) }, top?.share > 0.8 ? `Refuse it from ${top.key}…` : "Refuse it…"));
    const msg = d.errors?.[0]?.message;
    out.push(h("button", { class: "btn sm", title: "A summary to send the team that owns the client", onclick: () => copy(`Statement ${shortFp(st.fingerprint)} fails as a client error: ${fmt.n(sp.client)} of ${fmt.n(sp.calls)} calls ${timeRange().label || ""}${top ? `, ${fmt.pct(top.share, 0)} from API key ${top.key}` : ""}.\n${msg ? "Error: " + msg + "\n" : ""}SQL: ${st.fingerprint}\n${location.origin}/${link("statement/" + encodeURIComponent(st.fingerprint))}`, "Summary copied") }, icon("copy"), "Copy for the app team"));
  }
  if (w?.copy) {
    out.push(h("a", { class: "btn sm", href: copyLink(w.copy), title: copyTitle(w.copy) }, `Open ${copyName(w.copy)} in Cluster`));
    out.push(h("a", { class: "btn sm", href: nodeLink(w.copy), title: copyTitle(w.copy) }, icon("flame"), `Profile ${copyName(w.copy)}`));
  } else if (w) out.push(h("a", { class: "btn sm", href: shardLink(w.shard) }, `Open shard ${w.shard}`));
  if ((st.new || st.rows_per_call > 1000 || c?.callsX >= 3) && sp.client / Math.max(1, sp.calls) <= 0.05 && can("admin")) out.push(h("button", { class: "btn sm", onclick: () => containDialog({ fingerprint: st.fingerprint, sql, keys: ks, key: top?.share > 0.8 ? top.key : null }) }, top?.share > 0.8 ? `Limit it from ${top.key}…` : "Limit it…"));
  if (isCoord() && (w || c?.p99X >= 2)) out.push(h("a", { class: "btn sm", href: link("querypath", { sql: sample ? sqlWith(sql, sample) : sql, p: sample?.params ? JSON.stringify(paramsObj(sample.params)) : null }) }, sample?.params ? "Query path, with its params" : "Query path"));
  return out;
}

function whereCard(st, d) {
  const bs = (d.by_shard || []).slice().sort((a, b) => b.p99_ms - a.p99_ms);
  if (!bs.length) return card({ title: "Where", sub: "by shard and copy", body: h("div", { class: "small muted" }, isCoord() ? "This server does not split a statement by shard. A newer one does." : "It ran on this server.") });
  const top = bs.slice(0, 6), rest = bs.slice(6);
  const med = (xs, f) => { const v = xs.map(f).filter((x) => x != null).sort((a, b) => a - b); return v.length ? v[Math.floor(v.length / 2)] : null; };
  const rows = top.map((x) => ({ ...x, cp: x.copy ? copyOf(x.copy) : null }));
  const acts = (x) => [
      h("a", { class: "btn sm", href: x.copy ? copyLink(x.copy) : shardLink(x.shard) }, "Open"),
      x.copy ? h("a", { class: "btn sm", href: nodeLink(x.copy) }, "Profile") : null,
      h("a", { class: "btn sm", href: link("logs", { node: x.copy ? copyName(x.copy) : null, q: x.copy ? null : `shard ${x.shard}` }) }, "Logs")];
  const onRow = narrow() ? (x) => sheet(x.copy ? `Shard ${x.shard}, copy ${copyName(x.copy)}` : `Shard ${x.shard}`, [
    ["p99", fmt.ms(x.p99_ms)], ["calls", fmt.n(x.calls)], ["last in the fan-out", x.last_share != null ? `${fmt.pct(x.last_share, 0)} of the calls` : null], ["failed", fmt.n(x.errors || 0)],
    ["CPU", x.cp?.detail?.minute?.busy != null ? fmt.pct(x.cp.detail.minute.busy, 0) : null], ["segments", x.cp?.detail?.load?.segments], ["disk", x.cp?.detail?.load?.disk_used_ratio != null ? fmt.pct(x.cp.detail.load.disk_used_ratio, 0) : null],
    ["copy", x.copy ? h("span", { class: "mono small" }, x.copy) : "every copy"]], acts(x)) : null;
  const t = table(cols([
    { key: "shard", label: "shard", num: true },
    { key: "copy", label: "copy", render: (x) => x.copy ? h("span", { class: "row nw", style: { gap: "6px" } }, h("span", { class: "mono", title: copyTitle(x.copy) }, copyName(x.copy)), x.cp?.role ? h("span", { class: "badge" }, x.cp.role) : null) : h("span", { class: "muted" }, "every copy") },
    { key: "p99_ms", label: "p99", num: true, render: (x) => h("b", { class: x.p99_ms > 3 * med(bs, (y) => y.p99_ms) && x.p99_ms > 20 ? "wl-bad" : "" }, fmt.ms(x.p99_ms)) },
    { key: "calls", label: "calls", num: true, wide: true, render: (x) => fmt.si(x.calls) },
    { key: "last_share", label: "last in fan-out", num: true, wide: true, render: (x) => x.last_share != null ? fmt.pct(x.last_share, 0) : "–" },
    { key: "errors", label: "failed", num: true, wide: true, render: (x) => x.errors ? h("span", { class: "wl-bad" }, fmt.n(x.errors)) : "0" },
    { key: "cpu", label: "CPU", num: true, wide: true, sort: (x) => x.cp?.detail?.minute?.busy, render: (x) => x.cp?.detail?.minute?.busy != null ? fmt.pct(x.cp.detail.minute.busy, 0) : "–" },
    { key: "segments", label: "segments", num: true, wide: true, sort: (x) => x.cp?.detail?.load?.segments, render: (x) => x.cp?.detail?.load?.segments ?? x.cp?.segments ?? "–" },
    { key: "disk", label: "disk", num: true, wide: true, sort: (x) => x.cp?.detail?.load?.disk_used_ratio, render: (x) => { const v = x.cp?.detail?.load?.disk_used_ratio; return v != null ? h("span", { class: v > 0.95 ? "wl-bad" : "" }, fmt.pct(v, 0)) : "–"; } },
    { key: "act", label: "", sortable: false, wide: true, render: (x) => h("div", { class: "row nw", style: { gap: "4px" } }, acts(x)) },
  ]), { rows, sortKey: null, compact: true, onRow });
  return card({ title: "Where: by shard and copy", sub: `the shards this statement waited on, the slowest first${narrow() ? " · tap one for the rest" : ""}`, flush: true, body: [t,
    rest.length ? h("div", { class: "small muted", style: { padding: "8px 12px" } }, `${fmt.plural(rest.length, "other shard")}: p99 ${fmt.ms(med(rest, (y) => y.p99_ms))} (median), slowest ${fmt.ms(rest[0].p99_ms)}`) : null] });
}

function whoCard(st, d, secs) {
  const ks = d.by_key?.length ? d.by_key : st?.keys || [];
  if (!ks.length) return card({ title: "Who sends it", sub: "by API key", body: h("div", { class: "small muted" }, "This server does not record who sent a query. A newer one keeps the key, the app and the address of each.") });
  const tot = ks.reduce((a, k) => a + (k.calls || 0), 0) || 1;
  const containK = (k) => containDialog({ fingerprint: st?.fingerprint, sql: st?.sql, key: k.key, keys: ks.map((x) => ({ key: x.key, share: x.calls / tot })) });
  const onRow = narrow() ? (k) => sheet(`Key ${k.key}`, [["share", fmt.pct(k.calls / tot, 0)], ["calls/s", fmt.si(k.calls / secs)], ["p99", fmt.ms(k.p99_ms)], ["failed", k.errors ? fmt.pct(k.errors / Math.max(1, k.calls), 1) : "0"]],
    [h("a", { class: "btn sm", href: link("queries", { tab: "history", key: k.key }) }, "Its queries"), can("admin") ? h("button", { class: "btn sm", onclick: () => containK(k) }, "Contain…") : null]) : null;
  return card({ title: "Who sends it", sub: "by API key", flush: true, body: table(cols([
    { key: "key", label: "key", render: (k) => h("a", { class: "mono wl-link", href: link("queries", { tab: "history", key: k.key }) }, k.key) },
    { key: "calls", label: "share", num: true, render: (k) => fmt.pct(k.calls / tot, 0) },
    { key: "rate", label: "calls/s", num: true, sort: (k) => k.calls, render: (k) => fmt.si(k.calls / secs) },
    { key: "p99_ms", label: "p99", num: true, wide: true, render: (k) => fmt.ms(k.p99_ms) },
    { key: "errors", label: "failed", num: true, wide: true, title: "Every outcome but ok: server errors, client errors and deadlines", render: (k) => k.errors ? fmt.pct(k.errors / Math.max(1, k.calls), 1) : "0" },
    { key: "act", label: "", sortable: false, wide: true, render: (k) => can("admin") ? h("button", { class: "btn sm", onclick: () => containK(k) }, "Contain…") : null },
  ]), { rows: ks, sortKey: "calls", desc: true, compact: true, onRow }) });
}

function slowestCard(d, sql) {
  const xs = d.slowest || [];
  if (!xs.length) return card({ title: "Slowest calls", body: h("div", { class: "small muted" }, "No call kept in this range.") });
  const open = async (x) => {
    try {
      if (x.trace) return queryDrawer({ sql: x.sql || sql, fingerprint: d.statement?.fingerprint, ...x });
      if (x.id != null && !x.node) { const f = await api("/v1/queries/finished/" + x.id); return queryDrawer({ ...x, ...f }); }
      if (x.request_id && await hasLogStore()) {
        const j = await ask(`SELECT ts, node, request_id, kind, outcome, shard, sql, fingerprint, ms, n_rows, error, detail FROM queries WHERE request_id = '${String(x.request_id).replace(/'/g, "''")}' AND shard = false LIMIT 1`);
        const r = objects(j)[0];
        if (r) return queryDrawer({ ...(parseJson(r.detail) || {}), ...x, sql: r.sql, kind: r.kind, outcome: r.outcome, ms: r.ms, rows: r.n_rows, error: r.error, node: r.node, ts_ms: msOf(r.ts) });
      }
    } catch { /* the row itself */ }
    queryDrawer({ sql: x.sql || sql, ...x });
  };
  return card({ title: "Slowest calls", sub: narrow() ? "tap one for its trace, parameters and profile" : "with their parameters", flush: true, body: table(cols([
    { key: "ts_ms", label: "time", render: (x) => h("span", { class: "mono small nowrap" }, when(x.ts_ms)) },
    { key: "ms", label: "took", num: true, render: (x) => h("b", {}, fmt.ms(x.ms)) },
    { key: "node", label: "via", wide: true, render: (x) => x.node ? h("span", { class: "badge" }, x.node) : "–" },
    { key: "key", label: "key", render: (x) => x.key ? h("span", { class: "mono small nowrap" }, x.key) : h("span", { class: "muted" }, "–") },
    { key: "params", label: "params", wide: true, render: (x) => h("span", { class: "mono small ellipsis", style: { maxWidth: "150px", display: "inline-block", verticalAlign: "middle" }, title: paramsText(x.params, 2000) }, paramsText(x.params, 40) || "–") },
    { key: "waited", label: "waited on", wide: true, sort: (x) => x.waited_on?.ms, render: (x) => x.waited_on ? h("span", { class: "mono small nowrap", title: (x.waited_on.copy ? copyTitle(x.waited_on.copy) + ": " : "") + "the shard the answer waited on" }, `${x.waited_on.copy ? copyName(x.waited_on.copy) : "shard " + x.waited_on.shard} ${fmt.ms(x.waited_on.ms)}`) : x.outcome && x.outcome !== "ok" ? outcomePill(x.outcome) : "–" },
    { key: "act", label: "", sortable: false, wide: true, render: (x) => h("div", { class: "row nw", style: { gap: "4px" } },
      h("button", { class: "btn sm", onclick: () => open(x) }, "Trace ›"),
      h("a", { class: "btn ghost icon sm", href: profileLink(x.sql || sqlWith(sql, x), x.params), title: "Profile it again, with these parameters" }, icon("flame")),
      h("a", { class: "btn ghost icon sm", href: workbenchLink(x.sql || sqlWith(sql, x), x.params), title: "Open in the workbench, with these parameters" }, icon("sql"))) },
  ]), { rows: xs, sortKey: "ms", desc: true, compact: true, maxHeight: narrow() ? null : "330px", onRow: (x) => open(x) }) });
}

function errorsCard(d) {
  return card({ title: "Errors", sub: "by message", flush: true, attrs: { style: { marginBottom: "14px" } }, body: table(cols([
    { key: "message", label: "message", render: (e) => h("span", { class: "mono small" }, e.message) },
    { key: "count", label: "calls", num: true, render: (e) => fmt.n(e.count) },
    { key: "last_ms", label: "last", num: true, wide: true, render: (e) => when(e.last_ms) },
  ]), { rows: d.errors, sortKey: "count", desc: true, compact: true }) });
}

const EV_TONE = { critical: "bad", warning: "warn", info: "info" };
const EV_KIND = { alert_fired: "alert", alert_resolved: "alert", rollout: "change", upgrade: "change", change: "change", contain: "contain", primary_changed: "failover", copy_down: "copy", copy_up: "copy", majority_lost: "shard", majority_back: "shard", note: "note", log_level: "change", moves_started: "change", moves_finished: "change" };

/** What else happened in the range: events, alerts firing, logs. */
function aroundCard(range, evs, onEvents) {
  const box = h("div", { class: "stack", style: { gap: "6px" } }, h("div", { class: "small muted" }, "Reading the events…"));
  (async () => {
    let list = [];
    try { const j = await api(`/v1/events?${rangeQuery(range)}&limit=200`); list = j.events || []; } catch { list = null; }
    const alerts = firingAlerts();
    const rows = [];
    for (const a of alerts) rows.push(h("div", { class: "row nw wl-ev" }, h("span", { class: "pill " + (a.severity === "critical" ? "bad" : "warn") }, "firing"), h("span", { class: "mono small muted", style: { width: "64px" } }, when(a.since_ms, { sec: false })), h("span", { class: "grow" }, `Alert `, h("b", {}, a.name || a.id), a.since_ms ? " firing since then" : "", (a.where || []).length ? ` on ${(a.where || []).map((w) => w.short || copyName(w.copy) || w).slice(0, 3).join(", ")}` : ""), h("a", { class: "btn sm", href: link("alert/" + encodeURIComponent(a.id || a.rule || "")) }, "Open the alert")));
    for (const e of (list || []).filter((e) => e.kind !== "alert_fired" || !alerts.length).sort((a, b) => b.ts_ms - a.ts_ms).slice(0, 12)) {
      evs.push(e);
      rows.push(h("div", { class: "row nw wl-ev" }, h("span", { class: "pill " + (EV_TONE[e.severity] || "info") }, EV_KIND[e.kind] || e.kind), h("span", { class: "mono small muted", style: { width: "64px" } }, when(e.ts_ms, { sec: false })), h("span", { class: "grow ellipsis" }, e.title, e.who ? h("span", { class: "muted" }, ` · by ${e.who}`) : null),
        h("a", { class: "btn sm", href: link("timeline", { from: e.ts_ms - 10 * 60e3, to: e.ts_ms + 10 * 60e3 }) }, "On the Timeline")));
    }
    rows.push(h("div", { class: "row nw wl-ev" }, h("span", { class: "pill info" }, "logs"), h("span", { class: "grow" }, "The warnings and errors every server logged in this range"), h("a", { class: "btn sm", href: link("logs", { level: "warn" }) }, "Open in Logs")));
    clear(box, list == null ? h("div", { class: "small muted" }, "This server keeps no events: a newer one records rollouts, failovers, alerts and changes.") : null, rows);
    onEvents();
  })();
  return card({ title: "Around it", sub: "what else happened in this range", body: box });
}

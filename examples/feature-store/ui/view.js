// The feature store demo's page in ChloeDB's command center (#/app/features).
// The command center loads this module through chloedb-server, which
// forwards /v1/apps/features/... to the demo (featurestore.py), and runs
// render(el, ctx) with its own helpers (ctx.core, ctx.charts) and the
// demo's API (ctx.api). It imports nothing.
//
//   #/app/features            the pipeline, live: shoppers, features, models, serving
//   #/app/features/shop       the sample application: a shopper's ranking, explained; click to see features change
//   #/app/features/features   each feature's definition and spread; the tables, filtered
//   #/app/features/training   point-in-time training sets, models trained, compared, deployed
//   #/app/features/speed      serving and training-set timings, against Feast

export default { render };

const CSS = `
.fs-head { display: flex; gap: 10px 16px; align-items: center; flex-wrap: wrap; margin-bottom: 12px; }
.fs-head h2 { margin: 0; font-size: 18px; }
.fs-head .grow { flex: 1; }
.fs-kpis { display: grid; grid-template-columns: repeat(5, minmax(0, 1fr)); gap: 12px; margin-bottom: 14px; }
.fs-kpis .kpi { background: var(--surface); border: 1px solid var(--line); border-radius: var(--r); }
.fs-kpis .delta { max-width: none; }
.fs-pipe { width: 100%; height: auto; display: block; }
.fs-pipe .node rect { fill: var(--surface); stroke: var(--line-2); stroke-width: 1.2; }
.fs-pipe .node.k-stream rect { stroke: var(--s1); }
.fs-pipe .node.k-batch rect { stroke: var(--s4); }
.fs-pipe .node.k-train rect { stroke: var(--s3); }
.fs-pipe .node.k-serve rect { stroke: var(--s2); }
.fs-pipe .node .t { font-size: 13px; font-weight: 650; fill: var(--text); }
.fs-pipe .node .m { font-size: 11.5px; fill: var(--text-2); font-family: var(--mono); }
.fs-pipe .node .k { font-size: 10px; fill: var(--muted); text-transform: uppercase; letter-spacing: .05em; }
.fs-pipe .edge { fill: none; stroke: var(--axis); stroke-width: 1.6; }
.fs-pipe .flow { fill: none; stroke-width: 2.4; stroke-dasharray: 3 11; stroke-linecap: round; animation: fs-flow 1s linear infinite; }
.fs-pipe .flow.slow { animation-duration: 3.5s; }
.fs-pipe .flow.off { animation: none; opacity: .25; }
.fs-pipe .lbl { font-size: 10.5px; fill: var(--muted); }
@keyframes fs-flow { to { stroke-dashoffset: -14; } }
.fs-legend { display: flex; gap: 14px; flex-wrap: wrap; font-size: 12px; color: var(--text-2); margin-top: 6px; }
.fs-legend i { display: inline-block; width: 18px; height: 3px; border-radius: 2px; vertical-align: middle; margin-right: 6px; }
.fs-shop { display: grid; grid-template-columns: 290px minmax(0, 1fr); gap: 14px; align-items: start; }
.fs-who .big { font-size: 20px; font-weight: 650; }
.fs-who .truth { border: 1px dashed var(--line-2); border-radius: var(--r-sm); padding: 8px 10px; margin: 10px 0; font-size: 12.5px; }
.fs-who .truth b { color: var(--text); }
.fs-cats { display: grid; grid-template-columns: 70px minmax(0, 1fr) 54px; gap: 4px 8px; align-items: center; font-size: 12px; }
.fs-cats .bar { height: 8px; border-radius: 4px; background: var(--s1); }
.fs-cats .bg { height: 8px; border-radius: 4px; background: var(--surface-3); }
.fs-cats .like { font-weight: 650; color: var(--text); }
.fs-grid { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 12px; }
.fs-prod { border: 1px solid var(--line); border-radius: var(--r); background: var(--surface); padding: 10px 12px; display: flex; flex-direction: column; gap: 6px; position: relative; }
.fs-prod.liked { border-color: color-mix(in srgb, var(--s3) 60%, var(--line)); }
.fs-prod .rank { position: absolute; top: 8px; right: 10px; font-size: 11px; color: var(--muted); }
.fs-prod .ttl { font-weight: 650; padding-right: 18px; }
.fs-prod .meta { font-size: 12px; color: var(--text-2); display: flex; gap: 6px; align-items: center; flex-wrap: wrap; }
.fs-prod .sc { font-family: var(--mono); font-size: 12px; }
.fs-contrib { display: grid; grid-template-columns: 76px minmax(0, 1fr); gap: 2px 6px; align-items: center; font-size: 10.5px; color: var(--muted); }
.fs-contrib .axis { position: relative; height: 7px; background: var(--surface-2); border-radius: 3px; }
.fs-contrib .axis::after { content: ""; position: absolute; left: 50%; top: -1px; bottom: -1px; width: 1px; background: var(--axis); }
.fs-contrib .axis i { position: absolute; top: 0; bottom: 0; border-radius: 3px; }
.fs-wf { display: flex; height: 14px; border-radius: 4px; overflow: hidden; background: var(--surface-3); margin: 6px 0 4px; }
.fs-wf > i { display: block; height: 100%; }
.fs-note { border-left: 3px solid var(--s3); background: var(--surface-2); padding: 8px 12px; border-radius: 0 var(--r-sm) var(--r-sm) 0; font-size: 13px; margin-bottom: 12px; }
.fs-note.warn { border-left-color: var(--s2); }
.fs-note code { font-family: var(--mono); font-size: 12px; }
.fs-feat { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 12px; }
.fs-fc { border: 1px solid var(--line); border-radius: var(--r); background: var(--surface); padding: 10px 12px; }
.fs-fc .nm { font-family: var(--mono); font-weight: 650; }
.fs-fc .f { font-family: var(--mono); font-size: 11.5px; color: var(--text-2); margin: 4px 0 6px; overflow-wrap: anywhere; }
.fs-fc .what { font-size: 12px; color: var(--muted); min-height: 30px; }
.fs-hist { width: 100%; height: 54px; display: block; margin-top: 6px; }
.fs-hist rect { fill: var(--s1); }
.fs-fc.k-batch .fs-hist rect { fill: var(--s4); }
.fs-kind { font-size: 10.5px; text-transform: uppercase; letter-spacing: .05em; padding: 1px 7px; border-radius: 999px; font-weight: 650; }
.fs-kind.streaming { background: color-mix(in srgb, var(--s1) 16%, transparent); color: var(--s1); }
.fs-kind.batch { background: color-mix(in srgb, var(--s4) 18%, transparent); color: var(--s4); }
.fs-browse { display: grid; grid-template-columns: 180px minmax(0, 1fr) 200px auto; gap: 8px; margin-bottom: 10px; }
.fs-browse .input, .fs-browse .select { font-family: var(--mono); font-size: 12.5px; }
.fs-tl { width: 100%; height: 260px; display: block; }
.fs-tl .grid { stroke: var(--grid); }
.fs-tl .ax { font-size: 10.5px; fill: var(--muted); }
.fs-tl .step { fill: none; stroke: var(--s1); stroke-width: 2; }
.fs-tl .latest { stroke: var(--s2); stroke-width: 1.6; stroke-dasharray: 5 4; }
.fs-tl .pick { stroke: var(--s3); stroke-width: 1; opacity: .55; }
.fs-tl .imp { fill: var(--surface); stroke: var(--text-2); stroke-width: 1.2; }
.fs-tl .imp.c { fill: var(--s3); stroke: var(--s3); }
.fs-tl .snap { fill: var(--s1); }
.fs-sql2 { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
.fs-sql2 pre.code { font-size: 11.5px; white-space: pre-wrap; margin: 6px 0 10px; max-height: 260px; overflow: auto; }
.fs-auc { display: grid; grid-template-columns: 120px minmax(0, 1fr) 52px; gap: 6px 10px; align-items: center; font-size: 12.5px; }
.fs-auc .b { height: 12px; border-radius: 3px; }
.fs-auc .bg { background: var(--surface-3); border-radius: 3px; position: relative; height: 12px; }
.fs-auc .bg > i { position: absolute; left: 0; top: 0; bottom: 0; border-radius: 3px; }
.fs-vs { width: 100%; border-collapse: collapse; font-size: 13px; }
.fs-vs td, .fs-vs th { padding: 6px 8px; border-top: 1px solid var(--line); text-align: left; }
.fs-vs th { font-weight: 600; color: var(--text-2); font-size: 12px; }
.fs-vs td.num, .fs-vs th.num { text-align: right; font-variant-numeric: tabular-nums; }
.fs-vs .barcell { width: 40%; }
.fs-vs .barcell i { display: block; height: 10px; border-radius: 3px; }
.fs-flash { animation: fs-flash 1.6s ease-out; }
@keyframes fs-flash { from { background: color-mix(in srgb, var(--s3) 30%, transparent); } to { background: transparent; } }
@media (max-width: 1100px) { .fs-kpis { grid-template-columns: repeat(3, minmax(0, 1fr)); } .fs-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
@media (max-width: 900px) {
  .fs-kpis { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .fs-shop, .fs-sql2, .fs-browse { grid-template-columns: minmax(0, 1fr); }
}
@media (max-width: 560px) { .fs-grid { grid-template-columns: minmax(0, 1fr); } }
`;

const TABS = [["", "Pipeline"], ["shop", "Shop"], ["features", "Features"], ["training", "Training"], ["speed", "Speed"]];
const FEAT_COLORS = { u_ctr: "var(--s1)", i_ctr: "var(--s2)", i_trend: "var(--s4)", uc_ctr: "var(--s3)", uc_share: "var(--s5)", log_views: "var(--s6)", price_ratio: "var(--s7)" };

function render(el, ctx) {
  const { h } = ctx.core;
  el.append(h("style", {}, CSS));
  const head = h("div", { class: "fs-head" });
  const tabs = h("div", { class: "tabs", role: "tablist" });
  const body = h("div");
  el.append(head, tabs, body);
  let state = null;
  let stopPage = null;
  let listeners = [];
  const onState = (f) => { listeners.push(f); if (state) f(state); };

  // The header: what runs, and its controls.
  const pill = h("span", { class: "pill" });
  const modelChip = h("span", { class: "badge" });
  const pause = h("button", { class: "btn sm" });
  const rate = h("div", { class: "seg", title: "shopper sessions a second" });
  for (const r of [4, 12, 30]) {
    const b = h("button", { type: "button", "data-r": r, onclick: () => ctx.api("/api/sim", { method: "POST", body: { rate: r } }).then(poll) }, `${r}/s`);
    rate.append(b);
  }
  pause.onclick = () => ctx.api("/api/sim", { method: "POST", body: { running: !(state && state.running) } }).then(poll);
  head.append(h("h2", {}, "Feature store"), h("span", { class: "muted small" }, "a shop ranked by a model, on features ChloeDB keeps"), h("span", { class: "grow" }), pill, modelChip, rate, pause);
  const drawHead = (s) => {
    pill.className = "pill " + (s.error ? "warn" : s.running ? "ok" : "");
    pill.textContent = s.phase === "backfill" ? "making history…" : s.running ? `${ctx.core.fmt.si(s.rates.requests)} shoppers/s` : "paused";
    if (s.error) pill.title = s.error;
    modelChip.textContent = s.model ? `serving ${s.model.name}` : "serving random";
    pause.textContent = s.running ? "Pause shoppers" : "Resume shoppers";
    for (const b of rate.children) b.setAttribute("aria-pressed", String(Number(b.dataset.r) === s.rate));
  };
  listeners.push(drawHead);

  const show = (path) => {
    const tab = path[0] || "";
    for (const b of tabs.children) b.setAttribute("aria-selected", String(b.dataset.tab === tab));
    try { stopPage?.(); } catch (e) { console.warn(e); }
    listeners = [drawHead];
    body.replaceChildren();
    const page = { "": pipelinePage, shop: shopPage, features: featuresPage, training: trainingPage, speed: speedPage }[tab] || pipelinePage;
    stopPage = page(body, ctx, onState, poll);
  };
  for (const [id, label] of TABS) {
    tabs.append(h("button", { type: "button", role: "tab", "data-tab": id, onclick: () => { location.hash = id ? ctx.href(id) : ctx.href(); } }, label));
  }
  let timer = null, gone = false;
  async function poll() {
    try {
      state = await ctx.api("/api/state", { timeout: 8000 });
      for (const f of listeners) { try { f(state); } catch (e) { console.warn(e); } }
    } catch (e) {
      pill.className = "pill bad"; pill.textContent = "the demo does not answer"; pill.title = String(e.message || e);
    }
  }
  const loop = async () => { if (gone) return; await poll(); timer = setTimeout(loop, 1500); };
  ctx.onPath(show);
  show(ctx.path());
  loop();
  return () => { gone = true; clearTimeout(timer); try { stopPage?.(); } catch (e) { console.warn(e); } };
}

// ------------------------------------------------------------- helpers

function kpi(h, label, sub) {
  const val = h("div", { class: "val" }, "–"), delta = h("div", { class: "delta" }, sub || "");
  const e = h("div", { class: "kpi" }, h("div", { class: "lbl" }, label), val, delta);
  e.set = (v, d) => { val.replaceChildren(...(Array.isArray(v) ? v : [v])); if (d != null) delta.textContent = d; };
  return e;
}

function every(ms, f) {
  let t = null, gone = false;
  const go = async () => { if (gone) return; try { await f(); } catch (e) { console.warn(e); } if (!gone) t = setTimeout(go, ms); };
  go();
  return () => { gone = true; clearTimeout(t); };
}

/** Per model, the click rate over a sliding 40 s window, every 10 s. */
function ctrPoints(rows) {
  const byT = new Map(), names = new Map();
  for (const r of rows) {
    if (!byT.has(r.t)) byT.set(r.t, new Map());
    byT.get(r.t).set(r.name, [r.shown, r.clicked]);
    names.set(r.name, Math.max(names.get(r.name) || 0, r.t));
  }
  const ts = [...byT.keys()].sort((a, b) => a - b);
  const pts = ts.map((t) => {
    const p = { t };
    for (const n of names.keys()) {
      let s = 0, c = 0;
      for (const u of ts) if (u <= t && u > t - 40000) { const v = byT.get(u).get(n); if (v) { s += v[0]; c += v[1]; } }
      p[n] = s >= 40 ? c / s : null;
    }
    return p;
  });
  // The models seen most recently first; random always.
  const order = [...names.entries()].sort((a, b) => b[1] - a[1]).map((x) => x[0]);
  const keep = order.filter((n) => n !== "random").slice(0, 4);
  if (names.has("random")) keep.push("random");
  return { pts: pts.slice(0, -1), names: keep };
}

function ctrChart(ctx, host) {
  const { h } = ctx.core;
  let chart = null, key = "";
  const wrap = h("div");
  host.append(wrap);
  const draw = async () => {
    const r = await ctx.api("/api/ctr?minutes=15");
    const { pts, names } = ctrPoints(r.rows);
    const k = names.join(",");
    if (!chart || k !== key) {
      key = k;
      chart = ctx.charts.timeChart({ height: 210, fmt: (v) => ctx.core.fmt.pct(v, 1), min: 0, legend: true,
        threshold: r.random_ctr != null ? { value: r.random_ctr, label: "random ranking, all history:" } : null,
        series: names.map((n, i) => ({ name: n === "random" ? "random ranking" : n, color: n === "random" ? "var(--axis)" : ["var(--s3)", "var(--s1)", "var(--s5)", "var(--s4)"][i % 4], get: (p) => p[n], dash: n === "random" })) });
      wrap.replaceChildren(chart);
    }
    chart.update(pts);
  };
  return every(5000, draw);
}

function latencyChart(ctx, host, onState) {
  const chart = ctx.charts.timeChart({ height: 210, fmt: (v) => ctx.core.fmt.ms(v), min: 0,
    series: [
      { name: "serving p50 (4 reads by key, 489 rows, transforms, model)", color: "var(--s2)", get: (p) => p.serve_p50 },
      { name: "serving p99", color: "var(--s5)", get: (p) => p.serve_p99, dash: true },
      { name: "write p50 (impressions + streaming features, one transaction)", color: "var(--s1)", get: (p) => p.write_p50 },
      { name: "click to feature p50", color: "var(--s3)", get: (p) => p.fresh_p50 },
    ] });
  host.append(chart);
  onState((s) => chart.update(s.series));
}

// ------------------------------------------------------------- pipeline

const W = 170, HH = 66;
const NODES = {
  shoppers: { x: 20, y: 150, t: "Shoppers", k: "simulated", c: "" },
  imps: { x: 230, y: 44, t: "Impressions", k: "events + labels", c: "" },
  stream: { x: 440, y: 44, t: "Streaming transforms", k: "on write", c: "k-stream" },
  batch: { x: 440, y: 140, t: "Batch transform", k: "SQL every 10 s", c: "k-batch" },
  online: { x: 650, y: 44, t: "Online features", k: "4 tables, by key", c: "k-stream" },
  snaps: { x: 860, y: 44, t: "Snapshots", k: "every 2 s", c: "" },
  asof: { x: 860, y: 262, t: "Training set", k: "ASOF JOIN", c: "k-train" },
  train: { x: 650, y: 262, t: "Model training", k: "logistic regression", c: "k-train" },
  models: { x: 440, y: 262, t: "Models", k: "rows of fs_models", c: "k-train" },
  serve: { x: 230, y: 262, t: "Serving", k: "reads by key + model", c: "k-serve" },
};
// [from, to, colour, path, label, label x, label y]
const A = 77, C = 295; // the middles of the two rows
const EDGES = [
  ["shoppers", "imps", "s1", `M105,150 C105,${A} 160,${A} 230,${A}`],
  ["imps", "stream", "s1", `M400,${A} L440,${A}`],
  ["imps", "batch", "s4", `M400,${A} C422,${A} 418,173 440,173`],
  ["stream", "online", "s1", `M610,${A} L650,${A}`],
  ["batch", "online", "s4", `M610,173 C632,173 628,${A} 650,${A}`],
  ["online", "snaps", "s1", `M820,${A} L860,${A}`],
  ["snaps", "asof", "s3", `M945,110 L945,262`],
  ["imps", "asof", "s3", `M315,44 V22 H1052 V${C} H1030`, "labels: each impression's click, joined as of its time", 640, 16],
  ["asof", "train", "s3", `M860,${C} L820,${C}`],
  ["train", "models", "s3", `M650,${C} L610,${C}`],
  ["models", "serve", "s2", `M440,${C} L400,${C}`],
  ["online", "serve", "s2", `M735,110 V236 H315 V262`, "features by key", 520, 230],
  ["serve", "shoppers", "s2", `M230,${C} C105,${C} 105,${C - 20} 105,216`, "top 8", 150, 322],
];

function pipelineDiagram(ctx) {
  const { s } = ctx.core;
  const svg = s("svg", { class: "fs-pipe", viewBox: "0 0 1070 340", role: "img", "aria-label": "the feature pipeline" });
  const flows = {};
  for (const [a, b, c, d, lbl, lx, ly] of EDGES) {
    svg.append(s("path", { class: "edge", d }));
    const f = s("path", { class: "flow", d, style: `stroke: var(--${c})` });
    flows[a + ">" + b] = f;
    svg.append(f);
    if (lbl) svg.append(s("text", { class: "lbl", x: lx, y: ly, "text-anchor": "middle" }, lbl));
  }
  const metric = {};
  for (const [id, n] of Object.entries(NODES)) {
    const m = s("text", { class: "m", x: n.x + 12, y: n.y + 55 }, "–");
    metric[id] = m;
    svg.append(s("g", { class: "node " + n.c },
      s("rect", { x: n.x, y: n.y, width: W, height: HH, rx: 9 }),
      s("text", { class: "k", x: n.x + 12, y: n.y + 17 }, n.k),
      s("text", { class: "t", x: n.x + 12, y: n.y + 35 }, n.t), m));
  }
  svg.update = (st, extra) => {
    const f = ctx.core.fmt;
    const j = st.jobs || {};
    const on = st.running && st.phase === "live";
    metric.shoppers.textContent = `${f.si(st.rates.requests)} sessions/s`;
    metric.imps.textContent = `${f.si(st.counts.impressions)} · ${f.si(st.rates.impressions)}/s`;
    metric.stream.textContent = `write p50 ${f.ms(st.write.p50)}`;
    metric.batch.textContent = j.trend ? `${f.ms(j.trend.ms)} · ${f.ago(j.trend.t)}` : "–";
    metric.online.textContent = `fresh in ${f.ms(st.fresh.p50)}`;
    metric.snaps.textContent = `${f.si(st.counts.snap_rows)} rows · ${j.snapshot ? f.ms(j.snapshot.ms) : "–"}`;
    metric.asof.textContent = st.model ? `${f.si(st.model.train_rows)} rows · ${f.ms(st.model.build_ms)}` : "not built yet";
    metric.train.textContent = st.model ? `AUC ${st.model.auc_true?.toFixed(3)} · ${f.ms(st.model.train_ms)}` : "no model yet";
    metric.models.textContent = extra?.models != null ? `${extra.models} · ${st.model ? st.model.name : "random"}` : (st.model ? st.model.name : "random");
    metric.serve.textContent = `p50 ${f.ms(st.serve.p50)}`;
    // Training runs when asked: its edges move for 20 s after a model is made.
    const training = st.model && Date.now() - st.model.created < 20000;
    const trainEdges = new Set(["snaps>asof", "imps>asof", "asof>train", "train>models"]);
    for (const [k, p] of Object.entries(flows)) {
      const off = !on || (trainEdges.has(k) && !training) || ((k === "online>serve" || k === "models>serve") && !st.model);
      p.classList.toggle("off", off);
      p.classList.toggle("slow", k === "imps>batch" || k === "batch>online" || k === "online>snaps");
    }
  };
  return svg;
}

function pipelinePage(root, ctx, onState) {
  const { h, card, fmt } = ctx.core;
  const kRate = kpi(h, "Impressions written", "a row per product shown");
  const kServe = kpi(h, "Serving p50", "the shopper, 240 products, their categories");
  const kFresh = kpi(h, "Click to feature", "a click in the counters, p50");
  const kAuc = kpi(h, "Deployed model", "AUC on the last minute, as served");
  const kCtr = kpi(h, "Click rate, last 2 min", "");
  root.append(h("div", { class: "fs-kpis" }, kRate, kServe, kFresh, kAuc, kCtr));
  const note = h("div", { class: "fs-note" });
  const diag = pipelineDiagram(ctx);
  root.append(card({ title: "The pipeline, live", sub: "one ChloeDB server holds every box", body: [note, diag, h("div", { class: "fs-legend" },
    h("span", {}, h("i", { style: { background: "var(--s1)" } }), "streaming: on write"),
    h("span", {}, h("i", { style: { background: "var(--s4)" } }), "batch: SQL on a schedule"),
    h("span", {}, h("i", { style: { background: "var(--s3)" } }), "training: point in time"),
    h("span", {}, h("i", { style: { background: "var(--s2)" } }), "serving: reads by key"))] }));
  const charts = h("div", { class: "grid g2", style: { marginTop: "14px" } });
  const ctrHost = h("div"), latHost = h("div");
  charts.append(card({ title: "Click rate by model", sub: "from fs_impressions, a 40 s window", body: ctrHost }), card({ title: "Latency", sub: "every 5 s", body: latHost }));
  root.append(charts);
  const stopCtr = ctrChart(ctx, ctrHost);
  latencyChart(ctx, latHost, onState);
  let models = null, ctr2 = null;
  const stopModels = every(6000, async () => {
    const m = await ctx.api("/api/models");
    models = m.models.length;
    const c = await ctx.api("/api/ctr?minutes=2");
    const by = {};
    for (const r of c.rows) { const x = by[r.name] || (by[r.name] = [0, 0]); x[0] += r.shown; x[1] += r.clicked; }
    ctr2 = by; ctr2.base = c.random_ctr;
  });
  onState((st) => {
    kRate.set([fmt.si(st.counts.impressions), h("small", {}, `${fmt.si(st.rates.impressions)}/s`)], `${fmt.si(st.counts.clicks)} clicks, the labels`);
    kServe.set(fmt.ms(st.serve.p50), `p99 ${fmt.ms(st.serve.p99)}; 489 rows by key a request`);
    kFresh.set(fmt.ms(st.fresh.p50), `p99 ${fmt.ms(st.fresh.p99)}; the next ranking sees it`);
    kAuc.set(st.model ? st.model.auc_true.toFixed(3) : "random", st.model ? `${st.model.name}, ${st.model.join_kind}` : "train one on the Training tab");
    if (ctr2) {
      const cur = st.model ? st.model.name : "random";
      const a = ctr2[cur], b = ctr2.random;
      const r = a && a[0] ? a[1] / a[0] : null, rr = b && b[0] ? b[1] / b[0] : null;
      const base = rr != null ? rr : ctr2.base;
      kCtr.set(fmt.pct(r), cur === "random" ? "random ranking" : base != null ? `${cur}; random ranking ${fmt.pct(base)}${r && base ? `, ×${(r / base).toFixed(1)}` : ""}` : cur);
    }
    diag.update(st, { models });
    note.replaceChildren(st.model
      ? h("span", {}, "Shoppers see products ranked by ", h("b", {}, st.model.name), ` (${st.model.join_kind} training set). Every impression writes its streaming features in its own transaction; the batch job and the snapshots run beside it; the Training tab builds the next model from the same tables.`)
      : h("span", {}, "Shoppers see products in a random order, which is how the history was made. Train a model on the ", h("a", { href: ctx.href("training") }, "Training"), " tab and deploy it: the click rate below moves when it serves."));
  });
  return () => { stopCtr(); stopModels(); };
}

// ------------------------------------------------------------- shop

function contribBars(h, contrib) {
  const max = 1.2;
  const rows = [];
  for (const [k, v] of Object.entries(contrib)) {
    const w = Math.min(50, Math.abs(v) / max * 50);
    rows.push(h("span", {}, k), h("div", { class: "axis", title: `${k}: ${v >= 0 ? "+" : ""}${v.toFixed(3)} to the log-odds` },
      h("i", { style: { left: v >= 0 ? "50%" : `${50 - w}%`, width: `${w}%`, background: v >= 0 ? "var(--s3)" : "var(--s2)" } })));
  }
  return h("div", { class: "fs-contrib" }, rows);
}

function shopPage(root, ctx) {
  const { h, card, fmt } = ctx.core;
  let users = [], cats = [], uid = null, last = null;
  const pick = h("select", { class: "select", style: { width: "100%" } });
  const again = h("button", { class: "btn sm", onclick: () => { uid = users[Math.floor(Math.random() * users.length)].id; pick.value = uid; load(); } }, "Another shopper");
  const who = h("div", { class: "fs-who" });
  const grid = h("div", { class: "fs-grid" });
  const timing = h("div");
  const result = h("div");
  root.append(h("div", { class: "fs-note" }, "The sample application: the shop's ranking for one shopper, as the shoppers get it. Each card shows how much each feature pushed the product up or down. Click a product as this shopper: its impressions and the click are written in one transaction, and the shopper's features are read back at once."));
  root.append(h("div", { class: "fs-shop" },
    card({ title: "Shopper", tools: [again], body: [pick, who] }),
    h("div", {}, card({ title: "Ranked for them", sub: "top 8 of 240", body: [timing, result, grid] }))));
  pick.onchange = () => { uid = pick.value; load(); };

  async function load(flash) {
    const r = await ctx.api(`/api/shop?user=${encodeURIComponent(uid)}`);
    last = r;
    drawWho(r, flash);
    const ms = r.ms;
    const segs = [["fetch", "var(--s1)", "4 reads by key"], ["transform", "var(--s4)", "transforms"], ["score", "var(--s3)", "model"]];
    timing.replaceChildren(
      h("div", { class: "row between small" }, h("span", {}, `${r.model === "random" ? "random ranking (no model deployed)" : "model " + (r.model_name || r.model)} · ${fmt.ms(ms.total)} in all`),
        h("span", { class: "muted" }, segs.map(([k, , l]) => `${l} ${fmt.ms(ms[k])}`).join(" · "))),
      h("div", { class: "fs-wf" }, segs.map(([k, c, l]) => h("i", { style: { width: `${ms.total ? ms[k] / ms.total * 100 : 0}%`, background: c }, title: `${l}: ${fmt.ms(ms[k])}` }))));
    grid.replaceChildren(...r.items.map((e, i) => {
      const liked = r.user.likes.includes(e.category);
      const b = h("button", { class: "btn sm primary", onclick: () => click(e.id, b) }, "Click");
      return h("div", { class: "fs-prod" + (liked ? " liked" : "") },
        h("span", { class: "rank" }, `#${i + 1}`),
        h("div", { class: "ttl" }, e.title),
        h("div", { class: "meta" }, h("span", { class: "badge" }, e.category), `$${e.price.toFixed(2)}`, liked ? h("span", { class: "pill good" }, "a liked category") : null),
        e.score != null ? h("div", { class: "sc" }, `p(click) ${fmt.pct(e.score, 1)}`) : null,
        e.contrib ? contribBars(h, e.contrib) : null,
        h("div", { class: "row" }, b));
    }));
  }

  function drawWho(r, flash) {
    const u = r.user, on = r.online || {};
    const max = Math.max(1, ...cats.map((c) => (r.usercat[c] || {}).clicks || 0));
    who.replaceChildren(
      h("div", { class: "big", style: { marginTop: "10px" } }, u.name, h("span", { class: "muted small" }, "  " + u.id)),
      h("div", { class: "truth" }, h("div", { class: "muted" }, "The simulator's truth, hidden from the model:"), "likes ", h("b", {}, u.likes.join(" and ")), `, minds prices ${u.price_sens > 1 ? "a lot" : u.price_sens > 0.6 ? "some" : "little"}`),
      h("div", { class: "small muted", style: { marginBottom: "6px" } }, "What the features know (fs_user_online, fs_usercat_online):"),
      h("div", { class: "row small" + (flash ? " fs-flash" : ""), style: { marginBottom: "8px" } }, h("span", { class: "badge" }, `${fmt.n(on.views)} views`), h("span", { class: "badge" }, `${fmt.n(on.clicks)} clicks`),
        h("span", { class: "badge" }, on.clicks ? `clicked $${(on.click_price / on.clicks).toFixed(0)} on average` : "no clicks yet")),
      h("div", { class: "fs-cats" }, cats.flatMap((c) => {
        const x = r.usercat[c] || {};
        const cl = x.clicks || 0;
        return [h("span", { class: u.likes.includes(c) ? "like" : "" }, c), h("div", { class: "bg" }, h("div", { class: "bar", style: { width: `${cl / max * 100}%`, background: u.likes.includes(c) ? "var(--s3)" : "var(--s1)" } })),
          h("span", { class: "muted num" }, `${cl}/${x.views || 0}`)];
      })));
  }

  async function click(item, btn) {
    btn.disabled = true;
    const r = await ctx.api("/api/click", { method: "POST", body: { user: uid, shown: last.items.map((e) => e.id), item, model: last.model } });
    const b = r.before || {}, a = r.after || {}, bu = r.before_uc || {}, au = r.after_uc || {};
    result.replaceChildren(h("div", { class: "fs-note fs-flash" },
      `Wrote ${r.ops} operations (8 impressions, the click, the streaming features) in one transaction: ${fmt.ms(r.write_ms)}. Read back by key in ${fmt.ms(r.read_ms)}: `,
      h("code", {}, `views ${b.views ?? 0} → ${a.views}, clicks ${b.clicks ?? 0} → ${a.clicks}, ${r.category} clicks ${bu.clicks ?? 0} → ${au.clicks}`),
      ". The ranking below is the next one, with them."));
    await load(true);
  }

  ctx.api("/api/users").then((r) => {
    users = r.users; cats = r.categories;
    pick.replaceChildren(...users.map((u) => h("option", { value: u.id }, `${u.name} (${u.id}) · likes ${u.likes.join(", ")}`)));
    uid = users[5].id; pick.value = uid;
    load();
  });
  return () => {};
}

// ------------------------------------------------------------- features

function hist(ctx, counts) {
  const { s } = ctx.core;
  const max = Math.max(1, ...counts);
  const w = 100 / counts.length;
  return s("svg", { class: "fs-hist", viewBox: "0 0 100 40", preserveAspectRatio: "none" },
    counts.map((c, i) => s("rect", { x: i * w + 0.3, width: w - 0.6, y: 40 - c / max * 38, height: c / max * 38 })));
}

function featuresPage(root, ctx) {
  const { h, card, fmt, table } = ctx.core;
  const srcHost = h("div"), featHost = h("div", { class: "fs-feat" }), browseHost = h("div");
  root.append(card({ title: "Feature tables", sub: "what the transforms read; each a ChloeDB table, read by key when serving", body: srcHost, flush: true }));
  root.append(h("div", { style: { height: "14px" } }));
  root.append(card({ title: "Features the model reads", sub: "one definition each, for training and serving; spread over 3,000 (shopper, product) pairs as serving sees them now", body: featHost }));
  root.append(h("div", { style: { height: "14px" } }));
  root.append(card({ title: "View and filter", sub: "any feature table, snapshot or the impressions, with a WHERE", body: browseHost }));

  const draw = async () => {
    const r = await ctx.api("/api/features");
    srcHost.replaceChildren(table([
      { key: "table", label: "table", render: (x) => h("code", {}, x.table) },
      { key: "kind", label: "kind", render: (x) => h("span", { class: "fs-kind " + x.kind }, x.kind) },
      { key: "key", label: "key" },
      { key: "rows", label: "rows", num: true, render: (x) => fmt.n(x.rows) },
      { key: "columns", label: "columns", render: (x) => x.columns.join(", ") },
      { key: "how", label: "how it is kept", render: (x) => h("span", { class: "small" }, x.how, x.last ? h("span", { class: "muted" }, ` (last: ${fmt.ms(x.last.ms)}, ${fmt.ago(x.last.t)})`) : null) },
    ], { rows: r.sources, compact: true }));
    featHost.replaceChildren(...r.features.map((f) => h("div", { class: "fs-fc k-" + f.kind },
      h("div", { class: "row between" }, h("span", { class: "nm" }, f.name), h("span", { class: "fs-kind " + f.kind }, f.kind)),
      h("div", { class: "f" }, f.formula),
      h("div", { class: "what" }, f.what),
      hist(ctx, f.hist),
      h("div", { class: "row between small muted" }, h("span", {}, fmt.si(f.lo, 2)), h("span", {}, `mean ${fmt.si(f.mean, 3)}`), h("span", {}, fmt.si(f.hi, 2))),
      f.weight != null ? h("div", { class: "small", style: { marginTop: "4px" } }, `weight in the deployed model: `, h("b", { style: { color: f.weight >= 0 ? "var(--s3)" : "var(--s2)" } }, (f.weight >= 0 ? "+" : "") + f.weight.toFixed(3))) : null,
      f.cleaning ? h("div", { class: "small", style: { marginTop: "4px" } }, `cleaned: ${fmt.n(f.cleaning.missing)} of ${fmt.n(f.cleaning.n)} missing (no clicks yet) set to 0, ${fmt.n(f.cleaning.clipped)} clipped to ±2`) : null)));
  };
  draw();

  // Browse.
  const T = ["fs_user_online", "fs_item_online", "fs_usercat_online", "fs_item_trend", "fs_user_snap", "fs_item_snap", "fs_usercat_snap", "fs_impressions", "fs_items", "fs_models"];
  const EX = { fs_user_online: ["clicks >= 20", "clicks DESC"], fs_item_online: ["views > 0", "clicks DESC"], fs_usercat_online: ["id LIKE 'u005|%'", "clicks DESC"], fs_item_trend: ["clicks_2m > 3", "clicks_2m DESC"],
    fs_user_snap: ["usr = 'u005'", "ts DESC"], fs_item_snap: ["item = 'p010'", "ts DESC"], fs_usercat_snap: ["uc = 'u005|games'", "ts DESC"], fs_impressions: ["clicked = 1 AND price > 100", "ts DESC"], fs_items: ["category = 'audio'", "price"], fs_models: ["", "created DESC"] };
  const sel = h("select", { class: "select" }, T.map((t) => h("option", { value: t }, t)));
  const where = h("input", { class: "input", placeholder: "WHERE …" });
  const order = h("input", { class: "input", placeholder: "ORDER BY …" });
  const run = h("button", { class: "btn primary" }, "Run");
  const out = h("div");
  const setEx = () => { const e = EX[sel.value] || ["", ""]; where.value = e[0]; order.value = e[1]; };
  sel.onchange = () => { setEx(); go(); };
  setEx();
  const go = async () => {
    try {
      const r = await ctx.api("/api/browse", { method: "POST", body: { table: sel.value, where: where.value, order: order.value, limit: 30 } });
      const cols = r.columns.filter((c) => !["weights", "sql", "features"].includes(c));
      out.replaceChildren(h("div", { class: "row between small", style: { marginBottom: "6px" } }, h("code", {}, r.sql), h("span", { class: "muted" }, `${fmt.n(r.count)} rows match · ${fmt.ms(r.ms)}`)),
        table(cols.map((c) => ({ key: c, label: c, num: typeof r.rows[0]?.[c] === "number", render: (x) => x[c] == null ? "–" : typeof x[c] === "number" && !Number.isInteger(x[c]) ? fmt.si(x[c], 3) : c === "ts" || c === "created" ? fmt.time(x[c]) : String(x[c]) })), { rows: r.rows, compact: true, maxHeight: "360px" }));
    } catch (e) { out.replaceChildren(h("div", { class: "fs-note warn" }, String(e.message || e))); }
  };
  run.onclick = go;
  where.onkeydown = order.onkeydown = (e) => { if (e.key === "Enter") go(); };
  browseHost.append(h("div", { class: "fs-browse" }, sel, where, order, run), out);
  go();
  return () => {};
}

// ------------------------------------------------------------- training

function timelineSvg(ctx, r) {
  const { s, fmt } = ctx.core;
  const Wd = 1000, Ht = 260, M = { l: 44, r: 120, t: 14, b: 26 };
  const svg = s("svg", { class: "fs-tl", viewBox: `0 0 ${Wd} ${Ht}`, preserveAspectRatio: "none" });
  const imps = r.impressions, snaps = r.snaps;
  if (!imps.length) { svg.append(s("text", { x: Wd / 2, y: Ht / 2, "text-anchor": "middle", class: "ax" }, "no impressions for this shopper in the last minutes")); return svg; }
  const t0 = r.since, t1 = r.now;
  const latest = imps[0].latest_clicks ?? 0;
  const ymax = Math.max(1, latest, ...snaps.map((x) => x.clicks)) * 1.08;
  const ymin = Math.min(...snaps.map((x) => x.clicks), ...imps.map((x) => x.u_clicks ?? 0)) * 0.95;
  const X = (t) => M.l + (t - t0) / (t1 - t0) * (Wd - M.l - M.r), Y = (v) => Ht - M.b - (v - ymin) / Math.max(1, ymax - ymin) * (Ht - M.t - M.b);
  for (let i = 0; i <= 4; i++) {
    const v = ymin + (ymax - ymin) * i / 4;
    svg.append(s("line", { class: "grid", x1: M.l, x2: Wd - M.r, y1: Y(v), y2: Y(v) }), s("text", { class: "ax", x: M.l - 6, y: Y(v) + 3, "text-anchor": "end" }, fmt.si(v)));
  }
  for (let i = 0; i <= 6; i++) {
    const t = t0 + (t1 - t0) * i / 6;
    svg.append(s("text", { class: "ax", x: X(t), y: Ht - 8, "text-anchor": "middle" }, fmt.time(t)));
  }
  // The shopper's clicks as the snapshots saw them: a step line.
  let d = "", prev = null;
  for (const x of snaps) {
    if (x.ts < t0) { prev = x; continue; }
    if (!d) d = `M${X(t0)},${Y((prev || x).clicks)}`;
    d += ` L${X(x.ts)},${Y(prev ? prev.clicks : x.clicks)} L${X(x.ts)},${Y(x.clicks)}`;
    prev = x;
  }
  if (prev) d += ` L${X(t1)},${Y(prev.clicks)}`;
  svg.append(s("path", { class: "step", d }));
  svg.append(s("line", { class: "latest", x1: M.l, x2: Wd - M.r, y1: Y(latest), y2: Y(latest) }), s("text", { class: "ax", x: Wd - M.r + 6, y: Y(latest) + 3, style: "fill: var(--s2)" }, `latest: ${latest} clicks`));
  svg.append(s("text", { class: "ax", x: Wd - M.r + 6, y: Y(latest) + 16, style: "fill: var(--s2)" }, "what a plain join gives"), s("text", { class: "ax", x: Wd - M.r + 6, y: Y(latest) + 28, style: "fill: var(--s2)" }, "every row: the future"));
  for (const x of imps) {
    const c = s("circle", { class: "imp" + (x.label ? " c" : ""), cx: X(x.ts), cy: Y(x.u_clicks ?? 0), r: x.label ? 4.5 : 3 });
    c.append(s("title", {}, `${fmt.time(x.ts)} ${x.item} ${x.label ? "clicked" : "not clicked"}: the ASOF JOIN gave it the snapshot of ${fmt.time(x.u_ts)} (${x.u_clicks} clicks); the latest row says ${x.latest_clicks}`));
    svg.append(c);
  }
  return svg;
}

function trainingPage(root, ctx, onState) {
  const { h, card, fmt, table, sqlBlock, toast } = ctx.core;
  const tlHost = h("div"), tlTools = h("div", { class: "row" }), modelsHost = h("div"), aucHost = h("div"), sqlHost = h("div", { class: "fs-sql2" }), wHost = h("div");
  root.append(h("div", { class: "fs-note" }, "A training set needs each impression beside its features as they were just before it: the ", h("b", {}, "ASOF JOIN"), " takes, for each row, the latest snapshot at or before its time. Joining to the latest rows instead lets the label leak in: a clicked impression's own click is in the counters it is joined to. Train one of each and compare: the leaky model looks better offline and is worse on what serving sees."));
  root.append(card({ title: "Two ways to build the training set", body: sqlHost }));
  root.append(h("div", { style: { height: "14px" } }));
  root.append(card({ title: "Point in time, for one shopper", sub: "blue: their clicks in the snapshots; dots: impressions, 8 a request (green: clicked), each at the value the ASOF JOIN gave it; red: the one value a join to the latest rows gives them all", tools: tlTools, body: tlHost }));
  root.append(h("div", { style: { height: "14px" } }));
  root.append(h("div", { class: "grid g2" }, card({ title: "Offline AUC against the truth", sub: "on the last minute, held out", body: aucHost }), card({ title: "Weights", sub: "standardized features, the selected model", body: wHost })));
  root.append(h("div", { style: { height: "14px" } }));
  root.append(card({ title: "Models", sub: "rows of fs_models: weights, the SQL of the training set, timings, AUCs", body: modelsHost, flush: true }));

  let selected = null, models = [], deployed = null;
  const trainBtn = (leaky) => {
    const b = h("button", { class: "btn " + (leaky ? "" : "primary") }, leaky ? "Train on the latest rows (leaks)" : "Train on the ASOF JOIN");
    b.onclick = async () => {
      b.disabled = true; const t = b.textContent; b.textContent = "Training…";
      try { const m = await ctx.api("/api/train", { method: "POST", body: { leaky }, timeout: 300000 }); selected = m.id; toast(`${m.name}: ${fmt.n(m.train_rows)} rows in ${fmt.ms(m.build_ms)}, trained in ${fmt.ms(m.train_ms)}`, "ok"); await loadModels(); }
      catch (e) { toast(String(e.message || e), "bad"); }
      b.disabled = false; b.textContent = t;
    };
    return b;
  };
  ctx.api("/api/models").then((r) => {
    sqlHost.replaceChildren(
      h("div", {}, h("div", { class: "row between" }, h("b", {}, "As of each impression"), h("span", { class: "pill good" }, "point in time")), sqlBlock(r.asof_sql), trainBtn(false)),
      h("div", {}, h("div", { class: "row between" }, h("b", {}, "The latest rows"), h("span", { class: "pill bad" }, "leaks the label")), sqlBlock(r.leaky_sql), trainBtn(true)));
  });

  const aucRow = (label, v, color, note) => [h("span", {}, label), h("div", { class: "bg", title: note || "" }, h("i", { style: { width: `${Math.max(0, (v - 0.5) / 0.5 * 100)}%`, background: color } })), h("span", { class: "num" }, v == null ? "–" : v.toFixed(3))];
  const drawAuc = () => {
    const pick = [];
    const a = models.find((m) => m.join_kind === "asof"), l = models.find((m) => m.join_kind !== "asof");
    if (a) pick.push(a); if (l) pick.push(l);
    if (!pick.length) { aucHost.replaceChildren(h("div", { class: "muted" }, "No model yet: train one of each above.")); return; }
    aucHost.replaceChildren(...pick.map((m) => h("div", { style: { marginBottom: "12px" } },
      h("div", { class: "row", style: { marginBottom: "4px" } }, h("b", {}, m.name), h("span", { class: "pill " + (m.join_kind === "asof" ? "good" : "bad") }, m.join_kind)),
      h("div", { class: "fs-auc" },
        ...aucRow("training rows", m.auc_train, "var(--axis)"),
        ...aucRow("held out, own join", m.auc_holdout, m.join_kind === "asof" ? "var(--s3)" : "var(--s2)", "what this pipeline would report"),
        ...aucRow("held out, as served", m.auc_true, "var(--s1)", "the features serving had at the time: the truth")))),
      h("div", { class: "small muted" }, "AUC from 0.5 (chance) to 1. “As served” scores each model on the last minute with the features it would have had then, from the ASOF JOIN."));
  };
  const drawWeights = () => {
    const m = models.find((x) => x.id === selected) || models[0];
    if (!m) { wHost.replaceChildren(h("div", { class: "muted" }, "–")); return; }
    const max = Math.max(...Object.values(m.weights).map(Math.abs), 0.01);
    wHost.replaceChildren(h("div", { class: "small", style: { marginBottom: "6px" } }, h("b", {}, m.name), ` · ${fmt.n(m.train_rows)} rows`),
      h("div", { class: "fs-auc" }, Object.entries(m.weights).flatMap(([k, v]) => [h("code", {}, k), h("div", { class: "fs-contrib" }, h("div", { class: "axis", style: { gridColumn: "1 / -1" } }, h("i", { style: { left: v >= 0 ? "50%" : `${50 - Math.abs(v) / max * 50}%`, width: `${Math.abs(v) / max * 50}%`, background: v >= 0 ? "var(--s3)" : "var(--s2)" } }))), h("span", { class: "num" }, (v >= 0 ? "+" : "") + v.toFixed(2))])));
  };
  const loadModels = async () => {
    const r = await ctx.api("/api/models");
    models = r.models;
    deployed = models.find((m) => m.deployed)?.id || null;
    modelsHost.replaceChildren(table([
      { key: "name", label: "model", render: (m) => h("b", {}, m.name) },
      { key: "join_kind", label: "training set", render: (m) => h("span", { class: "pill " + (m.join_kind === "asof" ? "good" : "bad") }, m.join_kind) },
      { key: "created", label: "trained", render: (m) => fmt.time(m.created) },
      { key: "train_rows", label: "rows", num: true, render: (m) => fmt.n(m.train_rows) },
      { key: "build_ms", label: "join", num: true, render: (m) => fmt.ms(m.build_ms) },
      { key: "train_ms", label: "fit", num: true, render: (m) => fmt.ms(m.train_ms) },
      { key: "auc_holdout", label: "AUC, own join", num: true, render: (m) => m.auc_holdout?.toFixed(3) },
      { key: "auc_true", label: "AUC, as served", num: true, render: (m) => h("b", {}, m.auc_true?.toFixed(3)) },
      { key: "deployed", label: "", render: (m) => m.deployed ? h("span", { class: "pill ok" }, "serving") : h("button", { class: "btn sm", onclick: async () => { await ctx.api("/api/deploy", { method: "POST", body: { id: m.id } }); toast(`${m.name} serves now`, "ok"); loadModels(); } }, "Deploy") },
    ], { rows: models, compact: true, sortKey: "created", desc: true, onRow: (m) => { selected = m.id; drawWeights(); } }),
    h("div", { class: "row", style: { padding: "8px 12px" } }, h("button", { class: "btn sm", onclick: async () => { await ctx.api("/api/deploy", { method: "POST", body: { id: "random" } }); toast("random ranking serves now", "ok"); loadModels(); } }, "Serve random ranking"), h("span", { class: "small muted" }, "the Pipeline tab charts each model's click rate once it serves")));
    drawAuc(); drawWeights();
  };
  loadModels();

  // The timeline.
  let tuid = "u005";
  const sel = h("select", { class: "select sm" });
  const tlInfo = h("span", { class: "small muted" });
  tlTools.append(tlInfo, sel);
  sel.onchange = () => { tuid = sel.value; drawTl(); };
  ctx.api("/api/users").then((r) => {
    // The most active shoppers first: they have the most to show.
    sel.replaceChildren(...r.users.map((u) => h("option", { value: u.id }, `${u.name} (${u.id})`)));
    sel.value = tuid;
  });
  const drawTl = async () => {
    const r = await ctx.api(`/api/timeline?user=${tuid}`);
    tlInfo.textContent = `${r.impressions.length} impressions, ${r.snaps.length} snapshots · join ${fmt.ms(r.ms)}`;
    tlHost.replaceChildren(timelineSvg(ctx, r));
  };
  const stopTl = every(8000, drawTl);
  return () => stopTl();
}

// ------------------------------------------------------------- speed

function speedPage(root, ctx, onState) {
  const { h, card, fmt, toast } = ctx.core;
  const serveHost = h("div"), trainHost = h("div"), vsHost = h("div"), latHost = h("div");
  root.append(h("div", { class: "grid g2" },
    card({ title: "Serving under load", sub: "each request: 4 reads by key for 489 rows, 7 transforms, the model, the top 8", body: serveHost }),
    card({ title: "Building the training set", sub: "every impression with its features, median of 3", body: trainHost })));
  root.append(h("div", { style: { height: "14px" } }));
  root.append(card({ title: "Latency, live", body: latHost }));
  root.append(h("div", { style: { height: "14px" } }));
  root.append(card({ title: "Against Feast", sub: "measured, not live", body: vsHost }));
  latencyChart(ctx, latHost, onState);

  const runs = [];
  const seg = h("div", { class: "seg" });
  let clients = 4;
  for (const c of [1, 4, 16]) seg.append(h("button", { type: "button", "aria-pressed": String(c === clients), onclick: (e) => { clients = c; for (const b of seg.children) b.setAttribute("aria-pressed", String(b === e.currentTarget)); } }, `${c} client${c > 1 ? "s" : ""}`));
  const go = h("button", { class: "btn primary" }, "Run for 4 s");
  const res = h("div");
  go.onclick = async () => {
    go.disabled = true; go.textContent = "Running…";
    try { runs.unshift(await ctx.api("/api/bench/serve", { method: "POST", body: { clients, seconds: 4 }, timeout: 60000 })); drawRuns(); }
    catch (e) { toast(String(e.message || e), "bad"); }
    go.disabled = false; go.textContent = "Run for 4 s";
  };
  const drawRuns = () => {
    const max = Math.max(1, ...runs.map((r) => r.per_s));
    res.replaceChildren(h("table", { class: "fs-vs" }, h("tr", {}, h("th", {}, "clients"), h("th", { class: "num" }, "requests/s"), h("th", {}, ""), h("th", { class: "num" }, "p50"), h("th", { class: "num" }, "p99")),
      runs.map((r) => h("tr", {}, h("td", {}, r.clients), h("td", { class: "num" }, fmt.n(r.per_s)), h("td", { class: "barcell" }, h("i", { style: { width: `${r.per_s / max * 100}%`, background: "var(--s2)" } })), h("td", { class: "num" }, fmt.ms(r.p50)), h("td", { class: "num" }, fmt.ms(r.p99))))));
  };
  serveHost.append(h("div", { class: "row", style: { marginBottom: "10px" } }, seg, go, h("span", { class: "small muted" }, "the shoppers keep running meanwhile")), res);
  res.append(h("div", { class: "muted small" }, "Runs from this app's process (Python), over HTTP to the server, on this machine."));

  const tgo = h("button", { class: "btn primary" }, "Time it");
  const tres = h("div");
  tgo.onclick = async () => {
    tgo.disabled = true; tgo.textContent = "Joining…";
    try {
      const r = await ctx.api("/api/bench/train", { method: "POST", body: {}, timeout: 300000 });
      tres.replaceChildren(h("table", { class: "fs-vs" }, h("tr", {}, h("th", {}, "join"), h("th", { class: "num" }, "rows"), h("th", { class: "num" }, "time"), h("th", { class: "num" }, "rows/s")),
        [["ASOF JOIN, 3 snapshot tables", r.asof], ["LEFT JOIN to the latest, 4 tables", r.latest]].map(([n, x]) => h("tr", {}, h("td", {}, n), h("td", { class: "num" }, fmt.n(x.rows)), h("td", { class: "num" }, fmt.ms(x.ms)), h("td", { class: "num" }, fmt.si(x.rows / x.ms * 1000))))),
        h("div", { class: "small muted", style: { marginTop: "6px" } }, "Times include sending every row to this app as JSON."));
    } catch (e) { toast(String(e.message || e), "bad"); }
    tgo.disabled = false; tgo.textContent = "Time it";
  };
  trainHost.append(h("div", { class: "row", style: { marginBottom: "10px" } }, tgo), tres);

  ctx.api("/api/measured").then((m) => {
    const parts = [];
    const o = m.offline;
    if (o) {
      const rows = [["pandas merge_asof, in memory", o.pandas_merge_asof_s, "var(--axis)"], ["ChloeDB ASOF JOIN, columnar, over HTTP", o.chloedb_asof_s, "var(--s3)"], ["Feast get_historical_features, DuckDB", o.feast_duckdb_s, "var(--s2)"]];
      const max = Math.max(...rows.map((r) => r[1]));
      parts.push(h("b", {}, `Training set: ${fmt.n(o.spine_rows)} spine rows, features from ${fmt.n(o.user_rows)} and ${fmt.n(o.item_rows)} snapshot rows`),
        h("table", { class: "fs-vs", style: { margin: "6px 0 16px" } }, rows.map(([n, v, c]) => h("tr", {}, h("td", {}, n), h("td", { class: "barcell" }, h("i", { style: { width: `${v / max * 100}%`, background: c } })), h("td", { class: "num" }, `${v.toFixed(2)} s`)))));
    }
    const sv = m.serve;
    if (sv) {
      const cs = [1, 4, 16];
      const max = Math.max(...cs.map((c) => sv[`chloedb_kv@${c}`]?.per_s || 0));
      parts.push(h("b", {}, "Online: 10 features for 1,000 (user, item) pairs a request, over HTTP"),
        h("table", { class: "fs-vs", style: { marginTop: "6px" } }, h("tr", {}, h("th", {}, ""), h("th", { class: "num" }, "clients"), h("th", { class: "num" }, "requests/s"), h("th", {}, ""), h("th", { class: "num" }, "p50")),
          cs.flatMap((c) => [["ChloeDB /v1/kv/get", sv[`chloedb_kv@${c}`], "var(--s3)"], ["Feast feature server, Redis", sv[`feast_server_redis@${c}`], "var(--s2)"]].map(([n, x, col]) => x ? h("tr", {}, h("td", {}, n), h("td", { class: "num" }, c), h("td", { class: "num" }, fmt.n(x.per_s)), h("td", { class: "barcell" }, h("i", { style: { width: `${x.per_s / max * 100}%`, background: col } })), h("td", { class: "num" }, fmt.ms(x.p50_ms))) : null))));
    }
    parts.push(h("div", { class: "small muted", style: { marginTop: "8px" } }, `From ${m.source}. Same data and values on both sides.`));
    vsHost.replaceChildren(...(parts.length > 1 ? parts : [h("div", { class: "muted" }, "The measured results are not in this checkout.")]));
  });
  return () => {};
}

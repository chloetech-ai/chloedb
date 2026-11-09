// Simulator: a test run of the traffic simulator's mix (the profiles of
// deploy/compare/profiles, or weights of your own) against this cluster.
// The coordinator loads a table of test documents (strata_sim), measures
// each kind of request alone at two sizes, then sends the mix at a rising
// rate until the reads' p99 passes the SLO (or at a steady rate), and fits
// a model of the cluster to what it measured: a call's CPU by kind and by
// rows a shard, the coordinator's work, the copies' queues. The report
// grades the cluster against its SLO and a target rate, says what fills
// first and what it costs, and predicts other shapes: a copy more, a shard
// more, other machines, more rows, each by how much (POST
// /v1/admin/simulator/predict). Nothing here changes the cluster's shape.

import { h, icon, clear, fmt, card, api, toast, confirm, seg, pill, empty, tipOn, failure, store, can, needs, every, table } from "../core.js";
import { xyChart, barList } from "../charts.js";
import { isProduction } from "../data.js";

export default {
  id: "simulator", title: "Simulator", icon: "gauge", group: "Change",
  keywords: "simulator simulate test run load test benchmark traffic mix qps capacity throughput latency p99 slo replicas copies shards machines cost estimate what if predict model grade advice",
  render,
};

const KIND_NOTE = {
  "bm25": "words, ranked by BM25",
  "bm25 + filter": "BM25 within a year and four categories",
  "vector": "nearest vectors",
  "vector + filter": "nearest vectors rated 4 or more",
  "hybrid (RRF)": "BM25 and vectors, fused by rank",
  "hybrid (weighted)": "BM25 and vectors, weighted scores",
  "boosted (bm25 × decay)": "BM25 times recency",
  "facet": "counts by category of matching rows",
  "key lookup": "a row by its key",
  "phrase": "two words as a phrase, then all",
  "typo": "words with a typo, fuzzy",
  "prefix": "search as you type",
  "similar items": "items near an item, minus 20 seen",
  "sql aggregate": "GROUP BY over the structured fields",
};
const DOCS = [[20000, "20K"], [50000, "50K"], [100000, "100K"], [250000, "250K"], [1000000, "1M"]];
const DIMS = [[32, "32"], [64, "64"], [128, "128"], [256, "256"]];
const GRADE_TONE = { Healthy: "ok", Tight: "warn", Oversized: "info", "At risk": "warn", Undersized: "bad", "Too slow": "bad" };
const STATE_TONE = { ok: "ok", warn: "warn", bad: "bad", info: "info" };
const pct = (v, d = 0) => (v == null ? "–" : fmt.pct(v, d));
const money = (v) => (v == null ? "–" : "$" + fmt.n(Math.round(v)));
const signed = (v, f) => (v == null ? "–" : (v > 0 ? "+" : v < 0 ? "−" : "±") + f(Math.abs(v)));

function render(page, route) {
  const head = h("div"), main = h("div", { class: "sim" });
  page.append(head, main);

  let view = null, viewErr = null;   // GET /v1/admin/simulator
  let report = null, reportId = route?.params?.get("run") || null, reportErr = null;
  let draft = null;                  // the settings of the next run
  let mixOpen = false;
  let busy = false;
  let wi = null;                     // the what-if: {shards, copies, machine, docs, qps}
  let wiAns = null, wiErr = null, wiSeq = 0;
  let alive = true;
  let stopPoll = null;

  const running = () => view?.run?.state === "running";

  // ---------------------------------------------------------- loading
  async function load() {
    try { view = await api("/v1/admin/simulator"); viewErr = null; } catch (e) { viewErr = e; }
    if (!alive) return;
    if (view && !draft) draft = fromPreset(store.get("sim.draft", null) || { profile: "search-app" });
    if (view && !running()) {
      const want = reportId || view.reports.find((r) => r.state === "done")?.id || view.reports[0]?.id;
      if (want && report?.id !== want) await open(want, false);
    }
    draw();
  }
  async function open(id, redraw = true) {
    reportId = id;
    try { report = await api(`/v1/admin/simulator/runs/${encodeURIComponent(id)}`); reportErr = null; } catch (e) { report = null; reportErr = e; }
    wi = null; wiAns = null;
    if (report?.model?.base) {
      const b = report.model.base;
      wi = { shards: b.shape.shards, copies: b.shape.copies, machine: b.machine.name, docs: b.docs, qps: Math.round(report.config.target_qps || report.model.rate_for_latency) };
      predict();
    }
    if (redraw) draw();
  }
  function poll() {
    stopPoll?.();
    stopPoll = every(running() ? 1500 : 15000, async () => {
      const was = running();
      await load();
      if (was && !running() && view?.reports?.[0]) { await open(view.reports[0].id); toast(view.run?.state === "done" ? "The test run is done: its report is below" : `The test run ${view.run?.state}`, view.run?.state === "done" ? "ok" : "info"); }
      if (was !== running()) poll();
    }, false, { always: true });
  }

  // ---------------------------------------------------------- settings
  function preset(name) { return view?.presets?.find((p) => p.name === name) || view?.presets?.[0]; }
  function fromPreset(saved) {
    const d = { ...view.defaults, ...saved };
    const p = preset(d.profile);
    if (!saved.mix || !Object.keys(saved.mix).length) d.mix = { ...(p?.mix || {}) };
    if (d.write_share == null) d.write_share = p?.write_share ?? 0;
    if (d.slo_p99_ms == null) d.slo_p99_ms = p?.slo_p99_ms ?? 100;
    return d;
  }
  function setPreset(name) {
    const p = preset(name);
    draft = { ...draft, profile: name, mix: { ...(p?.mix || {}) }, write_share: p?.write_share ?? 0, slo_p99_ms: p?.slo_p99_ms ?? draft.slo_p99_ms };
    keep(); drawSetup();
  }
  function keep() { store.set("sim.draft", draft); }
  /** What a run takes, roughly: loading, each kind alone twice, the steps. */
  function estimate(d) {
    const kinds = Object.values(d.mix || {}).filter((w) => w > 0).length || 10;
    const load = d.docs / 9000 + 6;
    const alone = kinds * (d.alone_s + 0.5) * 2 + (d.write_share > 0 ? d.alone_s : 0);
    const traffic = d.shape === "steady" ? d.seconds + 2 : Math.min(d.max_steps, 14) * (d.step_s + 1.5);
    return load + alone + traffic + 3;
  }
  /** The body of POST /v1/admin/simulator: what differs from the preset is sent. */
  function body(d) {
    const p = preset(d.profile);
    const b = { profile: d.profile, docs: d.docs, dim: d.dim, shape: d.shape, slo_p99_ms: d.slo_p99_ms, keep: !!d.keep, label: d.label || null, seed: d.seed };
    const same = p && JSON.stringify(Object.entries(p.mix).filter(([, w]) => w > 0).sort()) === JSON.stringify(Object.entries(d.mix).filter(([, w]) => w > 0).sort());
    if (!same) b.mix = Object.fromEntries(Object.entries(d.mix).filter(([, w]) => w > 0));
    if (p?.write_share !== d.write_share) b.write_share = d.write_share;
    if (d.target_qps) b.target_qps = d.target_qps;
    if (d.estimate_docs) b.estimate_docs = d.estimate_docs;
    if (d.machine) b.machine = d.machine;
    if (d.shape === "steady") { b.rate = d.rate || 100; b.seconds = d.seconds; } else { b.step_s = d.step_s; b.max_steps = d.max_steps; if (d.start_qps) b.start_qps = d.start_qps; }
    return b;
  }
  async function start() {
    const d = draft;
    const total = Object.values(d.mix).reduce((a, w) => a + (w > 0 ? w : 0), 0);
    if (!(total > 0)) { toast("The mix needs a kind with a weight above 0", "bad"); return; }
    const mins = Math.max(1, Math.round(estimate(d) / 60));
    const prod = isProduction();
    const ok = await confirm({
      title: "Run a test against this cluster?",
      ok: "Start the test run",
      danger: prod,
      body: h("div", { class: "stack", style: { gap: "8px" } },
        h("p", { style: { margin: 0 } }, `It loads ${fmt.n(d.docs)} test documents into a table of its own (${view.table}), on every shard, then sends ${d.shape === "steady" ? `${fmt.n(d.rate || 100)} requests a second for ${fmt.n(d.seconds)} s` : "requests at a rising rate until the reads' p99 passes " + fmt.ms(d.slo_p99_ms)}. About ${mins} min.`),
        h("p", { class: "small dim", style: { margin: 0 } }, d.keep ? "The table is kept afterwards (drop it here when done)." : "The table is dropped afterwards. Other tables are not touched, but their reads share the cluster's cores while it runs."),
        prod ? h("div", { class: "sim-warn" }, icon("warn"), h("span", {}, "This cluster counts as production: the rising rate is meant to push it to its limit, so the traffic it serves will slow down for those minutes. Run it on a staging copy, or with a steady rate it has room for.")) : null),
      reason: prod ? "Why: kept in the audit log with the run" : false,
    });
    if (!ok) return;
    busy = true; drawSetup();
    try {
      await api("/v1/admin/simulator", { body: body(d), reason: ok.reason });
      report = null; reportId = null;
      await load(); poll();
      toast("The test run started", "ok");
    } catch (e) { toast(e.message, "bad"); }
    busy = false; draw();
  }
  async function stop() {
    try { await api("/v1/admin/simulator/stop", { method: "POST", body: {} }); toast("Stopping after the requests under way", "info"); } catch (e) { toast(e.message, "bad"); }
    load();
  }
  async function dropTable() {
    const ok = await confirm({ title: `Drop ${view.table}?`, body: "The test table a run kept goes from every shard.", ok: "Drop it", danger: true });
    if (!ok) return;
    try { const j = await api("/v1/admin/simulator/table", { method: "DELETE" }); toast(j.dropped ? "Dropped" : "There was no test table", "ok"); } catch (e) { toast(e.message, "bad"); }
  }

  // ---------------------------------------------------------- what-if
  async function predict() {
    if (!report || !wi) return;
    const seq = ++wiSeq;
    const shards = [1, 2, 3, 4, 6, 8, 12, 16];
    if (!shards.includes(wi.shards)) shards.push(wi.shards);
    shards.sort((a, b) => a - b);
    const copies = [1, 2, 3, 4, 5];
    if (!copies.includes(wi.copies)) copies.push(wi.copies);
    try {
      const j = await api("/v1/admin/simulator/predict", { body: { run: report.id, shards, copies: copies.sort((a, b) => a - b), machine: wi.machine, docs: wi.docs, qps: wi.qps, target_qps: report.config.target_qps, shape: { shards: wi.shards, copies: wi.copies } } });
      if (seq !== wiSeq) return;
      wiAns = j; wiErr = null;
    } catch (e) { if (seq !== wiSeq) return; wiErr = e; }
    if (alive) drawWhatIf();
  }
  const predictSoon = (() => { let t; return () => { clearTimeout(t); t = setTimeout(predict, 180); }; })();

  // ---------------------------------------------------------- drawing
  let setupBox, sideBox, bodyBox, whatIfBox;
  function draw() {
    if (!alive) return;
    drawHead();
    if (viewErr) { clear(main, failure(viewErr, "The Simulator")); return; }
    if (!view) { clear(main, empty("gauge", "Loading", "")); return; }
    if (!setupBox) {
      setupBox = h("div"); sideBox = h("div", { class: "stack" }); bodyBox = h("div", { class: "stack sim-body" });
      clear(main, h("div", { class: "sim-side stack" }, setupBox, sideBox), bodyBox);
    }
    drawSetup(); drawRuns(); drawBody();
  }

  function drawHead() {
    const tools = [];
    if (report) tools.push(h("button", { class: "btn sm", onclick: () => download(report) }, icon("download"), "Report JSON"));
    clear(head, h("div", { class: "page-head" },
      h("div", {}, h("h1", {}, "Simulator"),
        h("p", {}, "Send a test mix of searches at this cluster: what it holds within its SLO, what fills first, what it costs, and what more copies, more shards or other machines would change.")),
      h("div", { class: "tools" }, tools)));
  }

  function field(label, input, hint) {
    const l = h("label", { class: "f" }, label, input);
    if (hint) l.append(h("span", { class: "small muted", style: { fontWeight: 400 } }, hint));
    return l;
  }
  function num(value, onChange, attrs = {}) {
    const i = h("input", { class: "input", type: "number", value: value ?? "", ...attrs });
    i.oninput = () => { const v = i.value === "" ? null : Number(i.value); onChange(Number.isFinite(v) ? v : null); keep(); };
    return i;
  }
  function select(options, value, onChange, attrs = {}) {
    const sel = h("select", { class: "select", ...attrs }, options.map(([v, t]) => h("option", { value: String(v), selected: String(v) === String(value) }, t)));
    sel.onchange = () => { onChange(sel.value); keep(); };
    return sel;
  }

  function drawSetup() {
    if (!setupBox || !draft) return;
    const d = draft;
    const coordinator = view.coordinator;
    const p = preset(d.profile);
    const total = Object.values(d.mix).reduce((a, w) => a + (w > 0 ? w : 0), 0) || 1;
    const kinds = view.kinds.map((k) => k.kind);
    const mixRows = kinds.map((k) => {
      const w = d.mix[k] || 0;
      const inp = h("input", { class: "input sm", type: "number", min: 0, step: 0.01, value: w, "aria-label": `Weight of ${k}`, style: { width: "70px" } });
      inp.oninput = () => { d.mix[k] = Math.max(0, Number(inp.value) || 0); keep(); share.textContent = pct((d.mix[k] || 0) / (Object.values(d.mix).reduce((a, x) => a + (x > 0 ? x : 0), 0) || 1)); };
      const share = h("span", { class: "small dim num", style: { width: "36px" } }, w > 0 ? pct(w / total) : "–");
      const sql = view.kinds.find((x) => x.kind === k)?.sql;
      return h("div", { class: "sim-mixrow" + (w > 0 ? "" : " off") }, tipOn(h("span", { class: "grow", tabIndex: 0 }, h("b", {}, k), h("span", { class: "small muted" }, " " + (KIND_NOTE[k] || ""))), () => h("pre", { class: "code", style: { maxWidth: "460px", whiteSpace: "pre-wrap", margin: 0 } }, sql || k)), inp, share);
    });
    const ramp = d.shape !== "steady";
    const mins = Math.max(1, Math.round(estimate(d) / 60));
    const runBtn = h("button", { class: "btn primary", disabled: busy || running() || !can("admin"), title: can("admin") ? "" : needs("admin"), onclick: start }, icon("play"), running() ? "A run is under way" : "Run the test");
    clear(setupBox, card({
      title: "Test run", sub: p?.name ? `profile ${p.name}` : "",
      body: h("div", { class: "stack sim-form" },
        !coordinator ? h("div", { class: "sim-note small" }, icon("info"), h("span", {}, "This is a single server: the test measures it, and the model predicts clusters of it (a coordinator's own costs assumed).")) : null,
        field("Workload", seg((view.presets || []).map((x) => [x.name, x.name, x.description]), d.profile, setPreset, { "aria-label": "Workload profile" }), p?.description),
        h("div", { class: "sim-mix" },
          h("button", { class: "btn ghost sm sim-mixtoggle", "aria-expanded": String(mixOpen), onclick: () => { mixOpen = !mixOpen; drawSetup(); } }, icon(mixOpen ? "down" : "right"), "Mix of requests", h("span", { class: "small muted" }, ` ${Object.values(d.mix).filter((w) => w > 0).length} kinds, ${pct(d.write_share)} writes`)),
          mixOpen ? h("div", { class: "stack", style: { gap: "4px" } }, mixRows,
            h("div", { class: "sim-mixrow" }, h("span", { class: "grow" }, h("b", {}, "writes"), h("span", { class: "small muted" }, " inserts, updates, deletes")), num(Math.round((d.write_share || 0) * 1000) / 10, (v) => { d.write_share = Math.min(0.5, Math.max(0, (v || 0) / 100)); }, { class: "input sm", style: { width: "70px" }, step: 1, min: 0, max: 50, "aria-label": "Share of writes, percent" }), h("span", { class: "small dim", style: { width: "36px" } }, "%")),
            h("button", { class: "btn sm ghost", onclick: () => setPreset(d.profile) }, icon("reset"), "The profile's mix")) : null),
        h("div", { class: "sim-two" },
          field("Test documents", select(DOCS, d.docs, (v) => { d.docs = Number(v); drawSetup(); })),
          field("Vector size", select(DIMS, d.dim, (v) => { d.dim = Number(v); }))),
        field("Rate", seg([["ramp", "Rising, to find the limit"], ["steady", "Steady"]], d.shape, (v) => { d.shape = v; keep(); drawSetup(); }, { "aria-label": "Rate" })),
        ramp
          ? h("div", { class: "sim-two" },
            field("Each step, s", num(d.step_s, (v) => { d.step_s = v || 8; })),
            field("Steps at most", num(d.max_steps, (v) => { d.max_steps = v || 24; })))
          : h("div", { class: "sim-two" },
            field("Requests a second", num(d.rate ?? 100, (v) => { d.rate = v; })),
            field("For, s", num(d.seconds, (v) => { d.seconds = v || 60; }))),
        h("div", { class: "sim-two" },
          field("SLO: p99 of reads, ms", num(d.slo_p99_ms, (v) => { d.slo_p99_ms = v; })),
          field("Target rate, /s", num(d.target_qps, (v) => { d.target_qps = v; }, { placeholder: "none" }), "what it must hold")),
        h("div", { class: "sim-two" },
          field("Predict for rows", num(d.estimate_docs, (v) => { d.estimate_docs = v; }, { placeholder: "this cluster's" }), "the model scales to them"),
          field("Price on", select([["", "the nearest size"], ...view.machines.map((m) => [m.name, `${m.name} · ${money(m.usd_month)}/mo`])], d.machine || "", (v) => { d.machine = v || null; }))),
        field("Label", h("input", { class: "input", value: d.label || "", placeholder: "optional", oninput: (e) => { d.label = e.target.value; keep(); } })),
        h("label", { class: "row nw small dim", style: { gap: "8px" } }, h("input", { type: "checkbox", checked: !!d.keep, onchange: (e) => { d.keep = e.target.checked; keep(); } }), `Keep the table ${view.table} afterwards`)),
      foot: h("div", { class: "row between", style: { width: "100%" } }, h("span", { class: "small muted" }, `about ${mins} min`), runBtn),
    }));
  }

  function drawRuns() {
    if (!sideBox) return;
    const rs = view.reports || [];
    clear(sideBox, card({
      title: "Past runs", sub: rs.length ? `the last ${rs.length} kept` : "",
      tools: can("admin") ? h("button", { class: "btn sm ghost", title: `Drop ${view.table} (a run kept it)`, onclick: dropTable }, icon("trash")) : null,
      flush: true,
      body: rs.length ? h("div", { class: "sim-runs" }, rs.map((r) => h("button", { class: "sim-run" + (r.id === report?.id ? " on" : ""), onclick: () => open(r.id) },
        h("div", { class: "row between nw" }, h("b", {}, r.label || r.profile || r.id), r.grade ? pill(GRADE_TONE[r.grade] || "info", r.grade) : pill(r.state === "failed" ? "bad" : "disabled", r.state)),
        h("div", { class: "small muted" }, [fmt.ago(r.finished_ms || r.started_ms), r.cluster, r.capacity_qps != null ? `${r.bound ? "≥ " : ""}${fmt.si(r.capacity_qps)}/s` : null].filter(Boolean).join(" · "))))) : h("div", { class: "small muted", style: { padding: "0 14px 12px" } }, "None yet."),
    }));
  }

  function drawBody() {
    if (!bodyBox) return;
    if (view.run && (running() || (!report && view.run))) { clear(bodyBox, progress(view.run)); return; }
    if (reportErr) { clear(bodyBox, failure(reportErr, "The report")); return; }
    if (!report) {
      clear(bodyBox, card({ body: h("div", { class: "sim-intro" },
        icon("gauge", "lg"),
        h("h2", {}, "No test run yet"),
        h("p", {}, "A test run takes a few minutes. It loads a table of test documents (words by Zipf's law, topics, vectors near them), times each kind of request alone at a quarter of the rows and at all of them, then sends the mix at a rising rate. From that it fits a model of this cluster, and grades it."),
        h("ol", {},
          h("li", {}, "Pick a workload, or set the mix's weights yourself."),
          h("li", {}, "Set the SLO, and the rate the cluster must hold."),
          h("li", {}, "Run it, then try other shapes in the what-if: the model answers at once.")),
      ) }));
      return;
    }
    clear(bodyBox, reportView(report));
  }

  // ---------------------------------------------------------- progress
  function progress(run) {
    const steps = run.steps || [];
    const slo = draft?.slo_p99_ms || 100;
    const phases = h("ol", { class: "sim-phases" }, run.phases.map((p) => h("li", { class: "sim-phase " + p.state },
      h("span", { class: "sim-dot" }, p.state === "done" ? icon("check") : p.state === "running" ? h("span", { class: "sim-spin" }) : null),
      h("div", {}, h("div", {}, p.title), p.note ? h("div", { class: "small muted" }, p.note) : p.state === "running" ? h("div", { class: "small dim" }, run.text) : null))));
    const chart = steps.length ? stepsChart(steps, slo, null, null) : h("div", { class: "small muted", style: { padding: "30px 0", textAlign: "center" } }, "The rates show here once the mix starts.");
    const last = steps[steps.length - 1];
    return h("div", { class: "stack" },
      card({ title: run.state === "running" ? "Test run under way" : `Test run ${run.state}`, sub: [run.label, `started ${fmt.ago(run.started_ms)}`, run.by && run.by !== "open" ? `by ${run.by}` : null].filter(Boolean).join(" · "),
        tools: run.state === "running" ? h("button", { class: "btn sm", onclick: stop, disabled: !can("admin") }, icon("stop"), "Stop") : null,
        body: h("div", { class: "sim-prog" }, phases, h("div", { class: "grow", style: { minWidth: 0 } },
          last ? h("div", { class: "row", style: { gap: "18px", marginBottom: "6px" } },
            kv("Rate", fmt.rate(last.rate)), kv("p99", fmt.ms(last.p99_ms), last.holds ? "" : "bad"), kv("Errors", pct(last.error_share, 2)), kv("Coordinator", `${fmt.n(last.busy?.coordinator?.cores_used, 2)} cores`)) : null,
          chart, run.error ? h("div", { class: "error" }, run.error) : null)) }));
  }
  function kv(k, v, tone = "") { return h("div", { class: "sim-kv " + tone }, h("span", { class: "small muted" }, k), h("b", {}, v)); }

  /** Rate against p99: the steps measured, and the model's curves. */
  function stepsChart(steps, slo, curves, target) {
    // Past three times the SLO a step's p99 is drawn at the top, its value in the tooltip.
    const ymax = Math.ceil(Math.max(slo * 1.6, ...steps.map((s) => Math.min(s.p99_ms, slo * 3))) / 10) * 10;
    const at = (v) => (v == null ? null : Math.min(v, ymax));
    const series = [
      { name: "p99 measured", color: "var(--s1)", points: steps.map((s) => [s.rate, at(s.p99_ms), (s.p99_ms > ymax ? fmt.ms(s.p99_ms) + ", " : "") + (s.holds ? "held" : "missed")]) },
      { name: "p50 measured", color: "var(--s3)", points: steps.map((s) => [s.rate, at(s.p50_ms)]) },
    ];
    for (const c of curves || []) series.push({ ...c, points: c.points.map((p) => [p[0], at(p[1])]) });
    const xs = series.flatMap((s) => s.points.map((p) => p[0]));
    const xmax = Math.max(...xs, target || 0) * 1.02;
    series.push({ name: "SLO", color: "var(--crit)", dash: "4 4", points: [[0, slo], [xmax, slo]] });
    return xyChart({
      height: 230, series, min: 0, max: ymax, xmin: 0, xmax,
      xfmt: (x) => fmt.si(x), yfmt: (y) => fmt.ms(y), tipTitle: (x) => `${fmt.si(x)} requests a second`,
      marks: target ? [{ x: target, label: "target" }] : [],
      label: "p99 by rate",
    });
  }

  // ---------------------------------------------------------- the report
  function reportView(r) {
    if (r.error) {
      return card({ title: `Test run ${r.state}`, sub: fmt.datetime(r.finished_ms), body: h("div", { class: "stack" }, h("div", { class: "error" }, r.error), r.steps?.length ? stepsChart(r.steps, r.config?.slo_p99_ms || 100, null, null) : null) });
    }
    const g = r.grade, slo = r.settings.slo_p99_ms, target = r.config.target_qps;
    const base = r.model.base, bp = base.prediction;
    const tone = GRADE_TONE[g.label] || "info";
    const bandTone = tone === "bad" ? "bad" : tone === "ok" ? "ok" : "warn";
    const measured = r.measured;
    const atRate = r.model.rate_for_latency;
    const tested = r.model.tested;
    // The shape the model speaks of: the cluster as tested, or (its servers
    // sharing a machine) the same shape with a machine each.
    const main = r.model.main?.prediction || tested;
    const mainOn = r.model.main?.machine ? ` with one ${r.model.main.machine.name} each` : "";
    const busiest = [...r.steps].filter((s) => s.holds).sort((a, b) => b.rate - a.rate)[0] || r.steps[r.steps.length - 1];

    const verdict = h("div", { class: "banner sim-verdict " + bandTone },
      h("div", { class: "sim-score " + tone }, h("b", {}, String(g.score)), h("span", {}, "of 100")),
      h("div", { class: "grow" },
        h("div", { class: "row", style: { gap: "8px" } }, h("span", { class: "big" }, g.label), h("span", { class: "small muted" }, [r.label, r.cluster.text, fmt.datetime(r.finished_ms)].filter(Boolean).join(" · "))),
        h("div", {}, g.headline)));

    const kpis = h("div", { class: "grid g4 sim-kpis" },
      kpi("Holds within the SLO", measured.capacity_qps != null ? `${measured.bound ? "≥ " : ""}${fmt.si(measured.capacity_qps)}` : fmt.si(r.config.rate), "/s", measured.bound ? "no step missed: a lower bound" : `p99 ${fmt.ms(slo)}, measured`),
      kpi(target ? "p99 at the target" : "p99 at half of it", main.at?.p99_ms != null ? fmt.ms(main.at.p99_ms) : "over", "", `${fmt.si(atRate)}/s, by the model${mainOn}`),
      kpi("Fills first", { copies: "Copies' cores", coordinator: "Coordinator", writer: "Writers" }[main.bottleneck] || main.bottleneck, "", `saturates at ${fmt.si(main.saturation_qps)}/s${mainOn}`),
      kpi("Cost", money(base.cost_month), "/mo", `${base.shape.shards * base.shape.copies + base.shape.coordinators} × ${base.machine.name}`));

    const checks = h("div", { class: "sim-checks" }, g.checks.map((c) => h("div", { class: "advice " + (STATE_TONE[c.state] || "") },
      h("div", { class: "ic" }, icon(c.state === "ok" ? "okc" : c.state === "bad" ? "bad" : c.state === "warn" ? "warn" : "info")),
      h("div", {}, h("h4", {}, c.title), h("p", {}, c.text)))));

    // The model's curve over the rates the steps covered.
    const xTop = Math.max(...r.steps.map((s) => s.rate), target || 0) * 1.08;
    const modelCurve = r.model.curve?.length ? [{ name: r.model.main?.machine ? `p99 by the model, one ${r.model.main.machine.name} each` : "p99 by the model", color: "var(--s7)", dash: "6 3", points: r.model.curve.filter((p) => p[1] != null && p[0] <= xTop) }] : [];
    const scaled = r.model.kappa && Math.abs(r.model.kappa - 1) > 0.01;
    const chart = card({ title: "Rate and latency", sub: `each step ${fmt.n(r.config.shape === "steady" ? 5 : r.config.step_s)} s${scaled ? `; the model's cores scaled by ${fmt.n(r.model.kappa, 2)} to fit them` : ""}`,
      body: stepsChart(r.steps, slo, modelCurve, target) });

    whatIfBox = h("div");
    drawWhatIf();

    const adv = card({ title: "What would change it", sub: `on ${base.machine.name}, at ${fmt.si(base.docs)} rows, p99 at ${fmt.si(atRate)}/s`, flush: true,
      body: h("div", { class: "sim-advice" }, (r.advice || []).map((a) => adviceRow(a, true))) });

    const kinds = card({ title: "By kind", sub: "alone, one at a time, at the full test size; the CPU is the shards' a request", flush: true,
      body: table([
        { key: "kind", label: "Kind", render: (k) => h("span", {}, h("b", {}, k.kind), h("div", { class: "small muted" }, KIND_NOTE[k.kind] || "")) },
        { key: "share", label: "Share", num: true, render: (k) => pct(k.share) },
        { key: "p50", label: "p50 alone", num: true, sort: (k) => k.lat?.p50, render: (k) => fmt.ms(k.lat?.p50) },
        { key: "p99", label: "p99 alone", num: true, sort: (k) => k.lat?.p99, render: (k) => fmt.ms(k.lat?.p99) },
        { key: "busy", label: "p99 under load", num: true, sort: (k) => busiest?.kinds?.[k.kind]?.p99_ms, render: (k) => fmt.ms(busiest?.kinds?.[k.kind]?.p99_ms), title: `at ${fmt.si(busiest?.rate)}/s` },
        { key: "cpu_shard_ms", label: "CPU a request", num: true, render: (k) => fmt.ms(k.cpu_shard_ms) },
        { key: "doubling", label: "×2 rows", num: true, title: "a shard's work when it holds twice the rows", render: (k) => (k.doubling ? "×" + fmt.n(k.doubling, 2) : "–") },
        { key: "cpu_share", label: "Of the work", num: true, render: (k) => shareCell(k.cpu_share) },
      ], { rows: r.kinds, sortKey: "share", compact: true }) });

    const copies = busiest?.busy?.servers || [];
    const util = card({ title: "Where the work went", sub: busiest ? `cores used at ${fmt.si(busiest.rate)}/s, the busiest step that held` : "",
      body: h("div", { class: "stack" },
        barList([
          { name: "coordinator", value: busiest?.busy?.coordinator?.cores_used || 0, color: "var(--s7)", sub: `of ${fmt.n(busiest?.busy?.coordinator?.cores)} cores` },
          ...copies.map((c) => ({ name: `${c.name} (shard ${c.shard})`, value: c.cores_used || 0, color: "var(--s1)", sub: c.cores ? `of ${fmt.n(c.cores)} cores` : "" })),
          busiest?.busy?.load_generator ? { name: "the test's load generator", value: busiest.busy.load_generator.cores_used || 0, color: "var(--muted)", sub: "in the coordinator, not counted" } : null,
        ].filter(Boolean), { fmtv: (v) => fmt.n(v, 2) }),
        r.cluster.shared_host ? h("div", { class: "small muted" }, "These servers share machines: a copy's cores are its machine's, shared. The model gives each copy a machine of its own.") : null) });

    const steps = card({ title: "Steps", flush: true, body: table([
      { key: "i", label: "#", num: true, render: (s) => String(s.i + 1) },
      { key: "rate", label: "Rate", num: true, render: (s) => fmt.si(s.rate) },
      { key: "achieved", label: "Answered", num: true, render: (s) => fmt.si(s.achieved) },
      { key: "p50_ms", label: "p50", num: true, render: (s) => fmt.ms(s.p50_ms) },
      { key: "p99_ms", label: "p99", num: true, render: (s) => h("span", { class: s.p99_ms > slo ? "sim-bad" : "" }, fmt.ms(s.p99_ms)) },
      { key: "error_share", label: "Errors", num: true, render: (s) => pct(s.error_share, 2) },
      { key: "w", label: "Writes p99", num: true, sort: (s) => s.writes?.p99_ms, render: (s) => (s.writes?.n ? fmt.ms(s.writes.p99_ms) : "–") },
      { key: "holds", label: "", render: (s) => (s.holds ? pill("ok", "held") : pill("bad", "missed")) },
    ], { rows: r.steps, sortKey: "i", desc: false, compact: true }) });

    const notes = card({ title: "How the model works, and what it assumes", body: h("div", { class: "stack small dim", style: { gap: "6px" } },
      h("p", { style: { margin: 0 } }, "Each request is a call to every shard it needs (a search asks them all, a key lookup one), and each copy of a shard is a queue served by its cores. A call's CPU was measured by kind at two sizes, so it grows with the rows a shard holds as it grew between them; the coordinator's CPU grows with the shards it asks. Reads spread over a shard's copies, writes apply on every copy, by one writer a shard. A row added makes the next search on each copy rebuild the part of the memtable it changed (measured: a search right after an insert), so searches wait for it; copies do not cut that, rows written in batches do. The p99 comes from the calls' measured latency, waiting in the queues, and a search waiting for the slowest shard."),
      h("ul", { style: { margin: 0, paddingLeft: "18px" } }, (r.notes || []).map((n) => h("li", {}, n))),
      h("p", { style: { margin: 0 } }, `Prices: ${r.machines_source}.`)) });

    return h("div", { class: "stack" }, verdict, kpis, checks,
      h("div", { class: "grid g2" }, chart, util),
      whatIfBox, adv, kinds, steps, notes);
  }

  function kpi(label, value, unit, sub) {
    return h("div", { class: "card kpi" }, h("div", { class: "lbl" }, label), h("div", { class: "val" }, value, unit ? h("small", {}, unit) : null), h("div", { class: "delta", style: { maxWidth: "100%" } }, sub));
  }
  function shareCell(v) {
    if (v == null) return "–";
    return h("div", { class: "row nw", style: { gap: "6px", justifyContent: "flex-end" } }, h("div", { class: "barbox", style: { width: "48px" } }, h("div", { class: "bar", style: { width: Math.min(100, v * 100) + "%", background: "var(--s1)" } })), h("span", { class: "small dim num" }, pct(v)));
  }
  function adviceRow(a, tryIt) {
    const badge = (v, f, good, small) => (v == null || Math.abs(v) < small ? null : h("span", { class: "sim-delta " + ((v > 0) === good ? "good" : "bad") }, f(v)));
    const p99Small = Math.max(0.05, 0.02 * (a.p99_ms != null && a.p99_delta_ms != null ? a.p99_ms - a.p99_delta_ms : 0));
    return h("div", { class: "sim-adv" },
      h("div", { class: "grow" }, h("b", {}, a.title), h("div", { class: "small dim" }, a.text)),
      h("div", { class: "row nw sim-badges" },
        badge(a.capacity_delta, (v) => signed(v, (x) => pct(x)) + " room", true, 0.005),
        badge(a.p99_delta_ms, (v) => signed(v, fmt.ms) + " p99", false, p99Small),
        badge(a.cost_delta, (v) => signed(v, (x) => money(x)) + "/mo", false, 0.5),
        a.holds_target === true ? pill("ok", "holds the target") : a.holds_target === false ? pill("bad", "misses the target") : null,
        tryIt && a.shape ? h("button", { class: "btn sm ghost", title: "Try it in the what-if", onclick: () => { wi = { ...wi, shards: a.shape.shards, copies: a.shape.copies, machine: a.shape.machine }; predict(); whatIfBox?.scrollIntoView({ behavior: "smooth", block: "start" }); } }, icon("sliders")) : null));
  }

  function drawWhatIf() {
    if (!whatIfBox || !report?.model?.base || !wi) return;
    const r = report, slo = r.settings.slo_p99_ms, target = r.config.target_qps;
    const ans = wiAns, cell = ans?.detail?.cell, base = ans?.base;
    const stepper = (label, key, min, max) => h("div", { class: "sim-step" },
      h("span", { class: "small muted" }, label),
      h("div", { class: "row nw", style: { gap: "4px" } },
        h("button", { class: "btn sm", "aria-label": `Fewer ${label}`, disabled: wi[key] <= min, onclick: () => { wi[key] = Math.max(min, wi[key] - 1); drawWhatIf(); predictSoon(); } }, icon("minus")),
        h("b", { class: "num", style: { minWidth: "28px", textAlign: "center" } }, String(wi[key])),
        h("button", { class: "btn sm", "aria-label": `More ${label}`, disabled: wi[key] >= max, onclick: () => { wi[key] = Math.min(max, wi[key] + 1); drawWhatIf(); predictSoon(); } }, icon("plus"))));
    const machineSel = h("select", { class: "select sm", "aria-label": "Machine" }, r.machines.map((m) => h("option", { value: m.name, selected: m.name === wi.machine }, `${m.name} · ${m.vcpu} vCPU · ${money(m.usd_month)}/mo`)));
    machineSel.onchange = () => { wi.machine = machineSel.value; predict(); };
    const docsIn = h("input", { class: "input sm", type: "number", value: Math.round(wi.docs), style: { width: "110px" }, "aria-label": "Rows" });
    docsIn.onchange = () => { const v = Number(docsIn.value); if (v > 0) { wi.docs = v; predict(); } };
    const qpsIn = h("input", { class: "input sm", type: "number", value: wi.qps, style: { width: "90px" }, "aria-label": "Rate" });
    qpsIn.onchange = () => { const v = Number(qpsIn.value); if (v > 0) { wi.qps = v; predict(); } };
    const reset = h("button", { class: "btn sm ghost", onclick: () => { const b = r.model.base; wi = { shards: b.shape.shards, copies: b.shape.copies, machine: b.machine.name, docs: b.docs, qps: Math.round(r.config.target_qps || r.model.rate_for_latency) }; predict(); } }, icon("reset"), "This cluster");

    const cmp = (k, label, f, goodUp) => {
      const a = cell?.[k], b = base?.[k];
      const d = a != null && b != null ? a - b : null;
      return h("div", { class: "sim-cmp" }, h("span", { class: "small muted" }, label), h("b", {}, a == null ? (k === "p99_ms" ? "over" : "–") : f(a)),
        d != null && Math.abs(d) > 1e-9 ? h("span", { class: "sim-delta " + ((d > 0) === goodUp ? "good" : "bad") }, (d > 0 ? "+" : "−") + (k === "capacity_qps" && b > 0 ? pct(Math.abs(d) / b) : f(Math.abs(d)))) : h("span", { class: "small muted" }, "as now"));
    };
    const results = cell ? h("div", { class: "sim-cmps" },
      cmp("capacity_qps", "Holds within the SLO", (v) => fmt.si(v) + "/s", true),
      cmp("p99_ms", `p99 at ${fmt.si(wi.qps)}/s`, fmt.ms, false),
      cmp("p50_ms", `p50 at ${fmt.si(wi.qps)}/s`, fmt.ms, false),
      cmp("cost_month", "A month", money, false),
      h("div", { class: "sim-cmp" }, h("span", { class: "small muted" }, "Fills first"), h("b", {}, { copies: "copies' cores", coordinator: "coordinator", writer: "writers" }[cell.bottleneck] || cell.bottleneck), h("span", { class: "small muted" }, `${fmt.n(cell.nodes)} nodes`))) : null;

    const curves = [];
    if (ans?.base_curve?.length) curves.push({ name: "this cluster", color: "var(--s1)", points: ans.base_curve.filter((p) => p[1] != null && p[1] < slo * 3) });
    if (ans?.detail?.curve?.length && !(wi.shards === r.model.base.shape.shards && wi.copies === r.model.base.shape.copies && wi.machine === r.model.base.machine.name && Math.round(wi.docs) === Math.round(r.model.base.docs))) {
      curves.push({ name: `${wi.shards} × ${wi.copies} on ${wi.machine}`, color: "var(--s2)", points: ans.detail.curve.filter((p) => p[1] != null && p[1] < slo * 3) });
    }
    let chart = null;
    if (curves.length) {
      const xmax = Math.max(...curves.flatMap((c) => c.points.map((p) => p[0])), target || 0, wi.qps) * 1.02;
      // The p99 stays low until a shape fills, then climbs: the axis fits the curves, the SLO drawn when it is on it.
      const top = Math.max(...curves.flatMap((c) => c.points.map((p) => p[1])));
      const ymax = Math.min(slo * 1.6, Math.max(top * 1.3, slo * 0.2));
      const series = [...curves.map((c) => ({ ...c, points: c.points.map((p) => [p[0], Math.min(p[1], ymax)]) }))];
      if (slo <= ymax) series.push({ name: "SLO", color: "var(--crit)", dash: "4 4", points: [[0, slo], [xmax, slo]] });
      chart = h("div", {}, h("div", { class: "small muted" }, `p99 by rate, up to where each shape fills${slo > ymax ? ` (the SLO, ${fmt.ms(slo)}, is above the chart)` : ""}`), xyChart({ height: 200, series, min: 0, max: ymax, xmin: 0, xmax,
        xfmt: (x) => fmt.si(x), yfmt: fmt.ms, tipTitle: (x) => `${fmt.si(x)} requests a second`, marks: [{ x: wi.qps, label: "rate" }, ...(target && target !== wi.qps ? [{ x: target, label: "target" }] : [])], label: "p99 by rate, predicted" }));
    }

    clear(whatIfBox, card({
      title: "What if", sub: "the model of this run, for another shape: it answers at once, nothing changes",
      tools: reset,
      body: h("div", { class: "stack" },
        h("div", { class: "row sim-wi-controls" }, stepper("shards", "shards", 1, 64), stepper("copies a shard", "copies", 1, 8),
          h("label", { class: "sim-step" }, h("span", { class: "small muted" }, "machines"), machineSel),
          h("label", { class: "sim-step" }, h("span", { class: "small muted" }, "rows"), docsIn),
          h("label", { class: "sim-step" }, h("span", { class: "small muted" }, "rate, /s"), qpsIn)),
        wiErr ? h("div", { class: "error" }, wiErr.message) : null,
        results,
        h("div", { class: "grid g2" }, chart || h("div"), ans ? grid(ans) : h("div", { class: "small muted" }, "Asking the model…"))),
    }));
  }

  /** Shards by copies, each cell what it holds within the SLO; the cheapest that holds the target ringed. */
  function grid(ans) {
    const r = report, target = r.config.target_qps;
    const shards = [...new Set(ans.cells.map((c) => c.shards))].sort((a, b) => a - b);
    const copies = [...new Set(ans.cells.map((c) => c.copies))].sort((a, b) => a - b);
    const caps = ans.cells.map((c) => c.capacity_qps || 0);
    const lo = Math.log(Math.max(1, Math.min(...caps.filter((x) => x > 0)))), hi = Math.log(Math.max(2, ...caps));
    const shade = (v) => (v > 0 ? 1 + Math.round(((Math.log(v) - lo) / Math.max(1e-9, hi - lo)) * 6) : 0);
    const ok = (c) => (target ? c.capacity_qps >= target && c.p99_ms != null && c.p99_ms <= r.settings.slo_p99_ms : c.holds);
    const now = r.model.base.shape;
    // As the advice: two copies a shard at least, when the cluster has them.
    const least = Math.min(now.copies, 2);
    const cheapest = ans.cells.filter((c) => ok(c) && c.copies >= least).sort((a, b) => (a.cost_month ?? 1e12) - (b.cost_month ?? 1e12))[0];
    const t = h("table", { class: "sim-grid" },
      h("thead", {}, h("tr", {}, h("th", { class: "small muted" }, "shards ↓ copies →"), copies.map((c) => h("th", {}, String(c))))),
      h("tbody", {}, shards.map((s) => h("tr", {}, h("th", {}, String(s)), copies.map((c) => {
        const x = ans.cells.find((y) => y.shards === s && y.copies === c);
        if (!x) return h("td");
        const lv = shade(x.capacity_qps);
        const cls = ["sim-cell", `l${lv}`, ok(x) ? "" : "no", x === cheapest ? "best" : "", s === wi.shards && c === wi.copies ? "sel" : "", s === now.shards && c === now.copies && wi.machine === r.model.base.machine.name ? "now" : ""].filter(Boolean).join(" ");
        const td = h("td", {}, h("button", { class: cls, onclick: () => { wi.shards = s; wi.copies = c; predict(); } }, fmt.si(x.capacity_qps)));
        tipOn(td, () => h("div", {}, h("div", { class: "t" }, `${s} shards × ${c} copies on ${ans.machine.name}`),
          h("div", {}, `holds ${fmt.si(x.capacity_qps)}/s within p99 ${fmt.ms(r.settings.slo_p99_ms)}`),
          h("div", {}, `p99 at ${fmt.si(ans.qps)}/s: ${x.p99_ms == null ? "over" : fmt.ms(x.p99_ms)}`),
          h("div", {}, `${fmt.n(x.nodes)} nodes, ${money(x.cost_month)} a month`),
          h("div", { class: "small muted" }, `fills first: ${x.bottleneck}`)));
        return td;
      })))));
    return h("div", { class: "stack", style: { gap: "6px" } },
      h("div", { class: "small muted" }, `Requests a second each shape holds within p99 ${fmt.ms(r.settings.slo_p99_ms)}, on ${ans.machine.name}.`),
      h("div", { class: "tbl-wrap" }, t),
      h("div", { class: "row small muted", style: { gap: "12px" } },
        h("span", { class: "row nw", style: { gap: "4px" } }, h("span", { class: "sim-key now" }), "this cluster"),
        cheapest ? h("span", { class: "row nw", style: { gap: "4px" } }, h("span", { class: "sim-key best" }), `cheapest that holds ${target ? fmt.si(target) + "/s" : "the rate"}${least > 1 ? ` with ${least} copies a shard or more` : ""}: ${cheapest.shards} × ${cheapest.copies}, ${money(cheapest.cost_month)}/mo`) : null,
        target ? h("span", { class: "row nw", style: { gap: "4px" } }, h("span", { class: "sim-key no" }), "misses the target") : null));
  }

  function download(r) {
    const blob = new Blob([JSON.stringify(r, null, 2)], { type: "application/json" });
    const a = h("a", { href: URL.createObjectURL(blob), download: `chloedb-simulator-${r.id}.json` });
    document.body.append(a); a.click(); a.remove();
    setTimeout(() => URL.revokeObjectURL(a.href), 2000);
  }

  load().then(poll);
  return () => { alive = false; stopPoll?.(); };
}

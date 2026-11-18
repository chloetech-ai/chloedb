// Experiments, in ChloeDB's command center (#/app/experiments, loaded
// through chloedb-server from chloedb-experiments serve).
//
// A/B tests of a site's rankings on simulated users whose truth is known:
// every metric with its interval, p value, power and the effect it could
// have seen, the same with CUPED, the always-valid p value beside the one
// a peek gives, each cohort, and what an exploring log says of each arm
// without running it. Its paths:
//
//   (home)          the experiments, newest first
//   new[/<id>]      the builder (from an experiment's setup, to run again)
//   e/<id>[/<tab>]  one experiment: scorecard, metric, cohorts, what-if, setup
//   new/live        the builder, for a live run
//   live/<id>       a live run as it happens: every metric each second, the
//                   AI agents' screens, a search box of your own, the feed
//
// It imports nothing: ctx.core's h and s build every element (text goes in
// as text), and its charts are SVG drawn here.

export default { render };

const CSS = `
.ex-root { --ex-good: var(--good-ink, #006300); --ex-bad: var(--crit-ink, #b42323); --ex-flat: var(--muted); }
.ex-head { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; margin-bottom: 14px; }
.ex-head h1 { margin: 0; font-size: 20px; letter-spacing: -.01em; }
.ex-head .sub { color: var(--muted); font-size: 12.5px; }
.ex-head .tools { margin-left: auto; display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
.ex-list { display: grid; gap: 10px; }
.ex-item { display: grid; grid-template-columns: minmax(0, 1.4fr) minmax(0, 1fr) auto; gap: 14px; align-items: center; padding: 12px 14px; border: 1px solid var(--line); border-radius: var(--r); background: var(--surface); text-decoration: none; color: inherit; }
.ex-item:hover { border-color: var(--line-2); box-shadow: var(--shadow-1); }
.ex-item .nm { font-weight: 600; }
.ex-item .meta { color: var(--muted); font-size: 12px; margin-top: 2px; }
@media (max-width: 760px) { .ex-item { grid-template-columns: 1fr; } }
.ex-call { display: inline-flex; align-items: center; gap: 6px; font-weight: 600; font-size: 12.5px; padding: 2px 9px; border-radius: 999px; white-space: nowrap; }
.ex-call.ship, .ex-call.good { background: var(--good-soft); color: var(--good-ink); }
.ex-call.worse, .ex-call.bad { background: var(--crit-soft); color: var(--crit-ink); }
.ex-call.flat { background: var(--surface-3); color: var(--text-2); }
.ex-call.mixed { background: var(--warn-soft); color: var(--text); }
.ex-truth { font-size: 12px; color: var(--muted); }
.ex-truth b { color: var(--text-2); font-weight: 600; }
.ex-bar { height: 6px; border-radius: 3px; background: var(--surface-3); overflow: hidden; }
.ex-bar > div { height: 100%; background: var(--accent); transition: width .3s; }
.ex-intro { display: grid; grid-template-columns: minmax(0, 1.2fr) minmax(0, 1fr); gap: 18px; align-items: start; }
@media (max-width: 900px) { .ex-intro { grid-template-columns: 1fr; } }
.ex-intro p { margin: 0 0 10px; line-height: 1.5; }
.ex-steps { display: grid; gap: 8px; margin: 0; padding: 0; list-style: none; counter-reset: s; }
.ex-steps li { counter-increment: s; display: grid; grid-template-columns: 24px 1fr; gap: 8px; font-size: 13px; color: var(--text-2); }
.ex-steps li::before { content: counter(s); display: inline-grid; place-items: center; width: 22px; height: 22px; border-radius: 50%; background: var(--accent-soft); color: var(--accent); font-weight: 700; font-size: 12px; }
.ex-build { display: grid; grid-template-columns: minmax(0, 1fr) minmax(320px, .9fr); gap: 16px; align-items: start; }
@media (max-width: 1100px) { .ex-build { grid-template-columns: 1fr; } }
.ex-sites { display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: 10px; }
.ex-site { border: 1px solid var(--line); border-radius: var(--r-sm); padding: 10px 12px; cursor: pointer; background: var(--surface); }
.ex-site[aria-pressed="true"] { border-color: var(--accent); box-shadow: 0 0 0 3px var(--focus); }
.ex-site .about { font-size: 12px; color: var(--text-2); margin-top: 4px; line-height: 1.4; }
.ex-arm { border: 1px solid var(--line); border-left: 4px solid var(--arm); border-radius: var(--r-sm); padding: 10px 12px; display: grid; grid-template-columns: minmax(0, 1fr); gap: 8px; background: var(--surface); }
.ex-arm .ex-arm-top { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
.ex-arm textarea { width: 100%; font-family: var(--mono); font-size: 12px; min-height: 44px; resize: vertical; box-sizing: border-box; }
.ex-split { display: flex; height: 26px; border-radius: 6px; overflow: hidden; border: 1px solid var(--line); }
.ex-split > div { display: grid; place-items: center; color: #fff; font-size: 11.5px; font-weight: 600; white-space: nowrap; overflow: hidden; min-width: 0; }
.ex-knobs { display: grid; grid-template-columns: repeat(auto-fill, minmax(250px, 1fr)); gap: 14px 18px; }
.ex-knob label { display: flex; justify-content: space-between; gap: 8px; font-size: 12.5px; font-weight: 600; margin-bottom: 4px; }
.ex-knob label span { font-weight: 500; color: var(--text-2); font-variant-numeric: tabular-nums; }
.ex-knob input[type=range] { width: 100%; }
.ex-knob .hint { font-size: 11.5px; color: var(--muted); margin-top: 3px; line-height: 1.35; }
.ex-pv { display: grid; gap: 10px; }
.ex-pv .lists { display: grid; gap: 8px; }
.ex-pv ol { margin: 0; padding: 0; list-style: none; display: grid; gap: 3px; }
.ex-pv li { display: grid; grid-template-columns: 16px 1fr; gap: 6px; font-size: 12px; align-items: start; }
.ex-pv li .t { overflow: hidden; text-overflow: ellipsis; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; color: var(--text-2); }
.ex-dot { width: 12px; height: 12px; border-radius: 3px; margin-top: 2px; border: 1px solid var(--line-2); }
.ex-dot.g1 { background: var(--seq-3); border-color: var(--seq-3); }
.ex-dot.g2 { background: var(--seq-5); border-color: var(--seq-5); }
.ex-dot.g3, .ex-dot.g4 { background: var(--seq-7); border-color: var(--seq-7); }
.ex-verdicts { display: grid; gap: 10px; grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); margin-bottom: 14px; }
.ex-verdict { border: 1px solid var(--line); border-radius: var(--r); padding: 14px 16px; background: var(--surface); display: grid; gap: 8px; }
.ex-verdict .big { font-size: 28px; font-weight: 700; letter-spacing: -.02em; font-variant-numeric: tabular-nums; }
.ex-verdict .line { font-size: 12.5px; color: var(--text-2); line-height: 1.45; }
.ex-kpis { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 10px; margin-bottom: 14px; }
.ex-kpi { border: 1px solid var(--line); border-radius: var(--r-sm); padding: 8px 12px; background: var(--surface); }
.ex-kpi .k { font-size: 11.5px; color: var(--muted); }
.ex-kpi .v { font-size: 17px; font-weight: 650; font-variant-numeric: tabular-nums; }
.ex-score td, .ex-score th { vertical-align: middle; }
.ex-score .mname { font-weight: 600; }
.ex-score .mname small { display: block; font-weight: 400; color: var(--muted); font-size: 11px; }
.ex-num { font-variant-numeric: tabular-nums; white-space: nowrap; }
.ex-good { color: var(--ex-good); }
.ex-bad { color: var(--ex-bad); }
.ex-flatc { color: var(--ex-flat); }
.ex-tabs { display: flex; gap: 2px; border-bottom: 1px solid var(--line); margin-bottom: 14px; overflow-x: auto; }
.ex-tabs a { padding: 8px 12px; text-decoration: none; color: var(--text-2); font-weight: 500; border-bottom: 2px solid transparent; margin-bottom: -1px; white-space: nowrap; }
.ex-tabs a[aria-current="page"] { color: var(--text); border-bottom-color: var(--accent); }
.ex-chart { width: 100%; display: block; overflow: visible; }
.ex-chart text { font-size: 10.5px; fill: var(--muted); }
.ex-chart .ax { stroke: var(--axis); }
.ex-chart .gr { stroke: var(--grid); }
.ex-legend { display: flex; gap: 14px; flex-wrap: wrap; font-size: 12px; color: var(--text-2); margin-top: 6px; }
.ex-legend i { display: inline-block; width: 14px; height: 3px; border-radius: 2px; vertical-align: middle; margin-right: 5px; }
.ex-g2 { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 14px; }
@media (max-width: 1000px) { .ex-g2 { grid-template-columns: 1fr; } }
.ex-note { font-size: 12.5px; color: var(--text-2); line-height: 1.5; margin: 0 0 10px; }
.ex-heat { border-collapse: separate; border-spacing: 2px; font-size: 11.5px; }
.ex-heat td, .ex-heat th { padding: 6px 8px; text-align: center; border-radius: 4px; white-space: nowrap; }
.ex-heat th { color: var(--muted); font-weight: 600; }
.ex-heat td.c { color: #fff; font-weight: 600; font-variant-numeric: tabular-nums; }
.ex-heat td.c.ns { color: var(--text); }
.ex-chip-row { display: flex; gap: 6px; flex-wrap: wrap; }
.ex-live { display: inline-flex; align-items: center; gap: 6px; font-weight: 600; font-size: 12.5px; padding: 2px 9px; border-radius: 999px; background: var(--crit-soft); color: var(--crit-ink); white-space: nowrap; }
.ex-live::before { content: ""; width: 8px; height: 8px; border-radius: 50%; background: var(--crit); animation: ex-pulse 1.4s ease-in-out infinite; }
.ex-busy::after { content: ""; display: inline-block; width: 6px; height: 6px; margin-left: 6px; border-radius: 50%; background: var(--accent); animation: ex-pulse 1s ease-in-out infinite; vertical-align: middle; }
@keyframes ex-pulse { 0%, 100% { opacity: 1; transform: scale(1); } 50% { opacity: .3; transform: scale(.7); } }
@keyframes ex-in { from { background: var(--accent-soft); } to { background: transparent; } }
@media (prefers-reduced-motion: reduce) { .ex-live::before, .ex-busy::after, .ex-feed > div.new { animation: none; } }
.ex-clock { display: grid; grid-template-columns: 1fr auto; gap: 10px; align-items: center; margin-bottom: 14px; }
.ex-clock .ex-bar { height: 8px; }
.ex-clock .txt { font-size: 12.5px; color: var(--text-2); font-variant-numeric: tabular-nums; white-space: nowrap; }
.ex-rate { display: flex; align-items: center; gap: 8px; font-size: 12.5px; color: var(--text-2); }
.ex-rate input { width: 140px; }
.ex-agents { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(300px, 100%), 1fr)); gap: 12px; }
.ex-agent { border: 1px solid var(--line); border-top: 3px solid var(--arm); border-radius: var(--r); background: var(--surface); padding: 12px; display: grid; gap: 8px; align-content: start; min-width: 0; }
.ex-agent .who { display: flex; gap: 4px; flex-wrap: wrap; align-items: center; }
.ex-chip { font-size: 11px; padding: 1px 7px; border-radius: 999px; background: var(--surface-3); color: var(--text-2); white-space: nowrap; }
.ex-need { font-size: 12.5px; font-style: italic; line-height: 1.4; }
.ex-box { display: flex; align-items: center; gap: 6px; border: 1px solid var(--line-2); border-radius: 999px; padding: 3px 10px; font-size: 12.5px; min-height: 26px; min-width: 0; }
.ex-box .typed { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.ex-results { margin: 0; padding: 0; list-style: none; display: grid; gap: 2px; }
.ex-results li { display: grid; grid-template-columns: 16px 12px minmax(0, 1fr) auto; gap: 6px; align-items: start; font-size: 12px; padding: 3px 4px; border-radius: 4px; }
.ex-results li.opened { background: var(--accent-soft); }
.ex-results .t { overflow: hidden; text-overflow: ellipsis; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; color: var(--text-2); }
.ex-results .n { color: var(--muted); font-variant-numeric: tabular-nums; }
.ex-think { font-size: 12px; color: var(--text-2); border-left: 2px solid var(--line-2); padding-left: 8px; line-height: 1.4; }
.ex-trail { display: flex; gap: 4px; flex-wrap: wrap; font-size: 11px; color: var(--muted); }
.ex-feed { display: grid; max-height: 460px; overflow-y: auto; font-size: 12px; }
.ex-feed > div { display: grid; grid-template-columns: 40px 10px minmax(0, 1fr); gap: 8px; align-items: baseline; padding: 4px 2px; border-bottom: 1px solid var(--line); }
.ex-feed > div.new { animation: ex-in .8s ease-out; }
.ex-feed .tm { color: var(--muted); font-variant-numeric: tabular-nums; }
.ex-feed .d { width: 8px; height: 8px; border-radius: 50%; align-self: center; }
.ex-feed .tx { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-2); }
.ex-feed .tx b { color: var(--text); font-weight: 600; }
.ex-pick { display: flex; gap: 4px; flex-wrap: wrap; margin-bottom: 10px; }
.ex-pick button { font-size: 12px; padding: 3px 10px; border-radius: 999px; border: 1px solid var(--line); background: var(--surface); color: var(--text-2); cursor: pointer; }
.ex-pick button[aria-pressed="true"] { background: var(--accent-soft); color: var(--accent); border-color: var(--accent); font-weight: 600; }
.ex-doc { font-size: 12.5px; line-height: 1.5; max-height: 240px; overflow-y: auto; white-space: pre-wrap; border: 1px solid var(--line); border-radius: 6px; padding: 8px 10px; margin: 8px 0; }
.ex-g3 { display: grid; grid-template-columns: minmax(0, 1.6fr) minmax(0, 1fr); gap: 14px; }
@media (max-width: 1100px) { .ex-g3 { grid-template-columns: 1fr; } }
.ex-score tr.on td { background: var(--accent-soft); }
.ex-score tr.click { cursor: pointer; }
.ex-sql { font-family: var(--mono); font-size: 12px; background: var(--code); border-radius: 6px; padding: 6px 8px; overflow-x: auto; white-space: pre-wrap; word-break: break-word; }
`;

const ARM_COLORS = ["var(--s1)", "var(--s2)", "var(--s3)", "var(--s7)", "var(--s5)"];
const EXPLORE_COLOR = "var(--s4)";
const CALLS = {
  ship: ["ship", "Ship it"],
  worse: ["worse", "Worse"],
  "no detectable difference": ["flat", "No detectable difference"],
  "better, but a guardrail got worse": ["mixed", "Better, but a guardrail got worse"],
};
const USERS = [1000, 2000, 5000, 10000, 20000, 50000, 100000, 200000, 500000];
const RATES = [1, 2, 5, 10, 20, 50, 100, 200, 400];
const MINUTES = [1, 2, 5, 10, 15, 20, 30, 45, 60];
const BUDGETS = [0.25, 0.5, 1, 2, 5, 10, 20];
const SEARCHES = [10000, 50000, 100000, 200000, 400000, 1000000, 2000000, 5000000, 10000000, 20000000, 50000000];

function render(el, ctx) {
  const { core } = ctx;
  const { h } = core;
  if (!document.getElementById("ex-css")) document.head.append(h("style", { id: "ex-css" }, CSS));
  const root = h("div", { class: "ex-root" });
  el.append(root);
  let stops = [];
  let meta = null;
  const stopAll = () => { for (const f of stops) try { f(); } catch (e) { console.warn(e); } stops = []; };
  const go = (...parts) => { location.hash = ctx.href(...parts); };
  const loadMeta = async () => (meta ||= await ctx.api("/v1/meta"));

  async function draw(path) {
    stopAll();
    root.replaceChildren();
    try {
      await loadMeta();
      if (!path.length) await home(ctx, root, go, (f) => stops.push(f));
      else if (path[0] === "new") await builder(ctx, root, go, meta, path[1]);
      else if (path[0] === "live" && path[1]) await livePage(ctx, root, go, meta, path[1], (f) => stops.push(f));
      else if (path[0] === "e" && path[1]) await report(ctx, root, go, meta, path[1], path[2] || "scorecard", (f) => stops.push(f));
      else root.append(core.empty("box", "No such page", "", h("a", { class: "btn sm", href: ctx.href() }, "All experiments")));
    } catch (e) {
      root.append(core.failure(e, "Experiments"));
    }
  }
  draw(ctx.path());
  ctx.onPath(draw);
  return stopAll;
}

// ------------------------------------------------------------ helpers

const pctS = (v, d = 1) => (v == null || !Number.isFinite(v) ? "–" : (v > 0 ? "+" : v < 0 ? "−" : "") + Math.abs(v * 100).toFixed(d) + "%");
const signed = (v, d = 3) => (v == null || !Number.isFinite(v) ? "–" : (v > 0 ? "+" : v < 0 ? "−" : "") + fixed(Math.abs(v), d));
function fixed(v, d = 3) {
  if (v == null || !Number.isFinite(v)) return "–";
  const a = Math.abs(v);
  if (a >= 100) return v.toFixed(1);
  if (a >= 10) return v.toFixed(2);
  return v.toFixed(d);
}
function pText(p) {
  if (p == null || !Number.isFinite(p)) return "–";
  if (p < 1e-4) return "< 0.0001";
  if (p < 0.001) return p.toFixed(4);
  return p.toFixed(3);
}
const power = (p) => (p == null ? "–" : Math.round(p * 100) + "%");
/** good, bad or flat: a move of a metric, by its direction and whether its interval holds 0. */
function tone(m, lo, hi) {
  if (!(lo > 0 || hi < 0)) return "flat";
  return (lo > 0) === m.higher ? "good" : "bad";
}
const toneClass = (t) => (t === "good" ? "ex-good" : t === "bad" ? "ex-bad" : "ex-flatc");
const toneColor = (t) => (t === "good" ? "var(--good)" : t === "bad" ? "var(--crit)" : "var(--muted)");
function callChip(h, v) {
  const [cls, text] = CALLS[v.call] || ["flat", v.call];
  return h("span", { class: "ex-call " + cls }, text);
}
function unitValue(m, v) {
  if (v == null || !Number.isFinite(v)) return "–";
  if (m.unit === "share of searches") return (v * 100).toFixed(2) + "%";
  if (m.unit === "ms") return v.toFixed(3) + " ms";
  if (m.unit === "seconds") return v.toFixed(2) + " s";
  return fixed(v, 3);
}
function unitDelta(m, v) {
  if (v == null || !Number.isFinite(v)) return "–";
  if (m.unit === "share of searches") return (v > 0 ? "+" : v < 0 ? "−" : "") + Math.abs(v * 100).toFixed(2) + " pts";
  if (m.unit === "ms") return signed(v, 3) + " ms";
  if (m.unit === "seconds") return signed(v, 2) + " s";
  return signed(v, 4);
}

/** A horizontal interval on a shared axis, with the truth as a diamond: [lo, hi] around `mid`. */
function forest(core, { lo, hi, mid, truth, min, max, color, width = 180, height = 22, zero = 0 }) {
  const { s } = core;
  const x = (v) => 6 + ((v - min) / (max - min || 1)) * (width - 12);
  const y = height / 2;
  const g = s("svg", { class: "ex-chart", viewBox: `0 0 ${width} ${height}`, width, height, style: { width: width + "px" } });
  if (zero >= min && zero <= max) g.append(s("line", { x1: x(zero), x2: x(zero), y1: 2, y2: height - 2, class: "ax", "stroke-dasharray": "2 2" }));
  if (Number.isFinite(lo) && Number.isFinite(hi)) {
    g.append(s("line", { x1: x(Math.max(lo, min)), x2: x(Math.min(hi, max)), y1: y, y2: y, style: { stroke: color, strokeWidth: 3, strokeLinecap: "round" } }));
    g.append(s("circle", { cx: x(Math.min(Math.max(mid, min), max)), cy: y, r: 4, style: { fill: color } }));
  }
  if (truth != null && Number.isFinite(truth) && truth >= min && truth <= max) {
    const t = x(truth);
    g.append(s("path", { d: `M${t} ${y - 6} L${t + 5} ${y} L${t} ${y + 6} L${t - 5} ${y} Z`, style: { fill: "none", stroke: "var(--text)", strokeWidth: 1.5 } }, s("title", {}, "the truth")));
  }
  return g;
}

/** An axis domain around values, symmetric about 0 when they straddle it. */
function domain(vals) {
  const v = vals.filter(Number.isFinite);
  if (!v.length) return [-1, 1];
  let lo = Math.min(0, ...v), hi = Math.max(0, ...v);
  const pad = (hi - lo) * 0.08 || 0.01;
  return [lo - pad, hi + pad];
}

/** A line chart: series [{points: [[x, y]], color, dash, band: [[x, lo, hi]]}], with axes. */
function lineChart(core, { series, width = 560, height = 220, xLabel, yFmt = (v) => String(v), xFmt = (v) => core.fmt.si(v), yLog = false, yMin, yMax, hlines = [], vlines = [] }) {
  const { s } = core;
  const L = 54, R = 10, T = 10, B = 28;
  const xs = series.flatMap((sr) => [...(sr.points || []).map((p) => p[0]), ...(sr.band || []).map((p) => p[0])]);
  const ys = series.flatMap((sr) => [...(sr.points || []).map((p) => p[1]), ...(sr.band || []).flatMap((p) => [p[1], p[2]])]).concat(hlines.map((l) => l.y)).filter(Number.isFinite);
  const x0 = Math.min(...xs, 0);
  let x1 = Math.max(...xs);
  if (!(x1 > x0)) x1 = x0 + 1;
  const tf = yLog ? (v) => Math.log10(Math.max(v, 1e-12)) : (v) => v;
  let y0 = yMin ?? Math.min(...ys.map(tf)), y1 = yMax ?? Math.max(...ys.map(tf));
  if (!(y1 > y0)) { y0 -= 1; y1 += 1; }
  if (!yLog) { const pad = (y1 - y0) * 0.06; y0 -= pad; y1 += pad; }
  const X = (v) => L + ((v - x0) / (x1 - x0 || 1)) * (width - L - R);
  const Y = (v) => T + (1 - (tf(v) - y0) / (y1 - y0)) * (height - T - B);
  const g = s("svg", { class: "ex-chart", viewBox: `0 0 ${width} ${height}`, preserveAspectRatio: "none", style: { height: height + "px" } });
  const every = yLog ? Math.max(1, Math.ceil((Math.floor(y1) - Math.ceil(y0)) / 5)) : 1;
  const yt = yLog ? range(Math.ceil(y0), Math.floor(y1)).filter((e) => (Math.floor(y1) - e) % every === 0).map((e) => 10 ** e) : core.fmt.ticks(y0, y1, 4).filter((t) => t >= y0 && t <= y1);
  for (const t of yt) {
    g.append(s("line", { x1: L, x2: width - R, y1: Y(t), y2: Y(t), class: "gr" }));
    g.append(s("text", { x: L - 6, y: Y(t) + 3, "text-anchor": "end" }, yFmt(t)));
  }
  for (const t of core.fmt.ticks(x0, x1, 5).filter((t) => t >= x0 && t <= x1)) {
    g.append(s("text", { x: X(t), y: height - 10, "text-anchor": "middle" }, xFmt(t)));
  }
  if (xLabel) g.append(s("text", { x: width - R, y: height - 0, "text-anchor": "end" }, xLabel));
  g.append(s("line", { x1: L, x2: width - R, y1: height - B, y2: height - B, class: "ax" }));
  for (const sr of series) {
    if (sr.band?.length) {
      const top = sr.band.map((p) => `${X(p[0])},${Y(p[2])}`), bot = sr.band.slice().reverse().map((p) => `${X(p[0])},${Y(p[1])}`);
      g.append(s("polygon", { points: top.concat(bot).join(" "), style: { fill: sr.color, opacity: 0.16 } }));
    }
    if (sr.points?.length) {
      g.append(s("polyline", { points: sr.points.filter((p) => Number.isFinite(p[1])).map((p) => `${X(p[0])},${Y(p[1])}`).join(" "), style: { fill: "none", stroke: sr.color, strokeWidth: sr.width || 2, strokeDasharray: sr.dash || "none" } }));
      if (sr.dots) for (const p of sr.points) if (Number.isFinite(p[1])) g.append(s("circle", { cx: X(p[0]), cy: Y(p[1]), r: 2.5, style: { fill: sr.color } }));
    }
  }
  for (const l of vlines) {
    if (!Number.isFinite(l.x)) continue;
    g.append(s("line", { x1: X(l.x), x2: X(l.x), y1: T, y2: height - B, style: { stroke: l.color || "var(--axis)", strokeDasharray: l.dash || "4 3", strokeWidth: 1.2 } }));
    if (l.label) g.append(s("text", { x: X(l.x) + 4, y: T + 10, style: { fill: l.color || "var(--muted)" } }, l.label));
  }
  for (const l of hlines) {
    if (!Number.isFinite(l.y)) continue;
    g.append(s("line", { x1: L, x2: width - R, y1: Y(l.y), y2: Y(l.y), style: { stroke: l.color || "var(--axis)", strokeDasharray: l.dash || "4 3", strokeWidth: l.width || 1.2 } }));
    if (l.label) g.append(s("text", { x: width - R - 2, y: Y(l.y) - 4, "text-anchor": "end", style: { fill: l.color || "var(--muted)" } }, l.label));
  }
  return g;
}

const range = (a, b) => { const o = []; for (let i = a; i <= b; i++) o.push(i); return o; };
const legend = (h, items) => h("div", { class: "ex-legend" }, items.map(([color, text, dash]) => h("span", {}, h("i", { style: { background: dash ? `repeating-linear-gradient(90deg, ${color} 0 4px, transparent 4px 7px)` : color } }), text)));

function statusPill(core, r) {
  const st = r.status;
  if (st === "done") return core.pill("ok", "Done");
  if (st === "failed") return core.pill("bad", "Failed");
  if (st === "running") return core.pill("pending", "Running");
  if (st === "live") return core.h("span", { class: "ex-live" }, "Live");
  if (st === "starting") return core.pill("pending", "Starting");
  if (st === "ending") return core.pill("pending", "Ending");
  return core.pill("pending", "Queued");
}

// ------------------------------------------------------------ home

async function home(ctx, root, go, onStop) {
  const { core } = ctx;
  const { h } = core;
  const head = h("div", { class: "ex-head" },
    h("h1", {}, "Experiments"),
    h("span", { class: "sub" }, "A/B tests of rankings on simulated users, whose truth is known"),
    h("div", { class: "tools" }, h("a", { class: "btn", href: ctx.href("new", "live"), title: "Simulated users and AI agents on the site, every metric as it moves" }, core.icon("bolt"), "Watch one live"), h("a", { class: "btn primary", href: ctx.href("new") }, core.icon("plus"), "New experiment")));
  const body = h("div");
  root.append(head, body);
  let first = true;
  const refresh = async () => {
    const j = await ctx.api("/v1/experiments");
    const list = j.experiments || [];
    if (!list.length) {
      if (first) body.replaceChildren(await intro(ctx, go));
      first = false;
      return;
    }
    first = false;
    body.replaceChildren(h("div", { class: "ex-list" }, list.map((r) => item(ctx, r))));
  };
  await refresh();
  onStop(core.every(2500, refresh, false));
}

function item(ctx, r) {
  const { core } = ctx;
  const { h } = core;
  const sm = r.summary || {};
  const right = [];
  if (r.status === "done" && sm.verdicts) {
    for (const v of sm.verdicts) {
      right.push(h("div", { class: "row nw", style: { justifyContent: "flex-end" } },
        h("span", { class: "small dim" }, v.arm),
        callChip(h, v),
        h("span", { class: "ex-num small " + toneClass(v.p < 0.05 ? (v.call === "worse" ? "bad" : "good") : "flat") }, pctS(v.lift))));
    }
  } else if (r.status === "running" || r.status === "queued") {
    const p = r.progress || {};
    right.push(h("div", { style: { minWidth: "160px" } }, h("div", { class: "ex-bar" }, h("div", { style: { width: Math.round((p.share || 0) * 100) + "%" } })), h("div", { class: "small dim", style: { marginTop: "4px" } }, p.stage || "")));
  } else if (r.status === "live") {
    const l = r.live || {};
    right.push(h("div", { style: { minWidth: "160px" } }, h("div", { class: "ex-bar" }, h("div", { style: { width: Math.round(Math.min(1, (l.elapsed || 0) / (l.limit_s || 1)) * 100) + "%", background: "var(--crit)" } })), h("div", { class: "small dim", style: { marginTop: "4px" } }, l.status === "live" ? `${core.fmt.si(l.searches || 0)} searches so far${l.agents ? `, ${l.agents} AI agents` : ""}` : l.stage || l.status || "")));
  } else if (r.status === "failed") {
    right.push(h("span", { class: "small ex-bad" }, String(r.error || "").slice(0, 140)));
  }
  const truths = (sm.verdicts || []).filter((v) => v.true_lift != null).map((v) => `${v.arm}: truly ${pctS(v.true_lift)}`);
  return h("a", { class: "ex-item", href: r.status === "live" ? ctx.href("live", r.id) : ctx.href("e", r.id) },
    h("div", {}, h("div", { class: "nm" }, r.name || r.id),
      h("div", { class: "meta" }, `${r.site} · ${(sm.arms || []).join(" vs ")} · ${core.fmt.ago(r.created)}`)),
    h("div", { class: "ex-truth" }, r.status === "done" ? [h("div", {}, `${core.fmt.si(sm.users)} users · ${core.fmt.si(sm.searches)} searches · ${sm.model} · ${sm.mode}${sm.agents ? ` · ${sm.agents} AI agents` : ""}`), truths.length ? h("div", {}, h("b", {}, "Truth "), truths.join(" · ")) : null] : null),
    h("div", { class: "row", style: { justifyContent: "flex-end", gap: "10px" } }, statusPill(core, r), h("div", { style: { display: "grid", gap: "4px" } }, right)));
}

async function intro(ctx, go) {
  const { core } = ctx;
  const { h } = core;
  const sites = (await ctx.api("/v1/sites")).sites || [];
  const btn = h("button", { class: "btn primary", disabled: !sites.length }, "Run an example");
  btn.onclick = async () => {
    btn.disabled = true;
    const s = sites.find((x) => x.name === "nfcorpus") || sites[0];
    const r = s.rankings || [];
    try {
      const j = await ctx.api("/v1/experiments", { method: "POST", body: { name: `${r[1]?.[0] || "B"} against ${r[0]?.[0] || "A"}`, site: s.name, arms: [{ name: r[0][0], rank: r[0][1] }, { name: r[1][0], rank: r[1][1] }] } });
      go("e", j.id);
    } catch (e) { core.toast(e.message, "error"); btn.disabled = false; }
  };
  return core.card({
    title: "Experiments on users you can see through",
    body: h("div", { class: "ex-intro" },
      h("div", {},
        h("p", {}, "Every experiment here runs on simulated users of a site: thousands of people of each age, gender, country, device and tenure, each with their own way of reading a list of results, clicking and staying. Each search is ranked by ChloeDB, as the arm it was given asks, and the users react as a click model says."),
        h("p", {}, "Because the users are simulated, the truth is known: the report shows the true effect of each arm beside what the experiment measured, so you can see when an interval holds it, what peeking does to a p value, how much power the test had, and which cohorts really differ."),
        h("div", { class: "row" }, btn, h("a", { class: "btn", href: ctx.href("new") }, "Build one"))),
      h("ol", { class: "ex-steps" },
        h("li", {}, "Pick a site: a table of ChloeDB, the queries people ask it and how good each row is for each."),
        h("li", {}, "Give each arm a ranking (an ORDER BY of ChloeDB's SQL) and its share of the users."),
        h("li", {}, "Choose the users, the searches and how people click; keep a share of the users exploring, to value every arm off-policy too."),
        h("li", {}, "Read the report: every metric with its interval, p value and power, CUPED, always-valid p values, cohorts, and the truth."))),
  });
}

// ------------------------------------------------------------ builder

async function builder(ctx, root, go, meta, from) {
  const { core } = ctx;
  const { h } = core;
  const sites = (await ctx.api("/v1/sites")).sites || [];
  const loading = (await ctx.api("/v1/sites")).loading || {};
  let spec = JSON.parse(JSON.stringify(meta.defaults));
  for (const [k, v] of Object.entries(meta.live?.defaults || {})) if (spec[k] === undefined) spec[k] = v;
  const liveStart = from === "live";
  if (liveStart) { from = null; spec.mode = "live"; }
  if (from) {
    const r = await ctx.api(`/v1/experiments/${encodeURIComponent(from)}`);
    if (r.spec) spec = { ...spec, ...r.spec, name: (r.spec.name || "") + " (again)", seed: (r.spec.seed || 0) + 1 };
  }
  if (!spec.site && sites.length) spec.site = (sites.find((s) => s.name === "nfcorpus") || sites[0]).name;
  const siteOf = () => sites.find((s) => s.name === spec.site);
  const seedArms = () => {
    const r = siteOf()?.rankings || [];
    spec.arms = [{ name: r[0]?.[0] || "Control", rank: r[0]?.[1] || "", share: 1 }, { name: r[1]?.[0] || "Treatment", rank: r[1]?.[1] || "", share: 1 }];
  };
  if (!spec.arms?.length) seedArms();
  if (!spec.name) spec.name = `${spec.arms[1]?.name} against ${spec.arms[0]?.name}`;

  root.append(h("div", { class: "ex-head" }, h("h1", {}, "New experiment"), h("span", { class: "sub" }, "Arms, users and traffic; a look at what each arm shows"), h("div", { class: "tools" }, h("a", { class: "btn", href: ctx.href() }, "Cancel"))));
  if (!sites.length) {
    root.append(core.empty("box", "No sites yet", Object.keys(loading).length ? `Loading: ${Object.entries(loading).map(([k, v]) => `${k} (${v})`).join(", ")}` : "Start the app with --beir DIR to load BEIR collections, or add a site of your own tables (POST /v1/sites)."));
    return;
  }
  const left = h("div", { style: { display: "grid", gap: "14px" } });
  const right = h("div", { style: { display: "grid", gap: "14px", position: "sticky", top: "8px" } });
  root.append(h("div", { class: "ex-build" }, left, right));

  // The site.
  const siteBox = h("div", { class: "ex-sites" });
  const drawSites = () => siteBox.replaceChildren(...sites.map((s) => h("div", { class: "ex-site", role: "button", tabindex: 0, "aria-pressed": String(s.name === spec.site), onclick: () => { if (spec.site === s.name) return; spec.site = s.name; seedArms(); spec.name = `${spec.arms[1].name} against ${spec.arms[0].name}`; nameIn.value = spec.name; drawSites(); drawArms(); preview(); } },
    h("b", {}, s.name), h("div", { class: "about" }, s.about || s.table))));
  drawSites();
  const nameIn = h("input", { class: "input", value: spec.name, style: { width: "100%" }, oninput: () => { spec.name = nameIn.value; } });
  left.append(core.card({ title: "Site", sub: "a table, its queries and their judged rows", body: h("div", { style: { display: "grid", gap: "10px" } }, siteBox, h("label", { class: "small dim" }, "Name of the experiment"), nameIn) }));

  // The arms.
  const armsBox = h("div", { style: { display: "grid", gap: "10px" } });
  const split = h("div", { class: "ex-split" });
  const addBtn = h("button", { class: "btn sm", onclick: () => { const r = siteOf()?.rankings || []; const used = new Set(spec.arms.map((a) => a.rank)); const next = r.find((x) => !used.has(x[1])) || r[0] || ["Arm", ""]; spec.arms.push({ name: next[0], rank: next[1], share: 1 }); drawArms(); preview(); } }, core.icon("plus"), "Add an arm");
  function drawSplit() {
    const total = spec.arms.reduce((a, x) => a + (+x.share || 0), 0) || 1;
    const ex = spec.explore_share || 0;
    const parts = spec.arms.map((a, i) => [ARM_COLORS[i], a.name, ((+a.share || 0) / total) * (1 - ex)]);
    if (ex > 0) parts.push([EXPLORE_COLOR, "exploring", ex]);
    split.replaceChildren(...parts.map(([c, n, w]) => h("div", { style: { background: c, width: (w * 100).toFixed(2) + "%" }, title: `${n}: ${(w * 100).toFixed(1)}% of the users` }, w > 0.08 ? `${n} ${Math.round(w * 100)}%` : "")));
  }
  function drawArms() {
    const r = siteOf()?.rankings || [];
    armsBox.replaceChildren(...spec.arms.map((a, i) => {
      const name = h("input", { class: "input sm", value: a.name, style: { flex: "1", minWidth: "120px", fontWeight: 600 }, oninput: () => { a.name = name.value; drawSplit(); } });
      const pick = h("select", { class: "select sm", onchange: () => { if (!pick.value) return; const x = r.find((y) => y[1] === pick.value); a.rank = x[1]; a.name = x[0]; name.value = a.name; text.value = a.rank; drawSplit(); preview(); } },
        h("option", { value: "" }, "A ranking of the site…"), r.map(([t, x]) => h("option", { value: x, selected: x === a.rank }, t)));
      const text = h("textarea", { class: "input", spellcheck: "false", oninput: core.debounce(() => { a.rank = text.value; preview(); }, 500) });
      text.value = a.rank;
      const share = h("input", { type: "range", min: 1, max: 9, step: 1, value: a.share || 1, oninput: () => { a.share = +share.value; shareV.textContent = `share ${a.share}`; drawSplit(); } });
      const shareV = h("span", { class: "small dim" }, `share ${a.share || 1}`);
      const del = i >= 2 ? h("button", { class: "btn ghost sm", title: "Remove this arm", onclick: () => { spec.arms.splice(i, 1); drawArms(); preview(); } }, core.icon("x")) : null;
      return h("div", { class: "ex-arm", style: { "--arm": ARM_COLORS[i] } },
        h("div", { class: "ex-arm-top" }, h("span", { class: "small", style: { color: ARM_COLORS[i], fontWeight: 700, width: "64px" } }, i === 0 ? "Control" : `Arm ${String.fromCharCode(65 + i)}`), name, pick, del),
        h("div", { class: "small dim" }, "ORDER BY, of :t, the search's text"), text,
        h("div", { class: "row nw" }, shareV, share));
    }));
    addBtn.disabled = spec.arms.length >= meta.limits.arms;
    drawSplit();
  }
  drawArms();
  left.append(core.card({ title: "Arms", sub: "the first is the control; each user stays in one", tools: addBtn, body: h("div", { style: { display: "grid", gap: "10px" } }, armsBox, h("div", { class: "small dim" }, "How the users split"), split) }));

  // Users and traffic.
  const knobs = h("div", { class: "ex-knobs" });
  const estimate = h("div", { class: "small dim" });
  const knob = (title, hint, input, value) => h("div", { class: "ex-knob" }, h("label", {}, title, value), input, hint ? h("div", { class: "hint" }, hint) : null);
  const stepped = (vals, key, fmtv, after) => {
    const at = Math.max(0, vals.findIndex((v) => v >= spec[key]));
    const out = h("span", {}, fmtv(vals[at]));
    spec[key] = vals[at];
    const inp = h("input", { type: "range", min: 0, max: vals.length - 1, step: 1, value: at, oninput: () => { spec[key] = vals[+inp.value]; out.textContent = fmtv(spec[key]); after?.(); } });
    return [inp, out];
  };
  const slider = (key, min, max, step, fmtv, after) => {
    const out = h("span", {}, fmtv(spec[key]));
    const inp = h("input", { type: "range", min, max, step, value: spec[key], oninput: () => { spec[key] = +inp.value; out.textContent = fmtv(spec[key]); after?.(); } });
    return [inp, out];
  };
  const L = meta.live || { defaults: {}, limits: {}, models: [] };
  const modelOf = () => L.models.find((m) => m.id === spec.agent_model) || L.models[0] || { name: "Claude", input: 4, output: 20 };
  const est = () => {
    if (spec.mode === "live") {
      const n = spec.rate * spec.minutes * 60;
      const m = modelOf();
      // A task: about four calls of 1,500 tokens in and 300 out.
      const perTask = (4 * (1500 * m.input + 300 * m.output)) / 1e6;
      const agents = spec.agents ? (spec.brain === "scripted" ? ` ${spec.agents} scripted agents, no model.` : ` ${spec.agents} AI agents on ${m.name}, at most $${spec.budget}: about ${Math.max(1, Math.floor(spec.budget / perTask))} tasks.`) : "";
      estimate.textContent = `About ${core.fmt.si(n)} searches in ${spec.minutes} min, every one a logged query to ChloeDB, counted each second.${agents}`;
      return;
    }
    const pre = spec.impressions * spec.pre;
    const secs = spec.mode === "served" ? spec.impressions / 1200 + pre / 2e6 : (spec.impressions + pre) / 1.5e6 + spec.users * spec.arms.length / 4e5 + 2;
    estimate.textContent = `About ${secs < 90 ? Math.max(3, Math.round(secs)) + " s" : Math.round(secs / 60) + " min"} to run${spec.mode === "served" ? ", every search a logged query to ChloeDB" : ""}.`;
  };
  const searchesMax = () => (spec.mode === "served" ? meta.limits.served : meta.limits.fast);
  const run = h("button", { class: "btn primary" });
  const runLabel = () => run.replaceChildren(core.icon(spec.mode === "live" ? "bolt" : "play"), spec.mode === "live" ? "Start the live run" : "Run the experiment");
  function drawKnobs() {
    const live = spec.mode === "live";
    const maxUsers = live ? L.limits.users || 100000 : meta.limits.users;
    if (spec.users > maxUsers) spec.users = maxUsers;
    const [uIn, uOut] = stepped(USERS.filter((u) => u <= maxUsers), "users", (v) => core.fmt.n(v), est);
    const [sIn, sOut] = stepped(SEARCHES, "impressions", (v) => core.fmt.si(v), () => { if (spec.impressions > searchesMax()) { spec.impressions = searchesMax(); } sOut.textContent = core.fmt.si(spec.impressions); est(); });
    const [aIn, aOut] = slider("appetite", 0, 1.5, 0.05, (v) => v.toFixed(2));
    const [tIn, tOut] = slider("taste", 0, 1, 0.05, (v) => v.toFixed(2));
    const [lIn, lOut] = slider("limit", 1, 10, 1, (v) => `${v} rows`, preview);
    const [pIn, pOut] = slider("pre", 0, 2, 0.1, (v) => (v ? `${v.toFixed(1)}× the experiment` : "none"), est);
    const [eIn, eOut] = slider("explore_share", 0, 0.5, 0.01, (v) => (v ? `${Math.round(v * 100)}% of users` : "none"), drawSplit);
    const model = h("select", { class: "select sm", style: { width: "100%" }, onchange: () => { spec.model = model.value; modelHint.textContent = meta.models.find((m) => m.name === spec.model)?.about || ""; } }, meta.models.map((m) => h("option", { value: m.name, selected: m.name === spec.model }, m.name.toUpperCase())));
    const modelHint = h("div", { class: "hint" }, meta.models.find((m) => m.name === spec.model)?.about || "");
    const policy = h("select", { class: "select sm", style: { width: "100%" }, onchange: () => { spec.explore = meta.policies[+policy.value].explore; } }, meta.policies.map((p, i) => h("option", { value: i, selected: JSON.stringify(p.explore) === JSON.stringify(spec.explore) }, p.name)));
    const primary = h("select", { class: "select sm", style: { width: "100%" }, onchange: () => { spec.primary = primary.value; } }, meta.metrics.filter((m) => !m.guardrail).map((m) => h("option", { value: m.key, selected: m.key === spec.primary }, m.title)));
    const mode = core.seg([["fast", "Fast", "drawn here, from each arm's rows ranked once per query"], ["served", "Served", "every search a logged query to ChloeDB, and its clicks written back"], ["live", "Live", "watch it as it runs: simulated users at a rate you set, AI agents on the site, every metric each second"]], spec.mode, (v) => {
      spec.mode = v;
      if (v !== "live" && spec.impressions > searchesMax()) spec.impressions = searchesMax();
      drawKnobs();
    });
    const seed = h("input", { class: "input sm", type: "number", value: spec.seed, style: { width: "100px" }, oninput: () => { spec.seed = Math.max(0, Math.floor(+seed.value || 0)); } });
    const common = [
      knob("Users", live ? "Each is in one arm for the whole run, and comes back as often as they are active." : "Each is in one arm for the whole experiment.", uIn, uOut),
      knob("Click model", null, h("div", {}, model, modelHint)),
      knob("How much users differ", "In how readily they click: 0 is everyone alike; real sites are near 0.6. It is what CUPED feeds on.", aIn, aOut),
      knob("How much cohorts' tastes differ", "How far a row's appeal moves with age and gender.", tIn, tOut),
      knob("Rows shown", null, lIn, lOut),
      knob("Decide on", "The primary metric: the call is made on it, the rest are watched.", primary),
    ];
    let own;
    if (live) {
      const [rIn, rOut] = stepped(RATES.filter((r) => r <= (L.limits.rate || 400)), "rate", (v) => `${v} a second`, est);
      const [mIn, mOut] = stepped(MINUTES.filter((m) => m <= (L.limits.minutes || 60)), "minutes", (v) => `${v} min`, est);
      const [bIn, bOut] = stepped(BUDGETS.filter((b) => b <= (L.limits.budget || 20)), "budget", (v) => `$${v}`, est);
      if (!L.claude && spec.brain === "claude") spec.brain = "scripted";
      const pickModel = h("select", { class: "select sm", style: { width: "100%" }, disabled: spec.brain !== "claude", onchange: () => { spec.agent_model = pickModel.value; est(); } }, L.models.map((m) => h("option", { value: m.id, selected: m.id === spec.agent_model }, `${m.name} · $${m.input} / $${m.output} a million tokens`)));
      const brain = core.seg([["claude", "Claude", L.claude ? "each agent is played by Claude, through the Messages API" : "the app has no ANTHROPIC_API_KEY"], ["scripted", "Scripted", "no model: it searches the need's words, opens the result whose title shares the most, and stops"]], spec.brain, (v) => {
        if (v === "claude" && !L.claude) { core.toast("Claude agents need an API key on the app (ANTHROPIC_API_KEY). Scripted agents work without one.", "warn", 7000); brain.set("scripted"); return; }
        spec.brain = v;
        pickModel.disabled = v !== "claude";
        est();
      });
      const brainBox = h("div", { style: { display: spec.agents ? "" : "none", gridColumn: "1 / -1" } }, h("div", { class: "ex-knobs" },
        knob("Who plays them", L.claude ? "Claude reads each page and decides; a refusal goes to another model on the server (fallbacks)." : "Claude agents need an API key on the app (ANTHROPIC_API_KEY).", brain),
        knob("Model", null, pickModel),
        knob("Budget", "The agents stop once they have spent it (the cost worked out from the tokens each call reports).", bIn, bOut)));
      const [gIn, gOut] = slider("agents", 0, L.limits.agents || 6, 1, (v) => (v ? `${v} agents` : "none"), () => { est(); brainBox.style.display = spec.agents ? "" : "none"; });
      own = [
        knob("Searches a second", "Simulated users arriving; you can change it while it runs.", rIn, rOut),
        knob("For", "It ends then, or when you stop it, and becomes an experiment with the full report.", mIn, mOut),
        knob("AI agents", "People of the population, each in the arm the split gives them, using the site's search for a need of its own: they type, read, open and decide.", gIn, gOut),
        brainBox,
      ];
    } else {
      own = [
        knob("Searches", "Spread over the users by how active each is.", sIn, sOut),
        knob("Pre-period", "Searches before the experiment, under the control, for CUPED.", pIn, pOut),
        knob("Exploring share", "Users served the control through a policy that explores, whose log values every arm off-policy.", eIn, eOut),
        knob("Exploring policy", null, policy),
      ];
    }
    if (live) spec.explore_share = 0;
    knobs.replaceChildren(knob("Mode", null, mode), ...own, ...common, knob("Seed", "The same seed, the same users and the same searches.", seed));
    drawSplit();
    est();
    runLabel();
  }
  drawKnobs();
  run.onclick = async () => {
    run.disabled = true;
    try {
      if (spec.mode === "live") {
        const j = await ctx.api("/v1/live", { method: "POST", body: spec });
        go("live", j.id);
      } else {
        const j = await ctx.api("/v1/experiments", { method: "POST", body: spec });
        go("e", j.id);
      }
    } catch (e) { core.toast(e.message, "error", 8000); run.disabled = false; }
  };
  left.append(core.card({ title: "Users and traffic", body: knobs, foot: h("div", { class: "row between" }, estimate, run) }));

  // The preview.
  const pv = h("div", { class: "ex-pv" }, h("div", { class: "small dim" }, "Loading…"));
  right.append(core.card({ title: "What each arm shows", sub: "the site's busiest queries; a darker square is a better row", body: pv }));
  let asked = 0;
  async function preview() {
    const n = ++asked;
    const ranks = spec.arms.map((a) => a.rank).filter((r) => r.trim());
    if (!ranks.length) return;
    try {
      const j = await ctx.api("/v1/preview", { method: "POST", body: { site: spec.site, ranks: spec.arms.map((a) => a.rank), limit: spec.limit, queries: 3 } });
      if (n !== asked) return;
      pv.replaceChildren(...j.queries.map((q) => h("div", {},
        h("div", { class: "row between", style: { marginBottom: "6px" } }, h("b", { style: { fontSize: "13px" } }, q.text), h("span", { class: "small dim" }, `${(q.share * 100).toFixed(1)}% of searches · ${q.judged} judged`)),
        h("div", { class: "lists", style: { gridTemplateColumns: `repeat(${q.lists.length}, minmax(0, 1fr))` } }, q.lists.map((l, i) => h("div", { style: { borderTop: `3px solid ${ARM_COLORS[i]}`, paddingTop: "6px" } },
          h("div", { class: "small", style: { fontWeight: 600, marginBottom: "4px" } }, spec.arms[i]?.name, " ", h("span", { class: "dim", style: { fontWeight: 400 } }, `· ${dcg(l.rows, j.max_grade)}`)),
          l.rows.length ? h("ol", {}, l.rows.map((r) => h("li", { title: `${r.key}: grade ${r.grade}` }, h("div", { class: `ex-dot g${r.grade}` }), h("span", { class: "t" }, r.title || r.key)))) : h("div", { class: "small ex-bad" }, "no rows")))))));
    } catch (e) {
      if (n === asked) pv.replaceChildren(h("div", { class: "small ex-bad" }, e.message));
    }
  }
  preview();
}

/** A list's graded gain, as a share of the best order of the same rows. */
function dcg(rows, max) {
  const g = rows.map((r) => (2 ** r.grade - 1) / (2 ** max - 1));
  const d = (xs) => xs.reduce((a, x, i) => a + x / Math.log2(i + 2), 0);
  const ideal = d(g.slice().sort((a, b) => b - a));
  return ideal > 0 ? `${Math.round((d(g) / ideal) * 100)}% of its best order` : "no judged rows";
}

// ------------------------------------------------------------ report

async function report(ctx, root, go, meta, id, tab, onStop) {
  const { core } = ctx;
  const { h } = core;
  let r = await ctx.api(`/v1/experiments/${encodeURIComponent(id)}`);
  if (r.status === "live" && r.live) { location.hash = ctx.href("live", id); return; }
  const head = h("div", { class: "ex-head" });
  const body = h("div");
  root.append(head, body);
  const drawHead = () => {
    const spec = r.spec || {};
    head.replaceChildren(
      h("a", { class: "btn ghost sm", href: ctx.href(), title: "All experiments" }, "←"),
      h("h1", {}, r.name || id), statusPill(core, r),
      h("span", { class: "sub" }, `${r.site} · ${(spec.arms || []).map((a) => a.name).join(" vs ")} · ${core.fmt.datetime(r.created)}`),
      h("div", { class: "tools" },
        h("a", { class: "btn sm", href: ctx.href("new", id) }, "Run again"),
        r.status === "done" || r.status === "failed" ? h("button", { class: "btn sm danger", onclick: async () => { if (!(await core.confirm({ title: "Delete this experiment?", body: "Its report goes with it.", ok: "Delete", danger: true }))) return; await ctx.api(`/v1/experiments/${encodeURIComponent(id)}`, { method: "DELETE" }); go(); } }, "Delete") : null));
  };
  drawHead();
  if (r.status === "queued" || r.status === "running") {
    const bar = h("div", { style: { width: "0%" } });
    const stage = h("div", { class: "small dim", style: { marginTop: "6px" } });
    body.replaceChildren(core.card({ title: "Running", body: h("div", {}, h("div", { class: "ex-bar", style: { height: "10px" } }, bar), stage) }));
    await new Promise((resolve) => {
      const stop = core.every(800, async () => {
        r = await ctx.api(`/v1/experiments/${encodeURIComponent(id)}`);
        const p = r.progress || {};
        bar.style.width = Math.round((p.share || 0) * 100) + "%";
        stage.textContent = `${p.stage || r.status} · ${core.fmt.si(p.done || 0)} of ${core.fmt.si(p.total || 0)} searches`;
        if (r.status !== "queued" && r.status !== "running") { stop(); resolve(); }
      }, true, { always: true });
      onStop(stop);
    });
    drawHead();
  }
  if (r.status === "failed") { body.replaceChildren(h("div", { class: "error" }, `This experiment failed: ${r.error}`)); return; }
  const rep = r.report;
  if (!rep) { body.replaceChildren(core.empty("box", "No report", "")); return; }
  const tabs = [["scorecard", "Scorecard"], ["metric", "One metric"], ["cohorts", "Cohorts"], rep.live ? ["agents", "Agents"] : ["whatif", "What-if"], ["setup", "Setup"]];
  const bar = h("nav", { class: "ex-tabs" }, tabs.map(([k, t]) => h("a", { href: ctx.href("e", id, k), "aria-current": k === tab.split(":")[0] ? "page" : null }, t)));
  const page = h("div");
  body.replaceChildren(verdicts(ctx, rep), kpis(ctx, rep), bar, page);
  const [t, arg] = tab.split(":");
  if (t === "scorecard") page.append(scorecard(ctx, rep, id));
  else if (t === "metric") page.append(metricPage(ctx, rep, id, arg || rep.spec?.primary || "success"));
  else if (t === "cohorts") page.append(cohorts(ctx, rep, id, r.cuttable));
  else if (t === "whatif") page.append(whatif(ctx, rep));
  else if (t === "agents" && rep.live) page.append(agentsReport(ctx, rep));
  else page.append(setup(ctx, rep, r));
}

function verdicts(ctx, rep) {
  const { core } = ctx;
  const { h } = core;
  const pm = rep.metrics.find((m) => m.key === rep.spec?.primary) || rep.metrics[0];
  return h("div", { class: "ex-verdicts" }, rep.verdicts.map((v, i) => {
    const c = pm.comparisons[i];
    const t = tone(pm, c.lift_lo, c.lift_hi);
    const truthTone = v.true_lift == null ? null : Math.abs(v.true_lift) < 1e-9 ? "flat" : (v.true_lift > 0) === pm.higher ? "good" : "bad";
    const agrees = v.truth_call && ((v.call === "ship" || v.call.startsWith("better")) ? v.truth_call === "truly better" : v.call === "worse" ? v.truth_call === "truly worse" : null);
    const [lo, hi] = domain([c.lift_lo, c.lift_hi, v.true_lift ?? 0]);
    return h("div", { class: "ex-verdict", style: { borderTop: `3px solid ${ARM_COLORS[i + 1]}` } },
      h("div", { class: "row between" }, h("b", {}, `${v.arm} against ${rep.arms[0].name}`), callChip(h, v)),
      h("div", { class: "row", style: { alignItems: "baseline", gap: "10px" } }, h("span", { class: "big " + toneClass(t) }, pctS(c.lift)), h("span", { class: "small dim" }, `${pm.title}, 95% interval ${pctS(c.lift_lo)} to ${pctS(c.lift_hi)}, p ${pText(c.p)}`)),
      forest(core, { lo: c.lift_lo, hi: c.lift_hi, mid: c.lift, truth: v.true_lift, min: lo, max: hi, color: toneColor(t), width: 300, height: 26 }),
      h("div", { class: "line" }, v.true_lift != null ? [h("b", { class: toneClass(truthTone) }, `The truth: ${pctS(v.true_lift, 2)}`), ` (${v.truth_call}). `, agrees === true ? "The experiment's call is right." : agrees === false ? "The experiment's call is wrong: a false positive." : v.call === "no detectable difference" && v.truth_call !== "no true difference" ? "The experiment missed it: too little power." : null] : null),
      h("div", { class: "line" }, `Power for the true effect ${power(v.power)}`, v.power != null && v.power < 0.8 && v.users_needed ? ` · 80% would take about ${core.fmt.si(v.users_needed)} users` : "", v.guardrails_worse?.length ? ` · guardrails worse: ${v.guardrails_worse.join(", ")}` : " · no guardrail worse"));
  }));
}

function kpis(ctx, rep) {
  const { core } = ctx;
  const { h } = core;
  const k = (key, v, title) => h("div", { class: "ex-kpi", title }, h("div", { class: "k" }, key), h("div", { class: "v" }, v));
  const srm = rep.srm;
  return h("div", { class: "ex-kpis" },
    rep.live ? k("Users seen", core.fmt.n(rep.arms.reduce((s, a) => s + (a.users || 0), 0)), `of a population of ${core.fmt.n(rep.users)}`) : k("Users", core.fmt.n(rep.users)),
    k("Searches", core.fmt.si(rep.searches), rep.live ? "live: no pre-period" : `and ${core.fmt.si(rep.pre_searches)} in the pre-period`),
    rep.arms.map((a, i) => k(a.name, `${core.fmt.n(a.users)} users`, `${core.fmt.si(a.searches)} searches`)),
    k("Sample ratio", srm.ok ? "as planned" : "MISMATCH", `chi-square ${srm.chi2.toFixed(2)}, p ${pText(srm.p)}: the users each arm got against the split planned`),
    k("Ran in", core.fmt.dur(rep.seconds) === "0s" ? rep.seconds.toFixed(1) + " s" : core.fmt.dur(rep.seconds)));
}

function scorecard(ctx, rep, id) {
  const { core } = ctx;
  const { h } = core;
  const arms = rep.arms.slice(1);
  const lifts = rep.metrics.flatMap((m) => m.comparisons.flatMap((c) => [c.lift_lo, c.lift_hi, c.true_lift]));
  const [lo, hi] = domain(lifts.filter((x) => Number.isFinite(x) && Math.abs(x) < 2));
  const rows = [];
  for (const m of rep.metrics) {
    m.comparisons.forEach((c, i) => {
      const t = tone(m, c.lift_lo, c.lift_hi);
      rows.push(h("tr", { class: "click", onclick: () => { location.hash = ctx.href("e", id, "metric:" + m.key); } },
        i === 0 ? h("td", { rowspan: m.comparisons.length, class: "mname" }, m.title, m.guardrail ? h("span", { class: "pill warn", style: { marginLeft: "6px", fontSize: "10.5px" } }, "guardrail") : null, h("small", {}, m.higher ? "higher is better" : "lower is better")) : null,
        arms.length > 1 ? h("td", { style: { color: ARM_COLORS[i + 1], fontWeight: 600 } }, c.arm) : null,
        h("td", { class: "num ex-num" }, unitValue(m, c.control.value)),
        h("td", { class: "num ex-num" }, unitValue(m, c.treatment.value)),
        h("td", { class: "num ex-num " + toneClass(t) }, unitDelta(m, c.delta)),
        h("td", { class: "num ex-num " + toneClass(t) }, pctS(c.lift), h("div", { class: "small dim" }, `${pctS(c.lift_lo)} to ${pctS(c.lift_hi)}`)),
        h("td", {}, forest(core, { lo: c.lift_lo, hi: c.lift_hi, mid: c.lift, truth: c.true_lift, min: lo, max: hi, color: toneColor(t) })),
        h("td", { class: "num ex-num" }, pText(c.p), h("div", { class: "small dim" }, `q ${pText(c.q)}`)),
        h("td", { class: "num ex-num" }, c.cuped ? [Math.max(0, Math.round(c.cuped.variance_reduction * 100)) + "%", h("div", { class: "small dim" }, `p ${pText(c.cuped.p)}`)] : "–"),
        h("td", { class: "num ex-num" }, power(c.power), h("div", { class: "small dim" }, `MDE ${pctS(c.mde_lift)}`)),
        h("td", { class: "num ex-num" }, c.true_lift == null ? "measured" : [pctS(c.true_lift, 2), h("div", { class: "small " + (c.covers ? "dim" : "ex-bad") }, c.covers ? "inside the interval" : "outside it")])));
    });
  }
  const tbl = h("table", { class: "t ex-score" },
    h("thead", {}, h("tr", {}, h("th", {}, "Metric"), arms.length > 1 ? h("th", {}, "Arm") : null, h("th", { class: "num" }, rep.arms[0].name), h("th", { class: "num" }, arms.length > 1 ? "Arm" : arms[0].name), h("th", { class: "num" }, "Difference"), h("th", { class: "num" }, "Lift, 95%"), h("th", {}, "Interval ◇ truth"), h("th", { class: "num", title: "p: this metric alone; q: Benjamini-Hochberg over the metrics" }, "p · q"), h("th", { class: "num", title: "the variance CUPED removed, and its p" }, "CUPED"), h("th", { class: "num", title: "the chance this test sees the true effect, and the smallest lift it would see with 80% power" }, "Power"), h("th", { class: "num" }, "Truth"))),
    h("tbody", {}, rows));
  return h("div", { style: { display: "grid", gap: "14px" } },
    core.card({ title: "Every metric", sub: "a row opens the metric; ◇ marks the truth", body: h("div", { class: "tbl-wrap" }, tbl), flush: true }),
    latency(ctx, rep));
}

function latency(ctx, rep) {
  const { core } = ctx;
  const { h } = core;
  const max = Math.max(...rep.latency.map((l) => l.p99 || 0), 0.001);
  const bars = rep.latency.map((l, i) => h("div", { style: { display: "grid", gridTemplateColumns: "140px 1fr", gap: "10px", alignItems: "center" } },
    h("span", { class: "small", style: { color: ARM_COLORS[i], fontWeight: 600 } }, l.arm),
    h("div", { style: { display: "grid", gap: "3px" } }, [["p50", l.p50], ["p95", l.p95], ["p99", l.p99]].map(([k, v]) => h("div", { class: "row nw", style: { gap: "6px" } }, h("span", { class: "small dim", style: { width: "28px" } }, k), h("div", { style: { height: "8px", width: Math.max(1, (v / max) * 100) + "%", background: ARM_COLORS[i], opacity: k === "p50" ? 1 : k === "p95" ? 0.6 : 0.35, borderRadius: "3px" } }), h("span", { class: "small ex-num" }, core.fmt.ms(v)))))));
  return core.card({ title: "Query time", sub: rep.spec?.mode === "served" || rep.spec?.mode === "live" ? "every search, as the server answered it" : "each query once per arm, the arms taking turns at going first", body: h("div", { style: { display: "grid", gap: "10px" } }, bars) });
}

function metricPage(ctx, rep, id, key) {
  const { core } = ctx;
  const { h } = core;
  const m = rep.metrics.find((x) => x.key === key) || rep.metrics[0];
  const pick = h("select", { class: "select sm", onchange: () => { location.hash = ctx.href("e", id, "metric:" + pick.value); } }, rep.metrics.map((x) => h("option", { value: x.key, selected: x.key === m.key }, x.title)));
  const cards = [h("div", { class: "row between" }, h("p", { class: "ex-note", style: { margin: 0 } }, m.about), pick)];
  m.comparisons.forEach((c, i) => {
    const color = ARM_COLORS[i + 1];
    const t = tone(m, c.lo, c.hi);
    const s = c.series || [];
    const band = s.map((p) => [p.searches, p.lo, p.hi]);
    const over = lineChart(core, {
      series: [{ band, color, points: s.map((p) => [p.searches, p.delta]), dots: true }],
      hlines: [{ y: 0, color: "var(--axis)", dash: "none" }, c.true_delta != null ? { y: c.true_delta, color: "var(--text)", label: "truth" } : null].filter(Boolean),
      yFmt: (v) => unitDelta(m, v), xLabel: "searches",
    });
    const pChart = lineChart(core, {
      series: [{ points: s.map((p) => [p.searches, Math.max(p.p, 1e-12)]), color: "var(--s2)", dots: true }, { points: s.map((p) => [p.searches, Math.max(p.p_always_valid, 1e-12)]), color: "var(--s3)", dots: true, width: 2.5 }],
      yLog: true, yMax: 0, yMin: Math.max(-12, Math.floor(Math.log10(Math.max(1e-12, Math.min(...s.map((p) => p.p)))))), hlines: [{ y: 0.05, color: "var(--crit)", label: "0.05" }],
      yFmt: (v) => (v >= 0.01 ? String(v) : v.toExponential(0)), xLabel: "searches", height: 200,
    });
    const firstFixed = s.find((p) => p.p < 0.05), firstValid = s.find((p) => p.p_always_valid < 0.05);
    const peekNote = firstFixed ? `A peek at each checkpoint would have called it at ${core.fmt.si(firstFixed.searches)} searches; the always-valid p value, which may be read at every checkpoint, at ${firstValid ? core.fmt.si(firstValid.searches) : "none of them"}.` : "No checkpoint's p value fell below 0.05.";
    const cu = c.cuped;
    const [dlo, dhi] = domain([c.lo, c.hi, cu?.lo, cu?.hi, c.true_delta]);
    cards.push(h("div", { class: "ex-g2" },
      core.card({ title: `${c.arm}: the difference as the searches came`, sub: "95% interval at each of 20 checkpoints", body: h("div", {}, over, legend(h, [[color, "measured, with its interval"], ["var(--text)", "the truth", true]])) }),
      core.card({ title: "Peeking", sub: "p value at each checkpoint", body: h("div", {}, pChart, legend(h, [["var(--s2)", "p value of a fixed-size test, read at each peek"], ["var(--s3)", "always-valid p value (mSPRT)"]]), h("p", { class: "ex-note", style: { marginTop: "8px" } }, peekNote)) })));
    cards.push(h("div", { class: "ex-g2" },
      core.card({ title: "Power", sub: "the chance of seeing an effect, with these users", body: powerChart(ctx, m, c) }),
      core.card({ title: "CUPED", sub: cu ? `each user's pre-period under the control as the covariate: ${Math.max(0, Math.round(cu.variance_reduction * 100))}% of the variance gone` : "no pre-period", body: h("div", { style: { display: "grid", gap: "10px" } },
        h("div", { class: "row" }, h("span", { class: "small", style: { width: "90px" } }, "Plain"), forest(core, { lo: c.lo, hi: c.hi, mid: c.delta, truth: c.true_delta, min: dlo, max: dhi, color: toneColor(t), width: 240 }), h("span", { class: "small ex-num" }, `${unitDelta(m, c.delta)} ± ${unitDelta(m, 1.96 * c.se).replace(/^[+−]/, "")}`)),
        cu ? h("div", { class: "row" }, h("span", { class: "small", style: { width: "90px" } }, "With CUPED"), forest(core, { lo: cu.lo, hi: cu.hi, mid: cu.delta, truth: c.true_delta, min: dlo, max: dhi, color: toneColor(tone(m, cu.lo, cu.hi)), width: 240 }), h("span", { class: "small ex-num" }, `${unitDelta(m, cu.delta)} ± ${unitDelta(m, 1.96 * cu.se).replace(/^[+−]/, "")}`)) : null,
        cu ? h("p", { class: "ex-note" }, `With CUPED the smallest difference this test sees with 80% power is ${unitDelta(m, cu.mde).replace(/^[+−]/, "")}, against ${unitDelta(m, c.mde).replace(/^[+−]/, "")} without: as if it had ${Math.round(1 / Math.max(1 - cu.variance_reduction, 0.01) * 100 - 100)}% more users. p ${pText(cu.p)} against ${pText(c.p)}.`) : null) })));
  });
  if (m.histogram) cards.push(core.card({ title: "Each user's value", sub: "the share of users at each value, per arm (the top 1% left out)", body: histogram(ctx, m, rep) }));
  return h("div", { style: { display: "grid", gap: "14px" } }, cards);
}

function powerChart(ctx, m, c) {
  const { core } = ctx;
  const { h } = core;
  const se = c.se, z = 1.959963984540054;
  const Phi = (x) => 0.5 * (1 + erf(x / Math.SQRT2));
  const maxE = Math.max(Math.abs(c.true_delta ?? 0) * 1.6, c.mde * 1.6, 1e-9);
  const pts = range(0, 60).map((i) => { const e = (i / 60) * maxE; return [e, Phi(e / se - z) + Phi(-e / se - z)]; });
  const chart = lineChart(core, {
    series: [{ points: pts, color: "var(--s1)" }],
    hlines: [{ y: 0.8, color: "var(--muted)", label: "80%" }], yMin: 0, yMax: 1,
    vlines: [{ x: c.mde, color: "var(--s1)", label: "MDE" }, c.true_delta != null ? { x: Math.abs(c.true_delta), color: "var(--text)", label: "truth" } : null].filter(Boolean),
    yFmt: (v) => Math.round(v * 100) + "%", xFmt: (v) => unitDelta(m, v).replace(/^\+/, ""), xLabel: "effect", height: 180,
  });
  const lines = [h("p", { class: "ex-note" }, `With ${core.fmt.n(c.control.users + c.treatment.users)} users in these two arms the test sees a difference of ${unitDelta(m, c.mde).replace(/^[+−]/, "")} (${pctS(c.mde_lift).replace(/^\+/, "")}) 80% of the time.`)];
  if (c.true_delta != null) lines.push(h("p", { class: "ex-note" }, `The true difference is ${unitDelta(m, c.true_delta)}: this test had ${power(c.power)} power to see it${c.users_needed ? `; 80% would take about ${core.fmt.si(c.users_needed)} users` : ""}.`));
  return h("div", {}, chart, ...lines);
}

function erf(x) {
  // Abramowitz and Stegun 7.1.26.
  const t = 1 / (1 + 0.3275911 * Math.abs(x));
  const y = 1 - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t + 0.254829592) * t * Math.exp(-x * x);
  return x >= 0 ? y : -y;
}

function histogram(ctx, m, rep) {
  const { core } = ctx;
  const { h, s } = core;
  const hg = m.histogram;
  const W = 1000, H = 180, L = 40, B = 24, T = 6;
  const max = Math.max(...hg.share.flat(), 0.001);
  const bw = (W - L - 6) / hg.bins;
  const g = s("svg", { class: "ex-chart", viewBox: `0 0 ${W} ${H}`, preserveAspectRatio: "xMinYMid meet", style: { height: H + "px" } });
  for (const t of core.fmt.ticks(0, max, 3).filter((t) => t <= max)) {
    const y = T + (1 - t / max) * (H - T - B);
    g.append(s("line", { x1: L, x2: W - 6, y1: y, y2: y, class: "gr" }), s("text", { x: L - 6, y: y + 3, "text-anchor": "end" }, Math.round(t * 100) + "%"));
  }
  hg.share.forEach((sh, a) => {
    const pts = [];
    sh.forEach((v, i) => { const y = T + (1 - v / max) * (H - T - B); pts.push(`${L + i * bw},${y}`, `${L + (i + 1) * bw},${y}`); });
    g.append(s("polyline", { points: pts.join(" "), style: { fill: "none", stroke: ARM_COLORS[a], strokeWidth: 2 } }));
  });
  for (const t of core.fmt.ticks(hg.lo, hg.hi, 5).filter((t) => t >= hg.lo && t <= hg.hi)) {
    g.append(s("text", { x: L + ((t - hg.lo) / (hg.hi - hg.lo)) * (W - L - 6), y: H - 8, "text-anchor": "middle" }, m.unit === "share of searches" ? Math.round(t * 100) + "%" : core.fmt.tick(t)));
  }
  return h("div", {}, g, legend(h, rep.arms.map((a, i) => [ARM_COLORS[i], a.name])));
}

function cohorts(ctx, rep, id, cuttable) {
  const { core } = ctx;
  const { h } = core;
  let attr = "device", metric = rep.spec?.primary || "success", arm = 0, second = "";
  const box = h("div");
  const attrSeg = core.seg(rep.attributes.map((a) => [a, a]), attr, (v) => { attr = v; draw(); });
  const metricSel = h("select", { class: "select sm", onchange: () => { metric = metricSel.value; draw(); } }, rep.metrics.map((m) => h("option", { value: m.key, selected: m.key === metric }, m.title)));
  const armSel = rep.arms.length > 2 ? h("select", { class: "select sm", onchange: () => { arm = +armSel.value; draw(); } }, rep.arms.slice(1).map((a, i) => h("option", { value: i }, a.name))) : null;
  const secondSel = h("select", { class: "select sm", disabled: !cuttable, title: cuttable ? "" : "the app keeps the last three runs in memory for this: run it again to cut by two", onchange: () => { second = secondSel.value; draw(); } }, h("option", { value: "" }, "and by…"), rep.attributes.map((a) => h("option", { value: a }, a)));
  async function draw() {
    const m = rep.metrics.find((x) => x.key === metric);
    if (second && second !== attr) {
      box.replaceChildren(h("div", { class: "small dim" }, "Cutting…"));
      try {
        const t = await ctx.api(`/v1/experiments/${encodeURIComponent(id)}/cut`, { method: "POST", body: { by: [attr, second] } });
        box.replaceChildren(heat(ctx, m, t.metrics[metric][arm].cells, attr, second));
      } catch (e) { box.replaceChildren(h("div", { class: "error" }, e.message)); }
      return;
    }
    const cells = rep.cohorts[attr].metrics[metric][arm].cells;
    const [lo, hi] = domain(cells.flatMap((c) => [c.lift_lo, c.lift_hi, c.true_lift]).filter((x) => Number.isFinite(x) && Math.abs(x) < 3));
    const overall = m.comparisons[arm];
    const rows = cells.map((c) => {
      const t = tone(m, c.lift_lo, c.lift_hi);
      const sig = c.q < 0.05;
      return h("tr", {},
        h("td", { style: { fontWeight: 600 } }, c.cell),
        h("td", { class: "num ex-num" }, core.fmt.n(c.users)),
        h("td", { class: "num ex-num" }, unitValue(m, c.control.value)),
        h("td", { class: "num ex-num " + toneClass(t) }, pctS(c.lift)),
        h("td", {}, forest(core, { lo: c.lift_lo, hi: c.lift_hi, mid: c.lift, truth: c.true_lift, min: lo, max: hi, color: toneColor(t), width: 240 })),
        h("td", { class: "num ex-num" }, pText(c.p), h("div", { class: "small " + (sig ? "" : "dim") }, `q ${pText(c.q)}`)),
        h("td", { class: "num ex-num" }, power(c.power)),
        h("td", { class: "num ex-num" }, c.true_lift == null ? "–" : [pctS(c.true_lift, 2), h("div", { class: "small " + (c.covers ? "dim" : "ex-bad") }, c.covers ? "inside" : "outside")]));
    });
    const differ = cells.filter((c) => c.true_lift != null && overall.true_lift != null && Math.abs(c.true_lift - overall.true_lift) > Math.abs(overall.true_lift) * 0.5 + 0.005);
    box.replaceChildren(
      h("p", { class: "ex-note" }, `Overall: ${pctS(overall.lift)} (${pctS(overall.lift_lo)} to ${pctS(overall.lift_hi)}), truly ${pctS(overall.true_lift, 2)}. Each ${attr} below is a test of its own; q is Benjamini-Hochberg over the ${cells.length} cells, the one to read before believing a cohort differs.`, differ.length ? ` Truly different from the rest: ${differ.map((c) => c.cell).join(", ")}.` : ""),
      h("div", { class: "tbl-wrap" }, h("table", { class: "t" },
        h("thead", {}, h("tr", {}, h("th", {}, attr), h("th", { class: "num" }, "Users"), h("th", { class: "num" }, rep.arms[0].name), h("th", { class: "num" }, "Lift"), h("th", {}, "95% interval ◇ truth"), h("th", { class: "num" }, "p · q"), h("th", { class: "num" }, "Power"), h("th", { class: "num" }, "Truth"))),
        h("tbody", {}, rows))));
  }
  draw();
  return core.card({ title: "Cohorts", tools: h("div", { class: "row" }, attrSeg, secondSel, metricSel, armSel), body: box });
}

function heat(ctx, m, cells, a, b) {
  const { core } = ctx;
  const { h } = core;
  const rowsK = [...new Set(cells.map((c) => c.cell.split(" · ")[0]))];
  const colsK = [...new Set(cells.map((c) => c.cell.split(" · ")[1]))];
  const at = new Map(cells.map((c) => [c.cell, c]));
  const maxAbs = Math.max(...cells.map((c) => Math.abs(c.lift)).filter(Number.isFinite), 0.01);
  const color = (c) => {
    const good = (c.lift > 0) === m.higher;
    const k = Math.min(1, Math.abs(c.lift) / maxAbs);
    return `color-mix(in srgb, ${good ? "var(--good)" : "var(--crit)"} ${Math.round(20 + k * 70)}%, var(--surface))`;
  };
  return h("div", {},
    h("p", { class: "ex-note" }, `Lift of ${m.title.toLowerCase()} in each ${a} and ${b}; a bold cell has q below 0.05 over all ${cells.length} cells, a pale one too few users to say. Hover for the interval and the truth.`),
    h("div", { class: "tbl-wrap" }, h("table", { class: "ex-heat" },
      h("thead", {}, h("tr", {}, h("th", {}, `${a} \\ ${b}`), colsK.map((c) => h("th", {}, c)))),
      h("tbody", {}, rowsK.map((r) => h("tr", {}, h("th", { style: { textAlign: "right" } }, r), colsK.map((col) => {
        const c = at.get(`${r} · ${col}`);
        if (!c || c.treatment.users < 30) return h("td", { style: { background: "var(--surface-2)" }, title: "too few users" }, "·");
        const sig = c.q < 0.05;
        return h("td", { class: "c" + (sig ? "" : " ns"), style: { background: sig ? color(c) : `color-mix(in srgb, ${color(c)} 35%, var(--surface))`, fontWeight: sig ? 700 : 400 }, title: `${c.cell}: ${pctS(c.lift)} (${pctS(c.lift_lo)} to ${pctS(c.lift_hi)}), q ${pText(c.q)}, ${c.users} users; truly ${pctS(c.true_lift, 2)}` }, pctS(c.lift, 1));
      })))))));
}

function whatif(ctx, rep) {
  const { core } = ctx;
  const { h } = core;
  const w = rep.whatif;
  if (!w) return core.empty("info", "No exploring share", "This experiment kept no users exploring: give it an exploring share to value every arm from the log alone.");
  const reward = "clicks";
  const out = [h("p", { class: "ex-note" }, `${core.fmt.n(w.users)} users (${Math.round(w.share * 100)}%) were served the control through ${w.policy.policy} over its top ${w.policy.top} rows, and their ${core.fmt.si(w.searches)} searches logged with each row's chance of being where it was. From that log alone each estimator says what every arm would have got, here in clicks per search; beside it, what the arm measured and the truth for the exploring users. An estimator is shown when its weights are well supported; it holds when the click model is one it assumes.`)];
  const best = (a) => {
    const est = a.estimates.filter((e) => e.reward === reward && Number.isFinite(e.value));
    const by = (xs) => xs.slice().sort((x, y) => x.se - y.se)[0];
    return by(est.filter((e) => e.shown && e.holds)) || by(est.filter((e) => e.shown && e.estimator !== "naive" && !e.estimator.startsWith("dm-"))) || null;
  };
  const picks = w.arms.map(best);
  const base = w.arms[0], b0 = picks[0];
  const rows = w.arms.map((a, i) => {
    const e = picks[i];
    const lift = (x, y) => (Number.isFinite(x) && Number.isFinite(y) && y !== 0 ? x / y - 1 : null);
    return h("tr", {},
      h("td", { style: { color: ARM_COLORS[i], fontWeight: 600 } }, a.arm),
      h("td", {}, e ? [h("b", {}, e.estimator), " ", e.holds ? core.pill("ok", "holds") : core.pill("warn", "assumes position-based clicks")] : h("span", { class: "ex-bad small" }, "no estimator has the support: the log rarely shows its lists")),
      h("td", { class: "num ex-num" }, e ? `${fixed(e.value, 4)} ± ${fixed(1.96 * e.se, 4)}` : "–"),
      h("td", { class: "num ex-num" }, fixed(a.truth_explored[reward], 4)),
      i === 0 ? h("td", { class: "num dim" }, "–") : h("td", { class: "num ex-num" }, e && b0 ? pctS(lift(e.value, b0.value)) : "–"),
      i === 0 ? h("td", { class: "num dim" }, "–") : h("td", { class: "num ex-num" }, pctS(lift(a.measured[reward], base.measured[reward]))),
      i === 0 ? h("td", { class: "num dim" }, "–") : h("td", { class: "num ex-num" }, pctS(lift(a.truth[reward], base.truth[reward]), 2)));
  });
  out.push(core.card({ title: "What the log says before running", sub: "each arm's best supported estimate, against the A/B test and the truth", body: h("div", { class: "tbl-wrap" }, h("table", { class: "t" },
    h("thead", {}, h("tr", {}, h("th", {}, "Arm"), h("th", {}, "Estimator"), h("th", { class: "num" }, "Clicks per search, from the log"), h("th", { class: "num" }, "Truth, exploring users"), h("th", { class: "num" }, "Lift, from the log"), h("th", { class: "num" }, "Lift, A/B test"), h("th", { class: "num" }, "Lift, truth"))),
    h("tbody", {}, rows))) }));
  for (const a of w.arms) {
    const est = a.estimates.filter((e) => e.reward === reward);
    const truth = a.truth_explored[reward];
    const vals = est.flatMap((e) => [e.value - 1.96 * e.se, e.value + 1.96 * e.se]).concat([truth, a.measured[reward]]).filter(Number.isFinite);
    let lo = Math.min(...vals), hi = Math.max(...vals);
    const pad = (hi - lo) * 0.08 || 0.01; lo -= pad; hi += pad;
    const rows = est.map((e) => {
      const ok = e.shown && e.holds;
      const color = ok ? "var(--good)" : e.shown ? "var(--warn)" : "var(--muted)";
      const err = e.value - truth;
      return h("tr", {},
        h("td", { style: { fontWeight: 600 } }, e.estimator),
        h("td", {}, forest(core, { lo: e.value - 1.96 * e.se, hi: e.value + 1.96 * e.se, mid: e.value, truth, min: lo, max: hi, color, width: 260, zero: NaN })),
        h("td", { class: "num ex-num" }, fixed(e.value, 4), h("div", { class: "small dim" }, `± ${fixed(1.96 * e.se, 4)}`)),
        h("td", { class: "num ex-num " + (Math.abs(err) <= 1.96 * e.se ? "" : "ex-bad") }, signed(err, 4)),
        h("td", {}, e.shown ? core.pill("ok", "supported") : core.pill("warn", e.ess != null ? `ess ${Math.round(e.ess)}` : "no")),
        h("td", {}, e.holds ? core.pill("ok", "holds") : core.pill("info", "assumes another model")));
    });
    out.push(core.card({
      title: `${a.arm}`, sub: `truth for the exploring users ${fixed(truth, 4)} · measured in its arm ${fixed(a.measured[reward], 4)}${a.unsupported > 0 ? ` · ${pctS(a.unsupported, 0).replace("+", "")} of its slots never explored` : ""}`,
      body: h("div", { class: "tbl-wrap" }, h("table", { class: "t" }, h("thead", {}, h("tr", {}, h("th", {}, "Estimator"), h("th", {}, "95% interval ◇ truth"), h("th", { class: "num" }, "Value"), h("th", { class: "num" }, "Error"), h("th", {}, "Support"), h("th", {}, "Under " + (rep.spec?.model || "").toUpperCase()))), h("tbody", {}, rows))),
    }));
  }
  return h("div", { style: { display: "grid", gap: "14px" } }, out);
}

function setup(ctx, rep, r) {
  const { core } = ctx;
  const { h } = core;
  const sp = rep.spec || r.spec || {};
  const kv = (k, v) => h("tr", {}, h("th", { style: { textAlign: "left", width: "220px" } }, k), h("td", {}, v));
  return h("div", { style: { display: "grid", gap: "14px" } },
    core.card({ title: "Arms", body: h("div", { style: { display: "grid", gap: "10px" } }, rep.arms.map((a, i) => h("div", { class: "ex-arm", style: { "--arm": ARM_COLORS[i] } }, h("div", { class: "ex-arm-top" }, h("b", {}, a.name), h("span", { class: "small dim" }, `share ${a.share} · ${core.fmt.n(a.users)} users · ${core.fmt.si(a.searches)} searches`)), h("div", { class: "ex-sql" }, `SELECT id FROM ${rep.site.table} ORDER BY ${a.rank} DESC LIMIT ${sp.limit}`)))) }),
    core.card({ title: "Users and traffic", body: h("table", { class: "t compact" }, h("tbody", {},
      kv("Site", `${rep.site.name} (${rep.site.table}, ${rep.site.queries} queries ranked under every arm)`),
      kv("Users", core.fmt.n(sp.users)), kv("Searches", core.fmt.n(sp.impressions)), kv("Pre-period", `${core.fmt.n(rep.pre_searches)} searches under the control`),
      kv("Click model", (sp.model || "").toUpperCase()), kv("How much users differ", String(sp.appetite)), kv("Cohorts' tastes", String(sp.taste)), kv("Rows shown", String(sp.limit)),
      kv("Exploring", sp.explore_share ? `${Math.round(sp.explore_share * 100)}% of users, ${JSON.stringify(sp.explore)}` : "none"),
      kv("Mode", sp.mode === "live" ? "live: simulated users at a rate, every search a logged query to ChloeDB" : sp.mode === "served" ? "served: every search a logged query to ChloeDB" : "fast: drawn here from each arm's rows"),
      kv("Primary metric", sp.primary), kv("Seed", String(sp.seed)))) }),
    core.card({ title: "How it is computed", body: h("div", {},
      h("p", { class: "ex-note" }, "Users are split by a hash of who they are, so each stays in one arm. Each metric is a ratio over users (its total over their searches), and its standard error comes from the delta method over users, so one user's many searches are not taken for independent ones. CUPED adjusts each user's value by their pre-period's, with one coefficient for both arms. p values are two-sided z tests; q values are Benjamini-Hochberg over the metrics, and over the cells of a cohort. Power is for the true effect at this size; the MDE is the effect seen with 80% power. The always-valid p value is the mSPRT's with a normal mixture of a twentieth of the control's value."),
      h("p", { class: "ex-note" }, "The truth is exact: each user's expected search under each arm from the click model, weighted as the searches are drawn. ChloeDB ranks every query under every arm; in a served run every search is a logged query and its clicks are written back as events."))}));
}

// ------------------------------------------------------------ live

const clockText = (s) => { s = Math.max(0, Math.round(s || 0)); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`; };
const LIVE_CALLS = { "no detectable difference": ["flat", "No difference yet"] };
const STATUS_WORDS = { thinking: "thinking", searching: "searching", reading: "reading", starting: "starting", "between tasks": "between tasks", stopped: "stopped", "budget spent": "budget spent", failed: "failed" };

/** A small line of a series, with 0 marked. */
function spark(core, ys, color, width = 120, height = 26) {
  const { s } = core;
  const v = ys.map((y, i) => [i, y]).filter((p) => Number.isFinite(p[1]));
  const g = s("svg", { class: "ex-chart", viewBox: `0 0 ${width} ${height}`, width, height, style: { width: width + "px" } });
  if (v.length < 2) return g;
  const lo = Math.min(0, ...v.map((p) => p[1])), hi = Math.max(0, ...v.map((p) => p[1]));
  const X = (i) => 2 + (i / (ys.length - 1 || 1)) * (width - 4);
  const Y = (y) => 2 + (1 - (y - lo) / (hi - lo || 1)) * (height - 4);
  g.append(s("line", { x1: 2, x2: width - 2, y1: Y(0), y2: Y(0), class: "ax", "stroke-dasharray": "2 2" }));
  g.append(s("polyline", { points: v.map((p) => `${X(p[0])},${Y(p[1])}`).join(" "), style: { fill: "none", stroke: color, strokeWidth: 1.6 } }));
  return g;
}

/** An agent's screen: who they are, what they need, what they typed, the page they see, what they think. */
function agentCard(core, a, arms) {
  const { h } = core;
  const p = a.persona || {};
  const busy = ["thinking", "searching", "reading"].includes(a.status);
  const last = (a.steps || []).slice().reverse().find((x) => x.why || x.kind === "done" || x.kind === "cut" || x.kind === "say");
  const trail = (a.steps || []).map((x) => x.kind === "search" ? "searched" : x.kind === "open" ? `opened ${x.rank}` : x.kind === "done" ? (x.found ? "found it" : "gave up") : x.kind === "cut" ? "cut short" : "wrote");
  const lt = a.last;
  return h("div", { class: "ex-agent", style: { "--arm": ARM_COLORS[a.arm] } },
    h("div", { class: "row between" },
      h("b", {}, core.icon("agent"), ` Agent ${a.n + 1}`),
      h("span", { class: "small " + (a.status === "failed" ? "ex-bad" : "dim") + (busy ? " ex-busy" : "") }, STATUS_WORDS[a.status] || a.status)),
    h("div", { class: "who" },
      [p.age, p.gender, p.country, p.device, p.tenure].filter(Boolean).map((x) => h("span", { class: "ex-chip" }, x)),
      h("span", { class: "ex-chip", style: { background: ARM_COLORS[a.arm], color: "#fff" } }, arms[a.arm]?.name)),
    a.need ? h("div", { class: "ex-need" }, `“${a.need}”`) : null,
    h("div", { class: "ex-box" }, core.icon("search"), h("span", { class: "typed" }, a.typed || (a.need ? "…" : ""))),
    a.rows?.length ? h("ol", { class: "ex-results" }, a.rows.map((r, i) => h("li", { class: r.opened ? "opened" : "", title: r.snippet },
      h("span", { class: "n" }, i + 1), h("div", { class: `ex-dot g${r.grade || 0}`, title: `the judges' grade for this need: ${r.grade ?? "–"}` }), h("span", { class: "t" }, r.title || r.key), r.opened ? core.icon("eye") : h("span")))) : null,
    last ? h("div", { class: "ex-think" }, last.kind === "done" ? `${last.found ? "Found it" : "Gave up"}: ${last.text || ""}` : last.why || last.text) : null,
    trail.length ? h("div", { class: "ex-trail" }, trail.join(" → ")) : null,
    a.error ? h("div", { class: "small ex-bad" }, a.error) : null,
    h("div", { class: "row between small dim" },
      h("span", {}, `${a.tasks} tasks · ${a.found} found`),
      h("span", {}, a.cost ? `$${a.cost.toFixed(3)}` : "no model")),
    lt ? h("div", { class: "small dim", style: { lineHeight: 1.4 } }, `Last: “${(lt.need || "").slice(0, 70)}${(lt.need || "").length > 70 ? "…" : ""}”, ${lt.found ? (lt.relevant ? "found a relevant one" : "found one the judges don't grade relevant") : `not found (${lt.note || "gave up"})`} in ${lt.steps} steps`) : null);
}

/** Per arm, what the AI agents (and visitors) did. */
function agentTable(core, tallies, visitors, arms) {
  const { h } = core;
  const share = (a, b) => (b ? Math.round((a / b) * 100) + "%" : "–");
  const per = (a, b) => (b ? (a / b).toFixed(1) : "–");
  return h("div", { class: "tbl-wrap" }, h("table", { class: "t" },
    h("thead", {}, h("tr", {}, h("th", {}, "Arm"), h("th", { class: "num" }, "Tasks"), h("th", { class: "num", title: "the agent said a result it opened answered its need" }, "Found"), h("th", { class: "num", title: "of those, the result the judges grade relevant" }, "Relevant finds"), h("th", { class: "num" }, "Searches a task"), h("th", { class: "num" }, "Opens a task"), h("th", { class: "num", title: "opens of rows the judges grade relevant" }, "Relevant opens"), h("th", { class: "num", title: "nDCG of each task's first page" }, "First page nDCG"), h("th", { class: "num" }, "Cost"), visitors ? h("th", { class: "num" }, "Your searches · finds") : null)),
    h("tbody", {}, arms.map((arm, i) => {
      const t = tallies[i] || {};
      const v = visitors?.[i];
      return h("tr", {},
        h("td", { style: { color: ARM_COLORS[i], fontWeight: 600 } }, arm.name),
        h("td", { class: "num ex-num" }, t.tasks || 0),
        h("td", { class: "num ex-num" }, share(t.found, t.tasks)),
        h("td", { class: "num ex-num" }, share(t.found_relevant, t.tasks)),
        h("td", { class: "num ex-num" }, per(t.searches, t.tasks)),
        h("td", { class: "num ex-num" }, per(t.clicks, t.tasks)),
        h("td", { class: "num ex-num" }, share(t.relevant_clicks, t.clicks)),
        h("td", { class: "num ex-num" }, t.first_ndcg == null ? "–" : t.first_ndcg.toFixed(3)),
        h("td", { class: "num ex-num" }, `$${(t.cost || 0).toFixed(3)}`),
        visitors ? h("td", { class: "num ex-num" }, `${v?.searches || 0} · ${v?.found || 0}`) : null);
    }))));
}

/** A line of the feed. */
function feedLine(core, f, arms, fresh) {
  const { h } = core;
  const color = f.arm == null ? "var(--muted)" : ARM_COLORS[f.arm];
  let text;
  if (f.kind === "user") {
    const did = f.clicked?.length ? `clicked ${f.clicked.join(", ")}${f.satisfied?.length ? `, stayed on ${f.satisfied.join(", ")}` : ""}` : "no click";
    text = [h("b", {}, f.who), ` searched “${f.query}” · ${did}`, f.goes_on ? "" : " · left"];
  } else if (f.kind === "agent") {
    const who = h("b", {}, `Agent ${f.n + 1}`);
    if (f.action === "search") text = [who, ` searched “${f.text}”`, f.why ? h("span", { class: "dim" }, ` · ${f.why}`) : ""];
    else if (f.action === "open") text = [who, ` opened ${f.rank}: ${f.text}`, f.grade != null ? h("span", { class: "dim" }, ` · grade ${f.grade}`) : ""];
    else if (f.action === "done") text = [who, f.found ? (f.relevant ? " found it (relevant)" : " found it (the judges disagree)") : " gave up", f.text ? h("span", { class: "dim" }, ` · ${f.text}`) : ""];
    else text = [who, ` ${f.text}`];
  } else if (f.kind === "you") {
    text = [h("b", {}, "You"), f.action === "search" ? ` searched “${f.text}”` : f.action === "open" ? ` opened ${f.rank}: ${f.text}` : f.found ? " found it" : " found nothing"];
  } else {
    text = [h("i", {}, f.text)];
  }
  return h("div", { class: fresh ? "new" : "" }, h("span", { class: "tm" }, clockText(f.t)), h("span", { class: "d", style: { background: color } }), h("span", { class: "tx", title: f.arm != null ? arms[f.arm]?.name : "" }, text));
}

async function livePage(ctx, root, go, meta, id, onStop) {
  const { core } = ctx;
  const { h } = core;
  const path = `/v1/live/${encodeURIComponent(id)}`;
  let v;
  try { v = await ctx.api(path); }
  catch (e) { if (e.status === 404) { location.hash = ctx.href("e", id); return; } throw e; }
  const arms = v.arms || [];
  const k = arms.length;
  let points = v.points || [];
  let lastPoint = v.points_last || 0;
  let seq = v.seq || 0;
  let feed = (v.feed || []).slice().reverse().slice(0, 120);
  let metricKey = v.spec?.primary || "success";

  // The head: name, state, the rate, stop.
  const state = h("span");
  const rateOut = h("span", { class: "ex-num" });
  const rateIn = h("input", { type: "range", min: 0, max: RATES.length, step: 1 });
  const rateVals = [0, ...RATES];
  const setRateIn = (r) => { rateIn.value = String(Math.max(0, rateVals.findIndex((x) => x >= r))); rateOut.textContent = `${r} a second`; };
  setRateIn(v.rate || 0);
  let dragging = false;
  rateIn.oninput = () => { dragging = true; rateOut.textContent = `${rateVals[+rateIn.value]} a second`; };
  rateIn.onchange = async () => {
    try { const j = await ctx.api(`${path}/rate`, { method: "POST", body: { rate: rateVals[+rateIn.value] } }); setRateIn(j.rate); }
    catch (e) { core.toast(e.message, "error"); }
    dragging = false;
  };
  const rateBox = h("div", { class: "ex-rate", title: "Simulated searches a second" }, core.icon("user"), rateIn, rateOut);
  const stopBtn = h("button", { class: "btn sm danger" }, core.icon("stop"), "Stop");
  stopBtn.onclick = async () => { stopBtn.disabled = true; try { await ctx.api(`${path}/stop`, { method: "POST", body: {} }); } catch (e) { core.toast(e.message, "error"); stopBtn.disabled = false; } };
  const head = h("div", { class: "ex-head" },
    h("a", { class: "btn ghost sm", href: ctx.href(), title: "All experiments" }, "←"),
    h("h1", {}, v.name || id), state,
    h("span", { class: "sub" }, `${v.site} · ${arms.map((a) => a.name).join(" vs ")} · started ${core.fmt.datetime(v.created)}`),
    h("div", { class: "tools" }, rateBox, stopBtn));

  const bar = h("div", { style: { width: "0%", background: "var(--crit)" } });
  const clockTxt = h("span", { class: "txt" });
  const clock = h("div", { class: "ex-clock" }, h("div", { class: "ex-bar" }, bar), clockTxt);
  const banner = h("div");
  const kpiBox = h("div", { class: "ex-kpis" });
  const verdictBox = h("div", { class: "ex-verdicts" });
  const pick = h("div", { class: "ex-pick" });
  const chartL = h("div"), chartR = h("div"), chartNote = h("p", { class: "ex-note", style: { marginTop: "8px" } });
  const chartTitle = h("span"), pTitle = h("span", {}, "Peeking, safely");
  const table = h("div", { class: "tbl-wrap" });
  const agentsBox = h("div", { class: "ex-agents" });
  const agentScore = h("div");
  const feedBox = h("div", { class: "ex-feed" });

  // Try it yourself.
  let who = null;
  try { who = localStorage.getItem("ex-who"); } catch { /* none */ }
  if (!who) { who = "v" + Math.random().toString(36).slice(2, 10); try { localStorage.setItem("ex-who", who); } catch { /* none */ } }
  const tryIn = h("input", { class: "input", placeholder: "Search the site as a visitor…", style: { flex: "1", minWidth: "0" } });
  const tryBtn = h("button", { class: "btn" }, core.icon("search"), "Search");
  const tryOut = h("div", { class: "small dim" }, "You're a visitor too: the split puts you in an arm, and what you open counts in the agents' table.");
  const trySearch = async () => {
    const q = tryIn.value.trim();
    if (!q) return;
    tryBtn.disabled = true;
    try {
      const j = await ctx.api(`${path}/try`, { method: "POST", body: { who, q } });
      const doc = h("div");
      const list = h("ol", { class: "ex-results" }, j.rows.map((r, i) => {
        const li = h("li", { style: { cursor: "pointer" }, title: r.snippet, onclick: async () => {
          try {
            const d = await ctx.api(`${path}/try/open`, { method: "POST", body: { request: j.request, result: i + 1 } });
            li.classList.add("opened");
            const yes = h("button", { class: "btn sm primary" }, core.icon("check"), "This answered it");
            yes.onclick = async () => { try { await ctx.api(`${path}/try/done`, { method: "POST", body: { request: j.request, result: i + 1, found: true } }); doc.replaceChildren(h("div", { class: "small ex-good" }, "Counted: a find for " + j.arm_name + ".")); } catch (e) { core.toast(e.message, "error"); } };
            doc.replaceChildren(h("div", { class: "ex-doc" }, h("b", {}, d.title), "\n\n", d.body), h("div", { class: "row" }, yes, h("span", { class: "small dim" }, "or open another")));
          } catch (e) { core.toast(e.message, "error"); }
        } }, h("span", { class: "n" }, i + 1), h("span"), h("span", { class: "t" }, r.title || r.key), h("span"));
        return li;
      }));
      tryOut.replaceChildren(h("div", { class: "small", style: { marginBottom: "6px" } }, "Ranked by ", h("b", { style: { color: ARM_COLORS[j.arm] } }, j.arm_name), ", the arm you're in. Open one to read it."), j.rows.length ? list : h("div", { class: "small dim" }, "No results."), doc);
    } catch (e) { core.toast(e.message, "error"); }
    tryBtn.disabled = false;
  };
  tryBtn.onclick = trySearch;
  tryIn.onkeydown = (e) => { if (e.key === "Enter") trySearch(); };

  const agentsCard = core.card({ title: "AI agents on the site", sub: v.spec?.agents ? (v.brain === "scripted" ? "scripted: no model; each searches its need's words, opens the closest title and stops" : `played by ${(meta.live?.models || []).find((m) => m.id === v.brain)?.name || v.brain}, each a person of the population in their arm`) : "none in this run", body: h("div", { style: { display: "grid", gap: "12px", gridTemplateColumns: "minmax(0, 1fr)" } }, agentsBox, agentScore) });
  root.append(...[head, clock, banner, kpiBox, verdictBox,
    core.card({ title: h("span", {}, chartTitle), sub: "each arm against the control, as the searches come: the line, its 95% interval, and the truth", body: h("div", {}, pick, h("div", { class: "ex-g2" }, chartL, h("div", {}, h("div", { class: "small", style: { fontWeight: 600, marginBottom: "4px" } }, pTitle), chartR, chartNote))) }),
    core.card({ title: "Every metric, now", sub: "a row charts it above; ◇ marks the truth", body: table, flush: true }),
    v.spec?.agents ? agentsCard : null,
    h("div", { class: "ex-g3" },
      core.card({ title: "What's happening", sub: "a sample of the simulated users' searches, and everything the agents and you do", body: feedBox }),
      core.card({ title: "Try it yourself", body: h("div", { style: { display: "grid", gap: "8px", gridTemplateColumns: "minmax(0, 1fr)" } }, h("div", { class: "row nw" }, tryIn, tryBtn), tryOut) })),
    v.spec?.agents ? null : agentsCard].filter(Boolean));

  const pickMetric = (key) => { metricKey = key; drawChart(); drawTable(); };
  function drawPick() {
    pick.replaceChildren(...(v.metrics || []).map((m) => h("button", { type: "button", "aria-pressed": String(m.key === metricKey), onclick: () => pickMetric(m.key) }, m.title)));
  }

  function drawChart() {
    const mi = (v.metrics || []).findIndex((m) => m.key === metricKey);
    const m = v.metrics?.[mi];
    for (const b of pick.children) b.setAttribute("aria-pressed", String(b.textContent === m?.title));
    if (!m) { chartTitle.textContent = "Waiting for the first second"; return; }
    chartTitle.textContent = `${m.title}: the difference from ${arms[0].name}`;
    const pts = points.filter((p) => p.n > 0 && p.d?.[mi]);
    if (pts.length < 2) { chartL.replaceChildren(h("div", { class: "small dim" }, "The first points come in a second or two.")); chartR.replaceChildren(); return; }
    const series = [], pser = [];
    const hl = [{ y: 0, color: "var(--axis)", dash: "none" }];
    for (let a = 1; a < k; a++) {
      const color = ARM_COLORS[a];
      const d = pts.map((p) => [p.n, ...(p.d[mi][a - 1] || [])]);
      series.push({ color, band: d.filter((x) => Number.isFinite(x[2]) && Number.isFinite(x[3])).map((x) => [x[0], x[2], x[3]]), points: d.map((x) => [x[0], x[1]]) });
      pser.push({ color, points: d.filter((x) => Number.isFinite(x[4])).map((x) => [x[0], Math.max(x[4], 1e-12)]), width: 1.2, dash: "3 3" });
      pser.push({ color, points: d.filter((x) => Number.isFinite(x[5])).map((x) => [x[0], Math.max(x[5], 1e-12)]), width: 2.6 });
      const t = m.comparisons[a - 1]?.true_delta;
      if (t != null) hl.push({ y: t, color, label: k > 2 ? `truth ${arms[a].name}` : "truth" });
    }
    chartL.replaceChildren(lineChart(core, { series, hlines: hl, yFmt: (x) => unitDelta(m, x), xLabel: "searches", height: 230 }),
      legend(h, [...arms.slice(1).map((a, i) => [ARM_COLORS[i + 1], `${a.name}, with its 95% interval`]), ["var(--text)", "the truth (dashed)", true]]));
    const minP = Math.min(...pser.flatMap((sr) => sr.points.map((p) => p[1])), 0.01);
    chartR.replaceChildren(lineChart(core, { series: pser, yLog: true, yMax: 0, yMin: Math.max(-12, Math.floor(Math.log10(minP))), hlines: [{ y: 0.05, color: "var(--crit)", label: "0.05" }], yFmt: (x) => (x >= 0.01 ? String(x) : x.toExponential(0)), xLabel: "searches", height: 230 }),
      legend(h, [["var(--text-2)", "a peek's p value (fixed-size test)", true], ["var(--text-2)", "always-valid p value (mSPRT)"]]));
    const c = m.comparisons[0];
    const firstFixed = pts.find((p) => Number.isFinite(p.d[mi][0]?.[3]) && p.d[mi][0][3] < 0.05);
    chartNote.textContent = c ? `Watching a fixed-size test's p value and stopping when it dips under 0.05 calls far more false wins than 5%: here it first dipped at ${firstFixed ? core.fmt.si(firstFixed.n) + " searches" : "no point yet"}. The always-valid p value may be read at every second: it is ${pText(c.p_always_valid)} now.` : "";
  }

  function drawTable() {
    const ms = v.metrics || [];
    if (!ms.length) { table.replaceChildren(h("div", { class: "small dim", style: { padding: "12px" } }, v.stage || "Starting…")); return; }
    const lifts = ms.flatMap((m) => m.comparisons.flatMap((c) => [c.lift_lo, c.lift_hi, c.true_lift])).filter((x) => Number.isFinite(x) && Math.abs(x) < 2);
    const [lo, hi] = domain(lifts);
    const rows = [];
    ms.forEach((m, mi) => {
      m.comparisons.forEach((c, i) => {
        const t = c.p_always_valid < 0.05 ? tone(m, c.lift_lo, c.lift_hi) : "flat";
        const hist = points.filter((p) => p.n > 0).map((p) => p.d?.[mi]?.[i]?.[0]);
        rows.push(h("tr", { class: "click" + (m.key === metricKey ? " on" : ""), onclick: () => pickMetric(m.key) },
          i === 0 ? h("td", { rowspan: m.comparisons.length, class: "mname" }, m.title, m.guardrail ? h("span", { class: "pill warn", style: { marginLeft: "6px", fontSize: "10.5px" } }, "guardrail") : null, h("small", {}, m.higher ? "higher is better" : "lower is better")) : null,
          i === 0 ? h("td", { rowspan: m.comparisons.length, class: "num ex-num" }, unitValue(m, m.arms[0].value), m.arms[0].truth != null ? h("div", { class: "small dim" }, "◇ " + unitValue(m, m.arms[0].truth)) : null) : null,
          h("td", { class: "num ex-num", style: { color: k > 2 ? ARM_COLORS[i + 1] : null } }, unitValue(m, m.arms[i + 1].value), m.arms[i + 1].truth != null ? h("div", { class: "small dim" }, "◇ " + unitValue(m, m.arms[i + 1].truth)) : null),
          h("td", { class: "num ex-num " + toneClass(t) }, pctS(c.lift), h("div", { class: "small dim" }, `${pctS(c.lift_lo)} to ${pctS(c.lift_hi)}`)),
          h("td", {}, forest(core, { lo: c.lift_lo, hi: c.lift_hi, mid: c.lift, truth: c.true_lift, min: lo, max: hi, color: toneColor(t), width: 160 })),
          h("td", { class: "num ex-num" }, pText(c.p_always_valid), h("div", { class: "small dim" }, `peek ${pText(c.p)}`)),
          h("td", { class: "num ex-num" }, power(c.power), h("div", { class: "small dim" }, `MDE ${pctS(c.mde_lift)}`)),
          h("td", {}, spark(core, hist, toneColor(t) === "var(--muted)" ? ARM_COLORS[i + 1] : toneColor(t)))));
      });
    });
    table.replaceChildren(h("table", { class: "t ex-score" },
      h("thead", {}, h("tr", {}, h("th", {}, "Metric"), h("th", { class: "num" }, arms[0].name), h("th", { class: "num" }, k > 2 ? "Arm" : arms[1].name), h("th", { class: "num" }, "Lift, 95%"), h("th", {}, "Interval ◇ truth"), h("th", { class: "num", title: "always-valid (mSPRT), safe to read at any time; and a fixed-size test's, which is not" }, "p, always valid"), h("th", { class: "num", title: "the chance a test of this size sees the true effect, and the smallest lift it would see 80% of the time" }, "Power now"), h("th", {}, "Difference so far"))),
      h("tbody", {}, rows)));
  }

  function drawVerdicts() {
    const ms = v.metrics || [];
    const pm = ms.find((m) => m.key === v.spec?.primary) || ms[0];
    if (!pm || !v.verdicts) { verdictBox.replaceChildren(); return; }
    verdictBox.replaceChildren(...v.verdicts.map((vd, i) => {
      const c = pm.comparisons[i];
      const t = c.p_always_valid < 0.05 ? tone(pm, c.lift_lo, c.lift_hi) : "flat";
      const [cls, text] = (v.status === "live" && LIVE_CALLS[vd.call]) || CALLS[vd.call] || ["flat", vd.call];
      const [lo, hi] = domain([c.lift_lo, c.lift_hi, vd.true_lift ?? 0].filter(Number.isFinite));
      return h("div", { class: "ex-verdict", style: { borderTop: `3px solid ${ARM_COLORS[i + 1]}` } },
        h("div", { class: "row between" }, h("b", {}, `${vd.arm} against ${arms[0].name}`), h("span", { class: "ex-call " + cls }, text)),
        h("div", { class: "row", style: { alignItems: "baseline", gap: "10px" } }, h("span", { class: "big " + toneClass(t) }, pctS(c.lift)), h("span", { class: "small dim" }, `${pm.title}, 95% interval ${pctS(c.lift_lo)} to ${pctS(c.lift_hi)}`)),
        forest(core, { lo: c.lift_lo, hi: c.lift_hi, mid: c.lift, truth: vd.true_lift, min: lo, max: hi, color: toneColor(t), width: 300, height: 26 }),
        h("div", { class: "line" }, `Always-valid p ${pText(c.p_always_valid)}, safe to read now · a peek's p ${pText(c.p)}, which is not`),
        vd.true_lift != null ? h("div", { class: "line" }, h("b", {}, `The truth: ${pctS(vd.true_lift, 2)}`), ` (${vd.truth_call}) · power now ${power(vd.power)}`, vd.guardrails_worse?.length ? ` · guardrails worse: ${vd.guardrails_worse.join(", ")}` : "") : null);
    }));
  }

  function drawKpis() {
    const kp = (key, val, title, color) => h("div", { class: "ex-kpi", title }, h("div", { class: "k", style: color ? { color, fontWeight: 600 } : null }, key), h("div", { class: "v" }, val));
    const tally = v.agent_tally || [];
    const tasks = tally.reduce((a, t) => a + (t.tasks || 0), 0), found = tally.reduce((a, t) => a + (t.found || 0), 0);
    kpiBox.replaceChildren(...[
      kp("Searches", core.fmt.si(v.searches || 0), "simulated users' searches, each a logged query"),
      v.status === "live" ? kp("Now", `${(v.achieved || 0).toFixed(v.achieved < 10 ? 1 : 0)} a second`, `asked: ${v.rate} a second${v.errors ? `; ${v.errors} failed, the last: ${v.last_error}` : ""}`) : kp("Ran for", clockText(v.elapsed), v.errors ? `${v.errors} searches failed, the last: ${v.last_error}` : "no search failed"),
      kp("Users so far", core.fmt.n(v.users_seen || 0), `of ${core.fmt.n(v.spec?.users || 0)}`),
      ...arms.map((a, i) => kp(a.name, `${core.fmt.n(v.arm_users?.[i] || 0)} users`, "", ARM_COLORS[i])),
      v.srm ? kp("Sample ratio", v.srm.ok ? "as planned" : "MISMATCH", `chi-square ${fixed(v.srm.chi2, 2)}, p ${pText(v.srm.p)}`) : null,
      v.spec?.agents ? kp("AI agents", `${found} of ${tasks} found`, "tasks the agents ended saying a result answered their need") : null,
      v.spec?.agents && v.brain !== "scripted" ? kp("Spent", `$${(v.cost || 0).toFixed(2)} of $${v.budget}`, "worked out from the tokens each call reports") : null].flat().filter(Boolean));
  }

  function drawFeed(fresh) {
    feedBox.replaceChildren(...feed.map((f) => feedLine(core, f, arms, fresh.has(f.seq))));
  }

  function drawAll(fresh = new Set()) {
    const st = v.status;
    state.replaceChildren(statusPill(core, { status: st }));
    stopBtn.style.display = st === "live" || st === "starting" ? "" : "none";
    rateBox.style.display = st === "live" || st === "starting" ? "" : "none";
    if (!dragging && document.activeElement !== rateIn) setRateIn(v.rate || 0);
    const share = Math.min(1, (v.elapsed || 0) / (v.limit_s || 1));
    bar.style.width = Math.round(share * 100) + "%";
    clockTxt.textContent = `${clockText(v.elapsed)} of ${clockText(v.limit_s)}`;
    if (st === "starting") banner.replaceChildren(h("p", { class: "ex-note" }, `Starting: ${v.stage || ""}…`));
    else if (st === "ending") banner.replaceChildren(h("p", { class: "ex-note" }, `Ending: ${v.stage || "writing the report"}…`));
    else if (st === "done") banner.replaceChildren(h("div", { class: "ex-verdict", style: { marginBottom: "14px" } }, h("div", { class: "row between" }, h("b", {}, "The run is over: it is an experiment now."), h("a", { class: "btn primary", href: ctx.href("e", id) }, "Open the report")), h("div", { class: "line" }, "The report has every metric of every user with its distribution, the cohorts, the series over time and the truth.")));
    else if (st === "failed") banner.replaceChildren(h("div", { class: "error", style: { marginBottom: "14px" } }, `This live run failed: ${v.error}`));
    else banner.replaceChildren();
    drawKpis();
    drawVerdicts();
    if (pick.children.length !== (v.metrics || []).length) drawPick();
    drawChart();
    drawTable();
    agentsBox.replaceChildren(...(v.agents || []).map((a) => agentCard(core, a, arms)));
    agentScore.replaceChildren(v.spec?.agents || (v.visitor_tally || []).some((t) => t.searches) ? agentTable(core, v.agent_tally || [], v.visitor_tally, arms) : h("span"));
    drawFeed(fresh);
  }
  drawAll();
  if (v.status === "done" || v.status === "failed") return;
  const stop = core.every(1200, async () => {
    let n;
    try { n = await ctx.api(`${path}?after=${lastPoint}&since=${seq}`); }
    catch (e) { if (e.status === 404) { stop(); location.hash = ctx.href("e", id); } return; }
    if (n.points?.length) points = points.concat(n.points);
    if (points.length > 2400) points = points.filter((_, i) => i % 2 === 1);
    lastPoint = n.points_last || lastPoint;
    const fresh = new Set();
    for (const f of n.feed || []) { feed.unshift(f); fresh.add(f.seq); }
    feed = feed.slice(0, 120);
    seq = n.seq || seq;
    v = n;
    drawAll(fresh);
    if (n.status === "done" || n.status === "failed") stop();
  }, false, { always: true });
  onStop(stop);
}

/** The Agents tab of a live run's report. */
function agentsReport(ctx, rep) {
  const { core } = ctx;
  const { h } = core;
  const l = rep.live || {};
  const arms = rep.arms || [];
  return h("div", { style: { display: "grid", gap: "14px", gridTemplateColumns: "minmax(0, 1fr)" } },
    core.card({ title: "What the AI agents did, per arm", sub: l.brain === "scripted" ? "scripted agents: no model" : `played by ${l.brain}; $${(l.cost || 0).toFixed(3)} of $${l.budget} spent`, body: agentTable(core, l.agent_tally || [], l.visitor_tally, arms) }),
    (l.agents || []).length ? core.card({ title: "The agents at the end", body: h("div", { class: "ex-agents" }, l.agents.map((a) => agentCard(core, a, arms))) }) : null,
    core.card({ title: "How it ran", body: h("p", { class: "ex-note" }, `${core.fmt.si(rep.searches)} simulated searches over ${clockText(l.elapsed)} (asked at ${l.rate} a second at the end), every one a logged query to ChloeDB whose clicks were written back as events. The agents' searches are scored apart: they are a handful, there to be read, and the judges' grades say whether what they opened was relevant.`) }));
}

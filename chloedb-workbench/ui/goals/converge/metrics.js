// Metrics (under "⋯"): four tiles, open issues over time, the pipeline,
// where it hurts, spend and runs, each persona's ease. Every chart is
// inline SVG drawn at its box's real width, so its labels stay 11 px.
import { h, put, enc, SEV, THEME_WORDS, toMs, usd, secsWords } from "/goals/converge/util.js";
import { useStyle } from "/levels/common.js";
import { stepHead } from "/goals/converge/rows.js";

const CSS = `
.cv-panel.wide { grid-column: 1 / -1; }
.cv-tl-svg { width: 100%; height: auto; display: block; margin-top: 8px; }
.cv-legend { display: flex; flex-wrap: wrap; gap: 12px; font-size: var(--fs-xs); color: var(--text-2); margin-top: 4px; }
.cv-legend span { display: inline-flex; align-items: center; gap: 5px; }
.cv-legend i { width: 10px; height: 10px; border-radius: 2px; display: inline-block; }
.cv-legend i.ln { width: 16px; height: 0; border-top: 2px solid; border-radius: 0; }
.cv-panels { display: grid; grid-template-columns: repeat(auto-fit, minmax(340px, 1fr)); gap: 12px; }
.cv-panel { background: var(--surface); border: var(--hair) solid var(--line); border-radius: var(--r); padding: 14px; min-width: 0; }
.cv-panel h3 { margin: 0 0 2px; font-size: var(--fs-body); }
.cv-panel .sub { color: var(--text-2); font-size: var(--fs-sm); margin-bottom: 10px; }
.cv-t { width: 100%; border-collapse: collapse; font-size: var(--fs-sm); }
.cv-t th { text-align: left; font-weight: var(--w-mid); color: var(--muted); font-size: var(--fs-xs); font-family: var(--mono); text-transform: uppercase; letter-spacing: .06em; padding: 6px 8px; border-bottom: var(--hair) solid var(--line); }
.cv-t td { padding: 7px 8px; border-bottom: var(--hair) solid var(--line); font-variant-numeric: tabular-nums; }
.cv-t td.n { font-family: var(--mono); text-align: right; }
.cv-hbar { height: 10px; border-radius: 3px; background: var(--accent); opacity: .8; }
/* Dashboard */
.cv-outs { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 12px; margin-bottom: var(--s-5); }
.cv-out { display: flex; flex-direction: column; gap: 2px; padding: 14px 16px; border-radius: var(--r); border: var(--hair) solid var(--line); background: var(--surface); min-width: 0; }
.cv-out .v { font-family: var(--mono); font-size: 28px; line-height: 1.15; font-variant-numeric: tabular-nums; }
.cv-out .v .ar { color: var(--muted); margin: 0 6px; } .cv-out .v .up { color: var(--good-ink); } .cv-out .v .down { color: var(--crit-ink); }
.cv-out .sub { font-size: var(--fs-xs); line-height: var(--lh-xs); color: var(--text-2); }
.cv-acth { margin: var(--s-4) 0 var(--s-2); font-size: var(--fs-md); }
@media (max-width: 900px) { .cv-outs { grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; } .cv-out { padding: 10px 12px; } .cv-out .v { font-size: 22px; } }
.cv-dash-bar { gap: 10px; flex-wrap: wrap !important; }
.cv-dash-bar > .cv-meta { flex: 1 1 100%; }
.cv-kpis { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 12px; margin-bottom: 12px; }
.cv-kpi { display: flex; flex-direction: column; gap: 2px; align-items: flex-start; text-align: left; padding: 12px 14px; min-height: 112px; border-radius: var(--r); border: var(--hair) solid var(--line); background: var(--surface); color: var(--text); font: inherit; cursor: pointer; min-width: 0; }
.cv-kpi:hover { border-color: var(--accent-line); }
.cv-kpi .v { font-family: var(--mono); font-size: var(--fs-lg); line-height: var(--lh-lg); font-variant-numeric: tabular-nums; }
.cv-kpi .sub { font-size: var(--fs-xs); line-height: var(--lh-xs); color: var(--text-2); }
.cv-kpi .kd { font-family: var(--mono); } .cv-kpi .kd.good { color: var(--good-ink, var(--good)); } .cv-kpi .kd.bad { color: var(--crit-ink, var(--crit)); }
.cv-ks { width: 100%; height: 24px; display: block; margin-top: auto; }
.cv-budget { width: 100%; height: 8px; border-radius: 4px; background: var(--surface-3); overflow: hidden; margin-top: auto; }
.cv-budget i { display: block; height: 100%; background: var(--accent); border-radius: 4px; } .cv-budget i.over { background: var(--cv-s4); }
.cv-dash { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
.cv-ph { margin-bottom: 10px; } .cv-ph h3 { margin: 0; }
.cv-ph .sub { margin: 2px 0 0; }
.cv-plot { position: relative; width: 100%; touch-action: pan-y; }
.cv-plot svg { display: block; overflow: visible; }
.cv-plot .gl { stroke: var(--line); stroke-width: 1; }
.cv-plot .ax { font-size: 11px; fill: var(--muted); font-family: var(--mono); }
.cv-plot .xh { stroke: var(--text-2); stroke-width: 1; stroke-dasharray: 3 3; }
.cv-tip { position: absolute; top: 0; z-index: 2; pointer-events: none; min-width: 140px; background: var(--surface); border: var(--hair) solid var(--line-2); box-shadow: var(--shadow-2); border-radius: 8px; padding: 6px 8px; font-size: var(--fs-xs); }
.cv-tip[hidden] { display: none; }
.cv-tip .h { font-family: var(--mono); color: var(--muted); margin-bottom: 2px; }
.cv-tip .r { display: flex; align-items: center; gap: 6px; } .cv-tip .r span { flex: 1; } .cv-tip .r b { font-family: var(--mono); font-weight: var(--w-mid); }
.cv-tip .r i { width: 8px; height: 8px; border-radius: 2px; } .cv-tip .r i.none { background: none; }
.cv-legend { align-items: center; margin: 0 0 8px; }
.cv-lg { display: inline-flex; align-items: center; gap: 6px; min-height: 32px; padding: 0 10px; border-radius: var(--r-pill); border: var(--hair) solid var(--line-2); background: var(--surface); color: var(--text); font: inherit; font-size: var(--fs-xs); cursor: pointer; }
.cv-lg.off { opacity: .45; text-decoration: line-through; }
.cv-lg i { width: 10px; height: 10px; border-radius: 2px; display: inline-block; }
.cv-lg i.dot { border-radius: 50%; border: 1.5px solid; background: var(--surface); }
.cv-lg i.ln, .cv-legend i.ln { width: 16px; height: 0; border-top: 2px solid; border-radius: 0; background: none; }
.cv-legend i.ln.dash { border-top-style: dashed; }
.cv-pipe { list-style: none; margin: 0 0 8px; padding: 0; display: flex; flex-direction: column; gap: 6px; }
.cv-pipe li { display: grid; grid-template-columns: 92px minmax(0, 1fr) 34px 92px; gap: 8px; align-items: center; font-size: var(--fs-sm); }
.cv-pipe .nm { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cv-pipe .br { height: 12px; background: var(--surface-3); border-radius: 3px; overflow: hidden; }
.cv-pipe .br i { display: block; height: 100%; background: var(--accent); border-radius: 3px; }
.cv-pipe .n { font-family: var(--mono); text-align: right; }
.cv-pipe .pc { font-family: var(--mono); font-size: var(--fs-xs); color: var(--muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.cv-hurt { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 2px; }
.cv-hurt button { width: 100%; min-height: 40px; display: grid; grid-template-columns: 104px minmax(0, 1fr) 48px; gap: 8px; align-items: center; border: 0; background: none; padding: 2px 4px; border-radius: 6px; font: inherit; font-size: var(--fs-sm); color: var(--text); cursor: pointer; text-align: left; }
.cv-hurt button:hover { background: var(--surface-2); }
.cv-hurt .nm { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cv-hurt .br { display: flex; height: 14px; border-radius: 3px; overflow: hidden; gap: 1px; min-width: 4px; }
.cv-hurt .n { font-family: var(--mono); text-align: right; } .cv-hurt .b { color: var(--cv-s4); }
.cv-ease { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
.cv-ease li { display: grid; grid-template-columns: minmax(0, 1fr) 90px 34px; gap: 10px; align-items: center; min-height: 40px; }
.cv-ease .nm { display: flex; flex-direction: column; min-width: 0; font-weight: var(--w-mid); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cv-ease .rl { font-size: var(--fs-xs); color: var(--muted); font-weight: var(--w-text); }
.cv-ease .v { font-family: var(--mono); text-align: right; }
.cv-raw > summary { cursor: pointer; font-weight: var(--w-mid); min-height: 36px; display: flex; align-items: center; }
.cv-rawg { display: grid; grid-template-columns: repeat(auto-fit, minmax(280px, 1fr)); gap: 16px; margin-top: 8px; }
.cv-rawg h3 { font-size: var(--fs-sm); margin: 0 0 6px; }
.cv-tscroll { overflow-x: auto; }
@media (max-width: 900px) { .cv-kpis { grid-template-columns: repeat(3, minmax(0, 1fr)); } .cv-dash { grid-template-columns: 1fr; } }
.cv-chipf { min-height: 36px; padding: 0 12px; border-radius: var(--r-pill); border: var(--hair) solid var(--line-2); background: var(--surface); color: var(--text-2); font: inherit; font-size: var(--fs-sm); cursor: pointer; }
.cv-chipf.on { border-color: var(--accent-line); background: var(--accent-soft); color: var(--accent-text); }
.cv-seg { display: inline-flex; border: var(--hair) solid var(--line-2); border-radius: var(--r-sm); overflow: hidden; }
.cv-seg button { white-space: nowrap; border: 0; background: var(--surface); padding: 0 12px; min-height: 36px; font: inherit; color: var(--text-2); cursor: pointer; }
.cv-seg button.on { background: var(--accent-soft); color: var(--accent-text); font-weight: var(--w-mid); }
.cv-bar { display: flex; gap: var(--s-2); align-items: center; flex-wrap: wrap; margin-bottom: var(--s-3); }
@media (max-width: 640px) { .cv-seg button, .cv-chipf { min-height: 44px; } .cv-panel { padding: 12px; } .cv-pipe li { grid-template-columns: 76px minmax(0, 1fr) 28px 64px; gap: 6px; } .cv-hurt button { grid-template-columns: 88px minmax(0, 1fr) 40px; } .cv-kpi { min-height: 104px; padding: 10px 12px; } .cv-kpis { gap: 8px; } }
`;

export function metricsScreen(C) {
  useStyle("converge-metrics", CSS);
  const S = C.S;
  S.mc = S.mc || new Set();
  const repo = C.repo, view = C.view, api = C.api, draw = C.draw, keep = C.keep;
  const matches = (f) => !S.mc.size || S.mc.has(f.component || "");
  const anyFilter = () => S.mc.size > 0;
  const components = () => [...new Set([...(S.d.components || []).map((c) => (typeof c === "string" ? c : c && c.name)), ...(S.d.findings || []).map((f) => f.component)].filter(Boolean))].sort().map((name) => ({ name }));
  /** The component filter: one row of chips (the only filter here). */
  const filterStrip = () => {
    const comps = components();
    if (!comps.length) return null;
    const flip = (k) => { if (S.mc.has(k)) S.mc.delete(k); else S.mc.add(k); keep(); draw(); };
    return h("div", { class: "cv-chips", role: "group", "aria-label": "Component" }, h("button", { class: `cv-chipf${S.mc.size ? "" : " on"}`, type: "button", onclick: () => { S.mc.clear(); keep(); draw(); } }, "All components"), comps.map((c) => h("button", { class: `cv-chipf${S.mc.has(c.name) ? " on" : ""}`, type: "button", "aria-pressed": String(S.mc.has(c.name)), onclick: () => flip(c.name) }, c.name)));
  };
  // ------------------------------------------------------------ metrics
  // A dashboard: four tiles, the hero chart of open issues, the pipeline,
  // where it hurts, spend and runs, and each persona's ease. Every chart is
  // inline SVG drawn at its box's real width, so its labels stay 11 px.
  const SEVS = [[4, "blocker"], [3, "major"], [2, "minor"], [1, "papercut"]];
  const sevVar = (k) => `var(--cv-s${k})`;
  const RANGES = [["24h", "24 h"], ["7d", "7 d"], ["30d", "30 d"]];
  const RANGE_MS = { "24h": 86400000, "7d": 7 * 86400000, "30d": 30 * 86400000 };
  function niceTicks(max, n = 4, int = false) {
    if (!(max > 0)) return [0, 1];
    const raw = max / n, mag = 10 ** Math.floor(Math.log10(raw));
    let step = [1, 2, 2.5, 5, 10].map((x) => x * mag).find((x) => x >= raw) || raw;
    if (int) step = Math.max(1, Math.ceil(step));
    const out = []; for (let v = 0; v <= max + step * 0.001; v += step) out.push(+v.toFixed(6));
    if (out.at(-1) < max) out.push(+(out.at(-1) + step).toFixed(6));
    return out.slice(0, 6);
  }
  const fmtT = (ms, range) => { const d = new Date(ms); return range === "24h" ? `${String(d.getHours()).padStart(2, "0")}:00` : d.toLocaleDateString([], { month: "short", day: "numeric" }); };

  /** A chart box: drawn at its width, redrawn when that changes, with a
   *  crosshair and a tooltip that follow a finger or the mouse. */
  function plot(o) {
    const box = h("div", { class: "cv-plot", style: { height: `${o.H}px` } });
    const tip = h("div", { class: "cv-tip", role: "status", hidden: true });
    let w = 0;
    const render = () => {
      w = box.clientWidth || 320;
      const L = o.L ?? 34, R = o.R ?? 10, T = 10, B = 24, H = o.H;
      const n = Math.max(1, o.n);
      const step = (w - L - R) / n;
      const X = (i) => L + (i + 0.5) * step;
      const ticks = niceTicks(o.yMax, 4, o.int !== false);
      const top = ticks.at(-1) || 1;
      const Y = (v) => T + (H - T - B) * (1 - v / top);
      let g = "";
      for (const v of ticks) g += `<line x1="${L}" x2="${w - R}" y1="${Y(v)}" y2="${Y(v)}" class="gl"/><text x="${L - 6}" y="${Y(v) + 4}" text-anchor="end" class="ax">${o.yFmt ? o.yFmt(v) : v}</text>`;
      const every = Math.max(1, Math.ceil(n / 5));
      for (let i = 0; i < n; i += every) g += `<text x="${X(i)}" y="${H - 6}" text-anchor="middle" class="ax">${o.xLabel(i)}</text>`;
      if (o.y2) { const t2 = niceTicks(o.y2.max, 4); const top2 = t2.at(-1) || 1; o.y2.Y = (v) => T + (H - T - B) * (1 - v / top2); for (const v of t2) g += `<text x="${w - 2}" y="${o.y2.Y(v) + 4}" text-anchor="end" class="ax">${o.y2.fmt(v)}</text>`; }
      g += o.series({ X, Y, step, L, R, T, B, w, H, n });
      g += `<line class="xh" x1="0" x2="0" y1="${T}" y2="${H - B}" style="display:none"/>`;
      const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
      svg.setAttribute("viewBox", `0 0 ${w} ${H}`); svg.setAttribute("width", String(w)); svg.setAttribute("height", String(H));
      svg.setAttribute("role", "img"); svg.setAttribute("aria-label", o.label || "");
      svg.innerHTML = g;
      const xh = svg.querySelector(".xh");
      const at = (ev) => {
        const r = svg.getBoundingClientRect();
        const i = Math.max(0, Math.min(n - 1, Math.floor((ev.clientX - r.left - L) / step)));
        xh.setAttribute("x1", X(i)); xh.setAttribute("x2", X(i)); xh.style.display = "";
        put(tip, ...o.tip(i)); tip.hidden = false;
        const tw = tip.offsetWidth || 160;
        tip.style.left = `${Math.max(0, Math.min(w - tw, X(i) - tw / 2))}px`;
      };
      const off = () => { xh.style.display = "none"; tip.hidden = true; };
      svg.addEventListener("pointermove", at); svg.addEventListener("pointerdown", at); svg.addEventListener("pointerleave", (e) => { if (e.pointerType === "mouse") off(); });
      put(box, svg, tip);
    };
    box.redraw = render;
    if (typeof ResizeObserver === "function") { const ro = new ResizeObserver(() => { if (box.isConnected && Math.abs((box.clientWidth || 0) - w) > 2) render(); }); ro.observe(box); (S.ros = S.ros || []).push(ro); }
    else requestAnimationFrame(render);
    return box;
  }
  const tipRow = (color, label, v) => h("div", { class: "r" }, color ? h("i", { style: { background: color } }) : h("i", { class: "none" }), h("span", {}, label), h("b", {}, String(v)));

  /** Tiny sparkline for a tile. */
  function sparkOf(vals, color) {
    const v = (vals || []).filter((x) => Number.isFinite(x));
    if (v.length < 2) return h("span", { class: "cv-ks" });
    const max = Math.max(...v), min = Math.min(...v), W = 100, H = 24;
    const X = (i) => (i / (v.length - 1)) * W, Y = (x) => H - 2 - (max === min ? (H - 4) / 2 : ((x - min) / (max - min)) * (H - 4));
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("viewBox", `0 0 ${W} ${H}`); svg.setAttribute("preserveAspectRatio", "none"); svg.setAttribute("class", "cv-ks"); svg.setAttribute("aria-hidden", "true");
    svg.innerHTML = `<polyline points="${v.map((x, i) => `${X(i)},${Y(x)}`).join(" ")}" fill="none" style="stroke:${color || "var(--accent)"}" stroke-width="1.5" vector-effect="non-scaling-stroke"/>`;
    return svg;
  }

  function timeline() {
    const range = S.range || "7d";
    const comp = [...S.mc].join(",");
    const key = `${range}|${comp}`;
    if (S.tl && S.tlKey === key && Date.now() - S.tlAt < 30000) return S.tl;
    if (!S.tlBusy || S.tlBusyKey !== key) {
      S.tlBusy = true; S.tlBusyKey = key;
      api(`/api/converge/timeline?repo=${enc(repo())}&range=${range}${comp ? `&component=${enc(comp)}` : ""}`)
        .then((t) => { S.tl = t; S.tlKey = key; S.tlAt = Date.now(); if (S.alive && view() === "metrics") draw(); })
        .catch(() => {}).finally(() => { S.tlBusy = false; });
    }
    return S.tlKey === key ? S.tl : (S.tl && S.tl.range === range ? S.tl : null);
  }

  /** The outcomes (GET /api/converge/metrics → outcomes), read every 30 s. */
  function outcomesData() {
    const own = S.d.metrics && S.d.metrics.outcomes;
    if (own && typeof own === "object") return own;
    if (!S.moBusy && (!S.moAt || Date.now() - S.moAt > 30000)) {
      S.moBusy = true;
      api(`/api/converge/metrics?repo=${enc(repo())}`, { quiet: true }).then((x) => { S.mo = x && x.outcomes && typeof x.outcomes === "object" ? x.outcomes : null; S.moAt = Date.now(); if (S.alive && view() === "metrics" && S.mo) draw(); }, () => { S.moAt = Date.now(); }).finally(() => { S.moBusy = false; });
    }
    return S.mo || null;
  }

  /** What Converge got done: merged this week, found → merged, cost per fix, ease. */
  function outcomes() {
    const o = outcomesData() || {};
    const d = S.d, m = d.metrics || {};
    const week = Date.now() - 7 * 86400000;
    const merged = (d.findings || []).filter((f) => f.stage === "merged");
    const times = merged.map((f) => (toMs(f.stage_since) || 0) - (toMs(f.created) || 0)).filter((x) => x > 0).sort((a, b) => a - b);
    const num = (x) => (x != null && Number.isFinite(Number(x)) ? Number(x) : null);
    const mergedWeek = num(o.merged_7d) ?? merged.filter((f) => (toMs(f.stage_since) || 0) >= week).length;
    const median = num(o.median_found_to_merged_ms) ?? (times.length ? times[Math.floor(times.length / 2)] : null);
    const cost = num(o.cost_per_merged_fix_usd) ?? num(m.cost_per_merged_fix);
    const eb = num(o.ease_before), ea = num(o.ease_after);
    const tile = (label, v, sub) => h("div", { class: "cv-out" }, h("span", { class: "cv-label" }, label), h("span", { class: "v" }, v), h("span", { class: "sub" }, sub));
    const fx = (x) => (Math.round(x * 10) / 10).toFixed(1);
    return h("section", { "aria-label": "Outcomes" }, h("div", { class: "cv-outs" },
      tile("Merged this week", String(mergedWeek), mergedWeek ? "fixes you approved" : "none yet"),
      tile("Found → merged", median ? secsWords(median).replace(/ \d+ s$/, "") : "–", "median time a fix took"),
      tile("Cost per merged fix", cost != null ? usd(cost) : "–", "personas, planning and agents, per merge"),
      tile("Ease", eb != null && ea != null ? [h("span", {}, fx(eb)), h("span", { class: "ar" }, "→"), h("span", { class: ea > eb ? "up" : ea < eb ? "down" : "" }, fx(ea))] : "–", eb != null && ea != null ? "personas, before → after the fixes" : "no replays yet")));
  }

  function metrics() {
    (S.ros || []).forEach((r) => r.disconnect()); S.ros = [];
    const d = S.d, m = d.metrics || {}, st = d.settings || {};
    const range = S.range || "7d";
    const t = timeline();
    const fsAll = (d.findings || []).filter((f) => matches(f));
    const open = fsAll.filter((f) => f.state === "active" && f.stage !== "merged");
    const b0 = (t && t.buckets) || [];
    // The x-axis starts at the first event, not at an empty half.
    let first = b0.findIndex((x) => x.open_total || x.found || x.merged);
    first = first < 0 ? Math.max(0, b0.length - 2) : Math.max(0, first - 1);
    const b = b0.slice(first);
    const rl = RANGES.find((x) => x[0] === range)[1];
    const delta = (arr) => (arr.length >= 2 ? arr.at(-1) - arr[0] : null);
    const deltaEl = (v, goodDown = true) => v == null ? h("span", { class: "kd" }, "–") : h("span", { class: `kd ${v === 0 ? "" : (v < 0) === goodDown ? "good" : "bad"}` }, `${v > 0 ? "+" : v < 0 ? "−" : "±"}${Math.abs(v)} in ${rl}`);
    // The tiles.
    const week = Date.now() - 7 * 86400000;
    const merged = fsAll.filter((f) => f.stage === "merged");
    const mergedWeek = merged.filter((f) => (toMs(f.stage_since) || 0) >= week);
    const times = merged.map((f) => (toMs(f.stage_since) || 0) - (toMs(f.created) || 0)).filter((x) => x > 0).sort((a, b2) => a - b2);
    const median = times.length ? times[Math.floor(times.length / 2)] : null;
    const openSeries = b.map((x) => x.open_total);
    const blkSeries = b.map((x) => (x.open || {}).blocker || 0);
    const budget = Number(st.budget_usd_day) || 0, spent = Number(m.spent_today) || 0;
    const goBoard = (fn) => () => { fn(); C.keep(); location.hash = C.href("problems"); };
    const tileK = (label, value, sub, extra, onclick) => h("button", { class: "cv-kpi", type: "button", onclick }, h("span", { class: "cv-label" }, label), h("span", { class: "v" }, String(value)), h("span", { class: "sub" }, sub), extra);
    const kpis = h("div", { class: "cv-kpis" },
      tileK("Open issues", open.length, deltaEl(delta(openSeries)), sparkOf(openSeries, "var(--accent)"), goBoard(() => S.pf.sev.clear())),
      tileK("Blockers open", open.filter((f) => f.severity >= 4).length, deltaEl(delta(blkSeries)), sparkOf(blkSeries, sevVar(4)), goBoard(() => { S.pf.sev.clear(); S.pf.sev.add(4); })),
      tileK("Spend today", usd(spent), budget ? `of ${usd(budget)} · ${Math.round((spent / budget) * 100)}%` : "no budget set",
        h("span", { class: "cv-budget", "aria-hidden": "true" }, h("i", { class: spent > budget && budget ? "over" : null, style: { width: `${budget ? Math.min(100, (spent / budget) * 100) : 0}%` } })), () => { location.hash = C.href("settings"); }));
    const panel = (title, sub, ...kids) => h("section", { class: "cv-panel" }, h("div", { class: "cv-ph" }, h("h3", {}, title), sub ? h("div", { class: "sub" }, sub) : null), ...kids);
    const rangeSeg = h("div", { class: "cv-seg", role: "radiogroup", "aria-label": "Range" }, RANGES.map(([k, l]) =>
      h("button", { type: "button", class: range === k ? "on" : null, role: "radio", "aria-checked": String(range === k), onclick: () => { S.range = k; keep(); draw(); } }, l)));
    return h("div", {},
      outcomes(),
      h("h2", { class: "cv-acth" }, "Activity"),
      filterStrip(),
      h("div", { class: "cv-bar cv-dash-bar" }, h("span", { class: "cv-label" }, "Range"), rangeSeg, anyFilter() ? h("span", { class: "cv-meta" }, "Filtered by component") : null),
      kpis,
      h("div", { class: "cv-dash" },
        h("section", { class: "cv-panel wide" }, h("div", { class: "cv-ph" }, h("h3", {}, "Open issues over time"), h("div", { class: "sub" }, "Open by severity, stacked; found as dots, merged as a line. Tap for the numbers.")),
          t ? heroChart(b, range) : h("div", { class: "cv-plot", style: { height: "240px" } })),
        panel("Pipeline", "How far the findings got, Found to Merged", pipeline(fsAll)),
        panel("Where it hurts", "Open by kind, by severity, most blockers first; tap to see them", hurts(open, "theme")),
        components().length ? panel("By component", "Open by component, by severity; tap to see them", hurts(open, "component")) : null,
        panel("Spend and runs", `Runs by kind and model spend in the last ${rl}`, spendChart(range)),
        panel("Persona ease", "1 hard to 5 easy, lowest first", easeList()),
        h("details", { class: "cv-panel wide cv-raw" }, h("summary", {}, "Raw data"), rawData(m))));
  }

  function heroChart(b, range) {
    if (!b.length) return h("p", { class: "cv-empty" }, "No findings yet.");
    const shown = SEVS.filter(([, n]) => !S.hide.has(n));
    const total = (x) => shown.reduce((a, [, n]) => a + ((x.open || {})[n] || 0), 0);
    const yMax = Math.max(1, ...b.map((x) => Math.max(total(x), S.hide.has("found") ? 0 : x.found, S.hide.has("merged") ? 0 : x.merged)));
    const chart = plot({
      H: 240, n: b.length, yMax, xLabel: (i) => fmtT(b[i].t, range),
      label: `Open issues now: ${b.at(-1).open_total}. Found: ${b.reduce((a, x) => a + x.found, 0)}, merged: ${b.reduce((a, x) => a + x.merged, 0)}.`,
      series: ({ X, Y }) => {
        let g = "";
        const base = b.map(() => 0);
        for (const [k, n] of shown) {
          const top = b.map((x, i) => base[i] + ((x.open || {})[n] || 0));
          const pts = b.map((_, i) => `${X(i)},${Y(top[i])}`).concat(b.map((_, i) => `${X(b.length - 1 - i)},${Y(base[b.length - 1 - i])}`).join(" ")).join(" ");
          g += `<polygon points="${pts}" style="fill:${sevVar(k)};opacity:.78"/>`;
          top.forEach((v, i) => { base[i] = v; });
        }
        if (!S.hide.has("merged")) g += `<polyline points="${b.map((x, i) => `${X(i)},${Y(x.merged)}`).join(" ")}" fill="none" style="stroke:var(--good)" stroke-width="2"/>`;
        if (!S.hide.has("found")) g += b.map((x, i) => (x.found ? `<circle cx="${X(i)}" cy="${Y(x.found)}" r="3.5" style="fill:var(--surface);stroke:var(--text)" stroke-width="1.5"/>` : "")).join("");
        return g;
      },
      tip: (i) => { const x = b[i]; return [h("div", { class: "h" }, fmtT(x.t, range)), ...SEVS.map(([k, n]) => tipRow(sevVar(k), n, (x.open || {})[n] || 0)), tipRow(null, "open", x.open_total), tipRow("var(--text)", "found", x.found), tipRow("var(--good)", "merged", x.merged)]; },
    });
    const legend = h("div", { class: "cv-legend", role: "group", "aria-label": "Series" },
      [...SEVS.map(([k, n]) => [n, sevVar(k), ""]), ["found", "var(--text)", "dot"], ["merged", "var(--good)", "ln"]].map(([n, c, sh]) =>
        h("button", { type: "button", class: `cv-lg${S.hide.has(n) ? " off" : ""}`, "aria-pressed": String(!S.hide.has(n)), onclick: () => { if (S.hide.has(n)) S.hide.delete(n); else S.hide.add(n); keep(); draw(); } },
          h("i", { class: sh, style: sh === "ln" ? { borderColor: c } : sh === "dot" ? { borderColor: c } : { background: c } }), n)));
    return h("div", {}, legend, chart);
  }

  /** Found → … → Merged: how many reached each stage, and the drop-offs. */
  function pipeline(fs) {
    const st = S.d.stages || [];
    const f = (S.d.metrics || {}).funnel || {};
    const found = anyFilter() ? fs.length : Math.max(f.found || 0, fs.length);
    const reached = st.map((_, i) => fs.filter((x) => (x.stage === "merged" ? 7 : x.stage_index) >= i).length);
    reached[0] = found;
    const here0 = st.map((s) => fs.filter((x) => x.stage === s.stage && x.state === "active").length);
    const max = Math.max(1, found);
    return h("div", {},
      h("ol", { class: "cv-pipe" }, st.map((s, i) => h("li", {},
        h("span", { class: "nm" }, s.name),
        h("span", { class: "br" }, h("i", { style: { width: `${(reached[i] / max) * 100}%` } })),
        h("span", { class: "n" }, String(reached[i])),
        h("span", { class: "pc" }, i === 0 ? `${here0[i]} here` : `${Math.round((reached[i] / max) * 100)}%${here0[i] ? ` · ${here0[i]} here` : ""}`)))),
      h("p", { class: "cv-meta" }, `${reached[1] || 0} of ${found} reproduced, ${found ? Math.round(((reached[1] || 0) / found) * 100) : 0}% · ${reached[7] || 0} merged${f.cant_reproduce ? ` · ${f.cant_reproduce} could not be reproduced` : ""}${f.duplicates ? ` · ${f.duplicates} duplicates` : ""}`));
  }

  /** Open by kind or component, segments by severity, blockers first. */
  function hurts(open, by) {
    const keyOf = (f) => (by === "component" ? f.component || "" : f.theme);
    const groups = new Map();
    for (const f of open) { const k = keyOf(f); if (!groups.has(k)) groups.set(k, { 4: 0, 3: 0, 2: 0, 1: 0, n: 0 }); const g = groups.get(k); g[f.severity] = (g[f.severity] || 0) + 1; g.n++; }
    const rows = [...groups.entries()].sort((a, b) => b[1][4] - a[1][4] || b[1][3] - a[1][3] || b[1].n - a[1].n);
    if (!rows.length) return h("p", { class: "cv-empty" }, "Nothing open.");
    const max = Math.max(...rows.map((r) => r[1].n));
    return h("ul", { class: "cv-hurt" }, rows.map(([k, g]) => h("li", {}, h("button", { type: "button", onclick: () => { if (by === "component") { S.pf.comp.clear(); S.pf.comp.add(k); } else { S.pf.kind.clear(); S.pf.kind.add(k); } C.keep(); location.hash = C.href("problems"); } },
      h("span", { class: "nm" }, by === "component" ? k || "No component" : THEME_WORDS[k] || k),
      h("span", { class: "br", style: { width: `${(g.n / max) * 100}%` } }, SEVS.map(([s]) => (g[s] ? h("i", { style: { flex: String(g[s]), background: sevVar(s) }, title: `${g[s]} ${SEV[s]}` }) : null))),
      h("span", { class: "n" }, String(g.n), g[4] ? h("span", { class: "b" }, ` ${g[4]}!`) : null)))));
  }

  /** Runs per day (an hour for 24 h) by kind, and the spend as a line. */
  function spendChart(range) {
    const span = RANGE_MS[range], step = range === "24h" ? 3600000 : 86400000;
    const end = Date.now();
    const start0 = range === "24h" ? Math.floor((end - span) / step) * step + step : new Date(new Date(end - span + step).toDateString()).getTime();
    const n = Math.max(1, Math.ceil((end - start0) / step));
    const KIND = { explore: "persona", play: "persona", generate: "generate", repro: "repro", fix: "fix" };
    const kinds = ["persona", "generate", "repro", "fix"];
    const kcol = { persona: "var(--accent)", generate: "var(--cv-k2)", repro: "var(--cv-k3)", fix: "var(--cv-k4)" };
    const bk = Array.from({ length: n }, (_, i) => ({ t: start0 + i * step, persona: 0, generate: 0, repro: 0, fix: 0, cost: 0 }));
    const put = (ts, kind, cost) => { const ms = toMs(ts); if (!ms || ms < start0) return; const i = Math.min(n - 1, Math.floor((ms - start0) / step)); bk[i][kind] = (bk[i][kind] || 0) + 1; bk[i].cost += Number(cost) || 0; };
    for (const r of S.d.runs || []) put(r.created, KIND[r.purpose] || "persona", r.cost_usd);
    // The agents of the findings on the board (the overview names only each finding's current one).
    for (const f of S.d.findings || []) if (f.agent && f.agent_created) put(f.agent_created, f.stage === "reproduce" ? "repro" : "fix", 0);
    const used = kinds.filter((k) => bk.some((x) => x[k]));
    const yMax = Math.max(1, ...bk.map((x) => kinds.reduce((a, k) => a + x[k], 0)));
    const cMax = Math.max(1, ...bk.map((x) => x.cost));
    const y2 = { max: cMax, fmt: (v) => `$${v < 1 ? v.toFixed(2) : v.toFixed(0)}` };
    const total = bk.reduce((a, x) => a + x.cost, 0);
    const chart = plot({
      H: 200, n, yMax, R: 40, y2, xLabel: (i) => fmtT(bk[i].t, range), label: `Spend in the range: ${usd(total)}`,
      series: ({ X, Y, step: sw }) => {
        let g = "";
        const bw = Math.max(2, Math.min(28, sw * 0.62));
        bk.forEach((x, i) => { let y0 = Y(0); for (const k of kinds) { if (!x[k]) continue; const hh = Y(0) - Y(x[k]); g += `<rect x="${X(i) - bw / 2}" y="${y0 - hh}" width="${bw}" height="${Math.max(0, hh - 1)}" rx="1.5" style="fill:${kcol[k]}"/>`; y0 -= hh; } });
        g += `<polyline points="${bk.map((x, i) => `${X(i)},${y2.Y(x.cost)}`).join(" ")}" fill="none" style="stroke:var(--text-2)" stroke-width="1.75" stroke-dasharray="4 3"/>`;
        return g;
      },
      tip: (i) => { const x = bk[i]; return [h("div", { class: "h" }, fmtT(x.t, range)), ...kinds.filter((k) => x[k] || used.includes(k)).map((k) => tipRow(kcol[k], `${k} runs`, x[k])), tipRow("var(--text-2)", "spend", usd(x.cost))]; },
    });
    return h("div", {},
      h("div", { class: "cv-legend" }, (used.length ? used : ["persona"]).map((k) => h("span", {}, h("i", { style: { background: kcol[k] } }), k)), h("span", {}, h("i", { class: "ln dash", style: { borderColor: "var(--text-2)" } }), "spend"), h("span", { class: "cv-meta" }, `${usd(total)} in all`)),
      chart);
  }

  function easeList() {
    const ps = (S.d.personas || []).filter((p) => p.state !== "retired").sort((a, b) => (a.ease ?? 9) - (b.ease ?? 9));
    if (!ps.length) return h("p", { class: "cv-empty" }, "No personas yet.");
    return h("ul", { class: "cv-ease" }, ps.map((p) => h("li", {},
      h("span", { class: "nm" }, p.name.split(",")[0], h("span", { class: "rl" }, `${p.runs || 0} runs · ${p.open || 0} open`)),
      sparkOf(p.ease_hist, p.ease != null && p.ease < 2.5 ? sevVar(4) : p.ease != null && p.ease < 3.5 ? sevVar(3) : "var(--good)"),
      h("span", { class: "v" }, p.ease == null ? "–" : Number(p.ease).toFixed(1)))));
  }

  function rawData(m) {
    const hist = m.history || [];
    return h("div", { class: "cv-rawg" },
      h("div", {}, h("h3", {}, "Day by day"), h("div", { class: "cv-tscroll" }, h("table", { class: "cv-t" }, h("thead", {}, h("tr", {}, h("th", {}, "Day"), h("th", { class: "n" }, "Ease"), h("th", { class: "n" }, "Open"), h("th", { class: "n" }, "Merged"), h("th", { class: "n" }, "Spend 7d"))),
        h("tbody", {}, hist.slice(-14).reverse().map((r) => h("tr", {}, h("td", {}, r.day), h("td", { class: "n" }, r.ease == null ? "–" : r.ease), h("td", { class: "n" }, r.open), h("td", { class: "n" }, (r.funnel || {}).merged || 0), h("td", { class: "n" }, usd(r.cost_7d)))))))),
      h("div", {}, h("h3", {}, "Spend and quality"), h("table", { class: "cv-t" }, h("tbody", {},
        h("tr", {}, h("td", {}, "Model spend today"), h("td", { class: "n" }, usd(m.spent_today))),
        h("tr", {}, h("td", {}, "Spend, 7 days"), h("td", { class: "n" }, usd(m.cost_7d))),
        h("tr", {}, h("td", {}, "Per merged fix, 7 days"), h("td", { class: "n" }, usd(m.cost_per_merged_fix))),
        h("tr", {}, h("td", {}, "Repro rate"), h("td", { class: "n" }, m.repro_rate == null ? "–" : `${Math.round(m.repro_rate * 100)}%`)),
        h("tr", {}, h("td", {}, "Regression rate"), h("td", { class: "n" }, m.regression_rate == null ? "–" : `${Math.round(m.regression_rate * 100)}%`)),
        h("tr", {}, h("td", {}, "New serious per persona-hour"), h("td", { class: "n" }, m.serious_per_persona_hour == null ? "–" : m.serious_per_persona_hour.toFixed(2)))))));
  }

  return h("div", {}, stepHead(C, "Metrics", null, "Is it converging? Fewer open problems, fewer new serious ones, personas finding it easier."), metrics());
}

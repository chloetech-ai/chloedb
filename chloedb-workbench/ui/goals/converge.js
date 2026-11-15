// Converge (src/converge.rs, SPEC.md "Seventh build: Converge"): personas
// play the product, what they hit becomes findings, and each finding walks
// eight gated stages to a pull request you approve.
//   #/converge            the board: by stage, or grouped by kind of problem
//   #/converge/personas   the personas, their ease over runs, the coverage map
//   #/converge/metrics    ease, the funnel, open problems by kind, spend
//   #/converge/settings   the autopilot's limits and cadence
// A finding opens in a sheet (?f=<id>) with every gate's evidence.
//   mount(root, ctx) -> { update(state), unmount() }
import { h, useStyle } from "/levels/common.js";

const CSS = `
.cv { padding: var(--s-4) max(16px, env(safe-area-inset-left, 0px)) var(--s-7); max-width: 100%; }
.cv-top { display: flex; align-items: center; gap: var(--s-3); flex-wrap: wrap; margin-bottom: var(--s-3); }
.cv-top h1 { margin: 0; font-size: var(--fs-lg); line-height: var(--lh-lg); font-weight: var(--w-title); letter-spacing: var(--track-md); }
.cv-sp { flex: 1; }
.cv-pill { display: inline-flex; align-items: center; gap: 6px; height: 24px; padding: 0 10px; border-radius: var(--r-pill); border: var(--hair) solid var(--line-2); background: var(--surface); font-size: var(--fs-sm); white-space: nowrap; }
.cv-dot { width: 7px; height: 7px; border-radius: 50%; background: var(--warn); }
.cv-dot.on { background: var(--good); }
.cv-btn { height: var(--ctl-h); padding: 0 12px; border-radius: var(--r-sm); border: var(--hair) solid var(--line-2); background: var(--surface); color: var(--text); cursor: pointer; font: inherit; display: inline-flex; align-items: center; gap: 6px; white-space: nowrap; }
.cv-btn:hover { background: var(--surface-2); }
.cv-btn.pri { background: var(--accent); border-color: var(--accent); color: var(--accent-ink); }
.cv-btn.good { background: var(--good); border-color: var(--good); color: #fff; }
.cv-btn[disabled] { opacity: .5; cursor: default; }
.cv-tabs { display: flex; gap: 2px; border-bottom: var(--hair) solid var(--line); margin-bottom: var(--s-4); overflow-x: auto; }
.cv-tabs a { padding: 8px 12px; color: var(--text-2); text-decoration: none; border-bottom: 2px solid transparent; white-space: nowrap; }
.cv-tabs a.on { color: var(--text); border-color: var(--accent); font-weight: var(--w-mid); }
.cv-label { font-family: var(--mono); font-size: var(--fs-xs); letter-spacing: var(--track-label); text-transform: uppercase; color: var(--muted); }
.cv-strip { display: grid; grid-template-columns: repeat(auto-fit, minmax(150px, 1fr)); gap: 1px; background: var(--line); border: var(--hair) solid var(--line); border-radius: var(--r); overflow: hidden; margin-bottom: var(--s-4); }
.cv-strip > div { background: var(--surface); padding: 12px 14px; min-width: 0; }
.cv-strip .v { font-family: var(--mono); font-size: var(--fs-lg); line-height: var(--lh-lg); font-variant-numeric: tabular-nums; }
.cv-strip .d { font-size: var(--fs-sm); color: var(--text-2); }
.cv-bar { display: flex; gap: var(--s-2); align-items: center; flex-wrap: wrap; margin-bottom: var(--s-3); }
.cv-seg { display: inline-flex; border: var(--hair) solid var(--line-2); border-radius: var(--r-sm); overflow: hidden; }
.cv-seg button { white-space: nowrap; border: 0; background: var(--surface); padding: 0 10px; height: 28px; font: inherit; color: var(--text-2); cursor: pointer; }
.cv-seg button.on { background: var(--accent-soft); color: var(--accent-text); font-weight: var(--w-mid); }
.cv-chipf { border: var(--hair) solid var(--line-2); background: var(--surface); border-radius: var(--r-pill); height: 26px; padding: 0 10px; font: inherit; font-size: var(--fs-sm); color: var(--text-2); cursor: pointer; display: inline-flex; gap: 6px; align-items: center; }
.cv-chipf.on { border-color: var(--accent-line); background: var(--accent-soft); color: var(--accent-text); }
.cv-chipf .n { font-family: var(--mono); font-size: var(--fs-xs); }
.cv-chipf .b { color: var(--crit); font-family: var(--mono); font-size: var(--fs-xs); }
.cv-wrap { overflow-x: auto; padding-bottom: 8px; }
.cv-board { display: grid; grid-auto-flow: column; grid-auto-columns: minmax(230px, 1fr); gap: 10px; }
.cv-board.theme { grid-auto-columns: minmax(240px, 320px); }
.cv-sheet-h select { width: auto; height: 22px; font-size: var(--fs-xs); border: var(--hair) solid var(--line-2); border-radius: 4px; background: var(--surface); color: var(--text); padding: 0 4px; }
.cv-col { background: var(--sunken); border: var(--hair) solid var(--line); border-radius: var(--r); display: flex; flex-direction: column; min-width: 0; }
.cv-col-h { padding: 10px 12px 8px; border-bottom: var(--hair) solid var(--line); }
.cv-col-h .t { display: flex; justify-content: space-between; font-weight: var(--w-title); }
.cv-col-h .t .c { font-family: var(--mono); color: var(--muted); font-weight: var(--w-text); font-size: var(--fs-sm); }
.cv-col-h .g { font-size: var(--fs-xs); line-height: var(--lh-xs); color: var(--text-2); margin-top: 2px; }
.cv-col.you .cv-col-h { background: var(--accent-soft); border-bottom-color: var(--accent-line); border-radius: var(--r) var(--r) 0 0; }
.cv-cards { padding: 8px; display: flex; flex-direction: column; gap: 8px; }
.cv-card { background: var(--surface); border: var(--hair) solid var(--line); border-left: 3px solid var(--sev, var(--line-2)); border-radius: 8px; padding: 10px; cursor: pointer; text-align: left; font: inherit; color: var(--text); display: flex; flex-direction: column; gap: 6px; width: 100%; }
.cv-card:hover { box-shadow: var(--shadow-2); }
.cv-card[data-sev="4"] { --sev: var(--crit); } .cv-card[data-sev="3"] { --sev: var(--warn); } .cv-card[data-sev="1"] { --sev: var(--line); }
.cv-card .ttl { font-weight: var(--w-mid); line-height: var(--lh-sm); }
.cv-meta { display: flex; flex-wrap: wrap; gap: 4px 8px; align-items: center; font-size: var(--fs-xs); color: var(--text-2); }
.cv-chip { display: inline-flex; align-items: center; min-height: 18px; padding: 1px 6px; line-height: 16px; border-radius: 4px; background: var(--surface-3); font-family: var(--mono); font-size: var(--fs-xs); color: var(--text-2); max-width: 100%; }
.cv-chip.s4 { background: var(--crit-soft); color: var(--crit-ink); } .cv-chip.s3 { background: var(--warn-soft); color: var(--warn-ink); }
.cv-chip.ok { background: var(--good-soft); color: var(--good-ink); } .cv-chip.bad { background: var(--crit-soft); color: var(--crit-ink); }
.cv-chip.run { background: var(--accent-soft); color: var(--accent-text); } .cv-chip.you { background: var(--warn-soft); color: var(--warn-ink); }
.cv-gates { display: flex; gap: 3px; } .cv-gates i { width: 14px; height: 4px; border-radius: 2px; background: var(--surface-3); }
.cv-gates i.p { background: var(--good); } .cv-gates i.n { background: var(--accent); }
.cv-empty { color: var(--muted); font-size: var(--fs-sm); padding: 4px; }
.cv-scrim { position: fixed; inset: 0; background: var(--scrim); z-index: 40; }
.cv-sheet { position: fixed; top: 0; right: 0; bottom: 0; width: min(600px, 100%); background: var(--surface); box-shadow: var(--shadow-3); z-index: 41; display: flex; flex-direction: column; }
.cv-sheet-h { padding: 16px 20px 12px; border-bottom: var(--hair) solid var(--line); display: flex; flex-direction: column; gap: 8px; }
.cv-sheet-h h2 { margin: 0; font-size: var(--fs-md); line-height: var(--lh-md); font-weight: var(--w-title); padding-right: 32px; }
.cv-sheet-b { padding: 16px 20px 24px; overflow-y: auto; display: flex; flex-direction: column; gap: 18px; min-height: 0; flex: 1; }
.cv-sheet-f { padding: 12px 20px; border-top: var(--hair) solid var(--line); display: flex; gap: 8px; flex-wrap: wrap; }
.cv-x { position: absolute; top: 12px; right: 14px; border: 0; background: none; cursor: pointer; font-size: 18px; color: var(--muted); width: 30px; height: 30px; border-radius: 6px; }
.cv-stages { display: flex; gap: 2px; } .cv-stages span { flex: 1; height: 6px; border-radius: 3px; background: var(--surface-3); }
.cv-stages span.d { background: var(--good); } .cv-stages span.n { background: var(--accent); }
.cv-kv { display: grid; grid-template-columns: 110px 1fr; gap: 6px 12px; font-size: var(--fs-sm); margin: 0; }
.cv-kv dt { color: var(--muted); } .cv-kv dd { margin: 0; min-width: 0; overflow-wrap: anywhere; }
.cv-pre { margin: 0; background: var(--sunken); border: var(--hair) solid var(--line); border-radius: 8px; padding: 10px 12px; font-family: var(--mono); font-size: var(--fs-xs); line-height: var(--lh-xs); overflow-x: auto; white-space: pre; max-height: 260px; }
.cv-ta { width: 100%; min-height: 120px; font-family: var(--mono); font-size: var(--fs-xs); background: var(--sunken); color: var(--text); border: var(--hair) solid var(--line-2); border-radius: 8px; padding: 8px; }
.cv-tl { list-style: none; margin: 0; padding: 0; }
.cv-tl li { display: grid; grid-template-columns: 18px 1fr auto; gap: 10px; padding: 7px 0; border-bottom: var(--hair) solid var(--line); }
.cv-tl .ic { width: 16px; height: 16px; border-radius: 50%; display: grid; place-items: center; font-size: 10px; color: #fff; margin-top: 2px; background: var(--muted); }
.cv-tl .ic.p { background: var(--good); } .cv-tl .ic.f { background: var(--crit); }
.cv-tl .w { font-family: var(--mono); font-size: var(--fs-xs); color: var(--muted); white-space: nowrap; }
.cv-tl .s { font-size: var(--fs-sm); color: var(--text-2); }
.cv-tl details summary { cursor: pointer; font-size: var(--fs-xs); color: var(--accent-text); }
.cv-needs { background: var(--warn-soft); color: var(--warn-ink); border-radius: 8px; padding: 8px 10px; font-size: var(--fs-sm); }
.cv-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: 12px; }
.cv-pc { background: var(--surface); border: var(--hair) solid var(--line); border-radius: var(--r); padding: 14px; display: flex; flex-direction: column; gap: 10px; min-width: 0; }
.cv-pc.probation { border-style: dashed; } .cv-pc.retired { opacity: .6; }
.cv-pc .name { font-weight: var(--w-title); } .cv-pc .role { color: var(--text-2); font-size: var(--fs-sm); }
.cv-stats { display: grid; grid-template-columns: repeat(4, 1fr); gap: 6px; }
.cv-stats div { background: var(--surface-2); border-radius: 6px; padding: 6px 8px; } .cv-stats .v { font-family: var(--mono); } .cv-stats .l { font-size: var(--fs-xs); color: var(--muted); }
.cv-spark { width: 100%; height: 34px; display: block; }
.cv-panels { display: grid; grid-template-columns: repeat(auto-fit, minmax(340px, 1fr)); gap: 12px; }
.cv-panel { background: var(--surface); border: var(--hair) solid var(--line); border-radius: var(--r); padding: 14px; min-width: 0; }
.cv-panel h3 { margin: 0 0 2px; font-size: var(--fs-body); }
.cv-panel .sub { color: var(--text-2); font-size: var(--fs-sm); margin-bottom: 10px; }
.cv-t { width: 100%; border-collapse: collapse; font-size: var(--fs-sm); }
.cv-t th { text-align: left; font-weight: var(--w-mid); color: var(--muted); font-size: var(--fs-xs); font-family: var(--mono); text-transform: uppercase; letter-spacing: .06em; padding: 6px 8px; border-bottom: var(--hair) solid var(--line); }
.cv-t td { padding: 7px 8px; border-bottom: var(--hair) solid var(--line); font-variant-numeric: tabular-nums; }
.cv-t td.n { font-family: var(--mono); text-align: right; }
.cv-hbar { height: 10px; border-radius: 3px; background: var(--accent); opacity: .8; }
.cv-f { display: flex; flex-direction: column; gap: 4px; margin-bottom: 12px; }
.cv-f label { font-weight: var(--w-mid); } .cv-f .hint { font-size: var(--fs-sm); color: var(--text-2); }
.cv-f input[type=number], .cv-f input[type=text] { height: var(--ctl-h); border: var(--hair) solid var(--line-2); border-radius: var(--r-sm); background: var(--surface); color: var(--text); padding: 0 8px; font: inherit; max-width: 360px; }
.cv-toast { position: fixed; bottom: 20px; left: 50%; transform: translateX(-50%); background: var(--text); color: var(--bg); padding: 8px 14px; border-radius: 8px; font-size: var(--fs-sm); z-index: 60; }
.cv-intro { max-width: 70ch; color: var(--text-2); margin: 0 0 var(--s-4); }
/* Phones: one stage a screen, swiped; the numbers in one scrolled row. */
@media (max-width: 640px) {
  .cv { padding-top: var(--s-3); }
  .cv-top { gap: 8px; }
  .cv-top h1 { flex: 1 0 100%; }
  .cv-top .cv-sp { display: none; }
  .cv-top .cv-btn { height: 40px; flex: 1; justify-content: center; }
  .cv-top .cv-pill { height: 28px; }
  .cv-tabs a { padding: 10px 12px; }
  .cv-strip { display: flex; overflow-x: auto; scroll-snap-type: x mandatory; gap: 1px; }
  .cv-strip > div { flex: 0 0 44%; scroll-snap-align: start; padding: 10px 12px; }
  .cv-strip .v { font-size: var(--fs-md); line-height: var(--lh-md); }
  .cv-strip .d { font-size: var(--fs-xs); line-height: var(--lh-xs); }
  .cv-bar.kinds { flex-wrap: nowrap; overflow-x: auto; padding-bottom: 4px; margin-left: -16px; margin-right: -16px; padding-left: 16px; padding-right: 16px; }
  .cv-bar.kinds > * { flex: 0 0 auto; }
  .cv-chipf { height: 32px; }
  .cv-bar:not(.kinds) > .cv-label { display: none; }
  .cv-bar:not(.kinds) { flex-wrap: nowrap; }
  .cv-bar:not(.kinds) .cv-seg button { padding: 0 8px; }
  .cv-seg button { height: 36px; }
  .cv-wrap { margin-left: -16px; margin-right: -16px; padding-left: 16px; scroll-snap-type: x mandatory; scroll-padding-left: 16px; }
  .cv-board, .cv-board.theme { grid-auto-columns: 86vw; }
  .cv-col { scroll-snap-align: start; }
  .cv-board::after { content: ""; width: 4px; }
  .cv-sheet-h { padding: 12px 16px 10px; }
  .cv-sheet-b { padding: 14px 16px 20px; }
  .cv-sheet-f { padding: 10px 16px calc(10px + env(safe-area-inset-bottom, 0px)); }
  .cv-sheet-f .cv-btn { height: 40px; }
  .cv-kv { grid-template-columns: 1fr; gap: 2px 0; }
  .cv-kv dd { margin-bottom: 8px; }
  .cv-panels, .cv-grid { grid-template-columns: 1fr; }
  .cv-f input[type=number], .cv-f input[type=text] { max-width: none; height: 40px; }
}
`;

export const THEME_WORDS = { onboarding: "Onboarding", reliability: "Reliability", security: "Security", scalability: "Scalability", trust: "Trust", correctness: "Correctness", usability: "Usability", performance: "Performance", observability: "Observability", docs: "Docs" };
const SEV = { 4: "blocker", 3: "major", 2: "minor", 1: "papercut" };
const TABS = [["board", "Board", "#/converge"], ["personas", "Personas", "#/converge/personas"], ["metrics", "Metrics", "#/converge/metrics"], ["settings", "Autopilot", "#/converge/settings"]];

function toast(text) {
  const t = h("div", { class: "cv-toast", role: "status" }, text);
  document.body.append(t);
  setTimeout(() => t.remove(), 3000);
}
const enc = encodeURIComponent;
const ago = (ms) => {
  if (!ms) return "";
  const s = Math.max(0, (Date.now() - ms) / 1000);
  return s < 60 ? "now" : s < 3600 ? `${Math.round(s / 60)} min` : s < 86400 ? `${Math.round(s / 3600)} h` : `${Math.round(s / 86400)} d`;
};
const usd = (x) => (x == null ? "–" : `$${Number(x).toFixed(2)}`);

export function mount(root, ctx) {
  useStyle("converge", CSS);
  const host = h("div", { class: "cv" });
  root.replaceChildren(host);
  const S = { d: null, group: "stage", theme: null, sev: 0, alive: true, timer: null };
  const repo = () => (ctx.params && ctx.params.repo) || "";
  const api = (p, o) => ctx.api(p, o);
  const view = () => (location.hash.split("?")[0].split("/")[2] || "board");
  const fid = () => new URLSearchParams(location.hash.split("?")[1] || "").get("f");

  async function load() {
    if (!repo()) { host.replaceChildren(h("p", { class: "cv-intro" }, "Open a project first: Converge works on one repository.")); return; }
    try {
      S.d = await api(`/api/converge?repo=${enc(repo())}`);
    } catch (err) {
      host.replaceChildren(h("p", { class: "cv-intro" }, `Converge did not load: ${err.message || err}`));
      return;
    }
    if (S.alive) draw();
  }

  function draw() {
    const d = S.d;
    if (ctx.crumbs) ctx.crumbs([{ label: "Start", href: "#/" }, { label: "Converge", href: "#/converge" }]);
    const st = d.settings || {};
    const m = d.metrics || {};
    const v = view();
    const top = h("div", { class: "cv-top" },
      h("h1", {}, "Converge"),
      h("span", { class: "cv-sp" }),
      h("span", { class: "cv-pill" }, h("span", { class: `cv-dot${st.enabled ? " on" : ""}` }), st.enabled ? `Autopilot on · every ${Math.round(st.tick_s / 60)} min` : "Autopilot off"),
      h("span", { class: "cv-pill" }, `${usd(m.spent_today)} of ${usd(st.budget_usd_day)} today`),
      h("button", { class: "cv-btn", type: "button", onclick: () => act(() => api("/api/converge/tick", { method: "POST", body: { repo: d.repo } }), "Tick started: each item whose gate can be checked now is moving") }, "Run a tick"),
      h("button", { class: "cv-btn", type: "button", onclick: () => act(() => api("/api/converge/settings", { method: "PUT", body: { repo: d.repo, enabled: !st.enabled } }), st.enabled ? "Autopilot paused: running steps finish" : "Autopilot on") }, st.enabled ? "Pause" : "Turn on"));
    const tabs = h("nav", { class: "cv-tabs", "aria-label": "Converge" }, TABS.map(([k, l, href]) => h("a", { href, class: k === v ? "on" : null, "aria-current": k === v ? "page" : null }, l)));
    const body = v === "personas" ? personas() : v === "metrics" ? metrics() : v === "settings" ? settings() : board();
    host.replaceChildren(top, tabs, body);
    const f = fid();
    if (f) openSheet(f);
  }

  async function act(fn, msg) {
    try { await fn(); if (msg) toast(msg); } catch (err) { toast(err.message || String(err)); }
    await load();
  }

  // ------------------------------------------------------------ the board
  function board() {
    const d = S.d;
    const m = d.metrics || {};
    const all = d.findings || [];
    if (!all.length && !(d.personas || []).length) {
      return h("div", {},
        h("p", { class: "cv-intro" }, "No personas or findings yet. Load the six personas of ChloeDB's first evaluation and the problems they reported, or add a persona of your own on the Personas tab."),
        h("button", { class: "cv-btn pri", type: "button", onclick: () => act(() => api("/api/converge/seed", { method: "POST", body: { repo: d.repo } }), "Loaded the seed personas and their findings") }, "Load the seed personas"));
    }
    const shown = all.filter((f) => f.state === "active" && (!S.theme || f.theme === S.theme) && (!S.sev || f.severity >= S.sev));
    const strip = h("div", { class: "cv-strip" },
      tile("Persona ease", m.ease == null ? "–" : m.ease.toFixed(2), "average of active personas, 1 to 5"),
      tile("Open", m.open || 0, `${(m.open_by_severity || {}).blocker || 0} blockers · ${(m.open_by_severity || {}).major || 0} major`),
      tile("Needs you", m.needs_you || 0, "approvals, doors, missing repros"),
      tile("New serious / persona-hour", m.serious_per_persona_hour == null ? "–" : m.serious_per_persona_hour.toFixed(2), "falling means converging"),
      tile("Repro rate", m.repro_rate == null ? "–" : `${Math.round(m.repro_rate * 100)}%`, "reproduced of found"));
    // Kinds of problem: a chip each, the count and blockers, a click filters.
    const themes = d.themes || Object.keys(THEME_WORDS);
    const counts = Object.fromEntries(themes.map((t) => [t, all.filter((f) => f.state === "active" && f.stage !== "merged" && f.theme === t)]));
    const chips = h("div", { class: "cv-bar kinds", role: "group", "aria-label": "Kind of problem" },
      h("span", { class: "cv-label" }, "Kind"),
      h("button", { class: `cv-chipf${S.theme ? "" : " on"}`, type: "button", onclick: () => { S.theme = null; draw(); } }, "All"),
      themes.filter((t) => counts[t].length).map((t) => h("button", { class: `cv-chipf${S.theme === t ? " on" : ""}`, type: "button", onclick: () => { S.theme = S.theme === t ? null : t; draw(); } },
        THEME_WORDS[t] || t, h("span", { class: "n" }, counts[t].length), counts[t].some((f) => f.severity >= 4) ? h("span", { class: "b", title: "blockers" }, `${counts[t].filter((f) => f.severity >= 4).length}!`) : null)));
    const bar = h("div", { class: "cv-bar" },
      h("span", { class: "cv-label" }, "Group by"),
      h("div", { class: "cv-seg", role: "radiogroup", "aria-label": "Group by" },
        [["stage", "Stage"], ["theme", "Kind"]].map(([k, l]) => h("button", { type: "button", class: S.group === k ? "on" : null, role: "radio", "aria-checked": String(S.group === k), onclick: () => { S.group = k; draw(); } }, l))),
      h("span", { class: "cv-label", style: { marginLeft: "12px" } }, "Severity"),
      h("div", { class: "cv-seg", role: "radiogroup", "aria-label": "Severity" },
        [[0, "All"], [3, "Major+"], [4, "Blockers"]].map(([k, l]) => h("button", { type: "button", class: S.sev === k ? "on" : null, onclick: () => { S.sev = k; draw(); } }, l))));
    const cols = S.group === "theme"
      ? themes.filter((t) => shown.some((f) => f.theme === t)).map((t) => {
        const items = shown.filter((f) => f.theme === t).sort((a, b) => a.stage_index - b.stage_index || b.score - a.score);
        return col(THEME_WORDS[t] || t, `${items.filter((f) => f.stage === "merged").length} merged · ${items.filter((f) => f.severity >= 4 && f.stage !== "merged").length} blockers open`, items, false, true);
      })
      : d.stages.map((s) => col(s.name, s.gate, shown.filter((f) => f.stage === s.stage).sort((a, b) => (b.pinned - a.pinned) || b.score - a.score), s.you, false));
    const parked = all.filter((f) => f.state === "parked");
    return h("div", {}, strip, chips, bar,
      h("div", { class: "cv-wrap" }, h("div", { class: `cv-board${S.group === "theme" ? " theme" : ""}` }, cols.length ? cols : h("p", { class: "cv-empty" }, "Nothing matches these filters."))),
      parked.length ? h("details", { style: { marginTop: "12px" } }, h("summary", {}, `${parked.length} parked`), h("div", { class: "cv-cards" }, parked.map(card))) : null);
  }

  function tile(l, v, d) { return h("div", {}, h("div", { class: "cv-label" }, l), h("div", { class: "v" }, String(v)), h("div", { class: "d" }, d)); }

  function col(name, gate, items, you, showStage) {
    return h("section", { class: `cv-col${you ? " you" : ""}`, "aria-label": name },
      h("div", { class: "cv-col-h" }, h("div", { class: "t" }, h("span", {}, name), h("span", { class: "c" }, String(items.length))), h("div", { class: "g" }, gate)),
      h("div", { class: "cv-cards" }, items.length ? items.map((f) => card(f, showStage)) : h("div", { class: "cv-empty" }, "Nothing here")));
  }

  function personaName(id) { const p = (S.d.personas || []).find((x) => x.id === id); return p ? p.name.split(",")[0] : id; }

  function card(f, showStage) {
    const stageName = (S.d.stages.find((s) => s.stage === f.stage) || {}).name || f.stage;
    const chips = [h("span", { class: `cv-chip s${f.severity}` }, SEV[f.severity] || f.severity)];
    if (S.group !== "theme") chips.push(h("span", { class: "cv-chip" }, THEME_WORDS[f.theme] || f.theme || "–"));
    if (showStage) chips.push(h("span", { class: "cv-chip run" }, stageName));
    if (f.score && f.stage_index >= 3 && f.stage !== "merged") chips.push(h("span", { class: "cv-chip" }, `score ${f.score}`));
    if (f.door) chips.push(h("span", { class: "cv-chip you" }, `door: ${f.door}`));
    if (f.busy) chips.push(h("span", { class: "cv-chip run" }, f.busy));
    if (f.needs_you) chips.push(h("span", { class: "cv-chip you" }, "needs you"));
    if (f.stage === "review" && f.ci) chips.push(h("span", { class: `cv-chip ${f.ci === "pass" ? "ok" : f.ci === "fail" ? "bad" : ""}` }, `CI ${f.ci}`));
    if (f.stage === "merged" && f.guard_ok != null) chips.push(h("span", { class: `cv-chip ${f.guard_ok ? "ok" : "bad"}` }, f.guard_ok ? "guard green" : "guard red"));
    return h("button", { class: "cv-card", type: "button", "data-sev": String(f.severity), onclick: () => { location.hash = `#/converge${view() === "board" ? "" : "/" + view()}?f=${f.id}`; } },
      h("span", { class: "ttl" }, f.title),
      h("span", { class: "cv-meta" }, h("span", {}, (f.personas_hit || []).map(personaName).join(", ") || f.by), f.occurrences > 1 ? h("span", {}, `${f.occurrences} times`) : null, h("span", {}, ago(f.stage_since))),
      h("span", { class: "cv-meta" }, chips),
      h("span", { class: "cv-gates", "aria-label": `stage ${f.stage_index + 1} of 8` }, [0, 1, 2, 3, 4, 5, 6, 7].map((i) => h("i", { class: i < f.stage_index ? "p" : i === f.stage_index ? "n" : null }))));
  }

  // ------------------------------------------------------------ the sheet
  async function openSheet(id) {
    let d;
    try { d = await api(`/api/converge/findings/${enc(id)}`); } catch (err) { toast(err.message); return; }
    const f = d.finding;
    const close = () => { history.replaceState(history.state, "", location.hash.split("?")[0]); sheet.remove(); scrim.remove(); };
    const scrim = h("div", { class: "cv-scrim", onclick: close });
    const reload = async () => { close(); await load(); location.hash = `${location.hash.split("?")[0]}?f=${id}`; };
    const run = (fn, msg) => async () => { try { await fn(); toast(msg); } catch (err) { toast(err.message || String(err)); } await reload(); };
    const post = (verb, body) => api(`/api/converge/findings/${enc(id)}/${verb}`, { method: "POST", body: body || {} });
    const sc = f.scope || {};
    const repro = h("textarea", { class: "cv-ta", "aria-label": "Repro script", spellcheck: "false" });
    repro.value = f.repro || "#!/bin/sh\n# exits non-zero while the problem is there, 0 once it is fixed\n";
    const actions = [];
    if (f.stage === "review" && f.ci === "pass") actions.push(h("button", { class: "cv-btn good", type: "button", onclick: run(() => post("approve"), "Approved and merged") }, "Approve and merge"));
    if (f.stage === "review" || f.stage === "verify") actions.push(h("button", { class: "cv-btn", type: "button", onclick: run(() => post("back", { text: "Sent back from Converge" }), "Sent back to develop") }, "Send back to develop"));
    if (f.stage === "prioritize") actions.push(h("button", { class: "cv-btn pri", type: "button", onclick: run(() => post("start"), "Development started") }, "Start development"));
    if (f.stage === "reproduce" && !f.agent) actions.push(h("button", { class: "cv-btn", type: "button", onclick: run(() => post("repro-agent"), "An agent is writing the repro") }, "Ask an agent for the repro"));
    if (f.stage === "reproduce" && f.agent) actions.push(h("button", { class: "cv-btn", type: "button", onclick: run(() => post("take-repro"), "Took the agent's repro") }, "Take the agent's repro"));
    actions.push(h("button", { class: "cv-btn", type: "button", onclick: run(() => post(f.pinned ? "unpin" : "pin"), f.pinned ? "Unpinned" : "Pinned to the top") }, f.pinned ? "Unpin" : "Pin"));
    actions.push(h("button", { class: "cv-btn", type: "button", onclick: run(() => post(f.state === "parked" ? "unpark" : "park"), f.state === "parked" ? "Back in the loop" : "Parked") }, f.state === "parked" ? "Unpark" : "Park"));
    const reason = h("input", { type: "text", placeholder: "Why it is not a problem", "aria-label": "Why it is not a problem", style: { flex: "1", minWidth: "160px", height: "30px", border: "1px solid var(--line-2)", borderRadius: "6px", background: "var(--surface)", color: "var(--text)", padding: "0 8px" } });
    actions.push(reason, h("button", { class: "cv-btn", type: "button", onclick: run(() => post("dismiss", { reason: reason.value }), "Dismissed") }, "Dismiss"));
    const themeSel = h("select", { "aria-label": "Kind of problem", onchange: run(() => api(`/api/converge/findings/${enc(id)}`, { method: "PUT", body: { theme: themeSel.value } }), "Kind changed") },
      (S.d.themes || Object.keys(THEME_WORDS)).map((t) => h("option", { value: t, selected: t === f.theme ? true : null }, THEME_WORDS[t] || t)));
    const sheet = h("aside", { class: "cv-sheet", role: "dialog", "aria-modal": "true", "aria-label": f.title },
      h("button", { class: "cv-x", type: "button", "aria-label": "Close", onclick: close }, "×"),
      h("div", { class: "cv-sheet-h" },
        h("div", { class: "cv-meta" }, h("span", { class: "cv-chip" }, f.id), h("span", { class: `cv-chip s${f.severity}` }, SEV[f.severity]), themeSel, h("span", {}, (S.d.stages[f.stage_index] || {}).name), f.state !== "active" ? h("span", { class: "cv-chip bad" }, f.state) : null),
        h("h2", {}, f.title),
        h("div", { class: "cv-stages" }, [0, 1, 2, 3, 4, 5, 6, 7].map((i) => h("span", { class: i < f.stage_index ? "d" : i === f.stage_index ? "n" : null, title: (S.d.stages[i] || {}).name })))),
      h("div", { class: "cv-sheet-b" },
        f.needs_you ? h("div", { class: "cv-needs" }, f.needs_you) : null,
        h("section", {}, h("div", { class: "cv-label" }, "What happened"), h("p", { style: { margin: "6px 0 0" } }, f.detail || "–"),
          h("p", { class: "cv-meta", style: { marginTop: "6px" } }, d.persona ? `Found by ${d.persona.name}${f.step ? ` at “${f.step}”` : ""}` : `Reported by ${f.by}`, f.occurrences > 1 ? ` · ${f.personas_hit.length} personas, ${f.occurrences} times` : "")),
        h("section", {}, h("div", { class: "cv-label" }, f.repro_hash ? `Repro · locked ${f.repro_hash.slice(0, 8)}` : "Repro"),
          f.repro_hash ? h("pre", { class: "cv-pre" }, f.repro) : h("div", {}, repro, h("button", { class: "cv-btn", type: "button", style: { marginTop: "6px" }, onclick: run(() => api(`/api/converge/findings/${enc(id)}`, { method: "PUT", body: { repro: repro.value } }), "Repro saved: the next tick runs it") }, "Save the repro"))),
        f.scope ? h("section", {}, h("div", { class: "cv-label" }, "Scope"), h("dl", { class: "cv-kv" },
          h("dt", {}, "Area"), h("dd", {}, sc.area || "–"), h("dt", {}, "Files"), h("dd", { style: { fontFamily: "var(--mono)", fontSize: "var(--fs-xs)" } }, (sc.files || []).join(", ") || "–"),
          h("dt", {}, "Root cause"), h("dd", {}, sc.root_cause || "–"), h("dt", {}, "Acceptance"), h("dd", {}, sc.acceptance || "–"), h("dt", {}, "Size"), h("dd", {}, sc.size || "–"),
          h("dt", {}, "Door"), h("dd", {}, f.door ? `Yes: ${f.door}. Only you start it.` : "No"),
          f.branch ? [h("dt", {}, "Branch"), h("dd", { style: { fontFamily: "var(--mono)" } }, f.branch)] : null,
          f.agent ? [h("dt", {}, "Agent"), h("dd", {}, h("a", { href: `#/a/${f.agent}` }, `${f.agent}${f.agent_state ? ` (${f.agent_state})` : ""}`))] : null,
          f.pr_url ? [h("dt", {}, "Pull request"), h("dd", {}, h("a", { href: f.pr_url, target: "_blank", rel: "noopener" }, f.pr_url), f.ci ? ` · CI ${f.ci}` : "")] : null)) : null,
        f.ease_before != null || f.ease_after != null ? h("section", {}, h("div", { class: "cv-label" }, "The persona, before and after"), h("p", { style: { margin: "6px 0 0", fontFamily: "var(--mono)" } }, `ease ${f.ease_before ?? "–"} → ${f.ease_after ?? "–"}`)) : null,
        h("section", {}, h("div", { class: "cv-label" }, "Gates and events"), h("ul", { class: "cv-tl" }, (d.events || []).map((e) => h("li", {},
          h("span", { class: `ic ${e.passed ? "p" : e.gate === "reported" ? "" : "f"}` }, e.passed ? "✓" : e.gate === "reported" ? "" : "!"),
          h("div", {}, h("div", {}, `${e.gate}${e.from !== e.to && e.to ? ` → ${e.to}` : ""}`), h("div", { class: "s" }, e.summary || ""),
            e.evidence && Object.keys(e.evidence).length ? h("details", {}, h("summary", {}, "Evidence"), h("pre", { class: "cv-pre" }, JSON.stringify(e.evidence, null, 2))) : null),
          h("span", { class: "w" }, `${e.by} · ${ago(e.ts_ms)}`)))))),
      h("div", { class: "cv-sheet-f" }, actions));
    document.body.append(scrim, sheet);
  }

  // ------------------------------------------------------------ personas
  function spark(vals) {
    if (!vals || !vals.length) return h("div", { class: "cv-empty" }, "No runs yet");
    const W = 200, Y = (v) => 30 - ((v - 1) / 4) * 26, X = (i) => (vals.length === 1 ? 100 : 4 + i * (192 / (vals.length - 1)));
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("viewBox", `0 0 ${W} 34`); svg.setAttribute("class", "cv-spark"); svg.setAttribute("preserveAspectRatio", "none");
    svg.innerHTML = `<line x1="0" x2="200" y1="${Y(3)}" y2="${Y(3)}" stroke="var(--line-2)" stroke-dasharray="2 3"/><polyline points="${vals.map((v, i) => `${X(i)},${Y(v)}`).join(" ")}" fill="none" stroke="var(--accent)" stroke-width="1.75"/><circle cx="${X(vals.length - 1)}" cy="${Y(vals.at(-1))}" r="3" fill="var(--accent)"/>`;
    return svg;
  }

  function personas() {
    const d = S.d;
    const ps = d.personas || [];
    const pc = (p) => h("div", { class: `cv-pc ${p.state}` },
      h("div", {}, h("div", { class: "name" }, p.name), h("div", { class: "role" }, p.role)),
      h("div", { class: "cv-meta" }, h("span", { class: "cv-chip" }, p.origin), h("span", { class: `cv-chip ${p.state === "active" ? "ok" : p.state === "probation" ? "run" : ""}` }, p.state), (p.surfaces || []).map((x) => h("span", { class: "cv-chip" }, x)), p.busy ? h("span", { class: "cv-chip run" }, p.busy) : null),
      p.why ? h("div", { class: "role" }, p.why) : null,
      h("details", {}, h("summary", { class: "role" }, "Journey"), h("p", { class: "role" }, p.journey)),
      spark(p.ease_hist),
      h("div", { class: "cv-stats" },
        h("div", {}, h("div", { class: "v" }, p.ease == null ? "–" : String(p.ease)), h("div", { class: "l" }, "ease")),
        h("div", {}, h("div", { class: "v" }, String(p.runs)), h("div", { class: "l" }, "runs")),
        h("div", {}, h("div", { class: "v" }, p.repro_rate == null ? "–" : `${Math.round(p.repro_rate * 100)}%`), h("div", { class: "l" }, "repro rate")),
        h("div", {}, h("div", { class: "v" }, String(p.open)), h("div", { class: "l" }, "open"))),
      h("div", { class: "cv-meta" },
        h("button", { class: "cv-btn", type: "button", disabled: p.busy ? true : null, onclick: () => act(() => api(`/api/converge/personas/${p.id}/play`, { method: "POST", body: {} }), `${p.name.split(",")[0]} is playing`) }, "Play now"),
        p.state === "retired" ? h("button", { class: "cv-btn", type: "button", onclick: () => act(() => api(`/api/converge/personas/${p.id}/activate`, { method: "POST", body: {} }), "Active again") }, "Activate")
          : h("button", { class: "cv-btn", type: "button", onclick: () => act(() => api(`/api/converge/personas/${p.id}/retire`, { method: "POST", body: {} }), "Retired") }, "Retire")));
    const name = h("input", { type: "text", id: "cv-pn", placeholder: "Omar, data engineer" });
    const role = h("input", { type: "text", id: "cv-pr", placeholder: "Who they are and what they want" });
    const journey = h("textarea", { class: "cv-ta", id: "cv-pj", placeholder: "The task they try, end to end", style: { minHeight: "70px", fontFamily: "var(--sans)", fontSize: "var(--fs-sm)" } });
    const add = h("details", { class: "cv-panel", style: { marginTop: "16px" } }, h("summary", {}, "Add a persona"),
      h("div", { class: "cv-f" }, h("label", { for: "cv-pn" }, "Name"), name), h("div", { class: "cv-f" }, h("label", { for: "cv-pr" }, "Role"), role), h("div", { class: "cv-f" }, h("label", { for: "cv-pj" }, "Journey"), journey),
      h("button", { class: "cv-btn pri", type: "button", onclick: () => act(() => api("/api/converge/personas", { method: "POST", body: { repo: d.repo, name: name.value, role: role.value, journey: journey.value } }), "Persona added") }, "Add"));
    return h("div", {},
      h("div", { class: "cv-bar" }, h("p", { class: "cv-intro", style: { margin: 0, flex: "1" } }, "Each persona replays its journey on a schedule and after merges. A generated persona starts on probation and joins once a finding of theirs reproduces."),
        h("button", { class: "cv-btn pri", type: "button", onclick: () => act(() => api("/api/converge/personas/generate", { method: "POST", body: { repo: d.repo } }), "A new persona is on probation") }, "Generate a persona")),
      ["active", "probation", "retired"].map((stt) => {
        const list = ps.filter((p) => p.state === stt);
        return list.length ? h("section", { style: { marginBottom: "20px" } }, h("div", { class: "cv-label", style: { margin: "0 0 8px" } }, stt === "active" ? "Active" : stt === "probation" ? "On probation" : "Retired"), h("div", { class: "cv-grid" }, list.map(pc))) : null;
      }), add);
  }

  // ------------------------------------------------------------ metrics
  function metrics() {
    const m = S.d.metrics || {};
    const f = m.funnel || {};
    const rows = [["Found", f.found || 0], ["Reproduced", f.reproduced || 0], ["Merged", f.merged || 0]];
    const max = Math.max(1, ...rows.map((r) => r[1]));
    const bt = m.by_theme || {};
    const tmax = Math.max(1, ...Object.values(bt).map((x) => x.open));
    const hist = m.history || [];
    return h("div", { class: "cv-panels" },
      h("div", { class: "cv-panel" }, h("h3", {}, "Open problems by kind"), h("div", { class: "sub" }, "Where the product is weakest now; a click filters the board"),
        h("table", { class: "cv-t" }, h("thead", {}, h("tr", {}, h("th", {}, "Kind"), h("th", {}, ""), h("th", { class: "n" }, "Open"), h("th", { class: "n" }, "Blockers"), h("th", { class: "n" }, "Merged"))),
          h("tbody", {}, Object.entries(bt).sort((a, b) => b[1].blocker - a[1].blocker || b[1].open - a[1].open).map(([k, x]) => h("tr", { style: { cursor: "pointer" }, onclick: () => { S.theme = k; location.hash = "#/converge"; } },
            h("td", {}, THEME_WORDS[k] || k), h("td", { style: { width: "40%" } }, h("div", { class: "cv-hbar", style: { width: `${(x.open / tmax) * 100}%` } })), h("td", { class: "n" }, x.open), h("td", { class: "n" }, x.blocker), h("td", { class: "n" }, x.merged)))))),
      h("div", { class: "cv-panel" }, h("h3", {}, "Where findings leave the loop"), h("div", { class: "sub" }, `${f.cant_reproduce || 0} could not be reproduced, ${f.duplicates || 0} were duplicates`),
        h("table", { class: "cv-t" }, h("tbody", {}, rows.map(([k, v]) => h("tr", {}, h("td", {}, k), h("td", { style: { width: "60%" } }, h("div", { class: "cv-hbar", style: { width: `${(v / max) * 100}%` } })), h("td", { class: "n" }, v)))))),
      h("div", { class: "cv-panel" }, h("h3", {}, "The loop, day by day"), h("div", { class: "sub" }, "One row a day, written on each tick"),
        h("table", { class: "cv-t" }, h("thead", {}, h("tr", {}, h("th", {}, "Day"), h("th", { class: "n" }, "Ease"), h("th", { class: "n" }, "Open"), h("th", { class: "n" }, "Merged"), h("th", { class: "n" }, "Spend 7d"))),
          h("tbody", {}, hist.slice(-14).reverse().map((r) => h("tr", {}, h("td", {}, r.day), h("td", { class: "n" }, r.ease == null ? "–" : r.ease), h("td", { class: "n" }, r.open), h("td", { class: "n" }, (r.funnel || {}).merged || 0), h("td", { class: "n" }, usd(r.cost_7d))))))),
      h("div", { class: "cv-panel" }, h("h3", {}, "Spend and quality"), h("table", { class: "cv-t" }, h("tbody", {},
        h("tr", {}, h("td", {}, "Model spend today"), h("td", { class: "n" }, usd(m.spent_today))),
        h("tr", {}, h("td", {}, "Spend, 7 days"), h("td", { class: "n" }, usd(m.cost_7d))),
        h("tr", {}, h("td", {}, "Per merged fix, 7 days"), h("td", { class: "n" }, usd(m.cost_per_merged_fix))),
        h("tr", {}, h("td", {}, "Repro rate"), h("td", { class: "n" }, m.repro_rate == null ? "–" : `${Math.round(m.repro_rate * 100)}%`)),
        h("tr", {}, h("td", {}, "Regression rate"), h("td", { class: "n" }, m.regression_rate == null ? "–" : `${Math.round(m.regression_rate * 100)}%`))))));
  }

  // ------------------------------------------------------------ settings
  function settings() {
    const d = S.d;
    const st = { ...d.settings };
    const field = (k, l, hint, type = "number") => {
      const inp = h("input", { type, id: `cv-s-${k}`, value: st[k] == null ? "" : String(st[k]), step: type === "number" ? "any" : null });
      inp.addEventListener("change", () => { st[k] = type === "number" ? Number(inp.value) : (inp.value || null); });
      return h("div", { class: "cv-f" }, h("label", { for: `cv-s-${k}` }, l), inp, h("span", { class: "hint" }, hint));
    };
    const list = (k, l, hint) => {
      const ta = h("textarea", { class: "cv-ta", id: `cv-s-${k}`, spellcheck: "false", style: { minHeight: "140px" } });
      ta.value = (st[k] || []).join("\n");
      ta.addEventListener("change", () => { st[k] = ta.value.split("\n").map((x) => x.trim()).filter(Boolean); });
      return h("div", { class: "cv-f" }, h("label", { for: `cv-s-${k}` }, l), ta, h("span", { class: "hint" }, hint));
    };
    const tog = (k, l, hint) => {
      const inp = h("input", { type: "checkbox", id: `cv-s-${k}`, checked: st[k] ? true : null });
      inp.addEventListener("change", () => { st[k] = inp.checked; });
      return h("div", { class: "cv-f" }, h("label", { for: `cv-s-${k}` }, inp, " ", l), h("span", { class: "hint" }, hint));
    };
    return h("div", { class: "cv-panels" },
      h("div", { class: "cv-panel" }, h("h3", {}, "Limits"), h("div", { class: "sub" }, "The autopilot never goes past these"),
        field("budget_usd_day", "Model budget per day (USD)", "Persona runs, scoping and new personas. A spent budget pauses new work."),
        field("runs_at_once", "Persona runs at once", "Each in its own checkout."),
        field("wip", "Findings in development at once", "Each agent on its own worktree and branch."),
        field("max_rounds", "Rounds before it asks you", "Develop and verify rounds without passing.")),
      h("div", { class: "cv-panel" }, h("h3", {}, "Cadence and checks"), h("div", { class: "sub" }, "When personas play and what a fix must pass"),
        field("tick_s", "Autopilot tick (seconds)", "How often it advances what it can."),
        field("replay_every_h", "Replay each persona every (hours)", "Plus once after each merge."),
        field("repro_runs", "Repro runs", "How many times a repro must fail on the base branch, and pass on the fix."),
        field("test_cmd", "Test command", "Run in the agent's worktree by the develop gate, e.g. cd chloedb-workbench && cargo test --profile ci", "text"),
        field("base", "Base branch", "Where findings are reproduced and fixes merge (empty: the checkout's branch).", "text"),
        field("model", "Persona model", "For persona runs, scoping and the generator.", "text")),
      h("div", { class: "cv-panel" }, h("h3", {}, "What fix agents may do"), h("div", { class: "sub" }, "Set once, so a run does not stop at every new command. Each agent works in its own worktree; pushing and pull requests stay Converge's own."),
        field("dev_permission_mode", "Permission mode", "auto: Claude Code's classifier answers what the lists do not. default: those wait for you in Needs you.", "text"),
        list("dev_allow", "Allowed without asking", "One Claude Code rule a line, e.g. Bash(cargo:*)"),
        list("dev_deny", "Never", "One rule a line")),
      h("div", { class: "cv-panel" }, h("h3", {}, "What runs on its own"), h("div", { class: "sub" }, "Merges and one-way doors always wait for you"),
        tog("enabled", "Autopilot", "Ticks on its own, every tick."),
        tog("generate", "Generate new personas", "When new serious findings dry up, one aimed at the least-played part."),
        tog("retire", "Retire stale personas", "After 3 runs with nothing new, or a repro rate under 40%."),
        tog("open_pr", "Open pull requests", "Push the branch and open its pull request with gh at review."),
        h("button", { class: "cv-btn pri", type: "button", onclick: () => act(() => api("/api/converge/settings", { method: "PUT", body: { repo: d.repo, settings: st } }), "Settings saved") }, "Save")));
  }

  const onHash = () => { if (location.hash.startsWith("#/converge")) { document.querySelectorAll(".cv-sheet, .cv-scrim").forEach((e) => e.remove()); if (S.d) draw(); } };
  const onKey = (e) => { if (e.key === "Escape") { const x = document.querySelector(".cv-sheet .cv-x"); if (x) x.click(); } };
  window.addEventListener("hashchange", onHash);
  document.addEventListener("keydown", onKey);
  load();
  S.timer = setInterval(() => { if (!document.querySelector(".cv-sheet") && !document.querySelector(".cv input:focus, .cv textarea:focus")) load(); }, 10000);
  return {
    update() {},
    unmount() {
      S.alive = false;
      clearInterval(S.timer);
      window.removeEventListener("hashchange", onHash);
      document.removeEventListener("keydown", onKey);
      document.querySelectorAll(".cv-sheet, .cv-scrim").forEach((e) => e.remove());
    },
  };
}

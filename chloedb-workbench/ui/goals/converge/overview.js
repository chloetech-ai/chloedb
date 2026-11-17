// The project overview (Converge's home, #/converge?project=…): what the
// flow is doing and what waits for you, on one screen.
//   top bar      the project, the autopilot, the day's spend, Pause, Stop everything
//   KPI strip    open problems (trend), merged this week, found → merged,
//                cost per fix, persona ease before → after, needs you
//   the flow     the eight steps with live counts; ✋ marks your three picks;
//                a step filters the board (&step=)
//   filters      component, severity, persona (&comp= &sev= &who=)
//   board        Found · Reproduce/Scope · Build/Verify · Merge, a card each
//                problem: tags, seen N×, an 8-part bar, its live line and its move
//   right rail   Live now (every agent and persona run, foldable) and
//                What happened (the activity, newest first)
// A phone gets the compact version: a KPI row, the flow, Needs you, Live now;
// the board is #/converge/board. Keys (desktop): j/k, Enter, r, b, d, m, ?.
import { h, put, toast, enc, usd, ago, toMs, SEV, STAGES, STAGE_NAME, blockedOf, blockStep, queuedOf, agentOn, happening, seenOf, evidenceOf, firstName, doingWords, secsWords, firstSentences } from "/goals/converge/util.js";
import { useStyle } from "/levels/common.js";
import { reproduce, build, askDismiss, dismissBox, keyHelp, stopButton } from "/goals/converge/home.js";

const CSS = `
.cv.cv-wide { max-width: 1480px; }
.ov-top { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; margin-bottom: 16px; min-height: 44px; }
.ov-proj { position: relative; display: inline-flex; align-items: center; gap: 8px; min-height: 40px; padding: 0 12px; border: var(--hair) solid var(--line-2); border-radius: var(--r-sm); background: var(--surface); font-weight: var(--w-title); font-size: var(--fs-md); }
.ov-proj .car { color: var(--muted); font-size: var(--fs-sm); }
.ov-proj select { position: absolute; inset: 0; opacity: 0; width: 100%; cursor: pointer; font-size: 16px; }
.ov-proj:focus-within { outline: 2px solid var(--accent); outline-offset: 2px; }
.ov-auto { display: inline-flex; align-items: center; gap: 8px; font-size: var(--fs-sm); color: var(--text); text-decoration: none; }
.ov-auto .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--muted); }
.ov-auto .dot.on { background: var(--good); }
.ov-spend { font-size: var(--fs-sm); color: var(--text-2); }
.ov-top .cv-btn { min-height: 40px; }
.ov-top .cv-btn.stop { color: var(--crit-ink); border-color: var(--crit-soft); background: var(--surface); }
.ov-top .cv-btn.stop:hover { background: var(--crit-soft); }

.ov-k { font-family: var(--mono); font-size: 11px; letter-spacing: .06em; text-transform: uppercase; color: var(--muted); font-weight: var(--w-mid); }
.ov-card { background: var(--surface); border: var(--hair) solid var(--line); border-radius: 10px; padding: 12px; display: flex; flex-direction: column; gap: 6px; min-width: 0; }
.ov-kpis { display: grid; grid-template-columns: repeat(6, minmax(0, 1fr)); gap: 12px; margin-bottom: 16px; }
.ov-kpi { text-decoration: none; color: var(--text); }
.ov-kpi .row { display: flex; align-items: flex-end; justify-content: space-between; gap: 8px; min-width: 0; }
.ov-kpi .v { font-size: 24px; line-height: 1.1; font-weight: var(--w-title); font-variant-numeric: tabular-nums; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.ov-kpi .sub { font-size: var(--fs-xs); color: var(--text-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.ov-kpi .sub.good { color: var(--good-ink); } .ov-kpi .sub.bad { color: var(--crit-ink); }
.ov-kpi svg { flex: 0 1 72px; min-width: 32px; height: 28px; }
.ov-kpi.you { border-color: var(--warn-line); background: var(--warn-soft); }
.ov-kpi.you .ov-k, .ov-kpi.you .v, .ov-kpi.you .sub { color: var(--warn-ink); }
a.ov-kpi:hover { border-color: var(--accent-line); }

.ov-flow { padding: 14px 16px; margin-bottom: 16px; }
.ov-flow .hd { display: flex; justify-content: space-between; align-items: center; gap: 12px; }
.ov-flow .hd .key { font-size: var(--fs-xs); color: var(--text-2); display: inline-flex; align-items: center; gap: 6px; }
.ov-flow .hd .key i { width: 10px; height: 10px; border-radius: 2px; background: var(--warn); }
.ov-steps { display: flex; align-items: stretch; gap: 4px; margin-top: 10px; }
.ov-arr { align-self: center; color: var(--muted); font-size: var(--fs-sm); flex: none; }
.ov-step { flex: 1 1 0; min-width: 0; border: var(--hair) solid var(--line); border-radius: 8px; padding: 8px 10px; background: var(--surface); color: var(--text); text-align: left; font: inherit; cursor: pointer; display: flex; flex-direction: column; gap: 1px; text-decoration: none; }
.ov-step:hover { border-color: var(--accent-line); }
.ov-step .l { font-size: var(--fs-xs); color: var(--text-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.ov-step .n { font-size: 22px; line-height: 1.15; font-weight: var(--w-title); font-variant-numeric: tabular-nums; }
.ov-step .s { font-size: var(--fs-xs); color: var(--muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.ov-step .s.on { color: var(--accent-text); }
.ov-step.live { background: var(--accent-soft); border-color: var(--accent-line); }
.ov-step.you { border-color: var(--warn-line); }
.ov-step.you.has { background: var(--warn-soft); }
.ov-step.you .l, .ov-step.you .s { color: var(--warn-ink); }
.ov-step.done { border-color: color-mix(in srgb, var(--good) 35%, var(--line)); background: var(--good-soft); }
.ov-step.done .l, .ov-step.done .s { color: var(--good-ink); }
.ov-step[aria-pressed="true"] { outline: 2px solid var(--accent); outline-offset: 1px; }

.ov-filters { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; margin-bottom: 14px; }
.ov-filters .ov-k { margin: 0 2px 0 8px; } .ov-filters .ov-k:first-child { margin-left: 0; }
.ov-chip { display: inline-flex; align-items: center; gap: 6px; min-height: 32px; padding: 0 11px; border: var(--hair) solid var(--line-2); border-radius: var(--r-pill); background: var(--surface); color: var(--text); font: inherit; font-size: var(--fs-sm); cursor: pointer; white-space: nowrap; }
.ov-chip .n { font-family: var(--mono); font-size: var(--fs-xs); color: var(--muted); }
.ov-chip.on { background: var(--text); border-color: var(--text); color: var(--surface); }
.ov-chip.on .n { color: var(--surface); opacity: .8; }
.ov-chip .cv-sev { width: 8px; height: 8px; }
.ov-chip.clear { border-style: dashed; color: var(--text-2); }

.ov-main { display: flex; gap: 16px; align-items: flex-start; }
.ov-board { flex: 1; min-width: 0; display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 12px; }
.ov-col { display: flex; flex-direction: column; gap: 8px; min-width: 0; }
.ov-colh { display: flex; justify-content: space-between; align-items: baseline; gap: 6px; min-height: 22px; }
.ov-colh b { font-size: var(--fs-body); font-weight: var(--w-title); }
.ov-colh span { font-size: var(--fs-xs); color: var(--text-2); }
.ov-colh span.you { color: var(--warn-ink); }
.ov-colempty { font-size: var(--fs-sm); color: var(--muted); padding: 10px 2px; border-top: var(--hair) dashed var(--line); }
.ov-bc { position: relative; gap: 7px; }
.ov-bc.cur { outline: 2px solid var(--accent); outline-offset: 1px; }
.ov-bc.q { border-style: dashed; }
.ov-bc.ready { border-color: color-mix(in srgb, var(--good) 45%, var(--line)); }
.ov-bc.blk { border-color: var(--crit); }
.ov-bc.done { opacity: .78; }
.ov-bc .bc-main { all: unset; cursor: pointer; display: flex; flex-direction: column; gap: 7px; min-width: 0; }
.ov-bc .bc-main:focus-visible { outline: 2px solid var(--accent); outline-offset: 4px; border-radius: 4px; }
.ov-bc .t { font-size: var(--fs-sm); font-weight: var(--w-title); line-height: 1.35; overflow-wrap: anywhere; display: -webkit-box; -webkit-line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden; }
.ov-bc.done .t { font-size: var(--fs-sm); }
.ov-tags { display: flex; gap: 5px; flex-wrap: wrap; }
.ov-tag { font-size: 11px; padding: 2px 6px; border-radius: 4px; background: var(--surface-3); color: var(--text-2); white-space: nowrap; max-width: 100%; overflow: hidden; text-overflow: ellipsis; }
.ov-tag.s4 { background: var(--crit-soft); color: var(--crit-ink); } .ov-tag.s3 { background: var(--warn-soft); color: var(--warn-ink); }
.ov-tag.seen { background: var(--accent-soft); color: var(--accent-text); }
.ov-line { font-size: var(--fs-xs); color: var(--text-2); line-height: 1.4; overflow-wrap: anywhere; display: -webkit-box; -webkit-line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden; }
.ov-line.on { color: var(--accent-text); } .ov-line.good { color: var(--good-ink); } .ov-line.bad { color: var(--crit-ink); } .ov-line.you { color: var(--warn-ink); }
.ov-dots { display: flex; gap: 4px; align-items: center; font-size: var(--fs-xs); color: var(--text-2); }
.ov-dots i { width: 8px; height: 8px; border-radius: 50%; background: var(--surface-3); display: inline-block; }
.ov-dots i.f { background: var(--crit); } .ov-dots i.p { background: var(--good); }
.ov-seg { display: flex; gap: 2px; height: 6px; border-radius: 3px; overflow: hidden; }
.ov-seg i { flex: 1; background: var(--surface-3); }
.ov-seg i.d { background: var(--accent); } .ov-seg i.n { background: color-mix(in srgb, var(--accent) 50%, var(--surface)); } .ov-seg i.w { background: var(--warn); } .ov-seg i.x { background: var(--crit); }
.ov-seg i.n.pulse { animation: cv-pulse 1.4s ease-in-out infinite; }
.ov-acts { display: flex; gap: 6px; flex-wrap: wrap; }
.ov-acts .cv-btn { min-height: 32px; padding: 0 12px; font-size: var(--fs-sm); }
.ov-more { all: unset; cursor: pointer; font-size: var(--fs-sm); color: var(--text-2); padding: 10px 12px; border: var(--hair) solid var(--line); border-radius: 10px; background: var(--surface); text-align: left; }
.ov-more b { color: var(--accent-text); font-weight: var(--w-mid); }
.ov-more:focus-visible { outline: 2px solid var(--accent); }
.ov-bc .ib-dis { display: flex; flex-direction: column; gap: 6px; }
.ov-bc .ib-dis .cv-btn { min-height: 36px; }

.ov-rail { width: 380px; flex: none; display: flex; flex-direction: column; gap: 12px; min-width: 0; }
.ov-rail .ov-card { padding: 14px; gap: 8px; }
.ov-rail .hd { display: flex; justify-content: space-between; align-items: center; gap: 8px; }
.ov-rail .hd span, .ov-rail .hd a { font-size: var(--fs-xs); color: var(--text-2); }
.ov-rail .hd a { color: var(--accent-text); }
.ov-task { border: var(--hair) solid var(--line); border-radius: 8px; padding: 8px 10px; display: flex; flex-direction: column; gap: 6px; }
.ov-task .th { all: unset; cursor: pointer; display: flex; justify-content: space-between; gap: 8px; font-size: var(--fs-sm); min-height: 24px; align-items: center; }
.ov-task .th:focus-visible { outline: 2px solid var(--accent); border-radius: 4px; }
.ov-task .th b { font-weight: var(--w-title); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
.ov-task .th span { color: var(--text-2); font-size: var(--fs-xs); flex: none; }
.ov-task .now { font-size: var(--fs-xs); color: var(--text-2); overflow-wrap: anywhere; }
.ov-task .now.on { color: var(--accent-text); }
.ov-task .steps { display: flex; flex-direction: column; gap: 3px; font-size: var(--fs-xs); color: var(--text-2); border-left: 2px solid var(--line); padding-left: 8px; }
.ov-task .steps .on { color: var(--accent-text); } .ov-task .steps .todo { color: var(--muted); }
.ov-task .links { display: flex; gap: 12px; font-size: var(--fs-xs); }
.ov-task .links a, .ov-task .links button { all: unset; cursor: pointer; color: var(--accent-text); min-height: 24px; display: inline-flex; align-items: center; }
.ov-task .links a:focus-visible, .ov-task .links button:focus-visible { outline: 2px solid var(--accent); }
.ov-feed { display: flex; flex-direction: column; gap: 7px; font-size: var(--fs-xs); margin: 0; padding: 0; list-style: none; }
.ov-feed li { display: flex; gap: 8px; min-width: 0; line-height: 1.4; }
.ov-feed .tm { font-family: var(--mono); color: var(--muted); width: 40px; flex: none; }
.ov-feed .tx { min-width: 0; overflow-wrap: anywhere; color: var(--text); }
.ov-feed a.tx { text-decoration: none; }
.ov-feed a.tx:hover { text-decoration: underline; }
.ov-feed i { color: var(--text-2); }
.ov-quiet { font-size: var(--fs-sm); color: var(--muted); margin: 0; }

@media (max-width: 1180px) {
  .ov-main { flex-direction: column; }
  .ov-rail { width: 100%; display: grid; grid-template-columns: 1fr 1fr; }
  .ov-kpis { grid-template-columns: repeat(3, minmax(0, 1fr)); }
}
@media (max-width: 900px) {
  .ov-board { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .ov-steps { overflow-x: auto; }
  .ov-step { min-width: 108px; }
}

/* A phone */
.ov-ph { display: flex; flex-direction: column; gap: 14px; }
.ov-ph-h { display: flex; justify-content: space-between; align-items: center; gap: 8px; }
.ov-ph-h .ov-proj { border: 0; padding: 0; background: none; min-height: 44px; font-size: 15px; }
.ov-ph-h .ov-auto { font-size: var(--fs-xs); }
.ov-ph-k { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 6px; }
.ov-ph-k > * { border: var(--hair) solid var(--line); border-radius: 8px; padding: 6px 8px; background: var(--surface); min-width: 0; text-decoration: none; color: var(--text); }
.ov-ph-k b { display: block; font-size: 18px; font-variant-numeric: tabular-nums; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.ov-ph-k span { font-size: 11px; color: var(--text-2); white-space: nowrap; }
.ov-ph-f { margin-bottom: 4px; display: grid; grid-template-columns: repeat(7, minmax(0, 1fr)); gap: 4px; }
.ov-ph-f a { display: flex; flex-direction: column; align-items: center; justify-content: center; min-height: 48px; white-space: nowrap; overflow: hidden; letter-spacing: -.01em; border: var(--hair) solid var(--line); border-radius: 6px; background: var(--surface); font-size: 11px; color: var(--text-2); text-decoration: none; text-align: center; line-height: 1.15; padding: 2px 0; }
.ov-ph-f a b { font-size: 15px; color: var(--text); }
.ov-ph-f a.you { border-color: var(--warn-line); color: var(--warn-ink); } .ov-ph-f a.you.has { background: var(--warn-soft); } .ov-ph-f a.you b { color: var(--warn-ink); }
.ov-ph-f a.done { background: var(--good-soft); color: var(--good-ink); } .ov-ph-f a.done b { color: var(--good-ink); }
.ov-ph-f a.live b::after { content: ""; display: inline-block; width: 5px; height: 5px; border-radius: 50%; background: var(--accent); margin-left: 3px; vertical-align: 3px; animation: cv-pulse 1.4s ease-in-out infinite; }
.ov-ph .ov-k.you { color: var(--warn-ink); }
.ov-ph .big { display: flex; align-items: center; justify-content: center; min-height: 44px; border-radius: 8px; font-weight: var(--w-title); font-size: var(--fs-sm); text-decoration: none; }
.ov-ph .big.go { background: var(--good); color: #fff; }
.ov-ph .big.pri { background: var(--accent); color: var(--accent-ink); }
.ov-card.ov-ph-row { flex-direction: row; justify-content: space-between; align-items: center; gap: 8px; min-height: 44px; text-decoration: none; color: var(--text); }
.ov-ph-row small { display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; font-size: var(--fs-xs); color: var(--text-2); margin-top: 2px; }
.ov-ph-row > span:first-child { min-width: 0; }
.ov-ph-row .car { color: var(--muted); }
.ov-ph .ov-card { padding: 12px; }

/* The phone's own tabs: Overview · Board · Personas · Activity (below 640 px) */
@media (max-width: 640px) {
  .ov-board { grid-template-columns: minmax(0, 1fr); }
  .ov-filters { flex-wrap: nowrap; overflow-x: auto; margin: 0 -16px 12px; padding: 2px 16px 4px; scrollbar-width: none; }
  .ov-filters::-webkit-scrollbar { display: none; }
  .ov-chip { min-height: 44px; }
  .ov-acts .cv-btn { min-height: 44px; }
  .ov-more { min-height: 44px; box-sizing: border-box; display: flex; align-items: center; }
  .ov-rail { grid-template-columns: minmax(0, 1fr); }
  .ov-task .th { min-height: 44px; }
  .ov-task .links a, .ov-task .links button { min-height: 44px; }
  .ov-steps { display: none; }
}
`;

/** The eight steps of the flow; `you` marks your three picks. */
export const FLOW = [
  { k: "play", n: "Personas play", short: "play" },
  { k: "found", n: "Found", short: "found", you: true, stages: ["found"] },
  { k: "reproduce", n: "Reproduce", short: "repro", stages: ["reproduce"] },
  { k: "scope", n: "Scope", short: "scope", stages: ["scope"] },
  { k: "prioritize", n: "Ready to build", short: "build", you: true, stages: ["prioritize"] },
  { k: "develop", n: "Build + verify", short: "fixing", stages: ["develop", "verify"] },
  { k: "review", n: "Ready to merge", short: "merge", you: true, stages: ["review"] },
  { k: "merged", n: "Merged", short: "done", stages: ["merged"] },
];
/** The board's four columns. */
const COLS = [
  { k: "found", n: "Found", stages: ["found"] },
  { k: "repro", n: "Reproduce · Scope", stages: ["reproduce", "scope"] },
  { k: "build", n: "Build · Verify", stages: ["prioritize", "develop", "verify"] },
  { k: "merge", n: "Merge", stages: ["review", "merged"] },
];
const WEEK = 7 * 86400000;
const clock = (t) => { const ms = toMs(t); return ms ? new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false }) : ""; };
const short = (t, n = 28) => { const x = String(t || ""); return x.length > n ? `${x.slice(0, n - 1).trimEnd()}…` : x; };
const prNo = (u) => ((/\/pull\/(\d+)/.exec(u || "") || [])[1] || null);
const pname = (C, id) => { const p = (C.S.d.personas || []).find((x) => x.id === id || x.name === id); return p ? firstName(p.name) : id && !/^cp[0-9a-f]{6,}$/.test(id) ? firstName(id) : null; };

// ------------------------------------------------------------------ data
/** Which cards wait for you (the server's one rule when it sends it). */
export function needsOf(C) {
  const out = new Map();
  for (const f of C.findings()) {
    if (f.state !== "active") continue;
    if (blockedOf(f)) { out.set(f.id, `Blocked: ${blockStep(f, blockedOf(f))}`); continue; }
    if (queuedOf(f)) continue;
    const why = "needs_now" in f ? f.needs_now : f.needs_you || (f.stage === "found" ? "Reproduce or dismiss" : f.stage === "review" && f.ci === "pass" ? "Approve the merge" : null);
    if (why) out.set(f.id, why);
  }
  return out;
}

/** The URL's board filters: step, component, severity, persona. */
function filters() {
  const q = new URLSearchParams(location.hash.split("?")[1] || "");
  return { step: q.get("step") || "", comp: q.get("comp") || "", sev: q.get("sev") || "", who: q.get("who") || "" };
}
function setFilter(C, k, v) {
  const [path, qs] = location.hash.split("?");
  const q = new URLSearchParams(qs || "");
  if (v) q.set(k, v); else q.delete(k);
  history.replaceState(history.state, "", `${path}?${q}`);
  C.draw();
}

/** The active cards (and the week's merges) the filters leave. */
function boardCards(C, fl, skip = "") {
  const week = Date.now() - WEEK;
  return C.findings().filter((f) => {
    if (f.stage === "merged" ? (toMs(f.stage_since) || 0) < week : f.state !== "active") return false;
    if (skip !== "comp" && fl.comp && (f.component || "other") !== fl.comp) return false;
    if (skip !== "sev" && fl.sev && String(f.severity) !== fl.sev) return false;
    if (skip !== "who" && fl.who && !(f.persona === fl.who || (f.personas_hit || []).includes(fl.who))) return false;
    return true;
  });
}

/** An agent's place in its work, from what it is doing now: the most it reached is kept. */
const PHASES = {
  fix: ["Read the card", "Find the code", "Make the change", "Run the tests", "Commit"],
  repro: ["Read the card", "Write the repro", "Run it on the base"],
  scope: ["Read the code", "Write the plan"],
};
function phase(C, kind, agent, doing) {
  const w = String(doing || "");
  let n = 0;
  if (kind === "fix") n = /^commit|^git (add|commit|push)/.test(w) ? 4 : /running tests|^building|checking the code|running its repro|^running /.test(w) ? 3 : /^editing|^writing/.test(w) ? 2 : /^reading (?!its task)|looking around|searching|looking for/.test(w) ? 1 : 0;
  else if (kind === "repro") n = /running its repro|^running /.test(w) ? 2 : /^writing|^editing/.test(w) ? 1 : 0;
  else n = /^writing/.test(w) ? 1 : 0;
  const M = (C.S.phase = C.S.phase || new Map());
  const k = `${agent}`;
  const best = Math.max(n, M.get(k) || 0);
  M.set(k, best);
  return best;
}
const agentKind = (f) => (f.stage === "reproduce" ? "repro" : f.stage === "scope" || f.stage === "prioritize" ? "scope" : "fix");
const KIND_NAME = { repro: "Repro agent", fix: "Fix agent", scope: "Planning agent" };

/** Every agent at work and every persona run, newest first. */
export function liveItems(C) {
  const d = C.S.d, out = [];
  for (const r of d.runs || []) {
    if (r.state !== "running") continue;
    const p = (d.personas || []).find((x) => x.id === r.persona);
    const gen = r.purpose === "generate" || !r.persona;
    const at = r.at && typeof r.at === "object" ? r.at : null;
    const role = p && p.name.includes(",") ? p.name.split(",").slice(1).join(",").trim() : "";
    const card = r.finding ? C.findings().find((f) => f.id === r.finding) : null;
    out.push({
      key: `run:${r.id}`, run: r.id, since: toMs(r.created),
      title: gen ? "Writing personas from the code" : `${firstName(p ? p.name : "A persona")} ${r.purpose === "verify" ? `replays${card ? ` · ${short(card.title, 26)}` : " a fix"}` : `plays${role ? ` · ${short(role, 26)}` : ""}`}`,
      of: at && at.of ? at.of : 0, n: at && at.n ? at.n - 1 : 0,
      now: at ? `Step ${at.n} of ${at.of}${at.name ? `: “${short(at.name, 70)}”` : ""}` : gen ? "Reading the code" : "Opening a fresh checkout",
    });
  }
  for (const f of C.findings()) {
    if (f.state !== "active" || !(agentOn(f) || f.busy)) continue;
    if (!agentOn(f)) {
      out.push({ key: `busy:${f.id}`, fid: f.id, since: toMs(f.stage_since), title: `Converge · ${short(f.title, 30)}`, of: 1, n: 0, now: String(f.busy) });
      continue;
    }
    const kind = agentKind(f);
    const ph = PHASES[kind];
    const doing = f.agent_state === "starting" ? "starting" : f.agent_state === "waiting" ? "waits for an answer" : doingWords(f.agent_doing) || "working";
    const n = phase(C, kind, f.agent, doing);
    out.push({ key: `ag:${f.agent}`, fid: f.id, agent: f.agent, kind, since: toMs(f.agent_created) || toMs(f.stage_since), title: `${KIND_NAME[kind]} · ${short(f.title, 30)}`, of: ph.length, n, phases: ph, now: `Step ${n + 1} of ${ph.length}: ${doing}` });
  }
  return out.sort((a, b) => (b.since || 0) - (a.since || 0));
}

// ------------------------------------------------------------------ pieces
function seg(n, cls) { return h("span", { class: "ov-seg", "aria-hidden": "true" }, Array.from({ length: n }, (_, i) => h("i", { class: cls(i) || null }))); }
function spark(vals, color) {
  const v = (vals || []).map(Number).filter((x) => Number.isFinite(x));
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 84 28"); svg.setAttribute("aria-hidden", "true"); svg.setAttribute("preserveAspectRatio", "none");
  if (v.length < 2) return svg;
  const lo = Math.min(...v), hi = Math.max(...v), span = hi - lo || 1;
  const pts = v.map((x, i) => `${(i / (v.length - 1)) * 84},${24 - ((x - lo) / span) * 20}`).join(" ");
  svg.innerHTML = `<polyline points="${pts}" fill="none" style="stroke:${color}" stroke-width="2" vector-effect="non-scaling-stroke" stroke-linejoin="round"/>`;
  return svg;
}

/** A card's eight parts: done, the one it is in (amber when it waits for you, red when blocked), the rest. */
export function cardSeg(f, needs) {
  const i = f.stage === "merged" ? 8 : Math.max(0, STAGES.indexOf(f.stage));
  const live = agentOn(f) || !!f.busy;
  const cur = blockedOf(f) ? "x" : needs ? "w" : queuedOf(f) ? "" : live ? "n pulse" : "n";
  return seg(8, (j) => (j < i ? "d" : j === i ? cur : ""));
}

/** The live line of a card. */
function cardLine(C, f, needs) {
  const b = blockedOf(f);
  if (b) return h("span", { class: "ov-line bad", title: blockStep(f, b) }, `Blocked on you: ${firstSentences(blockStep(f, b), 1)}`);
  const q = queuedOf(f);
  if (q) return h("span", { class: "ov-line" }, `⏸ ${f.waits_for ? `Queued: waits for “${short((C.findings().find((x) => x.id === f.waits_for) || {}).title || "another card", 32)}” (same files)` : `Queued: ${q}`}`);
  if (f.stage === "merged") return h("span", { class: "ov-line" }, [f.pr_url && prNo(f.pr_url) ? `merged PR #${prNo(f.pr_url)}` : "merged", ago(f.stage_since) ? `${ago(f.stage_since)} ago` : null].filter(Boolean).join(" · "));
  if (f.stage === "found") {
    const hits = (f.personas_hit && f.personas_hit.length ? f.personas_hit : f.persona ? [f.persona] : []).map((x) => pname(C, x)).filter(Boolean);
    return h("span", { class: "ov-line" }, [hits.length ? hits.slice(0, 3).join(", ") : f.by === "you" ? "You" : null, ago(f.created) ? `${ago(f.created)} ago` : null].filter(Boolean).join(" · "));
  }
  if (f.stage === "review" && !f.conflict && f.ci === "pass") return h("span", { class: "ov-line good" }, mergeLine(f));
  if (agentOn(f)) {
    const kind = agentKind(f);
    const doing = f.agent_state === "starting" ? "starting" : f.agent_state === "waiting" ? "waits for an answer" : doingWords(f.agent_doing) || "working";
    const n = phase(C, kind, f.agent, doing);
    return h("span", { class: "ov-line on" }, h("i", { class: "cv-pulse", "aria-hidden": "true" }), `${KIND_NAME[kind]} · step ${n + 1} of ${PHASES[kind].length}: ${doing}${ago(f.stage_since) ? ` · ${ago(f.stage_since)}` : ""}`);
  }
  if (f.busy) return h("span", { class: "ov-line on" }, h("i", { class: "cv-pulse", "aria-hidden": "true" }), String(f.busy));
  if (needs) return h("span", { class: "ov-line you" }, firstSentences(needs, 1));
  return h("span", { class: "ov-line" }, happening(f, C.settings()));
}
function mergeLine(f) {
  const e = evidenceOf(f);
  const after = Array.isArray(e.repro_after) ? e.repro_after.map(Number) : [];
  const rp = e.replay && typeof e.replay === "object" ? e.replay : null;
  const eb = rp?.ease_before ?? f.ease_before, ea = rp?.ease_after ?? f.ease_after;
  return [f.ci === "pass" ? "CI green" : null, after.length ? `repro ${after.filter((x) => x === 0).length}/${after.length} fixed` : null, eb != null && ea != null ? `ease ${eb} → ${ea}` : null].filter(Boolean).join(" · ") || "Ready for your approval";
}

/** One card of the board. */
function boardCard(C, f, needs) {
  const S = C.S;
  const why = needs.get(f.id) || null;
  const merged = f.stage === "merged";
  const ready = f.stage === "review" && !f.conflict && f.ci === "pass";
  const kind = f.stage === "found" ? "fix" : f.stage === "prioritize" && !queuedOf(f) ? "build" : ready ? "merge" : "";
  const e = evidenceOf(f);
  const before = Array.isArray(e.repro_before) ? e.repro_before.map(Number) : [];
  const acts = [];
  const btn = (label, cls, fn, key) => h("button", { class: `cv-btn ${cls}`, type: "button", title: key ? `${label} (${key})` : label, onclick: (ev) => { ev.stopPropagation(); fn(); } }, label);
  if (f.state === "active" && !blockedOf(f)) {
    if (f.stage === "found") acts.push(btn("Reproduce", "pri", () => reproduce(C, [f.id]), "r"), btn("Dismiss", "", () => askDismiss(C, f), "d"));
    else if (kind === "build") acts.push(btn(f.door ? "Build (one-way door)" : "Build", "pri", () => build(C, [f]), "b"));
    else if (ready) acts.push(btn("Open merge pack", "good", () => C.open(f.id), "m"));
  }
  if (merged) {
    return h("div", { class: "ov-card ov-bc done", "data-id": f.id, "data-kind": "" },
      h("button", { class: "bc-main", type: "button", onclick: () => { S.cur = f.id; C.open(f.id); } }, h("span", { class: "t" }, `✓ ${f.title}`), cardLine(C, f, null)));
  }
  return h("div", { class: `ov-card ov-bc${S.cur === f.id ? " cur" : ""}${queuedOf(f) ? " q" : ""}${ready ? " ready" : ""}${blockedOf(f) ? " blk" : ""}`, "data-id": f.id, "data-kind": kind },
    h("button", { class: "bc-main", type: "button", onclick: () => { S.cur = f.id; C.open(f.id); } },
      h("span", { class: "t" }, f.title),
      h("span", { class: "ov-tags" },
        h("span", { class: `ov-tag s${f.severity}` }, SEV[f.severity] || "–"),
        f.component ? h("span", { class: "ov-tag" }, f.component) : null,
        seenOf(f) > 1 ? h("span", { class: "ov-tag seen", title: `${seenOf(f)} reports were folded into this card` }, `seen ${seenOf(f)}×`) : null),
      cardLine(C, f, why),
      cardSeg(f, !!why),
      f.stage === "reproduce" && before.length ? h("span", { class: "ov-dots" }, "repro on the base ", before.map((x) => h("i", { class: x === 0 ? "p" : "f" }))) : null),
    acts.length ? h("div", { class: "ov-acts" }, acts) : null,
    S.dismissRow === f.id ? dismissBox(C, f) : null);
}

// ------------------------------------------------------------------ screen parts
function projPicker(C, cls = "") {
  const ps = C.projects();
  const name = C.projName();
  const sel = h("select", { "aria-label": "Project", onchange: () => { location.hash = sel.value ? `#/converge?project=${enc(sel.value)}` : "#/converge"; } },
    ps.map((p) => h("option", { value: p.project, selected: p.project === name ? true : null }, p.project)),
    ps.some((p) => p.project === name) ? null : h("option", { value: name, selected: true }, name),
    h("option", { value: "" }, "All projects…"));
  return h("span", { class: `ov-proj ${cls}` }, name, h("span", { class: "car", "aria-hidden": "true" }, "▾"), sel);
}

function autoWords(C) {
  const st = C.settings();
  if (st.stopped) return "Stopped by you";
  return st.enabled ? `Autopilot on${st.tick_s ? ` · every ${Math.max(1, Math.round(st.tick_s / 60))} min` : ""}` : st.follow_picks ? "Autopilot off · your picks move" : "Autopilot off";
}

export function overviewTop(C) {
  const st = C.settings(), m = C.S.d.metrics || {};
  const pause = st.stopped ? null : h("button", { class: "cv-btn", type: "button", onclick: () => C.act(() => C.api("/api/converge/settings", { method: "PUT", body: { repo: C.repo(), enabled: !st.enabled } }), st.enabled ? "Autopilot paused: running steps finish" : "Autopilot on") }, st.enabled ? "Pause" : "Turn on");
  const stop = stopButton(C, true);
  if (stop && !st.stopped) stop.className = "cv-btn stop";
  return h("div", { class: "ov-top" },
    h("span", { class: "ov-k" }, "Converge"),
    projPicker(C),
    h("span", { class: "cv-sp" }),
    h("a", { class: "ov-auto", href: C.href("settings") }, h("span", { class: `dot${st.enabled && !st.stopped ? " on" : ""}`, "aria-hidden": "true" }), autoWords(C)),
    h("span", { class: "ov-spend" }, `${usd(m.spent_today)} of ${usd(st.budget_usd_day)} today`),
    pause, stop,
    h("button", { class: "cv-btn icon quiet cv-keyhelp", type: "button", "aria-label": "Keyboard shortcuts (?)", title: "Keyboard shortcuts (?)", onclick: () => keyHelp() }, "?"));
}

/** The metrics and timeline behind the KPIs: fetched quietly, at most once a minute. */
function fetchKpi(C) {
  const S = C.S;
  if (S.kpiBusy || (S.kpi && S.kpi.repo === C.repo() && Date.now() - S.kpi.at < 60000)) return;
  S.kpiBusy = true;
  const repo = C.repo();
  Promise.all([
    C.api(`/api/converge/metrics?repo=${enc(repo)}`, { quiet: true }).catch(() => null),
    C.api(`/api/converge/timeline?repo=${enc(repo)}&range=7d`, { quiet: true }).catch(() => null),
  ]).then(([m, tl]) => {
    S.kpi = { repo, at: Date.now(), m, tl };
    if (S.alive && C.repo() === repo && ["home", "board"].includes(C.view())) C.draw();
  }).finally(() => { S.kpiBusy = false; });
}

function kpiData(C) {
  const d = C.S.d, k = C.S.kpi && C.S.kpi.repo === C.repo() ? C.S.kpi : {};
  const m = (k.m && typeof k.m === "object" ? k.m : null) || d.metrics || {};
  const oc = m.outcomes && typeof m.outcomes === "object" ? m.outcomes : {};
  const b = k.tl && Array.isArray(k.tl.buckets) ? k.tl.buckets : [];
  const opens = b.map((x) => x.open_total).filter((x) => Number.isFinite(Number(x)));
  const open = Number.isFinite(Number(m.open)) ? Number(m.open) : C.findings().filter((f) => f.state === "active" && f.stage !== "merged").length;
  // The trend: against a day ago (the bucket that ended 24 h before the last).
  const step = k.tl && k.tl.step_ms ? Number(k.tl.step_ms) : 0;
  const back = step ? Math.round(86400000 / step) : 0;
  const dayAgo = back && opens.length > back ? Number(opens[opens.length - 1 - back]) : null;
  const eases = b.map((x) => x.ease).filter((x) => x != null && Number.isFinite(Number(x)));
  const week = Date.now() - WEEK;
  const mergedList = C.findings().filter((f) => f.stage === "merged" && (toMs(f.stage_since) || 0) >= week);
  const ps = (d.personas || []).filter((p) => p.state !== "retired" && p.ease != null);
  return {
    open, opens, delta: dayAgo == null ? null : open - dayAgo,
    merged: oc.merged_7d ?? d.merged_week ?? mergedList.length, mergedList,
    median: oc.median_found_to_merged_ms, cost: oc.cost_per_merged_fix_usd ?? m.cost_per_merged_fix,
    eb: oc.ease_before, ea: oc.ease_after, eases, hard: ps.filter((p) => Number(p.ease) <= 2).length, easy: ps.filter((p) => Number(p.ease) >= 4).length,
  };
}
const dur = (ms) => (ms == null || !Number.isFinite(Number(ms)) ? "–" : ms < 3600000 ? `${Math.max(1, Math.round(ms / 60000))} min` : ms < 86400000 ? `${(ms / 3600000).toFixed(ms < 36000000 ? 1 : 0)} h` : `${(ms / 86400000).toFixed(1)} d`);
const ez = (x) => (x == null ? "–" : Number(x).toFixed(1));

function kpis(C, needs) {
  const k = kpiData(C);
  const per = { merge: 0, build: 0, pick: 0, other: 0 };
  for (const id of needs.keys()) { const f = C.findings().find((x) => x.id === id); if (!f) continue; per[f.stage === "review" ? "merge" : f.stage === "prioritize" ? "build" : f.stage === "found" ? "pick" : "other"]++; }
  const prs = k.mergedList.map((f) => prNo(f.pr_url)).filter(Boolean);
  const card = (label, v, sub, extra = {}) => h(extra.href ? "a" : "div", { class: `ov-card ov-kpi${extra.cls ? " " + extra.cls : ""}`, href: extra.href || null, title: extra.title || null },
    h("span", { class: "ov-k" }, label), h("span", { class: "row" }, h("span", { class: "v" }, v), extra.spark || null), sub ? h("span", { class: `sub${extra.subCls ? " " + extra.subCls : ""}` }, sub) : null);
  const up = k.ea != null && k.eb != null ? Number(k.ea) - Number(k.eb) : null;
  return h("div", { class: "ov-kpis" },
    card("Open problems", String(k.open), k.delta == null ? "the last 7 days" : k.delta === 0 ? "same as yesterday" : `${k.delta < 0 ? "▼" : "▲"} ${Math.abs(k.delta)} since yesterday`, { spark: spark(k.opens, "var(--accent)"), subCls: k.delta < 0 ? "good" : k.delta > 0 ? "bad" : "", href: C.href("metrics") }),
    card("Merged this week", String(k.merged ?? 0), prs.length ? `PR ${prs.slice(0, 4).map((x) => `#${x}`).join(", ")}` : k.merged ? "merged locally" : "nothing yet", { href: C.href("board", { step: "merged" }) }),
    card("Found → merged", dur(k.median), "median, last 7 days"),
    card("Cost per fix", k.cost == null ? "–" : usd(k.cost), "personas + agents"),
    card("Persona ease", k.eb == null && k.ea == null ? "–" : `${ez(k.eb)} → ${ez(k.ea)}`, `${k.hard} hard · ${k.easy} easy`, { spark: spark(k.eases, up != null && up < 0 ? "var(--crit)" : "var(--good)"), title: "Mean ease of the fixed problems' personas before and after, 1 hard to 5 easy", href: C.href("personas") }),
    card("Needs you", String(needs.size), [per.merge ? `${per.merge} merge` : null, per.build ? `${per.build} build${per.build > 1 ? "s" : ""}` : null, per.pick ? `${per.pick} pick${per.pick > 1 ? "s" : ""}` : null, per.other ? `${per.other} other` : null].filter(Boolean).join(" · ") || "nothing", { cls: needs.size ? "you" : "", href: C.href("inbox") }));
}

/** Each step's count and its live sub-line. */
function flowData(C, needs) {
  const d = C.S.d, fs = C.findings().filter((f) => f.state === "active");
  const runs = (d.runs || []).filter((r) => r.state === "running" && r.persona && r.purpose !== "generate");
  const at = (st) => fs.filter((f) => st.includes(f.stage));
  return FLOW.map((s) => {
    const items = s.stages ? at(s.stages) : [];
    const wait = items.filter((f) => needs.has(f.id)).length;
    const live = items.filter((f) => agentOn(f) || f.busy);
    let n = items.length, sub = "", on = false;
    if (s.k === "play") {
      n = (d.personas || []).filter((p) => p.state !== "retired").length;
      const r = runs[0];
      if (r) { const p = (d.personas || []).find((x) => x.id === r.persona); on = true; sub = `${firstName(p ? p.name : "A persona")} playing${r.at && r.at.of ? ` · step ${r.at.n}/${r.at.of}` : ""}${runs.length > 1 ? ` +${runs.length - 1}` : ""}`; }
      else sub = C.settings().enabled ? "next when one is due" : "idle";
    } else if (s.k === "merged") {
      n = Number(d.merged_week ?? 0) || C.findings().filter((f) => f.stage === "merged" && (toMs(f.stage_since) || 0) >= Date.now() - WEEK).length;
      const bad = C.findings().filter((f) => f.stage === "merged" && f.guard_ok === false).length;
      sub = bad ? `${bad} regressed` : n ? "guard passing on the base" : "this week";
    } else if (live.length) {
      const f = live[0];
      on = true;
      sub = agentOn(f) ? `${KIND_NAME[agentKind(f)]}${agentKind(f) === "fix" ? ` · ${phase(C, "fix", f.agent, doingWords(f.agent_doing)) + 1}/5` : ` · ${doingWords(f.agent_doing) || "working"}`}` : String(f.busy);
    } else if (s.k === "prioritize" && items.some((f) => queuedOf(f))) {
      const q = items.filter((f) => queuedOf(f));
      sub = `${q.length} queued${q.some((f) => f.waits_for) ? ": same files" : ""}`;
    } else if (s.you && wait) sub = s.k === "found" ? `worth fixing? ${wait} new` : s.k === "review" ? (items.some((f) => f.ci === "pass") ? "CI green · mergeable" : "waits for you") : `${wait} wait for you`;
    else if (wait) sub = `${wait} need${wait === 1 ? "s" : ""} you`;
    else if (s.k === "scope") sub = n ? "planning" : "automatic";
    else if (s.k === "review" && n) sub = items.some((f) => f.conflict) ? "a conflict" : "CI running";
    else if (n && items.some((f) => blockedOf(f))) sub = "blocked on you";
    else sub = n ? "waiting" : "–";
    return { ...s, n, sub, on, wait };
  });
}

function flowStrip(C, needs, fl) {
  const steps = flowData(C, needs);
  const cells = [];
  steps.forEach((s, i) => {
    if (i) cells.push(h("span", { class: "ov-arr", "aria-hidden": "true" }, "→"));
    const pressed = fl.step === s.k;
    const cls = `ov-step${s.you ? " you" : ""}${s.you && s.n ? " has" : ""}${s.k === "merged" ? " done" : ""}${s.on && !s.you ? " live" : ""}`;
    const label = `${i + 1} · ${s.n}${s.you ? " ✋" : ""}`;
    const kids = [h("span", { class: "l" }, `${i + 1} · ${s.n === undefined ? "" : ""}${FLOW[i].n}${s.you ? " ✋" : ""}`), h("span", { class: "n" }, String(s.n)), h("span", { class: `s${s.on ? " on" : ""}` }, s.on ? h("i", { class: "cv-pulse", "aria-hidden": "true" }) : null, s.sub)];
    cells.push(s.k === "play"
      ? h("a", { class: cls, href: C.href("personas"), "aria-label": `${FLOW[i].n}: ${s.n}. ${s.sub}` }, kids)
      : h("button", { class: cls, type: "button", "aria-pressed": String(pressed), "aria-label": `${FLOW[i].n}: ${s.n}${s.you ? ", your pick" : ""}. ${s.sub}. Filter the board`, title: label, onclick: () => setFilter(C, "step", pressed ? "" : s.k) }, kids));
  });
  return h("section", { class: "ov-card ov-flow", "aria-label": "The flow" },
    h("div", { class: "hd" }, h("span", { class: "ov-k" }, "The flow · click a step to filter the board"), h("span", { class: "key" }, h("i", { "aria-hidden": "true" }), "✋ = your pick")),
    h("div", { class: "ov-steps" }, cells));
}

function filterBar(C, fl) {
  const chip = (label, on, n, onclick, dot) => h("button", { class: `ov-chip${on ? " on" : ""}`, type: "button", "aria-pressed": String(on), onclick }, dot != null ? h("span", { class: "cv-sev", "data-sev": String(dot) }) : null, label, n != null ? h("span", { class: "n" }, String(n)) : null);
  const base = (skip) => boardCards(C, fl, skip).filter((f) => f.stage !== "merged");
  const byComp = new Map(), bySev = new Map(), byWho = new Map();
  for (const f of base("comp")) byComp.set(f.component || "other", (byComp.get(f.component || "other") || 0) + 1);
  for (const f of base("sev")) bySev.set(String(f.severity), (bySev.get(String(f.severity)) || 0) + 1);
  for (const f of base("who")) for (const p of new Set([f.persona, ...(f.personas_hit || [])].filter(Boolean))) byWho.set(p, (byWho.get(p) || 0) + 1);
  const all = base("comp").length;
  const comps = [...byComp.entries()].sort((a, b) => b[1] - a[1]);
  const who = (C.S.d.personas || []).filter((p) => byWho.has(p.id) || fl.who === p.id);
  const step = FLOW.find((s) => s.k === fl.step);
  return h("div", { class: "ov-filters", role: "group", "aria-label": "Filter the board" },
    h("span", { class: "ov-k" }, "Component"),
    chip(`All`, !fl.comp, all, () => setFilter(C, "comp", "")),
    comps.map(([c, n]) => chip(c, fl.comp === c, n, () => setFilter(C, "comp", fl.comp === c ? "" : c))),
    h("span", { class: "ov-k" }, "Severity"),
    [4, 3, 2, 1].map((s) => chip(SEV[s], fl.sev === String(s), bySev.get(String(s)) || 0, () => setFilter(C, "sev", fl.sev === String(s) ? "" : String(s)), s)),
    who.length ? h("span", { class: "ov-k" }, "Persona") : null,
    who.map((p) => chip(firstName(p.name), fl.who === p.id, byWho.get(p.id) || 0, () => setFilter(C, "who", fl.who === p.id ? "" : p.id))),
    step ? chip(`Step: ${step.n} ×`, true, null, () => setFilter(C, "step", "")) : null);
}

/** Sort: what waits for you, then severity, then the newest. */
const order = (needs) => (a, b) => (needs.has(b.id) ? 1 : 0) - (needs.has(a.id) ? 1 : 0) || (b.pinned ? 1 : 0) - (a.pinned ? 1 : 0) || b.severity - a.severity || seenOf(b) - seenOf(a) || (toMs(b.stage_since) || 0) - (toMs(a.stage_since) || 0);

export function board(C, needs, fl) {
  const S = C.S;
  const step = FLOW.find((s) => s.k === fl.step);
  const cards = boardCards(C, fl).filter((f) => !step || !step.stages || step.stages.includes(f.stage));
  const N = { found: 3, repro: 4, build: 4, merge: 4 };
  return h("div", { class: "ov-board" }, COLS.map((col) => {
    const items = cards.filter((f) => col.stages.includes(f.stage));
    const active = items.filter((f) => f.stage !== "merged").sort(order(needs));
    const merged = items.filter((f) => f.stage === "merged").sort((a, b) => (toMs(b.stage_since) || 0) - (toMs(a.stage_since) || 0));
    const list = [...active, ...merged];
    const you = active.filter((f) => needs.has(f.id)).length;
    const open = (S.colAll && S.colAll[col.k]) || !!step;
    const shown = open ? list : list.slice(0, N[col.k]);
    const head = h("div", { class: "ov-colh" }, h("b", {}, col.n), you ? h("span", { class: "you" }, `✋ ${you} ${col.k === "merge" || col.k === "build" ? "ready" : "wait for you"}`) : h("span", {}, String(active.length)));
    return h("section", { class: "ov-col", "aria-label": col.n }, head,
      shown.length ? shown.map((f) => boardCard(C, f, needs)) : h("p", { class: "ov-colempty" }, step && !step.stages?.some((x) => col.stages.includes(x)) ? "Not in this step" : "Nothing here"),
      list.length > shown.length ? h("button", { class: "ov-more", type: "button", onclick: () => { S.colAll = { ...(S.colAll || {}), [col.k]: true }; C.draw(); } }, `+ ${list.length - shown.length} more · `, h("b", {}, "show")) : null,
      open && !step && list.length > N[col.k] ? h("button", { class: "ov-more", type: "button", onclick: () => { S.colAll = { ...(S.colAll || {}), [col.k]: false }; C.draw(); } }, h("b", {}, "Show fewer")) : null);
  }));
}

// ------------------------------------------------------------------ the right rail
/** An agent's last steps (from its events) or a run's, fetched while unfolded. */
function recentSteps(C, it) {
  const S = C.S;
  const M = (S.recent = S.recent || new Map());
  const c = M.get(it.key);
  if (!c || (!c.busy && Date.now() - c.at > 4000)) {
    M.set(it.key, { ...(c || {}), busy: true, at: Date.now() });
    const done = (lines) => { M.set(it.key, { lines, at: Date.now() }); if (S.alive && (S.liveOpen || new Set()).has(it.key)) paintRail(C); };
    if (it.run) C.api(`/api/converge/runs/${enc(it.run)}`, { quiet: true }).then((x) => {
      const st = (x && x.run && Array.isArray(x.run.steps) ? x.run.steps : []).filter((s) => s.kind === "step" || s.kind === "say" || s.kind === "tool");
      done(st.slice(-6).map((s) => ({ t: s.kind === "tool" ? doingWords(`${s.tool}: ${s.text || ""}`) || s.text : firstSentences(s.text, 1), step: s.kind === "step" })));
    }, () => done([]));
    else if (it.agent) C.api(`/api/agents/${enc(it.agent)}/events?limit=1000`, { quiet: true }).then((x) => {
      const ev = (x && Array.isArray(x.events) ? x.events : []).filter((e) => e.kind === "PreToolUse");
      const words = [];
      for (const e of ev) { const w = doingWords(e.summary); if (w && words.at(-1) !== w) words.push(w); }
      done(words.slice(-5).map((t) => ({ t })));
    }, () => done([]));
    else done([]);
  }
  return (c && c.lines) || null;
}

function liveCard(C) {
  const S = C.S;
  const items = liveItems(C);
  S.liveOpen = S.liveOpen || new Set();
  if (items.length && !S.liveTouched && !S.liveOpen.size) S.liveOpen.add(items[0].key);
  const now = Date.now();
  return h("section", { class: "ov-card", "aria-label": "Live now" },
    h("div", { class: "hd" }, h("span", { class: "ov-k" }, "Live now"), h("span", {}, items.length ? `${items.length} working` : "")),
    items.length ? items.map((it) => {
      const open = S.liveOpen.has(it.key);
      const lines = open ? recentSteps(C, it) : null;
      const toggle = () => { S.liveTouched = true; if (open) S.liveOpen.delete(it.key); else S.liveOpen.add(it.key); paintRail(C); };
      const talkHref = it.run ? C.here({ run: it.run }) : it.fid ? C.here({ f: it.fid }) : null;
      return h("div", { class: "ov-task", "data-key": it.key },
        h("button", { class: "th", type: "button", "aria-expanded": String(open), onclick: toggle }, h("b", {}, `${open ? "▾" : "▸"} ${it.title}`), h("span", {}, it.since ? secsWords(now - it.since) : "")),
        seg(Math.max(1, it.of), (i) => (i < it.n ? "d" : i === it.n ? "n pulse" : "")),
        open && it.phases ? h("div", { class: "steps" }, it.phases.map((p, i) => h("span", { class: i < it.n ? "" : i === it.n ? "on" : "todo" }, `${i < it.n ? "✓" : i === it.n ? "●" : "○"} ${p}${i === it.n ? `: ${it.now.replace(/^Step \d+ of \d+: /, "")}` : ""}`))) : h("span", { class: `now${open ? "" : " on"}` }, it.now),
        open ? (lines == null ? h("span", { class: "now" }, "Reading its steps…") : lines.length ? h("div", { class: "steps" }, h("span", { class: "todo" }, "Recent steps"), lines.map((l) => h("span", {}, `${l.step ? "▸ " : "· "}${short(l.t, 90)}`))) : null) : null,
        open ? h("div", { class: "links" },
          it.agent ? h("a", { href: `#/a/${enc(it.agent)}/terminal` }, "Terminal") : null,
          talkHref ? h("a", { href: talkHref }, it.run ? "Watch · talk to it" : "Talk to it") : null) : null);
    }) : h("p", { class: "ov-quiet" }, C.settings().enabled ? "Nothing running right now: the autopilot starts what is due." : "Nothing running. The autopilot is off: your picks and Play start work."));
}

const FEED_SKIP = new Set(["tick", "decide"]);
function fetchFeed(C) {
  const S = C.S;
  if (S.feedBusy || (S.feed && S.feed.repo === C.repo() && Date.now() - S.feed.at < 7000)) return;
  S.feedBusy = true;
  const repo = C.repo();
  C.api(`/api/converge/activity?repo=${enc(repo)}&limit=60`, { quiet: true }).then((x) => {
    const items = (x && Array.isArray(x.items) ? x.items : []).filter((i) => i && !FEED_SKIP.has(i.kind));
    const sig = JSON.stringify(items.slice(0, 12).map((i) => i.id));
    const changed = !S.feed || S.feed.sig !== sig || S.feed.repo !== repo;
    S.feed = { repo, at: Date.now(), items, sig };
    if (changed && S.alive && C.repo() === repo) paintRail(C);
  }, () => { S.feed = { repo, at: Date.now(), items: (S.feed && S.feed.items) || [], missing: true }; }).finally(() => { S.feedBusy = false; });
}

function feedCard(C, n = 10) {
  const S = C.S;
  const items = S.feed && S.feed.repo === C.repo() ? S.feed.items : null;
  const line = (x) => {
    const fid = typeof x.finding === "object" ? x.finding?.id : x.finding;
    const you = x.actor === "you" || x.kind === "you";
    let s = String(x.summary || "");
    if (you && /^You\b/.test(s)) s = s.replace(/^You\b\s*/, "");
    // "“<card>”: what happened" reads as what happened, then the card.
    const m = /^“([^”]+)”:\s*(.+)$/.exec(s);
    let card = null;
    if (m) { s = m[2]; card = m[1]; }
    if (you && s) s = s[0].toLowerCase() + s.slice(1);
    s = s.replace(/https:\/\/github\.com\/[^\s/]+\/[^\s/]+\/pull\/(\d+)/g, "PR #$1");
    const body = [you ? h("b", {}, "You ") : null, s, card ? h("i", {}, ` · ${short(card, 44)}`) : null];
    return h("li", {}, h("span", { class: "tm" }, clock(x.ts)), fid ? h("a", { class: "tx", href: C.here({ f: fid }) }, body) : h("span", { class: "tx" }, body));
  };
  return h("section", { class: "ov-card", "aria-label": "What happened" },
    h("div", { class: "hd" }, h("span", { class: "ov-k" }, "What happened · newest first"), h("a", { href: C.href("activity") }, "Full activity")),
    items == null ? h("p", { class: "ov-quiet" }, "Reading the activity…") : items.length ? h("ol", { class: "ov-feed" }, items.slice(0, n).map(line)) : h("p", { class: "ov-quiet" }, "Nothing has happened yet."));
}

/** The rail repaints on its own (a fold, a fetch) without the rest of the page. */
function paintRail(C) {
  const el = document.querySelector(".cv .ov-rail");
  if (!el) return;
  put(el, liveCard(C), feedCard(C));
}

// ------------------------------------------------------------------ screens
export const isPhone = () => window.matchMedia("(max-width: 640px)").matches;

export function overviewScreen(C) {
  useStyle("converge-overview", CSS);
  fetchKpi(C); fetchFeed(C);
  const d = C.S.d;
  if (!(d.findings || []).length && !(d.personas || []).length) {
    return h("div", {}, overviewTop(C),
      h("div", { class: "cv-none" },
        h("p", {}, "Nothing here yet. Personas play the product and what they hit shows up as problems. Start by writing personas from the code, or load ChloeDB's six evaluation personas and what they reported."),
        h("a", { class: "cv-btn pri", href: C.href("personas") }, "Set up personas"),
        h("button", { class: "cv-btn", type: "button", onclick: () => C.act(() => C.api("/api/converge/seed", { method: "POST", body: { repo: C.repo() } }), "Loaded the seed personas and their problems") }, "Load the seed personas")));
  }
  const needs = needsOf(C);
  if (isPhone()) return phoneOverview(C, needs);
  const fl = filters();
  return h("div", { class: "cvo" },
    overviewTop(C),
    kpis(C, needs),
    flowStrip(C, needs, fl),
    filterBar(C, fl),
    h("div", { class: "ov-main" },
      board(C, needs, fl),
      h("aside", { class: "ov-rail", "aria-label": "Live now and what happened" }, liveCard(C), feedCard(C))));
}

/** #/converge/board: the filters and the board (a phone's Board tab). */
export function boardScreen(C) {
  useStyle("converge-overview", CSS);
  const needs = needsOf(C);
  const fl = filters();
  return h("div", { class: "cvo" },
    isPhone() ? h("div", { class: "ov-ph-h", style: { marginBottom: "8px" } }, projPicker(C), h("span", { class: "ov-auto" }, `${needs.size} need you`)) : overviewTop(C),
    isPhone() ? phoneFlow(C, needs, fl) : flowStrip(C, needs, fl),
    filterBar(C, fl),
    board(C, needs, fl));
}

function phoneFlow(C, needs, fl) {
  const steps = flowData(C, needs).filter((s) => s.k !== "scope");
  return h("nav", { class: "ov-ph-f", "aria-label": "The flow" }, steps.map((s) =>
    h("a", { class: `${s.you ? "you" : ""}${s.you && s.n ? " has" : ""}${s.k === "merged" ? " done" : ""}${s.on ? " live" : ""}`, href: s.k === "play" ? C.href("personas") : C.href("board", { step: fl.step === s.k ? "" : s.k }), "aria-label": `${FLOW.find((x) => x.k === s.k).n}: ${s.n}. ${s.sub}`, "aria-current": fl.step === s.k ? "true" : null },
      h("b", {}, String(s.n)), h("span", {}, s.short, s.you ? h("small", { "aria-hidden": "true", style: { fontSize: "9px" } }, "✋") : null))));
}

function phoneOverview(C, needs) {
  const k = kpiData(C);
  const st = C.settings();
  const live = liveItems(C);
  const fs = C.findings();
  const mergeReady = fs.filter((f) => needs.has(f.id) && f.stage === "review").sort(order(needs));
  const builds = fs.filter((f) => f.state === "active" && f.stage === "prioritize");
  const queued = builds.filter((f) => queuedOf(f));
  const picks = fs.filter((f) => needs.has(f.id) && f.stage === "found");
  const blocked = fs.filter((f) => blockedOf(f));
  const other = [...needs.keys()].length - mergeReady.length - (builds.length - queued.length) - picks.length - blocked.length;
  const row = (title, sub, href) => h("a", { class: "ov-card ov-ph-row", href }, h("span", {}, h("b", {}, title), sub ? h("small", {}, sub) : null), h("span", { class: "car", "aria-hidden": "true" }, "›"));
  const top = mergeReady[0];
  return h("div", { class: "ov-ph" },
    h("div", { class: "ov-ph-h" }, projPicker(C), h("a", { class: "ov-auto", href: C.href("settings") }, h("span", { class: `dot${st.enabled && !st.stopped ? " on" : ""}`, "aria-hidden": "true" }), `${st.stopped ? "stopped" : st.enabled ? "autopilot" : "autopilot off"}${live.length ? ` · ${live.length} working` : ""}`)),
    h("div", { class: "ov-ph-k" },
      h("a", { href: C.href("metrics") }, h("b", {}, String(k.open)), h("span", {}, "open")),
      h("a", { href: C.href("board", { step: "merged" }) }, h("b", {}, String(k.merged ?? 0)), h("span", {}, "merged/wk")),
      h("div", {}, h("b", {}, k.median == null ? "–" : dur(k.median).replace(" min", "m").replace(" h", "h").replace(" d", "d")), h("span", {}, "to merge")),
      h("div", {}, h("b", {}, k.cost == null ? "–" : usd(k.cost)), h("span", {}, "per fix"))),
    h("div", {}, h("div", { class: "ov-k", style: { marginBottom: "6px" } }, "The flow"), phoneFlow(C, needs, filters())),
    h("section", { "aria-label": "Needs you" },
      h("div", { class: "ov-k you", style: { marginBottom: "6px" } }, `Needs you · ${needs.size}`),
      h("div", { style: { display: "flex", flexDirection: "column", gap: "8px" } },
        top ? h("div", { class: "ov-card", style: { borderColor: "color-mix(in srgb, var(--good) 45%, var(--line))" } },
          h("b", { style: { fontSize: "var(--fs-sm)" } }, top.title),
          h("span", { class: "ov-line good" }, `Ready to merge · ${mergeLine(top)}`),
          h("a", { class: "big go", href: C.here({ f: top.id }) }, "Open merge pack")) : null,
        mergeReady.length > 1 ? row(`${mergeReady.length - 1} more ready to merge`, null, C.href("board", { step: "review" })) : null,
        blocked.length ? row(`${blocked.length} blocked on you`, blockStep(blocked[0], blockedOf(blocked[0])), C.here({ f: blocked[0].id })) : null,
        builds.length ? row(`${builds.length - queued.length} ready to build`, queued.length ? `${queued.length} queued behind the same files` : "Planned and scored: Build starts a fix agent", C.href("board", { step: "prioritize" })) : null,
        picks.length ? row(`${picks.length} new problem${picks.length > 1 ? "s" : ""}`, "Worth fixing? Reproduce or dismiss", C.href("board", { step: "found" })) : null,
        other > 0 ? row(`${other} more waiting`, null, C.href("inbox")) : null,
        !needs.size ? h("p", { class: "ov-quiet" }, "Nothing needs you right now.") : null)),
    h("section", { "aria-label": "Live now" },
      h("div", { class: "ov-k", style: { marginBottom: "6px" } }, "Live now"),
      live.length ? h("div", { style: { display: "flex", flexDirection: "column", gap: "8px" } }, live.slice(0, 4).map((it) => h("a", { class: "ov-card", href: it.run ? C.here({ run: it.run }) : C.here({ f: it.fid }), style: { textDecoration: "none", color: "var(--text)" } },
        h("span", { style: { display: "flex", justifyContent: "space-between", fontSize: "var(--fs-sm)", gap: "8px" } }, h("b", { style: { overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" } }, it.title), h("span", { style: { color: "var(--text-2)", flex: "none" } }, it.since ? secsWords(Date.now() - it.since) : "")),
        seg(Math.max(1, it.of), (i) => (i < it.n ? "d" : i === it.n ? "n pulse" : "")),
        h("span", { class: "ov-line on" }, it.now)))) : h("p", { class: "ov-quiet" }, "Nothing running right now.")));
}

/** The 8 s poll: the rail's activity and the KPIs (the page redraws only when the overview changed). */
export function overviewTick(C) { fetchFeed(C); fetchKpi(C); }

// ------------------------------------------------------------------ keys
/** The board's keys; returns whether it used the key. */
export function boardKey(C, e, host) {
  if (e.metaKey || e.ctrlKey || e.altKey) return false;
  const t = e.target;
  if (t && t.closest && t.closest("input, textarea, select, [contenteditable]")) return false;
  if (document.querySelector(".cv-sheet, .cv-menu, .cv-pop:not(.cv-help)")) return false;
  if (e.key === "?") { e.preventDefault(); keyHelp(); return true; }
  const rows = [...host.querySelectorAll(".ov-bc")];
  if (!rows.length) return false;
  const S = C.S;
  const i = rows.findIndex((r) => r.dataset.id === S.cur);
  const find = (id) => C.findings().find((f) => f.id === id);
  const mark = (j) => {
    rows.forEach((r) => r.classList.remove("cur"));
    const r = rows[Math.max(0, Math.min(rows.length - 1, j))];
    S.cur = r.dataset.id; r.classList.add("cur");
    r.querySelector(".bc-main")?.focus({ preventScroll: true });
    r.scrollIntoView({ block: "nearest" });
  };
  const k = e.key;
  if (k === "j" || (k === "ArrowDown" && i >= 0)) { e.preventDefault(); mark(i < 0 ? 0 : i + 1); return true; }
  if (k === "k" || (k === "ArrowUp" && i >= 0)) { e.preventDefault(); mark(i < 0 ? 0 : i - 1); return true; }
  const row = i >= 0 ? rows[i] : null;
  const f = row ? find(row.dataset.id) : null;
  const kind = row ? row.dataset.kind : null;
  if (k === "Enter" && f && !(t && t.closest && t.closest("button, a"))) { e.preventDefault(); C.open(f.id); return true; }
  if (k === "m") {
    const target = kind === "merge" ? f : find((rows.find((r) => r.dataset.kind === "merge") || {}).dataset?.id);
    if (!target) { toast("Nothing waits to be merged"); return true; }
    e.preventDefault(); S.cur = target.id; C.open(target.id); return true;
  }
  if (!f) { if ("rbd".includes(k)) toast("Pick a card first: j or k"); return "rbd".includes(k); }
  if (k === "r") { e.preventDefault(); if (kind === "fix") reproduce(C, [f.id]); else toast("r reproduces a card in Found"); return true; }
  if (k === "b") { e.preventDefault(); if (kind === "build") build(C, [f]); else toast("b builds a card that is ready to build"); return true; }
  if (k === "d") { e.preventDefault(); if (kind === "fix" || kind === "build") askDismiss(C, f); else toast("d dismisses a card in Found or ready to build"); return true; }
  return false;
}

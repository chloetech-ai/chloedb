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
a.cv-btn { text-decoration: none; }
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
.cv-live { display: inline-flex; align-items: center; gap: 5px; }
.cv-live::before { content: ""; width: 7px; height: 7px; border-radius: 50%; background: currentColor; animation: cv-pulse 1.4s ease-in-out infinite; }
.cv-live.still::before { animation: none; opacity: .6; }
@keyframes cv-pulse { 0%, 100% { opacity: 1; } 50% { opacity: .25; } }
@media (prefers-reduced-motion: reduce) { .cv-live::before { animation: none; } }
.cv-card.working { box-shadow: 0 0 0 2px var(--accent-line); }
.cv-now { display: flex; gap: 8px; flex-wrap: wrap; align-items: center; margin-bottom: var(--s-3); font-size: var(--fs-sm); color: var(--text-2); }
.cv-now button { font: inherit; border: var(--hair) solid var(--accent-line); background: var(--accent-soft); color: var(--accent-text); border-radius: var(--r-pill); padding: 4px 10px; cursor: pointer; min-height: 32px; }
.cv-prog { display: flex; flex-direction: column; gap: 4px; width: 100%; }
.cv-prog .bar { display: grid; grid-template-columns: repeat(8, 1fr); gap: 2px; }
.cv-prog .bar i { height: 6px; border-radius: 3px; background: var(--surface-3); position: relative; overflow: hidden; }
.cv-prog .bar i.p { background: var(--good); }
.cv-prog .bar i.n { background: var(--accent-soft); }
.cv-prog .bar i.n::after { content: ""; position: absolute; inset: 0; width: var(--in, 35%); background: var(--accent); border-radius: 3px; }
.cv-prog .bar i.stop { background: var(--warn); }
.cv-prog .lbl { display: flex; justify-content: space-between; gap: 8px; font-size: var(--fs-xs); line-height: var(--lh-xs); color: var(--text-2); }
.cv-prog .lbl b { font-weight: var(--w-mid); color: var(--text); }
.cv-empty { color: var(--muted); font-size: var(--fs-sm); padding: 4px; }
.cv-quick { align-self: flex-start; min-height: 36px; }
.cv-pill.live { text-decoration: none; color: var(--accent-text); border-color: var(--accent-line); background: var(--accent-soft); }
.cv-work { display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: 10px; }
.cv-wrow { background: var(--surface); border: var(--hair) solid var(--line); border-radius: 8px; padding: 12px; display: flex; flex-direction: column; gap: 6px; cursor: pointer; }
.cv-wrow.you { border-color: var(--warn); }
.cv-wrow .k { display: flex; justify-content: space-between; align-items: center; font-size: var(--fs-xs); color: var(--text-2); }
.cv-wrow .ttl { font-weight: var(--w-mid); }
.cv-wrow .doing { font-family: var(--mono); font-size: var(--fs-sm); color: var(--accent-text); }
.cv-wrow .cv-btn { align-self: flex-start; }
.cv-pc.live { cursor: pointer; box-shadow: 0 0 0 2px var(--accent-line); }
.cv-iv { display: flex; flex-direction: column; gap: 10px; margin-top: 8px; }
.cv-iv-msg { display: flex; gap: 8px; align-items: flex-end; max-width: 92%; }
.cv-iv-msg .av { flex: none; width: 28px; height: 28px; border-radius: 50%; display: grid; place-items: center; background: var(--accent); color: var(--surface); font-weight: var(--w-title); font-size: var(--fs-sm); }
.cv-iv-msg .bub { background: var(--surface-2); border-radius: 14px 14px 14px 4px; padding: 8px 12px; line-height: var(--lh-sm); white-space: pre-wrap; overflow-wrap: anywhere; }
.cv-iv-msg .bub .ts { display: block; margin-top: 4px; font-size: var(--fs-xs); color: var(--muted); }
.cv-iv-msg.think .bub { background: none; border: var(--hair) dashed var(--line-2); color: var(--text-2); font-style: italic; }
.cv-iv-msg .bub .lbl { display: block; font-style: normal; font-size: var(--fs-xs); text-transform: uppercase; letter-spacing: .05em; color: var(--muted); margin-bottom: 2px; }
.cv-iv-acts { margin-left: 36px; display: flex; flex-direction: column; gap: 2px; border-left: 2px solid var(--line); padding-left: 8px; }
.cv-iv-act { font-size: var(--fs-xs); color: var(--text-2); overflow-wrap: anywhere; }
.cv-iv-act .w { text-transform: uppercase; letter-spacing: .04em; }
.cv-iv-act code { font-family: var(--mono); }
.cv-iv-step { display: flex; align-items: center; gap: 8px; font-size: var(--fs-xs); color: var(--accent-text); text-transform: uppercase; letter-spacing: .05em; margin: 6px 0; }
.cv-iv-step::before, .cv-iv-step::after { content: ""; flex: 1; border-top: var(--hair) solid var(--accent-line); }
.cv-iv-typing { display: flex; gap: 4px; margin: 10px 0 0 36px; }
.cv-iv-typing[hidden] { display: none; }
.cv-iv-typing i { width: 7px; height: 7px; border-radius: 50%; background: var(--muted); animation: cv-pulse 1.2s infinite; }
.cv-iv-typing i:nth-child(2) { animation-delay: .2s; } .cv-iv-typing i:nth-child(3) { animation-delay: .4s; }
.cv-dragme { touch-action: pan-x pan-y; -webkit-user-select: none; user-select: none; -webkit-touch-callout: none; }
.cv-ghost { position: fixed; z-index: 60; pointer-events: none; transform: rotate(2deg) scale(1.03); box-shadow: var(--shadow-3); opacity: .95; }
.cv-lifted { opacity: .35; }
.cv-col.cv-can { outline: 2px dashed var(--accent-line); outline-offset: -2px; }
.cv-col.cv-cannot { opacity: .45; }
.cv-col.cv-over { outline: 2px solid var(--accent); background: var(--accent-soft); }
.cv-btn.live { width: 100%; justify-content: center; border-color: var(--accent-line); background: var(--accent-soft); color: var(--accent-text); }
.cv-steps { list-style: none; margin: 8px 0 0; padding: 0; display: flex; flex-direction: column; gap: 6px; font-size: var(--fs-sm); }
.cv-steps li { display: flex; gap: 6px; align-items: baseline; flex-wrap: wrap; padding: 6px 8px; border-radius: 6px; background: var(--surface-2); }
.cv-steps li.s { background: none; padding-left: 0; color: var(--text); }
.cv-steps li .w { color: var(--text-2); font-size: var(--fs-xs); text-transform: uppercase; letter-spacing: .04em; }
.cv-steps li code { font-family: var(--mono); font-size: var(--fs-xs); word-break: break-all; }
.cv-steps li .ts { margin-left: auto; color: var(--text-3, var(--muted)); font-size: var(--fs-xs); }
.cv-panel.wide { grid-column: 1 / -1; }
.cv-tl-svg { width: 100%; height: auto; display: block; margin-top: 8px; }
.cv-legend { display: flex; flex-wrap: wrap; gap: 12px; font-size: var(--fs-xs); color: var(--text-2); margin-top: 4px; }
.cv-legend span { display: inline-flex; align-items: center; gap: 5px; }
.cv-legend i { width: 10px; height: 10px; border-radius: 2px; display: inline-block; }
.cv-legend i.ln { width: 16px; height: 0; border-top: 2px solid; border-radius: 0; }
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
/* One severity palette for cards, chips and charts; run kinds for the spend chart. */
.cv, .cv-sheet, .cv-toast { --cv-s4: var(--crit); --cv-s3: var(--warn); --cv-s2: var(--accent); --cv-s1: var(--muted); --cv-k2: var(--good); --cv-k3: var(--warn); --cv-k4: var(--text-2); }
.cv-card[data-sev="4"] { --sev: var(--cv-s4); } .cv-card[data-sev="3"] { --sev: var(--cv-s3); } .cv-card[data-sev="2"] { --sev: var(--cv-s2); } .cv-card[data-sev="1"] { --sev: var(--cv-s1); }
.cv-chip.comp { background: var(--surface-2); border: var(--hair) solid var(--line-2); }
.cv-pill.you { text-decoration: none; color: var(--warn-ink); border-color: var(--warn); background: var(--warn-soft); }
.cv-flash { animation: cv-flash 1.2s ease-in-out 3; box-shadow: 0 0 0 3px var(--accent); }
@keyframes cv-flash { 50% { box-shadow: 0 0 0 6px var(--accent-line); } }
/* Projects */
.cv-pjs { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 12px; }
.cv-pj { display: flex; flex-direction: column; gap: 6px; padding: 16px; min-height: 96px; border-radius: var(--r); border: var(--hair) solid var(--line); background: var(--surface); color: var(--text); text-decoration: none; min-width: 0; }
.cv-pj:hover { box-shadow: var(--shadow-2); border-color: var(--accent-line); }
.cv-pj .n { font-size: var(--fs-md); line-height: var(--lh-md); font-weight: var(--w-title); }
.cv-pj .p { font-family: var(--mono); font-size: var(--fs-xs); color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cv-pj.add { border-style: dashed; justify-content: center; }
.cv-ptitle { position: relative; display: flex; flex-direction: column; gap: 0; min-width: 0; max-width: 100%; }
.cv-ptitle .cv-brand { font-family: var(--mono); font-size: var(--fs-xs); line-height: var(--lh-xs); letter-spacing: var(--track-label); text-transform: uppercase; color: var(--muted); font-weight: var(--w-text); }
.cv-ptitle .cv-pname { display: inline-flex; align-items: center; gap: 6px; min-height: 32px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cv-ptitle .car { font-size: var(--fs-sm); color: var(--muted); }
.cv-psel { position: absolute; inset: 0; width: 100%; height: 100%; min-height: 44px; opacity: 0; cursor: pointer; font-size: 16px; }
.cv-ptitle:focus-within .cv-pname { outline: 2px solid var(--accent); outline-offset: 2px; border-radius: 4px; }
/* Ask */
.cv-ask { background: var(--surface); border: var(--hair) solid var(--line); border-radius: var(--r); padding: 10px; margin-bottom: var(--s-3); display: flex; flex-direction: column; gap: 8px; }
.cv-ask-row { display: flex; gap: 8px; align-items: stretch; }
.cv-ask-t { flex: 1; min-width: 0; min-height: 44px; resize: vertical; font: inherit; font-size: 16px; line-height: 1.35; padding: 10px 12px; border: var(--hair) solid var(--line-2); border-radius: var(--r-sm); background: var(--surface); color: var(--text); }
.cv-ask-b { min-height: 44px; height: auto; padding: 0 18px; }
.cv-ask-out:empty { display: none; }
.cv-pending { cursor: default; border-left-color: var(--accent); border-style: dashed; }
.cv-asked { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; font-size: var(--fs-sm); background: var(--good-soft); color: var(--good-ink); border-radius: 8px; padding: 6px 8px; }
.cv-asked .ok { font-weight: var(--w-title); }
.cv-asked .t { flex: 1; min-width: 0; }
.cv-btn.ghost { border-color: transparent; background: none; width: 44px; justify-content: center; }
.cv-hand > summary { cursor: pointer; color: var(--accent-text); font-size: var(--fs-sm); min-height: 36px; display: flex; align-items: center; }
.cv-hand .cv-f input[type=text] { max-width: none; }
.cv-f2 { display: flex; gap: 10px; flex-wrap: wrap; }
.cv-f2 .cv-f { flex: 1 1 120px; }
.cv-f select, .cv-gen .cv-num { height: var(--ctl-h); min-height: 40px; border: var(--hair) solid var(--line-2); border-radius: var(--r-sm); background: var(--surface); color: var(--text); padding: 0 8px; font: inherit; }
.cv-chip.mem { background: var(--accent-soft); color: var(--accent-text); }
.cv-mem { display: flex; flex-direction: column; gap: 2px; }
.cv-mem label { position: relative; display: inline-flex; align-items: center; gap: 10px; min-height: 44px; cursor: pointer; font-weight: var(--w-mid); }
.cv-mem input { position: absolute; opacity: 0; width: 1px; height: 1px; }
.cv-mem .sw { width: 38px; height: 22px; border-radius: 11px; background: var(--surface-3); border: var(--hair) solid var(--line-2); position: relative; flex: none; transition: background .15s; }
.cv-mem .sw::after { content: ""; position: absolute; top: 2px; left: 2px; width: 16px; height: 16px; border-radius: 50%; background: var(--surface); box-shadow: var(--shadow-2); transition: transform .15s; }
.cv-mem input:checked + .sw { background: var(--accent); border-color: var(--accent); }
.cv-mem input:checked + .sw::after { transform: translateX(16px); }
.cv-mem input:focus-visible + .sw { outline: 2px solid var(--accent); outline-offset: 2px; }
.cv-mem .hint { font-size: var(--fs-xs); line-height: var(--lh-xs); color: var(--text-2); }
.cv-phead { display: flex; flex-direction: column; gap: 10px; margin-bottom: var(--s-4); }
.cv-gen { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
.cv-gen .cv-label { display: inline-flex; gap: 6px; align-items: center; }
.cv-gen .cv-num { width: 64px; }
/* Now */
.cv-nowbox { margin-bottom: var(--s-3); }
.cv-nowbox .cv-now { margin-bottom: 4px; }
.cv-tick { display: flex; gap: 8px; align-items: baseline; min-height: 32px; text-decoration: none; color: var(--text-2); font-size: var(--fs-sm); padding: 4px 0; min-width: 0; }
.cv-tick .s { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cv-tick:hover .s { color: var(--text); }
.cv-chipf .sw { width: 8px; height: 8px; border-radius: 2px; display: inline-block; }
.cv-chipf[data-sev="4"] .sw { background: var(--cv-s4); } .cv-chipf[data-sev="3"] .sw { background: var(--cv-s3); } .cv-chipf[data-sev="2"] .sw { background: var(--cv-s2); } .cv-chipf[data-sev="1"] .sw { background: var(--cv-s1); }
.cv-bar.kinds .cv-label { margin-left: 6px; }
.cv-bar.kinds .cv-label:first-child { margin-left: 0; }
/* Tasks (a run) */
.cv-tasks { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
.cv-task { border: var(--hair) solid var(--line); border-radius: 8px; background: var(--surface); }
.cv-task[data-state="run"] { border-color: var(--accent-line); }
.cv-task[data-state="fail"] { border-color: var(--crit); }
.cv-task-h, .cv-wr-h, .cv-ev-h { width: 100%; min-height: 44px; display: grid; align-items: center; gap: 10px; padding: 6px 10px; border: 0; background: none; color: var(--text); font: inherit; text-align: left; cursor: pointer; }
.cv-task-h { grid-template-columns: 22px minmax(0, 1fr) auto 12px; }
.cv-task-h .tx, .cv-wr-h .tx { display: flex; flex-direction: column; min-width: 0; }
.cv-task-h .tt, .cv-wr-h .tt { font-weight: var(--w-mid); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cv-task-h .ts { font-size: var(--fs-xs); color: var(--text-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cv-task-h .du, .cv-wr-h .du { font-family: var(--mono); font-size: var(--fs-xs); color: var(--muted); white-space: nowrap; }
.cv-car { color: var(--muted); font-size: 10px; }
.mk { width: 20px; height: 20px; border-radius: 50%; display: grid; place-items: center; font-size: 11px; color: #fff; background: var(--good); flex: none; }
.mk.run { background: var(--accent-soft); color: var(--accent-text); }
.mk.fail { background: var(--crit); }
.cv-task-b { padding: 4px 10px 12px; margin-top: 0; border-top: var(--hair) solid var(--line); }
.cv-task-b[hidden] { display: none; }
/* Agents */
.cv-sum { font-size: var(--fs-md); line-height: var(--lh-md); margin: 0 0 var(--s-3); font-variant-numeric: tabular-nums; }
.cv-wg { margin-bottom: 12px; }
.cv-wg > summary { list-style: none; cursor: pointer; display: flex; align-items: center; gap: 8px; min-height: 44px; font-weight: var(--w-title); }
.cv-wg > summary::-webkit-details-marker { display: none; }
.cv-wg > summary::before { content: "▸"; color: var(--muted); font-size: 11px; transition: transform .15s; }
.cv-wg[open] > summary::before { transform: rotate(90deg); }
.cv-wg > summary .n { font-family: var(--mono); font-size: var(--fs-xs); color: var(--muted); border: var(--hair) solid var(--line-2); border-radius: var(--r-pill); padding: 0 8px; }
.cv-wg.need > summary .n { color: var(--warn-ink); background: var(--warn-soft); border-color: var(--warn); }
.cv-wl { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
.cv-wr { border: var(--hair) solid var(--line); border-radius: 8px; background: var(--surface); overflow: hidden; }
.cv-wr-h { grid-template-columns: 28px minmax(0, 1fr) auto 12px; row-gap: 4px; min-height: 56px; padding: 8px 10px 6px; }
.cv-wr-h .sx { font-size: var(--fs-xs); color: var(--accent-text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cv-wr-h .ic { width: 28px; height: 28px; border-radius: 8px; display: grid; place-items: center; font-family: var(--mono); font-size: var(--fs-sm); font-weight: var(--w-title); background: var(--accent-soft); color: var(--accent-text); }
.cv-wr-h .ic.you, .cv-wr-h .ic.need { background: var(--warn-soft); color: var(--warn-ink); }
.cv-wr-h .ic.merged { background: var(--good-soft); color: var(--good-ink); }
.cv-wr-h .ic.queued { background: var(--surface-3); color: var(--muted); }
.cv-wr-h .ic.bad { background: var(--crit-soft); color: var(--crit-ink); }
.cv-mbar { grid-column: 2 / -1; display: grid; grid-template-columns: repeat(8, 1fr); gap: 2px; }
.cv-mbar i { height: 3px; border-radius: 2px; background: var(--surface-3); }
.cv-mbar i.p { background: var(--good); } .cv-mbar i.n { background: var(--accent); }
.cv-wr-b { padding: 6px 12px 12px; border-top: var(--hair) solid var(--line); display: flex; flex-direction: column; gap: 10px; }
.cv-wr-b[hidden] { display: none; }
.cv-check { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 4px; font-size: var(--fs-sm); }
.cv-check li { display: flex; gap: 8px; align-items: center; min-width: 0; }
.cv-check li .tt { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cv-check li .du { font-family: var(--mono); font-size: var(--fs-xs); color: var(--muted); white-space: nowrap; }
.cv-check li a { color: var(--text); text-decoration: none; min-height: 36px; display: inline-flex; align-items: center; gap: 4px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cv-check .mk { width: 16px; height: 16px; font-size: 9px; }
.cv-last { margin: 0; font-size: var(--fs-sm); color: var(--text-2); border-left: 2px solid var(--accent-line); padding-left: 8px; }
.cv-btns { gap: 8px; } .cv-btns .cv-btn { text-decoration: none; min-height: 40px; }
/* Activity */
.cv-evs { list-style: none; margin: 0; padding: 0; border-top: var(--hair) solid var(--line); }
.cv-ev { border-bottom: var(--hair) solid var(--line); }
.cv-ev[hidden] { display: none; }
.cv-ev-h { grid-template-columns: 76px minmax(0, 1fr) auto; padding: 6px 2px; }
.cv-ev-h .tx { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; font-size: var(--fs-sm); }
.cv-ev.open .cv-ev-h .tx { white-space: normal; }
.cv-ev-h .tm { font-family: var(--mono); font-size: var(--fs-xs); color: var(--muted); white-space: nowrap; }
.cv-ev-b { padding: 0 2px 12px 88px; display: flex; flex-direction: column; gap: 8px; }
.cv-ev-b[hidden] { display: none; }
.cv-ev-b .s { margin: 0; font-size: var(--fs-sm); }
.cv-ev.cv-new { animation: cv-in 2.4s ease-out; }
@keyframes cv-in { from { background: var(--accent-soft); } to { background: transparent; } }
.cv-k { display: inline-flex; justify-content: center; min-width: 0; height: 20px; padding: 0 6px; align-items: center; border-radius: 4px; font-family: var(--mono); font-size: 10px; letter-spacing: .04em; text-transform: uppercase; background: var(--surface-3); color: var(--text-2); white-space: nowrap; overflow: hidden; }
.cv-k.k-tick, .cv-k.k-decide { background: var(--accent-soft); color: var(--accent-text); }
.cv-k.k-gate, .cv-k.k-pr { background: var(--good-soft); color: var(--good-ink); }
.cv-k.bad { background: var(--crit-soft); color: var(--crit-ink); }
.cv-k.k-you, .cv-k.k-ask { background: var(--warn-soft); color: var(--warn-ink); }
/* Dashboard */
.cv-dash-bar { gap: 10px; flex-wrap: wrap !important; }
.cv-dash-bar > .cv-meta { flex: 1 1 100%; }
.cv-kpis { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 12px; margin-bottom: 12px; }
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
@media (max-width: 900px) { .cv-kpis { grid-template-columns: repeat(2, minmax(0, 1fr)); } .cv-dash { grid-template-columns: 1fr; } }
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
  .cv-strip > div { flex: 0 0 46%; scroll-snap-align: start; padding: 10px 12px; }
  .cv-strip .v { font-size: var(--fs-md); line-height: var(--lh-md); }
  .cv-strip .d { font-size: var(--fs-xs); line-height: var(--lh-xs); }
  .cv-bar.kinds { flex-wrap: nowrap; overflow-x: auto; padding-bottom: 4px; margin-left: -16px; margin-right: -16px; padding-left: 16px; padding-right: 16px; }
  .cv-bar.kinds > * { flex: 0 0 auto; }
  .cv-chipf { height: 44px; padding: 0 14px; }
  .cv-strip > div .cv-label { white-space: normal; }
  .cv-sheet-h .cv-chip, .cv-sheet-h .cv-meta { font-size: var(--fs-sm); }
  .cv-sheet-h .cv-chip { min-height: 28px; padding: 4px 8px; }
  .cv-sheet-h select { height: 36px; font-size: var(--fs-sm); }
  .cv-x { width: 44px; height: 44px; top: 6px; right: 6px; }
  .cv-tabs a { min-height: 44px; display: inline-flex; align-items: center; }
  .cv-seg button { min-height: 44px; }
  .cv-bar:not(.kinds) > .cv-label { display: none; }
  .cv-bar:not(.kinds) { flex-wrap: nowrap; }
  .cv-bar:not(.kinds) .cv-seg button { padding: 0 8px; }

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
  .cv-f input[type=number], .cv-f input[type=text] { max-width: none; height: 44px; }
  .cv-top h1.cv-ptitle { flex: 1 1 100%; }
  .cv-top .cv-pill.you, .cv-top .cv-pill.live { height: 32px; }
  .cv-ask-b { padding: 0 14px; }
  .cv-ev-h { grid-template-columns: 64px minmax(0, 1fr) auto; gap: 8px; }
  .cv-ev-b { padding-left: 2px; }
  .cv-kpi { min-height: 104px; padding: 10px 12px; }
  .cv-kpis { gap: 8px; }
  .cv-pipe li { grid-template-columns: 76px minmax(0, 1fr) 28px 64px; gap: 6px; }
  .cv-hurt button { grid-template-columns: 88px minmax(0, 1fr) 40px; }
  .cv-panel { padding: 12px; }
  .cv-lg { min-height: 36px; }
  .cv-wr-h .ic { width: 28px; }
}
`;

export const THEME_WORDS = { onboarding: "Onboarding", reliability: "Reliability", security: "Security", scalability: "Scalability", trust: "Trust", correctness: "Correctness", usability: "Usability", performance: "Performance", observability: "Observability", docs: "Docs" };
const SEV = { 4: "blocker", 3: "major", 2: "minor", 1: "papercut" };
const TABS = [["board", "Board"], ["activity", "Activity"], ["work", "Agents"], ["personas", "Personas"], ["metrics", "Metrics"], ["settings", "Autopilot"]];
const SEV_NAME = { blocker: 4, major: 3, minor: 2, papercut: 1 };
const KINDS = ["bug", "gap", "docs", "ux", "opportunity", "feature"];
const toMs = (t) => { if (t == null || t === "") return null; const n = typeof t === "number" ? t : /^\d+$/.test(String(t)) ? Number(t) : Date.parse(t); return Number.isFinite(n) && n > 0 ? n : null; };

/** What an agent's last event says it is doing, in a few plain words. */
export function doingWords(ev) {
  if (!ev) return null;
  if (ev === "turn finished") return "finished its turn";
  if (ev.startsWith("prompt:")) return "reading its task";
  if (ev.startsWith("session started")) return "starting";
  const m = /^(\w+)( done| failed)?(?::\s*(.*))?$/s.exec(ev);
  if (!m) return ev.slice(0, 80);
  const [, tool, end, detail = ""] = m;
  const base = (p) => p.split("/").filter(Boolean).pop() || p;
  if (tool === "Bash") {
    // The command without its cd prefix, and its first words.
    const cmd = detail.replace(/^cd\s+\S+\s*&&\s*/, "").trim();
    const w = cmd.split(/\s+/);
    let what;
    if (/repro\.sh/.test(cmd)) what = "running its repro";
    else if (/^cargo (build|b)\b/.test(cmd)) what = "building";
    else if (/^cargo (test|t|nextest)\b/.test(cmd) || /^(pytest|npm test|go test)/.test(cmd)) what = "running tests";
    else if (/^cargo (clippy|check)\b/.test(cmd)) what = "checking the code";
    else if (/^git (commit)/.test(cmd)) what = "committing";
    else if (/^git /.test(cmd)) what = `git ${w[1] || ""}`.trim();
    else if (/^(cat|head|tail|less|sed -n)\b/.test(cmd)) what = `reading ${base(w.at(-1) || "")}`;
    else if (/^(grep|rg|find|ls)\b/.test(cmd)) what = "looking around the code";
    else what = `running ${w.slice(0, 3).join(" ")}`.slice(0, 60);
    return end === " failed" ? `${what} (failed)` : what;
  }
  const words = { Read: "reading", Edit: "editing", MultiEdit: "editing", Write: "writing", Grep: "searching for", Glob: "looking for", WebFetch: "opening", TodoWrite: "planning", Task: "delegating" };
  const v = words[tool] || tool;
  const d = /^(Read|Edit|MultiEdit|Write)$/.test(tool) ? base(detail) : detail.slice(0, 50);
  return `${v}${d ? " " + d : ""}${end === " failed" ? " (failed)" : ""}`;
}

const mins = (...ts) => { const ms = ts.map(toMs).find((x) => x != null); if (ms == null) return ""; const m = Math.max(0, Math.round((Date.now() - ms) / 60000)); return m < 1 ? "just now" : m < 60 ? `${m} min` : `${Math.floor(m / 60)} h ${m % 60} min`; };

function toast(text) {
  const t = h("div", { class: "cv-toast", role: "status" }, text);
  document.body.append(t);
  setTimeout(() => t.remove(), 3000);
}
const enc = encodeURIComponent;
/** replaceChildren, flattening arrays and leaving out null, as h() does. */
const put = (el, ...kids) => el.replaceChildren(...kids.flat(Infinity).filter((k) => k != null && k !== false));
const ago = (t) => {
  const ms = toMs(t);
  if (!ms) return "";
  const s = Math.max(0, (Date.now() - ms) / 1000);
  return s < 60 ? "now" : s < 3600 ? `${Math.round(s / 60)} min` : s < 86400 ? `${Math.round(s / 3600)} h` : `${Math.round(s / 86400)} d`;
};
const usd = (x) => (x == null ? "–" : `$${Number(x).toFixed(2)}`);

/** The last data and view settings of each repository, kept across visits:
 *  coming back draws at once, then refreshes. */
const KEEP = new Map();

export function mount(root, ctx) {
  useStyle("converge", CSS);
  const host = h("div", { class: "cv" });
  put(root, host);
  // Converge works on one project: the hash's ?project= (a name, or a path).
  const proj = () => (ctx.params && ctx.params.project) || "";
  const projects = () => { try { return (typeof ctx.projects === "function" ? ctx.projects() : []) || []; } catch { return []; } };
  const projRow = () => { const n = proj(); return n ? projects().find((p) => p.project === n || p.path === n) || null : null; };
  const repo = () => { const p = projRow(); return p ? p.path : proj() && !projects().length ? (ctx.params.repo || "") : ""; };
  const projName = () => (projRow() || {}).project || proj();
  const kept = KEEP.get(repo()) || {};
  const S = {
    d: kept.d || null, sig: kept.sig, group: kept.group || "stage", range: kept.range, alive: true, timer: null,
    // The board's filters, multi-select: components, kinds (themes) and severities.
    fc: new Set(kept.fc || []), ft: new Set(kept.ft || []), fs: new Set(kept.fs || []),
    fold: new Map(kept.fold || []), hide: new Set(kept.hide || []), act: kept.act || null, actKinds: new Set(kept.actKinds || []),
  };
  {
    // The hash wins over the cache: a shared link opens with its filters.
    const q = new URLSearchParams(location.hash.split("?")[1] || "");
    const list = (k) => (q.get(k) || "").split(",").map((x) => x.trim()).filter(Boolean);
    if (q.has("component")) S.fc = new Set(list("component"));
    if (q.has("kind")) S.ft = new Set(list("kind"));
    if (q.has("sev")) S.fs = new Set(list("sev").map(Number).filter((x) => x >= 1 && x <= 4));
  }
  const keep = () => { if (repo()) KEEP.set(repo(), { d: S.d, sig: S.sig, group: S.group, range: S.range, fc: [...S.fc], ft: [...S.ft], fs: [...S.fs], fold: [...S.fold], hide: [...S.hide], act: S.act, actKinds: [...S.actKinds] }); };
  const api = (p, o) => ctx.api(p, o);
  const view = () => (location.hash.split("?")[0].split("/")[2] || "board");
  const fid = () => new URLSearchParams(location.hash.split("?")[1] || "").get("f");
  const rid = () => new URLSearchParams(location.hash.split("?")[1] || "").get("run");
  /** A Converge address that keeps the project and the filters. */
  const href = (v, extra = {}) => {
    const q = new URLSearchParams();
    if (proj()) q.set("project", proj());
    if (S.fc.size) q.set("component", [...S.fc].join(","));
    if (S.ft.size) q.set("kind", [...S.ft].join(","));
    if (S.fs.size) q.set("sev", [...S.fs].join(","));
    for (const [k, x] of Object.entries(extra)) if (x != null && x !== "") q.set(k, x);
    const s = q.toString().replace(/%2C/g, ",");
    return `#/converge${v && v !== "board" ? "/" + v : ""}${s ? "?" + s : ""}`;
  };
  const here = (extra) => href(view(), extra);
  /** Filters changed: the address follows without a hashchange. */
  const filtersToHash = () => { const x = { f: fid(), run: rid() }; history.replaceState(history.state, "", here(x)); keep(); };
  const matches = (f, skip) => (skip === "c" || !S.fc.size || S.fc.has(f.component || "")) && (skip === "t" || !S.ft.size || S.ft.has(f.theme)) && (skip === "s" || !S.fs.size || S.fs.has(f.severity));
  const filtered = () => (S.d?.findings || []).filter((f) => matches(f));
  const anyFilter = () => S.fc.size + S.ft.size + S.fs.size > 0;

  async function load() {
    if (!repo()) { picker(); return; }
    try {
      const d = await api(`/api/converge?repo=${enc(repo())}`);
      // Nothing changed: keep the page as it is (its scroll, a card being tapped).
      const sig = JSON.stringify({ ...d, metrics: { ...d.metrics, history: (d.metrics?.history || []).length } }).replace(/"(stage_since|updated|ts_ms|last_run)":\d+/g, "");
      if (S.d && sig === S.sig) return;
      S.d = d;
      S.sig = sig;
    } catch (err) {
      if (!S.d) put(host, h("p", { class: "cv-intro" }, `Converge did not load: ${err.message || err}`));
      return;
    }
    if (S.alive) draw();
  }

  // Where the page and each sideways strip are scrolled, kept across a redraw.
  function scrollState() {
    const els = [];
    for (let e = host; e; e = e.parentElement) if (e.scrollTop) els.push([e, e.scrollTop]);
    const wraps = [...host.querySelectorAll(".cv-wrap, .cv-strip, .cv-bar.kinds")].map((w) => w.scrollLeft);
    const cols = [...host.querySelectorAll(".cv-cards")].map((c) => c.scrollTop);
    return () => {
      for (const [e, t] of els) e.scrollTop = t;
      host.querySelectorAll(".cv-wrap, .cv-strip, .cv-bar.kinds").forEach((w, i) => { if (wraps[i] != null) w.scrollLeft = wraps[i]; });
      host.querySelectorAll(".cv-cards").forEach((c, i) => { if (cols[i] != null) c.scrollTop = cols[i]; });
    };
  }

  // ------------------------------------------------------------ projects
  /** No project: each one as a large card, with its open findings when known. */
  function picker() {
    if (ctx.crumbs) ctx.crumbs([{ label: "Converge", href: "#/converge" }]);
    const ps = projects();
    const unknown = proj() && !projRow();
    const cardOf = (p) => {
      const k = KEEP.get(p.path);
      const m = k?.d?.metrics;
      const st = k?.d?.settings;
      return h("a", { class: "cv-pj", href: `#/converge?project=${enc(p.project)}` },
        h("span", { class: "n" }, p.project),
        h("span", { class: "p" }, p.path),
        h("span", { class: "cv-meta" }, m ? [h("span", { class: "cv-chip" }, `${m.open || 0} open`), (m.open_by_severity || {}).blocker ? h("span", { class: "cv-chip s4" }, `${m.open_by_severity.blocker} blockers`) : null, h("span", {}, st?.enabled ? "autopilot on" : "autopilot off")] : h("span", {}, " ")));
    };
    put(host, 
      h("div", { class: "cv-top" }, h("h1", {}, "Converge")),
      h("p", { class: "cv-intro" }, unknown ? `There is no open project named “${proj()}”. Pick one, or open it first.` : "Pick a project. Each has its own board, personas and autopilot: personas play the product, and what they hit is reproduced, fixed and sent to you as a pull request."),
      h("div", { class: "cv-pjs" }, ps.map(cardOf), h("a", { class: "cv-pj add", href: "#/p" }, h("span", { class: "n" }, "+ Open a project"), h("span", { class: "p" }, "A git checkout on this machine"))));
    // The counts, and the cache each project then opens from.
    if (!S.pickAt || Date.now() - S.pickAt > 30000) {
      S.pickAt = Date.now();
      Promise.all(ps.slice(0, 12).map((p) => api(`/api/converge?repo=${enc(p.path)}`).then((d) => { const k = KEEP.get(p.path) || {}; KEEP.set(p.path, { ...k, d }); }, () => {})))
        .then(() => { if (S.alive && !repo()) picker(); });
    }
    if (!ps.length && ctx.refreshProjects) ctx.refreshProjects().then(() => { if (S.alive && projects().length) load(); });
  }

  /** The project's name as the title; a native select over it switches. */
  function projectTitle() {
    const ps = projects();
    const name = projName();
    const sel = h("select", { class: "cv-psel", "aria-label": "Project", onchange: () => { location.hash = sel.value ? `#/converge${view() === "board" ? "" : "/" + view()}?project=${enc(sel.value)}` : "#/converge"; } },
      ps.map((p) => h("option", { value: p.project, selected: p.project === name ? true : null }, p.project)),
      ps.some((p) => p.project === name) ? null : h("option", { value: name, selected: true }, name),
      h("option", { value: "" }, "All projects…"));
    return h("h1", { class: "cv-ptitle" }, h("span", { class: "cv-brand" }, "Converge"), h("span", { class: "cv-pname" }, name, h("span", { class: "car", "aria-hidden": "true" }, "▾")), sel);
  }

  function draw() {
    const d = S.d;
    keep();
    if (ctx.crumbs) ctx.crumbs([{ label: "Converge", href: "#/converge" }, { label: projName(), href: href("board") }]);
    const st = d.settings || {};
    const m = d.metrics || {};
    const v = view();
    const top = h("div", { class: "cv-top" },
      projectTitle(),
      h("span", { class: "cv-sp" }),
      h("span", { class: "cv-pill" }, h("span", { class: `cv-dot${st.enabled ? " on" : ""}` }), st.enabled ? `Autopilot on · every ${Math.round(st.tick_s / 60)} min` : "Autopilot off"),
      h("span", { class: "cv-pill" }, `${usd(m.spent_today)} of ${usd(st.budget_usd_day)} today`),
      workCount() ? h("a", { class: "cv-pill live", href: href("work") }, h("span", { class: "cv-live" }, `${workCount()} working`)) : null,
      needGroups().length ? h("a", { class: "cv-pill you", href: href("work") }, `${needGroups().length} need you`) : null,
      h("button", { class: "cv-btn", type: "button", onclick: () => act(() => api("/api/converge/tick", { method: "POST", body: { repo: d.repo } }), "Tick started: each item whose gate can be checked now is moving") }, "Run a tick"),
      h("button", { class: "cv-btn", type: "button", onclick: () => act(() => api("/api/converge/settings", { method: "PUT", body: { repo: d.repo, enabled: !st.enabled } }), st.enabled ? "Autopilot paused: running steps finish" : "Autopilot on") }, st.enabled ? "Pause" : "Turn on"));
    const tabs = h("nav", { class: "cv-tabs", "aria-label": "Converge" }, TABS.map(([k, l]) => h("a", { href: href(k), class: k === v ? "on" : null, "aria-current": k === v ? "page" : null }, l)));
    const body = v === "work" ? work() : v === "activity" ? activity() : v === "personas" ? personas() : v === "metrics" ? metrics() : v === "settings" ? settings() : board();
    const restore = scrollState();
    put(host, top, tabs, body);
    restore();
    if (v === "activity") actPoll(); else actStop();
    if (v === "work") workPoll(); else workStop();
    if (S.flash && v === "board") {
      const el = host.querySelector(`.cv-card[data-id="${window.CSS.escape(S.flash)}"]`);
      if (el && !S.flashed) { S.flashed = true; el.classList.add("cv-flash"); el.scrollIntoView({ block: "nearest", inline: "center", behavior: "smooth" }); setTimeout(() => { S.flash = null; el.classList.remove("cv-flash"); }, 4000); }
    }
    const f = fid();
    if (f && document.querySelector(".cv-sheet")?.dataset.id !== f) openSheet(f);
    const r = rid();
    if (r && document.querySelector(".cv-sheet")?.dataset.run !== r) openRun(r);
  }

  /** What waits for you, grouped as the Needs-you list groups it (src/converge.rs
   *  needs_items): one group per reason, so this count and the badge agree. */
  function needGroups() {
    const out = new Map();
    for (const f of S.d?.findings || []) {
      if (f.state !== "active") continue;
      const why = f.needs_you ? f.needs_you : f.stage === "review" && f.ci === "pass" ? "CI passed: approve the merge." : f.stage === "prioritize" && f.door ? "Touches a one-way door: approve it for development." : null;
      if (!why) continue;
      const key = why.split(/[:(]/)[0].trim();
      if (!out.has(key)) out.set(key, { key, why, findings: [] });
      out.get(key).findings.push(f);
    }
    return [...out.values()];
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
      return h("div", {}, askPanel(),
        h("p", { class: "cv-intro" }, "No personas or findings yet. Describe a problem above, generate personas from the code on the Personas tab, or load the six personas of ChloeDB's first evaluation and the problems they reported."),
        h("div", { class: "cv-bar" },
          h("a", { class: "cv-btn pri", href: href("personas") }, "Generate personas"),
          h("button", { class: "cv-btn", type: "button", onclick: () => act(() => api("/api/converge/seed", { method: "POST", body: { repo: d.repo } }), "Loaded the seed personas and their findings") }, "Load the seed personas")));
    }
    const shown = all.filter((f) => f.state === "active" && matches(f));
    const open = shown.filter((f) => f.stage !== "merged");
    const groups = needGroups();
    const strip = h("div", { class: "cv-strip" },
      tile("Persona ease", m.ease == null ? "–" : m.ease.toFixed(2), "average of active personas, 1 to 5"),
      tile("Open", anyFilter() ? open.length : m.open || 0, anyFilter() ? `${open.filter((f) => f.severity >= 4).length} blockers · filtered` : `${(m.open_by_severity || {}).blocker || 0} blockers · ${(m.open_by_severity || {}).major || 0} major`),
      tile("Needs you", groups.length, groups.length ? `${groups.reduce((a, g) => a + g.findings.length, 0)} cards in ${groups.length} decisions` : "nothing waits for you"),
      tile("New serious / persona-hour", m.serious_per_persona_hour == null ? "–" : m.serious_per_persona_hour.toFixed(2), "falling means converging"),
      tile("Repro rate", m.repro_rate == null ? "–" : `${Math.round(m.repro_rate * 100)}%`, "reproduced of found"));
    const themes = d.themes || Object.keys(THEME_WORDS);
    const comps = components();
    const by = { stage: "Stage", theme: "Kind", component: "Component" };
    if (S.group === "component" && !comps.length) S.group = "stage";
    const bar = h("div", { class: "cv-bar" },
      h("span", { class: "cv-label" }, "Group by"),
      h("div", { class: "cv-seg", role: "radiogroup", "aria-label": "Group by" },
        Object.entries(by).filter(([k]) => k !== "component" || comps.length).map(([k, l]) => h("button", { type: "button", class: S.group === k ? "on" : null, role: "radio", "aria-checked": String(S.group === k), onclick: () => { S.group = k; draw(); } }, l))));
    const cols = S.group === "theme"
      ? themes.filter((t) => shown.some((f) => f.theme === t)).map((t) => {
        const items = shown.filter((f) => f.theme === t).sort((a, b) => a.stage_index - b.stage_index || b.score - a.score);
        return col(THEME_WORDS[t] || t, `${items.filter((f) => f.stage === "merged").length} merged · ${items.filter((f) => f.severity >= 4 && f.stage !== "merged").length} blockers open`, items, false, true);
      })
      : S.group === "component"
        ? [...comps.map((c) => c.name), ""].filter((c) => shown.some((f) => (f.component || "") === c)).map((c) => {
          const items = shown.filter((f) => (f.component || "") === c).sort((a, b) => a.stage_index - b.stage_index || b.score - a.score);
          return col(c || "No component", `${items.filter((f) => f.stage === "merged").length} merged · ${items.filter((f) => f.severity >= 4 && f.stage !== "merged").length} blockers open`, items, false, true);
        })
        : d.stages.map((s) => col(s.name, s.gate, shown.filter((f) => f.stage === s.stage).sort((a, b) => (b.pinned - a.pinned) || b.score - a.score), s.you, false, s.stage));
    const parked = all.filter((f) => f.state === "parked" && matches(f));
    return h("div", {}, askPanel(), nowLine(), strip, filterStrip(), bar,
      h("div", { class: "cv-wrap" }, h("div", { class: `cv-board${S.group !== "stage" ? " theme" : ""}` }, cols.length ? cols : h("p", { class: "cv-empty" }, "Nothing matches these filters."))),
      parked.length ? h("details", { style: { marginTop: "12px" } }, h("summary", {}, `${parked.length} parked`), h("div", { class: "cv-cards" }, parked.map(card))) : null);
  }

  /** The components the overview names, else the ones the findings carry. */
  function components() {
    const d = S.d || {};
    if (Array.isArray(d.components) && d.components.length) return d.components.map((c) => (typeof c === "string" ? { name: c } : c)).filter((c) => c && c.name);
    const names = [...new Set((d.findings || []).map((f) => f.component).filter(Boolean))].sort();
    return names.map((name) => ({ name }));
  }

  /** What is running now, and what the autopilot last decided. */
  function nowLine() {
    const d = S.d;
    const all = d.findings || [];
    const running = all.filter((f) => f.busy || (f.agent && ["starting", "working", "waiting"].includes(f.agent_state)));
    const playing = (d.personas || []).filter((p) => p.busy);
    const lt = d.last_tick;
    const word = (x) => (typeof x === "string" ? x : x && (x.summary || x.text || x.why || x.decision)) || "";
    const decisions = lt ? (lt.decisions || []).map(word).filter(Boolean) : [];
    const started = lt ? (Array.isArray(lt.started) ? lt.started.length : Number(lt.started) || 0) : 0;
    const tick = lt ? h("a", { class: "cv-tick", href: href("activity") },
      h("span", { class: "cv-label" }, `Autopilot · ${ago(lt.ts) || "now"}`),
      h("span", { class: "s" }, decisions.length ? decisions.slice(0, 3).join(" · ") + (decisions.length > 3 ? ` · and ${decisions.length - 3} more` : "") : lt.summary || (started ? `started ${started}` : "nothing to do"))) : null;
    const items = [
      ...running.map((f) => h("button", { type: "button", onclick: () => { location.hash = here({ f: f.id }); } }, h("span", { class: "cv-live" }, `${liveWord(f)}: ${f.title.slice(0, 40)}${f.title.length > 40 ? "…" : ""}`))),
      ...playing.map((p) => h("button", { type: "button", onclick: () => { location.hash = p.live_run ? here({ run: p.live_run }) : href("personas"); } }, h("span", { class: "cv-live" }, `${p.name.split(",")[0]} is playing: watch`)))];
    return h("div", { class: "cv-nowbox" },
      h("div", { class: "cv-now", role: "status" }, h("span", { class: "cv-label" }, "Now"),
        items.length ? items : h("span", {}, d.settings?.enabled ? "Nothing running; the next tick starts what is due." : "Nothing running. Run a tick, or turn the autopilot on.")),
      tick);
  }

  /** One swipeable strip of multi-select chips: component, kind, severity. */
  function filterStrip() {
    const live = (S.d.findings || []).filter((f) => f.state === "active" && f.stage !== "merged");
    const chip = (on, label, n, blockers, toggle, title) => h("button", { class: `cv-chipf${on ? " on" : ""}`, type: "button", "aria-pressed": String(on), title, onclick: () => { toggle(); filtersToHash(); draw(); } },
      label, h("span", { class: "n" }, String(n)), blockers ? h("span", { class: "b", title: "blockers" }, `${blockers}!`) : null);
    const flip = (set, k) => () => { if (set.has(k)) set.delete(k); else set.add(k); };
    const comps = components();
    const of = (skip, pred) => live.filter((f) => matches(f, skip) && pred(f));
    const themes = S.d.themes || Object.keys(THEME_WORDS);
    return h("div", { class: "cv-bar kinds", role: "group", "aria-label": "Filters" },
      anyFilter() ? h("button", { class: "cv-chipf clear", type: "button", onclick: () => { S.fc.clear(); S.ft.clear(); S.fs.clear(); filtersToHash(); draw(); } }, "Clear") : null,
      comps.length ? [h("span", { class: "cv-label" }, "Component"), comps.map((c) => { const l = of("c", (f) => (f.component || "") === c.name); return chip(S.fc.has(c.name), c.name, l.length, l.filter((f) => f.severity >= 4).length, flip(S.fc, c.name)); })] : null,
      h("span", { class: "cv-label" }, "Kind"),
      themes.map((t) => { const l = of("t", (f) => f.theme === t); return l.length || S.ft.has(t) ? chip(S.ft.has(t), THEME_WORDS[t] || t, l.length, l.filter((f) => f.severity >= 4).length, flip(S.ft, t)) : null; }),
      h("span", { class: "cv-label" }, "Severity"),
      [4, 3, 2, 1].map((k) => { const l = of("s", (f) => f.severity === k); return h("button", { class: `cv-chipf sev${S.fs.has(k) ? " on" : ""}`, "data-sev": String(k), type: "button", "aria-pressed": String(S.fs.has(k)), onclick: () => { flip(S.fs, k)(); filtersToHash(); draw(); } }, h("i", { class: "sw", "aria-hidden": "true" }), SEV[k], h("span", { class: "n" }, String(l.length))); }));
  }

  // ------------------------------------------------------------ asking for a card
  /** Built once a page, so a redraw keeps what you typed and the pending card. */
  function askPanel() {
    if (S.ask) { paintAsk(); return S.ask.el; }
    const ta = h("textarea", { class: "cv-ask-t", rows: "2", placeholder: "Describe a problem or what you want", "aria-label": "Describe a problem or what you want", onkeydown: (e) => { if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) { e.preventDefault(); send(); } } });
    const btn = h("button", { class: "cv-btn pri cv-ask-b", type: "button", onclick: () => send() }, "Send");
    const out = h("div", { class: "cv-ask-out", "aria-live": "polite" });
    // By hand: title, detail, kind and severity; it is yours, so it goes on to Reproduce.
    const title = h("input", { type: "text", id: "cv-ah-t", maxlength: "200", placeholder: "Search results miss accented words" });
    const detail = h("textarea", { class: "cv-ta", id: "cv-ah-d", placeholder: "What happens, what you expected, how to see it", style: { minHeight: "70px", fontFamily: "var(--sans)", fontSize: "var(--fs-sm)" } });
    const kind = h("select", { id: "cv-ah-k" }, KINDS.map((k) => h("option", { value: k }, k)));
    const sev = h("select", { id: "cv-ah-s" }, ["blocker", "major", "minor", "papercut"].map((k) => h("option", { value: k, selected: k === "major" ? true : null }, k)));
    const comp = h("select", { id: "cv-ah-c" });
    const hand = h("details", { class: "cv-hand" }, h("summary", {}, "Add by hand"),
      h("div", { class: "cv-f" }, h("label", { for: "cv-ah-t" }, "Title"), title),
      h("div", { class: "cv-f" }, h("label", { for: "cv-ah-d" }, "Detail"), detail),
      h("div", { class: "cv-f2" }, h("div", { class: "cv-f" }, h("label", { for: "cv-ah-k" }, "Kind"), kind), h("div", { class: "cv-f" }, h("label", { for: "cv-ah-s" }, "Severity"), sev), h("div", { class: "cv-f cv-ah-comp" }, h("label", { for: "cv-ah-c" }, "Component"), comp)),
      h("button", { class: "cv-btn pri", type: "button", onclick: () => addByHand() }, "Add the card"));
    const el = h("section", { class: "cv-ask", "aria-label": "Ask for a card" }, h("div", { class: "cv-ask-row" }, ta, btn), out, hand);
    S.ask = { el, ta, btn, out, hand, title, detail, kind, sev, comp, state: null };
    async function send() {
      const text = ta.value.trim();
      if (!text || S.ask.state?.pending) return;
      S.ask.state = { pending: true, text, since: Date.now() };
      paintAsk();
      try {
        const r = await api("/api/converge/ask", { method: "POST", body: { repo: repo(), text } });
        const f = r.finding || {};
        S.ask.state = { done: true, finding: f };
        ta.value = "";
        S.flash = f.id; S.flashed = false;
        if (S.alive) await load();
      } catch (err) {
        // A server without the card writer: the same card, by hand.
        if (err.status === 404 || err.status === 405) {
          S.ask.state = { error: "This server cannot write cards yet. Add it by hand below: it starts from what you wrote.", text };
          title.value = text.split(/[.\n]/)[0].slice(0, 200); detail.value = text; hand.open = true;
        } else S.ask.state = { error: `The card was not written: ${err.message || err}`, text };
      }
      paintAsk();
    }
    async function addByHand() {
      if (!title.value.trim()) { title.focus(); toast("A title, first"); return; }
      try {
        const body = { repo: repo(), title: title.value.trim(), detail: detail.value.trim(), kind: kind.value, severity: sev.value, pick: true };
        if (comp.value) body.component = comp.value;
        const r = await api("/api/converge/findings", { method: "POST", body });
        const f = r.finding || {};
        S.ask.state = { done: true, finding: f, hand: true };
        title.value = ""; detail.value = ""; hand.open = false; ta.value = "";
        S.flash = f.id; S.flashed = false;
        await load();
      } catch (err) { toast(err.message || String(err)); }
      paintAsk();
    }
    paintAsk();
    return el;
  }

  function paintAsk() {
    const A = S.ask;
    if (!A) return;
    const st = A.state;
    A.btn.disabled = !!st?.pending;
    A.ta.disabled = !!st?.pending;
    const comps = components();
    const want = comps.map((c) => c.name).join(",");
    if (A.comp.dataset.k !== want) {
      A.comp.dataset.k = want;
      put(A.comp, h("option", { value: "" }, "Let it decide"), comps.map((c) => h("option", { value: c.name }, c.name)));
    }
    A.el.querySelector(".cv-ah-comp").hidden = !comps.length;
    if (!st) { put(A.out); return; }
    if (st.pending) {
      const secs = Math.round((Date.now() - st.since) / 1000);
      put(A.out, h("div", { class: "cv-card cv-pending", "data-sev": "2" },
        h("span", { class: "ttl" }, h("span", { class: "cv-live" }, "Writing the card…")),
        h("span", { class: "cv-meta" }, `“${st.text.slice(0, 140)}${st.text.length > 140 ? "…" : ""}”`),
        h("span", { class: "cv-meta" }, secs > 5 ? `${secs} s · it reads the project first, up to a minute` : "It reads the project first, up to a minute")));
      clearTimeout(A.tick); A.tick = setTimeout(() => { if (A.state === st) paintAsk(); }, 1000);
    } else if (st.done) {
      const f = st.finding;
      put(A.out, h("div", { class: "cv-asked" },
        h("span", { class: "ok", "aria-hidden": "true" }, "✓"),
        h("span", { class: "t" }, h("b", {}, f.title || "The card"), ` · ${SEV[f.severity] || f.severity || ""} · ${st.hand ? "added" : "written"}, on to Reproduce`),
        f.id ? h("a", { class: "cv-btn", href: here({ f: f.id }) }, "Open") : null,
        h("button", { class: "cv-btn ghost", type: "button", "aria-label": "Dismiss", onclick: () => { A.state = null; paintAsk(); } }, "×")));
    } else if (st.error) {
      put(A.out, h("div", { class: "cv-needs" }, st.error));
      if (!A.ta.value) A.ta.value = st.text;
    }
  }

  function tile(l, v, d) { return h("div", {}, h("div", { class: "cv-label" }, l), h("div", { class: "v" }, String(v)), h("div", { class: "d" }, d)); }

  function col(name, gate, items, you, showStage, stage) {
    return h("section", { class: `cv-col${you ? " you" : ""}`, "aria-label": name, "data-stage": stage || null },
      h("div", { class: "cv-col-h" }, h("div", { class: "t" }, h("span", {}, name), h("span", { class: "c" }, items.some((f) => liveWord(f)) ? h("span", { class: "cv-live", style: { color: "var(--accent-text)", marginRight: "8px" } }, `${items.filter((f) => liveWord(f)).length} working`) : null, String(items.length))), h("div", { class: "g" }, gate)),
      h("div", { class: "cv-cards" }, items.length ? items.map((f) => card(f, showStage)) : h("div", { class: "cv-empty" }, "Nothing here")));
  }

  /** What is happening to a finding right now, in words, or null. */
  function liveWord(f) {
    if (f.busy) return f.busy[0].toUpperCase() + f.busy.slice(1);
    if (!f.agent || !f.agent_state || f.state !== "active") return null;
    const who = f.stage === "reproduce" ? "Repro agent" : "Fix agent";
    const doing = doingWords(f.agent_doing);
    return { starting: `${who}: starting`, working: `${who}: ${doing || "working"}`, waiting: `${who}: needs you`, idle: f.stage === "develop" || f.stage === "reproduce" ? `${who} done: checking it next` : null }[f.agent_state] || null;
  }

  /** Where a finding is in its journey: a bar of the eight stages, the
   *  current one filled as far as its own steps go, and what comes next. */
  function progress(f) {
    const st = S.d.stages || [];
    const i = f.stage_index;
    const merged = f.stage === "merged";
    const stuck = !!f.needs_you || f.agent_state === "waiting";
    // How far into its stage: the rounds of develop, CI at review, a running step.
    const rounds = Math.max(1, S.d.settings?.max_rounds || 3);
    const inside = merged ? 100
      : f.stage === "develop" ? Math.min(90, 25 + (f.rounds || 0) * (60 / rounds) + (f.agent_state === "idle" ? 15 : 0))
      : f.stage === "review" ? (f.ci === "pass" ? 85 : f.pr_url ? 50 : 20)
      : f.busy ? 60 : 25;
    const pct = merged ? 100 : Math.round(((i + inside / 100) / 8) * 100);
    const next = merged ? (f.guard_ok === false ? "regressed" : "done") : `next: ${(st[i + 1] || {}).name || "merged"}`;
    const sub = f.stage === "develop" && f.rounds ? ` · round ${f.rounds + 1} of ${rounds}` : f.stage === "review" && f.ci ? ` · CI ${f.ci}` : "";
    return h("span", { class: "cv-prog", role: "progressbar", "aria-valuemin": "0", "aria-valuemax": "100", "aria-valuenow": String(pct), "aria-label": `${(st[i] || {}).name || f.stage}, stage ${i + 1} of 8` },
      h("span", { class: "bar" }, [0, 1, 2, 3, 4, 5, 6, 7].map((k) => h("i", { class: k < i || merged ? "p" : k === i ? (stuck ? "stop" : "n") : null, style: k === i && !merged ? { "--in": `${inside}%` } : null, title: (st[k] || {}).name }))),
      h("span", { class: "lbl" }, h("span", {}, h("b", {}, `${i + 1}/8 ${(st[i] || {}).name || f.stage}`), sub), h("span", {}, stuck ? "waiting on you" : next)));
  }

  function personaName(id) { const p = (S.d.personas || []).find((x) => x.id === id); return p ? p.name.split(",")[0] : id; }

  function card(f, showStage) {
    const stageName = (S.d.stages.find((s) => s.stage === f.stage) || {}).name || f.stage;
    const chips = [h("span", { class: `cv-chip s${f.severity}` }, SEV[f.severity] || f.severity)];
    if (S.group !== "theme") chips.push(h("span", { class: "cv-chip" }, THEME_WORDS[f.theme] || f.theme || "–"));
    if (f.component && S.group !== "component") chips.push(h("span", { class: "cv-chip comp" }, f.component));
    if (showStage) chips.push(h("span", { class: "cv-chip run" }, stageName));
    if (f.score && f.stage_index >= 3 && f.stage !== "merged") chips.push(h("span", { class: "cv-chip" }, `score ${f.score}`));
    if (f.door) chips.push(h("span", { class: "cv-chip you" }, `door: ${f.door}`));
    const live = liveWord(f);
    if (live) chips.unshift(h("span", { class: `cv-chip ${f.agent_state === "waiting" ? "you" : "run"}` }, h("span", { class: `cv-live${f.busy || ["starting", "working"].includes(f.agent_state) ? "" : " still"}` }, live)));
    if (f.needs_you) chips.push(h("span", { class: "cv-chip you" }, "needs you"));
    if (f.stage === "review" && f.ci) chips.push(h("span", { class: `cv-chip ${f.ci === "pass" ? "ok" : f.ci === "fail" ? "bad" : ""}` }, `CI ${f.ci}`));
    if (f.stage === "merged" && f.guard_ok != null) chips.push(h("span", { class: `cv-chip ${f.guard_ok ? "ok" : "bad"}` }, f.guard_ok ? "guard green" : "guard red"));
    const open = () => { location.hash = here({ f: f.id }); };
    const quick = (label, verb, msg) => h("button", { class: "cv-btn pri cv-quick", type: "button", onclick: (e) => { e.stopPropagation(); act(() => api(`/api/converge/findings/${enc(f.id)}/${verb}`, { method: "POST", body: {} }), msg); } }, label);
    const pick = f.state !== "active" ? null
      : f.stage === "found" && f.needs_you ? quick("Reproduce", "reproduce", "On its way: an agent writes the repro")
        : f.stage === "prioritize" && !f.door ? quick("Start development", "start", "A fix agent is on it") : null;
    const el = h("div", { class: `cv-card${live ? " working" : ""}`, role: "button", tabindex: "0", "data-sev": String(f.severity), "data-id": f.id, onclick: (e) => { if (Date.now() - (S.dragEnd || 0) > 300) open(); }, onkeydown: (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); open(); } } },
      h("span", { class: "ttl" }, f.title),
      h("span", { class: "cv-meta" }, h("span", {}, (f.personas_hit || []).map(personaName).join(", ") || f.by), f.occurrences > 1 ? h("span", {}, `${f.occurrences} times`) : null, h("span", {}, ago(f.stage_since))),
      h("span", { class: "cv-meta" }, chips),
      progress(f),
      pick);
    if (!showStage && S.group === "stage") draggable(el, f);
    return el;
  }

  // ------------------------------------------------------------ drag and drop
  // A card moves where one of your picks takes it: Found to Reproduce,
  // Prioritize to Develop, Verify or Review back to Develop, Review to
  // Merged (asks first). A gate decides every other move, so the board says
  // which columns take the card while you hold it. Hold a card (a quarter of
  // a second on a phone) to pick it up.
  function movesOf(f) {
    if (f.state !== "active") return {};
    const m = {};
    if (f.stage === "found") m.reproduce = ["reproduce", "On to Reproduce: an agent writes its repro"];
    if (f.stage === "prioritize" && !f.door) m.develop = ["start", "A fix agent is on it"];
    if (f.stage === "verify" || f.stage === "review") m.develop = ["back", "Sent back to develop"];
    if (f.stage === "review" && f.ci === "pass") m.merged = ["approve", "Approved and merged"];
    return m;
  }

  function draggable(el, f) {
    const moves = movesOf(f);
    if (!Object.keys(moves).length) return;
    el.classList.add("cv-dragme");
    el.addEventListener("pointerdown", (e) => {
      if (e.button !== 0 || e.target.closest("button, a")) return;
      const x0 = e.clientX, y0 = e.clientY;
      let ghost = null, over = null;
      const hold = setTimeout(start, e.pointerType === "mouse" ? 120 : 260);
      function start() {
        S.dragging = true;
        const r = el.getBoundingClientRect();
        ghost = el.cloneNode(true);
        ghost.classList.add("cv-ghost");
        Object.assign(ghost.style, { width: `${r.width}px`, left: `${r.left}px`, top: `${r.top}px` });
        ghost.dataset.dx = String(x0 - r.left); ghost.dataset.dy = String(y0 - r.top);
        document.body.append(ghost);
        el.classList.add("cv-lifted");
        host.querySelectorAll(".cv-col[data-stage]").forEach((c) => c.classList.add(moves[c.dataset.stage] ? "cv-can" : "cv-cannot"));
        if (navigator.vibrate) try { navigator.vibrate(10); } catch { /* not on this device */ }
      }
      const move = (ev) => {
        if (!ghost) {
          if (Math.hypot(ev.clientX - x0, ev.clientY - y0) > 8) cancel();
          return;
        }
        ev.preventDefault();
        ghost.style.left = `${ev.clientX - Number(ghost.dataset.dx)}px`;
        ghost.style.top = `${ev.clientY - Number(ghost.dataset.dy)}px`;
        ghost.hidden = true;
        const col = document.elementFromPoint(ev.clientX, ev.clientY)?.closest(".cv-col[data-stage]");
        ghost.hidden = false;
        if (over !== col) { over?.classList.remove("cv-over"); over = col && moves[col.dataset.stage] ? col : null; over?.classList.add("cv-over"); }
        // Near an edge, the board scrolls sideways under the card.
        const wrap = host.querySelector(".cv-wrap");
        if (wrap) { const w = wrap.getBoundingClientRect(); if (ev.clientX > w.right - 40) wrap.scrollLeft += 14; else if (ev.clientX < w.left + 40) wrap.scrollLeft -= 14; }
      };
      const touchBlock = (ev) => { if (ghost) ev.preventDefault(); };
      function cleanup() {
        clearTimeout(hold);
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", up);
        window.removeEventListener("pointercancel", cancel);
        window.removeEventListener("touchmove", touchBlock);
        host.querySelectorAll(".cv-col").forEach((c) => c.classList.remove("cv-can", "cv-cannot", "cv-over"));
        el.classList.remove("cv-lifted");
        ghost?.remove();
        if (ghost) S.dragEnd = Date.now();
        S.dragging = false;
        S.touched = Date.now();
      }
      function cancel() { cleanup(); }
      function up() {
        const target = over && moves[over.dataset.stage];
        const name = over ? (S.d.stages.find((x) => x.stage === over.dataset.stage) || {}).name : "";
        const lifted = !!ghost;
        cleanup();
        if (lifted && !target) toast("A gate moves it there, not a drag: the board shows where it can go");
        if (!target) return;
        const [verb, msg] = target;
        if (verb === "approve" && !confirm(`Merge the pull request of “${f.title}”?`)) return;
        act(() => api(`/api/converge/findings/${enc(f.id)}/${verb}`, { method: "POST", body: verb === "back" ? { text: `Sent back from the board to ${name}` } : {} }), msg);
      }
      window.addEventListener("pointermove", move, { passive: false });
      window.addEventListener("pointerup", up);
      window.addEventListener("pointercancel", cancel);
      window.addEventListener("touchmove", touchBlock, { passive: false });
    });
  }

  // ------------------------------------------------------------ the sheet
  async function openSheet(id) {
    const req = (S.sheetReq = (S.sheetReq || 0) + 1);
    let d;
    try { d = await api(`/api/converge/findings/${enc(id)}`); } catch (err) { toast(err.message); return; }
    // A later tap, or a closed sheet, wins over this answer.
    if (req !== S.sheetReq || fid() !== id || !S.alive) return;
    document.querySelectorAll(".cv-sheet, .cv-scrim").forEach((e) => e.remove());
    const f = d.finding;
    const close = () => { history.replaceState(history.state, "", here()); sheet.remove(); scrim.remove(); };
    const scrim = h("div", { class: "cv-scrim", onclick: close });
    const reload = async () => { close(); await load(); location.hash = here({ f: id }); };
    const run = (fn, msg) => async () => { try { await fn(); toast(msg); } catch (err) { toast(err.message || String(err)); } await reload(); };
    const post = (verb, body) => api(`/api/converge/findings/${enc(id)}/${verb}`, { method: "POST", body: body || {} });
    const sc = f.scope || {};
    const repro = h("textarea", { class: "cv-ta", "aria-label": "Repro script", spellcheck: "false" });
    repro.value = f.repro || "#!/bin/sh\n# exits 1 while the problem is there, 0 once it is fixed\n";
    const actions = [];
    if (f.stage === "review" && f.ci === "pass") actions.push(h("button", { class: "cv-btn good", type: "button", onclick: run(() => post("approve"), "Approved and merged") }, "Approve and merge"));
    if (f.stage === "review" || f.stage === "verify") actions.push(h("button", { class: "cv-btn", type: "button", onclick: run(() => post("back", { text: "Sent back from Converge" }), "Sent back to develop") }, "Send back to develop"));
    if (f.stage === "found" && f.state === "active") actions.push(h("button", { class: "cv-btn pri", type: "button", onclick: run(() => post("reproduce"), "On to Reproduce: an agent writes its repro") }, "Reproduce"));
    if (f.stage === "prioritize") actions.push(h("button", { class: "cv-btn pri", type: "button", onclick: run(() => post("start"), "Development started") }, "Start development"));
    if (f.stage === "reproduce" && !f.agent) actions.push(h("button", { class: "cv-btn", type: "button", onclick: run(() => post("repro-agent"), "An agent is writing the repro") }, "Ask an agent for the repro"));
    if (f.stage === "reproduce" && f.agent) actions.push(h("button", { class: "cv-btn", type: "button", onclick: run(() => post("take-repro"), "Took the agent's repro") }, "Take the agent's repro"));
    actions.push(h("button", { class: "cv-btn", type: "button", onclick: run(() => post(f.pinned ? "unpin" : "pin"), f.pinned ? "Unpinned" : "Pinned to the top") }, f.pinned ? "Unpin" : "Pin"));
    actions.push(h("button", { class: "cv-btn", type: "button", onclick: run(() => post(f.state === "parked" ? "unpark" : "park"), f.state === "parked" ? "Back in the loop" : "Parked") }, f.state === "parked" ? "Unpark" : "Park"));
    const reason = h("input", { type: "text", placeholder: "Why it is not a problem", "aria-label": "Why it is not a problem", style: { flex: "1", minWidth: "160px", height: "30px", border: "1px solid var(--line-2)", borderRadius: "6px", background: "var(--surface)", color: "var(--text)", padding: "0 8px" } });
    actions.push(reason, h("button", { class: "cv-btn", type: "button", onclick: run(() => post("dismiss", { reason: reason.value }), "Dismissed") }, "Dismiss"));
    const themeSel = h("select", { "aria-label": "Kind of problem", onchange: run(() => api(`/api/converge/findings/${enc(id)}`, { method: "PUT", body: { theme: themeSel.value } }), "Kind changed") },
      (S.d.themes || Object.keys(THEME_WORDS)).map((t) => h("option", { value: t, selected: t === f.theme ? true : null }, THEME_WORDS[t] || t)));
    // The component, changed like the kind (a server without components ignores it).
    const compNames = [...new Set([...components().map((c) => c.name), f.component].filter(Boolean))];
    const compSel = compNames.length || "component" in f ? h("select", { "aria-label": "Component", onchange: run(() => api(`/api/converge/findings/${enc(id)}`, { method: "PUT", body: { component: compSel.value } }), "Component changed") },
      h("option", { value: "", selected: f.component ? null : true }, "no component"),
      compNames.map((c) => h("option", { value: c, selected: c === f.component ? true : null }, c))) : null;
    const sheet = h("aside", { class: "cv-sheet", "data-id": id, role: "dialog", "aria-modal": "true", "aria-label": f.title },
      h("button", { class: "cv-x", type: "button", "aria-label": "Close", onclick: close }, "×"),
      h("div", { class: "cv-sheet-h" },
        h("div", { class: "cv-meta" }, h("span", { class: "cv-chip" }, f.id), h("span", { class: `cv-chip s${f.severity}` }, SEV[f.severity]), themeSel, compSel, h("span", {}, (S.d.stages[f.stage_index] || {}).name), f.state !== "active" ? h("span", { class: "cv-chip bad" }, f.state) : null),
        h("h2", {}, f.title),
        h("div", { class: "cv-stages" }, [0, 1, 2, 3, 4, 5, 6, 7].map((i) => h("span", { class: i < f.stage_index ? "d" : i === f.stage_index ? "n" : null, title: (S.d.stages[i] || {}).name })))),
      h("div", { class: "cv-sheet-b" },
        f.needs_you ? h("div", { class: "cv-needs" }, f.needs_you) : null,
        h("section", {}, h("div", { class: "cv-label" }, "What happened"), h("p", { style: { margin: "6px 0 0" } }, f.detail || "–"),
          h("p", { class: "cv-meta", style: { marginTop: "6px" } }, d.persona ? `Found by ${d.persona.name}${f.step ? ` at “${f.step}”` : ""}` : `Reported by ${f.by}`, f.occurrences > 1 ? ` · ${f.personas_hit.length} personas, ${f.occurrences} times` : "",
            f.run ? h("a", { href: here({ run: f.run }), style: { marginLeft: "8px" } }, "See the run") : null)),
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

  // ------------------------------------------------------------ tasks
  // A run's steps as tasks (Roberto: "compact and summarized like tasks that
  // you can unfold"): a `step` starts a task; what comes before the first is
  // "Getting started". Each task is one line, folded; the running one is open.
  function groupTasks(steps) {
    const tasks = [{ key: "start", title: "Getting started", ts: toMs(steps[0]?.ts), items: [] }];
    steps.forEach((x, i) => {
      if (x.kind === "step") tasks.push({ key: `s${i}`, title: x.text || `Step ${tasks.length}`, ts: toMs(x.ts), items: [] });
      else tasks.at(-1).items.push(x);
    });
    if (tasks.length > 1 && !tasks[0].items.length) tasks.shift();
    return tasks;
  }
  const secsWords = (ms) => { if (!Number.isFinite(ms) || ms < 0) return ""; const s = Math.round(ms / 1000); return s < 60 ? `${s} s` : s < 3600 ? `${Math.floor(s / 60)} min ${s % 60 ? `${s % 60} s` : ""}`.trim() : `${Math.floor(s / 3600)} h ${Math.floor((s % 3600) / 60)} min`; };
  const firstSentences = (t, n = 2) => { const x = String(t || "").replace(/\s+/g, " ").trim(); const m = x.match(new RegExp(`^(.*?[.!?])(\\s|$)${n > 1 ? "(.*?[.!?])?(\\s|$)" : ""}`)); const out = m ? (m[1] + (m[3] ? " " + m[3] : "")) : x; return out.length > 220 ? out.slice(0, 218) + "…" : out; };
  /** A task's state, from where it is in the run. */
  function taskStates(tasks, o) {
    return tasks.map((t, i) => {
      const last = i === tasks.length - 1;
      const state = last && o.live ? "run" : last && o.failed ? "fail" : "done";
      const end = last ? (o.live ? o.now : o.ended) : tasks[i + 1].ts;
      const said = t.items.filter((x) => x.kind !== "tool" && x.kind !== "think");
      const tools = t.items.filter((x) => x.kind === "tool");
      const lastTool = tools.at(-1);
      const sum = said.length ? firstSentences(said.at(-1).text, 1)
        : state === "run" ? (lastTool ? `${TOOL_WORDS[lastTool.tool] || lastTool.tool} ${lastTool.text || ""}` : "working…")
          : "";
      return { ...t, state, dur: t.ts && end ? end - t.ts : null, sum, tools: tools.length, said: said.length };
    });
  }
  const MARK = { done: "✓", fail: "✕", run: "" };
  function taskList(openMap, initial) {
    const list = h("ol", { class: "cv-tasks" });
    const nodes = new Map();
    let lastArgs = null;
    function paint(steps, o) {
      lastArgs = [steps, o];
      const tasks = taskStates(groupTasks(steps), o);
      const keys = new Set(tasks.map((t) => t.key));
      for (const [k, n] of nodes) if (!keys.has(k)) { n.el.remove(); nodes.delete(k); }
      for (const t of tasks) {
        let n = nodes.get(t.key);
        if (!n) {
          const head = h("button", { class: "cv-task-h", type: "button" });
          const body = h("div", { class: "cv-task-b cv-iv" });
          const el = h("li", { class: "cv-task" }, head, body);
          n = { el, head, body, rendered: 0 };
          head.addEventListener("click", () => { openMap.set(t.key, !n.open); if (lastArgs) paint(...lastArgs); });
          nodes.set(t.key, n);
          list.append(el);
        }
        const open = openMap.has(t.key) ? openMap.get(t.key) : t.state === "run";
        n.open = open;
        n.el.dataset.state = t.state;
        n.el.classList.toggle("open", open);
        n.head.setAttribute("aria-expanded", String(open));
        const meta = [t.tools ? `${t.tools} action${t.tools > 1 ? "s" : ""}` : null, secsWords(t.dur)].filter(Boolean).join(" · ");
        put(n.head, 
          h("span", { class: `mk ${t.state}`, "aria-label": t.state === "run" ? "working" : t.state === "fail" ? "failed" : "done" }, t.state === "run" ? h("span", { class: "cv-live" }) : MARK[t.state]),
          h("span", { class: "tx" }, h("span", { class: "tt" }, t.title), t.sum ? h("span", { class: "ts" }, t.sum) : null),
          h("span", { class: "du" }, meta),
          h("span", { class: "cv-car", "aria-hidden": "true" }, open ? "▴" : "▾"));
        n.body.hidden = !open;
        if (open && n.rendered < t.items.length) { renderItems(n.body, t.items.slice(n.rendered), initial); n.rendered = t.items.length; }
        if (open && !t.items.length && !n.body.childNodes.length) n.body.append(h("p", { class: "cv-empty" }, t.state === "run" ? "Working on it…" : "Nothing said here."));
        else if (t.items.length && n.body.firstElementChild?.classList.contains("cv-empty")) n.body.firstElementChild.remove();
      }
    }
    return { el: list, paint };
  }
  function renderItems(box, items, initial) {
    for (const x of items) {
      if (x.kind === "tool") {
        const last = box.lastElementChild;
        const line = h("div", { class: "cv-iv-act" }, h("span", { class: "w" }, TOOL_WORDS[x.tool] || x.tool), " ", h("code", {}, x.text || ""));
        if (last && last.classList.contains("cv-iv-acts")) last.append(line); else box.append(h("div", { class: "cv-iv-acts" }, line));
      } else {
        const t = toMs(x.ts);
        box.append(h("div", { class: `cv-iv-msg${x.kind === "think" ? " think" : ""}` },
          h("span", { class: "av", "aria-hidden": "true" }, initial),
          h("div", { class: "bub" }, x.kind === "think" ? h("span", { class: "lbl" }, "thinking") : null, x.text, t ? h("span", { class: "ts" }, new Date(t).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })) : null)));
      }
    }
  }

  // ------------------------------------------------------------ the agents
  // Groups: Needs you and Working open, Queued and Done folded; each row a
  // folded line (icon, title, status, time, stage bar) that unfolds into a
  // checklist of what it did, its last words and its buttons.
  const ICON = { fix: "F", repro: "R", persona: "P", generate: "G", gate: "W", need: "!", merged: "M", queued: "·" };
  function workModel() {
    const d = S.d;
    const fs = (d.findings || []).filter((f) => f.state === "active" && matches(f));
    const g = { need: [], working: [], queued: [], done: [] };
    for (const n of needGroups()) {
      const list = n.findings.filter((f) => matches(f));
      if (!list.length) continue;
      g.need.push({ key: `need:${n.key}`, icon: "need", title: n.why, status: `${list.length} card${list.length > 1 ? "s" : ""}`, since: Math.min(...list.map((f) => toMs(f.updated) || toMs(f.stage_since) || Date.now())), findings: list, you: true });
    }
    const workingIds = new Set();
    for (const f of fs) {
      const agentOn = f.agent && ["starting", "working", "waiting"].includes(f.agent_state);
      if (!agentOn && !f.busy) continue;
      workingIds.add(f.id);
      const kind = agentOn ? (f.stage === "reproduce" ? "repro" : "fix") : "gate";
      const status = agentOn ? (f.agent_state === "waiting" ? "needs you" : f.agent_state === "starting" ? "starting" : doingWords(f.agent_doing) || (kind === "repro" ? "writing repro" : "working")) : f.busy;
      g.working.push({ key: `f:${f.id}`, icon: kind, title: f.title, status, since: [f.agent_created, f.agent_started, f.agent_since, f.stage_since, f.updated], f, agent: agentOn ? f.agent : null, you: f.agent_state === "waiting" });
    }
    for (const p of d.personas || []) {
      if (!p.busy) continue;
      g.working.push({ key: `p:${p.id}`, icon: "persona", title: `${p.name.split(",")[0]} plays their journey`, status: p.live_at ? `step ${p.live_at.n}/${p.live_at.of}` : p.busy === "playing" ? "starting" : p.busy, since: [p.live_since], p, run: p.live_run, at: p.live_at });
    }
    for (const r of d.runs || []) {
      if (r.purpose !== "generate" || r.state !== "running" || g.working.some((x) => x.run === r.id)) continue;
      g.working.push({ key: `r:${r.id}`, icon: "generate", title: "Reading the project to write personas", status: r.at ? `step ${r.at.n}/${r.at.of}` : "reading", since: [r.created], run: r.id, at: r.at });
    }
    for (const f of fs) {
      if (workingIds.has(f.id) || f.needs_you || f.stage === "merged" || f.stage === "found" || f.stage === "prioritize") continue;
      g.queued.push({ key: `f:${f.id}`, icon: "queued", title: f.title, status: `waits: ${(d.stages[f.stage_index + 1] || {}).name || "next"}`.replace("waits: ", f.stage === "review" ? "CI, then you: " : "next: "), since: [f.stage_since, f.updated], f });
    }
    const day = Date.now() - 86400000;
    for (const r of d.runs || []) {
      if (r.state === "running" || (toMs(r.ended) || toMs(r.created) || 0) < day) continue;
      const p = (d.personas || []).find((x) => x.id === r.persona);
      g.done.push({ key: `r:${r.id}`, icon: r.purpose === "generate" ? "generate" : "persona", title: r.purpose === "generate" ? "Wrote personas from the code" : `${p ? p.name.split(",")[0] : "A persona"} played`, status: r.state === "failed" ? "failed" : `${(r.findings || []).length} found${r.cost_usd ? " · " + usd(r.cost_usd) : ""}`, since: [r.ended, r.created], run: r.id, failed: r.state === "failed", ended: true });
    }
    for (const f of (d.findings || []).filter((f) => f.stage === "merged" && matches(f) && (toMs(f.stage_since) || 0) >= day)) {
      g.done.push({ key: `f:${f.id}`, icon: "merged", title: f.title, status: "merged", since: [f.stage_since], f, ended: true });
    }
    g.done.sort((a, b) => (toMs(b.since[0]) || 0) - (toMs(a.since[0]) || 0));
    return g;
  }
  function workItems() { return S.d ? workModel().working : []; }
  function workCount() { return S.d ? workItems().length : 0; }

  function work() {
    const g = workModel();
    const m = S.d.metrics || {};
    const parts = [`${g.working.length} working`, `${g.queued.length} queued`, `${g.need.length} need you`, `${usd(m.spent_today)} today`];
    const GROUPS = [["need", "Needs you", true], ["working", "Working", true], ["queued", "Queued", false], ["done", "Done today", false]];
    S.wrows = new Map();
    return h("div", {},
      h("p", { class: "cv-sum", role: "status" }, parts.join(" · ")),
      filterStrip(),
      GROUPS.map(([k, label, dflt]) => {
        const rows = g[k];
        const gk = `g:${k}`;
        const open = S.fold.has(gk) ? S.fold.get(gk) : dflt;
        const det = h("details", { class: `cv-wg ${k}`, open: open && rows.length ? true : null },
          h("summary", {}, h("span", {}, label), h("span", { class: "n" }, String(rows.length))),
          rows.length ? h("ul", { class: "cv-wl" }, rows.map(workRow)) : h("p", { class: "cv-empty" }, k === "need" ? "Nothing waits for you." : k === "working" ? "Nothing at work now." : "None."));
        det.addEventListener("toggle", () => { S.fold.set(gk, det.open); keep(); });
        return det;
      }));
  }

  function workRow(it) {
    const open = !!S.fold.get(it.key);
    const f = it.f;
    const bar = f ? h("span", { class: "cv-mbar", "aria-hidden": "true" }, [0, 1, 2, 3, 4, 5, 6, 7].map((k) => h("i", { class: k < f.stage_index || f.stage === "merged" ? "p" : k === f.stage_index ? "n" : null })))
      : it.at && it.at.of ? h("span", { class: "cv-mbar", "aria-hidden": "true", style: { gridTemplateColumns: `repeat(${Math.min(it.at.of, 12)}, 1fr)` } }, Array.from({ length: Math.min(it.at.of, 12) }, (_, i) => h("i", { class: i + 1 < it.at.n ? "p" : i + 1 === it.at.n ? "n" : null }))) : null;
    const live = !it.ended && !it.you && it.icon !== "queued";
    const head = h("button", { class: "cv-wr-h", type: "button", "aria-expanded": String(open) },
      h("span", { class: `ic ${it.icon}${it.you ? " you" : ""}${it.failed ? " bad" : ""}`, "aria-hidden": "true" }, ICON[it.icon] || "·"),
      h("span", { class: "tx" }, h("span", { class: "tt" }, it.title), h("span", { class: "sx" }, live ? h("span", { class: "cv-live" }, it.status) : it.status)),
      h("span", { class: "du" }, mins(...[].concat(it.since))),
      h("span", { class: "cv-car", "aria-hidden": "true" }, open ? "▴" : "▾"),
      bar);
    const body = h("div", { class: "cv-wr-b", hidden: open ? null : true });
    const li = h("li", { class: `cv-wr${open ? " open" : ""}` }, head, body);
    head.addEventListener("click", () => {
      const o = !S.fold.get(it.key);
      S.fold.set(it.key, o); keep();
      li.classList.toggle("open", o); body.hidden = !o; head.setAttribute("aria-expanded", String(o));
      head.querySelector(".cv-car").textContent = o ? "▴" : "▾";
      if (o) { paintWorkBody(it, body); fetchWork(it, body, true); }
    });
    S.wrows.set(it.key, { it, body });
    if (open) { paintWorkBody(it, body); fetchWork(it, body); }
    return li;
  }

  /** What a row needs to unfold: its finding's events, or its run's steps. */
  async function fetchWork(it, body, force) {
    S.wd = S.wd || new Map();
    const c = S.wd.get(it.key);
    if (c && (c.busy || (!force && Date.now() - c.at < 3000))) return;
    const url = it.run ? `/api/converge/runs/${enc(it.run)}` : it.f ? `/api/converge/findings/${enc(it.f.id)}` : null;
    if (!url) return;
    S.wd.set(it.key, { ...(c || {}), busy: true, at: c?.at || 0 });
    try { const data = await api(url); S.wd.set(it.key, { data, at: Date.now() }); } catch { S.wd.set(it.key, { ...(c || {}), at: Date.now() }); }
    const row = S.wrows?.get(it.key);
    if (row && row.body.isConnected && !row.body.hidden) paintWorkBody(row.it, row.body);
  }

  function paintWorkBody(it, body) {
    const data = S.wd?.get(it.key)?.data;
    const btns = [];
    if (it.agent) btns.push(h("a", { class: "cv-btn", href: `#/a/${it.agent}/terminal` }, "Terminal"));
    if (it.f) btns.push(h("a", { class: "cv-btn", href: href("work", { f: it.f.id }) }, "Open card"));
    const runId = it.run || it.f?.run;
    if (runId) btns.push(h("a", { class: "cv-btn", href: href("work", { run: runId }) }, it.run ? "Interview" : "The run that found it"));
    let list = null, last = null;
    if (it.findings) {
      list = h("ul", { class: "cv-check" }, it.findings.slice(0, 12).map((f) => h("li", {}, h("a", { href: href("work", { f: f.id }) }, h("span", { class: `cv-chip s${f.severity}` }, SEV[f.severity]), " ", f.title))));
      if (it.findings.length > 12) list.append(h("li", { class: "cv-empty" }, `and ${it.findings.length - 12} more`));
      btns.push(h("a", { class: "cv-btn", href: "#/needs" }, "Needs you"));
    } else if (data && data.run) {
      const r = data.run;
      const tasks = taskStates(groupTasks(r.steps || []), { live: data.live, failed: r.state === "failed", ended: toMs(r.ended), now: data.now });
      list = h("ul", { class: "cv-check" }, tasks.map((t) => h("li", { class: t.state }, h("span", { class: `mk ${t.state}` }, t.state === "run" ? h("span", { class: "cv-live" }) : MARK[t.state]), h("span", { class: "tt" }, t.title), h("span", { class: "du" }, secsWords(t.dur)))));
      const said = (r.steps || []).filter((x) => x.kind === "say" || (x.kind !== "tool" && x.kind !== "think" && x.kind !== "step"));
      last = said.length ? firstSentences(said.at(-1).text) : r.answer ? firstSentences(r.answer) : r.error || null;
    } else if (data && data.finding) {
      const evs = (data.events || []).slice(-8);
      list = h("ul", { class: "cv-check" }, evs.map((e) => h("li", { class: e.passed ? "done" : e.gate === "reported" ? "done" : "fail" }, h("span", { class: `mk ${e.passed || e.gate === "reported" ? "done" : "fail"}` }, e.passed || e.gate === "reported" ? "✓" : "!"), h("span", { class: "tt" }, `${e.gate}${e.summary ? ": " + e.summary : ""}`), h("span", { class: "du" }, ago(e.ts_ms)))));
      last = it.f.agent_doing ? `Now: ${doingWords(it.f.agent_doing)}` : data.finding.needs_you || (evs.at(-1) || {}).summary || null;
    } else if (it.f || it.run) list = h("p", { class: "cv-empty" }, "Reading what it did…");
    put(body, ...[
      it.f ? progress(it.f) : null,
      list,
      last ? h("p", { class: "cv-last" }, last) : null,
      btns.length ? h("div", { class: "cv-meta cv-btns" }, btns) : null].filter(Boolean));
  }

  function workPoll() {
    if (S.workTimer) return;
    S.workTimer = setInterval(() => {
      if (view() !== "work") { workStop(); return; }
      if (document.hidden || document.querySelector(".cv-sheet")) return;
      for (const { it, body } of S.wrows?.values() || []) if (!body.hidden && body.isConnected && !it.ended) fetchWork(it, body);
      if (Date.now() - (S.touched || 0) > 4000) load();
    }, 4000);
  }
  function workStop() { clearInterval(S.workTimer); S.workTimer = null; }

  function lastRun(pid) { return (S.d.runs || []).find((r) => r.persona === pid && r.state !== "running"); }

  // ------------------------------------------------------------ activity
  // The autopilot's log (GET /api/converge/activity), newest first: a line
  // an item, unfolded into its evidence; new items are put on top, nothing
  // else is redrawn.
  const ACT = { tick: "Tick", decide: "Decision", gate: "Gate", persona: "Persona", agent: "Agent", pr: "PR", generate: "Generate", ask: "Ask", you: "You" };
  function activity() {
    if (!S.actUI) {
      const list = h("ol", { class: "cv-evs", "aria-live": "polite" });
      const chips = h("div", { class: "cv-bar kinds", role: "group", "aria-label": "Kinds of step" });
      const note = h("div", {});
      S.actUI = { el: h("div", {}, h("p", { class: "cv-intro" }, "Every step the autopilot takes, and why: what it considered on each tick, what it started and what waits. Tap a line for its evidence."), chips, note, list), list, chips, note, nodes: new Map() };
    }
    paintAct([]);
    return S.actUI.el;
  }
  async function fetchAct() {
    if (S.actBusy || !repo()) return;
    S.actBusy = true;
    try {
      const A = S.act || { items: [], since: null };
      const q = A.since ? `&since=${A.since}` : "&limit=200";
      const j = await api(`/api/converge/activity?repo=${enc(repo())}${q}`);
      S.actMissing = false;
      const ids = new Set(A.items.map((x) => x.id));
      const fresh = (j.items || []).filter((x) => x && x.id && !ids.has(x.id)).sort((a, b) => (toMs(b.ts) || 0) - (toMs(a.ts) || 0));
      const items = [...fresh, ...A.items].slice(0, 500);
      const newest = items.length ? Math.max(...items.slice(0, 20).map((x) => toMs(x.ts) || 0)) : null;
      S.act = { items, since: newest || toMs(j.now) || A.since };
      keep();
      if (S.alive && view() === "activity") paintAct(fresh);
    } catch (err) {
      if (err.status === 404 || err.status === 405) { S.actMissing = true; if (S.actUI) paintAct([]); }
    } finally { S.actBusy = false; }
  }
  function paintAct(fresh) {
    const U = S.actUI;
    if (!U) return;
    const items = (S.act && S.act.items) || [];
    put(U.note, S.actMissing ? h("p", { class: "cv-needs" }, "This server does not keep the autopilot's log yet: it comes with the next build.") : !items.length ? h("p", { class: "cv-empty" }, S.act ? "Nothing logged yet. Run a tick, or turn the autopilot on." : "Reading the log…") : null);
    // Kinds: a chip each, with its count; several can be on.
    const counts = {};
    for (const x of items) counts[x.kind] = (counts[x.kind] || 0) + 1;
    const kinds = Object.keys(ACT).filter((k) => counts[k] || S.actKinds.has(k)).concat(Object.keys(counts).filter((k) => !ACT[k]));
    put(U.chips, 
      h("button", { class: `cv-chipf${S.actKinds.size ? "" : " on"}`, type: "button", onclick: () => { S.actKinds.clear(); keep(); paintAct([]); } }, "All", h("span", { class: "n" }, String(items.length))),
      kinds.map((k) => h("button", { class: `cv-chipf${S.actKinds.has(k) ? " on" : ""}`, type: "button", "aria-pressed": String(S.actKinds.has(k)), onclick: () => { if (S.actKinds.has(k)) S.actKinds.delete(k); else S.actKinds.add(k); keep(); paintAct([]); } }, h("span", { class: `cv-k k-${k}` }, ACT[k] || k), h("span", { class: "n" }, String(counts[k] || 0)))));
    // Rows: the new ones on top; the page stays where you are reading.
    const scroller = scrollerOf(U.list);
    const anchor = scroller && scroller.scrollTop > 0 ? U.list.querySelector(".cv-ev:not([hidden])") : null;
    const top0 = anchor ? anchor.getBoundingClientRect().top : 0;
    if (!U.list.childNodes.length) for (const x of items) U.list.append(actRow(x));
    else { const at = U.list.firstChild; for (const x of fresh) if (!U.nodes.has(x.id)) U.list.insertBefore(actRow(x), at); }
    for (const [id, n] of U.nodes) {
      n.el.hidden = S.actKinds.size ? !S.actKinds.has(n.x.kind) : false;
      n.time.textContent = ago(n.x.ts) || "now";
      if (fresh.some((x) => x.id === id) && anchor == null && U.list.childNodes.length > fresh.length) { n.el.classList.add("cv-new"); setTimeout(() => n.el.classList.remove("cv-new"), 2500); }
    }
    if (anchor) scroller.scrollTop += anchor.getBoundingClientRect().top - top0;
  }
  function scrollerOf(el) {
    for (let e = el.parentElement; e; e = e.parentElement) { const o = getComputedStyle(e).overflowY; if ((o === "auto" || o === "scroll") && e.scrollHeight > e.clientHeight) return e; }
    return document.scrollingElement;
  }
  function actRow(x) {
    const time = h("span", { class: "tm" }, ago(x.ts) || "now");
    const failed = x.kind === "gate" && (x.passed === false || x.detail?.passed === false);
    const head = h("button", { class: "cv-ev-h", type: "button", "aria-expanded": "false" },
      h("span", { class: `cv-k k-${x.kind}${failed ? " bad" : ""}` }, ACT[x.kind] || x.kind || "–"),
      // The server's summaries often start with the actor's name: said once.
      h("span", { class: "tx" }, x.actor ? h("b", {}, x.actor, " ") : null, x.actor && (x.summary || "").startsWith(x.actor) ? x.summary.slice(x.actor.length).replace(/^[\s:,]+/, "") : x.summary || ""),
      time);
    const body = h("div", { class: "cv-ev-b", hidden: true });
    const el = h("li", { class: "cv-ev", "data-kind": x.kind || "" }, head, body);
    head.addEventListener("click", () => {
      const o = body.hidden;
      body.hidden = !o; el.classList.toggle("open", o); head.setAttribute("aria-expanded", String(o));
      if (o && !body.childNodes.length) body.append(...actDetail(x));
    });
    S.actUI.nodes.set(x.id, { el, x, time });
    return el;
  }
  function actDetail(x) {
    const t = toMs(x.ts);
    const links = [];
    const fid0 = typeof x.finding === "object" ? x.finding?.id : x.finding;
    const run0 = typeof x.run === "object" ? x.run?.id : x.run;
    const ag = typeof x.agent === "object" ? x.agent?.id : x.agent;
    if (fid0) links.push(h("a", { class: "cv-btn", href: href("activity", { f: fid0 }) }, "The card"));
    if (run0) links.push(h("a", { class: "cv-btn", href: href("activity", { run: run0 }) }, "The interview"));
    if (ag) links.push(h("a", { class: "cv-btn", href: `#/a/${ag}/terminal` }, "The terminal"));
    if (x.persona) links.push(h("a", { class: "cv-btn", href: href("personas") }, "Personas"));
    return [
      h("p", { class: "s" }, x.summary || "", t ? h("span", { class: "cv-meta" }, ` · ${new Date(t).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", second: "2-digit" })}`) : null),
      x.detail != null && !(typeof x.detail === "object" && !Object.keys(x.detail).length) ? kvOf(x.detail) : null,
      links.length ? h("div", { class: "cv-meta cv-btns" }, links) : null].filter(Boolean);
  }
  /** Evidence as key/values; nested values as pretty JSON. */
  function kvOf(v) {
    if (v == null || typeof v !== "object" || Array.isArray(v) && v.some((x) => x && typeof x === "object")) return h("pre", { class: "cv-pre" }, typeof v === "string" ? v : JSON.stringify(v, null, 2));
    if (Array.isArray(v)) return h("ul", { class: "cv-check" }, v.map((x) => h("li", {}, String(x))));
    return h("dl", { class: "cv-kv" }, Object.entries(v).map(([k, x]) => [h("dt", {}, k.replace(/_/g, " ")),
      h("dd", {}, x == null ? "–" : typeof x !== "object" ? String(x) : Array.isArray(x) && x.every((y) => y == null || typeof y !== "object") ? (x.length ? (x.join(", ").length < 120 ? x.join(", ") : h("ul", { class: "cv-check" }, x.map((y) => h("li", {}, String(y))))) : "none") : h("pre", { class: "cv-pre" }, JSON.stringify(x, null, 2)))]));
  }
  function actPoll() {
    if (S.actTimer) return;
    fetchAct();
    S.actTimer = setInterval(() => { if (view() !== "activity") { actStop(); return; } if (!document.hidden) fetchAct(); }, 4000);
  }
  function actStop() { clearInterval(S.actTimer); S.actTimer = null; }

  // ------------------------------------------------------------ a run, live
  const TOOL_WORDS = { Bash: "ran", Read: "read", Write: "wrote", Edit: "edited", MultiEdit: "edited", Glob: "looked for", Grep: "searched for", WebFetch: "opened", WebSearch: "searched the web for", TodoWrite: "planned" };
  async function openRun(id) {
    const req = (S.sheetReq = (S.sheetReq || 0) + 1);
    document.querySelectorAll(".cv-sheet, .cv-scrim").forEach((e) => e.remove());
    const close = () => { clearInterval(timer); history.replaceState(history.state, "", here()); sheet.remove(); scrim.remove(); };
    const scrim = h("div", { class: "cv-scrim", onclick: close });
    const body = h("div", { class: "cv-sheet-b" }, h("p", { class: "cv-empty" }, "Joining the interview…"));
    const head = h("div", { class: "cv-sheet-h" });
    const sheet = h("aside", { class: "cv-sheet", "data-run": id, role: "dialog", "aria-modal": "true", "aria-label": "A run" },
      h("button", { class: "cv-x", type: "button", "aria-label": "Close", onclick: close }, "×"), head, body);
    document.body.append(scrim, sheet);
    let stick = true, started = false, tl = null;
    const openMap = new Map();
    const typing = h("div", { class: "cv-iv-typing", "aria-hidden": "true" }, h("i"), h("i"), h("i"));
    const note = h("div", {});
    const tail = h("div", {});
    body.addEventListener("scroll", () => { stick = body.scrollTop + body.clientHeight >= body.scrollHeight - 40; }, { passive: true });
    const paint = async () => {
      let d;
      try { d = await api(`/api/converge/runs/${enc(id)}`); } catch (err) { put(body, h("p", { class: "cv-empty" }, err.message || String(err))); clearInterval(timer); return; }
      if (req !== S.sheetReq || !sheet.isConnected) { clearInterval(timer); return; }
      const r = d.run, pe = d.persona || {};
      const gen = r.purpose === "generate" || (!d.persona && !r.persona);
      const title = gen ? "Reading the project to write personas" : pe.name || "A persona's run";
      sheet.setAttribute("aria-label", title);
      const c0 = toMs(r.created), e0 = toMs(r.ended) || toMs(d.now);
      const secs = c0 && e0 ? Math.max(0, Math.round((e0 - c0) / 1000)) : null;
      const dur = secs == null ? "" : secs >= 60 ? `${Math.floor(secs / 60)} min ${secs % 60} s` : `${secs} s`;
      const at = r.at;
      const pct = at && at.of ? Math.min(100, Math.round(((at.n - (d.live ? 0.5 : 0)) / at.of) * 100)) : null;
      put(head, 
        h("div", { class: "cv-meta" }, d.live ? h("span", { class: "cv-chip run" }, h("span", { class: "cv-live" }, gen ? "reading now" : "playing now")) : h("span", { class: `cv-chip ${r.state === "failed" ? "bad" : "ok"}` }, r.state === "failed" ? "failed" : r.outcome || r.state),
          h("span", { class: "cv-chip" }, r.purpose), h("span", {}, dur), r.cost_usd ? h("span", {}, usd(r.cost_usd)) : null, r.ease != null ? h("span", {}, `ease ${r.ease}`) : null, r.use_memory ? h("span", { class: "cv-chip mem", title: "This run remembered its earlier visits" }, "memory") : null),
        h("h2", {}, title),
        at ? h("div", { class: "cv-prog" }, h("div", { class: "bar", style: { gridTemplateColumns: `repeat(${Math.min(at.of, 12)}, 1fr)` } },
          Array.from({ length: Math.min(at.of, 12) }, (_, i) => h("i", { class: i + 1 < at.n || (!d.live && i + 1 <= at.n) ? "p" : i + 1 === at.n ? "n" : null }))),
          h("div", { class: "lbl" }, h("span", {}, h("b", {}, `Step ${at.n} of ${at.of}`), at.name ? `: ${at.name}` : ""), h("span", {}, pct != null ? `${pct}%` : ""))) : h("div", { class: "cv-meta" }, d.live ? (gen ? "Reading the README, the docs and the code…" : "Starting its journey…") : ""));
      const steps = r.steps || [];
      const initial = gen ? "C" : (pe.name || "?").trim()[0] || "?";
      if (!started) {
        tl = taskList(openMap, initial);
        put(body, ...[
          !gen && pe.journey ? h("details", { class: "cv-iv-journey" }, h("summary", { class: "cv-label" }, "Their task"), h("p", { class: "role" }, pe.journey)) : null,
          note, tl.el, typing, tail].filter(Boolean));
        started = true;
      }
      tl.paint(steps, { live: d.live, failed: r.state === "failed", ended: toMs(r.ended), now: toMs(d.now) || Date.now() });
      put(note, steps.length ? "" : h("p", { class: "cv-empty" }, d.live ? (gen ? "Opening the project…" : `${(pe.name || "They").split(",")[0]} is opening a fresh checkout…`) : "This run kept no conversation (it ran before runs were recorded)."));
      typing.hidden = !d.live;
      const made = gen ? (r.findings || []).filter((x) => x && (x.persona || x.name)) : [];
      put(tail, ...[
        !d.live && r.answer && !gen ? h("section", {}, h("div", { class: "cv-label" }, "What they achieved"), h("p", {}, r.answer)) : null,
        !d.live && !gen && (r.findings || []).length ? h("section", {}, h("div", { class: "cv-label" }, `What they reported (${r.findings.length})`), h("ul", {}, r.findings.map((x) => h("li", {}, x.title)))) : null,
        made.length ? h("section", {}, h("div", { class: "cv-label" }, `Personas written (${made.length}), on probation`), h("ul", {}, made.map((x) => h("li", {}, x.name || x.persona))), h("a", { class: "cv-btn", href: href("personas") }, "See them")) : null,
        r.error ? h("div", { class: "cv-needs" }, r.error) : null].filter(Boolean));
      if (!d.live) { clearInterval(timer); if (gen && made.length) load(); }
      if (stick) body.scrollTop = body.scrollHeight;
    };
    const timer = setInterval(paint, 1500);
    await paint();
  }

  /** "Generate from the code": the runner reads the project and writes
   *  personas, live as a run; an older server makes one persona at once. */
  function genControl() {
    const n = h("input", { type: "number", min: "1", max: "6", value: String(S.genN || 3), "aria-label": "How many personas", class: "cv-num", onchange: () => { S.genN = Math.max(1, Math.min(6, Number(n.value) || 3)); } });
    const go = h("button", { class: "cv-btn pri", type: "button", onclick: async () => {
      go.disabled = true;
      try {
        const count = Math.max(1, Math.min(6, Number(n.value) || 3));
        const r = await api("/api/converge/personas/generate", { method: "POST", body: { repo: repo(), from: "codebase", count } });
        const id = typeof r.run === "string" ? r.run : r.run && r.run.id;
        if (id) { location.hash = href("personas", { run: id }); load(); }
        else { toast(r.persona ? `${r.persona.name} is on probation (this server writes one at a time)` : "A new persona is on probation"); await load(); }
      } catch (err) { toast(err.message || String(err)); }
      go.disabled = false;
    } }, "Generate from the code");
    return h("div", { class: "cv-gen" }, h("label", { class: "cv-label" }, "How many", n), go);
  }

  /** "Use past memory": the persona keeps its own ChloeCtx store between
   *  visits. Saved on the persona (use_memory); an older server says so. */
  function memSwitch(p) {
    const id = `cv-mem-${p.id}`;
    const inp = h("input", { type: "checkbox", id, role: "switch", checked: p.use_memory ? true : null, "aria-describedby": `${id}-h` });
    inp.addEventListener("change", async () => {
      const v = inp.checked;
      const body = { use_memory: v };
      try {
        try { await api(`/api/converge/personas/${enc(p.id)}`, { method: "PUT", body }); }
        catch (err) { if (err.status === 404 || err.status === 405) await api(`/api/converge/personas/${enc(p.id)}/update`, { method: "POST", body }); else throw err; }
        toast(v ? `${p.name.split(",")[0]} remembers earlier visits from now on` : `${p.name.split(",")[0]} comes in fresh from now on`);
      } catch (err) { inp.checked = !v; toast(err.status === 404 || err.status === 405 ? "This server cannot keep that setting yet" : err.message || String(err)); }
      await load();
    });
    return h("div", { class: "cv-mem" }, h("label", { for: id }, inp, h("span", { class: "sw", "aria-hidden": "true" }), "Use past memory"),
      h("span", { class: "hint", id: `${id}-h` }, "Remembers its own earlier visits (its own ChloeCtx store, separate from yours). Off: comes in fresh like a first-time user."));
  }

  function personas() {
    const d = S.d;
    const ps = d.personas || [];
    const pc = (p) => h("div", { class: `cv-pc ${p.state}${p.live_run ? " live" : ""}`, onclick: p.live_run ? (e) => { if (!e.target.closest("button, a, summary, label, input")) location.hash = href("personas", { run: p.live_run }); } : null },
      h("div", {}, h("div", { class: "name" }, p.name), h("div", { class: "role" }, p.role)),
      h("div", { class: "cv-meta" }, h("span", { class: "cv-chip" }, p.origin), h("span", { class: `cv-chip ${p.state === "active" ? "ok" : p.state === "probation" ? "run" : ""}` }, p.state), (p.surfaces || []).map((x) => h("span", { class: "cv-chip" }, x)), p.busy ? h("span", { class: "cv-chip run" }, p.busy) : null, p.use_memory ? h("span", { class: "cv-chip mem", title: "Uses past memory" }, "memory") : null),
      p.why ? h("div", { class: "role" }, p.why) : null,
      h("details", {}, h("summary", { class: "role" }, "Journey"), h("p", { class: "role" }, p.journey)),
      spark(p.ease_hist),
      h("div", { class: "cv-stats" },
        h("div", {}, h("div", { class: "v" }, p.ease == null ? "–" : String(p.ease)), h("div", { class: "l" }, "ease")),
        h("div", {}, h("div", { class: "v" }, String(p.runs)), h("div", { class: "l" }, "runs")),
        h("div", {}, h("div", { class: "v" }, p.repro_rate == null ? "–" : `${Math.round(p.repro_rate * 100)}%`), h("div", { class: "l" }, "repro rate")),
        h("div", {}, h("div", { class: "v" }, String(p.open)), h("div", { class: "l" }, "open"))),
      memSwitch(p),
      p.live_run ? h("button", { class: "cv-btn live", type: "button", onclick: () => { location.hash = href("personas", { run: p.live_run }); } }, h("span", { class: "cv-live" }, "Playing now: join the live interview")) : null,
      h("div", { class: "cv-meta" },
        !p.live_run && lastRun(p.id) ? h("button", { class: "cv-btn", type: "button", onclick: () => { location.hash = href("personas", { run: lastRun(p.id).id }); } }, "Last run") : null,
        h("button", { class: "cv-btn", type: "button", disabled: p.busy ? true : null, onclick: () => act(() => api(`/api/converge/personas/${p.id}/play`, { method: "POST", body: {} }), `${p.name.split(",")[0]} is playing${p.use_memory ? ", with their memory" : ""}`) }, "Play now"),
        h("button", { class: "cv-btn", type: "button", disabled: p.busy ? true : null, title: p.use_memory ? "Play once as a first-time user" : "Play once with their memory of earlier visits", onclick: () => act(() => api(`/api/converge/personas/${p.id}/play`, { method: "POST", body: { use_memory: !p.use_memory } }), `${p.name.split(",")[0]} is playing ${p.use_memory ? "fresh, this once" : "with their memory, this once"}`) }, p.use_memory ? "Once, fresh" : "Once, with memory"),
        p.state === "retired" ? h("button", { class: "cv-btn", type: "button", onclick: () => act(() => api(`/api/converge/personas/${p.id}/activate`, { method: "POST", body: {} }), "Active again") }, "Activate")
          : h("button", { class: "cv-btn", type: "button", onclick: () => act(() => api(`/api/converge/personas/${p.id}/retire`, { method: "POST", body: {} }), "Retired") }, "Retire")));
    const name = h("input", { type: "text", id: "cv-pn", placeholder: "Omar, data engineer" });
    const role = h("input", { type: "text", id: "cv-pr", placeholder: "Who they are and what they want" });
    const journey = h("textarea", { class: "cv-ta", id: "cv-pj", placeholder: "The task they try, end to end", style: { minHeight: "70px", fontFamily: "var(--sans)", fontSize: "var(--fs-sm)" } });
    const add = h("details", { class: "cv-panel", style: { marginTop: "16px" } }, h("summary", {}, "Add a persona"),
      h("div", { class: "cv-f" }, h("label", { for: "cv-pn" }, "Name"), name), h("div", { class: "cv-f" }, h("label", { for: "cv-pr" }, "Role"), role), h("div", { class: "cv-f" }, h("label", { for: "cv-pj" }, "Journey"), journey),
      h("button", { class: "cv-btn pri", type: "button", onclick: () => act(() => api("/api/converge/personas", { method: "POST", body: { repo: d.repo, name: name.value, role: role.value, journey: journey.value } }), "Persona added") }, "Add"));
    return h("div", {},
      h("div", { class: "cv-phead" }, h("p", { class: "cv-intro", style: { margin: 0 } }, "Each persona replays its journey on a schedule and after merges. A generated persona starts on probation and joins once a finding of theirs reproduces."),
        genControl()),
      (d.runs || []).filter((r) => r.purpose === "generate" && r.state === "running").map((r) => h("button", { class: "cv-btn live", type: "button", style: { marginBottom: "12px" }, onclick: () => { location.hash = href("personas", { run: r.id }); } }, h("span", { class: "cv-live" }, "Reading the project to write personas: watch"))),
      ["active", "probation", "retired"].map((stt) => {
        const list = ps.filter((p) => p.state === stt);
        return list.length ? h("section", { style: { marginBottom: "20px" } }, h("div", { class: "cv-label", style: { margin: "0 0 8px" } }, stt === "active" ? "Active" : stt === "probation" ? "On probation" : "Retired"), h("div", { class: "cv-grid" }, list.map(pc))) : null;
      }), add);
  }

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
    const comp = [...S.fc].join(",");
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
    const goBoard = (fn) => () => { fn(); filtersToHash(); location.hash = href("board"); };
    const tileK = (label, value, sub, extra, onclick) => h("button", { class: "cv-kpi", type: "button", onclick }, h("span", { class: "cv-label" }, label), h("span", { class: "v" }, String(value)), h("span", { class: "sub" }, sub), extra);
    const kpis = h("div", { class: "cv-kpis" },
      tileK("Open issues", open.length, deltaEl(delta(openSeries)), sparkOf(openSeries, "var(--accent)"), goBoard(() => S.fs.clear())),
      tileK("Blockers open", open.filter((f) => f.severity >= 4).length, deltaEl(delta(blkSeries)), sparkOf(blkSeries, sevVar(4)), goBoard(() => { S.fs = new Set([4]); })),
      tileK("Merged this week", mergedWeek.length, median ? `median ${secsWords(median).replace(/ \d+ s$/, "")} found → merged` : "no merges yet", sparkOf(b.map((x) => x.merged), "var(--good)"), goBoard(() => { S.group = "stage"; })),
      tileK("Spend today", usd(spent), budget ? `of ${usd(budget)} · ${Math.round((spent / budget) * 100)}%` : "no budget set",
        h("span", { class: "cv-budget", "aria-hidden": "true" }, h("i", { class: spent > budget && budget ? "over" : null, style: { width: `${budget ? Math.min(100, (spent / budget) * 100) : 0}%` } })), () => { location.hash = href("settings"); }));
    const panel = (title, sub, ...kids) => h("section", { class: "cv-panel" }, h("div", { class: "cv-ph" }, h("h3", {}, title), sub ? h("div", { class: "sub" }, sub) : null), ...kids);
    const rangeSeg = h("div", { class: "cv-seg", role: "radiogroup", "aria-label": "Range" }, RANGES.map(([k, l]) =>
      h("button", { type: "button", class: range === k ? "on" : null, role: "radio", "aria-checked": String(range === k), onclick: () => { S.range = k; keep(); draw(); } }, l)));
    return h("div", {},
      filterStrip(),
      h("div", { class: "cv-bar cv-dash-bar" }, h("span", { class: "cv-label" }, "Range"), rangeSeg, anyFilter() ? h("span", { class: "cv-meta" }, S.ft.size || S.fs.size ? "Filtered; the chart over time follows the component filter only" : "Filtered by component") : null),
      kpis,
      h("div", { class: "cv-dash" },
        h("section", { class: "cv-panel wide" }, h("div", { class: "cv-ph" }, h("h3", {}, "Open issues over time"), h("div", { class: "sub" }, "Open by severity, stacked; found as dots, merged as a line. Tap for the numbers.")),
          t ? heroChart(b, range) : h("div", { class: "cv-plot", style: { height: "240px" } })),
        panel("Pipeline", "How far the findings got, Found to Merged", pipeline(fsAll)),
        panel("Where it hurts", "Open by kind, by severity, most blockers first; tap to filter the board", hurts(open, "theme")),
        components().length ? panel("By component", "Open by component, by severity; tap to filter the board", hurts(open, "component")) : null,
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
    return h("ul", { class: "cv-hurt" }, rows.map(([k, g]) => h("li", {}, h("button", { type: "button", onclick: () => { if (by === "component") S.fc = new Set([k]); else S.ft = new Set([k]); filtersToHash(); location.hash = href("board"); } },
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
        field("max_rounds", "Rounds before it asks you", "Develop and verify rounds without passing."),
        field("repro_agents", "Repro agents at once", "A finding in Reproduce without a script gets an agent to write one; 0 leaves it to you.")),
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
        tog("found_auto", "Move new findings to Reproduce", "Off: each new finding waits in Found until you choose Reproduce or Dismiss."),
        tog("develop_auto", "Start development on its own", "Off: a scoped finding waits in Prioritize until you choose Start development."),
        tog("follow_picks", "Carry on what you pick", "With the autopilot off, a finding you picked still moves to your next decision; personas play only with it on."),
        tog("generate", "Generate new personas", "When new serious findings dry up, one aimed at the least-played part."),
        tog("retire", "Retire stale personas", "After 3 runs with nothing new, or a repro rate under 40%."),
        tog("open_pr", "Open pull requests", "Push the branch and open its pull request with gh at review."),
        h("button", { class: "cv-btn pri", type: "button", onclick: () => act(() => api("/api/converge/settings", { method: "PUT", body: { repo: d.repo, settings: st } }), "Settings saved") }, "Save")));
  }

  const onHash = () => {
    if (!location.hash.startsWith("#/converge") || !S.d) return;
    const f = fid();
    const open = document.querySelector(".cv-sheet")?.dataset.id;
    if (view() !== S.view) { S.view = view(); document.querySelectorAll(".cv-sheet, .cv-scrim").forEach((e) => e.remove()); draw(); return; }
    const r = rid();
    if (r) { if (document.querySelector(".cv-sheet")?.dataset.run !== r) openRun(r); return; }
    if (!f) { S.sheetReq = (S.sheetReq || 0) + 1; document.querySelectorAll(".cv-sheet, .cv-scrim").forEach((e) => e.remove()); return; }
    if (open !== f) openSheet(f);
  };
  // A finger on the page: the refresh waits, so nothing moves under it.
  const touched = () => { S.touched = Date.now(); };
  host.addEventListener("pointerdown", touched, { passive: true });
  host.addEventListener("scroll", touched, { passive: true, capture: true });
  const onKey = (e) => { if (e.key === "Escape") { const x = document.querySelector(".cv-sheet .cv-x"); if (x) x.click(); } };
  window.addEventListener("hashchange", onHash);
  document.addEventListener("keydown", onKey);
  if (S.d && repo()) draw();
  load();
  S.view = view();
  S.timer = setInterval(() => {
    if (!repo()) { if (!document.hidden) load(); return; }
    if (document.hidden || document.querySelector(".cv-sheet") || document.querySelector(".cv input:focus, .cv textarea:focus, .cv select:focus") || S.ask?.state?.pending) return;
    if (S.dragging || Date.now() - (S.touched || 0) < 4000) return;
    load();
  }, 10000);
  return {
    update() {},
    unmount() {
      S.alive = false;
      clearInterval(S.timer);
      actStop(); workStop();
      (S.ros || []).forEach((r) => r.disconnect());
      window.removeEventListener("hashchange", onHash);
      document.removeEventListener("keydown", onKey);
      document.querySelectorAll(".cv-sheet, .cv-scrim").forEach((e) => e.remove());
    },
  };
}

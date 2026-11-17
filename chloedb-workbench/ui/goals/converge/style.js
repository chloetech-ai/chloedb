// Converge's styles: the page's tokens only, phone first, 44 px targets.
import { useStyle } from "/levels/common.js";

const CSS = `
.cv { padding: var(--s-4) 16px 96px; max-width: 1280px; margin: 0 auto; min-width: 0; }
.cv, .cv-sheet, .cv-toast, .cv-pop { --cv-s4: var(--crit); --cv-s3: var(--warn); --cv-s2: var(--accent); --cv-s1: var(--muted); --cv-k2: var(--good); --cv-k3: var(--warn); --cv-k4: var(--text-2); }
.cv-label { font-family: var(--mono); font-size: var(--fs-xs); letter-spacing: var(--track-label); text-transform: uppercase; color: var(--muted); }
.cv-sp { flex: 1; }
.cv-empty { color: var(--muted); font-size: var(--fs-sm); padding: 4px 0; }
.cv-intro { max-width: 70ch; color: var(--text-2); margin: 0 0 var(--s-4); }

/* Buttons */
.cv-btn { min-height: 44px; padding: 0 16px; border-radius: var(--r-sm); border: var(--hair) solid var(--line-2); background: var(--surface); color: var(--text); cursor: pointer; font: inherit; font-weight: var(--w-mid); display: inline-flex; align-items: center; justify-content: center; gap: 6px; white-space: nowrap; text-decoration: none; }
.cv-btn:hover { background: var(--surface-2); }
.cv-btn.pri { background: var(--accent); border-color: var(--accent); color: var(--accent-ink); }
.cv-btn.pri:hover { background: var(--accent-2); }
.cv-btn.good { background: var(--good); border-color: var(--good); color: #fff; }
.cv-parts { display: flex; flex-wrap: wrap; gap: 6px; margin: 4px 0; }
.cv-parts.sugg { flex-direction: column; flex-wrap: nowrap; }
.cv-part { display: inline-flex; flex-direction: column; align-items: flex-start; gap: 1px; max-width: 100%; min-height: 36px; padding: 4px 28px 4px 10px; position: relative; border: var(--hair) solid var(--line-2); border-radius: 8px; background: var(--surface); font-size: var(--fs-sm); text-align: left; color: var(--text); }
.cv-part b { font-weight: var(--w-mid); overflow-wrap: anywhere; }
.cv-part span { font-family: var(--mono); font-size: var(--fs-xs); color: var(--muted); overflow-wrap: anywhere; }
.cv-part span.warn { font-family: inherit; color: var(--warn, var(--crit)); }
.cv-part.pick { width: 100%; min-height: 44px; padding-right: 10px; cursor: pointer; font: inherit; }
.cv-part .x { position: absolute; top: 0; right: 0; width: 28px; height: 100%; border: 0; background: none; color: var(--muted); font-size: 18px; cursor: pointer; }
.cv-btn.crit { background: var(--crit); border-color: var(--crit); color: #fff; }
.cv-btn.quiet { border-color: transparent; background: none; color: var(--accent-text); }
.cv-btn.icon { width: 44px; padding: 0; font-size: 18px; }
.cv-btn[disabled] { opacity: .5; cursor: default; }
.cv-btn .n { font-family: var(--mono); font-size: var(--fs-xs); background: var(--surface); color: var(--crit-ink); border-radius: var(--r-pill); padding: 1px 7px; }
.cv-btn.you { background: var(--warn-soft); border-color: var(--warn-line); color: var(--warn-ink); }
.cv-btn.you .n { background: var(--warn); color: var(--on-warn); }

/* Top */
.cv-top { display: flex; align-items: center; gap: 8px; margin-bottom: var(--s-4); min-width: 0; }
.cv-ptitle { position: relative; display: flex; flex-direction: column; min-width: 0; flex: 1; margin: 0; font-size: var(--fs-lg); line-height: var(--lh-lg); font-weight: var(--w-title); letter-spacing: var(--track-md); }
.cv-ptitle .cv-brand { font-family: var(--mono); font-size: var(--fs-xs); line-height: var(--lh-xs); letter-spacing: var(--track-label); text-transform: uppercase; color: var(--muted); font-weight: var(--w-text); }
.cv-ptitle .cv-pname { display: inline-flex; align-items: center; gap: 6px; min-height: 32px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cv-ptitle .car { font-size: var(--fs-sm); color: var(--muted); }
.cv-psel { position: absolute; inset: 0; width: 100%; height: 100%; min-height: 44px; opacity: 0; cursor: pointer; font-size: 16px; }
.cv-ptitle:focus-within .cv-pname { outline: 2px solid var(--accent); outline-offset: 2px; border-radius: 4px; }
/* A phone: the title takes its own line, so the name is not cut and the buttons do not overlap it. */
@media (max-width: 480px) { .cv-top { flex-wrap: wrap; } .cv-ptitle { flex: 1 1 100%; } .cv-ptitle .cv-pname { white-space: normal; overflow-wrap: anywhere; } }
.cv-ptitle:has(.cv-psel:focus-visible) .cv-pname { outline: 2px solid var(--accent); outline-offset: 2px; border-radius: 4px; }
.cv-auto { display: flex; align-items: center; gap: 8px; min-height: 32px; margin: -8px 0 var(--s-4); font-size: var(--fs-sm); color: var(--text-2); text-decoration: none; }
.cv-auto .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--muted); flex: none; }
.cv-auto .dot.on { background: var(--good); }
.cv-auto .s { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }

/* A step screen's head */
.cv-sh { display: flex; flex-direction: column; gap: 4px; margin-bottom: var(--s-3); }
.cv-back { display: inline-flex; align-items: center; gap: 4px; min-height: 44px; color: var(--accent-text); text-decoration: none; font-size: var(--fs-sm); align-self: flex-start; margin: -8px 0 -4px -4px; padding: 0 4px; }
.cv-sh .row { display: flex; align-items: center; gap: 8px; min-width: 0; }
.cv-sh h1 { margin: 0; font-size: var(--fs-lg); line-height: var(--lh-lg); font-weight: var(--w-title); letter-spacing: var(--track-md); flex: 1; min-width: 0; }
.cv-sh h1 .c { font-family: var(--mono); color: var(--muted); font-weight: var(--w-text); font-size: var(--fs-md); margin-left: 6px; }
.cv-sh .hint { color: var(--text-2); font-size: var(--fs-sm); margin: 0; }
.cv-slots { font-size: var(--fs-sm); color: var(--text-2); display: flex; align-items: center; gap: 8px; }
.cv-slots .pips { display: inline-flex; gap: 3px; } .cv-slots .pips i { width: 14px; height: 6px; border-radius: 3px; background: var(--surface-3); } .cv-slots .pips i.on { background: var(--accent); }

/* Home: the pipeline */
.cv-pipe5 { display: flex; flex-direction: column; gap: 8px; }
.cv-step { display: grid; grid-template-columns: 32px minmax(0, 1fr) auto 12px; gap: 4px 12px; align-items: center; min-height: 72px; padding: 12px 14px; border-radius: var(--r); border: var(--hair) solid var(--line); background: var(--surface); color: var(--text); text-decoration: none; min-width: 0; }
.cv-step:hover { border-color: var(--accent-line); }
.cv-step .s-no { width: 28px; height: 28px; border-radius: 50%; display: grid; place-items: center; background: var(--surface-3); color: var(--text-2); font-family: var(--mono); font-size: var(--fs-sm); }
.cv-step.has .s-no { background: var(--accent-soft); color: var(--accent-text); }
.cv-step.you .s-no { background: var(--warn-soft); color: var(--warn-ink); }
.cv-step .s-tx { display: flex; flex-direction: column; min-width: 0; gap: 2px; }
.cv-step .s-nm { font-weight: var(--w-title); font-size: var(--fs-md); line-height: var(--lh-md); }
.cv-step .s-mv { font-size: var(--fs-sm); color: var(--text-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cv-step .s-ct { font-family: var(--mono); font-size: var(--fs-lg); line-height: 1; font-variant-numeric: tabular-nums; }
.cv-step .s-car { color: var(--muted); }
.cv-step .s-fl { grid-column: 2 / -1; display: flex; flex-direction: column; gap: 4px; min-width: 0; }
.cv-merged { display: flex; align-items: center; justify-content: space-between; gap: 8px; min-height: 48px; padding: 0 14px; margin-top: 8px; color: var(--good-ink); text-decoration: none; font-size: var(--fs-sm); border-radius: var(--r); border: var(--hair) dashed var(--line-2); }
.cv-cols { display: none; }
.cv-col { background: var(--sunken); border: var(--hair) solid var(--line); border-radius: var(--r); display: flex; flex-direction: column; min-width: 0; }
.cv-col-h { display: flex; flex-direction: column; gap: 2px; padding: 12px 12px 10px; text-decoration: none; color: var(--text); border-bottom: var(--hair) solid var(--line); min-height: 44px; }
.cv-col-h .t { display: flex; justify-content: space-between; align-items: baseline; font-weight: var(--w-title); }
.cv-col-h .t .c { font-family: var(--mono); color: var(--muted); font-weight: var(--w-text); }
.cv-col-h .g { font-size: var(--fs-xs); line-height: var(--lh-xs); color: var(--text-2); }
.cv-col-h:hover .t span:first-child { color: var(--accent-text); }
.cv-col .cards { padding: 8px; display: flex; flex-direction: column; gap: 8px; }
.cv-col .more { font-size: var(--fs-sm); color: var(--accent-text); text-decoration: none; min-height: 40px; display: flex; align-items: center; justify-content: center; }
@media (min-width: 1000px) {
  .cv-pipe5 { display: none; }
  .cv-cols { display: grid; grid-template-columns: repeat(5, minmax(0, 1fr)); gap: 10px; align-items: start; }
}

/* A card (home columns) and a row (step lists) */
.cv-card { background: var(--surface); border: var(--hair) solid var(--line); border-radius: 8px; padding: 10px; cursor: pointer; text-align: left; font: inherit; color: var(--text); display: flex; flex-direction: column; gap: 6px; width: 100%; min-width: 0; }
.cv-card:hover { border-color: var(--accent-line); }
.cv-card .ttl { font-weight: var(--w-mid); line-height: var(--lh-sm); overflow-wrap: anywhere; }
.cv-card.blocked { border-color: var(--crit); }
.cv-st { display: flex; gap: 0; align-items: baseline; min-width: 0; font-size: var(--fs-xs); line-height: var(--lh-xs); color: var(--text-2); }
.cv-st > span { white-space: nowrap; }
.cv-st > span + span::before { content: "·"; margin: 0 6px; color: var(--muted); }
.cv-st .sg { color: var(--text); font-weight: var(--w-mid); }
.cv-st .wh { overflow: hidden; text-overflow: ellipsis; min-width: 0; flex: 0 1 auto; }
.cv-st .tm { color: var(--muted); flex: none; }
.cv-st .wh.on { color: var(--accent-text); }
.cv-pulse { display: inline-block; width: 6px; height: 6px; border-radius: 50%; background: currentColor; margin-right: 5px; vertical-align: 1px; animation: cv-pulse 1.4s ease-in-out infinite; }
.cv-live { display: inline-flex; align-items: center; gap: 5px; }
.cv-live::before { content: ""; width: 6px; height: 6px; border-radius: 50%; background: currentColor; animation: cv-pulse 1.4s ease-in-out infinite; flex: none; }
@keyframes cv-pulse { 0%, 100% { opacity: 1; } 50% { opacity: .25; } }
@media (prefers-reduced-motion: reduce) { .cv-live::before, .cv-pulse { animation: none; } }
.cv-thin { display: block; height: 3px; border-radius: 2px; background: var(--surface-3); overflow: hidden; }
.cv-thin i { display: block; height: 100%; background: var(--accent); border-radius: 2px; }
.cv-thin.blk i { background: var(--crit); }
.cv-blkl { font-size: var(--fs-sm); line-height: var(--lh-sm); color: var(--crit-ink); overflow-wrap: anywhere; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
.cv-ql { font-size: var(--fs-xs); line-height: var(--lh-xs); color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cv-sev { width: 10px; height: 10px; border-radius: 50%; flex: none; background: var(--cv-s1); }
.cv-sev[data-sev="4"] { background: var(--cv-s4); } .cv-sev[data-sev="3"] { background: var(--cv-s3); } .cv-sev[data-sev="2"] { background: var(--cv-s2); }

.cv-list { list-style: none; margin: 0; padding: 0; border-top: var(--hair) solid var(--line); }
.cv-row { position: relative; border-bottom: var(--hair) solid var(--line); overflow: hidden; touch-action: pan-y; }
.cv-row .r-in { position: relative; z-index: 1; display: grid; grid-template-columns: 44px minmax(0, 1fr) auto; align-items: center; gap: 0 4px; min-height: 64px; background: var(--bg); transition: transform .18s ease; }
.cv-row.nosel .r-in { grid-template-columns: 12px minmax(0, 1fr) auto; }
.cv-row .r-ck { width: 44px; height: 44px; display: grid; place-items: center; cursor: pointer; margin: 0; }
.cv-row .r-ck input { width: 20px; height: 20px; accent-color: var(--accent); cursor: pointer; margin: 0; }
.cv-row .r-main { display: flex; flex-direction: column; gap: 3px; padding: 10px 4px 10px 0; min-width: 0; cursor: pointer; border: 0; background: none; text-align: left; font: inherit; color: var(--text); }
.cv-row .r-t1 { display: flex; align-items: center; gap: 8px; min-width: 0; }
.cv-row .r-ttl { font-weight: var(--w-mid); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
.cv-row .r-meta { display: flex; flex-wrap: wrap; gap: 0 10px; font-size: var(--fs-xs); line-height: var(--lh-xs); color: var(--text-2); min-width: 0; }
.cv-row .r-meta .r-new { color: var(--accent-text); font-weight: var(--w-mid); }
.cv-row .r-side { display: flex; flex-direction: column; align-items: flex-end; gap: 2px; padding-right: 4px; font-family: var(--mono); font-size: var(--fs-xs); color: var(--muted); white-space: nowrap; }
.cv-row .r-side .r-score { font-size: var(--fs-md); color: var(--text); }
.cv-row.sel .r-in { background: color-mix(in srgb, var(--accent) 9%, var(--bg)); }
.cv-row .r-sw { position: absolute; inset: 0; visibility: hidden; display: flex; align-items: center; justify-content: space-between; padding: 0 20px; font-weight: var(--w-mid); font-size: var(--fs-sm); }
.cv-row .r-sw .r { color: var(--accent-text); } .cv-row .r-sw .l { color: var(--text-2); }
.cv-row.swiping .r-sw { visibility: visible; }
.cv-row.go-r .r-sw { background: var(--accent-soft); } .cv-row.go-l .r-sw { background: var(--surface-3); }
.cv-row.blocked .r-in { box-shadow: inset 3px 0 0 var(--crit); }
.cv-sec { margin: var(--s-5) 0 var(--s-2); display: flex; align-items: baseline; gap: 8px; }
.cv-sec:first-child { margin-top: 0; }
.cv-sec .c { font-family: var(--mono); font-size: var(--fs-xs); color: var(--muted); }
.cv-none { padding: 28px 16px; text-align: center; color: var(--text-2); border: var(--hair) dashed var(--line-2); border-radius: var(--r); display: flex; flex-direction: column; align-items: center; gap: 12px; }
.cv-none p { margin: 0; max-width: 46ch; }

/* Filter chips: one row, each opens its choices */
.cv-chips { display: flex; gap: 8px; overflow-x: auto; margin: 0 -16px var(--s-3); padding: 2px 16px 4px; scrollbar-width: none; }
.cv-chips::-webkit-scrollbar { display: none; }
.cv-chip { flex: none; min-height: 36px; padding: 0 12px; border-radius: var(--r-pill); border: var(--hair) solid var(--line-2); background: var(--surface); color: var(--text-2); font: inherit; font-size: var(--fs-sm); display: inline-flex; align-items: center; gap: 6px; cursor: pointer; white-space: nowrap; }
.cv-chip.on { border-color: var(--accent-line); background: var(--accent-soft); color: var(--accent-text); }
.cv-chip .n { font-family: var(--mono); font-size: var(--fs-xs); }
.cv-chip .car { font-size: 10px; color: var(--muted); }
@media (max-width: 640px) { .cv-chip { min-height: 44px; } }
.cv-pop { position: fixed; z-index: 55; background: var(--surface); border: var(--hair) solid var(--line-2); box-shadow: var(--shadow-3); border-radius: var(--r); padding: 6px; display: flex; flex-direction: column; min-width: 220px; max-height: 60vh; overflow-y: auto; }
.cv-pop label { display: flex; align-items: center; gap: 10px; min-height: 44px; padding: 0 10px; border-radius: 6px; cursor: pointer; font-size: var(--fs-body); }
.cv-pop label:hover { background: var(--surface-2); }
.cv-pop input { width: 18px; height: 18px; accent-color: var(--accent); margin: 0; }
.cv-pop label .n { margin-left: auto; font-family: var(--mono); font-size: var(--fs-xs); color: var(--muted); }
.cv-pop .clr { border: 0; background: none; color: var(--accent-text); font: inherit; min-height: 44px; text-align: left; padding: 0 10px; cursor: pointer; }
@media (max-width: 640px) { .cv-pop { left: 0 !important; right: 0; bottom: 0; top: auto !important; border-radius: 14px 14px 0 0; padding: 8px 8px calc(8px + env(safe-area-inset-bottom, 0px)); } }
.cv-popscrim { position: fixed; inset: 0; z-index: 54; }

/* The sticky bar of a selection, and of a step's one action */
.cv-bulk { position: sticky; bottom: 0; z-index: 5; margin: 12px -16px 0; padding: 10px 16px calc(10px + env(safe-area-inset-bottom, 0px)); background: var(--surface); border-top: var(--hair) solid var(--line); box-shadow: 0 -6px 16px rgba(0,0,0,.06); display: flex; gap: 8px; align-items: center; }
.cv-bulk .n { flex: 1; font-size: var(--fs-sm); color: var(--text-2); min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
@media (min-width: 641px) { .cv-bulk { margin: 12px 0 0; border: var(--hair) solid var(--line); border-radius: var(--r); bottom: 12px; } }

/* Sheets */
.cv-scrim { position: fixed; inset: 0; background: var(--scrim); z-index: 40; }
.cv-sheet { position: fixed; top: 0; right: 0; bottom: 0; width: min(560px, 100%); background: var(--surface); box-shadow: var(--shadow-3); z-index: 41; display: flex; flex-direction: column; }
.cv-sheet:focus { outline: none; }
.cv-sheet-h { position: relative; padding: 14px 20px 12px; border-bottom: var(--hair) solid var(--line); display: flex; flex-direction: column; gap: 6px; }
.cv-sheet-h > .cv-meta, .cv-sheet-h > .cv-label { margin-right: 96px; min-height: 28px; }
.cv-sheet-h h2 { margin: 0; font-size: var(--fs-md); line-height: var(--lh-md); font-weight: var(--w-title); overflow-wrap: anywhere; }
.cv-sheet-h .tools { position: absolute; top: 6px; right: 6px; display: flex; gap: 2px; }
.cv-sheet-h .tools .cv-btn { color: var(--text-2); }
.cv-sheet-b { padding: 16px 20px 20px; overflow-y: auto; display: flex; flex-direction: column; gap: 20px; min-height: 0; flex: 1; overscroll-behavior: contain; }
.cv-sheet-f { padding: 10px 20px calc(10px + env(safe-area-inset-bottom, 0px)); border-top: var(--hair) solid var(--line); display: flex; flex-direction: column; gap: 8px; background: var(--surface); }
.cv-sheet-f .acts { display: flex; gap: 8px; }
.cv-sheet-f .acts .cv-btn { flex: 1; }
.cv-sheet-f .acts:empty { display: none; }
.cv-sheet section > .cv-label { display: block; margin-bottom: 6px; }
.cv-sheet p { margin: 0; }
.cv-meta { display: flex; flex-wrap: wrap; gap: 4px 10px; align-items: center; font-size: var(--fs-xs); color: var(--text-2); }
.cv-tag { display: inline-flex; align-items: center; min-height: 20px; padding: 1px 7px; border-radius: 4px; background: var(--surface-3); font-family: var(--mono); font-size: var(--fs-xs); color: var(--text-2); }
.cv-tag.s4 { background: var(--crit-soft); color: var(--crit-ink); } .cv-tag.s3 { background: var(--warn-soft); color: var(--warn-ink); }
.cv-tag.ok { background: var(--good-soft); color: var(--good-ink); } .cv-tag.bad { background: var(--crit-soft); color: var(--crit-ink); }
.cv-tag.run { background: var(--accent-soft); color: var(--accent-text); }
.cv-kv { display: grid; grid-template-columns: 120px minmax(0, 1fr); gap: 8px 12px; font-size: var(--fs-body); margin: 0; }
.cv-kv dt { color: var(--muted); font-size: var(--fs-sm); } .cv-kv dd { margin: 0; min-width: 0; overflow-wrap: anywhere; }
@media (max-width: 640px) { .cv-kv { grid-template-columns: 1fr; gap: 0; } .cv-kv dd { margin-bottom: 10px; } }
.cv-pre { margin: 0; background: var(--sunken); border: var(--hair) solid var(--line); border-radius: 8px; padding: 10px 12px; font-family: var(--mono); font-size: var(--fs-xs); line-height: var(--lh-xs); overflow-x: auto; white-space: pre; max-height: 260px; }
.cv-ta { width: 100%; box-sizing: border-box; min-height: 110px; font-family: var(--mono); font-size: var(--fs-xs); background: var(--sunken); color: var(--text); border: var(--hair) solid var(--line-2); border-radius: 8px; padding: 8px; }
.cv-fold > summary { cursor: pointer; min-height: 44px; display: flex; align-items: center; gap: 8px; list-style: none; font-weight: var(--w-mid); }
.cv-fold > summary::-webkit-details-marker { display: none; }
.cv-fold > summary::before { content: "▸"; color: var(--muted); font-size: 11px; transition: transform .15s; }
.cv-fold[open] > summary::before { transform: rotate(90deg); }
.cv-fold > summary .s { color: var(--text-2); font-weight: var(--w-text); font-size: var(--fs-sm); }
.cv-checks { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 2px; }
.cv-checks li { display: grid; grid-template-columns: 22px minmax(0, 1fr) auto; gap: 8px; align-items: start; min-height: 32px; padding: 5px 0; font-size: var(--fs-body); }
.cv-checks li .du { font-family: var(--mono); font-size: var(--fs-xs); color: var(--muted); white-space: nowrap; }
.cv-checks li .why { display: block; color: var(--crit-ink); font-size: var(--fs-sm); margin-top: 2px; overflow-wrap: anywhere; }
.cv .mk, .cv-sheet .mk { width: 20px; height: 20px; border-radius: 50%; display: grid; place-items: center; font-size: 11px; color: #fff; background: var(--good); flex: none; }
.cv .mk.run, .cv-sheet .mk.run { background: var(--accent-soft); color: var(--accent-text); }
.cv .mk.fail, .cv-sheet .mk.fail { background: var(--crit); }
.cv .mk.todo, .cv-sheet .mk.todo { background: var(--surface-3); color: var(--muted); }
.cv-dots { display: flex; gap: 6px; flex-wrap: wrap; }
.cv-dots button { width: 44px; height: 44px; border: 0; background: none; padding: 0; display: grid; place-items: center; cursor: pointer; }
.cv-dots button i { width: 16px; height: 16px; border-radius: 50%; background: var(--surface-3); }
.cv-dots button.p i { background: var(--good); } .cv-dots button.f i { background: var(--crit); }
.cv-dots button[aria-pressed="true"] i { box-shadow: 0 0 0 3px var(--accent-line); }
.cv-verdict { display: flex; gap: 10px; align-items: flex-start; padding: 12px; border-radius: 10px; background: var(--good-soft); color: var(--good-ink); font-weight: var(--w-mid); }
.cv-verdict.wait { background: var(--surface-2); color: var(--text); }
.cv-verdict.bad { background: var(--crit-soft); color: var(--crit-ink); }
.cv-note { background: var(--surface-2); border-radius: 8px; padding: 10px 12px; font-size: var(--fs-sm); color: var(--text-2); }
.cv-bsec { border: var(--hair) solid var(--crit); background: var(--crit-soft); border-radius: 10px; padding: 12px; display: flex; flex-direction: column; gap: 8px; }
.cv-bsec .hd { font-weight: var(--w-title); color: var(--crit-ink); }
.cv-bsec p { color: var(--text); overflow-wrap: anywhere; }
.cv-bsec .do { display: flex; gap: 8px; align-items: stretch; min-width: 0; }
.cv-bsec .do code { flex: 1; min-width: 0; display: block; font-family: var(--mono); font-size: var(--fs-xs); background: var(--surface); color: var(--text); border: var(--hair) solid var(--line-2); border-radius: 6px; padding: 10px; white-space: pre-wrap; overflow-wrap: anywhere; }
.cv-bsec .by { font-size: var(--fs-xs); color: var(--text-2); }
.cv-menu { position: fixed; z-index: 56; background: var(--surface); border: var(--hair) solid var(--line-2); box-shadow: var(--shadow-3); border-radius: var(--r); padding: 6px; display: flex; flex-direction: column; min-width: 220px; }
.cv-menu a, .cv-menu button { display: flex; align-items: center; min-height: 44px; padding: 0 12px; border-radius: 6px; color: var(--text); text-decoration: none; border: 0; background: none; font: inherit; text-align: left; cursor: pointer; }
.cv-menu a:hover, .cv-menu button:hover { background: var(--surface-2); }
.cv-menu hr { border: 0; border-top: var(--hair) solid var(--line); margin: 4px 0; }
.cv-menu .danger { color: var(--crit-ink); }
.cv-edit { display: grid; grid-template-columns: repeat(auto-fit, minmax(140px, 1fr)); gap: 10px; }
.cv-f { display: flex; flex-direction: column; gap: 4px; margin-bottom: 12px; min-width: 0; }
.cv-f label { font-weight: var(--w-mid); font-size: var(--fs-sm); }
.cv-f .hint { font-size: var(--fs-sm); color: var(--text-2); }
.cv-f select, .cv-f input[type=text], .cv-f input[type=number], .cv-in { min-height: 44px; border: var(--hair) solid var(--line-2); border-radius: var(--r-sm); background: var(--surface); color: var(--text); padding: 0 10px; font: inherit; font-size: 16px; box-sizing: border-box; width: 100%; min-width: 0; }
.cv-f textarea.cv-ta { font-family: var(--sans); font-size: 16px; }

.cv-panels { display: grid; grid-template-columns: repeat(auto-fit, minmax(340px, 1fr)); gap: 12px; }
.cv-panel { background: var(--surface); border: var(--hair) solid var(--line); border-radius: var(--r); padding: 14px; min-width: 0; }
.cv-panel h3 { margin: 0 0 2px; font-size: var(--fs-body); }
.cv-panel .sub { color: var(--text-2); font-size: var(--fs-sm); margin-bottom: 10px; }
.cv-bar { display: flex; gap: var(--s-2); align-items: center; flex-wrap: wrap; margin-bottom: var(--s-3); }
.cv-f label .cv-cb, .cv-cb { width: 20px; height: 20px; accent-color: var(--accent); vertical-align: -4px; margin-right: 6px; }
.cv-f input[type=text], .cv-f input[type=number] { max-width: 420px; }
@media (max-width: 640px) { .cv-panels { grid-template-columns: 1fr; } .cv-f input[type=text], .cv-f input[type=number] { max-width: none; } }
/* Talk */
.cv-talk { display: flex; flex-direction: column; gap: 10px; }
.cv-thread { display: flex; flex-direction: column; gap: 8px; }
.cv-msg { max-width: 88%; padding: 8px 12px; border-radius: 14px; line-height: var(--lh-sm); white-space: pre-wrap; overflow-wrap: anywhere; font-size: var(--fs-body); }
.cv-msg .m-w { display: block; font-size: var(--fs-xs); color: var(--muted); margin-bottom: 2px; }
.cv-msg.m-you { align-self: flex-end; background: var(--accent); color: var(--accent-ink); border-bottom-right-radius: 4px; }
.cv-msg.m-you .m-w { color: var(--accent-ink); opacity: .75; }
.cv-msg.m-ag { align-self: flex-start; background: var(--surface-2); border-bottom-left-radius: 4px; }
.cv-msg.m-pend { opacity: .6; }
.cv-agent { font-size: var(--fs-xs); color: var(--text-2); display: flex; gap: 6px; align-items: center; min-width: 0; }
.cv-agent .s { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cv-agent.on { color: var(--accent-text); }
.cv-tbox { display: flex; gap: 8px; align-items: flex-end; }
.cv-tbox textarea { flex: 1; min-width: 0; min-height: 44px; max-height: 140px; resize: none; font: inherit; font-size: 16px; line-height: 1.35; padding: 11px 12px; border: var(--hair) solid var(--line-2); border-radius: 22px; background: var(--surface-2); color: var(--text); box-sizing: border-box; }
.cv-tbox textarea:focus { outline: 2px solid var(--focus); border-color: var(--accent-line); background: var(--surface); }
.cv-tbox .cv-btn { border-radius: 22px; }
.cv-tnote { font-size: var(--fs-xs); color: var(--muted); }

.cv-tbox[hidden] { display: none; }
/* Toast */
.cv-toast { position: fixed; bottom: calc(20px + env(safe-area-inset-bottom, 0px)); left: 50%; transform: translateX(-50%); background: var(--text); color: var(--bg); padding: 6px 8px 6px 14px; min-height: 44px; box-sizing: border-box; border-radius: 10px; font-size: var(--fs-sm); z-index: 70; display: flex; align-items: center; gap: 12px; max-width: calc(100vw - 32px); }
.cv-toast-u { min-height: 36px; padding: 0 12px; border: 0; border-radius: 6px; background: transparent; color: var(--accent-soft); font: inherit; font-weight: var(--w-title); cursor: pointer; color: inherit; text-decoration: underline; }

/* Projects */
.cv-pjs { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 12px; }
.cv-pj { display: flex; flex-direction: column; gap: 6px; padding: 16px; min-height: 96px; border-radius: var(--r); border: var(--hair) solid var(--line); background: var(--surface); color: var(--text); text-decoration: none; min-width: 0; }
.cv-pj:hover { border-color: var(--accent-line); }
.cv-pj .n { font-size: var(--fs-md); font-weight: var(--w-title); }
.cv-pj .p { font-family: var(--mono); font-size: var(--fs-xs); color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cv-pj.add { border-style: dashed; justify-content: center; }

@media (max-width: 640px) {
  .cv { padding-top: var(--s-3); }
  .cv-sheet-h { padding: 12px 16px 10px; }
  .cv-sheet-b { padding: 14px 16px 18px; }
  .cv-sheet-f { padding: 8px 16px calc(8px + env(safe-area-inset-bottom, 0px)); }
}
`;

export function styles() { useStyle("converge-v2", CSS); }

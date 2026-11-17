// The internet's backbone on ChloeDB: the page, in ChloeDB's command center
// (#/app/backbone, loaded through chloedb-server from this app) and on its
// own (the app's / when it has no token, through ui/index.html). It imports
// nothing and builds its own elements, so the same file serves both. Its
// look is NetOps' (chloedb-netops/ui/view.js): a worklist, a map that zooms
// from the world to one network, an inspector, the stack of a country.
//
//   (now)                 what is wrong now, how it connects, what ended
//   map                   the world: countries red where something is wrong
//   map/<CC>[/<asn>]      a country: its exchanges, networks and probes;
//                         a network chosen in the inspector beside it
//   as/<asn>              a network: what it hangs from, its prefixes,
//                         its changes and faults
//   prefix/<p>            a prefix: who announces it, through whom, its
//                         ROAs, every update replayed into a timeline
//   incidents             the last day's incidents
//   incident/<id>         one incident: its alerts, graph and minutes
//   diagnose[/<CC>]       each country's stack, from its users down to K
//   rules                 the alert rules, each a SQL statement
//   data                  how it is built, and how big it is
//   search/<q>            a prefix, an address, a network or an exchange
//
// Every answer of the API carries the SQL it ran: the "SQL" button shows
// the queries behind the page in view.

export default { render };

const CSS = `/* ---- NetOps (chloedb-netops/ui/view.js CSS) ---- */

.bn-head { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; margin-bottom: 10px; }
.bn-head h1 { margin: 0; font-size: 20px; letter-spacing: -.01em; }
.bn-head .sub { color: var(--muted); font-size: 12.5px; }
.bn-head .tools { margin-left: auto; display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
.bn-hmenu { display: none; }
@media (max-width: 600px) {
  .bn-head { flex-wrap: nowrap; gap: 8px; margin-bottom: 8px; }
  .bn-head h1 { font-size: 17px; white-space: nowrap; }
  .bn-head .sub { display: none; }
  .bn-head .tools { flex-wrap: nowrap; }
  .bn-head .tools > :not(.bn-hmenu) { display: none; }
  .bn-head .bn-hmenu { display: inline-flex; }
  .bn-root .tabs { margin-bottom: 10px; }
}
.bn-root .tabs .bn-more { margin-left: auto; position: sticky; right: 0; background: var(--bg, var(--surface)); }
.bn-root .tabs .bn-more .i { width: 14px; height: 14px; }
.bn-synth { font-size: 11px; padding: 1px 7px; border-radius: 999px; border: 1px dashed var(--line-2); color: var(--text-2); }
.bn-found { border-style: solid; border-color: var(--accent); color: var(--accent); text-decoration: none; white-space: nowrap; }
.bn-found:hover { background: var(--accent-soft); }
.bn-feed { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 10px; }
.bn-feed > div { border: 1px solid var(--line); border-radius: var(--r-sm); padding: 10px 12px; background: var(--surface); min-width: 0; cursor: pointer; }
.bn-feed > div:hover { border-color: var(--line-2); }
.bn-feed .t { display: flex; justify-content: space-between; gap: 8px; align-items: baseline; }
.bn-feed .t b { font-size: 13px; }
.bn-feed .n { font-variant-numeric: tabular-nums; font-weight: 600; }
.bn-feed pre { margin: 6px 0 0; font-size: 11px; line-height: 1.45; max-height: 92px; overflow: auto; white-space: pre; background: var(--surface-2); border-radius: var(--r-sm); padding: 6px 8px; font-family: var(--mono); }
.bn-rules { margin: 0; padding-left: 18px; display: grid; gap: 4px; }
.bn-src { display: inline-block; font-size: 10.5px; padding: 0 6px; border-radius: 999px; border: 1px solid var(--line-2); color: var(--text-2); margin-right: 3px; white-space: nowrap; }
.bn-sim { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; color: var(--text-2); }
.bn-kpis { display: grid; grid-template-columns: repeat(auto-fit, minmax(128px, 1fr)); gap: 12px; margin-bottom: 14px; }
.bn-kpis .card { margin: 0; }
.bn-kpis .val { font-size: 22px; white-space: nowrap; }
.bn-kpis .lbl { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.bn-kpi-sub { font-size: 12px; color: var(--muted); margin-top: 2px; }
.bn-cols { display: grid; grid-template-columns: minmax(0, 2fr) minmax(0, 1fr); gap: 14px; align-items: start; }
.bn-cols.even { grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); }
.bn-cols.wide { grid-template-columns: minmax(0, 5fr) minmax(0, 2fr); }
.bn-col { display: flex; flex-direction: column; gap: 14px; min-width: 0; }
.bn-hero { border: 1px solid color-mix(in srgb, var(--crit) 45%, var(--line)); background: linear-gradient(90deg, var(--crit-soft), var(--surface) 70%); border-radius: var(--r); padding: 14px 16px; margin-bottom: 14px; display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 14px; align-items: center; }
.bn-hero.calm { border-color: color-mix(in srgb, var(--good) 35%, var(--line)); background: linear-gradient(90deg, var(--good-soft), var(--surface) 60%); }
.bn-hero h2 { margin: 0 0 4px; font-size: 17px; }
.bn-hero p { margin: 0; color: var(--text-2); max-width: 90ch; }
.bn-hero .meta { display: flex; gap: 6px 14px; flex-wrap: wrap; margin-top: 8px; font-size: 12.5px; color: var(--text-2); }
.bn-map { position: relative; }
.bn-map svg { display: block; width: 100%; height: auto; }
.bn-map .pod { fill: var(--surface-2); stroke: var(--line-2); }
.bn-map .pod.hot { stroke: var(--crit); stroke-width: 1.6; }
.bn-map .site { font-size: 12px; font-weight: 650; fill: var(--text); }
.bn-map .site-sub { font-size: 10px; fill: var(--muted); }
.bn-map .dev { stroke: var(--surface); stroke-width: 1.5; cursor: pointer; }
.bn-map .dev-lbl { font-size: 8.5px; fill: var(--muted); pointer-events: none; }
.bn-map .lnk { fill: none; stroke-linecap: round; }
.bn-map .hit { stroke: transparent; stroke-width: 12; fill: none; cursor: pointer; }
.bn-map .dimmed { opacity: .22; }
.bn-map .halo { fill: none; stroke: var(--crit); stroke-width: 2; opacity: .9; }
.bn-map .halo.pulse { animation: bn-pulse 1.6s ease-out infinite; transform-box: fill-box; transform-origin: center; }
.bn-map .root { fill: none; stroke: var(--accent); stroke-width: 2.5; stroke-dasharray: 3 3; }
.bn-map .pathline { fill: none; stroke: var(--accent); stroke-width: 5; stroke-linecap: round; stroke-linejoin: round; opacity: .55; }
@keyframes bn-pulse { 0% { transform: scale(1); opacity: .95; } 100% { transform: scale(2.1); opacity: 0; } }
.bn-legend { display: flex; gap: 6px 14px; flex-wrap: wrap; font-size: 11.5px; color: var(--text-2); padding: 8px 2px 0; align-items: center; }
.bn-legend i { display: inline-block; width: 18px; height: 4px; border-radius: 2px; vertical-align: middle; margin-right: 5px; }
.bn-legend i.d { width: 9px; height: 9px; border-radius: 50%; }
.bn-list { list-style: none; margin: 0; padding: 0; }
.bn-list > li { padding: 8px 0; border-top: 1px solid var(--line); }
.bn-list > li:first-child { border-top: 0; padding-top: 0; }
.bn-list > li:last-child { padding-bottom: 0; }
.bn-tl { list-style: none; margin: 0; padding: 0 0 0 14px; border-left: 2px solid var(--line); }
.bn-tl li { position: relative; padding: 0 0 12px 12px; font-size: 13px; }
.bn-tl li:last-child { padding-bottom: 0; }
.bn-tl li::before { content: ""; position: absolute; left: -20px; top: 4px; width: 10px; height: 10px; border-radius: 50%; background: var(--surface); border: 2px solid var(--muted); }
.bn-tl li.alert::before { border-color: var(--crit); } .bn-tl li.agent::before { border-color: var(--s7); background: var(--s7); }
.bn-tl li.action::before { border-color: var(--good); background: var(--good); } .bn-tl li.status::before { border-color: var(--accent); }
.bn-tl .when { font-size: 11.5px; color: var(--muted); }
.bn-step { border-top: 1px solid var(--line); padding: 9px 0; }
.bn-step:first-child { border-top: 0; }
.bn-step .top { display: grid; grid-template-columns: 22px 54px minmax(0, 1fr) auto; gap: 8px; align-items: baseline; cursor: pointer; }
.bn-step .n { color: var(--muted); font-variant-numeric: tabular-nums; font-size: 12px; text-align: right; }
.bn-step .find { color: var(--text-2); font-size: 12.5px; margin: 3px 0 0 92px; overflow-wrap: anywhere; }
.bn-step pre.code { margin: 8px 0 0 92px; white-space: pre-wrap; overflow-wrap: anywhere; font-size: 12px; }
.bn-tool { font-size: 10.5px; font-weight: 650; text-transform: uppercase; letter-spacing: .04em; padding: 1px 6px; border-radius: 5px; background: var(--surface-3); color: var(--text-2); text-align: center; }
.bn-tool.graph { background: color-mix(in srgb, var(--s7) 18%, var(--surface)); color: var(--s7); }
.bn-tool.pack { background: color-mix(in srgb, var(--s3) 18%, var(--surface)); color: var(--s3); }
.bn-tool.reason { background: var(--accent-soft); color: var(--accent); }
.bn-act { border: 1px solid var(--line); border-radius: var(--r-sm); padding: 10px 12px; background: var(--surface); }
.bn-act + .bn-act { margin-top: 10px; }
.bn-act.done { background: var(--surface-2); }
.bn-act pre.code { margin: 8px 0 0; font-size: 12px; white-space: pre-wrap; overflow-wrap: anywhere; }
.bn-conf { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 10px; align-items: center; }
.bn-conf .meter { height: 10px; }
.bn-cause { font-size: 14.5px; line-height: 1.5; }
.bn-why { margin: 8px 0 0; padding-left: 18px; color: var(--text-2); font-size: 13px; }
.bn-story { margin: 4px 0 0; font-size: 13.5px; line-height: 1.6; }
.bn-proofs { margin: 4px 0 0; padding: 0; list-style: none; display: grid; gap: 8px; }
.bn-proof { display: grid; grid-template-columns: 74px minmax(0, 1fr); gap: 2px 10px; font-size: 13px; line-height: 1.45; }
.bn-proof > .k { grid-row: span 3; font-size: 10.5px; text-transform: uppercase; letter-spacing: .04em; color: var(--text-2); border: 1px solid var(--line-2); border-radius: 999px; padding: 1px 6px; height: fit-content; text-align: center; white-space: nowrap; }
.bn-proof.absent > .k { border-style: dashed; }
.bn-proof .when { color: var(--muted); font-size: 12px; white-space: nowrap; }
.bn-proof .refs { display: flex; flex-wrap: wrap; gap: 4px 6px; align-items: center; }
.bn-quote { display: block; font-family: var(--mono); font-size: 11.5px; background: var(--surface-2); border-radius: var(--r-sm); padding: 4px 8px; overflow-x: auto; white-space: pre; color: var(--text-2); }
.bn-ref { font-family: var(--mono); font-size: 11.5px; padding: 0 6px; border-radius: 999px; border: 1px solid var(--line-2); color: var(--accent); text-decoration: none; cursor: pointer; white-space: nowrap; }
.bn-ref:hover { background: var(--accent-soft); }
.bn-ref.plain { color: var(--text-2); cursor: default; }
.bn-ref.plain:hover { background: none; }
.bn-against { margin-top: 12px; }
.bn-against > summary { cursor: pointer; font-size: 12.5px; color: var(--text-2); font-weight: 600; }
.bn-against .bn-proof { color: var(--text-2); }
.bn-key { margin-top: 10px; padding: 8px 10px; border-radius: var(--r-sm); border: 1px dashed var(--line-2); font-size: 12.5px; }
.bn-key.ok { border-color: var(--good); background: var(--good-soft); } .bn-key.no { border-color: var(--crit); background: var(--crit-soft); }
.bn-budget { display: flex; height: 14px; border-radius: 7px; overflow: hidden; background: var(--surface-3); }
.bn-budget i { display: block; height: 100%; }
.bn-rcpt { font-size: 12px; }
.bn-rcpt .skip td { color: var(--muted); }
.bn-log { font-family: var(--mono); font-size: 11.5px; line-height: 1.55; }
.bn-log .ln { display: grid; grid-template-columns: 62px 78px minmax(0, 1fr); gap: 8px; padding: 2px 0; border-top: 1px solid var(--line); }
.bn-log .ln:first-child { border-top: 0; }
.bn-log .ln .m { overflow-wrap: anywhere; white-space: pre-wrap; }
.bn-log .s0, .bn-log .s1, .bn-log .s2, .bn-log .s3 { color: var(--crit-ink); } .bn-log .s4 { color: color-mix(in srgb, var(--warn) 60%, var(--text)); }
.bn-log mark { background: color-mix(in srgb, var(--warn) 45%, transparent); color: inherit; border-radius: 2px; }
.bn-days { display: grid; grid-auto-flow: column; grid-auto-columns: minmax(190px, 1fr); gap: 10px; overflow-x: auto; padding-bottom: 6px; }
.bn-day { border: 1px solid var(--line); border-radius: var(--r-sm); background: var(--surface); min-height: 120px; }
.bn-day.today { border-color: var(--accent); }
.bn-day > .dh { padding: 6px 10px; font-size: 12px; font-weight: 600; border-bottom: 1px solid var(--line); display: flex; justify-content: space-between; color: var(--text-2); }
.bn-day > .db { padding: 8px; display: flex; flex-direction: column; gap: 6px; }
.bn-chg { border-left: 3px solid var(--seq-4); background: var(--surface-2); border-radius: 5px; padding: 6px 8px; font-size: 12px; cursor: pointer; }
.bn-chg:hover { background: var(--surface-3); }
.bn-chg.high { border-left-color: var(--crit); } .bn-chg.medium { border-left-color: var(--warn); } .bn-chg.low { border-left-color: var(--good); }
.bn-chg.clash { box-shadow: inset 0 0 0 1px var(--crit); }
.bn-chg.past { opacity: .72; }
.bn-chg .t { font-weight: 600; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
.bn-blame { font-family: var(--mono); font-size: 11.5px; line-height: 1.6; overflow: auto; max-height: 520px; border: 1px solid var(--line); border-radius: var(--r-sm); }
.bn-blame .bl { display: grid; grid-template-columns: 150px 34px minmax(0, 1fr); gap: 0; }
.bn-blame .bl > span { padding: 0 8px; white-space: pre; }
.bn-blame .who { color: var(--muted); border-right: 1px solid var(--line); overflow: hidden; text-overflow: ellipsis; }
.bn-blame .ln { color: var(--muted); text-align: right; }
.bn-blame .bl.new { background: color-mix(in srgb, var(--good) 14%, transparent); }
.bn-blame .bl.chg .who { color: var(--accent); }
.bn-score { font-size: 34px; font-weight: 680; letter-spacing: -.02em; line-height: 1; }
.bn-verdict { font-size: 13px; font-weight: 650; text-transform: uppercase; letter-spacing: .05em; }
.bn-agent { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 10px; }
.bn-agent h4 { margin: 0 0 4px; font-size: 14px; }
.bn-facts { display: flex; gap: 4px 16px; flex-wrap: wrap; font-size: 12px; color: var(--text-2); margin-top: 8px; }
.bn-facts b { color: var(--text); font-weight: 600; }
.bn-q { border-top: 1px solid var(--line); padding: 10px 0; }
.bn-q:first-child { border-top: 0; padding-top: 0; }
.bn-q pre.code { margin: 6px 0 0; white-space: pre-wrap; overflow-wrap: anywhere; font-size: 12px; }
.bn-setup { max-width: 560px; margin: 60px auto; text-align: center; }
.bn-setup .bar { height: 8px; background: var(--surface-3); border-radius: 4px; overflow: hidden; margin: 14px 0 8px; }
.bn-setup .bar i { display: block; height: 100%; background: var(--accent); transition: width .6s; }
.bn-search { display: grid; grid-template-columns: minmax(0, 1fr) auto auto auto; gap: 8px; margin-bottom: 12px; align-items: center; }
.bn-search .input { font-size: 14px; padding: 8px 11px; }
.bn-hit { padding: 8px 0; border-top: 1px solid var(--line); }
.bn-hit:first-child { border-top: 0; }
.bn-hit .x { color: var(--text-2); font-size: 12.5px; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
/* ---- the box above every page, its completions, the assistant (assist.rs, cmdBar, assistant) */
.bn-cmdrow { display: flex; gap: 8px; align-items: center; margin: 0 0 10px; position: relative; z-index: 6; }
.bn-cmd { position: relative; flex: 1; min-width: 0; display: flex; align-items: center; gap: 0; border: 1px solid var(--line-2); border-radius: 10px; background: var(--surface); box-shadow: var(--shadow-1); height: 40px; }
.bn-cmd:focus-within { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
.bn-cmd.compact { height: 38px; box-shadow: none; }
.bn-cmd-mode { display: flex; gap: 2px; padding: 3px; margin-left: 3px; background: var(--surface-2); border-radius: 7px; flex: none; }
.bn-cmd-mode button { border: 0; background: none; color: var(--text-2); font: inherit; font-size: 12.5px; padding: 4px 9px; border-radius: 5px; cursor: pointer; display: inline-flex; align-items: center; gap: 5px; }
.bn-cmd-mode button svg { width: 14px; height: 14px; }
.bn-cmd-mode button[aria-selected="true"] { background: var(--surface); color: var(--text); box-shadow: var(--shadow-1); font-weight: 600; }
.bn-scope { flex: none; display: inline-flex; align-items: center; gap: 5px; margin: 0 2px 0 8px; padding: 3px 9px; border-radius: 999px; border: 1px solid var(--line-2); background: var(--surface-2); color: var(--text-2); font: inherit; font-size: 12px; cursor: pointer; max-width: 230px; }
.bn-scope span:not(.x) { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.bn-scope svg { width: 13px; height: 13px; flex: none; }
.bn-scope.on { background: var(--accent-soft); border-color: color-mix(in srgb, var(--accent) 45%, transparent); color: var(--accent); }
.bn-scope .x { opacity: .7; margin-left: 2px; }
.bn-cmd-in { flex: 1; min-width: 0; border: 0; outline: 0; background: transparent; color: var(--text); font: inherit; font-size: 14px; padding: 0 12px 0 8px; height: 100%; }
.bn-sug { position: absolute; left: 0; right: 0; top: calc(100% + 6px); background: var(--surface); border: 1px solid var(--line-2); border-radius: 10px; box-shadow: var(--shadow-3); overflow: hidden; z-index: 70; max-height: min(460px, 70vh); display: flex; flex-direction: column; }
.bn-sug[hidden] { display: none; }
.bn-sug-i { display: grid; grid-template-columns: 26px minmax(0, 1fr) auto; gap: 10px; align-items: center; padding: 7px 12px; cursor: pointer; border-top: 1px solid var(--line); }
.bn-sug-i:first-child { border-top: 0; }
.bn-sug-i.on, .bn-sug-i:hover { background: var(--accent-soft); }
.bn-sug-i .k { width: 26px; height: 26px; border-radius: 7px; display: inline-flex; align-items: center; justify-content: center; background: var(--surface-2); color: var(--text-2); }
.bn-sug-i .k svg { width: 14px; height: 14px; }
.bn-sug-i .k.object, .bn-sug-i .k.circuit { color: var(--s1); } .bn-sug-i .k.question { color: var(--accent); } .bn-sug-i .k.history { color: var(--muted); } .bn-sug-i .k.customer { color: var(--s5); } .bn-sug-i .k.place { color: var(--s3); }
.bn-sug-i .t { display: flex; flex-direction: column; min-width: 0; }
.bn-sug-i .l { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-size: 13.5px; }
.bn-sug-i .l b { font-weight: 700; }
.bn-sug-i .s:empty { display: none; }
.bn-sug-i .s { font-size: 11.5px; color: var(--muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.bn-sug-i kbd, .bn-sug-ft kbd { font: 10.5px var(--mono); padding: 1px 5px; border: 1px solid var(--line-2); border-bottom-width: 2px; border-radius: 4px; color: var(--text-2); background: var(--surface-2); }
.bn-sug-none { padding: 10px 12px; color: var(--muted); font-size: 12.5px; }
.bn-sug-ft { display: flex; gap: 10px; align-items: center; padding: 6px 12px; border-top: 1px solid var(--line); background: var(--surface-2); font-size: 11.5px; color: var(--muted); }
.bn-sug-ft .grow { flex: 1; }
.bn-ag-btn { height: 40px; flex: none; }
.bn-root.bn-ag-open { padding-right: min(420px, 40vw); }
.bn-ag { position: fixed; top: 52px; right: 0; bottom: 0; width: min(420px, 40vw); background: var(--surface); border-left: 1px solid var(--line); box-shadow: var(--shadow-3); z-index: 30; display: flex; flex-direction: column; animation: slide .2s cubic-bezier(.2, .8, .2, 1); }
.bn-ag[hidden] { display: none; }
.bn-ag > .hd { display: flex; align-items: center; gap: 10px; padding: 12px 14px; border-bottom: 1px solid var(--line); }
.bn-ag > .hd > svg { width: 18px; height: 18px; color: var(--accent); }
.bn-ag > .hd h2 { margin: 0; font-size: 15px; }
.bn-ag-ctx { display: flex; gap: 6px; align-items: center; flex-wrap: wrap; padding: 8px 14px; border-bottom: 1px solid var(--line); font-size: 12.5px; background: var(--surface-2); }
.bn-ag-tabs { display: flex; gap: 2px; padding: 0 10px; border-bottom: 1px solid var(--line); }
.bn-ag-tabs button { border: 0; background: none; padding: 8px 10px; color: var(--muted); font: inherit; font-size: 12.5px; cursor: pointer; border-bottom: 2px solid transparent; }
.bn-ag-tabs button[aria-selected="true"] { color: var(--text); border-bottom-color: var(--accent); font-weight: 600; }
.bn-ag-bd { flex: 1; overflow: auto; padding: 12px 14px; display: flex; flex-direction: column; gap: 12px; }
.bn-ag-log { display: flex; flex-direction: column; gap: 10px; }
.bn-ag-q { align-self: flex-end; max-width: 88%; display: flex; gap: 6px; align-items: flex-start; padding: 7px 10px; border-radius: 10px 10px 2px 10px; background: var(--accent-soft); color: var(--text); font-weight: 550; font-size: 13px; }
.bn-ag-q svg { width: 14px; height: 14px; flex: none; margin-top: 2px; color: var(--accent); }
.bn-ag-a { display: flex; flex-direction: column; gap: 8px; padding: 10px 12px; border: 1px solid var(--line); border-radius: 10px 10px 10px 2px; background: var(--surface); font-size: 13px; line-height: 1.5; }
.bn-ag-a .txt p { margin: 0 0 6px; } .bn-ag-a .txt p:last-child { margin-bottom: 0; }
.bn-ag-ul { margin: 0 0 6px; padding-left: 18px; } .bn-ag-ul li { margin: 2px 0; overflow-wrap: anywhere; }
.bn-ag-sug { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
.bn-ag-sug .chip { white-space: normal; text-align: left; }
.bn-ag-w { border: 1px solid var(--line); border-radius: var(--r-sm); padding: 8px 10px; }
.bn-ag > .ft { padding: 10px 12px; border-top: 1px solid var(--line); }
.bn-ag > .ft .bn-sug { top: auto; bottom: calc(100% + 6px); }
@media (max-width: 900px) { .bn-root.bn-ag-open { padding-right: 0; } .bn-ag { width: 100vw; top: 52px; } .bn-cmd-mode button span, .bn-ag-btn .lbl { display: none; } .bn-scope { max-width: 96px; margin-left: 4px; } .bn-sug-ft .keys { display: none; } .bn-cmd-mode button { padding: 4px 7px; } }
/* ---- the map's inspector */
.bn-map .btn svg, .bn-map .chip svg { display: inline-block; width: 14px; height: 14px; }
.bn-days60 .cols { display: flex; align-items: flex-end; gap: 1px; height: 64px; border-bottom: 1px solid var(--line); }
.bn-days60 .cols > span { flex: 1; display: flex; flex-direction: column-reverse; height: 100%; justify-content: flex-start; }
.bn-days60 .cols i { display: block; min-height: 2px; }
.bn-days60 .cols i.f { background: var(--crit); } .bn-days60 .cols i.c { background: var(--s1); }
.bn-days60 .ax { display: flex; justify-content: space-between; font-size: 10.5px; color: var(--muted); margin-top: 2px; }
.bn-days60 .bn-legend { padding-top: 4px; }
.bn-otabs { margin: 0; padding: 0 8px; position: sticky; top: 0; background: var(--surface); z-index: 2; }
.bn-otabs button { padding: 8px 7px; font-size: 12px; }
.bn-otabs .n { font-size: 10.5px; padding: 0 5px; border-radius: 999px; background: var(--surface-3); color: var(--text-2); }
.bn-chain { list-style: none; margin: 0; padding: 0 0 0 12px; border-left: 2px solid var(--line-2); font-size: 12.5px; }
.bn-chain li { position: relative; padding: 3px 0 3px 10px; }
.bn-chain li::before { content: ""; position: absolute; left: -18px; top: 8px; width: 9px; height: 9px; border-radius: 50%; background: var(--surface); border: 2px solid var(--muted); }
.bn-chain li.bad::before { border-color: var(--crit); background: var(--crit); }
.bn-chain li.me::before { border-color: var(--accent); background: var(--accent); }
.bn-chain li.below { color: var(--text-2); }
.bn-chain li.below::before { border-style: dashed; }
.bn-cust { display: grid; grid-template-columns: minmax(0, 1fr) auto auto; gap: 3px 10px; font-size: 12.5px; }
.bn-cust > div { display: contents; }
.bn-meter { height: 8px; border-radius: 4px; background: var(--surface-3); overflow: hidden; margin-top: 8px; }
.bn-meter i { display: block; height: 100%; border-radius: 4px; }
/* ---- dashboards */
.bn-dash-head { margin: 0 0 12px; }
.bn-dash { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 420px), 1fr)); gap: 12px; }
.bn-dash .card { margin: 0; min-width: 0; }
.bn-dash .card.wide { grid-column: 1 / -1; }
.bn-heat { display: grid; gap: 2px; font-size: 11px; align-items: center; }
.bn-heat .d { color: var(--muted); text-align: center; font-variant-numeric: tabular-nums; }
.bn-heat .c { color: var(--text-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.bn-heat .cell { height: 18px; border-radius: 3px; }
.bn-sec { margin: 14px 0 6px; font-size: 12px; font-weight: 600; color: var(--text-2); text-transform: uppercase; letter-spacing: .04em; }
.bn-sec:first-child { margin-top: 0; }
.bn-mini { display: grid; grid-template-columns: repeat(auto-fit, minmax(260px, 1fr)); gap: 14px; }
.bn-replay { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; gap: 10px; align-items: center; margin-bottom: 8px; }
.bn-replay input[type=range] { width: 100%; }
.bn-link { color: var(--accent); cursor: pointer; text-decoration: none; }
.bn-link:hover { text-decoration: underline; }
.bn-ro { font-family: var(--mono); font-size: 12px; }
.bn-board { align-items: start; display: grid; grid-template-columns: repeat(auto-fit, minmax(330px, 1fr)); gap: 12px; }
.bn-panel { border: 1px solid var(--line); border-radius: var(--r-sm); background: var(--surface); min-width: 0; padding: 8px 10px 10px; }
.bn-panel.mine { border-color: color-mix(in srgb, var(--accent) 45%, var(--line)); }
.bn-panel > header { display: flex; align-items: center; gap: 8px; margin-bottom: 6px; font-size: 12.5px; min-width: 0; }
.bn-panel > header .chip { font-size: 10.5px; white-space: nowrap; }
.bn-ptable { max-height: 250px; overflow: auto; margin: 0 -10px -10px; font-size: 12px; }
.bn-stack { display: grid; gap: 4px; align-items: center; }
.bn-stack .ch { font-size: 11px; font-weight: 600; color: var(--text-2); text-align: center; }
.bn-stack .rh { font-size: 12.5px; font-weight: 600; white-space: nowrap; }
.bn-stack .cell { height: 30px; border-radius: 5px; border: 1px solid transparent; font: inherit; font-size: 12px; font-weight: 650; color: #fff; cursor: pointer; padding: 0; }
.bn-stack .cell.ok { background: color-mix(in srgb, var(--good) 22%, var(--surface)); cursor: default; }
.bn-stack .cell.none { background: var(--surface-2); cursor: default; }
.bn-stack .cell.warn { background: var(--warn); color: #111; }
.bn-stack .cell.bad { background: var(--crit); }
.bn-stack .cell.start { outline: 2px solid var(--text); outline-offset: 2px; }
.bn-stack .cell.on { box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 60%, transparent); }
.bn-lenses { display: grid; grid-template-columns: repeat(auto-fit, minmax(340px, 1fr)); gap: 12px; align-items: start; }
.bn-lens { border: 1px solid var(--line); border-radius: var(--r-sm); padding: 10px 12px 12px; min-width: 0; background: var(--surface); }
.bn-lens > header { display: flex; justify-content: space-between; gap: 8px; align-items: center; }
.bn-lens .f { font-size: 10.5px; font-weight: 700; letter-spacing: .06em; text-transform: uppercase; color: var(--accent); }
.bn-lens .c { font-family: var(--mono); font-size: 11px; color: var(--text-2); white-space: nowrap; }
.bn-lens h4 { margin: 4px 0 2px; font-size: 14px; text-transform: none; letter-spacing: 0; color: var(--text); font-weight: 650; }
.bn-lens .how { font-size: 12px; color: var(--muted); margin-bottom: 10px; }
.bn-sus a { gap: 8px; padding: 2px 0; cursor: pointer; align-items: center; }
.bn-sus .bar { flex: 1; height: 8px; background: var(--surface-3); border-radius: 4px; overflow: hidden; }
.bn-sus .bar i { display: block; height: 100%; border-radius: 4px; }
.bn-sparks { display: grid; grid-template-columns: repeat(auto-fit, minmax(150px, 1fr)); gap: 12px; }
.bn-sparks svg { width: 100%; }
.bn-one { display: grid; grid-template-columns: 20px minmax(0, 1fr); gap: 10px; align-items: start; margin-top: 12px; padding: 10px 12px; border-radius: var(--r-sm); background: color-mix(in srgb, var(--accent) 8%, var(--surface)); font-size: 12.5px; color: var(--text-2); }
.bn-trace { display: flex; flex-direction: column; }
.bn-trace .hop { display: grid; grid-template-columns: 12px auto minmax(0, 1fr); gap: 8px; align-items: center; cursor: pointer; padding: 3px 0; }
.bn-trace .hop i { width: 10px; height: 10px; border-radius: 50%; background: var(--good); }
.bn-trace .hop.warn i { background: var(--warn); } .bn-trace .hop.bad i { background: var(--crit); }
.bn-trace .lk { margin-left: 4px; border-left: 2px solid var(--good); padding: 5px 0 5px 14px; display: flex; flex-direction: column; cursor: pointer; }
.bn-trace .lk.warn { border-left-color: var(--warn); } .bn-trace .lk.bad { border-left-color: var(--crit); border-left-style: dashed; }
.bn-trace .lk.bad .muted, .bn-trace .hop.bad .muted { color: var(--crit-ink); }
.bn-find { width: 300px; max-width: 38vw; font-size: 12.5px; padding: 5px 9px; }
.bn-work { display: grid; grid-template-columns: 78px minmax(0, 1fr) auto; gap: 12px; align-items: center; padding: 10px 14px; border-top: 1px solid var(--line); border-left: 3px solid transparent; }
.bn-work:first-child { border-top: 0; }
.bn-work:hover { background: var(--surface-2); }
.bn-work.u1 { border-left-color: var(--crit); } .bn-work.u2 { border-left-color: var(--warn); } .bn-work.u3 { border-left-color: var(--line-2); }
.bn-work .k { font-size: 11px; font-weight: 650; text-transform: uppercase; letter-spacing: .05em; color: var(--text-2); }
.bn-work.u1 .k { color: var(--crit-ink); }
.bn-work .t { font-weight: 600; overflow-wrap: anywhere; }
.bn-work .s { font-size: 12.5px; color: var(--text-2); margin-top: 1px; }
.bn-check { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
.bn-check li { display: grid; grid-template-columns: 18px minmax(0, 1fr); gap: 8px; align-items: start; font-size: 13px; }
.bn-check li svg { color: var(--good); margin-top: 2px; }
.bn-path { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; justify-content: flex-start; }
.bn-path .sep { font-size: 12px; color: var(--muted); }
.bn-reply { margin: 0; padding: 10px 12px; background: var(--surface-2); border: 1px solid var(--line); border-radius: var(--r-sm); white-space: pre-wrap; overflow-wrap: anywhere; font: inherit; font-size: 13px; line-height: 1.5; max-height: 340px; overflow: auto; }
.bn-conflict { display: grid; grid-template-columns: 18px minmax(0, 1fr); gap: 8px; font-size: 12.5px; padding: 6px 8px; border-radius: 5px; margin-top: 6px; background: var(--surface-2); }
.bn-conflict.critical { background: var(--crit-soft); color: var(--crit-ink); }
.bn-conflict.major { background: color-mix(in srgb, var(--warn) 16%, transparent); }
.bn-act.crit { border-color: color-mix(in srgb, var(--crit) 55%, var(--line)); }
.bn-act.warnb { border-color: color-mix(in srgb, var(--warn) 55%, var(--line)); }
.bn-meter { display: inline-block; width: 72px; height: 6px; border-radius: 3px; background: var(--surface-3); overflow: hidden; vertical-align: middle; }
.bn-meter i { display: block; height: 100%; border-radius: 3px; }
.bn-report { font-size: 13.5px; line-height: 1.55; }
.bn-report p { margin: 8px 0 0; }
.bn-hero.warn { border-color: color-mix(in srgb, var(--warn) 45%, var(--line)); background: linear-gradient(90deg, color-mix(in srgb, var(--warn) 16%, var(--surface)), var(--surface) 70%); }
.bn-hero.info { border-color: color-mix(in srgb, var(--accent) 40%, var(--line)); background: linear-gradient(90deg, color-mix(in srgb, var(--accent) 12%, var(--surface)), var(--surface) 70%); }
.bn-map { display: grid; gap: 10px; }
.bn-map-bar { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 8px; }
.bn-map-bar .input { height: 38px; font-size: 14px; }
.bn-map-eg { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; }
.bn-map-eg .chip { cursor: pointer; border: 1px solid var(--line); background: var(--surface); font: inherit; font-size: 12px; color: var(--text-2); }
.bn-map-eg .chip:hover { border-color: var(--accent); color: var(--text); }
.bn-map-bar .btn { height: 38px; padding: 0 22px; }
.bn-map-main { display: grid; grid-template-columns: minmax(0, 1fr) 380px; gap: 12px; height: calc(100vh - 345px); min-height: 520px; }
.bn-map-stage { position: relative; border: 1px solid var(--line); border-radius: var(--r); background: var(--surface-3, var(--surface-2)); overflow: hidden; min-width: 0; }
.bn-map-canvas { position: absolute; inset: 0; width: 100%; height: 100%; cursor: grab; touch-action: none; outline: none; }
.bn-map-crumbs { position: absolute; left: 12px; top: 10px; display: flex; gap: 6px; font-size: 12.5px; background: color-mix(in srgb, var(--surface) 88%, transparent); border: 1px solid var(--line); border-radius: 999px; padding: 4px 12px; }
.bn-map-crumbs a { cursor: pointer; color: var(--text); font-weight: 600; }
.bn-map-crumbs a:hover { color: var(--accent); }
.bn-map-zoom { position: absolute; right: 12px; top: 10px; display: flex; gap: 4px; }
.bn-map-legend { position: absolute; left: 12px; bottom: 10px; display: flex; gap: 12px; flex-wrap: wrap; font-size: 11.5px; color: var(--text-2); background: color-mix(in srgb, var(--surface) 88%, transparent); border: 1px solid var(--line); border-radius: var(--r-sm); padding: 4px 10px; max-width: 60%; }
.bn-map-legend i { display: inline-block; width: 9px; height: 9px; border-radius: 50%; margin-right: 5px; }
.bn-map-foot { position: absolute; left: 12px; right: 12px; bottom: 10px; display: flex; flex-direction: column; align-items: flex-start; gap: 6px; pointer-events: none; }
.bn-map-foot-row { display: flex; width: 100%; justify-content: space-between; align-items: flex-end; gap: 6px 8px; flex-wrap: wrap; pointer-events: none; }
.bn-map-foot .bn-map-legend, .bn-map-foot .bn-map-stats { position: static; pointer-events: auto; }
.bn-map-foot .bn-map-stats { margin-left: auto; text-align: right; }
.bn-map-stats { position: absolute; right: 12px; bottom: 10px; font-size: 11.5px; color: var(--text-2); background: color-mix(in srgb, var(--surface) 88%, transparent); border: 1px solid var(--line); border-radius: var(--r-sm); padding: 4px 10px; font-variant-numeric: tabular-nums; }
.bn-map-tip { position: absolute; pointer-events: none; background: var(--surface); border: 1px solid var(--line-2, var(--line)); border-radius: var(--r-sm); padding: 6px 10px; font-size: 12px; max-width: 250px; box-shadow: 0 4px 14px rgba(0,0,0,.12); z-index: 2; }
.bn-map-side { border: 1px solid var(--line); border-radius: var(--r); background: var(--surface); overflow-y: auto; min-width: 0; padding-bottom: 12px; }
.bn-map-head { padding: 12px 14px 10px; border-bottom: 1px solid var(--line); }
.bn-map-head h3 { margin: 0; font-size: 15px; overflow-wrap: anywhere; }
.bn-map-why { margin-top: 8px; font-size: 13px; color: var(--crit-ink); }
.bn-map-kpis { display: grid; grid-template-columns: repeat(3, 1fr); border-bottom: 1px solid var(--line); }
.bn-map-kpis > div { padding: 10px 14px; display: grid; }
.bn-map-kpis b { font-size: 20px; font-variant-numeric: tabular-nums; }
.bn-map-kpis span { font-size: 11.5px; color: var(--text-2); }
.bn-map-sec { padding: 12px 14px 0; }
.bn-map-sec > header { display: grid; gap: 1px; margin-bottom: 6px; }
.bn-map-sec > header b { font-size: 12px; text-transform: uppercase; letter-spacing: .04em; color: var(--text-2); }
.bn-map-sec > header .how { font-size: 10.5px; color: var(--muted); font-family: var(--mono); }
.bn-map-row { display: grid; grid-template-columns: 10px minmax(0, 1fr) auto; gap: 8px; align-items: start; padding: 5px 6px; margin: 0 -6px; border-radius: var(--r-sm); cursor: pointer; }
.bn-map-row:hover { background: var(--surface-2); }
.bn-map-row .t { display: grid; min-width: 0; }
.bn-map-row .t b { font-size: 12.5px; font-weight: 600; overflow-wrap: anywhere; }
.bn-map-row .t span { font-size: 11.5px; color: var(--text-2); }
.bn-map-dot { width: 9px; height: 9px; border-radius: 50%; margin-top: 4px; }
.bn-map-verdict { margin: 12px 14px 0; padding: 10px 12px; border: 1px solid color-mix(in srgb, var(--crit) 45%, var(--line)); background: color-mix(in srgb, var(--crit) 8%, var(--surface)); border-radius: var(--r-sm); font-size: 13px; }
.bn-map-verdict.calm { border-color: color-mix(in srgb, var(--warn) 45%, var(--line)); background: color-mix(in srgb, var(--warn) 10%, var(--surface)); }
.bn-map-hist { display: grid; gap: 5px; font-size: 12px; }
.bn-map-hist > div { display: grid; grid-template-columns: 62px minmax(0, 1fr); gap: 8px; padding-left: 8px; border-left: 3px solid var(--line); }
.bn-map-hist > div.change { border-left-color: var(--accent); }
.bn-map-hist > div.fault { border-left-color: var(--crit); }
.bn-map-hist .when { color: var(--muted); font-variant-numeric: tabular-nums; }
.bn-map-read { display: grid; gap: 5px; font-size: 12.5px; }
.bn-map-partial { margin: 10px 14px 0; padding: 8px 10px; border: 1px solid color-mix(in srgb, var(--warn) 45%, var(--line)); border-left-width: 3px; background: color-mix(in srgb, var(--warn) 9%, var(--surface)); border-radius: 6px; font-size: 12.5px; line-height: 1.45; }
.bn-map-read > div { display: grid; grid-template-columns: 96px minmax(0, 1fr); gap: 8px; }
.bn-map-read .k { font-size: 10.5px; text-transform: uppercase; letter-spacing: .04em; color: var(--accent); font-weight: 700; padding-top: 2px; }
.bn-map-side > .btn.ghost { margin: 8px 8px 0; }
.bn-map-side pre { font-size: 11px; max-height: 260px; overflow: auto; }
@media (max-width: 1100px) { .bn-map-main { grid-template-columns: minmax(0, 1fr); height: auto; } .bn-map-stage { height: 560px; } }
@media (max-width: 1100px) { .bn-cols, .bn-cols.even, .bn-cols.wide { grid-template-columns: minmax(0, 1fr); } }
@media (max-width: 900px) {
  .bn-hero { grid-template-columns: minmax(0, 1fr); }
  .bn-search { grid-template-columns: minmax(0, 1fr); }
  .bn-step .top { grid-template-columns: 18px 50px minmax(0, 1fr); } .bn-step .top > :nth-child(4) { display: none; }
  .bn-step .find, .bn-step pre.code { margin-left: 0; }
  .bn-blame .bl { grid-template-columns: 90px 30px minmax(0, 1fr); }
}
/* ---- the mock's own: frame on a fixed board, the world map, connections, hops, charts ---- */
.seg button.on { background: var(--surface); color: var(--text); box-shadow: var(--shadow-1); outline: 1px solid var(--line); }
.tabs button.on { color: var(--text); border-bottom-color: var(--accent); }
.tabs .n.bad { background: var(--crit-soft); color: var(--crit-ink); }
.bn-cmd-mode button.on { background: var(--surface); color: var(--text); box-shadow: var(--shadow-1); font-weight: 600; }
@keyframes bbhalo { 0% { transform: scale(1); opacity: .8; } 100% { transform: scale(2.6); opacity: 0; } }
.bb-graph svg { display: block; width: 100%; height: auto; }
.bb-graph .col { font: 600 11px var(--sans); fill: var(--muted); letter-spacing: .05em; text-transform: uppercase; }
.bb-graph .nd rect { fill: var(--surface); stroke: var(--line-2); rx: 7; }
.bb-graph .nd.bad rect { stroke: var(--crit); fill: color-mix(in srgb, var(--crit) 7%, var(--surface)); }
.bb-graph .nd.warn rect { stroke: var(--warn); fill: color-mix(in srgb, var(--warn) 10%, var(--surface)); }
.bb-graph .nd.hub rect { stroke: var(--s7); fill: color-mix(in srgb, var(--s7) 8%, var(--surface)); }
.bb-graph .nd text { font: 600 12px var(--sans); fill: var(--text); }
.bb-graph .nd text.s { font-weight: 400; font-size: 10.5px; fill: var(--text-2); }
.bb-graph .e { fill: none; stroke: var(--line-2); }
.bb-graph .e.own { stroke: var(--crit); stroke-dasharray: 4 3; }
.bb-graph .e.ann { stroke: var(--text-2); }
.bb-graph .e.car { stroke: var(--s7); opacity: .75; }
.bb-graph .pct { font: 600 10px var(--sans); fill: var(--s7); paint-order: stroke; stroke: var(--surface); stroke-width: 3px; }
.bb-hops { display: flex; align-items: stretch; gap: 0; overflow-x: auto; padding: 4px 0; }
.bb-hop { border: 1px solid var(--line); border-radius: var(--r-sm); padding: 6px 9px; min-width: 128px; background: var(--surface); font-size: 12px; }
.bb-hop b { display: block; font-size: 12.5px; }
.bb-hop .ip { font: 11px var(--mono); color: var(--muted); }
.bb-hop .ms { font-variant-numeric: tabular-nums; color: var(--text-2); }
.bb-hop.bad { border-color: var(--crit); background: color-mix(in srgb, var(--crit) 6%, var(--surface)); }
.bb-hop.ok { border-color: color-mix(in srgb, var(--good) 50%, var(--line)); }
.bb-gap { display: grid; place-items: center; min-width: 46px; font-size: 11px; color: var(--muted); font-variant-numeric: tabular-nums; position: relative; }
.bb-gap::before { content: ""; position: absolute; left: 4px; right: 4px; top: 50%; border-top: 1.5px solid var(--line-2); }
.bb-gap span { position: relative; background: var(--surface); padding: 0 3px; }
.bb-gap.bad::before { border-top-color: var(--crit); border-top-style: dashed; } .bb-gap.bad span { color: var(--crit-ink); font-weight: 650; }
.bb-chart { width: 100%; display: block; overflow: visible; }
.bb-chart text { fill: var(--muted); font: 10.5px var(--sans); }
.bb-chart .grid { stroke: var(--grid); stroke-width: 1; }
.bb-chart .bar { fill: var(--s1); } .bb-chart .bar.c { fill: var(--crit); } .bb-chart .bar.g { fill: var(--s3); }
.bb-chart .mark { stroke: var(--crit); stroke-width: 1.2; stroke-dasharray: 3 3; }
.bb-hbars { display: grid; grid-template-columns: minmax(0, 150px) 64px minmax(0, 1fr); gap: 6px 10px; align-items: center; font-size: 12.5px; }
.bb-hbars .n { text-align: right; font-variant-numeric: tabular-nums; color: var(--text-2); }
.bb-hbars .meter > i.ok { background: var(--good); }
.bb-checks { display: flex; flex-wrap: wrap; gap: 6px; }
.bb-checks label { display: inline-flex; gap: 6px; align-items: center; font-size: 12.5px; padding: 3px 10px; border-radius: 999px; border: 1px solid var(--line-2); background: var(--surface); color: var(--text-2); }
.bb-checks label.on { background: var(--accent-soft); color: var(--accent); border-color: color-mix(in srgb, var(--accent) 45%, transparent); }
.bb-checks input { accent-color: var(--accent); margin: 0; }
.bb-f { margin-bottom: 12px; }
.bb-f > label { display: block; font-size: 11.5px; text-transform: uppercase; letter-spacing: .04em; color: var(--muted); font-weight: 600; margin-bottom: 5px; }
.bb-f .input { width: 100%; }
pre.code { font: 12px/1.5 var(--mono); background: var(--code); border: 1px solid var(--line); border-radius: var(--r-sm); padding: 8px 10px; margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; }
.bb-mono { font-family: var(--mono); font-size: 12px; }
.bb-why { font-size: 12.5px; color: var(--text-2); margin-top: 2px; }
.bb-cols3 { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 14px; align-items: start; }
.bb-stack3 > .card + .card { margin-top: 14px; }
.bn-cols > div > .card + .card, .bn-cols > div > .card + .bn-hero, .bn-cols > div > section + section { margin-top: 14px; }
.bn-root .tabs button { padding: 8px 11px; }

/* Backbone explorer: the zoomed map stages (NetOps' estate map, drawn as SVG for the mock) */
.bn-root .bn-map-main.bb-mm { height: auto; min-height: 0; align-items: start; }
.bb-mm .bn-map-stage { min-height: 0; }
.bb-mm .bn-map-side { max-height: none; }
.bb-zm { display: block; width: 100%; height: auto; }
.bb-zm .ct { fill: none; stroke: var(--line-2); }
.bb-zm .ct.reg { fill: color-mix(in srgb, var(--surface) 55%, transparent); }
.bb-zm .ct.met { fill: var(--surface); stroke-dasharray: 4 3; }
.bb-zm .ctl { font: 650 12.5px var(--sans); fill: var(--text); }
.bb-zm .cts { font: 11px var(--sans); fill: var(--muted); }
.bb-zm text { font: 600 12px var(--sans); fill: var(--text); }
.bb-zm text.s { font: 400 10.5px var(--sans); fill: var(--text-2); }
.bb-zm text.m { font: 600 11.5px var(--mono); }
.bb-zm .ix rect { fill: color-mix(in srgb, var(--s7) 7%, var(--surface)); stroke: color-mix(in srgb, var(--s7) 45%, var(--line)); }
.bb-zm .ix .dm { fill: var(--s7); }
.bb-zm .ix.kr rect { stroke: var(--good); stroke-width: 1.6; }
.bb-zm .kroot circle { fill: var(--good); }
.bb-zm .kroot text { fill: #fff; font: 700 10px var(--sans); }
.bb-zm .nt rect { fill: var(--surface); stroke: var(--line-2); }
.bb-zm .nt.ok rect { stroke: color-mix(in srgb, var(--good) 55%, var(--line)); }
.bb-zm .nt.warn rect { stroke: var(--warn); fill: color-mix(in srgb, var(--warn) 9%, var(--surface)); }
.bb-zm .nt.bad rect { stroke: var(--crit); fill: color-mix(in srgb, var(--crit) 8%, var(--surface)); }
.bb-zm .nt.car rect { stroke: color-mix(in srgb, var(--s7) 55%, var(--line)); }
.bb-zm .nt.more rect { stroke-dasharray: 4 3; fill: var(--surface-2); }
.bb-zm .nt.sel rect { stroke-width: 2.4; filter: drop-shadow(0 0 0 var(--accent)); }
.bb-zm .nt.sel rect { stroke: var(--accent); }
.bb-zm .nt.bad.sel rect { stroke: var(--crit); stroke-width: 2.6; }
.bb-zm .pb.ok { fill: var(--good); } .bb-zm .pb.warn { fill: var(--warn); } .bb-zm .pb.bad { fill: var(--crit); }
.bb-zm .mb { fill: none; stroke: var(--line-2); stroke-width: 1.1; opacity: .8; }
.bb-zm .mb.bk { stroke: var(--good); stroke-width: 1.5; opacity: .75; }
.bb-zm .ex { fill: none; stroke: var(--crit); stroke-width: 1.8; stroke-dasharray: 5 4; }
.bb-zm .exl { font: 600 10.5px var(--sans); fill: var(--crit-ink); paint-order: stroke; stroke: var(--surface); stroke-width: 3px; }
.bb-zm .ab rect { fill: color-mix(in srgb, var(--crit) 6%, var(--surface)); stroke: color-mix(in srgb, var(--crit) 40%, var(--line)); }
.bb-zm .nest { fill: none; stroke: var(--line-2); }
.bb-mm .bn-map-stage .bb-zm { margin-top: 44px; }
.bb-cap { font-size: 10px; font-weight: 700; letter-spacing: .06em; text-transform: uppercase; color: var(--accent); border: 1px solid color-mix(in srgb, var(--accent) 35%, transparent); border-radius: 999px; padding: 0 6px; white-space: nowrap; }
.bb-ch { display: block; width: 100%; height: auto; }
.bb-ch text { font: 10px var(--sans); fill: var(--muted); }
.bb-ch .g { stroke: var(--grid, var(--line)); }
.bb-ch .b { fill: var(--s1); } .bb-ch .b.c { fill: var(--crit); } .bb-ch .b.o { fill: var(--good); opacity: .55; }
a.bn-map-row { color: inherit; text-decoration: none; }
a.bn-map-row .btn { pointer-events: none; }
.bb-mm .bn-otabs button { padding: 8px 5px; }

/* ---- Backbone explorer's own ---- */
.bn-root { max-width: 1440px; margin: 0 auto; padding: 16px 0 40px; font: 14px/1.5 var(--sans); color: var(--text); }
.bn-root * { box-sizing: border-box; }
.bn-root a { color: inherit; text-decoration: none; }
.bn-root a:hover { color: var(--accent); }
.bn-root .row { display: flex; gap: 8px; align-items: center; }
.bn-root .row.between { justify-content: space-between; }
.bn-root .mono, .bn-root td.mono { font-family: var(--mono); font-size: 12.5px; }
.bn-root .bad { color: var(--crit-ink); }
.bn-cmd-ic { display: inline-flex; padding: 0 0 0 12px; color: var(--muted); }
.bn-cmd-ic svg { width: 16px; height: 16px; fill: none; stroke: currentColor; stroke-width: 2; }
.bn-tabs { margin-bottom: 14px; }
.bn-tabs button { padding: 8px 11px; }
.bn-main { min-height: 50vh; }
.bn-empty { color: var(--muted); font-size: 13px; padding: 8px 0; }
.bn-skel { display: grid; gap: 10px; padding: 12px 0; }
.bn-skel > div { height: 14px; border-radius: 6px; background: linear-gradient(90deg, var(--surface-3), var(--surface-2), var(--surface-3)); background-size: 200% 100%; animation: bnsk 1.2s infinite linear; }
@keyframes bnsk { to { background-position: -200% 0; } }
.bn-hero-top { display: flex; gap: 10px; align-items: center; margin-bottom: 6px; flex-wrap: wrap; }
.bn-hero-ul { margin: 6px 0 0; padding-left: 20px; color: var(--text-2); }
.bn-hero-ul li { margin: 2px 0; }
.bn-hero-act { align-self: center; }
.bn-hero h2.mono { font-family: var(--mono); font-size: 20px; }
.bn-conn-t { font-weight: 600; display: block; }
.bn-conn-ids { display: flex; flex-wrap: wrap; gap: 4px; margin-top: 5px; }
.bn-conn-ids .chip { font-size: 11.5px; max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.bn-dash { color: var(--crit); margin-right: 4px; letter-spacing: -1px; }
.bn-map-main.bn-tall { height: auto; min-height: 640px; }
.bn-map-stage.bn-scroll { overflow: auto; }
.bn-zm-wrap { padding: 46px 10px 70px; }
.bn-btnrow { display: flex; gap: 6px; flex-wrap: wrap; margin-top: 10px; }
.bn-mono-list { display: flex; flex-wrap: wrap; gap: 4px 10px; font: 12px var(--mono); }
.bn-two { display: grid; grid-template-columns: 1fr 1fr; gap: 14px; }
.bn-two .lbl { font-size: 11px; text-transform: uppercase; letter-spacing: .05em; color: var(--muted); margin-bottom: 4px; }
.bn-flush { margin: -14px -16px; }
.card > .bd.flush .bn-work:first-child { border-top: 0; }
.bn-insp { padding: 0; overflow: hidden; }
.bn-insp .bn-map-head { border-top: 0; }
.bn-rule { padding: 12px 0; border-top: 1px solid var(--line); display: grid; gap: 6px; }
.bn-rule:first-child { border-top: 0; padding-top: 0; }
.bn-rule pre { margin: 0; font-size: 11.5px; white-space: pre-wrap; background: var(--code); border-radius: var(--r-sm); padding: 8px 10px; }
.bn-rule .bn-map-dot { display: inline-block; margin-right: 8px; vertical-align: 0; }
.bn-prose p { margin: 0 0 10px; color: var(--text-2); }
.bn-check { list-style: none; margin: 12px 0 0; padding: 0; display: grid; gap: 6px; font-size: 13px; }
.bn-check li { display: flex; gap: 8px; align-items: baseline; }
.bn-check .bn-map-dot { flex: none; margin-top: 0; }
.bn-chart-box { padding: 4px 0; }
.bn-graph-box { overflow-x: auto; }
.bn-map-why.ok { color: var(--text-2); }
.bn-map-row { color: inherit; }
.bn-map-row .btn { align-self: center; }
.bn-stack .ch { text-decoration: none; }
.bn-stack .ch:hover { color: var(--accent); }
.bn-foot { margin-top: 28px; padding-top: 12px; border-top: 1px solid var(--line); font-size: 12px; color: var(--muted); }
.bn-foot a { text-decoration: underline; text-underline-offset: 2px; }
.bn-world { background: color-mix(in srgb, var(--accent) 4%, var(--surface-2)); }
.bn-world .land { fill: var(--surface-3); stroke: var(--surface); stroke-width: calc(.5px / var(--z, 1)); cursor: pointer; transition: fill .15s; }
.bn-world .land:hover { fill: color-mix(in srgb, var(--accent) 20%, var(--surface-3)); }
.bn-world .land.c-ok { fill: color-mix(in srgb, var(--good) 16%, var(--surface-3)); }
.bn-world .land.c-warn { fill: color-mix(in srgb, var(--warn) 42%, var(--surface-3)); }
.bn-world .land.c-bad { fill: color-mix(in srgb, var(--crit) 38%, var(--surface-3)); }
.bn-world .lk { fill: none; stroke-linecap: round; cursor: pointer; }
.bn-world .lk.own { stroke: var(--crit); stroke-width: calc(1.3px / var(--z, 1)); stroke-dasharray: calc(4px / var(--z, 1)) calc(3px / var(--z, 1)); opacity: .8; }
.bn-world .ixd { fill: var(--s7); opacity: .75; cursor: pointer; }
.bn-world .pin, .bn-world .ixd { cursor: pointer; transform: scale(calc(1 / var(--z, 1))); transform-box: fill-box; transform-origin: center; }
.bn-world .pin circle { transform-box: fill-box; transform-origin: center; }
.bn-world .halo { fill: none; stroke: var(--crit); stroke-width: 1.2; opacity: .7; animation: bbhalo 1.8s ease-out infinite; transform-box: fill-box; transform-origin: center; }
.bn-world .pn { font: 700 7px var(--sans); fill: #fff; pointer-events: none; }
.bb-hbars .meter { height: 8px; background: var(--surface-3); border-radius: 4px; overflow: hidden; }
.bb-hbars .meter > i { display: block; height: 100%; border-radius: 4px; background: var(--accent); }
.bb-hbars .meter > i.warn { background: var(--warn); }
.bb-hbars .meter > i.bad { background: var(--crit); }
.bb-hbars a { color: var(--accent); }
.bn-sqlp { position: fixed; right: 16px; bottom: 16px; width: min(760px, calc(100vw - 32px)); max-height: 70vh; overflow: auto; background: var(--surface); border: 1px solid var(--line-2); border-radius: var(--r); box-shadow: var(--shadow-3); padding: 12px 14px; display: none; z-index: 50; }
.bn-sqlp.on { display: block; }
.bn-sqlp-hd { display: flex; gap: 10px; align-items: center; margin-bottom: 8px; }
.bn-sqlp-hd .btn { margin-left: auto; }
.bn-sqlp .q { border-top: 1px solid var(--line); padding: 8px 0; }
.bn-sqlp .q .w { display: flex; justify-content: space-between; gap: 10px; font-size: 12.5px; font-weight: 600; }
.bn-sqlp .q pre { margin: 6px 0 0; font-size: 11.5px; white-space: pre-wrap; background: var(--code); border-radius: var(--r-sm); padding: 8px 10px; }
.bn-head .tools .btn svg, .bn-hero .btn svg { width: 14px; height: 14px; fill: none; stroke: currentColor; stroke-width: 2; }
@media (max-width: 1100px) { .bn-map-main { grid-template-columns: minmax(0, 1fr); height: auto; } .bn-map-main .bn-map-stage { height: 62vh; } .bn-map-main.bn-tall .bn-map-stage { height: auto; } }
@media (max-width: 900px) { .bn-cols, .bn-cols.wide, .bn-cols.even { grid-template-columns: minmax(0, 1fr); } .bn-two { grid-template-columns: 1fr; } .bn-hero { grid-template-columns: 1fr; } }
@media (max-width: 600px) { .bn-root { padding-top: 10px; } .bb-hbars { grid-template-columns: minmax(0, 110px) 56px minmax(0, 1fr); } }

/* The world map: the stage is the map's shape, the land is clipped to it,
   and the side list scrolls within the stage's height. */
.bn-root .bn-map-main.bn-wm { height: auto; min-height: 0; }
.bn-wm .bn-map-stage { aspect-ratio: 1000 / 470; }
.bn-map .bn-map-canvas.bn-world { height: 100%; }
.bn-wm .bn-map-side { contain: size; }
.bn-map-legend .bn-ln { display: inline-block; width: 16px; height: 0; border-top: 2px dashed var(--crit); border-radius: 0; margin-right: 5px; vertical-align: 3px; }
@media (max-width: 1100px) { .bn-wm .bn-map-side { contain: none; max-height: 70vh; } }
.bn-stack .cell.bad { color: #fff; }
`;

// ---------------------------------------------------------------- helpers

function h(tag, attrs, ...kids) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v == null || v === false) continue;
    if (k === "style" && typeof v === "object") Object.assign(e.style, v);
    else if (k === "html") e.innerHTML = v;
    else if (k.startsWith("on") && typeof v === "function") e.addEventListener(k.slice(2), v);
    else if (k === "value") e.value = v;
    else e.setAttribute(k, v === true ? "" : v);
  }
  add(e, kids);
  return e;
}
function add(e, kids) {
  for (const k of kids.flat(Infinity)) {
    if (k == null || k === false) continue;
    e.append(k instanceof Node ? k : document.createTextNode(String(k)));
  }
}
function fill(e, ...kids) { e.replaceChildren(); add(e, kids); return e; }
function svg(tag, attrs, ...kids) {
  const e = document.createElementNS("http://www.w3.org/2000/svg", tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v == null) continue;
    if (k.startsWith("on") && typeof v === "function") e.addEventListener(k.slice(2), v);
    else e.setAttribute(k, v);
  }
  for (const k of kids.flat(Infinity)) if (k != null && k !== false) e.append(k instanceof Node ? k : document.createTextNode(String(k)));
  return e;
}
const fmt = (n) => (n == null ? "" : Number(n).toLocaleString("en-US"));
const ms = (n) => (n == null ? "–" : `${Math.round(n)} ms`);
const short = (n) => { n = Number(n) || 0; const a = Math.abs(n); return a >= 1e9 ? (n / 1e9).toFixed(1) + "B" : a >= 1e6 ? (n / 1e6).toFixed(a >= 1e7 ? 0 : 1) + "M" : a >= 1e4 ? Math.round(n / 1e3) + "k" : a >= 1e3 ? (n / 1e3).toFixed(1) + "k" : String(Math.round(n)); };
const plural = (n, one, many) => `${fmt(n)} ${n === 1 ? one : many || one + "s"}`;
const TZ = "America/Los_Angeles";
const when = (s) => { const d = s ? new Date(s) : null; return d && !isNaN(d) ? d : null; };
// "7:12 PM PST", in Pacific time, as every time on the page.
function clock(s) {
  const d = when(s);
  if (!d) return "";
  return d.toLocaleTimeString("en-US", { timeZone: TZ, hour: "numeric", minute: "2-digit", timeZoneName: "short" });
}
function hm(s) { const d = when(s); return d ? d.toLocaleTimeString("en-US", { timeZone: TZ, hour: "numeric", minute: "2-digit" }) : ""; }
function day(s) { const d = when(s); return d ? d.toLocaleDateString("en-US", { timeZone: TZ, month: "short", day: "numeric" }) : ""; }
function stamp(s) {
  const d = when(s);
  if (!d) return "";
  const age = Date.now() - d.getTime();
  if (age < 20 * 3600e3) return hm(s);
  if (age < 300 * 86400e3) return day(s);
  return String(d.getUTCFullYear());
}
function ago(s) {
  const d = when(s);
  if (!d) return "";
  const m = Math.round((Date.now() - d.getTime()) / 60e3);
  if (m < 1) return "just now";
  if (m < 90) return `${m} min ago`;
  if (m < 48 * 60) return `${Math.round(m / 60)} h ago`;
  return `${Math.round(m / 1440)} days ago`;
}
function span(a, b) {
  const x = when(a), y = when(b);
  if (!x || !y) return "";
  const m = Math.max(0, Math.round((y - x) / 60e3));
  return m < 90 ? `${m} min` : m < 48 * 60 ? `${Math.round(m / 60)} h` : `${Math.round(m / 1440)} days`;
}
const SEV = { bad: "var(--crit)", warn: "var(--warn)", info: "var(--accent)", ok: "var(--good)", none: "var(--line-2)", ended: "color-mix(in srgb, var(--crit) 35%, var(--surface))" };
const KIND = { invalid: "Invalid", hijack: "Hijack", origin: "New origin", "new-origin": "New origin", subprefix: "More specific", "more-specific": "More specific", flap: "Flapping", flapping: "Flapping", withdrawn: "Withdrawn", blackhole: "Blackhole", outage: "Outage", slow: "Slow" };
const kindWord = (k) => KIND[k] || (k ? k[0].toUpperCase() + k.slice(1) : "");
function netName(n) { return n ? `${n.name || "AS" + n.asn} (AS${n.asn})` : ""; }
function dot(color, big) { return h("i", { class: "bn-map-dot", style: { background: color, ...(big ? { width: "10px", height: "10px" } : {}) } }); }
function pill(sev, text) { return h("span", { class: `pill ${sev === "bad" ? "bad" : sev === "warn" ? "warn" : sev === "ok" ? "ok" : "info"}` }, text); }
function empty(text) { return h("div", { class: "bn-empty" }, text); }
function card(title, sub, ...kids) {
  return h("section", { class: "card" },
    h("div", { class: "hd" }, h("h3", {}, title), sub ? h("span", { class: "sub" }, sub) : null),
    h("div", { class: "bd" }, kids));
}
function cardTools(title, sub, tools, ...kids) {
  return h("section", { class: "card" },
    h("div", { class: "hd" }, h("h3", {}, title), sub ? h("span", { class: "sub" }, sub) : null, h("div", { class: "tools" }, tools)),
    h("div", { class: "bd" }, kids));
}
function sec(title, how, ...kids) {
  return h("section", { class: "bn-map-sec" }, h("header", {}, h("b", {}, title), how ? h("span", { class: "how" }, how) : null), kids);
}
function row(color, title, sub, right, href) {
  return h(href ? "a" : "div", { class: "bn-map-row", href }, dot(color), h("span", { class: "t" }, h("b", {}, title), sub ? h("span", {}, sub) : null), right || h("span"));
}
function skel(n = 4) { return h("div", { class: "bn-skel" }, Array.from({ length: n }, (_, i) => h("div", { style: { width: `${55 + ((i * 37) % 40)}%` } }))); }

// A bar chart of minutes: `series` [{key, color, label}] stacked, `line`
// an optional key drawn as a line on its own scale.
function minutes(points, series, { height = 120, line = null, lineLabel = "" } = {}) {
  const W = 640, H = height, L = 40, B = 18, T = 8;
  const g = svg("svg", { class: "bb-chart", viewBox: `0 0 ${W} ${H}`, role: "img" });
  if (!points.length) { g.append(svg("text", { x: W / 2, y: H / 2, "text-anchor": "middle" }, "no updates in this window")); return g; }
  const tot = points.map((p) => series.reduce((a, s) => a + (p[s.key] || 0), 0));
  const max = Math.max(1, ...tot);
  const bw = (W - L - 4) / points.length;
  for (let i = 0; i <= 2; i++) {
    const y = T + (H - B - T) * (1 - i / 2);
    g.append(svg("line", { class: "grid", x1: L, x2: W, y1: y, y2: y }), svg("text", { x: L - 6, y: y + 3, "text-anchor": "end" }, short((max * i) / 2)));
  }
  points.forEach((p, i) => {
    let y = H - B;
    for (const s of series) {
      const v = p[s.key] || 0;
      if (!v) continue;
      const hgt = ((H - B - T) * v) / max;
      y -= hgt;
      g.append(svg("rect", { x: L + i * bw + 0.5, y, width: Math.max(1, bw - 1), height: hgt, fill: s.color }, svg("title", {}, `${hm(p.t)}: ${fmt(v)} ${s.label}`)));
    }
  });
  if (line) {
    const lm = Math.max(1, ...points.map((p) => p[line] || 0));
    const d = points.map((p, i) => `${i ? "L" : "M"}${(L + i * bw + bw / 2).toFixed(1)},${(T + (H - B - T) * (1 - (p[line] || 0) / lm)).toFixed(1)}`).join("");
    g.append(svg("path", { d, fill: "none", stroke: "var(--s7)", "stroke-width": 1.6 }), svg("text", { x: W - 4, y: T + 9, "text-anchor": "end", fill: "var(--s7)" }, `${lineLabel} (max ${fmt(lm)})`));
  }
  const every = Math.max(1, Math.ceil(points.length / 7));
  points.forEach((p, i) => { if (i % every === 0) g.append(svg("text", { x: L + i * bw + bw / 2, y: H - 4, "text-anchor": "middle" }, hm(p.t))); });
  return g;
}
function legend(items) {
  return h("div", { class: "bn-legend" }, items.map(([color, label, kind]) => h("span", {}, h("i", { class: kind === "d" ? "d" : null, style: { background: color } }), label)));
}
function hbars(rows) {
  const max = Math.max(1, ...rows.map((r) => r.v || 0));
  return h("div", { class: "bb-hbars" }, rows.map((r) => [
    r.href ? h("a", { href: r.href }, r.label) : h("span", {}, r.label),
    h("span", { class: "n" }, r.text),
    h("div", { class: "meter" }, h("i", { class: r.cls || "", style: { width: `${Math.min(100, (100 * (r.v || 0)) / max)}%` } })),
  ]));
}
function table(head, rows, { compact = true } = {}) {
  return h("div", { class: "tbl-wrap" }, h("table", { class: `t${compact ? " compact" : ""}` },
    h("thead", {}, h("tr", {}, head.map((x) => h("th", { class: x.num ? "num" : null }, x.t || x)))),
    h("tbody", {}, rows)));
}

// ---------------------------------------------------------------- the frame

const TABS = [["", "Now"], ["map", "Map"], ["diagnose", "Diagnose"], ["incidents", "Incidents"], ["rules", "Rules"], ["data", "Data"]];
const ICONS = {
  search: "M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM20 20l-4-4",
  code: "m8 7-5 5 5 5M16 7l5 5-5 5",
  copy: "M8 8h11v11H8zM5 16V5h11",
};
const icon = (n) => svg("svg", { class: "i", viewBox: "0 0 24 24", "aria-hidden": "true" }, svg("path", { d: ICONS[n] }));

function render(el, ctx) {
  if (!document.getElementById("bn-css")) document.head.append(h("style", { id: "bn-css" }, CSS));
  const app = {
    ctx, queries: [], took: 0, geo: null, now: null,
    api: async (path) => {
      const j = await ctx.api(path, { timeout: 60000 });
      if (j && Array.isArray(j.queries)) { app.queries = j.queries; app.took = j.took_ms; drawSql(); }
      return j;
    },
    href: (...parts) => ctx.href(...parts.filter((x) => x !== "" && x != null)),
    go: (...parts) => { location.hash = app.href(...parts); },
    country: (cc) => app.geo?.countries?.[cc]?.n || cc || "",
  };
  const root = h("div", { class: "bn-root" });
  fill(el, root);
  const state = h("span", { class: "sub" });
  const sqlBtn = h("button", { class: "btn sm", type: "button", onclick: () => sqlPanel.classList.toggle("on"), title: "The SQL behind this page" });
  const head = h("div", { class: "bn-head" }, h("h1", {}, "Backbone explorer"), state, h("span", { class: "bn-synth" }, "real, public data"), h("div", { class: "tools" }, sqlBtn));
  const input = h("input", { class: "bn-cmd-in", type: "search", "aria-label": "Search", autocomplete: "off", spellcheck: "false", placeholder: "Search: a prefix, an address, an ASN, a network or an exchange (84.255.184.0/24, 1.1.1.1, AS6453, Cloudflare, DE-CIX)" });
  const cmd = h("div", { class: "bn-cmdrow" }, h("div", { class: "bn-cmd" }, h("span", { class: "bn-cmd-ic" }, icon("search")), input));
  const tabs = h("div", { class: "tabs bn-tabs", role: "tablist" });
  const main = h("main", { class: "bn-main" });
  const sqlPanel = h("div", { class: "bn-sqlp" });
  const foot = h("div", { class: "bn-foot" },
    "Public data: RIPE RIS (BGP updates), rpki-client (RPKI), CAIDA (relationships, prefix origins), RIPE NCC (names), PeeringDB (exchanges), RIPE Atlas (measurements), IODA (outages). Kept in ChloeDB tables; ",
    h("a", { href: app.href("data") }, "how it is built"), ".");
  root.append(head, cmd, tabs, main, sqlPanel, foot);

  function drawSql() {
    const total = app.queries.reduce((a, q) => a + (q.ms || 0), 0);
    fill(sqlBtn, icon("code"), `SQL · ${plural(app.queries.length, "query", "queries")} · ${(app.took || total).toFixed(1)} ms`);
    fill(sqlPanel,
      h("div", { class: "bn-sqlp-hd" }, h("b", {}, "The SQL behind this page"), h("span", { class: "muted small" }, "each statement, its rows and its time on the server"),
        h("button", { class: "btn sm ghost", type: "button", onclick: () => sqlPanel.classList.remove("on") }, "Close")),
      app.queries.map((q) => h("div", { class: "q" },
        h("div", { class: "w" }, h("span", {}, q.what), h("span", { class: "muted" }, `${fmt(q.rows)} rows · ${(q.ms ?? 0).toFixed(1)} ms`)),
        q.note ? h("div", { class: "bad" }, q.note) : null,
        h("pre", { class: "code" }, q.sql, q.params && Object.keys(q.params).length ? `\n-- ${JSON.stringify(q.params).slice(0, 400)}` : ""))));
  }
  drawSql();

  function drawTabs(page) {
    const open = app.now?.open || [];
    const bad = open.filter((i) => i.severity === "bad").length;
    const badge = { "": open.length || null, incidents: bad || null, map: app.now ? (app.now.open || []).length || null : null };
    const map = { as: "map", prefix: "map", incident: "incidents", search: "" };
    const cur = map[page] ?? page;
    fill(tabs, TABS.map(([p, t]) => h("button", { type: "button", role: "tab", "aria-selected": String(cur === p), class: cur === p ? "on" : null, onclick: () => app.go(p) },
      t, badge[p] ? h("span", { class: `n${p === "incidents" ? " bad" : ""}` }, String(badge[p])) : null)));
  }
  function drawState() {
    const n = app.now;
    if (!n) return fill(state, "");
    const open = n.open || [];
    const bad = open.some((i) => i.severity === "bad");
    fill(state, h("span", { class: `dot ${open.length ? (bad ? "bad" : "warn") : "ok"}${open.length ? " pulse" : ""}` }),
      ` ${open.length ? plural(open.length, "problem") + " now" : "Nothing wrong now"}${n.connected ? `, ${n.connected} of them connected` : ""} · as of ${clock(n.as_of)}`);
  }
  async function refreshNow() {
    try {
      const j = await ctx.api("/v1/now", { timeout: 60000 });
      app.now = j;
      drawState();
      drawTabs(current?.[0] || "");
    } catch { /* the page shows its own errors */ }
  }

  input.addEventListener("keydown", (e) => { if (e.key === "Enter") { e.preventDefault(); const q = input.value.trim(); if (q) app.go("search", q); } });
  document.addEventListener("keydown", (e) => { if (e.key === "/" && document.activeElement !== input && !/INPUT|TEXTAREA|SELECT/.test(document.activeElement?.tagName || "")) { e.preventDefault(); input.focus(); input.select(); } });

  let current = null, seq = 0;
  async function show(path) {
    path = path || [];
    current = path;
    const me = ++seq;
    const alive = () => me === seq;
    const [page, ...rest] = path;
    drawTabs(page || "");
    if (page !== "search") input.value = "";
    try {
      if (!app.geo) app.geo = await ctx.api("/ui/geo.json").catch(() => ({ countries: {} }));
      switch (page || "") {
        case "": return await nowPage(app, main, alive);
        case "map": return await mapPage(app, main, rest, alive);
        case "as": return await networkPage(app, main, rest[0], alive);
        case "prefix": return await prefixPage(app, main, rest.join("/"), alive);
        case "incidents": return await incidentsPage(app, main, alive);
        case "incident": return await incidentPage(app, main, rest.join("/"), alive);
        case "diagnose": return await diagnosePage(app, main, rest[0] || "", alive);
        case "rules": return await rulesPage(app, main, alive);
        case "data": return await dataPage(app, main, alive);
        case "search": input.value = rest.join("/"); return await searchPage(app, main, rest.join("/"), alive);
        default: fill(main, empty("No such page."));
      }
    } catch (e) {
      if (alive()) fill(main, h("div", { class: "error" }, String(e.message || e)));
    }
  }
  ctx.onPath((p) => { if (JSON.stringify(p) !== JSON.stringify(current)) { scrollTo({ top: 0 }); show(p); } });
  refreshNow();
  setInterval(() => { if (document.visibilityState === "visible" && el.isConnected) refreshNow(); }, 60000);
  show(ctx.path());
}

// ---------------------------------------------------------------- Now

function incidentRow(app, i) {
  const u = i.state !== "open" ? "u3" : i.severity === "bad" ? "u1" : "u2";
  const k = `${kindWord(i.kind)}${i.alerts > 1 ? ` ×${i.alerts}` : ""}`;
  return h("div", { class: `bn-work ${u}` },
    h("div", { class: "k" }, k),
    h("div", {}, h("div", { class: "t" }, i.title), h("div", { class: "s" }, i.summary, ` · since ${hm(i.first)}`, i.state !== "open" ? `, ended ${hm(i.last)}` : "")),
    h("a", { class: `btn xs${i.state === "open" ? " primary" : ""}`, href: app.href("incident", i.id) }, "Open"));
}

function summaryLines(n) {
  const open = n.open || [];
  const by = {};
  for (const i of open) (by[i.kind] = by[i.kind] || []).push(i);
  const lines = [];
  const names = (list, k = 3) => list.slice(0, k).map((i) => i.title).join("; ") + (list.length > k ? `; and ${list.length - k} more` : "");
  if (by.outage) lines.push(`${plural(by.outage.length, "outage")} open at IODA: ${names(by.outage, 2)}.`);
  const inv = [...(by.invalid || []), ...(by.hijack || [])];
  if (inv.length) lines.push(`${plural(inv.reduce((a, i) => a + i.alerts, 0), "RPKI-invalid route")} heard from several peers, in ${plural(inv.length, "group")}: ${names(inv, 2)}.`);
  if (by["new-origin"]) lines.push(`${plural(by["new-origin"].reduce((a, i) => a + i.alerts, 0), "prefix", "prefixes")} announced by a network that did not announce them yesterday.`);
  if (by["more-specific"]) lines.push(`${plural(by["more-specific"].reduce((a, i) => a + i.alerts, 0), "more specific route")} inside someone else's prefix.`);
  if (by.flapping) lines.push(`${plural(by.flapping.reduce((a, i) => a + i.alerts, 0), "prefix", "prefixes")} flapping: ${names(by.flapping, 2)}.`);
  if (by.withdrawn) lines.push(`${plural(by.withdrawn.reduce((a, i) => a + i.alerts, 0), "prefix", "prefixes")} withdrawn by most peers and not back.`);
  if (by.blackhole) lines.push(`${plural(by.blackhole.reduce((a, i) => a + i.alerts, 0), "blackholed prefix", "blackholed prefixes")}.`);
  if (by.slow) lines.push(`${names(by.slow, 3)}; the world's median is ${ms(n.world?.med)}.`);
  return lines;
}

async function nowPage(app, main, alive) {
  if (!app.now) fill(main, skel(6));
  const n = await app.api("/v1/now");
  if (!alive()) return;
  app.now = n;
  const open = n.open || [];
  const bad = open.some((i) => i.severity === "bad");
  const lines = summaryLines(n);
  const sentence = open.length
    ? `${plural(open.length, "problem")} now.${n.connected ? ` ${n.connected} of them are connected.` : ""}${n.ended?.length ? ` ${plural(n.ended.length, "has", "have")} ended in the last hour.`.replace(/^ (\d+) has/, " $1 has") : ""}`
    : "Nothing wrong now: no rule finds anything in the last hour.";
  const hero = h("section", { class: `bn-hero${open.length ? (bad ? "" : " warn") : " calm"}` },
    h("div", {},
      h("div", { class: "bn-hero-top" }, open.length ? pill(bad ? "bad" : "warn", plural(open.length, "problem") + " now") : pill("ok", "All clear"), h("span", { class: "muted small" }, `The last hour, as of ${clock(n.as_of)}`)),
      h("h2", {}, sentence.replace(/ (\d+) have ended/, " $1 have ended").replace(/1 has ended/, "1 has ended")),
      lines.length ? h("ul", { class: "bn-hero-ul" }, lines.map((l) => h("li", {}, l))) : null,
      h("div", { class: "meta" }, n.heard?.updates != null
        ? h("span", {}, `Heard by `, h("b", {}, `${fmt(n.heard.peers)} BGP peers at RIPE RIS`), ` in the last ${n.heard.minutes || 5} minutes: ${fmt(n.heard.updates)} updates of ${fmt(n.heard.prefixes)} prefixes.`)
        : "")),
    h("div", { class: "bn-hero-act" }, h("button", { class: "btn", type: "button", onclick: (e) => { navigator.clipboard?.writeText([sentence, ...lines.map((l) => "- " + l)].join("\n")); e.currentTarget.textContent = "Copied"; } }, icon("copy"), "Copy the summary")));

  const work = h("section", { class: "card" },
    h("div", { class: "hd" }, h("h3", {}, "What is wrong now"), h("span", { class: "sub" }, "most serious first; each row is a row of bb_incidents, and a click from its incident room")),
    h("div", { class: "bd flush" }, open.length ? open.map((i) => incidentRow(app, i)) : empty("No open incidents. The rules run every minute over the newest updates.")));

  const world = n.world?.med;
  const weather = card("Internet weather", `the time to the nearest K root server, slowest countries, last 20 minutes${n.world ? ` · ${fmt(n.world.probes)} RIPE Atlas probes` : ""}`,
    (n.weather || []).length ? hbars([
      ...(n.weather || []).map((w) => ({ label: app.country(w.country), href: app.href("map", w.country), text: ms(w.med), v: w.med, cls: w.med > 150 ? "bad" : w.med > 60 ? "warn" : "ok" })),
      { label: "World median", text: ms(world), v: world, cls: "ok" },
    ]) : empty("No Atlas results yet."));

  const byId = Object.fromEntries(open.map((i) => [i.id, i]));
  const connect = card("How they connect", "what the open problems share",
    (n.connect || []).length
      ? h("ul", { class: "bn-list" }, n.connect.map((c) => h("li", {},
        h("div", { class: "bn-conn-t" }, c.net ? h("a", { href: app.href("as", c.net.asn) }, c.title) : c.country ? h("a", { href: app.href("map", c.country) }, c.title.replace(c.country, app.country(c.country))) : c.title),
        h("div", { class: "muted small" }, c.how === "country" ? c.text.replace(c.country, app.country(c.country)) : c.text),
        h("div", { class: "bn-conn-ids" }, c.incidents.slice(0, 6).map((id) => byId[id] ? h("a", { class: "chip", href: app.href("incident", id) }, byId[id].title.slice(0, 48)) : null)))))
      : empty(open.length ? "No two open problems share an owner, an announcer, an upstream or a country." : "Nothing open to connect."));

  const ended = card("Ended in the last hour", null,
    (n.ended || []).length ? h("ul", { class: "bn-list" }, n.ended.slice(0, 8).map((i) => h("li", {},
      h("a", { class: "bn-conn-t", href: app.href("incident", i.id) }, i.title),
      h("div", { class: "muted small" }, `${hm(i.first)} to ${hm(i.last)} · ${i.summary}`)))) : empty("None."));

  const busiest = card("Busiest prefixes", "updates in the last hour",
    (n.busiest || []).length ? table(["prefix", "announced by", { t: "updates", num: true }], n.busiest.map((b) => h("tr", {},
      h("td", { class: "mono" }, h("a", { href: app.href("prefix", b.prefix) }, b.prefix)),
      h("td", {}, h("a", { href: app.href("as", b.origin.asn) }, b.origin.name || `AS${b.origin.asn}`), b.origin.country ? h("span", { class: "muted" }, `, ${b.origin.country}`) : null),
      h("td", { class: "num" }, fmt(b.updates))))) : empty("No updates yet."));

  const sources = card("Sources", "where each feed stands",
    (n.feed || []).length ? h("ul", { class: "bn-list" }, n.feed.map((f) => h("li", { class: "small" },
      h("b", {}, f.id), ` · ${f.kind}${f.rows ? ` · ${fmt(f.rows)} rows` : ""} · ${ago(f.at)}`, f.note ? h("div", { class: "muted" }, f.note) : null))) : empty("No feed has run yet."));

  fill(main, hero, h("div", { class: "bn-cols" },
    h("div", { class: "bn-col" }, work, weather),
    h("div", { class: "bn-col" }, connect, ended, busiest, sources)));
}

// ---------------------------------------------------------------- Map: the world

// Pan and zoom an SVG by its viewBox: drag, the wheel, and the buttons.
function panzoom(svgEl, full, { min = 1, max = 24 } = {}) {
  let vb = { ...full };
  const set = () => svgEl.setAttribute("viewBox", `${vb.x.toFixed(2)} ${vb.y.toFixed(2)} ${vb.w.toFixed(2)} ${vb.h.toFixed(2)}`);
  const clamp = () => {
    vb.w = Math.min(full.w / min, Math.max(full.w / max, vb.w));
    vb.h = vb.w * (full.h / full.w);
    vb.x = Math.min(full.x + full.w - vb.w * 0.5, Math.max(full.x - vb.w * 0.5, vb.x));
    vb.y = Math.min(full.y + full.h - vb.h * 0.5, Math.max(full.y - vb.h * 0.5, vb.y));
  };
  const zoom = (f, cx = vb.x + vb.w / 2, cy = vb.y + vb.h / 2) => {
    const w = vb.w / f;
    vb = { x: cx - ((cx - vb.x) * w) / vb.w, y: cy - ((cy - vb.y) * w) / vb.w, w, h: w * (full.h / full.w) };
    clamp(); set(); svgEl.dispatchEvent(new CustomEvent("zoomed", { detail: full.w / vb.w }));
  };
  const toSvg = (e) => { const r = svgEl.getBoundingClientRect(); return { x: vb.x + ((e.clientX - r.left) / r.width) * vb.w, y: vb.y + ((e.clientY - r.top) / r.height) * vb.h }; };
  svgEl.addEventListener("wheel", (e) => { e.preventDefault(); const p = toSvg(e); zoom(e.deltaY < 0 ? 1.25 : 0.8, p.x, p.y); }, { passive: false });
  let drag = null;
  svgEl.addEventListener("pointerdown", (e) => { drag = { x: e.clientX, y: e.clientY, vb: { ...vb }, moved: false }; });
  addEventListener("pointermove", (e) => {
    if (!drag) return;
    const r = svgEl.getBoundingClientRect();
    const dx = e.clientX - drag.x, dy = e.clientY - drag.y;
    if (Math.abs(dx) + Math.abs(dy) > 3) drag.moved = true;
    if (!drag.moved) return;
    vb.x = drag.vb.x - (dx / r.width) * vb.w; vb.y = drag.vb.y - (dy / r.height) * vb.h; clamp(); set();
  });
  addEventListener("pointerup", () => { if (drag?.moved) { svgEl.dataset.dragged = "1"; setTimeout(() => delete svgEl.dataset.dragged, 0); } drag = null; });
  set();
  return {
    zoom, fit: () => { vb = { ...full }; set(); svgEl.dispatchEvent(new CustomEvent("zoomed", { detail: 1 })); },
    to: (x, y, f) => { const w = full.w / f; vb = { x: x - w / 2, y: y - (w * full.h) / full.w / 2, w, h: (w * full.h) / full.w }; clamp(); set(); svgEl.dispatchEvent(new CustomEvent("zoomed", { detail: f })); },
  };
}

function project(geo, lat, lon) { return [((lon + 180) / 360) * geo.w, ((geo.top - lat) / geo.span) * geo.h]; }

async function mapPage(app, main, rest, alive) {
  const [cc, asn] = rest;
  if (cc) return countryMap(app, main, cc.toUpperCase(), asn, alive);
  fill(main, skel(8));
  const w = await app.api("/v1/map");
  if (!alive()) return;
  const geo = app.geo;
  const G = geo.countries || {};
  const byCc = Object.fromEntries((w.countries || []).map((c) => [c.cc, c]));
  // Where to pin each country: its outline's centre, else its largest exchange.
  const pinOf = {};
  for (const [cc, g] of Object.entries(G)) if (g.x != null) pinOf[cc] = [g.x, g.y];
  for (const x of w.ixes || []) if (x.lat != null && !pinOf[x.cc]) pinOf[x.cc] = project(geo, x.lat, x.lon);

  const layers = { problems: true, links: true, exchanges: false, probes: true };
  const stage = h("div", { class: "bn-map-stage" });
  const tip = h("div", { class: "bn-map-tip", hidden: true });
  const VB = { x: 0, y: 0, w: geo.w || 1000, h: geo.h || 383.3 };
  const s = svg("svg", { class: "bn-map-canvas bn-world", viewBox: `0 0 ${VB.w} ${VB.h}`, preserveAspectRatio: "xMidYMid meet", role: "img", "aria-label": "The world, countries coloured by what is wrong there" });
  const showTip = (e, html) => { const r = stage.getBoundingClientRect(); tip.innerHTML = html; tip.hidden = false; tip.style.left = `${Math.min(r.width - 260, e.clientX - r.left + 12)}px`; tip.style.top = `${e.clientY - r.top + 12}px`; };
  const hideTip = () => { tip.hidden = true; };
  const esc = (t) => String(t ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
  const tone = (c) => !c ? "none" : c.severity === "bad" ? "bad" : c.severity === "warn" ? "warn" : c.probes ? "ok" : "none";
  // Outlines that cross the antimeridian are drawn at both edges; the clip keeps them in the frame.
  const gLand = svg("g", { "clip-path": "url(#bn-frame)" }), gLinks = svg("g", {}), gIx = svg("g", {}), gPins = svg("g", {});
  for (const [cc, g] of Object.entries(G)) {
    if (!g.d) continue;
    const c = byCc[cc];
    gLand.append(svg("path", {
      class: `land c-${tone(c)}`, d: g.d,
      onclick: () => { if (!s.dataset.dragged) app.go("map", cc); },
      onmousemove: (e) => showTip(e, `<b>${esc(g.n)}</b><br>${c ? [c.alerts ? `${c.alerts} open alert${c.alerts === 1 ? "" : "s"}` : "no open alerts", c.probes ? `${c.probes} probes, median ${Math.round(c.med)} ms to K` : ""].filter(Boolean).join("<br>") : "no probes or alerts"}`),
      onmouseleave: hideTip,
    }));
  }
  // Lines: a network in one country announcing routes owned in another.
  for (const l of w.lines || []) {
    const a = pinOf[l.from], b = pinOf[l.to];
    if (!a || !b) continue;
    const mx = (a[0] + b[0]) / 2, my = Math.min(a[1], b[1]) - Math.abs(a[0] - b[0]) * 0.18 - 10;
    gLinks.append(svg("path", { class: "lk own", d: `M${a[0]},${a[1]} Q${mx},${my} ${b[0]},${b[1]}`, onclick: () => app.go("incident", l.incident) },
      svg("title", {}, `${app.country(l.from)} announced ${l.alerts} route${l.alerts === 1 ? "" : "s"} owned in ${app.country(l.to)}`)));
  }
  for (const x of w.ixes || []) {
    if (x.lat == null) continue;
    const [px, py] = project(geo, x.lat, x.lon);
    gIx.append(svg("path", { class: "ixd", d: `M${px},${py - 3.2}l3.2,3.2-3.2,3.2-3.2-3.2z`, onmousemove: (e) => showTip(e, `<b>${esc(x.name)}</b><br>${esc(x.city)} · ${x.members} members`), onmouseleave: hideTip, onclick: () => app.go("map", x.cc) }));
  }
  const pins = (w.countries || []).filter((c) => c.alerts > 0 || c.severity !== "ok").filter((c) => pinOf[c.cc]);
  pins.sort((a, b) => (a.severity === "bad") - (b.severity === "bad"));
  for (const c of pins) {
    const [px, py] = pinOf[c.cc];
    const r = c.alerts ? Math.min(13, 5 + Math.sqrt(c.alerts) * 1.1) : 4.5;
    const col = c.severity === "bad" ? "var(--crit)" : c.severity === "warn" ? "var(--warn)" : "var(--accent)";
    gPins.append(svg("g", { class: "pin", onclick: () => app.go("map", c.cc), onmousemove: (e) => showTip(e, `<b>${esc(app.country(c.cc))}</b><br>${c.alerts ? `${c.alerts} open alert${c.alerts === 1 ? "" : "s"}` : ""}${c.med ? `<br>median ${Math.round(c.med)} ms to K, ${c.probes} probes` : ""}`), onmouseleave: hideTip },
      c.severity === "bad" ? svg("circle", { class: "halo", cx: px, cy: py, r }) : null,
      svg("circle", { cx: px, cy: py, r, fill: col, stroke: "var(--surface)", "stroke-width": 1 }),
      c.alerts > 1 ? svg("text", { x: px, y: py + 2.5, "text-anchor": "middle", class: "pn" }, c.alerts > 99 ? "99+" : String(c.alerts)) : null));
  }
  s.append(svg("defs", {}, svg("clipPath", { id: "bn-frame" }, svg("rect", { x: 0, y: 0, width: VB.w, height: VB.h }))), gLand, gLinks, gIx, gPins);
  const pz = panzoom(s, VB);
  s.addEventListener("zoomed", (e) => s.style.setProperty("--z", e.detail));
  const chip = (key, label) => {
    const b = h("button", { class: "chip", type: "button", "aria-pressed": String(layers[key]), onclick: () => { layers[key] = !layers[key]; b.setAttribute("aria-pressed", String(layers[key])); draw(); } }, label);
    return b;
  };
  function draw() {
    gLinks.style.display = layers.links ? "" : "none";
    gIx.style.display = layers.exchanges ? "" : "none";
    gPins.style.display = layers.problems ? "" : "none";
  }
  draw();
  stage.append(s, tip,
    h("div", { class: "bn-map-crumbs" }, h("a", {}, "World")),
    h("div", { class: "bn-map-zoom" }, chip("problems", "Problems"), chip("links", "Who took whose"), chip("exchanges", "Exchanges"),
      h("button", { class: "btn sm", type: "button", onclick: () => pz.zoom(1.6) }, "+"), h("button", { class: "btn sm", type: "button", onclick: () => pz.zoom(1 / 1.6) }, "−"), h("button", { class: "btn sm", type: "button", onclick: () => pz.fit() }, "Fit")),
    h("div", { class: "bn-map-foot" }, h("div", { class: "bn-map-foot-row" },
      h("div", { class: "bn-map-legend" }, h("span", {}, h("i", { style: { background: "var(--crit)" } }), "invalid route, hijack or outage"), h("span", {}, h("i", { style: { background: "var(--warn)" } }), "flapping, new origin or slow"), h("span", {}, h("i", { style: { background: "var(--accent)" } }), "blackholing asked for"), h("span", {}, h("i", { style: { background: "color-mix(in srgb, var(--good) 45%, var(--surface))" } }), "probes, nothing wrong"), h("span", {}, h("i", { class: "bn-ln" }), "announced from another country")),
      h("div", { class: "bn-map-stats" }, `${plural(w.open_alerts || 0, "open alert")} in ${plural((w.countries || []).filter((c) => c.alerts).length, "country", "countries")} · click a country to zoom in`))));

  const rank = { bad: 0, warn: 1, info: 2, ok: 3 };
  const open = (w.incidents || []).slice().sort((a, b) => (rank[a.severity] ?? 2) - (rank[b.severity] ?? 2) || (b.alerts || 0) - (a.alerts || 0));
  const side = h("aside", { class: "bn-map-side" },
    h("div", { class: "bn-map-head" }, h("h3", {}, "Where it is red"),
      h("div", { class: "muted small" }, `${fmt(w.counts?.bb_as)} networks, ${fmt((w.counts?.bb_p2c || 0) + (w.counts?.bb_p2p || 0))} links between them, ${fmt(w.counts?.bb_ix)} exchanges and ${fmt(w.counts?.bb_probe)} probes in ChloeDB tables. Zoom in, or click anything.`)),
    h("div", { class: "bn-map-kpis" },
      h("div", {}, h("b", {}, fmt(open.length)), h("span", {}, "open incidents")),
      h("div", {}, h("b", {}, fmt(w.red_countries || 0)), h("span", {}, "countries red")),
      h("div", {}, h("b", {}, fmt(w.open_alerts || 0)), h("span", {}, "open alerts"))),
    sec("Incidents", "bb_incidents WHERE state = 'open'",
      open.length ? open.slice(0, 12).map((i) => row(SEV[i.severity] || SEV.info, i.title, `${i.summary} · since ${hm(i.first)}`, h("span", { class: "btn xs" }, "Open"), app.href("incident", i.id))) : empty("None open.")),
    sec("Red now", "bb_alerts WHERE state = 'open', newest first",
      (w.alerts || []).slice(0, 14).map((a) => row(SEV[a.severity] || SEV.info, a.title, [a.detail, a.country ? app.country(a.country) : ""].filter(Boolean).join(" · "), null,
        a.prefix ? app.href("prefix", a.prefix) : a.asn ? app.href("as", a.asn.asn) : a.country ? app.href("map", a.country) : app.href("incident", a.incident)))));

  const zoomCard = card("Zoom levels", "each level is a few statements over the same tables, so a click never waits for another system",
    h("div", { class: "bn-map-read" },
      h("div", {}, h("span", { class: "k" }, "World"), h("span", {}, "countries coloured by their open alerts and their probes' time to the K root server; lines where a network in one country announced routes owned in another.")),
      h("div", {}, h("span", { class: "k" }, "Country"), h("span", {}, "its exchanges, its networks with probes, each probe coloured by its time to K, and where slow traceroutes leave the country.")),
      h("div", {}, h("span", { class: "k" }, "Network"), h("span", {}, "what it hangs from (REACHABLE over CAIDA's relationships), trouble within three hops, its prefixes and their RPKI state, its changes and faults.")),
      h("div", {}, h("span", { class: "k" }, "Prefix"), h("span", {}, "every update of the prefix replayed into who holds it now, its origins, its upstreams and its ROAs."))));
  fill(main, h("div", { class: "bn-map" }, h("div", { class: "bn-map-main bn-wm" }, stage, side), zoomCard));
}

// ---------------------------------------------------------------- Map: a country

function probeColor(rtt) { return rtt == null ? "none" : rtt < 60 ? "ok" : rtt <= 150 ? "warn" : "bad"; }

async function countryMap(app, main, cc, asn, alive) {
  fill(main, skel(8));
  const c = await app.api(`/v1/country/${cc}`);
  if (!alive()) return;
  const name = app.country(cc);
  const cont = app.geo?.countries?.[cc]?.c || "";
  const nets = c.nets || [];
  const ixes = c.ixes || [];
  const W = 1000, PAD = 24, IXW = 140, IXH = 54, NW = 140, NH = 88;
  const ixY = 84, cols = 6;
  const gap = (W - 2 * PAD - 16 - cols * NW) / (cols - 1);
  const ixX = (i) => PAD + 16 + i * (IXW + gap);
  const shown = nets.slice(0, 11);
  const rows = Math.ceil((shown.length + (c.more?.networks ? 1 : 0)) / cols) || 1;
  const netY = (r) => 244 + r * (NH + 44);
  const H = netY(rows - 1) + NH + 40 + ((c.abroad || []).length ? 110 : 20);
  const s = svg("svg", { class: "bb-zm", viewBox: `0 0 ${W} ${H}`, role: "img", "aria-label": `${name}: exchanges, networks and RIPE Atlas probes, coloured by their time to the K root server` });
  s.append(svg("rect", { class: "ct reg", x: 8, y: 8, width: W - 16, height: H - ((c.abroad || []).length ? 118 : 28), rx: 14 }),
    svg("text", { class: "ctl", x: 24, y: 32 }, name),
    svg("text", { class: "cts", x: 24 + name.length * 8 + 16, y: 32 }, `${fmt(c.probes || 0)} probes · ${fmt(c.ix_total || 0)} exchanges · median ${ms(c.med)} to K`));
  const ixPos = {};
  ixes.forEach((x, i) => {
    const x0 = ixX(i);
    ixPos[x.id] = [x0 + IXW / 2, ixY + IXH];
    s.append(svg("g", { class: `ix${x.k ? " kr" : ""}` },
      svg("rect", { x: x0, y: ixY, width: IXW, height: IXH, rx: 8 }),
      svg("path", { class: "dm", d: `M${x0 + 14} ${ixY + 18} l7 -7 7 7 -7 7z` }),
      svg("text", { x: x0 + 30, y: ixY + 23 }, x.name.length > 15 ? x.name.slice(0, 14) + "…" : x.name),
      svg("text", { class: "s", x: x0 + 12, y: ixY + 42 }, `${x.members} members${x.k ? " · K root" : ""}`, svg("title", {}, `${x.name}, ${x.city}`))),
      x.k ? svg("g", { class: "kroot" }, svg("circle", { cx: x0 + IXW - 14, cy: ixY + 14, r: 9 }), svg("text", { x: x0 + IXW - 14, y: ixY + 18, "text-anchor": "middle" }, "K")) : null);
  });
  if (!ixes.length) s.append(svg("text", { class: "cts", x: 40, y: ixY + 30 }, "No exchanges in PeeringDB for this country."));
  const netPos = {};
  shown.forEach((n, i) => {
    const r = Math.floor(i / cols), col = i % cols;
    const x0 = ixX(col), y0 = netY(r);
    netPos[n.asn] = [x0 + NW / 2, y0];
    for (const ix of n.ixes || []) {
      const p = ixPos[ix];
      if (!p) continue;
      const k = ixes.find((x) => x.id === ix)?.k;
      s.insertBefore(svg("path", { class: `mb${k ? " bk" : ""}`, d: `M${x0 + NW / 2} ${y0} C ${x0 + NW / 2} ${y0 - 50}, ${p[0]} ${p[1] + 50}, ${p[0]} ${p[1]}` }), s.firstChild.nextSibling);
    }
  });
  shown.forEach((n, i) => {
    const r = Math.floor(i / cols), col = i % cols;
    const x0 = ixX(col), y0 = netY(r);
    const sel = String(n.asn) === String(asn);
    const g = svg("g", { class: `nt ${n.state}${sel ? " sel" : ""}`, style: "cursor:pointer", onclick: () => app.go("map", cc, n.asn) },
      svg("rect", { x: x0, y: y0, width: NW, height: NH, rx: 8 }),
      svg("text", { x: x0 + 10, y: y0 + 18 }, (n.name || `AS${n.asn}`).slice(0, 17)),
      svg("text", { class: "s", x: x0 + 10, y: y0 + 33 }, `AS${n.asn} · ${n.ixes_here ? `${n.ixes_here} exchange${n.ixes_here === 1 ? "" : "s"} here` : n.ixes_anywhere ? "abroad only" : "no exchange"}`),
      svg("title", {}, `${n.name} (AS${n.asn})`));
    (n.dots || []).slice(0, 16).forEach((d, j) => g.append(svg("circle", { class: `pb ${probeColor(d.rtt)}`, cx: x0 + 14 + (j % 8) * 15, cy: y0 + 50 + Math.floor(j / 8) * 13, r: 5 }, svg("title", {}, `probe ${d.probe}: ${d.rtt == null ? "no answer" : Math.round(d.rtt) + " ms"}`))));
    g.append(svg("text", { class: "s", x: x0 + 10, y: y0 + NH - 9 }, n.probes ? `${plural(n.probes, "probe")} · ${n.probes > 1 ? `median ${ms(n.med)}` : ms(n.med)}` : "no probes"));
    s.append(g);
  });
  if (c.more?.networks) {
    const i = shown.length, r = Math.floor(i / cols), col = i % cols;
    const x0 = ixX(col), y0 = netY(r);
    s.append(svg("g", { class: "nt more" }, svg("rect", { x: x0, y: y0, width: NW, height: NH, rx: 8 }),
      svg("text", { x: x0 + 10, y: y0 + 18 }, `${c.more.networks} more networks`),
      svg("text", { class: "s", x: x0 + 10, y: y0 + 60 }, `${c.more.under20} under 20 ms,`),
      svg("text", { class: "s", x: x0 + 10, y: y0 + 74 }, `${c.more.over90} over 90 ms`)));
  }
  const ab = c.abroad || [];
  if (ab.length) {
    const y0 = H - 92;
    s.append(svg("text", { class: "ctl", x: 24, y: y0 - 10 }, "Abroad"), svg("text", { class: "cts", x: 84, y: y0 - 10 }, "where slow traceroutes from here jump 80 ms or more (RIPE Atlas)"));
    const worst = shown.filter((n) => n.state === "bad" || n.state === "warn");
    ab.forEach((a, i) => {
      const x0 = 40 + i * 236;
      const from = worst[i % Math.max(1, worst.length)];
      if (from && netPos[from.asn]) {
        const [fx, fy] = netPos[from.asn];
        s.insertBefore(svg("path", { class: "ex", d: `M${fx} ${fy + NH} C ${fx} ${fy + NH + 60}, ${x0 + 110} ${y0 - 60}, ${x0 + 110} ${y0}` }), s.firstChild.nextSibling);
      }
      s.append(svg("g", { class: "ab", style: "cursor:pointer", onclick: () => app.go("as", a.net.asn) },
        svg("rect", { x: x0, y: y0, width: 220, height: 44, rx: 7 }),
        svg("text", { x: x0 + 10, y: y0 + 18 }, `${(a.net.name || "AS" + a.net.asn).slice(0, 24)}${a.net.country ? `, ${a.net.country}` : ""}`),
        svg("text", { class: "s", x: x0 + 10, y: y0 + 34 }, `${plural(a.traces, "trace")} · +${Math.round(a.jump)} ms at the jump · ${ms(a.med)}`)));
    });
  }
  const stage = h("div", { class: "bn-map-stage bn-scroll" },
    h("div", { class: "bn-zm-wrap" }, s),
    h("div", { class: "bn-map-crumbs" }, h("a", { href: app.href("map") }, "World"), h("span", { class: "muted" }, "›"), cont ? [h("span", {}, cont), h("span", { class: "muted" }, "›")] : null, h("a", { href: app.href("map", cc) }, name)),
    h("div", { class: "bn-map-foot" }, h("div", { class: "bn-map-foot-row" },
      h("div", { class: "bn-map-legend" }, h("span", {}, h("i", { style: { background: "var(--good)" } }), "probe under 60 ms"), h("span", {}, h("i", { style: { background: "var(--warn)" } }), "60 to 150 ms"), h("span", {}, h("i", { style: { background: "var(--crit)" } }), "over 150 ms"),
        h("span", {}, h("b", { style: { color: "var(--s7)", marginRight: "4px" } }, "◆"), "exchange"), h("span", {}, h("i", { style: { background: "var(--good)", borderRadius: "2px", height: "3px", width: "14px" } }), "member where K is"), ab.length ? h("span", {}, h("i", { style: { background: "var(--crit)", borderRadius: "2px", height: "3px", width: "14px" } }), "slow traces abroad") : null),
      h("div", { class: "bn-map-stats" }, `${shown.length} of ${nets.length + (c.more?.networks ? 0 : 0)} networks with probes · ${fmt(c.probes || 0)} probes · pings, last 20 min`))));

  const side = h("aside", { class: "bn-map-side" });
  fill(main, h("div", { class: "bn-map" }, h("div", { class: "bn-map-main bn-tall" }, stage, side)));
  if (asn) {
    fill(side, skel(6));
    const n = await app.api(`/v1/as/${asn}`);
    if (!alive()) return;
    fill(side, h("a", { class: "btn xs ghost", href: app.href("map", cc) }, `‹ Back to ${name}`), networkInspector(app, n, { country: c }));
  } else {
    const bad = (c.alerts || []).length;
    fill(side,
      h("a", { class: "btn xs ghost", href: app.href("map") }, "‹ Back to the world"),
      h("div", { class: "bn-map-head" },
        h("div", { class: "row between" }, h("h3", {}, name), c.med > 150 ? pill("bad", "slow") : c.med > 60 ? pill("warn", "slowish") : c.probes ? pill("ok", "fast") : null),
        h("div", { class: "muted small" }, [cont, `${fmt(c.ix_total || 0)} exchanges in PeeringDB`].filter(Boolean).join(" · ")),
        c.med != null ? h("div", { class: c.med > 100 ? "bn-map-why" : "bn-map-why ok" }, `${fmt(c.slow || 0)} of ${fmt(c.probes || 0)} probes take over 100 ms to reach the nearest K root server; the country's median is ${ms(c.med)}.`) : null,
        h("div", { class: "bn-facts" }, h("span", {}, "median ", h("b", {}, ms(c.med))), h("span", {}, "world ", h("b", {}, ms(app.now?.world?.med))), h("span", {}, "open alerts ", h("b", {}, fmt(bad))), h("span", {}, "outages ", h("b", {}, fmt((c.outages || []).length)))),
        h("a", { class: "btn xs", style: { marginTop: "8px" }, href: app.href("diagnose", cc) }, `Diagnose ${name}'s stack`)),
      sec("Its networks", "bb_ping GROUP BY asn, most probes first", nets.slice(0, 14).map((n) => row(SEV[n.state] || SEV.none, `${n.name} (AS${n.asn})`, `${plural(n.probes, "probe")} · median ${ms(n.med)} · ${n.ixes_here ? `${n.ixes_here} exchanges here` : "no exchange here"}${n.at_k ? " · meets K" : ""}`, null, app.href("map", cc, n.asn)))),
      (c.alerts || []).length ? sec("Open alerts", "bb_alerts WHERE country = " + cc, c.alerts.slice(0, 12).map((a) => row(SEV[a.severity], a.title, a.detail, null, a.prefix ? app.href("prefix", a.prefix) : app.href("incident", a.incident)))) : null,
      (c.outages || []).length ? sec("IODA outages", "bb_outage WHERE open", c.outages.map((o) => row(SEV.bad, o.name || o.id, `${o.kind} · ${o.source} · since ${stamp(o.start)} · score ${short(o.score)}`))) : null,
      ab.length ? sec("Where slow traces jump", "bb_trace WHERE rtt > 100 GROUP BY jump_as", ab.map((a) => row(SEV.warn, netName(a.net), `${plural(a.traces, "traceroute")} · +${Math.round(a.jump)} ms at that hop · ${ms(a.med)} in all`, null, app.href("as", a.net.asn)))) : null);
  }
}

// ---------------------------------------------------------------- a network

function networkInspector(app, n, { country = null } = {}) {
  if (!n.found) return empty(`AS${n.asn} is not in the registries' list of networks.`);
  const t = n.net;
  const sevOf = (s) => SEV[s] || SEV.ok;
  const open = (n.alerts || []).filter((a) => a.state === "open");
  const tabs = ["Overview", "Metrics", "Events", "Records"];
  let tab = "Overview";
  const body = h("div");
  const tabEl = h("div", { class: "tabs bn-otabs", role: "tablist" });
  const drawTabs = () => fill(tabEl, tabs.map((x) => h("button", { type: "button", "aria-selected": String(x === tab), class: x === tab ? "on" : null, onclick: () => { tab = x; drawTabs(); draw(); } }, x,
    x === "Events" && open.length ? h("span", { class: "n", style: { background: "var(--crit-soft)", color: "var(--crit-ink)" } }, String(open.length)) : null)));
  const why = open.length
    ? `${plural(open.length, "open alert")}: ${open.slice(0, 2).map((a) => a.title).join("; ")}`
    : n.med != null && n.med > 150 ? `${plural(n.probes_now, "probe")} reach the K root server in a median ${ms(n.med)}.` : null;
  const headEl = h("div", { class: "bn-map-head" },
    h("div", { class: "row between" }, h("h3", {}, `${t.name || t.full || "AS" + t.asn} (AS${t.asn})`), n.state !== "ok" ? pill(n.state, n.state === "bad" ? "red" : "watch") : pill("ok", "ok")),
    h("div", { class: "muted small" }, [t.kind, t.traffic, t.full && t.full !== t.name ? `registered as ${t.full}` : null].filter(Boolean).join(" · ")),
    h("div", { class: "muted small" }, [app.geo?.countries?.[t.country]?.c, app.country(t.country), t.scope].filter(Boolean).join(" › ")),
    why ? h("div", { class: "bn-map-why" }, why) : null,
    h("div", { class: "bn-facts" },
      h("span", {}, "providers ", h("b", {}, fmt(t.providers))), h("span", {}, "customers ", h("b", {}, fmt(t.customers))), h("span", {}, "peers ", h("b", {}, fmt(t.peers))),
      h("span", {}, "exchanges ", h("b", {}, fmt(t.ixes))), h("span", {}, "ROAs ", h("b", {}, fmt(t.roas))),
      n.med != null ? h("span", {}, "to K ", h("b", {}, ms(n.med))) : null, country?.med != null ? h("span", {}, "country ", h("b", {}, ms(country.med))) : null),
    h("div", { class: "bn-btnrow" }, h("a", { class: "btn xs", href: app.href("as", t.asn) }, "Open the network"),
      open[0]?.incident ? h("a", { class: "btn xs primary", href: app.href("incident", open[0].incident) }, "Open its incident") : null));
  const trouble = n.trouble || [];
  const verdict = trouble.length
    ? h("div", { class: "bn-map-verdict" }, h("b", {}, `Trouble ${trouble[0].hops} hop${trouble[0].hops === 1 ? "" : "s"} away: ${trouble[0].name} (AS${trouble[0].asn})`), h("div", { class: "small" }, trouble[0].title || ""))
    : null;
  function draw() {
    if (tab === "Overview") {
      fill(body,
        sec("What it hangs from", "REACHABLE(bb_p2c, asn, dir => 'in'): its providers", (n.providers || []).length
          ? n.providers.slice(0, 8).map((p) => row(sevOf(p.state), `${p.name || "AS" + p.asn} (AS${p.asn})`, `provider · ${p.country || "?"} · ${p.ixes ? `${p.ixes} exchanges` : "at no exchange"} · ${fmt(p.customers)} customers`, null, app.href("as", p.asn)))
          : h("div", { class: "small muted" }, "No providers in CAIDA's relationships: a transit-free network, or one CAIDA did not see.")),
        sec("What depends on it", "REACHABLE(bb_p2c, asn, dir => 'out'): its customers",
          h("div", { class: "small" }, h("b", {}, fmt(t.customers)), ` customer network${t.customers === 1 ? "" : "s"}, `, h("b", {}, fmt(t.roas)), " ROAs, ", h("b", {}, fmt(t.probes)), " Atlas probes"),
          (n.customers || []).slice(0, 5).map((p) => row(SEV.none, `${p.name || "AS" + p.asn} (AS${p.asn})`, `customer · ${p.country || "?"}${p.customers ? ` · ${fmt(p.customers)} customers of its own` : ""}`, null, app.href("as", p.asn)))),
        sec(`Trouble within ${n.trouble_hops === 1 ? "one hop" : "three hops"}`, `REACHABLE(bb_p2c, hops => ${n.trouble_hops}, dir => 'both') AND asn IN (open alerts)`,
          trouble.length ? trouble.map((x) => row(SEV[x.severity] || SEV.warn, `${x.name || "AS" + x.asn} (AS${x.asn})`, `${plural(x.hops, "hop")} · ${x.title || ""}`, null, app.href("as", x.asn))) : h("div", { class: "small muted" }, "No open alerts on networks that near.")),
        sec("Its routes now", "bb_updates WHERE origin = asn, last day, GROUP BY rpki",
          (n.rpki || []).length ? h("div", { class: "bn-facts" }, n.rpki.map((r) => h("span", {}, `${r.rpki} `, h("b", {}, fmt(r.prefixes)), " prefixes"))) : h("div", { class: "small muted" }, "rrc00 heard no announcement from it in the last day."),
          (n.prefixes || []).slice(0, 8).map((p) => row(SEV.none, p.prefix, `${fmt(p.updates)} updates · ${p.peers} peers · last ${stamp(p.last)}`, null, app.href("prefix", p.prefix)))));
    } else if (tab === "Metrics") {
      const pts = (n.minutes || []).map((m) => ({ t: m.t, a: m.n - m.w, w: m.w }));
      fill(body, sec("Updates of its prefixes a minute", "GROUP BY date_trunc('minute', ts), last three hours", h("div", { class: "bn-chart-box" }, minutes(pts, [{ key: "a", color: "var(--s1)", label: "announcements" }, { key: "w", color: "var(--s2)", label: "withdrawals" }], { height: 150 })), legend([["var(--s1)", "announcements"], ["var(--s2)", "withdrawals"]])),
        n.med != null ? sec("Its probes to K", "bb_ping, last 20 minutes", h("div", { class: "bn-facts" }, h("span", {}, "probes ", h("b", {}, fmt(n.probes_now))), h("span", {}, "median ", h("b", {}, ms(n.med))))) : null);
    } else if (tab === "Events") {
      fill(body, sec("Changes and faults", "bb_alerts and PeeringDB's records, newest first",
        (n.changes || []).length ? h("div", { class: "bn-map-hist" }, n.changes.map((c) => h("div", { class: c.kind }, h("span", { class: "when" }, stamp(c.at)), h("span", {}, h("b", {}, c.kind === "fault" ? "Fault " : "Change "), c.text, c.incident ? [" · ", h("a", { href: app.href("incident", c.incident) }, "incident")] : null)))) : h("div", { class: "small muted" }, "Nothing recorded.")));
    } else {
      fill(body,
        sec("Exchanges", "bb_member JOIN bb_ix", (n.ixes || []).length ? n.ixes.slice(0, 20).map((x) => row("var(--s7)", x.name, `${x.city}, ${x.cc} · ${x.speed >= 1000 ? x.speed / 1000 + " Gb/s" : x.speed + " Mb/s"}`, null, app.href("map", x.cc))) : h("div", { class: "small muted" }, "At no exchange in PeeringDB.")),
        sec("ROAs", "bb_roa WHERE asn = asn", (n.roas || []).length ? h("div", { class: "bn-mono-list" }, n.roas.slice(0, 40).map((r) => h("a", { href: app.href("prefix", r.prefix) }, `${r.prefix}${r.maxlen ? ` ≤/${r.maxlen}` : ""}`))) : h("div", { class: "small muted" }, "No ROAs name it.")),
        t.website ? sec("Record", "bb_as", h("div", { class: "small" }, h("a", { href: t.website, target: "_blank", rel: "noopener" }, t.website), t.pdb ? [" · ", h("a", { href: `https://www.peeringdb.com/net/${t.pdb}`, target: "_blank", rel: "noopener" }, "PeeringDB")] : null)) : null);
    }
  }
  drawTabs();
  draw();
  return h("div", {}, headEl, verdict, tabEl, body);
}

async function networkPage(app, main, asn, alive) {
  fill(main, skel(8));
  const n = await app.api(`/v1/as/${encodeURIComponent(asn || "")}`);
  if (!alive()) return;
  if (!n.found) return fill(main, empty(`AS${n.asn} is not in the registries' list of networks.`));
  const t = n.net;
  const pts = (n.minutes || []).map((m) => ({ t: m.t, a: m.n - m.w, w: m.w }));
  const pfx = card("Its prefixes", "announced in the last day, by updates; click one for its timeline",
    (n.prefixes || []).length ? table(["prefix", { t: "updates", num: true }, { t: "peers", num: true }, "last"], n.prefixes.map((p) => h("tr", {},
      h("td", { class: "mono" }, h("a", { href: app.href("prefix", p.prefix) }, p.prefix)), h("td", { class: "num" }, fmt(p.updates)), h("td", { class: "num" }, fmt(p.peers)), h("td", {}, stamp(p.last))))) : empty("rrc00 heard none of its announcements in the last day."));
  const chart = card("Updates a minute", "of the prefixes it originates, last three hours", h("div", { class: "bn-chart-box" }, minutes(pts, [{ key: "a", color: "var(--s1)", label: "announcements" }, { key: "w", color: "var(--s2)", label: "withdrawals" }], { height: 160 })), legend([["var(--s1)", "announcements"], ["var(--s2)", "withdrawals"]]));
  const alerts = card("Alerts", "about it, as origin, owner or network", (n.alerts || []).length ? h("div", { class: "bn-flush" }, n.alerts.slice(0, 20).map((a) => h("div", { class: `bn-work ${a.state !== "open" ? "u3" : a.severity === "bad" ? "u1" : "u2"}` },
    h("div", { class: "k" }, kindWord(a.kind)), h("div", {}, h("div", { class: "t" }, a.title), h("div", { class: "s" }, a.detail, ` · ${stamp(a.first)}${a.state !== "open" ? ` to ${stamp(a.last)}` : ", open"}`)),
    a.prefix ? h("a", { class: "btn xs", href: app.href("prefix", a.prefix) }, "Prefix") : h("span")))) : empty("No alerts about it."));
  fill(main, h("div", { class: "bn-cols wide" },
    h("div", { class: "bn-col" }, chart, alerts, pfx),
    h("div", { class: "bn-col" }, h("section", { class: "card bn-insp" }, networkInspector(app, n)),
      h("a", { class: "btn sm", href: app.href("map", t.country, t.asn) }, `See it on the map of ${app.country(t.country)}`))));
}

// ---------------------------------------------------------------- a prefix

async function prefixPage(app, main, p, alive) {
  fill(main, skel(8));
  const x = await app.api(`/v1/prefix/${p}`);
  if (!alive()) return;
  const held = x.holding || [];
  const owner = x.owner;
  const moas = held.length > 1;
  const status = x.state === "bad" ? "red" : x.state === "warn" ? "watch" : x.state === "ended" ? "ended" : "ok";
  const headline = held.length
    ? `${plural(x.held_now, "peer")} of rrc00's ${x.peers_heard} hold a route to it now, from ${held.map((o) => netName(o.origin.asn ? o.origin : null)).join(" and ")}.`
    : x.updates ? `No peer holds a route to it now: the last word of each was a withdrawal.` : "rrc00 heard nothing of it in the last 3 days.";
  const top = h("section", { class: `bn-hero ${x.state === "bad" ? "" : x.state === "warn" ? "warn" : "calm"}` },
    h("div", {},
      h("div", { class: "bn-hero-top" }, pill(x.state === "ended" ? "info" : x.state, status), h("span", { class: "muted small" }, `as of ${clock(x.as_of)}`)),
      h("h2", { class: "mono" }, x.prefix),
      h("p", {}, headline, owner ? [" Its owner, by RPKI and yesterday's origins, is ", h("a", { href: app.href("as", owner.asn) }, netName(owner)), "."] : ""),
      h("div", { class: "meta" }, h("span", {}, `${fmt(x.updates)}${x.capped ? "+" : ""} updates kept`), moas ? h("span", { class: "bad" }, "two or more origins now") : null)));
  const pts = x.minutes || [];
  const chart = card("Its updates a minute, and how many peers held it", "every update of the prefix, replayed in order",
    h("div", { class: "bn-chart-box" }, minutes(pts, [{ key: "a", color: "var(--s1)", label: "announcements" }, { key: "w", color: "var(--s2)", label: "withdrawals" }], { height: 160, line: "held", lineLabel: "peers holding it" })),
    legend([["var(--s1)", "announcements"], ["var(--s2)", "withdrawals"], ["var(--s7)", "peers holding a route"]]));
  const origins = card("Who announced it", "each origin: first and last heard, how many peers, its RPKI state",
    (x.origins || []).length ? table(["origin", { t: "peers", num: true }, "rpki", "against yesterday", "first", "last"], x.origins.map((o) => h("tr", {},
      h("td", {}, h("a", { href: app.href("as", o.origin.asn) }, netName(o.origin))), h("td", { class: "num" }, fmt(o.peers)),
      h("td", {}, h("span", { class: `badge ${o.rpki === "valid" ? "ok" : /invalid/.test(o.rpki) ? "bad" : ""}` }, o.rpki)), h("td", {}, o.seen), h("td", {}, stamp(o.first)), h("td", {}, stamp(o.last))))) : empty("No announcements."));
  const path = card("The path to it", "the newest announcement's AS path, from rrc00's peer to the origin",
    (x.path || []).length ? h("div", { class: "bb-hops" }, x.path.map((n, i) => [i ? h("div", { class: "bb-gap" }, h("span", {}, "")) : null, h("a", { class: `bb-hop${i === x.path.length - 1 ? " ok" : ""}`, href: app.href("as", n.asn) }, h("b", {}, n.name || `AS${n.asn}`), h("span", { class: "ip" }, `AS${n.asn}${n.country ? " · " + n.country : ""}`))])) : empty("None held."),
    (x.upstreams || []).length ? h("div", { class: "bn-facts" }, "upstreams: ", x.upstreams.map((u) => h("span", {}, h("a", { href: app.href("as", u.net.asn) }, u.net.name || `AS${u.net.asn}`), ` ${fmt(u.routes)}`))) : null);
  const roas = card("ROAs that cover it", "bb_roa WHERE prefix IN (the prefix at each shorter length)",
    (x.roas || []).length ? table(["ROA", "may announce", { t: "up to", num: true }, "trust anchor", "expires"], x.roas.map((r) => h("tr", {},
      h("td", { class: "mono" }, r.prefix), h("td", {}, h("a", { href: app.href("as", r.asn) }, `${r.name || ""} (AS${r.asn})`)), h("td", { class: "num" }, `/${r.maxlen}`), h("td", {}, r.ta), h("td", {}, day(r.expires))))) : empty("No ROA covers it: its RPKI state is unknown."));
  const around = card("Around it", "routes covering it, and inside it (lo and hi within its range), last day",
    h("div", { class: "bn-two" },
      h("div", {}, h("div", { class: "lbl" }, "Covering"), (x.covering || []).length ? x.covering.map((r) => row(SEV.none, r.prefix, `${netName(r.origin)} · ${r.peers} peers`, null, app.href("prefix", r.prefix))) : h("div", { class: "small muted" }, "none heard")),
      h("div", {}, h("div", { class: "lbl" }, "Inside"), (x.inside || []).length ? x.inside.map((r) => row(SEV.none, r.prefix, `${netName(r.origin)} · ${r.peers} peers`, null, app.href("prefix", r.prefix))) : h("div", { class: "small muted" }, "none heard"))));
  const events = card("Changes and faults", "who began and stopped announcing it, and the alerts",
    (x.events || []).length ? h("div", { class: "bn-map-hist" }, x.events.slice(0, 30).map((e) => h("div", { class: e.kind }, h("span", { class: "when" }, stamp(e.at)), h("span", {}, e.text, e.incident ? [" · ", h("a", { href: app.href("incident", e.incident) }, "incident")] : null)))) : empty("Nothing changed."));
  fill(main, top, h("div", { class: "bn-cols" }, h("div", { class: "bn-col" }, chart, origins, path, around), h("div", { class: "bn-col" }, events, roas)));
}

// ---------------------------------------------------------------- incidents

async function incidentsPage(app, main, alive) {
  fill(main, skel(8));
  const j = await app.api("/v1/incidents");
  if (!alive()) return;
  const list = j.incidents || [];
  fill(main, h("section", { class: "card" },
    h("div", { class: "hd" }, h("h3", {}, "Incidents"), h("span", { class: "sub" }, "open first, then the last day's; alerts grouped by the owner, announcer or country they share")),
    h("div", { class: "bd flush" }, list.length ? list.map((i) => incidentRow(app, i)) : empty("No incidents in the last day."))));
}

function graphSvg(app, g) {
  const roles = ["announcer", "upstream", "owner"];
  const cols = roles.map((r) => (g.nodes || []).filter((n) => n.role === r).slice(0, 8));
  const W = 900, CW = 220, NH = 40, GAP = 14;
  const H = Math.max(140, 40 + Math.max(...cols.map((c) => c.length)) * (NH + GAP));
  const pos = {};
  const s = svg("svg", { class: "bb-graph", viewBox: `0 0 ${W} ${H}`, role: "img" });
  const x0 = [20, (W - CW) / 2, W - CW - 20];
  roles.forEach((r, ci) => {
    const label = r === "announcer" ? "announced the routes" : r === "upstream" ? "carried them (last hop before the origin)" : cols[0].length ? "own the address space" : "own and announce the space";
    if (cols[ci].length) s.append(svg("text", { class: "col", x: x0[ci], y: 16 }, label));
    cols[ci].forEach((n, i) => { pos[n.asn] = [x0[ci], 30 + i * (NH + GAP)]; });
  });
  for (const e of g.edges || []) {
    const a = pos[e.from], b = pos[e.to];
    if (!a || !b) continue;
    const [ax, ay] = a[0] < b[0] ? [a[0] + CW, a[1] + NH / 2] : [a[0], a[1] + NH / 2];
    const [bx, by] = a[0] < b[0] ? [b[0], b[1] + NH / 2] : [b[0] + CW, b[1] + NH / 2];
    const mx = (ax + bx) / 2;
    s.append(svg("path", { class: `e ${e.kind === "took" ? "own" : "car"}`, d: `M${ax},${ay} C${mx},${ay} ${mx},${by} ${bx},${by}` }, svg("title", {}, `${e.kind === "took" ? "announced routes owned by" : "carried routes of"}: ${e.n}`)));
  }
  roles.forEach((r, ci) => cols[ci].forEach((n) => {
    const [x, y] = pos[n.asn];
    s.append(svg("g", { class: `nd ${r === "announcer" ? "bad" : r === "upstream" ? "hub" : "warn"}`, style: "cursor:pointer", onclick: () => app.go("as", n.asn) },
      svg("rect", { x, y, width: CW, height: NH, rx: 7 }),
      svg("text", { x: x + 10, y: y + 17 }, `${(n.name || "AS" + n.asn).slice(0, 26)}`),
      svg("text", { class: "s", x: x + 10, y: y + 31 }, `AS${n.asn}${n.country ? " · " + n.country : ""} · ${n.n}`)));
  }));
  return s;
}

async function incidentPage(app, main, id, alive) {
  fill(main, skel(8));
  const j = await app.api(`/v1/incident/${encodeURIComponent(id)}`);
  if (!alive()) return;
  if (!j.found) return fill(main, empty("No such incident: it may have ended more than a day ago."));
  const i = j.incident;
  const top = h("section", { class: `bn-hero${i.state !== "open" ? " calm" : i.severity === "bad" ? "" : " warn"}` },
    h("div", {},
      h("div", { class: "bn-hero-top" }, pill(i.state === "open" ? i.severity : "info", i.state === "open" ? `${kindWord(i.kind)} · open` : `${kindWord(i.kind)} · ended`), h("span", { class: "muted small" }, `${stamp(i.first)} to ${i.state === "open" ? "now" : stamp(i.last)} · ${span(i.first, i.last)}`)),
      h("h2", {}, i.title),
      h("p", {}, i.summary),
      h("div", { class: "meta" }, h("span", {}, plural(i.alerts, "alert")), (i.countries || []).length ? h("span", {}, i.countries.map((c) => app.country(c)).join(", ")) : null, h("span", { class: "mono muted" }, i.shared))));
  const pts = (j.minutes || []).map((m) => ({ t: m.t, a: m.a, w: m.w, peers: m.peers }));
  const chart = (j.minutes || []).length ? card("Its prefixes' updates a minute", "from half an hour before to half an hour after",
    h("div", { class: "bn-chart-box" }, minutes(pts, [{ key: "a", color: "var(--s1)", label: "announcements" }, { key: "w", color: "var(--s2)", label: "withdrawals" }], { height: 150, line: "peers", lineLabel: "peers that sent one" })),
    legend([["var(--s1)", "announcements"], ["var(--s2)", "withdrawals"], ["var(--s7)", "peers"]])) : null;
  const graph = (j.graph?.nodes || []).length > 1 ? card("How it connects", "announcers, the networks that carried their routes, and the owners of the space", h("div", { class: "bn-graph-box" }, graphSvg(app, j.graph))) : null;
  const alerts = card("Its alerts", "each a row a rule found", h("div", { class: "bn-flush" }, (j.alerts || []).map((a) => h("div", { class: `bn-work ${a.state !== "open" ? "u3" : a.severity === "bad" ? "u1" : "u2"}` },
    h("div", { class: "k" }, kindWord(a.kind)),
    h("div", {}, h("div", { class: "t" }, a.title), h("div", { class: "s" }, a.detail, a.peers ? ` · ${a.peers} peers` : "", ` · ${stamp(a.first)}${a.state !== "open" ? ` to ${stamp(a.last)}` : ", open"}`)),
    a.prefix ? h("a", { class: "btn xs", href: app.href("prefix", a.prefix) }, "Prefix") : a.asn ? h("a", { class: "btn xs", href: app.href("as", a.asn.asn) }, "Network") : a.country ? h("a", { class: "btn xs", href: app.href("map", a.country) }, "Country") : h("span")))));
  const ups = (j.upstreams || []).length ? card("Carried by", "the network before the origin on the routes heard", (j.upstreams || []).map((u) => row(SEV.info, netName(u.net), `${plural(u.prefixes, "prefix", "prefixes")}`, null, app.href("as", u.net.asn)))) : null;
  const nets = (i.asns || []).length ? card("Networks in it", null, (i.asns || []).slice(0, 12).map((a) => row(SEV[i.severity] || SEV.info, `AS${a}`, null, null, app.href("as", a)))) : null;
  fill(main, top, h("div", { class: "bn-cols" }, h("div", { class: "bn-col" }, graph, chart, alerts), h("div", { class: "bn-col" }, ups, nets)));
}

// ---------------------------------------------------------------- Diagnose

const LAYERS = [["users", "Users", "what RIPE Atlas probes there measure to the nearest K root server"], ["path", "Path", "traceroutes whose time jumps 80 ms or more at one hop: the packets left the region"], ["peering", "Peering", "probes over 100 ms: their networks do not reach a node near them"], ["routing", "Routing", "open routing alerts of networks registered there"], ["outages", "Outages", "IODA's open outages there"], ["server", "K root", "pings the K root server did not answer"]];

async function diagnosePage(app, main, focus, alive) {
  fill(main, skel(8));
  const j = await app.api(`/v1/diagnose${focus ? `?cc=${encodeURIComponent(focus)}` : ""}`);
  if (!alive()) return;
  const cols = j.columns || [];
  const sel = cols.find((c) => c.cc === focus) || cols.find((c) => c.start) || cols[0];
  const grid = h("div", { class: "bn-stack", style: { gridTemplateColumns: `110px repeat(${cols.length}, minmax(44px, 1fr))` } },
    h("span"), cols.map((c) => h("a", { class: "ch", href: app.href("diagnose", c.cc), title: app.country(c.cc) }, c.cc)),
    LAYERS.map(([k, label, why]) => [h("span", { class: "rh", title: why }, label),
      cols.map((c) => h("button", { type: "button", class: `cell ${c[k].state}${c.start === k ? " start" : ""}${sel && c.cc === sel.cc ? " on" : ""}`, title: `${app.country(c.cc)}: ${c[k].v}, ${c[k].why}`, onclick: () => app.go("diagnose", c.cc) }, c[k].state === "ok" || c[k].state === "none" ? "" : c[k].v))]));
  const stack = cardTools("The stack, country by country", "the slowest countries to K, and those with the most probes; a ring marks the lowest layer that is red, where to start",
    legend([["color-mix(in srgb, var(--good) 22%, var(--surface))", "fine"], ["var(--warn)", "watch"], ["var(--crit)", "red"]]), grid);
  let detail = null;
  if (sel) {
    const name = app.country(sel.cc);
    const start = sel.start ? LAYERS.find((l) => l[0] === sel.start) : null;
    detail = h("section", { class: "card" },
      h("div", { class: "hd" }, h("h3", {}, name), h("span", { class: "sub" }, `${fmt(sel.probes)} probes · median ${ms(sel.med)} · world ${ms(j.world)}`)),
      h("div", { class: "bd" },
        start ? h("div", { class: "bn-map-verdict" }, h("b", {}, `Start at ${start[1]}`), h("div", { class: "small" }, `${sel[start[0]].v}: ${sel[start[0]].why}. The layers below it are fine, so this is the lowest place that explains the rest.`)) : h("div", { class: "bn-map-verdict calm" }, h("b", {}, "Nothing red"), h("div", { class: "small" }, "Every layer is fine or only worth watching.")),
        h("ul", { class: "bn-check" }, LAYERS.map(([k, label]) => h("li", {}, dot(SEV[sel[k].state] || SEV.none), h("b", {}, label), ` ${sel[k].v} · `, h("span", { class: "muted" }, sel[k].why)))),
        h("div", { class: "bn-btnrow" }, h("a", { class: "btn sm", href: app.href("map", sel.cc) }, `${name} on the map`))));
  }
  const how = card("How to read it", null, h("div", { class: "bn-map-read" }, LAYERS.map(([, label, why]) => h("div", {}, h("span", { class: "k" }, label), h("span", {}, why)))));
  fill(main, h("div", { class: "bn-cols" }, h("div", { class: "bn-col" }, stack), h("div", { class: "bn-col" }, detail, how)));
}

// ---------------------------------------------------------------- rules, data, search

async function rulesPage(app, main, alive) {
  fill(main, skel(8));
  const j = await app.api("/v1/rules");
  if (!alive()) return;
  fill(main, h("section", { class: "card" },
    h("div", { class: "hd" }, h("h3", {}, "Alert rules"), h("span", { class: "sub" }, "each a SQL statement over the tables, run every minute; its rows are the alerts, grouped into incidents")),
    h("div", { class: "bd" }, (j.rules || []).map((r) => h("div", { class: "bn-rule" },
      h("div", { class: "row between" }, h("div", {}, dot(SEV[r.severity] || SEV.info, true), h("b", {}, r.title), h("span", { class: "muted small" }, ` · ${r.id}`)),
        h("span", { class: "muted small" }, r.last_run ? `${fmt(r.open)} open · ${fmt(r.last_rows)} rows in ${r.last_ms} ms, ${ago(r.last_run)}` : "not run yet")),
      h("div", { class: "small" }, r.about),
      h("pre", { class: "code" }, r.sql))))));
}

async function dataPage(app, main, alive) {
  fill(main, skel(8));
  const j = await app.api("/v1/scale");
  if (!alive()) return;
  const c = j.counts || {};
  const size = Object.fromEntries((j.sizes || []).map((s) => [s.table, s.bytes]));
  const TABLES = [["bb_updates", "every BGP update RIPE RIS's rrc00 heard, checked against RPKI and yesterday's origins as it arrived (kept 3 days)"], ["bb_as", "networks: the RIRs' names and countries, PeeringDB's records, CAIDA's counts"], ["bb_p2c", "provider to customer edges (CAIDA), walked by REACHABLE"], ["bb_p2p", "peer edges (CAIDA)"], ["bb_roa", "RPKI's validated ROAs (rpki-client)"], ["bb_ix", "internet exchanges (PeeringDB)"], ["bb_member", "who is on which exchange"], ["bb_probe", "RIPE Atlas probes"], ["bb_ping", "Atlas pings to the K root server, a row per probe and round (30 days)"], ["bb_trace", "Atlas traceroutes to K, with where the time jumps (30 days)"], ["bb_outage", "IODA's outages"], ["bb_alerts", "what the rules found"], ["bb_incidents", "alerts grouped"]];
  const tbl = card("The tables", "rows now, and bytes on disk", table(["table", { t: "rows", num: true }, { t: "on disk", num: true }, "what"], TABLES.map(([t, what]) => h("tr", {},
    h("td", { class: "mono" }, t), h("td", { class: "num" }, fmt(c[t])), h("td", { class: "num" }, size[t] ? `${short(size[t])}B` : ""), h("td", { class: "small" }, what)))));
  const pts = (j.minutes || []).map((m) => ({ t: m.t, a: m.n }));
  const chart = card("Updates heard a minute", "from the rollup bb_updates_1m, last three hours", h("div", { class: "bn-chart-box" }, minutes(pts, [{ key: "a", color: "var(--s1)", label: "updates" }], { height: 140 })));
  const rules = card("Each rule's last run", "on the server, every minute", table(["rule", { t: "rows", num: true }, { t: "ms", num: true }], (j.rules || []).map((r) => h("tr", {}, h("td", {}, r.title), h("td", { class: "num" }, fmt(r.rows)), h("td", { class: "num" }, r.ms)))));
  const feed = card("Sources", "where each stands", (j.feed || []).length ? h("ul", { class: "bn-list" }, j.feed.map((f) => h("li", { class: "small" }, h("b", {}, f.id), ` · ${f.kind}${f.rows ? ` · ${fmt(f.rows)} rows` : ""} · ${ago(f.at)}`, f.note ? h("div", { class: "muted" }, f.note) : null))) : empty("No feed has run."));
  const how = card("How it is built", null, h("div", { class: "bn-prose" },
    h("p", {}, "Each BGP update RIPE RIS's collector rrc00 hears (its archive every five minutes, then RIS Live as it happens) becomes a row of bb_updates, checked as it arrives against every validated ROA (RFC 6811 origin validation) and against the origins RouteViews saw yesterday (CAIDA's prefix2as). Everything else is a table beside it: CAIDA's relationships as two edge tables that REACHABLE walks, PeeringDB's exchanges and members, RIPE Atlas's pings and traceroutes to the K root server from every probe, IODA's outages."),
    h("p", {}, "Eight alert rules are SQL statements over those tables, run every minute; their rows are the alerts, and alerts that share an owner, an announcer or a country become an incident. Each page is a handful of statements: the SQL button shows them, with their rows and time."),
    h("p", { class: "muted small" }, `This build: one collector (rrc00, Amsterdam), on one server, as of ${clock(j.as_of)}.`)));
  fill(main, h("div", { class: "bn-cols" }, h("div", { class: "bn-col" }, how, tbl, chart), h("div", { class: "bn-col" }, feed, rules)));
}

async function searchPage(app, main, q, alive) {
  fill(main, skel(6));
  const j = await app.api(`/v1/search?q=${encodeURIComponent(q)}`);
  if (!alive()) return;
  if (j.kind === "asn") { location.replace(app.href("as", j.asn)); return; }
  if (j.kind === "prefix") {
    const best = (j.routes || [])[0];
    fill(main, card(`Routes covering ${j.prefix}`, "the most specific first", (j.routes || []).length ? (j.routes || []).map((r) => row(r === best ? SEV.info : SEV.none, r.prefix, `${netName(r.origin)} · ${fmt(r.updates)} updates`, null, app.href("prefix", r.prefix))) : empty("rrc00 heard no route covering it in the last 30 days.")),
      (j.roas || []).length ? card("ROAs covering it", null, j.roas.map((r) => row(SEV.ok, r.prefix, `AS${r.asn} ${r.name || ""} · up to /${r.maxlen}`, null, app.href("as", r.asn)))) : null);
    return;
  }
  fill(main, h("div", { class: "bn-cols" },
    h("div", { class: "bn-col" }, card("Networks", "by name (BM25 over bb_as.name)", (j.nets || []).length ? j.nets.map((n) => row(SEV.none, `${n.name || "AS" + n.asn} (AS${n.asn})`, [n.kind, app.country(n.country), n.customers ? `${fmt(n.customers)} customers` : ""].filter(Boolean).join(" · "), null, app.href("as", n.asn))) : empty("No network by that name."))),
    h("div", { class: "bn-col" }, card("Exchanges", "by name", (j.ixes || []).length ? j.ixes.map((x) => row("var(--s7)", x.name, `${x.city}, ${app.country(x.country)} · ${x.members} members`, null, app.href("map", x.country))) : empty("No exchange by that name.")))));
}

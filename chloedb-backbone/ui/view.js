// The internet's backbone on ChloeDB: the page, in ChloeDB's command center
// (#/app/backbone, loaded through chloedb-server from this app) and on its
// own (the app's / when it has no token, through ui/index.html). It imports
// nothing and builds its own elements, so the same file serves both. Its
// look is NetOps' (chloedb-netops/ui/view.js).
//
// The console is one screen that stays while the reader moves: the last
// six hours along the top (every update a minute, RPKI-invalid ones, the
// incidents), the networks they watch, the worklist on the left, a map in
// the middle whose camera flies from the world to a country to a network
// (van Wijk and Nuij's zoom path), and an inspector on the right with
// what is wrong, what changed, the numbers and the records. A click moves
// the camera and fills the inspector; nothing else is built again.
//
//   (home)                what it is, what is wrong now, how to read it
//   map                   the console on the world: what is wrong now
//   map/<CC>[/<asn>]      a country: its buildings, exchanges, networks and
//                         probes; a network in it, its buildings and the
//                         providers it buys transit from
//   as/<asn>              a network, over its home country
//   prefix/<p>            a prefix: its paths now, new and gone in the
//                         last hour, its ROAs, every update replayed
//   incident/<id>         one incident: its places on the map, its window
//                         on the timeline, its alerts and graph
//   incidents             the last day's incidents
//   diagnose[/<CC>]       each country's stack, from its users down to K
//   rules                 the alert rules, each a SQL statement
//   data                  how it is built, and how big it is
//   search/<q>            a prefix, an address, a network or an exchange
//
// Every answer of the API carries the SQL it ran: the "SQL" button shows
// the queries behind the view on screen.

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
.bn-main { position: relative; transition: opacity .15s; }
.bn-main.bn-loading { opacity: .6; pointer-events: none; }
.bn-main.bn-loading::before { content: ""; position: absolute; left: 0; top: -8px; height: 2px; width: 40%; background: var(--accent); border-radius: 2px; animation: bnbar 1s ease-in-out infinite; }
@keyframes bnbar { 0% { left: 0; width: 10%; } 50% { left: 45%; width: 40%; } 100% { left: 90%; width: 10%; } }
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
/* ---- the frame */
.bn-home-link { color: inherit; text-decoration: none; }
.bn-home-link:hover { color: var(--accent); text-decoration: none; }
.bn-bar { display: flex; align-items: flex-end; gap: 12px; margin-bottom: 10px; border-bottom: 1px solid var(--line); }
.bn-bar .bn-tabs { border-bottom: 0; margin: 0; flex: 1; min-width: 0; overflow-x: auto; scrollbar-width: none; }
.bn-bar .bn-cmd { width: min(440px, 40vw); height: 34px; margin-bottom: 6px; flex: none; }
@media (max-width: 760px) { .bn-bar { flex-direction: column-reverse; align-items: stretch; gap: 6px; } .bn-bar .bn-cmd { width: auto; } }
.bn-conhost[hidden], .bn-main[hidden], .bn-foot[hidden] { display: none; }

/* ---- the console */
.bn-con { display: grid; gap: 8px; }
.bn-con-body { display: grid; grid-template-columns: 300px minmax(0, 1fr) 400px; gap: 10px; height: calc(100vh - 220px); min-height: 540px; transition: grid-template-columns .25s ease; }
.bn-con.wide .bn-con-body { grid-template-columns: 300px minmax(0, 1fr) 620px; }
.bn-wl, .bn-in { border: 1px solid var(--line); border-radius: var(--r); background: var(--surface); overflow-y: auto; min-width: 0; min-height: 0; position: relative; }
.bn-stage { position: relative; border: 1px solid var(--line); border-radius: var(--r); background: color-mix(in srgb, var(--accent) 5%, var(--surface-2)); overflow: hidden; min-width: 0; min-height: 0; }
@media (max-width: 1100px) {
  .bn-con-body, .bn-con.wide .bn-con-body { grid-template-columns: minmax(0, 1fr); height: auto !important; }
  .bn-stage { height: 62vh; min-height: 380px; order: 1; }
  .bn-in { order: 2; max-height: none; }
  .bn-wl { order: 3; max-height: 70vh; }
}
.bn-in.bn-busy::before { content: ""; position: sticky; display: block; top: 0; height: 2px; width: 40%; background: var(--accent); border-radius: 2px; animation: bnbar 1s ease-in-out infinite; z-index: 3; margin-bottom: -2px; }
.bn-in.bn-busy > :not(.bn-in-hd) { opacity: .6; transition: opacity .2s .15s; }
.bn-in-hd { padding: 10px 14px 10px; border-bottom: 1px solid var(--line); display: grid; gap: 3px; }
.bn-in-hd h3 { margin: 0; font-size: 15.5px; overflow-wrap: anywhere; line-height: 1.3; }
.bn-in-hd h3.mono { font-family: var(--mono); font-size: 15px; }
.bn-in-crumbs { display: flex; gap: 5px; align-items: center; font-size: 11.5px; color: var(--text-2); min-height: 22px; }
.bn-in-crumbs a { color: var(--text-2); }
.bn-in-crumbs a:hover { color: var(--accent); }
.bn-in-wide { margin-left: auto; padding: 2px 6px; }
.bn-in-wide .i { width: 13px; height: 13px; }
.bn-in-sum { padding: 10px 14px 0; font-size: 13px; }
.bn-in-sum ul { margin: 0 0 8px; padding-left: 18px; display: grid; gap: 4px; }
.bn-in-sum p { margin: 0 0 8px; color: var(--text-2); }
.bn-in-body { padding-bottom: 14px; }
.bn-in .bn-otabs { top: 0; }
.bn-otabs .n.bad { background: var(--crit-soft); color: var(--crit-ink); }
.bn-otabs .n.warn { background: var(--warn-soft); color: var(--text); }
.bn-otabs .n.info { background: var(--accent-soft); color: var(--accent); }
.bn-none { padding: 2px 0 4px; }
.bn-btnrow { display: flex; gap: 6px; flex-wrap: wrap; margin-top: 6px; }
.bn-watch-btn .i { width: 13px; height: 13px; fill: none; stroke: currentColor; stroke-width: 1.8; }
.bn-watch-btn.on { background: var(--accent-soft); color: var(--accent); border-color: transparent; }
.bn-watch-btn.on .i { fill: currentColor; }
.bn-conn { padding: 4px 0 8px; border-bottom: 1px solid var(--line); margin-bottom: 6px; }
.bn-conn:last-child { border-bottom: 0; }
.bn-conn-ids { display: flex; gap: 4px; flex-wrap: wrap; margin-top: 4px; }
.bn-conn-ids .chip { font-size: 11px; }
.bn-path { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 8px; align-items: center; padding: 5px 8px; margin: 0 -6px 3px; border-radius: var(--r-sm); border-left: 3px solid var(--line-2); background: var(--surface-2); font-size: 12px; }
.bn-path.new { border-left-color: var(--accent); background: var(--accent-soft); }
.bn-path.gone { border-left-color: var(--line-2); opacity: .75; text-decoration: line-through; text-decoration-color: var(--muted); }
.bn-path-hops { display: flex; flex-wrap: wrap; gap: 2px 4px; align-items: center; min-width: 0; }
.bn-path-hops a { color: var(--text); font-weight: 550; }
.bn-path-hops .sep { color: var(--muted); }
.bn-path-n { color: var(--text-2); font-size: 11.5px; display: flex; gap: 6px; align-items: center; white-space: nowrap; }
.bn-hist { display: flex; align-items: flex-end; gap: 6px; padding: 4px 0 2px; }
.bn-hist .c { display: grid; justify-items: center; gap: 2px; flex: 1; min-width: 0; }
.bn-hist .v { font-size: 10.5px; color: var(--text-2); font-variant-numeric: tabular-nums; min-height: 13px; }
.bn-hist .b { width: 100%; border-radius: 3px 3px 0 0; background: var(--good); min-height: 1px; }
.bn-hist .b.warn { background: var(--warn); } .bn-hist .b.bad { background: var(--crit); }
.bn-hist .l { font-size: 10px; color: var(--muted); white-space: nowrap; }
.bn-hist .u { font-size: 10px; color: var(--muted); align-self: flex-end; padding-bottom: 1px; }

/* the worklist */
.bn-wl-hd { padding: 10px 12px 8px; border-bottom: 1px solid var(--line); display: grid; gap: 3px; position: sticky; top: 0; background: var(--surface); z-index: 2; }
.bn-wl-hd b { font-size: 14px; }
.bn-wl-kinds { display: flex; gap: 4px; flex-wrap: wrap; margin-top: 5px; }
.bn-wl-kinds button { border: 1px solid var(--line); background: var(--surface); border-radius: 999px; font: inherit; font-size: 11.5px; padding: 2px 8px; color: var(--text-2); cursor: pointer; display: inline-flex; gap: 4px; align-items: center; }
.bn-wl-kinds button span { color: var(--muted); font-variant-numeric: tabular-nums; }
.bn-wl-kinds button.on { background: var(--accent-soft); border-color: transparent; color: var(--accent); }
.bn-wl-list { display: grid; }
.bn-wi { display: grid; gap: 2px; padding: 8px 12px 9px 11px; border-left: 3px solid var(--line-2); border-bottom: 1px solid var(--line); color: var(--text); text-decoration: none; }
.bn-wi:hover { background: var(--surface-2); text-decoration: none; }
.bn-wi.bad { border-left-color: var(--crit); } .bn-wi.warn { border-left-color: var(--warn); } .bn-wi.info { border-left-color: var(--accent); }
.bn-wi.ended { opacity: .7; }
.bn-wi.on { background: var(--accent-soft); box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 35%, transparent); }
.bn-wi .k { display: flex; justify-content: space-between; gap: 6px; font-size: 10.5px; font-weight: 650; text-transform: uppercase; letter-spacing: .05em; color: var(--text-2); }
.bn-wi.bad .k > span:first-child { color: var(--crit-ink); }
.bn-wi .k .w { font-weight: 500; text-transform: none; letter-spacing: 0; color: var(--muted); white-space: nowrap; }
.bn-wi .t { font-size: 12.5px; font-weight: 600; overflow-wrap: anywhere; line-height: 1.3; }
.bn-wi .s { font-size: 11.5px; color: var(--text-2); overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; }
.bn-wl-more summary { padding: 8px 12px; font-size: 12px; color: var(--text-2); cursor: pointer; border-bottom: 1px solid var(--line); }
.bn-wl-sec { padding: 10px 12px; display: grid; gap: 3px; border-bottom: 1px solid var(--line); }
.bn-wl-sec > b { font-size: 11px; text-transform: uppercase; letter-spacing: .04em; color: var(--text-2); margin-bottom: 2px; }
.bn-wl-wx { display: flex; justify-content: space-between; font-size: 12.5px; color: var(--text); }
.bn-wl-wx span:last-child { font-variant-numeric: tabular-nums; color: var(--text-2); }
.bn-wl-wx .bad { color: var(--crit-ink) !important; font-weight: 600; } .bn-wl-wx .warn { color: var(--text) !important; font-weight: 600; }
.bn-wl-foot { padding: 8px 12px; font-size: 11px; color: var(--muted); }

/* the strip */
.bn-tl { position: relative; border: 1px solid var(--line); border-radius: var(--r); background: var(--surface); padding: 6px 10px 2px; }
.bn-tl-lab { display: flex; gap: 4px 12px; flex-wrap: wrap; align-items: center; font-size: 11.5px; color: var(--text-2); }
.bn-tl-lab b { color: var(--text); font-weight: 600; margin-right: 4px; }
.bn-tl-lab .k::before { content: ""; display: inline-block; width: 10px; height: 8px; border-radius: 2px; margin-right: 5px; vertical-align: -0.5px; }
.bn-tl-lab .k.a::before { background: color-mix(in srgb, var(--s1) 55%, transparent); }
.bn-tl-lab .k.w::before { background: color-mix(in srgb, var(--s2) 60%, transparent); }
.bn-tl-lab .k.bad::before { background: var(--crit); width: 2px; }
.bn-tl-lab .k.inc::before { background: var(--warn); height: 4px; }
.bn-tl-lab .k.me::before { background: var(--text); height: 2px; }
.bn-tl-lab .now { margin-left: auto; font-variant-numeric: tabular-nums; }
.bn-tl-plot { height: 74px; }
.bn-tl-svg { display: block; overflow: visible; }
.bn-tl-svg .a { fill: color-mix(in srgb, var(--s1) 45%, transparent); stroke: var(--s1); stroke-width: .8; }
.bn-tl-svg .w { fill: color-mix(in srgb, var(--s2) 50%, transparent); stroke: var(--s2); stroke-width: .8; }
.bn-tl-svg .mid { stroke: var(--axis); stroke-width: 1; }
.bn-tl-svg .hr { stroke: var(--grid); stroke-width: 1; }
.bn-tl-svg .ax { font: 10px var(--sans); fill: var(--muted); }
.bn-tl-svg .bad { stroke: var(--crit); stroke-width: 1.4; }
.bn-tl-svg .me { fill: none; stroke: var(--text); stroke-width: 1.6; stroke-linejoin: round; }
.bn-tl-svg .win { fill: color-mix(in srgb, var(--accent) 12%, transparent); }
.bn-tl-svg .ib { fill: var(--warn); cursor: pointer; opacity: .55; }
.bn-tl-svg .ib.old { opacity: .25; }
.bn-tl-svg .ib:hover, .bn-tl-svg .ib.on { opacity: 1; }
.bn-tl-svg .ib.bad { fill: var(--crit); } .bn-tl-svg .ib.info { fill: var(--accent); } .bn-tl-svg .ib.ended { fill: var(--line-2); }
.bn-tl-svg .ib.on { stroke: var(--text); stroke-width: 1.4; }
.bn-tl-svg .cross { stroke: var(--text-2); stroke-width: 1; stroke-dasharray: 2 2; pointer-events: none; }
.bn-tl-tip { position: absolute; top: 24px; z-index: 5; pointer-events: none; background: var(--surface); border: 1px solid var(--line-2); border-radius: var(--r-sm); padding: 5px 9px; font-size: 11.5px; box-shadow: var(--shadow-2); max-width: 260px; }
.bn-tl-tip .bad { color: var(--crit-ink); }

/* watching */
.bn-wb { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; font-size: 12px; min-height: 26px; }
.bn-wb .lbl { display: inline-flex; gap: 5px; align-items: center; color: var(--text-2); font-weight: 600; margin-right: 2px; }
.bn-wb .lbl .i { width: 13px; height: 13px; fill: none; stroke: currentColor; stroke-width: 1.8; }
.bn-wc { display: inline-flex; gap: 6px; align-items: center; border: 1px solid var(--line); background: var(--surface); border-radius: 999px; padding: 2px 4px 2px 9px; color: var(--text); text-decoration: none; }
.bn-wc:hover { border-color: var(--accent); text-decoration: none; }
.bn-wc .d { width: 8px; height: 8px; border-radius: 50%; background: var(--good); }
.bn-wc.warn .d { background: var(--warn); } .bn-wc.bad .d { background: var(--crit); box-shadow: 0 0 0 3px var(--crit-soft); }
.bn-wc span { color: var(--text-2); font-size: 11.5px; }
.bn-wc .x { border: 0; background: none; color: var(--muted); cursor: pointer; font-size: 14px; line-height: 1; padding: 0 4px; border-radius: 50%; }
.bn-wc .x:hover { color: var(--crit-ink); background: var(--crit-soft); }

/* the map */
.bn-atlas { position: absolute; inset: 0; width: 100%; height: 100%; cursor: grab; touch-action: none; display: block; user-select: none; }
.bn-atlas.bn-drag { cursor: grabbing; }
.bn-atlas .mk { transform: scale(var(--k, 1)); }
.bn-atlas .go { cursor: pointer; }
.bn-atlas .land { fill: var(--surface-3); stroke: var(--surface); stroke-width: .6; vector-effect: non-scaling-stroke; cursor: pointer; transition: fill .3s, opacity .4s; }
.bn-atlas .land:hover { fill: color-mix(in srgb, var(--accent) 20%, var(--surface-3)); }
.bn-atlas .land.c-ok { fill: color-mix(in srgb, var(--good) 16%, var(--surface-3)); }
.bn-atlas .land.c-warn { fill: color-mix(in srgb, var(--warn) 42%, var(--surface-3)); }
.bn-atlas .land.c-bad { fill: color-mix(in srgb, var(--crit) 38%, var(--surface-3)); }
.bn-atlas .land.dim { opacity: .45; }
.bn-atlas .land.on { stroke: var(--text-2); stroke-width: 1.2; }
.bn-atlas.in-country .land.on { fill: var(--surface); }
.bn-atlas .land.hint { stroke: var(--accent); stroke-width: 2; opacity: 1; }
.bn-atlas .lk { fill: none; stroke-linecap: round; vector-effect: non-scaling-stroke; }
.bn-atlas .lk.own { stroke: var(--crit); stroke-width: 1.3; stroke-dasharray: 4 3; opacity: .8; }
.bn-atlas .lyr { transition: opacity .35s; }
.bn-atlas.in-country .links, .bn-atlas.in-places .links { opacity: 0; pointer-events: none; }
.bn-atlas .wix { opacity: 0; pointer-events: none; }
.bn-atlas.lv-region:not(.in-country):not(.in-places) .wix, .bn-atlas.lv-near:not(.in-country):not(.in-places) .wix { opacity: 1; pointer-events: auto; }
.bn-atlas .wx path { fill: var(--s7); opacity: .8; }
.bn-atlas .pin .dt { stroke: var(--surface); stroke-width: 1; }
.bn-atlas .pin.bad .dt { fill: var(--crit); } .bn-atlas .pin.warn .dt { fill: var(--warn); } .bn-atlas .pin.info .dt, .bn-atlas .pin.ok .dt { fill: var(--accent); }
.bn-atlas .pin .halo { fill: none; stroke: var(--crit); stroke-width: 2; transform-box: fill-box; transform-origin: center; animation: bn-pulse 1.8s ease-out infinite; }
.bn-atlas .pin text { font: 700 8px var(--sans); fill: #fff; pointer-events: none; }
.bn-atlas.in-country .pins, .bn-atlas.in-places .pins { opacity: .25; }
.bn-atlas.in-country .pins .m, .bn-atlas.in-places .pins .m { pointer-events: none; }
.bn-atlas .det { opacity: 0; transition: opacity .45s; }
.bn-atlas .det.in { opacity: 1; }
.bn-stage.bn-moving .det.in { opacity: .35; transition: opacity .15s; }
.bn-atlas .fac rect { fill: var(--surface); stroke: var(--text-2); stroke-width: 1.1; }
.bn-atlas .fac.mine rect { fill: var(--accent); stroke: var(--surface); }
.bn-atlas .dots path { fill: none; stroke-linecap: round; vector-effect: non-scaling-stroke; }
.bn-atlas .dots .dot-ring { stroke: var(--surface); }
.bn-atlas .dots .dot { stroke: var(--line-2); }
.bn-atlas .dots .dot.ok { stroke: var(--good); } .bn-atlas .dots .dot.warn { stroke: var(--warn); } .bn-atlas .dots .dot.bad { stroke: var(--crit); }
.bn-atlas.on-dot { cursor: pointer; }
.bn-atlas .ix path { fill: var(--s7); stroke: var(--surface); stroke-width: 1.2; }
.bn-atlas .ix .kring { fill: none; stroke: var(--good); stroke-width: 1.6; }
.bn-atlas .ixl text { font: 600 10px var(--sans); fill: var(--s7); paint-order: stroke; stroke: var(--surface); stroke-width: 3px; pointer-events: none; }
.bn-atlas .mb { fill: none; stroke: var(--text-2); stroke-width: .7; opacity: .1; vector-effect: non-scaling-stroke; }
.bn-atlas .mb.bk { stroke: var(--good); opacity: .22; }
.bn-atlas .mb.on { opacity: .9; stroke-width: 1.4; stroke: var(--accent); }
.bn-atlas .lead { stroke: var(--text-2); stroke-width: .8; stroke-dasharray: 2 2; opacity: .6; vector-effect: non-scaling-stroke; }
.bn-atlas .nt .dt { fill: var(--surface); stroke: var(--good); stroke-width: 2; }
.bn-atlas .nt.warn .dt { stroke: var(--warn); fill: color-mix(in srgb, var(--warn) 18%, var(--surface)); }
.bn-atlas .nt.bad .dt { stroke: var(--crit); fill: color-mix(in srgb, var(--crit) 15%, var(--surface)); }
.bn-atlas .nt.none .dt { stroke: var(--line-2); }
.bn-atlas .nt .c { font: 700 7.5px var(--sans); fill: var(--text); pointer-events: none; }
.bn-atlas .lbl { font: 600 11px var(--sans); fill: var(--text); paint-order: stroke; stroke: var(--surface); stroke-width: 3.2px; stroke-linejoin: round; pointer-events: none; }
.bn-atlas .lbl.hid { display: none; }
.bn-atlas .nt:hover .lbl.hid { display: inline; }
.bn-atlas .nt:hover .dt { stroke-width: 3; }
.bn-atlas .det.focus .nets .nt:not(.sel), .bn-atlas .det.focus .pbs, .bn-atlas .det.focus .mbs, .bn-atlas .det.focus .facs { opacity: .22; transition: opacity .4s; }
.bn-atlas .det.focus .ixs { opacity: .5; }
.bn-atlas .det.focus .nets .nt:not(.sel) .lbl, .bn-atlas .det.focus .ixl { display: none; }
.bn-atlas .nt.sel .dt { stroke-width: 3; }
.bn-atlas .ar { fill: none; stroke-width: calc(1.8px * var(--k, 1)); stroke-linecap: round; stroke-dasharray: 1; stroke-dashoffset: 1; animation: bndraw .9s ease forwards; }
.bn-atlas .ar.up { stroke: var(--accent); }
.bn-atlas .ar.warn { stroke: var(--warn); }
.bn-atlas .ar.bad { stroke: var(--crit); stroke-width: calc(2.2px * var(--k, 1)); }
.bn-atlas .ar.tr { stroke: var(--crit); opacity: .75; }
@keyframes bndraw { to { stroke-dashoffset: 0; } }
.bn-atlas .netl .m, .bn-atlas .top .m { animation: bnpop .45s ease both .35s; }
@keyframes bnpop { from { opacity: 0; } to { opacity: 1; } }
.bn-atlas .up circle { fill: var(--surface); stroke: var(--accent); stroke-width: 2; }
.bn-atlas .up.bad circle { stroke: var(--crit); } .bn-atlas .up.warn circle { stroke: var(--warn); }
.bn-atlas .up.tr circle { fill: var(--crit); stroke: var(--surface); }
.bn-atlas .up text, .bn-atlas .place text { font: 600 10.5px var(--sans); fill: var(--text); paint-order: stroke; stroke: var(--surface); stroke-width: 3px; stroke-linejoin: round; }
.bn-atlas .me .dt { fill: var(--accent); stroke: var(--surface); stroke-width: 2; }
.bn-atlas .me.bad .dt { fill: var(--crit); } .bn-atlas .me.warn .dt { fill: var(--warn); }
.bn-atlas .me .halo2 { fill: none; stroke: var(--accent); stroke-width: 1.5; opacity: .5; transform-box: fill-box; transform-origin: center; animation: bn-pulse 2s ease-out infinite; }
.bn-atlas .me .lbl { font-size: 12.5px; font-weight: 700; }
.bn-atlas .place circle { fill: var(--text); stroke: var(--surface); stroke-width: 1.5; }
.bn-stage .bn-map-crumbs { z-index: 4; }
.bn-stage .bn-map-crumbs .cur { font-weight: 650; }
.bn-stage .bn-map-zoom { z-index: 4; }
.bn-stage .bn-map-foot { z-index: 4; }
.bn-map-legend .bn-lg-dia { width: 8px; height: 8px; background: var(--s7); border-radius: 1px; transform: rotate(45deg); }
.bn-map-legend .bn-lg-sq { width: 7px; height: 7px; border: 1.3px solid var(--text-2); border-radius: 1.5px; background: var(--surface); }
.bn-map-legend .bn-lg-net { width: 9px; height: 9px; border: 2px solid var(--good); background: var(--surface); }
.bn-map-legend .bn-lg-arc { width: 14px; height: 0; border-top: 2px solid var(--accent); border-radius: 0; vertical-align: 3px; }
.bn-map-legend .bn-lg-arc.bad { border-top-color: var(--crit); }

/* the home page */
.bn-home { display: grid; gap: 22px; padding-bottom: 10px; }
.bn-hero2 { text-align: center; padding: 34px 0 6px; }
.bn-hero2 h1 { margin: 0; font-size: clamp(28px, 5vw, 40px); letter-spacing: -.03em; line-height: 1.08; font-weight: 760; }
.bn-hero2 h1 span { background: linear-gradient(90deg, var(--accent), var(--s7)); -webkit-background-clip: text; background-clip: text; color: transparent; }
.bn-hero2 > p { color: var(--text-2); font-size: 15.5px; line-height: 1.55; margin: 14px auto 0; max-width: 760px; }
.bn-count { display: inline-flex; flex-direction: column; align-items: center; margin: 20px auto 2px; padding: 12px 26px 11px; border: 1px solid var(--line); border-radius: 14px; background: var(--surface); box-shadow: var(--shadow-1); text-decoration: none; color: inherit; }
.bn-count[hidden] { display: none; }
.bn-count:hover { border-color: var(--line-2); text-decoration: none; }
.bn-count b { font-size: clamp(28px, 7vw, 42px); line-height: 1.1; font-weight: 800; letter-spacing: -.02em; font-variant-numeric: tabular-nums; color: var(--accent); }
.bn-count span { margin-top: 4px; font-size: 13.5px; color: var(--text-2); }
.bn-count small { margin-top: 3px; font-size: 11.5px; color: var(--muted); }
.bn-stats { display: flex; justify-content: center; flex-wrap: wrap; gap: 8px 24px; margin: 16px 0 4px; color: var(--muted); font-size: 13px; }
.bn-stats b { color: var(--text); font-size: 17px; font-weight: 700; font-variant-numeric: tabular-nums; margin-right: 4px; }
.bn-home-cta { display: flex; gap: 8px; justify-content: center; flex-wrap: wrap; margin-top: 18px; }
.bn-home-cta .btn { height: 38px; padding: 0 18px; font-size: 14px; }
.bn-try { display: flex; flex-wrap: wrap; justify-content: center; gap: 7px; margin: 14px auto 0; max-width: 820px; }
.bn-try a { padding: 4px 12px; border-radius: 999px; border: 1px solid var(--line-2); background: var(--surface); font-size: 12.5px; color: var(--text-2); font-family: var(--mono); }
.bn-try a:hover { border-color: var(--accent); color: var(--accent); text-decoration: none; }
.bn-home-grid { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1.25fr) minmax(0, 1fr); gap: 14px; align-items: start; }
.bn-home-grid > div { display: grid; gap: 14px; min-width: 0; }
@media (max-width: 1100px) { .bn-home-grid { grid-template-columns: minmax(0, 1fr); } }
.bn-home-mapcard { display: block; color: inherit; text-decoration: none; }
.bn-home-mapcard:hover { text-decoration: none; border-color: var(--accent); }
.bn-mini { display: block; width: 100%; height: auto; margin-bottom: 6px; background: color-mix(in srgb, var(--accent) 5%, var(--surface-2)); border-radius: var(--r-sm); }
.bn-mini .land { fill: var(--surface-3); stroke: var(--surface); stroke-width: .4; }
.bn-mini .land.c-ok { fill: color-mix(in srgb, var(--good) 16%, var(--surface-3)); }
.bn-mini .land.c-warn { fill: color-mix(in srgb, var(--warn) 42%, var(--surface-3)); }
.bn-mini .land.c-bad { fill: color-mix(in srgb, var(--crit) 38%, var(--surface-3)); }
.bn-mini .mp { stroke: var(--surface); stroke-width: 1; fill: var(--accent); }
.bn-mini .mp.bad { fill: var(--crit); } .bn-mini .mp.warn { fill: var(--warn); }
.bn-home-how { display: grid; grid-template-columns: minmax(0, 1.2fr) minmax(0, 1fr) minmax(0, .9fr); gap: 22px; border-top: 1px solid var(--line); padding-top: 18px; }
@media (max-width: 1100px) { .bn-home-how { grid-template-columns: minmax(0, 1fr); } }
@media (max-width: 700px) { .bn-stage .bn-map-legend { display: none; } .bn-tl-lab .now { margin-left: 0; } }
.bn-home-how h2 { font-size: 15px; margin: 0 0 6px; }
.bn-home-how p { font-size: 13px; margin: 0 0 10px; line-height: 1.5; }
.bn-watches { display: grid; gap: 6px; }
.bn-watches > div { display: grid; grid-template-columns: 124px minmax(0, 1fr); gap: 10px; font-size: 12.5px; line-height: 1.45; }
.bn-watches b { font-weight: 650; }
.bn-watches span { color: var(--text-2); }
.bn-steps { margin: 0; padding-left: 18px; display: grid; gap: 8px; font-size: 13px; line-height: 1.5; color: var(--text-2); }
.bn-steps b { color: var(--text); }
.bn-srcs { margin: 0; padding-left: 16px; display: grid; gap: 5px; font-size: 12.5px; color: var(--text-2); line-height: 1.45; }
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
// While the next page loads, the one on screen stays (dimmed, with a bar
// along the top); a skeleton only when there is nothing to keep.
function wait(main, n = 6) {
  if (main.childElementCount && !main.querySelector(":scope > .bn-skel")) { main.classList.add("bn-loading"); return; }
  fill(main, skel(n));
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

const NAV = [["", "Home"], ["map", "Console"], ["diagnose", "Diagnose"], ["incidents", "Incidents"], ["rules", "Rules"], ["data", "How it's built"]];
// The routes the console answers; the others are pages of their own.
const CONSOLE = new Set(["map", "console", "as", "prefix", "incident"]);
const ICONS = {
  search: "M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM20 20l-4-4",
  code: "m8 7-5 5 5 5M16 7l5 5-5 5",
  copy: "M8 8h11v11H8zM5 16V5h11",
  star: "M12 3.5l2.6 5.4 5.9.8-4.3 4.1 1 5.8-5.2-2.8-5.2 2.8 1-5.8-4.3-4.1 5.9-.8z",
  wide: "M4 9V4h5M20 15v5h-5M15 4h5v5M9 20H4v-5",
};
const icon = (n) => svg("svg", { class: "i", viewBox: "0 0 24 24", "aria-hidden": "true" }, svg("path", { d: ICONS[n] }));

function render(el, ctx) {
  if (!document.getElementById("bn-css")) document.head.append(h("style", { id: "bn-css" }, CSS));
  const cache = new Map();
  const app = {
    ctx, queries: [], took: 0, geo: null, now: null, world: null, tabs: {},
    // Answers kept 20 seconds, so going back and forth reads nothing again;
    // `fresh` reads again (the minute's refresh).
    api: async (path, { fresh = false, sql = true } = {}) => {
      const hit = cache.get(path);
      const j = hit && !fresh && Date.now() - hit.t < 20000 ? hit.j : await ctx.api(path, { timeout: 60000 });
      if (!hit || hit.j !== j) cache.set(path, { t: Date.now(), j });
      if (cache.size > 200) cache.delete(cache.keys().next().value);
      if (sql && j && Array.isArray(j.queries)) { app.queries = j.queries; app.took = j.took_ms; drawSql(); }
      return j;
    },
    href: (...parts) => ctx.href(...parts.filter((x) => x !== "" && x != null)),
    go: (...parts) => { const to = app.href(...parts); if (location.hash !== to) location.hash = to; },
    country: (cc) => app.geo?.countries?.[cc]?.n || cc || "",
  };
  app.watch = makeWatch(app);
  const root = h("div", { class: "bn-root" });
  fill(el, root);
  const state = h("span", { class: "sub" });
  const sqlBtn = h("button", { class: "btn sm", type: "button", onclick: () => sqlPanel.classList.toggle("on"), title: "The SQL behind what is on screen" });
  const head = h("div", { class: "bn-head" }, h("h1", {}, h("a", { href: app.href(""), class: "bn-home-link" }, "Backbone explorer")), state, h("span", { class: "bn-synth" }, "real, public data"), h("div", { class: "tools" }, sqlBtn));
  const input = h("input", { class: "bn-cmd-in", type: "search", "aria-label": "Search", autocomplete: "off", spellcheck: "false", placeholder: "A prefix, an address, an ASN, a network, an exchange or a country" });
  const nav = h("div", { class: "tabs bn-tabs", role: "tablist" });
  const bar = h("div", { class: "bn-bar" }, nav, h("div", { class: "bn-cmd compact" }, h("span", { class: "bn-cmd-ic" }, icon("search")), input));
  const main = h("main", { class: "bn-main" });
  const conHost = h("div", { class: "bn-conhost", hidden: true });
  const sqlPanel = h("div", { class: "bn-sqlp" });
  const foot = h("div", { class: "bn-foot" },
    "Public data: RIPE RIS (BGP updates), rpki-client (RPKI), CAIDA (relationships, prefix origins), RIPE NCC (names), PeeringDB (exchanges and buildings), RIPE Atlas (measurements), IODA (outages). ",
    h("a", { href: app.href("data") }, "How it's built"), ".");
  root.append(head, bar, conHost, main, sqlPanel, foot);
  let con = null;

  function drawSql() {
    const total = app.queries.reduce((a, q) => a + (q.ms || 0), 0);
    fill(sqlBtn, icon("code"), `SQL · ${plural(app.queries.length, "query", "queries")} · ${(app.took || total).toFixed(1)} ms`);
    fill(sqlPanel,
      h("div", { class: "bn-sqlp-hd" }, h("b", {}, "The SQL behind what is on screen"), h("span", { class: "muted small" }, "each statement, its rows and its time on the server"),
        h("button", { class: "btn sm ghost", type: "button", onclick: () => sqlPanel.classList.remove("on") }, "Close")),
      app.queries.map((q) => h("div", { class: "q" },
        h("div", { class: "w" }, h("span", {}, q.what), h("span", { class: "muted" }, `${fmt(q.rows)} rows · ${(q.ms ?? 0).toFixed(1)} ms`)),
        q.note ? h("div", { class: "bad" }, q.note) : null,
        h("pre", { class: "code" }, q.sql, q.params && Object.keys(q.params).length ? `\n-- ${JSON.stringify(q.params).slice(0, 400)}` : ""))));
  }
  drawSql();

  function drawNav(page) {
    const open = app.now?.open || [];
    const bad = open.filter((i) => i.severity === "bad").length;
    const cur = CONSOLE.has(page) ? "map" : page === "search" ? "" : page;
    fill(nav, NAV.map(([p, t]) => h("button", { type: "button", role: "tab", "aria-selected": String(cur === p), class: cur === p ? "on" : null, onclick: () => app.go(p) },
      t, p === "map" && open.length ? h("span", { class: `n${bad ? " bad" : ""}` }, String(open.length)) : null)));
  }
  function drawState() {
    const n = app.now;
    if (!n) return fill(state, "");
    const open = n.open || [];
    const bad = open.some((i) => i.severity === "bad");
    fill(state, h("span", { class: `dot ${open.length ? (bad ? "bad" : "warn") : "ok"}${open.length ? " pulse" : ""}` }),
      ` ${open.length ? plural(open.length, "problem") + " now" : "Nothing wrong now"}${n.connected ? `, ${n.connected} of them connected` : ""} · as of ${clock(n.as_of)}`);
  }
  async function refresh() {
    try {
      const [n, w] = await Promise.all([app.api("/v1/now", { fresh: true, sql: false }), app.api("/v1/map", { fresh: true, sql: false })]);
      app.now = n; app.world = w;
      drawState();
      drawNav(current?.[0] || "");
      if (con) con.refresh();
    } catch { /* each view shows its own errors */ }
    app.watch.refresh();
  }

  // Search: a country by name opens it on the map; the rest asks the API.
  input.addEventListener("keydown", (e) => {
    if (e.key !== "Enter") return;
    e.preventDefault();
    const q = input.value.trim();
    if (!q) return;
    const low = q.toLowerCase();
    const cc = Object.entries(app.geo?.countries || {}).find(([k, g]) => g.n?.toLowerCase() === low || (q.length === 2 && k === q.toUpperCase() && !/^\d/.test(q)))?.[0];
    if (cc) { input.value = ""; input.blur(); return app.go("map", cc); }
    app.go("search", q);
  });
  document.addEventListener("keydown", (e) => {
    if (!el.isConnected) return;
    const typing = /INPUT|TEXTAREA|SELECT/.test(document.activeElement?.tagName || "");
    if (e.key === "/" && !typing) { e.preventDefault(); input.focus(); input.select(); return; }
    if (typing || e.metaKey || e.ctrlKey || e.altKey) return;
    if (con && !conHost.hidden) con.key(e);
  });

  let current = null, seq = 0;
  async function show(path) {
    path = path || [];
    current = path;
    const me = ++seq;
    const alive = () => me === seq;
    const [page = "", ...rest] = path;
    drawNav(page);
    if (page !== "search") input.value = "";
    try {
      if (!app.geo) app.geo = await ctx.api("/ui/geo.json").catch(() => ({ countries: {} }));
      if (CONSOLE.has(page)) {
        const enter = conHost.hidden;
        main.hidden = true; foot.hidden = true; conHost.hidden = false;
        if (!con) { con = makeConsole(app); conHost.append(con.el); }
        if (enter) { scrollTo({ top: 0 }); con.fit(); }
        return await con.show(page === "console" ? "map" : page, rest);
      }
      const was = !conHost.hidden;
      conHost.hidden = true; main.hidden = false; foot.hidden = false;
      if (was) fill(main);
      scrollTo({ top: 0 });
      switch (page) {
        case "": return await homePage(app, main, alive);
        case "incidents": return await incidentsPage(app, main, alive);
        case "diagnose": return await diagnosePage(app, main, rest[0] || "", alive);
        case "rules": return await rulesPage(app, main, alive);
        case "data": return await dataPage(app, main, alive);
        case "search": input.value = rest.join("/"); return await searchPage(app, main, rest.join("/"), alive);
        default: fill(main, empty("No such page."));
      }
    } catch (e) {
      if (alive()) fill(main, h("div", { class: "error" }, String(e.message || e)));
    } finally {
      if (alive()) main.classList.remove("bn-loading");
    }
  }
  ctx.onPath((p) => { if (JSON.stringify(p) !== JSON.stringify(current)) show(p); });
  refresh();
  setInterval(() => { if (document.visibilityState === "visible" && el.isConnected) refresh(); }, 60000);
  show(ctx.path());
}

// ---------------------------------------------------------------- the home page

const TRY = [["an address", "1.1.1.1"], ["a prefix", "8.8.8.0/24"], ["a network", "AS3320"], ["by name", "Cloudflare"], ["an exchange", "DE-CIX"], ["a country", "Brazil"]];
const WATCHES = [
  ["Invalid routes", "a route RPKI gives to another network, or longer than its ROA allows, heard by three peers or more", "rpki-invalid"],
  ["New origins", "a prefix announced by a network that did not announce it yesterday, and is neither the owner's customer nor its provider", "new-origin"],
  ["More specifics", "a prefix no one announced yesterday inside space another network announces, from a network that is not its customer", "more-specific"],
  ["Flapping", "a prefix announced and withdrawn ten thousand times an hour or more", "flapping"],
  ["Mass withdrawals", "twenty or more of a network's prefixes withdrawn by five or more peers at once", "withdrawn"],
  ["Blackholing", "a prefix tagged 65535:666: its owner asked the world to drop its traffic", "blackhole"],
  ["Outages", "a country or region whose routes, pings or traffic IODA sees fall", "ioda"],
  ["Slow countries", "a country whose RIPE Atlas probes take over 100 ms to reach the nearest K root server", "slow-country"],
];

async function homePage(app, main, alive) {
  const count = h("a", { class: "bn-count", href: app.href("data"), hidden: true });
  const stats = h("div", { class: "bn-stats" });
  const nowCol = h("div", {}, skel(6));
  const mapCol = h("div", { class: "bn-home-map" }, skel(6));
  const wxCol = h("div", {}, skel(6));
  fill(main, h("div", { class: "bn-home" },
    h("div", { class: "bn-hero2" },
      h("h1", {}, "The internet's routing, ", h("span", {}, "as it happens"), "."),
      h("p", {}, "Every BGP update a RIPE RIS collector hears, checked as it arrives against RPKI and against the origins seen the day before, beside the exchanges and buildings where networks meet, who buys transit from whom, RIPE Atlas's measurements and IODA's outages. It says what is wrong now, where it is, and how the problems connect."),
      count, stats,
      h("div", { class: "bn-home-cta" }, h("a", { class: "btn primary", href: app.href("map") }, "Open the console"), h("a", { class: "btn", href: app.href("diagnose") }, "Diagnose a country"), h("a", { class: "btn ghost", href: app.href("rules") }, "The rules")),
      h("div", { class: "bn-try" }, TRY.map(([k, q]) => h("a", { href: app.href("search", q), title: `Look up ${k}` }, q)))),
    h("div", { class: "bn-home-grid" }, nowCol, mapCol, wxCol),
    h("div", { class: "bn-home-how" },
      h("section", {}, h("h2", {}, "What it watches for"), h("p", { class: "muted" }, "Each is a SQL statement over the tables, run every minute. What one finds is an alert, open while it keeps finding it; alerts that share an owner, an announcer, a carrier or a country become one incident."),
        h("div", { class: "bn-watches" }, WATCHES.map(([t, d]) => h("div", {}, h("b", {}, t), h("span", {}, d))))),
      h("section", {}, h("h2", {}, "How to read the console"),
        h("ol", { class: "bn-steps" },
          h("li", {}, h("b", {}, "What is wrong, most serious first. "), "The left column is the open incidents. Pick one and the map flies to where it is, with the networks that announced, carried and own the routes."),
          h("li", {}, h("b", {}, "From the world to one network. "), "Click a country to see its exchanges, its buildings, its networks where they sit and every RIPE Atlas probe coloured by its time to the K root server. Click a network for its buildings, its probes and the lines to its providers."),
          h("li", {}, h("b", {}, "What changed, and when. "), "The strip along the top is everything heard a minute for six hours, with the incidents on it. The inspector on the right says what is wrong, what changed (who began announcing, which paths are new), the metrics and the records."),
          h("li", {}, h("b", {}, "Your own networks. "), "Open a network and press Watch: it stays at the top of the console with its alerts and its routes' updates, kept in your browser."))),
      h("section", {}, h("h2", {}, "Where the data comes from"),
        h("ul", { class: "bn-srcs" },
          h("li", {}, h("a", { href: "https://ris.ripe.net", target: "_blank", rel: "noopener" }, "RIPE RIS"), ": every BGP update its collector rrc00 hears from about 90 peers, from its archive and then RIS Live, kept three days."),
          h("li", {}, h("a", { href: "https://www.rpki-client.org", target: "_blank", rel: "noopener" }, "rpki-client"), ": the validated ROAs, who may announce which prefixes."),
          h("li", {}, h("a", { href: "https://www.caida.org/catalog/datasets/as-relationships/", target: "_blank", rel: "noopener" }, "CAIDA"), ": who is whose provider, customer or peer; RouteViews' origins of the day before."),
          h("li", {}, h("a", { href: "https://www.peeringdb.com", target: "_blank", rel: "noopener" }, "PeeringDB"), ": exchanges, buildings, who is at which."),
          h("li", {}, h("a", { href: "https://atlas.ripe.net", target: "_blank", rel: "noopener" }, "RIPE Atlas"), ": pings and traceroutes from every probe to the K root server."),
          h("li", {}, h("a", { href: "https://ioda.inetintel.cc.gatech.edu", target: "_blank", rel: "noopener" }, "IODA"), ": outages of countries, regions and networks."))))));
  const [n, w] = await Promise.all([app.api("/v1/now"), app.api("/v1/map")]);
  if (!alive()) return;
  app.now = n; app.world = w;
  const c = w.counts || {};
  if (c.bb_updates) {
    fill(count, h("b", {}, fmt(c.bb_updates)), h("span", {}, "BGP updates kept, every one the collector heard in the last three days"),
      h("small", {}, n.heard?.updates != null ? `${fmt(n.heard.updates)} in the last ${n.heard.minutes || 5} minutes, from ${fmt(n.heard.peers)} peers` : ""));
    count.hidden = false;
  }
  fill(stats, [[c.bb_as, "networks"], [(c.bb_p2c || 0) + (c.bb_p2p || 0), "links between them"], [c.bb_ix, "exchanges"], [c.bb_fac, "buildings"], [c.bb_roa, "ROAs"], [c.bb_probe, "probes"]]
    .filter(([v]) => v).map(([v, t]) => h("span", {}, h("b", {}, short(v)), t)));
  const open = n.open || [];
  const bad = open.some((i) => i.severity === "bad");
  fill(nowCol, h("section", { class: "card" },
    h("div", { class: "hd" }, h("h3", {}, open.length ? plural(open.length, "problem") + " now" : "Nothing wrong now"), h("span", { class: "sub" }, `as of ${clock(n.as_of)}`), open.length ? h("span", { class: `dot ${bad ? "bad" : "warn"} pulse`, style: { marginLeft: "auto" } }) : null),
    h("div", { class: "bd flush" }, open.length ? open.slice(0, 6).map((i) => incidentRow(app, i)) : empty("No rule finds anything in the last hour.")),
    open.length > 6 ? h("div", { class: "bd" }, h("a", { href: app.href("map") }, `All ${open.length} in the console`)) : null));
  fill(mapCol, h("a", { class: "card bn-home-mapcard", href: app.href("map"), title: "Open the console" },
    h("div", { class: "hd" }, h("h3", {}, "Where it is red"), h("span", { class: "sub" }, `${plural(w.open_alerts || 0, "open alert")} in ${plural((w.countries || []).filter((x) => x.alerts).length, "country", "countries")}`)),
    h("div", { class: "bd" }, miniWorld(app, w), h("div", { class: "muted small" }, "Click to open the console: the map zooms from the world to a country to one network."))));
  const world = n.world?.med;
  fill(wxCol, card("Internet weather", `time to the nearest K root server, slowest countries, last 20 minutes${n.world ? ` · ${fmt(n.world.probes)} probes` : ""}`,
    (n.weather || []).length ? hbars([
      ...(n.weather || []).map((x) => ({ label: app.country(x.country), href: app.href("map", x.country), text: ms(x.med), v: x.med, cls: x.med > 150 ? "bad" : x.med > 60 ? "warn" : "ok" })),
      { label: "World median", text: ms(world), v: world, cls: "ok" },
    ]) : empty("No Atlas results yet.")),
    (n.busiest || []).length ? card("Busiest prefixes", "updates in the last hour", n.busiest.slice(0, 5).map((b) => row(SEV.warn, b.prefix, `${b.origin.name || "AS" + b.origin.asn} · ${fmt(b.updates)} updates`, null, app.href("prefix", b.prefix)))) : null);
}

// The world, coloured, not moving: the home page's way into the console.
function miniWorld(app, w) {
  const geo = app.geo, G = geo.countries || {};
  const byCc = Object.fromEntries((w.countries || []).map((c) => [c.cc, c]));
  const s = svg("svg", { class: "bn-mini", viewBox: `0 0 ${geo.w} ${geo.h}`, role: "img", "aria-label": "The world, countries coloured by what is wrong there" });
  const tone = (c) => !c ? "none" : c.severity === "bad" ? "bad" : c.severity === "warn" ? "warn" : c.probes ? "ok" : "none";
  const g = svg("g", { "clip-path": "url(#bn-mclip)" });
  for (const [cc, x] of Object.entries(G)) if (x.d) g.append(svg("path", { class: `land c-${tone(byCc[cc])}`, d: x.d }));
  s.append(svg("defs", {}, svg("clipPath", { id: "bn-mclip" }, svg("rect", { x: 0, y: 0, width: geo.w, height: geo.h }))), g);
  for (const c of (w.countries || []).filter((c) => c.alerts && G[c.cc]?.x != null)) {
    const r = Math.min(10, 3 + Math.sqrt(c.alerts));
    s.append(svg("circle", { cx: G[c.cc].x, cy: G[c.cc].y, r, class: `mp ${c.severity}` }));
  }
  return s;
}

// ---------------------------------------------------------------- watching

// The networks a reader watches, kept in this browser (and nowhere else).
function makeWatch(app) {
  const KEY = "bn-watch";
  const read = () => { try { const v = JSON.parse(localStorage.getItem(KEY) || "[]"); return Array.isArray(v) ? v.map(Number).filter((x) => x > 0).slice(0, 12) : []; } catch { return []; } };
  let list = read();
  let info = {};
  const els = new Set();
  const save = () => { try { localStorage.setItem(KEY, JSON.stringify(list)); } catch { /* private window: kept for this visit */ } };
  function chip(a) {
    const i = info[a];
    const what = !i ? "" : i.alerts ? plural(i.alerts, "open alert") : i.updates ? `${short(i.updates)} updates in the hour` : "quiet";
    return h("a", { class: `bn-wc ${i?.state || ""}`, href: app.href("as", a), title: i?.title || "" },
      h("i", { class: "d" }), h("b", {}, i?.name || `AS${a}`), what ? h("span", {}, what) : null,
      h("button", { type: "button", class: "x", title: "Stop watching", "aria-label": `Stop watching AS${a}`, onclick: (e) => { e.preventDefault(); e.stopPropagation(); w.toggle(a); } }, "×"));
  }
  const w = {
    has: (a) => list.includes(Number(a)),
    toggle(a) {
      a = Number(a);
      list = list.includes(a) ? list.filter((x) => x !== a) : [...list, a].slice(-12);
      save(); w.draw(); w.refresh();
      for (const f of w.listeners) f();
    },
    listeners: new Set(),
    bar() { const e = h("div", { class: "bn-wb" }); els.add(e); w.draw(); return e; },
    draw() {
      for (const e of els) fill(e, h("span", { class: "lbl" }, icon("star"), "Watching"),
        list.length ? list.map(chip) : h("span", { class: "muted small" }, "Open a network and press Watch: it stays here with its alerts and its routes' updates. Kept in this browser."));
    },
    async refresh() {
      if (!list.length) return w.draw();
      try {
        const j = await app.api(`/v1/watch?asns=${list.join(",")}`, { fresh: true, sql: false });
        info = Object.fromEntries((j.networks || []).map((x) => [x.asn, x]));
      } catch { /* the chips keep what they had */ }
      w.draw();
    },
  };
  return w;
}

// ---------------------------------------------------------------- the camera

// Van Wijk and Nuij's smooth zoom and pan ("Smooth and efficient zooming
// and panning", 2003): the path between two views that zooms out as far as
// the distance needs and back in, so going from a country to one of its
// networks, or across the world, is one movement. A view is [x, y, w]: its
// centre and its width, in map units.
function zoomPath(p0, p1, rho = Math.SQRT2) {
  const [ux0, uy0, w0] = p0, [ux1, uy1, w1] = p1;
  const dx = ux1 - ux0, dy = uy1 - uy0, d2 = dx * dx + dy * dy;
  const r2 = rho * rho, r4 = r2 * r2;
  const cosh = (x) => (Math.exp(x) + Math.exp(-x)) / 2, sinh = (x) => (Math.exp(x) - Math.exp(-x)) / 2, tanh = (x) => sinh(x) / cosh(x);
  let S, f;
  if (d2 < 1e-9) {
    S = Math.log(w1 / w0) / rho;
    f = (t) => [ux0 + t * dx, uy0 + t * dy, w0 * Math.exp(rho * t * S)];
  } else {
    const d1 = Math.sqrt(d2);
    const b0 = (w1 * w1 - w0 * w0 + r4 * d2) / (2 * w0 * r2 * d1);
    const b1 = (w1 * w1 - w0 * w0 - r4 * d2) / (2 * w1 * r2 * d1);
    const r0 = Math.log(Math.sqrt(b0 * b0 + 1) - b0), r1 = Math.log(Math.sqrt(b1 * b1 + 1) - b1);
    S = (r1 - r0) / rho;
    f = (t) => {
      const s = t * S, c0 = cosh(r0), u = (w0 / (r2 * d1)) * (c0 * tanh(rho * s + r0) - sinh(r0));
      return t >= 1 ? [ux1, uy1, w1] : [ux0 + u * dx, uy0 + u * dy, (w0 * c0) / cosh(rho * s + r0)];
    };
  }
  f.S = Math.abs(S);
  return f;
}
const ease = (t) => (t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2);
function probeColor(rtt) { return rtt == null ? "none" : rtt < 60 ? "ok" : rtt <= 150 ? "warn" : "bad"; }
// A curve from a to b, bowed to one side by how far apart they are.
function arc(a, b, bow = 0.22) {
  const mx = (a[0] + b[0]) / 2, my = (a[1] + b[1]) / 2, dx = b[0] - a[0], dy = b[1] - a[1];
  return `M${a[0].toFixed(2)},${a[1].toFixed(2)} Q${(mx - dy * bow).toFixed(2)},${(my + dx * bow - Math.hypot(dx, dy) * 0.05).toFixed(2)} ${b[0].toFixed(2)},${b[1].toFixed(2)}`;
}
const quant = (a, f) => a[Math.min(a.length - 1, Math.max(0, Math.round(f * (a.length - 1))))];

// ---------------------------------------------------------------- the map

// One SVG for every zoom: the world's outlines stay, and what a country or a
// network adds is drawn over them where it is (PeeringDB's coordinates for
// exchanges and buildings, RIPE Atlas's for probes). The camera moves the
// viewBox along a zoom path; markers keep their size on screen (--k, map
// units a pixel); the land and its lines keep their stroke.
function makeMap(app, stage) {
  const geo = app.geo, G = geo.countries || {};
  const W = geo.w || 1000, H = geo.h || 383.3;
  const MINW = 4, MAXW = W * 1.1;
  const P = (lat, lon) => [((lon + 180) / 360) * W, ((geo.top - lat) / geo.span) * H];
  const s = svg("svg", { class: "bn-atlas", role: "img", "aria-label": "The map: the world, a country, a network" });
  const gLand = svg("g", { "clip-path": "url(#bn-clip)" }), gLinks = svg("g", { class: "lyr links" }), gIx = svg("g", { class: "lyr wix" }), gPins = svg("g", { class: "lyr pins" });
  const gDet = svg("g", { class: "lyr det" }), gTop = svg("g", { class: "lyr top" });
  s.append(svg("defs", {}, svg("clipPath", { id: "bn-clip" }, svg("rect", { x: 0, y: -60, width: W, height: H + 120 }))), gLand, gLinks, gIx, gPins, gDet, gTop);
  const tip = h("div", { class: "bn-map-tip", hidden: true });
  const crumbs = h("div", { class: "bn-map-crumbs" });
  const legendEl = h("div", { class: "bn-map-legend" });
  const stats = h("div", { class: "bn-map-stats" });
  const btn = (t, title, f) => h("button", { class: "btn sm", type: "button", title, "aria-label": title, onclick: f }, t);
  stage.append(s, tip, crumbs,
    h("div", { class: "bn-map-zoom" }, btn("+", "Zoom in", () => zoomBy(1.8)), btn("−", "Zoom out", () => zoomBy(1 / 1.8)), btn("World", "Back to the world", () => app.go("map"))),
    h("div", { class: "bn-map-foot" }, h("div", { class: "bn-map-foot-row" }, legendEl, stats)));

  // ---- the camera
  let cam = { x: W / 2, y: H / 2 + 10, w: W };
  let size = { w: stage.clientWidth || 800, h: stage.clientHeight || 400 };
  let anim = null, raf = 0;
  const aspect = () => size.h / Math.max(1, size.w);
  const clampW = (w) => Math.min(MAXW, Math.max(MINW, w));
  const clampPos = (v) => ({ w: v.w, x: Math.min(W + v.w * 0.3, Math.max(-v.w * 0.3, v.x)), y: Math.min(H + v.w * aspect() * 0.3, Math.max(-v.w * aspect() * 0.3, v.y)) });
  let level = "";
  function apply() {
    const vw = cam.w, vh = cam.w * aspect();
    s.setAttribute("viewBox", `${(cam.x - vw / 2).toFixed(3)} ${(cam.y - vh / 2).toFixed(3)} ${vw.toFixed(3)} ${vh.toFixed(3)}`);
    s.style.setProperty("--k", (vw / Math.max(1, size.w)).toFixed(5));
    const lv = vw > 420 ? "world" : vw > 90 ? "region" : "near";
    if (lv !== level) { s.classList.remove(`lv-${level}`); s.classList.add(`lv-${lv}`); level = lv; }
  }
  function step(now) {
    raf = 0;
    if (anim?.kind === "fly") {
      const t = Math.min(1, (now - anim.t0) / anim.d);
      const [x, y, w] = anim.f(ease(t));
      cam = { x, y, w };
      if (t >= 1) anim = null;
    } else if (anim?.kind === "ease") {
      const g = anim.to;
      cam = { x: cam.x + (g.x - cam.x) * 0.24, y: cam.y + (g.y - cam.y) * 0.24, w: cam.w * Math.pow(g.w / cam.w, 0.24) };
      if (Math.abs(g.w / cam.w - 1) < 0.004 && Math.hypot(g.x - cam.x, g.y - cam.y) < cam.w * 0.0015) { cam = { ...g }; anim = null; }
    }
    apply();
    if (anim) raf = requestAnimationFrame(step);
    else { stage.classList.remove("bn-moving"); for (const f of settled.splice(0)) f(); }
  }
  const settled = [];
  const run = () => { if (!raf) raf = requestAnimationFrame(step); };
  // Fly along the zoom path; a short hop eases instead.
  function flyTo(t, { quick = false } = {}) {
    t = clampPos({ ...t, w: clampW(t.w) });
    const f = zoomPath([cam.x, cam.y, cam.w], [t.x, t.y, t.w]);
    if (f.S < 0.05) { anim = { kind: "ease", to: t }; run(); return; }
    anim = { kind: "fly", f, t0: performance.now(), d: quick ? 420 : Math.min(1700, Math.max(650, f.S * 520)) };
    stage.classList.add("bn-moving");
    run();
  }
  const after = () => new Promise((r) => (anim ? settled.push(r) : r()));
  function boxView([x0, y0, x1, y1], pad = 0.1) {
    const bw = Math.max(x1 - x0, MINW), bh = Math.max(y1 - y0, MINW * 0.5);
    const w = Math.max(bw * (1 + 2 * pad), (bh * (1 + 2 * pad)) / aspect());
    return { x: (x0 + x1) / 2, y: (y0 + y1) / 2, w };
  }
  function zoomBy(f, px = size.w / 2, py = size.h / 2) {
    const base = anim?.kind === "ease" ? anim.to : cam;
    const vw = base.w, vh = base.w * aspect();
    const mx = base.x - vw / 2 + (px / size.w) * vw, my = base.y - vh / 2 + (py / size.h) * vh;
    const w = clampW(base.w / f), k = w / base.w;
    anim = { kind: "ease", to: clampPos({ x: mx + (base.x - mx) * k, y: my + (base.y - my) * k, w }) };
    run();
  }
  s.addEventListener("wheel", (e) => {
    e.preventDefault();
    const r = s.getBoundingClientRect();
    const d = e.deltaMode === 1 ? e.deltaY * 16 : e.deltaY;
    zoomBy(Math.exp(-d * (e.ctrlKey ? 0.012 : 0.0024)), e.clientX - r.left, e.clientY - r.top);
  }, { passive: false });
  s.addEventListener("dblclick", (e) => { const r = s.getBoundingClientRect(); zoomBy(2, e.clientX - r.left, e.clientY - r.top); });
  let drag = null, dragged = false;
  s.addEventListener("pointerdown", (e) => { if (e.button !== 0) return; drag = { x: e.clientX, y: e.clientY, cam: { ...cam }, moved: false }; dragged = false; });
  addEventListener("pointermove", (e) => {
    if (!drag) return;
    const dx = e.clientX - drag.x, dy = e.clientY - drag.y;
    if (!drag.moved && Math.abs(dx) + Math.abs(dy) < 4) return;
    if (!drag.moved) { drag.moved = true; s.setPointerCapture?.(e.pointerId); s.classList.add("bn-drag"); tipHide(); }
    anim = null;
    const k = cam.w / size.w;
    cam = clampPos({ w: cam.w, x: drag.cam.x - dx * k, y: drag.cam.y - dy * k });
    apply();
  });
  addEventListener("pointerup", () => { if (drag?.moved) { dragged = true; setTimeout(() => { dragged = false; }, 0); } drag = null; s.classList.remove("bn-drag"); });
  new ResizeObserver(() => {
    const r = stage.getBoundingClientRect();
    if (r.width < 10 || r.height < 10) return;
    size = { w: r.width, h: r.height };
    apply();
  }).observe(stage);

  // ---- tooltips and clicks, for every marker at once: each carries its
  // lines (el.__tip) and where it goes (el.__go).
  function tipShow(e, lines) {
    const r = stage.getBoundingClientRect();
    fill(tip, lines.filter(Boolean).map((l, i) => (i ? h("div", {}, l) : h("b", {}, l))));
    tip.hidden = false;
    tip.style.left = `${Math.min(r.width - 260, e.clientX - r.left + 14)}px`;
    tip.style.top = `${Math.min(r.height - 60, e.clientY - r.top + 14)}px`;
  }
  function tipHide() { tip.hidden = true; }
  s.addEventListener("mousemove", (e) => {
    if (drag?.moved) return;
    let t = e.target.closest?.("[data-t]");
    if (!t || t.classList.contains("land")) t = dotAt(e) || t;
    s.classList.toggle("on-dot", !!t?.go);
    if (t?.tip || t?.__tip) { const v = t.tip || t.__tip; tipShow(e, typeof v === "function" ? v() : v); } else tipHide();
  });
  s.addEventListener("mouseleave", tipHide);
  s.addEventListener("click", (e) => {
    if (dragged) return;
    let t = e.target.closest?.("[data-t]");
    if (!t || t.classList.contains("land")) { const d = dotAt(e); if (d?.go) { tipHide(); d.go(); return; } }
    if (t?.__go) { tipHide(); t.__go(); }
  });
  const tag = (el, tipv, go) => { el.setAttribute("data-t", ""); el.__tip = tipv; if (go) { el.__go = go; el.classList.add("go"); } return el; };
  // A marker at (x, y) whose children are drawn in screen pixels around 0,0.
  const mk = (x, y, cls, ...kids) => svg("g", { class: `m ${cls || ""}`, transform: `translate(${x.toFixed(3)},${y.toFixed(3)})` }, svg("g", { class: "mk" }, kids));
  const diamond = (r) => svg("path", { d: `M0,${-r}L${r},0L0,${r}L${-r},0Z` });
  // Many small dots (probes: two thousand in the United States) as one
  // path per colour of zero-length strokes with round caps, which keep
  // their size on screen without a transform each, so a flight redraws
  // four paths, not thousands of markers. Their tooltips and clicks come
  // from the nearest dot under the pointer.
  function dots(cls, r, pts) {
    const g = svg("g", { class: `dots ${cls}` });
    const by = {};
    for (const p of pts) (by[p.c] ||= []).push(`M${p.x.toFixed(2)} ${p.y.toFixed(2)}h0`);
    for (const [c, d] of Object.entries(by)) {
      const dd = d.join("");
      g.append(svg("path", { class: "dot-ring", d: dd, "stroke-width": r * 2 + 1.4 }), svg("path", { class: `dot ${c}`, d: dd, "stroke-width": r * 2 }));
    }
    g.__dots = pts;
    g.__r = r;
    return g;
  }
  function dotAt(e) {
    const r = s.getBoundingClientRect();
    const vw = cam.w, vh = cam.w * aspect(), k = vw / Math.max(1, size.w);
    const mx = cam.x - vw / 2 + ((e.clientX - r.left) / size.w) * vw, my = cam.y - vh / 2 + ((e.clientY - r.top) / size.h) * vh;
    let best = null, bd = Infinity;
    for (const g of s.querySelectorAll(".dots")) {
      if (!g.getClientRects().length || +getComputedStyle(g.closest(".lyr")).opacity < 0.5) continue;
      const lim = (g.__r + 2) * k;
      for (const p of g.__dots) {
        const dx = p.x - mx, dy = p.y - my;
        if (Math.abs(dx) > lim || Math.abs(dy) > lim) continue;
        const d = dx * dx + dy * dy;
        if (d < bd && d <= lim * lim) { bd = d; best = p; }
      }
    }
    return best;
  }

  // ---- the world
  const land = {};
  const pinOf = {};
  for (const [cc, g] of Object.entries(G)) {
    if (g.x != null) pinOf[cc] = [g.x, g.y];
    if (!g.d) continue;
    land[cc] = tag(svg("path", { class: "land", d: g.d }), () => landTip(cc), () => app.go("map", cc));
    gLand.append(land[cc]);
  }
  let byCc = {};
  let focusCcs = new Set(), dimOthers = false, hinted = new Set();
  const tone = (c) => !c ? "none" : c.severity === "bad" ? "bad" : c.severity === "warn" ? "warn" : c.probes ? "ok" : "none";
  function landTip(cc) {
    const c = byCc[cc];
    return [app.country(cc), c?.alerts ? plural(c.alerts, "open alert") : "no open alerts", c?.probes ? `${fmt(c.probes)} probes, median ${ms(c.med)} to K` : "no probes heard from"];
  }
  function paintLand() {
    for (const [cc, p] of Object.entries(land)) {
      const on = focusCcs.has(cc);
      p.setAttribute("class", `land c-${tone(byCc[cc])}${on ? " on" : dimOthers ? " dim" : ""}${hinted.has(cc) ? " hint" : ""}`);
    }
  }
  function boxOf(cc) {
    const g = G[cc];
    if (g?.b) return g.b;
    const p = pinOf[cc];
    return p ? [p[0] - 3, p[1] - 2, p[0] + 3, p[1] + 2] : [0, 0, W, H];
  }
  let world = null;
  function paintWorld(w) {
    world = w;
    byCc = Object.fromEntries((w.countries || []).map((c) => [c.cc, c]));
    for (const x of w.ixes || []) if (x.lat != null && !pinOf[x.cc]) pinOf[x.cc] = P(x.lat, x.lon);
    paintLand();
    fill(gLinks);
    for (const l of w.lines || []) {
      const a = pinOf[l.from], b = pinOf[l.to];
      if (!a || !b) continue;
      gLinks.append(tag(svg("path", { class: "lk own", d: arc(a, b, 0.18) }), [`${app.country(l.from)} → ${app.country(l.to)}`, `a network in ${app.country(l.from)} announced ${plural(l.alerts, "route")} owned in ${app.country(l.to)}`], () => app.go("incident", l.incident)));
    }
    fill(gIx);
    for (const x of w.ixes || []) {
      if (x.lat == null) continue;
      const [px, py] = P(x.lat, x.lon);
      gIx.append(tag(mk(px, py, "wx", diamond(3.2)), [x.name, `${x.city} · ${fmt(x.members)} members`], () => app.go("map", x.cc)));
    }
    fill(gPins);
    const pins = (w.countries || []).filter((c) => (c.alerts > 0 || c.severity !== "ok") && pinOf[c.cc]);
    pins.sort((a, b) => (a.severity === "bad") - (b.severity === "bad"));
    for (const c of pins) {
      const [px, py] = pinOf[c.cc];
      const r = c.alerts ? Math.min(11, 4 + Math.sqrt(c.alerts) * 1.05) : 3.6;
      gPins.append(tag(mk(px, py, `pin ${c.severity}`, c.severity === "bad" ? svg("circle", { class: "halo", r }) : null, svg("circle", { class: "dt", r }),
        c.alerts > 1 ? svg("text", { y: 3.2, "text-anchor": "middle" }, c.alerts > 99 ? "99+" : String(c.alerts)) : null),
      () => landTip(c.cc), () => app.go("map", c.cc)));
      gPins.lastChild.dataset.cc = c.cc;
    }
    stats.textContent = `${plural(w.open_alerts || 0, "open alert")} in ${plural((w.countries || []).filter((c) => c.alerts).length, "country", "countries")}`;
  }

  // ---- what a focus draws
  const legends = {
    world: [["var(--crit)", "invalid route, hijack or outage"], ["var(--warn)", "flapping, new origin or slow"], ["color-mix(in srgb, var(--good) 45%, var(--surface))", "probes, nothing wrong"], ["line", "announced from another country"]],
    country: [["var(--good)", "probe under 60 ms to K"], ["var(--warn)", "60 to 150 ms"], ["var(--crit)", "over 150 ms"], ["dia", "exchange"], ["sq", "building"], ["net", "network, where its buildings are"]],
    network: [["sq", "its buildings"], ["dia", "its exchanges"], ["var(--good)", "its probes, by time to K"], ["arc", "to its providers"], ["bad", "trouble near it"]],
    places: [["arc", "path of the route"], ["bad", "announced space owned elsewhere"]],
  };
  function drawLegend(kind) {
    fill(legendEl, (legends[kind] || []).map(([c, t]) => h("span", {},
      c === "line" ? h("i", { class: "bn-ln" }) : c === "dia" ? h("i", { class: "bn-lg-dia" }) : c === "sq" ? h("i", { class: "bn-lg-sq" }) : c === "net" ? h("i", { class: "bn-lg-net" }) : c === "arc" ? h("i", { class: "bn-lg-arc" }) : c === "bad" ? h("i", { class: "bn-lg-arc bad" }) : h("i", { style: { background: c } }), t)));
  }
  function setCrumbs(parts) {
    fill(crumbs, parts.map((p, i) => [i ? h("span", { class: "muted" }, "›") : null, p.href ? h("a", { href: p.href }, p.t) : h("span", { class: i === parts.length - 1 ? "cur" : "muted" }, p.t)]));
  }
  const base = { cc: null, country: null };
  // The country layer: buildings, probes, exchanges, then the networks with
  // their labels laid out so they do not overlap at the zoom it flies to.
  function drawCountry(c, view) {
    fill(gDet);
    base.cc = c.cc; base.country = c;
    const upp = view.w / Math.max(1, size.w);
    const nets = (c.nets || []);
    const netOf = Object.fromEntries(nets.map((n) => [n.asn, n]));
    const gF = svg("g", { class: "facs" }), gP = svg("g", { class: "pbs" }), gX = svg("g", { class: "ixs" }), gM = svg("g", { class: "mbs" }), gN = svg("g", { class: "nets" });
    for (const f of c.facs || []) {
      const [x, y] = P(f.lat, f.lon);
      gF.append(tag(mk(x, y, "fac", svg("rect", { x: -2.4, y: -2.4, width: 4.8, height: 4.8, rx: 1 })), [f.name, `${f.city} · ${plural(f.nets, "network")} · ${plural(f.ixes, "exchange")}`]));
    }
    gP.append(dots("pb", 2.2, (c.probes_at || []).map(([id, asn, lat, lon, rtt]) => {
      const [x, y] = P(lat, lon);
      return { x, y, c: probeColor(rtt), tip: () => [`Probe ${id}`, netOf[asn] ? `${netOf[asn].name} (AS${asn})` : `AS${asn}`, rtt == null ? "no answer from K in 20 minutes" : `${Math.round(rtt)} ms to the K root server`],
        go: netOf[asn] ? () => app.go("map", c.cc, asn) : null };
    })));
    const ixPos = {};
    const ixes = (c.ixes || []).filter((x) => x.lat != null);
    ixes.forEach((x, i) => {
      const [px, py] = P(x.lat, x.lon);
      ixPos[x.id] = [px, py];
      const r = 4 + Math.min(5, Math.sqrt(x.members || 1) / 3.5);
      gX.append(tag(mk(px, py, `ix${x.k ? " kr" : ""}`, x.k ? svg("circle", { class: "kring", r: r + 3.2 }) : null, diamond(r)),
        [x.name, `${x.city} · ${fmt(x.members)} members`, x.k ? "the K root server is a member" : null]));
      x._r = r;
      x._rank = i;
    });
    // Networks: where their buildings are, else their probes, else in a
    // ring round the country's middle.
    const pin = pinOf[c.cc] || [(boxOf(c.cc)[0] + boxOf(c.cc)[2]) / 2, (boxOf(c.cc)[1] + boxOf(c.cc)[3]) / 2];
    const nodes = nets.map((n, i) => {
      const at = n.lat != null ? P(n.lat, n.lon) : [pin[0] + Math.cos(i * 2.4) * 26 * upp, pin[1] + Math.sin(i * 2.4) * 26 * upp];
      return { n, ax: at[0], ay: at[1], x: at[0], y: at[1], r: 6.5 + Math.min(5, Math.sqrt(n.probes || 0) * 0.8) };
    });
    // Push apart, in pixels at the view it flies to.
    for (let it = 0; it < 80; it++) {
      let moved = false;
      for (let i = 0; i < nodes.length; i++) for (let j = i + 1; j < nodes.length; j++) {
        const a = nodes[i], b = nodes[j];
        let dx = (b.x - a.x) / upp, dy = (b.y - a.y) / upp;
        const d = Math.hypot(dx, dy), need = a.r + b.r + 5;
        if (d >= need) continue;
        if (d < 0.01) { dx = Math.cos(i + j); dy = Math.sin(i + j); }
        const push = ((need - d) / 2) * upp, nx = dx / (d || 1), ny = dy / (d || 1);
        a.x -= nx * push; a.y -= ny * push; b.x += nx * push; b.y += ny * push;
        moved = true;
      }
      if (!moved) break;
    }
    for (const o of nodes) {
      const n = o.n;
      for (const ix of n.ixes || []) {
        const p = ixPos[ix];
        if (p) gM.append(svg("path", { class: `mb${ixes.find((x) => x.id === ix)?.k ? " bk" : ""}`, d: `M${o.x},${o.y}L${p[0]},${p[1]}`, "data-asn": n.asn }));
      }
      if (Math.hypot((o.x - o.ax) / upp, (o.y - o.ay) / upp) > 6) gM.append(svg("path", { class: "lead", d: `M${o.x},${o.y}L${o.ax},${o.ay}` }));
    }
    // Labels: most probes first, each kept only where it hits no other.
    const boxes = [];
    const hit = (b) => boxes.some((x) => b[0] < x[2] && b[2] > x[0] && b[1] < x[3] && b[3] > x[1]);
    for (const o of nodes) { const sx = o.x / upp, sy = o.y / upp; boxes.push([sx - o.r, sy - o.r, sx + o.r, sy + o.r]); }
    for (const x of ixes.slice(0, 8)) { const [px, py] = ixPos[x.id]; boxes.push([px / upp - x._r, py / upp - x._r, px / upp + x._r, py / upp + x._r]); }
    nodes.forEach((o) => {
      const n = o.n;
      const name = (n.name || `AS${n.asn}`).slice(0, 22);
      const lw = name.length * 6.3 + 6, sx = o.x / upp, sy = o.y / upp;
      const lb = [sx + o.r + 3, sy - 7, sx + o.r + 3 + lw, sy + 7];
      const show = !hit(lb);
      if (show) boxes.push(lb);
      const g = tag(mk(o.x, o.y, `nt ${n.state}`, svg("circle", { class: "dt", r: o.r }),
        n.probes ? svg("text", { class: "c", y: 3.3, "text-anchor": "middle" }, n.probes > 99 ? "99+" : String(n.probes)) : null,
        svg("text", { class: `lbl${show ? "" : " hid"}`, x: o.r + 4, y: 4 }, name)),
      [`${n.name || "AS" + n.asn} (AS${n.asn})`, `${plural(n.probes, "probe")}${n.med != null ? `, median ${ms(n.med)} to K` : ""}`, n.ixes_here ? `${plural(n.ixes_here, "exchange")} here${n.at_k ? ", one where K is" : ""}` : n.ixes_anywhere ? "on exchanges abroad only" : "on no exchange", n.at === "probes" ? "placed where its probes are" : n.at === "buildings" ? "placed where its buildings are" : "no buildings or probes with a place"],
      () => app.go("map", c.cc, n.asn));
      g.dataset.asn = n.asn;
      g.addEventListener("mouseenter", () => gM.querySelectorAll(`[data-asn="${n.asn}"]`).forEach((p) => p.classList.add("on")));
      g.addEventListener("mouseleave", () => gM.querySelectorAll(".on").forEach((p) => p.classList.remove("on")));
      gN.append(g);
    });
    ixes.slice(0, 8).forEach((x) => {
      const [px, py] = ixPos[x.id], sx = px / upp, sy = py / upp, lw = x.name.length * 6 + 4;
      const lb = [sx - lw / 2, sy + x._r + 2, sx + lw / 2, sy + x._r + 15];
      if (hit(lb)) return;
      boxes.push(lb);
      gX.append(mk(px, py, "ixl", svg("text", { y: x._r + 12, "text-anchor": "middle" }, x.name)));
    });
    gDet.append(gF, gP, gM, gX, gN);
    gDet.classList.add("in");
  }
  // The box to fly to for a country: its outline, unless it is a big one
  // (the United States, Russia, Brazil: then where its probes, buildings and
  // exchanges are, at least half its outline each way) or too small for
  // the world's outlines (then where they are).
  function countryBox(cc, c) {
    const gb = boxOf(cc);
    const pts = [...(c?.probes_at || []).map((p) => P(p[2], p[3])), ...(c?.facs || []).map((f) => P(f.lat, f.lon)), ...(c?.ixes || []).filter((x) => x.lat != null).map((x) => P(x.lat, x.lon))];
    const gw = gb[2] - gb[0];
    if (pts.length < 6 || (gw >= 7 && gw <= 120)) return gb;
    const xs = pts.map((p) => p[0]).sort((a, b) => a - b), ys = pts.map((p) => p[1]).sort((a, b) => a - b);
    let b = [quant(xs, 0.03), quant(ys, 0.03), quant(xs, 0.97), quant(ys, 0.97)];
    const mw = (gb[2] - gb[0]) * 0.5, mh = (gb[3] - gb[1]) * 0.5;
    if (b[2] - b[0] < mw) { const m = (b[0] + b[2]) / 2; b = [m - mw / 2, b[1], m + mw / 2, b[3]]; }
    if (b[3] - b[1] < mh) { const m = (b[1] + b[3]) / 2; b = [b[0], m - mh / 2, b[2], m + mh / 2]; }
    return b;
  }
  function dimNets(asn) {
    gDet.classList.toggle("focus", asn != null);
    gDet.querySelectorAll(".nt").forEach((g) => g.classList.toggle("sel", String(g.dataset.asn) === String(asn)));
  }
  // The network layer: its buildings, exchanges and probes, and a line to
  // each provider where they meet (a building they share, else the
  // provider's nearest), and to the trouble near it.
  function drawNetwork(n, cc) {
    fill(gTop);
    const t = n.net || {};
    const home = n.centre ? P(n.centre.lat, n.centre.lon) : pinOf[t.country] || null;
    const g = svg("g", { class: "netl" });
    const gl = svg("g", { class: "arcs" });
    for (const f of n.sites || []) {
      const [x, y] = P(f.lat, f.lon);
      g.append(tag(mk(x, y, "fac mine", svg("rect", { x: -3, y: -3, width: 6, height: 6, rx: 1.2 })), [f.name, `${f.city}, ${app.country(f.cc)}`, `a building ${t.name || "it"} is in`]));
    }
    for (const x of n.ixes || []) {
      if (x.lat == null) continue;
      const [px, py] = P(x.lat, x.lon);
      g.append(tag(mk(px, py, "ix mine", diamond(4.6)), [x.name, `${x.city}, ${app.country(x.cc)} · ${x.speed >= 1000 ? x.speed / 1000 + " Gb/s" : x.speed + " Mb/s"} port`], () => app.go("map", x.cc)));
    }
    g.append(dots("pb mine", 2.8, (n.probes_at || []).map(([id, lat, lon, rtt]) => {
      const [x, y] = P(lat, lon);
      return { x, y, c: probeColor(rtt), tip: [`Probe ${id}`, rtt == null ? "no answer from K in 20 minutes" : `${Math.round(rtt)} ms to the K root server`] };
    })));
    const ends = [];
    if (home) {
      (n.providers || []).slice(0, 8).forEach((p, i) => {
        const at = p.meet ? P(p.meet.lat, p.meet.lon) : pinOf[p.country];
        if (!at) return;
        const far = Math.hypot(at[0] - home[0], at[1] - home[1]);
        const end = far < 0.3 ? [at[0] + Math.cos(i * 1.7) * 1.2, at[1] - 0.8 - (i % 3) * 0.6] : at;
        const cls = p.state === "bad" ? "bad" : p.state === "warn" ? "warn" : "up";
        gl.append(svg("path", { class: `ar ${cls}`, d: arc(home, end, 0.2), pathLength: 1, style: `animation-delay:${0.15 + i * 0.07}s` }));
        g.append(tag(mk(end[0], end[1], `up ${cls}`, svg("circle", { r: 4.2 }), svg("text", { x: 7, y: 4 }, (p.name || "AS" + p.asn).slice(0, 20))),
          [`${p.name || "AS" + p.asn} (AS${p.asn})`, `a provider of ${t.name || "it"}${p.meet ? (p.meet.shared ? ", in a building both are in" : ", at its building nearest") : `, in ${app.country(p.country)}`}`, p.state !== "ok" ? "has open alerts" : null], () => app.go("as", p.asn)));
        ends.push(end);
      });
      (n.trouble || []).slice(0, 5).forEach((x, i) => {
        const at = pinOf[x.country];
        if (!at) return;
        gl.append(svg("path", { class: "ar tr", d: arc(home, at, -0.25), pathLength: 1, style: `animation-delay:${0.5 + i * 0.08}s` }));
        g.append(tag(mk(at[0], at[1], "up tr", svg("circle", { r: 3.6 })), [`${x.name || "AS" + x.asn} (AS${x.asn})`, `${plural(x.hops, "hop")} away: ${x.title || "open alerts"}`], () => app.go("as", x.asn)));
      });
      g.append(tag(mk(home[0], home[1], `me ${n.state}`, svg("circle", { class: "halo2", r: 13 }), svg("circle", { class: "dt", r: 8.5 }), svg("text", { class: "lbl", x: 13, y: 4.5 }, (t.name || "AS" + t.asn).slice(0, 26))),
        [`${t.name || "AS" + t.asn} (AS${t.asn})`, n.centre ? "placed at the middle of its buildings" : `in ${app.country(t.country)}`]));
    }
    gTop.append(gl, g);
    // The box: its buildings and probes (the middle 90%), those in the
    // country it was opened from when there are a few, so a network seen
    // from its country is a zoom into it; then its providers near it.
    let pts = [...(n.sites || []).map((f) => P(f.lat, f.lon)), ...(n.probes_at || []).map((p) => P(p[1], p[2])), ...(n.ixes || []).filter((x) => x.lat != null).map((x) => P(x.lat, x.lon))];
    if (home) pts.push(home);
    let least = [8, 5];
    if (cc) {
      const cb = boxOf(cc), mx = (cb[2] - cb[0]) * 0.25, my = (cb[3] - cb[1]) * 0.25;
      const inside = pts.filter(([x, y]) => x >= cb[0] - mx && x <= cb[2] + mx && y >= cb[1] - my && y <= cb[3] + my);
      if (inside.length >= 3) { pts = inside; least = [Math.max(8, (cb[2] - cb[0]) * 0.6), Math.max(5, (cb[3] - cb[1]) * 0.6)]; }
    }
    if (!pts.length) return boxOf(t.country);
    const xs = pts.map((p) => p[0]).sort((a, b) => a - b), ys = pts.map((p) => p[1]).sort((a, b) => a - b);
    const lo = pts.length > 12 ? 0.05 : 0, hi = 1 - lo;
    let b = [quant(xs, lo), quant(ys, lo), quant(xs, hi), quant(ys, hi)];
    const bw = Math.max(b[2] - b[0], least[0]), bh = Math.max(b[3] - b[1], least[1]);
    const cx = (b[0] + b[2]) / 2, cy = (b[1] + b[3]) / 2;
    b = [cx - bw / 2, cy - bh / 2, cx + bw / 2, cy + bh / 2];
    // Providers further away leave by their arcs, without pulling it.
    for (const e of ends) if (Math.abs(e[0] - cx) <= bw * 2 && Math.abs(e[1] - cy) <= bh * 2) b = [Math.min(b[0], e[0]), Math.min(b[1], e[1]), Math.max(b[2], e[0]), Math.max(b[3], e[1])];
    return b;
  }
  // Countries and the routes between them, for a prefix or an incident.
  function drawPlaces(ccs, arcs) {
    fill(gTop);
    const gl = svg("g", { class: "arcs" }), g = svg("g", {});
    arcs.forEach((a, i) => {
      const p = typeof a.from === "string" ? pinOf[a.from] : a.from, q = typeof a.to === "string" ? pinOf[a.to] : a.to;
      if (!p || !q || (p[0] === q[0] && p[1] === q[1])) return;
      gl.append(tag(svg("path", { class: `ar ${a.cls || "up"}`, d: arc(p, q, a.bow ?? 0.2), pathLength: 1, style: `animation-delay:${0.2 + i * 0.12}s` }), a.tip || null));
    });
    for (const cc of ccs) {
      const p = pinOf[cc];
      if (p) g.append(tag(mk(p[0], p[1], "place", svg("circle", { r: 4 }), svg("text", { x: 7, y: 4 }, app.country(cc))), [app.country(cc)], () => app.go("map", cc)));
    }
    gTop.append(gl, g);
    let b = null;
    for (const cc of ccs) {
      const x = boxOf(cc);
      b = b ? [Math.min(b[0], x[0]), Math.min(b[1], x[1]), Math.max(b[2], x[2]), Math.max(b[3], x[3])] : x;
    }
    return b || [0, 0, W, H];
  }
  function clearDetail() { fill(gDet); gDet.classList.remove("in", "focus"); base.cc = null; base.country = null; }

  apply();
  drawLegend("world");
  setCrumbs([{ t: "World" }]);
  return {
    el: s, paintWorld, after,
    get world() { return world; },
    hint(cc) { hinted = new Set(cc || []); paintLand(); },
    world0() {
      focusCcs = new Set(); dimOthers = false; paintLand();
      clearDetail(); fill(gTop);
      s.classList.remove("in-country", "in-places");
      drawLegend("world"); setCrumbs([{ t: "World" }]);
      flyTo({ x: W / 2, y: H / 2 + 10, w: W });
    },
    // Fly to a country at once, from its outline; its layer when it comes.
    country(cc, crumbs) {
      fill(gTop);
      focusCcs = new Set([cc]); dimOthers = true; paintLand();
      s.classList.add("in-country"); s.classList.remove("in-places");
      if (base.cc !== cc) clearDetail(); else dimNets(null);
      drawLegend("country"); setCrumbs(crumbs);
      const v = boxView(boxOf(cc), 0.08);
      flyTo(v);
      return v;
    },
    countryData(cc, c, { fly = true } = {}) {
      const v = boxView(countryBox(cc, c), 0.1);
      if (base.cc !== cc) drawCountry(c, v);
      if (fly) flyTo(v);
      dimNets(null);
    },
    network(n, { cc = null, crumbs } = {}) {
      const t = n.net || {};
      const keep = cc && base.cc === cc;
      if (!keep) clearDetail();
      focusCcs = new Set([cc || t.country].filter(Boolean)); dimOthers = true; paintLand();
      s.classList.add("in-country"); s.classList.remove("in-places");
      dimNets(keep ? n.asn : null);
      drawLegend("network"); setCrumbs(crumbs);
      flyTo(boxView(drawNetwork(n, cc), 0.12));
    },
    places(ccs, arcs, crumbs) {
      clearDetail();
      focusCcs = new Set(ccs); dimOthers = true; paintLand();
      s.classList.remove("in-country"); s.classList.add("in-places");
      drawLegend("places"); setCrumbs(crumbs);
      flyTo(boxView(drawPlaces(ccs, arcs), 0.25));
    },
    // A country's layer under a network opened on its own, without moving.
    underlay(cc, c, asn) {
      if (base.cc !== cc) drawCountry(c, boxView(countryBox(cc, c), 0.06));
      dimNets(asn);
    },
    pinOf, boxOf,
  };
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

// ---------------------------------------------------------------- the strip

// Everything heard a minute for six hours: announcements above the line,
// withdrawals below it, RPKI-invalid announcements as red ticks, and the
// incidents as bars under it. What the inspector shows (a network's or a
// prefix's updates, an incident's window) is drawn over it.
function makeTimeline(app) {
  const plot = h("div", { class: "bn-tl-plot" });
  const tip = h("div", { class: "bn-tl-tip", hidden: true });
  const lab = h("div", { class: "bn-tl-lab" });
  const el = h("div", { class: "bn-tl" }, lab, plot, tip);
  let data = null, mark = null;
  function draw() {
    const W = Math.max(300, Math.round(plot.clientWidth || 900)), H = 74, MID = 30, UP = 27, DN = 18, LANE = 50;
    const k = (n) => (n >= 1e6 ? (n / 1e6).toFixed(1) + "M" : n >= 1e3 ? Math.round(n / 1e3) + "k" : String(n));
    if (!data) { fill(plot, h("div", { class: "bn-skel" }, h("div", { style: { width: "70%" } }))); return; }
    const mins = (data.minutes || []).map(([t, a, w, bad]) => ({ t: Date.parse(t), a, w, bad }));
    const t1 = Date.parse(data.as_of), t0 = t1 - 6 * 3600e3;
    const X = (t) => ((t - t0) / (t1 - t0)) * W;
    // The scale: a peer resending its whole table makes one minute ten
    // times the others, so the top is the 99th percentile's and such a
    // minute reaches it (its tooltip has the number).
    const vals = mins.map((m) => Math.max(m.a, m.w)).sort((p, q) => p - q);
    const max = Math.max(1, Math.min(vals[vals.length - 1] || 1, (vals[Math.floor((vals.length - 1) * 0.99)] || 1) * 1.4));
    const cap = (v) => Math.min(v, max);
    const last = mins[mins.length - 1];
    fill(lab, h("b", {}, "Everything heard, a minute, the last six hours"),
      h("span", { class: "k a" }, "announced"), h("span", { class: "k w" }, "withdrawn"), h("span", { class: "k bad" }, "RPKI-invalid"), h("span", { class: "k inc" }, "incidents"),
      mark?.label ? h("span", { class: "k me" }, mark.label) : null,
      mins.length ? h("span", { class: "muted" }, `top: ${k(max)} a minute`) : null,
      last ? h("span", { class: "now" }, `${hm(new Date(last.t).toISOString())}: ${fmt(last.a)} announced, ${fmt(last.w)} withdrawn`) : null);
    const s = svg("svg", { class: "bn-tl-svg", width: W, height: H, viewBox: `0 0 ${W} ${H}` });
    // Hours, in Pacific time.
    const hr = 3600e3;
    for (let t = Math.ceil(t0 / hr) * hr; t < t1; t += hr) {
      const x = X(t);
      s.append(svg("line", { class: "hr", x1: x, x2: x, y1: 0, y2: LANE - 2 }), svg("text", { class: "ax", x: x + 3, y: H - 2 }, new Date(t).toLocaleTimeString("en-US", { timeZone: TZ, hour: "numeric" })));
    }
    if (mark?.from) {
      const a = X(Date.parse(mark.from)), b = mark.to ? X(Date.parse(mark.to)) : W;
      s.append(svg("rect", { class: "win", x: Math.max(0, a), y: 0, width: Math.max(2, Math.min(W, b) - Math.max(0, a)), height: LANE - 2 }));
    }
    const up = [], dn = [];
    for (const m of mins) { const x = X(m.t).toFixed(1); up.push(`${x},${(MID - (cap(m.a) / max) * UP).toFixed(1)}`); dn.push(`${x},${(MID + Math.min(DN, (m.w / max) * DN * 1.5)).toFixed(1)}`); }
    if (mins.length) {
      const x0 = X(mins[0].t).toFixed(1), x1 = X(last.t).toFixed(1);
      s.append(svg("path", { class: "a", d: `M${x0},${MID}L${up.join("L")}L${x1},${MID}Z` }), svg("path", { class: "w", d: `M${x0},${MID}L${dn.join("L")}L${x1},${MID}Z` }));
    }
    s.append(svg("line", { class: "mid", x1: 0, x2: W, y1: MID, y2: MID }));
    for (const m of mins) if (m.bad > 0) { const x = X(m.t); s.append(svg("line", { class: "bad", x1: x, x2: x, y1: 0, y2: Math.min(9, 2 + Math.log10(m.bad + 1) * 3) })); }
    if (mark?.pts?.length) {
      const mm = Math.max(1, ...mark.pts.map((p) => p[1]));
      const d = mark.pts.map((p, i) => `${i ? "L" : "M"}${X(Date.parse(p[0])).toFixed(1)},${(MID - (p[1] / mm) * UP).toFixed(1)}`).join("");
      s.append(svg("path", { class: "me", d }));
    }
    // Incidents in lanes, so bars that overlap in time stack.
    const lanes = [];
    const inc = (data.incidents || []).map((i) => ({ i, a: Math.max(0, X(Date.parse(i.first))), b: Math.min(W, i.state === "open" ? W : X(Date.parse(i.last))) })).filter((x) => x.b > 0).sort((p, q) => p.a - q.a);
    for (const x of inc) {
      let l = lanes.findIndex((end) => end < x.a - 2);
      const cur = mark?.incident === x.i.id;
      if (l < 0) { if (lanes.length < 4) { l = lanes.length; lanes.push(0); } else if (cur) l = 3; else continue; }
      lanes[l] = Math.max(lanes[l], x.b);
      const before = Date.parse(x.i.first) < t0;
      const r = svg("rect", { class: `ib ${x.i.state !== "open" ? "ended" : x.i.severity}${before ? " old" : ""}${cur ? " on" : ""}`, x: x.a, y: LANE + l * 4, width: Math.max(3, x.b - x.a), height: 3, rx: 1 });
      r.addEventListener("click", () => app.go("incident", x.i.id));
      r.__i = x.i;
      s.append(r);
    }
    const cross = svg("line", { class: "cross", x1: 0, x2: 0, y1: 0, y2: LANE - 2, visibility: "hidden" });
    s.append(cross);
    s.addEventListener("mousemove", (e) => {
      const r = s.getBoundingClientRect(), x = e.clientX - r.left;
      const t = t0 + (x / W) * (t1 - t0);
      let best = null;
      for (const m of mins) if (!best || Math.abs(m.t - t) < Math.abs(best.t - t)) best = m;
      const over = e.target.__i;
      cross.setAttribute("x1", x); cross.setAttribute("x2", x); cross.setAttribute("visibility", "visible");
      fill(tip, over ? [h("b", {}, over.title), h("div", {}, `${kindWord(over.kind)} · ${hm(over.first)} to ${over.state === "open" ? "now" : hm(over.last)}`), h("div", { class: "muted" }, "click to open")]
        : best ? [h("b", {}, hm(new Date(best.t).toISOString())), h("div", {}, `${fmt(best.a)} announced, ${fmt(best.w)} withdrawn`), best.bad ? h("div", { class: "bad" }, `${fmt(best.bad)} RPKI-invalid`) : null] : []);
      tip.hidden = false;
      tip.style.left = `${Math.min(W - 220, Math.max(0, x + 12))}px`;
    });
    s.addEventListener("mouseleave", () => { tip.hidden = true; cross.setAttribute("visibility", "hidden"); });
    fill(plot, s);
  }
  new ResizeObserver(() => { if (plot.clientWidth) draw(); }).observe(plot);
  return {
    el,
    async refresh() { try { data = await app.api("/v1/timeline", { fresh: true, sql: false }); } catch { /* keep the last */ } draw(); },
    mark(m) { mark = m; draw(); },
  };
}

// ---------------------------------------------------------------- the console

// One screen that stays: the worklist on the left, the map in the middle,
// the inspector on the right, the last six hours along the top and the
// networks the reader watches under it. A click moves the camera and fills
// the inspector; nothing else is built again.
function makeConsole(app) {
  const tl = makeTimeline(app);
  const left = h("aside", { class: "bn-wl", "aria-label": "What is wrong now" });
  const stage = h("div", { class: "bn-stage" });
  const right = h("aside", { class: "bn-in", "aria-label": "Inspector" });
  const body = h("div", { class: "bn-con-body" }, left, stage, right);
  const el = h("div", { class: "bn-con" }, tl.el, app.watch.bar(), body);
  const map = makeMap(app, stage);
  let sel = { page: "map", rest: [] }, seq = 0, filter = "";
  tl.refresh();
  app.watch.refresh();
  function fit() {
    if (matchMedia("(max-width: 1100px)").matches) { body.style.height = ""; return; }
    const top = body.getBoundingClientRect().top + scrollY;
    body.style.height = `${Math.max(540, innerHeight - top - 14)}px`;
  }
  addEventListener("resize", () => { if (el.isConnected && el.offsetParent) fit(); });
  app.watch.listeners.add(() => { if (sel.page === "as" || (sel.page === "map" && sel.rest[1])) { const b = right.querySelector(".bn-watch-btn"); if (b) b.replaceWith(watchBtn(Number(b.dataset.asn))); } });

  // ---- the worklist
  function drawList() {
    const n = app.now;
    if (!n) return fill(left, skel(10));
    const open = n.open || [];
    const kinds = {};
    for (const i of open) kinds[i.kind] = (kinds[i.kind] || 0) + 1;
    if (filter && !kinds[filter]) filter = "";
    const shown = filter ? open.filter((i) => i.kind === filter) : open;
    const cur = sel.page === "incident" ? sel.rest.join("/") : null;
    const scroll = left.scrollTop;
    fill(left,
      h("div", { class: "bn-wl-hd" },
        h("div", { class: "row between" }, h("b", {}, open.length ? plural(open.length, "problem") + " now" : "Nothing wrong now"), h("span", { class: "muted small" }, clock(n.as_of))),
        h("div", { class: "muted small" }, n.connected ? `${n.connected} of them share an announcer, an upstream or a country` : "most serious first; the rules run every minute"),
        Object.keys(kinds).length > 1 ? h("div", { class: "bn-wl-kinds" },
          h("button", { type: "button", class: !filter ? "on" : null, onclick: () => { filter = ""; drawList(); } }, "All"),
          Object.entries(kinds).sort((a, b) => b[1] - a[1]).map(([k, c]) => h("button", { type: "button", class: filter === k ? "on" : null, onclick: () => { filter = filter === k ? "" : k; drawList(); } }, kindWord(k), h("span", {}, String(c))))) : null),
      h("div", { class: "bn-wl-list" }, shown.length ? shown.map((i) => item(i, cur)) : empty("No rule finds anything in the last hour.")),
      (n.ended || []).length ? h("details", { class: "bn-wl-more" }, h("summary", {}, `Ended in the last hour (${n.ended.length})`), n.ended.slice(0, 12).map((i) => item(i, cur))) : null,
      (n.weather || []).length ? h("div", { class: "bn-wl-sec" }, h("b", {}, "Slowest countries to K"), n.weather.map((w) => h("a", { class: "bn-wl-wx", href: app.href("map", w.country) }, h("span", {}, app.country(w.country)), h("span", { class: w.med > 150 ? "bad" : w.med > 60 ? "warn" : "" }, ms(w.med))))) : null,
      n.heard?.updates != null ? h("div", { class: "bn-wl-foot" }, `Heard by ${fmt(n.heard.peers)} BGP peers at RIPE RIS in the last ${n.heard.minutes || 5} minutes: ${fmt(n.heard.updates)} updates of ${fmt(n.heard.prefixes)} prefixes.`) : null,
      h("div", { class: "bn-wl-foot" }, "Keys: j and k move through the list, Esc goes up a level, / searches."));
    left.scrollTop = scroll;
  }
  function item(i, cur) {
    return h("a", { class: `bn-wi ${i.state !== "open" ? "ended" : i.severity}${i.id === cur ? " on" : ""}`, href: app.href("incident", i.id), "data-id": i.id,
      onmouseenter: () => map.hint(i.countries), onmouseleave: () => map.hint(null) },
      h("div", { class: "k" }, h("span", {}, kindWord(i.kind), i.alerts > 1 ? ` ×${i.alerts}` : ""), h("span", { class: "w" }, i.state === "open" ? `since ${hm(i.first)}` : `${hm(i.first)} to ${hm(i.last)}`)),
      h("div", { class: "t" }, i.title),
      i.summary ? h("div", { class: "s" }, i.summary) : null);
  }

  // ---- the inspector
  const wideBtn = () => h("button", { type: "button", class: "btn xs ghost bn-in-wide", title: "A wider inspector", "aria-label": "A wider inspector", onclick: () => { el.classList.toggle("wide"); } }, icon("wide"));
  function watchBtn(asn) {
    const on = app.watch.has(asn);
    return h("button", { type: "button", class: `btn xs bn-watch-btn${on ? " on" : ""}`, "data-asn": asn, onclick: () => app.watch.toggle(asn) }, icon("star"), on ? "Watching" : "Watch");
  }
  function ihead({ crumbs = [], title, mono = false, tag = null, sub = [], why = null, whyOk = false, facts = [], actions = [] }) {
    return h("div", { class: "bn-in-hd" },
      h("div", { class: "bn-in-crumbs" }, crumbs.map((c, i) => [i ? h("span", { class: "muted" }, "›") : null, c.href ? h("a", { href: c.href }, c.t) : h("span", {}, c.t)]), wideBtn()),
      h("div", { class: "row between" }, h("h3", { class: mono ? "mono" : null }, title), tag),
      sub.filter(Boolean).length ? h("div", { class: "muted small" }, sub.filter(Boolean).join(" · ")) : null,
      why ? h("div", { class: `bn-map-why${whyOk ? " ok" : ""}` }, why) : null,
      facts.length ? h("div", { class: "bn-facts" }, facts.filter(Boolean).map(([k, v]) => h("span", {}, `${k} `, h("b", {}, v)))) : null,
      actions.filter(Boolean).length ? h("div", { class: "bn-btnrow" }, actions) : null);
  }
  // Tabs that remember the last one picked for each kind of thing.
  function tabbed(kind, defs) {
    const bar = h("div", { class: "tabs bn-otabs", role: "tablist" });
    const bodyEl = h("div", { class: "bn-in-body" });
    let cur = defs.some((d) => d[0] === app.tabs[kind]) ? app.tabs[kind] : defs[0][0];
    const draw = () => {
      fill(bar, defs.map(([name, n, , tone]) => h("button", { type: "button", role: "tab", "aria-selected": String(name === cur), class: name === cur ? "on" : null, onclick: () => { cur = name; app.tabs[kind] = name; draw(); } },
        name, n ? h("span", { class: `n${tone ? " " + tone : ""}` }, String(n)) : null)));
      fill(bodyEl, defs.find((d) => d[0] === cur)[2]());
    };
    draw();
    return [bar, bodyEl];
  }
  const none = (t) => h("div", { class: "small muted bn-none" }, t);
  const crumbsFor = (cc) => {
    const g = app.geo?.countries?.[cc];
    return [{ t: "World", href: app.href("map") }, g?.c ? { t: g.c } : null, { t: app.country(cc), href: app.href("map", cc) }].filter(Boolean);
  };

  function inspectWorld() {
    const n = app.now, w = app.world;
    if (!n || !w) return fill(right, skel(10));
    const open = n.open || [];
    const bad = open.some((i) => i.severity === "bad");
    const lines = summaryLines(n);
    const sentence = open.length ? `${plural(open.length, "problem")} now.${n.connected ? ` ${n.connected} of them are connected.` : ""}` : "Nothing wrong now.";
    const byId = Object.fromEntries(open.map((i) => [i.id, i]));
    fill(right,
      ihead({ crumbs: [{ t: "World" }], title: open.length ? plural(open.length, "problem") + " now" : "All clear", tag: open.length ? pill(bad ? "bad" : "warn", bad ? "red" : "watch") : pill("ok", "ok"),
        sub: [`the last hour, as of ${clock(n.as_of)}`] }),
      h("div", { class: "bn-in-sum" }, lines.length ? h("ul", {}, lines.map((l) => h("li", {}, l))) : h("p", {}, "No rule finds anything in the last hour."),
        h("button", { class: "btn xs", type: "button", onclick: (e) => { navigator.clipboard?.writeText([sentence, ...lines.map((l) => "- " + l)].join("\n")); e.currentTarget.textContent = "Copied"; } }, icon("copy"), "Copy the summary")),
      h("div", { class: "bn-map-kpis" },
        h("div", {}, h("b", {}, fmt(open.length)), h("span", {}, "open incidents")),
        h("div", {}, h("b", {}, fmt(w.red_countries || 0)), h("span", {}, "countries red")),
        h("div", {}, h("b", {}, fmt(w.open_alerts || 0)), h("span", {}, "open alerts"))),
      sec("How they connect", "what the open problems share",
        (n.connect || []).length ? n.connect.map((c) => h("div", { class: "bn-conn" },
          h("div", { class: "bn-conn-t" }, c.net ? h("a", { href: app.href("as", c.net.asn) }, c.title) : c.country ? h("a", { href: app.href("map", c.country) }, c.title.replace(c.country, app.country(c.country))) : c.title),
          h("div", { class: "muted small" }, c.how === "country" ? c.text.replace(c.country, app.country(c.country)) : c.text),
          h("div", { class: "bn-conn-ids" }, c.incidents.slice(0, 5).map((id) => byId[id] ? h("a", { class: "chip", href: app.href("incident", id) }, byId[id].title.slice(0, 44)) : null))))
          : none(open.length ? "No two open problems share an owner, an announcer, an upstream or a country." : "Nothing open to connect.")),
      sec("Internet weather", "the slowest countries to the nearest K root server, last 20 minutes",
        (n.weather || []).length ? hbars([...n.weather.map((x) => ({ label: app.country(x.country), href: app.href("map", x.country), text: ms(x.med), v: x.med, cls: x.med > 150 ? "bad" : x.med > 60 ? "warn" : "ok" })), { label: "World median", text: ms(n.world?.med), v: n.world?.med, cls: "ok" }]) : none("No Atlas results yet.")),
      sec("Busiest prefixes", "updates in the last hour", (n.busiest || []).length ? n.busiest.map((b) => row(SEV.warn, b.prefix, `${b.origin.name || "AS" + b.origin.asn}${b.origin.country ? `, ${b.origin.country}` : ""} · ${fmt(b.updates)} updates`, null, app.href("prefix", b.prefix))) : none("No updates yet.")),
      sec("Sources", "where each feed stands", (n.feed || []).map((f) => row(SEV.none, f.id, `${f.kind}${f.rows ? ` · ${fmt(f.rows)} rows` : ""} · ${ago(f.at)}${f.note ? ` · ${f.note}` : ""}`))));
  }

  function inspectCountry(c) {
    const cc = c.cc, name = app.country(cc), cont = app.geo?.countries?.[cc]?.c || "";
    const nets = c.nets || [], alerts = c.alerts || [], outs = c.outages || [], ab = c.abroad || [];
    const trouble = nets.filter((n) => n.state === "bad" || n.state === "warn");
    const rtts = (c.probes_at || []).map((p) => p[4]).filter((x) => x != null);
    const nWrong = alerts.length + outs.length;
    const mine = [...(app.now?.open || []), ...(app.now?.ended || [])].filter((i) => (i.countries || []).includes(cc));
    const [bar, bodyEl] = tabbed("country", [
      ["What's wrong", nWrong, () => [
        alerts.length ? sec("Open alerts", `bb_alerts WHERE country = '${cc}'`, alerts.slice(0, 20).map((a) => row(SEV[a.severity], a.title, a.detail, null, a.prefix ? app.href("prefix", a.prefix) : app.href("incident", a.incident)))) : null,
        outs.length ? sec("IODA outages", "bb_outage WHERE open", outs.map((o) => row(SEV.bad, o.name || o.id, `${o.kind} · ${o.source} · since ${stamp(o.start)} · score ${short(o.score)}`))) : null,
        trouble.length ? sec("Networks slow or with alerts", "its networks with probes, worst first", trouble.map((n) => row(SEV[n.state], `${n.name} (AS${n.asn})`, `${plural(n.probes, "probe")} · median ${ms(n.med)}${n.ixes_here ? "" : " · on no exchange here"}`, null, app.href("map", cc, n.asn)))) : null,
        ab.length ? sec("Where slow traceroutes leave it", "bb_trace WHERE rtt > 100 GROUP BY jump_as", ab.map((a) => row(SEV.warn, netName(a.net), `${plural(a.traces, "traceroute")} · +${Math.round(a.jump)} ms at that hop · ${ms(a.med)} in all`, null, app.href("as", a.net.asn)))) : null,
        !nWrong && !trouble.length ? none(`Nothing wrong in ${name}: no open alerts or outages, and its networks with probes reach K in under 60 ms.`) : null], nWrong ? "bad" : null],
      ["What changed", mine.length, () => [
        sec("Incidents here, the last hour", "open first", mine.length ? mine.map((i) => row(i.state === "open" ? SEV[i.severity] : SEV.ended, i.title, `${kindWord(i.kind)} · ${i.state === "open" ? `since ${hm(i.first)}` : `${hm(i.first)} to ${hm(i.last)}`}`, null, app.href("incident", i.id))) : none("None in the last hour.")),
        alerts.length ? sec("When each alert began", "bb_alerts.first_seen", h("div", { class: "bn-map-hist" }, alerts.slice().sort((a, b) => (b.first || "").localeCompare(a.first || "")).slice(0, 16).map((a) => h("div", { class: "fault" }, h("span", { class: "when" }, stamp(a.first)), h("span", {}, a.title))))) : null]],
      ["Metrics", null, () => [
        sec("Its probes' time to K", "each probe's latest ping, last 20 minutes", rtts.length ? rttHist(rtts) : none("No probe answered in the last 20 minutes."),
          h("div", { class: "bn-facts" }, h("span", {}, "median ", h("b", {}, ms(c.med))), h("span", {}, "world ", h("b", {}, ms(app.now?.world?.med))), h("span", {}, "over 100 ms ", h("b", {}, `${fmt(c.slow || 0)} of ${fmt(c.probes || 0)}`)))),
        nets.length ? sec("Its networks, by median", "bb_ping GROUP BY asn", hbars(nets.filter((n) => n.med != null).slice(0, 14).map((n) => ({ label: n.name || `AS${n.asn}`, href: app.href("map", cc, n.asn), text: ms(n.med), v: n.med, cls: n.med > 150 ? "bad" : n.med > 60 ? "warn" : "ok" })))) : null]],
      ["Records", null, () => [
        sec("Exchanges", `bb_ix WHERE country = '${cc}', largest first`, (c.ixes || []).length ? c.ixes.map((x) => row("var(--s7)", x.name, `${x.city} · ${fmt(x.members)} members${x.k ? " · K root is a member" : ""}`)) : none("None in PeeringDB.")),
        sec("Buildings", "bb_fac, most networks first", (c.facs || []).length ? c.facs.slice(0, 14).map((f) => row(SEV.none, f.name, `${f.city} · ${plural(f.nets, "network")}`)) : none("None in PeeringDB.")),
        sec("Networks with probes", "bb_ping GROUP BY asn, most probes first", nets.map((n) => row(SEV[n.state] || SEV.none, `${n.name} (AS${n.asn})`, `${plural(n.probes, "probe")} · median ${ms(n.med)} · ${n.ixes_here ? plural(n.ixes_here, "exchange") + " here" : "no exchange here"}${n.at_k ? " · meets K" : ""}`, null, app.href("map", cc, n.asn))),
          c.more?.networks ? none(`and ${c.more.networks} more: ${c.more.under20} under 20 ms, ${c.more.over90} over 90 ms`) : null)]],
    ]);
    fill(right,
      ihead({ crumbs: crumbsFor(cc).slice(0, -1).concat([{ t: name }]), title: name, tag: c.med > 150 ? pill("bad", "slow") : c.med > 60 ? pill("warn", "slowish") : c.probes ? pill("ok", "fast") : null,
        sub: [cont, `${fmt(c.ix_total || 0)} exchanges`, `${fmt((c.facs || []).length)}${(c.facs || []).length >= 80 ? "+" : ""} buildings`, `${fmt(c.probes || 0)} probes`],
        why: c.med != null ? `${fmt(c.slow || 0)} of ${fmt(c.probes || 0)} probes take over 100 ms to reach the nearest K root server; the median is ${ms(c.med)}.` : null, whyOk: !(c.med > 100),
        facts: [["median", ms(c.med)], ["world", ms(app.now?.world?.med)], ["open alerts", fmt(alerts.length)], ["outages", fmt(outs.length)]],
        actions: [h("a", { class: "btn xs", href: app.href("diagnose", cc) }, `Diagnose ${name}'s stack`)] }),
      bar, bodyEl);
  }

  function inspectNetwork(n, { cc = null, country = null } = {}) {
    if (!n.found) return fill(right, ihead({ crumbs: [{ t: "World", href: app.href("map") }], title: `AS${n.asn}` }), none(`AS${n.asn} is not in the registries' list of networks.`));
    const t = n.net;
    const open = (n.alerts || []).filter((a) => a.state === "open");
    const trouble = n.trouble || [];
    const sevOf = (s) => SEV[s] || SEV.ok;
    const pts = (n.minutes || []).map((m) => ({ t: m.t, a: m.n - m.w, w: m.w }));
    const [bar, bodyEl] = tabbed("network", [
      ["What's wrong", open.length + trouble.length, () => [
        trouble.length ? h("div", { class: "bn-map-verdict" }, h("b", {}, `Trouble ${plural(trouble[0].hops, "hop")} away: ${trouble[0].name || "AS" + trouble[0].asn} (AS${trouble[0].asn})`), h("div", { class: "small" }, trouble[0].title || "")) : null,
        sec("Its open alerts", "bb_alerts WHERE origin, owner or asn is it", open.length ? open.slice(0, 16).map((a) => row(SEV[a.severity], a.title, `${kindWord(a.kind)} · ${a.detail || ""} · since ${stamp(a.first)}`, null, a.prefix ? app.href("prefix", a.prefix) : app.href("incident", a.incident))) : none("None open.")),
        sec(`Trouble within ${n.trouble_hops === 1 ? "one hop" : "three hops"}`, `REACHABLE(bb_p2c, hops => ${n.trouble_hops}, dir => 'both') AND asn IN (open alerts)`,
          trouble.length ? trouble.map((x) => row(SEV[x.severity] || SEV.warn, `${x.name || "AS" + x.asn} (AS${x.asn})`, `${plural(x.hops, "hop")} · ${x.title || ""}`, null, app.href("as", x.asn))) : none("No open alerts on networks that near.")),
        sec("What it hangs from", "REACHABLE(bb_p2c, asn, dir => 'in'): its providers", (n.providers || []).length
          ? n.providers.slice(0, 8).map((p) => row(sevOf(p.state), `${p.name || "AS" + p.asn} (AS${p.asn})`, `provider · ${p.country || "?"}${p.meet?.shared ? " · in a building with it" : ""} · ${fmt(p.customers)} customers`, null, app.href("as", p.asn)))
          : none("No providers in CAIDA's relationships: transit-free, or not seen."))], open.length ? "bad" : trouble.length ? "warn" : null],
      ["What changed", (n.changes || []).length, () => [
        sec("Changes and faults", "bb_alerts and PeeringDB's records, newest first",
          (n.changes || []).length ? h("div", { class: "bn-map-hist" }, n.changes.map((c) => h("div", { class: c.kind }, h("span", { class: "when" }, stamp(c.at)), h("span", {}, h("b", {}, c.kind === "fault" ? "Fault " : "Change "), c.text, c.incident ? [" · ", h("a", { href: app.href("incident", c.incident) }, "incident")] : null)))) : none("Nothing recorded.")),
        sec("Its routes now", "bb_updates WHERE origin = asn, last day, GROUP BY rpki",
          (n.rpki || []).length ? h("div", { class: "bn-facts" }, n.rpki.map((r) => h("span", { class: /invalid/.test(r.rpki) ? "bad" : null }, `${r.rpki || "not checked"} `, h("b", {}, fmt(r.prefixes)), " prefixes"))) : none("rrc00 heard no announcement from it in the last day."))]],
      ["Metrics", null, () => [
        sec("Updates of its prefixes a minute", "GROUP BY date_trunc('minute', ts), last three hours", h("div", { class: "bn-chart-box" }, minutes(pts, [{ key: "a", color: "var(--s1)", label: "announcements" }, { key: "w", color: "var(--s2)", label: "withdrawals" }], { height: 150 })), legend([["var(--s1)", "announcements"], ["var(--s2)", "withdrawals"]])),
        sec("Its probes to K", "bb_ping, last 20 minutes", (n.probes_at || []).some((p) => p[3] != null) ? rttHist((n.probes_at || []).map((p) => p[3]).filter((x) => x != null)) : none("None of its probes answered in the last 20 minutes."),
          h("div", { class: "bn-facts" }, h("span", {}, "probes ", h("b", {}, fmt(n.probes_now))), h("span", {}, "median ", h("b", {}, ms(n.med))), country?.med != null ? h("span", {}, "country ", h("b", {}, ms(country.med))) : null)),
        sec("Its prefixes", "announced in the last day, most updates first", (n.prefixes || []).length ? n.prefixes.slice(0, 12).map((p) => row(SEV.none, p.prefix, `${fmt(p.updates)} updates · ${p.peers} peers · last ${stamp(p.last)}`, null, app.href("prefix", p.prefix))) : none("None heard in the last day."))]],
      ["Records", null, () => [
        sec("Customers", `${fmt(t.customers)} in CAIDA's relationships`, (n.customers || []).length ? n.customers.slice(0, 8).map((p) => row(SEV.none, `${p.name || "AS" + p.asn} (AS${p.asn})`, `${p.country || "?"}${p.customers ? ` · ${fmt(p.customers)} customers of its own` : ""}`, null, app.href("as", p.asn))) : none("None.")),
        sec("Exchanges", "bb_member JOIN bb_ix", (n.ixes || []).length ? n.ixes.slice(0, 20).map((x) => row("var(--s7)", x.name, `${x.city}, ${x.cc} · ${x.speed >= 1000 ? x.speed / 1000 + " Gb/s" : x.speed + " Mb/s"}`, null, app.href("map", x.cc))) : none("At no exchange in PeeringDB.")),
        sec("Buildings", "bb_netfac JOIN bb_fac", (n.sites || []).length ? n.sites.slice(0, 16).map((f) => row(SEV.none, f.name, `${f.city}, ${app.country(f.cc)}`, null, app.href("map", f.cc))) : none("In no building in PeeringDB.")),
        sec("ROAs", "bb_roa WHERE asn = asn", (n.roas || []).length ? h("div", { class: "bn-mono-list" }, n.roas.slice(0, 40).map((r) => h("a", { href: app.href("prefix", r.prefix) }, `${r.prefix}${r.maxlen ? ` ≤/${r.maxlen}` : ""}`))) : none("No ROAs name it.")),
        t.website || t.pdb ? sec("Record", "bb_as", h("div", { class: "small" }, t.website ? h("a", { href: t.website, target: "_blank", rel: "noopener" }, t.website) : null, t.pdb ? [" · ", h("a", { href: `https://www.peeringdb.com/net/${t.pdb}`, target: "_blank", rel: "noopener" }, "PeeringDB")] : null)) : null]],
    ]);
    const why = open.length ? `${plural(open.length, "open alert")}: ${open.slice(0, 2).map((a) => a.title).join("; ")}` : n.med != null && n.med > 150 ? `Its ${plural(n.probes_now, "probe")} reach the K root server in a median ${ms(n.med)}.` : null;
    const home = cc || t.country;
    fill(right,
      ihead({ crumbs: [...(home ? crumbsFor(home) : [{ t: "World", href: app.href("map") }]), { t: t.name || `AS${t.asn}` }], title: `${t.name || t.full || "AS" + t.asn} (AS${t.asn})`,
        tag: n.state !== "ok" ? pill(n.state, n.state === "bad" ? "red" : "watch") : pill("ok", "ok"),
        sub: [t.kind, t.traffic, t.full && t.full !== t.name ? `registered as ${t.full}` : null, t.scope],
        why, facts: [["providers", fmt(t.providers)], ["customers", fmt(t.customers)], ["peers", fmt(t.peers)], ["exchanges", fmt(t.ixes)], ["buildings", fmt(t.facs)], ["ROAs", fmt(t.roas)], n.med != null ? ["to K", ms(n.med)] : null],
        actions: [watchBtn(t.asn), open[0]?.incident ? h("a", { class: "btn xs primary", href: app.href("incident", open[0].incident) }, "Open its incident") : null] }),
      bar, bodyEl);
  }

  function inspectPrefix(x) {
    const held = x.holding || [];
    const pa = x.paths || {};
    const status = x.state === "bad" ? "red" : x.state === "warn" ? "watch" : x.state === "ended" ? "ended" : "ok";
    const headline = held.length
      ? `${plural(x.held_now, "peer")} of rrc00's ${x.peers_heard} hold a route to it now, from ${held.map((o) => netName(o.origin)).join(" and ")}.`
      : x.updates ? "No peer holds a route to it now: the last word of each was a withdrawal." : "rrc00 heard nothing of it in the last three days.";
    const open = (x.alerts || []).filter((a) => a.state === "open");
    const pathRow = (p) => h("div", { class: `bn-path ${p.mark}` },
      h("div", { class: "bn-path-hops" }, p.path.map((n, i) => [i ? h("span", { class: "sep" }, "›") : null, h("a", { href: app.href("as", n.asn), title: `${n.name || ""} (AS${n.asn})${n.country ? ", " + n.country : ""}` }, n.name || `AS${n.asn}`)])),
      h("div", { class: "bn-path-n" }, p.mark === "new" ? h("span", { class: "badge info" }, "new") : p.mark === "gone" ? h("span", { class: "badge" }, "gone") : null, plural(p.peers, "peer")));
    const changed = (pa.now || []).filter((p) => p.mark === "new").length + (pa.gone || []).length;
    const pts = x.minutes || [];
    const [bar, bodyEl] = tabbed("prefix", [
      ["What's wrong", open.length, () => [
        sec("Open alerts", "bb_alerts WHERE prefix = the prefix", open.length ? open.map((a) => row(SEV[a.severity], a.title, `${kindWord(a.kind)} · ${a.detail || ""} · since ${stamp(a.first)}`, null, a.incident ? app.href("incident", a.incident) : null)) : none("None open.")),
        sec("Who holds a route to it now", "each peer's last word, replayed", held.length ? held.map((o) => {
          const og = (x.origins || []).find((g) => g.origin.asn === o.origin.asn);
          return row(/invalid/.test(og?.rpki || "") ? SEV.bad : SEV.ok, netName(o.origin), `${plural(o.peers, "peer")} · RPKI ${og?.rpki || "?"} · ${og?.seen || ""}`, null, app.href("as", o.origin.asn));
        }) : none(headline)),
        held.length > 1 ? h("div", { class: "bn-map-verdict" }, h("b", {}, "Two or more origins now."), h("div", { class: "small" }, "Different networks announce it at once: a hijack, a move, or anycast.")) : null], open.length ? "bad" : null],
      ["What changed", changed, () => [
        sec("Paths now", pa.known ? `each peer's path, against an hour ago; ${plural(pa.now_total || 0, "path")} now, ${plural(pa.then_total || 0, "path")} then` : "each peer's path (the updates kept do not reach back an hour)",
          (pa.now || []).length ? pa.now.map(pathRow) : none("No peer holds a route.")),
        (pa.gone || []).length ? sec("Paths gone since an hour ago", "held then, not now", pa.gone.map(pathRow)) : null,
        sec("Origins and faults", "who began and stopped announcing it, and the alerts", (x.events || []).length ? h("div", { class: "bn-map-hist" }, x.events.slice(0, 24).map((e) => h("div", { class: e.kind }, h("span", { class: "when" }, stamp(e.at)), h("span", {}, e.text, e.incident ? [" · ", h("a", { href: app.href("incident", e.incident) }, "incident")] : null)))) : none("Nothing changed."))], changed ? "info" : null],
      ["Metrics", null, () => [
        sec("Its updates a minute, and how many peers held it", "every update of the prefix, replayed in order", h("div", { class: "bn-chart-box" }, minutes(pts, [{ key: "a", color: "var(--s1)", label: "announcements" }, { key: "w", color: "var(--s2)", label: "withdrawals" }], { height: 150, line: "held", lineLabel: "peers holding it" })),
          legend([["var(--s1)", "announcements"], ["var(--s2)", "withdrawals"], ["var(--s7)", "peers holding a route"]])),
        h("div", { class: "bn-facts", style: { padding: "0 14px" } }, h("span", {}, "updates kept ", h("b", {}, `${fmt(x.updates)}${x.capped ? "+" : ""}`)), h("span", {}, "peers heard ", h("b", {}, fmt(x.peers_heard))), h("span", {}, "holding now ", h("b", {}, fmt(x.held_now))))]],
      ["Records", null, () => [
        sec("Who announced it", "each origin: peers, RPKI, against yesterday", (x.origins || []).length ? x.origins.map((o) => row(/invalid/.test(o.rpki) ? SEV.bad : o.rpki === "valid" ? SEV.ok : SEV.none, netName(o.origin), `${plural(o.peers, "peer")} · ${o.rpki} · ${o.seen} · ${stamp(o.first)} to ${stamp(o.last)}`, null, app.href("as", o.origin.asn))) : none("No announcements.")),
        sec("Carried by", "the network before the origin", (x.upstreams || []).length ? x.upstreams.map((u) => row(SEV.info, netName(u.net), plural(u.routes, "route"), null, app.href("as", u.net.asn))) : none("None.")),
        sec("ROAs that cover it", "bb_roa WHERE prefix IN (the prefix at each shorter length)", (x.roas || []).length ? x.roas.map((r) => row(SEV.ok, r.prefix, `AS${r.asn} ${r.name || ""} · up to /${r.maxlen} · ${r.ta} · expires ${day(r.expires)}`, null, app.href("as", r.asn))) : none("No ROA covers it: its RPKI state is unknown.")),
        sec("Covering it", "last day", (x.covering || []).length ? x.covering.map((r) => row(SEV.none, r.prefix, `${netName(r.origin)} · ${r.peers} peers`, null, app.href("prefix", r.prefix))) : none("None heard.")),
        sec("Inside it", "last day", (x.inside || []).length ? x.inside.map((r) => row(SEV.none, r.prefix, `${netName(r.origin)} · ${r.peers} peers`, null, app.href("prefix", r.prefix))) : none("None heard."))]],
    ]);
    fill(right,
      ihead({ crumbs: [{ t: "World", href: app.href("map") }, { t: "Prefix" }], title: x.prefix, mono: true, tag: pill(x.state === "ended" ? "info" : x.state, status),
        sub: [x.owner ? `owner ${netName(x.owner)}` : null, `as of ${clock(x.as_of)}`], why: headline, whyOk: x.state === "ok" || x.state === "ended" }),
      bar, bodyEl);
  }

  function inspectIncident(j) {
    if (!j.found) return fill(right, ihead({ crumbs: [{ t: "World", href: app.href("map") }], title: "No such incident" }), none("It may have ended more than a day ago."));
    const i = j.incident;
    const al = j.alerts || [];
    const pts = (j.minutes || []).map((m) => ({ t: m.t, a: m.a, w: m.w, peers: m.peers }));
    const [bar, bodyEl] = tabbed("incident", [
      ["What's wrong", al.filter((a) => a.state === "open").length, () => [
        h("div", { class: "bn-in-sum" }, h("p", {}, i.summary)),
        sec("Its alerts", "each a row a rule found", al.map((a) => row(a.state === "open" ? SEV[a.severity] : SEV.ended, a.title, `${kindWord(a.kind)} · ${a.detail || ""}${a.peers ? ` · ${a.peers} peers` : ""}`, null,
          a.prefix ? app.href("prefix", a.prefix) : a.asn ? app.href("as", a.asn.asn) : a.country ? app.href("map", a.country) : null)))], i.state === "open" ? (i.severity === "bad" ? "bad" : "warn") : null],
      ["What changed", null, () => [
        sec("How it went", "when each alert began", h("div", { class: "bn-map-hist" },
          h("div", { class: "change" }, h("span", { class: "when" }, stamp(i.first)), h("span", {}, "First seen")),
          al.slice().sort((a, b) => (a.first || "").localeCompare(b.first || "")).slice(0, 20).map((a) => h("div", { class: "fault" }, h("span", { class: "when" }, stamp(a.first)), h("span", {}, a.title))),
          h("div", { class: "change" }, h("span", { class: "when" }, i.state === "open" ? "now" : stamp(i.last)), h("span", {}, i.state === "open" ? "Still open" : "Ended"))))]],
      ["Metrics", null, () => [
        (j.minutes || []).length ? sec("Its prefixes' updates a minute", "from half an hour before to half an hour after", h("div", { class: "bn-chart-box" }, minutes(pts, [{ key: "a", color: "var(--s1)", label: "announcements" }, { key: "w", color: "var(--s2)", label: "withdrawals" }], { height: 150, line: "peers", lineLabel: "peers that sent one" })),
          legend([["var(--s1)", "announcements"], ["var(--s2)", "withdrawals"], ["var(--s7)", "peers"]])) : none("No routes in it to count.")]],
      ["Records", null, () => [
        (j.graph?.nodes || []).length > 1 ? sec("How it connects", "announcers, the networks that carried their routes, the owners", h("div", { class: "bn-graph-box" }, graphSvg(app, j.graph))) : null,
        (j.upstreams || []).length ? sec("Carried by", "the network before the origin on the routes heard", j.upstreams.map((u) => row(SEV.info, netName(u.net), plural(u.prefixes, "prefix", "prefixes"), null, app.href("as", u.net.asn)))) : null,
        (i.asns || []).length ? sec("Networks in it", null, i.asns.slice(0, 16).map((a) => row(SEV[i.severity] || SEV.info, `AS${a}`, null, null, app.href("as", a)))) : null,
        sec("Shared", "what groups its alerts", h("div", { class: "mono small" }, i.shared))]],
    ]);
    fill(right,
      ihead({ crumbs: [{ t: "World", href: app.href("map") }, { t: "Incident" }], title: i.title, tag: pill(i.state === "open" ? i.severity : "info", i.state === "open" ? `${kindWord(i.kind)} · open` : `${kindWord(i.kind)} · ended`),
        sub: [`${stamp(i.first)} to ${i.state === "open" ? "now" : stamp(i.last)}, ${span(i.first, i.last)}`, plural(i.alerts, "alert"), (i.countries || []).map((c) => app.country(c)).join(", ")] }),
      bar, bodyEl);
  }

  // ---- moving between things
  async function show(page, rest) {
    const me = ++seq;
    const alive = () => me === seq;
    sel = { page, rest };
    drawList();
    right.classList.add("bn-busy");
    try {
      if (!map.world && app.world) map.paintWorld(app.world);
      if (page === "map" && !rest[0]) {
        map.world0(); tl.mark(null);
        if (!app.now) { fill(right, skel(10)); await new Promise((r) => setTimeout(r, 400)); if (!alive()) return; }
        return inspectWorld();
      }
      if (page === "map" || page === "as") {
        const cc = page === "map" ? rest[0].toUpperCase() : null;
        const asn = page === "map" ? rest[1] : rest[0];
        if (cc) map.country(cc, crumbsFor(cc));
        if (cc && !asn) {
          const c = await app.api(`/v1/country/${cc}`);
          if (!alive()) return;
          map.countryData(cc, c);
          tl.mark(null);
          return inspectCountry(c);
        }
        const [n, c] = await Promise.all([app.api(`/v1/as/${encodeURIComponent(asn)}`), cc ? app.api(`/v1/country/${cc}`) : null]);
        if (!alive()) return;
        if (c) map.countryData(cc, c, { fly: false });
        if (!n.found) return inspectNetwork(n);
        const home = cc || n.net.country;
        map.network(n, { cc: c ? cc : null, crumbs: [...(home ? crumbsFor(home) : [{ t: "World", href: app.href("map") }]), { t: n.net.name || `AS${n.asn}` }] });
        tl.mark({ label: `${n.net.name || "AS" + n.asn}'s prefixes`, pts: (n.minutes || []).map((m) => [m.t, m.n]) });
        inspectNetwork(n, { cc, country: c });
        // A network opened on its own: its country under it, once it comes.
        if (!c && n.net.country && n.net.country.length === 2) {
          app.api(`/v1/country/${n.net.country}`, { sql: false }).then((c2) => { if (alive()) map.underlay(n.net.country, c2, n.asn); }).catch(() => {});
        }
        return;
      }
      if (page === "prefix") {
        const p = rest.join("/");
        const x = await app.api(`/v1/prefix/${p}`);
        if (!alive()) return;
        // The path of the newest route, country to country, and the space
        // taken from its owner.
        const path = (x.path || []).filter((n) => n.country);
        const ccs = [...new Set([...path.map((n) => n.country), ...(x.holding || []).map((o) => o.origin.country), x.owner?.country].filter((c) => c && c.length === 2))].slice(0, 10);
        const arcs = [];
        for (let i = 1; i < path.length; i++) if (path[i].country !== path[i - 1].country) arcs.push({ from: path[i - 1].country, to: path[i].country, cls: "up", tip: [`${path[i - 1].name || "AS" + path[i - 1].asn} › ${path[i].name || "AS" + path[i].asn}`, "a hop of the newest route to it"] });
        for (const o of x.holding || []) if (x.owner && o.origin.asn !== x.owner.asn && o.origin.country && x.owner.country && o.origin.country !== x.owner.country) arcs.push({ from: o.origin.country, to: x.owner.country, cls: "bad", bow: -0.3, tip: [`${netName(o.origin)} announces it`, `owned by ${netName(x.owner)}`] });
        map.places(ccs, arcs, [{ t: "World", href: app.href("map") }, { t: x.prefix }]);
        tl.mark({ label: `${x.prefix}'s updates`, pts: (x.minutes || []).map((m) => [m.t, m.a + m.w]) });
        return inspectPrefix(x);
      }
      if (page === "incident") {
        const id = rest.join("/");
        const j = await app.api(`/v1/incident/${encodeURIComponent(id)}`);
        if (!alive()) return;
        drawList();
        if (!j.found) { tl.mark(null); return inspectIncident(j); }
        const i = j.incident;
        const nodes = Object.fromEntries((j.graph?.nodes || []).map((n) => [n.asn, n]));
        const ccs = [...new Set([...(i.countries || []), ...Object.values(nodes).map((n) => n.country)].filter((c) => c && c.length === 2))].slice(0, 10);
        const arcs = (j.graph?.edges || []).map((e) => ({ from: nodes[e.from]?.country, to: nodes[e.to]?.country, cls: e.kind === "took" ? "bad" : "up", bow: e.kind === "took" ? -0.3 : 0.2,
          tip: [`${nodes[e.from]?.name || "AS" + e.from} › ${nodes[e.to]?.name || "AS" + e.to}`, e.kind === "took" ? "announced space this network owns" : "carried its routes"] })).filter((a) => a.from && a.to);
        map.places(ccs.length ? ccs : [], arcs, [{ t: "World", href: app.href("map") }, { t: i.title.slice(0, 48) }]);
        tl.mark({ label: "its prefixes' updates", pts: (j.minutes || []).map((m) => [m.t, m.a + m.w]), from: i.first, to: i.state === "open" ? null : i.last, incident: i.id });
        return inspectIncident(j);
      }
    } catch (e) {
      if (alive()) fill(right, h("div", { class: "error" }, String(e.message || e)));
    } finally {
      if (alive()) right.classList.remove("bn-busy");
    }
  }

  return {
    el, fit, show,
    refresh() {
      if (app.world) map.paintWorld(app.world);
      drawList();
      tl.refresh();
      if (sel.page === "map" && !sel.rest[0]) inspectWorld();
    },
    key(e) {
      if (e.key === "j" || e.key === "k") {
        const ids = [...left.querySelectorAll(".bn-wl-list .bn-wi")].map((a) => a.dataset.id);
        if (!ids.length) return;
        const cur = sel.page === "incident" ? ids.indexOf(sel.rest.join("/")) : -1;
        const next = e.key === "j" ? Math.min(ids.length - 1, cur + 1) : Math.max(0, cur < 0 ? 0 : cur - 1);
        e.preventDefault();
        app.go("incident", ids[next]);
        left.querySelector(`[data-id="${CSS.escape(ids[next])}"]`)?.scrollIntoView({ block: "nearest" });
      } else if (e.key === "Escape") {
        const { page, rest } = sel;
        if (page === "map" && rest[1]) app.go("map", rest[0]);
        else if (page !== "map" || rest[0]) app.go("map");
      }
    },
  };
}

// A histogram of probes' times to K, in bands a reader can act on.
function rttHist(rtts) {
  const bands = [[0, 20, "ok"], [20, 40, "ok"], [40, 60, "ok"], [60, 100, "warn"], [100, 150, "warn"], [150, 250, "bad"], [250, 1e9, "bad"]];
  const n = bands.map(([a, b]) => rtts.filter((r) => r >= a && r < b).length);
  const max = Math.max(1, ...n);
  return h("div", { class: "bn-hist" }, bands.map(([a, b, cls], i) => h("div", { class: "c", title: `${fmt(n[i])} probes` },
    h("div", { class: "v" }, n[i] ? fmt(n[i]) : ""), h("div", { class: `b ${cls}`, style: { height: `${Math.round((48 * n[i]) / max)}px` } }), h("div", { class: "l" }, b >= 1e9 ? `${a}+` : `${a}–${b}`))),
  h("div", { class: "u" }, "ms"));
}

// ---------------------------------------------------------------- incidents

async function incidentsPage(app, main, alive) {
  wait(main, 8);
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

// ---------------------------------------------------------------- Diagnose

const LAYERS = [["users", "Users", "what RIPE Atlas probes there measure to the nearest K root server"], ["path", "Path", "traceroutes whose time jumps 80 ms or more at one hop: the packets left the region"], ["peering", "Peering", "probes over 100 ms: their networks do not reach a node near them"], ["routing", "Routing", "open routing alerts of networks registered there"], ["outages", "Outages", "IODA's open outages there"], ["server", "K root", "pings the K root server did not answer"]];

async function diagnosePage(app, main, focus, alive) {
  wait(main, 8);
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
  wait(main, 8);
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
  wait(main, 8);
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
  wait(main, 6);
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

// The network operations page in ChloeDB's command center (#/app/netops).
// The command center loads this file through chloedb-server, which forwards
// /v1/apps/netops/... to the app, and runs it with its own helpers:
// render(el, ctx) gets ctx.core (h, card, table, fmt, ...), ctx.charts and
// ctx.api (the app's API). It imports nothing, so the app serves it as one
// file.
//
//   #/app/netops                  overview: what is wrong now, and what it costs
//   #/app/netops/incidents        incidents; /incidents/<id> is the incident room
//   #/app/netops/topology         the map: live, as read from the logs, and the difference
//   #/app/netops/alerts           alerts and the rules (each a SQL query)
//   #/app/netops/changes          the change calendar, collisions and risk
//   #/app/netops/services         services against their objectives
//   #/app/netops/explore          logs, flows and one search over everything
//   #/app/netops/devices          the fleet; a device opens in a drawer
//   #/app/netops/agents           the agents, their runs and what they found
//   #/app/netops/data             the tables, where their rows come from, the simulator
//
// Every answer of the app's API carries the SQL it ran; the "SQL" button
// shows the queries behind the page in view.

export default { render };

const CSS = `
.no-head { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; margin-bottom: 10px; }
.no-head h1 { margin: 0; font-size: 20px; letter-spacing: -.01em; }
.no-head .sub { color: var(--muted); font-size: 12.5px; }
.no-head .tools { margin-left: auto; display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
.no-synth { font-size: 11px; padding: 1px 7px; border-radius: 999px; border: 1px dashed var(--line-2); color: var(--text-2); }
.no-sim { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; color: var(--text-2); }
.no-kpis { display: grid; grid-template-columns: repeat(auto-fit, minmax(128px, 1fr)); gap: 12px; margin-bottom: 14px; }
.no-kpis .card { margin: 0; }
.no-kpis .val { font-size: 22px; white-space: nowrap; }
.no-kpis .lbl { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.no-kpi-sub { font-size: 12px; color: var(--muted); margin-top: 2px; }
.no-cols { display: grid; grid-template-columns: minmax(0, 2fr) minmax(0, 1fr); gap: 14px; align-items: start; }
.no-cols.even { grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); }
.no-cols.wide { grid-template-columns: minmax(0, 5fr) minmax(0, 2fr); }
.no-col { display: flex; flex-direction: column; gap: 14px; min-width: 0; }
.no-hero { border: 1px solid color-mix(in srgb, var(--crit) 45%, var(--line)); background: linear-gradient(90deg, var(--crit-soft), var(--surface) 70%); border-radius: var(--r); padding: 14px 16px; margin-bottom: 14px; display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 14px; align-items: center; }
.no-hero.calm { border-color: color-mix(in srgb, var(--good) 35%, var(--line)); background: linear-gradient(90deg, var(--good-soft), var(--surface) 60%); }
.no-hero h2 { margin: 0 0 4px; font-size: 17px; }
.no-hero p { margin: 0; color: var(--text-2); max-width: 90ch; }
.no-hero .meta { display: flex; gap: 6px 14px; flex-wrap: wrap; margin-top: 8px; font-size: 12.5px; color: var(--text-2); }
.no-map { position: relative; }
.no-map svg { display: block; width: 100%; height: auto; }
.no-map .pod { fill: var(--surface-2); stroke: var(--line-2); }
.no-map .pod.hot { stroke: var(--crit); stroke-width: 1.6; }
.no-map .site { font-size: 12px; font-weight: 650; fill: var(--text); }
.no-map .site-sub { font-size: 10px; fill: var(--muted); }
.no-map .dev { stroke: var(--surface); stroke-width: 1.5; cursor: pointer; }
.no-map .dev-lbl { font-size: 8.5px; fill: var(--muted); pointer-events: none; }
.no-map .lnk { fill: none; stroke-linecap: round; }
.no-map .hit { stroke: transparent; stroke-width: 12; fill: none; cursor: pointer; }
.no-map .dimmed { opacity: .22; }
.no-map .halo { fill: none; stroke: var(--crit); stroke-width: 2; opacity: .9; }
.no-map .halo.pulse { animation: no-pulse 1.6s ease-out infinite; transform-box: fill-box; transform-origin: center; }
.no-map .root { fill: none; stroke: var(--accent); stroke-width: 2.5; stroke-dasharray: 3 3; }
.no-map .pathline { fill: none; stroke: var(--accent); stroke-width: 5; stroke-linecap: round; stroke-linejoin: round; opacity: .55; }
@keyframes no-pulse { 0% { transform: scale(1); opacity: .95; } 100% { transform: scale(2.1); opacity: 0; } }
.no-legend { display: flex; gap: 6px 14px; flex-wrap: wrap; font-size: 11.5px; color: var(--text-2); padding: 8px 2px 0; align-items: center; }
.no-legend i { display: inline-block; width: 18px; height: 4px; border-radius: 2px; vertical-align: middle; margin-right: 5px; }
.no-legend i.d { width: 9px; height: 9px; border-radius: 50%; }
.no-list { list-style: none; margin: 0; padding: 0; }
.no-list > li { padding: 8px 0; border-top: 1px solid var(--line); }
.no-list > li:first-child { border-top: 0; padding-top: 0; }
.no-list > li:last-child { padding-bottom: 0; }
.no-tl { list-style: none; margin: 0; padding: 0 0 0 14px; border-left: 2px solid var(--line); }
.no-tl li { position: relative; padding: 0 0 12px 12px; font-size: 13px; }
.no-tl li:last-child { padding-bottom: 0; }
.no-tl li::before { content: ""; position: absolute; left: -20px; top: 4px; width: 10px; height: 10px; border-radius: 50%; background: var(--surface); border: 2px solid var(--muted); }
.no-tl li.alert::before { border-color: var(--crit); } .no-tl li.agent::before { border-color: var(--s7); background: var(--s7); }
.no-tl li.action::before { border-color: var(--good); background: var(--good); } .no-tl li.status::before { border-color: var(--accent); }
.no-tl .when { font-size: 11.5px; color: var(--muted); }
.no-step { border-top: 1px solid var(--line); padding: 9px 0; }
.no-step:first-child { border-top: 0; }
.no-step .top { display: grid; grid-template-columns: 22px 54px minmax(0, 1fr) auto; gap: 8px; align-items: baseline; cursor: pointer; }
.no-step .n { color: var(--muted); font-variant-numeric: tabular-nums; font-size: 12px; text-align: right; }
.no-step .find { color: var(--text-2); font-size: 12.5px; margin: 3px 0 0 92px; overflow-wrap: anywhere; }
.no-step pre.code { margin: 8px 0 0 92px; white-space: pre-wrap; overflow-wrap: anywhere; font-size: 12px; }
.no-tool { font-size: 10.5px; font-weight: 650; text-transform: uppercase; letter-spacing: .04em; padding: 1px 6px; border-radius: 5px; background: var(--surface-3); color: var(--text-2); text-align: center; }
.no-tool.graph { background: color-mix(in srgb, var(--s7) 18%, var(--surface)); color: var(--s7); }
.no-tool.pack { background: color-mix(in srgb, var(--s3) 18%, var(--surface)); color: var(--s3); }
.no-tool.reason { background: var(--accent-soft); color: var(--accent); }
.no-act { border: 1px solid var(--line); border-radius: var(--r-sm); padding: 10px 12px; background: var(--surface); }
.no-act + .no-act { margin-top: 10px; }
.no-act.done { background: var(--surface-2); }
.no-act pre.code { margin: 8px 0 0; font-size: 12px; white-space: pre-wrap; overflow-wrap: anywhere; }
.no-conf { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 10px; align-items: center; }
.no-conf .meter { height: 10px; }
.no-cause { font-size: 14.5px; line-height: 1.5; }
.no-why { margin: 8px 0 0; padding-left: 18px; color: var(--text-2); font-size: 13px; }
.no-key { margin-top: 10px; padding: 8px 10px; border-radius: var(--r-sm); border: 1px dashed var(--line-2); font-size: 12.5px; }
.no-key.ok { border-color: var(--good); background: var(--good-soft); } .no-key.no { border-color: var(--crit); background: var(--crit-soft); }
.no-budget { display: flex; height: 14px; border-radius: 7px; overflow: hidden; background: var(--surface-3); }
.no-budget i { display: block; height: 100%; }
.no-rcpt { font-size: 12px; }
.no-rcpt .skip td { color: var(--muted); }
.no-log { font-family: var(--mono); font-size: 11.5px; line-height: 1.55; }
.no-log .ln { display: grid; grid-template-columns: 62px 78px minmax(0, 1fr); gap: 8px; padding: 2px 0; border-top: 1px solid var(--line); }
.no-log .ln:first-child { border-top: 0; }
.no-log .ln .m { overflow-wrap: anywhere; white-space: pre-wrap; }
.no-log .s0, .no-log .s1, .no-log .s2, .no-log .s3 { color: var(--crit-ink); } .no-log .s4 { color: color-mix(in srgb, var(--warn) 60%, var(--text)); }
.no-log mark { background: color-mix(in srgb, var(--warn) 45%, transparent); color: inherit; border-radius: 2px; }
.no-days { display: grid; grid-auto-flow: column; grid-auto-columns: minmax(190px, 1fr); gap: 10px; overflow-x: auto; padding-bottom: 6px; }
.no-day { border: 1px solid var(--line); border-radius: var(--r-sm); background: var(--surface); min-height: 120px; }
.no-day.today { border-color: var(--accent); }
.no-day > .dh { padding: 6px 10px; font-size: 12px; font-weight: 600; border-bottom: 1px solid var(--line); display: flex; justify-content: space-between; color: var(--text-2); }
.no-day > .db { padding: 8px; display: flex; flex-direction: column; gap: 6px; }
.no-chg { border-left: 3px solid var(--seq-4); background: var(--surface-2); border-radius: 5px; padding: 6px 8px; font-size: 12px; cursor: pointer; }
.no-chg:hover { background: var(--surface-3); }
.no-chg.high { border-left-color: var(--crit); } .no-chg.medium { border-left-color: var(--warn); } .no-chg.low { border-left-color: var(--good); }
.no-chg.clash { box-shadow: inset 0 0 0 1px var(--crit); }
.no-chg.past { opacity: .72; }
.no-chg .t { font-weight: 600; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
.no-blame { font-family: var(--mono); font-size: 11.5px; line-height: 1.6; overflow: auto; max-height: 520px; border: 1px solid var(--line); border-radius: var(--r-sm); }
.no-blame .bl { display: grid; grid-template-columns: 150px 34px minmax(0, 1fr); gap: 0; }
.no-blame .bl > span { padding: 0 8px; white-space: pre; }
.no-blame .who { color: var(--muted); border-right: 1px solid var(--line); overflow: hidden; text-overflow: ellipsis; }
.no-blame .ln { color: var(--muted); text-align: right; }
.no-blame .bl.new { background: color-mix(in srgb, var(--good) 14%, transparent); }
.no-blame .bl.chg .who { color: var(--accent); }
.no-score { font-size: 34px; font-weight: 680; letter-spacing: -.02em; line-height: 1; }
.no-verdict { font-size: 13px; font-weight: 650; text-transform: uppercase; letter-spacing: .05em; }
.no-agent { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 10px; }
.no-agent h4 { margin: 0 0 4px; font-size: 14px; }
.no-facts { display: flex; gap: 4px 16px; flex-wrap: wrap; font-size: 12px; color: var(--text-2); margin-top: 8px; }
.no-facts b { color: var(--text); font-weight: 600; }
.no-q { border-top: 1px solid var(--line); padding: 10px 0; }
.no-q:first-child { border-top: 0; padding-top: 0; }
.no-q pre.code { margin: 6px 0 0; white-space: pre-wrap; overflow-wrap: anywhere; font-size: 12px; }
.no-setup { max-width: 560px; margin: 60px auto; text-align: center; }
.no-setup .bar { height: 8px; background: var(--surface-3); border-radius: 4px; overflow: hidden; margin: 14px 0 8px; }
.no-setup .bar i { display: block; height: 100%; background: var(--accent); transition: width .6s; }
.no-search { display: grid; grid-template-columns: minmax(0, 1fr) auto auto auto; gap: 8px; margin-bottom: 12px; align-items: center; }
.no-search .input { font-size: 14px; padding: 8px 11px; }
.no-hit { padding: 8px 0; border-top: 1px solid var(--line); }
.no-hit:first-child { border-top: 0; }
.no-hit .x { color: var(--text-2); font-size: 12.5px; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
.no-sec { margin: 14px 0 6px; font-size: 12px; font-weight: 600; color: var(--text-2); text-transform: uppercase; letter-spacing: .04em; }
.no-sec:first-child { margin-top: 0; }
.no-mini { display: grid; grid-template-columns: repeat(auto-fit, minmax(260px, 1fr)); gap: 14px; }
.no-replay { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; gap: 10px; align-items: center; margin-bottom: 8px; }
.no-replay input[type=range] { width: 100%; }
.no-link { color: var(--accent); cursor: pointer; text-decoration: none; }
.no-link:hover { text-decoration: underline; }
.no-ro { font-family: var(--mono); font-size: 12px; }
.no-board { align-items: start; display: grid; grid-template-columns: repeat(auto-fit, minmax(330px, 1fr)); gap: 12px; }
.no-panel { border: 1px solid var(--line); border-radius: var(--r-sm); background: var(--surface); min-width: 0; padding: 8px 10px 10px; }
.no-panel.mine { border-color: color-mix(in srgb, var(--accent) 45%, var(--line)); }
.no-panel > header { display: flex; align-items: center; gap: 8px; margin-bottom: 6px; font-size: 12.5px; min-width: 0; }
.no-panel > header .chip { font-size: 10.5px; white-space: nowrap; }
.no-ptable { max-height: 250px; overflow: auto; margin: 0 -10px -10px; font-size: 12px; }
.no-stack { display: grid; gap: 4px; align-items: center; }
.no-stack .ch { font-size: 11px; font-weight: 600; color: var(--text-2); text-align: center; }
.no-stack .rh { font-size: 12.5px; font-weight: 600; white-space: nowrap; }
.no-stack .cell { height: 30px; border-radius: 5px; border: 1px solid transparent; font: inherit; font-size: 12px; font-weight: 650; color: #fff; cursor: pointer; padding: 0; }
.no-stack .cell.ok { background: color-mix(in srgb, var(--good) 22%, var(--surface)); cursor: default; }
.no-stack .cell.none { background: var(--surface-2); cursor: default; }
.no-stack .cell.warn { background: var(--warn); color: #111; }
.no-stack .cell.bad { background: var(--crit); }
.no-stack .cell.start { outline: 2px solid var(--text); outline-offset: 2px; }
.no-stack .cell.on { box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 60%, transparent); }
.no-lenses { display: grid; grid-template-columns: repeat(auto-fit, minmax(340px, 1fr)); gap: 12px; align-items: start; }
.no-lens { border: 1px solid var(--line); border-radius: var(--r-sm); padding: 10px 12px 12px; min-width: 0; background: var(--surface); }
.no-lens > header { display: flex; justify-content: space-between; gap: 8px; align-items: center; }
.no-lens .f { font-size: 10.5px; font-weight: 700; letter-spacing: .06em; text-transform: uppercase; color: var(--accent); }
.no-lens .c { font-family: var(--mono); font-size: 11px; color: var(--text-2); white-space: nowrap; }
.no-lens h4 { margin: 4px 0 2px; font-size: 14px; text-transform: none; letter-spacing: 0; color: var(--text); font-weight: 650; }
.no-lens .how { font-size: 12px; color: var(--muted); margin-bottom: 10px; }
.no-sus a { gap: 8px; padding: 2px 0; cursor: pointer; align-items: center; }
.no-sus .bar { flex: 1; height: 8px; background: var(--surface-3); border-radius: 4px; overflow: hidden; }
.no-sus .bar i { display: block; height: 100%; border-radius: 4px; }
.no-sparks { display: grid; grid-template-columns: repeat(auto-fit, minmax(150px, 1fr)); gap: 12px; }
.no-sparks svg { width: 100%; }
.no-one { display: grid; grid-template-columns: 20px minmax(0, 1fr); gap: 10px; align-items: start; margin-top: 12px; padding: 10px 12px; border-radius: var(--r-sm); background: color-mix(in srgb, var(--accent) 8%, var(--surface)); font-size: 12.5px; color: var(--text-2); }
.no-trace { display: flex; flex-direction: column; }
.no-trace .hop { display: grid; grid-template-columns: 12px auto minmax(0, 1fr); gap: 8px; align-items: center; cursor: pointer; padding: 3px 0; }
.no-trace .hop i { width: 10px; height: 10px; border-radius: 50%; background: var(--good); }
.no-trace .hop.warn i { background: var(--warn); } .no-trace .hop.bad i { background: var(--crit); }
.no-trace .lk { margin-left: 4px; border-left: 2px solid var(--good); padding: 5px 0 5px 14px; display: flex; flex-direction: column; cursor: pointer; }
.no-trace .lk.warn { border-left-color: var(--warn); } .no-trace .lk.bad { border-left-color: var(--crit); border-left-style: dashed; }
.no-trace .lk.bad .muted, .no-trace .hop.bad .muted { color: var(--crit-ink); }
.no-find { width: 300px; max-width: 38vw; font-size: 12.5px; padding: 5px 9px; }
.no-work { display: grid; grid-template-columns: 78px minmax(0, 1fr) auto; gap: 12px; align-items: center; padding: 10px 14px; border-top: 1px solid var(--line); border-left: 3px solid transparent; }
.no-work:first-child { border-top: 0; }
.no-work:hover { background: var(--surface-2); }
.no-work.u1 { border-left-color: var(--crit); } .no-work.u2 { border-left-color: var(--warn); } .no-work.u3 { border-left-color: var(--line-2); }
.no-work .k { font-size: 11px; font-weight: 650; text-transform: uppercase; letter-spacing: .05em; color: var(--text-2); }
.no-work.u1 .k { color: var(--crit-ink); }
.no-work .t { font-weight: 600; overflow-wrap: anywhere; }
.no-work .s { font-size: 12.5px; color: var(--text-2); margin-top: 1px; }
.no-check { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
.no-check li { display: grid; grid-template-columns: 18px minmax(0, 1fr); gap: 8px; align-items: start; font-size: 13px; }
.no-check li svg { color: var(--good); margin-top: 2px; }
.no-path { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; justify-content: flex-start; }
.no-path .sep { font-size: 12px; color: var(--muted); }
.no-reply { margin: 0; padding: 10px 12px; background: var(--surface-2); border: 1px solid var(--line); border-radius: var(--r-sm); white-space: pre-wrap; overflow-wrap: anywhere; font: inherit; font-size: 13px; line-height: 1.5; max-height: 340px; overflow: auto; }
.no-conflict { display: grid; grid-template-columns: 18px minmax(0, 1fr); gap: 8px; font-size: 12.5px; padding: 6px 8px; border-radius: 5px; margin-top: 6px; background: var(--surface-2); }
.no-conflict.critical { background: var(--crit-soft); color: var(--crit-ink); }
.no-conflict.major { background: color-mix(in srgb, var(--warn) 16%, transparent); }
.no-act.crit { border-color: color-mix(in srgb, var(--crit) 55%, var(--line)); }
.no-act.warnb { border-color: color-mix(in srgb, var(--warn) 55%, var(--line)); }
.no-meter { display: inline-block; width: 72px; height: 6px; border-radius: 3px; background: var(--surface-3); overflow: hidden; vertical-align: middle; }
.no-meter i { display: block; height: 100%; border-radius: 3px; }
.no-report { font-size: 13.5px; line-height: 1.55; }
.no-report p { margin: 8px 0 0; }
.no-hero.warn { border-color: color-mix(in srgb, var(--warn) 45%, var(--line)); background: linear-gradient(90deg, color-mix(in srgb, var(--warn) 16%, var(--surface)), var(--surface) 70%); }
.no-hero.info { border-color: color-mix(in srgb, var(--accent) 40%, var(--line)); background: linear-gradient(90deg, color-mix(in srgb, var(--accent) 12%, var(--surface)), var(--surface) 70%); }
@media (max-width: 1100px) { .no-cols, .no-cols.even, .no-cols.wide { grid-template-columns: minmax(0, 1fr); } }
@media (max-width: 900px) {
  .no-hero { grid-template-columns: minmax(0, 1fr); }
  .no-search { grid-template-columns: minmax(0, 1fr); }
  .no-step .top { grid-template-columns: 18px 50px minmax(0, 1fr); } .no-step .top > :nth-child(4) { display: none; }
  .no-step .find, .no-step pre.code { margin-left: 0; }
  .no-blame .bl { grid-template-columns: 90px 30px minmax(0, 1fr); }
}`;

const T = (x) => (x ? Date.parse(x) : null);
const SEV = { critical: "bad", major: "serious", minor: "warn", sev1: "bad", sev2: "serious", sev3: "warn" };
const STATUS = { investigating: ["warn", "Investigating"], identified: ["info", "Cause identified"], mitigated: ["ok", "Mitigated"], resolved: ["good", "Resolved"] };
const KIND = { fiber_cut: "Fibre cut", bad_change: "Bad change", optic_degrade: "Failing optic", ddos: "DDoS", cpu_bug: "Software defect", power: "Power", "": "Unknown", capacity: "Capacity" };
const PAGES = [["", "My shift"], ["diagnose", "Diagnose"], ["network", "Network"], ["incidents", "Incidents"], ["alerts", "Alerts"], ["changes", "Changes"], ["customers", "Customers"], ["carriers", "Carriers"], ["topology", "Topology"], ["services", "Services"], ["explore", "Explore"], ["devices", "Devices"], ["agents", "Agents"], ["data", "Data"]];
const gbps = (bps) => (bps == null ? "–" : bps >= 1e12 ? (bps / 1e12).toFixed(2) + " Tb/s" : bps >= 1e9 ? (bps / 1e9).toFixed(1) + " Gb/s" : (bps / 1e6).toFixed(0) + " Mb/s");
const pct = (v, d = 0) => (v == null || Number.isNaN(v) ? "–" : (v * 100).toFixed(d) + "%");
const mins = (m) => (m == null ? "–" : m >= 90 ? (m / 60).toFixed(1) + " h" : Math.round(m) + " min");
const cap = (mbps) => (mbps >= 1000 ? mbps / 1000 + "G" : mbps + "M");

function render(el, ctx) {
  const C = ctx.core, { h } = C;
  el.append(h("style", {}, CSS));
  const app = { ctx, C, h, s: C.s, charts: ctx.charts, queries: new Map(), status: null };
  const sqlN = h("span", {}, "SQL");
  const sqlBtn = h("button", { class: "btn sm", title: "The queries behind this page", onclick: () => showSql(app) }, C.icon("sql"), sqlN);
  const simEl = h("span", { class: "no-sim" });
  const injectBtn = h("button", { class: "btn sm", onclick: () => injectMenu(app, injectBtn) }, C.icon("bolt"), "Inject a fault");
  const sub = h("span", { class: "sub" });
  const find = h("input", { class: "input no-find", type: "search", placeholder: "Paste anything: an address, a circuit id, a customer, a device, an interface", "aria-label": "Look anything up", onkeydown: (e) => { if (e.key === "Enter" && find.value.trim()) { lookupDrawer(app, find.value.trim()); find.select(); } } });
  const alt = h("span");
  const drawAlt = (page) => alt.replaceChildren(C.seg([["eng", "Engineer", "The shift: what waits, what is planned, what is broken"], ["lead", "Leadership", "The same tables from further away: reliability, risk, carriers, the team"]], page === "leadership" ? "lead" : "eng", (v) => app.go(...(v === "lead" ? ["leadership"] : []))));
  const tabs = h("div", { class: "tabs", role: "tablist" });
  const body = h("div");
  const synth = h("span", { class: "no-synth", title: "Everything on these pages is made up: a simulated carrier network, written into ChloeDB tables every minute" }, "synthetic data");
  el.append(h("div", { class: "no-head" }, h("h1", {}, "Network operations"), sub, synth,
    h("div", { class: "tools" }, find, alt, simEl, injectBtn, sqlBtn)), tabs, body);

  /** The app's API, keeping the queries each answer says it ran. */
  app.call = async (path, opts) => {
    const j = await ctx.api(path, opts);
    if (j && Array.isArray(j.queries)) {
      app.queries.set(path.split("?")[0], j.queries);
      let n = 0, ms = 0;
      for (const qs of app.queries.values()) for (const q of qs) { n++; ms += q.ms || 0; }
      sqlN.textContent = `SQL · ${n} ${n === 1 ? "query" : "queries"} · ${C.fmt.ms(ms)}`;
    }
    return j;
  };
  app.go = (...parts) => { location.hash = ctx.href(...parts); };
  app.link = (text, ...parts) => h("a", { class: "no-link", href: ctx.href(...parts) }, text);
  app.refresh = () => {};

  let stop = null, current = null, ready = false;
  const show = (path) => {
    try { stop?.(); } catch (e) { console.warn(e); }
    stop = null;
    current = path;
    app.queries.clear();
    sqlN.textContent = "SQL";
    body.replaceChildren();
    const main = el.closest("main");
    if (main) main.scrollTop = 0;
    const page = path[0] || "";
    for (const b of tabs.children) b.setAttribute("aria-selected", String(b.dataset.p === page));
    tabs.style.display = page === "leadership" ? "none" : "";
    drawAlt(page);
    if (!ready) return;
    const f = { "": shiftPage, diagnose: diagnosePage, network: overviewPage, customers: customersPage, carriers: carriersPage, leadership: leadershipPage, incidents: path[1] ? incidentRoom : incidentsPage, topology: topologyPage, alerts: alertsPage, changes: changesPage, services: servicesPage, explore: explorePage, devices: devicesPage, agents: agentsPage, data: dataPage }[page] || shiftPage;
    stop = f(body, app, path);
  };
  const drawTabs = (st) => {
    const badge = { diagnose: st?.open_incidents ? "!" : 0, incidents: st?.open_incidents || 0, alerts: st?.firing || 0, "": st?.waiting || 0, carriers: st?.notices || 0 };
    const page = (current || [])[0] || "";
    tabs.replaceChildren(...PAGES.map(([p, label]) => h("button", { role: "tab", dataset: { p }, "aria-selected": String(p === page), onclick: () => app.go(...(p ? [p] : [])) }, label, badge[p] ? h("span", { class: "n", style: p === "incidents" ? { background: "var(--crit-soft)", color: "var(--crit-ink)" } : null }, String(badge[p])) : null)));
  };
  drawTabs(null);
  const setup = h("div", { class: "no-setup" });
  const poll = async () => {
    let st;
    try { st = await ctx.api("/v1/status"); } catch (e) { if (!ready) body.replaceChildren(C.failure(e, "Network operations")); return; }
    app.status = st;
    const n = st.network;
    sub.textContent = `${n.name}: ${n.sites} sites, ${n.devices} devices, ${n.links} links, ${n.services} services`;
    drawTabs(st);
    const sim = st.sim;
    injectBtn.style.display = sim.simulate ? "" : "none";
    synth.style.display = sim.simulate ? "" : "none";
    simEl.replaceChildren(...(!sim.simulate ? [h("span", { class: "dot ok" }), "Live feeds"] : sim.fault_active
      ? [h("span", { class: "dot firing pulse" }), `A fault is playing out (${sim.fault_phase})`, h("button", { class: "btn xs", title: "Repair it now", onclick: async () => { try { await ctx.api("/v1/sim/end", { body: {} }); C.toast("The fault was repaired; its alerts clear within a few minutes", "ok"); poll(); } catch (e) { C.toast(e.message, "bad"); } } }, "End it")]
      : [h("span", { class: "dot ok" }), sim.next_fault_at ? `Calm. Next fault ${C.fmt.ago(T(sim.next_fault_at))}` : "Calm. No faults scheduled"]));
    if (st.error && ready) simEl.append(h("span", { class: "badge ac-bad", title: st.error }, "write error"));
    if (!st.ready) {
      setup.replaceChildren(C.icon("db", "lg"), h("h2", {}, "Building the network"), h("div", { class: "dim" }, st.phase + "…"), h("div", { class: "bar" }, h("i", { style: { width: Math.round(st.progress * 100) + "%" } })),
        h("div", { class: "small muted" }, st.error || "Creating 35 tables, a month of history and the last hours of telemetry. This takes under a minute."));
      if (!setup.isConnected) body.replaceChildren(setup);
      return;
    }
    if (!ready) { ready = true; show(ctx.path()); }
  };
  app.poll = poll;
  const stopPoll = C.every(5000, poll, true, { always: true });
  current = ctx.path();
  ctx.onPath(show);
  return () => { stopPoll(); try { stop?.(); } catch (e) { console.warn(e); } };
}

// ---------------------------------------------------------------- shared pieces

function showSql(app) {
  const { C, h } = app;
  const groups = [...app.queries.entries()];
  const body = groups.length ? groups.map(([path, qs]) => h("div", {}, h("div", { class: "no-sec" }, `${path} · ${qs.length} ${qs.length === 1 ? "query" : "queries"}`), qs.map((q) => queryBlock(app, q))))
    : C.empty("sql", "No queries yet", "Open a page: each answer of the app's API lists the SQL it ran.");
  C.drawer({ title: "The SQL behind this page", sub: "Each number here is a query against ChloeDB tables. Nothing is cached in the app.", wide: true, body });
}

function queryBlock(app, q) {
  const { C, h } = app;
  const sql = q.sql + (q.params ? `\n-- ${JSON.stringify(q.params)}` : "");
  return h("div", { class: "no-q" },
    h("div", { class: "row nw between" }, h("b", {}, q.what), h("span", { class: "small muted", style: { whiteSpace: "nowrap" } }, `${C.fmt.n(q.rows)} rows · ${C.fmt.ms(q.ms)}`, " ", h("button", { class: "btn xs ghost", title: "Copy", onclick: () => C.copy(q.sql, "SQL copied") }, C.icon("copy")))),
    q.note ? h("div", { class: "small", style: { color: "var(--crit-ink)" } }, q.note) : null, C.sqlBlock(sql));
}

function injectMenu(app, anchor) {
  const { C } = app;
  const sc = app.status?.sim?.scenarios || [];
  C.menu(anchor, sc.map((x) => ({ label: x.name, icon: "bolt", onclick: () => inject(app, x) })));
}

async function inject(app, sc) {
  const { C } = app;
  try {
    await app.ctx.api("/v1/sim/inject", { body: { scenario: sc.id }, timeout: 60000 });
    C.toast(`${sc.name}: injected. The rules and the agents are already on it.`, "ok", 6000);
    await app.poll();
    app.refresh();
  } catch (e) { C.toast(e.message, "bad", 6000); }
}

const sevPill = (app, sev) => app.C.pill(SEV[sev] || "info", sev);
const statusPill = (app, st) => app.C.pill((STATUS[st] || ["info"])[0], (STATUS[st] || [0, st])[1]);
const kpi = (app, label, value, subText, attrs = {}) => app.h("section", { class: "card kpi", ...attrs }, app.h("div", { class: "lbl" }, label), app.h("div", { class: "val" }, value), subText ? app.h("div", { class: "no-kpi-sub" }, subText) : null);

function timeline(app, rows) {
  const { h, C } = app;
  if (!rows.length) return C.empty("clock", "Nothing yet");
  return h("ol", { class: "no-tl" }, rows.map((r) => h("li", { class: r.kind }, h("div", { class: "when" }, C.timeEl(T(r.ts)), " · ", r.actor), h("div", {}, r.note))));
}

function logLines(app, lines, mark) {
  const { h, C } = app;
  if (!lines.length) return C.empty("logs", "No lines");
  const hl = (m) => {
    if (!mark) return m;
    const i = m.toLowerCase().indexOf(mark.toLowerCase());
    return i < 0 ? m : [m.slice(0, i), h("mark", {}, m.slice(i, i + mark.length)), m.slice(i + mark.length)];
  };
  return h("div", { class: "no-log" }, lines.map((l) => h("div", { class: "ln s" + (l.sev ?? 6) }, h("span", { class: "muted", title: l.ts }, C.when(T(l.ts))), h("span", {}, l.device ? h("a", { class: "no-link", onclick: () => deviceDrawer(app, l.device) }, l.device) : (l.severity || "")), h("span", { class: "m" }, hl(l.message)))));
}

/** An agent's steps: what it asked, what it found, and the SQL on a click. */
function steps(app, list) {
  const { h, C } = app;
  if (!list.length) return C.empty("agent", "No steps kept");
  return h("div", {}, list.map((st) => {
    const code = st.sql_text ? C.sqlBlock(st.sql_text) : null;
    if (code) code.style.display = "none";
    const top = h("div", { class: "top", title: st.sql_text ? "Show the SQL" : null, onclick: () => { if (code) code.style.display = code.style.display === "none" ? "" : "none"; } },
      h("span", { class: "n" }, st.n), h("span", { class: "no-tool " + st.tool }, st.tool === "reason" ? "think" : st.tool), h("b", {}, st.title),
      h("span", { class: "small muted", style: { whiteSpace: "nowrap" } }, st.tool === "reason" ? "" : `${C.fmt.n(st.row_count)} rows · ${C.fmt.ms(st.ms)}`));
    return h("div", { class: "no-step" }, top, st.detail ? h("div", { class: "find" }, st.detail) : null, code);
  }));
}

/** What an agent was shown and what did not fit: PACK's receipt. */
function receipt(app, rows) {
  const { h, C } = app;
  if (!rows.length) return C.empty("layers", "No receipt", "The brief is packed once the agent has a hypothesis.");
  const by = {};
  for (const r of rows) { const g = (by[r.source] ||= { take: 0, skip: 0, tokens: 0, rows: [] }); g.rows.push(r); if (r.decision === "take") { g.take++; g.tokens = Math.max(g.tokens, r.used); } else g.skip++; }
  const colors = ["var(--s1)", "var(--s3)", "var(--s7)", "var(--s4)"];
  const total = Object.values(by).reduce((a, g) => a + g.tokens, 0);
  const budget = 2000;
  return h("div", { class: "stack" },
    h("div", {}, h("div", { class: "row between small" }, h("span", { class: "dim" }, `${C.fmt.n(total)} of ${C.fmt.n(budget)} tokens used across ${Object.keys(by).length} sources`), h("span", { class: "muted" }, "PACK n TOKENS WITH RECEIPT")),
      h("div", { class: "no-budget", style: { marginTop: "6px" } }, Object.values(by).map((g, i) => h("i", { style: { width: (g.tokens / budget * 100) + "%", background: colors[i % 4] } })))),
    h("div", { class: "no-legend", style: { paddingTop: 0 } }, Object.entries(by).map(([k, g], i) => h("span", {}, h("i", { style: { background: colors[i % 4] } }), `${k.replace("netops_", "")}: ${g.take} taken, ${g.skip} left out, ${C.fmt.n(g.tokens)} tokens`))),
    h("div", { class: "no-rcpt" }, C.table([
      { key: "source", label: "From", render: (r) => r.source.replace("netops_", "") },
      { key: "title", label: "Row", render: (r) => h("span", { title: r.excerpt }, r.title || r.row_id) },
      { key: "excerpt", label: "Text", sortable: false, render: (r) => h("span", { class: "ellipsis", style: { display: "block", maxWidth: "420px" }, title: r.excerpt }, r.excerpt) },
      { key: "score", label: "Score", num: true, render: (r) => r.score.toFixed(2) },
      { key: "tokens", label: "Tokens", num: true },
      { key: "decision", label: "Decision", render: (r) => r.decision === "take" ? C.pill("ok", "shown") : h("span", { class: "muted" }, r.decision) },
    ], { rows, compact: true, rowClass: (r) => (r.decision === "take" ? "" : "skip"), maxHeight: "360px" })));
}

// ---------------------------------------------------------------- the map

const POS = { nyc: [130, 250], ash: [175, 470], lon: [445, 235], ams: [655, 150], par: [525, 440], fra: [835, 325], mad: [395, 590], mil: [725, 560], sto: [935, 78], waw: [1080, 262] };
const ROWS = [["core"], ["border", "agg"], ["firewall", "spine"], ["leaf"]];

function layout(devices) {
  const pos = {}, pods = {};
  const by = {};
  for (const d of devices) (by[d.site] ||= []).push(d);
  for (const [site, list] of Object.entries(by)) {
    const rows = ROWS.map((roles) => list.filter((d) => roles.includes(d.role)).sort((a, b) => roles.indexOf(a.role) - roles.indexOf(b.role) || a.id.localeCompare(b.id))).filter((r) => r.length);
    const w = rows.length > 2 ? 176 : 128, hgt = rows.length * 27 + 16;
    const [cx, cy] = POS[site] || [600, 330];
    const x0 = cx - w / 2, y0 = cy - hgt / 2;
    pods[site] = { x: x0, y: y0, w, h: hgt, cx, cy };
    rows.forEach((row, r) => row.forEach((d, i) => { pos[d.id] = [x0 + w * (i + 1) / (row.length + 1), y0 + 21 + r * 27]; }));
  }
  return { pos, pods };
}

function utilColor(u) { return u == null ? "var(--muted)" : u >= 0.92 ? "var(--crit)" : u >= 0.8 ? "var(--serious)" : u >= 0.65 ? "var(--warn)" : u >= 0.4 ? "var(--seq-5)" : "var(--seq-3)"; }

/**
 * The network as a map: a box per site, a dot per device (cores on top, then
 * borders and aggregation, firewalls and spines, leaves), a line per link.
 * mode: "live" (the inventory's links, coloured by load), "logs" (the links
 * read from syslog) or "diff" (where the two disagree).
 */
function topoMap(app, opts = {}) {
  const { h, s, C } = app;
  const svg = s("svg", { viewBox: opts.viewBox || "40 20 1140 640", role: "img", "aria-label": "Network map" });
  const root = h("div", { class: "no-map" }, svg);
  let last = null, state = { mode: opts.mode || "live", focus: null, rootEntity: null, path: null, over: null };
  function draw() {
    const d = last;
    if (!d) return;
    svg.replaceChildren();
    const { pos, pods } = layout(d.devices);
    const sites = Object.fromEntries(d.sites.map((x) => [x.id, x]));
    const metric = Object.fromEntries((d.metrics || []).map((m) => [m.link, m]));
    const alertDev = {}, alertLink = {};
    const rank = { critical: 3, major: 2, minor: 1 };
    for (const a of d.alerts || []) {
      if (a.entity_kind === "link") { if ((rank[a.severity] || 0) > (rank[alertLink[a.entity]] || 0)) alertLink[a.entity] = a.severity; }
      for (const dv of a.devices || []) if (a.entity_kind !== "link" && (rank[a.severity] || 0) > (rank[alertDev[dv]] || 0)) alertDev[dv] = a.severity;
    }
    const ov = state.over || {};
    const focus = state.focus;
    const dim = (ids) => (focus && !ids.some((x) => focus.has(x)) ? " dimmed" : "");
    const hot = new Set();
    // Pods.
    const gp = s("g");
    for (const [site, p] of Object.entries(pods)) {
      const st = sites[site] || {};
      gp.append(s("rect", { class: "pod", x: p.x, y: p.y, width: p.w, height: p.h, rx: 10, dataset: { site } }),
        s("text", { class: "site", x: p.x + 2, y: p.y - 7 }, st.name || site.toUpperCase()),
        s("text", { class: "site-sub", x: p.x + p.w - 2, y: p.y - 7, "text-anchor": "end" }, `${site.toUpperCase()} · ${st.kind === "dc" ? "data centre" : st.kind === "hub" ? "hub" : "PoP"}`));
    }
    // Links.
    const gl = s("g"), gh = s("g");
    const seenBy = Object.fromEntries((d.seen || []).map((x) => [x.id, x]));
    const invIds = new Set(d.links.map((l) => l.id));
    const edge = (a, b, attrs, tip, onclick, ids) => {
      const pa = pos[a], pb = pos[b];
      if (!pa || !pb) return;
      const far = (d.devices.find((x) => x.id === a) || {}).site !== (d.devices.find((x) => x.id === b) || {}).site;
      const mx = (pa[0] + pb[0]) / 2, my = (pa[1] + pb[1]) / 2;
      const dx = pb[0] - pa[0], dy = pb[1] - pa[1], len = Math.hypot(dx, dy) || 1;
      const bend = far ? Math.min(26, len * 0.07) : 0;
      const path = `M${pa[0]},${pa[1]}Q${mx - dy / len * bend},${my + dx / len * bend} ${pb[0]},${pb[1]}`;
      gl.append(s("path", { class: "lnk" + dim(ids), d: path, ...attrs }));
      if (tip) {
        const hit = s("path", { class: "hit", d: path });
        C.tipOn(hit, tip);
        if (onclick) hit.addEventListener("click", onclick);
        gh.append(hit);
      }
    };
    const devSite = Object.fromEntries(d.devices.map((x) => [x.id, x.site]));
    if (state.mode === "live") {
      for (const l of d.links) {
        const m = { ...(metric[l.id] || {}), ...(ov.links?.[l.id] || {}) };
        const has = metric[l.id] || ov.links?.[l.id];
        const down = has ? m.up === 0 : l.status === "down";
        const far = devSite[l.src] !== devSite[l.dst];
        const w = far ? (l.capacity_mbps >= 400000 ? 3.6 : l.capacity_mbps >= 200000 ? 2.8 : 2.1) : 1.1;
        const color = down ? "var(--crit)" : !has ? "var(--muted)" : (m.loss > 0.5 || alertLink[l.id] === "major") && m.util < 0.8 ? "var(--serious)" : utilColor(m.util);
        if (down || alertLink[l.id]) { hot.add(devSite[l.src]); hot.add(devSite[l.dst]); }
        edge(l.src, l.dst, { style: { stroke: color, strokeWidth: w, strokeDasharray: down ? "5 4" : !has ? "1 4" : null, opacity: far || down || (m.util || 0) > 0.65 ? 1 : 0.5 } },
          () => [h("div", { class: "t" }, l.id), h("div", { class: "r" }, h("b", {}, down ? "down" : !has ? "no data" : pct(m.util)), h("span", { class: "n" }, `${cap(l.capacity_mbps)} ${l.kind}${l.plane ? " · plane " + l.plane : ""}`)),
            has && !down ? h("div", { class: "r" }, h("b", {}, (m.loss ?? 0).toFixed(2) + "%"), h("span", { class: "n" }, "loss"), h("b", {}, String(m.errors ?? 0)), h("span", { class: "n" }, "errors")) : null,
            l.circuit ? h("div", { class: "r" }, h("span", { class: "n" }, `${l.circuit} · ${l.provider}`)) : null],
          () => (opts.onLink || ((id) => linkDrawer(app, id)))(l.id), [l.src, l.dst]);
      }
    } else {
      // What the logs say, against what the inventory says.
      for (const l of d.links) {
        const sn = seenBy[l.id];
        const far = devSite[l.src] !== devSite[l.dst];
        if (state.mode === "logs" && !sn) continue;
        const silent = !sn;
        edge(l.src, l.dst, { style: { stroke: silent ? "var(--crit)" : state.mode === "diff" ? "var(--line-2)" : "var(--s3)", strokeWidth: silent ? 2.6 : far ? 2 : 1.1, strokeDasharray: silent ? "2 5" : null, opacity: silent || far ? 1 : 0.55 } },
          () => [h("div", { class: "t" }, l.id), silent ? h("div", { class: "r" }, h("b", {}, "in the inventory, never heard in the logs")) : h("div", { class: "r" }, h("b", {}, `${sn.evidence} lines`), h("span", { class: "n" }, `${(sn.protocols || []).join(" + ")} · confidence ${pct(sn.confidence)}`))],
          () => opts.onPair?.(l.src, l.dst, silent ? "silent" : "confirmed"), [l.src, l.dst]);
      }
      for (const sn of d.seen || []) {
        if (invIds.has(sn.id) || sn.state !== "undocumented") continue;
        edge(sn.src, sn.dst, { style: { stroke: "var(--s7)", strokeWidth: 3, strokeDasharray: "7 4" } },
          () => [h("div", { class: "t" }, `${sn.src} to ${sn.dst}`), h("div", { class: "r" }, h("b", {}, "heard in the logs, not in the inventory")), h("div", { class: "r" }, h("span", { class: "n" }, `${sn.evidence} lines · ${sn.a_if} to ${sn.z_if}`))],
          () => opts.onPair?.(sn.src, sn.dst, "undocumented"), [sn.src, sn.dst]);
      }
    }
    // A path asked for.
    const gx = s("g");
    if (state.path?.length > 1) gx.append(s("polyline", { class: "pathline", points: state.path.map((id) => (pos[id] || [0, 0]).join(",")).join(" ") }));
    // Devices.
    const gd = s("g");
    for (const dv of d.devices) {
      const p = pos[dv.id];
      if (!p) continue;
      const o = ov.devices?.[dv.id];
      const down = o ? o.up === 0 : dv.status === "down";
      const sev = (ov.alerting || alertDev)[dv.id];
      if (down || sev) hot.add(dv.site);
      const fill = down ? "var(--crit)" : sev === "critical" ? "var(--crit)" : sev === "major" ? "var(--serious)" : sev === "minor" ? "var(--warn)" : dv.role === "core" ? "var(--seq-6)" : dv.role === "border" ? "var(--s7)" : dv.role === "agg" ? "var(--s3)" : dv.role === "firewall" ? "var(--s2)" : "var(--muted)";
      const r = dv.role === "core" ? 7.5 : dv.role === "leaf" || dv.role === "spine" ? 5 : 6.2;
      if (focus?.has(dv.id) && (down || sev)) gd.append(s("circle", { class: "halo pulse", cx: p[0], cy: p[1], r: r + 3 }));
      if (state.rootEntity === dv.id) gd.append(s("circle", { class: "root", cx: p[0], cy: p[1], r: r + 6 }));
      const c = s("circle", { class: "dev" + dim([dv.id]), cx: p[0], cy: p[1], r, style: { fill, fillOpacity: down ? 0.35 : 1, stroke: down ? "var(--crit)" : null }, tabindex: 0 });
      C.tipOn(c, () => [h("div", { class: "t" }, dv.id), h("div", { class: "r" }, h("b", {}, down ? "unreachable" : sev ? sev + " alert" : "up"), h("span", { class: "n" }, `${dv.role} · ${dv.vendor} ${dv.model}`)), h("div", { class: "r" }, h("span", { class: "n" }, `${dv.os} ${dv.os_version}`))]);
      c.addEventListener("click", () => (opts.onDevice || ((id) => deviceDrawer(app, id)))(dv.id));
      gd.append(c);
      if (dv.role === "core" && !opts.compact) gd.append(s("text", { class: "dev-lbl" + dim([dv.id]), x: p[0], y: p[1] - 10, "text-anchor": "middle" }, dv.id.split("-")[0]));
    }
    // The root cause, when it is a link: a dashed ring at its middle.
    if (state.rootEntity && state.rootEntity.includes("--")) {
      const [a, b] = state.rootEntity.split("--");
      if (pos[a] && pos[b]) gx.append(s("circle", { class: "root", cx: (pos[a][0] + pos[b][0]) / 2, cy: (pos[a][1] + pos[b][1]) / 2, r: 13 }));
    }
    for (const el of gp.querySelectorAll("rect.pod")) if (hot.has(el.dataset.site)) el.classList.add("hot");
    svg.append(gp, gl, gx, gh, gd);
  }
  root.update = (data) => { last = data; draw(); };
  root.set = (patch) => { Object.assign(state, patch); draw(); };
  root.state = () => state;
  return root;
}

function mapLegend(app, mode) {
  const { h } = app;
  const k = (color, text, dash) => h("span", {}, h("i", { style: dash ? { background: `repeating-linear-gradient(90deg, ${color} 0 5px, transparent 5px 9px)` } : { background: color } }), text);
  const d = (color, text) => h("span", {}, h("i", { class: "d", style: { background: color } }), text);
  if (mode === "live") return h("div", { class: "no-legend" }, k("var(--seq-3)", "under 40%"), k("var(--seq-5)", "40 to 65%"), k("var(--warn)", "65 to 80%"), k("var(--serious)", "80 to 92%, or losing packets"), k("var(--crit)", "over 92%"), k("var(--crit)", "down", true),
    d("var(--seq-6)", "core"), d("var(--s7)", "border"), d("var(--s3)", "aggregation"), d("var(--s2)", "firewall"), d("var(--muted)", "fabric"));
  return h("div", { class: "no-legend" }, k(mode === "diff" ? "var(--line-2)" : "var(--s3)", "in the logs and in the inventory"), k("var(--s7)", "in the logs only: an undocumented cable", true), mode === "diff" ? k("var(--crit)", "in the inventory only: never heard", true) : null);
}

// ---------------------------------------------------------------- page helpers

/** replaceChildren that takes nested lists and skips what is absent. */
function put(el, ...kids) { el.replaceChildren(...kids.flat(Infinity).filter((k) => k != null && k !== false)); }


/** Fetch a path now and every `ms`; draw(answer, first). Returns its stop. */
function live(app, el, path, draw, ms = 15000) {
  let first = true, dead = false;
  const run = async () => {
    try {
      const j = await app.call(typeof path === "function" ? path() : path);
      if (dead) return;
      draw(j, first);
      first = false;
    } catch (e) { if (first && !dead) put(el, app.C.failure(e, "Network operations")); else console.warn(e); }
  };
  const stop = app.C.every(ms, run, true);
  app.refresh = run;
  return Object.assign(() => { dead = true; stop(); app.refresh = () => {}; }, { run });
}

const pts = (rows, key = "ts") => rows.map((r) => ({ ...r, t: T(r[key]) }));
const by = (rows, key) => { const m = {}; for (const r of rows) (m[r[key]] ||= []).push(r); return m; };
const money = (v) => (v == null ? "–" : v >= 1e6 ? "$" + (v / 1e6).toFixed(2) + "M" : v >= 1e4 ? "$" + Math.round(v / 1e3) + "k" : "$" + Math.round(v).toLocaleString("en-US"));
const hhmm = (ms) => new Date(ms).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit", hour12: false });
const chips = (app, list, max = 4) => app.h("span", {}, list.slice(0, max).map((x) => app.h("span", { class: "chip", style: { marginRight: "4px" } }, x)), list.length > max ? app.h("span", { class: "muted small" }, `+${list.length - max}`) : null);
const facts = (app, pairs) => app.h("div", { class: "no-facts" }, pairs.filter(Boolean).map(([k, v]) => app.h("span", {}, k + " ", app.h("b", {}, v))));
const kv = (app, pairs) => app.h("dl", { class: "kv" }, pairs.filter((p) => p && p[1] != null && p[1] !== "").flatMap(([k, v]) => [app.h("dt", {}, k), app.h("dd", {}, v)]));
const riskPill = (app, r) => app.C.pill({ high: "bad", medium: "warn", low: "ok" }[r] || "info", r + " risk");

function diffBlock(app, text) {
  const { h } = app;
  return h("pre", { class: "code", style: { whiteSpace: "pre-wrap", fontSize: "12px", margin: 0 } }, (text || "").split("\n").map((ln) => h("div", { style: ln.startsWith("+") ? { background: "color-mix(in srgb, var(--good) 16%, transparent)" } : ln.startsWith("-") ? { background: "color-mix(in srgb, var(--crit) 14%, transparent)" } : null }, ln || " ")));
}

/** One proposed action: what, why, the commands, and the two buttons. */
function actionCard(app, a, after) {
  const { h, C } = app;
  const decide = async (yes, btn) => {
    btn.disabled = true;
    try {
      const r = await app.ctx.api(`/v1/actions/${encodeURIComponent(a.id)}/${yes ? "approve" : "reject"}`, { body: { who: "You" }, timeout: 60000 });
      C.toast(yes ? (r.result || "Approved and carried out") : "Rejected", yes ? "ok" : "info", 6000);
      await app.poll();
      after?.();
    } catch (e) { C.toast(e.message, "bad", 6000); btn.disabled = false; }
  };
  const open = a.status === "proposed";
  const state = { executed: ["ok", "carried out"], done: ["ok", "done"], approved: ["ok", "approved"], rejected: ["disabled", "rejected"], expired: ["disabled", "no longer needed"], failed: ["bad", "failed"] }[a.status] || ["info", a.status];
  const yes = h("button", { class: "btn sm primary", onclick: () => decide(true, yes) }, C.icon("check"), "Approve and run");
  const no = h("button", { class: "btn sm", onclick: () => decide(false, no) }, "Reject");
  return h("div", { class: "no-act" + (open ? "" : " done") },
    h("div", { class: "row between" }, h("b", {}, a.title), h("span", { class: "row nw", style: { gap: "6px" } }, a.risk ? riskPill(app, a.risk) : null, open ? null : C.pill(state[0], state[1]))),
    a.detail ? h("div", { class: "small dim", style: { marginTop: "4px" } }, a.detail) : null,
    a.command ? h("pre", { class: "code" }, a.command) : null,
    a.result ? h("div", { class: "small", style: { marginTop: "6px" } }, C.icon("okc"), " ", a.result, a.decided_by ? h("span", { class: "muted" }, ` · ${a.decided_by}, ${C.fmt.ago(T(a.decided))}`) : null) : null,
    open ? h("div", { class: "row", style: { marginTop: "8px", gap: "8px" } }, yes, no, a.incident && after === undefined ? app.link(a.incident, "incidents", a.incident) : null) : null);
}

function runFacts(app, r) {
  return facts(app, [["queries", String(r.queries ?? Math.max(0, (r.steps || 1) - 1))], ["time in the database", app.C.fmt.ms(r.ms)], ["rows read", app.C.fmt.n(r.rows_read)], r.tokens ? ["tokens packed", app.C.fmt.n(r.tokens)] : null, r.graph_engine ? ["graph walked by", r.graph_engine === "sql" ? "SQL (REACHABLE, ppr)" : "the app"] : null]);
}

async function runDrawer(app, id) {
  const { h, C } = app;
  const body = h("div", {}, h("div", { class: "muted" }, "Loading…"));
  C.drawer({ title: "Agent run", sub: id, wide: true, body });
  try {
    const j = await app.call(`/v1/agents/runs/${encodeURIComponent(id)}`);
    const r = j.run;
    put(body, h("div", { class: "no-cause" }, r.summary), runFacts(app, r),
      r.incident ? h("div", { style: { marginTop: "8px" } }, app.link(`Open ${r.incident}`, "incidents", r.incident)) : null,
      h("div", { class: "no-sec" }, "Steps"), steps(app, j.steps),
      j.receipts.length ? [h("div", { class: "no-sec" }, "What it was shown"), receipt(app, j.receipts)] : null);
  } catch (e) { put(body, C.failure(e, "The run")); }
}

function runsTable(app, rows, opts = {}) {
  const { h, C } = app;
  return C.table([
    { key: "started", label: "When", render: (r) => C.timeEl(T(r.started)), sort: (r) => T(r.started) },
    { key: "agent", label: "Agent" },
    { key: "summary", label: "Found", sortable: false, render: (r) => h("span", { class: "ellipsis", style: { display: "block", maxWidth: opts.narrow ? "220px" : "560px" }, title: r.summary }, r.summary) },
    ...(opts.narrow ? [] : [{ key: "rows_read", label: "Rows", num: true, render: (r) => C.fmt.n(r.rows_read) }]),
    { key: "ms", label: "Time", num: true, render: (r) => C.fmt.ms(r.ms) },
    { key: "verdict", label: "Checked", render: (r) => r.verdict === "correct" ? C.pill("ok", "correct") : r.verdict ? C.pill("bad", r.verdict) : h("span", { class: "muted" }, "–") },
  ], { rows, compact: true, onRow: (r) => runDrawer(app, r.id), maxHeight: opts.maxHeight });
}

// ---------------------------------------------------------------- overview

function overviewPage(body, app) {
  const { h, C, charts } = app;
  const hero = h("div"), kpis = h("div", { class: "no-kpis" });
  const map = topoMap(app, { mode: "live" });
  const traffic = charts.timeChart({ height: 150, fmt: gbps, series: [{ name: "Backbone traffic", color: "var(--s1)", area: true, get: (p) => p.bps, fmt: gbps }] });
  const loss = charts.timeChart({ height: 110, fmt: (v) => v.toFixed(2) + "%", series: [{ name: "Worst loss on a link", color: "var(--crit)", area: true, get: (p) => p.loss, fmt: (v) => v.toFixed(2) + "%" }] });
  const svc = h("div"), right = h("div", { class: "no-col" });
  const mapSub = h("span", { class: "sub" });
  body.append(hero, kpis, h("div", { class: "no-cols wide" },
    h("div", { class: "no-col" },
      C.card({ title: "The network now", sub: mapSub, tools: app.link("Topology", "topology"), body: [map, mapLegend(app, "live")] }),
      h("div", { class: "no-cols even" }, C.card({ title: "Traffic", sub: "all backbone links, last 3 hours", body: traffic }), C.card({ title: "Packet loss", sub: "the worst link each minute", body: loss })),
      C.card({ title: "Services", sub: "against their objectives, last hour", tools: app.link("All services", "services"), body: svc, flush: true })),
    right));
  let topo = null;
  const stopTopo = C.every(20000, async () => { topo = await app.call("/v1/topology"); map.update(topo); mapSub.textContent = `${topo.devices.length} devices, ${topo.links.length} links · load from the last minute`; }, true);
  const stop = live(app, body, "/v1/overview", (o) => {
    const top = o.open[0];
    const n = (rows, key, val) => (rows.find((r) => r[key] === val) || {}).n || 0;
    const total = (rows) => rows.reduce((a, r) => a + r.n, 0);
    const judged = total(o.verdicts), correct = n(o.verdicts, "verdict", "correct");
    if (top) {
      const others = o.open.length - 1;
      put(hero, h("div", { class: "no-hero" },
        h("div", {}, h("div", { class: "row", style: { gap: "8px", marginBottom: "6px" } }, sevPill(app, top.severity), statusPill(app, top.status), h("span", { class: "mono small muted" }, top.id), h("span", { class: "small muted" }, `detected ${C.fmt.ago(T(top.detected))}`)),
          h("h2", {}, top.title), h("p", {}, top.root_cause || top.summary),
          h("div", { class: "meta" }, top.confidence ? h("span", {}, h("b", {}, pct(top.confidence)), " confident") : null, h("span", {}, h("b", {}, String(top.customers)), " customers attached"), top.revenue_at_risk ? h("span", {}, h("b", {}, money(top.revenue_at_risk)), " a minute at risk") : h("span", {}, "no service is missing its objective yet"), h("span", {}, "services ", chips(app, top.services)), h("span", {}, "commander ", h("b", {}, top.commander || "nobody yet")),
            others > 0 ? h("span", {}, app.link(`and ${others} more open`, "incidents")) : null)),
        h("div", { class: "stack", style: { gap: "8px", alignItems: "stretch" } }, h("button", { class: "btn primary", onclick: () => app.go("incidents", top.id) }, C.icon("right"), "Open the incident room"),
          o.pending.length ? h("span", { class: "small", style: { textAlign: "center" } }, h("b", {}, String(o.pending.length)), ` ${o.pending.length === 1 ? "action waits" : "actions wait"} for approval`) : null)));
    } else {
      put(hero, h("div", { class: "no-hero calm" }, h("div", {}, h("h2", {}, "The network is healthy"),
        h("p", {}, `No open incidents. ${total(o.links)} links, ${total(o.devices)} devices and ${total(o.bgp)} BGP sessions are watched by ${app.status?.rules || "the"} alert rules, each one a SQL query run every minute.`),
        h("div", { class: "meta" }, o.sim.simulate ? h("span", {}, "Use ", h("b", {}, "Inject a fault"), " to watch the rules fire and the agents work a live incident.") : null)),
        o.recent[0] ? h("button", { class: "btn", onclick: () => app.go("incidents", o.recent[0].id) }, "See the last incident") : null));
    }
    const last = o.traffic[o.traffic.length - 1] || {};
    const crit = n(o.alerts, "severity", "critical"), firing = total(o.alerts);
    const mttr = o.by_kind.length ? o.by_kind.reduce((a, r) => a + r.mttr * r.n, 0) / Math.max(1, total(o.by_kind)) : null;
    const ttd = o.by_kind.length ? o.by_kind.reduce((a, r) => a + r.ttd * r.n, 0) / Math.max(1, total(o.by_kind)) : null;
    put(kpis, 
      kpi(app, "Open incidents", String(o.open.length), o.open.length ? o.open.map((i) => i.severity).join(", ") : "none", o.open.length ? { style: { borderColor: "var(--crit)" } } : {}),
      kpi(app, "Alerts firing", String(firing), firing ? `${crit} critical` : "all rules quiet"),
      kpi(app, "Links up", `${n(o.links, "status", "up")}/${total(o.links)}`, total(o.links) - n(o.links, "status", "up") ? "some are down" : "all carrying traffic"),
      kpi(app, "Devices up", `${n(o.devices, "status", "up")}/${total(o.devices)}`, "reachable in the last minute"),
      kpi(app, "BGP sessions", `${n(o.bgp, "state", "established")}/${total(o.bgp)}`, "established"),
      kpi(app, "Traffic", gbps(last.bps), [charts.sparkline(o.traffic.slice(-60).map((p) => p.bps), { width: 120, height: 22 })]),
      kpi(app, "Time to detect", ttd == null ? "–" : Math.round(ttd) + " s", "mean, past month"),
      kpi(app, "Time to resolve", mins(mttr), "mean, past month"),
      kpi(app, "Agent diagnoses", judged ? `${correct}/${judged}` : "–", judged ? "match the simulator's answer key" : "none checked yet"));
    traffic.update(pts(o.traffic, "t"));
    loss.update(pts(o.traffic, "t"));
    put(svc, C.table([
      { key: "name", label: "Service", render: (r) => h("span", {}, h("b", {}, r.name), " ", h("span", { class: "muted small" }, `tier ${r.tier}`)) },
      { key: "state", label: "Now", render: (r) => C.pill(r.state === "ok" ? (r.availability < r.slo_target ? "warn" : "ok") : r.state === "degraded" ? "warn" : "bad", r.state === "ok" ? (r.availability < r.slo_target ? "recovered, hour below objective" : "within objective") : r.state) },
      { key: "availability", label: "Success", num: true, render: (r) => h("span", { style: r.availability < r.slo_target ? { color: "var(--crit-ink)", fontWeight: 600 } : null }, (r.availability * 100).toFixed(3) + "%") },
      { key: "slo_target", label: "Objective", num: true, render: (r) => (r.slo_target * 100).toFixed(2) + "%" },
      { key: "p95", label: "p95", num: true, render: (r) => h("span", { style: r.p95 > r.latency_slo_ms ? { color: "var(--crit-ink)", fontWeight: 600 } : null }, r.p95.toFixed(0) + " ms") },
      { key: "errors", label: "Errors, last hour", sortable: false, render: (r) => charts.sparkline(r.errors, { width: 130, height: 22, color: r.state === "ok" ? "var(--s3)" : "var(--crit)" }) },
      { key: "customers", label: "Customers", num: true },
    ], { rows: o.services, compact: true, onRow: (r) => serviceDrawer(app, r.id), sortKey: "tier", desc: false }));
    const kindRows = [...o.by_kind].sort((a, b) => b.n - a.n);
    put(right, 
      o.pending.length ? C.card({ title: "Waiting for your approval", body: o.pending.map((a) => h("div", { class: "no-act" }, h("div", { class: "row between" }, h("b", {}, a.title), riskPill(app, a.risk)), h("div", { class: "small", style: { marginTop: "4px" } }, app.link(`Review in ${a.incident}`, "incidents", a.incident)))) }) : null,
      C.card({ title: "Recent incidents", tools: app.link("All", "incidents"), body: h("ul", { class: "no-list" }, o.recent.slice(0, 6).map((i) => h("li", {}, h("div", { class: "row between nw" }, h("a", { class: "no-link ellipsis", href: app.ctx.href("incidents", i.id), title: i.title }, i.title), sevPill(app, i.severity)),
        h("div", { class: "small muted" }, `${i.id} · ${KIND[i.category] || i.category} · ${C.fmt.ago(T(i.started))} · resolved in ${mins(i.mttr_min)}`)))) }),
      C.card({ title: "Past month by cause", body: C.table([{ key: "category", label: "Cause", render: (r) => KIND[r.category] || r.category }, { key: "n", label: "Incidents", num: true }, { key: "ttd", label: "Detect", num: true, render: (r) => Math.round(r.ttd) + " s" }, { key: "mttr", label: "Resolve", num: true, render: (r) => mins(r.mttr) }], { rows: kindRows, compact: true }), flush: true }),
      C.card({ title: "Hottest links", sub: "last minute", body: charts.barList(o.hot.map((l) => ({ name: l.link, value: l.util, color: utilColor(l.util) })), { fmtv: (v) => pct(v), max: 1 }) }),
      C.card({ title: "Next changes", tools: app.link("Calendar", "changes"), body: h("ul", { class: "no-list" }, o.upcoming.slice(0, 5).map((c) => h("li", {}, h("div", { class: "row between nw" }, h("a", { class: "no-link ellipsis", href: app.ctx.href("changes"), title: c.title }, c.title), riskPill(app, c.risk)), h("div", { class: "small muted" }, `${c.id} · ${C.when(T(c.window_start), { sec: false })} · ${c.owner}`)))) }),
      C.card({ title: "What the agents did", tools: app.link("Agents", "agents"), body: o.runs.length ? runsTable(app, o.runs, { narrow: true }) : C.empty("agent", "No runs yet"), flush: true }),
      C.card({ title: "On call", body: h("ul", { class: "no-list" }, o.oncall.filter((p) => p.role === "primary").slice(0, 5).map((p) => h("li", { class: "row between nw" }, h("span", {}, h("b", {}, p.person), h("span", { class: "muted small" }, ` · ${p.team}`)), h("span", { class: "mono small muted" }, p.contact)))) }));
  }, 10000);
  return () => { stop(); stopTopo(); };
}

// ---------------------------------------------------------------- incidents

function incidentsPage(body, app) {
  const { h, C, charts } = app;
  let q = "";
  const input = h("input", { class: "input", type: "search", placeholder: "Search incidents by words or meaning: \"customers lost routes after maintenance\"", "aria-label": "Search incidents" });
  const list = h("div"), side = h("div", { class: "no-col" });
  const go = () => { q = input.value.trim(); stop.run(); };
  input.addEventListener("keydown", (e) => { if (e.key === "Enter") go(); });
  body.append(h("div", { class: "no-search", style: { gridTemplateColumns: "minmax(0, 1fr) auto" } }, input, h("button", { class: "btn primary", onclick: go }, C.icon("search"), "Search")),
    h("div", { class: "no-cols wide" }, C.card({ title: "Incidents", sub: "live ones and a month of history", body: list, flush: true }), side));
  const stop = live(app, body, () => "/v1/incidents" + (q ? "?q=" + encodeURIComponent(q) : ""), (j) => {
    put(list, j.incidents.length ? C.table([
      { key: "id", label: "Incident", render: (r) => h("span", { class: "mono small" }, r.id) },
      { key: "title", label: "What", render: (r) => h("span", { class: "ellipsis", style: { display: "block", maxWidth: "380px" }, title: r.title }, r.title) },
      { key: "severity", label: "Severity", render: (r) => sevPill(app, r.severity) },
      { key: "status", label: "Status", render: (r) => statusPill(app, r.status) },
      { key: "category", label: "Cause", render: (r) => KIND[r.category] || r.category },
      { key: "started", label: "Started", render: (r) => C.timeEl(T(r.started), { sec: false }), sort: (r) => T(r.started) },
      { key: "ttd_s", label: "Detect", num: true, render: (r) => (r.ttd_s == null ? "–" : Math.round(r.ttd_s) + " s") },
      { key: "mttr_min", label: "Resolve", num: true, render: (r) => (r.resolved ? mins(r.mttr_min) : "open") },
      { key: "customers", label: "Customers", num: true },
      { key: "commander", label: "Commander" },
    ], { rows: j.incidents, compact: true, onRow: (r) => app.go("incidents", r.id), sortKey: q ? null : "started", desc: true }) : C.empty("search", "No incident matches", "Try other words: the search reads titles, summaries and root causes, by words and by meaning."));
    put(side, 
      C.card({ title: "Time to resolve, by cause", sub: "mean, past month", body: charts.barList([...j.by_kind].sort((a, b) => b.mttr - a.mttr).map((r) => ({ name: `${KIND[r.category] || r.category} (${r.n})`, value: r.mttr })), { fmtv: mins }) }),
      C.card({ title: "Time to detect, by cause", sub: "mean, past month", body: charts.barList([...j.by_kind].sort((a, b) => b.ttd - a.ttd).map((r) => ({ name: KIND[r.category] || r.category, value: r.ttd })), { fmtv: (v) => Math.round(v) + " s" }) }),
      C.card({ title: "How this search works", body: h("div", { class: "small dim" }, "One query ranks incidents by BM25 over their text fused with the similarity of their embedding (reciprocal rank fusion). The root-cause agent runs the same query to find what a new incident looks like. See it under SQL.") }));
  }, 20000);
  return stop;
}

// ---------------------------------------------------------------- the incident room

function incidentRoom(body, app, path) {
  const { h, C, charts } = app;
  const id = path[1];
  const head = h("div"), left = h("div", { class: "no-col" }), right = h("div", { class: "no-col" });
  body.append(head, h("div", { class: "no-cols" }, left, right));
  // Pieces that live across refreshes, so a chart or the map is not rebuilt under the pointer.
  const map = topoMap(app, { mode: "live" });
  const causeEl = h("div"), actionsEl = h("div"), chartsEl = h("div", { class: "no-mini" }), stepsEl = h("div"), rcptEl = h("div"), logsEl = h("div");
  const replayEl = h("div"), keyEl = h("div");
  const mapCard = C.card({ title: "Where it is", sub: "the devices the incident touches, their neighbours dimmed", body: [replayEl, map, mapLegend(app, "live")] });
  const stepsSub = h("span", { class: "sub" });
  const board = boardCard(app, id);
  left.append(C.card({ title: "Likely cause", sub: "from the root-cause agent", body: [causeEl, keyEl] }), C.card({ title: "Proposed actions", sub: "nothing runs without an approval, unless autopilot is on", body: actionsEl }), board.el, mapCard,
    C.card({ title: "Around the incident", sub: "half an hour before it, to now", body: chartsEl }),
    C.card({ title: "How the agent got there", sub: stepsSub, body: stepsEl }),
    C.card({ title: "What the agent was shown", sub: "its brief, packed to a token budget; a click on a row shows the text", body: rcptEl }),
    C.card({ title: "Log lines around it", body: logsEl }));
  let topo = null, inc = null, replay = null, replayAt = null, drawnCharts = "";
  const focusMap = () => {
    if (!topo || !inc) return;
    map.update(topo);
    const f = new Set([...(inc.around || []), ...(inc.incident.devices || [])]);
    if (inc.incident.root_kind === "link") inc.incident.root_entity.split("--").forEach((x) => f.add(x));
    const patch = { focus: f, rootEntity: inc.incident.root_kind === "link" || inc.incident.root_kind === "device" ? inc.incident.root_entity : (inc.incident.devices || [])[0] || null };
    if (replay && replayAt != null) {
      const links = {}, devices = {}, alerting = {};
      for (const p of replay.links) if (T(p.ts) === replayAt) links[p.link] = p;
      for (const p of replay.device_points) if (T(p.ts) === replayAt) devices[p.device] = p;
      const rank = { critical: 3, major: 2, minor: 1 };
      for (const a of replay.alerts) if (T(a.started) <= replayAt && (!a.ended || T(a.ended) > replayAt) && a.entity_kind !== "link" && (rank[a.severity] || 0) > (rank[alerting[a.entity]] || 0)) alerting[a.entity] = a.severity;
      patch.over = { links, devices, alerting };
    } else patch.over = null;
    map.set(patch);
  };
  const stopTopo = C.every(30000, async () => { topo = await app.ctx.api("/v1/topology"); focusMap(); }, true);

  async function startReplay() {
    try { replay = await app.call(`/v1/incidents/${id}/replay`); } catch (e) { C.toast(e.message, "bad"); return; }
    const times = [...new Set(replay.links.map((p) => T(p.ts)))].sort((a, b) => a - b);
    if (!times.length) { C.toast("No telemetry kept for this window", "info"); replay = null; return; }
    let i = 0, timer = null;
    const label = h("span", { class: "mono small" });
    const slider = h("input", { type: "range", min: 0, max: times.length - 1, value: 0, "aria-label": "Minute" });
    const what = h("span", { class: "small dim" });
    const show = () => {
      replayAt = times[i]; slider.value = i;
      label.textContent = hhmm(replayAt);
      const evs = replay.timeline.filter((e) => T(e.ts) <= replayAt);
      const firing = replay.alerts.filter((a) => T(a.started) <= replayAt && (!a.ended || T(a.ended) > replayAt)).length;
      what.textContent = `${firing} ${firing === 1 ? "alert" : "alerts"} firing` + (evs.length ? ` · ${evs[evs.length - 1].note.slice(0, 110)}` : " · before the first alert");
      focusMap();
    };
    const play = h("button", { class: "btn sm" }, C.icon("play"), "Play");
    const halt = () => { clearInterval(timer); timer = null; put(play, C.icon("play"), "Play"); };
    play.onclick = () => { if (timer) return halt(); if (i >= times.length - 1) i = 0; put(play, C.icon("pause"), "Pause"); timer = setInterval(() => { if (i >= times.length - 1) return halt(); i++; show(); }, 700); };
    slider.oninput = () => { i = +slider.value; show(); };
    put(replayEl, h("div", { class: "no-replay" }, play, slider, h("span", { class: "row nw", style: { gap: "8px" } }, label, h("button", { class: "btn xs ghost", title: "Back to now", onclick: () => { halt(); replay = null; replayAt = null; drawReplayBtn(); focusMap(); } }, C.icon("x")))), h("div", { style: { marginBottom: "8px" } }, what));
    stopReplay = halt;
    show();
  }
  let stopReplay = () => {};
  const drawReplayBtn = () => put(replayEl, h("div", { class: "row between", style: { marginBottom: "6px" } }, h("span", { class: "small muted" }, "The map shows the network now."), h("button", { class: "btn sm", onclick: startReplay }, C.icon("timeline"), "Replay the incident minute by minute")));
  drawReplayBtn();

  async function checkKey(i) {
    try {
      const k = (await app.ctx.api(`/v1/incidents/${id}/key`)).key;
      if (!k) { put(keyEl, h("div", { class: "no-key" }, "The simulator did not cause this one, so there is no answer to check against.")); return; }
      const ok = k.truth_entity === i.root_entity;
      put(keyEl, h("div", { class: "no-key " + (ok ? "ok" : "no") }, h("b", {}, ok ? "Correct. " : "Not what the simulator did. "), `The simulator injected "${(app.status?.sim?.scenarios || []).find((s) => s.id === k.scenario)?.name || k.scenario}" on ${k.truth_entity} at ${hhmm(T(k.started))}. `, ok ? "The agent named the same thing, from the tables alone." : `The agent named ${i.root_entity || "nothing"}.`));
    } catch (e) { C.toast(e.message, "bad"); }
  }

  function closeDrawer(i, runbook, similar) {
    const options = [...new Map([runbook, ...similar.map((s) => ({ id: s.runbook, title: s.runbook }))].filter((r) => r && r.id).map((r) => [r.id, r])).values()];
    const sel = h("select", { class: "select" }, options.map((r) => h("option", { value: r.id }, r.title === r.id ? r.id : `${r.id}: ${r.title}`)), h("option", { value: "" }, "No runbook helped"));
    const grade = h("select", { class: "select" }, [[3, "It fixed it"], [2, "It helped"], [1, "Barely relevant"], [0, "Wrong runbook"]].map(([v, t]) => h("option", { value: v }, t)));
    const ok = h("button", { class: "btn primary" }, i.resolved ? "Record" : "Close the incident");
    const d = C.drawer({ title: i.resolved ? "Which runbook closed it?" : "Close the incident", sub: "Your answer is written to netops_judgments. The agent reads that table the next time an incident looks like this one, and the relevance autopilot can tune the ranking from it.",
      body: h("div", { class: "stack" }, h("label", { class: "stack", style: { gap: "4px" } }, h("span", { class: "small dim" }, "Runbook"), sel), h("label", { class: "stack", style: { gap: "4px" } }, h("span", { class: "small dim" }, "How well it worked"), grade)), foot: ok });
    ok.onclick = async () => {
      ok.disabled = true;
      try { await app.ctx.api(`/v1/incidents/${id}/close`, { body: { who: "You", runbook: sel.value, grade: +grade.value }, timeout: 60000 }); d.close(); C.toast(i.resolved ? "Recorded" : "Closed. A postmortem draft was written.", "ok"); await app.poll(); stop.run(); }
      catch (e) { C.toast(e.message, "bad"); ok.disabled = false; }
    };
  }

  const stop = live(app, body, `/v1/incidents/${id}`, (j) => {
    inc = j;
    const i = j.incident, open = !i.resolved;
    const act = async (what, okText) => { try { await app.ctx.api(`/v1/incidents/${id}/${what}`, { body: { who: "You" }, timeout: 120000 }); C.toast(okText, "ok"); stop.run(); } catch (e) { C.toast(e.message, "bad", 6000); } };
    put(head, h("div", { class: "no-hero" + (open ? "" : " calm") },
      h("div", {}, h("div", { class: "row", style: { gap: "8px", marginBottom: "6px" } }, app.link("Incidents", "incidents"), h("span", { class: "muted" }, "/"), h("span", { class: "mono small" }, i.id), sevPill(app, i.severity), statusPill(app, i.status), h("span", { class: "chip" }, KIND[i.category] || i.category)),
        h("h2", {}, i.title), h("p", {}, i.summary),
        h("div", { class: "meta" }, h("span", {}, "started ", h("b", {}, C.when(T(i.started), { sec: false }))), h("span", {}, "detected in ", h("b", {}, Math.round(i.ttd_s) + " s")), h("span", {}, open ? "open for " : "resolved in ", h("b", {}, open ? C.fmt.dur((Date.now() - T(i.started)) / 1000) : mins(i.mttr_min))),
          h("span", {}, h("b", {}, String(i.customers)), " customers"), i.revenue_at_risk ? h("span", {}, h("b", {}, money(i.revenue_at_risk)), open ? " a minute at risk" : " of revenue touched") : null, h("span", {}, "commander ", h("b", {}, i.commander || "nobody")))),
      h("div", { class: "row", style: { gap: "8px", justifyContent: "flex-end" } },
        open ? h("button", { class: "btn sm", onclick: () => act("take", "You are the commander") }, C.icon("user"), "Take it") : null,
        open ? h("button", { class: "btn sm", title: "Run the root-cause agent again on the alerts as they are now", onclick: () => act("investigate", "Investigated again") }, C.icon("agent"), "Investigate again") : null,
        i.postmortem ? h("button", { class: "btn sm", onclick: () => C.drawer({ title: "Postmortem draft", sub: `${i.id} · written by the agent from the timeline when the incident resolved`, wide: true, body: h("div", { style: { whiteSpace: "pre-wrap", lineHeight: 1.55 } }, i.postmortem), foot: h("button", { class: "btn", onclick: () => C.copy(i.postmortem, "Postmortem copied") }, C.icon("copy"), "Copy") }) }, C.icon("note"), "Postmortem") : null,
        h("button", { class: "btn sm " + (open ? "primary" : ""), onclick: () => closeDrawer(i, j.runbook, j.similar) }, C.icon("check"), open ? "Close…" : "Record the runbook"))));
    // Cause.
    const rc = j.runs.filter((r) => r.agent === "root-cause").pop();
    const last = j.steps[j.steps.length - 1];
    const because = last && last.tool === "reason" && last.detail.includes("Because:") ? last.detail.split("Because:")[1].split(";").map((x) => x.trim().replace(/\.$/, "")).filter(Boolean) : [];
    put(causeEl, i.root_cause ? h("div", {}, h("div", { class: "no-cause" }, i.root_cause),
      h("div", { class: "no-conf", style: { marginTop: "10px" } }, h("div", { class: "meter" }, h("i", { style: { width: i.confidence * 100 + "%", background: i.confidence >= 0.8 ? "var(--good)" : i.confidence >= 0.5 ? "var(--warn)" : "var(--crit)" } })), h("b", {}, pct(i.confidence) + " confident")),
      because.length ? h("ul", { class: "no-why" }, because.map((b) => h("li", {}, b))) : null,
      rc ? runFacts(app, rc) : null)
      : C.empty("agent", "No cause named yet", "The agent runs when the incident opens and again as alerts arrive."));
    if (!keyEl.firstChild) put(keyEl, h("button", { class: "btn xs", style: { marginTop: "10px" }, title: "The simulator knows what it broke. The agent is never shown this.", onclick: () => checkKey(i) }, C.icon("eye"), "Check against the simulator's answer key"));
    stepsSub.textContent = rc ? `${rc.queries ?? j.steps.length - 1} queries, ${C.fmt.ms(rc.ms)} in the database. Click a step for its SQL.` : "";
    // Actions.
    put(actionsEl, j.actions.length ? j.actions.map((a) => actionCard(app, a, () => stop.run())) : C.empty("bolt", "No actions proposed"));
    focusMap();
    // Charts: rebuilt only when the set of series changes.
    const mark = () => [{ t: T(i.started), label: "detected" }, i.resolved ? { t: T(i.resolved), label: "resolved" } : null].filter(Boolean);
    const groups = [["Link load", j.link_series, "link", (p) => p.util, (v) => pct(v), 1], ["Link loss", j.link_series, "link", (p) => p.loss, (v) => v.toFixed(2) + "%"], ["Receive power", j.link_series, "link", (p) => Math.max(0, (p.up ? p.rx_dbm : -40) + 40), (v) => (v - 40).toFixed(0) + " dBm", 40],
      ["Device CPU", j.device_series, "device", (p) => p.cpu, (v) => v.toFixed(0) + "%", 100], ["BGP sessions up", j.device_series.filter((p) => p.sessions > 0 || j.device_series.some((x) => x.device === p.device && x.sessions > 0)), "device", (p) => p.sessions, (v) => String(Math.round(v))],
      ["Service errors a minute", j.service_series, "service", (p) => p.errors, C.fmt.si], ["Service latency, p95", j.service_series, "service", (p) => p.latency_p95, (v) => v.toFixed(0) + " ms"]].filter((g) => g[1].length);
    const sig = groups.map((g) => g[0] + Object.keys(by(g[1], g[2])).join(",")).join("|");
    if (sig !== drawnCharts) {
      drawnCharts = sig;
      put(chartsEl);
      chartsEl._c = groups.map(([title, , key, get, fmtv, max]) => {
        const names = Object.keys(by(groups.find((g) => g[0] === title)[1], key)).slice(0, 6);
        const ch = charts.timeChart({ height: 130, fmt: fmtv, max: max || undefined, markers: mark, series: names.map((n, k) => ({ name: n, color: `var(--s${k + 1})`, get: (p) => p[n], fmt: fmtv })) });
        chartsEl.append(h("div", {}, h("div", { class: "no-sec" }, title), ch));
        return { title, ch, key, get, names };
      });
    }
    for (const c of chartsEl._c || []) {
      const rows = groups.find((g) => g[0] === c.title)?.[1] || [];
      const at = new Map();
      for (const r of rows) { const t = T(r.ts); const p = at.get(t) || { t }; p[r[c.key]] = c.get(r); at.set(t, p); }
      c.ch.update([...at.values()].sort((a, b) => a.t - b.t));
    }
    put(stepsEl, steps(app, j.steps));
    put(rcptEl, receipt(app, j.receipts));
    put(logsEl, logLines(app, j.logs));
    // The right column.
    put(right, 
      C.card({ title: "Timeline", body: timeline(app, j.timeline) }),
      C.card({ title: "Alerts", sub: `${j.alerts.length} grouped into this incident`, body: h("ul", { class: "no-list" }, j.alerts.map((a) => h("li", {}, h("div", { class: "row between nw" }, h("b", { class: "ellipsis" }, a.rule_name), sevPill(app, a.severity)),
        h("div", { class: "small muted" }, h("a", { class: "no-link", onclick: () => (a.entity_kind === "link" ? linkDrawer(app, a.entity) : a.entity_kind === "device" ? deviceDrawer(app, a.entity) : a.entity_kind === "service" ? serviceDrawer(app, a.entity) : null) }, a.entity), ` · ${C.fmt.n(a.reading)} ${a.unit} against ${C.fmt.n(a.threshold)} · ${a.status === "firing" ? "firing since " + hhmm(T(a.started)) : "cleared " + hhmm(T(a.ended))}`)))) }),
      C.card({ title: "Blast radius", sub: "one graph walk from the alerting devices", body: [
        h("div", { class: "no-sec" }, "Services that ride on them"), h("ul", { class: "no-list" }, j.blast.map((s) => h("li", { class: "row between nw" }, h("a", { class: "no-link", onclick: () => serviceDrawer(app, s.id) }, s.name, h("span", { class: "muted small" }, ` · tier ${s.tier} · ${s.customers} customers`)), h("span", { class: "num small" }, money(s.revenue_per_min) + "/min")))),
        j.customers.length ? [h("div", { class: "no-sec" }, "Customers attached there"), h("ul", { class: "no-list" }, j.customers.slice(0, 6).map((c) => h("li", { class: "row between nw" }, h("span", { class: "ellipsis" }, c.name, h("span", { class: "muted small" }, ` · ${c.tier} · ${c.device}`)), h("span", { class: "num small" }, money(c.mrr) + "/mo"))))] : null] }),
      j.change ? C.card({ title: "The change", sub: j.change.id, body: [h("b", {}, j.change.title), h("div", { class: "small dim", style: { margin: "4px 0 8px" } }, j.change.description),
        kv(app, [["Owner", j.change.owner], ["Approved by", j.change.approver], ["Executed", C.when(T(j.change.executed))], ["Risk declared", j.change.risk], ["Roll back with", j.change.rollback_plan]]), h("div", { class: "no-sec" }, "What it changed"), diffBlock(app, j.change.diff)] }) : null,
      j.runbook ? C.card({ title: "Runbook", sub: `${j.runbook.id} · closed ${j.runbook.wins} of the ${j.runbook.uses} incidents it was used on`, body: [h("b", {}, j.runbook.title), h("div", { class: "small", style: { whiteSpace: "pre-wrap", marginTop: "6px", color: "var(--text-2)", lineHeight: 1.5 } }, j.runbook.body)],
        foot: h("button", { class: "btn sm", onclick: () => closeDrawer(i, j.runbook, j.similar) }, C.icon("check"), "It worked") }) : null,
      C.card({ title: "It looks like", sub: "past incidents, by words and meaning", body: j.similar.length ? h("ul", { class: "no-list" }, j.similar.map((s) => h("li", {}, h("a", { class: "no-link", href: app.ctx.href("incidents", s.id) }, s.title),
        h("div", { class: "small muted" }, `${s.id} · ${KIND[s.category] || s.category} · ${C.fmt.ago(T(s.started))} · resolved in ${mins(s.mttr_min)}${s.runbook ? " with " + s.runbook : ""}`)))) : C.empty("search", "Nothing similar") }),
      j.judgments.length ? C.card({ title: "Judgments", sub: "who said which runbook worked", body: h("ul", { class: "no-list" }, j.judgments.map((g) => h("li", { class: "small" }, `${g.judge || g.who || "Someone"}: ${g.runbook || g.doc_id} graded ${g.grade}`))) }) : null);
  }, 12000);
  return () => { stop(); stopTopo(); stopReplay(); board.stop(); };
}

// ---------------------------------------------------------------- topology

function topologyPage(body, app) {
  const { h, C } = app;
  let mode = "live", data = null, hops = null;
  const map = topoMap(app, { mode, onPair: (a, b, state) => evidenceDrawer(app, a, b, state) });
  const legend = h("div"), side = h("div", { class: "no-col" }), title = h("span", { class: "sub" });
  const seg = C.seg([["live", "Live load"], ["logs", "As read from the logs"], ["diff", "Logs against the inventory"]], mode, (v) => { mode = v; map.set({ mode, path: mode === "live" ? hops : null }); draw(); });
  body.append(h("div", { class: "no-cols wide" }, C.card({ title: "Topology", sub: title, tools: seg, body: [map, legend] }), side));
  const fromSel = h("select", { class: "select", "aria-label": "From" }), toSel = h("select", { class: "select", "aria-label": "To" });
  const pathEl = h("div");
  const trace = async () => {
    try {
      const j = await app.call(`/v1/topology/path?from=${fromSel.value}&to=${toSel.value}`);
      hops = j.path.map((p) => p.id);
      map.set({ path: hops });
      put(pathEl, j.path.length ? h("div", {}, h("ol", { class: "no-tl", style: { marginTop: "10px" } }, j.path.map((p) => h("li", {}, h("a", { class: "no-link", onclick: () => deviceDrawer(app, p.id) }, p.id), h("span", { class: "muted small" }, ` · ${p.role} at ${p.site.toUpperCase()}`)))),
        h("div", { class: "small muted", style: { marginTop: "8px" } }, `${j.path.length - 1} hops, found by ${j.engine === "sql" ? "PATH() in SQL over netops_links" : "the app's copy of the graph (a coordinator does not walk graphs yet)"}.`)) : C.empty("route", "No path", "The two devices are not connected by links that are up."));
    } catch (e) { C.toast(e.message, "bad"); }
  };
  const rediscover = async (btn) => {
    btn.disabled = true;
    try { const r = await app.ctx.api("/v1/topology/discover", { body: {}, timeout: 120000 }); C.toast(r.summary || "Read the logs again", "ok", 6000); stop.run(); } catch (e) { C.toast(e.message, "bad"); }
    btn.disabled = false;
  };
  function draw() {
    if (!data) return;
    const c = data.compare || {};
    put(legend, mapLegend(app, mode));
    title.textContent = mode === "live" ? "the inventory's links, coloured by load in the last minute" : mode === "logs" ? `${c.seen ?? 0} links built from ${C.fmt.n(c.lines ?? 0)} LLDP and routing-adjacency lines in syslog` : "what the logs say that the inventory does not, and the reverse";
    if (mode === "live") {
      const m = Object.fromEntries(data.metrics.map((x) => [x.link, x]));
      const down = data.links.filter((l) => m[l.id] && m[l.id].up === 0);
      const hot = [...data.metrics].filter((x) => x.up).sort((a, b) => b.util - a.util).slice(0, 8);
      if (!fromSel.options.length) {
        const opts = () => data.devices.map((d) => h("option", { value: d.id }, d.id));
        fromSel.append(...opts()); toSel.append(...opts());
        fromSel.value = "lf1-fra"; toSel.value = "br1-nyc";
      }
      put(side, 
        h("div", { class: "no-kpis", style: { gridTemplateColumns: "1fr 1fr", marginBottom: 0 } }, kpi(app, "Links down", String(down.length), down.length ? down.map((l) => l.id).slice(0, 2).join(", ") : "none"), kpi(app, "Alerts on the map", String(data.alerts.length), "red and amber dots")),
        C.card({ title: "Trace a path", sub: "shortest path over links that are up", body: [h("div", { class: "row", style: { gap: "8px" } }, fromSel, h("span", { class: "muted" }, "to"), toSel, h("button", { class: "btn sm primary", onclick: trace }, C.icon("route"), "Trace")), pathEl] }),
        C.card({ title: "Hottest links", body: app.charts.barList(hot.map((l) => ({ name: l.link, value: l.util, color: utilColor(l.util) })), { fmtv: (v) => pct(v), max: 1 }) }),
        C.card({ title: "Sites", body: C.table([{ key: "name", label: "Site" }, { key: "kind", label: "Kind", render: (r) => ({ dc: "data centre", hub: "hub", pop: "PoP" }[r.kind] || r.kind) }, { key: "region", label: "Region" }, { key: "devices", label: "Devices", num: true }], { rows: data.sites, compact: true }), flush: true }));
    } else {
      const btn = h("button", { class: "btn sm", onclick: () => rediscover(btn) }, C.icon("refresh"), "Read the logs again");
      put(side, 
        C.card({ title: "Topology from logs", sub: c.at ? `read ${C.fmt.ago(T(c.at))}` : "", tools: btn, body: [h("div", { class: "no-cause" }, c.summary || "The topology agent has not run yet."),
          h("div", { class: "no-kpis", style: { gridTemplateColumns: "repeat(2, 1fr)", margin: "12px 0 0" } }, kpi(app, "Links heard", String(c.seen ?? 0), `in ${C.fmt.n(c.lines ?? 0)} log lines`), kpi(app, "Match the inventory", String(c.confirmed ?? 0), `of ${data.links.length}`), kpi(app, "Not in the inventory", String((c.undocumented || []).length), "cables nobody wrote down", (c.undocumented || []).length ? { style: { borderColor: "var(--s7)" } } : {}), kpi(app, "Never heard", String((c.silent || []).length), "in the inventory only", (c.silent || []).length ? { style: { borderColor: "var(--crit)" } } : {})),
          h("div", { class: "small dim", style: { marginTop: "10px" } }, "No device is polled for this. A router that brings a neighbour up says so in syslog (LLDP_NEIGHBOR_UP, OSPF adjacency Full), naming its own port and the far end. The agent finds those lines with the substring index, parses both ends and builds the links. A link heard from both ends is certain; one heard from one end is likely.")] }),
        C.card({ title: "In the logs, not in the inventory", sub: "someone ran a cable and told nobody", body: (c.undocumented || []).length ? h("ul", { class: "no-list" }, c.undocumented.map((u) => h("li", {}, h("div", { class: "row between nw" }, h("b", {}, `${u.src} to ${u.dst}`), h("button", { class: "btn xs", onclick: () => evidenceDrawer(app, u.src, u.dst, "undocumented") }, "Evidence")),
          h("div", { class: "small muted" }, `${u.a_if} to ${u.z_if} · ${u.evidence} lines · first heard ${C.fmt.ago(T(u.first_seen))} · ${pct(u.confidence)} confident`)))) : C.empty("okc", "Nothing undocumented") }),
        C.card({ title: "In the inventory, never heard", sub: "decommissioned, mislabelled, or dark", body: (c.silent || []).length ? h("ul", { class: "no-list" }, c.silent.map((u) => h("li", {}, h("div", { class: "row between nw" }, h("a", { class: "no-link", onclick: () => linkDrawer(app, u.id) }, u.id), h("button", { class: "btn xs", onclick: () => evidenceDrawer(app, u.src, u.dst, "silent") }, "Evidence")),
          h("div", { class: "small muted" }, `${u.kind}${u.circuit ? " · circuit " + u.circuit + ", still billed?" : ""}`)))) : C.empty("okc", "Every inventory link was heard") }),
        C.card({ title: "Links heard", body: C.table([{ key: "id", label: "Link", render: (r) => h("span", { class: "mono small" }, r.id) }, { key: "state", label: "State", render: (r) => r.state === "confirmed" ? C.pill("ok", "confirmed") : C.pill("warn", r.state) }, { key: "protocols", label: "Heard by", render: (r) => [...new Set(r.protocols)].join(", "), sortable: false }, { key: "evidence", label: "Lines", num: true }, { key: "confidence", label: "Confidence", num: true, render: (r) => pct(r.confidence) }],
          { rows: data.seen, compact: true, maxHeight: "320px", sortKey: "state", desc: true, onRow: (r) => evidenceDrawer(app, r.src, r.dst, r.state) }), flush: true }));
    }
  }
  const stop = live(app, body, "/v1/topology", (j) => { data = j; map.update(j); draw(); }, 15000);
  return stop;
}

async function evidenceDrawer(app, a, b, state) {
  const { h, C } = app;
  const body = h("div", {}, h("div", { class: "muted" }, "Loading…"));
  const says = { undocumented: "The logs show these two devices as neighbours. The inventory has no link between them.", silent: "The inventory has this link. Neither device has ever logged the other as a neighbour.", confirmed: "The logs and the inventory agree." }[state] || "";
  C.drawer({ title: `${a} and ${b}`, sub: says, wide: true, body });
  try {
    const j = await app.call(`/v1/topology/evidence?a=${a}&b=${b}`);
    put(body, h("div", { class: "no-sec" }, "What the inventory says"), j.inventory.length ? kv(app, j.inventory.flatMap((l) => [["Link", l.id], ["Ports", `${l.a_if} to ${l.z_if}`], ["Kind", l.kind], l.circuit ? ["Circuit", `${l.circuit} (${l.provider})`] : null, ["Status", l.status]])) : h("div", { class: "dim" }, "No link between these devices in netops_links."),
      h("div", { class: "no-sec" }, `What the logs say (${j.lines.length} lines)`), j.lines.length ? logLines(app, j.lines.map((l) => ({ ...l, sev: 6 })), b) : h("div", { class: "dim" }, "No log line of either device names the other."),
      h("div", { class: "no-sec" }, "The query"), (j.queries || []).map((q) => queryBlock(app, q)));
  } catch (e) { put(body, C.failure(e, "Evidence")); }
}

// ---------------------------------------------------------------- alerts and rules

function alertsPage(body, app) {
  const { h, C, charts } = app;
  let tab = "firing";
  const main = h("div"), side = h("div", { class: "no-col" });
  const seg = C.seg([["firing", "Firing"], ["resolved", "Resolved"], ["silences", "Silences"], ["rules", "Rules"]], tab, (v) => { tab = v; put(main); stop.run(); });
  const act = async (path, body, ok) => { try { await app.ctx.api(path, { body }); C.toast(ok, "ok"); stop.run(); app.poll(); } catch (e) { C.toast(e.message, "bad", 6000); } };
  const add = h("button", { class: "btn sm primary", onclick: () => ruleDrawer(app, null, () => stop.run()) }, C.icon("plus"), "New rule");
  body.append(h("div", { class: "row between", style: { marginBottom: "12px" } }, seg, add), h("div", { class: "no-cols wide" }, main, side));
  const stop = live(app, body, () => (tab === "rules" ? "/v1/rules" : tab === "silences" ? "/v1/silences" : "/v1/alerts?status=" + tab), (j) => {
    if (tab === "silences" && j.silences) {
      put(main, silencesCard(app, j, () => stop.run()));
      put(side, C.card({ title: "Quiet, not blind", body: h("div", { class: "small dim stack" }, h("div", {}, "A silenced alert is still written to the table with the silence that caught it, so the record of what happened during a window is complete. It pages nobody and opens no incident."), h("div", {}, "Starting a change silences its devices until its window ends. A carrier notice you plan around silences its circuit for the carrier's window."), h("div", {}, "An alert that belongs to an open incident is never silenced.")) }));
      return;
    }
    if (tab === "rules" && j.rules) {
      put(main, C.card({ title: "Alert rules", sub: `each is a SELECT that returns what is wrong; all ${j.rules.length} ran in ${C.fmt.ms(j.eval_ms)} a minute ago`, flush: true, body: C.table([
        { key: "enabled", label: "On", sortable: false, render: (r) => { const t = C.toggle(r.enabled, async () => { try { await app.ctx.api(`/v1/rules/${r.id}/toggle`, { body: {} }); stop.run(); } catch (e) { C.toast(e.message, "bad"); } }); t.addEventListener("click", (e) => e.stopPropagation()); return t; } },
        { key: "name", label: "Rule", render: (r) => h("span", {}, h("b", {}, r.name), r.builtin ? null : h("span", { class: "badge accent", style: { marginLeft: "6px" } }, "yours")) },
        { key: "severity", label: "Severity", render: (r) => sevPill(app, r.severity) },
        { key: "entity_kind", label: "About" },
        { key: "threshold", label: "Fires when", sortable: false, render: (r) => `${r.cmp} ${C.fmt.n(r.threshold)} ${r.unit}` },
        { key: "window_s", label: "Window", num: true, render: (r) => C.fmt.dur(r.window_s) },
        { key: "last_ms", label: "Took", num: true, render: (r) => C.fmt.ms(r.last_ms) },
        { key: "firing", label: "Firing", num: true, render: (r) => r.firing ? h("b", { style: { color: "var(--crit-ink)" } }, String(r.firing)) : "0" },
      ], { rows: j.rules, compact: true, onRow: (r) => ruleDrawer(app, r, () => stop.run()) }) }));
      put(side, C.card({ title: "Rules are queries", body: h("div", { class: "small dim stack" }, h("div", {}, "A rule is a SELECT over the telemetry tables that returns two columns: ", h("code", {}, "entity"), " (what is wrong) and ", h("code", {}, "v"), " (the reading). The app runs every rule each minute; a row is an alert, and an alert clears after two quiet minutes."), h("div", {}, "Alerts on devices that are neighbours in the topology are grouped into one incident, so a fibre cut is one page, not eight."), h("div", {}, "Alertmanager and Grafana can post here too: ", h("code", {}, "POST /v1/alerts/webhook"), ".")) }),
        C.card({ title: "Time to evaluate", sub: "per rule, last run", body: charts.barList([...j.rules].sort((a, b) => b.last_ms - a.last_ms).slice(0, 8).map((r) => ({ name: r.name, value: r.last_ms })), { fmtv: C.fmt.ms }) }));
      return;
    }
    if (!j.alerts) return;
    put(main, C.card({ title: tab === "firing" ? "Firing now" : "Resolved, last day", flush: true, body: j.alerts.length ? C.table([
      { key: "severity", label: "Severity", render: (r) => sevPill(app, r.severity), sort: (r) => ({ critical: 3, major: 2, minor: 1 }[r.severity]) },
      { key: "rule_name", label: "Rule" },
      { key: "entity", label: "On", render: (r) => h("a", { class: "no-link mono small", onclick: (e) => { e.stopPropagation(); r.entity_kind === "link" ? linkDrawer(app, r.entity) : r.entity_kind === "device" ? deviceDrawer(app, r.entity) : r.entity_kind === "service" ? serviceDrawer(app, r.entity) : null; } }, r.entity) },
      { key: "reading", label: "Reading", num: true, render: (r) => `${C.fmt.n(r.reading)} ${r.unit}` },
      { key: "threshold", label: "Threshold", num: true, render: (r) => C.fmt.n(r.threshold) },
      { key: "started", label: "Since", render: (r) => C.timeEl(T(r.started), { sec: false }), sort: (r) => T(r.started) },
      ...(tab === "resolved" ? [{ key: "ended", label: "Lasted", num: true, render: (r) => C.fmt.dur((T(r.ended) - T(r.started)) / 1000), sort: (r) => T(r.ended) - T(r.started) }] : []),
      { key: "incident", label: "Incident", render: (r) => r.incident ? app.link(r.incident, "incidents", r.incident) : h("span", { class: "muted" }, "–") },
      ...(tab === "firing" ? [{ key: "acked_by", label: "Owner", render: (r) => r.silenced ? h("span", { class: "chip", title: "Silenced by " + r.silenced }, "silenced") : r.acked_by ? h("span", { class: "small" }, r.acked_by) : h("span", { class: "muted small" }, "nobody") },
        { key: "id", label: "", sortable: false, render: (r) => h("span", { class: "row nw", style: { gap: "4px", justifyContent: "flex-end" } },
          r.silenced ? null : r.acked_by ? h("button", { class: "btn xs ghost", onclick: (e) => { e.stopPropagation(); act(`/v1/alerts/${encodeURIComponent(r.id)}/unack`, { who: WHO }, "Handed back"); } }, "Hand back") : h("button", { class: "btn xs", onclick: (e) => { e.stopPropagation(); act(`/v1/alerts/${encodeURIComponent(r.id)}/ack`, { who: WHO }, "Acknowledged: it is yours now"); } }, "Acknowledge"),
          r.silenced || !["device", "link"].includes(r.entity_kind) ? null : h("button", { class: "btn xs ghost", onclick: (e) => { e.stopPropagation(); silenceDrawer(app, r.entity_kind, r.entity, () => stop.run()); } }, "Silence")) }] : []),
    ], { rows: j.alerts, compact: true, sortKey: "started", desc: true }) : C.empty("okc", tab === "firing" ? "Nothing is firing" : "Nothing resolved yet", tab === "firing" ? "Every rule came back empty a minute ago." : null) }));
    const ph = charts.timeChart({ height: 130, fmt: C.fmt.n, series: [{ name: "Alerts opened", color: "var(--s7)", area: true, get: (p) => p.n, fmt: C.fmt.n }] });
    put(side, j.per_hour.length > 2 ? C.card({ title: "Alerts opened an hour", sub: "last day", body: ph }) : null, C.card({ title: "By rule", sub: "last day", body: j.by_rule.length ? charts.barList(j.by_rule.map((r) => ({ name: r.rule_name, value: r.n })), { fmtv: C.fmt.n }) : C.empty("alerts", "None") }));
    ph.update(pts(j.per_hour, "t"));
  }, 10000);
  return stop;
}

const RULE_EXAMPLE = `SELECT device AS entity, MAX(temp_c) AS v
FROM netops_device_metrics
WHERE ts >= :since
GROUP BY device
HAVING MAX(temp_c) > 60`;

function ruleDrawer(app, rule, after) {
  const { h, C } = app;
  const isNew = !rule, ro = rule?.builtin;
  const f = (label, el) => h("label", { class: "stack", style: { gap: "4px" } }, h("span", { class: "small dim" }, label), el);
  const inp = (v, attrs = {}) => h("input", { class: "input", value: v ?? "", disabled: ro || null, ...attrs });
  const sel = (v, options) => { const s = h("select", { class: "select", disabled: ro || null }, options.map((o) => h("option", { value: o }, o))); s.value = v; return s; };
  const name = inp(rule?.name, { placeholder: "Chassis running hot" }), desc = inp(rule?.description, { placeholder: "What it means when this fires" });
  const sev = sel(rule?.severity || "minor", ["critical", "major", "minor"]), kind = sel(rule?.entity_kind || "device", ["device", "link", "service", "site"]), cmp = sel(rule?.cmp || ">", [">", "<", ">=", "<="]);
  const thr = inp(rule?.threshold ?? 60, { type: "number", step: "any" }), unit = inp(rule?.unit ?? "°C"), win = inp(rule?.window_s ?? 180, { type: "number", min: 60, step: 60 });
  const sql = h("textarea", { class: "input mono", rows: 9, spellcheck: "false", style: { width: "100%", fontSize: "12.5px", resize: "vertical" }, readOnly: ro || null }, rule?.sql_text || RULE_EXAMPLE);
  const out = h("div");
  const test = h("button", { class: "btn" }, C.icon("play"), "Run it now");
  test.onclick = async () => {
    test.disabled = true;
    try {
      const r = await app.ctx.api("/v1/rules/test", { body: { sql: sql.value, window_s: +win.value }, timeout: 60000 });
      const q = (r.queries || [])[0];
      put(out, r.error ? h("div", { class: "error" }, r.error) : h("div", {}, h("div", { class: "small", style: { margin: "8px 0" } }, h("b", {}, r.count === 0 ? "Nothing would fire right now." : `${r.count} ${r.count === 1 ? "alert" : "alerts"} would fire right now.`), q ? h("span", { class: "muted" }, ` ${C.fmt.ms(q.ms)}`) : null, r.note ? h("div", { style: { color: "var(--crit-ink)" } }, r.note) : null),
        r.rows.length ? C.table(Object.keys(r.rows[0]).map((k) => ({ key: k, label: k, num: typeof r.rows[0][k] === "number", render: (x) => (typeof x[k] === "number" ? C.fmt.n(Math.round(x[k] * 100) / 100) : String(x[k])) })), { rows: r.rows, compact: true, maxHeight: "220px" }) : null));
    } catch (e) { put(out, h("div", { class: "error" }, e.message)); }
    test.disabled = false;
  };
  const save = h("button", { class: "btn primary" }, "Save the rule");
  const del = !isNew && !ro ? h("button", { class: "btn danger", onclick: async () => { try { await app.ctx.api(`/v1/rules/${rule.id}/delete`, { body: {} }); d.close(); after(); } catch (e) { C.toast(e.message, "bad"); } } }, C.icon("trash"), "Delete") : null;
  const d = C.drawer({ title: isNew ? "New alert rule" : rule.name, sub: ro ? "A built-in rule: it can be switched off, and copied into a rule of your own." : "A SELECT that returns entity and v. :since is the start of the window.", wide: true,
    body: h("div", { class: "stack" }, h("div", { class: "grid g2" }, f("Name", name), f("What it means", desc)), h("div", { class: "grid g3" }, f("Severity", sev), f("About a", kind), f("Window, seconds", win)), h("div", { class: "grid g3" }, f("Fires when v is", cmp), f("Threshold (shown on the alert)", thr), f("Unit", unit)), f("SQL", sql),
      rule ? facts(app, [["last run", C.fmt.ms(rule.last_ms)], ["rows", String(rule.last_rows)], ["firing", String(rule.firing)], rule.runbook ? ["runbook", rule.runbook] : null]) : null, out),
    foot: [test, ro ? null : save, del] });
  save.onclick = async () => {
    save.disabled = true;
    try { await app.ctx.api("/v1/rules", { body: { id: isNew ? undefined : rule.id, name: name.value, description: desc.value, sql: sql.value, severity: sev.value, entity_kind: kind.value, cmp: cmp.value, threshold: +thr.value, unit: unit.value, window_s: +win.value, runbook: rule?.runbook || "" }, timeout: 60000 }); d.close(); C.toast("Saved. It runs with the others from the next minute.", "ok"); after(); }
    catch (e) { C.toast(e.message, "bad", 6000); save.disabled = false; }
  };
}

// ---------------------------------------------------------------- changes

function changesPage(body, app) {
  const { h, C } = app;
  const cal = h("div"), list = h("div"), side = h("div", { class: "no-col" });
  body.append(h("div", { class: "no-col" }, C.card({ title: "Change calendar", sub: "three days back, a week ahead; a red outline is a collision", body: cal }), h("div", { class: "no-cols wide" }, C.card({ title: "All changes", body: list, flush: true }), side)));
  const stop = live(app, body, "/v1/changes", (j) => {
    const now = T(j.at), day = 86400e3, d0 = new Date(now); d0.setHours(0, 0, 0, 0);
    const days = Array.from({ length: 11 }, (_, k) => d0.getTime() + (k - 3) * day);
    const at = (c) => T(c.executed) || T(c.window_start);
    put(cal, h("div", { class: "no-days" }, days.map((t) => {
      const items = j.changes.filter((c) => at(c) >= t && at(c) < t + day).sort((a, b) => at(a) - at(b));
      const d = new Date(t);
      return h("div", { class: "no-day" + (t === d0.getTime() ? " today" : "") }, h("div", { class: "dh" }, h("span", {}, t === d0.getTime() ? "Today" : d.toLocaleDateString("en-US", { weekday: "short", month: "short", day: "numeric" })), h("span", {}, items.length ? String(items.length) : "")),
        h("div", { class: "db" }, items.map((c) => h("div", { class: `no-chg ${c.risk}${j.collisions[c.id] ? " clash" : ""}${at(c) < now ? " past" : ""}`, onclick: () => changeDrawer(app, c.id, j.collisions[c.id]) }, h("div", { class: "t" }, c.title),
          h("div", { class: "muted" }, `${hhmm(at(c))} · ${c.devices.slice(0, 2).join(", ")}`), j.collisions[c.id] ? h("div", { style: { color: "var(--crit-ink)" } }, `collides with ${j.collisions[c.id].map((x) => x.with).join(", ")}`) : c.status === "failed" || c.status === "rolled_back" ? h("div", { style: { color: "var(--crit-ink)" } }, c.status.replace("_", " ")) : null))));
    })));
    put(list, C.table([
      { key: "id", label: "Change", render: (r) => h("span", { class: "mono small" }, r.id) },
      { key: "title", label: "What", render: (r) => h("span", { class: "ellipsis", style: { display: "block", maxWidth: "340px" }, title: r.title }, r.title) },
      { key: "risk", label: "Risk", render: (r) => riskPill(app, r.risk), sort: (r) => ({ high: 3, medium: 2, low: 1 }[r.risk]) },
      { key: "status", label: "Status", render: (r) => C.pill({ completed: "ok", approved: "info", scheduled: "info", failed: "bad", rolled_back: "warn" }[r.status] || "info", r.status.replace("_", " ")) },
      { key: "window_start", label: "Window", render: (r) => C.timeEl(T(r.window_start), { sec: false }), sort: (r) => T(r.window_start) },
      { key: "owner", label: "Owner" },
      { key: "outcome", label: "Outcome", render: (r) => r.outcome || h("span", { class: "muted" }, "–") },
    ], { rows: j.changes, compact: true, sortKey: "window_start", desc: true, onRow: (r) => changeDrawer(app, r.id, j.collisions[r.id]), maxHeight: "520px" }));
    const clash = Object.entries(j.collisions);
    const seen = new Set();
    const pairs = clash.flatMap(([id, xs]) => xs.map((x) => ({ a: id, b: x.with, same: x.same_device }))).filter((p) => { const k = [p.a, p.b].sort().join(); if (seen.has(k)) return false; seen.add(k); return true; });
    const status = {};
    for (const o of j.outcomes) status[o.status] = (status[o.status] || 0) + o.n;
    put(side, 
      C.card({ title: "Collisions", sub: "windows that overlap on the same or neighbouring devices", body: pairs.length ? h("ul", { class: "no-list" }, pairs.map((p) => h("li", {}, h("div", {}, h("a", { class: "no-link mono", onclick: () => changeDrawer(app, p.a, j.collisions[p.a]) }, p.a), " and ", h("a", { class: "no-link mono", onclick: () => changeDrawer(app, p.b, j.collisions[p.b]) }, p.b)), h("div", { class: "small", style: { color: p.same ? "var(--crit-ink)" : "var(--text-2)" } }, p.same ? "the same device in the same window" : "two planes of one site in the same window")))) : C.empty("okc", "No collisions") }),
      C.card({ title: "How changes ended", sub: "this month", body: app.charts.barList(Object.entries(status).sort((a, b) => b[1] - a[1]).map(([k, n]) => ({ name: k.replace("_", " "), value: n, color: { completed: "var(--good)", failed: "var(--crit)", rolled_back: "var(--warn)" }[k] || "var(--s1)" })), { fmtv: C.fmt.n }) }),
      C.card({ title: "Why it is a table", body: h("div", { class: "small dim" }, "Most outages start with a change. Each one here lists its devices as an integer array, so \"which changes touched these routers in the hour before the alert\" is one indexed set operator (&&), and the root-cause agent asks it on every incident.") }));
  }, 30000);
  return stop;
}

async function changeDrawer(app, id, clashes) {
  const { h, C } = app;
  const body = h("div", {}, h("div", { class: "muted" }, "Loading…")), riskEl = h("div");
  const btn = h("button", { class: "btn primary" }, C.icon("agent"), "Assess the risk");
  const acts = h("span", { class: "row", style: { gap: "8px" } });
  C.drawer({ title: id, sub: "A change request", wide: true, body: [body, riskEl], foot: h("div", { class: "row between", style: { width: "100%" } }, btn, acts) });
  const doit = async (action, ok) => { try { await app.ctx.api(`/v1/changes/${id}/${action}`, { body: { who: WHO }, timeout: 60000 }); C.toast(ok, "ok"); load(); app.poll?.(); } catch (e) { C.toast(e.message, "bad", 7000); } };
  const showRisk = (run) => {
    const r = run.result || {};
    const color = r.score >= 70 ? "var(--crit)" : r.score >= 40 ? "var(--warn)" : "var(--good)";
    put(riskEl, h("div", { class: "no-sec" }, "Risk, from the change-risk agent"),
      h("div", { class: "row", style: { gap: "16px", alignItems: "center" } }, h("div", { class: "no-score", style: { color } }, String(r.score ?? "–")), h("div", {}, h("div", { class: "no-verdict", style: { color } }, r.verdict || ""), h("div", { class: "small muted" }, "out of 100"))),
      h("ul", { class: "no-why" }, (r.reasons || []).map((x) => h("li", {}, h("b", {}, `+${x.points} `), x.text))),
      facts(app, [["services on these devices", String((r.services || []).length)], ["customers attached", String(r.customers ?? 0)], ["neighbours", (r.near || []).join(", ") || "none"], (r.collisions || []).length ? ["collides with", r.collisions.map((c) => c.id || c.with || c).join(", ")] : null, (r.incidents || []).length ? ["open incidents nearby", r.incidents.map((c) => c.id || c).join(", ")] : null]),
      (r.n1 || []).length ? h("div", { class: "small", style: { marginTop: "8px", color: "var(--crit-ink)" } }, r.n1.map((x) => h("div", {}, typeof x === "string" ? x : x.text || `${x.link}: its other plane would run at ${pct(x.if_other_fails ?? x.util)}`))) : null,
      h("div", { class: "no-sec" }, "Steps"), steps(app, run.step_list || []));
  };
  btn.onclick = async () => { btn.disabled = true; try { showRisk(await app.ctx.api(`/v1/changes/${id}/risk`, { body: {}, timeout: 120000 })); } catch (e) { C.toast(e.message, "bad"); } btn.disabled = false; };
  const load = async () => { try {
    const j = await app.call(`/v1/changes/${id}`);
    const c = j.change;
    put(acts, c.status === "approved" || c.status === "scheduled" ? h("button", { class: "btn primary", title: "Records the start, silences its devices until the window ends", onclick: () => doit("start", "Started: its devices are silenced until the window ends") }, "Start the change")
      : c.status === "in_progress" ? [h("button", { class: "btn", onclick: () => doit("verify", "Compared with the state before it began") }, "Verify now"), h("button", { class: "btn", style: { color: "var(--crit-ink)" }, onclick: () => doit("rollback", "Rolled back") }, "Roll back"), h("button", { class: "btn primary", onclick: () => doit("complete", "Completed") }, "Complete")] : null);
    put(body, h("div", { class: "no-cause" }, c.title), h("div", { class: "dim", style: { margin: "6px 0 10px" } }, c.description),
      (clashes || []).length ? h("div", { class: "banner", style: { marginBottom: "10px", borderColor: "var(--crit)" } }, C.icon("warn"), ` Its window overlaps ${clashes.map((x) => x.with + (x.same_device ? " on the same device" : " on the other plane of the same site")).join(", ")}.`) : null,
      kv(app, [["Status", c.status.replace("_", " ")], ["Risk declared", c.risk], ["Kind", c.kind], ["Window", `${C.when(T(c.window_start), { sec: false })} to ${hhmm(T(c.window_end))}`], ["Executed", c.executed ? C.when(T(c.executed)) : "not yet"], c.started ? ["Started", `${C.when(T(c.started))} by ${c.started_by || "someone"}`] : null, ["Owner", c.owner], ["Approved by", c.approver], ["Devices", c.devices.join(", ")], ["Services", (c.services || []).join(", ")], ["Ticket", c.ticket], ["Outcome", c.outcome], ["Roll back with", c.rollback_plan]]),
      checksBlock(app, j.checks),
      c.diff ? [h("div", { class: "no-sec" }, "What it changes"), diffBlock(app, c.diff)] : null,
      j.incidents.length ? [h("div", { class: "no-sec" }, "Incidents tied to it"), h("ul", { class: "no-list" }, j.incidents.map((i) => h("li", {}, app.link(`${i.id}: ${i.title}`, "incidents", i.id))))] : null,
      j.configs.length ? [h("div", { class: "no-sec" }, "Configuration versions it wrote"), h("ul", { class: "no-list" }, j.configs.map((v) => h("li", { class: "row between" }, h("a", { class: "no-link", onclick: () => deviceDrawer(app, v.device, v.version) }, `${v.device} version ${v.version}`), h("span", { class: "small muted" }, `${v.author} · ${C.when(T(v.ts))} · ${v.lines} lines`))))] : null);
    if (j.risk_run) showRisk(j.risk_run);
  } catch (e) { put(body, C.failure(e, "The change")); } };
  await load();
}

// ---------------------------------------------------------------- services

function servicesPage(body, app) {
  const { h, C, charts } = app;
  const kp = h("div", { class: "no-kpis" }), grid = h("div", { class: "no-mini" });
  body.append(kp, grid);
  const stop = live(app, body, "/v1/services", (j) => {
    const tot = Object.fromEntries(j.totals.map((t) => [t.service, t]));
    const points = by(j.points, "service");
    const hit = new Set(j.open.flatMap((i) => i.services));
    let breach = 0, burnWorst = 0;
    const cards = [...j.services].sort((a, b) => a.tier - b.tier || a.name.localeCompare(b.name)).map((s) => {
      const t = tot[s.id] || { errors: 0, requests: 1, p95: 0, worst: 0 };
      const ok = 1 - t.errors / Math.max(1, t.requests), budget = 1 - s.slo_target;
      const burn = (1 - ok) / budget;
      if (ok < s.slo_target) breach++;
      burnWorst = Math.max(burnWorst, burn);
      const ps = points[s.id] || [];
      const rate = ps.map((p) => p.errors / Math.max(1, p.requests) * 100);
      const bad = ok < s.slo_target || hit.has(s.id);
      return h("section", { class: "card", style: { margin: 0, cursor: "pointer", borderColor: hit.has(s.id) ? "var(--crit)" : null }, onclick: () => serviceDrawer(app, s.id) }, h("div", { class: "bd" },
        h("div", { class: "row between nw" }, h("b", { class: "ellipsis" }, s.name), hit.has(s.id) ? C.pill("bad", "in an incident") : ok < s.slo_target ? C.pill("warn", "objective missed") : C.pill("ok", "ok")),
        h("div", { class: "small muted" }, `tier ${s.tier} · ${s.owner} · ${s.customers} customers · ${money(s.revenue_per_min)}/min`),
        h("div", { class: "row between", style: { margin: "10px 0 4px", alignItems: "baseline" } }, h("span", { style: { fontSize: "22px", fontWeight: 660, color: ok < s.slo_target ? "var(--crit-ink)" : null } }, (ok * 100).toFixed(3) + "%"), h("span", { class: "small muted" }, `objective ${(s.slo_target * 100).toFixed(2)}%`)),
        h("div", { class: "meter", title: "Error budget used in this window" }, h("i", { style: { width: Math.min(100, burn * 100) + "%", background: burn >= 1 ? "var(--crit)" : burn >= 0.6 ? "var(--warn)" : "var(--good)" } })),
        h("div", { class: "row between small muted", style: { marginTop: "3px" } }, h("span", {}, `${Math.round(Math.min(9.99, burn) * 100)}% of the error budget used`), h("span", { style: t.worst > s.latency_slo_ms ? { color: "var(--crit-ink)" } : null }, `p95 ${t.p95.toFixed(0)} ms, worst ${t.worst.toFixed(0)} of ${s.latency_slo_ms}`)),
        h("div", { style: { marginTop: "8px" } }, charts.sparkline(rate, { width: 300, height: 34, color: bad ? "var(--crit)" : "var(--s3)", cls: "spark no-wide" })),
        h("div", { class: "small muted" }, "error rate, last hour")));
    });
    put(grid, ...cards);
    const req = j.totals.reduce((a, t) => a + t.requests, 0), err = j.totals.reduce((a, t) => a + t.errors, 0);
    put(kp, kpi(app, "Services", String(j.services.length), `${j.services.filter((s) => s.tier === 1).length} are tier 1`), kpi(app, "Missing their objective", String(breach), `over the last ${j.hours} hours`, breach ? { style: { borderColor: "var(--crit)" } } : {}),
      kpi(app, "In an open incident", String(hit.size), hit.size ? [...hit].slice(0, 3).join(", ") : "none"), kpi(app, "Requests", C.fmt.si(req), `last ${j.hours} hours`), kpi(app, "Success overall", ((1 - err / Math.max(1, req)) * 100).toFixed(3) + "%", `${C.fmt.si(err)} errors`), kpi(app, "Worst budget burn", Math.round(burnWorst * 100) + "%", "of a window's error budget"));
  }, 20000);
  return stop;
}

async function serviceDrawer(app, id) {
  const { h, C, charts } = app;
  const body = h("div", {}, h("div", { class: "muted" }, "Loading…"));
  C.drawer({ title: id, sub: "A service", wide: true, body });
  try {
    const j = await app.call(`/v1/services/${id}`);
    const s = j.service;
    const rate = charts.timeChart({ height: 140, fmt: (v) => v.toFixed(2) + "%", threshold: { value: (1 - s.slo_target) * 100, label: "error budget" }, series: [{ name: "Error rate", color: "var(--crit)", area: true, get: (p) => p.errors / Math.max(1, p.requests) * 100, fmt: (v) => v.toFixed(3) + "%" }] });
    const lat = charts.timeChart({ height: 140, fmt: (v) => v.toFixed(0) + " ms", threshold: { value: s.latency_slo_ms, label: "objective" }, series: [{ name: "p95", color: "var(--s1)", get: (p) => p.latency_p95, fmt: (v) => v.toFixed(0) + " ms" }, { name: "p50", color: "var(--s3)", get: (p) => p.latency_p50, fmt: (v) => v.toFixed(0) + " ms" }] });
    const reqs = charts.timeChart({ height: 110, fmt: C.fmt.si, series: [{ name: "Requests a minute", color: "var(--s2)", area: true, get: (p) => p.requests, fmt: C.fmt.si }] });
    put(body, h("div", { class: "no-cause" }, s.name), h("div", { class: "dim", style: { margin: "4px 0 10px" } }, s.description),
      facts(app, [["tier", String(s.tier)], ["owner", s.owner], ["objective", (s.slo_target * 100).toFixed(2) + "%"], ["latency objective", s.latency_slo_ms + " ms"], ["revenue", money(s.revenue_per_min) + "/min"], ["sites", s.sites.join(", ").toUpperCase()], ["devices it rides on", String(s.devices.length)]]),
      h("div", { class: "no-sec" }, `Error rate, last ${j.hours} hours`), rate, h("div", { class: "no-sec" }, "Latency"), lat, h("div", { class: "no-sec" }, "Requests"), reqs,
      h("div", { class: "no-sec" }, "Incidents that touched it"), j.incidents.length ? h("ul", { class: "no-list" }, j.incidents.map((i) => h("li", { class: "row between nw" }, app.link(i.title, "incidents", i.id), h("span", { class: "small muted", style: { whiteSpace: "nowrap" } }, `${C.fmt.ago(T(i.started))} · ${i.status === "resolved" ? mins(i.mttr_min) : i.status}`)))) : h("div", { class: "dim" }, "None."),
      h("div", { class: "no-sec" }, `Customers (${j.customers.length})`), C.table([{ key: "name", label: "Customer" }, { key: "tier", label: "Tier" }, { key: "site", label: "Site", render: (r) => r.site.toUpperCase() }, { key: "mrr", label: "A month", num: true, render: (r) => money(r.mrr) }], { rows: j.customers, compact: true, sortKey: "mrr", desc: true, maxHeight: "260px" }));
    const p = pts(j.series);
    rate.update(p); lat.update(p); reqs.update(p);
  } catch (e) { put(body, C.failure(e, "The service")); }
}

// ---------------------------------------------------------------- explore: logs, flows, search

function explorePage(body, app, path) {
  const { h, C, charts } = app;
  const tab = ["logs", "flows", "search"].includes(path[1]) ? path[1] : "logs";
  const seg = C.seg([["logs", "Logs"], ["flows", "Flows"], ["search", "Search everything"]], tab, (v) => app.go("explore", v));
  const page = h("div");
  body.append(h("div", { style: { marginBottom: "12px" } }, seg), page);
  if (tab === "logs") {
    let mode = "substring", minutes = 60, filt = {};
    const input = h("input", { class: "input mono", type: "search", placeholder: "Any piece of a line: an interface, an error code, half an IP address", "aria-label": "Search the logs" });
    const modeSeg = C.seg([["substring", "Substring", "Any run of characters, from the substring index (LIKE '%…%' without a scan)"], ["words", "Words", "Ranked by BM25"]], mode, (v) => { mode = v; stop.run(); });
    const win = h("select", { class: "select", "aria-label": "Window", onchange: () => { minutes = +win.value; stop.run(); } }, [[15, "15 minutes"], [60, "1 hour"], [360, "6 hours"], [1440, "1 day"]].map(([v, t]) => h("option", { value: v, selected: v === 60 || null }, t)));
    const out = h("div"), chipsEl = h("div", { class: "row", style: { gap: "6px", marginBottom: "10px" } });
    const tries = ["LLDP_NEIGHBOR", "Rx loss of signal", "et-0/0/", "prefix-set", "scheduler slip", "198.51."];
    input.addEventListener("keydown", (e) => { if (e.key === "Enter") stop.run(); });
    page.append(h("div", { class: "no-search" }, input, modeSeg, win, h("button", { class: "btn primary", onclick: () => stop.run() }, C.icon("search"), "Search")),
      h("div", { class: "row small muted", style: { gap: "6px", marginBottom: "10px" } }, "Try", tries.map((t) => h("button", { class: "btn xs", onclick: () => { input.value = t; stop.run(); } }, t))), chipsEl, out);
    const hist = charts.timeChart({ height: 110, fmt: C.fmt.n, series: [{ name: "Lines a minute", color: "var(--s1)", area: true, get: (p) => p.n, fmt: C.fmt.n }] });
    const linesEl = h("div"), sideEl = h("div", { class: "no-col" }), sub = h("span", { class: "sub" });
    out.append(h("div", { class: "no-cols wide" }, h("div", { class: "no-col" }, C.card({ title: "Lines a minute", sub, body: hist }), C.card({ title: "Lines", body: linesEl })), sideEl));
    const setF = (k, v) => { if (v == null) delete filt[k]; else filt[k] = v; stop.run(); };
    const stop = live(app, page, () => { const p = new URLSearchParams({ mode, minutes, ...filt }); if (input.value.trim()) p.set("q", input.value.trim()); return "/v1/logs?" + p; }, (j) => {
      const q = (app.queries.get("/v1/logs") || [])[0];
      sub.textContent = `${C.fmt.n(j.histogram.reduce((a, r) => a + r.n, 0))} lines match${q ? ` · found in ${C.fmt.ms(q.ms)}` : ""}`;
      put(chipsEl, ...Object.entries(filt).map(([k, v]) => h("span", { class: "chip" }, `${k}: ${v} `, h("a", { class: "no-link", onclick: () => setF(k, null) }, "×"))));
      hist.update(pts(j.histogram, "t"));
      put(linesEl, logLines(app, j.lines, mode === "substring" ? j.q : null));
      put(sideEl, 
        C.card({ title: "Templates", sub: "what kind of line", body: j.templates.length ? h("ul", { class: "no-list" }, j.templates.slice(0, 12).map((t) => h("li", { class: "row between nw" }, h("a", { class: "no-link mono small ellipsis", onclick: () => setF("template", t.template) }, t.template), h("b", { class: "num small" }, C.fmt.n(t.n))))) : C.empty("logs", "None") }),
        C.card({ title: "Devices", body: j.devices.length ? charts.barList(j.devices.slice(0, 8).map((d) => ({ name: d.device, value: d.n })), { fmtv: C.fmt.n }) : C.empty("logs", "None") }),
        C.card({ title: "New templates", sub: "not seen in the six hours before", body: j.new_templates.length ? h("ul", { class: "no-list" }, j.new_templates.slice(0, 8).map((t) => h("li", { class: "row between nw" }, h("a", { class: "no-link mono small ellipsis", onclick: () => setF("template", t.template) }, t.template), h("span", { class: "small muted" }, `${t.n} · first ${hhmm(T(t.first))}`)))) : h("div", { class: "dim small" }, "Nothing new: the same kinds of line as before.") }));
    }, 20000);
    return stop;
  }
  if (tab === "flows") {
    const ch = charts.timeChart({ height: 170, fmt: (v) => C.fmt.bytes(v), stack: true, series: [{ name: "TCP", color: "var(--s1)", get: (p) => p.tcp, fmt: C.fmt.bytes }, { name: "UDP", color: "var(--s7)", get: (p) => p.udp, fmt: C.fmt.bytes }] });
    const a = h("div"), b = h("div"), c = h("div"), d = h("div");
    page.append(h("div", { class: "no-col" }, C.card({ title: "Traffic by protocol", sub: "sampled flows, bytes a minute, last hour", body: ch }),
      h("div", { class: "no-cols even" }, C.card({ title: "Who sends the most", sub: "source networks", body: a }), C.card({ title: "What it is", sub: "applications", body: b })),
      h("div", { class: "no-cols even" }, C.card({ title: "Where it lands", sub: "destination sites", body: c }), C.card({ title: "UDP targets", sub: "prefixes taking the most UDP: a flood shows here first", body: d, flush: true }))));
    return live(app, page, "/v1/flows", (j) => {
      const u = Object.fromEntries(j.udp.map((r) => [r.t, r.bytes]));
      ch.update(j.tcp.map((r) => ({ t: T(r.t), tcp: r.bytes, udp: u[r.t] || 0 })));
      put(a, charts.barList(j.sources.map((r) => ({ name: `${r.src_name} (AS${r.src_asn})`, value: r.bytes })), { fmtv: C.fmt.bytes }));
      put(b, h("div", { class: "row", style: { gap: "20px", alignItems: "center" } }, charts.donut(j.apps.map((r) => ({ name: r.app, value: r.bytes })), { size: 140, fmtv: C.fmt.bytes }), h("div", { class: "grow" }, charts.barList(j.apps.map((r) => ({ name: r.app, value: r.bytes })), { fmtv: C.fmt.bytes }))));
      put(c, charts.barList(j.sites.map((r) => ({ name: r.dst_site.toUpperCase(), value: r.bytes })), { fmtv: C.fmt.bytes }));
      put(d, C.table([{ key: "dst_net", label: "Prefix", render: (r) => h("span", { class: "mono small" }, r.dst_net) }, { key: "dst_site", label: "Site", render: (r) => r.dst_site.toUpperCase() }, { key: "networks", label: "Source networks", num: true }, { key: "bytes", label: "UDP", num: true, render: (r) => C.fmt.bytes(r.bytes) }], { rows: j.targets, compact: true }));
    }, 20000);
  }
  // One box over everything.
  const input = h("input", { class: "input", type: "search", placeholder: "A symptom, a device, a config line, a customer: \"sessions flap after policy change\"", "aria-label": "Search everything" });
  const out = h("div");
  const tries = ["routes dropped after maintenance", "optical power degrading", "cr1-lon", "prefix-set", "amplification attack", "TEL-STO-WAW-0412"];
  const run = async () => {
    const q = input.value.trim();
    if (!q) { put(out, C.empty("search", "One box, seven tables", "Runbooks, incidents and tickets are ranked by words and by meaning, fused. Configurations and logs are matched by substring. Devices by name.")); return; }
    put(out, h("div", { class: "muted" }, "Searching…"));
    try {
      const j = await app.call("/v1/search?q=" + encodeURIComponent(q));
      const ms = (app.queries.get("/v1/search") || []).reduce((a, x) => a + x.ms, 0);
      const sec = (title, sub, rows, f) => rows.length ? C.card({ title, sub: `${rows.length} · ${sub}`, body: rows.slice(0, 6).map(f) }) : null;
      const hit = (title, text, meta, onclick) => h("div", { class: "no-hit" }, h("a", { class: "no-link", onclick }, title), text ? h("div", { class: "x" }, text) : null, meta ? h("div", { class: "small muted" }, meta) : null);
      const cards = [
        sec("Devices", "by name", j.named.concat(j.devices), (r) => hit(r.id, null, `${r.role} at ${(r.site || "").toUpperCase()}`, () => deviceDrawer(app, r.id))),
        sec("Runbooks", "words and meaning", j.runbooks, (r) => hit(r.title, r.body, `${r.id} · ${KIND[r.category] || r.category}`, () => C.drawer({ title: r.title, sub: r.id, wide: true, body: h("div", { style: { whiteSpace: "pre-wrap", lineHeight: 1.55 } }, r.body) }))),
        sec("Incidents", "words and meaning", j.incidents, (r) => hit(r.title, r.root_cause, `${r.id} · ${KIND[r.category] || r.category} · ${C.fmt.ago(T(r.started))}`, () => app.go("incidents", r.id))),
        sec("Tickets", "words and meaning", j.tickets, (r) => hit(r.title, r.resolution, `${r.id} · ${r.status} · ${C.fmt.ago(T(r.opened))}`, null)),
        sec("Configurations", "substring", j.configs, (r) => hit(`${r.device} version ${r.version}`, null, `${r.change_id || "baseline"} · ${C.when(T(r.ts))}`, () => deviceDrawer(app, r.device, r.version))),
        sec("Log lines", "substring", j.lines, (r) => hit(r.device, r.message, C.when(T(r.ts)), () => deviceDrawer(app, r.device))),
      ].filter(Boolean);
      put(out, h("div", { class: "small muted", style: { marginBottom: "10px" } }, `${j.semantic ? "Words and meaning" : "Words only (no embedding model on this server)"} · ${(app.queries.get("/v1/search") || []).length} queries · ${C.fmt.ms(ms)}`), cards.length ? h("div", { class: "no-cols even" }, h("div", { class: "no-col" }, cards.filter((_, k) => k % 2 === 0)), h("div", { class: "no-col" }, cards.filter((_, k) => k % 2 === 1))) : C.empty("search", "Nothing matches"));
    } catch (e) { put(out, C.failure(e, "Search")); }
  };
  input.addEventListener("keydown", (e) => { if (e.key === "Enter") run(); });
  page.append(h("div", { class: "no-search", style: { gridTemplateColumns: "minmax(0, 1fr) auto" } }, input, h("button", { class: "btn primary", onclick: run }, C.icon("search"), "Search")),
    h("div", { class: "row small muted", style: { gap: "6px", marginBottom: "12px" } }, "Try", tries.map((t) => h("button", { class: "btn xs", onclick: () => { input.value = t; run(); } }, t))), out);
  if (path[2]) input.value = decodeURIComponent(path[2]);
  run();
  return () => {};
}

// ---------------------------------------------------------------- devices and links

function devicesPage(body, app) {
  const { h, C, charts } = app;
  let q = "", last = null;
  const input = h("input", { class: "input", type: "search", placeholder: "Filter: a name, a site, a role, a vendor, an OS version", "aria-label": "Filter devices", oninput: () => { q = input.value.toLowerCase(); draw(); } });
  const tbl = h("div"), side = h("div", { class: "no-col" });
  body.append(h("div", { class: "no-cols wide" }, C.card({ title: "Fleet", tools: input, body: tbl, flush: true }), side));
  function draw() {
    if (!last) return;
    const m = Object.fromEntries(last.metrics.map((x) => [x.device, x]));
    const rows = last.devices.map((d) => ({ ...d, ...(m[d.id] || {}) })).filter((d) => !q || [d.id, d.site, d.role, d.vendor, d.model, d.os, d.os_version, ...(d.tags || [])].join(" ").toLowerCase().includes(q));
    const hot = (v, w, c) => h("span", { style: v >= c ? { color: "var(--crit-ink)", fontWeight: 600 } : v >= w ? { color: "var(--serious)" } : null }, v == null ? "–" : v.toFixed(0));
    put(tbl, C.table([
      { key: "id", label: "Device", render: (r) => h("b", { class: "mono small" }, r.id) },
      { key: "status", label: "State", render: (r) => r.up === 0 || r.status === "down" ? C.pill("bad", "down") : C.pill("ok", "up") },
      { key: "site", label: "Site", render: (r) => r.site.toUpperCase() }, { key: "role", label: "Role" },
      { key: "model", label: "Hardware", render: (r) => `${r.vendor} ${r.model}` },
      { key: "os_version", label: "Software", render: (r) => `${r.os} ${r.os_version}` },
      { key: "cpu", label: "CPU %", num: true, render: (r) => hot(r.cpu, 70, 90) }, { key: "mem", label: "Mem %", num: true, render: (r) => hot(r.mem, 80, 92) }, { key: "temp_c", label: "°C", num: true, render: (r) => hot(r.temp_c, 60, 70) },
      { key: "sessions", label: "BGP", num: true },
    ], { rows, compact: true, onRow: (r) => deviceDrawer(app, r.id), sortKey: "cpu", desc: true }));
    const count = (key) => Object.entries(last.devices.reduce((a, d) => { a[d[key]] = (a[d[key]] || 0) + 1; return a; }, {})).sort((a, b) => b[1] - a[1]).map(([name, value]) => ({ name, value }));
    put(side, 
      C.card({ title: "Software versions", sub: "one GROUP BY over the inventory", body: charts.barList(last.versions.map((v) => ({ name: `${v.os} ${v.os_version}`, value: v.n })), { fmtv: C.fmt.n }) }),
      C.card({ title: "By role", body: h("div", { class: "row", style: { gap: "18px", alignItems: "center" } }, charts.donut(count("role"), { size: 130, center: String(last.devices.length), sub: "devices", fmtv: C.fmt.n }), h("div", { class: "grow" }, charts.barList(count("role"), { fmtv: C.fmt.n }))) }),
      C.card({ title: "By vendor", body: charts.barList(count("vendor"), { fmtv: C.fmt.n }) }));
  }
  return live(app, body, "/v1/devices", (j) => { last = j; draw(); }, 20000);
}

async function deviceDrawer(app, id, version) {
  const { h, C, charts } = app;
  const body = h("div", {}, h("div", { class: "muted" }, "Loading…"));
  C.drawer({ title: id, sub: "A device", wide: true, body });
  try {
    const j = await app.call(`/v1/devices/${id}`);
    const d = j.device;
    const cpu = charts.timeChart({ height: 130, fmt: (v) => v.toFixed(0) + "%", max: 100, series: [{ name: "CPU", color: "var(--s1)", get: (p) => p.cpu, fmt: (v) => v.toFixed(0) + "%" }, { name: "Memory", color: "var(--s3)", get: (p) => p.mem, fmt: (v) => v.toFixed(0) + "%" }] });
    const now = Object.fromEntries(j.link_now.map((l) => [l.link, l]));
    const blameEl = h("div");
    const actEl = h("div", {}, h("div", { class: "muted small" }, "Reading…"));
    const showBlame = async (v) => {
      put(blameEl, h("div", { class: "muted" }, "Loading…"));
      try {
        const b = await app.call(`/v1/devices/${id}/blame` + (v ? `?version=${v}` : ""));
        put(blameEl, h("div", { class: "row between", style: { marginBottom: "8px" } }, C.seg(b.versions.map((x) => [x.version, `v${x.version}`, `${x.change} · ${x.author}`]), b.version, showBlame), h("span", { class: "small muted" }, "green lines are new in this version; each line names the change that wrote it")),
          h("div", { class: "no-blame" }, b.lines.map((l) => h("div", { class: "bl" + (l.new ? " new" : "") + (l.change !== "baseline" ? " chg" : "") }, h("span", { class: "who", title: `${l.author}, ${l.ts}` }, `${l.change} ${l.author}`), h("span", { class: "ln" }, l.n), h("span", {}, l.text)))),
          b.removed.length ? h("div", { class: "small", style: { marginTop: "6px", color: "var(--crit-ink)" } }, `Removed in this version: ${b.removed.map((r) => (r.text || r).trim()).join(" · ")}`) : null);
      } catch (e) { put(blameEl, C.failure(e, "The configuration")); }
    };
    put(body, 
      h("div", { class: "row", style: { gap: "8px", marginBottom: "8px" } }, d.status === "down" ? C.pill("bad", "down") : C.pill("ok", "up"), h("span", { class: "chip" }, d.role), h("span", { class: "chip" }, d.site.toUpperCase()), (d.tags || []).map((t) => h("span", { class: "chip" }, t))),
      kv(app, [["Hardware", `${d.vendor} ${d.model}`], ["Software", `${d.os} ${d.os_version}`], ["Serial", d.serial], ["Loopback", d.loopback], ["Management", d.mgmt_ip], ["AS", String(d.asn)], ["Installed", C.fmt.ago(T(d.installed))], ["Services", j.services.map((s) => s.name).join(", ")]]),
      j.advisories.length ? h("div", { class: "banner", style: { margin: "10px 0", borderColor: "var(--warn)" } }, C.icon("shield"), " ", j.advisories.map((a) => h("div", {}, h("b", {}, a.id), ` (${a.severity}, CVSS ${a.cvss}): ${a.title}. Fixed in ${a.fixed_in}.`))) : null,
      j.alerts.length ? [h("div", { class: "no-sec" }, "Alerts"), h("ul", { class: "no-list" }, j.alerts.slice(0, 6).map((a) => h("li", { class: "row between nw" }, h("span", { class: "ellipsis" }, a.summary), h("span", { class: "row nw", style: { gap: "6px" } }, a.incident ? app.link(a.incident, "incidents", a.incident) : null, a.status === "firing" ? sevPill(app, a.severity) : h("span", { class: "small muted" }, "cleared")))))] : null,
      h("div", { class: "no-sec" }, `CPU and memory, last ${j.hours} hours`), cpu,
      h("div", { class: "no-sec" }, "Links"), C.table([{ key: "id", label: "Link", render: (r) => h("a", { class: "no-link mono small", onclick: () => linkDrawer(app, r.id) }, r.id) }, { key: "a_if", label: "Port", render: (r) => (r.src === id ? r.a_if : r.z_if) }, { key: "capacity_mbps", label: "Size", num: true, render: (r) => cap(r.capacity_mbps) },
        { key: "util", label: "Load", num: true, render: (r) => (now[r.id]?.up === 0 ? h("b", { style: { color: "var(--crit-ink)" } }, "down") : pct(now[r.id]?.util)), sort: (r) => now[r.id]?.util }, { key: "rx", label: "Rx dBm", num: true, render: (r) => now[r.id]?.rx_dbm?.toFixed(1) ?? "–", sort: (r) => now[r.id]?.rx_dbm }], { rows: j.links, compact: true }),
      j.bgp.length ? [h("div", { class: "no-sec" }, "BGP sessions"), C.table([{ key: "peer_name", label: "Peer" }, { key: "peer_asn", label: "AS", num: true }, { key: "kind", label: "Kind" }, { key: "state", label: "State", render: (r) => r.state === "established" ? C.pill("ok", "established") : C.pill("bad", r.state) }, { key: "prefixes_rx", label: "Prefixes", num: true, render: (r) => C.fmt.n(r.prefixes_rx) }, { key: "since", label: "Since", render: (r) => C.fmt.ago(T(r.since)) }], { rows: j.bgp, compact: true })] : null,
      j.customers.length ? [h("div", { class: "no-sec" }, "Customers attached here"), h("div", { class: "small" }, j.customers.map((c) => c.name).join(", "))] : null,
      j.changes.length ? [h("div", { class: "no-sec" }, "Changes"), h("ul", { class: "no-list" }, j.changes.slice(0, 5).map((c) => h("li", { class: "row between nw" }, h("a", { class: "no-link ellipsis", onclick: () => changeDrawer(app, c.id) }, `${c.id}: ${c.title}`), h("span", { class: "small muted", style: { whiteSpace: "nowrap" } }, `${c.status} · ${C.when(T(c.window_start), { sec: false })}`))))] : null,
      h("div", { class: "no-sec" }, "What happened here, last day"), actEl,
      h("div", { class: "no-sec" }, `Configuration (${j.configs.length} versions kept)`), blameEl,
      h("div", { class: "no-sec" }, "Latest log lines"), logLines(app, j.logs.slice(0, 40).map((l) => ({ ...l, device: "" }))));
    cpu.update(pts(j.series));
    app.ctx.api("/v1/activity?id=" + encodeURIComponent(id)).then((a) => put(actEl, a.events.length ? h("ul", { class: "no-list" }, a.events.slice(0, 12).map((x) => h("li", { class: "row between nw", style: x.kind === "incident" || x.kind === "change" ? { cursor: "pointer" } : null, onclick: x.kind === "incident" ? () => app.go("incidents", x.ref) : x.kind === "change" ? () => changeDrawer(app, x.ref) : null },
      h("span", { class: "ellipsis" }, h("span", { class: "chip", style: { marginRight: "8px" } }, x.kind), x.title, h("span", { class: "small muted" }, "  " + x.sub)), h("span", { class: "small muted", style: { whiteSpace: "nowrap" } }, C.fmt.ago(T(x.ts)))))) : h("div", { class: "dim" }, "Nothing: no alert, change, configuration write or burst of errors."))).catch(() => put(actEl, h("div", { class: "dim" }, "Could not read the activity.")));
    if (j.configs.length) showBlame(version);
    else put(blameEl, h("div", { class: "dim" }, "No configuration kept for this device."));
  } catch (e) { put(body, C.failure(e, "The device")); }
}

async function linkDrawer(app, id) {
  const { h, C, charts } = app;
  const body = h("div", {}, h("div", { class: "muted" }, "Loading…"));
  C.drawer({ title: id, sub: "A link", wide: true, body });
  try {
    const j = await app.call(`/v1/links/${encodeURIComponent(id)}`);
    const l = j.link, names = [id, j.other].filter(Boolean);
    const mk = (get, fmtv, extra = {}) => charts.timeChart({ height: 130, fmt: fmtv, ...extra, series: names.map((n, k) => ({ name: k ? `${n} (the other plane)` : n, color: k ? "var(--s3)" : "var(--s1)", dash: k ? "4 3" : null, get: (p) => p[n], fmt: fmtv })) });
    const util = mk((p) => p.util, (v) => pct(v), { max: 1, threshold: { value: 0.92, label: "saturated", showInScale: false } }), loss = mk((p) => p.loss, (v) => v.toFixed(2) + "%"), rx = charts.timeChart({ height: 120, fmt: (v) => (v - 40).toFixed(0) + " dBm", max: 40, series: [{ name: "Receive power", color: "var(--s7)", area: true, get: (p) => p.rx, fmt: (v) => (v - 40).toFixed(1) + " dBm" }] });
    const fold = (get) => { const at = new Map(); for (const r of j.series) { const t = T(r.ts); const p = at.get(t) || { t }; p[r.link] = get(r); if (r.link === id) p.rx = Math.max(0, (r.up ? r.rx_dbm : -40) + 40); at.set(t, p); } return [...at.values()].sort((a, b) => a.t - b.t); };
    const c = j.circuit, s = j.seen;
    put(body, 
      h("div", { class: "row", style: { gap: "8px", marginBottom: "8px" } }, l.status === "down" ? C.pill("bad", "down") : C.pill("ok", "up"), h("span", { class: "chip" }, l.kind), l.plane ? h("span", { class: "chip" }, "plane " + l.plane) : null, h("span", { class: "chip" }, cap(l.capacity_mbps))),
      kv(app, [["From", h("a", { class: "no-link", onclick: () => deviceDrawer(app, l.src) }, `${l.src} ${l.a_if}`)], ["To", h("a", { class: "no-link", onclick: () => deviceDrawer(app, l.dst) }, `${l.dst} ${l.z_if}`)], ["Latency by design", l.latency_ms != null ? l.latency_ms + " ms" : null], j.other ? ["Its other plane", h("a", { class: "no-link", onclick: () => linkDrawer(app, j.other) }, j.other)] : null]),
      c ? [h("div", { class: "no-sec" }, "The circuit"), kv(app, [["Circuit", c.id], ["Provider", c.provider], ["Size", cap(c.capacity_mbps)], ["Costs", money(c.monthly_cost) + " a month"], ["Contract", `${c.contract_months_left} months left`], ["Promised", c.sla + "%"], ["Status", c.status]])] : null,
      s ? [h("div", { class: "no-sec" }, "In the logs"), h("div", { class: "small" }, s.state === "confirmed" ? C.pill("ok", "confirmed") : C.pill("warn", s.state), ` ${s.evidence} neighbour lines, last heard ${C.fmt.ago(T(s.last_seen))}. `, h("a", { class: "no-link", onclick: () => evidenceDrawer(app, l.src, l.dst, s.state) }, "Evidence"))] : [h("div", { class: "no-sec" }, "In the logs"), h("div", { class: "small", style: { color: "var(--crit-ink)" } }, "Neither end has ever logged the other as a neighbour. ", h("a", { class: "no-link", onclick: () => evidenceDrawer(app, l.src, l.dst, "silent") }, "Evidence"))],
      h("div", { class: "no-sec" }, `Load, last ${j.hours} hours`), util, h("div", { class: "no-sec" }, "Packet loss"), loss, h("div", { class: "no-sec" }, "Receive power"), rx,
      j.alerts.length ? [h("div", { class: "no-sec" }, "Alerts"), h("ul", { class: "no-list" }, j.alerts.slice(0, 6).map((a) => h("li", { class: "row between nw" }, h("span", { class: "ellipsis" }, a.summary), h("span", { class: "row nw", style: { gap: "6px" } }, a.incident ? app.link(a.incident, "incidents", a.incident) : null, h("span", { class: "small muted" }, a.status === "firing" ? "firing" : C.fmt.ago(T(a.started)))))))] : null);
    util.update(fold((r) => (r.up ? r.util : null))); loss.update(fold((r) => r.loss)); rx.update(fold(() => null));
  } catch (e) { put(body, C.failure(e, "The link")); }
}

// ---------------------------------------------------------------- agents

function agentsPage(body, app) {
  const { h, C } = app;
  const top = h("div", { class: "no-kpis" }), cards = h("div", { class: "no-mini", style: { gridTemplateColumns: "repeat(auto-fit, minmax(340px, 1fr))" } }), pend = h("div"), results = h("div", { class: "no-col" }), runs = h("div");
  body.append(top, cards, h("div", { class: "no-cols", style: { marginTop: "14px" } }, h("div", { class: "no-col" }, results, C.card({ title: "Runs", sub: "a click shows the steps and the SQL of each", body: runs, flush: true })), h("div", { class: "no-col" }, pend)));
  const ondemand = { capacity: true, compliance: true, topology: true };
  const busy = new Set();
  const start = async (id) => {
    busy.add(id); stop.run();
    try { const r = await app.ctx.api(`/v1/agents/${id}/run`, { body: {}, timeout: 180000 }); C.toast(r.summary || "Done", "ok", 7000); } catch (e) { C.toast(e.message, "bad", 6000); }
    busy.delete(id); stop.run();
  };
  const stop = live(app, body, "/v1/agents", (j) => {
    const st = Object.fromEntries(j.stats.map((s) => [s.agent, s]));
    const judged = j.verdicts.reduce((a, v) => a + v.n, 0), ok = (j.verdicts.find((v) => v.verdict === "correct") || {}).n || 0;
    const all = j.stats.reduce((a, s) => ({ runs: a.runs + s.runs, ms: a.ms + s.ms, rows: a.rows + s.rows_read }), { runs: 0, ms: 0, rows: 0 });
    const auto = C.toggle(j.autopilot, async (on) => { try { await app.ctx.api("/v1/sim/settings", { body: { autopilot: on } }); C.toast(on ? "Autopilot on: low-risk actions run without waiting" : "Autopilot off: every action waits for approval", "ok"); stop.run(); } catch (e) { C.toast(e.message, "bad"); } }, "Autopilot");
    put(top, kpi(app, "Diagnoses checked", judged ? `${ok}/${judged}` : "–", "against the simulator's answer key"), kpi(app, "Runs", C.fmt.n(all.runs), "since the app started"), kpi(app, "Rows read", C.fmt.si(all.rows), "by every run together"),
      kpi(app, "Time in the database", C.fmt.ms(all.ms), "for all of them"), kpi(app, "Language model calls", "0", "the agents are queries and rules"), h("section", { class: "card kpi" }, h("div", { class: "lbl" }, "Low-risk actions"), h("div", { style: { marginTop: "8px" } }, auto), h("div", { class: "no-kpi-sub" }, j.autopilot ? "run on their own" : "wait for approval")));
    put(cards, ...j.agents.map((a) => {
      const s = st[a.id];
      const btn = ondemand[a.id] ? h("button", { class: "btn sm", disabled: busy.has(a.id) || null, onclick: () => start(a.id) }, C.icon("play"), busy.has(a.id) ? "Running…" : "Run now") : null;
      return h("section", { class: "card", style: { margin: 0 } }, h("div", { class: "bd no-agent" }, h("div", {}, h("h4", {}, a.name), h("div", { class: "small dim" }, a.what),
        facts(app, [["runs", a.when.toLowerCase()], s ? ["runs so far", String(s.runs)] : null, s ? ["mean time", C.fmt.ms(s.ms / Math.max(1, s.runs))] : null, s ? ["last", C.fmt.ago(T(s.last))] : null])), h("div", {}, btn)));
    }));
    const dec = (j.decided || []).slice(0, 6);
    put(pend, C.card({ title: "Waiting for approval", sub: `${j.pending.length} proposed`, body: j.pending.length ? j.pending.map((a) => actionCard(app, a)) : C.empty("okc", "Nothing waits", "The root-cause agent proposes actions when an incident opens.") }),
      dec.length ? C.card({ title: "Decided", body: h("ul", { class: "no-list" }, dec.map((a) => h("li", {}, h("div", { class: "row between nw" }, h("span", { class: "ellipsis" }, a.title), C.pill({ executed: "ok", done: "ok", approved: "ok", rejected: "disabled", expired: "disabled" }[a.status] || "info", a.status === "expired" ? "not needed" : a.status === "executed" ? "carried out" : a.status)), h("div", { class: "small muted" }, `${a.incident}${a.decided_by ? " · " + a.decided_by : ""}${a.result ? " · " + a.result : ""}`)))) }) : null,
      C.card({ title: "What an agent is here", body: h("div", { class: "small dim stack" }, h("div", {}, "Each agent is a fixed plan of queries against the tables: a graph walk for the blast radius, a substring search for known signatures, set operators for changes, hybrid search for similar incidents, and PACK to fit a brief into a token budget with a receipt of what was left out."), h("div", {}, "No language model is called, so a diagnosis costs milliseconds and can be replayed. The brief it packs is what you would hand a model for the write-up."), h("div", {}, `Graph steps run ${j.graph === "sql" ? "in SQL (REACHABLE, ppr, PATH)" : "in the app, because a coordinator does not walk graphs yet"}.`)) }));
    const r = j.results || {};
    const cap_ = r.capacity?.value, comp = r.compliance?.value, topo = r.topology?.value;
    put(results, 
      cap_ ? C.card({ title: "Capacity", sub: `${cap_.summary} · ${C.fmt.ago(T(r.capacity.at))}`, flush: true, body: C.table([
        { key: "link", label: "Link", render: (x) => h("a", { class: "no-link mono small", onclick: (e) => { e.stopPropagation(); linkDrawer(app, x.link); } }, x.link) }, { key: "capacity_gbps", label: "Size", num: true, render: (x) => x.capacity_gbps + "G" },
        { key: "p95", label: "p95", num: true, render: (x) => pct(x.p95) }, { key: "peak", label: "Peak", num: true, render: (x) => pct(x.peak) },
        { key: "if_other_fails", label: "If its pair fails", num: true, render: (x) => h("b", { style: x.if_other_fails >= 1 ? { color: "var(--crit-ink)" } : x.if_other_fails > 0.9 ? { color: "var(--serious)" } : null }, x.if_other_fails == null ? "–" : pct(x.if_other_fails)) },
        { key: "provider", label: "Circuit", render: (x) => (x.circuit ? `${x.provider}, ${money(x.monthly_cost)}/mo, ${x.months_left} mo left` : "–") }, { key: "finding", label: "Finding" },
      ], { rows: cap_.result?.findings || [], compact: true, sortKey: "if_other_fails", desc: true, onRow: () => runDrawer(app, cap_.id) }) }) : null,
      comp ? C.card({ title: "Advisories and drift", sub: `${comp.summary} · ${C.fmt.ago(T(r.compliance.at))}`, tools: h("button", { class: "btn xs", onclick: () => runDrawer(app, comp.id) }, "Steps"), body: [
        h("ul", { class: "no-list" }, (comp.result?.exposed || []).map((x) => h("li", {}, h("div", { class: "row between nw" }, h("b", { class: "ellipsis", title: x.title }, x.title), C.pill(x.severity === "critical" || x.severity === "high" ? "bad" : "warn", `${x.severity} · CVSS ${x.cvss}`)),
          h("div", { class: "small muted" }, `${x.advisory} · ${x.os}, fixed in ${x.fixed_in} · ${x.devices.length} devices: `, x.devices.slice(0, 8).map((d, k) => [k ? ", " : "", h("a", { class: "no-link", onclick: () => deviceDrawer(app, d.id) }, d.id)]), x.devices.length > 8 ? ` and ${x.devices.length - 8} more` : "")))),
        (comp.result?.drift || []).length ? [h("div", { class: "no-sec" }, "Configuration drift"), h("ul", { class: "no-list" }, comp.result.drift.map((x) => h("li", {}, h("b", {}, x.check), h("div", { class: "small muted" }, `${x.detail} · ${x.devices.join(", ")}`))))] : null] }) : null,
      topo ? C.card({ title: "Topology from logs", sub: C.fmt.ago(T(r.topology.at)), tools: app.link("See the map", "topology"), body: h("div", {}, topo.summary) }) : null,
      !cap_ && !comp ? C.card({ title: "Findings", body: C.empty("agent", "Run the capacity or the advisories agent", "Their findings stay here until the next run.") }) : null);
    put(runs, j.runs.length ? runsTable(app, j.runs, { maxHeight: "420px" }) : C.empty("agent", "No runs yet"));
  }, 12000);
  return stop;
}

// ---------------------------------------------------------------- data

function dataPage(body, app) {
  const { h, C } = app;
  const kp = h("div", { class: "no-kpis" }), src = h("div"), tbl = h("div"), side = h("div", { class: "no-col" });
  body.append(kp, h("div", { class: "no-cols wide" }, h("div", { class: "no-col" }, C.card({ title: "Where the rows come from", sub: "each source says whether its rows are simulated or arriving, and what writes them on a real network", body: src, flush: true }), C.card({ title: "Tables", sub: "all of them ordinary ChloeDB tables: query them from the SQL page or any client", body: tbl, flush: true })), side));
  const stop = live(app, body, "/v1/data", (j) => {
    const rows = j.tables.reduce((a, t) => a + (t.rows || 0), 0), minute = Object.values(j.last_minute_rows).reduce((a, n) => a + n, 0), written = Object.values(j.written).reduce((a, n) => a + n, 0);
    put(kp, kpi(app, "Tables", String(j.tables.length), "prefixed netops_"), kpi(app, "Rows", C.fmt.si(rows), "across all of them"), kpi(app, "Written a minute", C.fmt.n(minute), "telemetry, logs and flows"), kpi(app, "Written since start", C.fmt.si(written), C.fmt.ago(T(j.since))),
      kpi(app, "Search by meaning", j.semantic ? "on" : "off", j.semantic ? j.model : "no embedding model on this server"), kpi(app, "Graph walks", j.graph === "sql" ? "in SQL" : "in the app", j.server.role === "coordinator" ? "a coordinator does not walk graphs yet" : "REACHABLE, ppr and PATH"));
    put(src, C.table([
      { key: "name", label: "Source", render: (r) => h("b", {}, r.name) }, { key: "kind", label: "Kind" }, { key: "via", label: "On a real network", sortable: false, render: (r) => h("span", { class: "ellipsis", style: { display: "block", maxWidth: "300px" }, title: r.via }, r.via) },
      { key: "table_name", label: "Table", render: (r) => h("span", { class: "mono small" }, r.table_name) }, { key: "rate", label: "Rows/min", num: true, render: (r) => C.fmt.n(r.rate) }, { key: "lag_s", label: "Lag", num: true, render: (r) => (r.lag_s == null ? "–" : C.fmt.dur(r.lag_s)) },
      { key: "retention", label: "Kept", render: (r) => h("span", { style: { whiteSpace: "nowrap" } }, r.retention) }, { key: "status", label: "State", render: (r) => C.pill(r.status === "simulated" ? "info" : r.status === "ok" || r.status === "live" ? "ok" : "warn", r.status) },
    ], { rows: j.sources, compact: true, onRow: (r) => C.drawer({ title: r.name, sub: r.via, wide: true, body: [h("div", { class: "dim" }, r.note), kv(app, [["Writes to", r.table_name], ["Last row", C.when(T(r.last_seen))], ["Kept for", r.retention]]), r.snippet ? [h("div", { class: "no-sec" }, "How to connect it"), h("pre", { class: "code", style: { whiteSpace: "pre-wrap", fontSize: "12px" } }, r.snippet)] : null] }) }));
    put(tbl, C.table([
      { key: "table", label: "Table", render: (r) => h("b", { class: "mono small" }, r.table) }, { key: "what", label: "Holds", sortable: false }, { key: "rows", label: "Rows", num: true, render: (r) => C.fmt.n(r.rows) },
      { key: "telemetry", label: "", sortable: false, render: (r) => (r.telemetry ? h("span", { class: "badge accent" }, "written every minute") : null) },
    ], { rows: j.tables, compact: true, sortKey: "rows", desc: true, onRow: (r) => C.drawer({ title: r.table, sub: r.what, wide: true, body: [h("div", { class: "no-sec" }, "Definition"), C.sqlBlock(r.ddl), h("div", { class: "no-sec" }, "Try"), C.sqlBlock(`SELECT * FROM ${r.table} LIMIT 20`)] }) }));
    const sim = app.status?.sim || {};
    const every = h("select", { class: "select", onchange: async () => { try { await app.ctx.api("/v1/sim/settings", { body: { fault_every: +every.value } }); C.toast("Saved", "ok"); app.poll(); } catch (e) { C.toast(e.message, "bad"); } } }, [[0, "Only when I inject one"], [15, "Every 15 minutes"], [30, "Every 30 minutes"], [45, "Every 45 minutes"], [90, "Every 90 minutes"]].map(([v, t]) => h("option", { value: v, selected: v === sim.fault_every || null }, t)));
    put(side, 
      C.card({ title: "The simulator", body: h("div", { class: "stack" }, h("div", { class: "small dim" }, sim.simulate ? "A carrier network that does not exist: ten sites, two planes, twelve services. Every minute the simulator writes what its devices would report. A fault changes what they report; the rules and the agents only ever see the tables." : "The simulator is off (--no-simulate): the tables are only read."),
        sim.simulate ? h("label", { class: "stack", style: { gap: "4px" } }, h("span", { class: "small dim" }, "Faults"), every) : null,
        sim.simulate ? h("div", { class: "stack", style: { gap: "6px" } }, (sim.scenarios || []).map((s) => h("div", { class: "row between nw" }, h("span", { class: "small" }, h("b", {}, s.name), h("div", { class: "muted" }, s.what)), h("button", { class: "btn xs", disabled: sim.fault_active || null, onclick: () => inject(app, s) }, "Inject")))) : null) }),
      C.card({ title: "Written in the last minute", body: app.charts.barList(Object.entries(j.last_minute_rows).sort((a, b) => b[1] - a[1]).map(([k, n]) => ({ name: k, value: n })), { fmtv: C.fmt.n }) }),
      C.card({ title: "The server", body: kv(app, [["Address", j.server.url], ["Role", j.server.role], ["History seeded", j.history_hours + " hours of telemetry, a month of incidents"], ["Embedding model", j.model || "none"]]) }));
  }, 20000);
  return stop;
}

// ---------------------------------------------------------------- the shift

const WHO = "You";
const utc = (iso) => (iso ? iso.slice(11, 16) : "");
const dayUtc = (iso) => (iso ? new Date(Date.parse(iso)).toLocaleDateString("en-US", { weekday: "short", month: "short", day: "numeric", timeZone: "UTC" }) : "");
const windowText = (a, b) => `${dayUtc(a)} ${utc(a)} to ${utc(b)} UTC`;
const WORK = { approval: ["Approve", "bolt"], incident: ["Incident", "alerts"], change: ["Change", "clock"], maintenance: ["Carrier", "route"], alert: ["Alert", "alerts"], ticket: ["Ticket", "logs"], silence: ["Silence", "clock"], note: ["Note", "copy"] };

/** Opens what a worklist row or a lookup hit points at. */
function openThing(app, kind, id) {
  if (kind === "device" || kind === "bgp") return deviceDrawer(app, id);
  if (kind === "link" || kind === "circuit" || kind === "interface") return linkDrawer(app, id);
  if (kind === "service") return serviceDrawer(app, id);
  if (kind === "change") return changeDrawer(app, id);
  if (kind === "incident") return app.go("incidents", id);
  if (kind === "customer") return app.go("customers", id);
  if (kind === "maintenance") return app.go("carriers");
  if (kind === "site") return app.go("topology");
  if (kind === "logs") return app.go("explore", "logs");
  return null;
}

function shiftPage(body, app) {
  const { h, C } = app;
  const brief = h("div"), work = h("div"), side = h("div", { class: "no-col" });
  const note = h("textarea", { class: "input", rows: 2, placeholder: "Leave a note for the next shift: what to watch, what not to touch, who to call", "aria-label": "A note for the next shift" });
  const addNote = async () => {
    if (!note.value.trim()) return;
    try { await app.ctx.api("/v1/shift/notes", { body: { who: WHO, body: note.value.trim() } }); note.value = ""; C.toast("Left for the next shift", "ok"); stop.run(); } catch (e) { C.toast(e.message, "bad"); }
  };
  body.append(h("div", { class: "no-cols wide" }, h("div", { class: "no-col" }, brief, C.card({ title: "What waits for you", sub: "most urgent first; each row is a row of a table, and a click away from where it is handled", body: work, flush: true })), side));
  const act = async (path, okText) => { try { await app.ctx.api(path, { body: { who: WHO }, timeout: 60000 }); C.toast(okText, "ok"); stop.run(); app.poll(); } catch (e) { C.toast(e.message, "bad", 6000); } };
  const stop = live(app, body, "/v1/shift", (j) => {
    const lead = j.oncall.filter((o) => o.role === "primary");
    const urgent = j.work.filter((w) => w.urgency === 1).length;
    put(brief, h("div", { class: "no-hero" + (j.counts.open || urgent ? "" : " calm") },
      h("div", {}, h("div", { class: "row", style: { gap: "8px", marginBottom: "6px" } }, C.pill(j.counts.open ? "bad" : urgent ? "warn" : "ok", j.counts.open ? `${j.counts.open} open incident${j.counts.open === 1 ? "" : "s"}` : urgent ? `${urgent} thing${urgent === 1 ? "" : "s"} need you now` : "Quiet"), h("span", { class: "small muted" }, `Handover · the last twelve hours, as of ${utc(j.at)} UTC`)),
        h("h2", {}, j.briefing[0]),
        h("ul", { class: "no-why", style: { marginTop: "4px" } }, j.briefing.slice(1).map((l) => h("li", {}, l))),
        h("div", { class: "meta" }, h("span", {}, "on call now ", h("b", {}, lead.map((o) => `${o.person} (${o.team})`).slice(0, 3).join(", ") || "nobody")), j.before.length ? h("span", {}, "before them ", h("b", {}, [...new Set(j.before.map((o) => o.person))].slice(0, 3).join(", "))) : null)),
      h("div", { style: { textAlign: "right" } }, h("button", { class: "btn", onclick: () => C.copy(j.briefing.join("\n"), "Handover copied") }, C.icon("copy"), "Copy the handover"))));
    put(work, j.work.length ? h("div", {}, j.work.map((w) => {
      const [label] = WORK[w.kind] || [w.kind];
      const go = w.go ? () => (w.go.id ? (w.go.page === "changes" ? changeDrawer(app, w.go.id) : app.go(w.go.page, ...(w.go.page === "carriers" ? [] : [w.go.id]))) : app.go(w.go.page)) : null;
      const btns = [];
      if (w.act?.alert) btns.push(h("button", { class: "btn xs", onclick: (e) => { e.stopPropagation(); act(`/v1/alerts/${encodeURIComponent(w.act.alert)}/ack`, "Acknowledged: it is yours now"); } }, "Acknowledge"));
      if (w.act?.note) btns.push(h("button", { class: "btn xs", onclick: (e) => { e.stopPropagation(); act(`/v1/shift/notes/${encodeURIComponent(w.act.note)}/done`, "Marked as read"); } }, "Read"));
      if (go) btns.push(h("button", { class: "btn xs primary", onclick: (e) => { e.stopPropagation(); go(); } }, w.kind === "approval" ? "Review" : "Open"));
      return h("div", { class: "no-work u" + w.urgency, onclick: go || null, style: go ? { cursor: "pointer" } : null },
        h("span", { class: "k" }, label), h("div", { class: "b" }, h("div", { class: "t" }, w.title), h("div", { class: "s" }, w.sub)), h("div", { class: "row nw", style: { gap: "6px" } }, btns));
    })) : C.empty("okc", "Nothing waits", "No approvals, no unacknowledged alerts, no notices to read. The next change and the next carrier window are on the right."));
    const ahead = [...j.next.map((c) => ({ t: c.window_start, kind: "change", id: c.id, title: c.title, sub: `${c.risk} risk · ${c.owner} · ${c.devices.slice(0, 2).join(", ")}`, hot: c.status === "in_progress" })),
      ...j.notices.filter((n) => T(n.window_start) < T(j.at) + 36 * 3600e3).map((n) => ({ t: n.window_start, kind: "maintenance", id: n.id, title: `${n.provider}: ${n.impact} on ${n.circuit}`, sub: `until ${utc(n.window_end)} UTC${j.conflicts[n.id] ? " · collides" : ""}`, hot: !!j.conflicts[n.id] }))].sort((a, b) => T(a.t) - T(b.t));
    put(side,
      C.card({ title: "Coming up", sub: "next day and a half", body: ahead.length ? h("ul", { class: "no-list" }, ahead.map((x) => h("li", { style: { cursor: "pointer" }, onclick: () => openThing(app, x.kind, x.id) },
        h("div", { class: "row between nw" }, h("b", { class: "ellipsis", style: x.hot ? { color: "var(--crit-ink)" } : null }, x.title), h("span", { class: "small muted", style: { whiteSpace: "nowrap" } }, `${dayUtc(x.t).split(",")[0]} ${utc(x.t)}`)), h("div", { class: "small muted" }, `${x.id} · ${x.sub}`)))) : C.empty("clock", "Nothing planned") }),
      C.card({ title: "Notes for this shift", body: [j.notes.length ? h("ul", { class: "no-list", style: { marginBottom: "10px" } }, j.notes.map((n) => h("li", { style: n.done ? { opacity: 0.55 } : null }, h("div", { class: "small muted" }, `${n.author} · ${C.fmt.ago(T(n.ts))}${n.done ? " · read" : ""}`), h("div", {}, n.body)))) : null, note, h("div", { style: { marginTop: "6px", textAlign: "right" } }, h("button", { class: "btn sm", onclick: addNote }, "Leave the note"))] }),
      C.card({ title: "On call now", body: h("ul", { class: "no-list" }, j.oncall.filter((o) => o.role === "primary").map((o) => h("li", { class: "row between" }, h("span", {}, h("b", {}, o.person), h("span", { class: "muted small" }, ` · ${o.team}`)), h("span", { class: "mono small" }, o.contact)))) }),
      j.flaps.length ? C.card({ title: "Kept alerting", sub: "three times or more in twelve hours", body: h("ul", { class: "no-list" }, j.flaps.map((f) => h("li", { class: "row between" }, h("span", {}, `${f.rule_name} on `, h("span", { class: "mono small" }, f.entity)), h("b", {}, `${f.n}×`)))) }) : null,
      C.card({ title: "Open tickets", body: j.tickets.length ? h("ul", { class: "no-list" }, j.tickets.slice(0, 6).map((t) => h("li", {}, h("div", { class: "row between nw" }, h("span", { class: "ellipsis" }, t.title), C.pill(t.priority <= "P2" ? "warn" : "info", t.priority)), h("div", { class: "small muted" }, `${t.id} · ${t.status} · ${t.assignee}`)))) : C.empty("okc", "None open") }));
  }, 20000);
  return stop;
}

/** The box in the header: whatever a ticket or a phone call names. */
async function lookupDrawer(app, q) {
  const { h, C } = app;
  const body = h("div", {}, h("div", { class: "muted" }, "Looking…"));
  const d = C.drawer({ title: q, sub: "Looked up in the inventory, the customers, the sessions and the logs", body });
  try {
    const j = await app.call("/v1/lookup?q=" + encodeURIComponent(q));
    const state = (x) => !x ? null : C.pill(x.state === "bad" ? "bad" : x.state === "warn" ? "warn" : "ok", x.state === "bad" ? (x.incident || "incident") : x.state === "warn" ? `${x.alerts} alert${x.alerts === 1 ? "" : "s"}` : "healthy");
    put(body, j.hits.length ? j.hits.map((x) => h("div", { class: "no-hit", style: { cursor: "pointer" }, onclick: () => { d.close?.(); openThing(app, x.kind, x.id); } },
      h("div", { class: "row between nw" }, h("span", {}, h("span", { class: "chip", style: { marginRight: "8px" } }, x.kind), h("b", {}, x.title)), state(x.health)), h("div", { class: "x" }, x.sub)))
      : C.empty("search", "Nothing in the network is called that", "Try a device, an address, a circuit id, a customer's name, an interface or an AS number."),
      h("div", { style: { marginTop: "14px" } }, h("button", { class: "btn sm", onclick: () => { d.close?.(); app.go("explore", "search", q); } }, C.icon("search"), "Search runbooks, incidents, tickets and logs for it")));
  } catch (e) { put(body, C.failure(e, "The lookup")); }
}

// ---------------------------------------------------------------- where is it broken?

const STATE_COLOR = { ok: "var(--good)", warn: "var(--warn)", bad: "var(--crit)", none: "transparent" };

function diagnosePage(body, app, path) {
  const { h, C, charts } = app;
  const hero = h("div"), grid = h("div"), listEl = h("div"), inv = h("div"), side = h("div", { class: "no-col" });
  body.append(hero, h("div", { class: "no-cols wide" }, h("div", { class: "no-col" }, C.card({ title: "The stack, by place", sub: "customers at the top, power at the bottom: a fault shows on its own layer and on every layer above it", body: [grid, listEl] }), inv), side));
  let sel = null, picked = null, last = null, auto = true;
  const openIt = (o) => o.kind === "link" ? linkDrawer(app, o.id) : o.kind === "device" ? deviceDrawer(app, o.id) : o.kind === "service" ? serviceDrawer(app, o.id) : app.go("customers", o.id);

  // One broken thing, asked five ways of the same database.
  const investigate = async (o) => {
    picked = o;
    if (o.kind === "customer") { app.go("customers", o.id); return; }
    put(inv, C.card({ title: `Investigating ${o.id}`, body: h("div", { class: "muted" }, "Asking the graph, the logs, the history and the past…") }));
    let j;
    try { j = await app.call(`/v1/investigate?kind=${o.kind}&id=${encodeURIComponent(o.id)}`); } catch (e) { put(inv, C.failure(e, "The investigation")); return; }
    if (picked !== o) return;
    const L = Object.fromEntries(j.lenses.map((l) => [l.id, l]));
    const total = j.lenses.reduce((a, l) => a + l.ms, 0), nq = j.lenses.reduce((a, l) => a + l.queries, 0);
    const lens = (l, title, content) => h("section", { class: "no-lens" },
      h("header", {}, h("span", { class: "f" }, l.feature), h("span", { class: "c" }, `${l.queries} ${l.queries === 1 ? "query" : "queries"} · ${C.fmt.ms(l.ms)}`)),
      h("h4", {}, title), h("div", { class: "how" }, l.how), content);
    const g = L.graph.data, lg = L.logs.data, ch = L.changed.data, bf = L.before.data, tm = L.time.data;
    const others = lg.others || [];
    const fields = j.kind === "link" ? [["loss", "Loss", (v) => v.toFixed(2) + "%"], ["rx_dbm", "Receive power", (v) => v.toFixed(1) + " dBm"], ["util", "Load", (v) => pct(v)], ["errors", "Errors", (v) => C.fmt.n(v)]]
      : j.kind === "device" ? [["cpu", "CPU", (v) => v.toFixed(0) + "%"], ["bgp_up", "BGP sessions", (v) => String(v)], ["temp_c", "Temperature", (v) => v.toFixed(0) + " °C"]]
      : [["errors", "Failing requests", (v) => C.fmt.n(v)], ["latency_p95", "p95 latency", (v) => v.toFixed(0) + " ms"]];
    put(inv, C.card({ title: j.title, sub: j.now.length ? j.now.join(" · ") : "nothing wrong with it in the last three minutes",
      tools: h("span", { class: "row nw", style: { gap: "6px" } }, j.incident ? h("button", { class: "btn xs primary", onclick: () => app.go("incidents", j.incident) }, `Open ${j.incident}`) : null, h("button", { class: "btn xs", onclick: () => openIt(o) }, "Open it")),
      body: [
        h("div", { class: "no-lenses" },
          lens(L.graph, "What do the symptoms have in common, and what does it take down?", [
            h("div", { class: "small muted", style: { marginBottom: "4px" } }, `Ranked from the ${g.seeds} devices that alert now: the closer to all of them, the likelier the cause`),
            h("div", { class: "no-sus" }, g.suspects.map((s) => h("a", { class: "row nw", onclick: () => deviceDrawer(app, s.id) }, h("span", { class: "mono small", style: { width: "74px", fontWeight: s.this ? 700 : 400 } }, s.id), h("span", { class: "bar" }, h("i", { style: { width: Math.round(s.share * 100) + "%", background: s.this ? "var(--crit)" : s.alerting ? "var(--warn)" : "var(--seq-4)" } })), h("span", { class: "small muted", style: { width: "64px", textAlign: "right" } }, s.this ? "this" : s.alerting ? "alerting" : "quiet")))),
            facts(app, [["one hop away", String(g.near.one)], ["two hops", String(g.near.two)], ["services riding on it", String(g.services.length)], ["revenue a minute", money(g.services.reduce((a, s) => a + (s.revenue_per_min || 0), 0))]]),
            g.services.length ? h("div", { style: { marginTop: "6px" } }, chips(app, g.services.map((s) => s.name), 5)) : null,
            g.customers.length ? h("div", { class: "small", style: { marginTop: "6px" } }, h("span", { class: "muted" }, "Largest customers within a hop: "), g.customers.slice(0, 4).map((c) => `${c.name} (${money(c.mrr)})`).join(", ")) : null]),
          lens(L.logs, "What does every device's log say about it?", [
            h("div", { class: "small muted", style: { marginBottom: "6px" } }, "Lines containing ", h("code", {}, lg.needle), others.length ? ` · also written by ${others.slice(0, 4).join(", ")}, which only see it from the outside` : ""),
            lg.kinds.length ? h("div", { style: { marginBottom: "8px" } }, lg.kinds.slice(0, 6).map((k) => h("span", { class: "chip", style: { marginRight: "4px", marginBottom: "4px" }, title: `first at ${utc(k.first)} UTC` }, `${k.n} × ${k.template} on ${k.device}`))) : null,
            logLines(app, lg.lines.slice(0, 5), lg.needle.split(", ")[0])]),
          lens(L.changed, "Did anything change here?", [
            ch.changes.length ? h("ul", { class: "no-list" }, ch.changes.map((c) => h("li", { class: "row between nw", style: { cursor: "pointer" }, onclick: () => changeDrawer(app, c.id) }, h("span", { class: "ellipsis" }, h("b", {}, c.id), " ", c.title), h("span", { class: "small muted", style: { whiteSpace: "nowrap" } }, `${c.status.replace("_", " ")} · ${C.fmt.ago(T(c.executed))}`))))
              : h("div", { class: "banner", style: { borderColor: "var(--good)" } }, C.icon("okc"), " No change touched these devices in three days: a change is ruled out."),
            ch.versions.length ? [h("div", { class: "no-sec" }, "Configuration written"), h("ul", { class: "no-list" }, ch.versions.slice(0, 4).map((v) => h("li", { class: "row between nw" }, h("a", { class: "no-link", onclick: () => deviceDrawer(app, v.device, v.version) }, `${v.device} version ${v.version}`), h("span", { class: "small muted" }, `${v.author} · ${C.fmt.ago(T(v.ts))}${v.change_id ? " · " + v.change_id : ""}`))))] : null,
            ch.mentions.length ? h("div", { class: "small muted", style: { marginTop: "8px" } }, "Its interface was last configured in ", ch.mentions.map((v, i) => [i ? ", " : "", h("a", { class: "no-link", onclick: () => deviceDrawer(app, v.device, v.version) }, `${v.device} v${v.version}`), ` (${C.fmt.ago(T(v.ts))})`])) : null]),
          lens(L.before, "Has this happened before, and what fixed it?", [
            h("div", { class: "small muted", style: { marginBottom: "6px" } }, "Searched for ", h("i", {}, `“${bf.asked}”`)),
            bf.incidents.length ? h("ul", { class: "no-list" }, bf.incidents.slice(0, 3).map((i) => h("li", { style: { cursor: "pointer" }, onclick: () => app.go("incidents", i.id) }, h("div", { class: "row between nw" }, h("b", { class: "ellipsis" }, i.title), h("span", { class: "small muted", style: { whiteSpace: "nowrap" } }, `${C.fmt.ago(T(i.started))} · fixed in ${mins(i.mttr_min)}`)), h("div", { class: "small muted" }, `${i.id} · ${KIND[i.category] || i.category}${i.runbook ? " · closed with " + i.runbook : ""}`)))) : C.empty("search", "Nothing like it on record"),
            bf.runbooks.length ? h("div", { style: { marginTop: "8px" } }, h("span", { class: "small muted" }, "Runbooks: "), bf.runbooks.map((r) => h("span", { class: "chip", style: { marginRight: "4px" } }, r.title))) : null]),
          lens(L.time, "When did it start?", h("div", { class: "no-sparks" }, fields.filter(([k]) => tm.series.some((p) => p[k] != null)).map(([k, name, f]) => { const vals = tm.series.map((p) => p[k] ?? 0), lastv = vals[vals.length - 1] ?? 0, first = vals[0] ?? 0; const moved = Math.abs(lastv - first) > 1e-9; return h("div", {}, h("div", { class: "row between small" }, h("span", { class: "muted" }, name), h("b", { style: moved ? { color: "var(--crit-ink)" } : null }, f(lastv))), charts.sparkline(vals, { width: 220, height: 30, color: moved ? "var(--crit)" : "var(--s3)", cls: "spark no-wide" }), h("div", { class: "small muted" }, `an hour ago ${f(first)}`)); })))),
        h("div", { class: "no-one" }, C.icon("db"), h("span", {}, h("b", {}, `${nq} queries, ${C.fmt.ms(total)}, one database. `), "A graph walk, a substring index over the logs, array overlap, a search by words and meaning and a time series, with no second system to join against. The SQL button at the top lists every one."))] }));
  };

  const draw = (j) => {
    last = j;
    const s = j.start;
    if (s) {
      const f = s.first, colName = (j.cols.find((c) => c.id === s.col) || {}).name || s.col;
      put(hero, h("div", { class: "no-hero" + (s.state === "bad" ? "" : " warn") }, h("div", {},
        h("div", { class: "row", style: { gap: "8px", marginBottom: "4px" } }, C.pill(s.state === "bad" ? "bad" : "warn", "start here"), h("span", { class: "small muted" }, `the lowest layer where something is ${s.state === "bad" ? "broken" : "worn"} · as of ${utc(j.at)} UTC`)),
        h("h2", {}, `${s.layer_name}, ${colName}: ${f.id} ${f.reading}`),
        h("p", {}, `${f.where}${f.circuit ? ` · circuit ${f.circuit} from ${f.provider}` : ""}. `, s.above.length ? `What shows on ${s.above.join(", ")} above it most likely follows from this.` : "Nothing above it is affected yet.", f.change ? ` ${f.change.id} was carried out here ${C.fmt.ago(T(f.change.executed))}.` : "")),
        h("div", { class: "row", style: { gap: "8px" } }, f.incident ? h("button", { class: "btn", onclick: () => app.go("incidents", f.incident) }, `Open ${f.incident}`) : null, h("button", { class: "btn primary", onclick: () => { auto = false; investigate(f); } }, C.icon("search"), "Investigate"))));
      if (auto && !picked) investigate(f);
    } else put(hero, h("div", { class: "no-hero calm" }, h("div", {}, h("h2", {}, "Nothing is broken on any layer"), h("p", {}, `Every link, device and service was inside its limits in the last three minutes (as of ${utc(j.at)} UTC). Trace a path on the right, or inject a fault at the top to see this page work.`))));
    const layers = [...j.layers].reverse();
    put(grid, h("div", { class: "no-stack", style: { gridTemplateColumns: `170px repeat(${j.cols.length}, minmax(44px, 1fr))` } },
      h("span", {}), j.cols.map((c) => h("span", { class: "ch", title: c.city }, c.name)),
      layers.map((l) => [h("span", { class: "rh", title: l.what }, l.name),
        j.cols.map((c) => { const cell = j.cells[l.id][c.id]; const on = sel && sel.layer === l.id && sel.col === c.id; const start = s && s.layer === l.id && s.col === c.id;
          return h("button", { class: "cell " + cell.state + (on ? " on" : "") + (start ? " start" : ""), disabled: cell.state === "none" || !cell.n, title: cell.total ? `${l.name} at ${c.name}: ${cell.n} of ${cell.total} ${cell.n === 1 ? "is" : "are"} not right` : "nothing of this kind here", onclick: () => { sel = on ? null : { layer: l.id, col: c.id }; draw(last); } }, cell.n ? String(cell.n) : ""); })])),
      h("div", { class: "row small muted", style: { gap: "14px", marginTop: "8px" } }, ["ok", "warn", "bad"].map((k) => h("span", { class: "row nw", style: { gap: "5px" } }, h("i", { style: { width: "10px", height: "10px", borderRadius: "2px", background: STATE_COLOR[k], display: "inline-block", opacity: k === "ok" ? 0.35 : 1 } }), { ok: "inside its limits", warn: "worn", bad: "broken" }[k])), h("span", {}, "a number is how many; the outlined cell is where to start; click a cell")));
    const seen = new Set();
    const shown = j.offenders.filter((o) => (sel ? o.layer === sel.layer && o.col === sel.col : o.layer !== "customers" && o.layer !== "services" || !s)).filter((o) => { const k = o.layer + o.id; if (seen.has(k)) return false; seen.add(k); return true; });
    put(listEl, shown.length ? [h("div", { class: "no-sec", style: { marginTop: "14px" } }, sel ? `${j.layers.find((l) => l.id === sel.layer).name} at ${(j.cols.find((c) => c.id === sel.col) || {}).name}` : "What is not right, lowest layer first"),
      C.table([
        { key: "layer", label: "Layer", sortable: false, render: (r) => j.layers.find((l) => l.id === r.layer).name },
        { key: "id", label: "What", sortable: false, render: (r) => h("span", {}, h("i", { style: { display: "inline-block", width: "8px", height: "8px", borderRadius: "50%", background: STATE_COLOR[r.state], marginRight: "7px" } }), h("span", { class: "mono small" }, r.kind === "customer" || r.kind === "service" ? r.where : r.id)) },
        { key: "reading", label: "Reading", sortable: false },
        { key: "where", label: "Where", sortable: false, render: (r) => r.kind === "customer" || r.kind === "service" ? "" : r.where },
        { key: "change", label: "Changed", sortable: false, render: (r) => r.change ? h("a", { class: "no-link small", onclick: (e) => { e.stopPropagation(); changeDrawer(app, r.change.id); } }, `${r.change.id}, ${C.fmt.ago(T(r.change.executed))}`) : h("span", { class: "muted small" }, "no") },
        { key: "incident", label: "Incident", sortable: false, render: (r) => r.incident ? app.link(r.incident, "incidents", r.incident) : h("span", { class: "muted small" }, r.alert ? "alerting" : "no alert") },
      ], { rows: shown.slice(0, 14), compact: true, onRow: (r) => { auto = false; investigate(r); } })] : null);
    if (!side.childElementCount) put(side, traceCard(app, s ? s.first : null), C.card({ title: "Changes, last six hours", sub: "the first thing to rule out", body: j.changes.length ? h("ul", { class: "no-list" }, j.changes.slice(0, 6).map((c) => h("li", { style: { cursor: "pointer" }, onclick: () => changeDrawer(app, c.id) }, h("div", { class: "row between nw" }, h("b", { class: "ellipsis" }, c.title), h("span", { class: "small muted", style: { whiteSpace: "nowrap" } }, C.fmt.ago(T(c.executed)))), h("div", { class: "small muted" }, `${c.id} · ${c.devices.slice(0, 3).join(", ")}`)))) : h("div", { class: "dim" }, "None carried out.") }),
      C.card({ title: "How to read this page", body: h("div", { class: "small dim stack" }, h("div", {}, h("b", {}, "Start at the bottom. "), "A dark optic is a down link, a lost adjacency, a failing service and a customer on the phone. The lowest layer that is broken is the cause; the rest are its symptoms."),
        h("div", {}, h("b", {}, "Then ask one thing five ways. "), "Investigate walks the topology graph, finds its log lines by substring on every device, checks what changed, searches past incidents by meaning, and shows its last hour. All of it is SQL against one database."),
        h("div", {}, h("b", {}, "Or follow a path. "), "Trace walks from a customer or a device to another, hop by hop, and marks the hop where it goes wrong.")) }));
  };
  const stop = live(app, body, "/v1/where", draw, 20000);
  if (path[1] && path[2]) { auto = false; investigate({ kind: path[1], id: decodeURIComponent(path[2]) }); }
  return stop;
}

/** A path between two points of the network, hop by hop, with what each hop reads now. */
function traceCard(app, hint) {
  const { h, C } = app;
  const from = h("input", { class: "input", placeholder: "from: a customer, a device, a site", "aria-label": "Trace from" }), to = h("input", { class: "input", placeholder: "to: a device or a site", "aria-label": "Trace to" });
  const out = h("div");
  const run = async () => {
    if (!from.value.trim() || !to.value.trim()) return;
    put(out, h("div", { class: "muted small" }, "Walking the links table…"));
    try {
      const j = await app.call(`/v1/trace?from=${encodeURIComponent(from.value.trim())}&to=${encodeURIComponent(to.value.trim())}`);
      put(out, h("div", { class: "banner", style: { margin: "10px 0", borderColor: STATE_COLOR[j.state] } }, C.icon(j.state === "ok" ? "okc" : "warn"), " " + j.verdict),
        h("div", { class: "no-trace" }, j.hops.map((x) => [x.link ? h("div", { class: "lk " + x.link.state, onclick: () => linkDrawer(app, x.link.id) }, h("span", { class: "mono small" }, x.link.kind === "backbone" && x.link.circuit ? x.link.circuit : x.link.id), h("span", { class: "small muted" }, x.link.findings.length ? x.link.findings.join(", ") : `${x.link.latency_ms ?? "–"} ms · ${pct(x.link.util)} full${x.link.parallel > 1 ? ` · ${x.link.parallel} in parallel` : ""}`)) : null,
          h("div", { class: "hop " + x.state, onclick: () => deviceDrawer(app, x.device) }, h("i", {}), h("b", { class: "mono" }, x.device), h("span", { class: "small muted" }, `${(x.site || "").toUpperCase()} ${x.role}${x.findings.length && !x.link?.findings?.length ? " · " + x.findings[0] : ""}`))])),
        h("div", { class: "small muted", style: { marginTop: "6px" } }, `${j.hops.length - 1} hops, ${j.latency_ms} ms one way · ${j.engine === "sql" ? "PATH() in SQL over the links table" : "walked by the app"}`));
    } catch (e) { put(out, h("div", { class: "small", style: { color: "var(--crit-ink)", marginTop: "8px" } }, e.message)); }
  };
  for (const i of [from, to]) i.addEventListener("keydown", (e) => { if (e.key === "Enter") run(); });
  const tries = hint && hint.kind === "link" ? [[hint.id.split("--")[0], hint.id.split("--")[1]]] : [];
  tries.push(["cust-001", "nyc"], ["lon", "fra"]);
  return C.card({ title: "Trace a path", sub: "hop by hop", body: [h("div", { class: "stack", style: { gap: "6px" } }, from, to, h("div", { class: "row between" }, h("span", { class: "row small muted", style: { gap: "4px", flexWrap: "wrap" } }, "Try", tries.slice(0, 3).map(([a, b]) => h("button", { class: "btn xs", onclick: () => { from.value = a; to.value = b; run(); } }, `${a} to ${b}`))), h("button", { class: "btn sm primary", onclick: run }, "Trace"))), out] });
}

// ---------------------------------------------------------------- the incident's board

const PANEL_FMT = { "%": (v) => (v * 100).toFixed(0) + "%", pct: (v) => v.toFixed(v > 0 && v < 1 ? 2 : 0) + "%", bps: gbps, ms: (v) => v.toFixed(1) + " ms", dBm: (v) => v.toFixed(1) + " dBm", "°C": (v) => v.toFixed(0) + " °C" };

/** The output columns of a SELECT, in the order it names them. */
function selectList(sql) {
  const m = /^\s*select\s+([\s\S]*?)\s+from\s/i.exec(sql);
  if (!m) return [];
  const out = []; let depth = 0, cur = "";
  for (const ch of m[1] + ",") { if (ch === "(") depth++; if (ch === ")") depth--; if (ch === "," && depth === 0) { out.push(cur.trim()); cur = ""; } else cur += ch; }
  return out.map((x) => (/\sas\s+(\w+)$/i.exec(x) || /(\w+)$/.exec(x) || [])[1]).filter(Boolean);
}

/** The room's shared board: panels from the cause's template, and the ones people ask for. */
function boardCard(app, id) {
  const { h, C, charts } = app;
  const grid = h("div", { class: "no-board" }), hint = h("div", { class: "row small muted", style: { gap: "6px", flexWrap: "wrap", margin: "8px 0 12px" } }), sub = h("span", { class: "sub" });
  const ask = h("input", { class: "input", placeholder: "Add a panel: loss on LON-NYC last 2 hours, cpu on cr1-fra, logs \"BGP-5-ADJCHANGE\", or paste a SELECT", "aria-label": "Add a panel to the room" });
  const kept = new Map();
  let sig = "", stop = null;
  const add = async (text) => {
    const q = (text ?? ask.value).trim();
    if (!q) return;
    try { const j = await app.ctx.api(`/v1/incidents/${id}/panels`, { body: { ask: q, who: WHO } }); ask.value = ""; C.toast(`Added for everyone in the room: ${j.understood}`, "ok"); run(); } catch (e) { C.toast(e.message, "bad", 9000); }
  };
  const act = async (p, action, ok) => { try { await app.ctx.api(`/v1/incidents/${id}/panels/${encodeURIComponent(p.id)}/${action}`, { body: { who: WHO } }); if (ok) C.toast(ok, "ok"); run(); } catch (e) { C.toast(e.message, "bad"); } };
  ask.addEventListener("keydown", (e) => { if (e.key === "Enter") add(); });
  const cell = (v) => v == null ? "–" : typeof v === "number" ? (Number.isInteger(v) ? v.toLocaleString("en-US") : v.toFixed(Math.abs(v) < 10 ? 2 : 1)) : /^\d{4}-\d\d-\d\dT/.test(v) ? `${v.slice(5, 10)} ${v.slice(11, 16)}` : Array.isArray(v) ? v.join(", ") : String(v);
  const lines = (p) => {
    const names = [...new Set(p.rows.map((r) => r.series ?? "value"))].slice(0, 10);
    const at = new Map();
    // The shared chart starts its axis at zero, so readings below it (dBm) are drawn from their floor.
    let floor = 0;
    for (const r of p.rows) if (r.v != null && r.v < floor) floor = r.v;
    floor = Math.floor(floor / 5) * 5;
    for (const r of p.rows) { const t = T(r.t); if (!at.has(t)) at.set(t, { t }); at.get(t)[r.series ?? "value"] = r.v == null ? null : r.v - floor; }
    return { names, floor, points: [...at.values()].sort((a, b) => a.t - b.t) };
  };
  const draw = (j) => {
    sub.textContent = `${j.panels.length} panels, shared by everyone in the room${j.removed ? ` · ${j.removed} removed` : ""}`;
    put(hint, j.often.length ? ["Often added for this kind of incident", j.often.map((o) => h("button", { class: "btn xs", title: `Added by hand in ${o.n} other ${o.n === 1 ? "incident" : "incidents"} of this kind`, onclick: () => add(o.ask) }, C.icon("plus"), o.ask))] : ["Try", j.examples.slice(0, 5).map((x) => h("button", { class: "btn xs", onclick: () => add(x) }, x))]);
    const built = j.panels.map((p) => ({ p, d: p.kind === "line" ? lines(p) : null }));
    const now = built.map(({ p, d }) => p.id + ":" + (d ? d.names.join(",") : "t")).join("|");
    if (now !== sig) {
      sig = now; kept.clear();
      put(grid, built.map(({ p, d }) => {
        const bodyEl = h("div");
        const raw = PANEL_FMT[p.unit] || ((v) => (Math.abs(v) >= 100 || Number.isInteger(v) ? C.fmt.n(v) : String(+v.toFixed(2)))), hold = { floor: d ? d.floor : 0 }, fmtv = (v) => raw(v + hold.floor);
        const chart = d && d.names.length ? charts.timeChart({ height: 150, fmt: fmtv, series: d.names.map((n, i) => ({ name: n, color: `var(--s${(i % 8) + 1})`, get: (pt) => pt[n], fmt: fmtv })) }) : null;
        kept.set(p.id, { bodyEl, chart, hold });
        const tag = p.source === "person" ? h("span", { class: "chip", title: p.asked ? `Asked: ${p.asked}` : null }, `added by ${p.added_by}`) : p.source === "cause" ? h("span", { class: "chip", title: "Added when the cause was identified" }, "for this cause") : null;
        return h("section", { class: "no-panel" + (p.source === "person" ? " mine" : "") },
          h("header", {}, h("b", { class: "ellipsis", title: p.title }, p.title), tag,
            h("span", { class: "row nw", style: { gap: "2px", marginLeft: "auto" } },
              h("button", { class: "btn xs ghost", title: "Its query", onclick: () => C.drawer({ title: p.title, sub: `${p.rows.length} rows in ${C.fmt.ms(p.ms)}`, wide: true, body: [queryBlock(app, { what: p.title, sql: p.sql, ms: p.ms, rows: p.rows.length }), h("div", { class: "small muted", style: { marginTop: "8px" } }, ":since and :until are the incident's window, or the panel's own when it names one.")] }) }, C.icon("sql")),
              h("button", { class: "btn xs ghost", title: "Move to the top", onclick: () => act(p, "top") }, "↑"),
              h("button", { class: "btn xs ghost", title: "Remove from the room", onclick: () => act(p, "remove", "Removed from the room") }, C.icon("x")))),
          bodyEl);
      }));
    }
    for (const { p, d } of built) {
      const k = kept.get(p.id);
      if (!k) continue;
      if (p.error) put(k.bodyEl, h("div", { class: "small", style: { color: "var(--crit-ink)" } }, p.error));
      else if (!p.rows.length) put(k.bodyEl, h("div", { class: "dim small", style: { padding: "18px 0", textAlign: "center" } }, "No rows in this window"));
      else if (k.chart) { k.hold.floor = d.floor; if (!k.chart.isConnected) put(k.bodyEl, k.chart); k.chart.update(d.points); }
      else { const have = Object.keys(p.rows[0]).filter((c) => !c.startsWith("_")), want = selectList(p.sql).filter((c) => have.includes(c)); const cols = (want.length ? want : have).slice(0, 7); put(k.bodyEl, h("div", { class: "no-ptable" }, C.table(cols.map((c) => ({ key: c, label: c.replace(/_/g, " "), num: typeof p.rows[0][c] === "number", sortable: false, render: (r) => c === "mrr" ? money(r[c]) : c === "bytes" ? gbps(r[c] * 8 / 60) : cell(r[c]) })), { rows: p.rows.slice(0, 12), compact: true }))); }
    }
  };
  const el = C.card({ title: "The room's board", sub, body: [h("div", { class: "no-search", style: { gridTemplateColumns: "minmax(0, 1fr) auto", marginBottom: 0 } }, ask, h("button", { class: "btn primary", onclick: () => add() }, C.icon("plus"), "Add panel")), hint, grid] });
  let dead = false;
  const run = async () => { try { const j = await app.call(`/v1/incidents/${id}/panels`); if (!dead) draw(j); } catch (e) { console.warn(e); } };
  stop = C.every(30000, run, true);
  return { el, stop: () => { dead = true; stop(); } };
}

// ---------------------------------------------------------------- customers: is it the network?

function customersPage(body, app, path) {
  if (path[1]) return customerPage(body, app, path[1]);
  const { h, C } = app;
  let filter = "";
  const list = h("div");
  const input = h("input", { class: "input", type: "search", placeholder: "A customer, a prefix, a circuit id, a port, a site", "aria-label": "Filter customers", oninput: () => { filter = input.value.toLowerCase(); draw(); } });
  const seg = C.seg([["customers", "Customers"], ["services", "Services"]], "customers", (v) => { if (v === "services") app.go("services"); });
  body.append(h("div", { class: "no-search", style: { gridTemplateColumns: "auto minmax(0, 1fr)" } }, seg, input), C.card({ title: "Customers", sub: "open one to check the path between their port and the backbone", body: list, flush: true }));
  let rows = [];
  const draw = () => {
    const shown = rows.filter((c) => !filter || [c.name, c.prefix, c.circuit, c.port, c.site, c.device, c.id].some((v) => String(v).toLowerCase().includes(filter)));
    put(list, C.table([
      { key: "name", label: "Customer", render: (r) => h("b", {}, r.name) },
      { key: "state", label: "Their path now", render: (r) => C.pill(r.state === "bad" ? "bad" : r.state === "warn" ? "warn" : "ok", r.state === "bad" ? r.incident : r.state === "warn" ? `${r.alerts} alert${r.alerts === 1 ? "" : "s"}` : "clean"), sort: (r) => ({ bad: 2, warn: 1, ok: 0 }[r.state]) },
      { key: "tier", label: "Tier" },
      { key: "site", label: "Site", render: (r) => r.site.toUpperCase() },
      { key: "device", label: "Attached on", render: (r) => h("span", { class: "mono small" }, `${r.device} ${r.port}`) },
      { key: "prefix", label: "Prefix", render: (r) => h("span", { class: "mono small" }, r.prefix) },
      { key: "circuit", label: "Circuit", render: (r) => h("span", { class: "mono small" }, r.circuit) },
      { key: "services", label: "Buys", sortable: false, render: (r) => chips(app, r.services, 3) },
      { key: "tickets", label: "Tickets", num: true, render: (r) => r.tickets ? h("b", {}, String(r.tickets)) : h("span", { class: "muted" }, "0") },
      { key: "mrr", label: "A month", num: true, render: (r) => money(r.mrr) },
    ], { rows: shown, compact: true, sortKey: "state", desc: true, onRow: (r) => app.go("customers", r.id) }));
  };
  return live(app, body, "/v1/customers", (j) => { rows = j.customers; draw(); }, 30000);
}

function customerPage(body, app, id) {
  const { h, C, charts } = app;
  let hours = 24;
  const hero = h("div"), main = h("div", { class: "no-col" }), side = h("div", { class: "no-col" });
  const seg = C.seg([[6, "6 hours"], [24, "A day"], [72, "Three days"]], hours, (v) => { hours = v; stop.run(); });
  body.append(h("div", { class: "row between", style: { marginBottom: "12px" } }, h("div", {}, app.link("Customers", "customers"), h("span", { class: "muted" }, " / "), h("span", { class: "mono small" }, id)), h("div", { class: "row", style: { gap: "8px" } }, h("span", { class: "small muted" }, "look back"), seg)), hero, h("div", { class: "no-cols wide" }, main, side));
  const loss = charts.timeChart({ height: 150, fmt: (v) => v.toFixed(1) + "%", series: [{ name: "Worst loss on the path", color: "var(--crit)", area: true, get: (p) => p.loss, fmt: (v) => v.toFixed(2) + "%" }] });
  const load = charts.timeChart({ height: 150, fmt: (v) => v.toFixed(0) + "%", max: 100, series: [{ name: "Fullest link on the path", color: "var(--s1)", area: true, get: (p) => p.util * 100, fmt: (v) => v.toFixed(0) + "%" }] });
  const stop = live(app, body, () => `/v1/customers/${id}?hours=${hours}`, (j) => {
    const c = j.customer;
    const tone = { now: "", earlier: " warn", change: " info", clear: " calm" }[j.verdict];
    put(hero, h("div", { class: "no-hero" + tone },
      h("div", {}, h("div", { class: "row", style: { gap: "8px", marginBottom: "6px" } }, h("b", {}, c.name), h("span", { class: "chip" }, c.tier), h("span", { class: "small muted" }, "Is it the network?")),
        h("h2", {}, j.headline), j.reasons.length ? h("ul", { class: "no-why", style: { marginTop: "4px" } }, j.reasons.map((r) => h("li", {}, r))) : null,
        h("div", { class: "meta" }, h("span", {}, "attached on ", h("b", { class: "mono" }, `${c.device} ${c.port}`)), h("span", {}, "VLAN ", h("b", {}, c.vlan)), h("span", {}, "prefix ", h("b", { class: "mono" }, c.prefix)), h("span", {}, "circuit ", h("b", { class: "mono" }, c.circuit)), h("span", {}, "buys ", h("b", {}, c.services.join(", "))), h("span", {}, h("b", {}, money(c.mrr)), " a month"))),
      h("div", {}, h("button", { class: "btn", onclick: () => deviceDrawer(app, c.device) }, "Open " + c.device))));
    const stat = Object.fromEntries(j.links.map((l) => [l.link, l]));
    put(main,
      C.card({ title: "What was checked", sub: `${j.queries.length} queries over the last ${j.hours} hours`, body: [h("ul", { class: "no-check" }, j.checked.map((x) => h("li", {}, C.icon("okc"), h("span", {}, x)))),
        h("div", { class: "no-sec" }, "Their path"), h("div", { class: "no-path" }, h("span", { class: "chip" }, `${c.port}`), j.path.devices.map((d, i) => [h("span", { class: "sep" }, i === 0 ? "on" : i === 1 ? "up to" : "and"), h("a", { class: "chip no-link", onclick: () => deviceDrawer(app, d) }, d)]), h("span", { class: "sep" }, "then"), h("span", { class: "chip" }, `${j.path.links.filter((l) => !l.includes(c.device)).length} backbone links`))] }),
      h("div", { class: "no-cols even" }, C.card({ title: "Packet loss on the path", sub: "the worst link each minute", body: loss }), C.card({ title: "Load on the path", sub: "the fullest link each minute", body: load })),
      C.card({ title: "The links between them and the backbone", flush: true, body: C.table([
        { key: "link", label: "Link", render: (r) => h("a", { class: "no-link mono small", onclick: () => linkDrawer(app, r.link) }, r.link) },
        { key: "worst_loss", label: "Worst loss", num: true, render: (r) => h("span", { style: r.worst_loss > 0.2 ? { color: "var(--crit-ink)", fontWeight: 600 } : null }, r.worst_loss.toFixed(2) + "%") },
        { key: "peak", label: "Peak load", num: true, render: (r) => pct(r.peak) },
        { key: "errors", label: "Errors", num: true, render: (r) => C.fmt.n(r.errors) },
        { key: "latency", label: "Latency", num: true, render: (r) => (r.latency == null ? "–" : r.latency.toFixed(1) + " ms") },
        { key: "up", label: "Stayed up", render: (r) => C.pill(r.up ? "ok" : "bad", r.up ? "yes" : "went down") },
      ], { rows: j.path.links.map((l) => stat[l]).filter(Boolean), compact: true, sortKey: "worst_loss", desc: true }) }),
      j.logs.length ? C.card({ title: `Warnings and errors on ${c.device}`, body: logLines(app, j.logs.map((l) => ({ ...l, sev: { emergency: 0, alert: 1, critical: 2, error: 3, warning: 4 }[l.severity] ?? 5 }))) }) : null);
    loss.update(pts(j.series, "t")); load.update(pts(j.series, "t"));
    const reply = h("pre", { class: "no-reply" }, j.reply);
    put(side,
      C.card({ title: "A reply to send", sub: "edit before sending", tools: h("button", { class: "btn xs", onclick: () => C.copy(j.reply, "Reply copied") }, C.icon("copy"), "Copy"), body: reply }),
      C.card({ title: "Incidents on their path", body: j.incidents.length ? h("ul", { class: "no-list" }, j.incidents.map((i) => h("li", {}, h("div", { class: "row between nw" }, app.link(`${i.id}: ${i.title}`, "incidents", i.id), statusPill(app, i.status)), h("div", { class: "small muted" }, `${C.when(T(i.started), { sec: false })}${i.mttr_min ? ` · resolved in ${mins(i.mttr_min)}` : ""}`)))) : C.empty("okc", "None in the window") }),
      C.card({ title: "Changes on their path", sub: "done or planned", body: j.changes.length ? h("ul", { class: "no-list" }, j.changes.map((x) => h("li", { style: { cursor: "pointer" }, onclick: () => changeDrawer(app, x.id) }, h("div", { class: "row between nw" }, h("span", { class: "ellipsis" }, x.title), C.pill(x.status === "completed" ? "ok" : x.status === "rolled_back" ? "warn" : "info", x.status.replace("_", " "))), h("div", { class: "small muted" }, `${x.id} · ${windowText(x.window_start, x.window_end)}`)))) : C.empty("okc", "None") }),
      j.maintenance.length ? C.card({ title: "Carrier maintenance on their path", body: h("ul", { class: "no-list" }, j.maintenance.map((n) => h("li", {}, h("div", {}, `${n.provider}: ${n.impact} on `, h("span", { class: "mono small" }, n.circuit)), h("div", { class: "small muted" }, `${n.id} · ${windowText(n.window_start, n.window_end)} · ${n.status}`)))) }) : null,
      C.card({ title: "The services they buy", body: j.services.length ? h("ul", { class: "no-list" }, j.services.map((s) => { const ok = s.requests ? 1 - s.errors / s.requests : 1; return h("li", { class: "row between", style: { cursor: "pointer" }, onclick: () => serviceDrawer(app, s.service) }, h("span", {}, s.name), h("span", { class: "small", style: ok < s.slo ? { color: "var(--crit-ink)", fontWeight: 600 } : null }, `${(ok * 100).toFixed(3)}% of ${(s.slo * 100).toFixed(2)}%`)); })) : C.empty("okc", "No measurements") }),
      C.card({ title: "Their tickets", body: j.tickets.length ? h("ul", { class: "no-list" }, j.tickets.map((t) => h("li", {}, h("div", { class: "row between nw" }, h("span", { class: "ellipsis" }, t.title), C.pill(t.status === "closed" ? "good" : "warn", t.status)), h("div", { class: "small muted" }, `${t.id} · ${C.fmt.ago(T(t.opened))}${t.resolution ? ` · ${t.resolution}` : ""}`)))) : C.empty("okc", "None") }));
  }, 30000);
  return stop;
}

// ---------------------------------------------------------------- carriers

function carriersPage(body, app) {
  const { h, C } = app;
  const list = h("div", { class: "no-col" }), side = h("div", { class: "no-col" });
  const paste = () => {
    const ta = h("textarea", { class: "input mono", rows: 12, placeholder: "Paste the carrier's email here. It needs one of our circuit ids and a start and an end such as 2026-11-14 01:00 UTC." });
    const out = h("div", { style: { marginTop: "10px" } });
    const btn = h("button", { class: "btn primary" }, "Read it");
    const d = C.drawer({ title: "A carrier's notice", sub: "Read into a row of netops_maintenance, then checked against the other plane, the change calendar and the capacity left", body: [ta, out], foot: btn });
    btn.onclick = async () => {
      try { const n = await app.ctx.api("/v1/maintenance/parse", { body: { text: ta.value } }); C.toast(`${n.id}: ${n.provider} on ${n.circuit}, ${windowText(n.window_start, n.window_end)}`, "ok", 6000); d.close?.(); stop.run(); }
      catch (e) { put(out, h("div", { class: "banner", style: { borderColor: "var(--crit)" } }, e.message)); }
    };
  };
  body.append(h("div", { class: "row between", style: { marginBottom: "12px" } }, h("div", { class: "dim" }, "Carriers' maintenance notices, each matched to the circuit it names and checked for what it collides with."), h("button", { class: "btn sm primary", onclick: paste }, C.icon("plus"), "Paste a notice")), h("div", { class: "no-cols wide" }, list, side));
  const act = async (id, what, okText) => { try { await app.ctx.api(`/v1/maintenance/${id}/${what}`, { body: { who: WHO }, timeout: 60000 }); C.toast(okText, "ok", 6000); stop.run(); } catch (e) { C.toast(e.message, "bad", 6000); } };
  const stop = live(app, body, "/v1/maintenance", (j) => {
    const now = T(j.at);
    const ahead = j.notices.filter((n) => n.status !== "completed" && T(n.window_end) > now).sort((a, b) => T(a.window_start) - T(b.window_start));
    const past = j.notices.filter((n) => !ahead.includes(n));
    const card = (n) => {
      const cs = j.conflicts[n.id] || [];
      const raw = h("pre", { class: "no-reply", style: { display: "none", marginTop: "8px" } }, n.raw);
      const soon = T(n.window_start) - now;
      return h("div", { class: "no-act" + (cs.some((c) => c.severity === "critical") ? " crit" : cs.length ? " warnb" : "") },
        h("div", { class: "row between nw" }, h("div", {}, h("b", {}, `${n.provider}: ${n.impact} on `), h("a", { class: "no-link mono", onclick: () => linkDrawer(app, n.link) }, n.circuit), h("span", { class: "muted small" }, ` · ${n.a_site.toUpperCase()} to ${n.z_site.toUpperCase()}`)),
          h("div", { class: "row nw", style: { gap: "6px" } }, cs.length ? C.pill(cs.some((c) => c.severity === "critical") ? "bad" : "warn", `${cs.length} collision${cs.length === 1 ? "" : "s"}`) : C.pill("ok", "no collisions"), C.pill(n.status === "new" ? "warn" : "info", n.status))),
        h("div", { class: "small dim", style: { margin: "4px 0" } }, `${windowText(n.window_start, n.window_end)} · ${soon > 0 ? "starts " + C.fmt.ago(T(n.window_start)).replace(" ago", "").replace("in ", "in ") : "in its window now"} · ${n.expected} · ${n.id}, their reference ${n.reference || "none"}${n.acked_by ? ` · ${n.acked_by}` : ""}${n.change_id ? ` · planned as ${n.change_id}` : ""}`),
        cs.map((c) => h("div", { class: "no-conflict " + c.severity }, C.icon("warn"), h("span", {}, c.text))),
        h("div", { class: "row", style: { gap: "6px", marginTop: "8px" } },
          n.status === "new" ? h("button", { class: "btn xs", onclick: () => act(n.id, "ack", "Acknowledged") }, "Acknowledge") : null,
          !n.change_id ? h("button", { class: "btn xs primary", title: "Makes a change for the window, so it shows on the calendar, and silences the circuit's alerts for it", onclick: () => act(n.id, "plan", "On the calendar, and the circuit's alerts are silenced for the window") }, "Put it on the calendar and silence it") : h("button", { class: "btn xs", onclick: () => changeDrawer(app, n.change_id) }, "Open " + n.change_id),
          h("button", { class: "btn xs ghost", onclick: () => { raw.style.display = raw.style.display === "none" ? "" : "none"; } }, "The email")), raw);
    };
    put(list, C.card({ title: "Ahead", sub: `${ahead.length} window${ahead.length === 1 ? "" : "s"}, soonest first`, body: ahead.length ? ahead.map(card) : C.empty("okc", "No maintenance ahead") }),
      past.length ? C.card({ title: "Done", flush: true, body: C.table([{ key: "id", label: "Notice", render: (r) => h("span", { class: "mono small" }, r.id) }, { key: "provider", label: "Carrier" }, { key: "circuit", label: "Circuit", render: (r) => h("span", { class: "mono small" }, r.circuit) }, { key: "window_start", label: "Window", render: (r) => windowText(r.window_start, r.window_end), sort: (r) => T(r.window_start) }, { key: "impact", label: "Impact" }], { rows: past, compact: true }) }) : null);
    const per = {};
    const down = Object.fromEntries(j.down.map((d) => [d.link, d.minutes]));
    for (const c of j.circuits.filter((x) => x.status === "active")) { const p = (per[c.provider] ||= { provider: c.provider, n: 0, cost: 0, down: 0, short: 0 }); p.n++; p.cost += c.monthly_cost; p.down += down[c.link] || 0; if (c.contract_months_left <= 6) p.short++; }
    put(side, C.card({ title: "Carriers", sub: "cost and minutes down", flush: true, body: C.table([
      { key: "provider", label: "Carrier", render: (r) => h("b", {}, r.provider) }, { key: "n", label: "Circuits", num: true }, { key: "cost", label: "A month", num: true, render: (r) => money(r.cost) },
      { key: "down", label: "Down", num: true, render: (r) => r.down ? h("b", { style: { color: "var(--crit-ink)" } }, r.down + " min") : "0" },
      { key: "short", label: "Ending", num: true, render: (r) => r.short ? `${r.short}` : h("span", { class: "muted" }, "0") },
    ], { rows: Object.values(per), compact: true, sortKey: "cost", desc: true }) }),
      C.card({ title: "What is checked", body: h("div", { class: "small dim stack" }, h("div", {}, h("b", {}, "The other plane. "), "Every backbone path exists twice. Two carriers working on both planes of one path in the same night leave no direct path, and neither carrier knows about the other."), h("div", {}, h("b", {}, "The calendar. "), "A change on the router that carries the surviving plane, in the same window, is the same risk from our side."), h("div", {}, h("b", {}, "Capacity. "), "With one plane down the other carries both: its peak over the last day says whether it fits.")) }));
  }, 30000);
  return stop;
}

// ---------------------------------------------------------------- alerts: silences

function silenceDrawer(app, kind, matcher, after) {
  const { h, C } = app;
  const reason = h("input", { class: "input", placeholder: "Why: the next shift will read this" });
  const hrs = h("select", { class: "input" }, [[0.5, "30 minutes"], [1, "1 hour"], [2, "2 hours"], [4, "4 hours"], [12, "12 hours"], [24, "A day"]].map(([v, t]) => h("option", { value: v, selected: v === 2 }, t)));
  const btn = h("button", { class: "btn primary" }, "Silence");
  const d = C.drawer({ title: `Silence ${matcher}`, sub: `Alerts on this ${kind} are still recorded, but page nobody and open no incident until it ends`, body: h("div", { class: "stack" }, h("label", {}, h("div", { class: "small muted" }, "Reason"), reason), h("label", {}, h("div", { class: "small muted" }, "For"), hrs)), foot: btn });
  btn.onclick = async () => { try { await app.ctx.api("/v1/silences", { body: { kind, matcher, reason: reason.value, hours: +hrs.value, who: WHO } }); C.toast(`${matcher} is silenced`, "ok"); d.close?.(); after?.(); } catch (e) { C.toast(e.message, "bad", 6000); } };
}

function silencesCard(app, j, after) {
  const { h, C } = app;
  const quiet = Object.fromEntries(j.quiet.map((x) => [x.silenced, x.n]));
  return C.card({ title: "Silences", sub: "what is kept from paging, by whom and why; a change in progress and a planned carrier window make their own", flush: true, body: j.silences.length ? C.table([
    { key: "matcher", label: "On", render: (r) => h("span", {}, h("span", { class: "chip", style: { marginRight: "6px" } }, r.kind), h("span", { class: "mono small" }, r.matcher)) },
    { key: "reason", label: "Why" },
    { key: "created_by", label: "By" },
    { key: "source", label: "From", render: (r) => r.source === "manual" ? "a person" : r.source.replace("change:", "change ").replace("maintenance:", "notice ") },
    { key: "ends", label: "Until", render: (r) => `${dayUtc(r.ends).split(",")[0]} ${utc(r.ends)} UTC`, sort: (r) => T(r.ends) },
    { key: "n", label: "Keeps quiet", num: true, sortable: false, render: (r) => String(quiet[r.id] || 0) },
    { key: "status", label: "", sortable: false, render: (r) => r.status === "active" ? h("button", { class: "btn xs", onclick: async (e) => { e.stopPropagation(); try { await app.ctx.api(`/v1/silences/${r.id}/end`, { body: {} }); C.toast("Ended", "ok"); after(); } catch (x) { C.toast(x.message, "bad"); } } }, "End it") : h("span", { class: "muted small" }, "ended") },
  ], { rows: j.silences, compact: true }) : C.empty("okc", "Nothing is silenced", "Silence a device, a link or a site from an alert's row.") });
}

// ---------------------------------------------------------------- a change, carried out

/** The network before the change against after it, read back from the telemetry. */
function checksBlock(app, k) {
  const { h, C } = app;
  if (!k) return null;
  const bad = k.checks.filter((c) => !c.ok);
  return [h("div", { class: "no-sec" }, k.live ? "The network before it began, against now" : "The network before it, against after"),
    h("div", { class: "banner", style: { marginBottom: "8px", borderColor: k.ok ? "var(--good)" : "var(--crit)" } }, C.icon(k.ok ? "okc" : "warn"), k.ok ? ` Every check matches the state at ${utc(k.before_at)} UTC.` : ` ${bad.length} check${bad.length === 1 ? " differs" : "s differ"} from the state at ${utc(k.before_at)} UTC: ${bad.map((c) => `${c.check.toLowerCase()} on ${c.device}`).join(", ")}.`),
    C.table([{ key: "device", label: "Device", render: (r) => h("span", { class: "mono small" }, r.device) }, { key: "check", label: "Check" },
      { key: "before", label: `Before (${utc(k.before_at)})`, num: true, render: (r) => `${r.before}${r.unit}` }, { key: "after", label: `After (${utc(k.after_at)})`, num: true, render: (r) => h("span", { style: r.ok ? null : { color: "var(--crit-ink)", fontWeight: 600 } }, `${r.after}${r.unit}`) },
      { key: "ok", label: "", render: (r) => C.pill(r.ok ? "ok" : "bad", r.ok ? "same" : "differs") }], { rows: k.checks, compact: true }),
    h("div", { class: "small muted", style: { marginTop: "6px" } }, "Nothing was captured for this: both sides are queries over the minute rollups, so any change of the last three days can be checked, by anyone, afterwards.")];
}

// ---------------------------------------------------------------- leadership

function leadershipPage(body, app) {
  const { h, C, charts } = app;
  const kpis = h("div", { class: "no-kpis" }), main = h("div", { class: "no-col" }), side = h("div", { class: "no-col" }), note = h("span", { class: "small muted" });
  body.append(h("div", { class: "row between", style: { marginBottom: "12px" } }, h("div", { class: "dim" }, "The same tables from further away. Every number opens the rows behind it."), note), kpis, h("div", { class: "no-cols wide" }, main, side));
  const trend = charts.timeChart({ height: 150, fmt: (v) => v.toFixed(0), series: [{ name: "Minutes to resolve (mean)", color: "var(--s1)", get: (p) => p.mttr, fmt: (v) => v.toFixed(0) + " min" }, { name: "Incidents", color: "var(--s7)", get: (p) => p.incidents, fmt: (v) => String(v) }] });
  const bar = (v, max = 1) => { const w = Math.min(1, Math.abs(v) / max); return h("span", { class: "no-meter" }, h("i", { style: { width: Math.round(w * 100) + "%", background: v > max ? "var(--crit)" : v > 0.75 * max ? "var(--warn)" : "var(--good)" } })); };
  return live(app, body, "/v1/leadership", (j) => {
    const met = j.services.filter((s) => s.met && s.period_used <= 1).length;
    const inc = j.incidents, ch = j.change, total = ch.completed + ch.rolled_back;
    const cannot = j.risk.failover.filter((f) => !f.survives);
    note.textContent = `${j.days} days of records; measured rates over the ${j.kept_hours} hours of telemetry kept`;
    put(kpis,
      kpi(app, "Services on target", `${met}/${j.services.length}`, "for the period"),
      kpi(app, "Incidents", String(inc.count), `${inc.sev1} sev1 · ${j.days} days`),
      kpi(app, "Time to detect", Math.round(inc.ttd) + " s", "mean"),
      kpi(app, "Time to resolve", mins(inc.mttr), "mean"),
      kpi(app, "Customer-minutes", C.fmt.n(inc.customer_minutes), "customers × minutes affected"),
      kpi(app, "Revenue exposed", money(inc.revenue_exposed), "during incidents"),
      kpi(app, "Changes as planned", total ? pct(ch.completed / total) : "–", `${ch.rolled_back} of ${total} rolled back`),
      kpi(app, "Spans that cannot fail over", `${cannot.length}/${j.risk.failover.length}`, "at their measured peak"));
    put(main,
      C.card({ title: "Are we meeting what we sold?", sub: "each service against its objective; the budget is the failure its objective allows", flush: true, body: C.table([
        { key: "name", label: "Service", render: (r) => h("span", {}, h("b", {}, r.name), " ", h("span", { class: "muted small" }, `tier ${r.tier} · ${r.owner}`)) },
        { key: "slo_target", label: "Objective", num: true, render: (r) => (r.slo_target * 100).toFixed(2) + "%" },
        { key: "success", label: "Measured", num: true, render: (r) => h("span", { style: r.met ? null : { color: "var(--crit-ink)", fontWeight: 600 } }, (r.success * 100).toFixed(3) + "%") },
        { key: "budget_used", label: "Error budget spent", render: (r) => h("span", { class: "row nw", style: { gap: "8px" } }, bar(r.budget_used), h("span", { class: "small" }, pct(r.budget_used))), sort: (r) => r.budget_used },
        { key: "degraded_min", label: "Incident minutes", num: true, render: (r) => h("span", { title: `its objective allows ${r.allowed_min} minutes over the period`, style: r.period_used > 1 ? { color: "var(--crit-ink)", fontWeight: 600 } : null }, `${Math.abs(r.degraded_min)} of ${r.allowed_min}`), sort: (r) => r.period_used },
        { key: "customers", label: "Customers", num: true },
        { key: "revenue_exposed", label: "Revenue exposed", num: true, render: (r) => money(Math.abs(r.revenue_exposed)) },
      ], { rows: j.services, compact: true, sortKey: "degraded_min", desc: true, onRow: (r) => serviceDrawer(app, r.id) }) }),
      h("div", { class: "no-cols even" },
        C.card({ title: "What incidents cost", sub: "a week at a time", body: [trend, inc.by_change.length ? h("div", { class: "small", style: { marginTop: "8px" } }, h("b", {}, `${inc.by_change.length} began with a change: `), inc.by_change.map((x, i) => [i ? ", " : "", app.link(x.incident, "incidents", x.incident), " (", h("a", { class: "no-link", onclick: () => changeDrawer(app, x.change) }, x.change), ")"])) : null,
          inc.repeats.length ? h("div", { class: "small", style: { marginTop: "6px" } }, h("b", {}, "Repeat causes: "), inc.repeats.map((r) => `${r.entity} (${r.incidents.length})`).join(", ")) : h("div", { class: "small muted", style: { marginTop: "6px" } }, "No device or circuit caused two incidents.")] }),
        C.card({ title: "Where the minutes went", sub: "incident minutes by cause", body: charts.barList(inc.causes.map((c) => ({ name: `${KIND[c.category] || c.category} (${c.incidents})`, value: c.minutes })), { fmtv: (v) => Math.round(v) + " min" }) })),
      C.card({ title: "Where the risk is", sub: "backbone spans: what one plane would carry if the other failed at its measured peak", flush: true, body: C.table([
        { key: "span", label: "Span", render: (r) => h("b", {}, r.span) },
        { key: "capacity_gbps", label: "A plane", num: true, render: (r) => r.capacity_gbps + " Gb/s" },
        { key: "load", label: "One plane alone", render: (r) => h("span", { class: "row nw", style: { gap: "8px" } }, bar(r.load), h("span", { class: "small", style: r.survives ? null : { color: "var(--crit-ink)", fontWeight: 600 } }, pct(r.load))), sort: (r) => r.load },
        { key: "survives", label: "Survives a failure", render: (r) => C.pill(r.survives ? (r.load > 0.9 ? "warn" : "ok") : "bad", r.survives ? (r.load > 0.9 ? "barely" : "yes") : "no") },
        { key: "services", label: "Services on it", num: true },
      ], { rows: j.risk.failover, compact: true, sortKey: "load", desc: true, onRow: (r) => linkDrawer(app, r.a), maxHeight: "330px" }) }),
      C.card({ title: "Are the carriers delivering?", sub: `availability over the ${j.kept_hours} hours of telemetry kept, against what each contract promises`, flush: true, body: C.table([
        { key: "provider", label: "Carrier", render: (r) => h("b", {}, r.provider) }, { key: "circuits", label: "Circuits", num: true }, { key: "monthly_cost", label: "A month", num: true, render: (r) => money(r.monthly_cost) },
        { key: "availability", label: "Measured", num: true, render: (r) => h("span", { style: r.met ? null : { color: "var(--crit-ink)", fontWeight: 600 } }, r.availability.toFixed(3) + "%") },
        { key: "sla", label: "Promised", num: true, render: (r) => r.sla + "%" }, { key: "down_min", label: "Down", num: true, render: (r) => r.down_min + " min" },
        { key: "incidents", label: "Incidents", num: true, render: (r) => r.incidents ? r.incident_ids.map((id, i) => [i ? ", " : "", app.link(id, "incidents", id)]) : "0" },
        { key: "notices", label: "Windows ahead", num: true },
      ], { rows: j.carriers, compact: true, sortKey: "monthly_cost", desc: true, onRow: () => app.go("carriers") }) }));
    trend.update(inc.weeks.map((w) => ({ ...w, t: T(w.from) })));
    const exposed = j.risk.advisories.reduce((a, x) => a + x.devices, 0);
    const drift = Object.fromEntries(j.risk.drift.map((d) => [d.state, d.n]));
    const agents = j.team.agents.reduce((a, x) => a + x.runs, 0);
    const acts = Object.fromEntries(j.team.actions.map((a) => [a.status, a.n]));
    put(side,
      C.card({ title: "The week, written", tools: h("button", { class: "btn xs", onclick: () => C.copy(j.report, "Report copied") }, C.icon("copy"), "Copy"), body: h("div", { class: "no-report" }, j.report.split("\n").filter(Boolean).map((p, i) => i === 0 ? h("div", { class: "small muted" }, p) : h("p", {}, h("b", {}, p.split(". ")[0] + ". "), p.split(". ").slice(1).join(". ")))) }),
      C.card({ title: "Risk register", body: h("ul", { class: "no-list" },
        h("li", { style: { cursor: "pointer" }, onclick: () => app.go("agents") }, h("div", { class: "row between" }, h("b", {}, `${exposed} device-advisory pairs open`), C.pill(j.risk.advisories.some((a) => a.severity === "high") ? "bad" : "warn", `${j.risk.advisories.length} advisories`)), j.risk.advisories.slice(0, 3).map((a) => h("div", { class: "small muted" }, `${a.advisory}: ${a.devices} devices, fixed in ${a.fixed_in}`))),
        h("li", {}, h("div", { class: "row between" }, h("b", {}, `${cannot.length} spans cannot lose a plane at peak`), C.pill(cannot.length ? "bad" : "ok", cannot.length ? "capacity" : "fine")), h("div", { class: "small muted" }, cannot.map((f) => `${f.span} at ${pct(f.load)}`).join(", ") || "Every span fits on one plane.")),
        h("li", { style: { cursor: "pointer" }, onclick: () => app.go("carriers") }, h("div", { class: "row between" }, h("b", {}, `${j.risk.expiring.length} carrier contract${j.risk.expiring.length === 1 ? "" : "s"} end within six months`), C.pill(j.risk.expiring.length ? "warn" : "ok", "contracts")), h("div", { class: "small muted" }, j.risk.expiring.map((c) => `${c.id} (${c.contract_months_left} months, ${money(c.monthly_cost)} a month)`).join(", ") || "None.")),
        h("li", { style: { cursor: "pointer" }, onclick: () => app.go("topology") }, h("div", { class: "row between" }, h("b", {}, `${drift.undocumented || 0} links nobody documented, ${j.risk.ceased.length} documented and gone`), C.pill((drift.undocumented || 0) + j.risk.ceased.length ? "warn" : "ok", "inventory")), h("div", { class: "small muted" }, "Found by reading the topology back from the logs."))) }),
      C.card({ title: "Is change safe?", sub: `${total} carried out, by kind`, body: [charts.barList(ch.kinds.map((k) => ({ name: `${k.kind} (${k.completed + k.rolled_back})`, value: k.success * 100, color: k.success < 0.85 ? "var(--warn)" : "var(--good)" })), { fmtv: (v) => v.toFixed(0) + "% as planned", max: 100 }),
        h("div", { class: "small muted", style: { marginTop: "8px" } }, `Planned ahead: ${ch.ahead.map((a) => `${a.n} ${a.risk}`).join(", ")} risk.`)] }),
      C.card({ title: "Is the team coping?", body: [facts(app, [["alerts raised", C.fmt.n(j.team.loose + j.team.grouped)], ["grouped into incidents", C.fmt.n(j.team.grouped)], ["left loose", C.fmt.n(j.team.loose)], ["incidents out of hours", `${inc.out_of_hours} of ${inc.count}`]]),
        h("div", { class: "no-sec" }, "Incidents led, by person"), charts.barList(Object.entries(inc.pages).sort((a, b) => b[1] - a[1]).map(([name, value]) => ({ name, value })), { fmtv: C.fmt.n }),
        h("div", { class: "no-sec" }, "What the agents took"), facts(app, [["runs", C.fmt.n(agents)], ["causes confirmed by the answer key", `${j.team.right} of ${j.team.judged}`], ["actions proposed", C.fmt.n(Object.values(acts).reduce((a, b) => a + b, 0))], ["carried out", C.fmt.n(acts.executed || 0)]])] }));
  }, 60000);
}

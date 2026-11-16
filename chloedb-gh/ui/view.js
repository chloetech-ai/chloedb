// GitHub on ChloeDB: the page, in ChloeDB's command center (#/app/gh,
// loaded through chloedb-server from this app) and on its own (the app's /
// when it has no token, through ui/index.html). It imports nothing and
// builds its own elements, so the same file serves both.
//
//   (home)               what broke out this week, questions, languages rising
//   search/<query>       repos by what they do (a question goes to ask)
//   repo/<owner>/<name>  a repo: star history and why it jumped, vital signs,
//                        who keeps it going, alternatives, also starred
//   person/<login>       what a person or organisation did, where, and starred
//   rising/<days>        stars that jumped against the four weeks before
//   trends/<kind>/<a,b>  languages or topics, share of stars year by year
//   ask/<question>       a question read by patterns and answered in SQL
//   foryou/<login>       what to star next, from a login's public stars
//   weekly/<day>         a week of GitHub
//   about                how it works, and how big it is
//
// Every answer of the API carries the SQL it ran: the "SQL" button shows
// the queries behind the page in view.

export default { render };

const CSS = `
.gx { --gx-w: 1240px; color: var(--text); font: 14px/1.5 var(--sans); max-width: var(--gx-w); margin: 0 auto; }
.gx * { box-sizing: border-box; }
.gx a { color: inherit; text-decoration: none; }
.gx a:hover { text-decoration: underline; text-underline-offset: 2px; }
.gx mark { background: color-mix(in srgb, var(--warn) 34%, transparent); color: inherit; border-radius: 3px; padding: 0 1px; }
.gx button { font: inherit; }
.gx-top { position: sticky; top: 0; z-index: 5; background: color-mix(in srgb, var(--bg) 88%, transparent); backdrop-filter: blur(10px); padding: 12px 0 6px; }
.gx-bar { display: flex; align-items: center; gap: 14px; }
.gx-brand { display: flex; align-items: center; gap: 9px; font-weight: 680; letter-spacing: -.015em; white-space: nowrap; font-size: 15.5px; }
.gx-brand svg { width: 26px; height: 26px; color: var(--accent); fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; }
.gx-brand small { font-weight: 500; color: var(--muted); font-size: 12px; }
.gx-box { position: relative; flex: 1; min-width: 0; }
.gx-box input { width: 100%; font: 15px var(--sans); padding: 10px 14px 10px 38px; border-radius: 11px; border: 1px solid var(--line-2); background: var(--surface); color: var(--text); outline: none; box-shadow: var(--shadow-1); }
.gx-box input:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--focus); }
.gx-box > svg { position: absolute; left: 12px; top: 50%; width: 17px; height: 17px; transform: translateY(-50%); color: var(--muted); pointer-events: none; fill: none; stroke: currentColor; stroke-width: 2; }
.gx-box .kbd { position: absolute; right: 10px; top: 50%; transform: translateY(-50%); font: 11px var(--mono); color: var(--muted); border: 1px solid var(--line-2); border-radius: 5px; padding: 1px 6px; }
.gx-sugg { position: absolute; left: 0; right: 0; top: calc(100% + 6px); background: var(--surface); border: 1px solid var(--line-2); border-radius: 11px; box-shadow: var(--shadow-2); padding: 6px; display: none; z-index: 9; max-height: 70vh; overflow: auto; }
.gx-sugg.on { display: block; }
.gx-sugg .it { display: flex; gap: 10px; align-items: baseline; padding: 7px 9px; border-radius: 7px; cursor: pointer; }
.gx-sugg .it.sel, .gx-sugg .it:hover { background: var(--accent-soft); }
.gx-sugg .it .k { font-size: 10.5px; color: var(--muted); width: 64px; flex: none; text-transform: uppercase; letter-spacing: .05em; }
.gx-sugg .it .t { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.gx-sugg .it .m { margin-left: auto; color: var(--muted); font-size: 12px; white-space: nowrap; }
.gx-nav { display: flex; gap: 2px; margin-top: 8px; overflow-x: auto; scrollbar-width: none; align-items: center; }
.gx-nav a { padding: 5px 11px; border-radius: 8px; color: var(--text-2); font-size: 13px; white-space: nowrap; }
.gx-nav a:hover { background: var(--surface-2); text-decoration: none; }
.gx-nav a.on { background: var(--accent-soft); color: var(--accent); font-weight: 600; }
.gx-nav .sp { flex: 1; }
.gx-nav .live { display: inline-flex; align-items: center; gap: 6px; color: var(--muted); font-size: 12px; padding: 5px 4px; white-space: nowrap; }
.gx-dot { width: 7px; height: 7px; border-radius: 50%; background: var(--good); animation: gxpulse 2s infinite; }
@keyframes gxpulse { 0% { box-shadow: 0 0 0 0 color-mix(in srgb, var(--good) 55%, transparent); } 70% { box-shadow: 0 0 0 7px transparent; } 100% { box-shadow: 0 0 0 0 transparent; } }
.gx-main { padding: 8px 0 80px; min-height: 60vh; }
.gx-und { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; margin: 4px 0 12px; font-size: 12.5px; color: var(--muted); }
.gx-chip { display: inline-flex; align-items: center; gap: 5px; padding: 2px 9px; border-radius: 999px; background: var(--surface); border: 1px solid var(--line-2); color: var(--text-2); font-size: 12.5px; white-space: nowrap; cursor: pointer; line-height: 1.6; }
.gx-chip:hover { border-color: var(--accent); color: var(--accent); text-decoration: none !important; }
.gx-chip.f { background: var(--accent-soft); border-color: transparent; color: var(--accent); }
.gx-chip.t { background: color-mix(in srgb, var(--s7) 12%, var(--surface)); border-color: transparent; color: color-mix(in srgb, var(--s7) 80%, var(--text)); }
.gx-chip.d { background: color-mix(in srgb, var(--s3) 13%, var(--surface)); border-color: transparent; color: color-mix(in srgb, var(--s3) 75%, var(--text)); }
.gx-chip.c { font-family: var(--mono); font-size: 11.5px; }
.gx-chip .x { opacity: .6; font-size: 13px; line-height: 1; }
.gx-chip .from { color: var(--muted); font-family: var(--mono); font-size: 11px; }
.gx-grid { display: grid; grid-template-columns: 220px minmax(0, 1fr) 290px; gap: 26px; align-items: start; }
.gx-grid.two { grid-template-columns: minmax(0, 1fr) 320px; }
@media (max-width: 1120px) { .gx-grid { grid-template-columns: 200px minmax(0, 1fr); } .gx-grid > .gx-right { display: none; } }
@media (max-width: 780px) { .gx-grid, .gx-grid.two { grid-template-columns: minmax(0, 1fr); } .gx-grid > .gx-left { display: none; } .gx-brand small { display: none; } .gx-box .kbd { display: none; } .gx-box input { font-size: 16px; } .gx-try.gx-row { flex-wrap: nowrap; overflow-x: auto; scrollbar-width: none; padding-bottom: 2px; } .gx-try.gx-row > * { flex: none; } }
.gx-left { position: sticky; top: 108px; max-height: calc(100vh - 120px); overflow: auto; scrollbar-width: thin; padding-right: 4px; }
.gx-facet { margin-bottom: 18px; }
.gx-facet h4 { margin: 0 0 6px; font-size: 11px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); font-weight: 600; }
.gx-facet .r { display: flex; align-items: center; gap: 8px; padding: 3px 6px; margin: 0 -6px; border-radius: 6px; cursor: pointer; font-size: 13px; color: var(--text-2); }
.gx-facet .r:hover { background: var(--surface-2); color: var(--text); }
.gx-facet .r .n { margin-left: auto; color: var(--muted); font-variant-numeric: tabular-nums; font-size: 12px; }
.gx-facet .r .l { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.gx-years { display: flex; align-items: flex-end; gap: 1px; height: 54px; margin: 4px 0 2px; }
.gx-years div { flex: 1; background: color-mix(in srgb, var(--accent) 55%, transparent); border-radius: 2px 2px 0 0; min-height: 2px; cursor: pointer; }
.gx-years div:hover { background: var(--accent); }
.gx-yl { display: flex; justify-content: space-between; font-size: 10.5px; color: var(--muted); }
.gx-head { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; margin-bottom: 4px; }
.gx-head .t { font-size: 13px; color: var(--muted); }
.gx-head .t b { color: var(--text); font-weight: 600; }
.gx-head .sp { flex: 1; }
.gx-seg { display: inline-flex; border: 1px solid var(--line-2); border-radius: 8px; overflow: hidden; background: var(--surface); }
.gx-seg button { font: 12.5px var(--sans); border: 0; background: none; color: var(--text-2); padding: 4px 10px; cursor: pointer; }
.gx-seg button + button { border-left: 1px solid var(--line); }
.gx-seg button.on { background: var(--accent-soft); color: var(--accent); font-weight: 600; }
.gx-dym { font-size: 14px; margin: 2px 0 10px; color: var(--text-2); }
.gx-dym a { color: var(--accent); font-weight: 600; font-style: italic; }
.gx-list { list-style: none; margin: 0; padding: 0; }
.gx-p { padding: 13px 0; border-bottom: 1px solid var(--line); }
.gx-p:last-child { border-bottom: 0; }
.gx-p .ttl { font-size: 15.5px; font-weight: 580; line-height: 1.35; letter-spacing: -.006em; }
.gx-p .ttl a { color: var(--text); }
.gx-p .au { font-size: 13px; color: var(--text-2); margin-top: 2px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.gx-p .au a:hover { color: var(--accent); }
.gx-p .meta { display: flex; flex-wrap: wrap; gap: 4px 10px; font-size: 12.5px; color: var(--muted); margin-top: 4px; align-items: center; }
.gx-p .meta a:hover { color: var(--accent); }
.gx-p .abs { font-size: 13.5px; color: var(--text-2); margin-top: 6px; display: -webkit-box; -webkit-line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden; }
.gx-p .tags { display: flex; flex-wrap: wrap; gap: 5px; margin-top: 7px; }
.gx-p.compact { padding: 8px 0; }
.gx-p.compact .ttl { font-size: 14px; font-weight: 560; }
.gx-p .why { font-size: 11.5px; color: var(--accent); margin-top: 3px; }
.gx-badge { font-size: 10.5px; font-weight: 650; padding: 0 6px; border-radius: 5px; background: var(--surface-3); color: var(--text-2); text-transform: uppercase; letter-spacing: .04em; line-height: 1.7; }
.gx-badge.code { background: var(--good-soft); color: var(--good-ink); }
.gx-badge.survey { background: var(--accent-soft); color: var(--accent); }
.gx-badge.benchmark, .gx-badge.dataset { background: var(--warn-soft); color: color-mix(in srgb, var(--warn) 50%, var(--text)); }
.gx-badge.tool { background: var(--serious-soft); color: var(--s2); }
.gx-cites { font-variant-numeric: tabular-nums; color: var(--text-2); }
.gx-cites b { color: var(--text); font-weight: 650; }
.gx-sec { display: flex; align-items: baseline; justify-content: space-between; margin: 24px 0 6px; gap: 10px; }
.gx-sec h3 { margin: 0; font-size: 12.5px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); font-weight: 650; }
.gx-sec a, .gx-sec .a { font-size: 13px; color: var(--accent); cursor: pointer; }
.gx-sec .note { font-size: 12px; color: var(--muted); }
.gx-card { background: var(--surface); border: 1px solid var(--line); border-radius: var(--r); padding: 14px 16px; margin-bottom: 14px; box-shadow: var(--shadow-1); }
.gx-card h4 { margin: 0 0 8px; font-size: 11.5px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); font-weight: 650; display: flex; justify-content: space-between; gap: 8px; }
.gx-card h4 a { text-transform: none; letter-spacing: 0; color: var(--accent); font-weight: 500; }
.gx-ans { border: 1px solid color-mix(in srgb, var(--accent) 30%, var(--line)); background: linear-gradient(180deg, color-mix(in srgb, var(--accent) 6%, var(--surface)), var(--surface)); border-radius: var(--r); padding: 14px 16px 12px; margin: 10px 0 14px; }
.gx-ans .ey { font-size: 11px; text-transform: uppercase; letter-spacing: .07em; color: var(--accent); font-weight: 680; display: flex; gap: 8px; align-items: center; }
.gx-ans .ey .nm { color: var(--muted); font-weight: 500; text-transform: none; letter-spacing: 0; margin-left: auto; }
.gx-ans .big { font-size: 17.5px; font-weight: 640; line-height: 1.38; letter-spacing: -.008em; margin-top: 5px; }
.gx-ans ol { margin: 8px 0 0; padding: 0; list-style: none; counter-reset: r; }
.gx-ans ol li { counter-increment: r; display: grid; grid-template-columns: 22px 1fr auto; gap: 8px; align-items: center; padding: 5px 0; border-top: 1px solid var(--line); font-size: 13.5px; }
.gx-ans ol li:first-child { border-top: 0; }
.gx-ans ol li:before { content: counter(r); color: var(--muted); font-size: 12px; text-align: right; }
.gx-ans ol li .n { color: var(--muted); font-size: 12.5px; font-variant-numeric: tabular-nums; white-space: nowrap; }
.gx-ans ol li .who { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.gx-ans .cols { display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 16px; margin-top: 8px; }
.gx-ans .cols h5 { margin: 0 0 2px; font-size: 11px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); }
.gx-sum { font-size: 14.5px; line-height: 1.6; }
.gx-sum p { margin: 0 0 8px; }
.gx-sum ul { margin: 4px 0 8px; padding-left: 20px; }
.gx-sum li { margin: 4px 0; }
.gx-sum sup a { color: var(--accent); font-size: 10.5px; font-weight: 650; padding: 0 1px; }
.gx-src { font-size: 12.5px; color: var(--muted); display: grid; grid-template-columns: 24px 1fr; gap: 2px 6px; margin-top: 8px; }
.gx-src a { color: var(--text-2); }
.gx-btn { display: inline-flex; align-items: center; gap: 6px; padding: 6px 12px; border-radius: 8px; border: 1px solid var(--line-2); background: var(--surface); color: var(--text); cursor: pointer; font-size: 13px; white-space: nowrap; }
.gx-btn:hover { border-color: var(--accent); color: var(--accent); text-decoration: none !important; }
.gx-btn.pri { background: var(--accent); border-color: var(--accent); color: var(--accent-ink); font-weight: 600; }
.gx-btn.pri:hover { background: var(--accent-2); color: var(--accent-ink); }
.gx-btn.ghost { border-color: transparent; background: none; }
.gx-btn[disabled] { opacity: .55; cursor: default; }
.gx-btn.on { background: var(--accent-soft); border-color: transparent; color: var(--accent); }
.gx-empty { color: var(--muted); padding: 26px 0; text-align: center; }
.gx-skel { height: 14px; border-radius: 6px; background: linear-gradient(90deg, var(--surface-2), var(--surface-3), var(--surface-2)); background-size: 200% 100%; animation: gxsk 1.2s infinite; margin: 10px 0; }
@keyframes gxsk { 0% { background-position: 200% 0; } 100% { background-position: -200% 0; } }
.gx-err { color: var(--crit-ink); background: var(--crit-soft); padding: 10px 12px; border-radius: 8px; font-size: 13px; }
.gx-hero { text-align: center; padding: 46px 0 22px; }
.gx-hero h1 { margin: 0; font-size: 38px; letter-spacing: -.03em; line-height: 1.08; font-weight: 760; }
.gx-hero h1 span { background: linear-gradient(90deg, var(--accent), var(--s7)); -webkit-background-clip: text; background-clip: text; color: transparent; }
.gx-hero p { color: var(--text-2); font-size: 16px; margin: 12px auto 0; max-width: 700px; }
.gx-stats { display: flex; justify-content: center; flex-wrap: wrap; gap: 8px 26px; margin: 18px 0 6px; color: var(--muted); font-size: 13px; }
.gx-stats b { color: var(--text); font-size: 17px; font-weight: 700; font-variant-numeric: tabular-nums; margin-right: 4px; }
.gx-try { display: flex; flex-wrap: wrap; justify-content: center; gap: 7px; margin: 16px auto 4px; max-width: 920px; }
.gx-try a { padding: 5px 12px; border-radius: 999px; border: 1px solid var(--line-2); background: var(--surface); font-size: 13px; color: var(--text-2); }
.gx-try a:hover { border-color: var(--accent); color: var(--accent); text-decoration: none; }
.gx-cols { display: grid; grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); gap: 22px; }
.gx-rise { display: grid; grid-template-columns: 1fr auto; gap: 2px 10px; font-size: 13.5px; align-items: baseline; }
.gx-rise .g { font-variant-numeric: tabular-nums; font-weight: 650; color: var(--good-ink); font-size: 12.5px; }
.gx-rise .g.down { color: var(--crit-ink); }
.gx-rise a { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.gx-rise .s { grid-column: 1 / -1; font-size: 12px; color: var(--muted); margin: -2px 0 6px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.gx-pg { display: flex; gap: 8px; align-items: center; justify-content: center; margin: 16px 0; font-size: 13px; color: var(--muted); }
.gx-ph { padding: 4px 0 8px; }
.gx-ph .ey { font-size: 12px; color: var(--muted); display: flex; gap: 10px; align-items: center; flex-wrap: wrap; }
.gx-ph h1 { margin: 6px 0 6px; font-size: 27px; line-height: 1.2; letter-spacing: -.02em; font-weight: 720; }
.gx-ph .au { font-size: 14.5px; color: var(--text-2); }
.gx-ph .au a { color: var(--text); }
.gx-ph .au a:hover { color: var(--accent); }
.gx-ph .au .more { color: var(--accent); cursor: pointer; }
.gx-ph .abs { font-size: 15px; line-height: 1.62; margin: 14px 0; color: var(--text); max-width: 820px; }
.gx-ph .acts { display: flex; flex-wrap: wrap; gap: 8px; margin: 10px 0; }
.gx-ph .tags { display: flex; flex-wrap: wrap; gap: 6px; margin: 8px 0; }
.gx-kpis { display: grid; grid-template-columns: repeat(auto-fit, minmax(92px, 1fr)); gap: 10px; margin: 12px 0; }
.gx-kpi { background: var(--surface); border: 1px solid var(--line); border-radius: var(--r); padding: 10px 12px; }
.gx-kpi .v { font-size: 22px; font-weight: 720; letter-spacing: -.02em; font-variant-numeric: tabular-nums; }
.gx-kpi .l { font-size: 11.5px; color: var(--muted); text-transform: uppercase; letter-spacing: .05em; }
.gx-tldr { border-left: 3px solid var(--accent); padding: 8px 14px; background: var(--accent-soft); border-radius: 0 8px 8px 0; margin: 10px 0; font-size: 14.5px; max-width: 820px; }
.gx-tldr b { color: var(--accent); }
.gx-tabs { display: flex; gap: 2px; border-bottom: 1px solid var(--line); margin: 18px 0 4px; overflow-x: auto; scrollbar-width: none; }
.gx-tabs button { border: 0; background: none; padding: 8px 12px; color: var(--text-2); cursor: pointer; border-bottom: 2px solid transparent; white-space: nowrap; font-size: 13.5px; }
.gx-tabs button.on { color: var(--accent); border-bottom-color: var(--accent); font-weight: 600; }
.gx-tabs button .n { color: var(--muted); font-size: 12px; margin-left: 4px; }
.gx-people { display: grid; grid-template-columns: 1fr auto; gap: 3px 10px; font-size: 13.5px; }
.gx-people a { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.gx-people .n { color: var(--muted); font-size: 12.5px; font-variant-numeric: tabular-nums; text-align: right; }
.gx-chart { width: 100%; display: block; overflow: visible; }
.gx-chart text { fill: var(--muted); font: 10.5px var(--sans); }
.gx-chart .grid { stroke: var(--grid); stroke-width: 1; }
.gx-chart .ln { fill: none; stroke-width: 2.2; stroke-linejoin: round; stroke-linecap: round; }
.gx-chart .ar { opacity: .12; }
.gx-chart .hov { stroke: var(--axis); stroke-dasharray: 3 3; }
.gx-legend { display: flex; flex-wrap: wrap; gap: 6px 14px; font-size: 12.5px; color: var(--text-2); margin: 6px 0; }
.gx-legend i { display: inline-block; width: 10px; height: 10px; border-radius: 3px; margin-right: 6px; vertical-align: -1px; }
.gx-tip { position: fixed; z-index: 30; pointer-events: none; background: var(--surface); border: 1px solid var(--line-2); border-radius: 8px; box-shadow: var(--shadow-2); padding: 7px 10px; font-size: 12.5px; display: none; max-width: 320px; }
.gx-tip b { display: block; margin-bottom: 2px; }
.gx-atlas { position: relative; height: calc(100vh - 170px); min-height: 520px; border: 1px solid var(--line); border-radius: var(--r); background: var(--surface); overflow: hidden; }
.gx-atlas canvas { width: 100%; height: 100%; display: block; cursor: grab; }
.gx-atlas .side { position: absolute; top: 10px; left: 10px; width: 300px; max-height: calc(100% - 20px); overflow: auto; background: color-mix(in srgb, var(--surface) 92%, transparent); backdrop-filter: blur(8px); border: 1px solid var(--line); border-radius: 10px; padding: 10px 12px; box-shadow: var(--shadow-2); }
.gx-atlas .side input { width: 100%; padding: 6px 9px; border-radius: 7px; border: 1px solid var(--line-2); background: var(--surface); color: var(--text); margin-bottom: 8px; }
.gx-atlas .hint { position: absolute; right: 12px; bottom: 10px; font-size: 12px; color: var(--muted); }
.gx-studio { display: grid; grid-template-columns: 380px minmax(0, 1fr); gap: 24px; align-items: start; }
@media (max-width: 960px) { .gx-studio { grid-template-columns: minmax(0, 1fr); } }
.gx-form { position: sticky; top: 108px; max-height: calc(100vh - 120px); overflow: auto; scrollbar-width: thin; padding-right: 6px; }
.gx-f { margin-bottom: 14px; }
.gx-f > label { display: block; font-size: 11.5px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); font-weight: 650; margin-bottom: 5px; }
.gx-f > label small { text-transform: none; letter-spacing: 0; font-weight: 400; }
.gx-f input[type=text], .gx-f textarea, .gx-f select { width: 100%; padding: 7px 10px; border-radius: 8px; border: 1px solid var(--line-2); background: var(--surface); color: var(--text); font: 13.5px var(--sans); outline: none; }
.gx-f input[type=text]:focus, .gx-f textarea:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--focus); }
.gx-f input[type=range] { width: 100%; accent-color: var(--accent); }
.gx-f .row { display: flex; gap: 8px; align-items: center; }
.gx-f .row > * { flex: 1; }
.gx-f .val { font-variant-numeric: tabular-nums; color: var(--text); font-weight: 600; flex: none; min-width: 36px; text-align: right; font-size: 13px; }
.gx-tagsin { display: flex; flex-wrap: wrap; gap: 5px; padding: 5px; border-radius: 8px; border: 1px solid var(--line-2); background: var(--surface); min-height: 36px; align-items: center; }
.gx-tagsin:focus-within { border-color: var(--accent); box-shadow: 0 0 0 3px var(--focus); }
.gx-tagsin input { border: 0 !important; box-shadow: none !important; flex: 1; min-width: 120px; padding: 3px 4px !important; background: transparent !important; }
.gx-tagsin .gx-chip { cursor: default; }
.gx-tagsin .gx-chip .x { cursor: pointer; }
.gx-check { display: flex; flex-wrap: wrap; gap: 6px; }
.gx-check label { display: inline-flex; gap: 5px; align-items: center; font-size: 13px; padding: 3px 9px; border-radius: 999px; border: 1px solid var(--line-2); cursor: pointer; background: var(--surface); }
.gx-check label.on { background: var(--accent-soft); color: var(--accent); border-color: transparent; }
.gx-check input { display: none; }
.gx-news { max-width: 760px; }
.gx-news h1 { font-size: 30px; letter-spacing: -.02em; margin: 6px 0 4px; line-height: 1.15; }
.gx-news .tag { color: var(--muted); font-size: 15px; margin-bottom: 18px; }
.gx-news .item { padding: 16px 0 18px; border-top: 1px solid var(--line); }
.gx-news .item h2 { font-size: 19px; letter-spacing: -.01em; margin: 0 0 8px; line-height: 1.3; }
.gx-news .item h2 .n { color: var(--muted); font-weight: 500; margin-right: 6px; }
.gx-news .item p { font-size: 15px; line-height: 1.66; margin: 0 0 8px; }
.gx-news .links { display: flex; flex-wrap: wrap; gap: 6px 16px; font-size: 13.5px; }
.gx-news .links a { color: var(--accent); }
.gx-news .extra li { margin: 6px 0; font-size: 14px; }
.gx-news .open { font-size: 16px; line-height: 1.7; color: var(--text); margin: 0 0 18px; }
.gx-news .open a, .gx-news .toc a { color: var(--accent); }
.gx-news .toc { counter-reset: t; list-style: none; padding: 12px 14px; margin: 0 0 18px; border: 1px solid var(--line); border-radius: var(--r); background: var(--surface); }
.gx-news .toc li { counter-increment: t; display: grid; grid-template-columns: 26px 1fr; gap: 2px 6px; padding: 5px 0; font-size: 14px; line-height: 1.45; }
.gx-news .toc li::before { content: counter(t) "."; color: var(--muted); font-weight: 600; }
.gx-news .toc li span { grid-column: 2; color: var(--text-2); font-size: 13px; }
.gx-news .who { font-size: 12.5px; color: var(--muted); margin: -2px 0 10px; }
.gx-news .brief { font-size: 16.5px; line-height: 1.6; margin: 0 0 14px; padding: 10px 14px; border-left: 3px solid var(--accent); background: color-mix(in srgb, var(--accent) 7%, transparent); border-radius: 0 8px 8px 0; }
.gx-news .lab { display: block; font-size: 11px; font-weight: 700; letter-spacing: .07em; text-transform: uppercase; color: var(--muted); margin: 14px 0 3px; }
.gx-news .lab + p { margin-top: 0; }
.gx-news .fig { margin: 12px 0 14px; }
.gx-news .fig .imgs { display: flex; gap: 8px; flex-wrap: wrap; justify-content: center; background: #fff; border: 1px solid var(--line); border-radius: 10px; padding: 10px; }
.gx-news .fig img { max-width: 100%; max-height: 380px; object-fit: contain; flex: 1 1 0; min-width: 0; }
.gx-news .fig .ph { flex: 1 1 0; min-height: 160px; border-radius: 6px; background: linear-gradient(90deg, #f1f2f4, #e7e9ec, #f1f2f4); background-size: 200% 100%; animation: ax-sh 1.4s infinite; }
.gx-news .fig figcaption { font-size: 13.5px; line-height: 1.5; color: var(--text-2); margin-top: 6px; }
.gx-news .fig figcaption b { color: var(--text); }
.gx-news .fig details { font-size: 12.5px; color: var(--muted); margin-top: 4px; }
.gx-news .fig summary { cursor: pointer; }
.gx-news .tbl { overflow-x: auto; margin: 6px 0 4px; border: 1px solid var(--line); border-radius: 8px; }
.gx-news .tbl table { border-collapse: collapse; font-size: 12.5px; width: 100%; }
.gx-news .tbl td { padding: 4px 8px; border-top: 1px solid var(--line); white-space: nowrap; font-variant-numeric: tabular-nums; }
.gx-news .tbl tr:first-child td { border-top: 0; font-weight: 600; background: var(--surface); }
.gx-news .terms { display: grid; grid-template-columns: max-content 1fr; gap: 4px 12px; font-size: 13.5px; margin: 4px 0 10px; }
.gx-news .terms dt { font-weight: 600; }
.gx-news .terms dd { margin: 0; color: var(--text-2); }
.gx-news .note { font-size: 12.5px; color: var(--muted); margin: 4px 0 8px; }
@keyframes ax-sh { 0% { background-position: 200% 0; } 100% { background-position: -200% 0; } }
.gx-prev .gx-p .found { font-size: 11.5px; color: var(--muted); margin-top: 4px; }
.gx-prev .gx-p .found span { background: var(--surface-3); border-radius: 5px; padding: 0 5px; margin-right: 4px; }
.gx-prog { height: 4px; border-radius: 2px; background: var(--surface-3); overflow: hidden; margin: 8px 0; }
.gx-prog div { height: 100%; background: var(--accent); width: 30%; animation: axprog 1.4s infinite ease-in-out; }
@keyframes axprog { 0% { margin-left: -30%; } 100% { margin-left: 100%; } }
.gx-sql { position: fixed; right: 18px; bottom: 16px; z-index: 20; }
.gx-sql button { font: 12px var(--mono); padding: 6px 11px; border-radius: 999px; border: 1px solid var(--line-2); background: var(--surface); color: var(--text-2); cursor: pointer; box-shadow: var(--shadow-2); }
.gx-sql button b { color: var(--accent); }
.gx-sqlp { position: fixed; right: 18px; bottom: 56px; width: min(760px, calc(100vw - 36px)); max-height: 70vh; overflow: auto; z-index: 21; background: var(--surface); border: 1px solid var(--line-2); border-radius: 12px; box-shadow: var(--shadow-3); padding: 12px 14px; display: none; }
.gx-sqlp.on { display: block; }
.gx-sqlp .q { border-top: 1px solid var(--line); padding: 8px 0; }
.gx-sqlp .w { display: flex; justify-content: space-between; gap: 10px; font-size: 12.5px; color: var(--text-2); font-weight: 600; }
.gx-sqlp .w span { color: var(--muted); font-weight: 400; white-space: nowrap; font-variant-numeric: tabular-nums; }
.gx-sqlp pre { margin: 5px 0 0; font: 11.5px/1.5 var(--mono); white-space: pre-wrap; word-break: break-word; background: var(--code); padding: 8px 10px; border-radius: 7px; color: var(--text); }
.gx-sqlp .bad { color: var(--crit-ink); font-size: 12px; }
.gx-foot { color: var(--muted); font-size: 12px; text-align: center; padding: 30px 0 60px; border-top: 1px solid var(--line); margin-top: 30px; }
.gx-foot a { color: var(--accent); }
.gx-about { max-width: 780px; font-size: 14.5px; line-height: 1.65; }
.gx-about h2 { font-size: 18px; margin: 22px 0 6px; }
.gx-about code { font: 12.5px var(--mono); background: var(--code); padding: 1px 5px; border-radius: 4px; }
.gx-about pre { font: 12px/1.5 var(--mono); background: var(--code); padding: 10px 12px; border-radius: 8px; overflow: auto; }
.gx-sp { display: block; overflow: visible; }
.gx-sp path { fill: none; stroke: var(--accent); stroke-width: 1.5; stroke-linejoin: round; }
.gx-sp path.ar { fill: var(--accent); opacity: .12; stroke: none; }
.gx-r { padding: 11px 0; border-bottom: 1px solid var(--line); }
.gx-r:last-child { border-bottom: 0; }
.gx-r .nm { font-size: 15px; font-weight: 450; letter-spacing: -.005em; }
.gx-r .nm a { color: var(--text); }
.gx-r .nm b { font-weight: 650; }
.gx-r .ds { font-size: 13.5px; color: var(--text-2); margin-top: 2px; overflow: hidden; text-overflow: ellipsis; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; }
.gx-r .mt { display: flex; flex-wrap: wrap; gap: 4px 12px; font-size: 12.5px; color: var(--muted); margin-top: 5px; align-items: center; }
.gx-r .mt .up { color: var(--good-ink); font-weight: 650; }
.gx-r .why { font-size: 12.5px; color: var(--accent); margin-top: 4px; }
.gx-r .why b { font-weight: 650; }
.gx-r.compact { padding: 7px 0; }
.gx-r.compact .nm { font-size: 14px; }
.gx-lang { display: inline-flex; align-items: center; gap: 5px; }
.gx-lang i { width: 9px; height: 9px; border-radius: 50%; display: inline-block; }
.gx-word { font-size: 10.5px; font-weight: 700; padding: 1px 7px; border-radius: 5px; text-transform: uppercase; letter-spacing: .05em; line-height: 1.6; }
.gx-word.active { background: var(--good-soft); color: var(--good-ink); }
.gx-word.quiet { background: var(--warn-soft); color: color-mix(in srgb, var(--warn) 45%, var(--text)); }
.gx-word.slowing { background: var(--serious-soft); color: var(--s2); }
.gx-word.dormant, .gx-word.archived { background: var(--surface-3); color: var(--text-2); }
.gx-tiles { display: grid; grid-template-columns: repeat(3, 1fr); gap: 10px; margin-bottom: 14px; }
.gx-tile { background: var(--surface); border: 1px solid var(--line); border-radius: var(--r); padding: 12px 14px; box-shadow: var(--shadow-1); }
.gx-tile b { display: block; font-size: 22px; font-weight: 720; letter-spacing: -.02em; font-variant-numeric: tabular-nums; }
.gx-tile span { font-size: 11px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); }
.gx-vs .row { display: flex; justify-content: space-between; gap: 10px; padding: 8px 0; border-top: 1px solid var(--line); }
.gx-vs .row:first-of-type { border-top: 0; }
.gx-vs .row .k { font-size: 13.5px; }
.gx-vs .row .k small { display: block; color: var(--muted); font-size: 11.5px; }
.gx-vs .row .v { font-weight: 650; font-variant-numeric: tabular-nums; white-space: nowrap; }
.gx-vs .row .v.good { color: var(--good-ink); }
.gx-vs .row .v.bad { color: var(--crit-ink); }
.gx-ppl .p { padding: 5px 0; }
.gx-ppl .p .l { display: flex; justify-content: space-between; font-size: 13.5px; gap: 8px; }
.gx-ppl .p .l span { color: var(--muted); font-variant-numeric: tabular-nums; font-size: 12.5px; }
.gx-ppl .p .b { height: 5px; background: var(--surface-3); border-radius: 3px; margin-top: 4px; overflow: hidden; }
.gx-ppl .p .b i { display: block; height: 100%; background: var(--accent); border-radius: 3px; }
.gx-kv { display: grid; grid-template-columns: 1fr auto; gap: 4px 12px; font-size: 13.5px; }
.gx-kv span:nth-child(2n) { color: var(--muted); font-variant-numeric: tabular-nums; text-align: right; }
.gx-spk { list-style: none; margin: 0; padding: 0; }
.gx-spk li { display: grid; grid-template-columns: 22px 92px 1fr; gap: 10px; padding: 8px 0; border-top: 1px solid var(--line); font-size: 13.5px; align-items: baseline; }
.gx-spk li:first-child { border-top: 0; }
.gx-spk .no { width: 18px; height: 18px; border-radius: 50%; background: var(--s2); color: #fff; font-size: 11px; font-weight: 700; display: inline-flex; align-items: center; justify-content: center; }
.gx-spk .d { color: var(--muted); font-variant-numeric: tabular-nums; }
.gx-spk small { display: block; color: var(--muted); font-size: 12px; }
.gx-chart .mk { stroke: var(--s2); stroke-dasharray: 3 3; opacity: .8; }
.gx-chart .mkc { fill: var(--s2); }
.gx-chart .mkt { fill: #fff; font: 700 10px var(--sans); }
.gx-chart .bar { fill: color-mix(in srgb, var(--accent) 60%, transparent); }
.gx-chart .bar:hover { fill: var(--accent); }
.gx-hbar { display: grid; grid-template-columns: minmax(90px, 200px) 1fr auto; gap: 4px 10px; align-items: center; font-size: 13.5px; }
.gx-hbar .b { height: 14px; background: var(--surface-3); border-radius: 4px; overflow: hidden; }
.gx-hbar .b i { display: block; height: 100%; background: color-mix(in srgb, var(--accent) 70%, transparent); }
.gx-hbar .n { color: var(--muted); font-variant-numeric: tabular-nums; text-align: right; font-size: 12.5px; }
.gx-chg { font-weight: 650; font-variant-numeric: tabular-nums; }
.gx-chg.up { color: var(--good-ink); }
.gx-chg.down { color: var(--crit-ink); }
.gx-ex { list-style: none; margin: 0; padding: 0; }
.gx-ex li { padding: 8px 0; border-top: 1px solid var(--line); }
.gx-ex li:first-child { border-top: 0; }
.gx-ex a { font-weight: 560; }
.gx-form { display: flex; gap: 8px; margin: 10px 0 16px; flex-wrap: wrap; }
.gx-form input { flex: 1; min-width: 200px; font: 14px var(--sans); padding: 8px 12px; border-radius: 9px; border: 1px solid var(--line-2); background: var(--surface); color: var(--text); outline: none; }
.gx-form input:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--focus); }
.gx-h1 { font-size: 26px; font-weight: 720; letter-spacing: -.02em; margin: 2px 0 4px; }
.gx-h1 a { color: var(--text); }
.gx-sub { color: var(--text-2); font-size: 14.5px; margin: 0 0 10px; }
.gx-lead { color: var(--text-2); font-size: 14px; margin: 0 0 14px; max-width: 760px; }
.gx-crumb { display: flex; flex-wrap: wrap; gap: 6px 12px; align-items: center; font-size: 12.5px; color: var(--muted); }
.gx-crumb code { font: 12px var(--mono); }
.gx-ch3 { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 28px; margin-top: 18px; }
@media (max-width: 1000px) { .gx-ch3 { grid-template-columns: minmax(0, 1fr); } .gx-tiles { grid-template-columns: repeat(3, 1fr); } }
.gx-tbl { width: 100%; border-collapse: collapse; font-size: 13px; }
.gx-tbl th { text-align: left; font-weight: 600; color: var(--muted); font-size: 11.5px; text-transform: uppercase; letter-spacing: .05em; padding: 6px 8px; border-bottom: 1px solid var(--line-2); }
.gx-tbl td { padding: 6px 8px; border-bottom: 1px solid var(--line); font-variant-numeric: tabular-nums; }
.gx-tbl td.r, .gx-tbl th.r { text-align: right; }
.gx-prose { max-width: 780px; font-size: 14.5px; line-height: 1.65; }
.gx-prose h2 { font-size: 20px; margin: 26px 0 8px; letter-spacing: -.01em; }
.gx-prose code { font: 12.5px var(--mono); background: var(--code); padding: 1px 5px; border-radius: 5px; }
.gx-prose pre { font: 12px/1.55 var(--mono); background: var(--code); padding: 10px 12px; border-radius: 8px; overflow-x: auto; white-space: pre-wrap; }
`;

const ICON = `<svg viewBox="0 0 24 24"><circle cx="6" cy="5" r="2"/><circle cx="6" cy="19" r="2"/><path d="M6 7v10"/><path d="M6 13c0-4 4-4 7-6"/><path d="M17.5 2.5l1.1 2.3 2.4.3-1.8 1.7.5 2.4-2.2-1.2-2.2 1.2.5-2.4-1.8-1.7 2.4-.3z"/></svg>`;
const SEARCH = `<svg viewBox="0 0 24 24"><circle cx="11" cy="11" r="7"/><path d="M20 20l-3.5-3.5"/></svg>`;
const PALETTE = ["var(--s1)", "var(--s2)", "var(--s3)", "var(--s4)", "var(--s5)", "var(--s7)", "var(--s8)", "var(--s6)"];
// GitHub's colors for the most common languages; others get one from the palette.
const LANG_COLORS = { Rust: "#dea584", Python: "#3572A5", JavaScript: "#f1e05a", TypeScript: "#3178c6", Go: "#00ADD8", Java: "#b07219", "C++": "#f34b7d", C: "#555555", "C#": "#178600", Ruby: "#701516", PHP: "#4F5D95", Swift: "#F05138", Kotlin: "#A97BFF", Scala: "#c22d40", Haskell: "#5e5086", Elixir: "#6e4a7e", Zig: "#ec915c", Julia: "#a270ba", Dart: "#00B4AB", Lua: "#000080", Shell: "#89e051", "Jupyter Notebook": "#DA5B0B", Vue: "#41b883", Svelte: "#ff3e00", HTML: "#e34c26", CSS: "#563d7c", "Objective-C": "#438eff", Nim: "#ffc200", OCaml: "#ef7a08", Clojure: "#db5855", Perl: "#0298c3", R: "#198CE7", Erlang: "#B83998", Elm: "#60B5CC", Crystal: "#000100", Solidity: "#AA6746", Gleam: "#ffaff3" };

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
  for (const [k, v] of Object.entries(attrs || {})) if (v != null) e.setAttribute(k, v);
  for (const k of kids.flat(Infinity)) if (k != null) e.append(k instanceof Node ? k : document.createTextNode(String(k)));
  return e;
}
const enc = encodeURIComponent;
const fmt = (n) => (n == null ? "" : Number(n).toLocaleString("en-US"));
const short = (n) => { n = Number(n) || 0; const a = Math.abs(n); return a >= 1e9 ? (n / 1e9).toFixed(1) + "B" : a >= 1e6 ? (n / 1e6).toFixed(a >= 1e7 ? 0 : 1) + "M" : a >= 1e4 ? Math.round(n / 1e3) + "k" : a >= 1e3 ? (n / 1e3).toFixed(1) + "k" : String(Math.round(n)); };
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const iso = (t) => (t ? new Date(t * 1000).toISOString().slice(0, 10) : "");
const dm = (t) => { if (!t) return ""; const d = new Date(t * 1000); return `${d.getUTCDate()} ${MONTHS[d.getUTCMonth()]}`; };
const dmy = (t) => { if (!t) return ""; const d = new Date(t * 1000); return `${d.getUTCDate()} ${MONTHS[d.getUTCMonth()]} ${d.getUTCFullYear()}`; };
const my = (t) => { if (!t) return ""; const d = new Date(t * 1000); return `${MONTHS[d.getUTCMonth()]} ${d.getUTCFullYear()}`; };
const yearOf = (t) => new Date(t * 1000).getUTCFullYear();
function esc(s) { return String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]); }
function skel(n = 5) { return Array.from({ length: n }, (_, i) => h("div", { class: "gx-skel", style: { width: `${60 + ((i * 37) % 40)}%` } })); }
function sec(title, right) { return h("div", { class: "gx-sec" }, h("h3", {}, title), right || null); }
function card(title, right, ...kids) { return h("div", { class: "gx-card" }, h("h4", {}, h("span", {}, title), right || null), kids); }
const langColor = (l, i = 0) => LANG_COLORS[l] || PALETTE[i % PALETTE.length];
function lang(l) { return l ? h("span", { class: "gx-lang" }, h("i", { style: { background: langColor(l) } }), l) : null; }
function word(w) { return w ? h("span", { class: `gx-word ${w}` }, w) : null; }
function topics(t) { return String(t || "").split(",").map((x) => x.trim()).filter(Boolean); }

// --------------------------------------------------------------- charts

let tip;
function showTip(ev, html) {
  if (!tip) { tip = h("div", { class: "gx-tip" }); document.body.append(tip); }
  tip.innerHTML = html;
  tip.style.display = "block";
  const x = Math.min(ev.clientX + 14, innerWidth - tip.offsetWidth - 10);
  const y = Math.min(ev.clientY + 14, innerHeight - tip.offsetHeight - 10);
  tip.style.left = x + "px"; tip.style.top = y + "px";
}
function hideTip() { if (tip) tip.style.display = "none"; }

// A sparkline of [[t, n]] points.
function spark(points, { w = 90, hgt = 22 } = {}) {
  const g = svg("svg", { class: "gx-sp", width: w, height: hgt, viewBox: `0 0 ${w} ${hgt}` });
  if (!points || points.length < 2) return g;
  const xs = points.map((p) => p[0]), ys = points.map((p) => p[1]);
  const x0 = Math.min(...xs), x1 = Math.max(...xs), ym = Math.max(1, ...ys);
  const X = (x) => ((x - x0) / Math.max(1, x1 - x0)) * (w - 2) + 1;
  const Y = (y) => hgt - 2 - (y / ym) * (hgt - 4);
  const d = points.map((p, i) => `${i ? "L" : "M"}${X(p[0]).toFixed(1)},${Y(p[1]).toFixed(1)}`).join("");
  g.append(svg("path", { class: "ar", d: `${d}L${X(x1)},${hgt}L${X(x0)},${hgt}Z` }), svg("path", { d }));
  return g;
}

// Lines over time: series = [{name, color, points: [[x, y]]}]; x is seconds
// (time) or a year; marks = [{x, n}] numbered markers.
function lineChart(series, { height = 220, fy = short, fx = (x) => String(x), marks = [], tipx = fx, area = false } = {}) {
  const W = 680, H = height, L = 46, R = 14, T = marks.length ? 22 : 10, B = 24;
  const xs = series.flatMap((s) => s.points.map((p) => p[0]));
  const ys = series.flatMap((s) => s.points.map((p) => p[1]));
  if (!xs.length) return h("div", { class: "gx-empty" }, "No data yet");
  const x0 = Math.min(...xs), x1 = Math.max(...xs), ymax = Math.max(1e-9, ...ys) * 1.08;
  const X = (x) => L + ((x - x0) / Math.max(1, x1 - x0)) * (W - L - R);
  const Y = (y) => T + (1 - y / ymax) * (H - T - B);
  const g = svg("svg", { class: "gx-chart", viewBox: `0 0 ${W} ${H}`, preserveAspectRatio: "none", style: `height:${H}px` });
  for (let i = 0; i <= 4; i++) {
    const v = (ymax / 1.08) * (i / 4), y = Y(v);
    g.append(svg("line", { class: "grid", x1: L, x2: W - R, y1: y, y2: y }), svg("text", { x: L - 6, y: y + 3.5, "text-anchor": "end" }, fy(v)));
  }
  const ticks = 6;
  for (let i = 0; i <= ticks; i++) { const x = x0 + ((x1 - x0) * i) / ticks; g.append(svg("text", { x: X(x), y: H - 6, "text-anchor": i === 0 ? "start" : i === ticks ? "end" : "middle" }, fx(x))); }
  series.forEach((s, i) => {
    const c = s.color || PALETTE[i % PALETTE.length];
    const pts = s.points.slice().sort((a, b) => a[0] - b[0]);
    if (!pts.length) return;
    const d = pts.map((p, j) => `${j ? "L" : "M"}${X(p[0]).toFixed(1)},${Y(p[1]).toFixed(1)}`).join("");
    if (area || series.length === 1) g.append(svg("path", { class: "ar", d: `${d}L${X(pts[pts.length - 1][0])},${Y(0)}L${X(pts[0][0])},${Y(0)}Z`, fill: c }));
    g.append(svg("path", { class: "ln", d, stroke: c }));
  });
  marks.forEach((m) => {
    const x = X(Math.max(x0, Math.min(x1, m.x)));
    g.append(svg("line", { class: "mk", x1: x, x2: x, y1: T, y2: H - B }), svg("circle", { class: "mkc", cx: x, cy: T - 9, r: 8 }), svg("text", { class: "mkt", x, y: T - 5.5, "text-anchor": "middle" }, String(m.n)));
  });
  const hov = svg("line", { class: "hov", y1: T, y2: H - B, x1: -10, x2: -10 });
  g.append(hov);
  const rect = svg("rect", { x: L, y: T, width: W - L - R, height: H - T - B, fill: "transparent" });
  rect.addEventListener("mousemove", (ev) => {
    const b = g.getBoundingClientRect();
    const px = ((ev.clientX - b.left) / b.width) * W;
    const xv = x0 + ((px - L) / (W - L - R)) * (x1 - x0);
    let best = null;
    for (const s of series) for (const p of s.points) if (!best || Math.abs(p[0] - xv) < Math.abs(best - xv)) best = p[0];
    if (best == null) return;
    hov.setAttribute("x1", X(best)); hov.setAttribute("x2", X(best));
    const rows = series.map((s, i) => { const p = s.points.find((q) => q[0] === best); return p ? `<div><i style="display:inline-block;width:8px;height:8px;border-radius:2px;background:${s.color || PALETTE[i % PALETTE.length]};margin-right:6px"></i>${esc(s.name)}: <b style="display:inline">${fy(p[1])}</b></div>` : ""; }).join("");
    showTip(ev, `<b>${esc(tipx(best))}</b>${rows}`);
  });
  rect.addEventListener("mouseleave", () => { hideTip(); hov.setAttribute("x1", -10); hov.setAttribute("x2", -10); });
  g.append(rect);
  const wrap = h("div", {}, g);
  if (series.length > 1) wrap.prepend(h("div", { class: "gx-legend" }, series.map((s, i) => h("span", {}, h("i", { style: { background: s.color || PALETTE[i % PALETTE.length] } }), s.name))));
  return wrap;
}

// Bars of [[x, n]], x seconds or years.
function bars(points, { height = 120, fx = (x) => String(x), label = "" } = {}) {
  const W = 680, H = height, L = 40, R = 6, T = 6, B = 20;
  if (!points.length) return h("div", { class: "gx-empty" }, "No data yet");
  const ym = Math.max(1, ...points.map((p) => p[1])) * 1.05;
  const bw = (W - L - R) / points.length;
  const g = svg("svg", { class: "gx-chart", viewBox: `0 0 ${W} ${H}`, preserveAspectRatio: "none", style: `height:${H}px` });
  for (let i = 0; i <= 2; i++) { const v = (ym / 1.05) * (i / 2), y = T + (1 - v / ym) * (H - T - B); g.append(svg("line", { class: "grid", x1: L, x2: W - R, y1: y, y2: y }), svg("text", { x: L - 6, y: y + 3.5, "text-anchor": "end" }, short(v))); }
  points.forEach((p, i) => {
    const hh = (p[1] / ym) * (H - T - B);
    const r = svg("rect", { class: "bar", x: L + i * bw + bw * 0.12, y: H - B - hh, width: Math.max(1, bw * 0.76), height: Math.max(0.5, hh), rx: 1.5 });
    r.addEventListener("mousemove", (ev) => showTip(ev, `<b>${esc(fx(p[0]))}</b>${fmt(p[1])} ${esc(label)}`));
    r.addEventListener("mouseleave", hideTip);
    g.append(r);
  });
  const every = Math.max(1, Math.ceil(points.length / 8));
  points.forEach((p, i) => { if (i % every === 0) g.append(svg("text", { x: L + i * bw + bw / 2, y: H - 5, "text-anchor": "middle" }, fx(p[0]))); });
  return g;
}

function hbars(rows, { name = "name", n = "n", href = null, fmtn = fmt } = {}) {
  const max = Math.max(1, ...rows.map((r) => r[n]));
  return h("div", { class: "gx-hbar" }, rows.map((r) => [
    href ? h("a", { href: href(r) }, r[name] || "(none)") : h("span", {}, r[name] || "(none)"),
    h("div", { class: "b" }, h("i", { style: { width: `${(r[n] / max) * 100}%` } })),
    h("span", { class: "n" }, fmtn(r[n])),
  ]));
}

// --------------------------------------------------------------- pieces

function repoLink(app, name) {
  const [o, n] = String(name || "").split("/");
  return h("a", { href: app.href("repo", o, n || "") }, `${o}/`, h("b", {}, n || ""));
}

// One repo in a list: its name, what it says it does, and its numbers.
function repoItem(app, r, { spark: sp = null, extra = null, why = null, compact = false } = {}) {
  return h("li", { class: `gx-r${compact ? " compact" : ""}` },
    h("div", { class: "nm" }, repoLink(app, r.name)),
    r.description ? h("div", { class: "ds" }, r.description) : null,
    h("div", { class: "mt" },
      extra,
      h("span", { title: "stars in the archive" }, `★ ${short(r.stars)}`),
      lang(r.language),
      r.forks ? h("span", {}, `${short(r.forks)} forks`) : null,
      r.first_seen ? h("span", {}, `since ${my(r.first_seen)}`) : null,
      r.archived ? word("archived") : null,
      sp),
    why);
}

function whyLine(why) {
  if (!why || !why.length) return null;
  return h("div", { class: "why" }, why.map((w, i) => [i ? " · " : "", w.link ? h("a", { href: w.link, target: "_blank", rel: "noopener" }, w.text) : w.text]));
}

// --------------------------------------------------------------- render

function render(el, ctx) {
  if (!document.getElementById("gx-css")) document.head.append(h("style", { id: "gx-css" }, CSS));
  const app = {
    ctx,
    queries: [],
    took: 0,
    status: null,
    api: async (path, opts) => {
      const j = await ctx.api(path, opts);
      if (j && Array.isArray(j.queries)) { app.queries = j.queries; app.took = j.took_ms; drawSql(); }
      return j;
    },
    href: (...parts) => ctx.href(...parts),
    go: (...parts) => { location.hash = ctx.href(...parts); },
  };
  const root = h("div", { class: "gx" });
  fill(el, root);
  const input = h("input", { type: "search", placeholder: "Search public repos by what they do, or ask: “fastest growing rust repos this year”", autocomplete: "off", spellcheck: "false", "aria-label": "Search GitHub repos" });
  const nav = h("nav", { class: "gx-nav" });
  const live = h("span", { class: "live" });
  const box = h("div", { class: "gx-box" }, input, h("span", { class: "kbd" }, "/"));
  box.insertAdjacentHTML("afterbegin", SEARCH);
  const top = h("div", { class: "gx-top" }, h("div", { class: "gx-bar" }, h("a", { class: "gx-brand", href: app.href(""), html: `${ICON}<span>GitHub Explorer <small>on ChloeDB</small></span>` }), box), nav);
  const main = h("main", { class: "gx-main" });
  const sqlBtn = h("button", { onclick: () => sqlPanel.classList.toggle("on"), title: "The SQL behind this page" });
  const sqlPanel = h("div", { class: "gx-sqlp" });
  root.append(top, main, h("div", { class: "gx-sql" }, sqlBtn), sqlPanel,
    h("div", { class: "gx-foot" }, "Public events from GH Archive (gharchive.org), kept in ChloeDB tables. Public data only: no emails or contact details are kept. Not affiliated with GitHub. ",
      h("a", { href: app.href("about") }, "How this works")));

  function drawSql() {
    const ms = app.queries.reduce((a, q) => a + (q.ms || 0), 0);
    fill(sqlBtn, "SQL · ", h("b", {}, String(app.queries.length)), ` ${app.queries.length === 1 ? "query" : "queries"} · ${Math.round(app.took || ms)} ms`);
    fill(sqlPanel, h("div", { style: { display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "6px" } },
      h("b", {}, "The SQL behind this page"), h("button", { class: "gx-btn ghost", onclick: () => sqlPanel.classList.remove("on") }, "Close")),
      app.queries.map((q) => h("div", { class: "q" },
        h("div", { class: "w" }, q.what, h("span", {}, `${fmt(q.rows)} rows · ${(q.ms ?? 0).toFixed(1)} ms`)),
        q.note ? h("div", { class: "bad" }, q.note) : null,
        h("pre", {}, q.sql, q.params && Object.keys(q.params).length ? `\n-- ${JSON.stringify(q.params).slice(0, 600)}` : ""))));
  }
  drawSql();

  const PAGES = [["", "Search"], ["rising", "Rising"], ["trends", "Trends"], ["ask", "Ask"], ["foryou", "For you"], ["weekly", "Weekly"], ["about", "How it works"]];
  function drawNav(page) {
    fill(nav, ...PAGES.map(([p, t]) => h("a", { href: app.href(p), class: page === p || (p === "" && ["search", "repo", "person"].includes(page)) ? "on" : "" }, t)), h("span", { class: "sp" }), live);
  }

  function search(q) {
    q = q.trim();
    app.go(...(q ? ["search", q] : [""]));
  }
  input.addEventListener("keydown", (e) => { if (e.key === "Enter") { e.preventDefault(); search(input.value); } });
  document.addEventListener("keydown", (e) => { if (e.key === "/" && document.activeElement !== input && !/INPUT|TEXTAREA|SELECT/.test(document.activeElement?.tagName || "")) { e.preventDefault(); input.focus(); input.select(); } });

  let current = null, seq = 0;
  async function show(path) {
    path = path || ctx.path() || [];
    current = path;
    const me = ++seq;
    const alive = () => me === seq;
    const [page, ...rest] = path;
    drawNav(page || "");
    hideTip();
    scrollTo({ top: 0 });
    if (page !== "search") input.value = "";
    const arg = rest.join("/");
    try {
      switch (page || "") {
        case "": return await home(app, main, alive);
        case "search": input.value = arg; return await searchPage(app, main, arg, alive);
        case "repo": return await repoPage(app, main, arg, alive);
        case "person": return await personPage(app, main, arg, alive);
        case "rising": return await risingPage(app, main, rest, alive);
        case "trends": return await trendsPage(app, main, rest, alive);
        case "ask": return await askPage(app, main, arg, alive);
        case "foryou": return await forYouPage(app, main, arg, alive);
        case "weekly": return await weeklyPage(app, main, arg, alive);
        case "about": return await aboutPage(app, main, alive);
        default: fill(main, h("div", { class: "gx-empty" }, "No such page."));
      }
    } catch (e) {
      if (alive()) fill(main, h("div", { class: "gx-err" }, String(e.message || e)));
    }
  }
  ctx.onPath((p) => { if (JSON.stringify(p) !== JSON.stringify(current)) show(p); });
  ctx.api("/v1/status").then((s) => {
    app.status = s;
    fill(live, s.latest ? [h("span", { class: "gx-dot" }), `events to ${dmy(s.latest)}`] : "");
  }).catch(() => {});
  show(ctx.path());
}

// ------------------------------------------------------------------ home

const TRY = [
  ["choosing a library", "a fast key-value store in rust that speaks redis"],
  ["replacing one", "alternatives to moment/moment"],
  ["scouting", "fastest growing python repos this month"],
  ["asking", "which rust repos gained the most stars in 2024"],
  ["looking back", "languages by stars in 2015"],
  ["for you", "star next for your GitHub login"],
];

async function home(app, main, alive) {
  fill(main, h("div", { class: "gx-hero" },
    h("h1", {}, "Every GitHub repo, ", h("span", {}, "and why it moves"), "."),
    h("p", {}, "Find a repo by what it does, see why it is trending, what people use instead, who keeps it going and what to star next, from every public event since 2011."),
    h("div", { class: "gx-stats", id: "gx-stats" }),
    h("div", { class: "gx-try" }, TRY.map(([k, q]) => h("a", { href: k === "for you" ? app.href("foryou") : app.href("search", q), title: k }, q)))),
    h("div", { class: "gx-ch3" }, h("div", { id: "c1" }, skel(8)), h("div", { id: "c2" }, skel(8)), h("div", { id: "c3" }, skel(8))));
  const j = await app.api("/v1/home");
  if (!alive()) return;
  const st = main.querySelector("#gx-stats");
  const s = app.status;
  if (s) fill(st, h("span", {}, h("b", {}, short(s.repos)), "repos with 2+ stars"), h("span", {}, h("b", {}, short(s.people)), "people"), h("span", {}, h("b", {}, "2011"), "since"), h("span", {}, "newest ", h("b", {}, iso(j.latest))));
  fill(main.querySelector("#c1"), sec("Breaking out this week", h("a", { href: app.href("rising") }, "All rising")),
    j.rising.length ? h("ul", { class: "gx-list" }, j.rising.slice(0, 8).map((r) => repoItem(app, r, {
      extra: h("span", { class: "up" }, `+${fmt(r.window)} ★`),
      spark: spark(r.daily),
      why: whyLine(r.why),
    }))) : h("div", { class: "gx-empty" }, "No stars this week yet"));
  fill(main.querySelector("#c2"), sec("Ask the archive", h("a", { href: app.href("ask") }, "Ask")),
    h("ul", { class: "gx-ex" }, (s?.examples || []).slice(0, 6).map((q) => h("li", {}, h("a", { href: app.href("ask", q) }, q[0].toUpperCase() + q.slice(1))))),
    sec("Most starred, ever"), h("ul", { class: "gx-list" }, j.top.slice(0, 6).map((r) => repoItem(app, r, { compact: true }))));
  fill(main.querySelector("#c3"), sec("Languages rising", h("a", { href: app.href("trends") }, "Trends")),
    h("div", { class: "gx-vs" }, j.languages.slice(0, 7).map((l) => h("div", { class: "row" },
      h("span", { class: "k" }, h("a", { href: app.href("trends", "language", l.name) }, l.name), h("small", {}, `${fmt(l.stars)} stars in the last 12 months, ${l.share}% of all`)),
      h("span", { class: `v gx-chg ${l.change >= 0 ? "up" : "down"}` }, l.change >= 999 ? "new" : `${l.change >= 0 ? "+" : ""}${l.change}%`)))),
    card("Star next", h("a", { href: app.href("foryou") }, "Open"), h("div", { class: "gx-lead", style: { margin: 0 } }, "Type a GitHub login and get repos picked from that account's public stars and work, with the people behind each pick.")));
}

// ---------------------------------------------------------------- search

const SORTS = [["", "Best"], ["stars", "Stars"], ["rising", "Rising"], ["new", "New"]];

async function searchPage(app, main, arg, alive) {
  const [q0, qs] = arg.split("?");
  const params = new URLSearchParams(qs || "");
  const q = q0;
  fill(main, skel(10));
  // A question goes to Ask, a repo or a person opens it.
  const a = await app.api(`/v1/ask?q=${enc(q)}`);
  if (!alive()) return;
  if (a.open?.repo) { const [o, n] = a.open.repo.split("/"); history.replaceState(null, "", app.href("repo", o, n)); return repoPage(app, main, a.open.repo, alive, a.open.tab); }
  if (a.open?.person) { history.replaceState(null, "", app.href("person", a.open.person)); return personPage(app, main, a.open.person, alive); }
  if (a.kind !== "search") return drawAnswer(app, main, a);
  const sort = params.get("sort") || "", lng = params.get("lang") || "", page = Number(params.get("page") || 0);
  const j = await app.api(`/v1/search?q=${enc(q)}&sort=${enc(sort)}&lang=${enc(lng)}&page=${page}`);
  if (!alive()) return;
  const go = (o) => { const p = new URLSearchParams({ sort, lang: lng, ...o }); for (const [k, v] of [...p]) if (!v || v === "0") p.delete(k); location.hash = app.href("search", q) + (p.toString() ? `?${p}` : ""); };
  fill(main, h("div", { class: "gx-grid two" },
    h("div", {},
      h("div", { class: "gx-head" }, h("span", { class: "t" }, h("b", {}, fmt(j.total)), " repos with the words, and more by meaning"), h("span", { class: "sp" }),
        h("div", { class: "gx-seg" }, SORTS.map(([k, t]) => h("button", { class: sort === k ? "on" : "", onclick: () => go({ sort: k, page: 0 }) }, t)))),
      lng ? h("div", { class: "gx-und" }, "Only ", h("span", { class: "gx-chip f", onclick: () => go({ lang: "", page: 0 }) }, lng, h("span", { class: "x" }, " ×"))) : null,
      j.repos.length ? h("ul", { class: "gx-list" }, j.repos.map((r) => repoItem(app, r, { spark: spark(j.spark[r.repo] || [], { w: 70, hgt: 18 }) }))) : h("div", { class: "gx-empty" }, "Nothing does that, in words or in meaning."),
      h("div", { class: "gx-row", style: { justifyContent: "center", gap: "8px", marginTop: "14px" } },
        page > 0 ? h("button", { class: "gx-btn", onclick: () => go({ page: page - 1 }) }, "Previous") : null,
        j.repos.length === 30 ? h("button", { class: "gx-btn", onclick: () => go({ page: page + 1 }) }, "More") : null)),
    h("div", { class: "gx-right" },
      card("Languages", null, hbars(j.languages.slice(0, 12).map((l) => ({ name: l.language, n: l.n })), { href: (r) => { const p = new URLSearchParams({ lang: r.name }); return app.href("search", q) + `?${p}`; } })),
      card("How it searched", null, h("div", { class: "gx-kv" },
        h("span", {}, "by words (name, description, topics)"), h("span", {}, fmt(j.candidates.words)),
        h("span", {}, "by meaning (a vector of the same)"), h("span", {}, fmt(j.candidates.meaning)),
        h("span", {}, "ranked with stars"), h("span", {}, "both"))))));
}

// ------------------------------------------------------------------ repo

async function repoPage(app, main, name, alive, tab0) {
  fill(main, skel(12));
  const parts = name.split("/");
  const j = await app.api(`/v1/repo/${parts.map(enc).join("/")}`);
  if (!alive()) return;
  if (!j.found) return fill(main, h("div", { class: "gx-empty" }, `No repo ${name} with two stars or more in the archive.`));
  const r = j.repo, v = j.vitals;
  const months = j.months || [];
  // Star history: stars summed month by month, with the jumps marked.
  let acc = 0;
  const cum = months.map((m) => [m.month, (acc += m.stars)]);
  const hn = (j.hn?.submissions || []).filter((s) => s.points >= 30);
  const events = [];
  for (const s of hn) events.push({ t: Date.parse(s.created) / 1000, kind: "hn", title: `“${s.title}” on Hacker News, ${fmt(s.points)} points`, url: s.url, sub: `${fmt(s.comments)} comments` });
  for (const m of j.spikes.months) if (!events.some((e) => Math.abs(e.t - m.month) < 40 * 86400)) events.push({ t: m.month, kind: "month", title: `${fmt(m.stars)} stars in ${my(m.month)}`, sub: `${Math.round(m.stars / Math.max(1, m.usual))}× a usual month${m.releases ? `, ${m.releases} release${m.releases > 1 ? "s" : ""}` : ""}` });
  events.sort((a, b) => a.t - b.t);
  const marks = events.slice(-8).map((e, i) => ({ ...e, n: i + 1 }));
  const kept = (j.people_year || []).slice(0, 3).map((p) => p.login).filter(Boolean);
  const owner = r.name.split("/")[0];
  const tabs = [
    ["alternatives", "Alternatives", j.alike.length],
    ["costarred", "Also starred", j.costarred.length],
    ["people", "People", (j.people_ever || []).length],
    ["names", "Names", (j.names || []).length],
  ];
  let tab = tab0 || "alternatives";
  const tabBody = h("div", {});
  const tabBar = h("div", { class: "gx-tabs" });
  const drawTabs = () => {
    fill(tabBar, tabs.map(([k, t, n]) => h("button", { class: tab === k ? "on" : "", onclick: () => { tab = k; drawTabs(); } }, t, h("span", { class: "n" }, n))));
    if (tab === "alternatives") fill(tabBody, h("div", { class: "gx-lead" }, "What does the same job, by the meaning of its name, description and topics, with how starred each is."), h("ul", { class: "gx-list" }, j.alike.map((x) => repoItem(app, x))));
    else if (tab === "costarred") fill(tabBody, h("div", { class: "gx-lead" }, `What the last ${fmt(j.stargazers_sampled)} people who starred it starred too, against how starred each repo is anyway.`), h("ul", { class: "gx-list" }, j.costarred.map((x) => repoItem(app, x, { extra: h("span", { class: "up" }, `${fmt(x.shared)} shared`) }))));
    else if (tab === "people") fill(tabBody, h("div", { class: "gx-lead" }, "Who did the most work in it ever: pushes, pull requests, issues, reviews, comments and releases."), h("table", { class: "gx-tbl" }, h("tr", {}, h("th", {}, "who"), h("th", { class: "r" }, "events"), h("th", {}, "first"), h("th", {}, "last")),
      (j.people_ever || []).map((p) => h("tr", {}, h("td", {}, p.login ? h("a", { href: app.href("person", p.login) }, p.login) : `#${p.actor}`), h("td", { class: "r" }, fmt(p.n)), h("td", {}, dmy(p.first)), h("td", {}, dmy(p.last))))));
    else fill(tabBody, h("div", { class: "gx-lead" }, "The names it went by, as the events saw them."), h("table", { class: "gx-tbl" }, h("tr", {}, h("th", {}, "name"), h("th", {}, "first seen")), (j.names || []).map((n) => h("tr", {}, h("td", {}, n.name), h("td", {}, dmy(n.first_seen))))));
  };
  drawTabs();
  const total = (v.year?.pushes || 0) + (v.year?.prs || 0) + (v.year?.issues || 0);
  const people = j.people_year || [];
  const work = people.reduce((a, p) => a + p.n, 0);
  fill(main, h("div", { class: "gx-grid two" },
    h("div", {},
      h("div", { class: "gx-crumb" }, h("code", {}, `github.com/${r.name}`), lang(r.language), r.license ? h("span", { class: "gx-chip c" }, r.license) : null, r.first_seen ? h("span", {}, `public since ${my(r.first_seen)}`) : null, word(v.word)),
      h("h1", { class: "gx-h1" }, r.name),
      h("p", { class: "gx-sub" }, r.description || h("i", {}, "No description."), kept.length ? [" Kept going by ", kept.map((k, i) => [i ? ", " : "", h("a", { href: app.href("person", k) }, k)]), people.length > kept.length ? ` and ${people.length - kept.length} more this year.` : "."] : null),
      h("div", { class: "gx-row", style: { gap: "8px", flexWrap: "wrap" } },
        h("a", { class: "gx-btn pri", href: `https://github.com/${r.name}`, target: "_blank", rel: "noopener" }, "Open on GitHub"),
        r.homepage ? h("a", { class: "gx-btn", href: /^https?:/.test(r.homepage) ? r.homepage : `https://${r.homepage}`, target: "_blank", rel: "noopener nofollow" }, "Homepage") : null,
        h("a", { class: "gx-btn", href: app.href("person", owner) }, owner)),
      topics(r.topics).length ? h("div", { class: "gx-row", style: { gap: "6px", flexWrap: "wrap", marginTop: "12px" } }, topics(r.topics).slice(0, 12).map((t) => h("a", { class: "gx-chip t", href: app.href("search", t) }, t))) : null,
      sec("Star history, and why it jumped", h("span", { class: "note" }, "stars counted from the events since 2011")),
      h("div", { class: "gx-card" },
        lineChart([{ name: "stars", points: cum }], { fx: (x) => my(x), tipx: (x) => my(x), marks }),
        marks.length ? h("ol", { class: "gx-spk" }, marks.map((m) => h("li", {}, h("span", { class: "no" }, m.n), h("span", { class: "d" }, dmy(m.t)), h("span", {}, m.url ? h("a", { href: m.url, target: "_blank", rel: "noopener" }, m.title) : m.title, h("small", {}, m.sub))))) : null),
      sec("The last 120 days", h("span", { class: "note" }, "stars a day")),
      h("div", { class: "gx-card" }, bars((j.days || []).map((d) => [d.day, d.stars]), { fx: (x) => dm(x), label: "stars" })),
      tabBar, tabBody),
    h("div", { class: "gx-right" },
      h("div", { class: "gx-tiles" },
        h("div", { class: "gx-tile" }, h("b", {}, short(r.stars)), h("span", {}, "stars")),
        h("div", { class: "gx-tile" }, h("b", {}, short(r.forks)), h("span", {}, "forks")),
        h("div", { class: "gx-tile" }, h("b", {}, fmt(v.people_year)), h("span", {}, "people this year"))),
      card("Vital signs", null, h("div", { class: "gx-vs" },
        h("div", { class: "row" }, h("span", { class: "k" }, "Is it alive", h("small", {}, v.last_work ? `last work ${dmy(v.last_work)}` : "no work seen")), h("span", { class: `v ${v.word === "active" ? "good" : v.word === "dormant" || v.word === "slowing" ? "bad" : ""}` }, v.word)),
        h("div", { class: "row" }, h("span", { class: "k" }, "Work this year", h("small", {}, "pushes, pull requests and issues, against the year before")), h("span", { class: "v" }, `${short(total)} `, h("span", { class: `gx-chg ${total >= ((v.year_before?.pushes || 0) + (v.year_before?.prs || 0) + (v.year_before?.issues || 0)) ? "up" : "down"}` }, `(${short((v.year_before?.pushes || 0) + (v.year_before?.prs || 0) + (v.year_before?.issues || 0))})`))),
        h("div", { class: "row" }, h("span", { class: "k" }, "Pull requests merged", h("small", {}, "of those opened this year")), h("span", { class: "v" }, v.year?.prs ? `${Math.round(v.merge_rate * 100)}%` : "–")),
        h("div", { class: "row" }, h("span", { class: "k" }, "Releases", h("small", {}, "this year")), h("span", { class: "v" }, fmt(v.year?.releases || 0))),
        h("div", { class: "row" }, h("span", { class: "k" }, "Bus factor", h("small", {}, "people who do half the work this year")), h("span", { class: `v ${v.half_by === 1 ? "bad" : ""}` }, v.half_by || "–")),
        h("div", { class: "row" }, h("span", { class: "k" }, "Stars this year", h("small", {}, "against the year before")), h("span", { class: "v" }, short(v.year?.stars || 0), " ", h("span", { class: `gx-chg ${(v.year?.stars || 0) >= (v.year_before?.stars || 0) ? "up" : "down"}` }, `(${short(v.year_before?.stars || 0)})`))))),
      people.length ? card("Who keeps it going", null, h("div", { class: "gx-ppl" }, people.slice(0, 6).map((p) => h("div", { class: "p" },
        h("div", { class: "l" }, p.login ? h("a", { href: app.href("person", p.login) }, p.login) : `#${p.actor}`, h("span", {}, `${Math.round((p.n / Math.max(1, work)) * 100)}%`)),
        h("div", { class: "b" }, h("i", { style: { width: `${(p.n / Math.max(1, people[0].n)) * 100}%` } })))))) : null,
      j.costarred.length ? card("People who starred it also starred", null, h("div", { class: "gx-kv" }, j.costarred.slice(0, 7).flatMap((x) => [repoLink(app, x.name), h("span", {}, `${fmt(x.shared)}`)]))) : null,
      j.hn?.submissions?.length ? card("On Hacker News", null, h("div", { class: "gx-kv" }, j.hn.submissions.slice().sort((a, b) => b.points - a.points).slice(0, 6).flatMap((s) => [h("a", { href: s.url, target: "_blank", rel: "noopener" }, s.title), h("span", {}, `${fmt(s.points)} pts`)]))) : null)));
}

// ---------------------------------------------------------------- person

async function personPage(app, main, login, alive) {
  fill(main, skel(10));
  const j = await app.api(`/v1/person/${enc(login)}`);
  if (!alive()) return;
  if (!j.found) return fill(main, h("div", { class: "gx-empty" }, `No public activity by ${login} in the archive.`));
  const item = (x, extra) => x.info ? repoItem(app, x.info, { extra, compact: true }) : h("li", { class: "gx-r compact" }, `repo #${x.repo}`, " ", extra);
  fill(main, h("div", { class: "gx-grid two" },
    h("div", {},
      h("div", { class: "gx-crumb" }, h("code", {}, `github.com/${j.login}`), j.first_seen ? h("span", {}, `first seen ${my(j.first_seen)}`) : null, j.last_seen ? h("span", {}, `last seen ${dmy(j.last_seen)}`) : null),
      h("h1", { class: "gx-h1" }, j.login || login),
      j.years?.length ? [sec("Work, year by year", h("span", { class: "note" }, "pushes, pull requests, issues, reviews, comments, releases")), h("div", { class: "gx-card" }, bars(j.years.map((y) => [y.year, y.n]), { label: "events" }))] : null,
      j.repos?.length ? [sec("Where they work"), h("ul", { class: "gx-list" }, j.repos.map((x) => item(x, h("span", { class: "up" }, `${fmt(x.n)} events`))))] : null,
      j.owned?.length ? [sec("Repos they own"), h("ul", { class: "gx-list" }, j.owned.map((r) => repoItem(app, r, { compact: true })))] : null),
    h("div", { class: "gx-right" },
      j.kinds?.length ? card("What they did", null, hbars(j.kinds.slice(0, 12).map((k) => ({ name: k.kind.replace(/_/g, " "), n: k.n })))) : null,
      j.starred?.length ? card("Starred lately", h("a", { href: app.href("foryou", j.login) }, "Star next"), h("ul", { class: "gx-list" }, j.starred.slice(0, 12).map((x) => item(x, h("span", {}, dm(x.t)))))) : null)));
}

// ---------------------------------------------------------------- rising

async function risingPage(app, main, rest, alive) {
  const days = Number(rest[0] || 7);
  const at = rest[1] || "";
  fill(main, skel(10));
  const j = await app.api(`/v1/rising?days=${days}${at ? `&at=${enc(at)}` : ""}`);
  if (!alive()) return;
  fill(main,
    h("div", { class: "gx-head" }, h("span", { class: "t" }, "Repos whose stars jumped in the ", h("b", {}, days === 1 ? "last day" : `last ${days} days`), ` to ${dmy(j.at)}, against the four weeks before`), h("span", { class: "sp" }),
      h("div", { class: "gx-seg" }, [[1, "Day"], [7, "Week"], [30, "Month"]].map(([d, t]) => h("button", { class: days === d ? "on" : "", onclick: () => app.go("rising", String(d)) }, t)))),
    j.repos.length ? h("ul", { class: "gx-list" }, j.repos.map((r) => repoItem(app, r, {
      extra: [h("span", { class: "up" }, `+${fmt(r.window)} ★`), h("span", {}, `${r.spike}× usual`)],
      spark: spark(r.daily, { w: 110, hgt: 22 }),
      why: whyLine(r.why),
    }))) : h("div", { class: "gx-empty" }, "No stars in the window yet."));
}

// ---------------------------------------------------------------- trends

async function trendsPage(app, main, rest, alive) {
  const kind = rest[0] === "topic" ? "topic" : "language";
  const names = (rest[1] || "").split(",").filter(Boolean);
  fill(main, skel(10));
  const j = await app.api(`/v1/trends?kind=${kind}&names=${enc(names.join(","))}`);
  if (!alive()) return;
  const pick = (n) => { const set = new Set(j.picked); set.has(n) ? set.delete(n) : set.add(n); app.go("trends", kind, [...set].join(",")); };
  const label = kind === "topic" ? "topics" : "languages";
  fill(main,
    h("div", { class: "gx-head" }, h("span", { class: "t" }, `Each one's share of the stars GitHub's ${label} got, year by year`), h("span", { class: "sp" }),
      h("div", { class: "gx-seg" }, [["language", "Languages"], ["topic", "Topics"]].map(([k, t]) => h("button", { class: kind === k ? "on" : "", onclick: () => app.go("trends", k) }, t)))),
    h("div", { class: "gx-card" }, lineChart(j.lines.map((l, i) => ({ name: l.name, color: kind === "language" ? langColor(l.name, i) : undefined, points: l.share })), { fy: (v) => `${v.toFixed(v < 1 ? 1 : 0)}%`, height: 260 })),
    h("div", { class: "gx-row", style: { gap: "6px", flexWrap: "wrap", margin: "8px 0 16px" } }, j.top.slice(0, 24).map((t) => h("span", { class: `gx-chip${j.picked.includes(t.k) ? " f" : ""}`, onclick: () => pick(t.k) }, t.k))),
    h("div", { class: "gx-ch3" },
      h("div", {}, sec("Growing fastest", h("span", { class: "note" }, "share of stars, last 12 months against the 12 before")),
        h("div", { class: "gx-vs" }, j.rising.map((l) => h("div", { class: "row" }, h("span", { class: "k" }, h("a", { href: app.href("trends", kind, l.name) }, l.name), h("small", {}, `${l.share_before}% → ${l.share}% of stars`)), h("span", { class: "v gx-chg up" }, l.change >= 999 ? "new" : `+${l.change}%`))))),
      h("div", {}, sec("Shrinking"),
        h("div", { class: "gx-vs" }, j.falling.map((l) => h("div", { class: "row" }, h("span", { class: "k" }, h("a", { href: app.href("trends", kind, l.name) }, l.name), h("small", {}, `${l.share_before}% → ${l.share}% of stars`)), h("span", { class: "v gx-chg down" }, `${l.change}%`))))),
      h("div", {}, sec("Most stars, last 12 months"), hbars(j.top.slice(0, 14).map((t) => ({ name: t.k, n: t.s })), { href: (r) => app.href("trends", kind, r.name), fmtn: short }))));
}

// ------------------------------------------------------------------- ask

function drawAnswer(app, main, a) {
  const head = h("div", { class: "gx-ans" },
    h("div", { class: "ey" }, "Answer", h("span", { class: "nm" }, "computed in SQL: see the SQL button")),
    h("div", { class: "big" }, a.error || (a.understood ? a.understood[0].toUpperCase() + a.understood.slice(1) : a.q)));
  let body = null;
  const rows = a.rows || [];
  if (a.chart === "repos") body = h("ul", { class: "gx-list" }, rows.map((r) => repoItem(app, r, { extra: r.gained != null ? h("span", { class: "up" }, `+${fmt(r.gained)} ${a.gained === "commits" ? "commits" : "★"}`) : null })));
  else if (a.chart === "people") body = h("table", { class: "gx-tbl" }, h("tr", {}, h("th", {}, "who"), h("th", { class: "r" }, "events"), h("th", {}, "first"), h("th", {}, "last")), rows.map((p) => h("tr", {}, h("td", {}, p.login ? h("a", { href: app.href("person", p.login) }, p.login) : `#${p.actor}`), h("td", { class: "r" }, fmt(p.n)), h("td", {}, dmy(p.first)), h("td", {}, dmy(p.last)))));
  else if (a.chart === "years") body = [a.total != null ? h("div", { class: "gx-lead" }, h("b", {}, fmt(a.total)), ` ${a.what} in all`) : null, h("div", { class: "gx-card" }, bars(rows.map((r) => [r.year, r.n]), { label: a.what || "", height: 180 }))];
  else if (a.chart === "bars") body = h("div", { class: "gx-card" }, hbars(rows.map((r) => ({ name: r.name, n: r.n })), { fmtn: short }));
  else if (a.chart === "days") body = h("table", { class: "gx-tbl" }, h("tr", {}, h("th", {}, "day"), h("th", { class: "r" }, "stars")), rows.map((r) => h("tr", {}, h("td", {}, dmy(r.day)), h("td", { class: "r" }, fmt(r.n)))));
  fill(main, head, body);
}

async function askPage(app, main, q, alive) {
  const field = h("input", { value: q, placeholder: "Ask about GitHub's history: “who works on rust-lang/rust”", "aria-label": "Ask" });
  const out = h("div", {});
  const submit = () => { if (field.value.trim()) app.go("ask", field.value.trim()); };
  field.addEventListener("keydown", (e) => { if (e.key === "Enter") submit(); });
  fill(main, h("div", { class: "gx-form" }, field, h("button", { class: "gx-btn pri", onclick: submit }, "Ask")), out);
  if (!q) {
    const ex = app.status?.examples || (await app.api("/v1/status")).examples || [];
    if (!alive()) return;
    fill(out, h("div", { class: "gx-lead" }, "Questions are read by patterns, not a model, and answered by one or two SQL statements over every public event since 2011. These are the kinds it knows:"),
      h("ul", { class: "gx-ex" }, ex.map((x) => h("li", {}, h("a", { href: app.href("ask", x) }, x)))));
    return;
  }
  fill(out, skel(8));
  const a = await app.api(`/v1/ask?q=${enc(q)}`);
  if (!alive()) return;
  if (a.open?.repo) { const [o, n] = a.open.repo.split("/"); return app.go("repo", o, n); }
  if (a.open?.person) return app.go("person", a.open.person);
  if (a.open?.search) return app.go("search", a.open.search);
  drawAnswer(app, out, a);
}

// --------------------------------------------------------------- for you

async function forYouPage(app, main, login, alive) {
  const field = h("input", { value: login, placeholder: "A GitHub login", "aria-label": "GitHub login" });
  const out = h("div", {});
  const submit = () => { if (field.value.trim()) app.go("foryou", field.value.trim().replace(/^@/, "")); };
  field.addEventListener("keydown", (e) => { if (e.key === "Enter") submit(); });
  fill(main, h("div", { class: "gx-lead" }, "Repos to star next, from what a login starred and worked on in public: the people who starred the same repos, and what they starred that is not yet starred here, against how starred each repo is anyway."),
    h("div", { class: "gx-form" }, field, h("button", { class: "gx-btn pri", onclick: submit }, "Pick")), out);
  if (!login) return;
  fill(out, skel(10));
  const j = await app.api(`/v1/foryou?login=${enc(login)}`);
  if (!alive()) return;
  if (!j.found) return fill(out, h("div", { class: "gx-empty" }, `No public activity by ${login} in the archive.`));
  fill(out, h("div", { class: "gx-grid two" },
    h("div", {}, sec(`Picked for ${j.login}`, h("span", { class: "note" }, `from ${fmt(j.neighbors)} people with the same stars`)),
      j.picks.length ? h("ul", { class: "gx-list" }, j.picks.map((r) => repoItem(app, r, { extra: h("span", { class: "up" }, `${fmt(r.shared)} of them starred it`) }))) : h("div", { class: "gx-empty" }, "Not enough public stars to go on.")),
    h("div", { class: "gx-right" }, card("From what they starred and worked on", h("a", { href: app.href("person", j.login) }, "Their page"), h("ul", { class: "gx-list" }, j.seeds.slice(0, 16).map((r) => repoItem(app, r, { compact: true })))))));
}

// ---------------------------------------------------------------- weekly

async function weeklyPage(app, main, at, alive) {
  fill(main, skel(10));
  const j = await app.api(`/v1/weekly${at ? `?at=${enc(at)}` : ""}`);
  if (!alive()) return;
  const prev = iso(j.at - 7 * 86400), next = iso(j.at + 7 * 86400);
  fill(main,
    h("div", { class: "gx-head" }, h("span", { class: "t" }, "The week of GitHub from ", h("b", {}, dmy(j.from)), " to ", h("b", {}, dmy(j.at))), h("span", { class: "sp" }),
      h("button", { class: "gx-btn", onclick: () => app.go("weekly", prev) }, "Week before"),
      app.status && j.at < app.status.latest ? h("button", { class: "gx-btn", onclick: () => app.go("weekly", next) }, "Week after") : null),
    h("div", { class: "gx-ch3" },
      h("div", {}, sec("What jumped"), h("ul", { class: "gx-list" }, j.rising.map((r) => repoItem(app, r, { extra: h("span", { class: "up" }, `+${fmt(r.window)} ★`), why: whyLine(r.why), compact: true })))),
      h("div", {}, sec("New, and already starred", h("span", { class: "note" }, "first seen in the last 30 days")), h("ul", { class: "gx-list" }, j.new.map((r) => repoItem(app, r, { compact: true })))),
      h("div", {}, sec("Releases of the most starred"), j.releases.length ? h("ul", { class: "gx-list" }, j.releases.map((r) => repoItem(app, r, { extra: h("span", { class: "up" }, `${r.releases} release${r.releases > 1 ? "s" : ""}, ${dm(r.t)}`), compact: true }))) : h("div", { class: "gx-empty" }, "None this week."))));
}

// ----------------------------------------------------------------- about

const TABLE_ABOUT = {
  gh_events: "every public event since 2011: when, what kind, which repo, who; placed on shards by repo and sorted by it",
  gh_stars_daily: "the stars and forks each repo got each day",
  gh_repos: "each repo with two stars or more: its name, description, topics, language, and a vector of what it does",
  gh_repo_names: "the names repos had before",
  gh_actors: "public logins, by account id",
  gh_repo_months: "what happened in each repo each month",
  gh_lang_months: "the same, by language",
  gh_topic_months: "the same, by topic",
};

async function aboutPage(app, main, alive) {
  fill(main, skel(10));
  const j = await app.api("/v1/scale");
  if (!alive()) return;
  const shards = j.cluster?.shards || j.cluster?.members || [];
  fill(main, h("div", { class: "gx-prose" },
    h("h1", { class: "gx-h1" }, "How it works"),
    h("p", {}, "Every public event on GitHub since February 2011, as GH Archive recorded it hour by hour, kept in ChloeDB: a search engine with SQL. Each page above is a handful of SQL statements; the SQL button at the bottom right shows them, with their rows and time."),
    h("h2", {}, "What is kept"),
    h("table", { class: "gx-tbl" }, h("tr", {}, h("th", {}, "table"), h("th", { class: "r" }, "rows"), h("th", {}, "what")),
      Object.entries(j.tables || {}).map(([t, n]) => h("tr", {}, h("td", {}, h("code", {}, t)), h("td", { class: "r" }, fmt(n)), h("td", {}, TABLE_ABOUT[t] || "")))),
    j.span ? h("p", {}, `Events from ${dmy(j.span.first)} to ${dmy(j.span.last)}.`) : null,
    h("h2", {}, "Where it runs"),
    h("p", {}, "Four machines of 8 GB each, every one with a local SSD. The events are split by repo across four shards, so one repo's history is a short range on one machine, and a question about all of GitHub is asked of the four at once by a coordinator that adds up their answers. Each night the shards write a checkpoint to a network volume."),
    shards.length ? h("pre", {}, JSON.stringify(shards, null, 1).slice(0, 1500)) : null,
    h("h2", {}, "How the pages are made"),
    h("p", {}, h("b", {}, "Search"), " ranks repos by the words of their name, description and topics (BM25) and by meaning (a vector of the same, compared with the question's), with their stars."),
    h("p", {}, h("b", {}, "Rising"), " compares each repo's stars in the window with its four weeks before, and says what may explain the jump: a first appearance, a release in the window, one big day, or a story on Hacker News (from the Hacker News app on the same cluster)."),
    h("p", {}, h("b", {}, "Alternatives"), " are the repos nearest in meaning; ", h("b", {}, "also starred"), " are what the people who starred a repo starred too, weighed against how starred each is anyway. ", h("b", {}, "For you"), " walks the same graph from a login's own stars."),
    h("p", {}, h("b", {}, "Vital signs"), " come from a repo's months: work this year against the last, pull requests merged, releases, and how many people do half the work."),
    h("p", {}, h("b", {}, "Ask"), " reads a question with patterns, no model, and runs one or two statements."),
    h("h2", {}, "What is not kept"),
    h("p", {}, "Only public events. No emails, names or other contact details: the old format's actor attributes and commits' authors are never read.")));
}
